//! The mappings of one address space, ordered by address (ADR-0415 item 1,
//! ADR-0051, ADR-0270).
//!
//! A process may hold 4096 mappings. A flat sorted `Vec` would move up to 4096
//! records on every insertion and removal, and copy them all when it grows:
//! work in proportion to what the process holds, done with IRQ masked
//! (INV-0270-1). This list is a sorted list of sorted chunks of at most
//! [`CHUNK`] records: finding a record is two binary searches, an insertion or
//! removal moves at most a chunk, and the list grows by one chunk at a time,
//! so no allocation is larger than a chunk (1816 bytes for the kernel's
//! record). The only walk of every record is [`Mappings::find_gap`], which
//! reports how many it visited so that the caller counts them as work.
//!
//! Room is made before a change, never in it ([`Mappings::reserve_for`]), so
//! that a caller that has already changed other state (page tables) can
//! record the mapping without a failure it cannot undo: [`Mappings::insert`]
//! never allocates. The list keeps no more than it needs: an emptied chunk is
//! given back, and [`Mappings::bytes_held`] says what the heap holds for it,
//! which the kernel charges to the process's job (ADR-0260, ADR-0415 item 1).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::mem::size_of;

/// Records in a chunk, at most. A full chunk splits in two.
pub const CHUNK: usize = 32;

/// One mapping: `[va, va + length)` and what the owner keeps with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<T> {
    pub va: u64,
    pub length: u64,
    pub value: T,
}

/// An allocation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoRoom;

/// The mappings of one space, by address. No two overlap (the caller's duty,
/// checked in debug builds).
pub struct Mappings<T> {
    /// Sorted chunks, each with room for [`CHUNK`] records. No chunk is empty
    /// but the last, which a split for a record added at the top makes until
    /// the record comes.
    chunks: Vec<Vec<Entry<T>>>,
    len: usize,
}

impl<T> Default for Mappings<T> {
    fn default() -> Self {
        Mappings::new()
    }
}

impl<T> Mappings<T> {
    pub const fn new() -> Mappings<T> {
        Mappings {
            chunks: Vec::new(),
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// What the list is charged: every chunk at its full size, whatever it
    /// holds, and the chunk's place in the list of chunks. A chunk is made at
    /// its full size and kept or given back whole, so a mapping made and
    /// unmapped again leaves the charge as it was, and the heap holds no more
    /// than this and the list of chunks' own slack (at most one place for each
    /// chunk the list ever had at once).
    pub fn bytes_held(&self) -> u64 {
        self.chunks.len() as u64 * Self::CHUNK_BYTES
    }

    /// The bytes of one chunk, with its place in the list.
    pub const CHUNK_BYTES: u64 =
        (CHUNK * size_of::<Entry<T>>() + size_of::<Vec<Entry<T>>>()) as u64;

    /// What [`Mappings::reserve_for`] at `va` adds to [`Mappings::bytes_held`]:
    /// one chunk if the list is empty or the chunk is full with both neighbours
    /// (a full chunk gives a record to a neighbour that has room), else none.
    pub fn growth_for(&self, va: u64) -> u64 {
        if self.chunks.is_empty() {
            return Self::CHUNK_BYTES;
        }
        let at = self.route(va);
        let room = |i: usize| self.chunks.get(i).is_some_and(|c| c.len() < CHUNK);
        if room(at) || room(at + 1) || (at > 0 && room(at - 1)) {
            0
        } else {
            Self::CHUNK_BYTES
        }
    }

    /// The chunk that holds `va`, or that `va` would join: the last whose first
    /// record starts at or below it, or the first.
    fn chunk_for(&self, va: u64) -> usize {
        self.chunks
            .partition_point(|c| c.first().is_some_and(|e| e.va <= va))
            .saturating_sub(1)
    }

    /// The chunk a record at `va` goes to: the one that holds its place, or the
    /// next if that one is full and the next has room and the record is above
    /// all of the first's (the top of a list, or after a spill).
    fn route(&self, va: u64) -> usize {
        let at = self.chunk_for(va);
        let full = self.chunks[at].len() >= CHUNK;
        let above = self.chunks[at].last().is_some_and(|e| e.va < va);
        if full && above && at + 1 < self.chunks.len() && self.chunks[at + 1].len() < CHUNK {
            at + 1
        } else {
            at
        }
    }

    /// Makes room for one record at `va`, without recording it: a later
    /// [`Mappings::insert`] at the same address needs no allocation. A full
    /// chunk gives a record to a neighbour that has room; only when both are
    /// full does it split (so that every two neighbours hold at least a
    /// chunk's worth, and the list has under `2 * len / CHUNK + 2` chunks,
    /// which [`Mappings::check`] asserts). Leaves the list valid.
    pub fn reserve_for(&mut self, va: u64) -> Result<(), NoRoom> {
        if self.chunks.is_empty() {
            self.chunks.try_reserve(1).map_err(|_| NoRoom)?;
            self.chunks.push(Self::new_chunk()?);
            return Ok(());
        }
        let at = self.route(va);
        if self.chunks[at].len() < CHUNK {
            return Ok(());
        }
        // Full, and the record goes inside it (above all of it, the next has
        // no room: `route`).
        if at + 1 < self.chunks.len() && self.chunks[at + 1].len() < CHUNK {
            let last = self.chunks[at].pop().expect("a full chunk");
            self.chunks[at + 1].insert(0, last);
            return Ok(());
        }
        if at > 0 && self.chunks[at - 1].len() < CHUNK {
            let first = self.chunks[at].remove(0);
            self.chunks[at - 1].push(first);
            return Ok(());
        }
        // Both neighbours are full: the chunk that follows is made first, so
        // that a failure changes nothing.
        self.chunks.try_reserve(1).map_err(|_| NoRoom)?;
        let mut upper = Self::new_chunk()?;
        if at + 1 == self.chunks.len() && self.chunks[at].last().is_some_and(|e| e.va < va) {
            // Mappings are mostly added at the top: the full chunk stays full
            // and the new record starts the next, which is empty until it is
            // inserted (the only place an empty chunk may be).
        } else {
            // Its upper half moves to the new chunk.
            upper.extend(self.chunks[at].drain(CHUNK / 2..));
        }
        self.chunks.insert(at + 1, upper);
        Ok(())
    }

    /// An empty chunk with room for [`CHUNK`] records, exactly.
    fn new_chunk() -> Result<Vec<Entry<T>>, NoRoom> {
        let mut chunk = Vec::new();
        chunk.try_reserve_exact(CHUNK).map_err(|_| NoRoom)?;
        Ok(chunk)
    }

    /// Gives back the chunk [`Mappings::reserve_for`] made for a record that
    /// did not come (the mapping failed), so that a refused mapping leaves the
    /// list and its charge as they were.
    pub fn trim(&mut self) {
        if self.chunks.last().is_some_and(Vec::is_empty) {
            self.chunks.pop();
        }
    }

    /// Records `[va, va + length)`. Gives the record back if there is no room:
    /// `reserve_for(va)` was not called since the last change of the list.
    pub fn insert(&mut self, va: u64, length: u64, value: T) -> Result<(), Entry<T>> {
        let entry = Entry { va, length, value };
        if self.chunks.is_empty() {
            return Err(entry);
        }
        let at = self.route(va);
        let chunk = &mut self.chunks[at];
        if chunk.len() >= CHUNK {
            return Err(entry);
        }
        let place = chunk.partition_point(|e| e.va < va);
        debug_assert!(
            place == 0 || chunk[place - 1].va + chunk[place - 1].length <= va,
            "a mapping overlaps the one before it"
        );
        debug_assert!(
            place == chunk.len() || va + length <= chunk[place].va,
            "a mapping overlaps the one after it"
        );
        chunk.insert(place, entry);
        self.len += 1;
        Ok(())
    }

    /// Where the record that starts at `va` is: its chunk and place.
    fn find(&self, va: u64) -> Option<(usize, usize)> {
        if self.chunks.is_empty() {
            return None;
        }
        let at = self.chunk_for(va);
        let chunk = &self.chunks[at];
        let place = chunk.partition_point(|e| e.va < va);
        (chunk.get(place)?.va == va).then_some((at, place))
    }

    /// Whether a record is exactly `[va, va + length)`.
    pub fn contains_exact(&self, va: u64, length: u64) -> bool {
        self.find(va)
            .is_some_and(|(c, p)| self.chunks[c][p].length == length)
    }

    /// The record that is exactly `[va, va + length)`.
    pub fn get_exact(&self, va: u64, length: u64) -> Option<&Entry<T>> {
        let (c, p) = self.find(va)?;
        Some(&self.chunks[c][p]).filter(|e| e.length == length)
    }

    /// Removes the record that is exactly `[va, va + length)`.
    pub fn remove_exact(&mut self, va: u64, length: u64) -> Option<Entry<T>> {
        let (c, p) = self.find(va)?;
        if self.chunks[c][p].length != length {
            return None;
        }
        let entry = self.chunks[c].remove(p);
        self.len -= 1;
        self.rebalance(c);
        Some(entry)
    }

    /// Removes the record with the highest address.
    pub fn pop_last(&mut self) -> Option<Entry<T>> {
        let mut last = self.chunks.len().checked_sub(1)?;
        if self.chunks[last].is_empty() {
            // The chunk made for a record that never came.
            self.chunks.pop();
            last = self.chunks.len().checked_sub(1)?;
        }
        let entry = self.chunks[last].pop()?;
        self.len -= 1;
        self.rebalance(last);
        Some(entry)
    }

    /// After a removal from chunk `c`: gives an emptied chunk back, and
    /// merges the chunk with a neighbour when the two together are less than
    /// a chunk (the records move into the room the chunk already has: no
    /// allocation), so that every two neighbours hold at least [`CHUNK`]
    /// records and the list has at most `2 * len / CHUNK + 1` chunks, whatever
    /// was mapped and unmapped (INV-0437-1).
    fn rebalance(&mut self, mut c: usize) {
        loop {
            if self.chunks[c].is_empty() {
                self.chunks.remove(c);
                return;
            }
            if c + 1 < self.chunks.len() && self.chunks[c].len() + self.chunks[c + 1].len() < CHUNK
            {
                self.merge_next(c);
            } else if c > 0 && self.chunks[c - 1].len() + self.chunks[c].len() < CHUNK {
                self.merge_next(c - 1);
                c -= 1;
            } else {
                return;
            }
        }
    }

    /// Moves chunk `c + 1` into chunk `c`, which has room for it, and gives
    /// the emptied chunk back.
    fn merge_next(&mut self, c: usize) {
        let next = self.chunks.remove(c + 1);
        self.chunks[c].extend(next);
    }

    /// How many chunks the list has.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Every record, by address.
    pub fn iter(&self) -> impl Iterator<Item = &Entry<T>> {
        self.chunks.iter().flatten()
    }

    /// The records that start at or above `va`, by address.
    pub fn iter_from(&self, va: u64) -> impl Iterator<Item = &Entry<T>> {
        let first = self.chunk_for(va).min(self.chunks.len());
        self.chunks[first..]
            .iter()
            .flatten()
            .skip_while(move |e| e.va < va)
    }

    /// The lowest address at or above `low` where `length` bytes fit below
    /// `high`, with `guard` unmapped bytes after each record so that running
    /// off one faults; and how many records the search visited, which the
    /// caller counts as work (ADR-0270).
    pub fn find_gap(&self, length: u64, low: u64, high: u64, guard: u64) -> (Option<u64>, usize) {
        let mut at = low;
        let mut visited = 0;
        for e in self.iter() {
            visited += 1;
            match at.checked_add(length) {
                None => return (None, visited),
                Some(end) if end < e.va => return (Some(at), visited),
                Some(_) => {}
            }
            at = e.va + e.length + guard;
        }
        let fits = at.checked_add(length).is_some_and(|end| end <= high);
        (fits.then_some(at), visited)
    }

    /// The invariants, for tests: ordered, no overlap, chunks within their
    /// size and none empty but the last, `len` right.
    #[doc(hidden)]
    pub fn check(&self) {
        let mut count = 0;
        let mut end = 0;
        for (i, chunk) in self.chunks.iter().enumerate() {
            assert!(chunk.len() <= CHUNK, "chunk {i} has {}", chunk.len());
            assert!(
                !chunk.is_empty() || i + 1 == self.chunks.len(),
                "empty chunk {i} before the last"
            );
            assert!(
                i == 0 || chunk.is_empty() || self.chunks[i - 1].len() + chunk.len() >= CHUNK,
                "chunks {} and {i} could be one",
                i - 1
            );
            for e in chunk {
                assert!(e.va >= end, "{:#x} overlaps or is out of order", e.va);
                assert!(e.length > 0);
                end = e.va + e.length;
                count += 1;
            }
        }
        assert_eq!(count, self.len);
        assert!(
            self.chunks.len() <= 2 * self.len / CHUNK + 2,
            "{} chunks for {} records",
            self.chunks.len(),
            self.len
        );
    }
}

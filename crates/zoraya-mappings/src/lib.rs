//! The mappings of one address space, ordered by address (ADR-0415 item 1,
//! ADR-0051, ADR-0270).
//!
//! A process may hold 4096 mappings. A flat sorted `Vec` would move up to 4096
//! records on every insertion and removal, and copy them all when it grows:
//! work in proportion to what the process holds, done with IRQ masked
//! (INV-0270-1). This list is a sorted list of sorted chunks of at most
//! [`CHUNK`] records: finding a record is two binary searches, an insertion or
//! removal moves at most a chunk, and the list grows by one chunk at a time,
//! so no allocation is larger than a chunk (about 4.6 KiB for the kernel's
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
pub const CHUNK: usize = 64;

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
    /// Sorted chunks. No chunk is empty unless it is the only one or the last (which a
    /// split for a record added at the top makes, until the record comes).
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

    /// The bytes the heap holds for the list: every chunk's capacity and the
    /// list of chunks.
    pub fn bytes_held(&self) -> u64 {
        let entries: usize = self.chunks.iter().map(Vec::capacity).sum();
        (entries * size_of::<Entry<T>>() + self.chunks.capacity() * size_of::<Vec<Entry<T>>>())
            as u64
    }

    /// The most [`Mappings::reserve_for`] can add to [`Mappings::bytes_held`]
    /// now: a chunk that doubles or is split off, and the list of chunks
    /// growing by one place or by its doubling.
    pub fn max_growth(&self) -> u64 {
        let outer = self.chunks.capacity().max(4) * size_of::<Vec<Entry<T>>>();
        (CHUNK * size_of::<Entry<T>>() + outer) as u64
    }

    /// The chunk that holds `va`, or that `va` would join: the last whose first
    /// record starts at or below it, or the first.
    fn chunk_for(&self, va: u64) -> usize {
        self.chunks
            .partition_point(|c| c.first().is_some_and(|e| e.va <= va))
            .saturating_sub(1)
    }

    /// Makes room for one record at `va`, without recording it: a later
    /// [`Mappings::insert`] at an address in the same place needs no
    /// allocation. May split a full chunk, which leaves the list valid.
    pub fn reserve_for(&mut self, va: u64) -> Result<(), NoRoom> {
        if self.chunks.is_empty() {
            self.chunks.try_reserve(1).map_err(|_| NoRoom)?;
            self.chunks.push(Vec::new());
        }
        let at = self.chunk_for(va);
        if self.chunks[at].len() < CHUNK {
            // Doubling stops at the chunk's size: it has room below `CHUNK`.
            return self.chunks[at].try_reserve(1).map_err(|_| NoRoom);
        }
        self.chunks.try_reserve(1).map_err(|_| NoRoom)?;
        let mut upper = Vec::new();
        if at + 1 == self.chunks.len() && self.chunks[at].last().is_some_and(|e| e.va < va) {
            // Mappings are mostly added at the top: the full chunk stays full
            // and the new record starts the next, which is empty until it is
            // inserted (the only place an empty chunk may be but alone).
            upper.try_reserve_exact(1).map_err(|_| NoRoom)?;
        } else {
            // Its upper half moves to a chunk of its own, made first, so
            // that a failure changes nothing. The half that stays keeps its
            // capacity: the room for the next.
            upper.try_reserve_exact(CHUNK / 2 + 1).map_err(|_| NoRoom)?;
            upper.extend(self.chunks[at].drain(CHUNK / 2..));
        }
        self.chunks.insert(at + 1, upper);
        Ok(())
    }

    /// Records `[va, va + length)`. Gives the record back if there is no room:
    /// `reserve_for(va)` was not called since the last change of the list.
    pub fn insert(&mut self, va: u64, length: u64, value: T) -> Result<(), Entry<T>> {
        let entry = Entry { va, length, value };
        if self.chunks.is_empty() {
            return Err(entry);
        }
        let mut at = self.chunk_for(va);
        if self.chunks[at].len() >= CHUNK
            && at + 1 < self.chunks.len()
            && self.chunks[at + 1].is_empty()
        {
            at += 1;
        }
        let chunk = &mut self.chunks[at];
        if chunk.len() >= CHUNK || chunk.len() == chunk.capacity() {
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
        self.drop_if_empty(c);
        Some(entry)
    }

    /// Removes the record with the highest address.
    pub fn pop_last(&mut self) -> Option<Entry<T>> {
        let mut last = self.chunks.len().checked_sub(1)?;
        if self.chunks[last].is_empty() && last > 0 {
            self.chunks.pop();
            last -= 1;
        }
        let entry = self.chunks[last].pop()?;
        self.len -= 1;
        self.drop_if_empty(last);
        Some(entry)
    }

    /// Gives an emptied chunk back, unless it is the only one.
    fn drop_if_empty(&mut self, chunk: usize) {
        if self.chunks[chunk].is_empty() && self.chunks.len() > 1 {
            self.chunks.remove(chunk);
        }
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
            for e in chunk {
                assert!(e.va >= end, "{:#x} overlaps or is out of order", e.va);
                assert!(e.length > 0);
                end = e.va + e.length;
                count += 1;
            }
        }
        assert_eq!(count, self.len);
    }
}

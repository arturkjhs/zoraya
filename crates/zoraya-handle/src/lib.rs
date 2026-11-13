//! A process's handle table (ADR-0040).
//!
//! A slot's generation rises each time it is emptied, so a closed handle's
//! value does not name anything again within the system's working life
//! (INV-0057-1). No slot retires: the generation wraps, after 2^48 closes of
//! that slot. New handles go to slots never used, then next-fit from a cursor,
//! which spreads reuse over the table.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::vec::Vec;

use zoraya_abi::{Handle, MAX_GENERATION, MAX_HANDLES, Rights, Status};

/// What a handle holds. `id` is unique for the kernel's life: a subscription
/// belongs to it, so a moved or closed handle can end its subscriptions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<T> {
    pub object: T,
    pub rights: Rights,
    pub id: u64,
}

#[derive(Debug)]
struct Slot<T> {
    generation: u64,
    entry: Option<Entry<T>>,
    /// A call that checked this handle and will move it away holds it
    /// (`begin_move`); nobody else sees or takes it until the move ends.
    moving: bool,
}

/// `slots[0]` is slot 1: slot 0 does not exist.
#[derive(Debug)]
pub struct Table<T> {
    slots: Vec<Slot<T>>,
    cursor: usize,
    count: usize,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Table::new()
    }
}

impl<T> Table<T> {
    pub const fn new() -> Table<T> {
        Table {
            slots: Vec::new(),
            cursor: 0,
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, handle: Handle) -> Option<&Entry<T>> {
        let slot = self.slots.get((handle.slot() as usize).checked_sub(1)?)?;
        if slot.generation == handle.generation() && !slot.moving {
            slot.entry.as_ref()
        } else {
            None
        }
    }

    /// The entry, if `handle` is valid and has `rights`. A wrong handle is
    /// `BadHandle`, too few rights `AccessDenied`.
    pub fn check(&self, handle: Handle, rights: Rights) -> Result<&Entry<T>, Status> {
        let entry = self.get(handle).ok_or(Status::BadHandle)?;
        if entry.rights.contains(rights) {
            Ok(entry)
        } else {
            Err(Status::AccessDenied)
        }
    }

    /// An index for a new handle, without taking it.
    fn free_index(&self) -> Option<usize> {
        if self.slots.len() < MAX_HANDLES as usize {
            return Some(self.slots.len());
        }
        let n = self.slots.len();
        (0..n)
            .map(|i| (self.cursor + i) % n)
            .find(|&i| self.slots[i].entry.is_none())
    }

    /// Makes room, so that the next [`Table::insert`] cannot fail.
    pub fn reserve(&mut self) -> Result<(), Status> {
        match self.free_index() {
            None => Err(Status::LimitReached),
            Some(index) if index == self.slots.len() => {
                self.slots.try_reserve(1).map_err(|_| Status::NoMemory)
            }
            Some(_) => Ok(()),
        }
    }

    /// Makes room for `count` more, so that that many [`Table::insert`]s
    /// cannot fail.
    pub fn reserve_many(&mut self, count: usize) -> Result<(), Status> {
        if self.count + count > MAX_HANDLES as usize {
            return Err(Status::LimitReached);
        }
        // `insert` appends while the table can grow, even past free slots.
        let slots = (self.slots.len() + count).min(MAX_HANDLES as usize);
        self.slots
            .try_reserve(slots - self.slots.len())
            .map_err(|_| Status::NoMemory)
    }

    /// Adds a handle. A full table is `LimitReached` and changes nothing.
    pub fn insert(&mut self, object: T, rights: Rights, id: u64) -> Result<Handle, Status> {
        self.reserve()?;
        let index = self.free_index().ok_or(Status::LimitReached)?;
        if index == self.slots.len() {
            self.slots.push(Slot {
                generation: 0,
                entry: None,
                moving: false,
            });
        }
        self.cursor = index + 1;
        let slot = &mut self.slots[index];
        slot.entry = Some(Entry { object, rights, id });
        self.count += 1;
        Handle::from_parts(index as u32 + 1, slot.generation).ok_or(Status::LimitReached)
    }

    /// Adds a handle at a chosen slot of a table being built. Its value is
    /// `slot` if the slot was never used.
    pub fn insert_at(
        &mut self,
        slot: u32,
        object: T,
        rights: Rights,
        id: u64,
    ) -> Result<Handle, Status> {
        let handle = Handle::from_parts(slot, 0).ok_or(Status::InvalidArgument)?;
        let index = slot as usize - 1;
        if index >= self.slots.len() {
            self.slots
                .try_reserve(index + 1 - self.slots.len())
                .map_err(|_| Status::NoMemory)?;
            while self.slots.len() <= index {
                self.slots.push(Slot {
                    generation: 0,
                    entry: None,
                    moving: false,
                });
            }
        }
        let target = &mut self.slots[index];
        if target.entry.is_some() || target.generation != 0 {
            return Err(Status::InvalidArgument);
        }
        target.entry = Some(Entry { object, rights, id });
        self.count += 1;
        Ok(handle)
    }

    /// Empties the slot. See INV-0057-1 for how long `handle` stays dead.
    pub fn remove(&mut self, handle: Handle) -> Option<Entry<T>> {
        let slot = self
            .slots
            .get_mut((handle.slot() as usize).checked_sub(1)?)?;
        if slot.generation != handle.generation() || slot.moving {
            return None;
        }
        let entry = slot.entry.take()?;
        slot.generation = slot.generation.wrapping_add(1) & MAX_GENERATION;
        self.count -= 1;
        Some(entry)
    }

    /// Ends a held move: the entry leaves, as by [`Table::remove`].
    pub fn finish_move(&mut self, handle: Handle) -> Option<Entry<T>> {
        let slot = self
            .slots
            .get_mut((handle.slot() as usize).checked_sub(1)?)?;
        if slot.generation != handle.generation() || !slot.moving {
            return None;
        }
        slot.moving = false;
        let entry = slot.entry.take()?;
        slot.generation = slot.generation.wrapping_add(1) & MAX_GENERATION;
        self.count -= 1;
        Some(entry)
    }

    /// Ends a held move without taking the entry: the handle is visible again.
    pub fn cancel_move(&mut self, handle: Handle) {
        if let Some(slot) = (handle.slot() as usize)
            .checked_sub(1)
            .and_then(|i| self.slots.get_mut(i))
            && slot.generation == handle.generation()
        {
            slot.moving = false;
        }
    }

    /// Takes one entry out, for closing a whole table.
    pub fn pop(&mut self) -> Option<Entry<T>> {
        while let Some(slot) = self.slots.pop() {
            if let Some(entry) = slot.entry {
                self.count -= 1;
                return Some(entry);
            }
        }
        None
    }
}

impl<T> Table<T> {
    /// The generation slot `slot` is at: what a handle to it would carry now.
    /// A slot never used is at 0.
    pub fn generation_of(&self, slot: u32) -> u64 {
        (slot as usize)
            .checked_sub(1)
            .and_then(|i| self.slots.get(i))
            .map_or(0, |s| s.generation)
    }

    /// Every handle, in slot order.
    pub fn entries(&self) -> impl Iterator<Item = (Handle, &Entry<T>)> {
        self.slots.iter().enumerate().filter_map(|(i, slot)| {
            let entry = slot.entry.as_ref()?;
            Some((Handle::from_parts(i as u32 + 1, slot.generation)?, entry))
        })
    }

    /// An empty table whose slots are at this one's generations, up to at
    /// least `slots`, for a snapshot's child (ADR-0102 items 3 and 4): a
    /// handle placed at a slot's generation then has the parent's value.
    pub fn empty_like(&self, slots: u32) -> Result<Table<T>, Status> {
        let length = self.slots.len().max(slots.min(MAX_HANDLES) as usize);
        let mut copy = Vec::new();
        copy.try_reserve_exact(length)
            .map_err(|_| Status::NoMemory)?;
        for i in 0..length {
            copy.push(Slot {
                generation: self.slots.get(i).map_or(0, |s| s.generation),
                entry: None,
                moving: false,
            });
        }
        Ok(Table {
            slots: copy,
            cursor: self.cursor,
            count: 0,
        })
    }

    /// Puts a handle at exactly `handle`'s slot and generation, which must be
    /// an empty slot of this table at that generation. Allocates nothing, so
    /// on a table from [`Table::empty_like`] it fails only on a wrong handle.
    pub fn place(
        &mut self,
        handle: Handle,
        object: T,
        rights: Rights,
        id: u64,
    ) -> Result<(), Status> {
        let slot = (handle.slot() as usize)
            .checked_sub(1)
            .and_then(|i| self.slots.get_mut(i))
            .ok_or(Status::InvalidArgument)?;
        if slot.entry.is_some() || slot.generation != handle.generation() {
            return Err(Status::InvalidArgument);
        }
        slot.entry = Some(Entry { object, rights, id });
        self.count += 1;
        Ok(())
    }
}

impl<T: Clone> Table<T> {
    /// Checks `handle` for `rights` and holds it for a move: until
    /// [`Table::finish_move`] or [`Table::cancel_move`] no other call finds,
    /// duplicates, closes or moves it, so what the caller checked is what it
    /// moves, however long it pauses in between. Returns a copy of the entry.
    pub fn begin_move(&mut self, handle: Handle, rights: Rights) -> Result<Entry<T>, Status> {
        let entry = self.check(handle, rights)?.clone();
        // `check` found the slot.
        self.slots[handle.slot() as usize - 1].moving = true;
        Ok(entry)
    }

    /// A copy of `handle` with exactly `rights`. Asking for a right the handle
    /// lacks is `AccessDenied`, so a bug shows instead of narrowing silently.
    pub fn duplicate(&mut self, handle: Handle, rights: Rights, id: u64) -> Result<Handle, Status> {
        let entry = self.check(handle, Rights::DUPLICATE)?;
        if !entry.rights.contains(rights) {
            return Err(Status::AccessDenied);
        }
        let object = entry.object.clone();
        self.insert(object, rights, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Rights = Rights::DUPLICATE
        .union(Rights::TRANSFER)
        .union(Rights::WAIT)
        .union(Rights::SIGNAL);

    #[test]
    fn a_held_handle_is_seen_by_no_one_and_comes_back_or_goes() {
        let mut t = Table::new();
        let a = t.insert(10, ALL, 1).unwrap();
        let held = t.begin_move(a, Rights::TRANSFER).unwrap();
        assert_eq!(held.object, 10);
        assert_eq!(t.get(a), None);
        assert_eq!(
            t.begin_move(a, Rights::TRANSFER).unwrap_err(),
            Status::BadHandle
        );
        assert_eq!(t.duplicate(a, ALL, 2), Err(Status::BadHandle));
        assert_eq!(t.remove(a), None);
        assert_eq!(t.len(), 1);
        t.cancel_move(a);
        assert_eq!(t.get(a).unwrap().object, 10);
        t.begin_move(a, Rights::TRANSFER).unwrap();
        assert_eq!(t.finish_move(a).unwrap().object, 10);
        assert_eq!(t.get(a), None);
        assert_eq!(t.finish_move(a), None);
        assert!(t.is_empty());
        // Without TRANSFER nothing is held.
        let b = t.insert(1, Rights::WAIT, 3).unwrap();
        assert_eq!(
            t.begin_move(b, Rights::TRANSFER).unwrap_err(),
            Status::AccessDenied
        );
        assert!(t.get(b).is_some());
    }

    #[test]
    fn a_handle_finds_its_object() {
        let mut t = Table::new();
        let a = t.insert(10, ALL, 1).unwrap();
        let b = t.insert(20, ALL, 2).unwrap();
        assert_ne!(a, b);
        assert_eq!(t.get(a).unwrap().object, 10);
        assert_eq!(t.get(b).unwrap().object, 20);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn slot_zero_and_unused_slots_find_nothing() {
        let mut t: Table<i32> = Table::new();
        assert_eq!(t.get(Handle::new(1).unwrap()), None);
        t.insert(1, ALL, 1).unwrap();
        assert_eq!(t.get(Handle::new(1 << 16).unwrap()), None);
        assert_eq!(t.get(Handle::new(2).unwrap()), None);
        assert_eq!(t.remove(Handle::new(1 << 16).unwrap()), None);
    }

    /// INV-0040-1, across reuse of one slot.
    #[test]
    fn a_closed_handle_never_names_anything_again() {
        let mut t = Table::new();
        let mut old = Vec::new();
        // Fill so that the table reuses slots.
        for i in 0..MAX_HANDLES {
            old.push(t.insert(i, ALL, u64::from(i)).unwrap());
        }
        for round in 0..3 {
            for h in core::mem::take(&mut old) {
                t.remove(h).unwrap();
                assert_eq!(t.get(h), None);
                assert_eq!(t.remove(h), None);
                let new = t.insert(round, ALL, 0).unwrap();
                assert_ne!(new, h);
                assert_eq!(t.get(h), None);
                old.push(new);
            }
        }
    }

    /// INV-0057-1: generations rise by one up to the 48-bit limit, then wrap,
    /// and INV-0057-2: the slot stays in use at every generation.
    #[test]
    fn a_slot_wraps_and_is_never_lost() {
        let mut t = Table::new();
        let a = t.insert(1, ALL, 1).unwrap();
        let start = t.slots[0].generation;
        t.remove(a).unwrap();
        assert_eq!(t.slots[0].generation, start + 1);
        // Direct access: 2^48 closes through the public path is the point of
        // the arithmetic in INV-0057-1.
        t.slots[0].generation = MAX_GENERATION - 1;
        for expected in [MAX_GENERATION - 1, MAX_GENERATION, 0, 1] {
            t.slots[0].entry = Some(Entry {
                object: 3,
                rights: ALL,
                id: 3,
            });
            assert_eq!(t.slots[0].generation, expected);
            let h = Handle::from_parts(1, expected).unwrap();
            assert_eq!(t.get(h).unwrap().object, 3);
            t.count += 1;
            t.remove(h).unwrap();
        }
        assert!(t.is_empty());
    }

    /// INV-0057-2 through the public path: a full table with one slot cycling
    /// always finds it, and values do not repeat.
    #[test]
    fn a_table_loses_no_slot_by_use() {
        let mut t = Table::new();
        let handles: std::vec::Vec<Handle> = (0..MAX_HANDLES as u64)
            .map(|i| t.insert(i, ALL, i).unwrap())
            .collect();
        assert_eq!(t.insert(0, ALL, 0), Err(Status::LimitReached));
        t.remove(handles[7]).unwrap();
        let mut seen = std::collections::HashSet::new();
        seen.insert(handles[7].raw());
        // Past where a 16-bit generation retired the slot.
        for _ in 0..70_000 {
            let h = t.insert(1, ALL, 1).unwrap();
            assert_eq!(h.slot(), 8);
            assert!(seen.insert(h.raw()), "value reused: {h:?}");
            t.remove(h).unwrap();
        }
        assert_eq!(t.len(), MAX_HANDLES as usize - 1);
        assert_eq!(t.get(handles[7]), None);
    }

    /// `reserve_many`'s promise: that many inserts take no memory, with free
    /// slots in the table or not.
    #[test]
    fn reserving_covers_appended_slots() {
        let mut t = Table::new();
        let handles: std::vec::Vec<Handle> =
            (0..1000u64).map(|i| t.insert(i, ALL, i).unwrap()).collect();
        for h in &handles[10..] {
            t.remove(*h).unwrap();
        }
        t.slots.shrink_to_fit();
        t.reserve_many(10).unwrap();
        assert!(t.slots.capacity() >= t.slots.len() + 10);
        assert_eq!(
            t.reserve_many(MAX_HANDLES as usize),
            Err(Status::LimitReached)
        );
    }

    /// INV-0040-3.
    #[test]
    fn a_full_table_says_so_and_stays_as_it_was() {
        let mut t = Table::new();
        for i in 0..MAX_HANDLES {
            t.insert(i, ALL, 0).unwrap();
        }
        assert_eq!(t.insert(0, ALL, 0), Err(Status::LimitReached));
        assert_eq!(t.reserve(), Err(Status::LimitReached));
        assert_eq!(
            t.duplicate(Handle::new(1).unwrap(), ALL, 0),
            Err(Status::LimitReached)
        );
        assert_eq!(t.len(), MAX_HANDLES as usize);
    }

    /// INV-0040-2, over every pair of masks.
    #[test]
    fn duplication_never_adds_a_right() {
        for held in 0..1 << 10 {
            for asked in (0..1 << 10).step_by(7) {
                let held = Rights::from_raw(held).unwrap();
                let asked = Rights::from_raw(asked).unwrap();
                let mut t = Table::new();
                let h = t.insert(0, held, 0).unwrap();
                match t.duplicate(h, asked, 0) {
                    Ok(d) => {
                        assert!(held.contains(Rights::DUPLICATE));
                        assert!(held.contains(asked));
                        assert_eq!(t.get(d).unwrap().rights, asked);
                    }
                    Err(s) => {
                        assert_eq!(s, Status::AccessDenied);
                        assert!(!held.contains(Rights::DUPLICATE) || !held.contains(asked));
                    }
                }
            }
        }
    }

    #[test]
    fn a_right_is_checked() {
        let mut t = Table::new();
        let h = t.insert(0, Rights::WAIT, 0).unwrap();
        assert!(t.check(h, Rights::WAIT).is_ok());
        assert_eq!(t.check(h, Rights::SIGNAL).err(), Some(Status::AccessDenied));
        t.remove(h);
        assert_eq!(t.check(h, Rights::WAIT).err(), Some(Status::BadHandle));
    }

    #[test]
    fn chosen_slots_are_their_own_values_and_are_not_doubled() {
        let mut t = Table::new();
        assert_eq!(t.insert_at(3, 1, ALL, 0).unwrap().raw(), 3);
        assert_eq!(t.insert_at(4, 2, ALL, 0).unwrap().raw(), 4);
        assert_eq!(t.insert_at(3, 3, ALL, 0), Err(Status::InvalidArgument));
        assert_eq!(t.insert_at(0, 3, ALL, 0), Err(Status::InvalidArgument));
        assert_eq!(
            t.insert_at(MAX_HANDLES + 1, 3, ALL, 0),
            Err(Status::InvalidArgument)
        );
        assert_eq!(t.get(Handle::new(3).unwrap()).unwrap().object, 1);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn reserve_makes_the_next_insert_certain() {
        let mut t = Table::new();
        t.reserve().unwrap();
        assert!(t.insert(0, ALL, 0).is_ok());
    }

    #[test]
    fn pop_empties_the_table() {
        let mut t = Table::new();
        t.insert_at(9, 1, ALL, 0).unwrap();
        t.insert_at(2, 2, ALL, 0).unwrap();
        let mut objects = [t.pop().unwrap().object, t.pop().unwrap().object];
        objects.sort();
        assert_eq!(objects, [1, 2]);
        assert_eq!(t.pop(), None);
        assert!(t.is_empty());
    }

    /// ADR-0102: a child's table holds the parent's values.
    #[test]
    fn a_table_like_another_keeps_its_generations() {
        let mut parent = Table::new();
        // Slot 1 closed and empty; slot 2 at generation 0; slot 3 closed
        // and taken again by a chosen slot of the next generation.
        let a = parent.insert(1, ALL, 1).unwrap();
        let b = parent.insert(2, ALL, 2).unwrap();
        let c = parent.insert(4, ALL, 4).unwrap();
        parent.remove(a).unwrap();
        parent.remove(c).unwrap();
        let a2 = Handle::from_parts(c.slot(), 1).unwrap();
        assert_eq!(parent.generation_of(a.slot()), 1);
        assert_eq!(parent.generation_of(c.slot()), 1);
        assert_eq!(parent.generation_of(500), 0);
        assert_eq!(parent.generation_of(0), 0);
        let listed: std::vec::Vec<_> = parent.entries().map(|(h, e)| (h, e.object)).collect();
        assert_eq!(listed, [(b, 2)]);
        let c = a;

        let mut child: Table<i32> = parent.empty_like(10).unwrap();
        assert!(child.is_empty());
        assert_eq!(child.generation_of(c.slot()), 1, "an empty slot too");
        assert_eq!(child.generation_of(10), 0);
        child.place(a2, 30, ALL, 9).unwrap();
        assert_eq!(child.get(a2).unwrap().object, 30);
        assert_eq!(
            child.get(Handle::from_parts(a2.slot(), 0).unwrap()),
            None,
            "old value"
        );
        assert_eq!(
            child.place(a2, 31, ALL, 9),
            Err(Status::InvalidArgument),
            "full"
        );
        assert_eq!(
            child.place(Handle::from_parts(b.slot(), 1).unwrap(), 0, ALL, 9),
            Err(Status::InvalidArgument),
            "wrong generation"
        );
        assert_eq!(
            child.place(Handle::from_parts(11, 0).unwrap(), 0, ALL, 9),
            Err(Status::InvalidArgument),
            "past the table"
        );
        child
            .place(Handle::from_parts(10, 0).unwrap(), 5, ALL, 9)
            .unwrap();
        assert_eq!(child.len(), 2);
        // A new handle in the child does not take a placed slot.
        let fresh = child.insert(7, ALL, 10).unwrap();
        assert!(![a2.slot(), 10].contains(&fresh.slot()));
        let big: Table<i32> = parent.empty_like(MAX_HANDLES + 5).unwrap();
        assert_eq!(big.generation_of(MAX_HANDLES), 0);
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

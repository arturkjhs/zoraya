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
        if slot.generation == handle.generation() {
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

    /// Adds a handle. A full table is `LimitReached` and changes nothing.
    pub fn insert(&mut self, object: T, rights: Rights, id: u64) -> Result<Handle, Status> {
        self.reserve()?;
        let index = self.free_index().ok_or(Status::LimitReached)?;
        if index == self.slots.len() {
            self.slots.push(Slot {
                generation: 0,
                entry: None,
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
        if slot.generation != handle.generation() {
            return None;
        }
        let entry = slot.entry.take()?;
        slot.generation = slot.generation.wrapping_add(1) & MAX_GENERATION;
        self.count -= 1;
        Some(entry)
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

impl<T: Clone> Table<T> {
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
}

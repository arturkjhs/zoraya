//! The table of private anonymous mappings (ADR-0103 item 12, ADR-0415 item
//! 1): the address and size of each mapping `mmap` and a thread's stack made,
//! so that `munmap` ends only what `mmap` made and never the heap's steps,
//! the table itself or a mapping of another kind.
//!
//! One slot per mapping the kernel allows a process ([`MAPPING_SLOTS`], from
//! `zoraya-abi`, which is also the kernel's limit: INV-0415-1). A slot holds
//! no handle: the memory object's handle is closed as soon as the object is
//! mapped, so that a mapping costs one slot of the kernel's and none of the
//! handle table's 1024.
//!
//! The slots are somewhere the caller found (`libposix` maps them on the first
//! private `mmap`, so a program that never maps pays nothing for them).

#![forbid(unsafe_code)]

use zoraya_abi::MAPPING_SLOTS;

/// Bytes of the slots of a full table, a whole number of pages.
pub const TABLE_BYTES: usize = MAPPING_SLOTS * core::mem::size_of::<Slot>();

// The table is whole pages, so that it is one memory object of its own.
const _: () = assert!(TABLE_BYTES.is_multiple_of(4096));

/// A mapping's address and size in bytes. A size of 0 is a free slot.
pub type Slot = (u64, u64);

/// The table over `MAPPING_SLOTS` slots.
pub struct MapTable<'a> {
    slots: &'a mut [Slot],
    /// No slot below this one is free: the search for one starts here.
    first_free: usize,
}

impl<'a> MapTable<'a> {
    /// A table over `slots`, all free. `slots.len()` is the capacity.
    pub fn new(slots: &'a mut [Slot]) -> MapTable<'a> {
        slots.fill((0, 0));
        MapTable {
            slots,
            first_free: 0,
        }
    }

    /// The number of slots.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// A free slot, or `None` if the table is full. It stays free until
    /// [`MapTable::set`] takes it.
    pub fn free_slot(&mut self) -> Option<usize> {
        let at = self.first_free
            + self.slots[self.first_free..]
                .iter()
                .position(|s| s.1 == 0)?;
        self.first_free = at;
        Some(at)
    }

    /// Records the mapping `[address, address + size)` in `slot`, which
    /// [`MapTable::free_slot`] gave. A size of 0 is refused (it is a free
    /// slot) and so is a slot that is taken.
    pub fn set(&mut self, slot: usize, address: u64, size: u64) -> bool {
        match self.slots.get_mut(slot) {
            Some(s) if s.1 == 0 && size != 0 => {
                *s = (address, size);
                true
            }
            _ => false,
        }
    }

    /// Forgets the mapping that is exactly `[address, address + size)`.
    /// False if there is none (the kernel ends whole mappings only, and a
    /// mapping of another kind is not this table's to end).
    pub fn remove(&mut self, address: u64, size: u64) -> bool {
        if size == 0 {
            return false;
        }
        match self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| **s == (address, size))
        {
            Some((i, s)) => {
                *s = (0, 0);
                self.first_free = self.first_free.min(i);
                true
            }
            None => false,
        }
    }

    /// Whether a mapping is exactly `[address, address + size)`.
    pub fn contains(&self, address: u64, size: u64) -> bool {
        size != 0 && self.slots.contains(&(address, size))
    }

    /// How many mappings the table holds.
    pub fn used(&self) -> usize {
        self.slots.iter().filter(|s| s.1 != 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;

    #[test]
    fn the_table_has_the_kernels_number_of_slots() {
        // INV-0415-1: one constant, in `zoraya-abi`; 4096 mappings.
        assert_eq!(MAPPING_SLOTS, 4096);
        assert_eq!(TABLE_BYTES, 4096 * 16);
        let mut slots = vec![(0, 0); MAPPING_SLOTS];
        assert_eq!(MapTable::new(&mut slots).capacity(), MAPPING_SLOTS);
    }

    #[test]
    fn a_full_table_refuses_the_next_and_gives_a_slot_back() {
        let mut slots = vec![(7, 7); 8];
        let mut table = MapTable::new(&mut slots);
        for n in 0..8 {
            let slot = table.free_slot().expect("a free slot");
            assert_eq!(slot, n);
            assert!(table.set(slot, 0x1000 * (n as u64 + 1), 4096));
        }
        assert_eq!(table.used(), 8);
        assert_eq!(table.free_slot(), None, "full");
        assert!(table.remove(0x4000, 4096));
        assert_eq!(table.free_slot(), Some(3), "the slot just freed");
        assert_eq!(table.used(), 7);
    }

    #[test]
    fn only_a_whole_mapping_is_removed() {
        let mut slots = vec![(0, 0); 4];
        let mut table = MapTable::new(&mut slots);
        let slot = table.free_slot().unwrap();
        assert!(table.set(slot, 0x10000, 8192));
        assert!(!table.set(slot, 0x20000, 4096), "a taken slot");
        assert!(!table.remove(0x10000, 4096), "a part");
        assert!(!table.remove(0x11000, 4096), "a part from the middle");
        assert!(!table.remove(0x10000, 0), "no size");
        assert!(!table.remove(0x30000, 8192), "another address");
        assert!(table.contains(0x10000, 8192));
        assert!(table.remove(0x10000, 8192));
        assert!(!table.contains(0x10000, 8192));
        assert!(!table.remove(0x10000, 8192), "once");
    }

    #[test]
    fn a_size_of_zero_is_never_recorded() {
        let mut slots = vec![(0, 0); 2];
        let mut table = MapTable::new(&mut slots);
        assert!(!table.set(0, 0x1000, 0));
        assert!(!table.set(2, 0x1000, 4096), "past the end");
        assert_eq!(table.used(), 0);
    }

    #[test]
    fn the_search_finds_the_lowest_free_slot_after_any_order_of_frees() {
        let mut slots = vec![(0, 0); 64];
        let mut table = MapTable::new(&mut slots);
        for n in 0..64u64 {
            let slot = table.free_slot().unwrap();
            assert!(table.set(slot, 0x1000 * (n + 1), 4096));
        }
        // Free 40, then 10, then 50: the lowest free is always found.
        assert!(table.remove(0x1000 * 41, 4096));
        assert_eq!(table.free_slot(), Some(40));
        assert!(table.remove(0x1000 * 11, 4096));
        assert_eq!(table.free_slot(), Some(10));
        assert!(table.remove(0x1000 * 51, 4096));
        assert_eq!(table.free_slot(), Some(10));
        assert!(table.set(10, 0x99000, 4096));
        assert_eq!(table.free_slot(), Some(40));
    }
}

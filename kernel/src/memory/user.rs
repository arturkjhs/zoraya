//! User address spaces (ADR-0031).

use alloc::sync::Arc;
use alloc::vec::Vec;

use zoraya_pagetable::{Attributes, Half, MapError, PageTable, TableMemory};
use zoraya_pmm::{FRAME_SIZE, Frame};

use super::frames;
use super::heap::OutOfMemory;
use super::object::MemoryObject;
use super::phys_to_virt;
use crate::arch::mmu;
use crate::device::DeviceMapping;
use crate::sync::SpinLock;

/// Lowest address a user mapping may have.
pub const USER_LOW: u64 = 0x40_0000;
/// One past the highest user address.
pub const USER_HIGH: u64 = 1 << 47;

/// Mappings a process may hold (ADR-0051).
pub const MAX_MAPPINGS: usize = 256;
/// Where mappings of objects go: the kernel chooses inside this range.
pub const MAP_LOW: u64 = 1 << 46;
pub const MAP_HIGH: u64 = 3 << 45;

/// ASIDs in use; ASID 0 belongs to no process.
static ASIDS: SpinLock<[u64; 4]> = SpinLock::new([1, 0, 0, 0]);

fn alloc_asid() -> Option<u16> {
    let mut asids = ASIDS.lock();
    for (i, word) in asids.iter_mut().enumerate() {
        if *word != u64::MAX {
            let bit = word.trailing_ones();
            *word |= 1 << bit;
            return Some(i as u16 * 64 + bit as u16);
        }
    }
    None
}

fn free_asid(asid: u16) {
    ASIDS.lock()[usize::from(asid / 64)] &= !(1 << (asid % 64));
}

/// Table memory from frames, which the address space owns along with every
/// frame it maps.
struct UserMemory {
    asid: u16,
    frames: Vec<Frame>,
}

impl UserMemory {
    /// Keeps `frame`, or frees it and fails.
    fn adopt(&mut self, frame: Frame) -> Result<(), OutOfMemory> {
        if self.frames.try_reserve(1).is_err() {
            frames::free(frame);
            return Err(OutOfMemory);
        }
        self.frames.push(frame);
        Ok(())
    }
}

impl TableMemory for UserMemory {
    fn alloc_table(&mut self) -> Option<u64> {
        let frame = frames::alloc_zeroed().ok()?;
        self.adopt(frame).ok()?;
        Some(frame.addr())
    }

    fn read(&self, table: u64, index: usize) -> u64 {
        // SAFETY: `table` is a frame this space allocated for a table, which
        // the direct map holds read-write, and `index` is below 512.
        unsafe { ((phys_to_virt(table) as *const u64).add(index)).read_volatile() }
    }

    fn write(&mut self, table: u64, index: usize, value: u64) {
        // SAFETY: as in `read`; this space is only changed while it is being
        // built, by one thread.
        unsafe { ((phys_to_virt(table) as *mut u64).add(index)).write_volatile(value) }
    }

    fn invalidate(&mut self, _va: u64, _size: u64) {
        mmu::invalidate_asid(self.asid);
    }

    fn sync(&mut self) {
        mmu::sync_tables();
    }
}

/// What a mapping keeps alive.
pub enum Holder {
    // Read by nothing: holding them is the point.
    #[allow(dead_code)]
    Memory(Arc<MemoryObject>),
    #[allow(dead_code)]
    Device(DeviceMapping),
}

struct Mapping {
    va: u64,
    length: u64,
    holder: Holder,
}

/// Why a mapping was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingError {
    /// The process has `MAX_MAPPINGS`.
    Limit,
    Memory,
    /// No mapping is exactly that range.
    NotMapped,
}

/// One process's memory. Dropping it frees every frame and its ASID.
pub struct AddressSpace {
    table: PageTable,
    memory: UserMemory,
    /// By address.
    mappings: Vec<Mapping>,
}

impl AddressSpace {
    pub fn new() -> Result<AddressSpace, OutOfMemory> {
        let asid = alloc_asid().ok_or(OutOfMemory)?;
        let mut memory = UserMemory {
            asid,
            frames: Vec::new(),
        };
        match PageTable::new(&mut memory, Half::Lower) {
            Ok(table) => Ok(AddressSpace {
                table,
                memory,
                mappings: Vec::new(),
            }),
            Err(_) => {
                drop(memory.frames);
                free_asid(asid);
                Err(OutOfMemory)
            }
        }
    }

    /// The value for `TTBR0_EL1`.
    pub fn ttbr0(&self) -> u64 {
        self.table.root() | (u64::from(self.memory.asid) << 48)
    }

    /// Maps a new zeroed page at `va` and returns the address the kernel can
    /// fill it through.
    pub fn map_new_page(&mut self, va: u64, attributes: Attributes) -> Result<*mut u8, MapFailed> {
        let frame = frames::alloc_zeroed().map_err(|_| MapFailed::Memory)?;
        self.memory.adopt(frame).map_err(|_| MapFailed::Memory)?;
        self.table
            .map_pages(&mut self.memory, va, frame.addr(), FRAME_SIZE, attributes)
            .map_err(MapFailed::Table)?;
        Ok(phys_to_virt(frame.addr()) as *mut u8)
    }

    /// The lowest place for `length` bytes, with an unmapped page after what
    /// comes before it, so that running off a mapping faults.
    fn find_gap(&self, length: u64) -> Option<(usize, u64)> {
        let mut at = MAP_LOW;
        for (i, m) in self.mappings.iter().enumerate() {
            if at.checked_add(length)? < m.va {
                return Some((i, at));
            }
            at = m.va + m.length + FRAME_SIZE;
        }
        (at.checked_add(length)? <= MAP_HIGH).then_some((self.mappings.len(), at))
    }

    /// Maps pages given by `page`, a function of their offset, and keeps
    /// `holder` until they are unmapped.
    pub fn map_object(
        &mut self,
        length: u64,
        attributes: Attributes,
        holder: Holder,
        page: impl Fn(u64) -> u64,
    ) -> Result<u64, MappingError> {
        if self.mappings.len() >= MAX_MAPPINGS {
            return Err(MappingError::Limit);
        }
        self.mappings
            .try_reserve(1)
            .map_err(|_| MappingError::Memory)?;
        let (index, va) = self.find_gap(length).ok_or(MappingError::Memory)?;
        for offset in (0..length).step_by(FRAME_SIZE as usize) {
            let mapped = self.table.map_pages(
                &mut self.memory,
                va + offset,
                page(offset),
                FRAME_SIZE,
                attributes,
            );
            if mapped.is_err() {
                if offset > 0 {
                    let _ = self.table.unmap(&mut self.memory, va, offset);
                }
                return Err(MappingError::Memory);
            }
        }
        self.mappings.insert(index, Mapping { va, length, holder });
        Ok(va)
    }

    /// Ends the mapping that is exactly `[va, va + length)`, and gives back
    /// what it held only after no TLB can reach it: break, invalidate, free.
    pub fn unmap_object(&mut self, va: u64, length: u64) -> Result<Holder, MappingError> {
        let index = self
            .mappings
            .iter()
            .position(|m| m.va == va && m.length == length)
            .ok_or(MappingError::NotMapped)?;
        self.table
            .unmap(&mut self.memory, va, length)
            .map_err(|_| MappingError::NotMapped)?;
        Ok(self.mappings.remove(index).holder)
    }

    /// The 64-bit value at `va`, which must be mapped and aligned.
    #[cfg(feature = "qemu-test")]
    pub fn peek(&self, va: u64) -> Option<u64> {
        let mut found = None;
        self.for_each_mapping(&mut |m| {
            if (m.va..m.va + m.size).contains(&va) {
                found = Some(m.pa + (va - m.va));
            }
        });
        // SAFETY: a frame of this space, which the direct map holds.
        found.map(|pa| unsafe { (phys_to_virt(pa) as *const u64).read_volatile() })
    }

    /// Calls `f` for every mapping.
    #[cfg(feature = "qemu-test")]
    pub fn for_each_mapping(&self, f: &mut impl FnMut(zoraya_pagetable::Mapping)) {
        self.table.for_each_mapping(&self.memory, f);
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        mmu::invalidate_asid(self.memory.asid);
        // Only now can nothing reach what the mappings held.
        self.mappings.clear();
        for frame in self.memory.frames.drain(..) {
            frames::free(frame);
        }
        free_asid(self.memory.asid);
    }
}

/// Why a page could not be mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapFailed {
    Memory,
    Table(MapError),
}

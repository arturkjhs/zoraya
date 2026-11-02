//! User address spaces (ADR-0031).

use alloc::sync::Arc;
use alloc::vec::Vec;

#[cfg(feature = "qemu-test")]
use zoraya_pagetable::MemoryType;
use zoraya_pagetable::{Attributes, Half, MapError, PageTable, TableMemory};
use zoraya_pmm::{FRAME_SIZE, Frame};

use super::frames;
use super::heap::OutOfMemory;
use super::object::MemoryObject;
use super::phys_to_virt;
use crate::arch::mmu;
use crate::device::DeviceMapping;
use crate::sync::SpinLock;
use zoraya_abi::Status;

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
        // SAFETY: as in `read`; the owner holds the process's `space` lock while
        // it changes this table, or it is still being built by one thread.
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
    attributes: Attributes,
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
        self.mappings.insert(
            index,
            Mapping {
                va,
                length,
                attributes,
                holder,
            },
        );
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

    /// True if any mapping is of a device window (ADR-0054).
    pub fn has_device_mapping(&self) -> bool {
        self.mappings
            .iter()
            .any(|m| matches!(m.holder, Holder::Device(_)))
    }

    /// A copy of this space for a snapshot's child (ADR-0102 item 2): a fresh
    /// frame for every page this space owns, with the same contents at the
    /// same address with the same attributes, and a fresh memory object for
    /// every mapping of one, holding a copy of the mapped range. Nothing is
    /// shared. A device mapping refuses it with `BadState`. On failure,
    /// everything it took is given back as the copy drops (INV-0102-3).
    pub fn snapshot(&self, copied: &mut Copied) -> Result<AddressSpace, Status> {
        if self.has_device_mapping() {
            return Err(Status::BadState);
        }
        let mut child = AddressSpace::new().map_err(|_| Status::NoMemory)?;
        child
            .mappings
            .try_reserve_exact(self.mappings.len())
            .map_err(|_| Status::NoMemory)?;
        let mut objects = Vec::new();
        objects
            .try_reserve_exact(self.mappings.len())
            .map_err(|_| Status::NoMemory)?;
        for m in &self.mappings {
            // Every page is written by the copy below before the child exists.
            objects.push(MemoryObject::new_for_copy(m.length)?);
        }
        let mut failed = None;
        let mut executable = false;
        self.table.for_each_mapping(&self.memory, &mut |leaf| {
            for offset in (0..leaf.size).step_by(FRAME_SIZE as usize) {
                if failed.is_some() {
                    return;
                }
                let (va, pa) = (leaf.va + offset, leaf.pa + offset);
                // Mappings are kept by address, and do not overlap.
                let i = self.mappings.partition_point(|m| m.va + m.length <= va);
                let target = match self.mappings.get(i) {
                    Some(m) if m.va <= va => objects[i].page(va - m.va),
                    _ => match child.copy_page(va, leaf.attributes) {
                        Ok(pa) => pa,
                        Err(e) => {
                            failed = Some(e);
                            return;
                        }
                    },
                };
                // SAFETY: both are whole frames of ordinary RAM in the direct
                // map: `pa` is mapped by this space, which the caller's lock
                // keeps, and `target` was just taken for the copy and is
                // reached by nothing else yet.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        phys_to_virt(pa) as *const u8,
                        phys_to_virt(target) as *mut u8,
                        FRAME_SIZE as usize,
                    );
                }
                if leaf.attributes.execute {
                    // SAFETY: the frame just written, in the direct map.
                    unsafe { mmu::clean_to_unification(phys_to_virt(target), FRAME_SIZE) };
                    executable = true;
                }
                copied.pages += 1;
            }
        });
        if let Some(e) = failed {
            return Err(e);
        }
        if executable {
            // After every clean: fetches through the child's mappings then
            // see the copies (Arm ARM D7.5.9).
            mmu::invalidate_instruction_cache();
        }
        for (m, object) in self.mappings.iter().zip(objects) {
            for offset in (0..m.length).step_by(FRAME_SIZE as usize) {
                child
                    .table
                    .map_pages(
                        &mut child.memory,
                        m.va + offset,
                        object.page(offset),
                        FRAME_SIZE,
                        m.attributes,
                    )
                    .map_err(|_| Status::NoMemory)?;
            }
            child.mappings.push(Mapping {
                va: m.va,
                length: m.length,
                attributes: m.attributes,
                holder: Holder::Memory(object),
            });
        }
        Ok(child)
    }

    /// Maps a new frame at `va` for [`AddressSpace::snapshot`] to fill: not
    /// zeroed, since the whole page is written before anything can read it.
    fn copy_page(&mut self, va: u64, attributes: Attributes) -> Result<u64, Status> {
        let frame = frames::alloc().map_err(|_| Status::NoMemory)?;
        self.memory.adopt(frame).map_err(|_| Status::NoMemory)?;
        self.table
            .map_pages(&mut self.memory, va, frame.addr(), FRAME_SIZE, attributes)
            .map_err(|_| Status::NoMemory)?;
        Ok(frame.addr())
    }

    /// The 64-bit value at `va`, which must be mapped and aligned.
    #[cfg(feature = "qemu-test")]
    pub fn peek(&self, va: u64) -> Option<u64> {
        let mut found = None;
        self.for_each_mapping(&mut |m| {
            let normal = m.attributes.memory == MemoryType::Normal;
            if normal && (m.va..m.va + m.size).contains(&va) {
                found = Some(m.pa + (va - m.va));
            }
        });
        // SAFETY: RAM of this space or of an object it maps, which the direct
        // map holds: a device window has no direct-map entry, and is left out.
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

/// What a snapshot copied, for its measurement.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Copied {
    /// Pages copied: the process's own and those of mapped objects.
    pub pages: u64,
}

/// Why a page could not be mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapFailed {
    Memory,
    Table(MapError),
}

//! Building and editing aarch64 translation tables (ADR-0020).
//!
//! VMSAv8-64 stage 1, 4 KiB granule, 48-bit VA, so four levels: level 0
//! tables only, level 1 and 2 tables or blocks (1 GiB, 2 MiB), level 3 pages.
//! Section titles below are those of the Arm Architecture Reference Manual
//! for A-profile (ARM DDI 0487).
//!
//! The crate reaches table memory only through [`TableMemory`], so it runs on
//! the host against a buffer and needs no `unsafe`. Attributes that would make
//! a mapping writable and executable cannot be mapped ([`MapError`]).

#![no_std]
#![forbid(unsafe_code)]

/// Size of a page and of a table.
pub const PAGE_SIZE: u64 = 4096;
/// Entries per table.
pub const ENTRIES: usize = 512;
/// Bits of virtual address translated.
pub const VA_BITS: u32 = 48;

/// `MAIR_ELx` attribute index of Device-nGnRnE memory.
pub const MAIR_INDEX_DEVICE: u64 = 0;
/// `MAIR_ELx` attribute index of Normal write-back memory.
pub const MAIR_INDEX_NORMAL: u64 = 1;
/// `MAIR_EL1` for the two indices: `0x00` is Device-nGnRnE, `0xff` is Normal,
/// inner and outer write-back non-transient, read and write allocate.
pub const MAIR_VALUE: u64 = 0xff << (8 * MAIR_INDEX_NORMAL);

// Descriptor bits ("VMSAv8-64 descriptor formats").
const VALID: u64 = 1 << 0;
/// Bit 1 is 1 in a table descriptor (levels 0 to 2) and a page descriptor
/// (level 3), 0 in a block descriptor.
const TABLE_OR_PAGE: u64 = 1 << 1;
const ATTR_INDEX_SHIFT: u32 = 2;
const ATTR_INDEX_MASK: u64 = 0b111 << ATTR_INDEX_SHIFT;
/// AP[1]: EL0 may access.
const AP_USER: u64 = 1 << 6;
/// AP[2]: read-only.
const AP_READ_ONLY: u64 = 1 << 7;
const SH_INNER: u64 = 0b11 << 8;
/// Access flag: set up front, since hardware update is not enabled.
const AF: u64 = 1 << 10;
const NOT_GLOBAL: u64 = 1 << 11;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
const ADDRESS_MASK: u64 = 0x0000_ffff_ffff_f000;

/// Memory type of a mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    /// Normal, write-back, inner shareable.
    Normal,
    /// Device-nGnRnE.
    Device,
}

/// What a mapping allows. Reading is always allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attributes {
    pub memory: MemoryType,
    pub write: bool,
    pub execute: bool,
    /// Accessible from EL0. Such a mapping is never executable at EL1.
    pub user: bool,
}

impl Attributes {
    pub const KERNEL_TEXT: Attributes = Attributes {
        memory: MemoryType::Normal,
        write: false,
        execute: true,
        user: false,
    };
    pub const KERNEL_RODATA: Attributes = Attributes {
        memory: MemoryType::Normal,
        write: false,
        execute: false,
        user: false,
    };
    pub const KERNEL_DATA: Attributes = Attributes {
        memory: MemoryType::Normal,
        write: true,
        execute: false,
        user: false,
    };
    pub const DEVICE: Attributes = Attributes {
        memory: MemoryType::Device,
        write: true,
        execute: false,
        user: false,
    };

    /// Writable and executable, or executable device memory.
    pub const fn is_forbidden(&self) -> bool {
        self.execute && (self.write || matches!(self.memory, MemoryType::Device))
    }

    /// The upper and lower attribute bits of a block or page descriptor
    /// ("Stage 1 memory access permissions" and "Memory attribute fields").
    const fn encode(&self) -> u64 {
        let mut d = AF;
        d |= match self.memory {
            MemoryType::Normal => (MAIR_INDEX_NORMAL << ATTR_INDEX_SHIFT) | SH_INNER,
            MemoryType::Device => MAIR_INDEX_DEVICE << ATTR_INDEX_SHIFT,
        };
        if !self.write {
            d |= AP_READ_ONLY;
        }
        if self.user {
            d |= AP_USER | NOT_GLOBAL | PXN;
            if !self.execute {
                d |= UXN;
            }
        } else {
            d |= UXN;
            if !self.execute {
                d |= PXN;
            }
        }
        d
    }

    const fn decode(d: u64) -> Attributes {
        let user = d & AP_USER != 0;
        Attributes {
            memory: if (d & ATTR_INDEX_MASK) >> ATTR_INDEX_SHIFT == MAIR_INDEX_DEVICE {
                MemoryType::Device
            } else {
                MemoryType::Normal
            },
            write: d & AP_READ_ONLY == 0,
            execute: if user { d & UXN == 0 } else { d & PXN == 0 },
            user,
        }
    }
}

/// Table memory, addressed by physical address.
pub trait TableMemory {
    /// A new zeroed table, or `None` if there is no memory.
    fn alloc_table(&mut self) -> Option<u64>;
    /// Entry `index` of the table at `table`.
    fn read(&self, table: u64, index: usize) -> u64;
    /// Sets entry `index` of the table at `table`.
    fn write(&mut self, table: u64, index: usize, value: u64);
    /// Makes all earlier writes visible to the table walker, then removes
    /// every TLB entry for `[va, va + size)` and waits for that to finish.
    fn invalidate(&mut self, va: u64, size: u64);
    /// Makes all earlier writes visible to the table walker.
    fn sync(&mut self);
}

/// Why a change was refused. The mappings are as they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// The attributes are writable and executable, or executable device
    /// memory.
    Forbidden,
    /// An address or size is not page-aligned, or the size is 0.
    Unaligned,
    /// A virtual address is outside this half of the address space, or a
    /// physical one outside 48 bits.
    OutOfRange,
    /// Part of the range is already mapped.
    AlreadyMapped,
    /// Part of the range is not mapped.
    NotMapped,
    /// [`TableMemory::alloc_table`] failed.
    OutOfTables,
}

/// Which half of the address space a table serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    /// `TTBR0_EL1`: addresses with bits 63:48 clear.
    Lower,
    /// `TTBR1_EL1`: addresses with bits 63:48 set.
    Upper,
}

impl Half {
    const fn contains(self, va: u64) -> bool {
        match self {
            Half::Lower => va >> VA_BITS == 0,
            Half::Upper => va >> VA_BITS == 0xffff,
        }
    }
}

/// One block or page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub va: u64,
    pub pa: u64,
    /// 4 KiB, 2 MiB or 1 GiB.
    pub size: u64,
    pub attributes: Attributes,
}

/// Bytes covered by one entry at `level` (0 to 3).
pub const fn level_size(level: usize) -> u64 {
    1 << (39 - 9 * level as u32)
}

const fn entry_index(va: u64, level: usize) -> usize {
    ((va >> (39 - 9 * level as u32)) & (ENTRIES as u64 - 1)) as usize
}

const fn table_descriptor(table: u64) -> u64 {
    table | TABLE_OR_PAGE | VALID
}

const fn leaf_descriptor(level: usize, pa: u64, attributes: Attributes) -> u64 {
    let kind = if level == 3 { TABLE_OR_PAGE } else { 0 };
    pa | attributes.encode() | kind | VALID
}

const fn is_valid(d: u64) -> bool {
    d & VALID != 0
}

const fn is_table(d: u64, level: usize) -> bool {
    is_valid(d) && level < 3 && d & TABLE_OR_PAGE != 0
}

enum Slot {
    /// Nothing mapped in the aligned `size` bytes around the address.
    Hole { size: u64 },
    Leaf {
        table: u64,
        index: usize,
        level: usize,
        descriptor: u64,
    },
}

/// One translation table tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTable {
    root: u64,
    half: Half,
}

impl PageTable {
    /// A tree with an empty root table.
    pub fn new<M: TableMemory>(mem: &mut M, half: Half) -> Result<PageTable, MapError> {
        let root = mem.alloc_table().ok_or(MapError::OutOfTables)?;
        Ok(PageTable { root, half })
    }

    /// Physical address of the root, the value for `TTBRn_EL1`.
    pub const fn root(&self) -> u64 {
        self.root
    }

    fn check(&self, va: u64, size: u64) -> Result<(), MapError> {
        if !va.is_multiple_of(PAGE_SIZE) || !size.is_multiple_of(PAGE_SIZE) || size == 0 {
            return Err(MapError::Unaligned);
        }
        let last = va.checked_add(size - 1).ok_or(MapError::OutOfRange)?;
        if self.half.contains(va) && self.half.contains(last) {
            Ok(())
        } else {
            Err(MapError::OutOfRange)
        }
    }

    fn lookup<M: TableMemory>(&self, mem: &M, va: u64) -> Slot {
        let mut table = self.root;
        for level in 0..4 {
            let index = entry_index(va, level);
            let descriptor = mem.read(table, index);
            if !is_valid(descriptor) {
                return Slot::Hole {
                    size: level_size(level),
                };
            }
            if !is_table(descriptor, level) {
                return Slot::Leaf {
                    table,
                    index,
                    level,
                    descriptor,
                };
            }
            table = descriptor & ADDRESS_MASK;
        }
        // Level 3 has no table descriptors, so the loop has returned.
        Slot::Hole { size: PAGE_SIZE }
    }

    /// Whether anything is mapped in `[va, va + size)`, and whether everything
    /// is, as (any, all).
    fn coverage<M: TableMemory>(&self, mem: &M, va: u64, size: u64) -> (bool, bool) {
        let (mut any, mut all) = (false, true);
        let mut at = va;
        let last = va + (size - 1);
        loop {
            let step = match self.lookup(mem, at) {
                Slot::Hole { size } => {
                    all = false;
                    size
                }
                Slot::Leaf { level, .. } => {
                    any = true;
                    level_size(level)
                }
            };
            match (at & !(step - 1)).checked_add(step) {
                Some(next) if next - 1 < last => at = next,
                _ => return (any, all),
            }
        }
    }

    /// Maps `[va, va + size)` to `[pa, pa + size)`, using 1 GiB and 2 MiB
    /// blocks wherever both addresses and the remaining size allow.
    pub fn map<M: TableMemory>(
        &mut self,
        mem: &mut M,
        va: u64,
        pa: u64,
        size: u64,
        attributes: Attributes,
    ) -> Result<(), MapError> {
        self.map_with(mem, va, pa, size, attributes, 1)
    }

    /// As [`PageTable::map`], with 4 KiB pages only.
    pub fn map_pages<M: TableMemory>(
        &mut self,
        mem: &mut M,
        va: u64,
        pa: u64,
        size: u64,
        attributes: Attributes,
    ) -> Result<(), MapError> {
        self.map_with(mem, va, pa, size, attributes, 3)
    }

    fn map_with<M: TableMemory>(
        &mut self,
        mem: &mut M,
        va: u64,
        pa: u64,
        size: u64,
        attributes: Attributes,
        first_level: usize,
    ) -> Result<(), MapError> {
        if attributes.is_forbidden() {
            return Err(MapError::Forbidden);
        }
        self.check(va, size)?;
        if !pa.is_multiple_of(PAGE_SIZE) {
            return Err(MapError::Unaligned);
        }
        match pa.checked_add(size - 1) {
            Some(last) if last >> VA_BITS == 0 => {}
            _ => return Err(MapError::OutOfRange),
        }
        if self.coverage(mem, va, size).0 {
            return Err(MapError::AlreadyMapped);
        }
        let mut done = 0;
        while done < size {
            let (at, to, left) = (va + done, pa + done, size - done);
            let level = (first_level..3)
                .find(|l| {
                    let s = level_size(*l);
                    at.is_multiple_of(s) && to.is_multiple_of(s) && left >= s
                })
                .unwrap_or(3);
            if let Err(e) = self.map_leaf(mem, at, to, level, attributes) {
                if done > 0 {
                    // Cannot fail: everything in the range is mapped, and only
                    // whole leaves mapped above are removed.
                    let _ = self.remove(mem, va, done);
                }
                return Err(e);
            }
            done += level_size(level);
        }
        mem.sync();
        Ok(())
    }

    fn map_leaf<M: TableMemory>(
        &mut self,
        mem: &mut M,
        va: u64,
        pa: u64,
        leaf_level: usize,
        attributes: Attributes,
    ) -> Result<(), MapError> {
        let mut table = self.root;
        for level in 0..leaf_level {
            let index = entry_index(va, level);
            let descriptor = mem.read(table, index);
            table = if is_valid(descriptor) {
                descriptor & ADDRESS_MASK
            } else {
                let new = mem.alloc_table().ok_or(MapError::OutOfTables)?;
                mem.write(table, index, table_descriptor(new));
                new
            };
        }
        let descriptor = leaf_descriptor(leaf_level, pa, attributes);
        mem.write(table, entry_index(va, leaf_level), descriptor);
        Ok(())
    }

    /// Unmaps `[va, va + size)`. A block only partly inside the range is
    /// first split into the next level with break-before-make: invalid
    /// entry, TLB invalidation, then the new table ("TLB maintenance
    /// requirements and the Break-Before-Make sequence"). So nothing may
    /// access the rest of such a block during the call.
    ///
    /// Tables left empty stay allocated. They are reused by later mappings.
    pub fn unmap<M: TableMemory>(
        &mut self,
        mem: &mut M,
        va: u64,
        size: u64,
    ) -> Result<(), MapError> {
        self.check(va, size)?;
        if !self.coverage(mem, va, size).1 {
            return Err(MapError::NotMapped);
        }
        // Both splits happen before any removal, so running out of tables
        // leaves every translation in place.
        self.split_at(mem, va)?;
        if let Some(end) = va.checked_add(size) {
            self.split_at(mem, end)?;
        }
        self.remove(mem, va, size)
    }

    /// Makes `va` a leaf boundary.
    fn split_at<M: TableMemory>(&mut self, mem: &mut M, va: u64) -> Result<(), MapError> {
        loop {
            let Slot::Leaf {
                table,
                index,
                level,
                descriptor,
            } = self.lookup(mem, va)
            else {
                return Ok(());
            };
            let size = level_size(level);
            if va.is_multiple_of(size) {
                return Ok(());
            }
            let new = mem.alloc_table().ok_or(MapError::OutOfTables)?;
            let child = level + 1;
            let base_pa = descriptor & ADDRESS_MASK;
            let attributes = Attributes::decode(descriptor);
            for i in 0..ENTRIES {
                let pa = base_pa + i as u64 * level_size(child);
                mem.write(new, i, leaf_descriptor(child, pa, attributes));
            }
            mem.write(table, index, 0);
            mem.invalidate(va & !(size - 1), size);
            mem.write(table, index, table_descriptor(new));
            mem.sync();
        }
    }

    /// Clears the leaves of a range whose ends are leaf boundaries.
    fn remove<M: TableMemory>(&mut self, mem: &mut M, va: u64, size: u64) -> Result<(), MapError> {
        let mut at = va;
        let last = va + (size - 1);
        loop {
            let step = match self.lookup(mem, at) {
                Slot::Hole { size } => size,
                Slot::Leaf {
                    table,
                    index,
                    level,
                    ..
                } => {
                    mem.write(table, index, 0);
                    level_size(level)
                }
            };
            match (at & !(step - 1)).checked_add(step) {
                Some(next) if next - 1 < last => at = next,
                _ => break,
            }
        }
        mem.invalidate(va, size);
        Ok(())
    }

    /// The leaf covering `va`, if any. `Mapping::va` and `pa` are those of
    /// the start of the leaf.
    pub fn translate<M: TableMemory>(&self, mem: &M, va: u64) -> Option<Mapping> {
        if !self.half.contains(va) {
            return None;
        }
        match self.lookup(mem, va) {
            Slot::Hole { .. } => None,
            Slot::Leaf {
                level, descriptor, ..
            } => Some(Mapping {
                va: va & !(level_size(level) - 1),
                pa: descriptor & ADDRESS_MASK,
                size: level_size(level),
                attributes: Attributes::decode(descriptor),
            }),
        }
    }

    /// Calls `f` for every leaf, in address order.
    pub fn for_each_mapping<M: TableMemory>(&self, mem: &M, f: &mut impl FnMut(Mapping)) {
        let base = match self.half {
            Half::Lower => 0,
            Half::Upper => 0xffff << VA_BITS,
        };
        walk(mem, self.root, 0, base, f);
    }
}

fn walk<M: TableMemory>(mem: &M, table: u64, level: usize, base: u64, f: &mut impl FnMut(Mapping)) {
    for index in 0..ENTRIES {
        let descriptor = mem.read(table, index);
        if !is_valid(descriptor) {
            continue;
        }
        let va = base + index as u64 * level_size(level);
        if is_table(descriptor, level) {
            walk(mem, descriptor & ADDRESS_MASK, level + 1, va, f);
        } else {
            f(Mapping {
                va,
                pa: descriptor & ADDRESS_MASK,
                size: level_size(level),
                attributes: Attributes::decode(descriptor),
            });
        }
    }
}

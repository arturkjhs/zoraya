//! The kernel's translation tables.

use core::ptr::addr_of;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_dtb::platform::PlatformInfo;
use zoraya_pagetable::{Attributes, Half, MapError, PageTable, TableMemory};
use zoraya_pmm::{FRAME_SIZE, Range, RangeSet};

use super::layout::{DEVICE_WINDOW, DEVICE_WINDOW_SIZE, DIRECT_MAP_SIZE};
use super::{
    __data_end, __data_start, __emergency_stack_bottom, __emergency_stack_top, __rodata_end,
    __rodata_start, __stack_bottom, __stack_top, __text_end, __text_start, image_virt_to_phys,
    kernel_image_phys, phys_to_virt, symbol, table_virt,
};
use crate::arch::mmu;
use crate::sync::SpinLock;
use crate::{console, dtb};

/// Tables for building the kernel's own mappings, before there is a frame
/// allocator. Sized for the worst case seen, with room: a pool that runs out
/// stops the boot with a message.
const POOL_TABLES: usize = 64;

#[repr(C, align(4096))]
struct Pool([[u64; 512]; POOL_TABLES]);

// Only reached through table addresses handed out by `PoolTables`, one table
// at a time under the lock on `KERNEL`.
static mut POOL: Pool = Pool([[0; 512]; POOL_TABLES]);

/// Table memory in the boot pool. Mapped from the first instruction: the pool
/// is in `.bss`.
struct KernelTables {
    used: usize,
}

impl TableMemory for KernelTables {
    fn alloc_table(&mut self) -> Option<u64> {
        if self.used == POOL_TABLES {
            return None;
        }
        let va = (&raw mut POOL) as u64 + (self.used as u64) * FRAME_SIZE;
        self.used += 1;
        // The pool is in .bss, which is zero, and each table is handed out
        // once.
        Some(image_virt_to_phys(va))
    }

    fn read(&self, table: u64, index: usize) -> u64 {
        // SAFETY: `table` is a table this memory handed out, reached through
        // the image or the direct map, and `index` is below 512 by the
        // crate's construction. It is an aligned u64 in Normal memory.
        unsafe { ((table_virt(table) as *const u64).add(index)).read_volatile() }
    }

    fn write(&mut self, table: u64, index: usize, value: u64) {
        // SAFETY: as in `read`; the table is only written under the lock on
        // `KERNEL`, and this is a single-copy atomic store, as the table
        // walker requires of a live entry.
        unsafe { ((table_virt(table) as *mut u64).add(index)).write_volatile(value) }
    }

    fn invalidate(&mut self, va: u64, size: u64) {
        mmu::invalidate(va, size);
    }

    fn sync(&mut self) {
        mmu::sync_tables();
    }
}

struct Kernel {
    table: PageTable,
    memory: KernelTables,
}

static KERNEL: SpinLock<Option<Kernel>> = SpinLock::new(None);

/// Next free address in the device window.
static NEXT_DEVICE: AtomicU64 = AtomicU64::new(DEVICE_WINDOW);

fn map_device_in(kernel: &mut Kernel, pa: u64, size: u64) -> u64 {
    let range = Range::at(pa, size).frames_around();
    let va = NEXT_DEVICE.load(Ordering::Relaxed);
    let end = va + range.len();
    assert!(
        end <= DEVICE_WINDOW + DEVICE_WINDOW_SIZE,
        "device window full"
    );
    kernel
        .table
        .map(
            &mut kernel.memory,
            va,
            range.start,
            range.len(),
            Attributes::DEVICE,
        )
        .expect("device mapping");
    NEXT_DEVICE.store(end, Ordering::Relaxed);
    va + (pa - range.start)
}

fn map_image(
    kernel: &mut Kernel,
    start: *const u8,
    end: *const u8,
    attributes: Attributes,
) -> Result<(), MapError> {
    let (start, end) = (symbol(start), symbol(end));
    kernel.table.map(
        &mut kernel.memory,
        start,
        image_virt_to_phys(start),
        end - start,
        attributes,
    )
}

/// RAM minus the image, the device tree and every reservation: what the
/// direct map holds as read-write.
fn direct_map_ranges(info: &PlatformInfo<'_>) -> RangeSet<32> {
    let mut set = RangeSet::new();
    for r in info.memory.as_slice() {
        let r = Range::at(r.address, r.size).frames_inside();
        assert!(r.end <= DIRECT_MAP_SIZE, "RAM beyond the direct map");
        set.add(r).expect("direct map ranges");
    }
    set.remove(kernel_image_phys()).expect("direct map ranges");
    for r in info.reserved.as_slice() {
        let r = Range::at(r.address, r.size).frames_around();
        set.remove(r).expect("direct map ranges");
    }
    let (addr, len) = dtb::phys();
    set.remove(Range::at(addr as u64, len as u64).frames_around())
        .expect("direct map ranges");
    set
}

fn build(kernel: &mut Kernel, info: &PlatformInfo<'_>) -> Result<(), MapError> {
    map_image(
        kernel,
        addr_of!(__text_start),
        addr_of!(__text_end),
        Attributes::KERNEL_TEXT,
    )?;
    map_image(
        kernel,
        addr_of!(__rodata_start),
        addr_of!(__rodata_end),
        Attributes::KERNEL_RODATA,
    )?;
    map_image(
        kernel,
        addr_of!(__data_start),
        addr_of!(__data_end),
        Attributes::KERNEL_DATA,
    )?;
    map_image(
        kernel,
        addr_of!(__emergency_stack_bottom),
        addr_of!(__emergency_stack_top),
        Attributes::KERNEL_DATA,
    )?;
    // The page below __stack_bottom, __stack_guard, stays unmapped, and so
    // do the boot tables.
    map_image(
        kernel,
        addr_of!(__stack_bottom),
        addr_of!(__stack_top),
        Attributes::KERNEL_DATA,
    )?;

    for r in direct_map_ranges(info).as_slice() {
        kernel.table.map(
            &mut kernel.memory,
            phys_to_virt(r.start),
            r.start,
            r.len(),
            Attributes::KERNEL_DATA,
        )?;
    }
    let (addr, len) = dtb::phys();
    let blob = Range::at(addr as u64, len as u64).frames_around();
    kernel.table.map(
        &mut kernel.memory,
        phys_to_virt(blob.start),
        blob.start,
        blob.len(),
        Attributes::KERNEL_RODATA,
    )
}

/// Builds the final tables, replaces the boot tables and removes the identity
/// map. On return the console is on its new mapping.
pub fn init(info: &PlatformInfo<'_>) {
    let mut memory = KernelTables { used: 0 };
    let table = PageTable::new(&mut memory, Half::Upper).expect("root table");
    let empty = memory.alloc_table().expect("empty table");
    let mut kernel = Kernel { table, memory };
    build(&mut kernel, info).expect("kernel tables");
    let uart = map_device_in(&mut kernel, info.uart.base, info.uart.size);
    let root = kernel.table.root();
    *KERNEL.lock() = Some(kernel);

    // SAFETY: the new tables map all of the image the kernel uses at the same
    // addresses as the boot tables, and the stack. Everything reached before
    // the identity map goes away is either in the image or stays in the
    // identity map: `info` borrows the device tree through the identity map,
    // and is not used after this function. The console moves to its new
    // mapping before that.
    unsafe {
        mmu::install(root, empty);
        console::rebase(uart as usize);
        mmu::drop_identity_map();
    }
}

/// Calls `f` for every mapping in the kernel's tables.
#[cfg(feature = "qemu-test")]
pub fn for_each_mapping(f: &mut impl FnMut(zoraya_pagetable::Mapping)) {
    let kernel = KERNEL.lock();
    let kernel = kernel.as_ref().expect("paging is up");
    kernel.table.for_each_mapping(&kernel.memory, f);
}

/// The mapping covering `va`, if any.
#[cfg(feature = "qemu-test")]
pub fn translate(va: u64) -> Option<zoraya_pagetable::Mapping> {
    let kernel = KERNEL.lock();
    let kernel = kernel.as_ref().expect("paging is up");
    kernel.table.translate(&kernel.memory, va)
}

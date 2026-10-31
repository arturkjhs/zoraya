//! The kernel's address space and memory (ADR-0020).
//!
//! Hardware access lives in `arch::mmu`; table logic in `zoraya-pagetable`.

pub mod paging;

use core::ptr::addr_of;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_pmm::Range;

/// The virtual layout of ADR-0020. This module is the only place that
/// states it.
pub mod layout {
    pub const DIRECT_MAP: u64 = 0xffff_0000_0000_0000;
    pub const DIRECT_MAP_SIZE: u64 = 1 << 47;
    pub const DEVICE_WINDOW: u64 = 0xffff_8000_0000_0000;
    pub const DEVICE_WINDOW_SIZE: u64 = 1 << 40;
    pub const HEAP_WINDOW: u64 = 0xffff_8100_0000_0000;
    pub const HEAP_WINDOW_SIZE: u64 = 1 << 40;
    /// Reserved for thread stacks from stage 2. Nothing is mapped here.
    pub const STACK_WINDOW: u64 = 0xffff_8200_0000_0000;
    pub const STACK_WINDOW_SIZE: u64 = 1 << 40;
    pub const KERNEL_IMAGE: u64 = 0xffff_ffff_8000_0000;

    const _: () = assert!(DIRECT_MAP + DIRECT_MAP_SIZE == DEVICE_WINDOW);
    const _: () = assert!(DEVICE_WINDOW + DEVICE_WINDOW_SIZE == HEAP_WINDOW);
    const _: () = assert!(HEAP_WINDOW + HEAP_WINDOW_SIZE == STACK_WINDOW);
    const _: () = assert!(STACK_WINDOW + STACK_WINDOW_SIZE <= KERNEL_IMAGE);
}

unsafe extern "C" {
    pub static __kernel_start: u8;
    pub static __kernel_end: u8;
    pub static __text_start: u8;
    pub static __text_end: u8;
    pub static __rodata_start: u8;
    pub static __rodata_end: u8;
    pub static __data_start: u8;
    pub static __data_end: u8;
    pub static __emergency_stack_bottom: u8;
    pub static __emergency_stack_top: u8;
    pub static __stack_guard: u8;
    pub static __stack_bottom: u8;
    pub static __stack_top: u8;
}

static PHYS_BASE: AtomicU64 = AtomicU64::new(0);

/// Records where the image was loaded. Call once, first.
pub fn set_phys_base(phys_base: u64) {
    PHYS_BASE.store(phys_base, Ordering::Relaxed);
}

/// Address of a linker symbol, in the image.
pub fn symbol(s: *const u8) -> u64 {
    s as u64
}

/// Physical address of an address in the image.
pub fn image_virt_to_phys(va: u64) -> u64 {
    va - layout::KERNEL_IMAGE + PHYS_BASE.load(Ordering::Relaxed)
}

/// Physical range of the image, stacks and boot tables included.
pub fn kernel_image_phys() -> Range {
    Range {
        start: PHYS_BASE.load(Ordering::Relaxed),
        end: image_virt_to_phys(symbol(addr_of!(__kernel_end))),
    }
}

/// Where the direct map puts physical address `pa`. Whether that is mapped
/// depends on what it is: RAM other than the image and reserved regions.
pub const fn phys_to_virt(pa: u64) -> u64 {
    layout::DIRECT_MAP + pa
}

/// Where table memory at `pa` can be reached: tables from the boot pool are
/// in the image, which the direct map does not cover.
fn table_virt(pa: u64) -> u64 {
    let image = kernel_image_phys();
    if image.start <= pa && pa < image.end {
        pa - image.start + layout::KERNEL_IMAGE
    } else {
        phys_to_virt(pa)
    }
}

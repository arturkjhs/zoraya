//! MMU control after boot.rs has turned it on.

use core::arch::asm;

/// `SCTLR_EL1.WXN`: writable memory is never executable.
const SCTLR_WXN: u64 = 1 << 19;
/// `TCR_EL1.EPD0`: no table walks for TTBR0 addresses.
const TCR_EPD0: u64 = 1 << 7;

/// Above this size, one `vmalle1is` is cheaper than a loop by page.
const INVALIDATE_ALL_ABOVE: u64 = 2 << 20;

unsafe extern "C" {
    fn __replace_ttbr1(new: u64, empty: u64);
}

/// Replaces the TTBR1 tables, then turns on `SCTLR_EL1.WXN`.
///
/// # Safety
///
/// `root` must be a complete table that maps the running code, the stack and
/// everything the caller goes on to use, without writable executable pages.
/// `empty` must be a table of invalid entries. The boot identity map must
/// still be active, and it maps `__replace_ttbr1` at its physical address.
pub unsafe fn install(root: u64, empty: u64) {
    let replace = crate::memory::image_virt_to_phys(__replace_ttbr1 as *const () as u64);
    let daif = super::save_and_mask_all();
    // SAFETY: the caller's contract. `__replace_ttbr1` uses no stack and
    // touches only the registers named here and x30; all exceptions are
    // masked, because the vectors are not mapped during the change. The dsb
    // makes the tables visible to the walker first.
    unsafe {
        asm!(
            "dsb ishst",
            "blr {replace}",
            replace = in(reg) replace,
            in("x0") root,
            in("x1") empty,
            clobber_abi("C"),
            options(nostack),
        )
    };

    // SCTLR_EL1.WXN may be cached in a TLB, so it needs invalidation to take
    // effect ("Preferred Execute-never" and "TLB maintenance requirements").
    // SAFETY: the new tables have no writable executable page, so nothing
    // changes for the running code.
    unsafe {
        asm!(
            "mrs {t}, sctlr_el1",
            "orr {t}, {t}, {wxn}",
            "msr sctlr_el1, {t}",
            "isb",
            "tlbi vmalle1",
            "dsb nsh",
            "isb",
            t = out(reg) _,
            wxn = in(reg) SCTLR_WXN,
            options(nostack),
        )
    };
    super::restore_interrupts(daif);
}

/// Stops all translation through TTBR0.
///
/// # Safety
///
/// Nothing may use a TTBR0 address afterwards.
pub unsafe fn drop_identity_map() {
    // SAFETY: the caller's contract. EPD0 makes a TTBR0 address fault at once
    // with a level 0 translation fault; TTBR0 then holds no table, and the
    // invalidation removes what was cached from the boot tables.
    unsafe {
        asm!(
            "mrs {t}, tcr_el1",
            "orr {t}, {t}, {epd0}",
            "msr tcr_el1, {t}",
            "isb",
            "msr ttbr0_el1, xzr",
            "isb",
            "tlbi vmalle1",
            "dsb nsh",
            "isb",
            t = out(reg) _,
            epd0 = in(reg) TCR_EPD0,
            options(nostack),
        )
    };
}

/// Makes table writes visible to the walker, for a change from invalid to
/// valid, which needs no TLB invalidation. The `isb` makes the new mapping
/// usable by the following instructions.
pub fn sync_tables() {
    // SAFETY: barriers only.
    unsafe { asm!("dsb ishst", "isb", options(nostack, preserves_flags)) };
}

/// Removes every TLB entry for `[va, va + size)`, on all CPUs, after making
/// earlier table writes visible ("TLB maintenance instructions").
pub fn invalidate(va: u64, size: u64) {
    if size > INVALIDATE_ALL_ABOVE {
        // SAFETY: TLB maintenance has no effect on memory contents.
        unsafe {
            asm!(
                "dsb ishst",
                "tlbi vmalle1is",
                "dsb ish",
                "isb",
                options(nostack, preserves_flags)
            )
        };
        return;
    }
    // SAFETY: as above. The operand holds VA[55:12] in bits 43:0; VAAE1IS
    // matches all ASIDs.
    unsafe { asm!("dsb ishst", options(nostack, preserves_flags)) };
    for page in (0..size).step_by(4096) {
        let operand = ((va + page) >> 12) & ((1 << 44) - 1);
        // SAFETY: as above.
        unsafe { asm!("tlbi vaae1is, {}", in(reg) operand, options(nostack, preserves_flags)) };
    }
    // SAFETY: barriers only.
    unsafe { asm!("dsb ish", "isb", options(nostack, preserves_flags)) };
}

/// `TCR_EL1`.
#[cfg(feature = "qemu-test")]
pub fn tcr() -> u64 {
    let v: u64;
    // SAFETY: reading TCR_EL1 has no side effects.
    unsafe { asm!("mrs {}, tcr_el1", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// `TTBR0_EL1`.
#[cfg(feature = "qemu-test")]
pub fn ttbr0() -> u64 {
    let v: u64;
    // SAFETY: reading TTBR0_EL1 has no side effects.
    unsafe { asm!("mrs {}, ttbr0_el1", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// `SCTLR_EL1`.
#[cfg(feature = "qemu-test")]
pub fn sctlr() -> u64 {
    let v: u64;
    // SAFETY: reading SCTLR_EL1 has no side effects.
    unsafe { asm!("mrs {}, sctlr_el1", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

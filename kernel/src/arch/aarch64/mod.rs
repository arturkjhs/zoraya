//! aarch64 boot code and the few system registers stage 0 reads.

mod boot;

use core::arch::asm;

/// Current exception level (0 to 3).
pub fn current_el() -> u64 {
    let v: u64;
    // SAFETY: reading CurrentEL has no side effects and is permitted at EL1.
    unsafe { asm!("mrs {}, CurrentEL", out(reg) v, options(nomem, nostack, preserves_flags)) };
    (v >> 2) & 3
}

/// Physical address size in bits, from `ID_AA64MMFR0_EL1.PARange`.
pub fn pa_bits() -> u32 {
    let v: u64;
    // SAFETY: reading an ID register has no side effects and is permitted at EL1.
    unsafe {
        asm!("mrs {}, id_aa64mmfr0_el1", out(reg) v, options(nomem, nostack, preserves_flags))
    };
    match v & 0xf {
        0 => 32,
        1 => 36,
        2 => 40,
        3 => 42,
        4 => 44,
        5 => 48,
        _ => 52,
    }
}

/// `MPIDR_EL1`.
pub fn mpidr() -> u64 {
    let v: u64;
    // SAFETY: reading MPIDR_EL1 has no side effects and is permitted at EL1.
    unsafe { asm!("mrs {}, mpidr_el1", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// Stops this CPU for good.
pub fn halt() -> ! {
    loop {
        // SAFETY: `wfe` only waits for an event; looping makes spurious wake-ups harmless.
        unsafe { asm!("wfe", options(nomem, nostack, preserves_flags)) };
    }
}

unsafe extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
    static __stack_top: u8;
    static __stack_bottom: u8;
}

/// Written at the lowest word of the boot stack. With the MMU off there is no
/// guard page, so an overflow would run silently into `.bss`; a changed
/// canary is the only sign of it.
const STACK_CANARY: u64 = 0x5a0e_a57a_c4ca_2a2e;

fn stack_canary_ptr() -> *mut u64 {
    core::ptr::addr_of!(__stack_bottom) as *mut u64
}

/// Writes the stack canary. Call once, early.
pub fn stack_canary_set() {
    // SAFETY: __stack_bottom is the 16-byte aligned lowest address of the
    // boot stack (linker.ld); the stack grows down from __stack_top and
    // reaches this word only on overflow.
    unsafe { core::ptr::write_volatile(stack_canary_ptr(), STACK_CANARY) }
}

/// False if the stack has grown into its lowest word.
pub fn stack_canary_intact() -> bool {
    // SAFETY: as in `stack_canary_set`.
    unsafe { core::ptr::read_volatile(stack_canary_ptr()) == STACK_CANARY }
}

/// Physical address range `[start, end)` of the loaded image, stack included.
pub fn kernel_image() -> (usize, usize) {
    // Only the symbols' addresses (defined by linker.ld) are taken; the
    // bytes are never read.
    (
        core::ptr::addr_of!(__kernel_start) as usize,
        core::ptr::addr_of!(__kernel_end) as usize,
    )
}

/// Abandons the current stack: resets the stack pointer to the top of the
/// boot stack and calls `f(arg)`, which must not return.
///
/// # Safety
///
/// Every frame on the current stack is discarded without running any code
/// in it, so nothing may still reference data on the stack, and no other
/// code may be running on this stack (one CPU, interrupts masked).
#[cfg(feature = "qemu-test")]
pub unsafe fn reset_stack_and_call(f: extern "C" fn(usize) -> !, arg: usize) -> ! {
    // SAFETY: the caller guarantees the old frames are dead; __stack_top is
    // the 16-byte aligned top of the boot stack from linker.ld.
    unsafe {
        asm!(
            "mov sp, {top}",
            "br {f}",
            top = in(reg) core::ptr::addr_of!(__stack_top),
            f = in(reg) f,
            in("x0") arg,
            options(noreturn),
        )
    }
}

/// Exception class names for the catch-all handler.
fn exception_kind(index: u64) -> &'static str {
    match index {
        0..=3 => "current EL with SP_EL0",
        4..=7 => "current EL with SP_ELx",
        8..=11 => "lower EL, AArch64",
        _ => "lower EL, AArch32",
    }
}

/// Target of every entry of the catch-all vector table in boot.rs.
#[unsafe(no_mangle)]
extern "C" fn unhandled_exception(index: u64, esr: u64, elr: u64, far: u64) -> ! {
    let kind = ["synchronous", "IRQ", "FIQ", "SError"][(index & 3) as usize];
    panic!(
        "unhandled {kind} exception from {}: ESR_EL1={esr:#x} (EC={:#x}) ELR_EL1={elr:#x} FAR_EL1={far:#x}",
        exception_kind(index),
        esr >> 26,
    );
}

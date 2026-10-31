//! aarch64 boot code, exception handling and system registers.

mod boot;
mod exception;

pub use exception::init as exception_init;
#[cfg(feature = "qemu-test")]
pub use exception::{catch, recover_to};

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

/// Written at the lowest word of the boot stack, as a second check of the
/// stack's limit.
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

/// Masks IRQ and FIQ on this CPU. SError stays as it is.
pub fn mask_interrupts() {
    // SAFETY: setting mask bits has no effect on memory or on Rust's state.
    unsafe { asm!("msr daifset, #3", options(nomem, nostack, preserves_flags)) };
}

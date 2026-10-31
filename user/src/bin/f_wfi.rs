#![no_std]
#![no_main]

//! Must fault: `wfi` traps at EL0.

use zoraya_user as _;

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: not safe at all: this instruction is the test.
    unsafe { core::arch::asm!("wfi") };
    0
}

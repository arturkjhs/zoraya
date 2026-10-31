#![no_std]
#![no_main]

//! Must fault: a branch to an unaligned address.

use zoraya_user as _;

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: not safe at all: this jump is the test.
    unsafe { core::arch::asm!("br {}", in(reg) 0x40_0002u64) };
    0
}

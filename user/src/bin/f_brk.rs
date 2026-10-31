#![no_std]
#![no_main]

//! Must fault: a breakpoint with no debugger.

use zoraya_user as _;

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: not safe at all: this instruction is the test.
    unsafe { core::arch::asm!("brk #1") };
    0
}

#![no_std]
#![no_main]

//! Loops until something ends it, and never enters the kernel.

use core::arch::asm;

use zoraya_user as _;

zoraya_user::describe!("demos", "spin");

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: a branch to itself touches nothing.
    unsafe { asm!("2:", "b 2b", options(noreturn)) }
}

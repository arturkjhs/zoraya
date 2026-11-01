#![no_std]
#![no_main]

//! Never enters the kernel while it runs, so only a timer interrupt on the
//! way back to EL0 can let another thread in.

use core::arch::asm;

use zoraya_user as _;

const ROUNDS: u64 = 150_000_000;

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: only the named register changes.
    unsafe { asm!("2:", "subs {n}, {n}, #1", "b.ne 2b", n = inout(reg) ROUNDS => _) };
    1
}

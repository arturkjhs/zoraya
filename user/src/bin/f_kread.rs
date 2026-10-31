#![no_std]
#![no_main]

//! Must fault: reading kernel memory.

use zoraya_user as _;

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: not safe at all: this access is the test.
    unsafe { core::ptr::read_volatile(0xffff_ffff_8000_0000 as *const u64) as i32 }
}

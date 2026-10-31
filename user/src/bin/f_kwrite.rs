#![no_std]
#![no_main]

//! Must fault: writing the direct map.

use zoraya_user as _;

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: not safe at all: this access is the test.
    unsafe { core::ptr::write_volatile(0xffff_0000_4800_0000 as *mut u64, 1) };
    0
}

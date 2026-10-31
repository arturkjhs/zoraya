#![no_std]
#![no_main]

//! Must fault: address 0.

use zoraya_user as _;

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: not safe at all: this access is the test.
    unsafe { core::ptr::read_volatile(core::hint::black_box(0usize) as *const u64) as i32 }
}

#![no_std]
#![no_main]

//! Must fault: running off the stack into the unmapped page below it.

use zoraya_user as _;

#[inline(never)]
fn deep(n: u64) -> u64 {
    let frame = [n; 512];
    if n == u64::MAX {
        return 0;
    }
    deep(core::hint::black_box(n + 1)) + core::hint::black_box(&frame)[1]
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    deep(0) as i32
}

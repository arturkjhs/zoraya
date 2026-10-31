#![no_std]
#![no_main]

//! Must fault: running data, which W^X forbids.

use zoraya_user as _;

/// `ret`, in writable memory.
static mut CODE: [u32; 2] = [0xd65f_03c0; 2];

#[unsafe(no_mangle)]
fn main() -> i32 {
    // SAFETY: not safe at all: this jump is the test.
    let f: extern "C" fn() = unsafe { core::mem::transmute(&raw const CODE) };
    f();
    0
}

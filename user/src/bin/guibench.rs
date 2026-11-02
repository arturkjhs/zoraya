#![no_std]
#![no_main]

//! A helper of `t_gui`. `guibench scribble` maps the memory object at slot 5
//! writable and rewrites it without pause, as a client that changes its
//! buffer while the compositor copies it (ADR-0093).

use zoraya_abi::{Handle, MapFlags};
use zoraya_user::{memory_map, startup};

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut argv = startup().argv;
    if argv.next() != Some(&b"scribble"[..]) {
        return 2;
    }
    let memory = Handle::new(5).expect("slot");
    let size = 256 * 256 * 4;
    let at = memory_map(memory, 0, size, MapFlags::WRITE).expect("map");
    let mut value = 0u32;
    loop {
        for i in 0..(size / 4) as usize {
            // SAFETY: inside a writable mapping of `size` bytes of this
            // process; another process reads it, which is the point.
            unsafe { ((at as *mut u32).add(i)).write_volatile(value) };
            value = value.wrapping_add(0x0001_0203);
        }
    }
}

#![no_std]
#![no_main]

//! Two copies run at once at the same addresses: each must keep seeing its
//! own data, whatever the other writes.

use core::ptr::{addr_of_mut, read_volatile, write_volatile};

use zoraya_user::{clock_monotonic, thread_yield};

static mut VALUE: u64 = 0;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mine = clock_monotonic() | 1;
    let at = addr_of_mut!(VALUE);
    // SAFETY: this program has one thread.
    unsafe {
        if read_volatile(at) != 0 {
            return 1;
        }
        write_volatile(at, mine);
        for _ in 0..200 {
            thread_yield();
            if read_volatile(at) != mine {
                return 2;
            }
        }
    }
    0
}

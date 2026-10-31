#![no_std]
#![no_main]

//! INV-0030-1: running alone, with no deadline queued, the timer is off.

use zoraya_user::{clock_monotonic, raw_syscall};

/// `syscall::test_calls::TIMER_IS_ON`.
const TIMER_IS_ON: u64 = 0x7000;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let end = clock_monotonic() + 20_000_000;
    while clock_monotonic() < end {
        if raw_syscall(TIMER_IS_ON, 0, 0, 0).1 != 0 {
            return 1;
        }
    }
    0
}

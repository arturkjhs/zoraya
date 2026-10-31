#![no_std]
#![no_main]

//! Stays runnable for a while, so that only preemption can let others in.

use zoraya_user::clock_monotonic;

const RUN_NS: u64 = 60_000_000;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let end = clock_monotonic() + RUN_NS;
    let mut rounds = 0u32;
    while clock_monotonic() < end {
        rounds = rounds.wrapping_add(1);
    }
    i32::from(rounds > 0)
}

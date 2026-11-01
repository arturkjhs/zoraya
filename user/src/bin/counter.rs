#![no_std]
#![no_main]

//! Counts for ever, and tells the test kernel where (INV-0004-4).

use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_user::raw_syscall;

/// `syscall::test_calls::WATCH`.
const WATCH: u64 = 0x7004;

static COUNT: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
fn main() -> i32 {
    raw_syscall(WATCH, [COUNT.as_ptr() as u64, 0, 0, 0, 0, 0]);
    loop {
        COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

#![no_std]
#![no_main]

//! Makes the kernel overflow this thread's stack: the limit it runs against
//! was set on entry from EL0. The test kernel panics, as it should.

use zoraya_user::raw_syscall;

/// `syscall::test_calls::OVERFLOW_STACK`.
const OVERFLOW_STACK: u64 = 0x7001;

#[unsafe(no_mangle)]
fn main() -> i32 {
    raw_syscall(OVERFLOW_STACK, 0, 0, 0);
    1
}

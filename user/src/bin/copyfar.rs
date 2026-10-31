#![no_std]
#![no_main]

//! INV-0036-1: a fault on a kernel address in the copy routine is not taken
//! for a bad user pointer, even though the source here is good.

use zoraya_user::raw_syscall;

/// `syscall::test_calls::COPY_TO_BAD_KERNEL`.
const COPY_TO_BAD_KERNEL: u64 = 0x7002;

static SOURCE: u8 = 1;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let (_, not_resumed) = raw_syscall(COPY_TO_BAD_KERNEL, &raw const SOURCE as u64, 0, 0);
    i32::from(not_resumed != 1)
}

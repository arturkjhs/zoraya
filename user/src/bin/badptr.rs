#![no_std]
#![no_main]

//! Every bad argument must come back as a typed error.

use zoraya_abi::{CONSOLE, Status, Syscall};
use zoraya_user::{console_write, raw_syscall};

const WRITE: u64 = Syscall::ConsoleWrite as u64;

fn expect(addr: u64, len: u64, status: Status) -> bool {
    raw_syscall(WRITE, u64::from(CONSOLE.raw()), addr, len).0 == status as u64
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let checks = [
        expect(0, 1, Status::BadAddress),
        expect(0x1000, 16, Status::BadAddress),
        // Kernel addresses, past the range check and below it.
        expect(0xffff_ffff_8000_0000, 16, Status::BadAddress),
        expect(0xffff_0000_4000_0000, 16, Status::BadAddress),
        expect(0x7fff_ffff_f000, 0x2000, Status::BadAddress),
        // Wrapping.
        expect(u64::MAX - 4, 16, Status::BadAddress),
        // An unmapped page between mapped ones, reached part way through.
        expect(0x3f_f000, 0x2000, Status::BadAddress),
        expect(0x40_0000, 1 << 20, Status::InvalidArgument),
        raw_syscall(WRITE, 0, 0, 0).0 == Status::BadHandle as u64,
        raw_syscall(WRITE, 99, 0, 0).0 == Status::BadHandle as u64,
        raw_syscall(WRITE, u64::MAX, 0, 0).0 == Status::BadHandle as u64,
        raw_syscall(1000, 0, 0, 0).0 == Status::BadSyscall as u64,
        raw_syscall(u64::MAX, 0, 0, 0).0 == Status::BadSyscall as u64,
        console_write(CONSOLE, &[0xff, 0xfe]) == Err(Status::InvalidArgument),
        console_write(CONSOLE, b"") == Ok(0),
    ];
    match checks.iter().position(|ok| !ok) {
        Some(i) => i as i32 + 1,
        None => 0,
    }
}

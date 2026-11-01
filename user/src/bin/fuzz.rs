#![no_std]
#![no_main]

//! INV-0032-2: random register values into every call that can return.

use zoraya_abi::{SYSCALLS, Status, Syscall};
use zoraya_user::raw_syscall;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    let calls = SYSCALLS.len() as u64 + 2;
    for _ in 0..20_000 {
        let number = next() % calls;
        // Exit would end the run, and a far deadline would never come back.
        if number == Syscall::ProcessExit as u64
            || number == Syscall::ProcessExitWithReason as u64
            || number == Syscall::JobKill as u64
        {
            continue;
        }
        if number == Syscall::SleepUntil as u64 {
            raw_syscall(number, [next() % 1000, 0, 0, 0, 0, 0]);
            continue;
        }
        let mut registers = [0; 6];
        for register in &mut registers {
            // Small values reach the handles this process has.
            *register = match next() % 3 {
                0 => next() % 16,
                1 => next() >> (next() % 64),
                _ => next(),
            };
        }
        if number == Syscall::PortWait as u64 {
            // A wait with no deadline in it could block for ever.
            registers[1] = next() % 1000;
        }
        let (status, _) = raw_syscall(number, registers);
        if Status::from_raw(status).is_none() {
            return 1;
        }
    }
    0
}

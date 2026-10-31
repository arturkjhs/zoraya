#![no_std]
#![no_main]

//! INV-0032-2: random register values into every call that can return.

use zoraya_abi::Syscall;
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
    for _ in 0..20_000 {
        let number = next() % 12;
        // Exit would end the run, and a far deadline would never come back.
        if number == Syscall::ProcessExit as u64 || number == Syscall::SleepUntil as u64 {
            continue;
        }
        let (status, _) = raw_syscall(number, next() >> (next() % 64), next(), next() >> 40);
        if status > 5 {
            return 1;
        }
    }
    0
}

#![no_std]
#![no_main]

//! `sleep <seconds>`.

use zoraya_user::{clock_monotonic, println, sleep_until, startup};

#[unsafe(no_mangle)]
fn main() -> i32 {
    let seconds: Option<u64> = startup()
        .argv
        .next()
        .and_then(|a| core::str::from_utf8(a).ok())
        .and_then(|a| a.parse().ok());
    let Some(seconds) = seconds else {
        println!("usage: sleep <seconds>");
        return 2;
    };
    sleep_until(clock_monotonic() + seconds * 1_000_000_000);
    0
}

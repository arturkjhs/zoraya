#![no_std]
#![no_main]

//! Prints `tick` every 50 ms: output is how a test sees it is still alive.

use zoraya_user::{clock_monotonic, println, sleep_until};

#[unsafe(no_mangle)]
fn main() -> i32 {
    loop {
        println!("tick");
        sleep_until(clock_monotonic() + 50_000_000);
    }
}

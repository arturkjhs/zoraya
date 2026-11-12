#![no_std]
#![no_main]

//! Takes memory until its job refuses (`OverQuota`), says how much it got,
//! and exits with 42. A test program for the room tests (INV-0263-4's hint,
//! INV-0266-1): `tests/shell/rooms.script` runs it in a room with `--mem`.

use zoraya_abi::{MemoryFlags, Status};
use zoraya_user::{clock_monotonic, command, memory_create, println, sleep_until};

zoraya_user::describe!("tests", "memhog");

const CHUNK: u64 = 64 << 10;
/// Enough for a limit of 256 MiB; a job with none stops it here.
const MOST: u64 = 4096;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let hold: u64 = c
        .parse()
        .value(&c.d, "hold")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .min(600);
    for held in 0..MOST {
        match memory_create(CHUNK, MemoryFlags::NONE) {
            Ok(_) => {}
            Err(Status::OverQuota) => {
                println!("memhog: refused after {} KiB", held * (CHUNK >> 10));
                // The memory stays mapped while the program sleeps.
                sleep_until(clock_monotonic() + hold * 1_000_000_000);
                return 42;
            }
            Err(other) => {
                println!("memhog: {other:?}");
                return 1;
            }
        }
    }
    println!("memhog: no limit reached");
    1
}

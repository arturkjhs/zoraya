#![no_std]
#![no_main]

//! `sleep SECONDS`.

use zoraya_user::{clock_monotonic, command, sleep_until};

zoraya_user::describe!("commands", "sleep");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    let text = p.operand(&c.d, "SECONDS").unwrap_or("");
    let Some(seconds) = zoraya_describe::parse_number(text).filter(|s| (0..=1_000_000).contains(s))
    else {
        c.error(
            Some(text),
            "not a whole number of seconds from 0 to 1000000",
        );
        return 2;
    };
    // `sleep_until` may return early (a wake-up that is not the deadline); the
    // command ends when the clock has passed it.
    let deadline = clock_monotonic() + seconds as u64 * 1_000_000_000;
    while clock_monotonic() < deadline {
        sleep_until(deadline);
    }
    0
}

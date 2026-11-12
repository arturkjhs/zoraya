#![no_std]
#![no_main]

//! `date` (ADR-0226 item 5): the wall clock in UTC.

use zoraya_files::format::utc;
use zoraya_user::{command, namespace, println};

zoraya_user::describe!("commands", "date");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let _ = c.parse();
    match namespace::realtime() {
        Some(now) => {
            println!("{}", utc(now));
            0
        }
        None => {
            c.error(None, "this program has no wall clock");
            1
        }
    }
}

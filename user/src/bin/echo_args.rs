#![no_std]
#![no_main]

//! Prints each argument on a line: what `spawn` delivered.

use zoraya_user::{println, startup};

zoraya_user::describe!("demos", "echo-args");

#[unsafe(no_mangle)]
fn main() -> i32 {
    for (i, arg) in startup().argv.enumerate() {
        println!("arg {i}: {}", core::str::from_utf8(arg).unwrap_or("?"));
    }
    0
}

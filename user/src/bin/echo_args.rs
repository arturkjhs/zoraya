#![no_std]
#![no_main]

//! Prints each argument on a line, then each environment entry: what
//! `spawn` delivered (INV-0150-4's in-QEMU half reads the second part).

use zoraya_user::{command, println, startup};

zoraya_user::describe!("demos", "echo-args");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    for (i, arg) in p.operand_values(&c.d, "WORD").iter().enumerate() {
        println!("arg {i}: {arg}");
    }
    for (i, entry) in startup().envp.enumerate() {
        println!("env {i}: {}", core::str::from_utf8(entry).unwrap_or("?"));
    }
    0
}

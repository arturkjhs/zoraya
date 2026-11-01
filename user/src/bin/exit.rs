#![no_std]
#![no_main]

//! `exit <n>`: ends with code `n`.

use zoraya_user::{println, startup};

#[unsafe(no_mangle)]
fn main() -> i32 {
    let code = startup()
        .argv
        .next()
        .and_then(|a| core::str::from_utf8(a).ok())
        .and_then(|a| a.parse().ok());
    match code {
        Some(code) => code,
        None => {
            println!("usage: exit <n>");
            2
        }
    }
}

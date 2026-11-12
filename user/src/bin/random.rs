#![no_std]
#![no_main]

//! `random` (ADR-0227): random bytes in hex from the connection the shell
//! gave this program to the random service.

use zoraya_user::{command, print, println};

zoraya_user::describe!("commands", "random");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    let count = match p.operand(&c.d, "COUNT") {
        None => 16,
        Some(text) => {
            match zoraya_describe::parse_number(text).filter(|n| (1..=4096).contains(n)) {
                Some(n) => n as usize,
                None => {
                    c.error(Some(text), "not a number of bytes from 1 to 4096");
                    return 2;
                }
            }
        }
    };
    let mut bytes = alloc::vec![0u8; count];
    if !zoraya_user::random::random(&mut bytes) {
        c.error(None, "no random service");
        return 1;
    }
    print!("random: ");
    for b in bytes {
        print!("{b:02x}");
    }
    println!(" ok");
    0
}

extern crate alloc;

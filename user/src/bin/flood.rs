#![no_std]
#![no_main]

//! Prints without end, to show the shell still answers.

use zoraya_user::println;

#[unsafe(no_mangle)]
fn main() -> i32 {
    loop {
        println!("flood");
    }
}

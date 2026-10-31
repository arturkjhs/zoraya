#![no_std]
#![no_main]

//! The roadmap criterion: print through a native system call.

use zoraya_user::println;

#[unsafe(no_mangle)]
fn main() -> i32 {
    println!("hello from EL0");
    7
}

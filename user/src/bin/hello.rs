#![no_std]
#![no_main]

//! The roadmap criterion: print through a native system call.

use zoraya_user::println;

zoraya_user::describe!("demos", "hello");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = zoraya_user::command::start(DESCRIPTION);
    c.parse();
    println!("hello from EL0");
    7
}

#![no_std]
#![no_main]

//! Reports the handles it started with, and whether its console can read.

use zoraya_abi::{CONSOLE, Handle, Rights, Status};
use zoraya_user::{console_read, handle_duplicate, print, println};

#[unsafe(no_mangle)]
fn main() -> i32 {
    print!("slots:");
    for slot in 1..=16 {
        let handle = Handle::new(slot).expect("slot is not zero");
        if handle_duplicate(handle, Rights::NONE) != Err(Status::BadHandle) {
            print!(" {slot}");
        }
    }
    println!();
    let mut byte = [0];
    println!("console read: {:?}", console_read(CONSOLE, &mut byte));
    0
}

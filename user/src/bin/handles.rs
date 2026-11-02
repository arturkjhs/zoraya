#![no_std]
#![no_main]

//! Reports the handles it started with, and whether its console can read.

use zoraya_abi::{CONSOLE, Handle, Rights, Status};
use zoraya_user::{console_read, handle_close, handle_duplicate, print, println};

#[unsafe(no_mangle)]
fn main() -> i32 {
    print!("slots:");
    for slot in 1..=16 {
        let handle = Handle::new(slot).expect("slot is not zero");
        let copy = handle_duplicate(handle, Rights::NONE);
        if copy != Err(Status::BadHandle) {
            print!(" {slot}");
        }
        if let Ok(copy) = copy {
            handle_close(copy).expect("close");
        }
    }
    println!();
    let mut byte = [0];
    println!("console read: {:?}", console_read(CONSOLE, &mut byte));
    0
}

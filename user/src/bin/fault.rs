#![no_std]
#![no_main]

//! Touches an address nothing maps.

use zoraya_user as _;

zoraya_user::describe!("demos", "fault");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = zoraya_user::command::start(DESCRIPTION);
    c.parse();
    // SAFETY: none: the fault is the point, and it ends this process.
    unsafe { core::ptr::read_volatile(0x10 as *const u8) as i32 }
}

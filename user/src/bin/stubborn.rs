#![no_std]
#![no_main]

//! Ignores `Interrupt` and spins: only a kill ends it.

use zoraya_abi::{Disposition, RequestKind};
use zoraya_user::{println, process_set_disposition};

zoraya_user::describe!("demos", "stubborn");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = zoraya_user::command::start(DESCRIPTION);
    c.parse();
    process_set_disposition(RequestKind::Interrupt, Disposition::Ignore).expect("disposition");
    println!("stubborn: ignoring Interrupt");
    loop {
        core::hint::spin_loop();
    }
}

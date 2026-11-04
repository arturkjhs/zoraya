#![no_std]
#![no_main]

//! `mkdir` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "mkdir");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("mkdir", DESCRIPTION)
}

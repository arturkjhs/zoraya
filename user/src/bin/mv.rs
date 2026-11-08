#![no_std]
#![no_main]

//! `mv` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "mv");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("mv", DESCRIPTION)
}

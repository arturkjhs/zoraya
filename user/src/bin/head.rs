#![no_std]
#![no_main]

//! `head` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "head");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("head", DESCRIPTION)
}

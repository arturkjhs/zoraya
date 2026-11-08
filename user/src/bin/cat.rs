#![no_std]
#![no_main]

//! `cat` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "cat");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("cat", DESCRIPTION)
}

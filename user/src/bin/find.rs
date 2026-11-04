#![no_std]
#![no_main]

//! `find` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "find");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("find", DESCRIPTION)
}

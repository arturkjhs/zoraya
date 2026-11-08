#![no_std]
#![no_main]

//! `cp` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "cp");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("cp", DESCRIPTION)
}

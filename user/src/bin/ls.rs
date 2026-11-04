#![no_std]
#![no_main]

//! `ls` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "ls");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("ls", DESCRIPTION)
}

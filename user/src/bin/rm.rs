#![no_std]
#![no_main]

//! `rm` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "rm");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("rm", DESCRIPTION)
}

#![no_std]
#![no_main]

//! `stat` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "stat");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("stat", DESCRIPTION)
}

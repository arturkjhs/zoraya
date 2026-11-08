#![no_std]
#![no_main]

//! `search` (ADR-0155): `zoraya-commands` over what `zosh` designated.

zoraya_user::describe!("commands", "search");

#[unsafe(no_mangle)]
fn main() -> i32 {
    zoraya_user::filecmd::main("search", DESCRIPTION)
}

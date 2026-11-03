#![no_std]
#![no_main]

//! `exit-with CODE`: ends with exit code `CODE` (TASK-11's `exit`, renamed
//! so that `exit` is only the builtin, ADR-0155 rule 8).

use zoraya_user::command;

zoraya_user::describe!("demos", "exit-with");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    let text = p.operand(&c.d, "CODE").unwrap_or("");
    match zoraya_describe::parse_number(text) {
        Some(code @ 0..=255) => code as i32,
        _ => {
            c.error(Some(text), "not an exit code from 0 to 255");
            2
        }
    }
}

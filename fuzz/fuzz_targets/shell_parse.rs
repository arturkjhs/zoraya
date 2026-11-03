//! The shell's parser (ADR-0150, INV-0150-3) on arbitrary text: a tree or an
//! error with a position inside the input, never a panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zoraya_shell::syntax::{Error, parse};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };
    match parse(text) {
        Ok(_) | Err(Error::Incomplete) => {}
        Err(Error::Invalid { at, .. }) => assert!(at <= text.len()),
    }
});

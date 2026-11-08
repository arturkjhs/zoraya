//! The command-description parser (ADR-0156, INV-0156-1) on arbitrary
//! bytes: it must give a description or an error with a line inside the
//! input, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let lines = 1 + data.iter().filter(|b| **b == b'\n').count();
    match zoraya_describe::parse(data) {
        Ok(d) => assert!(!d.name.is_empty()),
        Err(e) => assert!(e.line <= lines),
    }
});

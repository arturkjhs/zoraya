#![no_std]
#![no_main]

//! Writes until the directory's limit refuses, and times syncs: a test
//! program for the gate's `disk-filler` row (ADR-0265, INV-0265-3 and
//! INV-0265-5). `tests/shell/gate.script` runs it in a room with `--disk`.

extern crate alloc;

use alloc::format;
use alloc::vec;

use zoraya_proto_file::{open_flags, rights, status};
use zoraya_user::{clock_monotonic, command, namespace, println};

zoraya_user::describe!("tests", "diskfill");

const BLOCK: usize = 32 << 10;
/// Stops a run that nothing limits.
const MOST: u64 = 64 << 20;

fn refused(code: u32) -> bool {
    code == status::OVER_QUOTA || code == status::NO_SPACE
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    let Some(ns) = namespace::get() else {
        println!("diskfill: it was given no files: run it from zosh");
        return 1;
    };
    let dir = p.operand(&c.d, "DIR").unwrap_or(".");
    let number = |name: &str| p.value(&c.d, name).and_then(|v| v.parse::<u64>().ok());
    let syncs = number("syncs");
    let seconds = number("loop");
    if syncs.is_some() || seconds.is_some() {
        let f = match ns.open(
            &format!("{dir}/synced"),
            open_flags::CREATE | open_flags::TRUNCATE,
            rights::READ | rights::WRITE | rights::SYNC,
        ) {
            Ok(f) => f,
            Err(s) => {
                println!("diskfill: {dir}/synced: status {s}");
                return 1;
            }
        };
        let started = clock_monotonic();
        let (mut done, mut slowest) = (0u64, 0u64);
        loop {
            if let Some(n) = syncs
                && done >= n
            {
                break;
            }
            if let Some(s) = seconds
                && clock_monotonic() - started > s * 1_000_000_000
            {
                break;
            }
            let t = clock_monotonic();
            let bytes = [done as u8; 4096];
            if let Err(s) = f.write((done % 16) * 4096, 0, &bytes).and_then(|_| f.sync()) {
                println!("diskfill: sync {done}: status {s}");
                return 1;
            }
            slowest = slowest.max((clock_monotonic() - t) / 1_000_000);
            done += 1;
        }
        // INV-0265-5: each sync within 5 s plus one commit's time.
        let late = if syncs.is_some() && slowest > 5_000 { " (over 5 s)" } else { "" };
        println!("diskfill: {done} syncs, slowest {slowest} ms{late}");
        return i32::from(!late.is_empty());
    }
    let block = vec![0x5au8; BLOCK];
    let mut written = 0u64;
    let mut index = 0u32;
    loop {
        let f = match ns.open(
            &format!("{dir}/fill-{index}"),
            open_flags::CREATE | open_flags::TRUNCATE,
            rights::READ | rights::WRITE,
        ) {
            Ok(f) => f,
            Err(s) if refused(s) => {
                println!("diskfill: stopped after {} KiB: create refused ({s})", written >> 10);
                return 42;
            }
            Err(s) => {
                println!("diskfill: create: status {s}");
                return 1;
            }
        };
        for chunk in 0..32u64 {
            match f.write(chunk * BLOCK as u64, 0, &block) {
                Ok(_) => written += BLOCK as u64,
                Err(s) if refused(s) => {
                    println!("diskfill: stopped after {} KiB: refused ({s})", written >> 10);
                    return 42;
                }
                Err(s) => {
                    println!("diskfill: write: status {s}");
                    return 1;
                }
            }
            if written >= MOST {
                println!("diskfill: no limit reached");
                return 1;
            }
        }
        index += 1;
    }
}

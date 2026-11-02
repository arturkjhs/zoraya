#![no_std]
#![no_main]

//! After a crash: `crashcheck <n>`. Files `f0..=n` must be whole, and what
//! exists is a prefix of the writer's order (ADR-0010 rule 3): nothing past a
//! gap, and only the last may be empty, between its create and its write.

use alloc::format;

use zoraya_abi::Handle;
use zoraya_proto_file::status;
use zoraya_user::files::{ALL, File, bytes_of, read_all, status_name};
use zoraya_user::{println, startup};

extern crate alloc;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let durable: i64 = startup()
        .argv
        .next()
        .and_then(|a| core::str::from_utf8(a).ok())
        .and_then(|a| a.parse().ok())
        .unwrap_or(-1);
    let root = File {
        end: Handle::new(5).expect("slot"),
    };
    let mut names = 0u64;
    let listed = root.list(&mut |name, _| {
        if name.starts_with('f') {
            names += 1;
        }
    });
    if listed.is_err() {
        println!("crash: bad: no listing");
        return 1;
    }
    let mut i = 0u64;
    let mut empty = 0;
    loop {
        let file = match root.open(&format!("f{i}"), 0, ALL) {
            Ok(file) => file,
            Err(status::NOT_FOUND) => break,
            Err(s) => {
                println!("crash: bad: f{i}: {}", status_name(s));
                return 1;
            }
        };
        let data = match read_all(&file) {
            Ok(data) => data,
            Err(s) => {
                println!("crash: bad: read f{i}: {}", status_name(s));
                return 1;
            }
        };
        if data.is_empty() {
            empty += 1;
        } else if data != bytes_of(i) {
            println!("crash: bad: f{i} is wrong");
            return 1;
        }
        i += 1;
    }
    if (i as i64) <= durable {
        println!("crash: bad: {i} files, {durable} durable");
        return 1;
    }
    if names != i {
        println!("crash: bad: {names} names, a prefix of {i}");
        return 1;
    }
    if empty > 1 || (empty == 1 && (i as i64 - 1) <= durable) {
        println!("crash: bad: {empty} empty");
        return 1;
    }
    println!("crash: ok {i} files, {durable} durable");
    0
}

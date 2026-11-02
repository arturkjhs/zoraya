#![no_std]
#![no_main]

//! Writes files for the crash test, without end, and says which are durable.
//! `f<i>` holds `bytes(i)` bytes of a pattern. Every eighth file is followed
//! by a `sync`, and only after it returns does the line appear. It starts
//! after what the volume already holds, so a run can follow a crash.

use alloc::format;

use zoraya_abi::Handle;
use zoraya_proto_file::open_flags;
use zoraya_user::files::{ALL, File, bytes_of, status_name};
use zoraya_user::println;

extern crate alloc;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let root = File {
        end: Handle::new(5).expect("slot"),
    };
    println!("crash: start");
    let mut i = 0u64;
    loop {
        let name = format!("f{i}");
        let file = match root.open(&name, open_flags::CREATE, ALL) {
            Ok(file) => file,
            Err(s) => {
                println!("crash: open: {}", status_name(s));
                return 1;
            }
        };
        if let Err(s) = file.write(0, 0, &bytes_of(i)) {
            println!("crash: write: {}", status_name(s));
            return 1;
        }
        drop(file);
        if (i + 1).is_multiple_of(8) {
            if let Err(s) = root.sync() {
                println!("crash: sync: {}", status_name(s));
                return 1;
            }
            println!("crash: durable {i}");
        }
        i += 1;
    }
}

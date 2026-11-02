#![no_std]
#![no_main]

//! Writes files for the crash test, without end, and says which are durable.
//! `f<i>` holds the bytes of `bytes_of(i)`. `crashwriter <n>` syncs after every
//! `n` files, and only after the sync returns does the line appear. With a
//! large `n`, the engine's own commits run between them.

use alloc::format;

use zoraya_abi::Handle;
use zoraya_proto_file::open_flags;
use zoraya_user::files::{ALL, File, bytes_of, status_name};
use zoraya_user::{println, startup};

extern crate alloc;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let root = File {
        end: Handle::new(5).expect("slot"),
    };
    let every = startup()
        .argv
        .next()
        .and_then(|a| core::str::from_utf8(a).ok())
        .and_then(|a| a.parse().ok())
        .unwrap_or(8u64)
        .max(1);
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
        if (i + 1).is_multiple_of(every) {
            if let Err(s) = root.sync() {
                println!("crash: sync: {}", status_name(s));
                return 1;
            }
            println!("crash: durable {i}");
        }
        i += 1;
    }
}

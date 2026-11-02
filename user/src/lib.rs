//! The programs in the initial ramdisk share the runtime.

#![no_std]

extern crate alloc;

pub mod files;
mod heap;

pub use zoraya_rt::*;

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, DEADLINE_INFINITE, DIRECTORY, ExitRecord, Handle, JOB, Mode, Packet, Rights, Status,
    process_signals,
};

/// `program` from the directory this process started with.
pub fn image(program: &str) -> Handle {
    directory_lookup(DIRECTORY, program).expect("program in the ramdisk")
}

/// A handle list entry that duplicates `handle` to `slot` with `rights`.
pub fn dup(slot: u32, handle: Handle, rights: Rights) -> HandleEntry {
    HandleEntry {
        slot,
        handle,
        rights,
        transfer: Transfer::Duplicate,
    }
}

/// The console, for a child to print with.
pub fn console_entry() -> HandleEntry {
    dup(CONSOLE.slot(), CONSOLE, Rights::WRITE)
}

/// Spawns `child` with one argument, its role, in this process's job.
pub fn spawn_role(role: &str, handles: &[HandleEntry], suspended: bool) -> Result<Handle, Status> {
    let mut buffer = [0; 1024];
    let image = image("child");
    let mut e = Encoder::new(&mut buffer, JOB, image);
    if suspended {
        e.suspended();
    }
    for entry in handles {
        e.handle(*entry);
    }
    e.arg(role.as_bytes());
    let length = e.finish().expect("the buffer is large enough");
    let result = spawn(&buffer[..length]);
    handle_close(image).expect("image handle");
    result
}

/// Blocks on a port until `process` ends, and gives its record.
pub fn wait_exit(process: Handle) -> ExitRecord {
    let port = port_create().expect("port");
    port_subscribe(port, process, process_signals::TERMINATED, 1, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default()];
    port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
    handle_close(port).expect("close");
    process_exit_record(process).expect("record")
}

/// Asserts a call's status.
#[macro_export]
macro_rules! assert_err {
    ($call:expr, $status:expr) => {
        match $call {
            Err(status) => assert_eq!(status, $status),
            Ok(_) => panic!("{} succeeded", stringify!($call)),
        }
    };
}

//! The programs in the initial ramdisk share the runtime.

#![no_std]

extern crate alloc;

pub mod app;
pub mod beside;
pub mod command;
pub mod filecmd;
pub mod files;
pub mod gui;
mod heap;
pub mod namespace;
pub mod netclient;
pub mod netconn;
pub mod posix_child;
pub mod random;
pub mod stack;

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

/// The types of the handles this process started with and how many of each,
/// found by looking at every slot (a handle made later has a generation, so
/// its value is not its slot and is not counted). INV-0221-1 is checked with
/// it: `netd` starts with no device, interrupt or pin, and `ethd` with one
/// channel.
pub fn start_table() -> alloc::vec::Vec<(&'static str, u32)> {
    let mut counts: alloc::vec::Vec<(&'static str, u32)> = alloc::vec::Vec::new();
    for slot in 1..u64::from(zoraya_abi::MAX_HANDLES) {
        let Some(handle) = Handle::new(slot) else {
            continue;
        };
        let Ok(kind) = handle_type(handle) else {
            continue;
        };
        match counts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((kind, 1)),
        }
    }
    counts
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

/// The command descriptions, checked when this crate was built (ADR-0156):
/// `(name, text)` for each group under `user/describe/`.
pub mod descriptions {
    include!(concat!(env!("OUT_DIR"), "/descriptions.rs"));
}

/// Puts this program's description, `user/describe/GROUP/NAME.cmd`, into its
/// ELF as the section `.zoraya.command`, which no segment loads (ADR-0156
/// item 2). The ramdisk packer checks that it matches the file. The program
/// reads its own copy, `DESCRIPTION`, through [`command::start`].
#[macro_export]
macro_rules! describe {
    ($group:literal, $name:literal) => {
        const DESCRIPTION: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/describe/",
            $group,
            "/",
            $name,
            ".cmd"
        ));

        #[used]
        #[unsafe(link_section = ".zoraya.command")]
        static ZORAYA_COMMAND: [u8; include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/describe/",
            $group,
            "/",
            $name,
            ".cmd"
        ))
        .len()] = *include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/describe/",
            $group,
            "/",
            $name,
            ".cmd"
        ));
    };
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

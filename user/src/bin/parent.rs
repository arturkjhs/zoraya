#![no_std]
#![no_main]

//! The stage 3 roadmap criterion: a process spawns a child from the ramdisk
//! with an explicit handle list, waits on its exit through a port, and checks
//! its exit record.

use zoraya_abi::{DEADLINE_INFINITE, ExitReason, ExitRecord, Mode, Packet, process_signals};
use zoraya_user::{
    console_entry, port_create, port_subscribe, port_wait, println, process_exit_record, spawn_role,
};

const CHILD: u64 = 0xc417d;

#[unsafe(no_mangle)]
fn main() -> i32 {
    println!("parent: spawning child");
    let child = spawn_role("hello", &[console_entry()], false).expect("spawn");
    let port = port_create().expect("port");
    port_subscribe(port, child, process_signals::TERMINATED, CHILD, Mode::Level)
        .expect("subscribe");
    let mut packets = [Packet::default(); 4];
    let count = port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
    assert_eq!(count, 1);
    assert_eq!(packets[0].key, CHILD);
    assert_eq!(packets[0].signals, process_signals::TERMINATED);
    let record = process_exit_record(child).expect("record");
    assert_eq!(
        record,
        ExitRecord {
            code: 42,
            reason: ExitReason::Exited,
            detail: 0,
            request_kind: None,
        }
    );
    println!("parent: child ended: {record:?}");
    0
}

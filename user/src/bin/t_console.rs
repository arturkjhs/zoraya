#![no_std]
#![no_main]

//! ADR-0060, ADR-0061 and ADR-0062 from a process. The test kernel has put
//! `ab` and 254 of `c` in the console's ring. It ends with the number of names it listed.

use zoraya_abi::{
    CONSOLE, DIRECTORY, DirectoryEntry, JOB, Mode, POWER, Packet, Rights, Status, console_signals,
};
use zoraya_user::{
    assert_err, console_dropped, console_read, directory_entry, handle_close, handle_duplicate,
    handle_type, port_create, port_subscribe, port_wait, power_off, power_reset, raw_syscall,
};

/// `console_read`'s number.
const CONSOLE_READ: u64 = 23;

/// INV-0060-2, INV-0060-3.
fn input() {
    let port = port_create().expect("port");
    port_subscribe(port, CONSOLE, console_signals::READABLE, 1, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default()];
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1), "input waits");
    assert_eq!(packets[0].signals, console_signals::READABLE);

    // An address nothing maps loses no input.
    let call = [CONSOLE.raw(), 0x10, 8, 0, 0, 0];
    assert_eq!(raw_syscall(CONSOLE_READ, call).0, Status::BadAddress as u64);

    let write_only = handle_duplicate(CONSOLE, Rights::WRITE).expect("duplicate");
    let mut bytes = [0; 512];
    assert_err!(console_read(write_only, &mut bytes), Status::AccessDenied);
    assert_err!(console_dropped(write_only), Status::AccessDenied);
    handle_close(write_only).expect("close");

    assert_eq!(console_read(CONSOLE, &mut bytes[..1]), Ok(1));
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1), "still one left");
    assert_eq!(console_read(CONSOLE, &mut bytes[1..]), Ok(255));
    assert_eq!(&bytes[..3], b"abc");
    assert_eq!(bytes[255], b'c');
    assert_eq!(bytes[300], 0, "nothing past the bytes taken");
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    assert_eq!(
        console_read(CONSOLE, &mut bytes),
        Ok(0),
        "it does not block"
    );
    handle_close(port).expect("close");
}

/// INV-0061-2, INV-0061-3.
fn listing() -> i32 {
    let mut record = [0; 128];
    let mut cookie = 0;
    let mut seen = 0;
    let mut hello = 0;
    loop {
        let size = directory_entry(DIRECTORY, cookie, &mut record).expect("entry");
        assert_ne!(size, 0);
        let entry = DirectoryEntry::decode(&record[..size]).expect("decodes");
        seen += 1;
        hello += u32::from(entry.name == b"hello");
        if entry.next == 0 {
            break;
        }
        cookie = entry.next;
    }
    assert!(seen > 10, "{seen}");
    assert_eq!(hello, 1, "each name once");
    assert_eq!(directory_entry(DIRECTORY, 100_000, &mut record), Ok(0));
    assert_err!(
        directory_entry(DIRECTORY, 0, &mut record[..4]),
        Status::InvalidArgument
    );
    let no_lookup = handle_duplicate(DIRECTORY, Rights::DUPLICATE).expect("duplicate");
    assert_err!(
        directory_entry(no_lookup, 0, &mut record),
        Status::AccessDenied
    );
    handle_close(no_lookup).expect("close");
    seen
}

/// INV-0072-1: each handle's type.
fn types() {
    assert_eq!(handle_type(CONSOLE), Ok("console"));
    assert_eq!(handle_type(JOB), Ok("job"));
    assert_eq!(handle_type(DIRECTORY), Ok("directory"));
    let port = port_create().expect("port");
    assert_eq!(handle_type(port), Ok("port"));
    handle_close(port).expect("close");
    assert_err!(handle_type(port), Status::BadHandle);
}

/// INV-0062-1, INV-0062-2, and INV-0159-1 for `power_reset`: refused by
/// the same handle check, and the machine goes on.
fn power() {
    assert_eq!(power_off(POWER), Status::BadHandle, "no such handle here");
    assert_eq!(power_off(CONSOLE), Status::AccessDenied, "wrong right");
    assert_eq!(power_reset(POWER), Status::BadHandle, "no such handle here");
    assert_eq!(power_reset(CONSOLE), Status::AccessDenied, "wrong right");
    let without = handle_duplicate(CONSOLE, Rights::WRITE).expect("a copy with less");
    assert_eq!(power_reset(without), Status::AccessDenied, "no POWER right");
    let _ = handle_close(without);
}

/// INV-0159-1 (and INV-0062-2) with a `Power` handle that lacks `POWER`:
/// the test kernel starts this program a second time with one, at `POWER`'s
/// slot. Both calls are refused, and the program goes on to return 0.
fn power_without_the_right() -> i32 {
    assert_eq!(
        power_reset(POWER),
        Status::AccessDenied,
        "a Power handle without POWER"
    );
    assert_eq!(
        power_off(POWER),
        Status::AccessDenied,
        "a Power handle without POWER"
    );
    0
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    if handle_type(POWER) == Ok("power") {
        return power_without_the_right();
    }
    input();
    types();
    let names = listing();
    power();
    names
}

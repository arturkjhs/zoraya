#![no_std]
#![no_main]

//! ADR-0060, ADR-0061 and ADR-0062 from a process. The test kernel has put
//! `ab` in the console's ring.

use zoraya_abi::{
    CONSOLE, DIRECTORY, DirectoryEntry, Mode, POWER, Packet, Rights, Status, console_signals,
};
use zoraya_user::{
    assert_err, console_dropped, console_read, directory_entry, handle_close, handle_duplicate,
    port_create, port_subscribe, port_wait, power_off,
};

/// INV-0060-2, INV-0060-3.
fn input() {
    let port = port_create().expect("port");
    port_subscribe(port, CONSOLE, console_signals::READABLE, 1, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default()];
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1), "input waits");
    assert_eq!(packets[0].signals, console_signals::READABLE);

    let write_only = handle_duplicate(CONSOLE, Rights::WRITE).expect("duplicate");
    let mut bytes = [0; 8];
    assert_err!(console_read(write_only, &mut bytes), Status::AccessDenied);
    assert_err!(console_dropped(write_only), Status::AccessDenied);
    handle_close(write_only).expect("close");

    assert_eq!(console_read(CONSOLE, &mut bytes[..1]), Ok(1));
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1), "still one left");
    assert_eq!(console_read(CONSOLE, &mut bytes[1..]), Ok(1));
    assert_eq!(&bytes[..2], b"ab");
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    assert_eq!(
        console_read(CONSOLE, &mut bytes),
        Ok(0),
        "it does not block"
    );
    handle_close(port).expect("close");
}

/// INV-0061-2, INV-0061-3.
fn listing() {
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
}

/// INV-0062-1, INV-0062-2.
fn power() {
    assert_eq!(power_off(POWER), Status::BadHandle, "no such handle here");
    assert_eq!(power_off(CONSOLE), Status::AccessDenied, "wrong right");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    input();
    listing();
    power();
    0
}

#![no_std]
#![no_main]

//! The storage stack from a supervisor's place (TASK-13): what `init` does,
//! with a test around it. Failure paths, hostile clients.

use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, Handle, Mode, ObjectType, Packet, Status,
    channel_signals,
};
use zoraya_proto_file::{self as proto, Failure, failure, open_flags, rights, status};
use zoraya_protocol::{ErrorReply, Flags, Path, decode, encode};
use zoraya_user::files::{ALL, DIRECTORY, File, read_all};
use zoraya_user::stack::{Stack, find_disk, start_stack};
use zoraya_user::{
    channel_create, channel_read, channel_write, clock_monotonic, handle_close, job_kill,
    port_create, port_subscribe, port_wait, println, wait_exit,
};

extern crate alloc;

const DURABLE: &[u8] = b"this reached the device";

fn up(device: (Handle, u32)) -> Stack {
    start_stack(device.0, device.1).expect("the stack starts")
}

fn write_file(root: &File, name: &str, text: &[u8]) {
    let f = root
        .open(name, open_flags::CREATE | open_flags::TRUNCATE, ALL)
        .expect("create");
    f.write(0, 0, text).expect("write");
}

fn contents(root: &File, name: &str) -> Vec<u8> {
    read_all(&root.open(name, 0, ALL).expect("open")).expect("read")
}

/// Waits for the service's report on the supervisor channel.
fn heard(stack: &Stack) -> u32 {
    let port = port_create().expect("port");
    let wanted = channel_signals::READABLE;
    port_subscribe(port, stack.supervisor, wanted, 1, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default()];
    let deadline = clock_monotonic() + 10_000_000_000;
    port_wait(port, deadline, &mut packets).expect("a report in time");
    handle_close(port).expect("close");
    let mut bytes = [0; 64];
    let sizes = channel_read(stack.supervisor, &mut bytes, &mut []).expect("a report");
    let (_, m) = decode::<Failure>(&bytes[..sizes.bytes as usize], 0).expect("a Failure");
    m.reason.0
}

fn after_loss(stack: &Stack) {
    assert_eq!(stack.root.sync(), Err(status::STORAGE_FAILED), "sticky");
    assert_eq!(heard(stack), failure::DEVICE_LOST);
}

/// INV-0054-5, INV-0054-6, INV-0080-1, in each state a driver can die.
fn the_driver_dies(device: (Handle, u32)) {
    // Idle, and clean.
    let stack = up(device);
    write_file(&stack.root, "durable.txt", DURABLE);
    stack.root.sync().expect("sync");
    job_kill(stack.driver.0).expect("kill");
    wait_exit(stack.driver.1);
    after_loss(&stack);
    stack.end();

    // Idle, and dirty: the file may be gone, and the durable one may not.
    let stack = up(device);
    assert_eq!(contents(&stack.root, "durable.txt"), DURABLE, "remounted");
    write_file(&stack.root, "dirty.txt", b"not synced");
    job_kill(stack.driver.0).expect("kill");
    wait_exit(stack.driver.1);
    after_loss(&stack);
    stack.end();

    // During a commit: the request is on its way.
    let stack = up(device);
    assert_eq!(contents(&stack.root, "durable.txt"), DURABLE);
    write_file(&stack.root, "during.txt", b"in a commit");
    let mut out = [0; 32];
    let length = encode(&proto::Sync {}, 1, &mut out).expect("fits");
    channel_write(stack.root.end, &out[..length], &[]).expect("write");
    job_kill(stack.driver.0).expect("kill");
    wait_exit(stack.driver.1);
    // A commit that was waiting reports the write it lost; one that had not
    // started reports the driver. Either way it is reported, and `init`
    // does the same for both.
    let reason = heard(&stack);
    assert!(
        matches!(
            reason,
            failure::DEVICE_WRITE | failure::DEVICE_FLUSH | failure::DEVICE_LOST
        ),
        "{reason}"
    );
    stack.end();

    // During a read of uncached data.
    let stack = up(device);
    let file = stack.root.open("durable.txt", 0, ALL).expect("open");
    let mut out = [0; 64];
    let m = proto::Read {
        offset: 0,
        length: 16,
    };
    let length = encode(&m, 1, &mut out).expect("fits");
    channel_write(file.end, &out[..length], &[]).expect("write");
    job_kill(stack.driver.0).expect("kill");
    wait_exit(stack.driver.1);
    drop(file);
    let reason = heard(&stack);
    assert!(
        matches!(reason, failure::METADATA_READ | failure::DEVICE_LOST),
        "{reason}"
    );
    stack.end();
}

/// ADR-0080: a service that dies leaves a driver that resets and ends.
fn the_service_dies(device: (Handle, u32)) {
    let stack = up(device);
    assert_eq!(contents(&stack.root, "durable.txt"), DURABLE);
    write_file(&stack.root, "service.txt", b"synced");
    stack.root.sync().expect("sync");
    job_kill(stack.service.0).expect("kill");
    wait_exit(stack.service.1);
    assert_eq!(
        stack.root.sync(),
        Err(zoraya_user::files::GONE),
        "PEER_CLOSED"
    );
    wait_exit(stack.driver.1);
    stack.end();
    let stack = up(device);
    assert_eq!(contents(&stack.root, "service.txt"), b"synced");
    stack.end();
}

/// Sends raw bytes, and gives the reply's status.
fn raw(end: Handle, bytes: &[u8], handles: &[ChannelEntry]) -> Option<u32> {
    let port = port_create().expect("port");
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, end, wanted, 1, Mode::Level).expect("subscribe");
    channel_write(end, bytes, handles).expect("write");
    let mut packets = [Packet::default()];
    port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
    handle_close(port).expect("close");
    let mut reply = [0; 256];
    let sizes = channel_read(end, &mut reply, &mut []).ok()?;
    let (_, m) = decode::<ErrorReply>(&reply[..sizes.bytes as usize], 0).ok()?;
    Some(m.status)
}

fn malformed_messages(root: &File) {
    let client = root.clone_as(ALL).expect("clone");
    // Unknown ordinal, and it stays open.
    let mut out = [0; 64];
    let length = encode(&ErrorReply { status: 0 }, 1, &mut out).expect("fits");
    out[4] = 200;
    assert_eq!(
        raw(client.end, &out[..length], &[]),
        Some(status::UNSUPPORTED)
    );
    // A bad body.
    let m = proto::Read {
        offset: 0,
        length: 1,
    };
    let length = encode(&m, 1, &mut out).expect("fits");
    assert_eq!(
        raw(client.end, &out[..length - 4], &[]),
        None,
        "a header that lies about the size ends the connection"
    );
    let other = root.clone_as(ALL).expect("clone");
    out[12..16].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        raw(other.end, &out[..24], &[]),
        Some(status::INVALID_ARGUMENT)
    );
    other.stat().expect("it survives");
    // Noise.
    let noise = root.clone_as(ALL).expect("clone");
    assert_eq!(raw(noise.end, &[0xff; 40], &[]), None);
}

/// A path the client library would refuse, sent anyway: `Path` on the wire.
fn hostile_paths(root: &File) {
    let client = root.clone_as(ALL).expect("clone");
    for bad in [&b"../etc"[..], b"/abs", b"a/../b", b"a\x01b", b"..", b"."] {
        let mut text = [b'a'; 16];
        text[..bad.len()].copy_from_slice(bad);
        let name = core::str::from_utf8(&text[..bad.len()]).expect("ascii");
        let good = core::str::from_utf8(&[b'a'; 16][..bad.len()]).map(alloc::string::String::from);
        let good = good.expect("ascii");
        let (mine, theirs) = channel_create().expect("create");
        let m = proto::Open {
            flags: Flags(open_flags::CREATE),
            rights: Flags(ALL),
            path: Path(&good),
        };
        let mut out = vec![0; 8192];
        let length = encode(&m, 1, &mut out).expect("fits");
        // The path is the last bytes of the message.
        out[length - bad.len()..length].copy_from_slice(name.as_bytes());
        let entry = ChannelEntry {
            handle: theirs,
            rights: ChannelObject::RIGHTS,
            transfer: Transfer::Move,
        };
        assert_eq!(
            raw(client.end, &out[..length], &[entry]),
            Some(status::INVALID_ARGUMENT),
            "{bad:?}"
        );
        handle_close(mine).expect("close");
    }
    // And the library refuses before it sends.
    assert_eq!(
        root.open("../x", 0, ALL).err(),
        Some(status::INVALID_ARGUMENT)
    );
}

/// INV-0055-8 in the running system.
fn too_many_open(root: &File) {
    let mut held = Vec::new();
    let refused = loop {
        match root.clone_as(ALL) {
            Ok(file) => held.push(file),
            Err(s) => break s,
        }
    };
    assert_eq!(refused, status::TOO_MANY_OPEN);
    assert!(held.len() > 800, "{}", held.len());
    root.stat().expect("the connection works");
    held.clear();
    // The service sees the closes in its own time.
    let deadline = clock_monotonic() + 10_000_000_000;
    while root.clone_as(ALL).is_err() {
        assert!(clock_monotonic() < deadline, "the closes free the slots");
    }
}

/// The gate's `service-floods` row, files (ADR-0268 point 3, ADR-0262 item 2,
/// INV-0262-1) through the real `fsd`: a launcher carves an account of `BUDGET`
/// open handles for a room; the room opens without end and stops at the
/// budget with `TooManyOpen`, a second account and home (the session's) keep
/// opening to their own bounds, and the flooder's closes give the room back.
fn account_flood(root: &File) {
    const BUDGET: u32 = 20;
    let mut client = zoraya_fsclient::Client::new(zoraya_user::app::Files);
    let carve = |client: &mut zoraya_fsclient::Client<_>| {
        client
            .clone_account(root.end, ALL, BUDGET)
            .map(|end| File { end })
    };
    let room = carve(&mut client).expect("an account for the room");
    let other = carve(&mut client).expect("an account for another room");
    let mut held = Vec::new();
    let refused = loop {
        match room.clone_as(ALL) {
            Ok(file) => held.push(file),
            Err(s) => break s,
        }
    };
    assert_eq!(refused, status::TOO_MANY_OPEN);
    assert!(
        (BUDGET as usize - 2..=BUDGET as usize).contains(&held.len()),
        "the room opened {} of its {BUDGET}",
        held.len()
    );
    // Nobody else is refused because of it.
    let mut others = Vec::new();
    while others.len() < BUDGET as usize - 2 {
        others.push(other.clone_as(ALL).expect("the other account is whole"));
    }
    let mut home = Vec::new();
    for _ in 0..100 {
        home.push(root.clone_as(ALL).expect("home is whole"));
    }
    root.stat().expect("home answers");
    println!(
        "t_storage: account of {BUDGET}: room stopped at {} open, another account opened {}, home opened {}",
        held.len(),
        others.len(),
        home.len()
    );
    held.clear();
    let deadline = clock_monotonic() + 10_000_000_000;
    while room.clone_as(ALL).is_err() {
        assert!(
            clock_monotonic() < deadline,
            "the closes give the account back"
        );
    }
}

/// INV-0265-5 (ADR-0265 item 9): a `sync` of an untrusted account waits for
/// the next commit the volume makes instead of making one, so a loop of them
/// delays nobody. A carved account (an untrusted one) writes and sends a
/// `sync` it does not wait for: no reply while nothing commits, and none of
/// root's 100 write-and-sync rounds is slow; root's first commit answers it.
/// (Without the fold the volume would commit for the room's `sync` at once and
/// the reply would be there within the first wait.)
fn untrusted_sync_folds(root: &File) {
    let mut client = zoraya_fsclient::Client::new(zoraya_user::app::Files);
    let room = client
        .clone_account(root.end, ALL, 20)
        .map(|end| File { end })
        .expect("an account for the room");
    let f = room
        .open("fold.txt", open_flags::CREATE | open_flags::TRUNCATE, ALL)
        .expect("the room creates a file");
    f.write(0, 0, b"dirty").expect("the room writes");
    let port = port_create().expect("port");
    port_subscribe(port, f.end, channel_signals::READABLE, 1, Mode::Level).expect("subscribe");
    let mut out = [0; 64];
    let length = encode(&proto::Sync {}, 1, &mut out).expect("fits");
    channel_write(f.end, &out[..length], &[]).expect("the room's sync");
    let mut packets = [Packet::default()];
    assert!(
        port_wait(port, clock_monotonic() + 300_000_000, &mut packets).is_err(),
        "the room's sync was answered before any commit: it is not folded"
    );
    let started = clock_monotonic();
    let mut slowest = 0;
    for round in 0..100u8 {
        let t = clock_monotonic();
        let g = root
            .open(
                "fold-home.txt",
                open_flags::CREATE | open_flags::TRUNCATE,
                ALL,
            )
            .expect("home creates");
        g.write(0, 0, &[round; 64]).expect("home writes");
        g.sync().expect("home syncs");
        slowest = slowest.max(clock_monotonic() - t);
    }
    assert!(slowest < 1_000_000_000, "home's sync took {slowest} ns");
    port_wait(port, clock_monotonic() + 10_000_000_000, &mut packets)
        .expect("a commit answers the room's sync");
    let mut reply = [0; 64];
    channel_read(f.end, &mut reply, &mut []).expect("the reply");
    handle_close(port).expect("close");
    println!(
        "t_storage: a room's sync waited for a commit; 100 home syncs took {} ms, slowest {} ms",
        (clock_monotonic() - started) / 1_000_000,
        slowest / 1_000_000
    );
}

/// INV-0055-6: a client that sends and never reads.
fn a_client_that_never_reads(root: &File) {
    let client = root.clone_as(ALL).expect("clone");
    let mut out = [0; 64];
    let length = encode(&proto::GetAttributes {}, 1, &mut out).expect("fits");
    let mut sent = 0;
    loop {
        match channel_write(client.end, &out[..length], &[]) {
            Ok(()) => sent += 1,
            Err(Status::WouldBlock) => break,
            Err(e) => panic!("{e:?}"),
        }
        // Others are served the whole time.
        root.stat().expect("still answered");
        if sent > 500 {
            break;
        }
    }
    root.stat().expect("still answered");
}

/// What a `+w` grant of a directory is (`TREE_RIGHTS` of the shell).
const GRANT: u32 = rights::ALL & !rights::PUBLISH;
/// What the own directory's entry is (`own_connection` of the shell).
const OWN: u32 = GRANT | rights::BIND;
const SOCKET: u32 = rights::READ | rights::WRITE;

fn refused(dir: &File, name: &str, why: &str) {
    match dir.make_socket_node(name, SOCKET) {
        Err(status::ACCESS_DENIED) => println!("t_storage: bind: {why}: refused"),
        other => panic!(
            "t_storage: bind: {why}: expected ACCESS_DENIED, got {:?}",
            other.map(|_| ())
        ),
    }
}

fn made(dir: &File, name: &str, why: &str) {
    match dir.make_socket_node(name, SOCKET) {
        Ok(_) => println!("t_storage: bind: {why}: made"),
        Err(s) => panic!("t_storage: bind: {why}: expected a node, got status {s}"),
    }
}

/// ADR-0466 (INV-0466-1, INV-0466-2) from a raw-call program that does not use
/// `libposix`: the service refuses to make a socket node through a connection
/// that does not hold `BIND`, whatever the program says of itself. It takes the
/// connections the shell takes: `ALL | BIND` for a program's own directory (and a
/// line's `/tmp`), and what a `+w` grant is (`ALL` without `PUBLISH`) for any
/// other directory.
fn socket_nodes_need_bind(root: &File) {
    // The volume's root holds `BIND`; every directory below is opened with the rights asked.
    root.make_directory("plain").expect("plain");
    root.make_directory("own").expect("own");

    // (a) A directory the program can write (a `+w` grant, or a made directory
    // opened with `ALL`): a create is allowed, a socket node is refused.
    let plain = root.open("plain", DIRECTORY, GRANT).expect("plain");
    plain
        .open("f", open_flags::CREATE | open_flags::EXCLUSIVE, GRANT)
        .expect("a file is made there");
    refused(&plain, "s", "a writable directory");
    let all = root.open("plain", DIRECTORY, ALL).expect("plain, all");
    refused(&all, "s", "a directory with every ordinary right");
    // `BIND` cannot be asked for by a connection that has none.
    assert_eq!(
        plain.clone_as(GRANT | rights::BIND).err(),
        Some(status::ACCESS_DENIED)
    );
    assert_eq!(
        plain.open("", DIRECTORY, rights::BIND).err(),
        Some(status::ACCESS_DENIED)
    );
    println!("t_storage: bind: BIND cannot be asked for through a connection without it");

    // (b) The own directory (and the line's `/tmp`, which is the same
    // connection with the same rights): a node is made, once.
    let own = root.open("own", DIRECTORY, OWN).expect("own");
    made(&own, "s", "the own directory");
    assert_eq!(
        own.make_socket_node("s", SOCKET).err(),
        Some(status::EXISTS),
        "the name is taken"
    );
    // A level below, opened with `BIND`, makes one; opened without, does not.
    own.make_directory("sub").expect("sub");
    let sub_bind = own.open("sub", DIRECTORY, OWN).expect("sub with BIND");
    made(&sub_bind, "s", "a level below, opened with BIND");
    let sub_plain = own.open("sub", DIRECTORY, GRANT).expect("sub without BIND");
    refused(&sub_plain, "t", "a level below, opened without BIND");

    // (c) What is derived from the own directory: a clone or a narrowing that
    // does not ask for `BIND` has none; one that does has it only of a
    // connection that holds it, and a child's `READ | CLONE` has none.
    let copy = own.clone_as(GRANT).expect("clone");
    refused(&copy, "c1", "a clone without BIND");
    let child = own.clone_as(rights::READ | rights::CLONE).expect("child");
    refused(&child, "c2", "the child of an own directory (READ | CLONE)");
    assert_eq!(
        child
            .clone_as(rights::READ | rights::CLONE | rights::BIND)
            .err(),
        Some(status::ACCESS_DENIED)
    );
    assert_eq!(
        copy.clone_as(rights::BIND).err(),
        Some(status::ACCESS_DENIED)
    );
    let narrowed = own.narrow("n", GRANT).expect("narrow");
    refused(&narrowed, "n", "a narrowing without BIND");
    let narrowed = own
        .narrow("n", rights::CONNECT | rights::CLONE)
        .expect("narrow to connect");
    refused(&narrowed, "n", "a +sock narrowing (CONNECT | CLONE)");
    assert_eq!(
        copy.narrow("n", GRANT | rights::BIND).err(),
        Some(status::ACCESS_DENIED)
    );
    // A narrowing that keeps `BIND` makes its one name and no other.
    let one = own.narrow("n", OWN).expect("narrow with BIND");
    refused(&one, "other", "another name than the narrowed one");
    made(&one, "n", "the narrowed name, with BIND");
    println!("t_storage: socket nodes need BIND: ok");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let device = find_disk().expect("a block device: is --disk on?");
    the_driver_dies(device);
    the_service_dies(device);
    let stack = up(device);
    malformed_messages(&stack.root);
    hostile_paths(&stack.root);
    too_many_open(&stack.root);
    account_flood(&stack.root);
    untrusted_sync_folds(&stack.root);
    a_client_that_never_reads(&stack.root);
    socket_nodes_need_bind(&stack.root);
    stack.end();
    0
}

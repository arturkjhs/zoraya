#![no_std]
#![no_main]

//! Channels (ADR-0050) from a process: each invariant, and hostile use.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    ChannelEntry, ChannelObject, DIRECTORY, Handle, JOB, MapFlags, MemoryFlags, Mode, ObjectType,
    Packet, Rights, Status, channel_signals,
};
use zoraya_user::{
    assert_err, channel_create, channel_read, channel_write, clock_monotonic, directory_lookup,
    handle_close, handle_duplicate, memory_create, memory_map, port_create, port_subscribe,
    port_wait, println, process_exit_record, raw_syscall, spawn,
};

const MAX: usize = 64 * 1024;
static mut BIG: [u8; MAX + 1] = [0; MAX + 1];

fn big() -> &'static mut [u8] {
    // SAFETY: one thread, and each caller drops the slice before the next.
    unsafe { &mut *core::ptr::addr_of_mut!(BIG) }
}

fn moved(handle: Handle) -> ChannelEntry {
    ChannelEntry {
        handle,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    }
}

fn read_one(end: Handle) -> (u32, Handle) {
    let mut buffer = [0; 16];
    let mut handles = [end; 1];
    let sizes = channel_read(end, &mut buffer, &mut handles).expect("read");
    (sizes.bytes, handles[0])
}

/// INV-0050-1: whole, in order, in each direction.
fn messages_go_whole_and_in_order() {
    let (a, b) = channel_create().expect("create");
    for i in 0..10u8 {
        channel_write(a, &[i; 3], &[]).expect("write");
    }
    channel_write(b, b"back", &[]).expect("write");
    for i in 0..10u8 {
        let mut buffer = [0; 8];
        let sizes = channel_read(b, &mut buffer, &mut []).expect("read");
        assert_eq!(sizes.bytes, 3);
        assert_eq!(&buffer[..3], &[i; 3]);
    }
    assert_eq!(read_one(a).0, 4);
    assert_err_read(b, Status::WouldBlock);
    for h in [a, b] {
        handle_close(h).expect("close");
    }
}

fn assert_err_read(end: Handle, status: Status) {
    let mut buffer = [0; 8];
    match channel_read(end, &mut buffer, &mut []) {
        Err((got, _)) => assert_eq!(got, status),
        Ok(_) => panic!("read succeeded"),
    }
}

/// INV-0050-1: a refused write leaves the handle usable, and the message out.
fn a_failed_write_has_no_effect() {
    let (a, b) = channel_create().expect("create");
    for _ in 0..64 {
        channel_write(a, b"x", &[]).expect("write");
    }
    let (c, d) = channel_create().expect("create");
    assert_err!(channel_write(a, b"y", &[moved(c)]), Status::WouldBlock);
    channel_write(c, b"still mine", &[]).expect("c is still here");
    for _ in 0..64 {
        assert_eq!(read_one(b).0, 1);
    }
    assert_err_read(b, Status::WouldBlock);
    // Peer closed.
    handle_close(a).expect("close");
    assert_err!(channel_write(b, b"y", &[moved(d)]), Status::PeerClosed);
    channel_write(d, b"", &[]).expect("d is still here");
    assert_err_read(b, Status::PeerClosed);
    for h in [b, c, d] {
        handle_close(h).expect("close");
    }
}

/// `BufferTooSmall` reports the sizes and keeps the message.
fn a_short_buffer_keeps_the_message() {
    let (a, b) = channel_create().expect("create");
    let (c, d) = channel_create().expect("create");
    channel_write(a, &[7; 100], &[moved(c)]).expect("write");
    let mut small = [0; 10];
    match channel_read(b, &mut small, &mut []) {
        Err((status, sizes)) => {
            assert_eq!(status, Status::BufferTooSmall);
            assert_eq!((sizes.bytes, sizes.handles), (100, 1));
        }
        Ok(_) => panic!("read succeeded"),
    }
    let mut room = [0; 100];
    match channel_read(b, &mut room, &mut []) {
        Err((status, sizes)) => {
            assert_eq!(status, Status::BufferTooSmall, "no room for the handle");
            assert_eq!(sizes.handles, 1);
        }
        Ok(_) => panic!("read succeeded"),
    }
    let mut handles = [b; 2];
    let sizes = channel_read(b, &mut room, &mut handles).expect("read");
    assert_eq!((sizes.bytes, sizes.handles), (100, 1));
    assert_eq!(room[99], 7);
    channel_write(handles[0], b"through it", &[]).expect("a moved end works");
    assert_eq!(read_one(d).0, 10);
    for h in [a, b, handles[0], d] {
        handle_close(h).expect("close");
    }
}

/// A bad address loses no message, in any buffer.
fn a_bad_address_loses_nothing() {
    let (a, b) = channel_create().expect("create");
    channel_write(a, b"keep", &[]).expect("write");
    let number = zoraya_abi::Syscall::ChannelRead as u64;
    let mut sizes = [0u8; 8];
    let mut good = [0u8; 8];
    let good_address = good.as_mut_ptr() as u64;
    let sizes_address = sizes.as_mut_ptr() as u64;
    let call = |bytes: u64, sizes: u64| raw_syscall(number, [b.raw(), bytes, 8, 0, 0, sizes]).0;
    assert_eq!(call(0x10, sizes_address), Status::BadAddress as u64);
    assert_eq!(call(good_address, 0x10), Status::BadAddress as u64);
    assert_eq!(call(u64::MAX - 3, sizes_address), Status::BadAddress as u64);
    assert_eq!(read_one(b).0, 4, "it is still there");
    let write = zoraya_abi::Syscall::ChannelWrite as u64;
    let (status, _) = raw_syscall(write, [a.raw(), 0x10, 8, 0, 0, 0]);
    assert_eq!(status, Status::BadAddress as u64);
    assert_err_read(b, Status::WouldBlock);
    for h in [a, b] {
        handle_close(h).expect("close");
    }
}

/// Malformed lists and sizes.
fn malformed_requests() {
    let (a, b) = channel_create().expect("create");
    let bad = Status::InvalidArgument;
    assert_err!(channel_write(a, &big()[..MAX + 1], &[]), bad);
    channel_write(a, &big()[..MAX], &[]).expect("the largest message");
    assert_eq!(
        channel_read(b, big(), &mut []).expect("read").bytes as usize,
        MAX
    );
    let write = zoraya_abi::Syscall::ChannelWrite as u64;
    let body = [0u8; 8];
    let list = [0u8; 24];
    let raw = |length: u64| {
        raw_syscall(
            write,
            [
                a.raw(),
                body.as_ptr() as u64,
                0,
                list.as_ptr() as u64,
                length,
                0,
            ],
        )
        .0
    };
    assert_eq!(raw(15), bad as u64, "not whole entries");
    assert_eq!(raw(16 * 65), bad as u64, "too many handles");
    assert_eq!(raw(16), Status::BadHandle as u64, "a zero handle");
    let same = ChannelEntry {
        handle: b,
        rights: Rights::NONE,
        transfer: Transfer::Duplicate,
    };
    assert_err!(channel_write(a, b"", &[same]), Status::AccessDenied);
    let (c, d) = channel_create().expect("create");
    assert_err!(channel_write(a, b"", &[moved(c), moved(c)]), bad);
    let wider = ChannelEntry {
        handle: port_create().expect("port"),
        rights: Rights::DUPLICATE,
        transfer: Transfer::Move,
    };
    assert_err!(channel_write(a, b"", &[wider]), Status::AccessDenied);
    handle_close(wider.handle).expect("close");
    for h in [a, b, c, d] {
        handle_close(h).expect("close");
    }
}

/// INV-0050-2: an end cannot go through itself or its peer, or with handles
/// queued for it.
fn the_no_cycle_rule() {
    let (a, b) = channel_create().expect("create");
    assert_err!(channel_write(a, b"", &[moved(a)]), Status::BadState);
    assert_err!(channel_write(a, b"", &[moved(b)]), Status::BadState);
    let (c, d) = channel_create().expect("create");
    let (e, f) = channel_create().expect("create");
    channel_write(d, b"", &[moved(e)]).expect("into c's queue");
    assert_err!(channel_write(a, b"", &[moved(c)]), Status::BadState);
    channel_write(a, b"", &[moved(d)]).expect("d has nothing queued");
    let got = read_one(c).1;
    handle_close(got).expect("close e");
    channel_write(a, b"", &[moved(c)]).expect("now drained");
    let d = read_one(b).1;
    let c = read_one(b).1;
    for h in [a, b, c, d, f] {
        handle_close(h).expect("close");
    }
}

/// INV-0050-4: a process handle in a message closes with the reader.
fn closing_closes_what_is_queued() {
    let (a, b) = channel_create().expect("create");
    let image = directory_lookup(DIRECTORY, "child").expect("child");
    let mut buffer = [0; 256];
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.arg(b"hello");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(image).expect("close");
    channel_write(a, b"", &[moved_process(child)]).expect("write");
    let memory = memory_create(4096, MemoryFlags::NONE).expect("memory");
    let at = memory_map(memory, 0, 4096, MapFlags::READ).expect("map");
    let _ = at;
    channel_write(
        a,
        b"",
        &[ChannelEntry {
            handle: memory,
            rights: Rights::READ,
            transfer: Transfer::Move,
        }],
    )
    .expect("write");
    // Several handles in one message: each is closed, not only the first.
    let (second_child, tail_peer) = {
        let (tail, peer) = channel_create().expect("create");
        let image = directory_lookup(DIRECTORY, "child").expect("child");
        let length = {
            let mut e = Encoder::new(&mut buffer, JOB, image);
            e.arg(b"hello");
            e.finish().expect("fits")
        };
        let second = spawn(&buffer[..length]).expect("spawn");
        handle_close(image).expect("close");
        channel_write(a, b"", &[moved_process(second), moved(tail)]).expect("write");
        (second, peer)
    };
    handle_close(b).expect("close the reader");
    assert_err_read(tail_peer, Status::PeerClosed);
    handle_close(tail_peer).expect("close");
    assert_err!(process_exit_record(second_child), Status::BadHandle);
    assert_err!(process_exit_record(child), Status::BadHandle);
    assert_err!(handle_close(memory), Status::BadHandle);
    handle_close(a).expect("close");
}

fn moved_process(handle: Handle) -> ChannelEntry {
    ChannelEntry {
        handle,
        rights: Rights::WAIT.union(Rights::TRANSFER),
        transfer: Transfer::Move,
    }
}

/// INV-0050-6: a chain far deeper than a stack allows, closed from its head.
fn a_deep_chain_closes() {
    let (head, mut peer) = channel_create().expect("create");
    for _ in 0..20_000 {
        let (z, next) = channel_create().expect("create");
        channel_write(peer, b"", &[moved(z)]).expect("write");
        handle_close(peer).expect("close");
        peer = next;
    }
    let start = clock_monotonic();
    handle_close(head).expect("close the head");
    // INV-0050-6: interrupts stay masked for this long.
    println!(
        "t_channels: closing a chain of 20000 took {} us",
        (clock_monotonic() - start) / 1000
    );
    assert_err!(channel_write(peer, b"", &[]), Status::PeerClosed);
    handle_close(peer).expect("close");
}

/// ADR-0050: a full table is `LimitReached`, and the message stays.
fn a_full_table_keeps_the_message() {
    let (a, b) = channel_create().expect("create");
    let (c, d) = channel_create().expect("create");
    channel_write(a, b"held", &[moved(c)]).expect("write");
    let mut fill = [a; 1024];
    let mut filled = 0;
    while let Ok(port) = port_create() {
        fill[filled] = port;
        filled += 1;
    }
    let mut body = [0; 8];
    let mut handles = [a; 1];
    match channel_read(b, &mut body, &mut handles) {
        Err((status, _)) => assert_eq!(status, Status::LimitReached),
        Ok(_) => panic!("read succeeded"),
    }
    for port in &fill[..filled] {
        handle_close(*port).expect("close");
    }
    let sizes = channel_read(b, &mut body, &mut handles).expect("it stayed");
    assert_eq!((sizes.bytes, sizes.handles), (4, 1));
    for h in [a, b, handles[0], d] {
        handle_close(h).expect("close");
    }
}

/// Signals, through a port.
fn signals_reach_a_port() {
    let (a, b) = channel_create().expect("create");
    let port = port_create().expect("port");
    let all = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, b, all, 1, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default()];
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    channel_write(a, b"x", &[]).expect("write");
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1));
    assert_eq!(packets[0].signals & all, channel_signals::READABLE);
    assert_eq!(read_one(b).0, 1);
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    handle_close(a).expect("close");
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1));
    assert_eq!(packets[0].signals & all, channel_signals::PEER_CLOSED);
    handle_close(b).expect("close");
    handle_close(port).expect("close");
}

fn rights_are_checked() {
    let (a, b) = channel_create().expect("create");
    assert_err!(handle_duplicate(a, Rights::READ), Status::AccessDenied);
    let (c, d) = channel_create().expect("create");
    let narrow = ChannelEntry {
        handle: c,
        rights: Rights::READ,
        transfer: Transfer::Move,
    };
    channel_write(a, b"", &[narrow]).expect("a narrower move");
    let c = read_one(b).1;
    assert_err!(channel_write(c, b"", &[]), Status::AccessDenied);
    assert_eq!(read_one_empty(c), Status::WouldBlock);
    for h in [a, b, c, d] {
        handle_close(h).expect("close");
    }
}

fn read_one_empty(end: Handle) -> Status {
    let mut buffer = [0; 1];
    match channel_read(end, &mut buffer, &mut []) {
        Ok(_) => Status::Ok,
        Err((status, _)) => status,
    }
}

/// A channel between two processes, with the end given at spawn.
fn across_a_spawn() {
    let (a, b) = channel_create().expect("create");
    let image = directory_lookup(DIRECTORY, "child").expect("child");
    let mut buffer = [0; 256];
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.handle(HandleEntry {
        slot: 3,
        handle: b,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    });
    e.arg(b"channel-echo");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(image).expect("close");
    let port = port_create().expect("port");
    port_subscribe(port, a, channel_signals::READABLE, 1, Mode::Level).expect("subscribe");
    channel_write(a, b"ping", &[]).expect("write");
    let mut packets = [Packet::default()];
    port_wait(port, zoraya_abi::DEADLINE_INFINITE, &mut packets).expect("wait");
    let mut reply = [0; 8];
    let sizes = channel_read(a, &mut reply, &mut []).expect("read");
    assert_eq!(&reply[..sizes.bytes as usize], b"ping");
    for h in [a, port, child] {
        handle_close(h).expect("close");
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    messages_go_whole_and_in_order();
    a_failed_write_has_no_effect();
    a_short_buffer_keeps_the_message();
    a_bad_address_loses_nothing();
    malformed_requests();
    the_no_cycle_rule();
    closing_closes_what_is_queued();
    a_deep_chain_closes();
    a_full_table_keeps_the_message();
    signals_reach_a_port();
    rights_are_checked();
    across_a_spawn();
    0
}

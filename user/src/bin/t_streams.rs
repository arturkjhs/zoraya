#![no_std]
#![no_main]

//! Byte streams (ADR-0101) from a process: each invariant, hostile use, and
//! the transfer rate. It spawns itself for the roles that need a second
//! process; the first argument names the role.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelEntry, ChannelObject, DEADLINE_INFINITE, Handle, JOB, MapFlags, MemoryFlags,
    Mode, ObjectType, Packet, Rights, STREAM_ATOMIC, STREAM_CAPACITY, Status, StreamReadObject,
    StreamWriteObject, process_signals, stream_signals,
};
use zoraya_user::{
    assert_err, channel_create, channel_read, channel_write, clock_monotonic, console_entry,
    handle_close, handle_duplicate, handle_type, image, memory_create, memory_map,
    object_get_signals, port_create, port_subscribe, port_wait, println, raw_syscall, sleep_until,
    spawn, startup, stream_create, stream_read, stream_write, wait_exit,
};

const CAPACITY: usize = STREAM_CAPACITY;
const ATOMIC: usize = STREAM_ATOMIC;

static mut BIG: [u8; CAPACITY + ATOMIC] = [0; CAPACITY + ATOMIC];

fn big() -> &'static mut [u8] {
    // SAFETY: one thread, and each caller drops the slice before the next.
    unsafe { &mut *core::ptr::addr_of_mut!(BIG) }
}

fn signals(h: Handle) -> u32 {
    object_get_signals(h).expect("signals")
}

/// Reads until the stream is empty, and gives the count.
fn drain(read: Handle) -> usize {
    let mut total = 0;
    loop {
        match stream_read(read, big()) {
            Ok(0) | Err(Status::WouldBlock) => return total,
            Ok(n) => total += n,
            Err(e) => panic!("read: {e:?}"),
        }
    }
}

fn close(handles: &[Handle]) {
    for h in handles {
        handle_close(*h).expect("close");
    }
}

/// Spawns this program as `role` with `handles`, in this process's job.
fn spawn_role(role: &str, handles: &[HandleEntry]) -> Handle {
    let mut buffer = [0; 1024];
    let program = image("t_streams");
    let mut e = Encoder::new(&mut buffer, JOB, program);
    e.handle(console_entry());
    for entry in handles {
        e.handle(*entry);
    }
    e.arg(role.as_bytes());
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    child
}

fn moved(slot: u32, handle: Handle, rights: Rights) -> HandleEntry {
    HandleEntry {
        slot,
        handle,
        rights,
        transfer: Transfer::Move,
    }
}

fn a_new_stream_has_two_typed_ends() {
    let (read, write) = stream_create().expect("create");
    assert_eq!(handle_type(read), Ok("stream-read"));
    assert_eq!(handle_type(write), Ok("stream-write"));
    assert_eq!(signals(read), 0);
    assert_eq!(signals(write), stream_signals::WRITABLE);
    assert_eq!(stream_write(write, b"hello"), Ok(5));
    assert_eq!(signals(read), stream_signals::READABLE);
    let mut buffer = [0; 16];
    assert_eq!(stream_read(read, &mut buffer), Ok(5));
    assert_eq!(&buffer[..5], b"hello");
    assert_eq!(stream_write(write, b""), Ok(0));
    assert_eq!(stream_read(read, &mut []), Ok(0), "a read of nothing");
    close(&[read, write]);
}

/// INV-0101-3: on one CPU with nothing else to run, an empty read and a full
/// write each come back at once with `WouldBlock`.
fn transfers_never_block() {
    let (read, write) = stream_create().expect("create");
    let mut buffer = [0; 8];
    let start = clock_monotonic();
    assert_err!(stream_read(read, &mut buffer), Status::WouldBlock);
    assert_eq!(
        stream_write(write, &big()[..CAPACITY + ATOMIC]),
        Ok(CAPACITY)
    );
    assert_err!(stream_write(write, b"x"), Status::WouldBlock);
    assert_err!(
        stream_write(write, &big()[..ATOMIC + 1]),
        Status::WouldBlock
    );
    assert_err!(stream_write(write, &big()[..ATOMIC]), Status::WouldBlock);
    let elapsed = clock_monotonic() - start;
    assert!(elapsed < 1_000_000_000, "{elapsed} ns");
    assert_eq!(signals(write), 0, "full: not WRITABLE");
    assert_eq!(drain(read), CAPACITY);
    close(&[read, write]);
}

/// INV-0101-1 in the kernel: a small write with less than `ATOMIC` free
/// writes nothing; a large one writes what fits.
fn a_small_write_is_whole_or_nothing() {
    let (read, write) = stream_create().expect("create");
    assert_eq!(
        stream_write(write, &big()[..CAPACITY - ATOMIC + 1]),
        Ok(CAPACITY - ATOMIC + 1)
    );
    assert_err!(stream_write(write, &big()[..10]), Status::WouldBlock);
    assert_err!(stream_write(write, &big()[..ATOMIC]), Status::WouldBlock);
    assert_eq!(
        drain(read),
        CAPACITY - ATOMIC + 1,
        "nothing of the refused writes"
    );
    // Exactly ATOMIC free: an ATOMIC write goes whole.
    assert_eq!(
        stream_write(write, &big()[..CAPACITY - ATOMIC]),
        Ok(CAPACITY - ATOMIC)
    );
    assert_eq!(signals(write), stream_signals::WRITABLE);
    let record = [0x5a; ATOMIC];
    assert_eq!(stream_write(write, &record), Ok(ATOMIC));
    assert_eq!(drain(read), CAPACITY);
    // A larger write with some room takes part.
    assert_eq!(
        stream_write(write, &big()[..CAPACITY - 100]),
        Ok(CAPACITY - 100)
    );
    assert_eq!(stream_write(write, &big()[..ATOMIC + 1]), Ok(100));
    assert_eq!(drain(read), CAPACITY);
    close(&[read, write]);
}

fn peer_closed_on(h: Handle) -> bool {
    signals(h) & stream_signals::PEER_CLOSED != 0
}

/// Waits on a port for `bits` on `h`, with a deadline of `ms`.
fn wait_for(h: Handle, bits: u32, ms: u64) -> Result<u32, Status> {
    let port = port_create().expect("port");
    port_subscribe(port, h, bits, 7, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default()];
    let result = port_wait(port, clock_monotonic() + ms * 1_000_000, &mut packets);
    handle_close(port).expect("close");
    result.map(|_| packets[0].signals)
}

/// INV-0101-2: duplicates in each closing order; returns and signals before
/// and after the last close.
fn the_last_handle_of_an_end_closes_it() {
    for order in 0..2 {
        let (read, write) = stream_create().expect("create");
        let write2 = handle_duplicate(write, StreamWriteObject::RIGHTS).expect("dup");
        let read2 = handle_duplicate(read, StreamReadObject::RIGHTS).expect("dup");
        stream_write(write2, b"ab").expect("write");
        let (first, last) = if order == 0 {
            (write, write2)
        } else {
            (write2, write)
        };
        handle_close(first).expect("close");
        assert!(!peer_closed_on(read), "one writer is left");
        assert_eq!(
            wait_for(read, stream_signals::PEER_CLOSED, 0),
            Err(Status::DeadlineExceeded)
        );
        let mut buffer = [0; 4];
        assert_eq!(stream_read(read2, &mut buffer[..1]), Ok(1));
        stream_write(last, b"c").expect("the last writer works");
        handle_close(last).expect("close");
        assert!(peer_closed_on(read) && peer_closed_on(read2));
        let both = stream_signals::READABLE | stream_signals::PEER_CLOSED;
        assert_eq!(wait_for(read, both, 1000), Ok(both));
        assert_eq!(stream_read(read, &mut buffer), Ok(2), "the data first");
        assert_eq!(&buffer[..2], b"bc");
        assert_eq!(stream_read(read, &mut buffer), Ok(0), "then end of file");
        assert_eq!(stream_read(read2, &mut buffer), Ok(0));
        close(&[read, read2]);

        let (read, write) = stream_create().expect("create");
        let read2 = handle_duplicate(read, StreamReadObject::RIGHTS).expect("dup");
        let (first, last) = if order == 0 {
            (read, read2)
        } else {
            (read2, read)
        };
        handle_close(first).expect("close");
        assert!(!peer_closed_on(write));
        assert_eq!(stream_write(write, b"x"), Ok(1));
        handle_close(last).expect("close");
        assert_eq!(signals(write), stream_signals::PEER_CLOSED);
        assert_err!(stream_write(write, b"y"), Status::PeerClosed);
        assert_err!(stream_write(write, b""), Status::PeerClosed);
        close(&[write]);
    }
}

/// INV-0101-2: an end in flight in a channel message is held.
fn an_end_in_a_message_keeps_the_peer_open() {
    let (read, write) = stream_create().expect("create");
    let (a, b) = channel_create().expect("channel");
    let entry = ChannelEntry {
        handle: write,
        rights: StreamWriteObject::RIGHTS,
        transfer: Transfer::Move,
    };
    channel_write(a, b"", &[entry]).expect("send the write end");
    assert!(!peer_closed_on(read), "the message holds it");
    let mut got = [read; 1];
    channel_read(b, &mut [], &mut got).expect("receive");
    assert_eq!(stream_write(got[0], b"through"), Ok(7));
    handle_close(got[0]).expect("close");
    assert!(peer_closed_on(read));
    // An end in a message no one reads goes when the channel goes.
    let (read2, write2) = stream_create().expect("create");
    let entry = ChannelEntry {
        handle: write2,
        rights: StreamWriteObject::RIGHTS,
        transfer: Transfer::Move,
    };
    channel_write(a, b"", &[entry]).expect("send");
    close(&[b]);
    assert!(
        peer_closed_on(read2),
        "closing the reader closed the message"
    );
    let mut buffer = [0; 8];
    assert_eq!(stream_read(read, &mut buffer), Ok(7));
    close(&[a, read, read2]);
}

/// INV-0101-2: a writer waiting on a full stream wakes when the last reader,
/// in another process, goes.
fn a_blocked_writer_wakes_when_the_reader_goes() {
    let (read, write) = stream_create().expect("create");
    stream_write(write, &big()[..CAPACITY]).expect("fill");
    let child = spawn_role(
        "hold-read-then-exit",
        &[moved(3, read, StreamReadObject::RIGHTS)],
    );
    assert!(!peer_closed_on(write), "the child holds the reader");
    let bits = stream_signals::WRITABLE | stream_signals::PEER_CLOSED;
    let start = clock_monotonic();
    assert_eq!(
        wait_for(write, bits, 10_000),
        Ok(stream_signals::PEER_CLOSED)
    );
    assert!(
        clock_monotonic() - start >= 10_000_000,
        "it waited for the child"
    );
    assert_err!(stream_write(write, b"x"), Status::PeerClosed);
    assert_eq!(wait_exit(child).code, 0);
    close(&[write, child]);
}

/// INV-0040-2: each call needs its right, on its own type.
fn each_call_needs_its_right() {
    let (read, write) = stream_create().expect("create");
    let mut buffer = [0; 4];
    let denied = Status::AccessDenied;
    assert_err!(stream_read(write, &mut buffer), denied);
    assert_err!(stream_write(read, b"x"), denied);
    let read_wait = handle_duplicate(read, Rights::WAIT).expect("dup");
    let write_wait = handle_duplicate(write, Rights::WAIT).expect("dup");
    assert_err!(stream_read(read_wait, &mut buffer), denied);
    assert_err!(stream_write(write_wait, b"x"), denied);
    let read_only = handle_duplicate(read, Rights::READ).expect("dup");
    assert_err!(object_get_signals(read_only), denied);
    assert_err!(handle_duplicate(read_only, Rights::READ), denied);
    let (a, b) = channel_create().expect("channel");
    assert_err!(stream_read(a, &mut buffer), Status::BadHandle);
    assert_err!(stream_write(b, b"x"), Status::BadHandle);
    // A port has WAIT and no signals; other kinds lack WAIT, which is
    // checked first.
    let memory = port_create().expect("port");
    assert_err!(object_get_signals(memory), Status::BadHandle); // not waitable
    let other = memory_create(4096, MemoryFlags::NONE).expect("memory");
    assert_err!(object_get_signals(other), Status::AccessDenied);
    handle_close(other).expect("close");
    close(&[read, write, read_wait, write_wait, read_only, a, b, memory]);
    assert_err!(stream_write(write, b"x"), Status::BadHandle); // written after close
    assert_err!(stream_read(read, &mut buffer), Status::BadHandle);
    let _ = ChannelObject::RIGHTS;
}

/// Hostile buffers: nothing moves, nothing panics.
fn bad_buffers_move_nothing() {
    let (read, write) = stream_create().expect("create");
    let unmapped = 0x10_0000_0000u64;
    let call = |n, h: Handle, address: u64, length: u64| {
        raw_syscall(n, [h.raw(), address, length, 0, 0, 0]).0
    };
    const READ: u64 = 43;
    const WRITE: u64 = 44;
    let bad = Status::BadAddress as u64;
    assert_eq!(call(WRITE, write, unmapped, 10), bad);
    assert_eq!(call(WRITE, write, u64::MAX - 4, 10), bad, "wraps");
    assert_eq!(call(WRITE, write, 1 << 47, 1), bad, "kernel half");
    assert_eq!(call(WRITE, write, 0x40_0000, u64::MAX), bad, "huge count");
    assert_eq!(
        call(WRITE, write, unmapped, 1 << 40),
        bad,
        "huge and unmapped"
    );
    assert_eq!(signals(read), 0, "no bytes went in");
    // A huge read of an empty stream would block; of a full one, faults.
    stream_write(write, b"abc").expect("write");
    assert_eq!(call(READ, read, unmapped, 10), bad);
    assert_eq!(call(READ, read, 0x40_0000, u64::MAX), bad);
    let text = memory_create(4096, MemoryFlags::NONE).expect("memory");
    let ro = memory_map(text, 0, 4096, MapFlags::READ).expect("map");
    assert_eq!(call(READ, read, ro, 3), bad, "read-only target");
    let mut buffer = [0; 3];
    assert_eq!(
        stream_read(read, &mut buffer),
        Ok(3),
        "a failed read lost nothing"
    );
    assert_eq!(&buffer, b"abc");
    // `stream_create`'s record: wrong size, bad address.
    const CREATE: u64 = 42;
    let mut record = [0u8; 16];
    let at = record.as_mut_ptr() as u64;
    assert_eq!(
        raw_syscall(CREATE, [at, 15, 0, 0, 0, 0]).0,
        Status::InvalidArgument as u64
    );
    assert_eq!(raw_syscall(CREATE, [unmapped, 16, 0, 0, 0, 0]).0, bad);
    assert_eq!(raw_syscall(CREATE, [ro, 16, 0, 0, 0, 0]).0, bad);
    core::hint::black_box(&mut record);
    close(&[read, write, text]);
}

/// INV-0040-3: a full table refuses a stream, and takes neither end.
fn a_full_table_refuses_a_stream() {
    let mut held = [Handle::new(1).expect("slot"); 1024];
    let mut count = 0;
    loop {
        match handle_duplicate(CONSOLE, Rights::WRITE) {
            Ok(h) => {
                held[count] = h;
                count += 1;
            }
            Err(Status::LimitReached) => break,
            Err(e) => panic!("{e:?}"),
        }
    }
    // One slot free: still not enough for two ends.
    handle_close(held[count - 1]).expect("close");
    count -= 1;
    assert_err!(stream_create(), Status::LimitReached);
    close(&held[..count]);
    let (read, write) = stream_create().expect("room again");
    close(&[read, write]);
}

/// INV-0101-4's loop: streams made, filled and closed with data in flight.
/// The kernel compares its counts around the whole program.
fn streams_come_and_go() {
    for round in 0..64 {
        let (read, write) = stream_create().expect("create");
        stream_write(write, &big()[..(round * 1031) % CAPACITY + 1]).expect("write");
        if round % 2 == 0 {
            close(&[read, write]);
        } else {
            close(&[write, read]);
        }
    }
}

/// `WNOHANG` (ADR-0104 item 11): a child's `TERMINATED` without waiting.
fn signals_can_be_read_without_waiting() {
    let (read, write) = stream_create().expect("create");
    let child = spawn_role(
        "hold-read-then-exit",
        &[moved(3, read, StreamReadObject::RIGHTS)],
    );
    assert_eq!(
        signals(child) & process_signals::TERMINATED,
        0,
        "still running"
    );
    assert_eq!(wait_exit(child).code, 0);
    assert_eq!(signals(child), process_signals::TERMINATED);
    assert_eq!(
        signals(JOB) & zoraya_abi::job_signals::TERMINATED,
        0,
        "this one lives"
    );
    close(&[write, child]);
}

/// The rate through one stream, writer and reader in one process.
fn measure_the_transfer_rate() {
    let (read, write) = stream_create().expect("create");
    let total = 16 * 1024 * 1024;
    let start = clock_monotonic();
    let mut moved = 0;
    while moved < total {
        let n = stream_write(write, &big()[..CAPACITY]).expect("write");
        let mut got = 0;
        while got < n {
            got += stream_read(read, &mut big()[got..CAPACITY]).expect("read");
        }
        moved += n;
    }
    let ns = (clock_monotonic() - start).max(1);
    let mb_per_s = moved as u64 * 1000 / ns;
    println!("t_streams: transfer {mb_per_s} MB/s ({moved} bytes in {ns} ns)");
    close(&[read, write]);
}

fn role(name: &[u8]) -> i32 {
    match name {
        b"hold-read-then-exit" => {
            sleep_until(clock_monotonic() + 20_000_000);
            0
        }
        _ => 90,
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    if let Some(name) = startup().argv.next() {
        return role(name);
    }
    a_new_stream_has_two_typed_ends();
    transfers_never_block();
    a_small_write_is_whole_or_nothing();
    the_last_handle_of_an_end_closes_it();
    an_end_in_a_message_keeps_the_peer_open();
    a_blocked_writer_wakes_when_the_reader_goes();
    each_call_needs_its_right();
    bad_buffers_move_nothing();
    a_full_table_refuses_a_stream();
    streams_come_and_go();
    signals_can_be_read_without_waiting();
    measure_the_transfer_rate();
    let _ = DEADLINE_INFINITE;
    println!("t_streams: all passed");
    0
}

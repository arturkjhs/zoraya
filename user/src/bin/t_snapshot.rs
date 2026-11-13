#![no_std]
#![no_main]

//! `snapshot_process` (ADR-0102) from a process: the stage 6a criterion (a
//! program snapshots itself and both halves run, each with its own memory and
//! the handles the move list gave it) and every INV-0102 check.
//!
//! The test kernel starts it with the device handles, in a job without the
//! `posix` bit. It checks the refusal there, then runs itself as `inner` in a
//! `posix` job, with that job at slot 2, its own job at `OUTSIDE`, a second
//! `posix` job beside the first at `PEER` and the first device at `DEVICE`.

extern crate alloc;

use alloc::vec::Vec;

use zoraya_abi::quota::JobLimits;
use zoraya_abi::snapshot::{Encoder as SnapshotEncoder, Move};
use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::stream_signals;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DIRECTORY, Disposition, Duplication, ExitReason, Handle, JOB,
    JobFlags, JobObject, MAX_HANDLES, MapFlags, MemoryFlags, MemoryObject, OBJECT_TYPES,
    ObjectType, RequestKind, Rights, Status, StreamReadObject, StreamWriteObject, process_signals,
};
use zoraya_abi::{Mode, Packet};
use zoraya_user::{
    assert_err, channel_create, channel_read, channel_write, console_entry, device_map,
    directory_lookup, dma_pin, dma_unpin, handle_close, handle_duplicate, handle_type, job_create,
    job_get_usage, job_kill, job_set_limits, job_signal, memory_create, memory_map, memory_unmap,
    object_get_signals, port_create, println, process_exit, process_resume,
    process_set_disposition, process_take_request, raw_syscall, snapshot_process, spawn, startup,
    stream_create, stream_read, stream_write, wait_exit,
};
use zoraya_user::{
    clock_monotonic, job_resume, job_suspend, port_subscribe, port_wait, sleep_until,
};

const OUTSIDE: u64 = 9;
/// A `posix` job beside `inner`'s, outside its tree (ADR-0192).
const PEER: u64 = 10;
const DEVICE: u64 = 5;
/// Where `inner` finds a pin `outer` made: a `posix` job cannot make one.
const PIN: u64 = 6;
const SELF_SLOT: u32 = 21;

const FREE_FRAMES: u64 = 0x7006;
const HEAP_LIVE: u64 = 0x700b;
const EVENTS_LIVE: u64 = 0x7014;
const HOLD_FRAMES: u64 = 0x700c;
const RELEASE_FRAMES: u64 = 0x700d;
const SNAPSHOT_NS: u64 = 0x700e;
const SNAPSHOT_PAGES: u64 = 0x700f;
const HANDLE_AT: u64 = 0x7010;
const RIGHTS_AT: u64 = 0x7011;
const GENERATION_AT: u64 = 0x7012;

fn test_call(number: u64, a0: u64) -> u64 {
    raw_syscall(number, [a0, 0, 0, 0, 0, 0]).1
}

fn h(raw: u64) -> Handle {
    Handle::new(raw).expect("not zero")
}

fn close(handles: &[Handle]) {
    for handle in handles {
        handle_close(*handle).expect("close");
    }
}

/// A buffer for `snapshot_process` into `job`.
fn request(job: Handle, moves: &[Move], self_slot: u32, suspended: bool) -> Vec<u8> {
    let mut buffer = alloc::vec![0; 2048];
    let mut e = SnapshotEncoder::new(&mut buffer, job);
    if suspended {
        e.suspended();
    }
    e.self_slot(self_slot);
    for m in moves {
        e.moved(*m);
    }
    let length = e.finish().expect("room");
    buffer.truncate(length);
    buffer
}

fn snap(
    job: Handle,
    moves: &[Move],
    self_slot: u32,
    suspended: bool,
) -> Result<Option<Handle>, Status> {
    snapshot_process(&request(job, moves, self_slot, suspended))
}

/// In a child: ends it with `code`, which the parent checks.
fn child_exit(ok: bool, code: i32) -> ! {
    process_exit(if ok { 0 } else { code })
}

/// Writes all of `bytes` to a stream, which has room.
fn send(write: Handle, bytes: &[u8]) {
    let mut at = 0;
    while at < bytes.len() {
        at += stream_write(write, &bytes[at..]).expect("room in the stream");
    }
}

/// Reads exactly `out.len()` bytes, waiting on a port while the stream is
/// empty, as a reader of a stream that never blocks must (ADR-0101): the
/// writer may not have run yet.
fn receive(read: Handle, out: &mut [u8]) {
    let port = port_create().expect("port");
    let wanted = stream_signals::READABLE | stream_signals::PEER_CLOSED;
    port_subscribe(port, read, wanted, 1, Mode::Level).expect("subscribe");
    let deadline = clock_monotonic() + 30_000_000_000;
    let mut at = 0;
    while at < out.len() {
        match stream_read(read, &mut out[at..]) {
            Ok(0) => panic!("end of file after {at}"),
            Ok(n) => at += n,
            Err(Status::WouldBlock) => {
                let mut packets = [Packet::default()];
                port_wait(port, deadline, &mut packets).expect("the writer wrote in time");
            }
            Err(e) => panic!("read: {e:?}"),
        }
    }
    handle_close(port).expect("close");
}

const PAGES: usize = 16;
static mut PATTERN: [u8; PAGES * 4096] = [0; PAGES * 4096];

fn pattern() -> &'static mut [u8] {
    // SAFETY: one thread; each caller drops the slice before the next.
    unsafe { &mut *core::ptr::addr_of_mut!(PATTERN) }
}

fn fill(bytes: &mut [u8], value: u8) {
    for page in bytes.chunks_mut(4096) {
        page.fill(value);
    }
}

fn all(bytes: &[u8], value: u8) -> bool {
    bytes.iter().all(|&b| b == value)
}

/// The criterion and INV-0102-1: the parent changes every page of its own
/// before the child runs, the child sees the old contents and changes them,
/// and the parent sees its own. A static array, a mapped object and the
/// stack, and the handles the move list gave.
fn both_halves_run_with_their_own_memory() {
    fill(pattern(), 0x11);
    let object = memory_create(4 * 4096, MemoryFlags::NONE).expect("memory");
    let at = memory_map(object, 0, 4 * 4096, MapFlags::WRITE).expect("map");
    // SAFETY: the mapping just made.
    let mapped = unsafe { core::slice::from_raw_parts_mut(at as *mut u8, 4 * 4096) };
    fill(mapped, 0x22);
    let mut on_stack = [0x33u8; 512];
    let (read, write) = stream_create().expect("stream");
    let (mine, theirs) = channel_create().expect("channel");
    let moves = [Move {
        slot: 20,
        handle: theirs,
    }];
    let child = snap(JOB, &moves, SELF_SLOT, true).expect("snapshot");
    let Some(child) = child else {
        // The child: it starts after the parent changed everything.
        let mut ok = all(pattern(), 0x11) && all(mapped, 0x22) && all(&on_stack, 0x33);
        fill(pattern(), 0x44);
        fill(mapped, 0x55);
        on_stack.fill(0x66);
        // The moved end is at slot 20, and works.
        let end = h(20 | test_call(GENERATION_AT, 20) << 16);
        ok &= handle_type(end) == Ok("channel");
        ok &= channel_write(end, b"from the child", &[]).is_ok();
        // The stream end it duplicated reaches the parent.
        ok &= stream_write(write, b"child ran").is_ok();
        core::hint::black_box(&mut on_stack);
        child_exit(ok, 3);
    };
    assert_err!(handle_type(theirs), Status::BadHandle); // moved out of the parent
    fill(pattern(), 0x77);
    fill(mapped, 0x88);
    on_stack.fill(0x99);
    process_resume(child).expect("resume");
    assert_eq!(wait_exit(child).code, 0, "the child saw its own memory");
    assert!(
        all(pattern(), 0x77) && all(mapped, 0x88) && all(&on_stack, 0x99),
        "the parent's are its own"
    );
    let mut message = [0; 32];
    let sizes = channel_read(mine, &mut message, &mut []).expect("the child's message");
    assert_eq!(&message[..sizes.bytes as usize], b"from the child");
    let mut report = [0; 9];
    receive(read, &mut report);
    assert_eq!(&report, b"child ran");
    core::hint::black_box(&mut on_stack);
    memory_unmap(at, 4 * 4096).expect("unmap");
    close(&[child, read, write, mine, object]);
    println!("t_snapshot: both halves ran, each with its own memory and handles");
}

/// The registers right after the `svc`, the vector registers v16 to v31 and
/// `TPIDR_EL0`, as each half sees them.
#[repr(C, align(16))]
struct Dump {
    x: [u64; 32],
    q: [u128; 16],
    tpidr: u64,
    _pad: u64,
}

const DUMP_SIZE: usize = core::mem::size_of::<Dump>();

fn snapshot_with_dump(buffer: &[u8], dump: &mut Dump) -> (u64, u64) {
    let (status, value): (u64, u64);
    // SAFETY: `svc` with the snapshot's number and buffer; every register the
    // block writes is declared, and `x20` points at `dump`, which has room
    // for every store. In the child the same stores go to its own copy.
    unsafe {
        core::arch::asm!(
        "movz x2, #0x0202",
        "movk x2, #0x5a02, lsl #32",
        "movz x3, #0x0303",
        "movk x3, #0x5a03, lsl #32",
        "movz x4, #0x0404",
        "movk x4, #0x5a04, lsl #32",
        "movz x5, #0x0505",
        "movk x5, #0x5a05, lsl #32",
        "movz x6, #0x0606",
        "movk x6, #0x5a06, lsl #32",
        "movz x7, #0x0707",
        "movk x7, #0x5a07, lsl #32",
        "movz x9, #0x0909",
        "movk x9, #0x5a09, lsl #32",
        "movz x10, #0x0a0a",
        "movk x10, #0x5a0a, lsl #32",
        "movz x11, #0x0b0b",
        "movk x11, #0x5a0b, lsl #32",
        "movz x12, #0x0c0c",
        "movk x12, #0x5a0c, lsl #32",
        "movz x13, #0x0d0d",
        "movk x13, #0x5a0d, lsl #32",
        "movz x14, #0x0e0e",
        "movk x14, #0x5a0e, lsl #32",
        "movz x15, #0x0f0f",
        "movk x15, #0x5a0f, lsl #32",
        "movz x16, #0x1010",
        "movk x16, #0x5a10, lsl #32",
        "movz x17, #0x1111",
        "movk x17, #0x5a11, lsl #32",
        "movz x21, #0x1515",
        "movk x21, #0x5a15, lsl #32",
        "movz x22, #0x1616",
        "movk x22, #0x5a16, lsl #32",
        "movz x23, #0x1717",
        "movk x23, #0x5a17, lsl #32",
        "movz x24, #0x1818",
        "movk x24, #0x5a18, lsl #32",
        "movz x25, #0x1919",
        "movk x25, #0x5a19, lsl #32",
        "movz x26, #0x1a1a",
        "movk x26, #0x5a1a, lsl #32",
        "movz x27, #0x1b1b",
        "movk x27, #0x5a1b, lsl #32",
        "movz x28, #0x1c1c",
        "movk x28, #0x5a1c, lsl #32",
        "dup v16.2d, x2",
        "mov v16.d[1], x3",
        "dup v17.2d, x3",
        "mov v17.d[1], x4",
        "dup v18.2d, x4",
        "mov v18.d[1], x5",
        "dup v19.2d, x5",
        "mov v19.d[1], x6",
        "dup v20.2d, x6",
        "mov v20.d[1], x7",
        "dup v21.2d, x7",
        "mov v21.d[1], x9",
        "dup v22.2d, x9",
        "mov v22.d[1], x10",
        "dup v23.2d, x10",
        "mov v23.d[1], x11",
        "dup v24.2d, x11",
        "mov v24.d[1], x12",
        "dup v25.2d, x12",
        "mov v25.d[1], x13",
        "dup v26.2d, x13",
        "mov v26.d[1], x14",
        "dup v27.2d, x14",
        "mov v27.d[1], x15",
        "dup v28.2d, x15",
        "mov v28.d[1], x16",
        "dup v29.2d, x16",
        "mov v29.d[1], x17",
        "dup v30.2d, x17",
        "mov v30.d[1], x21",
        "dup v31.2d, x21",
        "mov v31.d[1], x22",
        "svc #0",
        "str x0, [x20, #0]",
        "str x1, [x20, #8]",
        "str x2, [x20, #16]",
        "str x3, [x20, #24]",
        "str x4, [x20, #32]",
        "str x5, [x20, #40]",
        "str x6, [x20, #48]",
        "str x7, [x20, #56]",
        "str x9, [x20, #72]",
        "str x10, [x20, #80]",
        "str x11, [x20, #88]",
        "str x12, [x20, #96]",
        "str x13, [x20, #104]",
        "str x14, [x20, #112]",
        "str x15, [x20, #120]",
        "str x16, [x20, #128]",
        "str x17, [x20, #136]",
        "str x21, [x20, #168]",
        "str x22, [x20, #176]",
        "str x23, [x20, #184]",
        "str x24, [x20, #192]",
        "str x25, [x20, #200]",
        "str x26, [x20, #208]",
        "str x27, [x20, #216]",
        "str x28, [x20, #224]",
        "str q16, [x20, #256]",
        "str q17, [x20, #272]",
        "str q18, [x20, #288]",
        "str q19, [x20, #304]",
        "str q20, [x20, #320]",
        "str q21, [x20, #336]",
        "str q22, [x20, #352]",
        "str q23, [x20, #368]",
        "str q24, [x20, #384]",
        "str q25, [x20, #400]",
        "str q26, [x20, #416]",
        "str q27, [x20, #432]",
        "str q28, [x20, #448]",
        "str q29, [x20, #464]",
        "str q30, [x20, #480]",
        "str q31, [x20, #496]",
        "mrs x9, tpidr_el0",
        "str x9, [x20, #512]",
            in("x20") dump as *mut Dump,
            in("x8") 45u64,
            inout("x0") buffer.as_ptr() as u64 => status,
            inout("x1") buffer.len() as u64 => value,
            out("x2") _,
            out("x3") _,
            out("x4") _,
            out("x5") _,
            out("x6") _,
            out("x7") _,
            out("x10") _,
            out("x11") _,
            out("x12") _,
            out("x13") _,
            out("x14") _,
            out("x15") _,
            out("x16") _,
            out("x17") _,
            out("x21") _,
            out("x22") _,
            out("x23") _,
            out("x24") _,
            out("x25") _,
            out("x26") _,
            out("x27") _,
            out("x28") _,
            out("x9") _,
            out("v16") _,
            out("v17") _,
            out("v18") _,
            out("v19") _,
            out("v20") _,
            out("v21") _,
            out("v22") _,
            out("v23") _,
            out("v24") _,
            out("v25") _,
            out("v26") _,
            out("v27") _,
            out("v28") _,
            out("v29") _,
            out("v30") _,
            out("v31") _,
            options(nostack),
        )
    };
    (status, value)
}

/// INV-0102-4: the child starts after the `svc` with the parent's registers
/// but x0 and x1, and with an empty request queue.
fn the_child_starts_with_the_parents_registers() {
    let (read, write) = stream_create().expect("stream");
    // A request queued to the parent before the call stays the parent's.
    process_set_disposition(RequestKind::Custom(3), Disposition::Deliver).expect("disposition");
    job_signal(JOB, RequestKind::Custom(3), 1234).expect("signal");
    let tls = 0x7e57_0000_1234_5678u64;
    // SAFETY: nothing in this program uses TPIDR_EL0.
    unsafe { core::arch::asm!("msr tpidr_el0, {}", in(reg) tls) };
    let mut dump = Dump {
        x: [0; 32],
        q: [0; 16],
        tpidr: 0,
        _pad: 0,
    };
    let buffer = request(JOB, &[], 0, false);
    let (status, value) = snapshot_with_dump(&buffer, &mut dump);
    // SAFETY: as above.
    unsafe { core::arch::asm!("msr tpidr_el0, xzr") };
    assert_eq!(status, Status::Ok as u64);
    if value == 0 {
        // The child: its own dump and queue go to the parent.
        let empty = process_take_request() == Err(Status::NotFound);
        // SAFETY: `Dump` is plain data.
        let bytes =
            unsafe { core::slice::from_raw_parts(&dump as *const Dump as *const u8, DUMP_SIZE) };
        send(write, bytes);
        child_exit(empty, 4);
    }
    let child = h(value);
    assert_eq!(wait_exit(child).code, 0, "the child's queue was empty");
    assert_eq!(
        process_take_request(),
        Ok((RequestKind::Custom(3), 1234)),
        "the parent's is there"
    );
    process_set_disposition(RequestKind::Custom(3), Disposition::Terminate).expect("disposition");
    let mut theirs = Dump {
        x: [0; 32],
        q: [0; 16],
        tpidr: 0,
        _pad: 0,
    };
    // SAFETY: `Dump` is plain data, and any bytes are a valid one.
    let bytes =
        unsafe { core::slice::from_raw_parts_mut(&mut theirs as *mut Dump as *mut u8, DUMP_SIZE) };
    receive(read, bytes);
    assert_eq!(theirs.x[0], Status::Ok as u64);
    assert_eq!(theirs.x[1], 0, "the child sees 0");
    for n in [
        2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 16, 17, 21, 22, 23, 24, 25, 26, 27, 28,
    ] {
        assert_eq!(theirs.x[n], dump.x[n], "x{n}");
        assert_ne!(dump.x[n], 0, "x{n} was set");
    }
    assert_eq!(theirs.q, dump.q, "v16 to v31");
    assert_eq!(theirs.tpidr, tls, "TPIDR_EL0");
    assert_eq!(dump.tpidr, tls);
    close(&[child, read, write]);
}

fn duplicable(name: &str) -> bool {
    OBJECT_TYPES
        .iter()
        .any(|t| t.name == name && t.duplication == Duplication::Duplicable)
}

/// INV-0102-6 and -7: slot by slot, the child's table against the parent's.
fn the_childs_table_mirrors_the_parents() {
    // Something of every kind this process can hold, at known slots.
    let port = port_create().expect("port");
    let (keep, moved_end) = channel_create().expect("channel");
    let (unmoved, unmoved_peer) = channel_create().expect("channel");
    let memory = memory_create(4096, MemoryFlags::NONE).expect("memory");
    let no_dup = handle_duplicate(memory, Rights::READ).expect("dup without DUPLICATE");
    let (read, write) = stream_create().expect("stream");
    // A duplicable handle on the move list: it moves, it is not also copied
    // (ADR-0130 item 2).
    let (moved_read, moved_write) = stream_create().expect("stream");
    let moved_to = 700;
    let process_job = job_create(JOB, JobFlags::POSIX).expect("job");
    // A slot closed and reopened, so that its generation is not 0, for the
    // moved end to land in.
    let spare = memory_create(4096, MemoryFlags::NONE).expect("memory");
    let target = spare.slot();
    handle_close(spare).expect("close");
    assert!(test_call(GENERATION_AT, u64::from(target)) > 0);
    let mut before = [(0u64, 0u32); MAX_HANDLES as usize + 1];
    for slot in 1..=MAX_HANDLES {
        before[slot as usize] = (
            test_call(HANDLE_AT, u64::from(slot)),
            test_call(RIGHTS_AT, u64::from(slot)) as u32,
        );
    }
    let generation_of = |slot: u32| test_call(GENERATION_AT, u64::from(slot));
    let moves = [
        Move {
            slot: target,
            handle: moved_end,
        },
        Move {
            slot: moved_to,
            handle: moved_write,
        },
    ];
    // The child inherits this disposition (ADR-0102 item 5): without it the
    // request below would end the child.
    process_set_disposition(RequestKind::Custom(5), Disposition::Deliver).expect("disposition");
    let child = snap(process_job, &moves, SELF_SLOT, false).expect("snapshot");
    let Some(_) = child else {
        // The child sends its table, then proves its self slot is itself.
        let mut values = Vec::new();
        for slot in 1..=MAX_HANDLES {
            values.extend_from_slice(&test_call(HANDLE_AT, u64::from(slot)).to_le_bytes());
            let rights = test_call(RIGHTS_AT, u64::from(slot)) as u32;
            values.extend_from_slice(&rights.to_le_bytes());
        }
        // The parent reads before this writes, as on a slow host (CI under
        // TCG): `receive` must wait, not take an empty stream for an error.
        sleep_until(clock_monotonic() + 20_000_000);
        send(write, &values);
        let me = h(u64::from(SELF_SLOT) | generation_of(SELF_SLOT) << 16);
        let mut ok = handle_type(me) == Ok("process");
        ok &= object_get_signals(me).map(|s| s & process_signals::REQUEST_PENDING) == Ok(0);
        // Its own job holds only it: the request is its own, and queued
        // under the copied disposition.
        ok &= job_signal(process_job, RequestKind::Custom(5), 9).is_ok();
        ok &= object_get_signals(me).map(|s| s & process_signals::REQUEST_PENDING)
            == Ok(process_signals::REQUEST_PENDING);
        ok &= process_take_request() == Ok((RequestKind::Custom(5), 9));
        ok &= object_get_signals(me).map(|s| s & process_signals::TERMINATED) == Ok(0);
        // The moved ends work at their slots.
        let end = h(u64::from(target) | generation_of(target) << 16);
        ok &= channel_write(end, b"moved", &[]).is_ok();
        let stream_end = h(u64::from(moved_to) | generation_of(moved_to) << 16);
        ok &= stream_write(stream_end, b"s").is_ok();
        child_exit(ok, 5);
    };
    let child = child.expect("the parent");
    process_set_disposition(RequestKind::Custom(5), Disposition::Terminate).expect("disposition");
    const ROW: usize = 12;
    let mut values = alloc::vec![0u8; MAX_HANDLES as usize * ROW];
    receive(read, &mut values);
    assert_eq!(wait_exit(child).code, 0, "the child's self slot names it");
    let mut message = [0; 8];
    let sizes = channel_read(keep, &mut message, &mut []).expect("message");
    assert_eq!(&message[..sizes.bytes as usize], b"moved");
    let mut byte = [0; 1];
    assert_eq!(
        stream_read(moved_read, &mut byte),
        Ok(1),
        "through the moved stream end"
    );
    let at_slot = |slot: u32| {
        let at = (slot as usize - 1) * ROW;
        (
            u64::from_le_bytes(values[at..at + 8].try_into().unwrap()),
            u32::from_le_bytes(values[at + 8..at + 12].try_into().unwrap()),
        )
    };
    for slot in 1..=MAX_HANDLES {
        let (got, got_rights) = at_slot(slot);
        let (value, rights) = before[slot as usize];
        let moved_from = |m: &Move| m.slot == slot;
        let expected = if slot == SELF_SLOT {
            (
                u64::from(slot) | generation_of(slot) << 16,
                Rights::WAIT.raw(),
            )
        } else if let Some(m) = moves.iter().find(|m| moved_from(m)) {
            // The full rights the caller held.
            let source = before[m.handle.slot() as usize].1;
            (u64::from(slot) | generation_of(slot) << 16, source)
        } else if moves.iter().any(|m| m.handle.raw() == value) || value == 0 {
            (0, 0)
        } else {
            let kind = handle_type(h(value)).unwrap_or("closed");
            let dup = Rights::from_raw(rights)
                .expect("rights")
                .contains(Rights::DUPLICATE);
            if duplicable(kind) && dup {
                (value, rights)
            } else {
                (0, 0)
            }
        };
        assert_eq!((got, got_rights), expected, "slot {slot}");
    }
    // The ones INV-0102-6 names, by name, and the moved stream end's old
    // slot.
    for empty in [port, unmoved, unmoved_peer, no_dup, moved_write] {
        assert_eq!(
            at_slot(empty.slot()),
            (0, 0),
            "{:?} is not in the child",
            handle_type(empty)
        );
    }
    for present in [memory, read, write, process_job, JOB, DIRECTORY] {
        assert_eq!(
            at_slot(present.slot()).0,
            present.raw(),
            "the parent's value"
        );
    }
    close(&[
        child,
        port,
        keep,
        unmoved,
        unmoved_peer,
        memory,
        no_dup,
        read,
        write,
        moved_read,
        process_job,
    ]);
}

/// INV-0102-2: a refused call leaves every listed handle in the caller; a
/// good one moves them all. Each kind of refusal.
fn the_move_list_is_all_or_nothing() {
    let (a, b) = channel_create().expect("channel");
    let (c, d) = channel_create().expect("channel");
    let still_mine = |hs: &[Handle]| hs.iter().all(|h| handle_type(*h).is_ok());
    let stale = h(b.raw() + (1 << 16));
    let memory = memory_create(4096, MemoryFlags::NONE).expect("memory");
    let no_transfer = handle_duplicate(memory, Rights::READ.union(Rights::DUPLICATE)).expect("dup");
    let cases: &[(&[Move], Status)] = &[
        (
            &[
                Move {
                    slot: 30,
                    handle: b,
                },
                Move {
                    slot: 31,
                    handle: h(999),
                },
            ],
            Status::BadHandle,
        ),
        (
            &[
                Move {
                    slot: 30,
                    handle: b,
                },
                Move {
                    slot: 31,
                    handle: stale,
                },
            ],
            Status::BadHandle,
        ),
        (
            &[
                Move {
                    slot: 30,
                    handle: b,
                },
                Move {
                    slot: 31,
                    handle: no_transfer,
                },
            ],
            Status::AccessDenied,
        ),
    ];
    for (moves, status) in cases {
        assert_err!(snap(JOB, moves, 0, false), *status);
        assert!(still_mine(&[a, b, c, d, memory, no_transfer]));
    }
    // An end with handles queued cannot move (ADR-0050's rule).
    let (e, f) = channel_create().expect("channel");
    let entry = ChannelEntry {
        handle: e,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    };
    channel_write(c, b"", &[entry]).expect("queue an end for d");
    assert_err!(
        snap(
            JOB,
            &[
                Move {
                    slot: 30,
                    handle: b
                },
                Move {
                    slot: 31,
                    handle: d
                }
            ],
            0,
            false
        ),
        Status::BadState
    );
    assert!(still_mine(&[a, b, c, d, f]));
    // A full table: no room for the child's handle.
    let mut filler = Vec::new();
    while let Ok(x) = handle_duplicate(memory, MemoryObject::RIGHTS) {
        filler.push(x);
    }
    assert_err!(
        snap(
            JOB,
            &[Move {
                slot: 30,
                handle: b
            }],
            0,
            false
        ),
        Status::LimitReached
    );
    assert!(still_mine(&[a, b]));
    // Full but for the one slot the child's handle takes: it goes.
    handle_close(filler.pop().expect("one")).expect("close");
    let child = snap(
        JOB,
        &[Move {
            slot: 30,
            handle: b,
        }],
        0,
        false,
    )
    .expect("snapshot");
    let Some(child) = child else {
        let end = h(30 | test_call(GENERATION_AT, 30) << 16);
        child_exit(channel_write(end, b"full", &[]).is_ok(), 6);
    };
    assert_err!(handle_type(b), Status::BadHandle); // moved
    assert_eq!(wait_exit(child).code, 0);
    let mut message = [0; 8];
    channel_read(a, &mut message, &mut []).expect("through the moved end");
    close(&filler);
    close(&[child, a, c, d, f, memory, no_transfer]);
}

/// INV-0102-2 and -3 at every allocation: with fewer and fewer frames left,
/// each refusal gives back every frame and byte of heap, and leaves the
/// listed handles; then enough frames, and it works.
fn frame_exhaustion_gives_everything_back() {
    // A first snapshot grows whatever kernel tables grow once.
    let warm = snap(JOB, &[], 0, false).expect("snapshot");
    let Some(warm) = warm else { process_exit(0) };
    wait_exit(warm);
    handle_close(warm).expect("close");
    let (a, b) = channel_create().expect("channel");
    let frames = test_call(FREE_FRAMES, 0);
    let heap = test_call(HEAP_LIVE, 0);
    let mut refusals = 0;
    // Frames stay held across steps and each step gives one back, so each
    // allocation the call makes meets an empty allocator in turn. With the
    // self slot, a failure after the child's self handle exists is reached.
    test_call(HOLD_FRAMES, 0);
    let child = loop {
        let moves = [Move {
            slot: 30,
            handle: b,
        }];
        let before = test_call(FREE_FRAMES, 0);
        match snap(JOB, &moves, SELF_SLOT, true) {
            Ok(Some(child)) => break child,
            Ok(None) => process_exit(0),
            Err(status) => {
                let expected = matches!(status, Status::NoMemory | Status::LimitReached);
                assert!(expected, "{status:?}");
                assert_eq!(handle_type(b), Ok("channel"), "still the caller's");
                // A refusal gave back all it took.
                assert_eq!(test_call(FREE_FRAMES, 0), before, "frames at {refusals}");
                assert_eq!(test_call(HEAP_LIVE, 0), heap, "heap at {refusals}");
                refusals += 1;
            }
        }
        assert!(refusals < 4096, "never enough frames");
        assert_eq!(test_call(RELEASE_FRAMES, 1), 1, "a frame to give back");
    };
    test_call(RELEASE_FRAMES, 0);
    assert!(
        refusals > 10,
        "{refusals} refusals: the sweep reached the copy"
    );
    assert_err!(handle_type(b), Status::BadHandle);
    process_resume(child).expect("resume");
    assert_eq!(wait_exit(child).code, 0);
    close(&[child, a]);
    assert_eq!(test_call(FREE_FRAMES, 0), frames, "frames after the sweep");
    println!("t_snapshot: {refusals} refusals by frame exhaustion, each gave everything back");
}

/// INV-0102-5: a device mapping or a live pin refuses the snapshot.
fn devices_refuse_a_snapshot() {
    let device = h(DEVICE);
    if handle_type(device) != Ok("device") {
        println!("t_snapshot: no device, INV-0102-5 not exercised");
        return;
    }
    let (at, _) = device_map(device, 0).expect("map the device");
    assert_err!(snap(JOB, &[], 0, false), Status::BadState);
    memory_unmap(at, 4096).expect("unmap");
    // INV-0130-3: no pin is made in a `posix` job.
    let memory = memory_create(4096, MemoryFlags::NONE).expect("memory");
    let mut runs = [[0u64; 2]; 1];
    assert_err!(dma_pin(memory, device, &mut runs), Status::AccessDenied);
    close(&[memory]);
    // INV-0102-5: a pin made outside and handed in refuses the snapshot.
    let pin = h(PIN);
    assert_eq!(handle_type(pin), Ok("pin"), "outer's pin");
    assert_err!(snap(JOB, &[], 0, false), Status::BadState);
    dma_unpin(pin).expect("unpin");
    let child = snap(JOB, &[], 0, false).expect("and without them it works");
    let Some(child) = child else { process_exit(0) };
    wait_exit(child);
    close(&[child]);
}

/// INV-0003-4, INV-0046-2 and the job rules of ADR-0102 item 1 as ADR-0192
/// states them (INV-0192-1): the handle's `CREATE_PROCESS` is the authority,
/// and the target job must have the `posix` bit.
fn the_job_must_be_ours_and_open() {
    let weak =
        handle_duplicate(JOB, JobObject::RIGHTS.without(Rights::CREATE_PROCESS)).expect("dup");
    assert_err!(snap(weak, &[], 0, false), Status::AccessDenied);
    let weak_peer =
        handle_duplicate(h(PEER), JobObject::RIGHTS.without(Rights::CREATE_PROCESS)).expect("dup");
    assert_err!(snap(weak_peer, &[], 0, false), Status::AccessDenied);
    // A job without the `posix` bit is refused, with every right.
    assert_err!(snap(h(OUTSIDE), &[], 0, false), Status::AccessDenied);
    // A `posix` job outside our own tree is not: a fork child's job sits
    // beside its parent's (ADR-0104).
    let child = snap(h(PEER), &[], 0, false).expect("a snapshot into a job beside ours");
    let Some(child) = child else { process_exit(3) };
    assert_eq!(wait_exit(child).code, 3);
    // A job we hold no handle to cannot be named: a handle value is not a
    // job's name, so another session's jobs are out of reach.
    for raw in [200u64, 0xffff, 0x1234_5678] {
        assert_err!(snap(h(raw), &[], 0, false), Status::BadHandle);
    }
    let memory = memory_create(4096, MemoryFlags::NONE).expect("memory");
    assert_err!(snap(memory, &[], 0, false), Status::AccessDenied);
    let port = port_create().expect("port");
    let with_right = handle_duplicate(port, Rights::NONE);
    assert!(with_right.is_err(), "a port cannot be duplicated");
    handle_close(port).expect("close");
    let killed = job_create(JOB, JobFlags::NONE).expect("job");
    job_kill(killed).expect("kill");
    assert_err!(snap(killed, &[], 0, false), Status::BadState);
    // A group outside the job.
    let sibling = job_create(JOB, JobFlags::NONE).expect("job");
    let other = job_create(JOB, JobFlags::NONE).expect("job");
    let mut buffer = [0; 64];
    let length = SnapshotEncoder::new(&mut buffer, sibling)
        .group(other)
        .finish()
        .expect("room");
    assert_err!(snapshot_process(&buffer[..length]), Status::InvalidArgument);
    // The child joins the job it was given: killing that ends it.
    let child = snap(sibling, &[], 0, true).expect("snapshot");
    let Some(child) = child else { process_exit(1) };
    job_kill(sibling).expect("kill");
    assert_eq!(wait_exit(child).reason, ExitReason::Killed);
    close(&[weak, weak_peer, memory, killed, sibling, other, child]);
}

/// Malformed buffers reach the kernel's decoder and fail there.
fn bad_buffers_are_refused() {
    const SNAPSHOT: u64 = 45;
    let good = request(JOB, &[], 0, false);
    let call = |address: u64, length: u64| raw_syscall(SNAPSHOT, [address, length, 0, 0, 0, 0]).0;
    let bad = Status::InvalidArgument as u64;
    assert_eq!(
        call(0x10_0000_0000, good.len() as u64),
        Status::BadAddress as u64
    );
    assert_eq!(call(good.as_ptr() as u64, u64::MAX), bad);
    assert_eq!(call(good.as_ptr() as u64, 1 << 20), bad);
    for length in 0..good.len() {
        assert_eq!(call(good.as_ptr() as u64, length as u64), bad, "{length}");
    }
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..2000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let mut m = good.clone();
        let at = (x % m.len() as u64) as usize;
        m[at] ^= (x >> 32) as u8 | 1;
        let (status, value) =
            raw_syscall(SNAPSHOT, [m.as_ptr() as u64, m.len() as u64, 0, 0, 0, 0]);
        if status == Status::Ok as u64 {
            // A flipped bit can still be a good request (another self slot).
            if value == 0 {
                process_exit(0);
            }
            // The flags may have become START_SUSPENDED.
            let _ = process_resume(h(value));
            wait_exit(h(value));
            handle_close(h(value)).expect("close");
        }
    }
}

/// The leak loop: snapshots whose children exit at once; the kernel compares
/// its counts around the whole program, and this around the loop.
fn snapshots_come_and_go() {
    let frames = test_call(FREE_FRAMES, 0);
    let heap = test_call(HEAP_LIVE, 0);
    for round in 0..32 {
        let child =
            snap(JOB, &[], if round % 2 == 0 { SELF_SLOT } else { 0 }, false).expect("snapshot");
        let Some(child) = child else {
            process_exit(round)
        };
        assert_eq!(wait_exit(child).code, round);
        handle_close(child).expect("close");
    }
    assert_eq!(test_call(FREE_FRAMES, 0), frames, "frames");
    assert_eq!(test_call(HEAP_LIVE, 0), heap, "heap");
}

/// The measurements: the copy's time with interrupts masked, and the frames
/// it charges, for a process of about 1 MiB and about 8 MiB.
fn measure_the_copy() {
    for mib in [1u64, 8] {
        let size = mib << 20;
        let object = memory_create(size, MemoryFlags::NONE).expect("memory");
        let at = memory_map(object, 0, size, MapFlags::WRITE).expect("map");
        let mut worst = 0;
        let mut pages = 0;
        let mut charged = 0;
        for _ in 0..5 {
            let free = test_call(FREE_FRAMES, 0);
            let child = snap(JOB, &[], 0, true).expect("snapshot");
            let Some(child) = child else { process_exit(0) };
            charged = free - test_call(FREE_FRAMES, 0);
            worst = worst.max(test_call(SNAPSHOT_NS, 0));
            pages = test_call(SNAPSHOT_PAGES, 0);
            process_resume(child).expect("resume");
            wait_exit(child);
            handle_close(child).expect("close");
        }
        println!(
            "t_snapshot: {mib} MiB object: {pages} pages copied ({} KiB), {charged} frames charged, worst {} us masked",
            pages * 4,
            worst / 1000
        );
        memory_unmap(at, size).expect("unmap");
        handle_close(object).expect("close");
    }
}

/// ADR-0130 item 7, INV-0130-2: a child made into a suspended job runs no
/// instruction until the job is resumed.
fn a_child_in_a_suspended_job_waits() {
    let (read, write) = stream_create().expect("stream");
    let job = job_create(JOB, JobFlags::POSIX).expect("job");
    job_suspend(job).expect("suspend");
    let child = snap(job, &[], 0, false).expect("snapshot");
    let Some(child) = child else {
        let _ = stream_write(write, b"r");
        process_exit(0);
    };
    sleep_until(clock_monotonic() + 30_000_000);
    let readable = object_get_signals(read).expect("signals") & stream_signals::READABLE;
    assert_eq!(readable, 0, "the child did not run");
    job_resume(job).expect("resume");
    assert_eq!(wait_exit(child).code, 0);
    let mut byte = [0; 1];
    assert_eq!(
        stream_read(read, &mut byte),
        Ok(1),
        "and ran after the resume"
    );
    close(&[child, job, read, write]);
}

fn inner() -> i32 {
    // First: until it ends the pin `outer` handed over, every snapshot is
    // refused.
    devices_refuse_a_snapshot();
    both_halves_run_with_their_own_memory();
    the_child_starts_with_the_parents_registers();
    the_childs_table_mirrors_the_parents();
    the_move_list_is_all_or_nothing();
    frame_exhaustion_gives_everything_back();
    a_child_in_a_suspended_job_waits();
    the_job_must_be_ours_and_open();
    bad_buffers_are_refused();
    snapshots_come_and_go();
    a_snapshot_at_a_limit_is_refused_whole();
    measure_the_copy();
    println!("t_snapshot: all passed");
    0
}

/// ADR-0261 point 6, INV-0261-2 and INV-0263-1: a snapshot into a job at its
/// process limit is refused with `OverQuota` and has no effect (no frame, no
/// heap, no event source, no handle moved), and a job at its memory limit
/// refuses a copy it cannot hold, also with no effect.
fn a_snapshot_at_a_limit_is_refused_whole() {
    let room = job_create(JOB, JobFlags::POSIX).expect("job");
    job_set_limits(
        room,
        &JobLimits {
            memory: None,
            processes: Some(1),
            cpu: None,
            threads: None,
        },
    )
    .expect("limits");
    // The one slot goes to a suspended child.
    let first = snap(room, &[], 0, true).expect("first");
    let Some(first) = first else { process_exit(0) };
    let (frames, heap, events) = (
        test_call(FREE_FRAMES, 0),
        test_call(HEAP_LIVE, 0),
        test_call(EVENTS_LIVE, 0),
    );
    let before = job_get_usage(room, false).expect("usage");
    let object = memory_create(4096, MemoryFlags::NONE).expect("memory");
    assert_err!(
        snap(
            room,
            &[Move {
                handle: object,
                slot: 9
            }],
            0,
            false
        ),
        Status::OverQuota
    );
    assert_eq!(test_call(FREE_FRAMES, 0) + 1, frames, "frames");
    assert!(test_call(HEAP_LIVE, 0) <= heap + 4096, "heap");
    assert_eq!(test_call(EVENTS_LIVE, 0), events, "event sources");
    let after = job_get_usage(room, false).expect("usage");
    assert_eq!(after.processes.used, before.processes.used);
    assert_eq!(after.processes.refused, before.processes.refused + 1);
    assert_eq!(after.memory.used, before.memory.used, "the room's use");
    // The moved handle is still ours: it was not moved.
    handle_duplicate(object, Rights::READ).expect("the object is still held");
    // Memory: a room whose limit is its use cannot hold the copy.
    let tight = job_create(JOB, JobFlags::POSIX).expect("job");
    let size = 1 << 20;
    let big = memory_create(size, MemoryFlags::NONE).expect("memory");
    let at = memory_map(big, 0, size, MapFlags::WRITE).expect("map");
    job_set_limits(
        tight,
        &JobLimits {
            memory: Some(64 << 10),
            processes: None,
            cpu: None,
            threads: None,
        },
    )
    .expect("limits");
    let (frames, heap) = (test_call(FREE_FRAMES, 0), test_call(HEAP_LIVE, 0));
    let used = job_get_usage(tight, false).expect("u").memory.used;
    assert_err!(snap(tight, &[], 0, false), Status::OverQuota);
    assert_eq!(test_call(FREE_FRAMES, 0), frames, "frames after the copy");
    assert!(
        test_call(HEAP_LIVE, 0) <= heap + 4096,
        "heap after the copy"
    );
    assert_eq!(job_get_usage(tight, false).expect("u").memory.used, used);
    memory_unmap(at, size).expect("unmap");
    job_kill(room).expect("kill");
    wait_exit(first);
    close(&[first, object, big, tight, room]);
}

/// INV-0003-2 and INV-0002-2: refused without the `posix` bit; then the rest
/// in a `posix` job.
fn outer() -> i32 {
    assert_err!(snap(JOB, &[], 0, false), Status::AccessDenied);
    let not_posix = job_create(JOB, JobFlags::NONE).expect("job");
    assert_err!(snap(not_posix, &[], 0, false), Status::AccessDenied);
    let posix = job_create(JOB, JobFlags::POSIX).expect("posix job");
    let peer = job_create(JOB, JobFlags::POSIX).expect("posix job");
    // The gate is the caller's job, not the target's.
    assert_err!(snap(posix, &[], 0, false), Status::AccessDenied);
    let me = directory_lookup(DIRECTORY, "t_snapshot").expect("myself");
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, posix, me);
    let dup = |slot: u32, handle: Handle, rights: Rights| HandleEntry {
        slot,
        handle,
        rights,
        transfer: Transfer::Duplicate,
    };
    e.handle(console_entry())
        .handle(dup(2, posix, JobObject::RIGHTS))
        .handle(dup(3, DIRECTORY, zoraya_abi::DirectoryObject::RIGHTS))
        .handle(dup(OUTSIDE as u32, JOB, JobObject::RIGHTS))
        .handle(dup(PEER as u32, peer, JobObject::RIGHTS));
    // Outside a `posix` job a pin can be made; it moves to `inner`.
    let pinned = memory_create(4096, MemoryFlags::NONE).expect("memory");
    if handle_type(h(DEVICE)) == Ok("device") {
        e.handle(dup(
            DEVICE as u32,
            h(DEVICE),
            zoraya_abi::DeviceObject::RIGHTS,
        ));
        let mut runs = [[0u64; 2]; 1];
        let pin = dma_pin(pinned, h(DEVICE), &mut runs).expect("a pin outside posix");
        e.handle(HandleEntry {
            slot: PIN as u32,
            handle: pin,
            rights: zoraya_abi::PinObject::RIGHTS,
            transfer: Transfer::Move,
        });
    }
    e.arg(b"inner");
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn inner");
    let record = wait_exit(child);
    close(&[child, me, posix, peer, not_posix, pinned]);
    let _ = (StreamReadObject::RIGHTS, StreamWriteObject::RIGHTS);
    record.code
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    match startup().argv.next() {
        Some(b"inner") => inner(),
        _ => outer(),
    }
}

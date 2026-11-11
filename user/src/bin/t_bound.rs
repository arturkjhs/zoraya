#![no_std]
#![no_main]

//! Bounded kernel work (ADR-0270, INV-0270-1, INV-0270-2): the longest time
//! the kernel kept IRQ masked over each call that scales with its argument,
//! at one step and at a large size, and a `job_kill` during a large call.

extern crate alloc;

use zoraya_abi::snapshot::Encoder as SnapshotEncoder;
use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    ChannelEntry, DIRECTORY, ExitReason, Handle, JOB, JobFlags, MapFlags, MemoryFlags,
    OBJECT_TYPES, Rights, Status,
};
use zoraya_user::{
    channel_create, channel_write, clock_monotonic, console_entry, handle_close, handle_type,
    image, job_create, job_get_usage, job_kill, memory_create, memory_map, memory_unmap, println,
    process_exit, raw_syscall, sleep_until, snapshot_process, spawn, startup, wait_exit,
};

const MASKED_MAX_NS: u64 = 0x7013;
const FREE_FRAMES: u64 = 0x7006;
const PAGE: u64 = 4096;
/// One step of ADR-0270 point 2: 64 pages.
const STEP: u64 = 64 * PAGE;
/// The large size: a quarter of the 128 MiB test machine.
const BIG: u64 = 24 << 20;
/// The absolute bound, INV-0270-1 (HVF).
const BOUND_NS: u64 = 2_000_000;
/// Chain length of the channel close, INV-0270-1.
const CHAIN: usize = 10_000;

fn masked(reset: bool) -> u64 {
    raw_syscall(MASKED_MAX_NS, [reset as u64, 0, 0, 0, 0, 0]).1
}

fn free_frames() -> u64 {
    raw_syscall(FREE_FRAMES, [0; 6]).1
}

fn nap(ms: u64) {
    sleep_until(clock_monotonic() + ms * 1_000_000);
}

fn rights_of(h: Handle) -> Rights {
    let name = handle_type(h).unwrap_or("");
    OBJECT_TYPES
        .iter()
        .find(|t| t.name == name)
        .map_or(Rights::NONE, |t| t.rights)
}

/// Runs `f` and gives the longest masked interval during it.
fn measure<T>(f: impl FnOnce() -> T) -> (T, u64) {
    masked(true);
    let value = f();
    (value, masked(true))
}

/// What a site's large case may take: the INV-0270-1 bound, or on a slow
/// host (TCG) four times its own one-step case plus a fixed 300 us.
fn check(site: &str, step: u64, big: u64) {
    println!(
        "t_bound: {site}: one step {} us, large {} us masked",
        step / 1000,
        big / 1000
    );
    let limit = BOUND_NS.max(4 * step + 300_000);
    assert!(big <= limit, "{site}: {big} ns masked, limit {limit}");
}

fn child(job: Handle, role: &[u8]) -> Result<Handle, Status> {
    let mut buffer = [0; 512];
    let program = image("t_bound");
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_entry());
    e.arg(role);
    let length = e.finish().expect("room");
    let result = spawn(&buffer[..length]);
    handle_close(program).expect("image");
    result
}

fn memory_sites() {
    let mut step = [0; 4];
    let mut big = [0; 4];
    for (i, size) in [STEP, BIG].into_iter().enumerate() {
        let out = if i == 0 { &mut step } else { &mut big };
        let (object, ns) = measure(|| memory_create(size, MemoryFlags::NONE).expect("create"));
        out[0] = ns;
        let (at, ns) = measure(|| memory_map(object, 0, size, MapFlags::WRITE).expect("map"));
        out[1] = ns;
        let ((), ns) = measure(|| memory_unmap(at, size).expect("unmap"));
        out[2] = ns;
        let ((), ns) = measure(|| handle_close(object).expect("close"));
        out[3] = ns;
    }
    for (i, site) in [
        "memory_create",
        "memory_map",
        "memory_unmap",
        "memory close",
    ]
    .into_iter()
    .enumerate()
    {
        check(site, step[i], big[i]);
    }
}

fn snapshot_of(size: u64) -> u64 {
    let object = memory_create(size, MemoryFlags::NONE).expect("create");
    let at = memory_map(object, 0, size, MapFlags::WRITE).expect("map");
    let job = job_create(JOB, JobFlags::POSIX).expect("job");
    let mut buffer = alloc::vec![0u8; 512];
    let mut e = SnapshotEncoder::new(&mut buffer, job);
    e.suspended();
    let length = e.finish().expect("room");
    masked(true);
    let child = snapshot_process(&buffer[..length]).expect("snapshot");
    let ns = masked(true);
    let Some(child) = child else { process_exit(0) };
    job_kill(job).expect("kill");
    wait_exit(child);
    handle_close(child).expect("close");
    handle_close(job).expect("job");
    memory_unmap(at, size).expect("unmap");
    handle_close(object).expect("close");
    ns
}

fn chain_close() -> (u64, u64) {
    let build = |n: usize| {
        let mut held: Option<Handle> = None;
        for _ in 0..n {
            let (x, y) = channel_create().expect("channel");
            if let Some(prev) = held {
                let entry = [ChannelEntry {
                    handle: prev,
                    rights: rights_of(prev),
                    transfer: Transfer::Move,
                }];
                channel_write(x, b"c", &entry).expect("write");
            }
            handle_close(x).expect("close");
            held = Some(y);
        }
        held.expect("a chain")
    };
    let short = build(64);
    let (_, step) = measure(|| handle_close(short).expect("close"));
    let long = build(CHAIN);
    let (_, big) = measure(|| handle_close(long).expect("close"));
    (step, big)
}

/// A child that holds `BIG` of memory, mapped, then waits to be killed or
/// to end by itself.
fn hold(wait_ms: u64) -> i32 {
    let object = memory_create(BIG, MemoryFlags::NONE).expect("create");
    memory_map(object, 0, BIG, MapFlags::WRITE).expect("map");
    nap(wait_ms);
    0
}

/// A child that makes and closes `BIG` objects without end, or snapshots
/// itself with `BIG` mapped without end, until it is killed.
fn churn(snapshots: bool) -> i32 {
    if !snapshots {
        loop {
            let object = memory_create(BIG, MemoryFlags::NONE).expect("create");
            handle_close(object).expect("close");
        }
    }
    let object = memory_create(BIG, MemoryFlags::NONE).expect("create");
    memory_map(object, 0, BIG, MapFlags::WRITE).expect("map");
    let mut buffer = alloc::vec![0u8; 512];
    let length = SnapshotEncoder::new(&mut buffer, JOB)
        .finish()
        .expect("room");
    loop {
        let Some(child) = snapshot_process(&buffer[..length]).expect("snapshot") else {
            process_exit(0)
        };
        wait_exit(child);
        handle_close(child).expect("close");
    }
}

/// INV-0270-2: a `job_kill` from another process during a large
/// `memory_create` and a large `snapshot_process` leaves no object and no
/// charge: the room's use and the free frames return to what they were once
/// the reaper is idle.
fn a_kill_between_steps_leaves_nothing() {
    for role in [&b"create-loop"[..], b"snapshot-loop"] {
        let free = free_frames();
        let room = job_create(JOB, JobFlags::POSIX).expect("job");
        let p = child(room, role).expect("spawn");
        nap(150);
        masked(true);
        job_kill(room).expect("kill");
        let record = wait_exit(p);
        assert_eq!(record.reason, ExitReason::Killed, "{role:?}");
        let mut tries = 0;
        while (free_frames() != free || job_get_usage(room, false).expect("usage").memory.used != 0)
            && tries < 200
        {
            nap(10);
            tries += 1;
        }
        let usage = job_get_usage(room, false).expect("usage");
        assert_eq!(usage.memory.used, 0, "{role:?}: the room's use");
        assert_eq!(usage.processes.used, 0, "{role:?}: the room's processes");
        assert_eq!(free_frames(), free, "{role:?}: frames");
        println!(
            "t_bound: job_kill during {}: nothing left; {} us masked",
            core::str::from_utf8(role).unwrap_or("?"),
            masked(true) / 1000
        );
        handle_close(p).expect("close");
        handle_close(room).expect("close");
    }
}

fn teardown_sites() {
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    // Exit of a process holding BIG, then job_kill of one.
    let free = free_frames();
    masked(true);
    let p = child(room, b"hold-short").expect("spawn");
    wait_exit(p);
    handle_close(p).expect("close");
    let exit_ns = masked(true);
    let p = child(room, b"hold-long").expect("spawn");
    nap(300);
    masked(true);
    job_kill(room).expect("kill");
    wait_exit(p);
    nap(100);
    let kill_ns = masked(true);
    handle_close(p).expect("close");
    handle_close(room).expect("room");
    println!(
        "t_bound: exit of a process holding {} MiB: {} us masked; job_kill of one: {} us masked",
        BIG >> 20,
        exit_ns / 1000,
        kill_ns / 1000
    );
    assert!(exit_ns <= BOUND_NS.max(4_000_000), "exit {exit_ns}");
    assert!(kill_ns <= BOUND_NS.max(4_000_000), "kill {kill_ns}");
    // The frames are back (INV-0270-2 for a killed process).
    let mut tries = 0;
    while free_frames() != free && tries < 100 {
        nap(10);
        tries += 1;
    }
    assert_eq!(free_frames(), free, "frames after the exit and the kill");
}

/// Runs the measurements inside a `posix` job, where `snapshot_process` is
/// allowed.
fn outer() -> i32 {
    let posix = job_create(JOB, JobFlags::POSIX).expect("posix job");
    let me = image("t_bound");
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, posix, me);
    e.handle(console_entry());
    e.handle(HandleEntry {
        slot: DIRECTORY.slot(),
        handle: DIRECTORY,
        rights: rights_of(DIRECTORY),
        transfer: Transfer::Duplicate,
    });
    e.arg(b"inner");
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn inner");
    let record = wait_exit(child);
    handle_close(child).expect("close");
    handle_close(me).expect("close");
    handle_close(posix).expect("close");
    record.code
}

fn inner() -> i32 {
    memory_sites();
    check("snapshot_process", snapshot_of(STEP), snapshot_of(BIG));
    let (step, big) = chain_close();
    check("channel chain close", step, big);
    teardown_sites();
    a_kill_between_steps_leaves_nothing();
    println!("t_bound: all passed");
    0
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    match startup().argv.next() {
        Some(b"hold-short") => hold(50),
        Some(b"hold-long") => hold(30_000),
        Some(b"create-loop") => churn(false),
        Some(b"snapshot-loop") => churn(true),
        Some(b"inner") => inner(),
        _ => outer(),
    }
}

#![no_std]
#![no_main]

//! Limits on jobs (ADR-0261, ADR-0263): `job_set_limits`' rules, a memory hog
//! that gets `OverQuota` and exits by itself, a sibling job that maps to its
//! own limit while another job holds its whole limit, the process limit at
//! `spawn`, the `OVER_QUOTA` signal, and the job's use returning to what it
//! was when its objects are gone.

use zoraya_abi::quota::JobLimits;
use zoraya_abi::spawn::Encoder;
use zoraya_abi::{
    DIRECTORY, ExitReason, Handle, JOB, JobFlags, JobObject, MemoryFlags, OBJECT_TYPES, ObjectType,
    Rights, Status, job_signals,
};
use zoraya_user::{
    assert_err, clock_monotonic, console_entry, handle_close, handle_duplicate, handle_type, image,
    job_create, job_get_usage, job_kill, job_set_limits, memory_create, println, raw_syscall,
    sleep_until, spawn, startup, wait_exit,
};

const HEAP_LIVE: u64 = 0x700b;
const FREE_FRAMES: u64 = 0x7006;
const EVENTS_LIVE: u64 = 0x7014;

fn events_live() -> u64 {
    raw_syscall(EVENTS_LIVE, [0; 6]).1
}

fn free_frames() -> u64 {
    raw_syscall(FREE_FRAMES, [0; 6]).1
}

fn heap_live() -> u64 {
    raw_syscall(HEAP_LIVE, [0; 6]).1
}

const MIB: u64 = 1 << 20;
const CHUNK: u64 = 64 << 10;
const EXIT_AFTER_REFUSAL: i32 = 42;

fn nap(ms: u64) {
    sleep_until(clock_monotonic() + ms * 1_000_000);
}

fn eventually(done: impl Fn() -> bool) -> bool {
    let deadline = clock_monotonic() + 10_000_000_000;
    while !done() {
        if clock_monotonic() > deadline {
            return false;
        }
        nap(5);
    }
    true
}

fn child(job: Handle, role: &[u8]) -> Result<Handle, Status> {
    let mut buffer = [0; 512];
    let program = image("t_quota");
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_entry());
    e.arg(role);
    let length = e.finish().expect("room");
    let result = spawn(&buffer[..length]);
    handle_close(program).expect("image");
    result
}

fn limits(memory: Option<u64>, processes: Option<u32>) -> JobLimits {
    JobLimits {
        memory,
        processes,
        cpu: None,
    }
}

fn signalled(job: Handle) -> bool {
    zoraya_user::object_get_signals(job).expect("signals") & job_signals::OVER_QUOTA != 0
}

/// INV-0261-3: a handle without `LIMIT` sets nothing; a limit above the
/// parent's effective one is refused; the CPU ceiling is not there yet.
fn the_rules_of_setting_limits() {
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    let weak = handle_duplicate(room, JobObject::RIGHTS.without(Rights::LIMIT)).expect("dup");
    assert_err!(
        job_set_limits(weak, &limits(Some(MIB), None)),
        Status::AccessDenied
    );
    job_set_limits(room, &limits(Some(2 * MIB), Some(3))).expect("limits");
    let below = job_create(room, JobFlags::NONE).expect("job");
    assert_err!(
        job_set_limits(below, &limits(Some(4 * MIB), None)),
        Status::InvalidArgument
    );
    assert_err!(
        job_set_limits(below, &limits(None, Some(4))),
        Status::InvalidArgument
    );
    job_set_limits(below, &limits(Some(MIB), Some(2))).expect("narrower");
    let cpu = JobLimits {
        memory: None,
        processes: None,
        cpu: Some(250),
    };
    assert_err!(job_set_limits(below, &cpu), Status::NotSupported);
    let usage = job_get_usage(room, false).expect("usage");
    assert_eq!(usage.memory.limit, Some(2 * MIB));
    assert_eq!(usage.processes.limit, Some(3));
    assert_eq!(usage.user, 0);
    for h in [below, weak, room] {
        handle_close(h).expect("close");
    }
}

/// INV-0263-1, INV-0263-2: the hog sees `OverQuota`, the signal is on the
/// room that refused it, and the hog ends by its own hand.
fn a_hog_is_refused_and_exits_by_itself() {
    let outer = job_create(JOB, JobFlags::NONE).expect("job");
    let room = job_create(outer, JobFlags::NONE).expect("job");
    job_set_limits(outer, &limits(Some(8 * MIB), None)).expect("outer");
    job_set_limits(room, &limits(Some(2 * MIB), None)).expect("room");
    let hog = child(room, b"hog-exit").expect("spawn");
    let record = wait_exit(hog);
    assert_eq!(record.reason, ExitReason::Exited);
    assert_eq!(record.code, EXIT_AFTER_REFUSAL);
    let usage = job_get_usage(room, false).expect("usage");
    assert!(usage.memory.refused >= 1, "{usage:?}");
    assert!(usage.memory.used <= 2 * MIB, "{usage:?}");
    // On exactly the job whose limit refused it (INV-0263-2).
    assert!(signalled(room));
    assert!(!signalled(outer));
    job_get_usage(room, true).expect("clear");
    assert!(!signalled(room));
    handle_close(hog).expect("close");
    for h in [room, outer] {
        handle_close(h).expect("close");
    }
}

/// INV-0261-4 and INV-0261-6: a job at its limit leaves a sibling its own,
/// and a killed job's use returns to what it was.
fn a_sibling_keeps_its_own_limit_and_a_kill_gives_everything_back() {
    let full = job_create(JOB, JobFlags::NONE).expect("job");
    let sibling = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(full, &limits(Some(2 * MIB), None)).expect("full");
    job_set_limits(sibling, &limits(Some(2 * MIB), None)).expect("sibling");
    let before = job_get_usage(full, false).expect("usage").memory.used;
    let hog = child(full, b"hog-hold").expect("spawn");
    assert!(eventually(|| job_get_usage(full, false)
        .expect("u")
        .memory
        .refused
        > 0));
    let mapper = child(sibling, b"fill").expect("spawn");
    let record = wait_exit(mapper);
    assert_eq!(
        (record.reason, record.code),
        (ExitReason::Exited, 0),
        "{record:?}"
    );
    handle_close(mapper).expect("close");
    job_kill(full).expect("kill");
    let record = wait_exit(hog);
    assert_eq!(record.reason, ExitReason::Killed);
    handle_close(hog).expect("close");
    assert!(eventually(|| {
        let u = job_get_usage(full, false).expect("u");
        u.memory.used == before && u.processes.used == 0
    }));
    for h in [full, sibling] {
        handle_close(h).expect("close");
    }
}

/// INV-0261-5: what `job_get_usage` says a call cost is what the heap and the
/// frames show. The job here is this process's own, which nothing else
/// allocates for; the heap grows only by what the call took (a 64 KiB chunk
/// of a class may already be there, but `live` counts blocks).
fn usage_matches_the_heap_and_the_frames() {
    let used = || job_get_usage(JOB, false).expect("usage").memory.used;
    let (u0, h0) = (used(), heap_live());
    // A memory object: its frames, then its record and list.
    let m = memory_create(4 * CHUNK, MemoryFlags::NONE).expect("memory");
    let (u1, h1) = (used(), heap_live());
    assert_eq!(u1 - u0, (h1 - h0) + 4 * CHUNK, "memory_create");
    // Each of the others costs exactly what the heap took.
    let p = zoraya_user::port_create().expect("port");
    let (u2, h2) = (used(), heap_live());
    // A port is charged for a first 64 packets of 24 bytes and its place in the
    // event tables, which the heap shows only as they fill; it pays for 64
    // more each time its ready list outgrows what it has paid.
    assert_eq!(u2 - u1, (h2 - h1) + 128 + 64 * 24, "port_create");
    let e = zoraya_user::event_create().expect("event");
    let (u3, h3) = (used(), heap_live());
    assert_eq!(u3 - u2, h3 - h2, "event_create");
    let (c0, c1) = zoraya_user::channel_create().expect("channel");
    let (u4, h4) = (used(), heap_live());
    // Charged for the queues' slots at their most and two sources, which the heap
    // shows only as messages queue.
    assert_eq!(u4 - u3, (h4 - h3) + 2 * 64 * 8 + 2 * 64, "channel_create");
    let (r, w) = zoraya_user::stream_create().expect("stream");
    let (u5, h5) = (used(), heap_live());
    // The ring is 64 KiB of user frames; the rest is the stream, its two ends,
    // the ring's record and two sources. The heap's own delta also holds the
    // handle table growing for two handles, which a process pays for up front.
    let kernel = (u5 - u4) - 65536;
    assert!(
        kernel >= 512 && kernel <= (h5 - h4) + 256,
        "stream_create: {kernel} of {}",
        h5 - h4
    );
    // And everything is given back.
    for h in [m, p, e, c0, c1, r, w] {
        handle_close(h).expect("close");
    }
    assert_eq!(used(), u0);
    // The heap keeps the handle table's grown capacity (charged up front).
    assert!(heap_live() - h0 <= 4096);
}

/// `spawn` is refused at the process limit, with no effect.
fn spawn_stops_at_the_process_limit() {
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(room, &limits(None, Some(2))).expect("limits");
    let a = child(room, b"idle").expect("first");
    let b = child(room, b"idle").expect("second");
    // A refused spawn leaves no trace: no event source (review Q1, error 1),
    // no heap, no frame (INV-0261-2).
    let (events, heap, frames) = (events_live(), heap_live(), free_frames());
    assert_err!(child(room, b"idle"), Status::OverQuota);
    assert_eq!(events_live(), events, "event sources after a refused spawn");
    assert_eq!(free_frames(), frames, "frames after a refused spawn");
    assert!(heap_live() <= heap + 4096, "heap after a refused spawn");
    let usage = job_get_usage(room, false).expect("usage");
    assert_eq!(usage.processes.used, 2);
    assert_eq!(usage.processes.refused, 1);
    job_kill(room).expect("kill");
    for p in [a, b] {
        wait_exit(p);
        handle_close(p).expect("close");
    }
    assert!(eventually(|| job_get_usage(room, false)
        .expect("u")
        .processes
        .used
        == 0));
    handle_close(room).expect("close");
}

/// The classic `:|:&` without a shell: each copy starts two more in its own
/// job and then waits to be killed. The job's process limit is what stops it.
fn bomb() -> i32 {
    for _ in 0..2 {
        let mut buffer = [0; 512];
        let program = image("t_quota");
        let mut e = Encoder::new(&mut buffer, JOB, program);
        e.handle(console_entry());
        e.handle(zoraya_user::dup(JOB.slot(), JOB, JobObject::RIGHTS));
        e.handle(zoraya_user::dup(
            DIRECTORY.slot(),
            DIRECTORY,
            directory_rights(),
        ));
        e.arg(b"bomb");
        let length = e.finish().expect("room");
        // Refused at the limit; the copies already running stay.
        if let Err(status) = spawn(&buffer[..length])
            && status != Status::OverQuota
        {
            println!("t_quota: bomb: spawn: {status:?}");
        }
        handle_close(program).expect("image");
    }
    nap(30_000);
    0
}

fn directory_rights() -> Rights {
    OBJECT_TYPES
        .iter()
        .find(|t| Some(t.name) == handle_type(DIRECTORY).ok())
        .map_or(Rights::NONE, |t| t.rights)
}

/// Q2's criterion: a fork bomb in a limited job stops at its process limit,
/// and a command outside it starts and ends within 2 s (INV-0261-4,
/// INV-0263-1).
fn a_fork_bomb_stops_at_the_process_limit() {
    const LIMIT: u32 = 24;
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(room, &limits(None, Some(LIMIT))).expect("limits");
    let frames = free_frames();
    let started = clock_monotonic();
    let mut buffer = [0; 512];
    let program = image("t_quota");
    let mut e = Encoder::new(&mut buffer, room, program);
    e.handle(console_entry());
    e.handle(zoraya_user::dup(JOB.slot(), room, JobObject::RIGHTS));
    e.handle(zoraya_user::dup(
        DIRECTORY.slot(),
        DIRECTORY,
        directory_rights(),
    ));
    e.arg(b"bomb");
    let length = e.finish().expect("room");
    let root = spawn(&buffer[..length]).expect("the first copy");
    handle_close(program).expect("image");
    assert!(
        eventually(|| {
            let u = job_get_usage(room, false).expect("usage");
            u.processes.used == u64::from(LIMIT) && u.processes.refused > 0
        }),
        "the bomb did not reach its limit: {:?}",
        job_get_usage(room, false).expect("usage").processes
    );
    let reached = clock_monotonic() - started;
    let usage = job_get_usage(room, false).expect("usage").processes;
    println!(
        "t_quota: fork bomb reached its limit of {LIMIT} processes in {} ms, {} refused",
        reached / 1_000_000,
        usage.refused
    );
    // A command outside the limited job starts and ends within 2 s, while the
    // bomb sits at its limit.
    let t0 = clock_monotonic();
    let quick = child(JOB, b"quick").expect("a command outside the bomb's job");
    let record = wait_exit(quick);
    let took = clock_monotonic() - t0;
    handle_close(quick).expect("close");
    println!(
        "t_quota: a command outside the bomb's job took {} ms",
        took / 1_000_000
    );
    assert_eq!(record.code, 3);
    assert!(took < 2_000_000_000, "{took} ns");
    assert_eq!(
        job_get_usage(room, false).expect("u").processes.used,
        u64::from(LIMIT)
    );
    job_kill(room).expect("kill");
    wait_exit(root);
    handle_close(root).expect("close");
    assert!(eventually(|| {
        job_get_usage(room, false).expect("u").processes.used == 0 && free_frames() == frames
    }));
    handle_close(room).expect("close");
}

/// Review Q1, error 2: mapping an object another job made, in a job with no
/// room for the page tables, is `OverQuota` (not `NoMemory`) and changes
/// nothing.
fn mapping_a_foreign_object_at_the_limit_is_refused() {
    const SIZE: u64 = 8 * MIB;
    let foreign = memory_create(SIZE, MemoryFlags::NONE).expect("made outside the room");
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(room, &limits(Some(4 * MIB), None)).expect("limits");
    let mut buffer = [0; 512];
    let program = image("t_quota");
    let mut e = Encoder::new(&mut buffer, room, program);
    e.handle(console_entry());
    e.handle(zoraya_user::dup(
        8,
        foreign,
        Rights::READ.union(Rights::WRITE).union(Rights::DUPLICATE),
    ));
    e.arg(b"map-foreign");
    let length = e.finish().expect("room");
    let p = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    // While the child sleeps, the room's limit becomes its use plus two
    // pages: the page tables of an 8 MiB mapping do not fit. It reports in
    // its exit code.
    nap(150);
    let used = job_get_usage(room, false).expect("usage").memory.used;
    job_set_limits(room, &limits(Some(used + 8192), None)).expect("narrower");
    let record = wait_exit(p);
    assert_eq!(record.code, EXIT_AFTER_REFUSAL, "{record:?}");
    handle_close(p).expect("close");
    handle_close(room).expect("close");
    handle_close(foreign).expect("close");
}

/// In the child of `mapping_a_foreign_object_at_the_limit_is_refused`.
fn map_foreign() -> i32 {
    let object = Handle::from_parts(8, 0).expect("slot");
    nap(400);
    let before = job_get_usage(JOB, false).expect("usage").memory;
    let result = zoraya_user::memory_map(object, 0, 8 * MIB, zoraya_abi::MapFlags::WRITE);
    if result != Err(Status::OverQuota) {
        println!("t_quota: map-foreign: {result:?}");
        return 1;
    }
    let after = job_get_usage(JOB, false).expect("usage").memory;
    if after.used != before.used || after.refused <= before.refused {
        println!("t_quota: map-foreign: {before:?} -> {after:?}");
        return 2;
    }
    EXIT_AFTER_REFUSAL
}

fn hog_until_refused(hold: bool) -> i32 {
    let mut held = 0;
    loop {
        match memory_create(CHUNK, MemoryFlags::NONE) {
            Ok(_) => held += 1,
            Err(Status::OverQuota) => break,
            Err(other) => panic!("memory_create: {other:?} after {held}"),
        }
        assert!(held < 1000, "no limit reached");
    }
    if hold {
        nap(30_000);
    }
    EXIT_AFTER_REFUSAL
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    match startup().argv.next() {
        Some(b"hog-exit") => return hog_until_refused(false),
        Some(b"hog-hold") => return hog_until_refused(true),
        Some(b"fill") => {
            // Half the sibling's limit, in pieces, all granted.
            for _ in 0..(MIB / CHUNK) {
                memory_create(CHUNK, MemoryFlags::NONE).expect("within its own limit");
            }
            return 0;
        }
        Some(b"bomb") => return bomb(),
        Some(b"quick") => return 3,
        Some(b"map-foreign") => return map_foreign(),
        Some(b"idle") => {
            nap(30_000);
            return 0;
        }
        _ => {}
    }
    the_rules_of_setting_limits();
    a_hog_is_refused_and_exits_by_itself();
    a_sibling_keeps_its_own_limit_and_a_kill_gives_everything_back();
    spawn_stops_at_the_process_limit();
    a_fork_bomb_stops_at_the_process_limit();
    mapping_a_foreign_object_at_the_limit_is_refused();
    usage_matches_the_heap_and_the_frames();
    println!("t_quota: all passed");
    0
}

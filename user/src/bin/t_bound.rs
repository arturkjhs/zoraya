#![no_std]
#![no_main]

//! Bounded kernel work (ADR-0270, INV-0270-1, INV-0270-2): the longest time
//! the kernel kept IRQ masked over each call that scales with its argument,
//! at one step and at a large size, and a `job_kill` during a large call.

extern crate alloc;

use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::snapshot::Encoder as SnapshotEncoder;
use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelEntry, DIRECTORY, ExitReason, Handle, JOB, JobFlags, JobObject, MapFlags,
    MemoryFlags, OBJECT_TYPES, ObjectType, RequestKind, Rights, Status,
};
use zoraya_user::{
    channel_create, channel_write, clock_monotonic, handle_close, handle_type, image, job_create,
    job_get_usage, job_kill, job_resume, job_signal, job_suspend, memory_create, memory_map,
    memory_unmap, println, process_exit, raw_syscall, sleep_until, snapshot_process, spawn,
    startup, wait_exit,
};

const MASKED_MAX_NS: u64 = 0x7013;
const MASKED_MAX_UNITS: u64 = 0x7015;
const CANCELLED: u64 = 0x7016;
const HEAP_BACKING: u64 = 0x7017;
const FREE_FRAMES: u64 = 0x7006;
const DIAGNOSE_UNITS: u64 = 0x7019;
const PAGE: u64 = 4096;
/// One step of ADR-0270 point 2: 64 pages.
const STEP: u64 = 64 * PAGE;
/// The large size: a quarter of the 128 MiB test machine.
const BIG: u64 = 24 << 20;
/// The absolute bound, INV-0270-1, as ADR-0270 states it for HVF.
const BOUND_NS: u64 = 2_000_000;
/// Units of work (frames, messages, handles, mappings) one interval with IRQ
/// masked may hold: a step, plus the page-table and heap frames a step of a
/// map or a list may need besides. Checked on every host, whatever its speed.
const MAX_UNITS: u64 = 64 + 16;
/// The time `calibrate` takes on the machine the bound is stated for (HVF),
/// in nanoseconds.
const REFERENCE_FILL_NS: u64 = 10_000;
/// The most the printed factor goes to.
const MAX_FACTOR: u64 = 64;

/// How many times slower than the reference host this one is, from
/// `calibrate`; 1 under HVF.
static FACTOR: AtomicU64 = AtomicU64::new(1);

/// Times a fixed amount of work (a 256 KiB fill, the size of a step's
/// pages, five times, the best) against the reference: a factor of 1 is
/// HVF, where the time bound is INV-0270-1's 2 ms (`bound_ns`).
fn calibrate() {
    let mut buffer = alloc::vec![0u64; 32768];
    let mut best = u64::MAX;
    for _ in 0..5 {
        let began = clock_monotonic();
        for (i, word) in buffer.iter_mut().enumerate() {
            // SAFETY: a valid, aligned reference to a word of the buffer.
            unsafe { core::ptr::write_volatile(word, i as u64) };
        }
        best = best.min(clock_monotonic() - began);
    }
    let factor = best.div_ceil(REFERENCE_FILL_NS).clamp(1, MAX_FACTOR);
    FACTOR.store(factor, Ordering::Relaxed);
    let limit_ms = if factor > 1 {
        SLOW_HOST_LIMIT_NS
    } else {
        BOUND_NS
    } / 1_000_000;
    println!("t_bound: fill of 256 KiB takes {best} ns: factor {factor}, time bound {limit_ms} ms");
}

/// What the longest masked interval of a case may last in wall time. Under
/// HVF (factor 1) it is INV-0270-1's 2 ms. Off HVF the guest's clock runs
/// while the host stalls or slows the vCPU, by more than any calibration
/// taken before can tell: in one CI run of `tcg-smp4-gicv2` (factor 64, the
/// clamp) every memory case took 20 to 40 times what the same job takes in
/// green runs (`memory_map` of a step 14.5 ms against 0.47 ms, the
/// contiguous step 244 ms against 5.9 ms) with no more units of work. There,
/// as in `t_cpu`, the units (`measure`) are the check that the work between
/// step points is bounded, and the time only that nothing ran masked for a
/// second.
fn bound_ns() -> u64 {
    if FACTOR.load(Ordering::Relaxed) > 1 {
        return SLOW_HOST_LIMIT_NS;
    }
    BOUND_NS
}

/// The masked-time limit off HVF (`bound_ns`): about 3.5 times the longest
/// interval seen on a slowed CI host (287 ms, a contiguous object of 4 MiB).
const SLOW_HOST_LIMIT_NS: u64 = 1_000_000_000;
/// Chain length of the channel close, INV-0270-1.
const CHAIN: usize = 10_000;

fn masked(reset: bool) -> u64 {
    raw_syscall(MASKED_MAX_NS, [reset as u64, 0, 0, 0, 0, 0]).1
}

/// The most units of work in one interval with IRQ masked since the last
/// reset.
fn units(reset: bool) -> u64 {
    raw_syscall(MASKED_MAX_UNITS, [reset as u64, 0, 0, 0, 0, 0]).1
}

fn cancelled() -> u64 {
    raw_syscall(CANCELLED, [0; 6]).1
}

fn free_frames() -> u64 {
    raw_syscall(FREE_FRAMES, [0; 6]).1
}

/// The free frames plus the frames the kernel heap has taken so far. The heap
/// grows by 64 KiB at a time and never gives frames back, so a kill that makes
/// it grow (more CPUs, more concurrent allocations) moves frames from free to
/// the heap without leaking any: this sum is what must return to its start.
/// Both calls wait for the reaper to be idle first.
fn frames_accounted() -> u64 {
    loop {
        let before = raw_syscall(HEAP_BACKING, [0; 6]).1;
        let free = free_frames();
        // The heap did not grow between the two reads.
        if raw_syscall(HEAP_BACKING, [0; 6]).1 == before {
            return free + before / 4096;
        }
    }
}

fn nap(ms: u64) {
    sleep_until(clock_monotonic() + ms * 1_000_000);
}

/// The console for a child that passes it on: with `DUPLICATE`.
fn console_again() -> HandleEntry {
    HandleEntry {
        slot: CONSOLE.slot(),
        handle: CONSOLE,
        rights: Rights::WRITE.union(Rights::DUPLICATE),
        transfer: Transfer::Duplicate,
    }
}

fn rights_of(h: Handle) -> Rights {
    let name = handle_type(h).unwrap_or("");
    OBJECT_TYPES
        .iter()
        .find(|t| t.name == name)
        .map_or(Rights::NONE, |t| t.rights)
}

/// Runs `f` and gives the longest masked interval during it. The units of
/// work in any interval may not pass a step, whatever the host's speed
/// (INV-0270-1).
fn measure<T>(f: impl FnOnce() -> T) -> (T, u64) {
    masked(true);
    units(true);
    let value = f();
    let ns = masked(true);
    let done = units(true);
    if done > MAX_UNITS {
        // Where the units of the busiest interval came from, for the log.
        raw_syscall(DIAGNOSE_UNITS, [0; 6]);
    }
    assert!(
        done <= MAX_UNITS,
        "{done} units of work between two step points, limit {MAX_UNITS}"
    );
    (value, ns)
}

/// What a site's large case may take: the INV-0270-1 bound under HVF, the
/// limit of `bound_ns` off it.
fn check(site: &str, step: u64, big: u64) {
    println!(
        "t_bound: {site}: one step {} us, large {} us masked",
        step / 1000,
        big / 1000
    );
    let limit = bound_ns();
    assert!(big <= limit, "{site}: {big} ns masked, limit {limit}");
}

fn child(job: Handle, role: &[u8]) -> Result<Handle, Status> {
    let mut buffer = [0; 512];
    let program = image("t_bound");
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_again());
    // The child's own job, at the slot a process finds it (a spawned process
    // gets only what its list names).
    e.handle(HandleEntry {
        slot: JOB.slot(),
        handle: job,
        rights: JobObject::RIGHTS,
        transfer: Transfer::Duplicate,
    });
    e.arg(role);
    let length = e.finish().expect("room");
    let result = spawn(&buffer[..length]);
    handle_close(program).expect("image");
    result
}

fn memory_sites() {
    let mut step = [u64::MAX; 4];
    let mut big = [u64::MAX; 4];
    // The step case once; the large case three times, the best of them: a
    // pause of the host in one run is not the kernel holding IRQ masked, and
    // a regression shows in all three.
    for (is_big, size) in [(false, STEP), (true, BIG), (true, BIG), (true, BIG)] {
        let out = if is_big { &mut big } else { &mut step };
        let (object, ns) = measure(|| memory_create(size, MemoryFlags::NONE).expect("create"));
        out[0] = out[0].min(ns);
        let (at, ns) = measure(|| memory_map(object, 0, size, MapFlags::WRITE).expect("map"));
        out[1] = out[1].min(ns);
        let ((), ns) = measure(|| memory_unmap(at, size).expect("unmap"));
        out[2] = out[2].min(ns);
        let ((), ns) = measure(|| handle_close(object).expect("close"));
        out[3] = out[3].min(ns);
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
    contiguous_sites();
}

/// The most a contiguous object may have (ADR-0270 open questions): 4 MiB.
const CONTIGUOUS_CAP: u64 = 4 << 20;

/// `memory_create(CONTIGUOUS)` is zeroed in steps, closed in steps and
/// refused above the cap, each with the work of a step between two points
/// (INV-0270-1).
fn contiguous_sites() {
    let mut step = u64::MAX;
    let mut big = u64::MAX;
    for (is_big, size) in [
        (false, STEP),
        (true, CONTIGUOUS_CAP),
        (true, CONTIGUOUS_CAP),
    ] {
        let (object, ns) =
            measure(|| memory_create(size, MemoryFlags::CONTIGUOUS).expect("contiguous create"));
        let ((), closed) = measure(|| handle_close(object).expect("close"));
        let out = if is_big { &mut big } else { &mut step };
        *out = (*out).min(ns.max(closed));
    }
    check("memory_create contiguous", step, big);
    let free = free_frames();
    let (refused, ns) = measure(|| memory_create(CONTIGUOUS_CAP + PAGE, MemoryFlags::CONTIGUOUS));
    assert_eq!(refused.err(), Some(Status::NoMemory), "above the cap");
    assert!(ns <= bound_ns(), "the refusal took {ns} ns masked");
    assert_eq!(free_frames(), free, "a refusal keeps nothing");
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
    // INV-0050-6's chain: each new pair's end travels in the message queued
    // for the one before, and the head's close walks the whole chain.
    let build = |n: usize| {
        let (head, mut peer) = channel_create().expect("channel");
        for _ in 0..n {
            let (z, next) = channel_create().expect("channel");
            let entry = [ChannelEntry {
                handle: z,
                rights: rights_of(z),
                transfer: Transfer::Move,
            }];
            channel_write(peer, b"", &entry).expect("write");
            handle_close(peer).expect("close");
            peer = next;
        }
        (head, peer)
    };
    let (head, tail) = build(64);
    let (_, step) = measure(|| handle_close(head).expect("close"));
    handle_close(tail).expect("close");
    let (head, tail) = build(CHAIN);
    let (_, big) = measure(|| handle_close(head).expect("close"));
    handle_close(tail).expect("close");
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

/// A child that makes and closes contiguous objects without end.
fn churn_contiguous() -> i32 {
    loop {
        // A run that was just closed may still be with the reaper, and the
        // machine may not have another: wait for it a little.
        match memory_create(CONTIGUOUS_CAP, MemoryFlags::CONTIGUOUS) {
            Ok(object) => handle_close(object).expect("close"),
            Err(Status::NoMemory) => nap(1),
            Err(status) => panic!("create: {status:?}"),
        }
    }
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
    // The duration of one call of the loops (create and close of `BIG`),
    // the median of 3, with no kill. The children loop for ever, so a kill
    // at delay `d` meets them at some phase of a call; the delays sweep
    // from 0 (before the child has run: a miss, still checked for leaks) to
    // the child's start-up plus 1.5 calls, in 60 steps, cycled. The sweep
    // spans at least 20 ms of calls, whatever the host's speed.
    let mut samples = [0u64; 3];
    for sample in &mut samples {
        let start = clock_monotonic();
        let object = memory_create(BIG, MemoryFlags::NONE).expect("create");
        handle_close(object).expect("close");
        *sample = clock_monotonic() - start;
    }
    samples.sort_unstable();
    let call_ms = samples[1] / 1_000_000;
    let span_ms = 100 + call_ms.max(20) * 3 / 2;
    println!(
        "t_bound: calibration: one call takes {} us; kill delays sweep 0..{span_ms} ms",
        samples[1] / 1000
    );
    for role in [&b"create-loop"[..], b"snapshot-loop", b"contiguous-loop"] {
        // The first round lets the kernel heap take the room it will use
        // (it keeps what it grew by); the next is the one that counts. A
        // kill may land at the child's return to EL0 and not inside the
        // call, which leaves nothing either; so rounds are repeated until a
        // step point ended a call with `Cancelled` (`steps::cancelled`
        // counts every call a kill ended, whatever status the child saw),
        // which shows the cancel path ran (INV-0270-2).
        kill_round(role, false, 150);
        let mut landed = false;
        let mut rounds = 0;
        while rounds < 120 {
            let before = cancelled();
            kill_round(role, true, (rounds % 60) * span_ms / 60);
            rounds += 1;
            if cancelled() > before {
                landed = true;
                break;
            }
        }
        assert!(
            landed,
            "{role:?}: no kill ended a call between steps in {rounds} rounds"
        );
        println!(
            "t_bound: {}: a kill ended a call between steps in round {rounds}",
            core::str::from_utf8(role).unwrap_or("?")
        );
    }
}

fn kill_round(role: &[u8], strict: bool, delay_ms: u64) {
    let free = frames_accounted();
    let room = job_create(JOB, JobFlags::POSIX).expect("job");
    // The room's own record is charged to it, so "nothing left" is the use
    // it began with.
    let base = job_get_usage(room, false).expect("usage").memory.used;
    let p = child(room, role).expect("spawn");
    nap(delay_ms);
    masked(true);
    job_kill(room).expect("kill");
    let record = wait_exit(p);
    assert_eq!(record.reason, ExitReason::Killed, "{role:?}");
    // A zombie keeps its record charged until its last handle closes.
    handle_close(p).expect("close");
    let mut tries = 0;
    while (frames_accounted() != free
        || job_get_usage(room, false).expect("usage").memory.used != base)
        && tries < 200
    {
        nap(10);
        tries += 1;
    }
    let usage = job_get_usage(room, false).expect("usage");
    if strict {
        assert_eq!(usage.memory.used, base, "{role:?}: the room's use");
        assert_eq!(usage.processes.used, 0, "{role:?}: the room's processes");
        assert_eq!(frames_accounted(), free, "{role:?}: frames");
        println!(
            "t_bound: job_kill during {}: nothing left; {} us masked",
            core::str::from_utf8(role).unwrap_or("?"),
            masked(true) / 1000
        );
    }
    handle_close(room).expect("close");
}

/// The exit of a process that holds a lot, and the `job_kill` of one: the
/// longest masked interval and the most units of work in one (INV-0270-1),
/// and the frames back (INV-0270-2).
fn teardown(label: &str, short: &[u8], long: &[u8]) {
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    let free = frames_accounted();
    masked(true);
    units(true);
    let p = child(room, short).expect("spawn");
    wait_exit(p);
    handle_close(p).expect("close");
    let exit_ns = masked(true);
    let exit_units = units(true);
    let p = child(room, long).expect("spawn");
    nap(500);
    masked(true);
    units(true);
    job_kill(room).expect("kill");
    wait_exit(p);
    nap(100);
    let kill_ns = masked(true);
    let kill_units = units(true);
    handle_close(p).expect("close");
    handle_close(room).expect("room");
    println!(
        "t_bound: exit of {label}: {} us masked, {exit_units} units; job_kill of one: {} us masked, {kill_units} units",
        exit_ns / 1000,
        kill_ns / 1000
    );
    // The bound itself (INV-0270-1); off HVF only a sanity limit.
    let limit = bound_ns();
    assert!(
        exit_ns <= limit,
        "{label}: exit {exit_ns} ns, limit {limit}"
    );
    assert!(
        kill_ns <= limit,
        "{label}: kill {kill_ns} ns, limit {limit}"
    );
    assert!(exit_units <= MAX_UNITS, "{label}: exit {exit_units} units");
    assert!(kill_units <= MAX_UNITS, "{label}: kill {kill_units} units");
    // The frames are back (INV-0270-2 for a killed process).
    let mut tries = 0;
    while frames_accounted() != free && tries < 100 {
        nap(10);
        tries += 1;
    }
    assert_eq!(
        frames_accounted(),
        free,
        "{label}: frames after the exit and the kill"
    );
}

/// ADR-0270, INV-0270-1: `job_signal`, `job_suspend`, `job_resume` and
/// `job_kill` walk a whole tree of jobs, in steps. A top job with `n` jobs
/// below it (chains of 12 under it, all held by handles, which a room's table and
/// memory limit allow in the hundreds), then each call on the top job, with the longest masked interval.
fn job_tree_sites() {
    let build = |n: usize| {
        let top = job_create(JOB, JobFlags::NONE).expect("top job");
        let mut held = alloc::vec::Vec::new();
        let mut made = 0;
        while made < n {
            let mut parent = top;
            for _ in 0..12 {
                if made >= n {
                    break;
                }
                let job = job_create(parent, JobFlags::NONE).expect("job");
                held.push(job);
                made += 1;
                parent = job;
            }
        }
        (top, held)
    };
    let mut results = [(0, 0); 4];
    for (round, n) in [8usize, 700].into_iter().enumerate() {
        let (top, held) = build(n);
        let calls: [(&str, &dyn Fn()); 3] = [
            ("job_signal", &|| {
                job_signal(top, RequestKind::Custom(7), 0).expect("signal");
            }),
            ("job_suspend", &|| job_suspend(top).expect("suspend")),
            ("job_resume", &|| job_resume(top).expect("resume")),
        ];
        for (at, (_, call)) in calls.iter().enumerate() {
            let (_, ns) = measure(call);
            if round == 0 {
                results[at].0 = ns;
            } else {
                results[at].1 = ns;
            }
        }
        let (_, ns) = measure(|| job_kill(top).expect("kill"));
        if round == 0 {
            results[3].0 = ns;
        } else {
            results[3].1 = ns;
        }
        for job in held {
            handle_close(job).expect("close");
        }
        handle_close(top).expect("close");
    }
    for (at, site) in ["job_signal", "job_suspend", "job_resume", "job_kill"]
        .into_iter()
        .enumerate()
    {
        check(site, results[at].0, results[at].1);
    }
}

fn teardown_sites() {
    teardown("a process holding 24 MiB", b"hold-short", b"hold-long");
    teardown("400 objects of 256 KiB", b"crowd-short", b"crowd-long");
    teardown(
        "256 mappings and 1024 handles",
        b"crowd-mapped-short",
        b"crowd-mapped-long",
    );
}

/// A child that holds many objects of 64 pages (256 KiB), up to 400 or as
/// many as half the free frames allow, by handles alone, or by 256 mappings
/// besides and then small objects until its table is full (1024 handles).
/// Then it waits, to end by itself or to be killed.
fn crowd(mapped: bool, wait_ms: u64) -> i32 {
    let fit = frames_accounted() / 2 / 64;
    let count = if mapped { fit.min(256) } else { fit.min(400) };
    for _ in 0..count {
        let object = memory_create(STEP, MemoryFlags::NONE).expect("create");
        if mapped {
            memory_map(object, 0, STEP, MapFlags::WRITE).expect("map");
        }
    }
    if mapped {
        while memory_create(PAGE, MemoryFlags::NONE).is_ok() {}
    }
    nap(wait_ms);
    0
}

/// Runs the measurements inside a `posix` job, where `snapshot_process` is
/// allowed.
fn outer() -> i32 {
    let posix = job_create(JOB, JobFlags::POSIX).expect("posix job");
    let me = image("t_bound");
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, posix, me);
    e.handle(console_again());
    e.handle(HandleEntry {
        slot: JOB.slot(),
        handle: posix,
        rights: JobObject::RIGHTS,
        transfer: Transfer::Duplicate,
    });
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
    calibrate();
    memory_sites();
    check("snapshot_process", snapshot_of(STEP), snapshot_of(BIG));
    let (step, big) = chain_close();
    check("channel chain close", step, big);
    job_tree_sites();
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
        Some(b"crowd-short") => crowd(false, 50),
        Some(b"crowd-long") => crowd(false, 30_000),
        Some(b"crowd-mapped-short") => crowd(true, 50),
        Some(b"crowd-mapped-long") => crowd(true, 30_000),
        Some(b"contiguous-loop") => churn_contiguous(),
        Some(b"create-loop") => churn(false),
        Some(b"snapshot-loop") => churn(true),
        Some(b"inner") => inner(),
        _ => outer(),
    }
}

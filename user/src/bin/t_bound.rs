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
    MemoryFlags, Mode, OBJECT_TYPES, ObjectType, RequestKind, Rights, Status, channel_signals,
    job_signals,
};
use zoraya_user::{
    channel_create, channel_write, clock_monotonic, handle_close, handle_type, image, job_create,
    job_get_usage, job_kill, job_resume, job_signal, job_suspend, memory_create, memory_map,
    memory_unmap, object_get_signals, port_create, port_subscribe, println, process_exit,
    raw_syscall, sleep_until, snapshot_process, spawn, startup, thread_yield, wait_exit,
};

const MASKED_MAX_NS: u64 = 0x7013;
const MASKED_MAX_UNITS: u64 = 0x7015;
const CANCELLED: u64 = 0x7016;
const HEAP_BACKING: u64 = 0x7017;
const HEAP_LIVE: u64 = 0x700b;
const FORCE_SWITCH: u64 = 0x701a;
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

/// ADR-0270, ADR-0299, INV-0270-1: a port holds at most 1024 subscriptions and
/// a source has at most 1024, so the sorted insert, the walk of `set` (a job
/// signal that rises) and the close of the port are a fixed amount. One port,
/// one source, keys in descending order (each insert goes to the front of the
/// sorted list), against a port with 8; the call after the limit is refused.
fn subscription_sites() {
    let max = 1024u64;
    let mut results = [(0, 0); 3];
    for (round, n) in [8u64, max].into_iter().enumerate() {
        let job = job_create(JOB, JobFlags::NONE).expect("job");
        let port = port_create().expect("port");
        let mut worst = 0;
        for key in (0..n).rev() {
            let (r, ns) =
                measure(|| port_subscribe(port, job, job_signals::SUSPENDED, key, Mode::Edge));
            r.expect("subscribe");
            worst = worst.max(ns);
        }
        if n == max {
            let r = port_subscribe(port, job, job_signals::SUSPENDED, max, Mode::Edge);
            assert_eq!(r, Err(Status::LimitReached), "the 1025th subscription");
        }
        let (_, signal) = measure(|| job_suspend(job).expect("suspend"));
        let (_, close) = measure(|| handle_close(port).expect("close"));
        let r = [worst, signal, close];
        for at in 0..3 {
            if round == 0 {
                results[at].0 = r[at];
            } else {
                results[at].1 = r[at];
            }
        }
        handle_close(job).expect("close");
    }
    for (at, site) in ["port_subscribe", "signal of a source", "port close"]
        .into_iter()
        .enumerate()
    {
        check(site, results[at].0, results[at].1);
    }
}

/// A child for `many_subscriptions_sites`: a port subscribed to a job's source
/// `count` times, in the order that makes the last handle the one that
/// matters (`port_first`: the port is made first, so the job's handle is
/// closed first and frees a source with `count` subscribers).
fn full_pair(count: u64, port_first: bool) -> (Handle, Handle) {
    let (port, job) = if port_first {
        let port = port_create().expect("port");
        (port, job_create(JOB, JobFlags::NONE).expect("job"))
    } else {
        let job = job_create(JOB, JobFlags::NONE).expect("job");
        (port_create().expect("port"), job)
    };
    for key in 0..count {
        port_subscribe(port, job, job_signals::SUSPENDED, key, Mode::Edge).expect("subscribe");
    }
    (port, job)
}

/// Review error 4 (TASK-26 fix): a process that holds 16 ports of 1024
/// subscriptions, each to a job of its own (so each job's source has 1024
/// subscribers), half of them with the port closed first and half with the
/// job. Its exit closes 32 handles. (The review's case was 64 ports; the test machine
/// has 128 MiB and the kernel heap does not give frames back, so 16 ports, 16k
/// subscriptions.) The bound of 1024 per port and per source does not bound
/// those together: each handle's weight scales with its
/// subscriptions (`object::weight`), so the exit pauses between them.
fn full_ports() -> i32 {
    for i in 0..16 {
        full_pair(1024, i % 2 == 0);
    }
    0
}

/// Review round 2, error 1: 16 jobs whose sources have 1024 subscribers each
/// (and their ports), all moved in one `channel_write`. Ending the old
/// handles' subscriptions is paced like a close (`object::weight`), with no
/// lock held; unpaced, the write was 1024 units between two step points.
fn moved_jobs() -> i32 {
    let (a, _b) = channel_create().expect("channel");
    let mut entries = alloc::vec::Vec::new();
    for i in 0..16 {
        let (_, job) = full_pair(1024, i % 2 == 0);
        entries.push(ChannelEntry {
            handle: job,
            rights: rights_of(job),
            transfer: Transfer::Move,
        });
    }
    channel_write(a, b"", &entries).expect("write");
    0
}

/// The child of `peer_closing_in_a_write`: holds the peer end (slot 3) and
/// closes it as soon as the flag in the shared page (slot 4) is set. With
/// `FORCE_SWITCH` the writer's first pause hands it the CPU.
fn closer() -> i32 {
    let end = Handle::new(3).expect("slot");
    let object = Handle::new(4).expect("slot");
    let at = memory_map(object, 0, PAGE, MapFlags::WRITE).expect("map");
    let gave_up = clock_monotonic() + 60_000_000_000;
    // SAFETY: the first word of the page mapped above; the parent writes it.
    while unsafe { (at as *const u64).read_volatile() } == 0 {
        if clock_monotonic() > gave_up {
            return 1;
        }
        thread_yield();
    }
    handle_close(end).expect("close");
    0
}

fn heap_live() -> u64 {
    raw_syscall(HEAP_LIVE, [0; 6]).1
}

/// Review round 3, error 1: `channel_write` queues its message right after
/// `prepare`, with nothing that pauses in between, and the moved handles'
/// subscriptions end after it. A writer moving heavy jobs and a channel end
/// pauses at each step (forced to switch here); the peer end is closed by
/// another process at the first of them. The message either is refused
/// (`PeerClosed`: the close came first) or was queued before the close and
/// is drained by it: then the carried end's peer must see `PEER_CLOSED`,
/// and the heap must return to where it was (nothing queued into a closed
/// end, never drained). Before the fix the message was queued into the
/// closed end and the carried end was never closed.
fn peer_closing_in_a_write() {
    let room = job_create(JOB, JobFlags::POSIX).expect("room");
    let mut start = 0;
    let mut accepted = 0;
    for round in 0..4 {
        let page = memory_create(PAGE, MemoryFlags::NONE).expect("page");
        let at = memory_map(page, 0, PAGE, MapFlags::WRITE).expect("map");
        let (a, b) = channel_create().expect("channel");
        let (x, y) = channel_create().expect("carried");
        let mut buffer = [0; 512];
        let program = image("t_bound");
        let mut e = Encoder::new(&mut buffer, room, program);
        e.handle(console_again());
        e.handle(HandleEntry {
            slot: 3,
            handle: b,
            rights: rights_of(b),
            transfer: Transfer::Move,
        });
        e.handle(HandleEntry {
            slot: 4,
            handle: page,
            rights: rights_of(page),
            transfer: Transfer::Duplicate,
        });
        e.arg(b"closer");
        let length = e.finish().expect("room");
        let process = spawn(&buffer[..length]).expect("spawn closer");
        handle_close(program).expect("image");
        let mut entries = alloc::vec::Vec::new();
        entries.push(ChannelEntry {
            handle: x,
            rights: rights_of(x),
            transfer: Transfer::Move,
        });
        let mut ports = alloc::vec::Vec::new();
        let mut jobs = alloc::vec::Vec::new();
        for i in 0..4 {
            let (port, job) = full_pair(1024, i % 2 == 0);
            ports.push(port);
            jobs.push(job);
            entries.push(ChannelEntry {
                handle: job,
                rights: rights_of(job),
                transfer: Transfer::Move,
            });
        }
        raw_syscall(FORCE_SWITCH, [1, 0, 0, 0, 0, 0]);
        // SAFETY: the first word of the page mapped above; the child reads it.
        unsafe { (at as *mut u64).write_volatile(1) };
        let result = channel_write(a, b"", &entries);
        raw_syscall(FORCE_SWITCH, [0; 6]);
        match result {
            Ok(_) => {
                accepted += 1;
                let mut tries = 0;
                while object_get_signals(y).expect("signals") & channel_signals::PEER_CLOSED == 0 {
                    assert!(
                        tries < 500,
                        "round {round}: the carried end's peer is never closed"
                    );
                    tries += 1;
                    nap(10);
                }
            }
            Err(Status::PeerClosed) => {
                handle_close(x).expect("close");
                for job in jobs {
                    handle_close(job).expect("close");
                }
            }
            Err(status) => panic!("round {round}: write: {status:?}"),
        }
        assert_eq!(wait_exit(process).code, 0, "round {round}: closer");
        handle_close(process).expect("close");
        for h in ports.into_iter().chain([a, y, page]) {
            handle_close(h).expect("close");
        }
        // The first round lets the heap take the room it will use.
        let mut tries = 0;
        if round == 0 {
            start = heap_live();
        }
        while heap_live() != start && tries < 200 {
            nap(10);
            tries += 1;
        }
        assert_eq!(heap_live(), start, "round {round}: heap back");
    }
    println!("t_bound: a peer closing during a write: {accepted} of 4 accepted, nothing left");
    handle_close(room).expect("close");
}

/// Review error 3 (TASK-26 fix): a job's source with a subscriber on each of
/// 32 ports, each port holding 1024 entries with the job's at the front of
/// its sorted list (the worst place to remove from), and the other 1023
/// entries to 128 other jobs (a source takes 1024). One 32nd of the
/// product the bounds allow (1024 ports; memory as above); closing the job's handle frees the
/// source and removes its 32 entries.
fn wide_source() -> i32 {
    const PORTS: usize = 32;
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let others: alloc::vec::Vec<Handle> = (0..128)
        .map(|_| job_create(JOB, JobFlags::NONE).expect("other"))
        .collect();
    for _ in 0..PORTS {
        let port = port_create().expect("port");
        port_subscribe(port, job, job_signals::SUSPENDED, 0, Mode::Edge).expect("subscribe");
        for at in 0..1023usize {
            let other = others[at / 8];
            port_subscribe(
                port,
                other,
                job_signals::SUSPENDED,
                1 + at as u64,
                Mode::Edge,
            )
            .expect("subscribe");
        }
    }
    let (_, ns) = measure(|| handle_close(job).expect("close"));
    println!(
        "t_bound: free_source of 32 subscribers on ports of 1024: {} us masked",
        ns / 1000
    );
    let limit = bound_ns();
    assert!(ns <= limit, "free_source: {ns} ns masked, limit {limit}");
    0
}

/// ADR-0299, review errors 3 and 4: the two bounds are on a port and on a
/// source, and what an exit or a `free_source` does is their product.
fn many_subscriptions_sites() {
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    for (label, role) in [
        ("free_source", &b"wide-source"[..]),
        ("exit with 16 full ports", &b"full-ports"[..]),
        ("16 moved jobs of 1024 subscribers", &b"moved-jobs"[..]),
    ] {
        let began = clock_monotonic();
        let (p, ns) = measure(|| {
            let p = child(room, role).expect("spawn");
            let record = wait_exit(p);
            assert_eq!(record.code, 0, "{label}: the child");
            p
        });
        handle_close(p).expect("close");
        println!(
            "t_bound: {label}: {} us masked at most, run took {} ms",
            ns / 1000,
            (clock_monotonic() - began) / 1_000_000
        );
        let limit = bound_ns();
        assert!(ns <= limit, "{label}: {ns} ns masked, limit {limit}");
    }
    handle_close(room).expect("room");
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
    subscription_sites();
    many_subscriptions_sites();
    peer_closing_in_a_write();
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
        Some(b"wide-source") => wide_source(),
        Some(b"full-ports") => full_ports(),
        Some(b"moved-jobs") => moved_jobs(),
        Some(b"closer") => closer(),
        Some(b"inner") => inner(),
        _ => outer(),
    }
}

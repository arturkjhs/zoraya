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
    channel_create, channel_read, channel_write, clock_monotonic, handle_close, handle_type, image,
    job_create, job_get_usage, job_kill, job_resume, job_signal, job_suspend, memory_create,
    memory_map, memory_unmap, object_get_signals, port_create, port_subscribe, println,
    process_exit, raw_syscall, sleep_until, snapshot_process, spawn, startup, thread_yield,
    wait_exit,
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
/// `calibrate`; 1 under HVF on the reference host. It scales the host-speed
/// floors of some checks; it does not choose the time bound (`on_tcg`).
static FACTOR: AtomicU64 = AtomicU64::new(1);

/// The test kernel's `MIDR_EL1`.
const MIDR: u64 = 0x701d;
/// 0 not read yet, 1 HVF (or any CPU but TCG's), 2 TCG.
static ACCELERATOR: AtomicU64 = AtomicU64::new(0);

/// True under TCG: `tools/run-qemu.sh` gives TCG a Cortex-A72 and HVF the
/// host's CPU. The time bound follows the accelerator, not a calibration (a
/// fill on a busy HVF host once took 11167 ns against the reference's 10000
/// and turned the 2 ms bound into 1 s). Any CPU but the Cortex-A72 is held
/// to the 2 ms bound, so a mistake fails loudly.
fn on_tcg() -> bool {
    if ACCELERATOR.load(Ordering::Relaxed) == 0 {
        let midr = raw_syscall(MIDR, [0; 6]).1;
        let cortex_a72 = (midr >> 24) & 0xff == 0x41 && (midr >> 4) & 0xfff == 0xd08;
        ACCELERATOR.store(if cortex_a72 { 2 } else { 1 }, Ordering::Relaxed);
    }
    ACCELERATOR.load(Ordering::Relaxed) == 2
}

/// Times a fixed amount of work (a 256 KiB fill, the size of a step's
/// pages, five times, the best) against the reference: the factor scales
/// the host-speed floors (as the shared snapshot's).
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
    let accelerator = if on_tcg() { "TCG" } else { "HVF" };
    let limit_ms = bound_ns() / 1_000_000;
    println!(
        "t_bound: fill of 256 KiB takes {best} ns: factor {factor}; {accelerator} (by the CPU model), time bound {limit_ms} ms"
    );
    if factor > 1 && !on_tcg() {
        println!(
            "t_bound: factor {factor} under HVF: the host ran the fill {factor} times slower than the reference (busy or slow); the bound stays 2 ms"
        );
    }
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
    if on_tcg() {
        return SLOW_HOST_LIMIT_NS;
    }
    BOUND_NS
}

/// Masked-time checks over the limit off HVF in this process (`time_ok`).
static OVER: AtomicU64 = AtomicU64::new(0);
/// The exit code of a run or a child that went over the time limit.
const OVER_TIME: i32 = 3;

/// A case's longest masked interval against `bound_ns`. The guest's clock
/// runs while the host stalls the vCPU, under TCG (6 of 10 rounds under 10
/// CPU hogs went over 1 s at different sites, with their units in bound) and
/// under HVF too (a preemption of the vCPU thread, three guests in parallel
/// lanes): a run over the limit is noted and repeated (`outer`), and fails if
/// three runs in a row go over. A kernel loop repeats, a stall does not; a
/// time-only loop that shows in one run of three passes about 26 times in 27,
/// which is why the units carry the claim for work that is counted: they are
/// checked in every run (`measure`) and fail at once.
fn time_ok(site: &str, ns: u64) {
    let limit = bound_ns();
    if ns <= limit {
        return;
    }
    println!("t_bound: {site}: {ns} ns masked, over the limit {limit}: the run is repeated");
    OVER.fetch_add(1, Ordering::Relaxed);
}

/// This process's verdict on time: 0, or `OVER_TIME`.
fn time_verdict() -> i32 {
    if OVER.load(Ordering::Relaxed) > 0 {
        OVER_TIME
    } else {
        0
    }
}

/// A child's record: its own time verdict counts as this process's.
fn child_ended(label: &str, record: zoraya_abi::ExitRecord) {
    if record.code == OVER_TIME {
        OVER.fetch_add(1, Ordering::Relaxed);
        return;
    }
    assert_eq!(record.code, 0, "{label}: {record:?}");
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
    time_ok(site, big);
}

fn child(job: Handle, role: &[u8]) -> Result<Handle, Status> {
    child_with(job, role, None)
}

/// The slot where a teardown child finds the channel end it reports on.
const BUILT_SLOT: u32 = 20;

/// As `child`, with `built` moved to `BUILT_SLOT`: the child writes one
/// message there once it holds all it was to build (`built`).
fn child_with(job: Handle, role: &[u8], built: Option<Handle>) -> Result<Handle, Status> {
    let mut buffer = [0; 512];
    let program = image("t_bound");
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_again());
    if let Some(end) = built {
        e.handle(HandleEntry {
            slot: BUILT_SLOT,
            handle: end,
            rights: rights_of(end),
            transfer: Transfer::Move,
        });
    }
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
    time_ok("the refusal", ns);
    assert_eq!(free_frames(), free, "a refusal keeps nothing");
}

/// `shared`: the object is mapped `SHARED` (ADR-0431), whose pages a
/// snapshot walks and does not copy (INV-0270-1 still holds).
fn snapshot_of(size: u64, shared: bool) -> u64 {
    let object = memory_create(size, MemoryFlags::NONE).expect("create");
    let flags = if shared {
        MapFlags::WRITE.union(MapFlags::SHARED)
    } else {
        MapFlags::WRITE
    };
    let at = memory_map(object, 0, size, flags).expect("map");
    // A shared object mapped many times: the walk meets all those pages and
    // copies none, and one step must still be bounded (INV-0270-1).
    let more: alloc::vec::Vec<u64> = if shared {
        (0..SHARED_COPIES)
            .map(|_| memory_map(object, 0, size, flags).expect("map again"))
            .collect()
    } else {
        alloc::vec::Vec::new()
    };
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
    for m in more {
        memory_unmap(m, size).expect("unmap");
    }
    memory_unmap(at, size).expect("unmap");
    handle_close(object).expect("close");
    ns
}

/// Mappings the process holds in the cases of 4096 (ADR-0415 item 1,
/// INV-0415-1): the limit less the few the program, its stack and the case's
/// own take.
const MANY: usize = zoraya_abi::MAPPING_SLOTS - 64;

/// The cases of a process with the most mappings it may hold, each a step's
/// work between two step points (INV-0270-1, ADR-0415 item 1): `memory_map`
/// that searches all of them, `memory_unmap` that ends one, `snapshot_process`
/// that plans, copies and records them, and the process's exit and kill that
/// drop them one by one. Then the same with every mapping in a page-table of
/// its own (the layout that makes one table frame for each page), and
/// the refusal at the limit with nothing changed.
fn many_mappings_sites() {
    // The cases before left 40 unmapped mappings of 24 MiB: a thousand empty
    // page-tables, which every snapshot from here walks (INV-0270-1: in steps
    // of `WALK_READS` descriptors, counted).
    let small = best_snapshot();
    let object = memory_create(PAGE, MemoryFlags::NONE).expect("create");
    let mut places = alloc::vec::Vec::new();
    let (mut first, mut worst) = (u64::MAX, 0);
    for _ in 0..MANY {
        let (at, ns) = measure(|| memory_map(object, 0, PAGE, MapFlags::READ).expect("map"));
        first = first.min(ns);
        worst = worst.max(ns);
        places.push(at);
    }
    check("memory_map, up to 4032 mappings", first, worst);
    // A hole in the middle: the next map finds it by the walk from the top.
    let hole = places.swap_remove(MANY / 2);
    let (_, unmap_ns) = measure(|| memory_unmap(hole, PAGE).expect("unmap"));
    let (again, map_ns) = measure(|| memory_map(object, 0, PAGE, MapFlags::READ).expect("map"));
    assert_eq!(again, hole, "the lowest place is the hole");
    places.push(again);
    check("memory_unmap of one of 4032", unmap_ns, unmap_ns);
    check("memory_map into a hole of 4032", map_ns, map_ns);
    // Up to the limit: the next is refused, and nothing is left behind.
    let mut at_limit = alloc::vec::Vec::new();
    let refused = loop {
        match memory_map(object, 0, PAGE, MapFlags::READ) {
            Ok(at) => at_limit.push(at),
            Err(status) => break status,
        }
    };
    assert_eq!(refused, Status::LimitReached, "the limit");
    assert!(
        at_limit.len() <= 64,
        "{} mappings besides the {MANY}: the program's own are few",
        at_limit.len()
    );
    let (r, ns) = measure(|| memory_map(object, 0, PAGE, MapFlags::READ));
    assert_eq!(r.err(), Some(Status::LimitReached), "still the limit");
    time_ok("the refusal", ns);
    for at in at_limit.drain(..) {
        memory_unmap(at, PAGE).expect("unmap");
    }

    // A snapshot of the process with the mappings of the plan, the copy and
    // the record, and the end of the child that drops them.
    let large = best_snapshot();
    check("snapshot_process, 4032 mappings", small, large);
    for at in places.drain(..) {
        memory_unmap(at, PAGE).expect("unmap");
    }
    handle_close(object).expect("close");

    // The same, one mapping in each table of the page-table: every page of
    // the copy brings its own table frames (INV-0270-1: a step is 64 frames,
    // not 64 pages).
    let (spread, spacers) = spread_mappings(MANY / 2);
    let large = best_snapshot();
    check(
        "snapshot_process, a table for each of 2016 pages",
        small,
        large,
    );
    for at in spread {
        memory_unmap(at, PAGE).expect("unmap");
    }
    for object in spacers {
        handle_close(object).expect("close");
    }

    // 22 mappings of one page, each in a gigabyte of its own (two tables
    // each): a child builds them, so that its tables go with it. Placed by
    // the test kernel's floor, they need 44 table frames, so every guest runs
    // it (with spacer mappings it needed 45 MB of tables: 2 GiB guests only).
    let p = child(JOB, b"far-pages").expect("spawn");
    let record = wait_exit(p);
    handle_close(p).expect("close");
    child_ended("far pages", record);

    // `memory_unmap` of a mapping whose handle was closed frees the object's
    // frames in the call (up to a step; more go to the reaper): one step
    // point keeps them out of the interval of the unmapping and the list.
    let (mut worst, mut worst_big) = (0, 0);
    for (size, worst) in [(STEP, &mut worst), (BIG, &mut worst_big)] {
        for _ in 0..3 {
            let object = memory_create(size, MemoryFlags::NONE).expect("create");
            let at = memory_map(object, 0, size, MapFlags::WRITE).expect("map");
            handle_close(object).expect("close: the mapping holds the object");
            let ((), ns) = measure(|| memory_unmap(at, size).expect("unmap"));
            *worst = (*worst).max(ns);
        }
    }
    check("memory_unmap of the last holder", worst, worst_big);
}

/// 22 one-page mappings 1 GiB apart: each brings two page-table frames to
/// the copy of a snapshot (`SnapshotBuild::finish`: a step is 64 units, and
/// 22 such mappings are 88 without the count of the tables).
fn far_pages() -> i32 {
    const GIB: u64 = 1 << 30;
    let page = memory_create(PAGE, MemoryFlags::NONE).expect("create");
    // A gigabyte of empty page-tables under a valid table descriptor: a page
    // in each 2 MiB, mapped and unmapped (tables left empty stay allocated),
    // so that the snapshot's walk reads 512 empty tables in counted steps
    // (the old spacers left 11 264 of them, 5.8 million descriptors).
    for i in 0..512 {
        let at = map_page_at(page, SPARSE_BASE + 22 * GIB + i * (2 << 20));
        memory_unmap(at, PAGE).expect("unmap");
    }
    for k in 0..22 {
        map_page_at(page, SPARSE_BASE + k * GIB);
    }
    let large = best_snapshot();
    println!(
        "t_bound: snapshot_process, 22 pages 1 GiB apart: {} us masked",
        large / 1000
    );
    time_ok("snapshot_process, 22 pages 1 GiB apart", large);
    time_verdict()
}

/// The test kernel's floor for the next mappings' addresses (0: none).
const SET_MAP_FLOOR: u64 = 0x701c;
/// Where the sparse layouts below begin: 16 TiB above the lowest user
/// address, far above every mapping the cases make, below `MAP_HIGH`.
const SPARSE_BASE: u64 = (1 << 46) + (1 << 44);

/// Maps one page of `page` at `va`, by the test kernel's floor: the place a
/// spacer mapping would have made, without the page-table entry for every
/// page a spacer spans (22 GiB of them in `far_pages` took 22 s under TCG).
fn map_page_at(page: Handle, va: u64) -> u64 {
    raw_syscall(SET_MAP_FLOOR, [va, 0, 0, 0, 0, 0]);
    let at = memory_map(page, 0, PAGE, MapFlags::READ).expect("page");
    raw_syscall(SET_MAP_FLOOR, [0; 6]);
    assert_eq!(at, va, "the floor places the page");
    at
}

/// `count` one-page mappings, each in its own 2 MiB of address space, so
/// each in a page-table of its own. Gives the pages and the objects to
/// close.
fn spread_mappings(count: usize) -> (alloc::vec::Vec<u64>, alloc::vec::Vec<Handle>) {
    const REGION: u64 = 2 << 20;
    let page = memory_create(PAGE, MemoryFlags::NONE).expect("create");
    let pages = (0..count as u64)
        .map(|i| map_page_at(page, SPARSE_BASE + i * REGION))
        .collect();
    (pages, alloc::vec![page])
}

/// A snapshot's longest masked interval; the best of three if the first is
/// over the bound: a pause of the host, or work of another thread inside the
/// interval, is not the kernel holding IRQ masked, and a regression shows in
/// all three (as in `memory_sites`). The units of each are checked
/// (`measure`).
fn best_snapshot() -> u64 {
    let first = snapshot_with();
    if first <= bound_ns() {
        return first;
    }
    (0..2).map(|_| snapshot_with()).fold(first, u64::min)
}

/// A snapshot of this process into a suspended child, and its end: the longest
/// masked interval of the call (the units of every interval are checked by
/// `measure`).
fn snapshot_with() -> u64 {
    let job = job_create(JOB, JobFlags::POSIX).expect("job");
    let mut buffer = alloc::vec![0u8; 512];
    let mut e = SnapshotEncoder::new(&mut buffer, job);
    e.suspended();
    let length = e.finish().expect("room");
    let (child, ns) = measure(|| snapshot_process(&buffer[..length]).expect("snapshot"));
    let Some(child) = child else { process_exit(0) };
    // The child's end drops every mapping it was given, one by one.
    let (_, end_ns) = measure(|| {
        job_kill(job).expect("kill");
        wait_exit(child);
    });
    println!(
        "t_bound: snapshot_with: the call {} us masked, the child's end {} us; {} pages copied in {} us",
        ns / 1000,
        end_ns / 1000,
        raw_syscall(0x700f, [0; 6]).1,
        raw_syscall(0x700e, [0; 6]).1 / 1000
    );
    time_ok("the snapshot child's end", end_ns);
    handle_close(child).expect("close");
    handle_close(job).expect("job");
    ns
}

/// More mappings of one shared object than a limited job's tables would
/// allow it to copy.
const SHARED_COPIES: usize = 40;

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
    built();
    nap(wait_ms);
    0
}

/// Tells the parent that this child holds all it was to build, if it was
/// given the channel end (the long children of `teardown`).
fn built() {
    if let Some(end) = Handle::from_parts(BUILT_SLOT, 0) {
        let _ = channel_write(end, b"built", &[]);
    }
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

/// A child that maps and unmaps one `BIG` object without end: a kill in the
/// middle of a `memory_map` undoes what it mapped, in steps (INV-0270-1).
fn churn_maps() -> i32 {
    let object = memory_create(BIG, MemoryFlags::NONE).expect("create");
    loop {
        let at = memory_map(object, 0, BIG, MapFlags::WRITE).expect("map");
        memory_unmap(at, BIG).expect("unmap");
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
    for role in [
        &b"create-loop"[..],
        b"snapshot-loop",
        b"contiguous-loop",
        b"map-loop",
    ] {
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
    units(true);
    job_kill(room).expect("kill");
    let record = wait_exit(p);
    assert_eq!(record.reason, ExitReason::Killed, "{role:?}");
    // The kill and what it undoes (a `memory_map` cut in the middle, a
    // snapshot's child): the work between two step points (INV-0270-1).
    let kill_units = units(false);
    if kill_units > MAX_UNITS {
        raw_syscall(DIAGNOSE_UNITS, [0; 6]);
    }
    assert!(
        kill_units <= MAX_UNITS,
        "{role:?}: {kill_units} units of work between two step points in the kill, limit {MAX_UNITS}"
    );
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
        let ns = masked(true);
        println!(
            "t_bound: job_kill during {}: nothing left; {} us masked, {} units",
            core::str::from_utf8(role).unwrap_or("?"),
            ns / 1000,
            units(true)
        );
        time_ok("job_kill during a loop", ns);
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
    // The long child says when it holds all it builds (a fixed wait was too
    // long under HVF and could be too short on a stalled host; its charge
    // comes before its frames). Within 25 s: it ends by itself after 30.
    let (ours, theirs) = channel_create().expect("channel");
    let p = child_with(room, long, Some(theirs)).expect("spawn");
    let began = clock_monotonic();
    loop {
        let signals = object_get_signals(ours).expect("signals");
        if signals & channel_signals::READABLE != 0 {
            break;
        }
        if signals & channel_signals::PEER_CLOSED != 0 {
            // It ended before it said it was built.
            let record = wait_exit(p);
            panic!("{label}: the long child ended before its build: {record:?}");
        }
        assert!(
            clock_monotonic() - began < 25_000_000_000,
            "{label}: the long child did not finish its build in 25 s"
        );
        nap(5);
    }
    handle_close(ours).expect("close");
    masked(true);
    units(true);
    job_kill(room).expect("kill");
    wait_exit(p);
    // The reaper's part of the kill is in the interval too: until it is idle.
    let _ = frames_accounted();
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
    time_ok(label, exit_ns);
    time_ok(label, kill_ns);
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
    for round in 0..6 {
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
        memory_unmap(at, PAGE).expect("unmap");
        for h in ports.into_iter().chain([a, y, page]) {
            handle_close(h).expect("close");
        }
        // Round 0 lets the heap take the room it will use; after it the
        // heap returns exactly (an earlier version allowed a residue of 150
        // bytes a round, which was this test's own unmapped page).
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
    println!("t_bound: a peer closing during a write: {accepted} of 6 accepted, nothing left");
    handle_close(room).expect("close");
}

/// The child of `moved_memory_is_freed_a_step_at_a_time`: reads the message
/// on slot 3 until it arrives and closes every handle in it.
fn reader() -> i32 {
    let end = Handle::new(3).expect("slot");
    let gave_up = clock_monotonic() + 60_000_000_000;
    let mut handles = [end; 64];
    loop {
        match channel_read(end, &mut [], &mut handles) {
            Ok(sizes) => {
                for h in &handles[..sizes.handles as usize] {
                    handle_close(*h).expect("close");
                }
                return 0;
            }
            Err(_) => {
                if clock_monotonic() > gave_up {
                    return 1;
                }
                thread_yield();
            }
        }
    }
}

/// Review round 4, error 1: 64 memory objects of 63 pages moved in one write
/// to a reader that closes them as soon as it can (a forced switch at the
/// writer's first pause). The writer's own references to the moved objects must
/// not be the last ones when they are all dropped at the end: each is dropped
/// after its own announcement, so no interval frees more than a step.
fn moved_memory_is_freed_a_step_at_a_time() {
    let room = job_create(JOB, JobFlags::POSIX).expect("room");
    let (a, b) = channel_create().expect("channel");
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
    e.arg(b"reader");
    let length = e.finish().expect("room");
    let process = spawn(&buffer[..length]).expect("spawn reader");
    handle_close(program).expect("image");
    let mut entries = alloc::vec::Vec::new();
    for _ in 0..64 {
        let object = memory_create(63 * PAGE, MemoryFlags::NONE).expect("create");
        entries.push(ChannelEntry {
            handle: object,
            rights: rights_of(object),
            transfer: Transfer::Move,
        });
    }
    // Let the reader reach its loop, then reset the meter.
    nap(50);
    units(true);
    raw_syscall(FORCE_SWITCH, [1, 0, 0, 0, 0, 0]);
    channel_write(a, b"", &entries).expect("write");
    raw_syscall(FORCE_SWITCH, [0; 6]);
    assert_eq!(wait_exit(process).code, 0, "reader");
    handle_close(process).expect("close");
    let most = units(false);
    println!("t_bound: 64 moved memory objects freed by the reader: {most} units in one interval");
    assert!(
        most <= MAX_UNITS,
        "{most} units in one interval, limit {MAX_UNITS}"
    );
    handle_close(a).expect("close");
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
    time_ok("free_source", ns);
    time_verdict()
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
            child_ended(label, wait_exit(p));
            p
        });
        handle_close(p).expect("close");
        println!(
            "t_bound: {label}: {} us masked at most, run took {} ms",
            ns / 1000,
            (clock_monotonic() - began) / 1_000_000
        );
        time_ok(label, ns);
    }
    handle_close(room).expect("room");
}

fn teardown_sites() {
    teardown("a process holding 24 MiB", b"hold-short", b"hold-long");
    teardown("400 objects of 256 KiB", b"crowd-short", b"crowd-long");
    teardown("4000 mappings of 4000 objects", b"maps-short", b"maps-long");
    teardown(
        "256 mappings and 1024 handles",
        b"crowd-mapped-short",
        b"crowd-mapped-long",
    );
}

/// A child that maps 4000 objects of one page each and closes their handles
/// (ADR-0415 item 1: what holds each object is its mapping), then waits, to end
/// by itself or to be killed.
fn maps(wait_ms: u64) -> i32 {
    for _ in 0..4000 {
        let object = memory_create(PAGE, MemoryFlags::NONE).expect("create");
        memory_map(object, 0, PAGE, MapFlags::WRITE).expect("map");
        handle_close(object).expect("close");
    }
    built();
    nap(wait_ms);
    0
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
        // Until a create is refused; the last one made is closed again, so
        // that the message saying it is built has room.
        let mut last = None;
        while let Ok(object) = memory_create(PAGE, MemoryFlags::NONE) {
            last = Some(object);
        }
        if let Some(object) = last {
            handle_close(object).expect("close");
        }
    }
    built();
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
    // A run over the time limit is repeated, up to three runs
    // (`time_ok`).
    let mut code = OVER_TIME;
    for run in 1..=3 {
        let child = spawn(&buffer[..length]).expect("spawn inner");
        code = wait_exit(child).code;
        handle_close(child).expect("close");
        if code != OVER_TIME {
            break;
        }
        println!("t_bound: run {run} went over the time limit");
    }
    handle_close(me).expect("close");
    handle_close(posix).expect("close");
    code
}

fn inner() -> i32 {
    calibrate();
    memory_sites();
    check(
        "snapshot_process",
        snapshot_of(STEP, false),
        snapshot_of(BIG, false),
    );
    let (small, large) = (snapshot_of(STEP, true), snapshot_of(BIG, true));
    check("snapshot_process, shared", small, large);
    // Where the time limit is loose (a slow host), the walk of the shared
    // pages must still not grow with their number: a skipped page is a unit
    // of the step (INV-0270-1), so a big mapping is as quick as a small one.
    let floor = 1_000_000 * FACTOR.load(Ordering::Relaxed);
    assert!(
        large <= 10 * small.max(floor),
        "shared snapshot grows with the pages: {small} ns, {large} ns"
    );
    many_mappings_sites();
    let (step, big) = chain_close();
    check("channel chain close", step, big);
    job_tree_sites();
    subscription_sites();
    many_subscriptions_sites();
    peer_closing_in_a_write();
    moved_memory_is_freed_a_step_at_a_time();
    teardown_sites();
    a_kill_between_steps_leaves_nothing();
    if time_verdict() == 0 {
        println!("t_bound: all passed");
    }
    time_verdict()
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    match startup().argv.next() {
        Some(b"hold-short") => hold(50),
        Some(b"hold-long") => hold(30_000),
        Some(b"far-pages") => far_pages(),
        Some(b"maps-short") => maps(50),
        Some(b"maps-long") => maps(30_000),
        Some(b"crowd-short") => crowd(false, 50),
        Some(b"crowd-long") => crowd(false, 30_000),
        Some(b"crowd-mapped-short") => crowd(true, 50),
        Some(b"crowd-mapped-long") => crowd(true, 30_000),
        Some(b"contiguous-loop") => churn_contiguous(),
        Some(b"map-loop") => churn_maps(),
        Some(b"create-loop") => churn(false),
        Some(b"snapshot-loop") => churn(true),
        Some(b"wide-source") => wide_source(),
        Some(b"full-ports") => full_ports(),
        Some(b"moved-jobs") => moved_jobs(),
        Some(b"closer") => closer(),
        Some(b"reader") => reader(),
        Some(b"inner") => inner(),
        _ => outer(),
    }
}

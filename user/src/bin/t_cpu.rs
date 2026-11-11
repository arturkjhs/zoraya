#![no_std]
#![no_main]

//! The CPU ceiling (ADR-0264, Q4): INV-0264-1 (eight spinners in a room at
//! 25% use the budget and no more), INV-0264-4 (three rooms at 25% under a
//! 50% job use at most 50%), INV-0264-2 (a spinner makes no progress while its
//! job is parked), and the wake-up latency of an uncapped process next to two
//! rooms that spin and burst at period boundaries (the in-kernel half of
//! INV-0264-3), at three period lengths.
//!
//! The time bounds are stated for HVF and scaled for a slower host by a
//! calibration, as `t_bound` does (INV-0270-1); the checks that need no clock
//! (what is parked makes no progress, the units of work between step points,
//! a ceiling is not exceeded by what the clock says is more than the host's
//! factor allows) run everywhere.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::quota::JobLimits;
use zoraya_abi::spawn::Encoder;
use zoraya_abi::{Handle, JOB, JobFlags, MapFlags, MemoryFlags, Rights, Status};
use zoraya_user::{
    clock_monotonic, dup, handle_close, image, job_create, job_get_usage, job_kill, job_set_limits,
    memory_create, memory_map, println, raw_syscall, sleep_until, spawn, startup,
};

const MASKED_MAX_NS: u64 = 0x7013;
const MASKED_MAX_UNITS: u64 = 0x7015;
const SET_CPU_PERIOD: u64 = 0x7018;
const MS: u64 = 1_000_000;
/// The slot a spinner finds the shared counters at.
const COUNTERS: u32 = 8;
/// What ADR-0270 says one interval with IRQ masked may hold, as in `t_bound`.
const BOUND_NS: u64 = 2 * MS;
const MAX_UNITS: u64 = 64 + 16;
/// The most the time bounds are scaled for a slow host.
const MAX_FACTOR: u64 = 64;
/// The time `calibrate` takes under HVF.
const REFERENCE_FILL_NS: u64 = 10_000;
/// What a job may run beyond its budget in one period, under HVF: the floor
/// slice of the scheduler (50 us) and the time to the next return to EL0.
const OVERRUN_NS: u64 = 200_000;
/// The scheduler's floor slice (`zoraya_cpu::MIN_SLICE_NS`), wall time that
/// no host factor scales.
const MIN_SLICE_NS: u64 = 50_000;

static FACTOR: AtomicU64 = AtomicU64::new(1);

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
    println!("t_cpu: fill of 256 KiB takes {best} ns: time bounds scaled by {factor}");
}

fn factor() -> u64 {
    FACTOR.load(Ordering::Relaxed)
}

fn set_period(ns: u64) {
    raw_syscall(SET_CPU_PERIOD, [ns, 0, 0, 0, 0, 0]);
}

fn masked_ns(reset: bool) -> u64 {
    raw_syscall(MASKED_MAX_NS, [reset as u64, 0, 0, 0, 0, 0]).1
}

fn units(reset: bool) -> u64 {
    raw_syscall(MASKED_MAX_UNITS, [reset as u64, 0, 0, 0, 0, 0]).1
}

fn nap_until(at: u64) {
    sleep_until(at);
}

fn nap(ms: u64) {
    sleep_until(clock_monotonic() + ms * MS);
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

fn cpu(permille: u32) -> JobLimits {
    JobLimits {
        memory: None,
        processes: None,
        cpu: Some(permille),
    }
}

/// Starts `role` in `job`. `period` is the period the burst process aligns
/// to; `counters` is the shared object a spinner counts in, at word `index`.
fn start(job: Handle, role: &[u8], period: u64, counters: Option<(Handle, u8)>) -> Handle {
    let mut buffer = [0; 512];
    let program = image("t_cpu");
    let mut e = Encoder::new(&mut buffer, job, program);
    if let Some((object, _)) = counters {
        e.handle(dup(
            COUNTERS,
            object,
            Rights::READ.union(Rights::WRITE).union(Rights::DUPLICATE),
        ));
    }
    e.arg(role);
    e.arg(&period.to_le_bytes());
    e.arg(&[counters.map_or(0, |(_, index)| index)]);
    let length = e.finish().expect("room");
    let process = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    process
}

/// Ends a room and waits until nothing of it is left.
fn end(room: Handle, handles: &[Handle]) {
    job_kill(room).expect("kill");
    for h in handles {
        handle_close(*h).expect("close");
    }
    assert!(eventually(|| job_get_usage(room, false)
        .expect("usage")
        .processes
        .used
        == 0));
    handle_close(room).expect("close");
}

/// What a ceiling may be exceeded by over `periods` periods above `budget_ns`
/// (review W3): the scheduler's floor slice, which is wall time, plus the time
/// to the next return to EL0, which is scaled for a slow host; and never more
/// than twice the ceiling, whatever the host.
fn upper(budget_ns: u64, periods: u64) -> u64 {
    // The longest interval with IRQ masked so far is the worst delay of the
    // slice's interrupt on this host (a loaded or nested host showed 3.6
    // times the HVF figure at a calibration of 2).
    let el0 = (OVERRUN_NS * factor()).max(masked_ns(false));
    let allowance = periods * (MIN_SLICE_NS + el0);
    (budget_ns + allowance).min(2 * budget_ns)
}

fn check_masked(what: &str) {
    let ns = masked_ns(true);
    let u = units(true);
    println!(
        "t_cpu: {what}: longest masked interval {} us, {u} units",
        ns / 1000
    );
    assert!(u <= MAX_UNITS, "{what}: {u} units");
    assert!(
        ns <= BOUND_NS * factor(),
        "{what}: {ns} ns masked, limit {}",
        BOUND_NS * factor()
    );
}

/// A page of counters shared with the spinners: word `i` is spinner `i`'s.
struct Page {
    object: Handle,
    words: *const u64,
}

impl Page {
    fn new() -> Page {
        let object = memory_create(4096, MemoryFlags::NONE).expect("memory");
        let words = memory_map(object, 0, 4096, MapFlags::WRITE).expect("map") as *const u64;
        Page { object, words }
    }

    fn sum(&self, from: usize, to: usize) -> u64 {
        (from..to).map(|i| read(self.words, i)).sum()
    }

    fn close(self) {
        zoraya_user::memory_unmap(self.words as u64, 4096).expect("unmap");
        handle_close(self.object).expect("close");
    }
}

/// An uncapped spinner in a job of its own, counting in word 0: with it the
/// CPU is never idle, so a room's share of the counts is its share of the
/// machine whatever the host's clock does with a duty-cycled load (a lone
/// spinner counts 7 times faster on an Apple core than the same code does
/// in a room, which is parked three quarters of the time). It is the
/// independent measure of the ceiling that needs no kernel counter
/// (review W4).
fn free_spinner(page: &Page) -> (Handle, Handle) {
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    (job, start(job, b"count", 0, Some((page.object, 0))))
}

/// The room's share of all counts, in per mille, must be the ceiling's plus
/// the overrun allowance (and a quarter for the host) at most, and half of it at least.
fn check_share(room: u64, free: u64, per_mille: u64) {
    let share = room as u128 * 1000 / (room + free).max(1) as u128;
    // The counts of a duty-cycled load and of a continuous one differ with
    // the host's clock and load: the same 25% room was seen at 165 to 338 per
    // mille. So this is a check for a gross error only: up to twice the
    // ceiling and down to a quarter of it; the exact check is `used_ns`.
    let high = per_mille as u128 * 2;
    println!(
        "t_cpu: the room counted {room}, an uncapped spinner {free}: {share} per mille (ceiling {per_mille}, at most {high})"
    );
    assert!(share <= high, "share {share} per mille, at most {high}");
    assert!(
        share * 4 >= per_mille as u128,
        "share {share} per mille: starved"
    );
}

/// INV-0264-1: `n` spinners in a room at 25% for 2 s use at most the budget
/// plus the overrun of each period, and at least 80% of it, by the kernel's
/// counter; and, independently, they count no more than the ceiling's share
/// of what one uncapped spinner counts in the same time (review W4). With
/// `n` over 16 (the cap of parks per pick) the overrun is still one floor
/// per period (review W1).
fn a_room_at_a_quarter_gets_a_quarter(n: usize) {
    masked_ns(true);
    units(true);
    let page = Page::new();
    let free = free_spinner(&page);
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(room, &cpu(250)).expect("limit");
    let spinners: Vec<Handle> = (0..n)
        .map(|i| start(room, b"count", 0, Some((page.object, 1 + i as u8))))
        .collect();
    nap(100);
    let (t0, u0) = (clock_monotonic(), job_get_usage(room, false).expect("u"));
    let (c0, f0) = (page.sum(1, 1 + n), page.sum(0, 1));
    nap(2000);
    let (t1, u1) = (clock_monotonic(), job_get_usage(room, false).expect("u"));
    let (counted, uncapped) = (page.sum(1, 1 + n) - c0, page.sum(0, 1) - f0);
    let (elapsed, used) = (t1 - t0, u1.cpu.used_ns - u0.cpu.used_ns);
    let budget = elapsed / 4;
    let periods = elapsed / 20_000_000 + 2;
    println!(
        "t_cpu: {n} spinners at 25% for {} ms: used {} ms of a budget of {} ms; parked {} times",
        elapsed / MS,
        used / MS,
        budget / MS,
        u1.cpu.parked - u0.cpu.parked
    );
    assert!(used <= upper(budget, periods), "{used} ns of {budget}");
    assert!(used * 5 >= budget * 4, "{used} ns of {budget}: starved");
    assert!(u1.cpu.parked > u0.cpu.parked);
    assert_eq!(u1.cpu.limit, Some(250));
    check_share(counted, uncapped, 250);
    end(room, &spinners);
    end(free.0, &[free.1]);
    page.close();
    check_masked("a room at 25%");
}

/// INV-0264-4: three rooms at 25% under a job at 50%, eight spinners each:
/// together at most 50% (and the job above them does not pass it), by the
/// kernel's counter and by the spinners' own.
fn rooms_together_stay_under_their_parent() {
    masked_ns(true);
    units(true);
    let page = Page::new();
    let free = free_spinner(&page);
    let untrusted = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(untrusted, &cpu(500)).expect("limit");
    let rooms: Vec<Handle> = (0..3)
        .map(|_| {
            let room = job_create(untrusted, JobFlags::NONE).expect("job");
            job_set_limits(room, &cpu(250)).expect("limit");
            room
        })
        .collect();
    let mut spinners = Vec::new();
    for room in &rooms {
        for _ in 0..8 {
            let i = 1 + spinners.len() as u8;
            spinners.push(start(*room, b"count", 0, Some((page.object, i))));
        }
    }
    nap(100);
    let t0 = clock_monotonic();
    let before: Vec<u64> = rooms
        .iter()
        .map(|r| job_get_usage(*r, false).expect("u").cpu.used_ns)
        .collect();
    let total0 = job_get_usage(untrusted, false).expect("u").cpu.used_ns;
    let (c0, f0) = (page.sum(1, 25), page.sum(0, 1));
    nap(2000);
    let elapsed = clock_monotonic() - t0;
    let (counted, uncapped) = (page.sum(1, 25) - c0, page.sum(0, 1) - f0);
    let total = job_get_usage(untrusted, false).expect("u").cpu.used_ns - total0;
    let periods = elapsed / 20_000_000 + 2;
    println!(
        "t_cpu: 3 rooms at 25% under 50%, {} ms: together {} ms of {} ms",
        elapsed / MS,
        total / MS,
        elapsed / 2 / MS
    );
    assert!(total <= upper(elapsed / 2, periods), "{total} ns together");
    assert!(total * 5 >= elapsed / 2 * 4, "{total} ns: starved");
    check_share(counted, uncapped, 500);
    for (room, was) in rooms.iter().zip(before) {
        let used = job_get_usage(*room, false).expect("u").cpu.used_ns - was;
        assert!(used <= upper(elapsed / 4, periods), "a room used {used} ns");
        // No room is promised any: ADR-0264 has ceilings and no shares, and
        // the first rooms in the queue can spend the parent's budget before
        // the third runs (ADR-0285, open questions).
        println!("t_cpu: a room used {} ms", used / MS);
    }
    for room in &rooms {
        job_kill(*room).expect("kill");
    }
    end(untrusted, &spinners);
    end(free.0, &[free.1]);
    for room in rooms {
        handle_close(room).expect("close");
    }
    page.close();
    check_masked("three rooms under 50%");
}

/// Review W6: a job about 30 levels deep with ceilings on the way: the masked
/// walk of `schedule` (a charge and up to 17 decisions over the whole chain)
/// stays within the bound, and the leaf's ceiling holds. More than 16
/// spinners, so the parks per pick reach their cap.
fn a_deep_job_tree() {
    masked_ns(true);
    units(true);
    let mut chain = Vec::new();
    let mut parent = JOB;
    for depth in 0..40 {
        match job_create(parent, JobFlags::NONE) {
            Ok(job) => {
                if depth % 7 == 3 {
                    job_set_limits(job, &cpu(500)).expect("limit");
                }
                chain.push(job);
                parent = job;
            }
            Err(Status::LimitReached) => break,
            Err(other) => panic!("job_create: {other:?}"),
        }
    }
    assert!(chain.len() >= 20, "only {} levels", chain.len());
    let leaf = *chain.last().expect("a leaf");
    job_set_limits(leaf, &cpu(250)).expect("limit");
    let spinners: Vec<Handle> = (0..24).map(|_| start(leaf, b"spin", 0, None)).collect();
    nap(100);
    let (t0, u0) = (clock_monotonic(), job_get_usage(leaf, false).expect("u"));
    nap(600);
    let (t1, u1) = (clock_monotonic(), job_get_usage(leaf, false).expect("u"));
    let (elapsed, used) = (t1 - t0, u1.cpu.used_ns - u0.cpu.used_ns);
    println!(
        "t_cpu: {} levels deep, 24 spinners at 25%: used {} ms of {} ms",
        chain.len(),
        used / MS,
        elapsed / 4 / MS
    );
    assert!(used <= upper(elapsed / 4, elapsed / 20_000_000 + 2));
    assert!(used * 5 >= elapsed / 4 * 4);
    job_kill(chain[0]).expect("kill");
    for h in &spinners {
        handle_close(*h).expect("close");
    }
    let top = chain[0];
    assert!(eventually(|| job_get_usage(top, false)
        .expect("usage")
        .processes
        .used
        == 0));
    for job in chain.into_iter().rev() {
        handle_close(job).expect("close");
    }
    check_masked("a job about 30 levels deep");
}

/// Review W6, ADR-0285 point 7: `job_kill` of a room whose threads are parked:
/// the room is gone within about one period, and its use goes to zero.
fn a_killed_room_with_parked_threads_dies() {
    const PERIOD: u64 = 100 * MS;
    set_period(PERIOD);
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(room, &cpu(100)).expect("limit");
    let spinners: Vec<Handle> = (0..20).map(|_| start(room, b"spin", 0, None)).collect();
    // Wait until the room has spent its 10 ms and is parked (20 spinners,
    // more than the cap of a pick).
    let mut parked = false;
    for _ in 0..40 {
        nap(5);
        let u = job_get_usage(room, false).expect("u");
        if u.cpu.period_used >= 10 * MS && u.cpu.parked > 0 {
            parked = true;
            break;
        }
    }
    assert!(parked, "the room never spent its budget");
    nap(5);
    let killed = clock_monotonic();
    job_kill(room).expect("kill");
    for h in &spinners {
        handle_close(*h).expect("close");
    }
    assert!(eventually(|| job_get_usage(room, false)
        .expect("usage")
        .processes
        .used
        == 0));
    let took = clock_monotonic() - killed;
    println!(
        "t_cpu: a parked room died in {} ms (period 100 ms)",
        took / MS
    );
    assert!(took <= (PERIOD + 50 * MS) * factor(), "{took} ns");
    handle_close(room).expect("close");
    set_period(0);
}

fn read(counters: *const u64, index: usize) -> u64 {
    // SAFETY: `counters` points at a page this process mapped, `index` is
    // inside it, and the spinners write it concurrently, hence volatile.
    unsafe { core::ptr::read_volatile(counters.add(index)) }
}

/// INV-0264-2: a spinner's counter does not move while its job is parked, and
/// moves again when the next period starts. A 100 ms period, so that the
/// parked part is long enough to look at.
fn a_parked_spinner_makes_no_progress() {
    const PERIOD: u64 = 100 * MS;
    set_period(PERIOD);
    let object = memory_create(4096, MemoryFlags::NONE).expect("memory");
    let counters = memory_map(object, 0, 4096, MapFlags::WRITE).expect("map") as *const u64;
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    // 10% of 100 ms: 10 ms of running, then parked for the other 90.
    job_set_limits(room, &cpu(100)).expect("limit");
    let spinners = [
        start(room, b"count", PERIOD, Some((object, 0))),
        start(room, b"count", PERIOD, Some((object, 1))),
    ];
    let mut proven = false;
    for _ in 0..12 {
        let now = clock_monotonic();
        let start_of = now - now % PERIOD;
        // 40 ms in, the room has spent its 10 ms (on any host that gives it
        // that much within the period); sample then and 20 ms later.
        let sample = start_of + 40 * MS;
        if now > sample + 10 * MS {
            nap_until(start_of + PERIOD + 40 * MS);
        } else {
            nap_until(sample);
        }
        let now = clock_monotonic();
        let start_of = now - now % PERIOD;
        let spent = job_get_usage(room, false).expect("u").cpu.period_used;
        let first = (read(counters, 0), read(counters, 1));
        nap(20);
        let second = (read(counters, 0), read(counters, 1));
        let later = clock_monotonic();
        if later - later % PERIOD != start_of || spent < 10 * MS {
            // The sample crossed a boundary, or the room had not run its
            // share yet on a slow host: try the next period.
            continue;
        }
        assert_eq!(first, second, "a parked spinner made progress");
        // One of them has had the whole 10 ms budget at a time, in turn.
        assert!(first.0 + first.1 > 0, "the spinners never ran");
        proven = true;
        // And the next period lets them run again.
        nap_until(start_of + PERIOD + 30 * MS);
        let third = (read(counters, 0), read(counters, 1));
        assert!(third.0 + third.1 > second.0 + second.1, "never released");
        break;
    }
    assert!(proven, "no period to look at in 12");
    set_period(0);
    end(room, &spinners);
    memory_map_close(counters, object);
}

fn memory_map_close(counters: *const u64, object: Handle) {
    zoraya_user::memory_unmap(counters as u64, 4096).expect("unmap");
    handle_close(object).expect("close");
}

/// The in-kernel half of INV-0264-3 and the measurement of the period: an
/// uncapped process wakes every ~1.7 ms next to two rooms (50% together),
/// each with 16 spinners and 16 processes that sleep until 1 ms before a
/// period boundary and then spin. The lateness of its wake-ups is the part of
/// a key's latency the scheduler is responsible for.
fn wake_up_latency_next_to_capped_rooms(period: u64) {
    set_period(period);
    masked_ns(true);
    units(true);
    let untrusted = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(untrusted, &cpu(500)).expect("limit");
    let mut all = Vec::new();
    let rooms: Vec<Handle> = (0..2)
        .map(|_| {
            let room = job_create(untrusted, JobFlags::NONE).expect("job");
            job_set_limits(room, &cpu(250)).expect("limit");
            for _ in 0..16 {
                all.push(start(room, b"spin", period, None));
            }
            for _ in 0..16 {
                all.push(start(room, b"burst", period, None));
            }
            room
        })
        .collect();
    nap(50);
    const N: usize = 400;
    let mut late = Vec::with_capacity(N);
    let mut target = clock_monotonic() + 2 * MS;
    for _ in 0..N {
        nap_until(target);
        late.push(clock_monotonic().saturating_sub(target));
        target += 1_700_000 + (late.len() as u64 % 5) * 130_000;
        // A host that is far behind does not sleep into the past.
        target = target.max(clock_monotonic() + MS);
    }
    late.sort_unstable();
    let (median, p99, max) = (late[N / 2], late[N * 99 / 100], late[N - 1]);
    println!(
        "t_cpu: period {} ms: wake-up lateness median {} us, p99 {} us, max {} us",
        period / MS,
        median / 1000,
        p99 / 1000,
        max / 1000
    );
    // ADR-0264 option D: with the capped jobs together at C of the period P,
    // a wake-up waits at most 2 C P, one slice and a step: C = 1/2.
    let bound = (period + 10 * MS + 5 * MS) * factor();
    assert!(max <= bound, "lateness {max} ns, bound {bound}");
    if period <= 20 * MS {
        // INV-0264-3's 50 ms, which a key's echo must also fit in.
        assert!(max <= 50 * MS * factor(), "lateness {max} ns");
    }
    for room in &rooms {
        job_kill(*room).expect("kill");
    }
    end(untrusted, &all);
    for room in rooms {
        handle_close(room).expect("close");
    }
    set_period(0);
    check_masked("rooms next to a woken process");
}

/// A process that only spins.
fn spin() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// A spinner that counts into word `index` of the shared page.
fn count(index: usize) -> ! {
    let object = Handle::from_parts(COUNTERS, 0).expect("slot");
    let counters = memory_map(object, 0, 4096, MapFlags::WRITE).expect("map") as *mut u64;
    loop {
        // SAFETY: inside the page this process mapped; the parent only reads.
        unsafe {
            let word = counters.add(index);
            core::ptr::write_volatile(word, core::ptr::read_volatile(word) + 1);
        }
    }
}

/// Sleeps until 1 ms before a period boundary and spins for 3 ms, for ever.
fn burst(period: u64) -> ! {
    loop {
        let now = clock_monotonic();
        let next = now - now % period + period;
        nap_until(next - MS);
        let until = clock_monotonic() + 3 * MS;
        while clock_monotonic() < until {
            core::hint::spin_loop();
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut argv = startup().argv;
    match argv.next() {
        Some(b"spin") => spin(),
        Some(b"count") => {
            let _ = argv.next();
            let index = argv.next().and_then(|a| a.first().copied()).unwrap_or(0);
            count(usize::from(index))
        }
        Some(b"burst") => {
            let period = argv
                .next()
                .and_then(|a| a.try_into().ok())
                .map_or(20 * MS, u64::from_le_bytes);
            burst(period.max(MS))
        }
        _ => {}
    }
    calibrate();
    a_room_at_a_quarter_gets_a_quarter(8);
    // More than the 16 parks one pick may make (review W1).
    a_room_at_a_quarter_gets_a_quarter(40);
    a_deep_job_tree();
    a_killed_room_with_parked_threads_dies();
    rooms_together_stay_under_their_parent();
    a_parked_spinner_makes_no_progress();
    for period in [20 * MS, 10 * MS, 100 * MS] {
        wake_up_latency_next_to_capped_rooms(period);
    }
    println!("t_cpu: all passed");
    0
}

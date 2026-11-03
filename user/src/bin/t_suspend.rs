#![no_std]
#![no_main]

//! `job_suspend` and `job_resume` (ADR-0105 item 8, INV-0105-5) from a
//! process, with counting children it spawns from itself, and the latency of
//! suspending a job of 16 processes.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    ExitReason, Handle, JOB, JobFlags, JobObject, MapFlags, MemoryFlags, MemoryObject, ObjectType,
    Rights, Status, job_signals,
};
use zoraya_user::{
    assert_err, clock_monotonic, console_entry, handle_close, handle_duplicate, image, job_create,
    job_kill, job_resume, job_suspend, memory_create, memory_map, object_get_signals, println,
    process_resume, sleep_until, spawn, startup, wait_exit,
};

const COUNTERS: usize = 32;

struct Counters {
    object: Handle,
    at: u64,
}

impl Counters {
    fn new() -> Counters {
        let object = memory_create(4096, MemoryFlags::NONE).expect("memory");
        let at = memory_map(object, 0, 4096, MapFlags::WRITE).expect("map");
        Counters { object, at }
    }

    fn read(&self) -> [u64; COUNTERS] {
        let mut values = [0; COUNTERS];
        for (i, v) in values.iter_mut().enumerate() {
            // SAFETY: inside the page mapped above; children write it, so
            // the read is volatile.
            *v = unsafe { (self.at as *const u64).add(i).read_volatile() };
        }
        values
    }
}

/// A child in `job` that does `what` with counter 7, perhaps suspended.
fn role(job: Handle, counters: &Counters, what: &[u8], suspended: bool) -> Handle {
    let mut buffer = [0; 512];
    let program = image("t_suspend");
    let mut e = Encoder::new(&mut buffer, job, program);
    if suspended {
        e.suspended();
    }
    e.handle(console_entry()).handle(HandleEntry {
        slot: 3,
        handle: counters.object,
        rights: MemoryObject::RIGHTS,
        transfer: Transfer::Duplicate,
    });
    e.arg(what).arg(b"07");
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    child
}

/// A counting child in `job`, writing counter `index`.
fn counter(job: Handle, counters: &Counters, index: u8) -> Handle {
    let mut buffer = [0; 512];
    let program = image("t_suspend");
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_entry()).handle(HandleEntry {
        slot: 3,
        handle: counters.object,
        rights: MemoryObject::RIGHTS,
        transfer: Transfer::Duplicate,
    });
    e.arg(b"count").arg(&[b'0' + index / 10, b'0' + index % 10]);
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    child
}

fn nap(ms: u64) {
    sleep_until(clock_monotonic() + ms * 1_000_000);
}

/// Waits until `done`, up to 2 s, sleeping between looks: true if it
/// happened. A slow host makes this slower; it fails only if what it waits
/// for gets no CPU for 2 s.
fn eventually(done: impl Fn() -> bool) -> bool {
    let deadline = clock_monotonic() + 2_000_000_000;
    while !done() {
        if clock_monotonic() > deadline {
            return false;
        }
        nap(5);
    }
    true
}

fn suspended(job: Handle) -> bool {
    object_get_signals(job).expect("signals") & job_signals::SUSPENDED != 0
}

/// INV-0105-5: a two-level job of counting processes.
fn suspend_stops_the_whole_tree_and_resume_starts_it() {
    let counters = Counters::new();
    let top = job_create(JOB, JobFlags::NONE).expect("job");
    let below = job_create(top, JobFlags::NONE).expect("job");
    let children = [
        counter(top, &counters, 0),
        counter(top, &counters, 1),
        counter(below, &counters, 2),
        counter(below, &counters, 3),
    ];
    let all = || counters.read()[..4].iter().all(|&c| c > 0);
    assert!(eventually(all), "all counting: {:?}", counters.read());

    job_suspend(top).expect("suspend");
    assert!(
        suspended(top) && suspended(below),
        "SUSPENDED on every job in it"
    );
    assert!(!suspended(JOB), "not above it");
    let stopped = counters.read();
    nap(50);
    assert_eq!(
        counters.read(),
        stopped,
        "no instruction ran while suspended"
    );
    job_suspend(top).expect("twice is the same");

    job_resume(top).expect("resume");
    assert!(!suspended(top) && !suspended(below));
    let again = || (0..4).all(|i| counters.read()[i] > stopped[i]);
    assert!(eventually(again), "all count again");

    // The lower job alone.
    job_suspend(below).expect("suspend");
    assert!(suspended(below) && !suspended(top));
    let part = counters.read();
    let top_runs = || {
        let now = counters.read();
        now[0] > part[0] && now[1] > part[1]
    };
    assert!(eventually(top_runs), "the top job runs");
    // A fixed wait is safe before a check that nothing happened.
    nap(50);
    assert_eq!(counters.read()[2..4], part[2..4], "the lower one does not");
    // A process that joins a suspended job starts stopped.
    let late = counter(below, &counters, 4);
    nap(30);
    assert_eq!(counters.read()[4], 0, "a newcomer waits");
    job_resume(below).expect("resume");
    assert!(
        eventually(|| counters.read()[4] > 0),
        "and runs after resume"
    );

    // Killing a suspended tree ends it.
    job_suspend(top).expect("suspend");
    job_kill(top).expect("kill");
    for child in children.iter().chain([&late]) {
        assert_eq!(wait_exit(*child).reason, ExitReason::Killed);
        handle_close(*child).expect("close");
    }
    for h in [below, top, counters.object] {
        handle_close(h).expect("close");
    }
}

/// `KILL` is the right both calls need (ADR-0105, the architect's change).
fn suspend_and_resume_need_kill() {
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let weak = handle_duplicate(job, JobObject::RIGHTS.without(Rights::KILL)).expect("dup");
    assert_err!(job_suspend(weak), Status::AccessDenied);
    assert_err!(job_resume(weak), Status::AccessDenied);
    let memory = memory_create(4096, MemoryFlags::NONE).expect("memory");
    // Rights are checked before the kind, as everywhere.
    assert_err!(job_suspend(memory), Status::AccessDenied);
    assert_err!(job_resume(memory), Status::AccessDenied);
    job_resume(job).expect("resuming a running job does nothing");
    job_suspend(job).expect("an empty job");
    assert!(suspended(job));
    let below = job_create(job, JobFlags::NONE).expect("job");
    assert!(
        suspended(below),
        "a job made under a suspended one starts suspended"
    );
    job_resume(job).expect("resume");
    assert!(!suspended(below));
    for h in [below, weak, job, memory] {
        handle_close(h).expect("close");
    }
}

/// A process asleep in the kernel is not woken early by a suspend and resume
/// of its job: only a parked thread is woken (ADR-0130 item 7). And a process
/// started suspended in a suspended job waits for both resumes.
fn resume_wakes_only_what_it_stopped() {
    let counters = Counters::new();
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let sleeper = role(job, &counters, b"sleep", false);
    nap(20);
    job_suspend(job).expect("suspend");
    job_resume(job).expect("resume");
    assert_eq!(wait_exit(sleeper).code, 0, "it slept its whole time");
    handle_close(sleeper).expect("close");

    job_suspend(job).expect("suspend");
    let late = role(job, &counters, b"count", true);
    process_resume(late).expect("resume the process");
    nap(30);
    assert_eq!(counters.read()[7], 0, "still stopped by its job");
    job_resume(job).expect("resume the job");
    assert!(eventually(|| counters.read()[7] > 0), "now it runs");
    job_kill(job).expect("kill");
    wait_exit(late);
    for h in [late, job, counters.object] {
        handle_close(h).expect("close");
    }
}

/// The measurement: `job_suspend` of a job of 16 counting processes.
fn measure_suspend_latency() {
    let counters = Counters::new();
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let mut children = [JOB; 16];
    for (i, child) in children.iter_mut().enumerate() {
        *child = counter(job, &counters, i as u8);
    }
    nap(50);
    let mut worst = 0;
    let mut total = 0;
    let rounds = 20;
    for _ in 0..rounds {
        let start = clock_monotonic();
        job_suspend(job).expect("suspend");
        let ns = clock_monotonic() - start;
        worst = worst.max(ns);
        total += ns;
        let stopped = counters.read();
        nap(5);
        assert_eq!(counters.read(), stopped);
        job_resume(job).expect("resume");
        nap(5);
    }
    println!(
        "t_suspend: job_suspend of 16 processes: mean {} ns, worst {} ns",
        total / rounds,
        worst
    );
    job_kill(job).expect("kill");
    for child in children {
        wait_exit(child);
        handle_close(child).expect("close");
    }
    for h in [job, counters.object] {
        handle_close(h).expect("close");
    }
}

fn count(index: usize) -> ! {
    let object = Handle::new(3).expect("slot");
    let at = memory_map(object, 0, 4096, MapFlags::WRITE).expect("map");
    // SAFETY: counter `index` of the page mapped above; only this process
    // writes it.
    let counter = unsafe { (at as *mut u64).add(index) };
    loop {
        // SAFETY: as above.
        unsafe { counter.write_volatile(counter.read_volatile() + 1) };
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut argv = startup().argv;
    let first = argv.clone().next();
    if let Some(b"sleep") = first {
        let start = clock_monotonic();
        let until = start + 100_000_000;
        sleep_until(until);
        return if clock_monotonic() >= until { 0 } else { 1 };
    }
    if let Some(b"count") = argv.next() {
        let digits = argv.next().unwrap_or(b"0");
        let index = digits.iter().fold(0, |n, d| n * 10 + usize::from(d - b'0'));
        count(index % COUNTERS);
    }
    suspend_stops_the_whole_tree_and_resume_starts_it();
    suspend_and_resume_need_kill();
    resume_wakes_only_what_it_stopped();
    measure_suspend_latency();
    println!("t_suspend: all passed");
    0
}

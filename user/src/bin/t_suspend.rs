#![no_std]
#![no_main]

//! `job_suspend` and `job_resume` (ADR-0105 item 8, INV-0105-5) from a
//! process, with counting children it spawns from itself, and the latency of
//! suspending a job of 16 processes.

extern crate alloc;

use alloc::vec::Vec;

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    ExitReason, Handle, JOB, JobFlags, JobObject, MapFlags, MemoryFlags, MemoryObject, ObjectType,
    Rights, Status, job_signals,
};
use zoraya_user::{
    assert_err, clock_monotonic, console_entry, handle_close, handle_duplicate, image, job_create,
    job_kill, job_resume, job_suspend, memory_create, memory_map, object_get_signals, println,
    process_exit, process_resume, raw_syscall, sleep_until, spawn, startup, thread_yield,
    wait_exit,
};

const COUNTERS: usize = 32;
/// The test kernel's call that makes every step point switch away
/// (`steps::set_always_switch`): the walks interleave at each, whatever the
/// slice, so the cases that need an interleaving do not depend on chance.
const FORCE_SWITCH: u64 = 0x701a;

fn force_switch(on: bool) {
    raw_syscall(FORCE_SWITCH, [on as u64, 0, 0, 0, 0, 0]);
}

/// Forced switching for the life of a case: on at creation, off when dropped.
/// The kernel also shortens the slice while it is on (`FORCED_SLICE_NS`), so
/// that the processes spinning at EL0 do not make each forced switch cost a
/// whole 10 ms slice each.
/// A failed assertion ends the program (the `Drop` may not run then); the
/// flag is also reset before the first case and at the start of every forced case, so that a
/// case never starts with another's setting.
struct Forced;

impl Forced {
    fn on() -> Forced {
        force_switch(false);
        force_switch(true);
        Forced
    }
}

impl Drop for Forced {
    fn drop(&mut self) {
        force_switch(false);
    }
}

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

/// `n` jobs in chains of 12 under `top`, every one held by a handle: a tree
/// whose walk takes several steps (`steps::STEP` visits each), so that it
/// pauses between them.
fn big_tree(top: Handle, n: usize) -> Vec<Handle> {
    let mut held = Vec::new();
    while held.len() < n {
        let mut parent = top;
        for _ in 0..12 {
            if held.len() >= n {
                break;
            }
            let job = job_create(parent, JobFlags::NONE).expect("job");
            held.push(job);
            parent = job;
        }
    }
    held
}

/// A child in `job` that is given `jobs` at slots 3.. and does `what` with
/// them.
fn with_jobs(job: Handle, what: &[u8], jobs: &[Handle]) -> Handle {
    let mut buffer = [0; 1024];
    let program = image("t_suspend");
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_entry());
    for (at, &handle) in jobs.iter().enumerate() {
        e.handle(HandleEntry {
            slot: 3 + at as u32,
            handle,
            rights: JobObject::RIGHTS,
            transfer: Transfer::Duplicate,
        });
    }
    e.arg(what);
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    child
}

/// Review of the fixes, error 2 (INV-0105-5): `job_suspend` of a tree of 700
/// jobs pauses between steps. A process inside the tree that holds handles to
/// jobs below it calls `job_resume` on them without end; none of them may run
/// when `job_suspend` has returned. A stress case: the walks interleave only
/// when a slice ends during one (certain on SMP, by chance on one CPU), so it
/// repeats, and `FORCE_SWITCH` makes every step point switch, so that the
/// resumer runs inside each walk on one CPU too; without the turns
/// (`job::Exclusive`) it fails (round 2, warning 3: the mutation was run).
fn a_resume_from_inside_cannot_undo_a_suspend_walk() {
    let _forced = Forced::on();
    let counters = Counters::new();
    let top = job_create(JOB, JobFlags::NONE).expect("job");
    let held = big_tree(top, 700);
    // Counting processes in the jobs the walk meets first (the last chains),
    // the resumer in the first job, which it meets last.
    let at = [699, 690, 680, 640];
    let jobs: Vec<Handle> = at.iter().map(|&i| held[i]).collect();
    let counting: Vec<Handle> = at
        .iter()
        .enumerate()
        .map(|(i, &j)| counter(held[j], &counters, i as u8))
        .collect();
    let resumer = with_jobs(held[0], b"resumer", &jobs);
    assert!(
        eventually(|| counters.read()[..4].iter().all(|&c| c > 0)),
        "all counting: {:?}",
        counters.read()
    );
    for round in 0..40 {
        job_suspend(top).expect("suspend");
        for &j in &jobs {
            assert!(suspended(j), "round {round}: SUSPENDED on a job below");
        }
        let before = counters.read();
        nap(3);
        assert_eq!(before, counters.read(), "round {round}: nothing runs");
        job_resume(top).expect("resume");
        assert!(
            eventually(|| counters.read()[..4].iter().zip(&before).all(|(a, b)| a > b)),
            "round {round}: all running again"
        );
    }
    job_kill(top).expect("kill");
    for child in counting.into_iter().chain([resumer]) {
        wait_exit(child);
        handle_close(child).expect("close");
    }
    for job in held {
        handle_close(job).expect("close");
    }
    handle_close(top).expect("close");
}

/// The child of the case above: resumes the jobs it was given, without end,
/// until the tree is killed.
fn resumer() -> ! {
    loop {
        for slot in 3..7 {
            let _ = job_resume(Handle::new(slot).expect("slot"));
        }
    }
}

/// Review of the fixes, warning 7: `job_suspend` of 700 jobs marks every one
/// `SUSPENDED` and stops a process in each of 16 spread through the chains
/// (the first, the last, and in between), while another process makes and
/// closes jobs under the top job and under a job in the middle between the
/// walk's steps (a dead child leaves the list by `swap_remove`: the walk
/// skips nobody and may visit one twice). A job made during the suspend is
/// suspended too (`Job::create` reads the parent's state under `TREE`), and
/// `job_resume` undoes it all.
fn a_large_tree_is_suspended_whole_while_jobs_come_and_go() {
    let _forced = Forced::on();
    let counters = Counters::new();
    let top = job_create(JOB, JobFlags::NONE).expect("job");
    // The first four children of `top`, which a walk visits last: the
    // dropper child drops one in each walk, between two steps (round 2,
    // warning 4), so a removal at a low index is made while the walk still
    // has the entries after it to visit.
    let leaves: Vec<Handle> = (0..4)
        .map(|_| job_create(top, JobFlags::NONE).expect("leaf"))
        .collect();
    let held = big_tree(top, 700);
    let at: Vec<usize> = (0..16).map(|i| i * 46).collect();
    let counting: Vec<Handle> = at
        .iter()
        .enumerate()
        .map(|(i, &j)| counter(held[j], &counters, i as u8))
        .collect();
    let churn = with_jobs(JOB, b"churn", &[top, held[350]]);
    let dropper = with_jobs(
        JOB,
        b"dropper",
        &[top, leaves[0], leaves[1], leaves[2], leaves[3]],
    );
    for leaf in leaves {
        handle_close(leaf).expect("close");
    }
    let all = |c: &[u64; COUNTERS]| c[..16].iter().all(|&n| n > 0);
    assert!(
        eventually(|| all(&counters.read())),
        "all counting: {:?}",
        counters.read()
    );
    for round in 0..15 {
        job_suspend(top).expect("suspend");
        assert!(suspended(top), "round {round}: the top job");
        for (i, &job) in held.iter().enumerate() {
            assert!(suspended(job), "round {round}: job {i} of 700 is SUSPENDED");
        }
        let before = counters.read();
        nap(3);
        assert_eq!(
            before,
            counters.read(),
            "round {round}: a process in each of 16 jobs is stopped"
        );
        job_resume(top).expect("resume");
        for (i, &job) in held.iter().enumerate() {
            assert!(!suspended(job), "round {round}: job {i} is running again");
        }
        assert!(
            eventually(|| counters.read()[..16]
                .iter()
                .zip(&before)
                .all(|(a, b)| a > b)),
            "round {round}: all running again"
        );
    }
    // The killed tree refuses the churn's next job: it ends.
    job_kill(top).expect("kill");
    let record = wait_exit(churn);
    assert_eq!(record.code, 0, "the churning process ended by itself");
    let record = wait_exit(dropper);
    assert_eq!(record.code, 0, "the dropper dropped its four jobs");
    handle_close(dropper).expect("close");
    for child in counting {
        wait_exit(child);
        handle_close(child).expect("close");
    }
    handle_close(churn).expect("close");
    for job in held {
        handle_close(job).expect("close");
    }
    handle_close(top).expect("close");
}

/// The child of the case above: holds the last handles of the first four
/// children of the top job (slots 4..8) and drops one each time it sees the
/// top job suspended, then waits for it to be running again. It yields while
/// it waits, so that a step point of the walk (forced to switch) runs it
/// without a slice of spinning. Gives up after 60 s.
fn dropper() -> ! {
    let top = Handle::new(3).expect("slot");
    let gave_up = clock_monotonic() + 60_000_000_000;
    for slot in 4..8 {
        for want in [true, false] {
            if !want {
                handle_close(Handle::new(slot).expect("slot")).expect("close");
            }
            while suspended(top) != want {
                if clock_monotonic() > gave_up {
                    process_exit(1);
                }
                thread_yield();
            }
        }
    }
    process_exit(0)
}

/// The child of the case above: makes three jobs under each job it was given
/// (slots 3 and 4) and closes them middle, first, last, until a job cannot
/// be made (the tree was killed).
fn churn() -> ! {
    loop {
        for slot in 3..5 {
            let parent = Handle::new(slot).expect("slot");
            let mut made = [parent; 3];
            for job in &mut made {
                match job_create(parent, JobFlags::NONE) {
                    Ok(j) => *job = j,
                    Err(_) => process_exit(0),
                }
            }
            for at in [1, 0, 2] {
                handle_close(made[at]).expect("close");
            }
        }
    }
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
    if let Some(b"resumer") = first {
        resumer();
    }
    if let Some(b"dropper") = first {
        dropper();
    }
    if let Some(b"churn") = first {
        churn();
    }
    if let Some(b"count") = argv.next() {
        let digits = argv.next().unwrap_or(b"0");
        let index = digits.iter().fold(0, |n, d| n * 10 + usize::from(d - b'0'));
        count(index % COUNTERS);
    }
    // Only the test process resets it: the children run this `main` too, and
    // one that starts during a forced case must not turn the setting off.
    force_switch(false);
    suspend_stops_the_whole_tree_and_resume_starts_it();
    suspend_and_resume_need_kill();
    resume_wakes_only_what_it_stopped();
    measure_suspend_latency();
    a_resume_from_inside_cannot_undo_a_suspend_walk();
    a_large_tree_is_suspended_whole_while_jobs_come_and_go();
    println!("t_suspend: all passed");
    0
}

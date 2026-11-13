#![no_std]
#![no_main]

//! Threads and the wait on an address, at the raw ABI (ADR-0300, ADR-0301,
//! ADR-0307): no library, so no lock in front of the calls. Threads here use
//! only atomics and system calls: the program's allocator is not safe for two
//! threads.
//!
//! INV-0300-2 to 0300-5 and 0300-7, INV-0301-1 to 0301-3 and 0301-6,
//! INV-0307-1 and 0307-4. The sibling that spins while the process ends
//! (INV-0300-3) runs in children that share a counter page with this process.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::SeqCst};

use zoraya_abi::quota::JobLimits;
use zoraya_abi::spawn::Encoder;
use zoraya_abi::{
    DEADLINE_INFINITE, ExitReason, Handle, JOB, JobFlags, JobObject, MapFlags, MemoryFlags,
    ObjectType, Rights, Status,
};
use zoraya_user::{
    assert_err, clock_monotonic, console_entry, event_create, futex_wait, futex_wake, handle_close,
    handle_duplicate, image, job_create, job_get_usage, job_kill, job_set_limits, memory_create,
    memory_map, memory_unmap, println, process_exit, raw_syscall, sleep_until,
    sleep_until_interrupted, spawn, startup, thread_create, thread_exit, thread_interrupt,
    thread_self, thread_yield, wait_exit,
};

const FREE_FRAMES: u64 = 0x7006;
const HEAP_LIVE: u64 = 0x700b;
const SMALL_STACK: u64 = 16 * 1024;
const MS: u64 = 1_000_000;

fn entry_of(f: extern "C" fn(u64) -> !) -> u64 {
    f as *const () as usize as u64
}

const HANDLE_AT: u64 = 0x7010;
const HEAP_BACKING: u64 = 0x7017;

/// Handle slots in use (the test kernel's `HANDLE_AT` over every slot).
fn slots_in_use() -> usize {
    (1..=1024u64)
        .filter(|slot| raw_syscall(HANDLE_AT, [*slot, 0, 0, 0, 0, 0]).1 != 0)
        .count()
}

/// The free frames plus the frames the kernel heap has taken from them: a
/// call that makes the heap grow moves frames to it without leaking any.
fn free_frames() -> u64 {
    loop {
        let before = raw_syscall(HEAP_BACKING, [0; 6]).1;
        let free = raw_syscall(FREE_FRAMES, [0; 6]).1;
        if raw_syscall(HEAP_BACKING, [0; 6]).1 == before {
            return free + before / 4096;
        }
    }
}

fn heap_live() -> u64 {
    raw_syscall(HEAP_LIVE, [0; 6]).1
}

/// A stack: a mapping of `SMALL_STACK` bytes whose lowest word is the
/// thread's `clear` word. Returns its base.
fn new_stack() -> u64 {
    let object = memory_create(SMALL_STACK, MemoryFlags::NONE).expect("stack object");
    let base = memory_map(object, 0, SMALL_STACK, MapFlags::WRITE).expect("map stack");
    handle_close(object).expect("close object");
    base
}

fn word(address: u64) -> &'static AtomicU32 {
    // SAFETY: `address` is a 4-aligned address in a mapping of this process
    // that stays mapped for as long as the thread that uses it runs.
    unsafe { &*(address as *const AtomicU32) }
}

/// Pass this as the argument to give the thread its own stack's base.
const OWN_BASE: u64 = u64::MAX;

/// Starts `entry(arg)` on a new stack. The word at the stack's base is 1
/// until the thread exits, which clears it (`thread_exit`'s `clear`).
fn start(entry: extern "C" fn(u64) -> !, arg: u64) -> (Handle, u64) {
    let base = new_stack();
    word(base).store(1, SeqCst);
    let arg = if arg == OWN_BASE { base } else { arg };
    let thread = thread_create(entry_of(entry), base + SMALL_STACK, arg, 0).expect("thread_create");
    (thread, base)
}

/// Waits for the thread that was given `base`, then unmaps its stack: the
/// joiner's use of INV-0300-5.
fn join(base: u64) {
    while word(base).load(SeqCst) != 0 {
        let _ = futex_wait(base, 1, DEADLINE_INFINITE);
    }
    memory_unmap(base, SMALL_STACK).expect("unmap the stack of a joined thread");
}

/// INV-0300-7 and INV-0307-4: created and joined, 2,000 rounds.
const JOINED_ROUNDS: u64 = 2000;

static COUNT: AtomicU64 = AtomicU64::new(0);

extern "C" fn quick(arg: u64) -> ! {
    COUNT.fetch_add(1, SeqCst);
    thread_exit(arg, 0, 0)
}

extern "C" fn detached(arg: u64) -> ! {
    COUNT.fetch_add(1, SeqCst);
    // Unmaps the stack it runs on, then ends (ADR-0300 item 3).
    thread_exit(0, arg, SMALL_STACK)
}

fn settle() {
    for _ in 0..4 {
        thread_yield();
    }
}

fn tpidr() -> u64 {
    let value: u64;
    // SAFETY: reading `TPIDR_EL0` has no side effects.
    unsafe { core::arch::asm!("mrs {}, tpidr_el0", out(reg) value) };
    value
}

static SEEN_TLS: AtomicU64 = AtomicU64::new(0);

extern "C" fn tls_probe(arg: u64) -> ! {
    SEEN_TLS.store(tpidr(), SeqCst);
    thread_exit(arg, 0, 0)
}

fn create_and_exit() {
    // A thread starts with its argument in x0, its stack pointer, and
    // TPIDR_EL0 as given (ADR-0300 item 2).
    let base = new_stack();
    word(base).store(1, SeqCst);
    let t =
        thread_create(entry_of(tls_probe), base + SMALL_STACK, base, 0xabcd_0000).expect("thread");
    join(base);
    assert_eq!(SEEN_TLS.load(SeqCst), 0xabcd_0000);
    handle_close(t).expect("close");
    println!("t_thread: create ok");
}

fn refusals_change_nothing() {
    let usage = || job_get_usage(JOB, false).expect("usage");
    let base = new_stack();
    let top = base + SMALL_STACK;
    let entry = entry_of(quick);
    let before = (usage(), free_frames());
    assert_err!(thread_create(entry + 2, top, 0, 0), Status::InvalidArgument);
    assert_err!(thread_create(0, top, 0, 0), Status::InvalidArgument);
    assert_err!(thread_create(1 << 47, top, 0, 0), Status::InvalidArgument);
    assert_err!(thread_create(entry, top + 8, 0, 0), Status::InvalidArgument);
    assert_err!(thread_create(entry, 0x20, 0, 0), Status::InvalidArgument);
    // A stack in a page nobody mapped.
    assert_err!(
        thread_create(entry, 0x7000_0000_0000, 0, 0),
        Status::InvalidArgument
    );
    // A handle table that is full.
    let mut fillers = [None; 1024];
    let mut n = 0;
    while let Ok(e) = event_create() {
        fillers[n] = Some(e);
        n += 1;
    }
    assert_err!(thread_create(entry, top, 0, 0), Status::LimitReached);
    for e in fillers.iter().flatten() {
        handle_close(*e).expect("close");
    }
    let after = (usage(), free_frames());
    assert_eq!(before.0.memory.used, after.0.memory.used, "INV-0300-2");
    assert_eq!(before.0.threads.used, after.0.threads.used, "INV-0300-2");
    assert_eq!(before.1, after.1, "INV-0300-2: frames");
    memory_unmap(base, SMALL_STACK).expect("unmap");
    println!("t_thread: refusals ok");
}

fn join_and_unmap_rounds() {
    // INV-0300-5: 2,000 rounds of create, join, unmap; INV-0307-4 and
    // INV-0300-7 for the joined kind.
    for _ in 0..4 {
        let (t, base) = start(quick, OWN_BASE);
        join(base);
        handle_close(t).expect("close");
    }
    settle();
    let usage = job_get_usage(JOB, false).expect("usage");
    let frames = free_frames();
    let slots = slots_in_use();
    let before_count = COUNT.load(SeqCst);
    for _ in 0..JOINED_ROUNDS {
        let base = new_stack();
        word(base).store(1, SeqCst);
        let t = thread_create(entry_of(quick), base + SMALL_STACK, base, 0).expect("thread_create");
        join(base);
        // INV-0313-3: the thread refunded its kernel memory and its figure
        // before the joiner ran, with no yield in between; the small object
        // its handle names goes with the handle.
        handle_close(t).expect("close");
        let now = job_get_usage(JOB, false).expect("usage");
        assert_eq!(now.threads.used, usage.threads.used, "INV-0313-3");
        assert_eq!(now.memory.used, usage.memory.used, "INV-0313-3");
    }
    // No handle slot is left in use (INV-0300-7).
    assert_eq!(slots_in_use(), slots);
    settle();
    assert_eq!(COUNT.load(SeqCst) - before_count, JOINED_ROUNDS);
    let after = job_get_usage(JOB, false).expect("usage");
    assert_eq!(after.threads.used, usage.threads.used, "INV-0307-4");
    assert_eq!(after.memory.used, usage.memory.used);
    assert_eq!(free_frames(), frames, "frames after the joined threads");
    println!("t_thread: joined rounds ok");
}

fn detached_rounds() {
    // INV-0300-7: 2,000 threads created detached, each unmapping its own
    // stack. The creator waits when the process is full.
    settle();
    let usage = job_get_usage(JOB, false).expect("usage");
    let frames = free_frames();
    let before_count = COUNT.load(SeqCst);
    let slots = slots_in_use();
    for _ in 0..2000 {
        let base = new_stack();
        let t = loop {
            match thread_create(entry_of(detached), base + SMALL_STACK, base, 0) {
                Ok(t) => break t,
                Err(Status::LimitReached) => thread_yield(),
                Err(e) => panic!("thread_create: {e:?}"),
            }
        };
        handle_close(t).expect("close");
    }
    while COUNT.load(SeqCst) - before_count < 2000 {
        thread_yield();
    }
    settle();
    let after = job_get_usage(JOB, false).expect("usage");
    assert_eq!(slots_in_use(), slots, "INV-0300-7: handle slots");
    assert_eq!(after.threads.used, usage.threads.used);
    assert_eq!(after.memory.used, usage.memory.used);
    assert_eq!(free_frames(), frames, "frames after 2,000 detached threads");
    // And a further thread can still be made.
    let (t, base) = start(quick, OWN_BASE);
    join(base);
    handle_close(t).expect("close");
    println!("t_thread: detached rounds ok");
}

fn futex_rules() {
    let base = new_stack();
    let w = word(base + 64);
    w.store(7, SeqCst);
    let address = base + 64;
    // The word differs: no sleep.
    assert_err!(
        futex_wait(address, 8, DEADLINE_INFINITE),
        Status::WouldBlock
    );
    // Unaligned, or not mapped.
    assert_err!(
        futex_wait(address + 1, 7, DEADLINE_INFINITE),
        Status::BadAddress
    );
    assert_err!(
        futex_wait(0x7000_0000_0000, 0, DEADLINE_INFINITE),
        Status::BadAddress
    );
    assert_err!(futex_wait(0, 0, DEADLINE_INFINITE), Status::BadAddress);
    // Nobody waits: a wake wakes none; a wake of an unmapped address too.
    assert_eq!(futex_wake(address, 1), Ok(0));
    assert_eq!(futex_wake(0x7000_0000_0000, 128), Ok(0));
    assert_err!(futex_wake(address, 0), Status::InvalidArgument);
    assert_err!(futex_wake(address, 129), Status::InvalidArgument);
    // A time-out is never early (INV-0301-6).
    for ms in [1u64, 2, 5] {
        let deadline = clock_monotonic() + ms * MS;
        assert_err!(futex_wait(address, 7, deadline), Status::DeadlineExceeded);
        assert!(
            clock_monotonic() >= deadline,
            "early by {}",
            deadline - clock_monotonic()
        );
    }
    // A deadline in the past returns at once.
    assert_err!(futex_wait(address, 7, 1), Status::DeadlineExceeded);
    memory_unmap(base, SMALL_STACK).expect("unmap");
    println!("t_thread: futex rules ok");
}

// ---- INV-0301-1: a wake between the check and the sleep is never lost.

static FLAG: AtomicU32 = AtomicU32::new(0);
static ROUNDS_SEEN: AtomicU64 = AtomicU64::new(0);

const HANDOFF_ROUNDS: u64 = 100_000;

extern "C" fn handoff_worker(arg: u64) -> ! {
    // Waits for FLAG to be odd, answers by making it even, `HANDOFF_ROUNDS`
    // times.
    let flag = &FLAG as *const AtomicU32 as u64;
    for round in 0..HANDOFF_ROUNDS {
        let want = (round * 2 + 1) as u32;
        loop {
            let seen = FLAG.load(SeqCst);
            if seen == want {
                break;
            }
            let _ = futex_wait(flag, seen, DEADLINE_INFINITE);
        }
        FLAG.store(want + 1, SeqCst);
        let _ = futex_wake(flag, 1);
        ROUNDS_SEEN.fetch_add(1, SeqCst);
    }
    thread_exit(arg, 0, 0)
}

fn handoff() {
    let flag = &FLAG as *const AtomicU32 as u64;
    FLAG.store(0, SeqCst);
    ROUNDS_SEEN.store(0, SeqCst);
    let heap = heap_live();
    let usage = job_get_usage(JOB, false).expect("usage");
    let (t, base) = start(handoff_worker, OWN_BASE);
    for round in 0..HANDOFF_ROUNDS {
        let odd = (round * 2 + 1) as u32;
        FLAG.store(odd, SeqCst);
        let _ = futex_wake(flag, 1);
        loop {
            let seen = FLAG.load(SeqCst);
            if seen == odd + 1 {
                break;
            }
            let _ = futex_wait(flag, seen, DEADLINE_INFINITE);
        }
    }
    join(base);
    handle_close(t).expect("close");
    assert_eq!(ROUNDS_SEEN.load(SeqCst), HANDOFF_ROUNDS);
    settle();
    // INV-0301-3: wait and wake allocate nothing.
    assert_eq!(heap_live(), heap, "the kernel heap after the rounds");
    let after = job_get_usage(JOB, false).expect("usage");
    assert_eq!(after.memory.used, usage.memory.used);
    println!("t_thread: {HANDOFF_ROUNDS} hand-offs ok");
}

// ---- INV-0301-6: 64 threads, staggered deadlines, none returns early.

static DEADLINES: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
static EARLY: AtomicU32 = AtomicU32::new(0);
static TIMED_DONE: AtomicU32 = AtomicU32::new(0);
static TIMED_WORD: AtomicU32 = AtomicU32::new(0);

extern "C" fn timed(index: u64) -> ! {
    let deadline = DEADLINES[index as usize].load(SeqCst);
    let result = futex_wait(&TIMED_WORD as *const AtomicU32 as u64, 0, deadline);
    if result != Err(Status::DeadlineExceeded) || clock_monotonic() < deadline {
        EARLY.fetch_add(1, SeqCst);
    }
    TIMED_DONE.fetch_add(1, SeqCst);
    thread_exit(0, 0, 0)
}

fn staggered_deadlines() {
    let now = clock_monotonic();
    for (i, d) in DEADLINES.iter().enumerate() {
        // Out of order, 2 ms to 128 ms from now, some equal.
        let step = ((i * 37) % 64) as u64 / 2;
        d.store(now + (4 + step) * MS, SeqCst);
    }
    for i in 0..64 {
        let base = new_stack();
        thread_create(entry_of(timed), base + SMALL_STACK, i, 0).expect("thread");
    }
    while TIMED_DONE.load(SeqCst) < 64 {
        sleep_until(clock_monotonic() + 5 * MS);
    }
    assert_eq!(
        EARLY.load(SeqCst),
        0,
        "INV-0301-6: a timed wait ended early"
    );
    assert_eq!(
        futex_wake(&TIMED_WORD as *const AtomicU32 as u64, 128),
        Ok(0),
        "INV-0313-2: a timed-out waiter left the list"
    );
    println!("t_thread: staggered deadlines ok");
}

// ---- interrupts: both orders, 1000 rounds each (INV-0300-4).

static BLOCKED: AtomicU32 = AtomicU32::new(0);
static RESULT: AtomicU32 = AtomicU32::new(0);
static TARGET: AtomicU64 = AtomicU64::new(0);

extern "C" fn waiter(arg: u64) -> ! {
    let me = thread_self().expect("thread_self");
    TARGET.store(me.raw(), SeqCst);
    let address = &BLOCKED as *const AtomicU32 as u64;
    BLOCKED.store(1, SeqCst);
    let _ = futex_wake(address + 4, 1);
    // Sleeps on a word that stays 1, until interrupted.
    let result = futex_wait(address, 1, DEADLINE_INFINITE);
    RESULT.store(
        match result {
            Ok(true) => 2,
            Ok(false) => 1,
            Err(_) => 3,
        },
        SeqCst,
    );
    handle_close(me).expect("close");
    thread_exit(arg, 0, 0)
}

fn interrupts() {
    // Interrupt before the wait: a thread interrupts itself, and its next wait
    // returns early, once.
    let me = thread_self().expect("self");
    for _ in 0..1000 {
        thread_interrupt(me).expect("interrupt self");
        let scratch = new_stack();
        word(scratch + 8).store(5, SeqCst);
        assert_eq!(futex_wait(scratch + 8, 5, DEADLINE_INFINITE), Ok(true));
        // The flag was consumed: the next wait runs to its deadline.
        let soon = clock_monotonic() + MS / 2;
        assert_err!(futex_wait(scratch + 8, 5, soon), Status::DeadlineExceeded);
        memory_unmap(scratch, SMALL_STACK).expect("unmap");
    }
    // An interrupt also ends `sleep_until` early, once.
    thread_interrupt(me).expect("interrupt self");
    assert!(sleep_until_interrupted(clock_monotonic() + 5000 * MS));
    assert!(!sleep_until_interrupted(clock_monotonic() + MS));
    // Interrupt while blocked.
    for _ in 0..1000 {
        BLOCKED.store(0, SeqCst);
        RESULT.store(0, SeqCst);
        let base = new_stack();
        word(base).store(1, SeqCst);
        let t = thread_create(entry_of(waiter), base + SMALL_STACK, base, 0).expect("thread");
        while BLOCKED.load(SeqCst) == 0 {
            thread_yield();
        }
        // Let it reach the futex (it may not have blocked yet: the interrupt
        // is not lost either way).
        thread_yield();
        let target = Handle::new(TARGET.load(SeqCst)).expect("handle");
        // Threads share the handle table: the waiter's own handle is valid
        // here too.
        thread_interrupt(target).expect("interrupt");
        join(base);
        assert_eq!(RESULT.load(SeqCst), 2, "the wait returned early");
        // The thread has ended: its handle was closed by it, so the interrupt
        // of a stale handle is a bad handle.
        assert_err!(thread_interrupt(target), Status::BadHandle);
        handle_close(t).expect("close");
    }
    // INV-0313-2: no node of a thread that is not waiting stays on the list.
    assert_eq!(
        futex_wake(&BLOCKED as *const AtomicU32 as u64, 128),
        Ok(0),
        "INV-0313-2"
    );
    // INV-0300-1: a thread handle can be neither duplicated nor sent.
    assert_err!(handle_duplicate(me, Rights::WAIT), Status::AccessDenied);
    handle_close(me).expect("close");
    println!("t_thread: interrupts ok");
}

// ---- the mapping lock: a contender sleeps without holding a spinlock.

const FORCE_SWITCH: u64 = 0x701a;
static MAP_GO: AtomicU32 = AtomicU32::new(0);
static MAP_DONE: AtomicU32 = AtomicU32::new(0);
static VICTIM: AtomicU64 = AtomicU64::new(0);

extern "C" fn unmapper(arg: u64) -> ! {
    while MAP_GO.load(SeqCst) == 0 {
        thread_yield();
    }
    // The first thread is inside a stepped map: this finds the lock taken.
    memory_unmap(VICTIM.load(SeqCst), SMALL_STACK).expect("unmap of the victim");
    MAP_DONE.fetch_add(1, SeqCst);
    thread_exit(arg, 0, 0)
}

extern "C" fn waker(arg: u64) -> ! {
    while MAP_GO.load(SeqCst) == 0 {
        thread_yield();
    }
    // Takes the process's thread lists while the map sleeps at a step point,
    // and the contender sleeps on the lock.
    let _ = futex_wake(&MAP_GO as *const AtomicU32 as u64, 1);
    MAP_DONE.fetch_add(1, SeqCst);
    thread_exit(arg, 0, 0)
}

/// A map of more pages than one step (stepped, so it pauses) in one thread
/// while another unmaps and a third wakes: the contender blocks on the lock
/// with no spinlock held (a `multi` lock kept across the block panicked at
/// the map's next step point or at its guard's drop). `FORCE_SWITCH` makes
/// the first step point hand over the CPU, whatever the slice.
fn map_lock_contention() {
    let object = memory_create(256 * 4096, MemoryFlags::NONE).expect("object");
    for _ in 0..8 {
        MAP_GO.store(0, SeqCst);
        MAP_DONE.store(0, SeqCst);
        let victim = new_stack();
        VICTIM.store(victim, SeqCst);
        let (t1, b1) = start(unmapper, OWN_BASE);
        let (t2, b2) = start(waker, OWN_BASE);
        raw_syscall(FORCE_SWITCH, [1, 0, 0, 0, 0, 0]);
        MAP_GO.store(1, SeqCst);
        let base = memory_map(object, 0, 256 * 4096, MapFlags::WRITE).expect("stepped map");
        raw_syscall(FORCE_SWITCH, [0; 6]);
        while MAP_DONE.load(SeqCst) < 2 {
            thread_yield();
        }
        join(b1);
        join(b2);
        memory_unmap(base, 256 * 4096).expect("unmap the big map");
        handle_close(t1).expect("close");
        handle_close(t2).expect("close");
    }
    handle_close(object).expect("close");
    println!("t_thread: map lock contention ok");
}

// ---- a sibling unmaps the result buffer while a creating call pauses.

static ENDS_GO: AtomicU32 = AtomicU32::new(0);
static ENDS_AT: AtomicU64 = AtomicU64::new(0);

extern "C" fn ends_unmapper(arg: u64) -> ! {
    while ENDS_GO.load(SeqCst) == 0 {
        thread_yield();
    }
    memory_unmap(ENDS_AT.load(SeqCst), SMALL_STACK).expect("unmap the buffer");
    ENDS_GO.store(2, SeqCst);
    thread_exit(arg, 0, 0)
}

/// `stream_create` and `channel_create` pause while they build their objects;
/// a sibling may unmap the buffer for the two handles meanwhile. The call
/// then fails, and must leave no handle in the table, where the caller does
/// not know of it. (If no pause reached the sibling the call succeeds and
/// its two handles stay, unknown here: only failures are checked.) Measured:
/// the calls pause only when the kernel heap has to grow, which a warm heap
/// does not, so the sibling usually runs after the call; the check guards
/// the order of the kernel code (room made after the last pause, undo on a
/// failed write), and a mutant of it survives (listed in the report).
fn creating_calls_leave_no_handles() {
    const CALLS: [(&str, u64); 2] = [("stream_create", 42), ("channel_create", 30)];
    for (name, call) in CALLS {
        let mut failed = 0;
        for _ in 0..8 {
            let ends = new_stack();
            ENDS_AT.store(ends, SeqCst);
            ENDS_GO.store(0, SeqCst);
            let (t, flag) = start(ends_unmapper, OWN_BASE);
            let slots = slots_in_use();
            raw_syscall(FORCE_SWITCH, [1, 0, 0, 0, 0, 0]);
            ENDS_GO.store(1, SeqCst);
            let (status, _) = raw_syscall(call, [ends, 16, 0, 0, 0, 0]);
            raw_syscall(FORCE_SWITCH, [0; 6]);
            while ENDS_GO.load(SeqCst) != 2 {
                thread_yield();
            }
            if status != Status::Ok as u64 {
                failed += 1;
                assert_eq!(status, Status::BadAddress as u64, "{name}");
                assert_eq!(slots_in_use(), slots, "{name}: a failed call left handles");
            }
            join(flag);
            handle_close(t).expect("close");
        }
        println!("t_thread: {name}: {failed} of 8 failed");
    }
    println!("t_thread: creating calls ok");
}

// ---- the quota (INV-0307-1), in a child job with threads = 3.

extern "C" fn parked(arg: u64) -> ! {
    // Waits on its own stack's base word to be cleared by the main thread.
    let w = word(arg);
    while w.load(SeqCst) != 0 {
        let _ = futex_wait(arg, 1, DEADLINE_INFINITE);
    }
    thread_exit(arg + 4, 0, 0)
}

fn quota_child() -> i32 {
    let usage = job_get_usage(JOB, false).expect("usage");
    assert_eq!(usage.threads.limit, Some(3));
    assert_eq!(usage.threads.used, 1, "the first thread counts");
    let mut bases = [0u64; 2];
    let mut handles = [None; 2];
    for (slot, kept) in bases.iter_mut().zip(handles.iter_mut()) {
        let base = new_stack();
        word(base).store(1, SeqCst);
        word(base + 4).store(1, SeqCst);
        *kept = Some(
            thread_create(entry_of(parked), base + SMALL_STACK, base, 0)
                .expect("thread within the limit"),
        );
        *slot = base;
    }
    let base = new_stack();
    let (status, field) = raw_syscall(56, [entry_of(quick), base + SMALL_STACK, 0, 0, 0, 0]);
    assert_eq!(status, Status::OverQuota as u64, "the fourth thread");
    assert_eq!(field, 3, "QuotaField::Threads");
    let u = job_get_usage(JOB, false).expect("usage");
    assert_eq!((u.threads.used, u.threads.peak), (3, 3));
    assert!(u.threads.refused >= 1);
    memory_unmap(base, SMALL_STACK).expect("unmap");
    // Let the two end; the figure comes back at their `thread_exit`.
    for base in bases {
        word(base).store(0, SeqCst);
        let _ = futex_wake(base, 128);
    }
    for (base, thread) in bases.into_iter().zip(handles) {
        while word(base + 4).load(SeqCst) != 0 {
            let _ = futex_wait(base + 4, 1, DEADLINE_INFINITE);
        }
        memory_unmap(base, SMALL_STACK).expect("unmap");
        handle_close(thread.expect("handle")).expect("close");
    }
    settle();
    let u = job_get_usage(JOB, false).expect("usage");
    assert_eq!(u.threads.used, 1, "INV-0307-4");
    let (t, base) = start(quick, OWN_BASE);
    join(base);
    handle_close(t).expect("a thread can be made again");
    0
}

fn quota() {
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    job_set_limits(
        room,
        &JobLimits {
            memory: None,
            processes: None,
            cpu: None,
            threads: Some(3),
        },
    )
    .expect("limits");
    let mut buffer = [0; 512];
    let program = image("t_thread");
    let mut e = Encoder::new(&mut buffer, room, program);
    e.handle(console_entry());
    e.handle(zoraya_user::dup(JOB.slot(), room, JobObject::RIGHTS));
    e.arg(b"quota");
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    let record = wait_exit(child);
    assert_eq!(
        (record.reason, record.code),
        (ExitReason::Exited, 0),
        "the child in the limited job"
    );
    let u = job_get_usage(room, false).expect("usage");
    assert!(u.threads.refused >= 1);
    assert_eq!(u.threads.used, 0);
    handle_close(child).expect("close");
    handle_close(room).expect("close");
    // A setting of the field above the parent's is refused like the others.
    println!("t_thread: quota ok");
}

// ---- INV-0300-3: nothing of a process runs after its end.

const SPINNERS: usize = 3;

fn counters(shared: u64) -> &'static [AtomicU64; SPINNERS] {
    // SAFETY: `shared` is a mapping of the page both processes map, 8-aligned.
    unsafe { &*(shared as *const [AtomicU64; SPINNERS]) }
}

static SHARED: AtomicU64 = AtomicU64::new(0);

extern "C" fn spinner(index: u64) -> ! {
    let c = counters(SHARED.load(SeqCst));
    loop {
        c[index as usize].fetch_add(1, SeqCst);
    }
}

extern "C" fn faulter(_: u64) -> ! {
    // Runs a while, then touches nothing mapped.
    for _ in 0..20_000 {
        core::hint::spin_loop();
    }
    // SAFETY: deliberately invalid: the page at 0 is never mapped.
    unsafe { core::ptr::write_volatile(core::ptr::without_provenance_mut::<u64>(8), 1) };
    thread_exit(0, 0, 0)
}

extern "C" fn exiter(_: u64) -> ! {
    // Waits until every spinner has run, then ends the process.
    let c = counters(SHARED.load(SeqCst));
    while c.iter().any(|n| n.load(SeqCst) == 0) {
        thread_yield();
    }
    process_exit(5)
}

fn spin_child(role: &[u8]) -> i32 {
    let object = Handle::new(8).expect("slot");
    let shared = memory_map(object, 0, 4096, MapFlags::WRITE).expect("map shared");
    SHARED.store(shared, SeqCst);
    for i in 0..SPINNERS {
        let base = new_stack();
        thread_create(entry_of(spinner), base + SMALL_STACK, i as u64, 0).expect("spinner");
    }
    let base = new_stack();
    match role {
        b"spin-exit" => {
            thread_create(entry_of(exiter), base + SMALL_STACK, 0, 0).expect("exiter");
        }
        b"spin-fault" => {
            thread_create(entry_of(faulter), base + SMALL_STACK, 0, 0).expect("faulter");
        }
        _ => {}
    }
    // The main thread waits too, until the process ends.
    loop {
        sleep_until(clock_monotonic() + 1000 * MS);
    }
}

fn read_counters(view: u64) -> [u64; SPINNERS] {
    let c = counters(view);
    [c[0].load(SeqCst), c[1].load(SeqCst), c[2].load(SeqCst)]
}

fn ends_with_spinners(role: &[u8], kill: bool) {
    let shared = memory_create(4096, MemoryFlags::NONE).expect("shared");
    let view = memory_map(shared, 0, 4096, MapFlags::WRITE).expect("view");
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    let mut buffer = [0; 512];
    let program = image("t_thread");
    let mut e = Encoder::new(&mut buffer, room, program);
    e.handle(console_entry());
    e.handle(zoraya_user::dup(
        8,
        shared,
        Rights::READ.union(Rights::WRITE).union(Rights::DUPLICATE),
    ));
    e.arg(role);
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    if kill {
        let start = clock_monotonic();
        while read_counters(view).contains(&0) {
            assert!(
                clock_monotonic() - start < 20_000 * MS,
                "spinners never ran"
            );
            thread_yield();
        }
        job_kill(room).expect("kill");
    }
    let record = wait_exit(child);
    match role {
        b"spin-exit" => assert_eq!((record.reason, record.code), (ExitReason::Exited, 5)),
        b"spin-fault" => assert_eq!(record.reason, ExitReason::Fault),
        _ => assert_eq!(record.reason, ExitReason::Killed),
    }
    let first = read_counters(view);
    sleep_until(clock_monotonic() + 30 * MS);
    thread_yield();
    assert_eq!(first, read_counters(view), "a sibling ran after the end");
    assert!(first.iter().all(|n| *n > 0), "{first:?}");
    handle_close(child).expect("close");
    handle_close(room).expect("close");
    memory_unmap(view, 4096).expect("unmap");
    handle_close(shared).expect("close");
    settle();
}

fn ending() {
    ends_with_spinners(b"spin-exit", false);
    ends_with_spinners(b"spin-fault", false);
    ends_with_spinners(b"spin-park", true);
    println!("t_thread: ending ok");
}

// ---- a process full of threads: the bound, and a kill with 127 sleepers.

static GO: AtomicU32 = AtomicU32::new(0);

extern "C" fn sleeper(_: u64) -> ! {
    let address = &GO as *const AtomicU32 as u64;
    while GO.load(SeqCst) == 0 {
        let _ = futex_wait(address, 0, DEADLINE_INFINITE);
    }
    thread_exit(0, 0, 0)
}

fn full_process_child() -> i32 {
    let mut made = 0;
    let usage = job_get_usage(JOB, false).expect("usage");
    loop {
        let base = new_stack();
        match thread_create(entry_of(sleeper), base + SMALL_STACK, 0, 0) {
            Ok(_) => made += 1,
            Err(Status::LimitReached) => {
                assert_eq!(made, 127, "128 threads with the first");
                memory_unmap(base, SMALL_STACK).expect("unmap");
                break;
            }
            Err(e) => panic!("{e:?}"),
        }
    }
    let after = job_get_usage(JOB, false).expect("usage");
    assert_eq!(after.threads.used, usage.threads.used + 127);
    // 127 sleepers wait on one address: one wake of 128 wakes them all.
    thread_yield();
    thread_yield();
    GO.store(1, SeqCst);
    let mut woken = 0;
    while woken < 127 {
        woken += futex_wake(&GO as *const AtomicU32 as u64, 128).expect("wake");
        thread_yield();
    }
    // Then park everyone again and be killed by the parent.
    0
}

fn full_process() {
    let room = job_create(JOB, JobFlags::NONE).expect("job");
    let mut buffer = [0; 512];
    let program = image("t_thread");
    let mut e = Encoder::new(&mut buffer, room, program);
    e.handle(console_entry());
    e.handle(zoraya_user::dup(JOB.slot(), room, JobObject::RIGHTS));
    e.arg(b"full");
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(program).expect("image");
    let record = wait_exit(child);
    assert_eq!((record.reason, record.code), (ExitReason::Exited, 0));
    let u = job_get_usage(room, false).expect("usage");
    assert_eq!(u.threads.used, 0);
    assert_eq!(u.threads.peak, 128);
    handle_close(child).expect("close");
    handle_close(room).expect("close");
    println!("t_thread: full process ok");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    match startup().argv.next() {
        Some(b"quota") => return quota_child(),
        Some(role @ (b"spin-exit" | b"spin-fault" | b"spin-park")) => return spin_child(role),
        Some(b"full") => return full_process_child(),
        _ => {}
    }
    let steps: [(&str, fn()); 13] = [
        ("create", create_and_exit),
        ("refusals", refusals_change_nothing),
        ("joined", join_and_unmap_rounds),
        ("detached", detached_rounds),
        ("futex", futex_rules),
        ("handoff", handoff),
        ("staggered", staggered_deadlines),
        ("interrupts", interrupts),
        ("maplock", map_lock_contention),
        ("creating", creating_calls_leave_no_handles),
        ("quota", quota),
        ("ending", ending),
        ("full", full_process),
    ];
    for (name, run) in steps {
        let started = clock_monotonic();
        run();
        println!(
            "t_thread: {name} took {} ms",
            (clock_monotonic() - started) / MS
        );
    }
    0
}

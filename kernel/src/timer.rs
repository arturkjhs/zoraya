//! The monotonic clock and one-shot deadlines on the virtual timer
//! (ADR-0023). There is no periodic tick: the comparator is set for the
//! earliest queued deadline, and the timer is off when the queue is empty.
//! Periodic work is a callback that arms its own next deadline.

use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_dtb::platform::Timer;
use zoraya_time::{Clock, DeadlineQueue, Instant, QueueFull, TimerId};

use crate::arch::timer as hw;
use crate::irq;
use crate::sync::SpinLock;

/// Live user threads at once, in the whole system (ADR-0300 item 1): the
/// bound on the deadline queue's users. Services and the trusted jobs have no
/// thread figure of their own, so this is what limits them.
pub const MAX_THREADS_SYSTEM: usize = 1000;

/// Deadlines the kernel itself may hold beside the threads' (the scheduler's
/// slice and release are in [`RESERVED`]; this is for device and driver
/// timers), an upper bound.
const KERNEL_TIMERS: usize = 22;

/// Deadlines pending at once: every user thread of the system can hold one
/// (a timed `futex_wait`, `port_wait` or `sleep_until`), with room for the
/// kernel's own and the two reserved entries (ADR-0301 item 5, INV-0301-7). A
/// full queue is an error for the caller. About 40 bytes an entry.
const QUEUE_SIZE: usize = 1024;

const _: () = assert!(QUEUE_SIZE >= MAX_THREADS_SYSTEM + KERNEL_TIMERS + RESERVED);

/// Due entries `on_interrupt` runs per pass; the comparator is left in the
/// past if more are due, so the interrupt comes again at once and IRQ is
/// unmasked between passes (ADR-0301 item 5, ADR-0270).
const POP_PER_PASS: usize = 64;

/// A deadline's callback. It runs in interrupt context, with IRQ masked and
/// no lock held, so it may arm further deadlines.
pub type Callback = fn(usize);

#[derive(Clone, Copy)]
struct Entry {
    callback: Callback,
    arg: usize,
}

static HZ: AtomicU64 = AtomicU64::new(0);
static START: AtomicU64 = AtomicU64::new(0);
static QUEUE: SpinLock<DeadlineQueue<Entry, QUEUE_SIZE>> = SpinLock::new(DeadlineQueue::new());

fn clock() -> Clock {
    Clock::new(HZ.load(Ordering::Relaxed), START.load(Ordering::Relaxed))
        .expect("the clock has started")
}

/// Starts the clock and takes the virtual timer's interrupt.
pub fn init(timer: &Timer) {
    let hz = hw::frequency();
    assert_ne!(hz, 0, "CNTFRQ_EL0 is not set");
    HZ.store(hz, Ordering::Relaxed);
    START.store(hw::counter(), Ordering::Relaxed);
    hw::disable();
    irq::register(
        timer.virtual_timer.intid,
        timer.virtual_timer.edge,
        on_interrupt,
    );
}

/// The timer's frequency in hertz.
pub fn frequency() -> u64 {
    HZ.load(Ordering::Relaxed)
}

/// The monotonic clock.
pub fn now() -> Instant {
    clock().instant(hw::counter())
}

/// Sets the comparator for the earliest deadline. A comparator in the past
/// fires at once: the timer condition is a level.
fn program(queue: &DeadlineQueue<Entry, QUEUE_SIZE>) {
    match queue.earliest() {
        Some(at) => hw::fire_at(clock().ticks_at(at)),
        None => hw::disable(),
    }
}

/// Slots [`arm`] leaves free for [`arm_reserved`]: the scheduler's time slice
/// and the one deadline that returns parked threads (ADR-0264 point 5).
const RESERVED: usize = 2;

/// Calls `callback(arg)` once, no earlier than `at`. Two slots stay free for
/// [`arm_reserved`].
pub fn arm(at: Instant, callback: Callback, arg: usize) -> Result<TimerId, QueueFull> {
    let mut queue = QUEUE.lock();
    if queue.len() + RESERVED >= QUEUE_SIZE {
        return Err(QueueFull);
    }
    let id = queue.insert(at, Entry { callback, arg })?;
    program(&queue);
    Ok(id)
}

/// As [`arm`], for the scheduler's time slice and the period's release
/// deadline, which must not be lost to a full queue.
pub fn arm_reserved(at: Instant, callback: Callback, arg: usize) -> Result<TimerId, QueueFull> {
    let mut queue = QUEUE.lock();
    let id = queue.insert(at, Entry { callback, arg })?;
    program(&queue);
    Ok(id)
}

/// Cancels a deadline. False if it already fired or was cancelled.
pub fn cancel(id: TimerId) -> bool {
    let mut queue = QUEUE.lock();
    let found = queue.cancel(id);
    program(&queue);
    found
}

fn on_interrupt() {
    // One snapshot: a callback that arms a deadline in the past is served by
    // the next interrupt, so it cannot hold this loop forever.
    let now = now();
    for _ in 0..POP_PER_PASS {
        let due = QUEUE.lock().pop_due(now);
        match due {
            Some(entry) => (entry.callback)(entry.arg),
            None => break,
        }
    }
    program(&QUEUE.lock());
}

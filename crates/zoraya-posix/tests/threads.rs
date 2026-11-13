//! The thread table and the routing of signals between threads (ADR-0304
//! items 1 to 3; INV-0304-1), against the fake kernel.

mod kernel;

use kernel::*;
use zoraya_posix::errno::*;
use zoraya_posix::signals::{Action, SIG_BLOCK, SIG_SETMASK, Taken, bit};
use zoraya_posix::threads::Tkill;

zoraya_test_alloc::cap_memory!();

const USR1: u32 = 10;
const USR2: u32 = 12;
const HANDLER: Action = Action {
    handler: 0x4000,
    mask: 0,
    flags: 0,
};

/// A process whose main thread (pointer 100) has made threads 2 (200) and 3
/// (300); the caller is the main thread.
fn three() -> Rig {
    let mut r = rig();
    r.p.signals.set(USR1, Some(HANDLER)).unwrap();
    r.p.signals.set(USR2, Some(HANDLER)).unwrap();
    r.p.switch_to(100);
    r.p.thread_add(200, 2, Some(1002));
    r.p.thread_add(300, 3, Some(1003));
    r
}

#[test]
fn each_thread_has_its_own_mask_and_pending_set() {
    let mut r = three();
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR1))).unwrap();
    r.p.thread_add(400, 4, None);
    r.p.switch_to(400);
    assert_eq!(
        r.p.signals.blocked,
        bit(USR1),
        "a thread starts with its creator's mask"
    );
    r.p.switch_to(200);
    r.p.signals.mask(SIG_SETMASK, Some(0)).unwrap();
    assert_eq!(r.p.signals.take(USR1), Taken::Run(HANDLER));
    r.p.switch_to(100);
    assert_eq!(
        r.p.signals.blocked,
        bit(USR1),
        "the main thread's mask is kept"
    );
    assert_eq!(r.p.signals.take(USR1), Taken::Pending);
    r.p.switch_to(300);
    assert_eq!(r.p.signals.pending, 0, "pending is per thread");
    r.p.switch_to(100);
    assert_eq!(r.p.signals.pending, bit(USR1));
}

#[test]
fn tkill_reaches_the_target_thread_only() {
    let mut r = three();
    assert_eq!(r.p.tkill(9, USR1), Err(ESRCH));
    assert_eq!(r.p.tkill(2, 99), Err(EINVAL));
    assert_eq!(r.p.tkill(2, 0), Ok(Tkill::Sent), "0 only looks");
    assert!(r.p.sys.interrupts.is_empty());
    assert_eq!(r.p.tkill(1, USR1), Ok(Tkill::Myself));
    assert_eq!(
        r.p.signals.pending,
        bit(USR1),
        "oneself: only marked, delivered when musl's locks are released"
    );
    r.p.signals.pending = 0;
    assert_eq!(r.p.tkill(3, USR1), Ok(Tkill::Sent));
    assert_eq!(r.p.sys.interrupts, vec![(1003, None)]);
    r.p.switch_to(300);
    assert_eq!(r.p.signals.next_unblocked(), Some(USR1));
    r.p.switch_to(200);
    assert_eq!(r.p.signals.next_unblocked(), None);
}

#[test]
fn a_blocked_thread_directed_signal_waits_in_its_thread() {
    let mut r = three();
    r.p.switch_to(300);
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR1))).unwrap();
    r.p.switch_to(100);
    r.p.tkill(3, USR1).unwrap();
    r.p.switch_to(300);
    assert_eq!(r.p.signals.next_unblocked(), None);
    r.p.signals.mask(SIG_SETMASK, Some(0)).unwrap();
    assert_eq!(r.p.signals.next_unblocked(), Some(USR1));
}

#[test]
fn a_process_signal_the_taker_blocks_goes_to_one_thread_that_does_not() {
    let mut r = three();
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR1))).unwrap();
    r.p.switch_to(200);
    r.p.signals.mask(SIG_SETMASK, Some(bit(USR1))).unwrap();
    // Thread 3 is at a wait point, thread 2 blocks it, the taker (1) blocks it.
    r.p.switch_to(300);
    r.p.signals.mask(SIG_SETMASK, Some(0)).unwrap();
    assert!(!r.p.set_wait_port(Some(77)));
    r.p.switch_to(100);
    assert_eq!(r.p.signals.take(USR1), Taken::Pending);
    r.p.route(USR1, false);
    assert_eq!(
        r.p.sys.interrupts,
        vec![(1003, Some(77))],
        "woken through its port"
    );
    assert_eq!(r.p.signals.pending, 0, "no longer the taker's");
    assert_eq!(
        r.p.signals.shared,
        bit(USR1),
        "process-pending (ADR-0304 3b)"
    );
    r.p.switch_to(300);
    assert_eq!(r.p.signals.next_unblocked(), Some(USR1));
    assert!(r.p.take_interrupted());
    // Exactly one thread has it: not thread 2.
    r.p.switch_to(200);
    assert_eq!(r.p.signals.pending, 0);
    r.p.signals.mask(SIG_SETMASK, Some(0)).unwrap();
    assert_eq!(r.p.signals.next_unblocked(), None, "taken once");
}

#[test]
fn a_routed_signal_is_not_lost_when_its_thread_blocks_it_or_ends() {
    let mut r = three();
    // Thread 1 blocks USR1; thread 2 and 3 have it unblocked; 2 is picked.
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR1))).unwrap();
    assert_eq!(r.p.signals.take(USR1), Taken::Pending);
    r.p.route(USR1, false);
    assert_eq!(r.p.sys.interrupts, vec![(1002, None)]);
    // Thread 2 blocks it before it looks, then ends: thread 3 still has it.
    r.p.switch_to(200);
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR1))).unwrap();
    assert_eq!(r.p.signals.next_unblocked(), None);
    r.p.switch_to(300);
    r.p.thread_remove(200);
    assert_eq!(r.p.signals.next_unblocked(), Some(USR1));
}

#[test]
fn routing_leaves_a_thread_directed_signal_of_the_taker_alone() {
    let mut r = three();
    // pthread_kill(1, USR1) while thread 1 blocks it: pending in thread 1.
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR1))).unwrap();
    r.p.tkill(1, USR1).unwrap();
    assert_eq!(r.p.signals.pending, bit(USR1));
    // A process-directed one arrives: it coalesces; thread 1 keeps its own.
    let was = r.p.signals.pending & bit(USR1) != 0;
    assert_eq!(r.p.signals.take(USR1), Taken::Pending);
    r.p.route(USR1, was);
    assert_eq!(r.p.signals.pending, bit(USR1), "still thread 1's");
    assert!(r.p.sys.interrupts.is_empty());
    // And a thread-directed signal of thread 1 is never moved to thread 3
    // by a later process-directed one of another number.
    assert_eq!(r.p.signals.take(USR2), Taken::Run(HANDLER));
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR2))).unwrap();
    assert_eq!(r.p.signals.take(USR2), Taken::Pending);
    r.p.route(USR2, false);
    assert_eq!(r.p.signals.pending, bit(USR1), "USR1 stays");
    r.p.switch_to(300);
    assert_eq!(r.p.signals.pending, 0);
}

#[test]
fn a_signal_every_thread_blocks_stays_with_the_process_for_the_first_to_unblock() {
    let mut r = three();
    for tp in [100, 200, 300] {
        r.p.switch_to(tp);
        r.p.signals.mask(SIG_SETMASK, Some(bit(USR2))).unwrap();
    }
    r.p.switch_to(100);
    assert_eq!(r.p.signals.take(USR2), Taken::Pending);
    r.p.route(USR2, false);
    assert!(r.p.sys.interrupts.is_empty());
    assert_eq!(r.p.signals.shared, bit(USR2));
    assert_eq!(r.p.signals.next_unblocked(), None);
    r.p.switch_to(200);
    r.p.signals.mask(SIG_SETMASK, Some(0)).unwrap();
    assert_eq!(r.p.signals.next_unblocked(), Some(USR2));
    r.p.switch_to(300);
    r.p.signals.mask(SIG_SETMASK, Some(0)).unwrap();
    assert_eq!(r.p.signals.next_unblocked(), None, "delivered once");
}

#[test]
fn a_thread_that_is_interrupted_does_not_wait() {
    let mut r = three();
    r.p.tkill(1, 0).unwrap();
    r.p.switch_to(200);
    r.p.interrupt_tid(1).unwrap();
    r.p.switch_to(100);
    assert!(
        r.p.set_wait_port(Some(5)),
        "the interrupt came before the wait"
    );
    assert!(!r.p.set_wait_port(Some(5)), "and is spent");
    assert_eq!(r.p.interrupt_tid(9), Err(ESRCH));
    let gone = r.p.thread_remove(200).unwrap();
    assert_eq!(gone.handle, Some(1002));
    assert_eq!(r.p.tkill(2, USR1), Err(ESRCH));
    assert_eq!(r.p.thread_count(), 2);
}

#[test]
fn the_main_thread_seen_before_its_thread_pointer_was_set_keeps_its_state() {
    let mut r = rig();
    r.p.switch_to(0);
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR1))).unwrap();
    // Later the same, only thread has a pointer: it is the same thread.
    r.p.switch_to(100);
    assert_eq!(r.p.signals.blocked, bit(USR1));
    assert_eq!(r.p.thread_count(), 1);
    r.p.thread_add(200, 2, None);
    r.p.switch_to(200);
    r.p.signals.mask(SIG_SETMASK, Some(0)).unwrap();
    r.p.switch_to(100);
    assert_eq!(
        r.p.signals.blocked,
        bit(USR1),
        "the other thread's mask does not leak"
    );
}

#[test]
fn a_fork_child_keeps_the_calling_thread_only() {
    let mut r = three();
    r.p.switch_to(200);
    r.p.signals.mask(SIG_BLOCK, Some(bit(USR1))).unwrap();
    r.p.signals.shared = bit(USR2);
    r.p.sys.interrupts.clear();
    let dropped = r.p.threads_after_fork();
    assert_eq!(dropped, 2);
    assert_eq!(r.p.thread_count(), 1);
    assert_eq!(
        r.p.threads.list[0].tp, 200,
        "the caller, not the main thread"
    );
    assert_eq!(r.p.threads.list[0].tid, 2, "its tid is kept");
    assert_eq!(r.p.signals.shared, 0, "the process-pending set is empty");
    assert_eq!(r.p.signals.blocked, bit(USR1), "the caller's mask is kept");
    // The table is usable: switching to the same thread changes nothing,
    // and a new thread can be added.
    r.p.switch_to(200);
    r.p.thread_add(500, 5, None);
    assert_eq!(r.p.thread_count(), 2);
}

//! Threads in a process (ADR-0300) and the wait on an address (ADR-0301).
//!
//! A process owns one address space, one handle table and from one to
//! `MAX_THREADS` threads. This file holds what a thread needs beyond the
//! scheduler's record: the object a handle names it by, the six calls' logic,
//! the per-process mapping lock, and the sweeps that wake siblings in steps.
//! The lists are `zoraya-thread`'s, host tested.
//!
//! Lock order: a process's `members`, `multi` and `park_head`, then its
//! `Multi` lists, then the scheduler's lock (waking), then the event tables.
//! No lock is held across a step point.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};

use zoraya_abi::{Status, thread_signals};
use zoraya_events::SourceId;
use zoraya_thread::{JoinError, MAX_THREADS, Members, WAKE_STEP, WaitList};
use zoraya_time::Deadline;

use crate::charge::{self, Paid};
use crate::job::Job;
use crate::process::Process;
use crate::sched::{self, Thread};
use crate::sync::SpinLock;
use crate::syscall::UserSlice;
use crate::{arch, port, steps, timer};

pub use crate::timer::MAX_THREADS_SYSTEM;

/// User threads in the system, first threads included (ADR-0300 item 1).
static USER_THREADS: AtomicUsize = AtomicUsize::new(0);

/// Counts a user thread record made; the matching [`thread_gone`] is its
/// drop.
pub fn thread_made() {
    USER_THREADS.fetch_add(1, Ordering::Relaxed);
}

/// A user thread record was freed.
pub fn thread_gone() {
    USER_THREADS.fetch_sub(1, Ordering::Relaxed);
}

/// What a handle to a thread names (ADR-0300 item 2): the signal record, and
/// until the thread ends, the way to reach it for `thread_interrupt`. It holds
/// no stack and no process: `target` is dropped at the thread's end, which
/// also breaks the cycle with `Thread::object`.
pub struct ThreadObject {
    source: SourceId,
    target: SpinLock<Option<Arc<Thread>>>,
    _paid: Paid,
}

impl ThreadObject {
    pub fn source(&self) -> SourceId {
        self.source
    }

    /// The thread, until it has ended.
    pub fn target(&self) -> Option<Arc<Thread>> {
        self.target.lock().clone()
    }

    /// The thread ended: `TERMINATED` is asserted and the link is cut.
    pub fn terminated(&self) {
        let target = self.target.lock().take();
        drop(target);
        port::set_signals(self.source, thread_signals::TERMINATED);
    }
}

impl Drop for ThreadObject {
    fn drop(&mut self) {
        port::free_source(self.source);
    }
}

/// Makes the object for `thread`, paid by `job`. Nothing is left charged on a
/// failure.
pub fn new_object(job: &Arc<Job>, thread: &Arc<Thread>) -> Result<Arc<ThreadObject>, Status> {
    let mut paid = Paid::new(Some(job));
    paid.kernel(charge::arc_cost::<ThreadObject>() + charge::SOURCE_COST)?;
    let source = port::new_source(0).map_err(|_| Status::NoMemory)?;
    let object = charge::prepaid_arc(ThreadObject {
        source,
        target: SpinLock::new(Some(thread.clone())),
        _paid: paid,
    })?;
    *thread.object().lock() = Some(object.clone());
    Ok(object)
}

/// The lists a process needs once it has, or may have, a second thread, made
/// at the first `thread_create` or `futex_wait` and charged then (so a
/// process that never threads pays nothing): the futex wait list and the
/// waiters for the mapping lock, each with room for every thread, so that a
/// wait cannot fail for memory after its check (ADR-0301 driver 3).
pub struct Multi {
    pub futex: SpinLock<WaitList<Arc<Thread>>>,
    pub map_waiters: SpinLock<WaitList<Arc<Thread>>>,
}

/// What [`ensure_multi`] charges.
fn multi_cost() -> u64 {
    charge::arc_cost::<Multi>()
        + 2 * (MAX_THREADS * core::mem::size_of::<(u64, Arc<Thread>)>()) as u64
        + Members::<Arc<Thread>>::reserved_bytes() as u64
}

/// The process's thread lists, made on first use. `OverQuota` or `NoMemory`
/// change nothing.
pub fn ensure_multi(process: &Process) -> Result<Arc<Multi>, Status> {
    if let Some(multi) = process.multi().lock().clone() {
        return Ok(multi);
    }
    let cost = multi_cost();
    // A failed earlier call may have kept the member list's charge (below):
    // it is paid already, and `multi_cost` holds it until the lists are made.
    let kept = process.multi_cost().load(Ordering::Relaxed);
    let owed = cost - kept;
    process.paid().lock().kernel(owed)?;
    let built = (|| {
        let futex = WaitList::new(MAX_THREADS).map_err(|_| Status::NoMemory)?;
        let map_waiters = WaitList::new(MAX_THREADS).map_err(|_| Status::NoMemory)?;
        process
            .members()
            .lock()
            .reserve_all()
            .map_err(|_| Status::NoMemory)?;
        charge::prepaid_arc(Multi {
            futex: SpinLock::new(futex),
            map_waiters: SpinLock::new(map_waiters),
        })
    })();
    match built {
        Ok(multi) => {
            process.multi_cost().store(cost, Ordering::Relaxed);
            *process.multi().lock() = Some(multi.clone());
            Ok(multi)
        }
        Err(e) => {
            // The member list may have grown. Give it back if the allocator
            // allows (the smaller list is built first, so a failure keeps the
            // large one); else its charge stays, and `multi_cost` records it
            // for the next attempt and for the teardown.
            let members_cost = Members::<Arc<Thread>>::reserved_bytes() as u64;
            let still = if process.members().lock().shrink().is_ok() {
                0
            } else {
                members_cost
            };
            process.multi_cost().store(still, Ordering::Relaxed);
            process.paid().lock().release_kernel(kept + owed - still);
            Err(e)
        }
    }
}

fn user_word(address: u64, expected_len: u64) -> Result<UserSlice, Status> {
    if !address.is_multiple_of(4) {
        return Err(Status::BadAddress);
    }
    UserSlice::new(address, expected_len)
}

/// `thread_create` (ADR-0300 item 2). The order of checks is the ADR's, so
/// that a refusal changes nothing: the process has room, the system has room,
/// a handle slot is free, the job's `threads` figure then its memory, the
/// kernel stack and record; the handle is put in the table last.
pub fn create(
    process: &Arc<Process>,
    entry: u64,
    stack: u64,
    argument: u64,
    tls: u64,
) -> Result<u64, Status> {
    use crate::memory::user::{USER_HIGH, USER_LOW};
    if !(USER_LOW..USER_HIGH).contains(&entry) || !entry.is_multiple_of(4) {
        return Err(Status::InvalidArgument);
    }
    if !stack.is_multiple_of(16) || !(16 + USER_LOW..=USER_HIGH).contains(&stack) {
        return Err(Status::InvalidArgument);
    }
    {
        let space = process.space.lock();
        let space = space.as_ref().ok_or(Status::BadState)?;
        if !space.user_writable(stack - 16) {
            return Err(Status::InvalidArgument);
        }
    }
    if process.exit_requested() {
        return Err(Status::Cancelled);
    }
    match process.members().lock().has_room() {
        Ok(()) => {}
        Err(JoinError::Full) => return Err(Status::LimitReached),
        Err(JoinError::Ending) => return Err(Status::Cancelled),
    }
    if USER_THREADS.load(Ordering::Relaxed) >= MAX_THREADS_SYSTEM {
        return Err(Status::LimitReached);
    }
    process.handles.lock().reserve()?;
    // The record, the kernel stack and the figure; a refusal here is `OverQuota`
    // from the first field passed, and has changed nothing.
    let thread = sched::spawn_user_thread(process.clone(), entry, stack, argument, tls)?;
    let made = (|| {
        let object = new_object(process.job(), &thread)?;
        ensure_multi(process)?;
        Ok::<_, Status>(object)
    })();
    let object = match made {
        Ok(object) => object,
        Err(e) => {
            undo(&thread);
            return Err(e);
        }
    };
    // Past the last check that can fail except the two below, which cannot
    // after `has_room` and `reserve`: nothing yields between them.
    if process
        .members()
        .lock()
        .join(thread.id(), thread.clone())
        .is_err()
    {
        undo(&thread);
        return Err(Status::LimitReached);
    }
    let rights = zoraya_abi::Rights::WAIT.union(zoraya_abi::Rights::SIGNAL);
    let handle = process
        .handles
        .lock()
        .insert(
            crate::object::Object::Thread(object),
            rights,
            crate::object::next_id(),
        )
        .inspect_err(|_| {
            // Cannot happen after `reserve`; undone for the record.
            process.members().lock().leave(thread.id());
            undo(&thread);
        })?;
    sched::wake(thread);
    Ok(handle.raw())
}

/// Gives up a thread that never ran and was not listed: its object link is
/// cut, its figure goes with its record.
fn undo(thread: &Arc<Thread>) {
    if let Some(object) = thread.object().lock().take() {
        object.terminated();
    }
    thread.release_thread();
    sched::discard(thread.clone());
}

/// `thread_self` (ADR-0300 item 4): a new handle to the calling thread.
pub fn this(process: &Arc<Process>) -> Result<u64, Status> {
    let thread = sched::current();
    process.handles.lock().reserve()?;
    // Bound first: `new_object` takes the same lock.
    let known = thread.object().lock().clone();
    let object = match known {
        Some(object) => object,
        None => new_object(process.job(), &thread)?,
    };
    let rights = zoraya_abi::Rights::WAIT.union(zoraya_abi::Rights::SIGNAL);
    let handle = process.handles.lock().insert(
        crate::object::Object::Thread(object),
        rights,
        crate::object::next_id(),
    )?;
    Ok(handle.raw())
}

/// `thread_interrupt` (ADR-0300 item 5): sets the target's one-shot flag and
/// wakes it if it is blocked where the flag is consumed.
pub fn interrupt(object: &ThreadObject) -> Result<(), Status> {
    let target = object.target().ok_or(Status::BadState)?;
    target.interrupted().store(true, Ordering::Relaxed);
    if target.interruptible().load(Ordering::Relaxed) && target.is_blocked_now() {
        sched::wake(target);
    }
    Ok(())
}

/// `sleep_until` for a user thread: returns 1 when `thread_interrupt` ended it
/// early and 0 otherwise (ADR-0300 item 5).
pub fn sleep_until(at: Deadline) -> Result<u64, Status> {
    let thread = sched::current();
    let daif = arch::save_and_mask_interrupts();
    let result = (|| {
        if thread.interrupted().swap(false, Ordering::Relaxed) {
            return Ok(1);
        }
        thread.interruptible().store(true, Ordering::Relaxed);
        let blocked = sched::block_until(at);
        thread.interruptible().store(false, Ordering::Relaxed);
        blocked.map_err(|_| Status::NoMemory)?;
        Ok(u64::from(
            thread.interrupted().swap(false, Ordering::Relaxed),
        ))
    })();
    arch::restore_interrupts(daif);
    result
}

/// `futex_wait` (ADR-0301 item 1). The word is read and the thread listed
/// with interrupts masked, so that no wake falls between the check and the
/// block.
pub fn futex_wait(
    process: &Arc<Process>,
    address: u64,
    expected: u32,
    at: Deadline,
) -> Result<u64, Status> {
    let word = user_word(address, 4)?;
    let multi = ensure_multi(process)?;
    let thread = sched::current();
    let daif = arch::save_and_mask_interrupts();
    let result = futex_wait_masked(process, &multi, &thread, &word, address, expected, at);
    arch::restore_interrupts(daif);
    result
}

fn futex_wait_masked(
    process: &Process,
    multi: &Multi,
    thread: &Arc<Thread>,
    word: &UserSlice,
    address: u64,
    expected: u32,
    at: Deadline,
) -> Result<u64, Status> {
    if process.exit_requested() {
        return Err(Status::Cancelled);
    }
    if thread.interrupted().swap(false, Ordering::Relaxed) {
        return Ok(1);
    }
    let mut bytes = [0u8; 4];
    word.read_into(0, &mut bytes)?;
    if u32::from_le_bytes(bytes) != expected {
        return Err(Status::WouldBlock);
    }
    if let Deadline::At(t) = at
        && timer::now() >= t
    {
        return Err(Status::DeadlineExceeded);
    }
    thread.woken().store(false, Ordering::Relaxed);
    // Room for every thread of the process was made with the list.
    multi
        .futex
        .lock()
        .push(address, thread.clone())
        .map_err(|_| Status::NoMemory)?;
    thread.interruptible().store(true, Ordering::Relaxed);
    let outcome = loop {
        if sched::block_until(at).is_err() {
            break Err(Status::NoMemory);
        }
        if thread.woken().load(Ordering::Relaxed) {
            break Ok(0);
        }
        if process.exit_requested() {
            break Err(Status::Cancelled);
        }
        if thread.interrupted().swap(false, Ordering::Relaxed) {
            break Ok(1);
        }
        if let Deadline::At(t) = at
            && timer::now() >= t
        {
            break Err(Status::DeadlineExceeded);
        }
        // Woken for another reason (a resume, a spurious wake): wait on.
    };
    thread.interruptible().store(false, Ordering::Relaxed);
    if !thread.woken().load(Ordering::Relaxed) {
        // Not taken off by a waker: take the node off now.
        let me = thread.clone();
        let node = multi.futex.lock().remove_where(|t| Arc::ptr_eq(t, &me));
        drop(node);
    }
    outcome
}

/// `futex_wake` (ADR-0301 item 2): up to `count` threads waiting on
/// `address`, oldest first, in steps of [`WAKE_STEP`] with the list's lock
/// dropped and a step point between them. Returns how many.
pub fn futex_wake(process: &Process, address: u64, count: u32) -> Result<u64, Status> {
    let Some(multi) = process.multi().lock().clone() else {
        return Ok(0);
    };
    let mut left = count as usize;
    let mut total = 0;
    while left > 0 {
        let ask = left.min(WAKE_STEP);
        let taken = multi.futex.lock().take(address, ask, |t| {
            t.woken().store(true, Ordering::Relaxed);
            sched::wake(t);
        });
        steps::count(taken);
        total += taken;
        left -= taken;
        if taken < ask {
            break;
        }
        steps::pause();
    }
    Ok(total as u64)
}

/// Wakes every thread of `process` but the caller, in steps of [`WAKE_STEP`]:
/// what the first thread to leave an ending process does (ADR-0300 item 6),
/// outside every spinlock. A thread that leaves also wakes the new first one
/// (`process::leave`), so one the steps skip while the list shortens is still
/// reached.
pub fn wake_siblings(process: &Process) {
    let me = sched::current();
    let mut from = Some(0);
    while let Some(at) = from {
        from = process.members().lock().step(at, WAKE_STEP, |t| {
            if !Arc::ptr_eq(t, &me) {
                sched::wake(t.clone());
            }
        });
        if from.is_some() {
            steps::pause();
        }
    }
}

/// Wakes the threads of `process` that wait in `park_if_stopped`, in steps:
/// what a resumed process's first woken thread does, and a fork's thaw.
pub fn wake_parked(process: &Process) {
    let me = sched::current();
    let mut from = Some(0);
    while let Some(at) = from {
        from = process.members().lock().step(at, WAKE_STEP, |t| {
            if !Arc::ptr_eq(t, &me) && t.parked().load(Ordering::Relaxed) {
                sched::wake(t.clone());
            }
        });
        if from.is_some() {
            steps::pause();
        }
    }
}

/// The per-process mapping lock (ADR-0301 item 6): a sleeping lock held for
/// the whole of a mapping call, including its steps, which `map_reserve` and
/// `map_commit` need because nothing may map between them. Released on every
/// path by its drop (ADR-0300 item 6).
pub struct MapGuard<'a> {
    process: &'a Process,
}

impl<'a> MapGuard<'a> {
    /// Takes the lock, sleeping while another thread of the process holds it.
    /// `Cancelled` if the process is ending.
    pub fn acquire(process: &'a Process) -> Result<MapGuard<'a>, Status> {
        let thread = sched::current();
        loop {
            let daif = arch::save_and_mask_interrupts();
            let outcome = if process.exit_requested() {
                Some(Err(Status::Cancelled))
            } else if !process.map_locked().swap(true, Ordering::Acquire) {
                Some(Ok(()))
            } else {
                // Bound first: a guard in the scrutinee would live to the end
                // of the match, across the block below.
                let multi = process.multi().lock().clone();
                match multi {
                    Some(multi) => {
                        let listed = multi.map_waiters.lock().push(0, thread.clone());
                        if listed.is_ok() {
                            sched::block_current();
                            let me = thread.clone();
                            let node = multi
                                .map_waiters
                                .lock()
                                .remove_where(|t| Arc::ptr_eq(t, &me));
                            drop(node);
                        }
                        None
                    }
                    // Contended without the lists cannot happen: a second
                    // thread comes with them. Wait by yielding.
                    None => {
                        sched::yield_now();
                        None
                    }
                }
            };
            arch::restore_interrupts(daif);
            if let Some(done) = outcome {
                return done.map(|()| MapGuard { process });
            }
        }
    }
}

impl Drop for MapGuard<'_> {
    fn drop(&mut self) {
        let daif = arch::save_and_mask_interrupts();
        self.process.map_locked().store(false, Ordering::Release);
        let waiter = self
            .process
            .multi()
            .lock()
            .clone()
            .and_then(|m| m.map_waiters.lock().pop_front());
        if let Some(waiter) = waiter {
            sched::wake(waiter);
        }
        arch::restore_interrupts(daif);
    }
}

/// The freeze a fork takes (ADR-0306 item 2): from `freeze` until the guard
/// drops, the process's other threads park at their next return to EL0 and
/// run no user instruction, so the copy sees one memory.
pub struct Freeze<'a> {
    process: &'a Process,
}

impl<'a> Freeze<'a> {
    pub fn new(process: &'a Process) -> Freeze<'a> {
        process.set_frozen(true);
        Freeze { process }
    }
}

impl Drop for Freeze<'_> {
    fn drop(&mut self) {
        self.process.set_frozen(false);
        wake_parked(self.process);
    }
}

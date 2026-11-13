//! Threads and the scheduler (ADR-0030).
//!
//! One run queue, round robin. The kernel is not preempted: a slice that ends
//! takes effect on return to EL0, or at `yield_now` and blocking points.
//! The slice is a one-shot deadline, armed only when another thread is ready,
//! so a lone thread takes no timer interrupts, except a thread of a job with a
//! CPU ceiling (ADR-0264): its run time is charged at each switch, its slice
//! is the smaller of 10 ms and the budget left, and when the budget is spent
//! its threads wait on the parked list until the period ends. The queue and
//! the budget arithmetic are `zoraya-cpu`; this file is the glue.
//!
//! Lock order: `SCHED`, then the timer queue's lock (arming a deadline), then
//! a job's `cpu` lock (the walks); nothing is taken under the last. Those
//! walks run with every exception masked, inside `schedule`.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use zoraya_abi::Status;
use zoraya_cpu::RunQueue;
use zoraya_time::{Deadline, Instant, TimerId};

use crate::arch::THREAD_STACK_SIZE;

use crate::arch;
use crate::arch::Context;
use crate::arch::mmu;
use crate::charge::{self, Paid};
use crate::cpu::{self, JobRef, JobTree};
use crate::memory::heap::{OutOfMemory, uncharged_arc};
use crate::memory::stack::KernelStack;
use crate::process::Process;
use crate::sync::SpinLock;
use crate::timer;

/// A running user thread's time slice.
const SLICE_NS: u64 = 10_000_000;

/// A shorter slice a test asked for, or 0 for [`SLICE_NS`]. It stands for a
/// slow host, where a slice holds fewer guest instructions.
#[cfg(feature = "qemu-test")]
static TEST_SLICE_NS: AtomicU64 = AtomicU64::new(0);

/// Sets the slice of user threads switched to from now on; 0 restores
/// [`SLICE_NS`]. The harness restores it before every test.
#[cfg(feature = "qemu-test")]
pub fn set_test_slice(ns: u64) {
    TEST_SLICE_NS.store(ns, Ordering::Relaxed);
}

fn slice_ns() -> u64 {
    #[cfg(feature = "qemu-test")]
    {
        let ns = TEST_SLICE_NS.load(Ordering::Relaxed);
        if ns != 0 {
            return ns;
        }
    }
    SLICE_NS
}

const READY: u8 = 0;
const RUNNING: u8 = 1;
const BLOCKED: u8 = 2;
const DEAD: u8 = 3;

pub struct Thread {
    context: UnsafeCell<Context>,
    state: AtomicU8,
    /// `None` for the boot thread, which runs on the image's stack.
    _stack: Option<KernelStack>,
    process: Option<Arc<Process>>,
    /// A user thread's stack and record are its process's job's (ADR-0261
    /// point 5); the boot and kernel threads have no payer. Behind a lock
    /// because `thread_exit` gives the `threads` figure back at the call
    /// (ADR-0307 item 7), while the record lives on until the reaper.
    paid: SpinLock<Paid>,
    /// Names the thread in its process's list (ADR-0300 item 1).
    id: u64,
    /// `thread_interrupt` was called and no wait has consumed it (ADR-0300
    /// item 5). One-shot.
    interrupted: AtomicBool,
    /// The thread is blocked where `thread_interrupt` may wake it:
    /// `futex_wait` and `sleep_until`, never `port_wait` (INV-0006-6).
    interruptible: AtomicBool,
    /// A `futex_wake` took this thread off its process's wait list (ADR-0301
    /// item 1): the waker sets it under the list's lock before it wakes.
    woken: AtomicBool,
    /// The thread waits in `park_if_stopped` (job suspend or a fork's freeze).
    parked: AtomicBool,
    /// The object `thread_self` and `thread_create` name it by, if any.
    object: SpinLock<Option<Arc<crate::thread::ThreadObject>>>,
}

static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

fn next_thread_id() -> u64 {
    NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed)
}

impl Drop for Thread {
    fn drop(&mut self) {
        if self.process.is_some() {
            crate::thread::thread_gone();
        }
    }
}

// SAFETY: `context` is only touched by `schedule`, with interrupts masked, on
// the one CPU: saved for the running thread and read for the next. Everything
// else is atomic or immutable.
unsafe impl Send for Thread {}
// SAFETY: as above.
unsafe impl Sync for Thread {}

impl Thread {
    fn build(
        context: Context,
        state: u8,
        stack: Option<KernelStack>,
        process: Option<Arc<Process>>,
        paid: Paid,
    ) -> Thread {
        if process.is_some() {
            crate::thread::thread_made();
        }
        Thread {
            context: UnsafeCell::new(context),
            state: AtomicU8::new(state),
            _stack: stack,
            process,
            paid: SpinLock::new(paid),
            id: next_thread_id(),
            interrupted: AtomicBool::new(false),
            interruptible: AtomicBool::new(false),
            woken: AtomicBool::new(false),
            parked: AtomicBool::new(false),
            object: SpinLock::new(None),
        }
    }

    /// Its id in its process's list.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn interrupted(&self) -> &AtomicBool {
        &self.interrupted
    }

    pub(crate) fn interruptible(&self) -> &AtomicBool {
        &self.interruptible
    }

    pub(crate) fn woken(&self) -> &AtomicBool {
        &self.woken
    }

    pub(crate) fn parked(&self) -> &AtomicBool {
        &self.parked
    }

    pub(crate) fn object(&self) -> &SpinLock<Option<Arc<crate::thread::ThreadObject>>> {
        &self.object
    }

    /// Gives back what the thread cost its job when it ends: the `threads`
    /// figure, the record and the kernel stack, at the `thread_exit` call and
    /// not at the reaper's free (ADR-0307 item 7).
    pub(crate) fn release_thread(&self) {
        self.paid.lock().release_exited();
    }

    /// Whether the thread is blocked (for `thread_interrupt`'s wake).
    pub(crate) fn is_blocked_now(&self) -> bool {
        self.state.load(Ordering::Relaxed) == BLOCKED
    }

    /// The process this thread belongs to.
    pub fn process(&self) -> Option<&Arc<Process>> {
        self.process.as_ref()
    }

    /// The program counter of a user thread that has not run yet.
    #[cfg(feature = "qemu-test")]
    pub fn first_pc(&self) -> Option<u64> {
        self._stack.as_ref().map(KernelStack::first_pc)
    }

    #[cfg(feature = "qemu-test")]
    pub fn is_blocked(&self) -> bool {
        self.state.load(Ordering::Relaxed) == BLOCKED
    }
}

struct Sched {
    current: Option<Arc<Thread>>,
    #[cfg(feature = "qemu-test")]
    boot: Option<Arc<Thread>>,
    idle: Option<Arc<Thread>>,
    ready: RunQueue<Arc<Thread>>,
    /// Ended threads, freed by a later thread: none can free its own stack.
    dead: Vec<Arc<Thread>>,
    /// Threads that exist, for reserving queue space.
    threads: usize,
    need_resched: bool,
    slice: Option<TimerId>,
    /// When the running thread was switched in, in nanoseconds: its run
    /// time is charged to its job at the next switch (ADR-0264 point 3).
    switched_in: u64,
    /// The deadline that returns the parked threads at the period's end is
    /// armed (one for all jobs, ADR-0264 point 5).
    release_armed: bool,
}

static SCHED: SpinLock<Sched> = SpinLock::new(Sched {
    current: None,
    #[cfg(feature = "qemu-test")]
    boot: None,
    idle: None,
    ready: RunQueue::new(),
    dead: Vec::new(),
    threads: 0,
    need_resched: false,
    slice: None,
    switched_in: 0,
    release_armed: false,
});

static PREEMPTIONS: AtomicU64 = AtomicU64::new(0);

/// Times a thread was switched out because its slice ended.
#[cfg(feature = "qemu-test")]
pub fn preemptions() -> u64 {
    PREEMPTIONS.load(Ordering::Relaxed)
}

fn boot_thread() -> Arc<Thread> {
    // uncharged: the boot thread, made before the heap can refuse.
    Arc::new(Thread::build(
        Context::default(),
        RUNNING,
        None,
        None,
        Paid::default(),
    ))
}

/// Makes the running code the boot thread and creates the idle thread. Call
/// once, with the heap up.
pub fn init() {
    let boot = boot_thread();
    let idle = new_kernel(idle_main, 0).expect("idle thread");
    let mut s = SCHED.lock();
    #[cfg(feature = "qemu-test")]
    {
        s.boot = Some(boot.clone());
    }
    s.current = Some(boot);
    s.idle = Some(idle);
    s.threads = 2;
}

fn new_kernel(entry: fn(usize), arg: usize) -> Result<Arc<Thread>, OutOfMemory> {
    let stack = KernelStack::new()?;
    let context = Context::new_kernel(entry as usize, arg, stack.top(), stack.limit());
    // uncharged: kernel threads have no paying job
    uncharged_arc(Thread::build(
        context,
        READY,
        Some(stack),
        None,
        Paid::default(),
    ))
}

/// Reserves queue room for one more thread, so that the scheduler itself never
/// has to allocate.
fn count_new() -> Result<(), OutOfMemory> {
    let mut s = SCHED.lock();
    let threads = s.threads + 1;
    // Both the ready list and the parked list: any thread may be in either.
    s.ready.try_reserve(threads).map_err(|_| OutOfMemory)?;
    s.dead.try_reserve(threads).map_err(|_| OutOfMemory)?;
    s.threads = threads;
    Ok(())
}

/// Starts a kernel thread running `entry(arg)`. When `entry` returns, the
/// thread ends.
pub fn spawn_kernel(entry: fn(usize), arg: usize) -> Result<Arc<Thread>, OutOfMemory> {
    let thread = new_kernel(entry, arg)?;
    count_new()?;
    SCHED.lock().ready.push_back(thread.clone());
    Ok(thread)
}

/// What the first thread of a process costs the process's job in the
/// `threads` field: charged before the process takes a page, right after its
/// slot in the count, so the field that refuses is the first passed in the
/// order processes, threads, memory (ADR-0307 item 1).
pub fn reserve_thread(job: &Arc<crate::job::Job>) -> Result<Paid, Status> {
    let mut paid = Paid::new(Some(job));
    paid.thread()?;
    Ok(paid)
}

/// Makes `process`'s first thread, to go to EL0 at `pc` with `sp`. It is
/// blocked: [`wake`] starts it, and [`discard`] gives it up. `paid` came from
/// [`reserve_thread`].
pub fn spawn_user(
    process: Arc<Process>,
    pc: u64,
    sp: u64,
    paid: Paid,
) -> Result<Arc<Thread>, Status> {
    let paid = thread_paid(&process, paid)?;
    let stack = KernelStack::new().map_err(|_| Status::NoMemory)?;
    let frame = stack.push_user_frame(pc, sp);
    let context = Context::new_user(frame, stack.limit());
    let thread = charge::prepaid_arc(Thread::build(
        context,
        BLOCKED,
        Some(stack),
        Some(process),
        paid,
    ))?;
    count_new().map_err(|_| Status::NoMemory)?;
    Ok(thread)
}

/// As [`spawn_user`], for a thread `thread_create` makes (ADR-0300 item 2):
/// `x0 = argument` and `TPIDR_EL0 = tls`, every other register zero. The
/// `threads` figure is charged here, with the record and the kernel stack, and
/// nothing is changed if a charge or an allocation fails.
pub fn spawn_user_thread(
    process: Arc<Process>,
    pc: u64,
    sp: u64,
    argument: u64,
    tls: u64,
) -> Result<Arc<Thread>, Status> {
    let paid = reserve_thread(process.job())?;
    let paid = thread_paid(&process, paid)?;
    let stack = KernelStack::new().map_err(|_| Status::NoMemory)?;
    let frame = stack.push_user_frame(pc, sp);
    stack.set_first_argument(argument);
    let context = Context::new_user_with_tls(frame, stack.limit(), tls);
    let thread = charge::prepaid_arc(Thread::build(
        context,
        BLOCKED,
        Some(stack),
        Some(process),
        paid,
    ))?;
    count_new().map_err(|_| Status::NoMemory)?;
    Ok(thread)
}

/// What a user thread costs its process's job: the record, the kernel stack
/// and a place in each of the scheduler's three lists (ready, parked, dead).
fn thread_paid(process: &Process, mut paid: Paid) -> Result<Paid, Status> {
    let queues = 3 * core::mem::size_of::<usize>() as u64;
    paid.kernel(charge::arc_cost::<Thread>() + THREAD_STACK_SIZE as u64 + queues)?;
    let _ = process;
    Ok(paid)
}

/// Makes `process`'s thread from a copy of `frame`, returning `Ok` and 0
/// from the call that frame was saved at, with `TPIDR_EL0` set to
/// `tpidr_el0`. Blocked, as from [`spawn_user`].
pub fn spawn_user_from(
    process: Arc<Process>,
    frame: &arch::Frame,
    tpidr_el0: u64,
    paid: Paid,
) -> Result<Arc<Thread>, Status> {
    let paid = thread_paid(&process, paid)?;
    let stack = KernelStack::new().map_err(|_| Status::NoMemory)?;
    let at = stack.push_user_frame_copy(frame, zoraya_abi::Status::Ok as u64, 0);
    let context = Context::new_user_with_tls(at, stack.limit(), tpidr_el0);
    let thread = charge::prepaid_arc(Thread::build(
        context,
        BLOCKED,
        Some(stack),
        Some(process),
        paid,
    ))?;
    count_new().map_err(|_| Status::NoMemory)?;
    Ok(thread)
}

/// Forgets a thread from [`spawn_user`] that never ran.
pub fn discard(thread: Arc<Thread>) {
    SCHED.lock().threads -= 1;
    drop(thread);
}

/// Runs on a new kernel thread, from boot-time assembly.
#[unsafe(no_mangle)]
extern "C" fn kernel_thread_main(entry: usize, arg: usize) -> ! {
    // SAFETY: threads exist only after the vectors and GIC are up. New threads
    // start from `schedule`, with every exception masked.
    unsafe { arch::unmask_all() };
    // SAFETY: `new_kernel` stored a `fn(usize)` here.
    let entry: fn(usize) = unsafe { core::mem::transmute(entry) };
    entry(arg);
    exit_current()
}

fn idle_main(_: usize) {
    loop {
        reap();
        // Masked around the check: an interrupt between it and `wfi` would
        // otherwise be lost. `wfi` wakes on a masked pending interrupt.
        let daif = arch::save_and_mask_interrupts();
        let empty = SCHED.lock().ready.is_empty();
        if empty {
            // Waiting is not holding off an interrupt: a pending one wakes
            // `wfi` and runs at once when the mask is restored (INV-0270-1's
            // counter does not count this time).
            crate::steps::mask_ended();
            arch::wait_for_interrupt();
            crate::steps::mask_began();
        }
        arch::restore_interrupts(daif);
        if !SCHED.lock().ready.is_empty() {
            schedule();
        }
    }
}

/// Frees ended threads. Their stacks are not in use: a thread joins the list
/// only as it switches away.
fn reap() {
    loop {
        // Dropped outside the lock, which freeing a stack does not need. The
        // list keeps its capacity, so `schedule` never allocates.
        let Some(thread) = SCHED.lock().dead.pop() else {
            break;
        };
        drop(thread);
    }
}

/// Asks for a switch at the next preemption point (the test kernel's
/// `FORCE_SWITCH`, `steps::set_always_switch`).
#[cfg(feature = "qemu-test")]
pub fn request_resched() {
    SCHED.lock().need_resched = true;
}

fn end_of_slice(_: usize) {
    SCHED.lock().need_resched = true;
}

/// The period ends (ADR-0264 point 5): the parked threads return to the end
/// of the run queue, in batches of `RELEASE_BATCH`. If more remain, the next
/// batch is a deadline for now, so the interrupt returns and unmasks between
/// them (INV-0270-1). Runs in the timer's interrupt, with no lock held.
fn period_end(_: usize) {
    let mut s = SCHED.lock();
    if s.ready.release() > 0 {
        s.need_resched = true;
    }
    if s.ready.parked_len() > 0 {
        let again = timer::arm_reserved(timer::now(), period_end, 0);
        if again.is_ok() {
            return;
        }
        // No slot for the next batch (the queue keeps two for the scheduler,
        // so this is not reached): the next `schedule` arms it again.
    }
    s.release_armed = false;
}

/// Arms the deadline that returns the parked threads, unless it is armed.
/// Called with `SCHED` held, after something may have been parked.
fn arm_release(s: &mut Sched, now: u64) {
    if s.release_armed || s.ready.parked_len() == 0 {
        return;
    }
    let at = Instant::from_nanos(zoraya_cpu::next_boundary(now, cpu::period_ns()));
    if timer::arm_reserved(at, period_end, 0).is_ok() {
        s.release_armed = true;
    } else {
        // A parked thread must have a way back (INV-0285-1). A slot is
        // always free (two are reserved, only this deadline and the slice use
        // them), so this is not reached; if it were, one bounded batch goes
        // back now and the next `schedule` tries again.
        s.ready.release();
    }
}

/// Chooses the next thread and switches to it. Returns when this thread next
/// runs, and never if it is dead.
fn schedule() {
    reap();
    let daif = arch::save_and_mask_all();
    let (prev, next, ttbr0) = {
        let mut guard = SCHED.lock();
        let s = &mut *guard;
        let now = timer::now().as_nanos();
        let period_ns = cpu::period_ns();
        let current = s.current.take().expect("a thread is running");
        let prev: *const Thread = Arc::as_ptr(&current);
        // ADR-0264 point 3: the time since this thread was switched in is its
        // job's, and its ancestors'. A kernel thread has no job. Only the part
        // after the last period boundary counts to this period's budget.
        if let Some(process) = current.process.as_ref() {
            let ran = now.saturating_sub(s.switched_in);
            let since = zoraya_cpu::period_start(now, period_ns).max(s.switched_in);
            zoraya_cpu::charge(
                &mut JobTree::new(),
                // SAFETY: `current` (an `Arc<Thread>`, so its process and the
                // job chain above it) is a local until the end of this block.
                unsafe { JobRef::of(process.job()) },
                ran,
                now.saturating_sub(since),
                zoraya_cpu::period_of(now, period_ns),
            );
        }
        let idle = s.idle.as_ref().expect("idle thread");
        let is_idle = Arc::ptr_eq(&current, idle);
        match current.state.load(Ordering::Relaxed) {
            RUNNING => {
                current.state.store(READY, Ordering::Relaxed);
                if !is_idle {
                    s.ready.push_back(current);
                }
            }
            DEAD => s.dead.push(current),
            // A blocked thread is owned by whoever will wake it.
            _ => {}
        }
        // The first ready thread whose job has budget; those ahead of it
        // whose job has none are parked (ADR-0264 point 5).
        // `None` with threads still ready: the cap of parks per pick was hit;
        // idle runs and calls `schedule` again (see `RunQueue::pick`).
        let picked = s.ready.pick(&mut JobTree::new(), now, period_ns, |t| {
            // SAFETY: `pick` owns `t` (popped from the queue) until it has
            // decided on it, and a thread holds its process and so the jobs.
            t.process.as_ref().map(|p| unsafe { JobRef::of(p.job()) })
        });
        arm_release(s, now);
        let (next, limited) = match picked {
            Some(p) => (p.thread, p.slice_ns),
            None => (s.idle.clone().expect("idle thread"), None),
        };
        next.state.store(RUNNING, Ordering::Relaxed);
        s.need_resched = false;
        s.switched_in = now;
        if let Some(id) = s.slice.take() {
            timer::cancel(id);
        }
        let user = next.process.is_some();
        // A limited job's slice is armed also for a lone thread (point 4).
        let slice = match limited {
            Some(ns) if user => Some(ns.min(slice_ns())),
            _ if user && !s.ready.is_empty() => Some(slice_ns()),
            _ => None,
        };
        if let Some(ns) = slice {
            let end = timer::now().plus_nanos(ns);
            s.slice = timer::arm_reserved(end, end_of_slice, 0).ok();
        }
        let ttbr0 = next.process.as_ref().map(|p| p.ttbr0());
        let raw: *const Thread = Arc::as_ptr(&next);
        s.current = Some(next);
        (prev, raw, ttbr0)
    };
    if !core::ptr::eq(prev, next) {
        // SAFETY: `ttbr0` is the table of `next`'s live process, or none.
        unsafe { mmu::set_user_table(ttbr0) };
        // SAFETY: every exception is masked. Both threads are kept alive past
        // the switch: `next` by SCHED.current; `prev` by the ready queue, the
        // parked list, the dead list, `s.idle`, its process's `thread`, or the waiter list or
        // count that will wake it. No local here holds a count.
        unsafe { arch::switch((*prev).context.get(), (*next).context.get()) };
    }
    arch::restore_interrupts(daif);
}

/// Gives way to any ready thread.
pub fn yield_now() {
    schedule();
}

/// Switches away if the slice ended. Called on the way back to EL0, on the
/// thread's own stack, with interrupts masked. True if it called the
/// scheduler, which may have run other threads before this one again.
pub fn preempt_point() -> bool {
    let (due, other) = {
        let s = SCHED.lock();
        (s.need_resched, !s.ready.is_empty())
    };
    if due {
        if other {
            PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
        }
        schedule();
    }
    due
}

/// The running thread.
pub fn current() -> Arc<Thread> {
    SCHED.lock().current.clone().expect("a thread is running")
}

/// Stops running until [`wake`]. Interrupts must be masked since the check
/// that decided to block, and someone must hold the thread: a waiter list, a
/// deadline, or its process.
pub fn block_current() {
    let current: *const Thread = {
        let s = SCHED.lock();
        Arc::as_ptr(s.current.as_ref().expect("a thread is running"))
    };
    // SAFETY: the running thread is alive.
    unsafe { (*current).state.store(BLOCKED, Ordering::Relaxed) };
    schedule();
}

/// Makes a blocked thread ready. A slice is not needed: the woken thread takes
/// over at the next return to EL0.
pub fn wake(thread: Arc<Thread>) {
    let mut s = SCHED.lock();
    if thread.state.load(Ordering::Relaxed) == BLOCKED {
        thread.state.store(READY, Ordering::Relaxed);
        s.ready.push_back(thread);
        s.need_resched = true;
    }
}

fn wake_sleeper(raw: usize) {
    // SAFETY: `block_until` made this from `Arc::into_raw`, and this callback
    // runs once, or `block_until` cancels it.
    wake(unsafe { Arc::from_raw(raw as *const Thread) });
}

/// Blocks until [`wake`] or `deadline`. Interrupts must be masked since the
/// check that decided to block. An error if the deadline queue is full. With
/// `Infinite`, someone other than a deadline must hold the thread.
pub fn block_until(deadline: Deadline) -> Result<(), zoraya_time::QueueFull> {
    let Deadline::At(at) = deadline else {
        block_current();
        return Ok(());
    };
    let raw = Arc::into_raw(current());
    let id = match timer::arm(at, wake_sleeper, raw as usize) {
        Ok(id) => id,
        Err(e) => {
            // SAFETY: made just above and given to nobody.
            drop(unsafe { Arc::from_raw(raw) });
            return Err(e);
        }
    };
    block_current();
    if timer::cancel(id) {
        // SAFETY: the callback did not run and now never will, so its count
        // is still here.
        drop(unsafe { Arc::from_raw(raw) });
    }
    Ok(())
}

/// Blocks until `deadline`. An error if the deadline queue is full. For
/// kernel threads; a user thread's `sleep_until` is `thread::sleep_until`.
#[cfg(feature = "qemu-test")]
pub fn sleep_until(deadline: Deadline) -> Result<(), zoraya_time::QueueFull> {
    let daif = arch::save_and_mask_interrupts();
    let held = match deadline {
        // Nothing else holds a kernel thread: this count keeps its memory
        // valid for good. A user thread's process holds it, to end it.
        Deadline::Infinite if current().process.is_none() => Some(Arc::into_raw(current())),
        _ => None,
    };
    let result = block_until(deadline);
    arch::restore_interrupts(daif);
    let _ = held;
    result
}

/// Ends the running thread.
pub fn exit_current() -> ! {
    // Never restored: this thread does not come back.
    arch::save_and_mask_interrupts();
    let current: *const Thread = {
        let mut s = SCHED.lock();
        s.threads -= 1;
        Arc::as_ptr(s.current.as_ref().expect("a thread is running"))
    };
    // SAFETY: the running thread is alive.
    unsafe { (*current).state.store(DEAD, Ordering::Relaxed) };
    schedule();
    unreachable!("a dead thread was scheduled");
}

/// After a test failed on another thread, the harness goes on on the boot
/// stack: that thread is abandoned, and the boot thread takes over. The
/// abandoned thread is leaked, since the panic may still be running on its
/// stack.
#[cfg(feature = "qemu-test")]
pub fn adopt_boot_thread() {
    let mut s = SCHED.lock();
    let boot = s.boot.clone().expect("boot thread");
    if !s.current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &boot)) {
        boot.state.store(RUNNING, Ordering::Relaxed);
        s.ready.retain(|t| !Arc::ptr_eq(t, &boot));
        if let Some(old) = s.current.replace(boot) {
            core::mem::forget(old);
        }
        // SAFETY: no table is the safe value.
        unsafe { mmu::set_user_table(None) };
    }
}

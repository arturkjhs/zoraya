//! Threads and the scheduler (ADR-0030).
//!
//! One run queue, round robin. The kernel is not preempted: a slice that ends
//! takes effect on return to EL0, or at `yield_now` and blocking points.
//! The slice is a one-shot deadline, armed only when another thread is ready,
//! so a lone thread takes no timer interrupts.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use zoraya_abi::Status;
use zoraya_time::{Deadline, TimerId};

use crate::arch::THREAD_STACK_SIZE;

use crate::arch;
use crate::arch::Context;
use crate::arch::mmu;
use crate::charge::{self, Paid};
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
    /// point 5); the boot and kernel threads have no payer.
    _paid: Paid,
}

// SAFETY: `context` is only touched by `schedule`, with interrupts masked, on
// the one CPU: saved for the running thread and read for the next. Everything
// else is atomic or immutable.
unsafe impl Send for Thread {}
// SAFETY: as above.
unsafe impl Sync for Thread {}

impl Thread {
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
    ready: VecDeque<Arc<Thread>>,
    /// Ended threads, freed by a later thread: none can free its own stack.
    dead: Vec<Arc<Thread>>,
    /// Threads that exist, for reserving queue space.
    threads: usize,
    need_resched: bool,
    slice: Option<TimerId>,
}

static SCHED: SpinLock<Sched> = SpinLock::new(Sched {
    current: None,
    #[cfg(feature = "qemu-test")]
    boot: None,
    idle: None,
    ready: VecDeque::new(),
    dead: Vec::new(),
    threads: 0,
    need_resched: false,
    slice: None,
});

static PREEMPTIONS: AtomicU64 = AtomicU64::new(0);

/// Times a thread was switched out because its slice ended.
#[cfg(feature = "qemu-test")]
pub fn preemptions() -> u64 {
    PREEMPTIONS.load(Ordering::Relaxed)
}

fn boot_thread() -> Arc<Thread> {
    // uncharged: the boot thread, made before the heap can refuse.
    Arc::new(Thread {
        context: UnsafeCell::new(Context::default()),
        state: AtomicU8::new(RUNNING),
        _stack: None,
        process: None,
        _paid: Paid::default(),
    })
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
    uncharged_arc(Thread {
        context: UnsafeCell::new(context),
        state: AtomicU8::new(READY),
        _stack: Some(stack),
        process: None,
        _paid: Paid::default(),
    })
}

/// Reserves queue room for one more thread, so that the scheduler itself never
/// has to allocate.
fn count_new() -> Result<(), OutOfMemory> {
    let mut s = SCHED.lock();
    let threads = s.threads + 1;
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

/// Makes `process`'s thread, to go to EL0 at `pc` with `sp`. It is blocked:
/// [`wake`] starts it, and [`discard`] gives it up.
pub fn spawn_user(process: Arc<Process>, pc: u64, sp: u64) -> Result<Arc<Thread>, Status> {
    let paid = thread_paid(&process)?;
    let stack = KernelStack::new().map_err(|_| Status::NoMemory)?;
    let frame = stack.push_user_frame(pc, sp);
    let context = Context::new_user(frame, stack.limit());
    let thread = charge::prepaid_arc(Thread {
        context: UnsafeCell::new(context),
        state: AtomicU8::new(BLOCKED),
        _stack: Some(stack),
        process: Some(process),
        _paid: paid,
    })?;
    count_new().map_err(|_| Status::NoMemory)?;
    Ok(thread)
}

/// What a user thread costs its process's job: the record, the kernel stack
/// and a place in each of the scheduler's two queues.
fn thread_paid(process: &Process) -> Result<Paid, Status> {
    let mut paid = Paid::new(Some(process.job()));
    let queues = 2 * core::mem::size_of::<usize>() as u64;
    paid.kernel(charge::arc_cost::<Thread>() + THREAD_STACK_SIZE as u64 + queues)?;
    Ok(paid)
}

/// Makes `process`'s thread from a copy of `frame`, returning `Ok` and 0
/// from the call that frame was saved at, with `TPIDR_EL0` set to
/// `tpidr_el0`. Blocked, as from [`spawn_user`].
pub fn spawn_user_from(
    process: Arc<Process>,
    frame: &arch::Frame,
    tpidr_el0: u64,
) -> Result<Arc<Thread>, Status> {
    let paid = thread_paid(&process)?;
    let stack = KernelStack::new().map_err(|_| Status::NoMemory)?;
    let at = stack.push_user_frame_copy(frame, zoraya_abi::Status::Ok as u64, 0);
    let context = Context::new_user_with_tls(at, stack.limit(), tpidr_el0);
    let thread = charge::prepaid_arc(Thread {
        context: UnsafeCell::new(context),
        state: AtomicU8::new(BLOCKED),
        _stack: Some(stack),
        process: Some(process),
        _paid: paid,
    })?;
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
            arch::wait_for_interrupt();
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

fn end_of_slice(_: usize) {
    SCHED.lock().need_resched = true;
}

/// Chooses the next thread and switches to it. Returns when this thread next
/// runs, and never if it is dead.
fn schedule() {
    reap();
    let daif = arch::save_and_mask_all();
    let (prev, next, ttbr0) = {
        let mut guard = SCHED.lock();
        let s = &mut *guard;
        let current = s.current.take().expect("a thread is running");
        let prev: *const Thread = Arc::as_ptr(&current);
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
        let next = s
            .ready
            .pop_front()
            .unwrap_or_else(|| s.idle.clone().expect("idle thread"));
        next.state.store(RUNNING, Ordering::Relaxed);
        s.need_resched = false;
        if let Some(id) = s.slice.take() {
            timer::cancel(id);
        }
        let user = next.process.is_some();
        if user && !s.ready.is_empty() {
            let end = timer::now().plus_nanos(slice_ns());
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
        // dead list, `s.idle`, its process's `thread`, or the waiter list or
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
/// thread's own stack, with interrupts masked.
pub fn preempt_point() {
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

/// Blocks until `deadline`. An error if the deadline queue is full.
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

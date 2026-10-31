//! Threads and the scheduler (ADR-0030).
//!
//! One run queue, round robin. The kernel is not preempted: a slice that ends
//! takes effect on return to EL0, or at `yield_now` and blocking points.
//! The slice is a one-shot deadline, armed only when another thread is ready,
//! so a lone thread takes no timer interrupts.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use zoraya_time::{Deadline, TimerId};

use crate::arch;
use crate::arch::Context;
use crate::arch::mmu;
use crate::memory::heap::OutOfMemory;
use crate::memory::stack::KernelStack;
use crate::process::Process;
use crate::sync::SpinLock;
use crate::timer;

/// A running user thread's time slice.
const SLICE_NS: u64 = 10_000_000;

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
}

// SAFETY: `context` is only touched by `schedule`, with interrupts masked, for
// a thread that is not running; everything else is atomic or immutable.
unsafe impl Send for Thread {}
// SAFETY: as above.
unsafe impl Sync for Thread {}

impl Thread {
    /// The process this thread belongs to.
    pub fn process(&self) -> Option<&Arc<Process>> {
        self.process.as_ref()
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

fn boot_thread() -> Result<Arc<Thread>, OutOfMemory> {
    arc(Thread {
        context: UnsafeCell::new(Context::default()),
        state: AtomicU8::new(RUNNING),
        _stack: None,
        process: None,
    })
}

fn arc(thread: Thread) -> Result<Arc<Thread>, OutOfMemory> {
    // `Arc::try_new` is not stable; a box first would only move the failure.
    let boxed = crate::memory::heap::try_box(thread)?;
    Ok(Arc::from(boxed as Box<Thread>))
}

/// Makes the running code the boot thread and creates the idle thread. Call
/// once, with the heap up.
pub fn init() {
    let boot = boot_thread().expect("boot thread");
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
    arc(Thread {
        context: UnsafeCell::new(context),
        state: AtomicU8::new(READY),
        _stack: Some(stack),
        process: None,
    })
}

/// Reserves queue room for one more thread, so that the scheduler itself never
/// has to allocate, then queues it.
fn enqueue_new(thread: Arc<Thread>) -> Result<Arc<Thread>, OutOfMemory> {
    let mut s = SCHED.lock();
    let threads = s.threads + 1;
    s.ready.try_reserve(threads).map_err(|_| OutOfMemory)?;
    s.dead.try_reserve(threads).map_err(|_| OutOfMemory)?;
    s.threads = threads;
    s.ready.push_back(thread.clone());
    Ok(thread)
}

/// Starts a kernel thread running `entry(arg)`. When `entry` returns, the
/// thread ends.
#[cfg(feature = "qemu-test")]
pub fn spawn_kernel(entry: fn(usize), arg: usize) -> Result<Arc<Thread>, OutOfMemory> {
    enqueue_new(new_kernel(entry, arg)?)
}

/// Starts `process`'s thread at EL0 with `pc` and `sp`.
pub fn spawn_user(process: Arc<Process>, pc: u64, sp: u64) -> Result<Arc<Thread>, OutOfMemory> {
    let stack = KernelStack::new()?;
    let frame = stack.push_user_frame(pc, sp);
    let context = Context::new_user(frame, stack.limit());
    enqueue_new(arc(Thread {
        context: UnsafeCell::new(context),
        state: AtomicU8::new(READY),
        _stack: Some(stack),
        process: Some(process),
    })?)
}

/// Runs on a new kernel thread, from boot-time assembly.
#[unsafe(no_mangle)]
extern "C" fn kernel_thread_main(entry: usize, arg: usize) -> ! {
    // SAFETY: threads exist only after the vectors and GIC are up. New threads
    // start from `schedule`, with interrupts masked.
    unsafe { arch::unmask_interrupts() };
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
    let daif = arch::save_and_mask_interrupts();
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
            let end = timer::now().plus_nanos(SLICE_NS);
            s.slice = timer::arm(end, end_of_slice, 0).ok();
        }
        let ttbr0 = next.process.as_ref().map(|p| p.ttbr0());
        let raw: *const Thread = Arc::as_ptr(&next);
        s.current = Some(next);
        (prev, raw, ttbr0)
    };
    if !core::ptr::eq(prev, next) {
        mmu::set_user_table(ttbr0);
        // SAFETY: interrupts are masked. Both threads are kept alive past the
        // switch: `next` by SCHED.current; `prev` by the ready queue, the dead
        // list, or whoever will wake it, and no local here holds a count.
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
    let due = SCHED.lock().need_resched;
    if due {
        PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
        schedule();
    }
}

/// The running thread.
pub fn current() -> Arc<Thread> {
    SCHED.lock().current.clone().expect("a thread is running")
}

/// Stops running until [`wake`]. Interrupts must be masked since the check
/// that decided to block, and someone must hold the thread to wake it.
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
    // SAFETY: `sleep_until` made this from `Arc::into_raw`, and this callback
    // runs once.
    wake(unsafe { Arc::from_raw(raw as *const Thread) });
}

/// Blocks until `deadline`. An error if the deadline queue is full.
pub fn sleep_until(deadline: Deadline) -> Result<(), zoraya_time::QueueFull> {
    let daif = arch::save_and_mask_interrupts();
    let raw = Arc::into_raw(current());
    if let Deadline::At(at) = deadline
        && let Err(e) = timer::arm(at, wake_sleeper, raw as usize)
    {
        // SAFETY: made just above and given to nobody.
        drop(unsafe { Arc::from_raw(raw) });
        arch::restore_interrupts(daif);
        return Err(e);
    }
    // With `Infinite` nothing wakes it, and the count made above keeps the
    // thread's memory valid for good.
    block_current();
    arch::restore_interrupts(daif);
    Ok(())
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
        mmu::set_user_table(None);
    }
}

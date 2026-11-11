//! The reaper (ADR-0270 point 4): a kernel thread that frees the frames of
//! dead address spaces and closed memory objects in steps of
//! [`steps::STEP`](crate::steps::STEP) frames with a yield between them, so
//! that the exit, the `job_kill` or the `handle_close` that dropped them
//! does not keep the CPU for the time their size takes.
//!
//! What it has not yet freed stays charged to the job that paid for it (the
//! [`Paid`] travels with the frames), and is shown as `held` (ADR-0261
//! point 9). It is a kernel thread and is never capped (ADR-0264 point 7).
//! What is small is freed on the spot: a step is the most one drop frees.

use alloc::sync::Arc;
use alloc::vec::Vec;

use zoraya_pmm::Frame;

use crate::charge::Paid;
use crate::memory::frames;
use crate::sched::{self, Thread};
use crate::steps::STEP;

/// Frames the reaper frees before it yields. The reaper runs with interrupts
/// on, so its steps never delay one (INV-0270-1 counts masked time); the size
/// is only how long it holds the CPU against the threads waiting, and large
/// enough that it frees at least as fast as processes can be made.
const REAPER_STEP: usize = 1024;
use crate::sync::SpinLock;

/// Items the queue holds from the start; past them it grows, or a drop frees
/// on the spot.
const QUEUE_ROOM: usize = 256;

/// Frames to free and the charge they carry.
struct Garbage {
    frames: Vec<Frame>,
    _paid: Paid,
}

struct Queue {
    items: Vec<Garbage>,
    /// The reaper, once started.
    thread: Option<Arc<Thread>>,
    /// An item is being freed.
    busy: bool,
}

static QUEUE: SpinLock<Queue> = SpinLock::new(Queue {
    items: Vec::new(),
    thread: None,
    busy: false,
});

/// Starts the thread. Call once, after `sched::init`.
pub fn start() {
    // Room for a burst of drops before the first one: a drop that fails and
    // frees frames must not make the heap take frames for the queue.
    QUEUE
        .lock()
        .items
        .try_reserve_exact(QUEUE_ROOM)
        .expect("room for the reaper's queue");
    let thread = sched::spawn_kernel(reaper_main, 0).expect("reaper thread");
    QUEUE.lock().thread = Some(thread);
}

/// Frees `frames` and then gives `paid` back: on the spot if there are few,
/// or on the reaper (ADR-0270 point 4). Never blocks and holds no lock, so
/// any drop may call it.
pub fn release(frames: Vec<Frame>, paid: Paid) {
    let (frames, paid) = if frames.len() > STEP {
        match defer(frames, paid) {
            Ok(()) => return,
            Err(back) => back,
        }
    } else {
        (frames, paid)
    };
    for frame in frames {
        frames::free(frame);
    }
    drop(paid);
}

/// Queues the frames, or gives them back if the queue has no room or the
/// reaper is not running.
fn defer(frames: Vec<Frame>, paid: Paid) -> Result<(), (Vec<Frame>, Paid)> {
    let mut queue = QUEUE.lock();
    let Some(thread) = queue.thread.clone() else {
        return Err((frames, paid));
    };
    // Never grows: a drop must not make the heap take frames, and a full
    // queue means the drop frees on the spot.
    if queue.items.len() == queue.items.capacity() {
        return Err((frames, paid));
    }
    queue.items.push(Garbage {
        frames,
        _paid: paid,
    });
    drop(queue);
    sched::wake(thread);
    Ok(())
}

/// True when nothing waits to be freed.
#[cfg(feature = "qemu-test")]
pub fn idle() -> bool {
    let queue = QUEUE.lock();
    queue.items.is_empty() && !queue.busy
}

/// Yields until the reaper has freed everything queued: for tests that
/// compare counts with their starting values (INV-0270-2).
#[cfg(feature = "qemu-test")]
pub fn drain() {
    // At least one turn of the scheduler, which drops a dead thread's stack
    // (`schedule` calls `reap`), then until the reaper is idle.
    sched::yield_now();
    while !idle() {
        sched::yield_now();
    }
}

fn reaper_main(_: usize) {
    loop {
        let item = {
            let mut queue = QUEUE.lock();
            let item = queue.items.pop();
            queue.busy = item.is_some();
            item
        };
        match item {
            Some(mut garbage) => {
                while !garbage.frames.is_empty() {
                    let keep = garbage.frames.len().saturating_sub(REAPER_STEP);
                    for frame in garbage.frames.drain(keep..) {
                        frames::free(frame);
                    }
                    sched::yield_now();
                }
                // The charge goes back only now.
                drop(garbage);
                QUEUE.lock().busy = false;
            }
            None => {
                // Masked from the check to the block, so that a `defer` cannot
                // slip between them.
                let daif = crate::arch::save_and_mask_interrupts();
                if QUEUE.lock().items.is_empty() {
                    sched::block_current();
                }
                crate::arch::restore_interrupts(daif);
            }
        }
    }
}

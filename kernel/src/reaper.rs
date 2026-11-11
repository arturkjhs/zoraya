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
use crate::sync::SpinLock;

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
    if queue.items.try_reserve(1).is_err() {
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
                    let keep = garbage.frames.len().saturating_sub(STEP);
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

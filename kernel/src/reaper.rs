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

/// Items the queue holds from the start; past them `grow_queue` doubles it.
const QUEUE_ROOM: usize = 256;

/// Frames to free and the charge they carry.
struct Garbage {
    frames: Vec<Frame>,
    /// A contiguous run not yet freed: first frame and count.
    run: Option<(Frame, u64)>,
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
    let mut items = Vec::new();
    items
        .try_reserve_exact(QUEUE_ROOM)
        .expect("room for the reaper's queue");
    QUEUE.lock().items = items;
    let thread = sched::spawn_kernel(reaper_main, 0).expect("reaper thread");
    QUEUE.lock().thread = Some(thread);
}

/// Frees `frames` and then gives `paid` back: on the spot if there are few,
/// or on the reaper (ADR-0270 point 4). Never blocks and holds no lock, so
/// any drop may call it.
pub fn release(frames: Vec<Frame>, paid: Paid) {
    let (frames, paid) = if frames.len() > STEP {
        grow_queue();
        match defer(frames, None, paid) {
            Ok(()) => return,
            Err((frames, _, paid)) => (frames, paid),
        }
    } else {
        (frames, paid)
    };
    for frame in frames {
        frames::free(frame);
    }
    drop(paid);
}

/// As [`release`], for a contiguous run of `count` frames at `first`.
pub fn release_run(first: Frame, count: u64, paid: Paid) {
    let paid = if count > STEP as u64 {
        grow_queue();
        match defer(Vec::new(), Some((first, count)), paid) {
            Ok(()) => return,
            Err((_, _, paid)) => paid,
        }
    } else {
        paid
    };
    frames::free_contig(first, count);
    drop(paid);
}

/// Makes room for one more item, outside the queue's lock: the queue doubles
/// when full, so that no number of drops makes one free its frames on the
/// spot (INV-0270-1). If the heap cannot grow it, `defer` fails and the drop
/// frees on the spot, which is only the case at exhaustion.
fn grow_queue() {
    let (len, capacity) = {
        let queue = QUEUE.lock();
        (queue.items.len(), queue.items.capacity())
    };
    if len < capacity || QUEUE.lock().thread.is_none() {
        return;
    }
    let mut bigger = Vec::new();
    if bigger
        .try_reserve_exact(capacity.max(QUEUE_ROOM) * 2)
        .is_err()
    {
        return;
    }
    let mut queue = QUEUE.lock();
    // Another drop may have grown it meanwhile.
    if queue.items.len() == queue.items.capacity() {
        bigger.append(&mut queue.items);
        core::mem::swap(&mut queue.items, &mut bigger);
    }
    drop(queue);
    // `bigger` is now the old, empty vector, or the unused new one: it goes
    // back to the heap outside the lock.
}

/// Queues the frames, or gives them back if the queue has no room or the
/// reaper is not running.
type Back = (Vec<Frame>, Option<(Frame, u64)>, Paid);

fn defer(frames: Vec<Frame>, run: Option<(Frame, u64)>, paid: Paid) -> Result<(), Back> {
    let mut queue = QUEUE.lock();
    let Some(thread) = queue.thread.clone() else {
        return Err((frames, run, paid));
    };
    // Never grows under the lock: `grow_queue` did it before, and a queue
    // still full (the heap is exhausted) means the drop frees on the spot.
    if queue.items.len() == queue.items.capacity() {
        return Err((frames, run, paid));
    }
    queue.items.push(Garbage {
        frames,
        run,
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
    // Before the reaper there is no scheduler to turn (early boot).
    if QUEUE.lock().thread.is_none() {
        return;
    }
    // At least one turn of the scheduler, which drops a dead thread's stack
    // (`schedule` calls `reap`), then until the reaper is idle.
    sched::yield_now();
    while !idle() {
        sched::yield_now();
    }
}

/// Frees up to `max` queued frames on the spot and says whether it freed
/// any. Drops nothing that frees heap or gives a charge back, so it can run
/// under the heap's lock; the reaper drops the emptied items.
fn free_some(max: usize) -> bool {
    let mut queue = QUEUE.lock();
    let mut left = max;
    for item in queue.items.iter_mut() {
        while left > 0 {
            let Some(frame) = item.frames.pop() else {
                break;
            };
            frames::free(frame);
            left -= 1;
        }
        if let Some((first, count)) = item.run
            && left > 0
        {
            let n = (left as u64).min(count);
            frames::free_contig(first, n);
            left -= n as usize;
            item.run = (count > n).then(|| {
                (
                    Frame::from_addr(first.addr() + n * zoraya_pmm::FRAME_SIZE)
                        .expect("in the run"),
                    count - n,
                )
            });
        }
        if left == 0 {
            break;
        }
    }
    left < max
}

/// The frames queued and not yet freed. Like `take`, it takes only the
/// queue's lock, so it runs under the heap's.
pub fn held() -> u64 {
    let queue = QUEUE.lock();
    queue
        .items
        .iter()
        .map(|item| item.frames.len() as u64 + item.run.map_or(0, |(_, count)| count))
        .sum()
}

/// For a contiguous run that found none: frees a step of what the reaper
/// holds, so that a short machine is not refused because teardown has not
/// caught up. False if there is nothing queued. It is a step of work: the
/// caller makes a step point after it (`frames::alloc_contig`, INV-0270-1).
pub fn help() -> bool {
    free_some(STEP)
}

/// For an allocation of one frame that found none free: hands it one of the
/// frames the reaper holds, without freeing it first, so that memory on its
/// way back is not exhaustion and the allocation stays one unit of work
/// (INV-0270-1; `help`, which this replaces there, freed a step for each
/// frame). The dead object's charge stays until its item is emptied and
/// dropped, as for the frames still queued, and the new owner's job pays for
/// the frame as for any other (ADR-0261). Like `free_some` it frees no heap
/// and gives no charge back, so it runs under the heap's lock; it takes only
/// the queue's.
pub fn take() -> Option<Frame> {
    let mut queue = QUEUE.lock();
    for item in queue.items.iter_mut() {
        if let Some(frame) = item.frames.pop() {
            return Some(frame);
        }
        if let Some((first, count)) = item.run {
            item.run = (count > 1).then(|| {
                (
                    Frame::from_addr(first.addr() + zoraya_pmm::FRAME_SIZE).expect("in the run"),
                    count - 1,
                )
            });
            return Some(first);
        }
    }
    None
}

fn reaper_main(_: usize) {
    loop {
        // Frames first, `REAPER_STEP` of them in steps of `STEP` under the
        // lock, so that the lock is held for one step only.
        let mut freed = false;
        for _ in 0..REAPER_STEP / STEP {
            if !free_some(STEP) {
                break;
            }
            freed = true;
        }
        // An item with no frames left is dropped outside the lock: its list
        // goes back to the heap and its charge is given back.
        let done = {
            let mut queue = QUEUE.lock();
            let position = queue
                .items
                .iter()
                .position(|g| g.frames.is_empty() && g.run.is_none());
            position.map(|at| {
                queue.busy = true;
                queue.items.swap_remove(at)
            })
        };
        if let Some(garbage) = done {
            drop(garbage);
            QUEUE.lock().busy = false;
            // A preemption point after each item, so that a long queue of
            // emptied items is not dropped in one turn (ADR-0270 point 4).
            sched::yield_now();
            continue;
        }
        if freed {
            sched::yield_now();
            continue;
        }
        // Masked from the check to the block, so that a `defer` cannot slip
        // between them.
        let daif = crate::arch::save_and_mask_interrupts();
        if QUEUE.lock().items.is_empty() {
            sched::block_current();
        }
        crate::arch::restore_interrupts(daif);
    }
}

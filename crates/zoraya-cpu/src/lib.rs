//! The CPU ceiling of ADR-0264: a budget per job per period, charged at each
//! context switch, and a run queue that parks the threads of a job that spent
//! its budget until the period ends.
//!
//! Time is a plain `u64` of nanoseconds on the monotonic clock, so a test
//! drives the logic with a simulated clock. A period `n` is
//! `[n * period, (n + 1) * period)`: boundaries are global, the same for every
//! job. A node keeps the period its `period_used` was counted in, and reads as
//! zero in any later period, so the end of a period does no work per job.
//!
//! The crate knows nothing of locks or threads: the caller gives it a [`Tree`]
//! and holds whatever keeps the tree still for the call. Nothing here panics
//! on any input and nothing allocates but [`RunQueue::try_reserve`].

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::collections::{TryReserveError, VecDeque};

/// The period (ADR-0264 point 2). The kernel's value is this one; the test
/// kernel can set another for the measurement of ADR-0264's open question.
pub const PERIOD_NS: u64 = 20_000_000;
/// The longest slice of a user thread (ADR-0030).
pub const SLICE_NS: u64 = 10_000_000;
/// The shortest slice of a limited job's thread: with less budget left than
/// this the job still runs this long once, so a slice is never a timer storm.
/// It is also the overrun of ADR-0264 point 4, apart from the time to the
/// next return to EL0. A ceiling below `MIN_SLICE_NS / PERIOD_NS` (0.25%) is
/// therefore a ceiling of about that.
pub const MIN_SLICE_NS: u64 = 50_000;
/// The most threads one [`RunQueue::pick`] parks. A job over its budget can
/// have up to the session's 200 processes ready; the rest are parked by later
/// picks (idle calls again), so no masked interval holds more than this many
/// walks (INV-0270-1).
pub const MAX_PARKS_PER_PICK: usize = 16;
/// The most threads one release returns to the run queue; the caller repeats
/// at once if more are parked (ADR-0264 point 5), with interrupts unmasked
/// between.
pub const RELEASE_BATCH: usize = 64;
/// A ceiling of the whole machine, in thousandths.
pub const FULL: u32 = 1000;

/// The CPU use of one job, and of everything below it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuAcct {
    /// The node's own ceiling in thousandths of a period, if it has one.
    pub limit: Option<u32>,
    /// Nanoseconds run since creation.
    pub used_ns: u64,
    /// The period `period_used` was counted in.
    pub period: u64,
    /// Nanoseconds run in `period`.
    pub period_used: u64,
    /// Times a thread was parked because this node was over budget.
    pub parked: u64,
}

impl CpuAcct {
    pub const fn new() -> CpuAcct {
        CpuAcct {
            limit: None,
            used_ns: 0,
            period: 0,
            period_used: 0,
            parked: 0,
        }
    }

    /// `period_used` as of `period`: zero if it was counted in another one.
    pub const fn used_in(&self, period: u64) -> u64 {
        if self.period == period {
            self.period_used
        } else {
            0
        }
    }
}

/// A tree of nodes with CPU counters. The caller keeps it still during a call.
pub trait Tree {
    type Id: Copy;

    fn parent(&self, id: Self::Id) -> Option<Self::Id>;
    fn get(&self, id: Self::Id) -> CpuAcct;
    fn set(&mut self, id: Self::Id, acct: CpuAcct);
}

/// A ceiling that may not be set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitError {
    /// Zero, or above [`FULL`].
    OutOfRange,
    /// Above the parent's effective ceiling (ADR-0264 point 6).
    AboveParent,
}

/// The budget of a ceiling in a period: `limit` thousandths of `period_ns`.
pub const fn budget_ns(limit: u32, period_ns: u64) -> u64 {
    ((period_ns as u128 * limit as u128) / FULL as u128) as u64
}

/// The period that contains `now_ns`. A `period_ns` of zero is one period.
pub const fn period_of(now_ns: u64, period_ns: u64) -> u64 {
    match now_ns.checked_div(period_ns) {
        Some(period) => period,
        None => 0,
    }
}

/// The start of the period containing `now_ns`.
pub const fn period_start(now_ns: u64, period_ns: u64) -> u64 {
    if period_ns == 0 {
        0
    } else {
        now_ns - now_ns % period_ns
    }
}

/// The start of the next period after `now_ns`.
pub const fn next_boundary(now_ns: u64, period_ns: u64) -> u64 {
    if period_ns == 0 {
        return u64::MAX;
    }
    period_start(now_ns, period_ns).saturating_add(period_ns)
}

/// The smallest ceiling of `id` and its ancestors.
pub fn effective_limit<T: Tree>(tree: &T, id: T::Id) -> Option<u32> {
    let mut best: Option<u32> = None;
    let mut at = Some(id);
    while let Some(node) = at {
        if let Some(limit) = tree.get(node).limit {
            best = Some(best.map_or(limit, |b| b.min(limit)));
        }
        at = tree.parent(node);
    }
    best
}

/// Sets the ceiling of `id` (ADR-0264 points 1 and 6). `None` removes it.
/// A ceiling below the current use is allowed: the job is parked until the
/// period ends.
pub fn set_limit<T: Tree>(tree: &mut T, id: T::Id, limit: Option<u32>) -> Result<(), LimitError> {
    if let Some(new) = limit {
        if new == 0 || new > FULL {
            return Err(LimitError::OutOfRange);
        }
        if let Some(parent) = tree.parent(id)
            && let Some(above) = effective_limit(tree, parent)
            && new > above
        {
            return Err(LimitError::AboveParent);
        }
    }
    let mut acct = tree.get(id);
    acct.limit = limit;
    tree.set(id, acct);
    Ok(())
}

/// Charges a run of `ran_ns` to `id` and every ancestor (ADR-0264 point 3).
/// `in_period_ns` is the part of it that lies in `period`, the period of the
/// switch: a run that began in the period before charges only the part after
/// the boundary to the budget.
pub fn charge<T: Tree>(tree: &mut T, id: T::Id, ran_ns: u64, in_period_ns: u64, period: u64) {
    let in_period_ns = in_period_ns.min(ran_ns);
    let mut at = Some(id);
    while let Some(node) = at {
        let mut a = tree.get(node);
        a.used_ns = a.used_ns.saturating_add(ran_ns);
        a.period_used = a.used_in(period).saturating_add(in_period_ns);
        a.period = period;
        tree.set(node, a);
        at = tree.parent(node);
    }
}

/// What the scheduler does with a thread of a job about to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision<Id> {
    /// No ceiling at or above the job: ADR-0030's slice, armed only when
    /// another thread is ready.
    Unlimited,
    /// Run, with the slice armed for this long even when alone (point 4).
    Run { slice_ns: u64 },
    /// The budget of `at` is spent: park until the period ends.
    Park { at: Id },
}

/// Decides for a thread of `id` at `now_ns`: the most constrained limited
/// ancestor sets the slice, and one with nothing left parks the thread.
pub fn decide<T: Tree>(tree: &T, id: T::Id, now_ns: u64, period_ns: u64) -> Decision<T::Id> {
    let period = period_of(now_ns, period_ns);
    let mut least: Option<u64> = None;
    let mut at = Some(id);
    while let Some(node) = at {
        let a = tree.get(node);
        if let Some(limit) = a.limit {
            let left = budget_ns(limit, period_ns).saturating_sub(a.used_in(period));
            if left == 0 {
                return Decision::Park { at: node };
            }
            least = Some(least.map_or(left, |l| l.min(left)));
        }
        at = tree.parent(node);
    }
    match least {
        None => Decision::Unlimited,
        Some(left) => Decision::Run {
            slice_ns: left.clamp(MIN_SLICE_NS, SLICE_NS),
        },
    }
}

/// Counts a park on the node whose budget was spent.
pub fn note_parked<T: Tree>(tree: &mut T, at: T::Id) {
    let mut a = tree.get(at);
    a.parked = a.parked.saturating_add(1);
    tree.set(at, a);
}

/// What [`RunQueue::pick`] chose.
#[derive(Debug, PartialEq, Eq)]
pub struct Picked<T> {
    pub thread: T,
    /// The slice to arm for a limited job; `None` for an unlimited one.
    pub slice_ns: Option<u64>,
}

/// The run queue of ADR-0030 and the parked list of ADR-0264 point 5.
///
/// `ready` is round robin. A thread is parked only by [`RunQueue::pick`], when
/// it reaches the front with its job over budget, so a thread woken or
/// preempted just joins `ready` (and is parked before it can run). The parked threads return to the end of
/// `ready` in [`RunQueue::release`], called when the period ends: one list for
/// all jobs, since they all come back at the same boundary.
pub struct RunQueue<T> {
    ready: VecDeque<T>,
    parked: VecDeque<T>,
}

impl<T> RunQueue<T> {
    pub const fn new() -> RunQueue<T> {
        RunQueue {
            ready: VecDeque::new(),
            parked: VecDeque::new(),
        }
    }

    /// Room for `n` more threads in each list, so that no later push
    /// allocates.
    pub fn try_reserve(&mut self, n: usize) -> Result<(), TryReserveError> {
        self.ready.try_reserve(n)?;
        self.parked.try_reserve(n)
    }

    /// No thread is ready to run. Parked threads are not ready.
    pub fn is_empty(&self) -> bool {
        self.ready.is_empty()
    }

    pub fn parked_len(&self) -> usize {
        self.parked.len()
    }

    /// Adds a thread at the end of the ready list.
    pub fn push_back(&mut self, thread: T) {
        self.ready.push_back(thread);
    }

    /// Keeps the threads in both lists for which `keep` holds.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.ready.retain(&mut keep);
        self.parked.retain(&mut keep);
    }

    /// Takes the first ready thread whose job has budget. `job_of` gives the
    /// job of a thread, `None` for a kernel thread (never limited). Threads
    /// ahead of it whose job has none are parked, at most
    /// [`MAX_PARKS_PER_PICK`] per call, which bounds the walks one masked
    /// interval holds (INV-0270-1). At that cap the thread found next is put
    /// back at the front and `None` is returned: the caller runs idle, and
    /// idle calls again while the ready list is not empty, so the rest are
    /// parked in further intervals and **no thread of a spent job ever runs**
    /// (the overrun per period stays one floor slice however many threads
    /// the job has). `None` with [`RunQueue::is_empty`] false means "call
    /// again".
    pub fn pick<Tr: Tree>(
        &mut self,
        tree: &mut Tr,
        now_ns: u64,
        period_ns: u64,
        job_of: impl Fn(&T) -> Option<Tr::Id>,
    ) -> Option<Picked<T>> {
        let mut parks = 0;
        loop {
            let thread = self.ready.pop_front()?;
            let Some(job) = job_of(&thread) else {
                return Some(Picked {
                    thread,
                    slice_ns: None,
                });
            };
            match decide(tree, job, now_ns, period_ns) {
                Decision::Unlimited => {
                    return Some(Picked {
                        thread,
                        slice_ns: None,
                    });
                }
                Decision::Run { slice_ns } => {
                    return Some(Picked {
                        thread,
                        slice_ns: Some(slice_ns),
                    });
                }
                Decision::Park { at } if parks < MAX_PARKS_PER_PICK => {
                    parks += 1;
                    note_parked(tree, at);
                    self.parked.push_back(thread);
                }
                Decision::Park { .. } => {
                    self.ready.push_front(thread);
                    return None;
                }
            }
        }
    }

    /// Returns up to [`RELEASE_BATCH`] parked threads to the end of the ready
    /// list, in the order they were parked. The number returned; the caller
    /// calls again while threads remain parked.
    pub fn release(&mut self) -> usize {
        let mut moved = 0;
        while moved < RELEASE_BATCH {
            let Some(thread) = self.parked.pop_front() else {
                break;
            };
            self.ready.push_back(thread);
            moved += 1;
        }
        moved
    }
}

impl<T> Default for RunQueue<T> {
    fn default() -> Self {
        RunQueue::new()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
zoraya_test_alloc::cap_memory!();

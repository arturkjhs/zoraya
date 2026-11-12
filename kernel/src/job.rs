//! Jobs (ADR-0003, ADR-0004, ADR-0046). Every process is in one, and a job
//! tree is how a set of processes is signalled or killed as one.
//!
//! [`TREE`] covers adding to a job, leaving one, and one step of a walk of a
//! tree (ADR-0270), so a process being created is either in the walk or
//! created after it.

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use zoraya_abi::quota::{FieldUsage, JobLimits, JobUsage};
use zoraya_abi::{ExitReason, ExitRecord, RequestKind, Status, job_signals};
use zoraya_charge::{Counters, Field, Kind};
use zoraya_cpu::CpuAcct;
use zoraya_events::SourceId;

use crate::charge;
use crate::cpu;
use crate::memory::heap::uncharged_arc;
use crate::port;
use crate::process::Process;
use crate::steps;
use crate::sync::SpinLock;

/// Deepest job tree (ADR-0046).
const MAX_DEPTH: u32 = 32;

static TREE: SpinLock<()> = SpinLock::new(());
/// Walks of a tree now running or paused between steps. While there are
/// any, a dropped job leaves its dead slot in its parent's list (a dead
/// `Weak` is skipped) so that no walk's index moves.
static WALKS: AtomicUsize = AtomicUsize::new(0);
static ROOT: SpinLock<Option<Arc<Job>>> = SpinLock::new(None);

struct State {
    children: Vec<Weak<Job>>,
    processes: Vec<Arc<Process>>,
    /// Live processes in this job and below it.
    live: usize,
    /// `job_kill` ran: nothing new may start here.
    closed: bool,
    /// Between `job_suspend` and `job_resume` (ADR-0105 item 8). A process
    /// joining a suspended job starts stopped, and a job made under one starts
    /// suspended.
    suspended: bool,
}

pub struct Job {
    parent: Option<Arc<Job>>,
    depth: u32,
    /// The policy bit `snapshot_process` needs (ADR-0002 rule 3), passed to
    /// child jobs.
    posix: bool,
    source: SourceId,
    state: SpinLock<State>,
    /// The job's counters (ADR-0261), under [`charge`]'s walk.
    acct: SpinLock<Counters>,
    /// The job's CPU ceiling and use (ADR-0264). A leaf lock, taken under the
    /// scheduler's (`cpu.rs`).
    cpu: SpinLock<CpuAcct>,
    /// What the job's own record was charged, given back when it drops.
    record: AtomicU64,
}

impl Job {
    fn new(parent: Option<Arc<Job>>, posix: bool) -> Result<Arc<Job>, Status> {
        let depth = parent.as_ref().map_or(0, |p| p.depth + 1);
        let suspended = parent.as_ref().is_some_and(|p| p.state.lock().suspended);
        let bits = if suspended {
            job_signals::TERMINATED | job_signals::SUSPENDED
        } else {
            job_signals::TERMINATED
        };
        let source = port::new_source(bits)?;
        // uncharged: the job charges its own record next (charge_record), the root has no payer
        let job = uncharged_arc(Job {
            parent,
            depth,
            posix,
            source,
            state: SpinLock::new(State {
                children: Vec::new(),
                processes: Vec::new(),
                live: 0,
                closed: false,
                suspended,
            }),
            acct: SpinLock::new(Counters::new()),
            cpu: SpinLock::new(CpuAcct::new()),
            record: AtomicU64::new(0),
        });
        // On failure the value was dropped, and its `Drop` freed the source.
        job.map_err(|_| Status::NoMemory)
    }

    /// Makes the job pay for its own record, and so its ancestors too
    /// (ADR-0261 point 6). The root has no payer.
    fn charge_record(job: Arc<Job>) -> Result<Arc<Job>, Status> {
        if job.parent.is_some() {
            let cost = charge::arc_cost::<Job>();
            charge::charge_memory(&job, cost, Kind::Kernel)?;
            job.record.store(cost, Ordering::Relaxed);
        }
        Ok(job)
    }

    /// The job of processes the kernel starts.
    pub fn root() -> Result<Arc<Job>, Status> {
        let mut root = ROOT.lock();
        if root.is_none() {
            *root = Some(Job::new(None, false)?);
        }
        root.clone().ok_or(Status::NoMemory)
    }

    /// A job under `parent`. `posix` is inherited (ADR-0046).
    pub fn create(parent: &Arc<Job>, posix: bool) -> Result<Arc<Job>, Status> {
        if parent.depth + 1 >= MAX_DEPTH {
            return Err(Status::LimitReached);
        }
        let job = Job::charge_record(Job::new(Some(parent.clone()), posix || parent.posix)?)?;
        let _tree = TREE.lock();
        let mut state = parent.state.lock();
        if state.closed {
            return Err(Status::BadState);
        }
        if state.children.len() == state.children.capacity() && WALKS.load(Ordering::SeqCst) == 0 {
            // Dead slots left by drops during a walk go before the list grows.
            state.children.retain(|c| c.strong_count() > 0);
        }
        state
            .children
            .try_reserve(1)
            .map_err(|_| Status::NoMemory)?;
        state.children.push(Arc::downgrade(&job));
        drop(state);
        Ok(job)
    }

    pub fn source(&self) -> SourceId {
        self.source
    }

    pub(crate) fn acct(&self) -> &SpinLock<Counters> {
        &self.acct
    }

    pub(crate) fn cpu(&self) -> &SpinLock<CpuAcct> {
        &self.cpu
    }

    pub(crate) fn parent_ref(&self) -> Option<&Job> {
        self.parent.as_deref()
    }

    /// `job_set_limits` (ADR-0261 points 1 to 3, ADR-0264 point 6): all three
    /// limits are set together or none is.
    pub fn set_limits(&self, limits: JobLimits) -> Result<(), Status> {
        let old = self.cpu.lock().limit;
        cpu::set_limit(self, limits.cpu)?;
        let result = charge::set_limits(self, limits.memory, limits.processes);
        if result.is_err() {
            cpu::restore_limit(self, old);
        }
        result
    }

    /// `job_get_usage` (ADR-0261 point 11). With `clear`, also clears
    /// `OVER_QUOTA`.
    pub fn usage(&self, clear: bool) -> JobUsage {
        if clear {
            port::clear_signals(self.source, job_signals::OVER_QUOTA);
        }
        let counters = charge::counters(self);
        let field = |f: Field| {
            let c = counters.get(f);
            FieldUsage {
                limit: c.limit,
                used: c.used,
                peak: c.peak,
                refused: c.refused,
            }
        };
        let (memory, processes) = (field(Field::Memory), field(Field::Processes));
        let user = counters.get(Field::User).used;
        let state = self.state.lock();
        // Charges kept alive from outside after the last process ended.
        let held = if state.closed && state.live == 0 {
            memory.used
        } else {
            0
        };
        JobUsage {
            memory,
            processes,
            user,
            kernel: memory.used.saturating_sub(user),
            held,
            cpu: cpu::usage(self),
        }
    }

    /// The `posix` policy bit.
    pub fn is_posix(&self) -> bool {
        self.posix
    }

    /// No live process in the tree.
    #[cfg(feature = "qemu-test")]
    pub fn is_empty(&self) -> bool {
        self.state.lock().live == 0
    }

    /// True if this job is `ancestor` or lies under it.
    pub fn is_within(&self, ancestor: &Job) -> bool {
        let mut job = self;
        loop {
            if core::ptr::eq(job, ancestor) {
                return true;
            }
            match &job.parent {
                Some(parent) => job = parent,
                None => return false,
            }
        }
    }

    fn add_live(&self, count: isize) {
        let mut job = Some(self);
        while let Some(j) = job {
            let (was, now) = {
                let mut state = j.state.lock();
                let was = state.live;
                state.live = was.wrapping_add_signed(count);
                (was, state.live)
            };
            match (was, now) {
                (0, _) => port::clear_signals(j.source, job_signals::TERMINATED),
                (_, 0) => port::set_signals(j.source, job_signals::TERMINATED),
                _ => {}
            }
            job = j.parent.as_deref();
        }
    }

    /// Makes `process` a member. `BadState` once the job was killed.
    pub fn admit(&self, process: &Arc<Process>) -> Result<(), Status> {
        let _tree = TREE.lock();
        {
            let mut state = self.state.lock();
            if state.closed {
                return Err(Status::BadState);
            }
            state
                .processes
                .try_reserve(1)
                .map_err(|_| Status::NoMemory)?;
            state.processes.push(process.clone());
            if state.suspended {
                process.stop();
            }
        }
        self.add_live(1);
        Ok(())
    }

    /// Ends `process`'s membership.
    pub fn leave(&self, process: &Arc<Process>) {
        let member = {
            let _tree = TREE.lock();
            let member = {
                let mut state = self.state.lock();
                let at = state.processes.iter().position(|p| Arc::ptr_eq(p, process));
                at.map(|at| state.processes.swap_remove(at))
            };
            self.add_live(-1);
            member
        };
        drop(member);
    }

    /// Visits every job of the tree below `this` (`on_job`, then `on_process`
    /// for each of its processes) in steps of at most [`steps::STEP`] visits
    /// (ADR-0270 point 1). [`TREE`] is held inside a step only: between steps
    /// no lock is held and the call pauses, so the tree may change meanwhile.
    ///
    /// The way through it is a path of at most [`MAX_DEPTH`] frames, each with
    /// the number of children and of processes of its job not yet visited,
    /// counted down from the end of the list: a process that leaves
    /// (`swap_remove`) moves the last one into the hole, which a visit from
    /// the end has either passed or will still reach. Dead children are not
    /// taken out of a list while any walk is running ([`WALKS`]), so no index
    /// moves. What joins during a walk is appended and not reached by it:
    /// a job that is closed or suspended already has the effect (`join`,
    /// `create`), and a signal is for what was there when the call began.
    fn walk(
        this: &Arc<Job>,
        on_job: &mut impl FnMut(&Job),
        on_process: &mut impl FnMut(&Arc<Process>),
    ) {
        struct Frame {
            job: Arc<Job>,
            entered: bool,
            processes: usize,
            children: usize,
        }
        /// Keeps [`WALKS`] true for the life of the walk.
        struct Active;
        impl Active {
            fn new() -> Active {
                WALKS.fetch_add(1, Ordering::SeqCst);
                Active
            }
        }
        impl Drop for Active {
            fn drop(&mut self) {
                WALKS.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _active = Active::new();
        let mut path: [Option<Frame>; MAX_DEPTH as usize + 1] =
            [const { None }; MAX_DEPTH as usize + 1];
        let mut depth = 1;
        path[0] = Some(Frame {
            job: this.clone(),
            entered: false,
            processes: 0,
            children: 0,
        });
        while depth > 0 {
            {
                let _tree = TREE.lock();
                let mut budget = steps::STEP;
                while budget > 0 && depth > 0 {
                    let Some(frame) = path[depth - 1].as_mut() else {
                        break;
                    };
                    budget -= 1;
                    steps::count(1);
                    if !frame.entered {
                        frame.entered = true;
                        on_job(&frame.job);
                        let state = frame.job.state.lock();
                        frame.processes = state.processes.len();
                        frame.children = state.children.len();
                        continue;
                    }
                    if frame.processes > 0 {
                        frame.processes -= 1;
                        let process = frame
                            .job
                            .state
                            .lock()
                            .processes
                            .get(frame.processes)
                            .cloned();
                        if let Some(process) = process {
                            on_process(&process);
                        }
                        continue;
                    }
                    if frame.children > 0 {
                        frame.children -= 1;
                        let child = frame
                            .job
                            .state
                            .lock()
                            .children
                            .get(frame.children)
                            .and_then(Weak::upgrade);
                        if let Some(child) = child {
                            path[depth] = Some(Frame {
                                job: child,
                                entered: false,
                                processes: 0,
                                children: 0,
                            });
                            depth += 1;
                        }
                        continue;
                    }
                    depth -= 1;
                    path[depth] = None;
                }
            }
            if depth > 0 {
                // No lock is held: a pending interrupt runs, and the slice
                // may end. The walk goes on to its end whatever the caller
                // was asked, as a teardown does.
                steps::pause();
            }
        }
    }

    /// ADR-0004 rule 3: all or later. Delivers to everything it can, and
    /// reports a full queue afterwards (ADR-0046).
    pub fn signal(self: &Arc<Self>, kind: RequestKind, detail: u32) -> Result<(), Status> {
        let mut result = Ok(());
        Job::walk(self, &mut |_| {}, &mut |process| {
            if let Err(e) = process.signal(kind, detail) {
                result = Err(e);
            }
        });
        result
    }

    /// Stops every process in the tree and asserts `SUSPENDED` on every job in
    /// it (ADR-0105 item 8). On one CPU, with the kernel running, no other
    /// thread is at EL0: each one parks on its next way there, so none runs a
    /// user instruction after this returns (INV-0105-5). A process that joins
    /// a job already visited starts stopped (`join`); a job made under one
    /// starts suspended.
    pub fn suspend(self: &Arc<Self>) {
        Job::walk(
            self,
            &mut |job| {
                job.state.lock().suspended = true;
                port::set_signals(job.source, job_signals::SUSPENDED);
            },
            &mut |process| process.stop(),
        );
    }

    /// Undoes [`Job::suspend`] for the tree. A job that is not suspended is
    /// left as it is.
    pub fn resume(self: &Arc<Self>) {
        Job::walk(
            self,
            &mut |job| {
                job.state.lock().suspended = false;
                port::clear_signals(job.source, job_signals::SUSPENDED);
            },
            &mut |process| process.cont(),
        );
    }

    /// Ends the tree and closes it to new processes (ADR-0046).
    pub fn kill(self: &Arc<Self>) {
        Job::walk(
            self,
            &mut |job| job.state.lock().closed = true,
            &mut |process| {
                process.request_exit(ExitRecord {
                    code: 0,
                    reason: ExitReason::Killed,
                    detail: 0,
                    request_kind: None,
                });
            },
        );
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        let record = self.record.load(Ordering::Relaxed);
        if record != 0 {
            charge::refund_memory(self, record, Kind::Kernel);
        }
        if let Some(parent) = &self.parent {
            // Not while a walk runs: it counts on the slots staying (`walk`).
            if WALKS.load(Ordering::SeqCst) == 0 {
                let this: *const Job = self;
                parent
                    .state
                    .lock()
                    .children
                    .retain(|c| !core::ptr::eq(c.as_ptr(), this));
            }
        }
        port::free_source(self.source);
    }
}

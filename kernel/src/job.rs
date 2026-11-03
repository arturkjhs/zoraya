//! Jobs (ADR-0003, ADR-0004, ADR-0046). Every process is in one, and a job
//! tree is how a set of processes is signalled or killed as one.
//!
//! [`TREE`] covers adding to a job, leaving one, and walking a tree, so a
//! process being created is either in the walk or created after it.

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use zoraya_abi::{ExitReason, ExitRecord, RequestKind, Status, job_signals};
use zoraya_events::SourceId;

use crate::memory::heap::try_arc;
use crate::port;
use crate::process::Process;
use crate::sync::SpinLock;

/// Deepest job tree (ADR-0046).
const MAX_DEPTH: u32 = 32;

static TREE: SpinLock<()> = SpinLock::new(());
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
        let job = try_arc(Job {
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
        });
        // On failure the value was dropped, and its `Drop` freed the source.
        job.map_err(|_| Status::NoMemory)
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
        let job = Job::new(Some(parent.clone()), posix || parent.posix)?;
        let _tree = TREE.lock();
        let mut state = parent.state.lock();
        if state.closed {
            return Err(Status::BadState);
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

    /// Calls `f` for every job in the tree, holding [`TREE`] throughout, which
    /// stops the lists changing, so an index is a stable way through them.
    fn walk(&self, f: &mut impl FnMut(&Job)) {
        f(self);
        let mut i = 0;
        loop {
            let child = self.state.lock().children.get(i).and_then(Weak::upgrade);
            match child {
                Some(child) => child.walk(f),
                None if i >= self.state.lock().children.len() => break,
                None => {}
            }
            i += 1;
        }
    }

    fn for_each_process(&self, f: &mut impl FnMut(&Arc<Process>)) {
        let mut i = 0;
        loop {
            let process = self.state.lock().processes.get(i).cloned();
            match process {
                Some(process) => f(&process),
                None => break,
            }
            i += 1;
        }
    }

    /// ADR-0004 rule 3: all or later. Delivers to everything it can, and
    /// reports a full queue afterwards (ADR-0046).
    pub fn signal(&self, kind: RequestKind, detail: u32) -> Result<(), Status> {
        let _tree = TREE.lock();
        let mut result = Ok(());
        self.walk(&mut |job| {
            job.for_each_process(&mut |process| {
                if let Err(e) = process.signal(kind, detail) {
                    result = Err(e);
                }
            });
        });
        result
    }

    /// Stops every process in the tree and asserts `SUSPENDED` on every job in
    /// it (ADR-0105 item 8). On one CPU, with the kernel running, no other
    /// thread is at EL0: each one parks on its next way there, so none runs a
    /// user instruction after this returns (INV-0105-5).
    pub fn suspend(&self) {
        let _tree = TREE.lock();
        self.walk(&mut |job| {
            job.state.lock().suspended = true;
            port::set_signals(job.source, job_signals::SUSPENDED);
            job.for_each_process(&mut |process| process.stop());
        });
    }

    /// Undoes [`Job::suspend`] for the tree. A job that is not suspended is
    /// left as it is.
    pub fn resume(&self) {
        let _tree = TREE.lock();
        self.walk(&mut |job| {
            job.state.lock().suspended = false;
            port::clear_signals(job.source, job_signals::SUSPENDED);
            job.for_each_process(&mut |process| process.cont());
        });
    }

    /// Ends the tree and closes it to new processes (ADR-0046).
    pub fn kill(&self) {
        let _tree = TREE.lock();
        self.walk(&mut |job| {
            job.state.lock().closed = true;
            job.for_each_process(&mut |process| {
                process.request_exit(ExitRecord {
                    code: 0,
                    reason: ExitReason::Killed,
                    detail: 0,
                    request_kind: None,
                });
            });
        });
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(parent) = &self.parent {
            let this: *const Job = self;
            parent
                .state
                .lock()
                .children
                .retain(|c| !core::ptr::eq(c.as_ptr(), this));
        }
        port::free_source(self.source);
    }
}

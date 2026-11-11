//! The CPU ceiling of ADR-0264: the kernel's side of `zoraya-cpu`.
//!
//! The logic (budgets, the parking queue) is in the crate. This file gives it
//! the job tree and the period. A job's [`CpuAcct`] sits under its own lock,
//! a leaf: [`sched`](crate::sched) takes it with its own lock held (lock order
//! `SCHED`, then a job's `cpu`, nothing taken under it), and every other user
//! takes it alone.

use core::marker::PhantomData;
use core::ptr::NonNull;

use zoraya_abi::Status;
use zoraya_abi::quota::CpuUsage;
use zoraya_cpu::{CpuAcct, LimitError, Tree};

use crate::job::Job;

/// The period, nanoseconds. The test kernel can set another (the
/// measurement of ADR-0264's open question); the harness restores it.
#[cfg(feature = "qemu-test")]
static TEST_PERIOD_NS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn period_ns() -> u64 {
    #[cfg(feature = "qemu-test")]
    {
        let ns = TEST_PERIOD_NS.load(core::sync::atomic::Ordering::Relaxed);
        if ns != 0 {
            return ns;
        }
    }
    zoraya_cpu::PERIOD_NS
}

/// Sets the period from now on; 0 restores [`zoraya_cpu::PERIOD_NS`]. Budgets
/// are read afresh at every decision, so a change takes effect at once.
#[cfg(feature = "qemu-test")]
pub fn set_test_period(ns: u64) {
    TEST_PERIOD_NS.store(ns, core::sync::atomic::Ordering::Relaxed);
}

/// A job named for the walk. A raw pointer, because the scheduler names the
/// job of a thread it holds by value and the walk needs a type that is `Copy`
/// without a lifetime to tie to that borrow.
#[derive(Clone, Copy)]
pub struct JobRef(NonNull<Job>);

impl JobRef {
    /// Names `job`. The reference is valid while `job` is: the caller (the
    /// scheduler) keeps the thread, and so its process and the job tree above
    /// it, alive for the whole walk.
    pub fn of(job: &Job) -> JobRef {
        JobRef(NonNull::from(job))
    }

    fn job(&self) -> &Job {
        // SAFETY: made from a live `&Job` by `of`, whose caller keeps the job
        // alive for the walk (see `of`): every job holds an `Arc` of its
        // parent, so the parents that `parent` returns are alive too.
        unsafe { self.0.as_ref() }
    }
}

/// The job tree as `zoraya-cpu` walks it.
pub struct JobTree<'a>(pub PhantomData<&'a ()>);

impl Tree for JobTree<'_> {
    type Id = JobRef;

    fn parent(&self, id: JobRef) -> Option<JobRef> {
        id.job().parent_ref().map(JobRef::of)
    }

    fn get(&self, id: JobRef) -> CpuAcct {
        *id.job().cpu().lock()
    }

    fn set(&mut self, id: JobRef, acct: CpuAcct) {
        *id.job().cpu().lock() = acct;
    }
}

fn refuse(e: LimitError) -> Status {
    match e {
        LimitError::OutOfRange | LimitError::AboveParent => Status::InvalidArgument,
    }
}

/// Sets `job`'s own ceiling; `None` removes it. `job_set_limits` sets this
/// first and puts the old value back with [`restore_limit`] if the memory or
/// process limit is then refused (INV-0261-3).
pub fn set_limit(job: &Job, limit: Option<u32>) -> Result<(), Status> {
    zoraya_cpu::set_limit(&mut JobTree(PhantomData), JobRef::of(job), limit).map_err(refuse)
}

/// Puts back a ceiling [`set_limit`] replaced, whatever the rules say now.
pub fn restore_limit(job: &Job, limit: Option<u32>) {
    job.cpu().lock().limit = limit;
}

/// The CPU part of `job_get_usage`.
pub fn usage(job: &Job) -> CpuUsage {
    let a = *job.cpu().lock();
    CpuUsage {
        limit: a.limit,
        used_ns: a.used_ns,
        period_used: a.used_in(zoraya_cpu::period_of(
            crate::timer::now().as_nanos(),
            period_ns(),
        )),
        parked: a.parked,
    }
}

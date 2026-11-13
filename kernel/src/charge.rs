//! Charging jobs for what they use (ADR-0261): the kernel's side of
//! `zoraya-charge`.
//!
//! A [`Paid`] is what an object keeps of its payer's charges: it holds the
//! paying job and gives every byte (and process) back when it drops, so an
//! object's charge lasts exactly as long as the object and the job's record
//! lasts at least as long (ADR-0261 point 9). The charging forms of
//! allocation, [`arc`] and [`boxed`], charge before they allocate and undo the
//! charge if the heap then fails, so a refused call changes nothing
//! (INV-0261-2). Only boot code and objects without a paying job (devices,
//! kernel threads, the root job) use the `uncharged_*` forms of `heap`
//! (INV-0261-5, `make check-charged-alloc`).
//!
//! One lock, [`WALK`], serialises every walk so that the all-or-nothing check
//! sees counters nobody else changes. It is a leaf: nothing is called with it
//! held but the job counters' own locks. A refusal asserts `OVER_QUOTA` after
//! the lock is released.

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::quota::QuotaField;
use zoraya_abi::{Status, job_signals};
use zoraya_charge::{self as walk, Counters, Field, Kind, Refusal, Tree};

use crate::job::Job;
use crate::memory::heap::{block_size, uncharged_arc, uncharged_box};
use crate::port;
use crate::sync::SpinLock;

static WALK: SpinLock<()> = SpinLock::new(());

/// The field of the last `OverQuota` this CPU returned: what the system call
/// return puts in the value register (ADR-0263 point 1). One CPU, and read
/// before the thread can be preempted into another call.
static LAST_REFUSED: AtomicU64 = AtomicU64::new(0);

/// The value register for a failure with `status`.
pub fn failure_value(status: Status) -> u64 {
    if status == Status::OverQuota {
        LAST_REFUSED.load(Ordering::Relaxed)
    } else {
        0
    }
}

struct JobTree<'a>(PhantomData<&'a ()>);

impl<'a> Tree for JobTree<'a> {
    type Id = &'a Job;

    fn parent(&self, id: &'a Job) -> Option<&'a Job> {
        id.parent_ref()
    }

    fn counters(&self, id: &'a Job) -> Counters {
        *id.acct().lock()
    }

    fn set_counters(&mut self, id: &'a Job, counters: Counters) {
        *id.acct().lock() = counters;
    }
}

fn refused(r: Refusal<&Job>) -> Status {
    let field = match r.field {
        Field::Processes => QuotaField::Processes,
        Field::Threads => QuotaField::Threads,
        _ => QuotaField::Memory,
    };
    LAST_REFUSED.store(field as u64, Ordering::Relaxed);
    port::set_signals(r.at.source(), job_signals::OVER_QUOTA);
    Status::OverQuota
}

/// Charges `bytes` of `kind` to `job` and its ancestors, or refuses.
pub fn charge_memory(job: &Job, bytes: u64, kind: Kind) -> Result<(), Status> {
    let result = {
        let _walk = WALK.lock();
        walk::charge_memory(&mut JobTree(PhantomData), job, bytes, kind)
    };
    result.map_err(refused)
}

/// Gives back what [`charge_memory`] took.
pub fn refund_memory(job: &Job, bytes: u64, kind: Kind) {
    let _walk = WALK.lock();
    walk::refund_memory(&mut JobTree(PhantomData), job, bytes, kind);
}

/// Charges one process to `job` and its ancestors, or refuses.
pub fn charge_process(job: &Job) -> Result<(), Status> {
    let result = {
        let _walk = WALK.lock();
        walk::charge(&mut JobTree(PhantomData), job, Field::Processes, 1)
    };
    result.map_err(refused)
}

pub fn refund_process(job: &Job) {
    let _walk = WALK.lock();
    walk::refund(&mut JobTree(PhantomData), job, Field::Processes, 1);
}

/// Charges one live thread to `job` and its ancestors, or refuses.
pub fn charge_thread(job: &Job) -> Result<(), Status> {
    let result = {
        let _walk = WALK.lock();
        walk::charge(&mut JobTree(PhantomData), job, Field::Threads, 1)
    };
    result.map_err(refused)
}

pub fn refund_thread(job: &Job) {
    let _walk = WALK.lock();
    walk::refund(&mut JobTree(PhantomData), job, Field::Threads, 1);
}

/// The counters of `job` for `job_get_usage`, with the limit each has
/// (its own, not the effective one), and the signal cleared if asked.
pub fn counters(job: &Job) -> Counters {
    let _walk = WALK.lock();
    *job.acct().lock()
}

/// Sets `memory`, `processes` and `threads` limits together, or none
/// (INV-0261-3, ADR-0307 item 2).
pub fn set_limits(
    job: &Job,
    memory: Option<u64>,
    processes: Option<u32>,
    threads: Option<u32>,
) -> Result<(), Status> {
    let _walk = WALK.lock();
    let mut tree = JobTree(PhantomData);
    let before = tree.counters(job);
    let result = walk::set_limit(&mut tree, job, Field::Memory, memory)
        .and_then(|()| walk::set_limit(&mut tree, job, Field::Processes, processes.map(u64::from)))
        .and_then(|()| walk::set_limit(&mut tree, job, Field::Threads, threads.map(u64::from)));
    match result {
        Ok(()) => Ok(()),
        Err(_) => {
            tree.set_counters(job, before);
            Err(Status::InvalidArgument)
        }
    }
}

/// What an object keeps of its payer's charges. Dropping it gives them all
/// back. A `Paid` with no job charges nothing: boot objects, devices and
/// kernel threads (ADR-0261 point 6).
pub struct Paid {
    job: Option<Arc<Job>>,
    user: u64,
    kernel: u64,
    process: bool,
    thread: bool,
}

impl Paid {
    pub fn new(job: Option<&Arc<Job>>) -> Paid {
        Paid {
            job: job.cloned(),
            user: 0,
            kernel: 0,
            process: false,
            thread: false,
        }
    }

    /// Charges `bytes` of kernel objects.
    pub fn kernel(&mut self, bytes: u64) -> Result<(), Status> {
        if let Some(job) = &self.job {
            charge_memory(job, bytes, Kind::Kernel)?;
        }
        self.kernel += bytes;
        Ok(())
    }

    /// Charges `bytes` of user frames.
    pub fn user(&mut self, bytes: u64) -> Result<(), Status> {
        if let Some(job) = &self.job {
            charge_memory(job, bytes, Kind::User)?;
        }
        self.user += bytes;
        Ok(())
    }

    /// Charges one live process.
    pub fn process(&mut self) -> Result<(), Status> {
        if let Some(job) = &self.job
            && !self.process
        {
            charge_process(job)?;
        }
        self.process = true;
        Ok(())
    }

    /// Charges one live thread (ADR-0307 item 1). Taken after the process's
    /// slot and before its memory, so the field that refuses is the first
    /// that would be passed.
    pub fn thread(&mut self) -> Result<(), Status> {
        if let Some(job) = &self.job
            && !self.thread
        {
            charge_thread(job)?;
        }
        self.thread = true;
        Ok(())
    }

    /// The thread ended: its place in the count goes, its memory stays until
    /// the record is dropped (`thread_exit`, ADR-0307 item 7).
    pub fn release_thread(&mut self) {
        if self.thread {
            if let Some(job) = &self.job {
                refund_thread(job);
            }
            self.thread = false;
        }
    }

    /// The process ended: its slot in the count goes, its memory stays until
    /// the object is dropped.
    pub fn release_process(&mut self) {
        if self.process {
            if let Some(job) = &self.job {
                refund_process(job);
            }
            self.process = false;
        }
    }

    /// A thread ended: the `threads` figure and the kernel memory it holds go
    /// back at once (ADR-0307 item 7). The record and the stack are freed
    /// a little later, by the reaper.
    pub fn release_exited(&mut self) {
        self.release_thread();
        let held = self.kernel;
        self.release_kernel(held);
    }

    /// Gives back `bytes` of kernel memory taken with [`Paid::kernel`].
    pub fn release_kernel(&mut self, bytes: u64) {
        let bytes = bytes.min(self.kernel);
        if let Some(job) = &self.job {
            refund_memory(job, bytes, Kind::Kernel);
        }
        self.kernel -= bytes;
    }

    /// Gives back `bytes` of user memory taken with [`Paid::user`].
    pub fn release_user(&mut self, bytes: u64) {
        let bytes = bytes.min(self.user);
        if let Some(job) = &self.job {
            refund_memory(job, bytes, Kind::User);
        }
        self.user -= bytes;
    }
}

impl Default for Paid {
    fn default() -> Paid {
        Paid::new(None)
    }
}

impl Drop for Paid {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            if self.kernel != 0 {
                refund_memory(job, self.kernel, Kind::Kernel);
            }
            if self.user != 0 {
                refund_memory(job, self.user, Kind::User);
            }
            if self.process {
                refund_process(job);
            }
            if self.thread {
                refund_thread(job);
            }
        }
    }
}

/// What a subscription costs its holder: its record, its place in the port's
/// and the source's lists, and a slot in the port's ready queue.
pub const SUBSCRIPTION_COST: u64 = 128;

/// What a source in the event tables costs: its record and its place in the
/// list, rounded up (`zoraya-events`' `Source`, a `Vec<u32>` and the slot).
pub const SOURCE_COST: u64 = 64;

/// The bytes `Arc::new(T)` takes from the heap.
pub fn arc_cost<T>() -> u64 {
    let layout = Layout::new::<[usize; 2]>()
        .extend(Layout::new::<T>())
        .map(|(layout, _)| layout.pad_to_align());
    layout.map_or(u64::MAX, |l| block_size(l) as u64)
}

/// The bytes `Box::new(T)` takes (nothing for a zero-sized `T`).
pub fn box_cost<T>() -> u64 {
    let layout = Layout::new::<T>();
    if layout.size() == 0 {
        0
    } else {
        block_size(layout) as u64
    }
}

/// The bytes a `Vec<T>` of capacity `len` takes.
pub fn vec_cost<T>(len: usize) -> u64 {
    match Layout::array::<T>(len) {
        Ok(layout) if layout.size() != 0 => block_size(layout) as u64,
        Ok(_) => 0,
        Err(_) => u64::MAX,
    }
}

/// `Arc::new(build(paid))`, charged to `paid`'s job first together with
/// `extra` bytes the object owns beside itself. On any failure nothing is
/// left charged: a refusal is `OverQuota`, a failed allocation `NoMemory`.
pub fn arc<T>(mut paid: Paid, extra: u64, build: impl FnOnce(Paid) -> T) -> Result<Arc<T>, Status> {
    paid.kernel(arc_cost::<T>().saturating_add(extra))?;
    uncharged_arc(build(paid)).map_err(|_| Status::NoMemory)
}

/// `Arc::new(value)` for a value that already holds a [`Paid`] charged for the
/// whole of it (its [`arc_cost`] included). A failed allocation drops the
/// value, which gives the charge back.
pub fn prepaid_arc<T>(value: T) -> Result<Arc<T>, Status> {
    uncharged_arc(value).map_err(|_| Status::NoMemory)
}

/// As [`prepaid_arc`], for a `Box` and its [`box_cost`].
pub fn prepaid_box<T>(value: T) -> Result<Box<T>, Status> {
    uncharged_box(value).map_err(|_| Status::NoMemory)
}

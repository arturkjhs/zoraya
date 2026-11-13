//! Processes (ADR-0003, ADR-0004, ADR-0031, ADR-0033, ADR-0034, ADR-0041,
//! ADR-0300). A process has from one to `MAX_THREADS` threads.
//!
//! A process ends in two phases (ADR-0300 item 6). Whoever ends it fixes the
//! exit record once and wakes the first of its threads; each thread acts on
//! the record at its next return to EL0 or as it starts, wakes its siblings
//! (the first to leave does, in steps), and leaves. The thread whose leave
//! empties the list is the owner and tears the process down, so it runs no
//! user instruction after the request, and nothing is torn down under a
//! thread that is still running or blocked in the kernel.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use zoraya_abi::spawn::{
    STARTUP_BLOCK, STARTUP_BLOCK_MAX, STARTUP_HEADER_SIZE, Strings, USER_STACK_SIZE,
    USER_STACK_TOP, startup_header, startup_table_offset,
};
use zoraya_abi::{
    CONSOLE, ConsoleObject, DIRECTORY, DeviceObject, DirectoryObject, Disposition, ExitReason,
    ExitRecord, FIRST_DEVICE_SLOT, Handle, JOB, JobObject, ObjectType, REQUEST_KINDS, RTC_SLOT,
    RequestKind, Rights, SEED_SLOT, Status, process_signals,
};
use zoraya_abi::{POWER, PowerObject};
use zoraya_elf::{Error as ElfError, PAGE_SIZE, PHDR_SIZE, Program, Segment};
use zoraya_events::{RequestQueue, SourceId};
use zoraya_pagetable::{Attributes, MemoryType};
use zoraya_thread::Members;

use crate::arch::{Frame, mmu};
use crate::charge::{self, Paid};
use crate::job::Job;
use crate::memory::heap::OutOfMemory;
use crate::memory::object::MemoryObject;
use crate::memory::user::{AddressSpace, MapFailed, USER_HIGH, USER_LOW};
use crate::object::{self, HandleTable, MemoryImage, Object};
use crate::sched::{self, Thread};
use crate::sync::SpinLock;
use crate::thread::Multi;
use crate::{arch, port, println, ramdisk, steps, thread};

const _: () = assert!(USER_STACK_TOP == USER_HIGH - PAGE_SIZE);

/// Why a process could not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    NotFound,
    BadProgram(ElfError),
    NoMemory,
    /// A limit of the job (or one above it) refused the process.
    OverQuota,
    /// The job was killed.
    JobClosed,
    /// The process was asked to end while the call was in steps
    /// (ADR-0270); nothing was made.
    Cancelled,
    /// The vectors and program headers do not fit the startup block.
    TooLarge,
}

impl From<OutOfMemory> for SpawnError {
    fn from(_: OutOfMemory) -> SpawnError {
        SpawnError::NoMemory
    }
}

impl From<MapFailed> for SpawnError {
    fn from(e: MapFailed) -> SpawnError {
        match e {
            MapFailed::Quota => SpawnError::OverQuota,
            _ => SpawnError::NoMemory,
        }
    }
}

impl From<Status> for SpawnError {
    fn from(e: Status) -> SpawnError {
        match e {
            Status::OverQuota => SpawnError::OverQuota,
            Status::Cancelled => SpawnError::Cancelled,
            _ => SpawnError::NoMemory,
        }
    }
}

impl From<SpawnError> for Status {
    fn from(e: SpawnError) -> Status {
        match e {
            SpawnError::NotFound => Status::NotFound,
            SpawnError::BadProgram(_) => Status::InvalidArgument,
            SpawnError::NoMemory => Status::NoMemory,
            SpawnError::OverQuota => Status::OverQuota,
            SpawnError::JobClosed => Status::BadState,
            SpawnError::Cancelled => Status::Cancelled,
            SpawnError::TooLarge => Status::LimitReached,
        }
    }
}

struct Exit {
    record: Option<ExitRecord>,
    /// The kernel threads blocked in [`Process::wait`].
    waiters: Vec<Arc<Thread>>,
}

struct Requests {
    dispositions: [Disposition; REQUEST_KINDS],
    /// What `Deliver` queued, in fixed memory, so delivery cannot fail for
    /// lack of it.
    queue: RequestQueue,
    /// How this process is to end, once it next can.
    exit: Option<ExitRecord>,
}

fn terminated(kind: RequestKind, detail: u32) -> ExitRecord {
    ExitRecord {
        code: 0,
        reason: ExitReason::Terminated,
        detail,
        request_kind: Some(kind),
    }
}

pub struct Process {
    name: String,
    ttbr0: u64,
    /// Released at exit, whoever still holds the process.
    pub space: SpinLock<Option<AddressSpace>>,
    exit: SpinLock<Exit>,
    job: Arc<Job>,
    /// The process's slot in its job's count and its record, thread-less:
    /// the slot goes at exit, the rest when the last holder lets go
    /// (ADR-0261 point 5).
    paid: SpinLock<Paid>,
    source: SourceId,
    pub handles: SpinLock<HandleTable>,
    /// Its threads, until exit: how a request reaches a process that is not
    /// running (ADR-0300 item 1).
    members: SpinLock<Members<Arc<Thread>>>,
    /// The futex and mapping-lock lists, made at the first second thread or
    /// futex wait (`thread::ensure_multi`), and what they were charged.
    multi: SpinLock<Option<Arc<Multi>>>,
    multi_cost: AtomicU64,
    requests: SpinLock<Requests>,
    suspended: AtomicBool,
    /// Its job tree is suspended (ADR-0105 item 8): it runs no user
    /// instruction until `job_resume`.
    stopped: AtomicBool,
    /// A fork is copying its space (ADR-0306 item 2): its threads but the
    /// caller park at their next return to EL0, like `stopped`.
    frozen: AtomicBool,
    /// One thread waiting in [`park_if_stopped`], the one `cont` wakes; it
    /// wakes the other parked threads itself, in steps (ADR-0300 item 7).
    park_head: SpinLock<Option<Arc<Thread>>>,
    /// The first thread to leave an ending process woke the others.
    swept: AtomicBool,
    /// The mapping lock is held (ADR-0301 item 6).
    map_locked: AtomicBool,
    /// A user address to read as the process ends, and what was there.
    #[cfg(feature = "qemu-test")]
    watch: SpinLock<(u64, Option<u64>)>,
}

impl Process {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The value for `TTBR0_EL1`.
    pub fn ttbr0(&self) -> u64 {
        self.ttbr0
    }

    pub fn source(&self) -> SourceId {
        self.source
    }

    pub fn job(&self) -> &Arc<Job> {
        &self.job
    }

    /// Its job tree was suspended: from now on it does not return to EL0.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    /// Its job tree was resumed: a thread waiting to return to EL0 goes on.
    /// One parked thread is woken, as one is for an end (ADR-0300 item 7), so
    /// the cost under the job tree's lock is one wake whatever the number of
    /// threads; it wakes the others from its own context.
    pub fn cont(&self) {
        self.stopped.store(false, Ordering::Relaxed);
        self.wake_park_head();
    }

    fn wake_park_head(&self) {
        let head = self.park_head.lock().take();
        if let Some(head) = head {
            sched::wake(head);
        }
    }

    pub(crate) fn members(&self) -> &SpinLock<Members<Arc<Thread>>> {
        &self.members
    }

    pub(crate) fn multi(&self) -> &SpinLock<Option<Arc<Multi>>> {
        &self.multi
    }

    pub(crate) fn multi_cost(&self) -> &AtomicU64 {
        &self.multi_cost
    }

    pub(crate) fn paid(&self) -> &SpinLock<Paid> {
        &self.paid
    }

    pub(crate) fn map_locked(&self) -> &AtomicBool {
        &self.map_locked
    }

    /// A fork starts or ends its freeze (`thread::Freeze`).
    pub(crate) fn set_frozen(&self, on: bool) {
        self.frozen.store(on, Ordering::Relaxed);
    }

    fn must_park(&self) -> bool {
        self.stopped.load(Ordering::Relaxed) || self.frozen.load(Ordering::Relaxed)
    }

    /// Blocks a kernel thread until the process has ended. Any number may wait.
    pub fn wait(&self) -> ExitRecord {
        let daif = arch::save_and_mask_interrupts();
        loop {
            // A block of its own, so that the guard restores its mask before
            // `daif` is restored.
            let (record, listed) = {
                let mut exit = self.exit.lock();
                let record = exit.record;
                let listed = record.is_none() && exit.waiters.try_reserve(1).is_ok();
                if listed {
                    exit.waiters.push(sched::current());
                }
                (record, listed)
            };
            match (record, listed) {
                (Some(record), _) => {
                    arch::restore_interrupts(daif);
                    return record;
                }
                (None, true) => sched::block_current(),
                // No room to be listed: this needs no memory.
                (None, false) => sched::yield_now(),
            }
        }
    }

    /// The record, once the process has ended.
    pub fn exit_record(&self) -> Option<ExitRecord> {
        self.exit.lock().record
    }

    /// Calls `f` for every mapping, while the process lives.
    #[cfg(feature = "qemu-test")]
    pub fn for_each_mapping(&self, f: &mut impl FnMut(zoraya_pagetable::Mapping)) {
        if let Some(space) = self.space.lock().as_ref() {
            space.for_each_mapping(f);
        }
    }

    /// The value at user address `va`, while the process lives.
    #[cfg(feature = "qemu-test")]
    pub fn peek(&self, va: u64) -> Option<u64> {
        self.space.lock().as_ref().and_then(|s| s.peek(va))
    }

    /// Asks that `va` be read as the process ends, for [`Process::watched`].
    #[cfg(feature = "qemu-test")]
    pub fn watch(&self, va: u64) {
        *self.watch.lock() = (va, None);
    }

    #[cfg(feature = "qemu-test")]
    pub fn watch_address(&self) -> Option<u64> {
        let va = self.watch.lock().0;
        (va != 0).then_some(va)
    }

    #[cfg(feature = "qemu-test")]
    pub fn watched(&self) -> Option<u64> {
        self.watch.lock().1
    }

    /// Requests queued, of any kind.
    #[cfg(feature = "qemu-test")]
    pub fn requests_queued(&self) -> usize {
        self.requests.lock().queue.len()
    }

    #[cfg(feature = "qemu-test")]
    pub fn thread(&self) -> Option<Arc<Thread>> {
        self.members.lock().first().cloned()
    }

    /// Whether this process must end, at its next chance.
    pub fn exit_requested(&self) -> bool {
        self.requests.lock().exit.is_some()
    }

    /// Asks the process to end with `record`, unless it was already asked: the
    /// first phase (ADR-0300 item 6). One thread is woken, the first of the
    /// list, never all: the walks of `Job::kill` call this under the job tree's
    /// spinlock, and a process can have 128 threads. A thread that is running
    /// or ready sees the record at its next return to EL0; the one woken wakes
    /// the rest from its own context, in steps, before it leaves.
    pub fn request_exit(&self, record: ExitRecord) {
        self.requests.lock().exit.get_or_insert(record);
        let first = {
            let mut members = self.members.lock();
            members.begin_end();
            members.first().cloned()
        };
        if let Some(thread) = first {
            sched::wake(thread);
        }
    }

    /// ADR-0004 rule 2.
    pub fn signal(&self, kind: RequestKind, detail: u32) -> Result<(), Status> {
        if self.exit_record().is_some() {
            return Ok(());
        }
        let mut requests = self.requests.lock();
        match requests.dispositions[kind.raw() as usize] {
            Disposition::Ignore => Ok(()),
            Disposition::Terminate => {
                drop(requests);
                self.request_exit(terminated(kind, detail));
                Ok(())
            }
            Disposition::Deliver => {
                requests
                    .queue
                    .push(kind, detail)
                    .map_err(|_| Status::LimitReached)?;
                drop(requests);
                port::set_signals(self.source, process_signals::REQUEST_PENDING);
                Ok(())
            }
        }
    }

    /// Changes a disposition. Changing to `Terminate` with a request queued
    /// ends the process at once (ADR-0004 rule 2).
    pub fn set_disposition(&self, kind: RequestKind, disposition: Disposition) {
        let mut requests = self.requests.lock();
        requests.dispositions[kind.raw() as usize] = disposition;
        let queued = requests.queue.queued(kind);
        drop(requests);
        if disposition == Disposition::Terminate
            && let Some(detail) = queued
        {
            self.request_exit(terminated(kind, detail));
        }
    }

    pub fn take_request(&self) -> Option<(RequestKind, u32)> {
        let mut requests = self.requests.lock();
        let request = requests.queue.pop();
        let empty = requests.queue.is_empty();
        drop(requests);
        if empty {
            port::clear_signals(self.source, process_signals::REQUEST_PENDING);
        }
        request
    }

    /// Starts a process made suspended. `BadState` if it is not suspended.
    pub fn resume(&self) -> Result<(), Status> {
        if !self.suspended.swap(false, Ordering::Relaxed) {
            return Err(Status::BadState);
        }
        port::clear_signals(self.source, process_signals::SUSPENDED);
        let thread = self.members.lock().first().cloned();
        if let Some(thread) = thread {
            sched::wake(thread);
        }
        Ok(())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        port::free_source(self.source);
    }
}

fn attributes(segment: &Segment) -> Attributes {
    Attributes {
        memory: MemoryType::Normal,
        write: segment.write,
        execute: segment.execute,
        user: true,
    }
}

const STARTUP: Attributes = Attributes {
    memory: MemoryType::Normal,
    write: false,
    execute: false,
    user: true,
};

const USER_STACK: Attributes = Attributes {
    memory: MemoryType::Normal,
    write: true,
    execute: false,
    user: true,
};

/// Where a program's bytes are.
#[derive(Clone, Copy)]
pub enum Source<'a> {
    /// A file in one slice: the ramdisk.
    Bytes(&'a [u8]),
    /// A memory object a process filled (ADR-0108).
    Memory(&'a MemoryObject),
}

impl Source<'_> {
    /// Copies the file's bytes from `offset` into `out`, if they are there.
    fn copy(&self, offset: u64, out: &mut [u8]) -> bool {
        match self {
            Source::Bytes(file) => {
                let Some(bytes) = usize::try_from(offset)
                    .ok()
                    .and_then(|at| file.get(at..at.checked_add(out.len())?))
                else {
                    return false;
                };
                out.copy_from_slice(bytes);
                true
            }
            Source::Memory(object) => object.read(offset, out),
        }
    }
}

/// A checked program and where its bytes are.
#[derive(Clone, Copy)]
pub struct Loadable<'a> {
    pub program: Program,
    pub source: Source<'a>,
    /// The whole program header table, for the startup block (ADR-0109).
    pub table: &'a [u8],
}

impl<'a> Loadable<'a> {
    /// Parses a file in one slice.
    pub fn from_file(file: &'a [u8]) -> Result<Loadable<'a>, SpawnError> {
        let program = Program::parse(file, USER_LOW, STARTUP_BLOCK - PAGE_SIZE)
            .map_err(SpawnError::BadProgram)?;
        let table = program
            .header_table(file)
            .ok_or(SpawnError::BadProgram(ElfError::Truncated))?;
        Ok(Loadable {
            program,
            source: Source::Bytes(file),
            table,
        })
    }

    /// An image `image_create` checked.
    pub fn from_image(image: &'a MemoryImage) -> Loadable<'a> {
        Loadable {
            program: image.program,
            source: Source::Memory(&image.memory),
            table: &image.table,
        }
    }
}

/// The kernel reserve is 1/16 of the frames free after boot: ADR-0261 point 10's
/// `[assumption]`, to be measured by a later Q task, not tuned to pass a test.
/// 1/32 and 1/64 were tried and removed: the GUI session at scale 2 used about
/// 96% of 256 MiB, so a smaller reserve only hid that; the GUI guests now have
/// 512 MiB (backlog: Desktop memory footprint).
const KERNEL_RESERVE_DIVISOR: u64 = 16;

/// A process's place in its job's lists of members: one `Arc` each in the
/// job's `processes` vector and in the scheduler's queues, rounded up.
const LIST_ENTRIES: u64 = 64;

/// The range `image_create` checks programs against (ADR-0108 item 1).
pub const PROGRAM_LOW: u64 = USER_LOW;
pub const PROGRAM_HIGH: u64 = STARTUP_BLOCK - PAGE_SIZE;

/// Copies a segment into new pages.
fn load_segment(
    space: &mut AddressSpace,
    source: Source<'_>,
    s: &Segment,
    pages: &mut usize,
) -> Result<(), SpawnError> {
    let attributes = attributes(s);
    let (start, end) = s.pages();
    let file_end = s.vaddr + s.filesz;
    for page in (start..end).step_by(PAGE_SIZE as usize) {
        // The image is copied in steps of `STEP` pages (ADR-0270 point 2),
        // each with room for its frames in the space's list. `pages` counts
        // from the first page of the first segment, so that the last pages
        // of one segment and the first of the next are one step, not two.
        if pages.is_multiple_of(steps::STEP) {
            if *pages > 0 {
                steps::between()?;
            }
            space.prepare_step();
        }
        *pages += 1;
        let target = space.map_new_page(page, attributes)?;
        // The part of this page that has file bytes.
        let from = page.max(s.vaddr);
        let to = (page + PAGE_SIZE).min(file_end);
        if from < to {
            let offset = s.offset + (from - s.vaddr);
            // SAFETY: `target` is a whole new page, only reachable by this
            // function until the process runs, and `from - page` plus the
            // length stays inside it.
            let bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    target.add((from - page) as usize),
                    (to - from) as usize,
                )
            };
            // The parser checked the range against the file's length.
            if !source.copy(offset, bytes) {
                return Err(SpawnError::BadProgram(ElfError::BadSegment));
            }
        }
        if s.execute {
            // SAFETY: `target` is a whole page in the direct map.
            unsafe { mmu::sync_instruction_cache(target as u64, PAGE_SIZE) };
        }
    }
    Ok(())
}

/// Maps the block a process reads its vectors and program headers from
/// (ADR-0042, ADR-0109 item 5).
fn map_startup_block(
    space: &mut AddressSpace,
    argv: Strings<'_>,
    envp: Strings<'_>,
    table: &[u8],
) -> Result<(), SpawnError> {
    let strings = argv.as_bytes().len() + envp.as_bytes().len();
    let header = startup_header(argv.len(), envp.len(), table.len() / PHDR_SIZE);
    let padding = startup_table_offset(strings) - STARTUP_HEADER_SIZE - strings;
    let size = (startup_table_offset(strings) + table.len()) as u64;
    if size > STARTUP_BLOCK_MAX || !table.len().is_multiple_of(PHDR_SIZE) {
        return Err(SpawnError::TooLarge);
    }
    let parts = [
        &header[..],
        argv.as_bytes(),
        envp.as_bytes(),
        &[0; 8][..padding],
        table,
    ];
    let mut parts = parts.iter().flat_map(|p| p.iter().copied());
    for page in (STARTUP_BLOCK..STARTUP_BLOCK + size).step_by(PAGE_SIZE as usize) {
        let target = space.map_new_page(page, STARTUP)?;
        for (i, byte) in parts.by_ref().take(PAGE_SIZE as usize).enumerate() {
            // SAFETY: `target` is a whole new page, only reachable from here
            // until the process runs, and `i` is below its size.
            unsafe { target.add(i).write(byte) };
        }
    }
    Ok(())
}

/// What a new process is made from.
pub struct Start<'a> {
    pub name: &'a str,
    pub image: Loadable<'a>,
    pub job: Arc<Job>,
    pub handles: HandleTable,
    pub argv: Strings<'a>,
    pub envp: Strings<'a>,
    pub dispositions: [Disposition; REQUEST_KINDS],
    pub suspended: bool,
}

/// Makes a process. Nothing else has changed if this fails.
pub fn create(start: Start<'_>) -> Result<Arc<Process>, SpawnError> {
    let program = start.image.program;
    // The slot in the job's count first: a fork bomb stops here, before it
    // takes a page (ADR-0263, INV-0263-1).
    let mut count = Paid::new(Some(&start.job));
    count.process()?;
    // Then the first thread's figure, before the process takes a page
    // (ADR-0307 item 1): the field that refuses is the first passed in the
    // order processes, threads, memory.
    let first = sched::reserve_thread(&start.job)?;
    let mut space = AddressSpace::new(Some(&start.job))?;
    let mut pages = 0;
    for segment in program.segments() {
        load_segment(&mut space, start.image.source, segment, &mut pages)?;
    }
    let stack_low = USER_STACK_TOP - USER_STACK_SIZE;
    for page in (stack_low..USER_STACK_TOP).step_by(PAGE_SIZE as usize) {
        if pages.is_multiple_of(steps::STEP) {
            if pages > 0 {
                steps::between()?;
            }
            space.prepare_step();
        }
        pages += 1;
        space.map_new_page(page, USER_STACK)?;
    }
    // The block (at most `STARTUP_BLOCK_MAX`) is a step of its own, with
    // room for its pages and tables in the list, as for the stack.
    steps::between()?;
    space.prepare_step();
    map_startup_block(&mut space, start.argv, start.envp, start.image.table)?;
    let process = assemble(
        start.name,
        space,
        start.job,
        count,
        start.handles,
        start.dispositions,
        start.suspended,
    )?;
    let thread = sched::spawn_user(process.clone(), program.entry, USER_STACK_TOP, first)?;
    admit(process, thread, start.suspended)
}

/// What a process's handle table is charged at creation: its largest, 1024
/// slots. A zombie keeps its record but not the table, so `exit_current` gives
/// this back.
fn handle_table_cost() -> u64 {
    u64::from(zoraya_abi::MAX_HANDLES) * (core::mem::size_of::<object::Entry>() as u64 + 16)
}

/// The process object around a built address space and table.
fn assemble(
    name: &str,
    space: AddressSpace,
    job: Arc<Job>,
    count: Paid,
    handles: HandleTable,
    dispositions: [Disposition; REQUEST_KINDS],
    suspended: bool,
) -> Result<Arc<Process>, SpawnError> {
    let mut owned = String::new();
    owned.try_reserve(name.len()).map_err(|_| OutOfMemory)?;
    owned.push_str(name);
    // The table is charged at its largest, 1024 slots: handles are covered by
    // memory and processes (ADR-0260), not by a count of their own.
    let table = handle_table_cost();
    let extra = table + charge::vec_cost::<u8>(name.len()) + LIST_ENTRIES;
    let bits = if suspended {
        process_signals::SUSPENDED
    } else {
        0
    };
    // The charge first: a refused one allocates nothing, and the source below is
    // made only once the process is paid for (INV-0261-2, review Q1 error 1).
    let mut count = count;
    count.kernel(charge::arc_cost::<Process>().saturating_add(extra))?;
    let source = port::new_source(bits).map_err(|_| OutOfMemory)?;
    let members = Members::new_small().map_err(|_| OutOfMemory)?;
    let ttbr0 = space.ttbr0();
    let process = charge::prepaid_arc(Process {
        name: owned,
        ttbr0,
        space: SpinLock::new(Some(space)),
        exit: SpinLock::new(Exit {
            record: None,
            waiters: Vec::new(),
        }),
        job,
        paid: SpinLock::new(count),
        source,
        handles: SpinLock::new(handles),
        members: SpinLock::new(members),
        multi: SpinLock::new(None),
        multi_cost: AtomicU64::new(0),
        park_head: SpinLock::new(None),
        frozen: AtomicBool::new(false),
        swept: AtomicBool::new(false),
        map_locked: AtomicBool::new(false),
        requests: SpinLock::new(Requests {
            dispositions,
            queue: RequestQueue::new(),
            exit: None,
        }),
        suspended: AtomicBool::new(suspended),
        stopped: AtomicBool::new(false),
        #[cfg(feature = "qemu-test")]
        watch: SpinLock::new((0, None)),
    });
    // On failure the value was dropped, and its `Drop` freed the source and
    // gave the charge back.
    Ok(process?)
}

/// Makes `process` a member of its job with `thread`, and starts it unless
/// it is to wait for `process_resume`.
fn admit(
    process: Arc<Process>,
    thread: Arc<Thread>,
    suspended: bool,
) -> Result<Arc<Process>, SpawnError> {
    // Room for the first thread was made with the list; the join cannot fail.
    let _ = process.members.lock().join(thread.id(), thread.clone());
    if let Err(status) = process.job.admit(&process) {
        process.members.lock().leave(thread.id());
        sched::discard(thread);
        return Err(match status {
            Status::BadState => SpawnError::JobClosed,
            _ => SpawnError::NoMemory,
        });
    }
    if !suspended {
        sched::wake(thread);
    }
    Ok(process)
}

/// What a snapshot's child is made from (ADR-0102).
pub struct Snapshot<'a> {
    pub parent: &'a Process,
    pub space: AddressSpace,
    pub job: Arc<Job>,
    /// The child's slot in its job's count, taken before the copy.
    pub count: Paid,
    /// The child's first thread's figure in its job's count, taken with
    /// `count` (ADR-0307 item 1).
    pub first: Paid,
    pub handles: HandleTable,
    /// Where the child gets a handle to itself.
    pub self_handle: Option<Handle>,
    /// The caller's registers at its `svc`.
    pub frame: &'a Frame,
    pub tpidr_el0: u64,
    pub suspended: bool,
}

/// Makes a snapshot's child: the parent's name and dispositions, an empty
/// request queue, and a thread that returns from the same `svc` with `Ok`
/// and 0 (ADR-0102 item 5). Nothing else has changed if this fails.
pub fn create_snapshot(s: Snapshot<'_>) -> Result<Arc<Process>, SpawnError> {
    let dispositions = s.parent.requests.lock().dispositions;
    let process = assemble(
        &s.parent.name,
        s.space,
        s.job,
        s.count,
        s.handles,
        dispositions,
        s.suspended,
    )?;
    if let Some(handle) = s.self_handle {
        // The table was made with this slot empty at this generation.
        let entry = Object::Process(process.clone());
        let placed = process
            .handles
            .lock()
            .place(handle, entry, Rights::WAIT, object::next_id());
        if placed.is_err() {
            return Err(SpawnError::NoMemory);
        }
    }
    // From here a failure must break the cycle the self handle makes.
    let result = sched::spawn_user_from(process.clone(), s.frame, s.tpidr_el0, s.first)
        .map_err(SpawnError::from)
        .and_then(|thread| admit(process.clone(), thread, s.suspended));
    if result.is_err() {
        let table = core::mem::take(&mut *process.handles.lock());
        drop(table);
    }
    result
}

/// The handles of a process the kernel starts (ADR-0043).
fn kernel_handles(job: Arc<Job>) -> Result<HandleTable, SpawnError> {
    let mut handles = HandleTable::new();
    for (handle, object, rights) in [
        (CONSOLE, Object::Console, ConsoleObject::RIGHTS),
        (JOB, Object::Job(job), JobObject::RIGHTS),
        (DIRECTORY, Object::Directory, DirectoryObject::RIGHTS),
    ] {
        handles
            .insert_at(handle.slot(), object, rights, object::next_id())
            .map_err(|_| SpawnError::NoMemory)?;
    }
    Ok(handles)
}

/// The real-time clock at `RTC_SLOT` when the device tree has one (ADR-0226
/// item 1), then every virtio device from slot `FIRST_DEVICE_SLOT` on
/// (ADR-0053).
fn insert_devices(handles: &mut HandleTable) -> Result<(), SpawnError> {
    if let Some(clock) = crate::device::rtc_device() {
        handles
            .insert_at(
                RTC_SLOT,
                Object::Device(clock),
                DeviceObject::RIGHTS,
                object::next_id(),
            )
            .map_err(|_| SpawnError::NoMemory)?;
    }
    for (i, device) in crate::device::all().iter().enumerate() {
        handles
            .insert_at(
                FIRST_DEVICE_SLOT + i as u32,
                Object::Device(device.clone()),
                DeviceObject::RIGHTS,
                object::next_id(),
            )
            .map_err(|_| SpawnError::NoMemory)?;
    }
    Ok(())
}

/// Starts the ramdisk's `name` with the device handles, for tests of them.
#[cfg(feature = "qemu-test")]
pub fn spawn_with_devices(name: &str) -> Result<Arc<Process>, SpawnError> {
    spawn_with_devices_and(name, None)
}

/// As [`spawn_with_devices`], with one more handle at a chosen slot.
#[cfg(feature = "qemu-test")]
pub fn spawn_with_devices_and(
    name: &str,
    extra: Option<(u32, Object, zoraya_abi::Rights)>,
) -> Result<Arc<Process>, SpawnError> {
    let (name, file) = ramdisk::find(name).ok_or(SpawnError::NotFound)?;
    let root = Job::root().map_err(|_| SpawnError::NoMemory)?;
    let job = Job::create(&root, false).map_err(|_| SpawnError::NoMemory)?;
    let mut handles = kernel_handles(job.clone())?;
    insert_devices(&mut handles)?;
    if let Some((slot, object, rights)) = extra {
        handles
            .insert_at(slot, object, rights, object::next_id())
            .map_err(|_| SpawnError::NoMemory)?;
    }
    create(Start {
        name,
        image: Loadable::from_file(file)?,
        job,
        handles,
        argv: Strings::empty(),
        envp: Strings::empty(),
        dispositions: [Disposition::Terminate; REQUEST_KINDS],
        suspended: false,
    })
}

/// Starts a process from ELF bytes, in the root job.
#[cfg(feature = "qemu-test")]
pub fn spawn_bytes(name: &str, file: &[u8]) -> Result<Arc<Process>, SpawnError> {
    spawn_bytes_as(name, file, false)
}

#[cfg(feature = "qemu-test")]
fn spawn_bytes_as(name: &str, file: &[u8], suspended: bool) -> Result<Arc<Process>, SpawnError> {
    // A job of its own, so that killing it leaves the root usable.
    let root = Job::root().map_err(|_| SpawnError::NoMemory)?;
    let job = Job::create(&root, false).map_err(|_| SpawnError::NoMemory)?;
    create(Start {
        name,
        image: Loadable::from_file(file)?,
        job: job.clone(),
        handles: kernel_handles(job)?,
        argv: Strings::empty(),
        envp: Strings::empty(),
        dispositions: [Disposition::Terminate; REQUEST_KINDS],
        suspended,
    })
}

/// Starts the first process (ADR-0063), the only one with a `Power` handle
/// (ADR-0062).
pub fn spawn_first(name: &str) -> Result<Arc<Process>, SpawnError> {
    println!("first process: {name}");
    let (name, file) = ramdisk::find(name).ok_or(SpawnError::NotFound)?;
    let root = Job::root().map_err(|_| SpawnError::NoMemory)?;
    let job = Job::create(&root, false).map_err(|_| SpawnError::NoMemory)?;
    // The reserve (ADR-0261 point 10): everything `init` starts is bounded by
    // the frames free now less 1/16 for the heap's overhead and the fixed
    // allocations, so that the kernel always has room to refuse with.
    let free = crate::memory::frames::stats().free * zoraya_pmm::FRAME_SIZE;
    job.set_limits(zoraya_abi::quota::JobLimits {
        memory: Some(free - free / KERNEL_RESERVE_DIVISOR),
        processes: None,
        cpu: None,
        threads: None,
    })
    .map_err(|_| SpawnError::NoMemory)?;
    let mut handles = kernel_handles(job.clone())?;
    handles
        .insert_at(
            POWER.slot(),
            Object::Power,
            PowerObject::RIGHTS,
            object::next_id(),
        )
        .map_err(|_| SpawnError::NoMemory)?;
    insert_devices(&mut handles)?;
    if let Some(seed) = crate::device::seed_object() {
        // Read and map only: `init` hands it to `rngd` and keeps nothing.
        handles
            .insert_at(
                SEED_SLOT,
                Object::Memory(seed),
                Rights::READ.union(Rights::MAP).union(Rights::TRANSFER),
                object::next_id(),
            )
            .map_err(|_| SpawnError::NoMemory)?;
    }
    create(Start {
        name,
        image: Loadable::from_file(file)?,
        job,
        handles,
        argv: Strings::empty(),
        envp: Strings::empty(),
        dispositions: [Disposition::Terminate; REQUEST_KINDS],
        suspended: false,
    })
}

/// Starts the ramdisk's program `name`.
#[cfg(feature = "qemu-test")]
pub fn spawn(name: &str) -> Result<Arc<Process>, SpawnError> {
    let (name, file) = ramdisk::find(name).ok_or(SpawnError::NotFound)?;
    spawn_bytes(name, file)
}

/// As [`spawn`], but suspended before its first instruction (ADR-0003): it
/// holds its address space and ASID, and runs nothing until resumed.
#[cfg(feature = "qemu-test")]
pub fn spawn_suspended(name: &str) -> Result<Arc<Process>, SpawnError> {
    let (name, file) = ramdisk::find(name).ok_or(SpawnError::NotFound)?;
    spawn_bytes_as(name, file, true)
}

/// As [`spawn_suspended`], from ELF bytes (the test kernel patches a program's
/// segment to make a large address space).
#[cfg(feature = "qemu-test")]
pub fn spawn_bytes_suspended(name: &str, file: &[u8]) -> Result<Arc<Process>, SpawnError> {
    spawn_bytes_as(name, file, true)
}

/// Ends the running process (ADR-0041). Every kind of exit comes here: the
/// record is fixed, if no one fixed one first, and the calling thread leaves.
pub fn exit_current(record: ExitRecord) -> ! {
    {
        let thread = sched::current();
        let process = thread.process().expect("a process is running");
        process.request_exit(record);
    }
    leave_current()
}

/// The running thread leaves because its process is ending, or because it
/// called `thread_exit` (the second phase of ADR-0300 item 6). The thread
/// whose leave empties the list is the owner and tears the process down;
/// any other ends here. Does not return.
pub fn leave_current() -> ! {
    // This function does not return, so nothing it holds at the end would
    // ever be dropped: every count goes before `sched::exit_current`.
    let (process, id) = {
        let thread = sched::current();
        (
            thread.process().expect("a process is running").clone(),
            thread.id(),
        )
    };
    // The first to leave an ending process wakes the others, in steps and
    // outside every lock; each finds the record and leaves.
    if process.exit_requested() && !process.swept.swap(true, Ordering::Relaxed) {
        thread::wake_siblings(&process);
    }
    end_thread_record(&process);
    // From here to the switch nothing yields: the thread is out of the list
    // and a teardown may begin as soon as it is off the CPU.
    arch::save_and_mask_interrupts();
    let left = process.members.lock().leave(id);
    if left.began_end {
        // The last thread's exit ends the process as `Exited` with code 0
        // (ADR-0300 item 3).
        process.requests.lock().exit.get_or_insert(ExitRecord {
            code: 0,
            reason: ExitReason::Exited,
            detail: 0,
            request_kind: None,
        });
    }
    if left.owner {
        teardown(process);
    }
    // Not the owner: wake the new first thread, so that a sibling the
    // steps skipped is still reached (ADR-0300 item 6).
    if process.exit_requested() {
        let first = process.members.lock().first().cloned();
        if let Some(first) = first {
            sched::wake(first);
        }
    }
    drop(process);
    sched::exit_current()
}

/// What a thread does with its own record when it leaves: gives the `threads`
/// figure back (ADR-0307 item 7), cuts its object's link and asserts
/// `TERMINATED`, and leaves the park slot.
fn end_thread_record(process: &Process) {
    let thread = sched::current();
    thread.release_thread();
    let object = thread.object().lock().take();
    if let Some(object) = object {
        object.terminated();
    }
    let mut head = process.park_head.lock();
    if head.as_ref().is_some_and(|h| Arc::ptr_eq(h, &thread)) {
        *head = None;
    }
}

/// The owner's teardown (ADR-0041): every other thread has left. Does not
/// return.
fn teardown(process: Arc<Process>) -> ! {
    let record = process
        .requests
        .lock()
        .exit
        .expect("the record was fixed before the end");
    // A pause first, so that this exit's first part is not added to the last
    // part of the exit that ran just before it (the scheduler switches from
    // one dying thread to the next with IRQ masked throughout): without it a
    // job_kill of many processes held the tail of one exit (up to 64 frames)
    // and the head of the next in one interval, 94 to 212 units against 80
    // (found on CI and under TCG, `t_cpu`; INV-0270-1). A thread that comes
    // here from `return_to_user`'s preemption point resumed inside the
    // interval of the threads that ran meanwhile; one that is switched away
    // at a step point below gets a window of its own when it resumes
    // (`steps::step_point`), so the tail after the second flush is not
    // added to another exit's tail either.
    crate::steps::flush();
    // The handles first, while the space is still the running one: closing
    // a long chain of channels yields between steps (ADR-0270), which is not
    // allowed once the table is off (below).
    loop {
        let entry = process.handles.lock().pop();
        match entry {
            Some(entry) => {
                // The work of the whole exit is counted, not of each
                // object: one unit for the handle, and the frames an object
                // frees count themselves; a pause after each step of them
                // (INV-0270-1).
                crate::steps::before(object::weight(&entry));
                crate::steps::count(1);
                object::close(entry);
            }
            None => break,
        }
    }
    // The mappings next, for the same reason, while the space is still the
    // running one: dropping a mapping may free the object it was the last
    // holder of. This thread is the process's last (the owner).
    loop {
        let holder = process.space.lock().as_mut().and_then(|s| s.pop_mapping());
        let Some(holder) = holder else { break };
        crate::steps::before(holder.weight());
        crate::steps::count(1);
        drop(holder);
    }
    // The count starts again, for what follows cannot yield.
    crate::steps::flush();
    // SAFETY: no table is the safe value.
    unsafe { mmu::set_user_table(None) };
    // From here to `sched::exit_current` nothing may block or yield: the
    // scheduler would install `process.ttbr0()` again, a root freed below
    // with an ASID another process may already hold. No other thread of the
    // process exists (the owner is the last to leave), so no other CPU can be
    // using the space either.
    let space = process.space.lock().take();
    #[cfg(feature = "qemu-test")]
    {
        let mut watch = process.watch.lock();
        if watch.0 != 0 {
            watch.1 = space.as_ref().and_then(|s| s.peek(watch.0));
        }
    }
    drop(space);
    // A zombie holds its record, not the table's memory, nor the lists of
    // its threads.
    let table = core::mem::take(&mut *process.handles.lock());
    drop(table);
    process.paid.lock().release_kernel(handle_table_cost());
    let multi = process.multi.lock().take();
    if multi.is_some() {
        let cost = process.multi_cost.swap(0, Ordering::Relaxed);
        process.paid.lock().release_kernel(cost);
        // The member list grown with the lists was part of that charge: a
        // zombie keeps none of it (the owner left, so the list is empty).
        process.members.lock().release();
    }
    drop(multi);
    *process.park_head.lock() = None;
    let waiters = {
        let mut exit = process.exit.lock();
        exit.record = Some(record);
        core::mem::take(&mut exit.waiters)
    };
    process.suspended.store(false, Ordering::Relaxed);
    port::clear_signals(
        process.source,
        process_signals::SUSPENDED | process_signals::REQUEST_PENDING,
    );
    port::set_signals(process.source, process_signals::TERMINATED);
    process.job.leave(&process);
    process.paid.lock().release_process();
    for waiter in waiters {
        sched::wake(waiter);
    }
    drop(process);
    sched::exit_current()
}

/// Waits while the process's job tree is suspended or a fork is copying its
/// space, unless it is to end. Called on the thread's own stack with
/// interrupts masked, on the way to EL0 (or by a walk of the job tree that
/// waits its turn, with no lock held, `job::Exclusive`); [`Process::cont`], a
/// thaw or an exit request wakes it. The thread woken by `cont` wakes the
/// other parked threads, in steps (ADR-0300 item 7).
pub(crate) fn park_if_stopped() {
    let thread = sched::current();
    let Some(process) = thread.process() else {
        return;
    };
    let mut waited = false;
    while process.must_park() && !process.exit_requested() {
        waited = true;
        thread.parked().store(true, Ordering::Relaxed);
        {
            let mut head = process.park_head.lock();
            if head.is_none() {
                *head = Some(thread.clone());
            }
        }
        sched::block_current();
    }
    thread.parked().store(false, Ordering::Relaxed);
    if waited {
        let mut head = process.park_head.lock();
        if head.as_ref().is_some_and(|h| Arc::ptr_eq(h, &thread)) {
            *head = None;
        }
        drop(head);
        thread::wake_parked(process);
    }
}

/// Whether the running process is stopped and not to end, so that
/// [`park_if_stopped`] would wait. False for a thread that is not a process.
pub(crate) fn current_is_stopped() -> bool {
    let thread = sched::current();
    thread
        .process()
        .is_some_and(|p| p.stopped.load(Ordering::Relaxed) && !p.exit_requested())
}

/// Leaves if the process was asked to end.
fn exit_if_requested() {
    let ending = {
        let thread = sched::current();
        thread.process().is_some_and(|p| p.exit_requested())
    };
    if ending {
        leave_current();
    }
}

/// The last thing on every way back to EL0. Called on the thread's own stack
/// with interrupts masked.
pub fn return_to_user() {
    sched::preempt_point();
    park_if_stopped();
    exit_if_requested();
}

/// Runs first on a new user thread, from assembly. A request that came before
/// it ever ran ends it here, before its first instruction.
#[unsafe(no_mangle)]
extern "C" fn user_thread_start() {
    park_if_stopped();
    exit_if_requested();
    // The thread returns to EL0, where IRQ is on, with no exception taken.
    // The counter ends here and not before, so that the exit of a process
    // killed before it ran is measured (INV-0270-1).
    crate::steps::mask_ended();
}

/// Ends the running process for a synchronous exception from EL0 (ADR-0034).
pub fn fault_current(class: u64, elr: u64, far: Option<u64>) -> ! {
    {
        let thread = sched::current();
        let name = thread.process().map_or("?", |p| p.name());
        match far {
            Some(far) => {
                println!("process {name}: fault, EC {class:#x}, ELR_EL1 {elr:#x}, FAR_EL1 {far:#x}")
            }
            None => println!("process {name}: fault, EC {class:#x}, ELR_EL1 {elr:#x}"),
        }
    }
    exit_current(ExitRecord {
        code: 0,
        reason: ExitReason::Fault,
        detail: class as u32,
        request_kind: None,
    })
}

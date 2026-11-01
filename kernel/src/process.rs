//! Processes (ADR-0003, ADR-0004, ADR-0031, ADR-0033, ADR-0034, ADR-0041). A
//! process has one thread at this stage.
//!
//! A process ends on its own thread: whoever else ends it sets the exit it
//! wants and wakes it, and it acts on that at its next return to EL0 or as it
//! starts, so it runs no user instruction after the request, and nothing is
//! torn down under a thread blocked in the kernel.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use zoraya_abi::spawn::{
    STARTUP_BLOCK, StartupBlock, Strings, USER_STACK_SIZE, USER_STACK_TOP, startup_header,
};
use zoraya_abi::{
    CONSOLE, ConsoleObject, DIRECTORY, DirectoryObject, Disposition, ExitReason, ExitRecord, JOB,
    JobObject, ObjectType, REQUEST_KINDS, RequestKind, Status, process_signals,
};
use zoraya_elf::{Error as ElfError, PAGE_SIZE, Program, Segment};
use zoraya_events::SourceId;
use zoraya_pagetable::{Attributes, MemoryType};

use crate::arch::mmu;
use crate::job::Job;
use crate::memory::heap::{OutOfMemory, try_arc};
use crate::memory::user::{AddressSpace, MapFailed, USER_HIGH, USER_LOW};
use crate::object::{self, HandleTable, Object};
use crate::sched::{self, Thread};
use crate::sync::SpinLock;
use crate::{arch, port, println, ramdisk};

const _: () = assert!(USER_STACK_TOP == USER_HIGH - PAGE_SIZE);

/// Requests queued per process (ADR-0046).
const MAX_REQUESTS: usize = 64;

/// Why a process could not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    NotFound,
    BadProgram(ElfError),
    NoMemory,
    /// The job was killed.
    JobClosed,
}

impl From<OutOfMemory> for SpawnError {
    fn from(_: OutOfMemory) -> SpawnError {
        SpawnError::NoMemory
    }
}

impl From<MapFailed> for SpawnError {
    fn from(_: MapFailed) -> SpawnError {
        SpawnError::NoMemory
    }
}

impl From<SpawnError> for Status {
    fn from(e: SpawnError) -> Status {
        match e {
            SpawnError::NotFound => Status::NotFound,
            SpawnError::BadProgram(_) => Status::InvalidArgument,
            SpawnError::NoMemory => Status::NoMemory,
            SpawnError::JobClosed => Status::BadState,
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
    /// A ring of what `Deliver` queued, in fixed memory, so delivery cannot
    /// fail for lack of it.
    queue: [(RequestKind, u32); MAX_REQUESTS],
    first: usize,
    count: usize,
    /// How this process is to end, once it next can.
    exit: Option<ExitRecord>,
}

impl Requests {
    fn push(&mut self, kind: RequestKind, detail: u32) -> Result<(), Status> {
        if self.count == MAX_REQUESTS {
            return Err(Status::LimitReached);
        }
        self.queue[(self.first + self.count) % MAX_REQUESTS] = (kind, detail);
        self.count += 1;
        Ok(())
    }

    fn pop(&mut self) -> Option<(RequestKind, u32)> {
        if self.count == 0 {
            return None;
        }
        let request = self.queue[self.first];
        self.first = (self.first + 1) % MAX_REQUESTS;
        self.count -= 1;
        Some(request)
    }

    /// The first queued request of `kind`.
    fn queued(&self, kind: RequestKind) -> Option<u32> {
        (0..self.count)
            .map(|i| self.queue[(self.first + i) % MAX_REQUESTS])
            .find(|(k, _)| *k == kind)
            .map(|(_, detail)| detail)
    }
}

fn terminated(detail: u32) -> ExitRecord {
    ExitRecord {
        code: 0,
        reason: ExitReason::Terminated,
        detail,
    }
}

pub struct Process {
    name: String,
    ttbr0: u64,
    /// Released at exit, whoever still holds the process.
    space: SpinLock<Option<AddressSpace>>,
    exit: SpinLock<Exit>,
    job: Arc<Job>,
    source: SourceId,
    pub handles: SpinLock<HandleTable>,
    /// Its thread, until exit: how a request reaches a process that is not
    /// running.
    thread: SpinLock<Option<Arc<Thread>>>,
    requests: SpinLock<Requests>,
    suspended: AtomicBool,
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
        self.requests.lock().count
    }

    #[cfg(feature = "qemu-test")]
    pub fn thread(&self) -> Option<Arc<Thread>> {
        self.thread.lock().clone()
    }

    /// Whether this process must end, at its next chance.
    pub fn exit_requested(&self) -> bool {
        self.requests.lock().exit.is_some()
    }

    /// Asks the process to end with `record`, unless it was already asked.
    pub fn request_exit(&self, record: ExitRecord) {
        self.requests.lock().exit.get_or_insert(record);
        let thread = self.thread.lock().clone();
        if let Some(thread) = thread {
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
                self.request_exit(terminated(detail));
                Ok(())
            }
            Disposition::Deliver => {
                requests.push(kind, detail)?;
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
        let queued = requests.queued(kind);
        drop(requests);
        if disposition == Disposition::Terminate
            && let Some(detail) = queued
        {
            self.request_exit(terminated(detail));
        }
    }

    pub fn take_request(&self) -> Option<(RequestKind, u32)> {
        let mut requests = self.requests.lock();
        let request = requests.pop();
        let empty = requests.count == 0;
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
        let thread = self.thread.lock().clone();
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

/// Copies a segment into new pages.
fn load_segment(space: &mut AddressSpace, file: &[u8], s: &Segment) -> Result<(), SpawnError> {
    let attributes = attributes(s);
    let (start, end) = s.pages();
    let file_end = s.vaddr + s.filesz;
    for page in (start..end).step_by(PAGE_SIZE as usize) {
        let target = space.map_new_page(page, attributes)?;
        // The part of this page that has file bytes.
        let from = page.max(s.vaddr);
        let to = (page + PAGE_SIZE).min(file_end);
        if from < to {
            let offset = (s.offset + (from - s.vaddr)) as usize;
            let bytes = &file[offset..offset + (to - from) as usize];
            // SAFETY: `target` is a whole new page, only reachable by this
            // function until the process runs, and `from - page` plus the
            // length stays inside it. The parser checked the file range.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    target.add((from - page) as usize),
                    bytes.len(),
                );
            }
        }
        if s.execute {
            // SAFETY: `target` is a whole page in the direct map.
            unsafe { mmu::sync_instruction_cache(target as u64, PAGE_SIZE) };
        }
    }
    Ok(())
}

/// Maps the block a process reads its vectors from.
fn map_startup_block(
    space: &mut AddressSpace,
    argv: Strings<'_>,
    envp: Strings<'_>,
) -> Result<(), SpawnError> {
    let header = startup_header(argv.len(), envp.len());
    let parts = [&header[..], argv.as_bytes(), envp.as_bytes()];
    let mut parts = parts.iter().flat_map(|p| p.iter().copied());
    let size = (header.len() + argv.as_bytes().len() + envp.as_bytes().len()) as u64;
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
    pub file: &'a [u8],
    pub job: Arc<Job>,
    pub handles: HandleTable,
    pub argv: Strings<'a>,
    pub envp: Strings<'a>,
    pub dispositions: [Disposition; REQUEST_KINDS],
    pub suspended: bool,
}

/// Makes a process. Nothing else has changed if this fails.
pub fn create(start: Start<'_>) -> Result<Arc<Process>, SpawnError> {
    let file = start.file;
    let program = Program::parse(file, USER_LOW, STARTUP_BLOCK - PAGE_SIZE)
        .map_err(SpawnError::BadProgram)?;
    let mut space = AddressSpace::new()?;
    for segment in program.segments() {
        load_segment(&mut space, file, segment)?;
    }
    let stack_low = USER_STACK_TOP - USER_STACK_SIZE;
    for page in (stack_low..USER_STACK_TOP).step_by(PAGE_SIZE as usize) {
        space.map_new_page(page, USER_STACK)?;
    }
    map_startup_block(&mut space, start.argv, start.envp)?;
    let mut name = String::new();
    name.try_reserve(start.name.len())
        .map_err(|_| OutOfMemory)?;
    name.push_str(start.name);
    let bits = if start.suspended {
        process_signals::SUSPENDED
    } else {
        0
    };
    let source = port::new_source(bits).map_err(|_| OutOfMemory)?;
    let ttbr0 = space.ttbr0();
    let process = try_arc(Process {
        name,
        ttbr0,
        space: SpinLock::new(Some(space)),
        exit: SpinLock::new(Exit {
            record: None,
            waiters: Vec::new(),
        }),
        job: start.job,
        source,
        handles: SpinLock::new(start.handles),
        thread: SpinLock::new(None),
        requests: SpinLock::new(Requests {
            dispositions: start.dispositions,
            queue: [(RequestKind::Interrupt, 0); MAX_REQUESTS],
            first: 0,
            count: 0,
            exit: None,
        }),
        suspended: AtomicBool::new(start.suspended),
        #[cfg(feature = "qemu-test")]
        watch: SpinLock::new((0, None)),
    });
    let process = match process {
        Ok(process) => process,
        Err(e) => {
            port::free_source(source);
            return Err(e.into());
        }
    };
    let thread = sched::spawn_user(process.clone(), program.entry, USER_STACK_TOP)?;
    *process.thread.lock() = Some(thread.clone());
    if let Err(status) = process.job.admit(&process) {
        *process.thread.lock() = None;
        sched::discard(thread);
        return Err(match status {
            Status::BadState => SpawnError::JobClosed,
            _ => SpawnError::NoMemory,
        });
    }
    if !start.suspended {
        sched::wake(thread);
    }
    Ok(process)
}

/// The handles of a process the kernel starts (ADR-0043).
fn kernel_handles() -> Result<HandleTable, SpawnError> {
    let job = Job::root().map_err(|_| SpawnError::NoMemory)?;
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

/// Starts a process from ELF bytes, in the root job.
pub fn spawn_bytes(name: &str, file: &[u8]) -> Result<Arc<Process>, SpawnError> {
    let none = StartupBlock::decode(&[0; 8]).ok_or(SpawnError::NoMemory)?;
    create(Start {
        name,
        file,
        job: Job::root().map_err(|_| SpawnError::NoMemory)?,
        handles: kernel_handles()?,
        argv: none.argv,
        envp: none.envp,
        dispositions: [Disposition::Terminate; REQUEST_KINDS],
        suspended: false,
    })
}

/// Starts the ramdisk's program `name`.
pub fn spawn(name: &str) -> Result<Arc<Process>, SpawnError> {
    let (name, file) = ramdisk::find(name).ok_or(SpawnError::NotFound)?;
    spawn_bytes(name, file)
}

/// Ends the running process (ADR-0041). Every kind of exit comes here.
pub fn exit_current(record: ExitRecord) -> ! {
    // This function does not return, so nothing it holds at the end would
    // ever be dropped: every count goes before `sched::exit_current`.
    let process = sched::current()
        .process()
        .expect("a process is running")
        .clone();
    // SAFETY: no table is the safe value.
    unsafe { mmu::set_user_table(None) };
    let space = process.space.lock().take();
    #[cfg(feature = "qemu-test")]
    {
        let mut watch = process.watch.lock();
        if watch.0 != 0 {
            watch.1 = space.as_ref().and_then(|s| s.peek(watch.0));
        }
    }
    drop(space);
    loop {
        let entry = process.handles.lock().pop();
        match entry {
            Some(entry) => object::close(entry),
            None => break,
        }
    }
    let waiters = {
        let mut exit = process.exit.lock();
        exit.record = Some(record);
        core::mem::take(&mut exit.waiters)
    };
    port::clear_signals(
        process.source,
        process_signals::SUSPENDED | process_signals::REQUEST_PENDING,
    );
    port::set_signals(process.source, process_signals::TERMINATED);
    process.job.leave(&process);
    let thread = process.thread.lock().take();
    drop(thread);
    for waiter in waiters {
        sched::wake(waiter);
    }
    drop(process);
    sched::exit_current()
}

/// Ends the running process if it was asked to.
fn exit_if_requested() {
    let record = {
        let thread = sched::current();
        thread.process().and_then(|p| p.requests.lock().exit.take())
    };
    if let Some(record) = record {
        exit_current(record);
    }
}

/// The last thing on every way back to EL0. Called on the thread's own stack
/// with interrupts masked.
pub fn return_to_user() {
    sched::preempt_point();
    exit_if_requested();
}

/// Runs first on a new user thread, from assembly. A request that came before
/// it ever ran ends it here, before its first instruction.
#[unsafe(no_mangle)]
extern "C" fn user_thread_start() {
    exit_if_requested();
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
    })
}

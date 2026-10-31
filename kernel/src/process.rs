//! Processes (ADR-0031, ADR-0033, ADR-0034). At this stage a process has one
//! thread and is started by the kernel; `spawn` for user space is stage 3.

use alloc::string::String;
use alloc::sync::Arc;

use zoraya_abi::{CONSOLE, ExitReason, ExitRecord, Handle};
use zoraya_elf::{Error as ElfError, PAGE_SIZE, Program, Segment};
use zoraya_pagetable::{Attributes, MemoryType};

use crate::arch::mmu;
use crate::memory::heap::{OutOfMemory, try_box};
use crate::memory::user::{AddressSpace, MapFailed, USER_HIGH, USER_LOW};
use crate::sched::{self, Thread};
use crate::sync::SpinLock;
use crate::{arch, println, ramdisk};

/// Size of a process's stack. Its page below stays unmapped.
pub const USER_STACK_SIZE: u64 = 64 * 1024;
/// Top of a process's stack: one page below the end of the user range, so
/// that a stray `sp` around the limit stays out of range.
const USER_STACK_TOP: u64 = USER_HIGH - PAGE_SIZE;

/// Why a process could not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    NotFound,
    BadProgram(ElfError),
    NoMemory,
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

struct Exit {
    record: Option<ExitRecord>,
    /// The thread blocked in [`Process::wait`].
    waiter: Option<Arc<Thread>>,
}

pub struct Process {
    name: String,
    ttbr0: u64,
    /// Released at exit, whoever still holds the process.
    space: SpinLock<Option<AddressSpace>>,
    exit: SpinLock<Exit>,
}

impl Process {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The value for `TTBR0_EL1`.
    pub fn ttbr0(&self) -> u64 {
        self.ttbr0
    }

    /// Whether `handle` names this process's console. Stage 3 replaces this
    /// with a handle table.
    pub fn has_console(&self, handle: Handle) -> bool {
        handle == CONSOLE
    }

    /// Blocks until the process has ended.
    pub fn wait(&self) -> ExitRecord {
        let daif = arch::save_and_mask_interrupts();
        loop {
            {
                let mut exit = self.exit.lock();
                if let Some(record) = exit.record {
                    arch::restore_interrupts(daif);
                    return record;
                }
                exit.waiter = Some(sched::current());
            }
            sched::block_current();
        }
    }

    /// Calls `f` for every mapping, while the process lives.
    #[cfg(feature = "qemu-test")]
    pub fn for_each_mapping(&self, f: &mut impl FnMut(zoraya_pagetable::Mapping)) {
        if let Some(space) = self.space.lock().as_ref() {
            space.for_each_mapping(f);
        }
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
            mmu::sync_instruction_cache(target as u64, PAGE_SIZE);
        }
    }
    Ok(())
}

/// Starts a process from ELF bytes.
pub fn spawn_bytes(name: &str, file: &[u8]) -> Result<Arc<Process>, SpawnError> {
    let program = Program::parse(file, USER_LOW, USER_STACK_TOP - USER_STACK_SIZE - PAGE_SIZE)
        .map_err(SpawnError::BadProgram)?;
    let mut space = AddressSpace::new()?;
    for segment in program.segments() {
        load_segment(&mut space, file, segment)?;
    }
    let stack_low = USER_STACK_TOP - USER_STACK_SIZE;
    for page in (stack_low..USER_STACK_TOP).step_by(PAGE_SIZE as usize) {
        space.map_new_page(page, USER_STACK)?;
    }
    let mut owned_name = String::new();
    owned_name
        .try_reserve(name.len())
        .map_err(|_| OutOfMemory)?;
    owned_name.push_str(name);
    let process = Process {
        name: owned_name,
        ttbr0: space.ttbr0(),
        space: SpinLock::new(Some(space)),
        exit: SpinLock::new(Exit {
            record: None,
            waiter: None,
        }),
    };
    let process: Arc<Process> = Arc::from(try_box(process)?);
    sched::spawn_user(process.clone(), program.entry, USER_STACK_TOP)?;
    Ok(process)
}

/// Starts the ramdisk's program `name`.
pub fn spawn(name: &str) -> Result<Arc<Process>, SpawnError> {
    let file = ramdisk::get().find(name).ok_or(SpawnError::NotFound)?;
    spawn_bytes(name, file)
}

/// Ends the running process.
pub fn exit_current(record: ExitRecord) -> ! {
    // This function does not return, so nothing it holds at the end would
    // ever be dropped: both counts go before `sched::exit_current`.
    let process = sched::current()
        .process()
        .expect("a process is running")
        .clone();
    mmu::set_user_table(None);
    let space = process.space.lock().take();
    let waiter = {
        let mut exit = process.exit.lock();
        exit.record = Some(record);
        exit.waiter.take()
    };
    drop(space);
    if let Some(waiter) = waiter {
        sched::wake(waiter);
    }
    drop(process);
    sched::exit_current()
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

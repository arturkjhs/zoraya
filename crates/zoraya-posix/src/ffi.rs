//! The C boundary: every name `libc.a` needs from the Rust half (ADR-0100,
//! ADR-0140's list in `ports/posix-stub/stubs.txt` before this crate), the
//! start-up of ADR-0109 item 4, `malloc` over memory objects, and the
//! system-call shim's targets (ADR-0141 item 2). The only module with
//! `unsafe` (INV-0100-3): C hands it pointers.
//!
//! Process functions (`fork`, `posix_spawn`, `wait*`, signals, `kill`, the
//! terminal) are stage 6d's, in `procs`. A process has one thread (ADR-0100
//! item 5), so the state is a cell with no lock.

mod procs;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::{Cell, UnsafeCell};
use core::ffi::c_char;
use core::ptr;

use zoraya_abi::posix::{self as abi, Bootstrap, SELF_MESSAGE, slot};
use zoraya_abi::{
    Buffer, Bytes, ChannelEntry, DEADLINE_INFINITE, Handle, Invoke, MapFlags, MemoryFlags,
    MemoryObject, Mode, OBJECT_TYPES, ObjectType, Packet, Status, stubs,
};
use zoraya_heap::{Backing, Heap, PAGE_SIZE};

use crate::errno::{self, *};
use crate::path::PATH_MAX;
use crate::process::{self, AT_EMPTY_PATH, AT_FDCWD, Desc, Posix, Target, o};
use crate::stat;
use crate::sys::{Sys, User};

// ------------------------------------------------------------ the kernel

struct Kernel;

impl Invoke for Kernel {
    fn invoke(number: u64, registers: [u64; 6]) -> (u64, u64) {
        zoraya_rt::raw_syscall(number, registers)
    }
}

fn buffer(user: User) -> Bytes {
    Bytes(Buffer {
        address: user.address,
        length: user.length as u64,
    })
}

/// The rights a handle of `h`'s type can have: what a moved entry names.
fn rights_of(h: Handle) -> zoraya_abi::Rights {
    let name = zoraya_rt::handle_type(h).unwrap_or("");
    OBJECT_TYPES
        .iter()
        .find(|t| t.name == name)
        .map_or(zoraya_abi::Rights::NONE, |t| t.rights)
}

/// `zoraya-rt` behind the library's trait.
struct RtSys {
    port: Option<Handle>,
}

impl Sys for RtSys {
    type H = Handle;

    fn close(&mut self, h: Handle) {
        let _ = zoraya_rt::handle_close(h);
    }

    fn duplicate(&mut self, h: Handle) -> Result<Handle, Status> {
        zoraya_rt::handle_duplicate(h, MemoryObject::RIGHTS)
    }

    fn channel_create(&mut self) -> Result<(Handle, Handle), Status> {
        zoraya_rt::channel_create()
    }

    fn channel_write(&mut self, end: Handle, bytes: &[u8], moved: &[Handle]) -> Result<(), Status> {
        let mut entries = [ChannelEntry {
            handle: end,
            rights: zoraya_abi::Rights::NONE,
            transfer: zoraya_abi::spawn::Transfer::Move,
        }; 4];
        let list = entries
            .get_mut(..moved.len())
            .ok_or(Status::InvalidArgument)?;
        for (e, &h) in list.iter_mut().zip(moved) {
            e.handle = h;
            e.rights = rights_of(h);
        }
        let r = zoraya_rt::channel_write(end, bytes, list);
        if r.is_err() {
            for &h in moved {
                let _ = zoraya_rt::handle_close(h);
            }
        }
        r
    }

    fn channel_read(
        &mut self,
        end: Handle,
        bytes: &mut [u8],
        handles: &mut Vec<Handle>,
    ) -> Result<usize, Status> {
        let mut got = [end; 64];
        match zoraya_rt::channel_read(end, bytes, &mut got) {
            Ok(sizes) => {
                handles.extend_from_slice(&got[..sizes.handles as usize]);
                Ok(sizes.bytes as usize)
            }
            Err((s, _)) => Err(s),
        }
    }

    fn wait(&mut self, h: Handle, wanted: u32) -> Result<u32, Status> {
        let port = match self.port {
            Some(port) => port,
            None => {
                let port = zoraya_rt::port_create()?;
                self.port = Some(port);
                port
            }
        };
        zoraya_rt::port_subscribe(port, h, wanted, 1, Mode::Level)?;
        let mut packets = [Packet::default()];
        let r = zoraya_rt::port_wait(port, DEADLINE_INFINITE, &mut packets);
        let _ = zoraya_rt::port_unsubscribe(port, 1);
        r?;
        Ok(packets[0].signals)
    }

    fn stream_create(&mut self) -> Result<(Handle, Handle), Status> {
        zoraya_rt::stream_create()
    }

    fn stream_read(&mut self, end: Handle, into: User) -> Result<usize, Status> {
        // The kernel checks the program's memory: a bad pointer is EFAULT.
        stubs::stream_read::<Kernel>(end, buffer(into)).map(|n| n as usize)
    }

    fn stream_write(&mut self, end: Handle, from: User) -> Result<usize, Status> {
        stubs::stream_write::<Kernel>(end, buffer(from)).map(|n| n as usize)
    }

    fn copy_in(&mut self, from: User, out: &mut [u8]) {
        let n = out.len().min(from.length);
        // SAFETY: the program passed this memory for this call: a C library
        // reads it the same way, and a bad address faults the process.
        unsafe { ptr::copy_nonoverlapping(from.address as *const u8, out.as_mut_ptr(), n) };
    }

    fn copy_out(&mut self, to: User, bytes: &[u8]) {
        let n = bytes.len().min(to.length);
        // SAFETY: as in `copy_in`, for memory the program gave to be written.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), to.address as *mut u8, n) };
    }

    fn copy(&mut self, to: User, from: User) {
        let n = to.length.min(from.length);
        // SAFETY: as in `copy_in`; the scratch buffer is this library's own
        // mapping and never the program's, so the two do not overlap.
        unsafe { ptr::copy_nonoverlapping(from.address as *const u8, to.address as *mut u8, n) };
    }

    fn memory_create(&mut self, size: u64) -> Result<Handle, Status> {
        zoraya_rt::memory_create(size, MemoryFlags::NONE)
    }

    fn memory_map(&mut self, object: Handle, length: u64) -> Result<u64, Status> {
        zoraya_rt::memory_map(object, 0, length, MapFlags::WRITE)
    }

    fn memory_unmap(&mut self, address: u64, length: u64) -> Result<(), Status> {
        zoraya_rt::memory_unmap(address, length)
    }

    fn now(&mut self) -> u64 {
        zoraya_rt::clock_monotonic()
    }

    fn wait_any(
        &mut self,
        sources: &[(Handle, u32)],
        deadline: Option<u64>,
    ) -> Result<(usize, u32), Status> {
        let port = match self.port {
            Some(port) => port,
            None => {
                let port = zoraya_rt::port_create()?;
                self.port = Some(port);
                port
            }
        };
        let mut subscribed = 0;
        let mut r = Ok(());
        for (i, &(h, wanted)) in sources.iter().enumerate() {
            r = zoraya_rt::port_subscribe(port, h, wanted, i as u64 + 1, Mode::Level);
            if r.is_err() {
                break;
            }
            subscribed = i + 1;
        }
        let mut packets = [Packet::default()];
        let waited = match r {
            Ok(()) => {
                zoraya_rt::port_wait(port, deadline.unwrap_or(DEADLINE_INFINITE), &mut packets)
            }
            Err(s) => Err(s),
        };
        for i in 0..subscribed {
            let _ = zoraya_rt::port_unsubscribe(port, i as u64 + 1);
        }
        waited?;
        let key = packets[0].key as usize;
        if key == 0 || key > sources.len() {
            return Err(Status::BadState);
        }
        Ok((key - 1, packets[0].signals))
    }

    fn signals_of(&mut self, h: Handle) -> Result<u32, Status> {
        zoraya_rt::object_get_signals(h)
    }

    fn take_request(&mut self) -> Option<(zoraya_abi::RequestKind, u32)> {
        zoraya_rt::process_take_request().ok()
    }

    fn set_disposition(
        &mut self,
        kind: zoraya_abi::RequestKind,
        d: zoraya_abi::Disposition,
    ) -> Result<(), Status> {
        zoraya_rt::process_set_disposition(kind, d)
    }

    fn slot_of(&self, h: Handle) -> u32 {
        h.slot()
    }
}

// ------------------------------------------------------------ the state

/// One thread (ADR-0100 item 5): a cell, with a flag that turns a reentry,
/// which would alias the state, into a panic.
struct Global {
    state: UnsafeCell<Option<Posix<RtSys>>>,
    busy: Cell<bool>,
}

// SAFETY: a POSIX process on Zoraya has one thread (ADR-0100 item 5); nothing
// runs asynchronously on it (ADR-0004).
unsafe impl Sync for Global {}

static STATE: Global = Global {
    state: UnsafeCell::new(None),
    busy: Cell::new(false),
};

/// Runs `f` on the process's state; `None` before start-up.
fn with<R>(f: impl FnOnce(&mut Posix<RtSys>) -> R) -> Option<R> {
    assert!(!STATE.busy.replace(true), "zoraya-posix entered twice");
    // SAFETY: one thread, and `busy` makes this the only reference.
    let state = unsafe { &mut *STATE.state.get() };
    let r = state.as_mut().map(f);
    STATE.busy.set(false);
    r
}

unsafe extern "C" {
    fn __errno_location() -> *mut i32;
    fn fdopen(fd: i32, mode: *const c_char) -> *mut u8;
    fn __fmodeflags(mode: *const c_char) -> i32;
    fn longjmp(env: *mut u8, value: i32) -> !;
}

fn set_errno(e: i32) {
    // SAFETY: musl's `__errno_location` gives this thread's errno, always.
    unsafe { *__errno_location() = e };
}

/// C's `int` convention: the value, or -1 with `errno`.
fn int(r: Result<i32, i32>) -> i32 {
    r.unwrap_or_else(|e| {
        set_errno(e);
        -1
    })
}

fn unit(r: Result<(), i32>) -> i32 {
    int(r.map(|()| 0))
}

fn size(r: Result<usize, i32>) -> isize {
    match r {
        Ok(n) => n as isize,
        Err(e) => {
            set_errno(e);
            -1
        }
    }
}

/// The shim's convention (ADR-0141 item 2): the value or `-errno`.
fn raw(r: Result<i64, i32>) -> i64 {
    r.unwrap_or_else(|e| -i64::from(e))
}

fn state<R>(f: impl FnOnce(&mut Posix<RtSys>) -> Result<R, i32>) -> Result<R, i32> {
    with(f).unwrap_or(Err(ENOSYS))
}

/// The bytes of a C string, without its NUL.
///
/// # Safety
///
/// `p` is NULL or points at memory readable up to its NUL or `PATH_MAX`
/// bytes, as POSIX asks of a path argument.
unsafe fn c_str<'a>(p: *const c_char) -> Result<&'a [u8], i32> {
    if p.is_null() {
        return Err(EFAULT);
    }
    let mut n = 0;
    // SAFETY: the caller's contract; the scan stops at the NUL or PATH_MAX.
    while unsafe { *p.add(n) } != 0 {
        n += 1;
        if n >= PATH_MAX {
            return Err(ENAMETOOLONG);
        }
    }
    // SAFETY: the `n` bytes before the NUL were just read.
    Ok(unsafe { core::slice::from_raw_parts(p.cast::<u8>(), n) })
}

fn user(p: *const u8, n: usize) -> Result<User, i32> {
    if p.is_null() && n > 0 {
        return Err(EFAULT);
    }
    if n > isize::MAX as usize {
        return Err(EINVAL);
    }
    Ok(User::new(p as u64, n))
}

/// # Safety
///
/// `out` is NULL or writable for `bytes.len()` bytes.
unsafe fn put(out: *mut u8, bytes: &[u8]) -> Result<(), i32> {
    if out.is_null() {
        return Err(EFAULT);
    }
    // SAFETY: the caller's contract.
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()) };
    Ok(())
}

// ------------------------------------------------------------ memory

struct Objects;

// SAFETY: a mapping of a new memory object is page-aligned, readable,
// writable and this process's own; the handle and mapping stay for the
// process's life, and the kernel picks addresses that do not overlap.
unsafe impl Backing for Objects {
    fn grow(&mut self, pages: usize) -> Option<usize> {
        let size = (pages * PAGE_SIZE) as u64;
        let object = zoraya_rt::memory_create(size, MemoryFlags::NONE).ok()?;
        match zoraya_rt::memory_map(object, 0, size, MapFlags::WRITE) {
            Ok(address) => Some(address as usize),
            Err(_) => {
                let _ = zoraya_rt::handle_close(object);
                None
            }
        }
    }
}

struct ProcessHeap(UnsafeCell<Heap<Objects>>);

// SAFETY: one thread (ADR-0100 item 5).
unsafe impl Sync for ProcessHeap {}

// SAFETY: `Heap` returns blocks of the layout asked, or null.
unsafe impl GlobalAlloc for ProcessHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: one thread, and `Heap` calls nothing that allocates.
        unsafe { (*self.0.get()).alloc(layout) }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: as in `alloc`; the caller's contract is `Heap::dealloc`'s.
        unsafe { (*self.0.get()).dealloc(block, layout) }
    }
}

/// Rust's allocations and C's `malloc` share one heap over memory objects
/// (ADR-0100 item 3).
#[global_allocator]
static HEAP: ProcessHeap = ProcessHeap(UnsafeCell::new(Heap::new(Objects)));

/// `malloc`'s header: the size asked, so that `free` and `realloc` know it.
const HEADER: usize = 16;

fn layout(n: usize) -> Option<Layout> {
    Layout::from_size_align(n.checked_add(HEADER)?, 16).ok()
}

#[unsafe(no_mangle)]
extern "C" fn malloc(n: usize) -> *mut u8 {
    let Some(l) = layout(n) else {
        set_errno(ENOMEM);
        return ptr::null_mut();
    };
    // SAFETY: `l` has a non-zero size (the header).
    let block = unsafe { HEAP.alloc(l) };
    if block.is_null() {
        set_errno(ENOMEM);
        return block;
    }
    // SAFETY: the block holds `HEADER + n` bytes, 16-aligned.
    unsafe {
        block.cast::<usize>().write(n);
        block.add(HEADER)
    }
}

/// The size `malloc` was asked for `p`.
///
/// # Safety
///
/// `p` came from `malloc` and was not freed.
unsafe fn asked(p: *mut u8) -> usize {
    // SAFETY: the header is `HEADER` bytes before every block `malloc` gives.
    unsafe { p.sub(HEADER).cast::<usize>().read() }
}

#[unsafe(no_mangle)]
extern "C" fn free(p: *mut u8) {
    if p.is_null() {
        return;
    }
    // SAFETY: C's contract for `free`: `p` came from this `malloc`.
    unsafe {
        let n = asked(p);
        if let Some(l) = layout(n) {
            HEAP.dealloc(p.sub(HEADER), l);
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn calloc(count: usize, each: usize) -> *mut u8 {
    let Some(n) = count.checked_mul(each) else {
        set_errno(ENOMEM);
        return ptr::null_mut();
    };
    let p = malloc(n);
    if !p.is_null() {
        // SAFETY: `malloc` gave `n` bytes.
        unsafe { ptr::write_bytes(p, 0, n) };
    }
    p
}

#[unsafe(no_mangle)]
extern "C" fn realloc(p: *mut u8, n: usize) -> *mut u8 {
    if p.is_null() {
        return malloc(n);
    }
    let new = malloc(n);
    if new.is_null() {
        return new;
    }
    // SAFETY: `p` came from `malloc` (C's contract); both hold what is copied.
    unsafe {
        let old = asked(p);
        ptr::copy_nonoverlapping(p, new, old.min(n));
    }
    free(p);
    new
}

#[unsafe(no_mangle)]
extern "C" fn __libc_malloc(n: usize) -> *mut u8 {
    malloc(n)
}

#[unsafe(no_mangle)]
extern "C" fn __libc_calloc(count: usize, each: usize) -> *mut u8 {
    calloc(count, each)
}

#[unsafe(no_mangle)]
extern "C" fn __libc_free(p: *mut u8) {
    free(p)
}

/// Private anonymous mappings (ADR-0103 item 12), for `__init_tls` and
/// whatever else maps: a memory object each, recorded for `munmap`.
struct Mappings(UnsafeCell<[(u64, u64, u64); 64]>);

// SAFETY: one thread.
unsafe impl Sync for Mappings {}

static MAPPINGS: Mappings = Mappings(UnsafeCell::new([(0, 0, 0); 64]));

fn mappings() -> &'static mut [(u64, u64, u64); 64] {
    // SAFETY: one thread, and no caller holds the reference across another.
    unsafe { &mut *MAPPINGS.0.get() }
}

const MAP_SHARED: i64 = 0x01;
const MAP_PRIVATE: i64 = 0x02;
const MAP_FIXED: i64 = 0x10;
const MAP_ANONYMOUS: i64 = 0x20;

fn map(length: i64, flags: i64, fd: i64) -> Result<i64, i32> {
    if flags & MAP_SHARED != 0 || flags & MAP_ANONYMOUS == 0 || fd != -1 {
        // File and shared mappings are out of scope (ADR-0103 item 12).
        return Err(ENODEV);
    }
    if flags & MAP_PRIVATE == 0 || flags & MAP_FIXED != 0 || length <= 0 {
        return Err(EINVAL);
    }
    let size = (length as u64).next_multiple_of(PAGE_SIZE as u64);
    let free = mappings().iter().position(|m| m.1 == 0).ok_or(ENOMEM)?;
    let object = zoraya_rt::memory_create(size, MemoryFlags::NONE)
        .map_err(|s| errno::from_status(s, Call::Memory))?;
    match zoraya_rt::memory_map(object, 0, size, MapFlags::WRITE) {
        Ok(address) => {
            mappings()[free] = (address, size, object.raw());
            Ok(address as i64)
        }
        Err(s) => {
            let _ = zoraya_rt::handle_close(object);
            Err(errno::from_status(s, Call::Memory))
        }
    }
}

fn unmap(address: u64, length: u64) -> Result<(), i32> {
    let size = length.next_multiple_of(PAGE_SIZE as u64);
    // The kernel ends whole mappings only: a part of one is refused.
    let m = mappings()
        .iter_mut()
        .find(|m| m.1 != 0 && m.0 == address && m.1 == size)
        .ok_or(EINVAL)?;
    zoraya_rt::memory_unmap(m.0, m.1).map_err(|s| errno::from_status(s, Call::Memory))?;
    if let Some(h) = Handle::new(m.2) {
        let _ = zoraya_rt::handle_close(h);
    }
    *m = (0, 0, 0);
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_mmap(
    _a: i64,
    length: i64,
    _prot: i64,
    flags: i64,
    fd: i64,
    _off: i64,
) -> i64 {
    raw(map(length, flags, fd))
}

#[unsafe(no_mangle)]
extern "C" fn munmap(address: *mut u8, length: usize) -> i32 {
    unit(unmap(address as u64, length as u64))
}

/// The C name of the mapping call (proposed ADR-0210): musl's `mmap.c` is
/// not compiled, so the shim's target serves C callers too, with `MAP_FAILED`
/// and `errno` for an error.
#[unsafe(no_mangle)]
extern "C" fn mmap(a: *mut u8, length: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8 {
    let r = zoraya_posix_mmap(
        a as i64,
        length as i64,
        i64::from(prot),
        i64::from(flags),
        i64::from(fd),
        off,
    );
    if r < 0 {
        set_errno((-r) as i32);
        usize::MAX as *mut u8
    } else {
        r as *mut u8
    }
}

#[unsafe(no_mangle)]
extern "C" fn __munmap(address: *mut u8, length: usize) -> i32 {
    munmap(address, length)
}

/// ADR-0100 item 6: no permission changes on mappings.
#[unsafe(no_mangle)]
extern "C" fn mprotect(_a: *mut u8, _n: usize, _prot: i32) -> i32 {
    int(Err(ENOSYS))
}

// ------------------------------------------------------------ start-up

const AT_NULL: usize = 0;
const AT_PHDR: usize = 3;
const AT_PHENT: usize = 4;
const AT_PHNUM: usize = 5;
const AT_PAGESZ: usize = 6;
const AT_RANDOM: usize = 25;
const PT_PHDR: u32 = 6;
const PHENT: usize = 56;

/// The handle a spawner put for fd `fd` (generation 0, as `spawn` fills):
/// at its number, fd 0's at `FD0`.
fn at_slot(fd: u32) -> Handle {
    Handle::from_parts(slot::of_fd(fd), 0).unwrap_or(slot::BOOTSTRAP)
}

/// Reads the bootstrap channel's two messages, if the process has one
/// (ADR-0107 item 5), and closes it.
fn bootstrap(sys: &mut RtSys) -> Posix<RtSys> {
    let mut record = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
    let mut handles = Vec::new();
    let read = |sys: &mut RtSys, bytes: &mut [u8], handles: &mut Vec<Handle>| loop {
        match sys.channel_read(slot::BOOTSTRAP, bytes, handles) {
            Err(Status::WouldBlock) => {
                use crate::sys::signals::*;
                if sys.wait(slot::BOOTSTRAP, READABLE | PEER_CLOSED).is_err() {
                    return None;
                }
            }
            Ok(n) => return Some(n),
            Err(_) => return None,
        }
    };
    let report = zoraya_rt::handle_type(slot::REPORT)
        .ok()
        .map(|_| slot::REPORT);
    let facts = abi::Facts::default();
    let had_channel = zoraya_rt::handle_type(slot::BOOTSTRAP).is_ok();
    let Some(n) = read(sys, &mut record, &mut handles) else {
        if had_channel {
            // The spawner gave a channel and no record: it failed after the
            // spawn (procs::send_record).
            zoraya_rt::process_exit(127);
        }
        return Posix::new(
            RtSys {
                port: sys.port.take(),
            },
            facts,
        );
    };
    let decoded = Bootstrap::decode(&record[..n], handles.len());
    // The second message: SELF, which a fork child gets otherwise.
    let mut second = [0u8; 16];
    let mut self_handle = Vec::new();
    let kept = match read(sys, &mut second, &mut self_handle) {
        Some(4) if &second[..4] == SELF_MESSAGE && self_handle.len() == 1 => self_handle.pop(),
        _ => None,
    };
    for h in self_handle.drain(..) {
        let _ = zoraya_rt::handle_close(h);
    }
    let _ = zoraya_rt::handle_close(slot::BOOTSTRAP);
    let sys = RtSys {
        port: sys.port.take(),
    };
    match decoded {
        Some(record) => {
            let mut p = Posix::from_bootstrap(sys, &record, &handles, at_slot, report);
            p.self_handle = kept;
            p
        }
        None => {
            for h in handles.into_iter().chain(kept) {
                let _ = zoraya_rt::handle_close(h);
            }
            Posix::new(sys, facts)
        }
    }
}

/// Called by `crt1.o`'s `_start` (ADR-0109 item 4) with musl's `_start_c`:
/// lays out `argc`, `argv`, `envp` and the auxiliary vector as `_start_c`
/// expects, sets up the state from the bootstrap record, and starts musl.
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_start(_stack: usize, start_c: extern "C" fn(*mut usize) -> !) -> ! {
    let block = zoraya_rt::startup();
    // The strings, NUL-terminated, in memory the program may write.
    let mut strings: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    for s in block.argv.chain(block.envp) {
        offsets.push(strings.len());
        strings.extend_from_slice(s);
        strings.push(0);
    }
    let strings = strings.leak();
    let base = strings.as_ptr() as usize;
    let argc = block.argv.len();
    let headers = block.program_headers;
    // AT_PHDR: the loaded copy of the table if PT_PHDR names it, so that
    // musl's base (AT_PHDR - PT_PHDR's address) is 0 as for every static
    // executable; else the startup block's copy (ADR-0180).
    let mut phdr = headers.as_ptr() as usize;
    for h in headers.as_chunks::<PHENT>().0 {
        if u32::from_le_bytes([h[0], h[1], h[2], h[3]]) == PT_PHDR {
            phdr = usize::from_le_bytes(h[16..24].try_into().unwrap_or([0; 8]));
        }
    }
    let random: &mut [u8; 16] = Box::leak(Box::new([0; 16]));
    let seed = zoraya_rt::clock_monotonic() ^ (random.as_ptr() as u64).rotate_left(17);
    random[..8].copy_from_slice(&seed.to_le_bytes());
    random[8..].copy_from_slice(&seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).to_le_bytes());
    let mut vector: Vec<usize> = Vec::with_capacity(offsets.len() + 16);
    vector.push(argc);
    for (i, at) in offsets.iter().enumerate() {
        if i == argc {
            vector.push(0);
        }
        vector.push(base + at);
    }
    if offsets.len() == argc {
        vector.push(0);
    }
    vector.push(0);
    for (key, value) in [
        (AT_PAGESZ, PAGE_SIZE),
        (AT_PHDR, phdr),
        (AT_PHENT, PHENT),
        (AT_PHNUM, headers.len() / PHENT),
        (AT_RANDOM, random.as_ptr() as usize),
        (AT_NULL, 0),
    ] {
        vector.push(key);
        vector.push(value);
    }
    let mut sys = RtSys { port: None };
    let mut posix = bootstrap(&mut sys);
    procs::start_up(&mut posix);
    // SAFETY: start-up runs once, before anything else uses the state.
    unsafe { *STATE.state.get() = Some(posix) };
    let vector = vector.leak();
    // `vector` is laid out as `_start_c` reads it (argc, argv, NULL, envp,
    // NULL, auxv), and lives for the process's life.
    start_c(vector.as_mut_ptr())
}

/// With `ZORAYA_POSIX_STATS` in the environment, the library's counters go
/// to fd 2 at exit, one line, for the task's measurements (ADR-0180; the
/// broker's statistics of ADR-0110 item 7 are 6d's and 6e's).
fn print_stats() {
    let wanted = zoraya_rt::startup()
        .envp
        .any(|e| e.starts_with(b"ZORAYA_POSIX_STATS="));
    if !wanted {
        return;
    }
    let _ = with(|p| {
        use core::fmt::Write;
        let s = p.stats;
        let mut line = String::new();
        let mean = s.resolve_ns.checked_div(s.resolutions).unwrap_or(0);
        let _ = writeln!(
            line,
            "POSIX-STATS resolutions={} resolve_ns_mean={} requests={} seeks={} shares={} reports={} broker={} takes={}",
            s.resolutions, mean, s.requests, s.seeks, s.shares, s.reports, s.broker, s.takes
        );
        let mean = |total: u64, n: u64| total.checked_div(n).unwrap_or(0);
        if s.prepares > 0 || s.forks > 0 {
            let _ = writeln!(
                line,
                "POSIX-PROCESS prepares={} prepare_ns={} commits={} commit_ns={} reaps={} reap_ns={} forks={} fork_ns={} snapshot_ns={}",
                s.prepares,
                mean(s.prepare_ns, s.prepares),
                s.commits,
                mean(s.commit_ns, s.commits),
                s.reaps,
                mean(s.reap_ns, s.reaps),
                s.forks,
                mean(s.fork_ns, s.forks),
                mean(s.snapshot_ns, s.forks)
            );
        }
        let _ = p.write(2, User::new(line.as_ptr() as u64, line.len()));
    });
}

// ------------------------------------------------------------ the report

#[cfg(feature = "stats")]
unsafe extern "C" {
    /// The call log's bitmap, one byte a function (`calls.S`, made by
    /// `posix-surface-check calllog` with `libc.a`).
    static mut __zoraya_calls: [u8; 0];
    static __zoraya_calls_count: u32;
    /// The hash of the bitmap's order (posix-surface-check's `list_hash`).
    static __zoraya_calls_list: u64;
}

/// The call log's bytes.
#[cfg(feature = "stats")]
fn called() -> &'static mut [u8] {
    // SAFETY: `calls.S` defines `__zoraya_calls` as `__zoraya_calls_count`
    // bytes for the life of the process. One thread: the wrappers' stores
    // and this slice never run at once, and only the report and the fork
    // child's restart take the slice, each for one call.
    unsafe {
        core::slice::from_raw_parts_mut(
            (&raw mut __zoraya_calls).cast::<u8>(),
            ptr::read(&raw const __zoraya_calls_count) as usize,
        )
    }
}

/// ADR-0110 item 7: the counters and the call log to the broker, before an
/// exit or an `exec`'s hand-over; then counting starts again, so that an
/// `exec` that fails afterwards reports only what came later.
#[cfg(feature = "stats")]
fn report_stats(p: &mut Posix<RtSys>) {
    let calls = process::pack_calls(called());
    // SAFETY: `calls.S` defines `__zoraya_calls_list`, read-only data.
    p.stats.calls_list = unsafe { ptr::read(&raw const __zoraya_calls_list) };
    let counters = p.stats.encode(
        zoraya_proto_posix::stats::NAMES,
        zoraya_rt::take_syscalls(),
        p.fds.peak() as u64,
    );
    let _ = p.report_stats(&counters, &calls);
    restart_stats(p);
}

/// Counting from zero: after a report, and in a `fork` child, whose copy of
/// the parent's counters is the parent's to report. `inherit_failed_at`
/// goes too: a later failure's debug line must not show an earlier one's
/// fd (the TASK-22 mutants).
#[cfg(feature = "stats")]
fn restart_stats(p: &mut Posix<RtSys>) {
    p.stats = process::Stats::default();
    called().fill(0);
    p.fds.restart_peak();
    let _ = zoraya_rt::take_syscalls();
}

#[cfg(not(feature = "stats"))]
fn report_stats(_p: &mut Posix<RtSys>) {}

#[cfg(not(feature = "stats"))]
fn restart_stats(_p: &mut Posix<RtSys>) {}

/// Ends the process with `code` (`_exit`, `_Exit`, and the shim's
/// `exit_group`).
fn exit(code: i64) -> ! {
    print_stats();
    let _ = with(report_stats);
    zoraya_rt::process_exit((code & 0xff) as i32)
}

#[unsafe(no_mangle)]
extern "C" fn _exit(code: i32) -> ! {
    exit(i64::from(code))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_exit_group(code: i64) -> ! {
    exit(code)
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_set_tid_address(_p: i64) -> i64 {
    // musl keeps it as the thread's id: the process's, or 1 without one.
    i64::from(with(|p| p.facts.pid).unwrap_or(0).max(1))
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    zoraya_rt::process_exit_terminated(6)
}

// ------------------------------------------------------------ files

#[unsafe(no_mangle)]
extern "C" fn read(fd: i32, buf: *mut u8, n: usize) -> isize {
    size(user(buf, n).and_then(|u| procs::read_fd(fd, u)))
}

#[unsafe(no_mangle)]
extern "C" fn write(fd: i32, buf: *const u8, n: usize) -> isize {
    size(user(buf, n).and_then(|u| procs::write_fd(fd, u)))
}

/// `struct iovec`s as `User`s.
///
/// # Safety
///
/// `iov` is NULL or points at `count` `struct iovec`s.
unsafe fn vectors(iov: *const [usize; 2], count: i64) -> Result<Vec<User>, i32> {
    if !(0..=1024).contains(&count) {
        return Err(EINVAL);
    }
    if count > 0 && iov.is_null() {
        return Err(EFAULT);
    }
    let mut out = Vec::new();
    let mut total = 0usize;
    for i in 0..count as usize {
        // SAFETY: the caller's contract.
        let [address, length] = unsafe { iov.add(i).read() };
        total = total
            .checked_add(length)
            .filter(|&t| t <= isize::MAX as usize)
            .ok_or(EINVAL)?;
        out.push(user(address as *const u8, length)?);
    }
    Ok(out)
}

fn readv_all(fd: i32, parts: Vec<User>) -> Result<usize, i32> {
    let mut done = 0;
    for u in parts {
        match procs::read_fd(fd, u) {
            Ok(n) => {
                done += n;
                if n < u.length {
                    break;
                }
            }
            Err(e) if done == 0 => return Err(e),
            Err(_) => break,
        }
    }
    Ok(done)
}

/// The largest `writev` gathered into one write: what one request carries.
const GATHER: usize = zoraya_proto_file::MAX_INLINE;

fn writev_all(fd: i32, parts: Vec<User>) -> Result<usize, i32> {
    // One write for what fits one request, so that musl's two iovecs of a
    // line are one append (ADR-0010 rule 2), as writev is on Linux.
    let total: usize = parts.iter().map(|u| u.length).sum();
    // Files only: a stream takes each piece's pointer to the kernel, which
    // checks it (EFAULT), and a piece of at most 4 KiB is atomic there.
    let object = with(|p| p.is_object(fd)).unwrap_or(false);
    if object && parts.len() > 1 && total <= GATHER {
        let mut joined = Vec::new();
        if joined.try_reserve_exact(total).is_ok() {
            for u in &parts {
                let at = joined.len();
                joined.resize(at + u.length, 0);
                // SAFETY: as `RtSys::copy_in`: memory the program passed.
                unsafe {
                    ptr::copy_nonoverlapping(
                        u.address as *const u8,
                        joined[at..].as_mut_ptr(),
                        u.length,
                    )
                };
            }
            return procs::write_fd(fd, User::new(joined.as_ptr() as u64, joined.len()));
        }
    }
    let mut done = 0;
    for u in parts {
        match procs::write_fd(fd, u) {
            Ok(n) => {
                done += n;
                if n < u.length {
                    break;
                }
            }
            Err(e) if done == 0 => return Err(e),
            Err(_) => break,
        }
    }
    Ok(done)
}

#[unsafe(no_mangle)]
extern "C" fn writev(fd: i32, iov: *const [usize; 2], count: i32) -> isize {
    // SAFETY: C's contract for writev.
    size(unsafe { vectors(iov, i64::from(count)) }.and_then(|v| writev_all(fd, v)))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_read(fd: i64, buf: i64, n: i64) -> i64 {
    raw(user(buf as *const u8, n as usize)
        .and_then(|u| procs::read_fd(fd as i32, u))
        .map(|n| n as i64))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_readv(fd: i64, iov: i64, count: i64) -> i64 {
    // SAFETY: musl's stdio passes its own iovec array.
    let r =
        unsafe { vectors(iov as *const [usize; 2], count) }.and_then(|v| readv_all(fd as i32, v));
    raw(r.map(|n| n as i64))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_writev(fd: i64, iov: i64, count: i64) -> i64 {
    // SAFETY: musl's stdio passes its own iovec array.
    let r =
        unsafe { vectors(iov as *const [usize; 2], count) }.and_then(|v| writev_all(fd as i32, v));
    raw(r.map(|n| n as i64))
}

#[unsafe(no_mangle)]
extern "C" fn close(fd: i32) -> i32 {
    unit(state(|p| p.close(fd)))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_close(fd: i64) -> i64 {
    raw(state(|p| p.close(fd as i32)).map(|()| 0))
}

#[unsafe(no_mangle)]
extern "C" fn open(path: *const c_char, flags: i32, mode: u32) -> i32 {
    openat(AT_FDCWD, path, flags, mode)
}

#[unsafe(no_mangle)]
extern "C" fn openat(dirfd: i32, path: *const c_char, flags: i32, mode: u32) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    int(path.and_then(|path| state(|p| p.open(dirfd, path, flags as u32, mode))))
}

#[unsafe(no_mangle)]
extern "C" fn lseek(fd: i32, offset: i64, whence: i32) -> i64 {
    match state(|p| p.lseek(fd, offset, whence)) {
        Ok(at) => at,
        Err(e) => {
            set_errno(e);
            -1
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn __lseek(fd: i32, offset: i64, whence: i32) -> i64 {
    lseek(fd, offset, whence)
}

#[unsafe(no_mangle)]
extern "C" fn ftruncate(fd: i32, length: i64) -> i32 {
    unit(state(|p| p.ftruncate(fd, length)))
}

#[unsafe(no_mangle)]
extern "C" fn fstat(fd: i32, out: *mut u8) -> i32 {
    let r = state(|p| p.fstat(fd));
    // SAFETY: C's contract: `out` is a `struct stat`.
    unit(r.and_then(|st| unsafe { put(out, &st) }))
}

#[unsafe(no_mangle)]
extern "C" fn fstatat(dirfd: i32, path: *const c_char, out: *mut u8, flags: i32) -> i32 {
    // SAFETY: C's contract: `path` is a string, `out` a `struct stat`.
    unsafe {
        let r = c_str(path).and_then(|path| state(|p| p.stat_at(dirfd, path, flags as u32)));
        unit(r.and_then(|st| put(out, &st)))
    }
}

#[unsafe(no_mangle)]
extern "C" fn stat(path: *const c_char, out: *mut u8) -> i32 {
    fstatat(AT_FDCWD, path, out, 0)
}

#[unsafe(no_mangle)]
extern "C" fn lstat(path: *const c_char, out: *mut u8) -> i32 {
    fstatat(AT_FDCWD, path, out, process::AT_SYMLINK_NOFOLLOW as i32)
}

#[unsafe(no_mangle)]
extern "C" fn faccessat(dirfd: i32, path: *const c_char, amode: i32, flags: i32) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    unit(path.and_then(|path| state(|p| p.access_at(dirfd, path, amode as u32, flags as u32))))
}

#[unsafe(no_mangle)]
extern "C" fn access(path: *const c_char, amode: i32) -> i32 {
    faccessat(AT_FDCWD, path, amode, 0)
}

/// The effective identity is the real one: there is no set-uid (ADR-0005).
#[unsafe(no_mangle)]
extern "C" fn eaccess(path: *const c_char, amode: i32) -> i32 {
    faccessat(AT_FDCWD, path, amode, process::AT_EACCESS as i32)
}

#[unsafe(no_mangle)]
extern "C" fn mkdirat(dirfd: i32, path: *const c_char, mode: u32) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    unit(path.and_then(|path| state(|p| p.mkdir_at(dirfd, path, mode))))
}

#[unsafe(no_mangle)]
extern "C" fn mkdir(path: *const c_char, mode: u32) -> i32 {
    mkdirat(AT_FDCWD, path, mode)
}

#[unsafe(no_mangle)]
extern "C" fn unlinkat(dirfd: i32, path: *const c_char, flags: i32) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    unit(path.and_then(|path| state(|p| p.unlink_at(dirfd, path, flags as u32))))
}

#[unsafe(no_mangle)]
extern "C" fn unlink(path: *const c_char) -> i32 {
    unlinkat(AT_FDCWD, path, 0)
}

/// Proposed ADR-0210: `unlinkat` with `AT_REMOVEDIR`, as musl's `rmdir.c`.
#[unsafe(no_mangle)]
extern "C" fn rmdir(path: *const c_char) -> i32 {
    unlinkat(AT_FDCWD, path, process::AT_REMOVEDIR as i32)
}

#[unsafe(no_mangle)]
extern "C" fn remove(path: *const c_char) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    unit(path.and_then(|path| state(|p| p.remove(path))))
}

#[unsafe(no_mangle)]
extern "C" fn rename(old: *const c_char, new: *const c_char) -> i32 {
    // SAFETY: C's contract: both are strings.
    let (old, new) = unsafe { (c_str(old), c_str(new)) };
    unit(old.and_then(|old| new.and_then(|new| state(|p| p.rename(old, new)))))
}

#[unsafe(no_mangle)]
extern "C" fn symlinkat(target: *const c_char, dirfd: i32, path: *const c_char) -> i32 {
    // SAFETY: C's contract: both are strings.
    let (target, path) = unsafe { (c_str(target), c_str(path)) };
    unit(target.and_then(|t| path.and_then(|path| state(|p| p.symlink_at(t, dirfd, path)))))
}

/// Proposed ADR-0210: `symlinkat` from the working directory.
#[unsafe(no_mangle)]
extern "C" fn symlink(target: *const c_char, path: *const c_char) -> i32 {
    symlinkat(target, AT_FDCWD, path)
}

#[unsafe(no_mangle)]
extern "C" fn readlinkat(dirfd: i32, path: *const c_char, buf: *mut u8, n: usize) -> isize {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    let r = path.and_then(|path| state(|p| p.readlink_at(dirfd, path)));
    size(r.and_then(|target| {
        // readlink truncates and writes no NUL.
        let k = target.len().min(n);
        // SAFETY: C's contract: `buf` holds `n` bytes.
        unsafe { put(buf, &target.as_bytes()[..k]) }.map(|()| k)
    }))
}

#[unsafe(no_mangle)]
extern "C" fn readlink(path: *const c_char, buf: *mut u8, n: usize) -> isize {
    readlinkat(AT_FDCWD, path, buf, n)
}

#[unsafe(no_mangle)]
extern "C" fn linkat(
    olddirfd: i32,
    old: *const c_char,
    newdirfd: i32,
    new: *const c_char,
    flags: i32,
) -> i32 {
    // SAFETY: C's contract: both are strings.
    let (old, new) = unsafe { (c_str(old), c_str(new)) };
    unit(old.and_then(|old| {
        // ADR-0100 item 6: only the form that publishes an unnamed file.
        if flags as u32 & AT_EMPTY_PATH == 0 || !old.is_empty() {
            return Err(EPERM);
        }
        new.and_then(|new| state(|p| p.publish(olddirfd, newdirfd, new)))
    }))
}

#[unsafe(no_mangle)]
extern "C" fn fchmod(fd: i32, mode: u32) -> i32 {
    unit(state(|p| p.fchmod(fd, mode)))
}

#[unsafe(no_mangle)]
extern "C" fn fchmodat(dirfd: i32, path: *const c_char, mode: u32, flags: i32) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    unit(path.and_then(|path| state(|p| p.fchmod_at(dirfd, path, mode, flags as u32))))
}

/// Proposed ADR-0210: `fchmodat` from the working directory.
#[unsafe(no_mangle)]
extern "C" fn chmod(path: *const c_char, mode: u32) -> i32 {
    fchmodat(AT_FDCWD, path, mode, 0)
}

/// Proposed ADR-0210 item 3: see `Posix::fsync`.
#[unsafe(no_mangle)]
extern "C" fn fsync(fd: i32) -> i32 {
    unit(state(|p| p.fsync(fd)))
}

/// ADR-0014 rule 5: the session's own ids succeed and change nothing.
fn chown_ok(p: &Posix<RtSys>, uid: u32, gid: u32) -> Result<(), i32> {
    let same = |want: u32, have: u32| want == u32::MAX || want == have;
    if same(uid, p.facts.uid) && same(gid, p.facts.gid) {
        Ok(())
    } else {
        Err(EPERM)
    }
}

#[unsafe(no_mangle)]
extern "C" fn fchown(fd: i32, uid: u32, gid: u32) -> i32 {
    unit(state(|p| {
        p.fstat(fd)?;
        chown_ok(p, uid, gid)
    }))
}

#[unsafe(no_mangle)]
extern "C" fn fchownat(dirfd: i32, path: *const c_char, uid: u32, gid: u32, flags: i32) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    unit(path.and_then(|path| {
        state(|p| {
            p.stat_at(dirfd, path, flags as u32)?;
            chown_ok(p, uid, gid)
        })
    }))
}

const UTIME_NOW: i64 = (1 << 30) - 1;
const UTIME_OMIT: i64 = (1 << 30) - 2;

/// The modification time `times` asks; `None` to leave it.
///
/// # Safety
///
/// `times` is NULL or points at two `struct timespec`s.
unsafe fn mtime_of(times: *const [i64; 2]) -> Option<(i64, u32)> {
    let now = || {
        let t = zoraya_rt::clock_monotonic();
        ((t / 1_000_000_000) as i64, (t % 1_000_000_000) as u32)
    };
    if times.is_null() {
        return Some(now());
    }
    // SAFETY: the caller's contract; [1] is the modification time.
    let [seconds, nanos] = unsafe { times.add(1).read() };
    match nanos {
        UTIME_OMIT => None,
        UTIME_NOW => Some(now()),
        n => Some((seconds, u32::try_from(n).unwrap_or(u32::MAX))),
    }
}

#[unsafe(no_mangle)]
extern "C" fn utimensat(
    dirfd: i32,
    path: *const c_char,
    times: *const [i64; 2],
    flags: i32,
) -> i32 {
    // SAFETY: C's contract for utimensat.
    let (path, mtime) = unsafe { (c_str(path), mtime_of(times)) };
    unit(
        path.and_then(|path| {
            state(|p| p.set_mtime(Target::Path(dirfd, path), mtime, flags as u32))
        }),
    )
}

#[unsafe(no_mangle)]
extern "C" fn futimens(fd: i32, times: *const [i64; 2]) -> i32 {
    // SAFETY: C's contract for futimens.
    let mtime = unsafe { mtime_of(times) };
    unit(state(|p| p.set_mtime(Target::Fd(fd), mtime, 0)))
}

/// Proposed ADR-0210: `utimensat` with the two `struct timeval`s turned into
/// `struct timespec`s; NULL is now, as for `utimensat`.
#[unsafe(no_mangle)]
extern "C" fn utimes(path: *const c_char, times: *const [i64; 2]) -> i32 {
    if times.is_null() {
        return utimensat(AT_FDCWD, path, ptr::null(), 0);
    }
    // SAFETY: C's contract: `times` is NULL or two `struct timeval`s.
    let [a, m] = unsafe { [times.read(), times.add(1).read()] };
    let micros = |t: [i64; 2]| {
        if (0..1_000_000).contains(&t[1]) {
            Ok([t[0], t[1] * 1000])
        } else {
            Err(EINVAL)
        }
    };
    match (micros(a), micros(m)) {
        (Ok(a), Ok(m)) => utimensat(AT_FDCWD, path, [a, m].as_ptr(), 0),
        _ => int(Err(EINVAL)),
    }
}

// ------------------------------------------------------------ directories

#[unsafe(no_mangle)]
extern "C" fn chdir(path: *const c_char) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = unsafe { c_str(path) };
    unit(path.and_then(|path| state(|p| p.chdir(path))))
}

#[unsafe(no_mangle)]
extern "C" fn getcwd(buf: *mut u8, n: usize) -> *mut u8 {
    let cwd: String = with(|p| p.getcwd()).unwrap_or_else(|| String::from("/"));
    let needed = cwd.len() + 1;
    let out = if buf.is_null() {
        // The common extension (musl's too): a buffer of its own.
        let p = malloc(needed.max(n));
        if p.is_null() {
            return p;
        }
        p
    } else if n == 0 {
        set_errno(EINVAL);
        return ptr::null_mut();
    } else if n < needed {
        set_errno(ERANGE);
        return ptr::null_mut();
    } else {
        buf
    };
    // SAFETY: `out` holds at least `needed` bytes, checked or allocated.
    unsafe {
        ptr::copy_nonoverlapping(cwd.as_ptr(), out, cwd.len());
        out.add(cwd.len()).write(0);
    }
    out
}

/// musl's AArch64 `struct dirent`: ino, off, reclen, type, name[256].
const DIRENT: usize = 280;

/// `DIR`, which the program never looks into.
#[repr(C, align(8))]
struct Dir {
    entry: [u8; DIRENT],
    fd: i32,
}

#[unsafe(no_mangle)]
extern "C" fn fdopendir(fd: i32) -> *mut Dir {
    if let Err(e) = state(|p| p.is_directory(fd)) {
        set_errno(e);
        return ptr::null_mut();
    }
    Box::into_raw(Box::new(Dir {
        entry: [0; DIRENT],
        fd,
    }))
}

#[unsafe(no_mangle)]
extern "C" fn opendir(path: *const c_char) -> *mut Dir {
    let fd = open(path, (o::RDONLY | o::DIRECTORY | o::CLOEXEC) as i32, 0);
    if fd < 0 {
        return ptr::null_mut();
    }
    let dir = fdopendir(fd);
    if dir.is_null() {
        close(fd);
    }
    dir
}

#[unsafe(no_mangle)]
extern "C" fn readdir(dir: *mut Dir) -> *mut u8 {
    if dir.is_null() {
        set_errno(EBADF);
        return ptr::null_mut();
    }
    // SAFETY: `dir` came from `fdopendir` (C's contract) and is not shared.
    let dir = unsafe { &mut *dir };
    match state(|p| p.read_dir(dir.fd)) {
        Ok(Some((name, kind, id))) => {
            let n = name.len().min(255);
            dir.entry = [0; DIRENT];
            dir.entry[..8].copy_from_slice(&id.to_le_bytes());
            // d_off is unspecified (ADR-0180 item 11): no telldir or seekdir.
            dir.entry[16..18].copy_from_slice(&(DIRENT as u16).to_le_bytes());
            dir.entry[18] = kind;
            dir.entry[19..19 + n].copy_from_slice(&name.as_bytes()[..n]);
            dir.entry.as_mut_ptr()
        }
        // The end leaves errno as it was.
        Ok(None) => ptr::null_mut(),
        Err(e) => {
            set_errno(e);
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn closedir(dir: *mut Dir) -> i32 {
    if dir.is_null() {
        return int(Err(EBADF));
    }
    // SAFETY: `dir` came from `fdopendir`, which boxed it (C's contract).
    let dir = unsafe { Box::from_raw(dir) };
    close(dir.fd)
}

// ------------------------------------------------------------ descriptors

#[unsafe(no_mangle)]
extern "C" fn dup(fd: i32) -> i32 {
    int(state(|p| p.dup(fd)))
}

#[unsafe(no_mangle)]
extern "C" fn dup2(old: i32, new: i32) -> i32 {
    int(state(|p| p.dup3(old, new, 0, false)))
}

#[unsafe(no_mangle)]
extern "C" fn __dup3(old: i32, new: i32, flags: i32) -> i32 {
    int(state(|p| p.dup3(old, new, flags as u32, true)))
}

#[unsafe(no_mangle)]
extern "C" fn fcntl(fd: i32, command: i32, argument: i64) -> i32 {
    int(state(|p| p.fcntl(fd, command, argument)))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_fcntl(fd: i64, command: i64, argument: i64) -> i64 {
    raw(state(|p| p.fcntl(fd as i32, command as i32, argument)).map(i64::from))
}

/// A terminal's requests (ADR-0105 item 7); `ENOTTY` on anything else,
/// `TIOCGWINSZ` (musl's stdio asks it of stdout) among the rest.
#[unsafe(no_mangle)]
extern "C" fn ioctl(fd: i32, request: i64, argument: i64) -> i32 {
    int(procs::ioctl_tty(fd, request, argument))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_ioctl(fd: i64, request: i64, argument: i64) -> i64 {
    raw(procs::ioctl_tty(fd as i32, request, argument).map(i64::from))
}

#[unsafe(no_mangle)]
extern "C" fn isatty(fd: i32) -> i32 {
    match state(|p| p.isatty(fd)) {
        Ok(true) => 1,
        Ok(false) => {
            set_errno(ENOTTY);
            0
        }
        Err(e) => {
            set_errno(e);
            0
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn pipe2(fds: *mut i32, flags: i32) -> i32 {
    if fds.is_null() {
        return int(Err(EFAULT));
    }
    unit(state(|p| p.pipe(flags as u32)).map(|(r, w)| {
        // SAFETY: C's contract: `fds` holds two ints.
        unsafe {
            fds.write(r);
            fds.add(1).write(w);
        }
    }))
}

#[unsafe(no_mangle)]
extern "C" fn pipe(fds: *mut i32) -> i32 {
    pipe2(fds, 0)
}

#[unsafe(no_mangle)]
extern "C" fn umask(mask: u32) -> u32 {
    with(|p| p.umask(mask)).unwrap_or(0o022)
}

/// `MFD_CLOEXEC`.
const MFD_CLOEXEC: u32 = 1;

#[unsafe(no_mangle)]
extern "C" fn memfd_create(_name: *const c_char, flags: u32) -> i32 {
    int(state(|p| p.unnamed(flags & MFD_CLOEXEC != 0)))
}

#[unsafe(no_mangle)]
extern "C" fn fopen(path: *const c_char, mode: *const c_char) -> *mut u8 {
    // musl's own fopen, over this open (src/stdio/fopen.c, which opens by
    // path and so is not retained, ADR-0141).
    // SAFETY: C's contract: `mode` is a string.
    let known = matches!(unsafe { c_str(mode) }, Ok([first, ..]) if b"rwa".contains(first));
    if !known {
        set_errno(EINVAL);
        return ptr::null_mut();
    }
    // SAFETY: `mode` is a string (checked above).
    let flags = unsafe { __fmodeflags(mode) };
    let fd = open(path, flags, 0o666);
    if fd < 0 {
        return ptr::null_mut();
    }
    // SAFETY: `fd` is open and `mode` a string.
    let file = unsafe { fdopen(fd, mode) };
    if file.is_null() {
        close(fd);
    }
    file
}

#[unsafe(no_mangle)]
extern "C" fn tmpfile() -> *mut u8 {
    let fd = int(state(|p| p.unnamed(true)));
    if fd < 0 {
        return ptr::null_mut();
    }
    // SAFETY: `fd` is open, and the mode a string.
    let file = unsafe { fdopen(fd, c"w+".as_ptr()) };
    if file.is_null() {
        close(fd);
    }
    file
}

/// FIFOs live on the temporary directory service (ADR-0107 item 13); every
/// other node type is `EPERM`.
#[unsafe(no_mangle)]
extern "C" fn mknodat(dirfd: i32, path: *const c_char, mode: u32, _dev: u64) -> i32 {
    // SAFETY: C's contract: `path` is a string.
    let path = match unsafe { c_str(path) } {
        Ok(p) => p,
        Err(e) => return int(Err(e)),
    };
    if mode & stat::S_IFMT == stat::S_IFIFO {
        unit(state(|p| p.mkfifo_at(dirfd, path, mode)))
    } else {
        int(Err(EPERM))
    }
}

#[unsafe(no_mangle)]
extern "C" fn mkfifo(path: *const c_char, mode: u32) -> i32 {
    mknodat(AT_FDCWD, path, stat::S_IFIFO | (mode & 0o777), 0)
}

#[unsafe(no_mangle)]
extern "C" fn fgetxattr(_fd: i32, _n: *const c_char, _v: *mut u8, _s: usize) -> isize {
    // ADR-0014 rule 8.
    size(Err(ENOTSUP))
}

#[unsafe(no_mangle)]
extern "C" fn flistxattr(_fd: i32, _l: *mut u8, _s: usize) -> isize {
    size(Err(ENOTSUP))
}

#[unsafe(no_mangle)]
extern "C" fn fsetxattr(_fd: i32, _n: *const c_char, _v: *const u8, _s: usize, _f: i32) -> i32 {
    int(Err(ENOTSUP))
}

// ------------------------------------------------------------ the process's facts

#[unsafe(no_mangle)]
extern "C" fn getpid() -> i32 {
    with(|p| p.facts.pid as i32).unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn getppid() -> i32 {
    with(|p| p.facts.ppid as i32).unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn getpgrp() -> i32 {
    with(|p| p.facts.pgid as i32).unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn getuid() -> u32 {
    with(|p| p.facts.uid).unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn geteuid() -> u32 {
    getuid()
}

#[unsafe(no_mangle)]
extern "C" fn getgid() -> u32 {
    with(|p| p.facts.gid).unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn getegid() -> u32 {
    getgid()
}

#[unsafe(no_mangle)]
extern "C" fn getlogin() -> *mut c_char {
    set_errno(ENXIO);
    ptr::null_mut()
}

#[unsafe(no_mangle)]
extern "C" fn ttyname(fd: i32) -> *mut c_char {
    set_errno(match state(|p| p.isatty(fd)) {
        Err(e) => e,
        Ok(_) => ENOTTY,
    });
    ptr::null_mut()
}

#[unsafe(no_mangle)]
extern "C" fn uname(out: *mut [u8; 65]) -> i32 {
    if out.is_null() {
        return int(Err(EFAULT));
    }
    // ADR-0107 item 11.
    let fields: [&str; 6] = [
        "Zoraya",
        "zoraya",
        env!("CARGO_PKG_VERSION"),
        "zoraya-posix",
        "aarch64",
        "",
    ];
    for (i, f) in fields.iter().enumerate() {
        let mut field = [0u8; 65];
        field[..f.len()].copy_from_slice(f.as_bytes());
        // SAFETY: C's contract: `out` is a `struct utsname`, six fields.
        unsafe { out.add(i).write(field) };
    }
    0
}

/// Proposed ADR-0210: `uname`'s node name, as musl's `gethostname.c`:
/// truncated to `len`, with a NUL when it fits.
#[unsafe(no_mangle)]
extern "C" fn gethostname(name: *mut u8, len: usize) -> i32 {
    let mut uts = [[0u8; 65]; 6];
    uname(uts.as_mut_ptr());
    let node = &uts[1];
    let n = node.iter().position(|&b| b == 0).unwrap_or(node.len());
    let k = len.min(n + 1);
    if k == 0 {
        return 0;
    }
    // SAFETY: C's contract: `name` holds `len` bytes, and `k <= len`.
    unsafe {
        ptr::copy_nonoverlapping(node.as_ptr(), name, k);
        if k == len {
            name.add(k - 1).write(0);
        }
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn sysconf(name: i32) -> i64 {
    match name {
        0 => 64 * 1024, // _SC_ARG_MAX: spawn's buffer (ADR-0042)
        1 => 255,       // _SC_CHILD_MAX: the live processes (ADR-0031)
        2 => 100,       // _SC_CLK_TCK
        3 => 0,         // _SC_NGROUPS_MAX
        4 => i64::from(abi::OPEN_MAX),
        30 => PAGE_SIZE as i64,
        43 => 2048,           // _SC_LINE_MAX
        60 => 1024,           // _SC_IOV_MAX
        69 | 70 => 4096,      // _SC_GET*_R_SIZE_MAX
        71 | 72 | 180 => 256, // name lengths
        83 | 84 => 1,         // processors: the kernel runs one
        173 => i64::from(crate::path::MAX_LINKS),
        _ => {
            set_errno(EINVAL);
            -1
        }
    }
}

const RLIMIT_STACK: i32 = 3;
const RLIMIT_NOFILE: i32 = 7;
const RLIM_INFINITY: u64 = u64::MAX;

/// ADR-0100 item 6: fixed limits.
fn limit(resource: i32) -> Result<u64, i32> {
    match resource {
        0..=15 => Ok(match resource {
            RLIMIT_STACK => zoraya_abi::spawn::USER_STACK_SIZE,
            RLIMIT_NOFILE => u64::from(abi::OPEN_MAX),
            _ => RLIM_INFINITY,
        }),
        _ => Err(EINVAL),
    }
}

#[unsafe(no_mangle)]
extern "C" fn getrlimit(resource: i32, out: *mut [u64; 2]) -> i32 {
    unit(limit(resource).and_then(|l| {
        if out.is_null() {
            return Err(EFAULT);
        }
        // SAFETY: C's contract: `out` is a `struct rlimit`.
        unsafe { out.write([l, l]) };
        Ok(())
    }))
}

/// A limit can be asked for at or below the fixed one, and nothing changes.
#[unsafe(no_mangle)]
extern "C" fn setrlimit(resource: i32, new: *const [u64; 2]) -> i32 {
    unit(limit(resource).and_then(|l| {
        if new.is_null() {
            return Err(EFAULT);
        }
        // SAFETY: C's contract: `new` is a `struct rlimit`.
        let [soft, hard] = unsafe { new.read() };
        if soft > hard || hard > l {
            Err(EPERM)
        } else {
            Ok(())
        }
    }))
}

#[unsafe(no_mangle)]
extern "C" fn getloadavg(out: *mut f64, n: i32) -> i32 {
    // ADR-0100 item 6: zero load.
    if n < 0 {
        return -1;
    }
    let n = n.min(3);
    for i in 0..n as usize {
        // SAFETY: C's contract: `out` holds `n` doubles.
        unsafe { out.add(i).write(0.0) };
    }
    n
}

#[unsafe(no_mangle)]
extern "C" fn htonl(x: u32) -> u32 {
    x.to_be()
}

// ------------------------------------------------------------ time

/// `CLOCK_REALTIME` is the boot's time base plus the monotonic clock, the
/// clock every `fsd` stamps files with (ADR-0191); the others are the
/// monotonic clock.
fn clock(id: i32) -> Result<u64, i32> {
    let now = zoraya_rt::clock_monotonic();
    match id {
        0 => Ok(with(|p| p.facts.realtime_base)
            .unwrap_or(0)
            .saturating_add(now)),
        1..=7 => Ok(now),
        _ => Err(EINVAL),
    }
}

#[unsafe(no_mangle)]
extern "C" fn clock_gettime(id: i32, out: *mut [i64; 2]) -> i32 {
    unit(clock(id).and_then(|t| {
        if out.is_null() {
            return Err(EFAULT);
        }
        // SAFETY: C's contract: `out` is a `struct timespec`.
        unsafe { out.write([(t / 1_000_000_000) as i64, (t % 1_000_000_000) as i64]) };
        Ok(())
    }))
}

#[unsafe(no_mangle)]
extern "C" fn __clock_gettime(id: i32, out: *mut [i64; 2]) -> i32 {
    clock_gettime(id, out)
}

const TIMER_ABSTIME: i32 = 1;

/// `nanosleep`'s core, which returns the error itself.
#[unsafe(no_mangle)]
extern "C" fn __clock_nanosleep(
    id: i32,
    flags: i32,
    request: *const [i64; 2],
    _rem: *mut [i64; 2],
) -> i32 {
    let now = match clock(id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if request.is_null() {
        return EFAULT;
    }
    // SAFETY: C's contract: `request` is a `struct timespec`.
    let [seconds, nanos] = unsafe { request.read() };
    if seconds < 0 || !(0..1_000_000_000).contains(&nanos) {
        return EINVAL;
    }
    let length = (seconds as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(nanos as u64);
    // The deadline on the monotonic clock: an absolute realtime one less
    // the base.
    let deadline = if flags & TIMER_ABSTIME != 0 {
        let base = now.saturating_sub(zoraya_rt::clock_monotonic());
        length.saturating_sub(base)
    } else {
        zoraya_rt::clock_monotonic().saturating_add(length)
    };
    procs::nanosleep_until(deadline, _rem)
}

#[unsafe(no_mangle)]
extern "C" fn times(out: *mut [i64; 4]) -> i64 {
    let ticks = (zoraya_rt::clock_monotonic() / 10_000_000) as i64;
    if !out.is_null() {
        // No accounting of CPU time: the elapsed time as user time.
        // SAFETY: C's contract: `out` is a `struct tms`.
        unsafe { out.write([ticks, 0, 0, 0]) };
    }
    ticks
}

const RUSAGE_SELF: i32 = 0;
const RUSAGE_CHILDREN: i32 = -1;
/// One thread: the thread's use is the process's (review R15).
const RUSAGE_THREAD: i32 = 1;

/// Proposed ADR-0210: no accounting of resources (as `wait3`): a zero
/// `struct rusage` (144 bytes) for the process, its one thread or its
/// children.
#[unsafe(no_mangle)]
extern "C" fn getrusage(who: i32, usage: *mut u8) -> i32 {
    if who != RUSAGE_SELF && who != RUSAGE_CHILDREN && who != RUSAGE_THREAD {
        return int(Err(EINVAL));
    }
    if usage.is_null() {
        return int(Err(EFAULT));
    }
    // SAFETY: C's contract: `usage` is a `struct rusage`.
    unsafe { ptr::write_bytes(usage, 0, 144) };
    0
}

// ------------------------------------------------------------ threads

/// ADR-0100 item 5: one thread, so a semaphore is a count (musl's `sem_t`
/// begins with it).
#[unsafe(no_mangle)]
extern "C" fn sem_init(sem: *mut i32, _shared: i32, value: u32) -> i32 {
    if sem.is_null() {
        return int(Err(EFAULT));
    }
    if value > i32::MAX as u32 {
        return int(Err(EINVAL));
    }
    // SAFETY: C's contract: `sem` is a `sem_t`.
    unsafe { sem.write(value as i32) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn sem_post(sem: *mut i32) -> i32 {
    if sem.is_null() {
        return int(Err(EFAULT));
    }
    // SAFETY: C's contract: `sem` was initialised.
    unsafe { sem.write(sem.read().saturating_add(1)) };
    0
}

/// A wait on a zero count would wait for ever with no other thread to post.
#[unsafe(no_mangle)]
extern "C" fn sem_wait(sem: *mut i32) -> i32 {
    if sem.is_null() {
        return int(Err(EFAULT));
    }
    // SAFETY: C's contract: `sem` was initialised.
    unsafe {
        let n = sem.read();
        if n <= 0 {
            return int(Err(35)); // EDEADLK
        }
        sem.write(n - 1);
    }
    0
}

/// No cancellation, one thread: the state is always "enabled" and nothing
/// changes (ADR-0141's open question, settled in ADR-0180).
#[unsafe(no_mangle)]
extern "C" fn pthread_setcancelstate(_new: i32, old: *mut i32) -> i32 {
    if !old.is_null() {
        // SAFETY: C's contract: `old` is an int.
        unsafe { old.write(0) };
    }
    0
}

/// One thread and no cancellation (ADR-0100 item 5): musl's `system.c`
/// calls it first; nothing is ever pending.
#[unsafe(no_mangle)]
extern "C" fn pthread_testcancel() {}

#[unsafe(no_mangle)]
extern "C" fn __lock(_l: *mut i32) {}

#[unsafe(no_mangle)]
extern "C" fn __unlock(_l: *mut i32) {}

#[unsafe(no_mangle)]
extern "C" fn _pthread_cleanup_push(_c: *mut u8, _f: usize, _x: *mut u8) {}

#[unsafe(no_mangle)]
extern "C" fn _pthread_cleanup_pop(_c: *mut u8, _run: i32) {}

#[unsafe(no_mangle)]
extern "C" fn __block_all_sigs(_set: *mut u8) {}

#[unsafe(no_mangle)]
extern "C" fn __restore_sigs(_set: *mut u8) {}

/// musl's default thread stack size, which `__init_tls` reads and may raise:
/// an atomic has `u32`'s layout and lives in writable memory.
#[unsafe(no_mangle)]
static __default_stacksize: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(131_072);

/// `siglongjmp` without the signal mask, which is 6d's: a `sigjmp_buf` begins
/// with a `jmp_buf`.
#[unsafe(no_mangle)]
extern "C" fn siglongjmp(env: *mut u8, value: i32) -> ! {
    // SAFETY: C's contract: `env` was set by `sigsetjmp`, whose buffer starts
    // with `setjmp`'s.
    unsafe { longjmp(env, value) }
}

// ------------------------------------------------------------ outside T1

/// Names of the tier list that no measured T1 program needs working
/// (sockets, `syscall`, `setsid`): each fails with `ENOSYS`, as ADR-0140's
/// stub did.
macro_rules! not_yet {
    (int $($name:ident)*) => {$(
        #[unsafe(no_mangle)]
        extern "C" fn $name() -> i32 {
            int(Err(ENOSYS))
        }
    )*};
    (ptr $($name:ident)*) => {$(
        #[unsafe(no_mangle)]
        extern "C" fn $name() -> *mut u8 {
            set_errno(ENOSYS);
            ptr::null_mut()
        }
    )*};
}

not_yet!(int syscall tee connect getrandom send sendmsg setgroups setsid socket);
not_yet!(ptr __map_file);

//! The C boundary: every name `libc.a` needs from the Rust half (ADR-0100,
//! ADR-0140's list in `ports/posix-stub/stubs.txt` before this crate), the
//! start-up of ADR-0109 item 4, `malloc` over memory objects, and the
//! system-call shim's targets (ADR-0141 item 2). The only module with
//! `unsafe` (INV-0100-3): C hands it pointers.
//!
//! Process functions (`fork`, `posix_spawn`, `wait*`, signals, `kill`, the
//! terminal) are stage 6d's, in `procs`. A process has threads (ADR-0300,
//! ADR-0302, which supersede ADR-0100 item 5): the state is a cell behind the
//! library lock, the heap and the mapping table behind a second lock, and
//! musl's own `src/thread` reaches the kernel through the shim targets
//! below (`zoraya_posix_futex`, `__clone`, `__unmapself`, thread exit).

mod procs;
mod sockets;
mod window;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ffi::c_char;
use core::ptr;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use zoraya_abi::posix::{
    self as abi, Bootstrap, EXT_NET, EXT_RANDOM, EXT_WINDOW, SELF_EXT_MESSAGE, SELF_MESSAGE,
    SELF_NET_MESSAGE, ext_handles, slot,
};
use zoraya_abi::{
    Buffer, Bytes, ChannelEntry, DEADLINE_INFINITE, Handle, Invoke, MapFlags, MemoryFlags,
    MemoryObject, Mode, OBJECT_TYPES, ObjectType, Packet, Status, stubs,
};
use zoraya_heap::{Backing, Heap, PAGE_SIZE};

use crate::errno::{self, *};
use crate::lock::{Lock, Wait};
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
        port_wait_any(port, sources, deadline)
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

    fn thread_self(&mut self) -> Option<Handle> {
        zoraya_rt::thread_self().ok()
    }

    fn interrupt(&mut self, h: Handle, port: Option<Handle>) {
        let _ = zoraya_rt::thread_interrupt(h);
        if let Some(port) = port {
            let _ = zoraya_rt::port_queue(port, USER_WAKE, 0);
        }
    }
}

/// The key of the packet that wakes a thread out of a port wait for another
/// thread's sake (ADR-0304 item 11); the subscriptions' keys are small.
const USER_WAKE: u64 = u64::MAX;

// ------------------------------------------------------------ the state

/// The kernel's wait on an address behind the lock's trait (ADR-0301).
struct Futex;

/// The futex calls this process made (`futex_wait`, `futex_wake`, through the
/// library lock or musl's): zero in a program with one thread (INV-0302-5).
static FUTEX_CALLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

impl Wait for Futex {
    fn wait(&self, word: &AtomicU32, expected: u32) {
        FUTEX_CALLS.fetch_add(1, Ordering::Relaxed);
        // Any return is an early return to the lock's loop, which re-checks.
        let _ = zoraya_rt::futex_wait(word as *const AtomicU32 as u64, expected, DEADLINE_INFINITE);
    }

    fn wake(&self, word: &AtomicU32) {
        FUTEX_CALLS.fetch_add(1, Ordering::Relaxed);
        let _ = zoraya_rt::futex_wake(word as *const AtomicU32 as u64, 1);
    }
}

/// This thread's thread pointer: zero before `__init_tls`, then unique to the
/// thread (its `struct pthread`).
fn thread_pointer() -> usize {
    let tp: usize;
    // SAFETY: reads a register, touches no memory.
    unsafe {
        core::arch::asm!("mrs {}, tpidr_el0", out(reg) tp, options(nomem, nostack, preserves_flags));
    }
    tp
}

/// No owner: the value of [`LIB_OWNER`] while the lock is free.
const NO_OWNER: usize = usize::MAX;

/// The library lock (ADR-0302 item 8): guards [`STATE`]. Order: this lock,
/// then [`MEM`]; never the other way.
static LIB: Lock = Lock::new();
static LIB_OWNER: AtomicUsize = AtomicUsize::new(NO_OWNER);

/// The memory lock (ADR-0302 item 8): the heap and the mapping table. The
/// heap never calls the library.
static MEM: Lock = Lock::new();

/// The memory lock across a `fork`'s snapshot (ADR-0306 item 1): taken by the
/// forking thread, which then allocates nothing until [`unlock_memory`].
fn lock_memory() {
    MEM.lock(&Futex);
}

/// Releases it; in the child (`child`) the copy of the lock is made free
/// instead: its holder, the forking thread, is the one running.
fn unlock_memory(child: bool) {
    if child {
        MEM.reset();
    } else {
        MEM.unlock(&Futex);
    }
}

/// Runs `f` with `lock` held.
fn locked<R>(lock: &Lock, f: impl FnOnce() -> R) -> R {
    lock.lock(&Futex);
    let r = f();
    lock.unlock(&Futex);
    r
}

struct Global {
    state: UnsafeCell<Option<Posix<RtSys>>>,
}

// SAFETY: the state is reached only through `with`, which holds `LIB`; two
// threads are never inside at once.
unsafe impl Sync for Global {}

static STATE: Global = Global {
    state: UnsafeCell::new(None),
};

/// Runs `f` on the process's state; `None` before start-up. A second entry by
/// the same thread (which would alias the state and deadlock on the lock) is
/// a panic, as it was with one thread.
fn with<R>(f: impl FnOnce(&mut Posix<RtSys>) -> R) -> Option<R> {
    let me = thread_pointer();
    assert!(
        LIB_OWNER.load(Ordering::Relaxed) != me,
        "zoraya-posix entered twice"
    );
    LIB.lock(&Futex);
    LIB_OWNER.store(me, Ordering::Relaxed);
    // SAFETY: `LIB` is held, so this is the only reference.
    let state = unsafe { &mut *STATE.state.get() };
    let r = state.as_mut().map(|p| {
        // The signal state is this thread's from here (ADR-0304 item 1).
        p.switch_to(me);
        f(p)
    });
    LIB_OWNER.store(NO_OWNER, Ordering::Relaxed);
    LIB.unlock(&Futex);
    r
}

/// How long the exit path waits for the library lock before it gives the
/// statistics up: a sibling's short broker round trip, not a wait.
const EXIT_LOCK_BUDGET_NS: u64 = 500_000_000;

/// As [`with`], but waits for the library lock for at most `budget_ns` (a
/// sleep of 1 ms between tries) and gives up after that: `None` then, and
/// before start-up. For the exit path, which must end the process even if a
/// sibling holds the lock for good (it is in a service call, ADR-0314 item
/// 2), but should not drop the ADR-0110 report because a sibling held it
/// for a moment.
fn try_with_for<R>(budget_ns: u64, f: impl FnOnce(&mut Posix<RtSys>) -> R) -> Option<R> {
    let me = thread_pointer();
    if LIB_OWNER.load(Ordering::Relaxed) == me {
        return None;
    }
    let end = zoraya_rt::clock_monotonic().saturating_add(budget_ns);
    while !LIB.try_lock() {
        let now = zoraya_rt::clock_monotonic();
        if now >= end {
            return None;
        }
        zoraya_rt::sleep_until(now.saturating_add(1_000_000).min(end));
    }
    LIB_OWNER.store(me, Ordering::Relaxed);
    // SAFETY: `LIB` is held, so this is the only reference.
    let state = unsafe { &mut *STATE.state.get() };
    let r = state.as_mut().map(f);
    LIB_OWNER.store(NO_OWNER, Ordering::Relaxed);
    LIB.unlock(&Futex);
    r
}

unsafe extern "C" {
    /// musl's (`cancel.c`): ends the calling thread if a cancel is pending
    /// and enabled.
    fn __testcancel();
    fn __zoraya_cancel_begin() -> i32;
    fn __zoraya_cancel_seen(old: i32) -> i32;
    fn __zoraya_cancel_end(old: i32);
    fn __errno_location() -> *mut i32;
    fn fdopen(fd: i32, mode: *const c_char) -> *mut u8;
    fn __fmodeflags(mode: *const c_char) -> i32;
    fn longjmp(env: *mut u8, value: i32) -> !;
}

/// A cancellation point of the calling thread; never holds a lock.
fn testcancel() {
    // SAFETY: `cancel.c`'s function; it only reads the thread's own flags
    // and ends the thread, which holds no library lock here.
    unsafe { __testcancel() };
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

/// Runs `f` on the state; a long wait it asks for (`Err(PENDING)`, ADR-0302
/// item 9) is made here with the library lock released, and `f` runs again
/// with the outcome. So `f` is run again from its start: it must have changed
/// nothing it cannot repeat before a wait (the methods keep that rule, and a
/// write keeps its progress in the pending wait).
fn state<R>(mut f: impl FnMut(&mut Posix<RtSys>) -> Result<R, i32>) -> Result<R, i32> {
    let mut resume: Option<crate::jobs::Resume<Handle>> = None;
    loop {
        let r = with(|p| p.run_deferred(resume.take(), &mut f));
        match r {
            None => return Err(ENOSYS),
            Some((r, None)) => return r,
            Some((_, Some(w))) => {
                let result = wait_unlocked(&w.sources, w.deadline);
                resume = Some(crate::jobs::Resume {
                    sources: w.sources,
                    own_len: w.own_len,
                    result,
                    progress: w.progress,
                });
            }
        }
    }
}

/// A wait of a thread, made with no library lock held (INV-0302-2): on a
/// port of its own, since two threads waiting on one port would take each
/// other's packets (ADR-0302 item 5). The port lives for the one wait.
///
/// Both ends are cancellation points (ADR-0305, INV-0315-4) and hold no lock.
/// The test is made whatever ended the wait, not only the user packet: the
/// `interrupted` flag a cancel sets is one-shot and a futex wait or a data
/// packet may have taken it, while musl's own `cancel` flag stays set.
fn wait_unlocked(sources: &[(Handle, u32)], deadline: Option<u64>) -> Result<(usize, u32), Status> {
    testcancel();
    let port = zoraya_rt::port_create()?;
    // Tell the table where this thread waits, so that another thread can wake
    // it; and do not wait if something was already sent (ADR-0304 items 2, 3).
    let skip = with(|p| p.set_wait_port(Some(port))).unwrap_or(false);
    let r = if skip {
        Ok((usize::MAX, 0))
    } else {
        port_wait_any(port, sources, deadline)
    };
    let _ = with(|p| {
        p.set_wait_port(None);
        p.take_interrupted()
    });
    let _ = zoraya_rt::handle_close(port);
    testcancel();
    r
}

/// Subscribes `sources` on `port` (key = index + 1), waits for one packet or
/// the deadline, unsubscribes.
fn port_wait_any(
    port: Handle,
    sources: &[(Handle, u32)],
    deadline: Option<u64>,
) -> Result<(usize, u32), Status> {
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
        Ok(()) => zoraya_rt::port_wait(port, deadline.unwrap_or(DEADLINE_INFINITE), &mut packets),
        Err(s) => Err(s),
    };
    for i in 0..subscribed {
        let _ = zoraya_rt::port_unsubscribe(port, i as u64 + 1);
    }
    waited?;
    if packets[0].key == USER_WAKE {
        return Ok((usize::MAX, 0));
    }
    let key = packets[0].key as usize;
    if key == 0 || key > sources.len() {
        return Err(Status::BadState);
    }
    Ok((key - 1, packets[0].signals))
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

// SAFETY: every access is under `MEM`.
unsafe impl Sync for ProcessHeap {}

// SAFETY: `Heap` returns blocks of the layout asked, or null.
unsafe impl GlobalAlloc for ProcessHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        locked(&MEM, || {
            // SAFETY: `MEM` is held, and `Heap` calls nothing that allocates.
            unsafe { (*self.0.get()).alloc(layout) }
        })
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        locked(&MEM, || {
            // SAFETY: as in `alloc`; the caller's contract is `Heap::dealloc`'s.
            unsafe { (*self.0.get()).dealloc(block, layout) }
        })
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
/// One for each of the kernel's 256 mappings of a process, so that 128 thread
/// stacks fit with the image and the heap (ADR-0300).
const MAPPING_SLOTS: usize = 256;

struct Mappings(UnsafeCell<[(u64, u64, u64); MAPPING_SLOTS]>);

// SAFETY: every access is under `MEM`.
unsafe impl Sync for Mappings {}

static MAPPINGS: Mappings = Mappings(UnsafeCell::new([(0, 0, 0); MAPPING_SLOTS]));

/// The table. The caller holds `MEM` and does not keep the reference across
/// releasing it.
fn mappings() -> &'static mut [(u64, u64, u64); MAPPING_SLOTS] {
    // SAFETY: the caller holds `MEM` (see above).
    unsafe { &mut *MAPPINGS.0.get() }
}

const MAP_SHARED: i64 = 0x01;
const MAP_PRIVATE: i64 = 0x02;
const MAP_FIXED: i64 = 0x10;
const MAP_ANONYMOUS: i64 = 0x20;

/// `errno` for a status from a system call, with the field of an `OverQuota`
/// (ADR-0263 item 4; the field is the runtime's last refusal).
pub(crate) fn rt_errno(status: Status, call: Call) -> i32 {
    errno::from_status_seen(status, call, zoraya_rt::last_refusal())
}

/// As [`rt_errno`], with the field read earlier: by a caller that makes other
/// calls (which can be refused too) before it maps the failing one's status.
pub(crate) fn rt_errno_seen(
    status: Status,
    call: Call,
    seen: Option<zoraya_abi::quota::QuotaField>,
) -> i32 {
    errno::from_status_seen(status, call, seen)
}

fn map(length: i64, flags: i64, fd: i64) -> Result<i64, i32> {
    if flags & MAP_SHARED != 0 || flags & MAP_ANONYMOUS == 0 || fd != -1 {
        // File and shared mappings are out of scope (ADR-0103 item 12).
        return Err(ENODEV);
    }
    if flags & MAP_PRIVATE == 0 || flags & MAP_FIXED != 0 || length <= 0 {
        return Err(EINVAL);
    }
    let size = (length as u64).next_multiple_of(PAGE_SIZE as u64);
    // The table is `MEM`'s; the calls inside allocate nothing.
    locked(&MEM, || {
        let free = mappings().iter().position(|m| m.1 == 0).ok_or(ENOMEM)?;
        let object = zoraya_rt::memory_create(size, MemoryFlags::NONE)
            .map_err(|s| rt_errno(s, Call::Memory))?;
        match zoraya_rt::memory_map(object, 0, size, MapFlags::WRITE) {
            Ok(address) => {
                mappings()[free] = (address, size, object.raw());
                Ok(address as i64)
            }
            Err(s) => {
                let _ = zoraya_rt::handle_close(object);
                Err(rt_errno(s, Call::Memory))
            }
        }
    })
}

fn unmap(address: u64, length: u64) -> Result<(), i32> {
    let size = length.next_multiple_of(PAGE_SIZE as u64);
    locked(&MEM, || {
        // The kernel ends whole mappings only: a part of one is refused.
        let m = mappings()
            .iter_mut()
            .find(|m| m.1 != 0 && m.0 == address && m.1 == size)
            .ok_or(EINVAL)?;
        zoraya_rt::memory_unmap(m.0, m.1).map_err(|s| rt_errno(s, Call::Memory))?;
        if let Some(h) = Handle::new(m.2) {
            let _ = zoraya_rt::handle_close(h);
        }
        *m = (0, 0, 0);
        Ok(())
    })
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

/// The C name of the mapping call (ADR-0210): musl's `mmap.c` is
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

/// musl's `pthread_create` maps a thread's stack with `__mmap`.
#[unsafe(no_mangle)]
extern "C" fn __mmap(
    a: *mut u8,
    length: usize,
    prot: i32,
    flags: i32,
    fd: i32,
    off: i64,
) -> *mut u8 {
    mmap(a, length, prot, flags, fd, off)
}

#[unsafe(no_mangle)]
extern "C" fn __munmap(address: *mut u8, length: usize) -> i32 {
    munmap(address, length)
}

/// ADR-0100 item 6: no permission changes on mappings. `pthread_create`
/// tolerates the `ENOSYS` (ADR-0302 item 7).
#[unsafe(no_mangle)]
extern "C" fn mprotect(_a: *mut u8, _n: usize, _prot: i32) -> i32 {
    int(Err(ENOSYS))
}

#[unsafe(no_mangle)]
extern "C" fn __mprotect(a: *mut u8, n: usize, prot: i32) -> i32 {
    mprotect(a, n, prot)
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
    let (kept, net, random, window) = match read(sys, &mut second, &mut self_handle) {
        Some(4) if &second[..4] == SELF_MESSAGE && self_handle.len() == 1 => {
            (self_handle.pop(), None, None, None)
        }
        // `SELF` and the connection to `netd` (ADR-0287): only a channel
        // is taken for the network.
        Some(4) if &second[..4] == SELF_NET_MESSAGE && self_handle.len() == 2 => {
            let net = self_handle.pop();
            (self_handle.pop(), net, None, None)
        }
        // `SELF`, then the connections the flags name (proposed ADR-0310).
        Some(5)
            if &second[..4] == SELF_EXT_MESSAGE
                && ext_handles(second[4]).is_some_and(|n| self_handle.len() == 1 + n) =>
        {
            let window = if second[4] & EXT_WINDOW != 0 {
                self_handle.pop()
            } else {
                None
            };
            let random = if second[4] & EXT_RANDOM != 0 {
                self_handle.pop()
            } else {
                None
            };
            let net = if second[4] & EXT_NET != 0 {
                self_handle.pop()
            } else {
                None
            };
            (self_handle.pop(), net, random, window)
        }
        // A flag this library does not know: `SELF` is still the first
        // handle (abi::SELF_EXT_MESSAGE's contract); the rest is closed below.
        Some(5)
            if &second[..4] == SELF_EXT_MESSAGE
                && ext_handles(second[4]).is_none()
                && !self_handle.is_empty() =>
        {
            (Some(self_handle.remove(0)), None, None, None)
        }
        _ => (None, None, None, None),
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
            // Only a channel is taken for a connection; anything else
            // that arrived in its place is closed.
            let channel = |h: Handle| {
                let channel = zoraya_rt::handle_type(h) == Ok("channel");
                if !channel {
                    let _ = zoraya_rt::handle_close(h);
                }
                channel
            };
            p.net = net.filter(|h| channel(*h));
            p.random = random.filter(|h| channel(*h));
            p.window = window.filter(|h| channel(*h));
            p
        }
        None => {
            for h in handles
                .into_iter()
                .chain(kept)
                .chain(net)
                .chain(random)
                .chain(window)
            {
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
    let line = try_with_for(EXIT_LOCK_BUDGET_NS, |p| {
        use core::fmt::Write;
        let s = p.stats;
        let mut line = String::new();
        let mean = s.resolve_ns.checked_div(s.resolutions).unwrap_or(0);
        let _ = writeln!(
            line,
            "POSIX-STATS resolutions={} resolve_ns_mean={} requests={} seeks={} shares={} reports={} broker={} takes={} futex={}",
            s.resolutions,
            mean,
            s.requests,
            s.seeks,
            s.shares,
            s.reports,
            s.broker,
            s.takes,
            FUTEX_CALLS.load(Ordering::Relaxed)
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
        line
    });
    if let Some(line) = line {
        // Written with the lock free: a full pipe makes the write ask for a
        // wait (`PENDING`), which `restartable` makes outside the lock.
        let _ = procs::restartable(|p| p.write(2, User::new(line.as_ptr() as u64, line.len())));
    }
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
    // A sibling may hold the library lock for ever (a service call that does
    // not end, ADR-0315 item 1): the report is skipped after a bounded wait
    // rather than the exit lost.
    if cfg!(feature = "stats") {
        let _ = try_with_for(EXIT_LOCK_BUDGET_NS, report_stats);
    }
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

/// The address the kernel clears and wakes when a thread ends (`thread_exit`'s
/// `clear`). musl gives the same one to every thread, `&__thread_list_lock`,
/// at `set_tid_address` for the main thread and as `__clone`'s `ctid`; the
/// library keeps the last one given (all equal) instead of a record per
/// thread (ADR-0314 item 3).
static CLEAR_ON_EXIT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// The id of the main thread (ADR-0302 item 2); new threads take 2, 3, ...
const MAIN_TID: i64 = 1;
static NEXT_TID: AtomicU32 = AtomicU32::new(2);
/// musl's tids stay below 2^30 (`__lock` uses the top bits).
const TID_LIMIT: u32 = 1 << 30;

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_set_tid_address(p: i64) -> i64 {
    CLEAR_ON_EXIT.store(p as u64, Ordering::Relaxed);
    MAIN_TID
}

/// The words musl's `__clone` caller leaves at the top of a new thread's
/// stack: what to run.
#[repr(C)]
struct Start {
    function: usize,
    argument: usize,
}

/// A new thread begins here (`thread_create`'s entry), on its own stack with
/// its thread pointer set: musl's `start`, which never returns.
extern "C" fn thread_start(start: *const Start) -> ! {
    // SAFETY: `__clone` wrote a `Start` at the new stack's top before the
    // thread was made, and only this thread reads it.
    let Start { function, argument } = unsafe { start.read() };
    // SAFETY: `function` is the `int (*)(void *)` `__clone` was given.
    let function: extern "C" fn(usize) -> i32 =
        unsafe { core::mem::transmute::<usize, extern "C" fn(usize) -> i32>(function) };
    function(argument);
    zoraya_posix_thread_exit(0)
}

/// musl's `__clone` (`src/thread/aarch64/clone.s`, excluded): makes a thread
/// that runs `function(argument)` on `stack` with thread pointer `tls`.
/// `flags` are musl's constant set and are ignored; the new id is stored at
/// `ptid` before the thread starts. Returns the id or `-EAGAIN`.
#[unsafe(no_mangle)]
extern "C" fn __clone(
    function: usize,
    stack: usize,
    _flags: i32,
    argument: usize,
    ptid: *mut i32,
    tls: usize,
    ctid: usize,
) -> i32 {
    let tid = NEXT_TID.fetch_add(1, Ordering::Relaxed);
    if tid >= TID_LIMIT {
        return -EAGAIN;
    }
    CLEAR_ON_EXIT.store(ctid as u64, Ordering::Relaxed);
    // From the first thread on, long waits are made outside the library lock.
    let _ = with(|p| p.defer_waits = true);
    let sp = (stack - core::mem::size_of::<Start>()) & !15;
    // SAFETY: `stack` is the top of the new thread's own, unused stack, in a
    // mapping the caller made; the thread does not exist yet.
    unsafe {
        (sp as *mut Start).write(Start { function, argument });
        ptid.write(tid as i32);
    }
    // The record first: the new thread may run, and take the library lock,
    // before this returns.
    let _ = with(|p| p.thread_add(tls, tid, None));
    let entry: extern "C" fn(*const Start) -> ! = thread_start;
    match zoraya_rt::thread_create(entry as usize as u64, sp as u64, sp as u64, tls as u64) {
        Ok(handle) => {
            // Kept for `thread_interrupt`; closed by the thread's own exit.
            let _ = with(|p| {
                if let Some(r) = p.threads.list.iter_mut().find(|r| r.tp == tls) {
                    r.handle = Some(handle);
                }
            });
            tid as i32
        }
        Err(_) => {
            let _ = with(|p| p.thread_remove(tls));
            -EAGAIN
        }
    }
}

/// The thread's table record goes, and its handle with it (ADR-0302 item 4),
/// before the thread ends: no slot outlives the thread (INV-0300-7).
fn forget_thread() {
    let me = thread_pointer();
    if let Some(Some(r)) = with(|p| p.thread_remove(me))
        && let Some(h) = r.handle
    {
        let _ = zoraya_rt::handle_close(h);
    }
}

/// `SYS_exit`: ends the calling thread (the last one ends the process with
/// code 0, ADR-0300 item 3). `_Exit` calls `exit_group` first, so this is
/// reached only by a thread leaving.
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_thread_exit(_code: i64) -> ! {
    forget_thread();
    zoraya_rt::thread_exit(CLEAR_ON_EXIT.load(Ordering::Relaxed), 0, 0)
}

/// musl's `__unmapself`: a detached thread's last act. Gives the mapping's
/// object handle back (the library's table forgets the mapping), then the
/// kernel unmaps `[base, base + size)` and ends the thread, without touching
/// the stack that goes (ADR-0300 item 3).
#[unsafe(no_mangle)]
extern "C" fn __unmapself(base: u64, size: u64) -> ! {
    forget_thread();
    let handle = locked(&MEM, || {
        mappings()
            .iter_mut()
            .find(|m| m.1 != 0 && m.0 == base && m.1 == size)
            .map(|m| core::mem::take(&mut m.2))
    });
    if let Some(h) = handle.and_then(Handle::new) {
        let _ = zoraya_rt::handle_close(h);
    }
    // The table entry is cleared by `take` only for the handle: free it too.
    locked(&MEM, || {
        if let Some(m) = mappings().iter_mut().find(|m| m.0 == base && m.1 == size) {
            *m = (0, 0, 0);
        }
    });
    zoraya_rt::thread_exit(CLEAR_ON_EXIT.load(Ordering::Relaxed), base, size)
}

/// `SYS_sched_yield`.
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_sched_yield() -> i64 {
    zoraya_rt::thread_yield();
    0
}

/// `FUTEX_WAIT` and `FUTEX_WAKE` of musl, over ADR-0301's two calls
/// (ADR-0302 item 2). The private flag is ignored (every futex is private);
/// `REQUEUE` is a wake of the wake count plus the requeue count; anything
/// else is `ENOTSUP`. A `futex_wait` that returns early on `thread_interrupt`
/// is a spurious wake here (0): ADR-0304 item 11's handler and cancel cases
/// come with signals (ADR-0314 item 4).
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_futex(
    address: i64,
    op: i64,
    value: i64,
    timeout: i64,
    address2: i64,
    value3: i64,
) -> i64 {
    FUTEX_CALLS.fetch_add(1, Ordering::Relaxed);
    const WAIT: i64 = 0;
    const WAKE: i64 = 1;
    const REQUEUE: i64 = 3;
    const CMP_REQUEUE: i64 = 4;
    const PRIVATE: i64 = 128;
    match op & !PRIVATE {
        WAIT => {
            let deadline = if timeout == 0 {
                DEADLINE_INFINITE
            } else {
                // SAFETY: C's contract: a non-null `timeout` is a `struct
                // timespec` (two 64-bit words, relative: musl converts).
                let [sec, nsec] = unsafe { ptr::read(timeout as *const [i64; 2]) };
                if sec < 0 || !(0..1_000_000_000).contains(&nsec) {
                    return -i64::from(if sec < 0 { ETIMEDOUT } else { EINVAL });
                }
                zoraya_rt::clock_monotonic()
                    .saturating_add((sec as u64).saturating_mul(1_000_000_000))
                    .saturating_add(nsec as u64)
                    .min(DEADLINE_INFINITE - 1)
            };
            match zoraya_rt::futex_wait(address as u64, value as u32, deadline) {
                Ok(false) => 0,
                // Interrupted (ADR-0304 item 11): `-EINTR` only if a handler
                // ran on this thread; otherwise a spurious wake, which
                // `cancel.c` turns into the cancel if one is pending.
                Ok(true) => {
                    let _ = with(|p| p.take_interrupted());
                    if procs::deliver_ran() {
                        -i64::from(EINTR)
                    } else {
                        0
                    }
                }
                Err(Status::WouldBlock) => -i64::from(EAGAIN),
                Err(Status::DeadlineExceeded) => -i64::from(ETIMEDOUT),
                Err(Status::BadAddress) => -i64::from(EFAULT),
                // The process is ending: the thread leaves at its next
                // return to EL0; a spurious wake until then.
                Err(Status::Cancelled) => 0,
                Err(_) => -i64::from(ENOMEM),
            }
        }
        WAKE => wake_up_to(address as u64, value),
        REQUEUE | CMP_REQUEUE => {
            if op & !PRIVATE == CMP_REQUEUE {
                // SAFETY: C's contract: `address` is a futex word.
                let now = unsafe { ptr::read_volatile(address as *const i32) };
                if i64::from(now) != value3 {
                    return -i64::from(EAGAIN);
                }
            }
            // No requeue (ADR-0301 item 4): wake the waiters that would have
            // moved too, each of which re-checks and sleeps on `address2`.
            let _ = address2;
            wake_up_to(
                address as u64,
                value.saturating_add(timeout.min(i64::from(i32::MAX))),
            )
        }
        // Priority inheritance (`FUTEX_LOCK_PI`) and the rest: refused as
        // "not supported", which `pthread_mutexattr_setprotocol` returns to
        // the program (ADR-0302 item 6, INV-0302-4).
        _ => -i64::from(ENOTSUP),
    }
}

/// Wakes up to `count` waiters (`FUTEX_WAKE`'s `INT_MAX` means all): the
/// kernel takes 128 at a call.
fn wake_up_to(address: u64, count: i64) -> i64 {
    let mut left = count.max(0);
    let mut woken = 0i64;
    while left > 0 {
        let ask = left.min(128) as u32;
        match zoraya_rt::futex_wake(address, ask) {
            Ok(n) => {
                woken += i64::from(n);
                left -= i64::from(ask);
                if n < ask {
                    break;
                }
            }
            Err(Status::BadAddress) => return -i64::from(EFAULT),
            Err(_) => return -i64::from(EINVAL),
        }
    }
    woken
}

/// `SYS_rt_sigprocmask`: the calling thread's mask (ADR-0304 item 1), the
/// call musl's thread start and `abort` make. `pthread_sigmask` is the Rust
/// one, not musl's.
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_rt_sigprocmask(how: i64, set: i64, old: i64, size: i64) -> i64 {
    // musl passes `_NSIG / 8` (16): the set is two words, the library keeps
    // the first (signals 1 to 64; the second is always empty).
    if size != 8 && size != 16 {
        return -i64::from(EINVAL);
    }
    let new = if set == 0 {
        None
    } else {
        // SAFETY: C's contract: `set` is a `sigset_t` of 8 bytes.
        Some(unsafe { ptr::read_unaligned(set as *const u64) })
    };
    let Some(r) = with(|p| p.signals.mask(how as i32, new)) else {
        return 0;
    };
    match r {
        Ok(o) => {
            if old != 0 {
                // SAFETY: C's contract: `old` is a `sigset_t` of 8 bytes.
                unsafe {
                    ptr::write_unaligned(old as *mut u64, o);
                    if size == 16 {
                        ptr::write_unaligned((old as *mut u64).add(1), 0);
                    }
                };
            }
            // Unblocking: what was pending is delivered now (ADR-0304 item 3).
            if new.is_some() && how != 0 {
                procs::poll();
            }
            0
        }
        Err(e) => -i64::from(e),
    }
}

/// `SYS_tkill`: a signal for one thread (ADR-0304 item 2).
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_tkill(tid: i64, sig: i64) -> i64 {
    let (Ok(tid), Ok(n)) = (u32::try_from(tid), u32::try_from(sig)) else {
        return -i64::from(EINVAL);
    };
    match with(|p| p.tkill(tid, n)) {
        None => -i64::from(ENOSYS),
        Some(Err(e)) => -i64::from(e),
        Some(Ok(crate::threads::Tkill::Sent)) => 0,
        // Left pending in this thread: `__restore_sigs` delivers it once
        // musl's `killlock` is released (it blocks everything meanwhile).
        Some(Ok(crate::threads::Tkill::Myself)) => 0,
    }
}

/// `pthread_cancel`'s wake (`cancel.c`, ADR-0305): interrupts thread `tid`
/// out of a futex wait, a sleep or a library wait, so that it sees its flag.
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_interrupt(tid: i64) -> i64 {
    let Ok(tid) = u32::try_from(tid) else {
        return -i64::from(EINVAL);
    };
    match with(|p| p.interrupt_tid(tid)) {
        None => -i64::from(ENOSYS),
        Some(Err(e)) => -i64::from(e),
        Some(Ok(())) => 0,
    }
}

/// `pthread_setname_np`/`getname_np` (`attr.c`): thread `tid`'s name, 16 bytes
/// at `buf`; `set` writes it from `buf` (NUL-terminated, at most 15 bytes).
/// Returns 0 or `-errno`.
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_thread_name(tid: i64, buf: *mut u8, set: i64) -> i64 {
    let Ok(tid) = u32::try_from(tid) else {
        return -i64::from(EINVAL);
    };
    let r = with(|p| {
        if set != 0 {
            let mut name = [0u8; 16];
            // SAFETY: C's contract: `buf` holds a NUL-terminated string; at
            // most 16 bytes are read, the 16th only to find it missing.
            let n = (0..16)
                .take_while(|&i| unsafe { buf.add(i).read() } != 0)
                .count();
            for (i, b) in name.iter_mut().enumerate().take(n.min(15)) {
                // SAFETY: as above, within the first `n` bytes.
                *b = unsafe { buf.add(i).read() };
            }
            if n > 15 {
                return Err(crate::errno::ERANGE);
            }
            p.set_thread_name(tid, &name[..n])
        } else {
            p.thread_name(tid).map(|n| {
                // SAFETY: C's contract: `buf` has room for 16 bytes.
                unsafe { ptr::copy_nonoverlapping(n.as_ptr(), buf, 16) }
            })
        }
    });
    match r {
        None => -i64::from(ENOSYS),
        Some(Err(e)) => -i64::from(e),
        Some(Ok(())) => 0,
    }
}

/// The main thread's stack (`pthread_getattr_np`): the top and the size the
/// kernel gave every process.
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_main_stack(out: *mut [usize; 2]) {
    // SAFETY: C's contract: `out` holds two words.
    unsafe {
        out.write([
            zoraya_abi::spawn::USER_STACK_TOP as usize,
            zoraya_abi::spawn::USER_STACK_SIZE as usize,
        ])
    };
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
    // A cancellation point (ADR-0305 item 2), at the top, outside the lock.
    testcancel();
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

/// ADR-0210: `unlinkat` with `AT_REMOVEDIR`, as musl's `rmdir.c`.
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

/// ADR-0210: `symlinkat` from the working directory.
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

/// ADR-0210: `fchmodat` from the working directory.
#[unsafe(no_mangle)]
extern "C" fn chmod(path: *const c_char, mode: u32) -> i32 {
    fchmodat(AT_FDCWD, path, mode, 0)
}

/// ADR-0210 item 3: see `Posix::fsync`.
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

/// ADR-0210: `utimensat` with the two `struct timeval`s turned into
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

/// `fcntl`'s record-lock commands (ADR-0319): `argument` is a `struct flock`.
/// A waiting command sleeps a millisecond at a time while the lock is held by
/// another owner (a sleep that a handler interrupts is `EINTR`).
fn fcntl_record_lock(fd: i32, command: i32, argument: i64) -> Result<i32, i32> {
    use crate::records::{self, FLOCK_SIZE};
    if argument == 0 {
        return Err(EFAULT);
    }
    let at = argument as *mut [u8; FLOCK_SIZE];
    // SAFETY: the caller (C's `fcntl`) passes, for a record-lock command, a
    // pointer to a readable `struct flock` of `FLOCK_SIZE` bytes. Only null is
    // checked: any other bad pointer faults the process, where Linux answers
    // `EFAULT` (as the other pointer arguments of this file do). The read is
    // unaligned-safe for `[u8; N]`.
    let mut flock = unsafe { at.read() };
    // A waiter must not take the lock on another file if its fd is closed and
    // the number reused meanwhile: the description is checked between polls.
    let conn = with(|p| p.lock_description(fd));
    loop {
        match state(|p| p.fcntl_lock(fd, command, &mut flock)) {
            Ok(()) => break,
            Err(EAGAIN) if records::waits(command) => {
                let until = zoraya_rt::clock_monotonic().saturating_add(1_000_000);
                let e = procs::nanosleep_until(until, ptr::null_mut());
                if e != 0 {
                    return Err(e);
                }
                if with(|p| p.lock_description(fd)) != conn {
                    return Err(EBADF);
                }
            }
            Err(e) => return Err(e),
        }
    }
    if matches!(command, records::F_GETLK | records::F_OFD_GETLK) {
        // SAFETY: as above, the pointer is writable (`F_GETLK` fills the struct).
        unsafe { at.write(flock) };
    }
    Ok(0)
}

#[unsafe(no_mangle)]
extern "C" fn fcntl(fd: i32, command: i32, argument: i64) -> i32 {
    if crate::records::is_lock_command(command) {
        return int(fcntl_record_lock(fd, command, argument));
    }
    int(state(|p| p.fcntl(fd, command, argument)))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_fcntl(fd: i64, command: i64, argument: i64) -> i64 {
    if crate::records::is_lock_command(command as i32) {
        return raw(fcntl_record_lock(fd as i32, command as i32, argument).map(i64::from));
    }
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

/// ADR-0210: `uname`'s node name, as musl's `gethostname.c`:
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

/// ADR-0210: no accounting of resources (as `wait3`): a zero
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

/// musl's thread code blocks signals around its locks and thread creation:
/// `set` gets the mask in force, which `pthread_create` gives the new thread
/// (`args->sig_mask`) and `__restore_sigs` puts back. The calling thread's
/// mask really becomes "everything" (except `SIGKILL` and `SIGSTOP`) until
/// then, so a handler never runs inside musl's critical sections (`killlock`,
/// the thread-list lock): neither a `tkill` of oneself nor a delivery at a
/// futex wait finds it unblocked (ADR-0304 item 1; reviewed: the first
/// version blocked nothing). A null `set` (`abort`) forgets the old mask.
fn block_sigs(set: *mut u64) {
    let old = with(|p| {
        p.signals
            .mask(crate::signals::SIG_SETMASK, Some(u64::MAX))
            .unwrap_or(0)
    })
    .unwrap_or(0);
    if !set.is_null() {
        // SAFETY: C's contract: `set` is a `sigset_t` (128 bytes) the caller owns.
        unsafe {
            set.write_unaligned(old);
            set.add(1).write_unaligned(0);
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn __block_all_sigs(set: *mut u64) {
    block_sigs(set);
}

#[unsafe(no_mangle)]
extern "C" fn __block_app_sigs(set: *mut u64) {
    block_sigs(set);
}

/// Puts the saved mask back and delivers what it lets through now: a signal
/// that a `tkill` of oneself left pending runs its handler here, after
/// musl's lock is released (`pthread_kill`, `raise`).
#[unsafe(no_mangle)]
extern "C" fn __restore_sigs(set: *mut u64) {
    if set.is_null() {
        return;
    }
    // SAFETY: C's contract: `set` was filled by `__block_*_sigs`.
    let old = unsafe { set.read_unaligned() };
    let _ = with(|p| p.signals.mask(crate::signals::SIG_SETMASK, Some(old)));
    procs::poll();
}

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
/// (`sendmsg`, `syscall`, `setsid`; the stream client's socket calls are
/// `sockets.rs`, N4): each fails with `ENOSYS`, as ADR-0140's
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

not_yet!(int syscall tee sendmsg setgroups setsid);
not_yet!(ptr __map_file);

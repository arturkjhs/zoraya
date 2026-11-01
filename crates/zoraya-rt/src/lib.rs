//! The runtime of Zoraya programs: the entry point, system calls and a panic
//! handler. A program defines `main`.
//!
//! The calls are `zoraya_abi::stubs` with `svc` behind them: this crate has no
//! numbers or register layouts of its own (ADR-0044).

#![no_std]

use core::arch::{asm, global_asm};
use core::fmt;

use zoraya_abi::spawn::{STARTUP_BLOCK, STARTUP_BLOCK_MAX, StartupBlock};
use zoraya_abi::{
    Address, Buffer, Bytes, CONSOLE, ChannelEntry, ChannelSizes, Cookie, Deadline, Detail,
    Disposition, ExitCode, ExitReason, ExitRecord, Handle, Invoke, JobFlags, MapFlags, MemoryFlags,
    Mode, Opaque, Out, Packet, PacketBuffer, PathBuffer, RequestKind, Rights, SignalSet, Size,
    SpawnBuffer, Status, request_from_value, stubs,
};

const MAX_MESSAGE_HANDLES: usize = 64;

global_asm!(
    r#"
    .text
    .global _start
_start:
    // A process starts with nothing in its registers, so that nothing of the
    // kernel's or another process's can reach it: `brk` makes a fault of any
    // leak, and every program checks this on every start.
    .irp    n, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30
    orr     x0, x0, x\n
    orr     v0.16b, v0.16b, v\n\().16b
    .endr
    orr     v0.16b, v0.16b, v31.16b
    mrs     x1, tpidr_el0
    orr     x0, x0, x1
    mrs     x1, fpcr
    orr     x0, x0, x1
    umov    x1, v0.d[0]
    orr     x0, x0, x1
    umov    x1, v0.d[1]
    orr     x0, x0, x1
    cbz     x0, 1f
    brk     #2
1:
    bl      __user_main
"#
);

unsafe extern "Rust" {
    safe fn main() -> i32;
}

#[unsafe(no_mangle)]
extern "C" fn __user_main() -> ! {
    process_exit(main())
}

/// Makes a system call with up to six arguments, returning x0 and x1.
pub fn raw_syscall(number: u64, registers: [u64; 6]) -> (u64, u64) {
    let (status, value);
    // SAFETY: `svc` enters the kernel, which validates every value and
    // preserves everything but x0 and x1. Pointers are only read or written
    // as the call defines, on behalf of this process.
    unsafe {
        asm!(
            "svc #0",
            inout("x0") registers[0] => status,
            inout("x1") registers[1] => value,
            in("x2") registers[2],
            in("x3") registers[3],
            in("x4") registers[4],
            in("x5") registers[5],
            in("x8") number,
            options(nostack),
        )
    };
    (status, value)
}

struct Svc;

impl Invoke for Svc {
    fn invoke(number: u64, registers: [u64; 6]) -> (u64, u64) {
        raw_syscall(number, registers)
    }
}

fn bytes(slice: &[u8]) -> Buffer {
    Buffer {
        address: slice.as_ptr() as u64,
        length: slice.len() as u64,
    }
}

fn handle(raw: u64) -> Result<Handle, Status> {
    Handle::new(raw).ok_or(Status::BadHandle)
}

pub fn process_exit(code: i32) -> ! {
    let _ = stubs::process_exit::<Svc>(ExitCode(code));
    // The kernel does not return from this call.
    loop {
        core::hint::spin_loop();
    }
}

/// Ends this process as a request with `Terminate` disposition would.
pub fn process_exit_terminated(detail: u32) -> ! {
    let _ = stubs::process_exit_with_reason::<Svc>(ExitReason::Terminated, Detail(detail));
    loop {
        core::hint::spin_loop();
    }
}

pub fn thread_yield() {
    let _ = stubs::thread_yield::<Svc>();
}

pub fn console_write(console: Handle, text: &[u8]) -> Result<u64, Status> {
    stubs::console_write::<Svc>(console, Bytes(bytes(text)))
}

/// Nanoseconds on the monotonic clock.
pub fn clock_monotonic() -> u64 {
    stubs::clock_monotonic::<Svc>().unwrap_or(0)
}

/// Blocks until the instant `nanos` on the monotonic clock.
pub fn sleep_until(nanos: u64) {
    let _ = stubs::sleep_until::<Svc>(Deadline(nanos));
}

pub fn handle_close(h: Handle) -> Result<(), Status> {
    stubs::handle_close::<Svc>(h).map(|_| ())
}

pub fn handle_duplicate(h: Handle, rights: Rights) -> Result<Handle, Status> {
    handle(stubs::handle_duplicate::<Svc>(h, rights)?)
}

pub fn job_create(parent: Handle, flags: JobFlags) -> Result<Handle, Status> {
    handle(stubs::job_create::<Svc>(parent, flags)?)
}

/// `buffer` is what `zoraya_abi::spawn::Encoder` wrote.
pub fn spawn(buffer: &[u8]) -> Result<Handle, Status> {
    handle(stubs::spawn::<Svc>(SpawnBuffer(bytes(buffer)))?)
}

pub fn process_resume(process: Handle) -> Result<(), Status> {
    stubs::process_resume::<Svc>(process).map(|_| ())
}

pub fn process_exit_record(process: Handle) -> Result<ExitRecord, Status> {
    let mut raw = [0; ExitRecord::SIZE];
    let target = Buffer {
        address: raw.as_mut_ptr() as u64,
        length: raw.len() as u64,
    };
    stubs::process_exit_record::<Svc>(process, Bytes(target))?;
    ExitRecord::from_bytes(&raw).ok_or(Status::InvalidArgument)
}

pub fn process_signal(process: Handle, kind: RequestKind, detail: u32) -> Result<(), Status> {
    stubs::process_signal::<Svc>(process, kind, Detail(detail)).map(|_| ())
}

pub fn process_set_disposition(kind: RequestKind, d: Disposition) -> Result<(), Status> {
    stubs::process_set_disposition::<Svc>(kind, d).map(|_| ())
}

pub fn process_take_request() -> Result<(RequestKind, u32), Status> {
    let value = stubs::process_take_request::<Svc>()?;
    request_from_value(value).ok_or(Status::InvalidArgument)
}

pub fn job_signal(job: Handle, kind: RequestKind, detail: u32) -> Result<(), Status> {
    stubs::job_signal::<Svc>(job, kind, Detail(detail)).map(|_| ())
}

pub fn job_kill(job: Handle) -> Result<(), Status> {
    stubs::job_kill::<Svc>(job).map(|_| ())
}

pub fn port_create() -> Result<Handle, Status> {
    handle(stubs::port_create::<Svc>()?)
}

pub fn port_subscribe(
    port: Handle,
    target: Handle,
    signals: u32,
    key: u64,
    mode: Mode,
) -> Result<(), Status> {
    stubs::port_subscribe::<Svc>(port, target, SignalSet(signals), Opaque(key), mode).map(|_| ())
}

pub fn port_unsubscribe(port: Handle, key: u64) -> Result<(), Status> {
    stubs::port_unsubscribe::<Svc>(port, Opaque(key)).map(|_| ())
}

pub fn port_queue(port: Handle, key: u64, data: u64) -> Result<(), Status> {
    stubs::port_queue::<Svc>(port, Opaque(key), Opaque(data)).map(|_| ())
}

/// Fills the front of `packets`, and gives how many.
pub fn port_wait(port: Handle, deadline: u64, packets: &mut [Packet]) -> Result<usize, Status> {
    let buffer = PacketBuffer(Buffer {
        address: packets.as_mut_ptr() as u64,
        length: packets.len() as u64,
    });
    let count = stubs::port_wait::<Svc>(port, Deadline(deadline), buffer)?;
    core::hint::black_box(packets);
    Ok(count as usize)
}

/// Moves waiting input to the front of `buffer`, and gives how many bytes. It
/// never blocks: 0 means none.
pub fn console_read(console: Handle, buffer: &mut [u8]) -> Result<usize, Status> {
    let count = stubs::console_read::<Svc>(console, Bytes(bytes(buffer)))?;
    core::hint::black_box(buffer);
    Ok(count as usize)
}

pub fn console_dropped(console: Handle) -> Result<u64, Status> {
    stubs::console_dropped::<Svc>(console)
}

/// Writes the record at `cookie`, and gives its size: 0 past the last name.
pub fn directory_entry(directory: Handle, cookie: u64, entry: &mut [u8]) -> Result<usize, Status> {
    let size = stubs::directory_entry::<Svc>(directory, Cookie(cookie), Bytes(bytes(entry)))?;
    core::hint::black_box(entry);
    Ok(size as usize)
}

/// Turns the machine off. It returns only on a refusal.
pub fn power_off(power: Handle) -> Status {
    match stubs::power_off::<Svc>(power) {
        Ok(_) => Status::Ok,
        Err(status) => status,
    }
}

/// A new memory object of `size` bytes, all committed and zeroed.
pub fn memory_create(size: u64, flags: MemoryFlags) -> Result<Handle, Status> {
    handle(stubs::memory_create::<Svc>(Size(size), flags)?)
}

/// Maps part of a memory object, and gives the address the kernel chose.
pub fn memory_map(
    object: Handle,
    offset: u64,
    length: u64,
    flags: MapFlags,
) -> Result<u64, Status> {
    stubs::memory_map::<Svc>(object, Size(offset), Size(length), flags)
}

/// Ends the whole mapping at `address`.
pub fn memory_unmap(address: u64, length: u64) -> Result<(), Status> {
    stubs::memory_unmap::<Svc>(Address(address), Size(length)).map(|_| ())
}

/// Two ends of a new channel.
pub fn channel_create() -> Result<(Handle, Handle), Status> {
    let mut ends = [0u8; 16];
    let buffer = Buffer {
        address: ends.as_mut_ptr() as u64,
        length: 16,
    };
    stubs::channel_create::<Svc>(Bytes(buffer))?;
    core::hint::black_box(&mut ends);
    let [a, b] = [&ends[..8], &ends[8..]].map(|e| {
        let mut raw = [0; 8];
        raw.copy_from_slice(e);
        handle(u64::from_le_bytes(raw))
    });
    Ok((a?, b?))
}

/// Sends a message, all or nothing.
pub fn channel_write(end: Handle, message: &[u8], handles: &[ChannelEntry]) -> Result<(), Status> {
    let mut list = [0; ChannelEntry::SIZE * MAX_MESSAGE_HANDLES];
    let room = handles.len() * ChannelEntry::SIZE;
    let target = list.get_mut(..room).ok_or(Status::InvalidArgument)?;
    for (slot, entry) in target.chunks_exact_mut(ChannelEntry::SIZE).zip(handles) {
        slot.copy_from_slice(&entry.to_bytes());
    }
    stubs::channel_write::<Svc>(end, Bytes(bytes(message)), Bytes(bytes(&list[..room])))?;
    Ok(())
}

/// Takes the next message. On `BufferTooSmall` the message stays, and the
/// sizes it needs are in the error's second part.
pub fn channel_read(
    end: Handle,
    message: &mut [u8],
    handles: &mut [Handle],
) -> Result<ChannelSizes, (Status, ChannelSizes)> {
    let mut raw = [0u64; MAX_MESSAGE_HANDLES];
    let room = handles.len().min(MAX_MESSAGE_HANDLES);
    let mut sizes = [0u8; ChannelSizes::SIZE];
    let list = Buffer {
        address: raw.as_mut_ptr() as u64,
        length: (room * 8) as u64,
    };
    let result = stubs::channel_read::<Svc>(
        end,
        Bytes(bytes(message)),
        Bytes(list),
        Out(sizes.as_mut_ptr() as u64),
    );
    core::hint::black_box((&mut sizes, &mut raw, &mut *message));
    let sizes = ChannelSizes::from_bytes(&sizes);
    match result {
        Ok(_) => {
            for (slot, raw) in handles.iter_mut().zip(&raw[..sizes.handles as usize]) {
                *slot = handle(*raw).map_err(|e| (e, sizes))?;
            }
            Ok(sizes)
        }
        Err(status) => Err((status, sizes)),
    }
}

pub fn directory_lookup(directory: Handle, name: &str) -> Result<Handle, Status> {
    let name = PathBuffer(bytes(name.as_bytes()));
    handle(stubs::directory_lookup::<Svc>(directory, name)?)
}

/// This process's arguments and environment.
pub fn startup() -> StartupBlock<'static> {
    startup_block().expect("the kernel wrote this block")
}

fn startup_block() -> Option<StartupBlock<'static>> {
    // Up to the end of its last page: lengths in the block keep `decode`
    // inside what is mapped, but a slice must not cover unmapped memory, so
    // this first finds the size by walking one string at a time.
    let mut size = 8usize;
    let word = |at: usize| {
        // SAFETY: `at` is inside the block: the walk below only reads a length
        // the kernel wrote, and the block ends after the last string.
        unsafe { ((STARTUP_BLOCK as usize + at) as *const u32).read_unaligned() }
    };
    for _ in 0..u64::from(word(0)) + u64::from(word(4)) {
        size = size.checked_add(4)?.checked_add(word(size) as usize)?;
        if size as u64 > STARTUP_BLOCK_MAX {
            return None;
        }
    }
    // SAFETY: the walk above found `size` bytes, all mapped and immutable.
    let block = unsafe { core::slice::from_raw_parts(STARTUP_BLOCK as *const u8, size) };
    StartupBlock::decode(block)
}

/// Prints to the console this process started with.
pub struct Console;

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        console_write(CONSOLE, s.as_bytes())
            .map(|_| ())
            .map_err(|_| fmt::Error)
    }
}

/// Prints to the console, with no newline.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = write!($crate::Console, $($arg)*);
    }};
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = writeln!($crate::Console, $($arg)*);
    }};
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    println!("user panic: {info}");
    process_exit(101)
}

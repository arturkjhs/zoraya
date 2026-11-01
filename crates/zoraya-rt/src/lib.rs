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
    Buffer, Bytes, CONSOLE, Deadline, Detail, Disposition, ExitCode, ExitReason, ExitRecord,
    Handle, Invoke, JobFlags, Mode, Opaque, Packet, PacketBuffer, PathBuffer, RequestKind, Rights,
    SignalSet, SpawnBuffer, Status, request_from_value, stubs,
};

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

/// Makes a system call with up to three arguments, returning x0 and x1.
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
    u32::try_from(raw)
        .ok()
        .and_then(Handle::new)
        .ok_or(Status::BadHandle)
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
    stubs::process_exit_record::<Svc>(process, Bytes(bytes(&raw)))?;
    // The kernel wrote through the pointer above, which the compiler cannot
    // see: `black_box` keeps it from assuming `raw` is still zero.
    let raw = core::hint::black_box(&mut raw);
    ExitRecord::from_bytes(raw).ok_or(Status::InvalidArgument)
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

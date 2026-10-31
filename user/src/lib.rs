//! The user runtime: the entry point, system call wrappers and a panic
//! handler. A program defines `main`.

#![no_std]

use core::arch::{asm, global_asm};
use core::fmt;

use zoraya_abi::{CONSOLE, Handle, Status, Syscall};

global_asm!(
    r#"
    .text
    .global _start
_start:
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
pub fn raw_syscall(number: u64, a0: u64, a1: u64, a2: u64) -> (u64, u64) {
    let (status, value);
    // SAFETY: `svc` enters the kernel, which validates every value and
    // preserves everything but x0 and x1. Pointers are only read or written
    // as the call defines, on behalf of this process.
    unsafe {
        asm!(
            "svc #0",
            inout("x0") a0 => status,
            inout("x1") a1 => value,
            in("x2") a2,
            in("x8") number,
            options(nostack),
        )
    };
    (status, value)
}

fn call(call: Syscall, a0: u64, a1: u64, a2: u64) -> Result<u64, Status> {
    let (status, value) = raw_syscall(call as u64, a0, a1, a2);
    match Status::from_raw(status) {
        Some(Status::Ok) => Ok(value),
        Some(s) => Err(s),
        None => Err(Status::BadSyscall),
    }
}

pub fn process_exit(code: i32) -> ! {
    let _ = call(Syscall::ProcessExit, code as u32 as u64, 0, 0);
    // The kernel does not return from this call.
    loop {
        core::hint::spin_loop();
    }
}

pub fn thread_yield() {
    let _ = call(Syscall::ThreadYield, 0, 0, 0);
}

pub fn console_write(handle: Handle, bytes: &[u8]) -> Result<u64, Status> {
    call(
        Syscall::ConsoleWrite,
        u64::from(handle.raw()),
        bytes.as_ptr() as u64,
        bytes.len() as u64,
    )
}

/// Nanoseconds on the monotonic clock.
pub fn clock_monotonic() -> u64 {
    call(Syscall::ClockMonotonic, 0, 0, 0).unwrap_or(0)
}

/// Blocks until the instant `nanos` on the monotonic clock.
pub fn sleep_until(nanos: u64) {
    let _ = call(Syscall::SleepUntil, nanos, 0, 0);
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

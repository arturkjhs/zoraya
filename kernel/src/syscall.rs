//! System calls (ADR-0032). Nothing here trusts a register: every value comes
//! from user space, and every failure is a [`Status`].

use alloc::vec::Vec;

use zoraya_abi::{DEADLINE_INFINITE, ExitReason, ExitRecord, Handle, Status, Syscall};
use zoraya_time::{Deadline, Instant};

use crate::arch::{self, Frame};
use crate::memory::user::USER_HIGH;
use crate::{print, process, sched, timer};

/// Longest `console_write`.
///
/// System calls run with interrupts masked, and the UART is polled, so this
/// bounds how long one call can hold off everything else. 4 KiB took about
/// 60 ms under QEMU (from 690 ms measured for 64 KiB).
const MAX_WRITE: u64 = 4 * 1024;

/// A range in the user half. The only way to reach user memory (ADR-0031).
pub struct UserSlice {
    address: u64,
    len: usize,
}

impl UserSlice {
    pub fn new(address: u64, len: u64) -> Result<UserSlice, Status> {
        match address.checked_add(len) {
            Some(end) if end <= USER_HIGH => Ok(UserSlice {
                address,
                len: len as usize,
            }),
            _ => Err(Status::BadAddress),
        }
    }

    /// Copies the range into a new vector.
    pub fn read(&self) -> Result<Vec<u8>, Status> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.len)
            .map_err(|_| Status::NoMemory)?;
        bytes.resize(self.len, 0);
        // SAFETY: `bytes` has `len` bytes, and any user address is safe: the
        // access is checked as EL0's, and a fault returns an error.
        unsafe { arch::copy_from_user(&mut bytes, self.address) }
            .map_err(|_| Status::BadAddress)?;
        Ok(bytes)
    }
}

fn console_write(handle: u64, address: u64, len: u64) -> Result<u64, Status> {
    let handle = u32::try_from(handle)
        .ok()
        .and_then(Handle::new)
        .ok_or(Status::BadHandle)?;
    let thread = sched::current();
    let process = thread.process().ok_or(Status::BadHandle)?;
    if !process.has_console(handle) {
        return Err(Status::BadHandle);
    }
    if len > MAX_WRITE {
        return Err(Status::InvalidArgument);
    }
    let bytes = UserSlice::new(address, len)?.read()?;
    let text = core::str::from_utf8(&bytes).map_err(|_| Status::InvalidArgument)?;
    print!("{text}");
    Ok(len)
}

fn sleep_until(raw: u64) -> Result<u64, Status> {
    let deadline = match raw {
        DEADLINE_INFINITE => Deadline::Infinite,
        nanos => Deadline::At(Instant::from_nanos(nanos)),
    };
    sched::sleep_until(deadline).map_err(|_| Status::NoMemory)?;
    Ok(0)
}

/// Calls that exist only in the test kernel, outside `zoraya-abi`.
#[cfg(feature = "qemu-test")]
pub mod test_calls {
    /// Returns 1 if the timer is on.
    pub const TIMER_IS_ON: u64 = 0x7000;
    /// Overflows the kernel stack of the calling thread.
    pub const OVERFLOW_STACK: u64 = 0x7001;
}

#[cfg(feature = "qemu-test")]
fn test_call(number: u64) -> Option<Result<u64, Status>> {
    match number {
        test_calls::TIMER_IS_ON => Some(Ok(u64::from(arch::timer::is_enabled()))),
        test_calls::OVERFLOW_STACK => {
            crate::testing::overflow_stack();
            Some(Ok(0))
        }
        _ => None,
    }
}

/// `ESR_EL1.ISS` of an `svc`: its immediate, which must be 0 (ADR-0032).
const fn svc_immediate(esr: u64) -> u64 {
    esr & 0xffff
}

/// Handles the `svc` in `frame`.
pub fn dispatch(frame: &mut Frame) {
    let [a0, a1, a2] = [frame.x[0], frame.x[1], frame.x[2]];
    let number = frame.x[8];
    #[cfg(feature = "qemu-test")]
    if let Some(result) = test_call(number) {
        frame.x[0] = match result {
            Ok(_) => Status::Ok as u64,
            Err(status) => status as u64,
        };
        frame.x[1] = result.unwrap_or(0);
        return;
    }
    let known = if svc_immediate(frame.esr) == 0 {
        Syscall::from_number(number)
    } else {
        None
    };
    let result = match known {
        None => Err(Status::BadSyscall),
        Some(Syscall::ProcessExit) => process::exit_current(ExitRecord {
            code: a0 as u32 as i32,
            reason: ExitReason::Exited,
            detail: 0,
        }),
        Some(Syscall::ThreadYield) => {
            sched::yield_now();
            Ok(0)
        }
        Some(Syscall::ConsoleWrite) => console_write(a0, a1, a2),
        Some(Syscall::ClockMonotonic) => Ok(timer::now().as_nanos()),
        Some(Syscall::SleepUntil) => sleep_until(a0),
    };
    let (status, value) = match result {
        Ok(value) => (Status::Ok, value),
        Err(status) => (status, 0),
    };
    frame.x[0] = status as u64;
    frame.x[1] = value;
}

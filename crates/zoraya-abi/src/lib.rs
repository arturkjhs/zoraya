//! The native ABI (ADR-0032), shared by the kernel and user programs.
//!
//! Trap: `svc #0`, number in x8, arguments in x0 to x5. Return: x0 is a
//! [`Status`], x1 a value. Every other register is preserved.
//!
//! [`SYSCALLS`] states each call's parameters as data, so that tests can check
//! the rules of INV-0002-1, INV-0004-1 and INV-0006-2 over the whole ABI.

#![no_std]
#![forbid(unsafe_code)]

/// Result of a system call.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    /// The number names no call.
    BadSyscall = 1,
    /// The handle is not valid in this process, or is the wrong kind.
    BadHandle = 2,
    /// A pointer does not lie in readable or writable user memory.
    BadAddress = 3,
    /// An argument has a value the call does not accept.
    InvalidArgument = 4,
    /// The kernel could not get the memory the call needs.
    NoMemory = 5,
}

impl Status {
    pub const fn from_raw(raw: u64) -> Option<Status> {
        Some(match raw {
            0 => Status::Ok,
            1 => Status::BadSyscall,
            2 => Status::BadHandle,
            3 => Status::BadAddress,
            4 => Status::InvalidArgument,
            5 => Status::NoMemory,
            _ => return None,
        })
    }
}

/// Names an object in a process's handle table. Zero is never valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(u32);

impl Handle {
    pub const fn new(raw: u32) -> Option<Handle> {
        if raw == 0 { None } else { Some(Handle(raw)) }
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// The console handle a process starts with, until stage 3 passes handles
/// explicitly at spawn.
pub const CONSOLE: Handle = Handle(1);

/// `sleep_until`'s encoding of `Deadline::Infinite`. Any other value is an
/// instant in nanoseconds on the monotonic clock.
pub const DEADLINE_INFINITE: u64 = u64::MAX;

/// Why a process ended (ADR-0004 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Exited,
    Terminated,
    Killed,
    Fault,
    Limit,
}

/// What a process leaves behind (ADR-0004 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitRecord {
    pub code: i32,
    pub reason: ExitReason,
    pub detail: u32,
}

/// System call numbers.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    ProcessExit = 0,
    ThreadYield = 1,
    ConsoleWrite = 2,
    ClockMonotonic = 3,
    SleepUntil = 4,
}

impl Syscall {
    pub const fn from_number(n: u64) -> Option<Syscall> {
        Some(match n {
            0 => Syscall::ProcessExit,
            1 => Syscall::ThreadYield,
            2 => Syscall::ConsoleWrite,
            3 => Syscall::ClockMonotonic,
            4 => Syscall::SleepUntil,
            _ => return None,
        })
    }
}

/// The kinds a parameter may have. Adding a kind is how the ABI could break
/// the rules, so the tests below list which are allowed where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    Handle,
    /// A pointer and a length, in two registers.
    Buffer,
    Deadline,
    /// `ExitRecord.code`: opaque to the kernel (ADR-0004 rule 1).
    ExitCode,
    /// Code to run. Only thread creation may take one (INV-0004-1).
    EntryPoint,
}

/// One call's signature.
#[derive(Debug, Clone, Copy)]
pub struct Signature {
    pub name: &'static str,
    pub call: Syscall,
    pub params: &'static [Param],
    /// The caller can wait inside the call.
    pub blocking: bool,
}

pub const SYSCALLS: &[Signature] = &[
    Signature {
        name: "process_exit",
        call: Syscall::ProcessExit,
        params: &[Param::ExitCode],
        blocking: false,
    },
    Signature {
        name: "thread_yield",
        call: Syscall::ThreadYield,
        params: &[],
        blocking: false,
    },
    Signature {
        name: "console_write",
        call: Syscall::ConsoleWrite,
        params: &[Param::Handle, Param::Buffer],
        blocking: false,
    },
    Signature {
        name: "clock_monotonic",
        call: Syscall::ClockMonotonic,
        params: &[],
        blocking: false,
    },
    Signature {
        name: "sleep_until",
        call: Syscall::SleepUntil,
        params: &[Param::Deadline],
        blocking: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// INV-0002-1: objects are named by handles, and integers are opaque
    /// payloads. Listing the kinds by match means a new kind must be judged
    /// here to compile.
    #[test]
    fn parameters_come_from_the_allow_list() {
        for s in SYSCALLS {
            for p in s.params {
                match p {
                    Param::Handle | Param::Buffer | Param::Deadline | Param::ExitCode => {}
                    Param::EntryPoint => assert_eq!(s.name, "thread_create"),
                }
            }
        }
    }

    /// INV-0004-1: nothing registers code to run asynchronously.
    #[test]
    fn only_thread_creation_takes_an_entry_point() {
        for s in SYSCALLS
            .iter()
            .filter(|s| s.params.contains(&Param::EntryPoint))
        {
            assert_eq!(s.name, "thread_create");
        }
    }

    /// INV-0006-2: blocking calls take handles and a `Deadline`, and nothing
    /// else, so no signal mask, no array to scan and no relative timeout.
    #[test]
    fn blocking_calls_take_a_deadline_and_no_masks_or_timeouts() {
        let mut blocking = 0;
        for s in SYSCALLS.iter().filter(|s| s.blocking) {
            blocking += 1;
            assert!(s.params.contains(&Param::Deadline), "{}", s.name);
            for p in s.params {
                assert!(matches!(p, Param::Handle | Param::Deadline), "{}", s.name);
            }
        }
        assert!(blocking > 0);
    }

    /// INV-0032-1.
    #[test]
    fn numbers_round_trip_and_unknown_ones_are_refused() {
        for s in SYSCALLS {
            assert_eq!(
                Syscall::from_number(s.call as u64),
                Some(s.call),
                "{}",
                s.name
            );
        }
        assert_eq!(Syscall::from_number(SYSCALLS.len() as u64), None);
        assert_eq!(Syscall::from_number(u64::MAX), None);
        for (i, s) in SYSCALLS.iter().enumerate() {
            assert_eq!(s.call as usize, i, "table in number order");
        }
    }

    #[test]
    fn registers_are_enough() {
        for s in SYSCALLS {
            let registers: usize = s
                .params
                .iter()
                .map(|p| if *p == Param::Buffer { 2 } else { 1 })
                .sum();
            assert!(registers <= 6, "{}", s.name);
        }
    }

    #[test]
    fn status_round_trips() {
        for s in [
            Status::Ok,
            Status::BadSyscall,
            Status::BadHandle,
            Status::BadAddress,
            Status::InvalidArgument,
            Status::NoMemory,
        ] {
            assert_eq!(Status::from_raw(s as u64), Some(s));
        }
        assert_eq!(Status::from_raw(99), None);
    }

    #[test]
    fn handle_zero_is_not_valid() {
        assert_eq!(Handle::new(0), None);
        assert_eq!(Handle::new(1), Some(CONSOLE));
    }
}

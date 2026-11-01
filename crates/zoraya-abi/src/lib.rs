//! The native ABI (ADR-0032), shared by the kernel and user programs.
//!
//! Trap: `svc #0`, number in x8, arguments in x0 to x5. Return: x0 is a
//! [`Status`], x1 a value. Every other register is preserved.
//!
//! One declaration, [`syscalls!`], makes [`Syscall`], [`SYSCALLS`], the
//! [`Handlers`] trait the kernel implements, [`dispatch`], and the user side
//! [`stubs`] (ADR-0044). [`SYSCALLS`] states each call's parameters as data,
//! so that tests can check the rules of INV-0045-1, INV-0004-1 and INV-0006-2
//! over the whole ABI.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

pub mod spawn;

macro_rules! statuses {
    ($($(#[$doc:meta])* $name:ident = $value:literal,)*) => {
        /// Result of a system call. There is no `Interrupted`: nothing but the
        /// ways ADR-0006 rule 5 lists ends a wait.
        #[repr(u64)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Status {
            $($(#[$doc])* $name = $value,)*
        }

        impl Status {
            /// Every status, from the one list that defines them.
            pub const ALL: &[Status] = &[$(Status::$name),*];

            pub const fn from_raw(raw: u64) -> Option<Status> {
                Some(match raw {
                    $($value => Status::$name,)*
                    _ => return None,
                })
            }
        }
    };
}

statuses! {
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
    /// The handle lacks a right the call needs, or asked for more than it has.
    AccessDenied = 6,
    /// A fixed limit was reached: handles, packets, requests or job depth.
    LimitReached = 7,
    /// No such name, or nothing to take.
    NotFound = 8,
    /// The object is not in a state that allows this.
    BadState = 9,
    /// The deadline passed.
    DeadlineExceeded = 10,
    /// The port was closed during the wait.
    Cancelled = 11,
}

/// Slots in a handle table (ADR-0040). Slot 0 does not exist.
pub const MAX_HANDLES: u32 = 1024;
const SLOT_BITS: u32 = 16;

/// Names an object in a process's handle table: a slot and a generation
/// (ADR-0040). Zero is never valid. A handle present at start has generation
/// 0, so its value is its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(u32);

impl Handle {
    pub const fn new(raw: u32) -> Option<Handle> {
        if raw == 0 { None } else { Some(Handle(raw)) }
    }

    /// The handle at `slot`, of the given generation.
    pub const fn from_parts(slot: u32, generation: u16) -> Option<Handle> {
        if slot == 0 || slot > MAX_HANDLES {
            None
        } else {
            Some(Handle(slot | (generation as u32) << SLOT_BITS))
        }
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn slot(self) -> u32 {
        self.0 & ((1 << SLOT_BITS) - 1)
    }

    pub const fn generation(self) -> u16 {
        (self.0 >> SLOT_BITS) as u16
    }
}

/// What a process started by the kernel holds (ADR-0043). A child holds what
/// its parent listed, and nothing else.
pub const CONSOLE: Handle = Handle(1);
pub const JOB: Handle = Handle(2);
pub const DIRECTORY: Handle = Handle(3);
/// In the first process only (ADR-0062).
pub const POWER: Handle = Handle(4);

/// What a handle allows (ADR-0040). Rights only decrease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rights(u32);

impl Rights {
    pub const NONE: Rights = Rights(0);
    pub const DUPLICATE: Rights = Rights(1 << 0);
    pub const TRANSFER: Rights = Rights(1 << 1);
    pub const WAIT: Rights = Rights(1 << 2);
    pub const SIGNAL: Rights = Rights(1 << 3);
    pub const CREATE_PROCESS: Rights = Rights(1 << 4);
    pub const KILL: Rights = Rights(1 << 5);
    pub const RESUME: Rights = Rights(1 << 6);
    pub const READ: Rights = Rights(1 << 7);
    pub const WRITE: Rights = Rights(1 << 8);
    pub const LOOKUP: Rights = Rights(1 << 9);
    pub const POWER: Rights = Rights(1 << 10);
    const KNOWN: u32 = (1 << 11) - 1;

    /// Unknown bits are refused (CP-11).
    pub const fn from_raw(raw: u32) -> Option<Rights> {
        if raw & !Rights::KNOWN == 0 {
            Some(Rights(raw))
        } else {
            None
        }
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn union(self, other: Rights) -> Rights {
        Rights(self.0 | other.0)
    }

    pub const fn without(self, other: Rights) -> Rights {
        Rights(self.0 & !other.0)
    }

    /// True if this has every right in `other`.
    pub const fn contains(self, other: Rights) -> bool {
        self.0 & other.0 == other.0
    }
}

/// How a type answers ADR-0006 rule 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waiting {
    /// The bits a port can subscribe to.
    Signals(u32),
    NotWaitable,
}

/// A kernel object type. Every type must state its [`Waiting`], which is the
/// compile-time half of INV-0006-1.
pub trait ObjectType {
    const NAME: &'static str;
    const WAITING: Waiting;
    /// All the rights a handle to this type can carry.
    const RIGHTS: Rights;
}

/// Signal bits of a process (ADR-0006 rule 1).
pub mod process_signals {
    pub const TERMINATED: u32 = 1 << 0;
    pub const REQUEST_PENDING: u32 = 1 << 1;
    pub const SUSPENDED: u32 = 1 << 2;
}

/// Signal bits of a job (ADR-0006 rule 1).
pub mod job_signals {
    pub const TERMINATED: u32 = 1 << 0;
    /// Reserved: `job_suspend` is not built yet (ADR-0046).
    pub const SUSPENDED: u32 = 1 << 2;
}

/// Signal bits of the console (ADR-0060).
pub mod console_signals {
    /// Asserted exactly while input waits.
    pub const READABLE: u32 = 1 << 0;
}

const COMMON: Rights = Rights::DUPLICATE.union(Rights::TRANSFER);

macro_rules! object_types {
    ($($type:ident $name:literal $waiting:expr, $rights:expr;)*) => {
        $(
            pub struct $type;

            impl ObjectType for $type {
                const NAME: &'static str = $name;
                const WAITING: Waiting = $waiting;
                const RIGHTS: Rights = $rights;
            }
        )*

        /// Every object type, as data.
        pub const OBJECT_TYPES: &[(&str, Waiting, Rights)] =
            &[$((<$type>::NAME, <$type>::WAITING, <$type>::RIGHTS)),*];
    };
}

object_types! {
    ProcessObject "process"
        Waiting::Signals(
            process_signals::TERMINATED
                | process_signals::REQUEST_PENDING
                | process_signals::SUSPENDED
        ),
        COMMON.union(Rights::WAIT).union(Rights::SIGNAL).union(Rights::RESUME);
    JobObject "job"
        Waiting::Signals(job_signals::TERMINATED),
        COMMON
            .union(Rights::WAIT)
            .union(Rights::SIGNAL)
            .union(Rights::CREATE_PROCESS)
            .union(Rights::KILL);
    PortObject "port"
        Waiting::NotWaitable,
        Rights::TRANSFER.union(Rights::WAIT).union(Rights::WRITE);
    ConsoleObject "console"
        Waiting::Signals(console_signals::READABLE),
        COMMON
            .union(Rights::READ)
            .union(Rights::WRITE)
            .union(Rights::WAIT);
    PowerObject "power" Waiting::NotWaitable, COMMON.union(Rights::POWER);
    DirectoryObject "directory" Waiting::NotWaitable, COMMON.union(Rights::LOOKUP);
    ImageObject "image" Waiting::NotWaitable, COMMON.union(Rights::READ);
}

/// `Deadline::Infinite` on the wire. Any other value is an instant in
/// nanoseconds on the monotonic clock.
pub const DEADLINE_INFINITE: u64 = u64::MAX;

/// Why a process ended (ADR-0004 rule 1).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Exited = 0,
    Terminated = 1,
    Killed = 2,
    Fault = 3,
    Limit = 4,
}

impl ExitReason {
    pub const fn from_raw(raw: u32) -> Option<ExitReason> {
        Some(match raw {
            0 => ExitReason::Exited,
            1 => ExitReason::Terminated,
            2 => ExitReason::Killed,
            3 => ExitReason::Fault,
            4 => ExitReason::Limit,
            _ => return None,
        })
    }
}

/// What a process leaves behind (ADR-0004 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitRecord {
    pub code: i32,
    pub reason: ExitReason,
    pub detail: u32,
    /// The request that ended the process, if one did (ADR-0046).
    pub request_kind: Option<RequestKind>,
}

impl ExitRecord {
    pub const SIZE: usize = 16;

    pub fn to_bytes(self) -> [u8; ExitRecord::SIZE] {
        let mut b = [0; ExitRecord::SIZE];
        b[0..4].copy_from_slice(&self.code.to_le_bytes());
        b[4..8].copy_from_slice(&(self.reason as u32).to_le_bytes());
        b[8..12].copy_from_slice(&self.detail.to_le_bytes());
        let kind = match self.request_kind {
            Some(kind) => kind.raw() + 1,
            None => 0,
        };
        b[12..16].copy_from_slice(&kind.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8; ExitRecord::SIZE]) -> Option<ExitRecord> {
        let word = |at: usize| [b[at], b[at + 1], b[at + 2], b[at + 3]];
        Some(ExitRecord {
            code: i32::from_le_bytes(word(0)),
            reason: ExitReason::from_raw(u32::from_le_bytes(word(4)))?,
            detail: u32::from_le_bytes(word(8)),
            request_kind: match u32::from_le_bytes(word(12)).checked_sub(1) {
                Some(raw) => Some(RequestKind::from_raw(raw)?),
                None => None,
            },
        })
    }
}

/// Custom request slots (ADR-0004 rule 2).
pub const CUSTOM_REQUESTS: u32 = 32;
/// Request kinds, fixed and custom.
pub const REQUEST_KINDS: usize = 4 + CUSTOM_REQUESTS as usize;

/// What a request asks for (ADR-0004 rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Interrupt,
    Terminate,
    Hangup,
    Quit,
    Custom(u32),
}

impl RequestKind {
    pub const fn from_raw(raw: u32) -> Option<RequestKind> {
        Some(match raw {
            0 => RequestKind::Interrupt,
            1 => RequestKind::Terminate,
            2 => RequestKind::Hangup,
            3 => RequestKind::Quit,
            n if n >= 4 && n - 4 < CUSTOM_REQUESTS => RequestKind::Custom(n - 4),
            _ => return None,
        })
    }

    pub const fn raw(self) -> u32 {
        match self {
            RequestKind::Interrupt => 0,
            RequestKind::Terminate => 1,
            RequestKind::Hangup => 2,
            RequestKind::Quit => 3,
            RequestKind::Custom(n) => 4 + n,
        }
    }
}

/// What a request does to a process (ADR-0004 rule 2).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Disposition {
    #[default]
    Terminate = 0,
    Ignore = 1,
    Deliver = 2,
}

impl Disposition {
    pub const fn from_raw(raw: u32) -> Option<Disposition> {
        Some(match raw {
            0 => Disposition::Terminate,
            1 => Disposition::Ignore,
            2 => Disposition::Deliver,
            _ => return None,
        })
    }
}

/// ADR-0006 rule 2.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Level = 0,
    Edge = 1,
}

impl Mode {
    pub const fn from_raw(raw: u32) -> Option<Mode> {
        Some(match raw {
            0 => Mode::Level,
            1 => Mode::Edge,
            _ => return None,
        })
    }
}

/// `Packet::kind` of a signal packet.
pub const PACKET_SIGNAL: u32 = 0;
/// `Packet::kind` of a user packet.
pub const PACKET_USER: u32 = 1;

/// What `port_wait` returns (ADR-0006 rule 2). `Completion` is reserved.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Packet {
    pub key: u64,
    pub kind: u32,
    /// For a signal packet, the bits asserted.
    pub signals: u32,
    /// For a user packet, what was queued.
    pub data: u64,
}

impl Packet {
    pub const SIZE: usize = 24;

    pub fn to_bytes(self) -> [u8; Packet::SIZE] {
        let mut b = [0; Packet::SIZE];
        b[0..8].copy_from_slice(&self.key.to_le_bytes());
        b[8..12].copy_from_slice(&self.kind.to_le_bytes());
        b[12..16].copy_from_slice(&self.signals.to_le_bytes());
        b[16..24].copy_from_slice(&self.data.to_le_bytes());
        b
    }
}

/// A name relative to a directory (INV-0045-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelativePath<'a>(&'a str);

impl<'a> RelativePath<'a> {
    pub fn new(path: &'a str) -> Option<RelativePath<'a>> {
        if path.is_empty() || path.starts_with('/') {
            None
        } else {
            Some(RelativePath(path))
        }
    }

    pub fn as_str(self) -> &'a str {
        self.0
    }
}

/// The kinds a parameter may have. Adding a kind is how the ABI could break
/// the rules, so the tests below list which are allowed where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    Handle,
    /// A pointer and a length, in two registers.
    Buffer,
    /// A pointer and a count of [`Packet`]s, in two registers.
    Packets,
    /// A pointer and a length: the flat buffer of [`spawn`], vectors within.
    SpawnArgs,
    /// A pointer and a length: a [`RelativePath`].
    RelativePath,
    Deadline,
    /// Bits or a small enumeration, checked against what the call knows.
    Flags,
    /// `ExitRecord.code`: opaque to the kernel (ADR-0004 rule 1).
    ExitCode,
    /// `detail`: opaque to the kernel (ADR-0004).
    Detail,
    /// A port key or user packet data: opaque to the kernel (ADR-0045).
    Opaque,
    /// A place in a listing: opaque to the caller (ADR-0061).
    Cookie,
    /// Code to run. Only thread creation may take one (INV-0004-1).
    EntryPoint,
}

impl Param {
    pub const fn registers(self) -> usize {
        match self {
            Param::Buffer | Param::Packets | Param::SpawnArgs | Param::RelativePath => 2,
            _ => 1,
        }
    }
}

/// A value that crosses in registers. `decode` refuses anything too wide for
/// the type, and never truncates (INV-0044-2).
pub trait Arg: Sized {
    const PARAM: Param;
    fn decode(registers: &[u64]) -> Result<Self, Status>;
    fn encode(&self, registers: &mut [u64]);
}

fn word(register: u64) -> Result<u32, Status> {
    u32::try_from(register).map_err(|_| Status::InvalidArgument)
}

impl Arg for Handle {
    const PARAM: Param = Param::Handle;

    fn decode(r: &[u64]) -> Result<Handle, Status> {
        u32::try_from(r[0])
            .ok()
            .and_then(Handle::new)
            .ok_or(Status::BadHandle)
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = u64::from(self.0);
    }
}

/// A `Deadline` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline(pub u64);

impl Deadline {
    pub const INFINITE: Deadline = Deadline(DEADLINE_INFINITE);
}

impl Arg for Deadline {
    const PARAM: Param = Param::Deadline;

    fn decode(r: &[u64]) -> Result<Deadline, Status> {
        Ok(Deadline(r[0]))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = self.0;
    }
}

/// The `code` of an `ExitRecord`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitCode(pub i32);

impl Arg for ExitCode {
    const PARAM: Param = Param::ExitCode;

    fn decode(r: &[u64]) -> Result<ExitCode, Status> {
        Ok(ExitCode(word(r[0])? as i32))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = u64::from(self.0 as u32);
    }
}

/// A `detail` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detail(pub u32);

impl Arg for Detail {
    const PARAM: Param = Param::Detail;

    fn decode(r: &[u64]) -> Result<Detail, Status> {
        Ok(Detail(word(r[0])?))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = u64::from(self.0);
    }
}

/// A key or user data: any value, never interpreted (ADR-0045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opaque(pub u64);

impl Arg for Opaque {
    const PARAM: Param = Param::Opaque;

    fn decode(r: &[u64]) -> Result<Opaque, Status> {
        Ok(Opaque(r[0]))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = self.0;
    }
}

/// A place in a listing: 0 is the start (ADR-0061).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cookie(pub u64);

impl Arg for Cookie {
    const PARAM: Param = Param::Cookie;

    fn decode(r: &[u64]) -> Result<Cookie, Status> {
        Ok(Cookie(r[0]))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = self.0;
    }
}

/// Signal bits. The call checks them against the object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalSet(pub u32);

impl Arg for SignalSet {
    const PARAM: Param = Param::Flags;

    fn decode(r: &[u64]) -> Result<SignalSet, Status> {
        Ok(SignalSet(word(r[0])?))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = u64::from(self.0);
    }
}

/// `job_create`'s flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobFlags(u32);

impl JobFlags {
    pub const NONE: JobFlags = JobFlags(0);
    /// The `posix` policy bit (ADR-0002 rule 3).
    pub const POSIX: JobFlags = JobFlags(1);

    pub const fn is_posix(self) -> bool {
        self.0 & 1 != 0
    }
}

impl Arg for JobFlags {
    const PARAM: Param = Param::Flags;

    fn decode(r: &[u64]) -> Result<JobFlags, Status> {
        match word(r[0])? {
            raw @ (0 | 1) => Ok(JobFlags(raw)),
            _ => Err(Status::InvalidArgument),
        }
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = u64::from(self.0);
    }
}

macro_rules! flag_arg {
    ($type:ty, $from:expr, $to:expr) => {
        impl Arg for $type {
            const PARAM: Param = Param::Flags;

            fn decode(r: &[u64]) -> Result<$type, Status> {
                $from(word(r[0])?).ok_or(Status::InvalidArgument)
            }

            fn encode(&self, r: &mut [u64]) {
                r[0] = u64::from($to(*self));
            }
        }
    };
}

flag_arg!(Rights, Rights::from_raw, Rights::raw);
flag_arg!(RequestKind, RequestKind::from_raw, RequestKind::raw);
flag_arg!(Disposition, Disposition::from_raw, |d| d as u32);
flag_arg!(Mode, Mode::from_raw, |m| m as u32);
flag_arg!(ExitReason, ExitReason::from_raw, |r| r as u32);

/// A range of user memory, in the caller's terms: nothing here checks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Buffer {
    pub address: u64,
    pub length: u64,
}

macro_rules! buffer_arg {
    ($name:ident, $param:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(pub Buffer);

        impl Arg for $name {
            const PARAM: Param = $param;

            fn decode(r: &[u64]) -> Result<$name, Status> {
                Ok($name(Buffer {
                    address: r[0],
                    length: r[1],
                }))
            }

            fn encode(&self, r: &mut [u64]) {
                r[0] = self.0.address;
                r[1] = self.0.length;
            }
        }
    };
}

buffer_arg!(Bytes, Param::Buffer);
buffer_arg!(PacketBuffer, Param::Packets);
buffer_arg!(SpawnBuffer, Param::SpawnArgs);
buffer_arg!(PathBuffer, Param::RelativePath);

/// Enters the kernel. User programs implement this with `svc`.
pub trait Invoke {
    fn invoke(number: u64, registers: [u64; 6]) -> (u64, u64);
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

macro_rules! syscalls {
    ($($number:literal $variant:ident $name:ident($($arg:ident: $type:ty),*) $blocking:literal;)*) => {
        /// System call numbers.
        #[repr(u64)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Syscall {
            $($variant = $number,)*
        }

        impl Syscall {
            pub const fn from_number(n: u64) -> Option<Syscall> {
                Some(match n {
                    $($number => Syscall::$variant,)*
                    _ => return None,
                })
            }
        }

        pub const SYSCALLS: &[Signature] = &[$(Signature {
            name: stringify!($name),
            call: Syscall::$variant,
            params: &[$(<$type as Arg>::PARAM),*],
            blocking: $blocking,
        }),*];

        /// What the kernel implements, one method per call.
        pub trait Handlers {
            $(fn $name(&mut self, $($arg: $type),*) -> Result<u64, Status>;)*
        }

        /// Decodes the registers by type and calls the handler.
        pub fn dispatch<H: Handlers>(
            handlers: &mut H,
            number: u64,
            registers: &[u64; 6],
        ) -> Result<u64, Status> {
            match Syscall::from_number(number) {
                None => Err(Status::BadSyscall),
                $(Some(Syscall::$variant) => {
                    let mut _at = 0;
                    $(
                        let $arg = <$type as Arg>::decode(&registers[_at..])?;
                        _at += <$type as Arg>::PARAM.registers();
                    )*
                    handlers.$name($($arg),*)
                })*
            }
        }

        /// The calls as typed functions, for user programs.
        pub mod stubs {
            use super::*;

            fn finish((status, value): (u64, u64)) -> Result<u64, Status> {
                match Status::from_raw(status) {
                    Some(Status::Ok) => Ok(value),
                    Some(status) => Err(status),
                    None => Err(Status::BadSyscall),
                }
            }

            $(
                pub fn $name<I: Invoke>($($arg: $type),*) -> Result<u64, Status> {
                    #[allow(unused_mut)] // A call may have no arguments.
                    let mut registers = [0; 6];
                    let mut _at = 0;
                    $(
                        Arg::encode(&$arg, &mut registers[_at..]);
                        _at += <$type as Arg>::PARAM.registers();
                    )*
                    finish(I::invoke($number, registers))
                }
            )*
        }
    };
}

syscalls! {
    0 ProcessExit process_exit(code: ExitCode) false;
    1 ThreadYield thread_yield() false;
    2 ConsoleWrite console_write(console: Handle, bytes: Bytes) false;
    3 ClockMonotonic clock_monotonic() false;
    4 SleepUntil sleep_until(deadline: Deadline) true;
    5 HandleClose handle_close(handle: Handle) false;
    6 HandleDuplicate handle_duplicate(handle: Handle, rights: Rights) false;
    7 JobCreate job_create(parent: Handle, flags: JobFlags) false;
    8 Spawn spawn(args: SpawnBuffer) false;
    9 ProcessResume process_resume(process: Handle) false;
    10 ProcessExitWithReason process_exit_with_reason(reason: ExitReason, detail: Detail) false;
    11 ProcessExitRecord process_exit_record(process: Handle, record: Bytes) false;
    12 ProcessSignal process_signal(process: Handle, kind: RequestKind, detail: Detail) false;
    13 ProcessSetDisposition process_set_disposition(
        kind: RequestKind, disposition: Disposition) false;
    14 ProcessTakeRequest process_take_request() false;
    15 JobSignal job_signal(job: Handle, kind: RequestKind, detail: Detail) false;
    16 JobKill job_kill(job: Handle) false;
    17 PortCreate port_create() false;
    18 PortSubscribe port_subscribe(
        port: Handle, handle: Handle, signals: SignalSet, key: Opaque, mode: Mode) false;
    19 PortUnsubscribe port_unsubscribe(port: Handle, key: Opaque) false;
    20 PortQueue port_queue(port: Handle, key: Opaque, data: Opaque) false;
    21 PortWait port_wait(port: Handle, deadline: Deadline, packets: PacketBuffer) true;
    22 DirectoryLookup directory_lookup(directory: Handle, name: PathBuffer) false;
    23 ConsoleRead console_read(console: Handle, bytes: Bytes) false;
    24 ConsoleDropped console_dropped(console: Handle) false;
    25 DirectoryEntry directory_entry(directory: Handle, cookie: Cookie, entry: Bytes) false;
    26 PowerOff power_off(power: Handle) false;
}

/// One name in a listing (ADR-0061): the next cookie, 0 at the last name, then
/// the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryEntry<'a> {
    pub next: u64,
    pub name: &'a [u8],
}

impl<'a> DirectoryEntry<'a> {
    pub const HEADER_SIZE: usize = 16;

    /// Writes the record, and gives its size, or `None` if it does not fit.
    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        let size = DirectoryEntry::HEADER_SIZE.checked_add(self.name.len())?;
        let out = out.get_mut(..size)?;
        out[..8].copy_from_slice(&self.next.to_le_bytes());
        out[8..12].copy_from_slice(&(self.name.len() as u32).to_le_bytes());
        out[12..16].fill(0);
        out[16..].copy_from_slice(self.name);
        Some(size)
    }

    /// Total: any bytes give a record or `None`.
    pub fn decode(bytes: &'a [u8]) -> Option<DirectoryEntry<'a>> {
        let next = u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?);
        let length = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?) as usize;
        if bytes.get(12..16)? != [0; 4] {
            return None;
        }
        let name = bytes.get(16..)?.get(..length)?;
        Some(DirectoryEntry { next, name })
    }
}

/// `process_take_request`'s value, a kind and a detail in one register.
pub const fn request_value(kind: RequestKind, detail: u32) -> u64 {
    (kind.raw() as u64) << 32 | detail as u64
}

/// The inverse of [`request_value`].
pub const fn request_from_value(value: u64) -> Option<(RequestKind, u32)> {
    match RequestKind::from_raw((value >> 32) as u32) {
        Some(kind) => Some((kind, value as u32)),
        None => None,
    }
}

#[cfg(test)]
mod tests;

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

pub mod posix;
pub mod snapshot;
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
    /// Nothing to read, or a queue is full: try when signalled.
    WouldBlock = 12,
    /// The other end of a channel is gone.
    PeerClosed = 13,
    /// The message stays; the sizes it needs have been reported.
    BufferTooSmall = 14,
}

/// Slots in a handle table (ADR-0040). Slot 0 does not exist.
pub const MAX_HANDLES: u32 = 1024;
const SLOT_BITS: u32 = 16;
/// The last generation of a slot, 48 bits (ADR-0057).
pub const MAX_GENERATION: u64 = (1 << 48) - 1;

/// Names an object in a process's handle table: a slot and a generation
/// (ADR-0040). Zero is never valid. A handle present at start has generation
/// 0, so its value is its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(u64);

impl Handle {
    pub const fn new(raw: u64) -> Option<Handle> {
        if raw == 0 { None } else { Some(Handle(raw)) }
    }

    /// The handle at `slot`, of the given generation.
    pub const fn from_parts(slot: u32, generation: u64) -> Option<Handle> {
        if slot == 0 || slot > MAX_HANDLES || generation > MAX_GENERATION {
            None
        } else {
            Some(Handle(slot as u64 | generation << SLOT_BITS))
        }
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn slot(self) -> u32 {
        (self.0 & ((1 << SLOT_BITS) - 1)) as u32
    }

    pub const fn generation(self) -> u64 {
        self.0 >> SLOT_BITS
    }
}

/// What a process started by the kernel holds (ADR-0043). A child holds what
/// its parent listed, and nothing else.
pub const CONSOLE: Handle = Handle(1);
pub const JOB: Handle = Handle(2);
pub const DIRECTORY: Handle = Handle(3);
/// In the first process only (ADR-0062).
pub const POWER: Handle = Handle(4);
/// The first process's first device, then one a slot (ADR-0053).
pub const FIRST_DEVICE_SLOT: u32 = 5;

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
    pub const MAP: Rights = Rights(1 << 11);
    pub const INTERRUPT: Rights = Rights(1 << 12);
    pub const DMA: Rights = Rights(1 << 13);
    const KNOWN: u32 = (1 << 14) - 1;

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

/// Whether `snapshot_process` duplicates a handle of a type (ADR-0102 item 3,
/// ADR-0040's follow-up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duplication {
    /// Duplicated into a snapshot's child, if the handle has `DUPLICATE`.
    Duplicable,
    /// Never duplicated: absent in the child unless the move list names it.
    NotDuplicable,
}

/// A kernel object type. Every type must state its [`Waiting`], which is the
/// compile-time half of INV-0006-1.
pub trait ObjectType {
    const NAME: &'static str;
    const WAITING: Waiting;
    /// All the rights a handle to this type can carry.
    const RIGHTS: Rights;
    const DUPLICATION: Duplication;
}

/// One row of [`OBJECT_TYPES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeInfo {
    pub name: &'static str,
    pub waiting: Waiting,
    pub rights: Rights,
    pub duplication: Duplication,
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
    /// Between `job_suspend` and `job_resume` of this job or one above it
    /// (ADR-0105 item 8).
    pub const SUSPENDED: u32 = 1 << 2;
}

/// Signal bits of the console (ADR-0060).
pub mod console_signals {
    /// Asserted exactly while input waits.
    pub const READABLE: u32 = 1 << 0;
}

/// Signal bits of a channel end (ADR-0050).
pub mod channel_signals {
    /// A message waits.
    pub const READABLE: u32 = 1 << 0;
    /// Any message would fit.
    pub const WRITABLE: u32 = 1 << 1;
    pub const PEER_CLOSED: u32 = 1 << 2;
}

/// Signal bits of a byte stream's ends (ADR-0101). The read end has
/// `READABLE` and `PEER_CLOSED`, the write end `WRITABLE` and `PEER_CLOSED`.
pub mod stream_signals {
    /// A byte waits, or no write end exists (so a read sees end of file).
    pub const READABLE: u32 = 1 << 0;
    /// `STREAM_ATOMIC` bytes are free and a read end exists.
    pub const WRITABLE: u32 = 1 << 1;
    /// No handle to the other end exists any more.
    pub const PEER_CLOSED: u32 = 1 << 2;
}

/// Bytes a stream holds (ADR-0101).
pub const STREAM_CAPACITY: usize = 64 * 1024;
/// A `stream_write` of at most this many bytes is all or nothing (ADR-0101).
pub const STREAM_ATOMIC: usize = 4096;

/// Signal bits of an interrupt object (ADR-0054).
pub mod interrupt_signals {
    /// The line fired, and stays masked until `interrupt_ack`.
    pub const INTERRUPT: u32 = 1 << 0;
}

const COMMON: Rights = Rights::DUPLICATE.union(Rights::TRANSFER);

macro_rules! object_types {
    ($($type:ident $name:literal $waiting:expr, $rights:expr, $duplication:ident;)*) => {
        $(
            pub struct $type;

            impl ObjectType for $type {
                const NAME: &'static str = $name;
                const WAITING: Waiting = $waiting;
                const RIGHTS: Rights = $rights;
                const DUPLICATION: Duplication = Duplication::$duplication;
            }
        )*

        /// Every object type, as data. A handle's type is its index here.
        pub const OBJECT_TYPES: &[TypeInfo] = &[$(TypeInfo {
            name: <$type>::NAME,
            waiting: <$type>::WAITING,
            rights: <$type>::RIGHTS,
            duplication: <$type>::DUPLICATION,
        }),*];
    };
}

object_types! {
    ProcessObject "process"
        Waiting::Signals(
            process_signals::TERMINATED
                | process_signals::REQUEST_PENDING
                | process_signals::SUSPENDED
        ),
        COMMON.union(Rights::WAIT).union(Rights::SIGNAL).union(Rights::RESUME),
        Duplicable;
    JobObject "job"
        Waiting::Signals(job_signals::TERMINATED | job_signals::SUSPENDED),
        COMMON
            .union(Rights::WAIT)
            .union(Rights::SIGNAL)
            .union(Rights::CREATE_PROCESS)
            .union(Rights::KILL),
        Duplicable;
    PortObject "port"
        Waiting::NotWaitable,
        Rights::TRANSFER.union(Rights::WAIT).union(Rights::WRITE),
        NotDuplicable;
    ConsoleObject "console"
        Waiting::Signals(console_signals::READABLE),
        COMMON
            .union(Rights::READ)
            .union(Rights::WRITE)
            .union(Rights::WAIT),
        Duplicable;
    PowerObject "power" Waiting::NotWaitable, COMMON.union(Rights::POWER), Duplicable;
    ChannelObject "channel"
        Waiting::Signals(
            channel_signals::READABLE | channel_signals::WRITABLE | channel_signals::PEER_CLOSED
        ),
        Rights::READ
            .union(Rights::WRITE)
            .union(Rights::WAIT)
            .union(Rights::TRANSFER),
        NotDuplicable;
    DeviceObject "device"
        Waiting::NotWaitable,
        COMMON
            .union(Rights::MAP)
            .union(Rights::INTERRUPT)
            .union(Rights::DMA),
        Duplicable;
    InterruptObject "interrupt"
        Waiting::Signals(interrupt_signals::INTERRUPT),
        Rights::WAIT.union(Rights::WRITE).union(Rights::TRANSFER),
        NotDuplicable;
    PinObject "pin" Waiting::NotWaitable, Rights::DMA.union(Rights::TRANSFER), NotDuplicable;
    EventObject "event" Waiting::NotWaitable, COMMON, Duplicable;
    MemoryObject "memory"
        Waiting::NotWaitable,
        COMMON.union(Rights::READ).union(Rights::WRITE),
        Duplicable;
    DirectoryObject "directory" Waiting::NotWaitable, COMMON.union(Rights::LOOKUP), Duplicable;
    ImageObject "image" Waiting::NotWaitable, COMMON.union(Rights::READ), Duplicable;
    StreamReadObject "stream-read"
        Waiting::Signals(stream_signals::READABLE | stream_signals::PEER_CLOSED),
        COMMON.union(Rights::READ).union(Rights::WAIT),
        Duplicable;
    StreamWriteObject "stream-write"
        Waiting::Signals(stream_signals::WRITABLE | stream_signals::PEER_CLOSED),
        COMMON.union(Rights::WRITE).union(Rights::WAIT),
        Duplicable;
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
    /// A pointer and a length: a flat buffer of ADR-0042, the one of [`spawn`]
    /// (vectors within) or of [`snapshot`].
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
    /// A size or offset in a memory object (ADR-0051).
    Size,
    /// A pointer to a record of fixed size, in one register (ADR-0050).
    Out,
    /// Which range of a device: it names no object (ADR-0054).
    RangeIndex,
    /// A place in the caller's own space: it names no object (ADR-0051).
    Address,
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
        Handle::new(r[0]).ok_or(Status::BadHandle)
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = self.0;
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

/// A size or offset in bytes (ADR-0051).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size(pub u64);

impl Arg for Size {
    const PARAM: Param = Param::Size;

    fn decode(r: &[u64]) -> Result<Size, Status> {
        Ok(Size(r[0]))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = self.0;
    }
}

/// Where a call writes a record of fixed size (ADR-0050).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Out(pub u64);

impl Arg for Out {
    const PARAM: Param = Param::Out;

    fn decode(r: &[u64]) -> Result<Out, Status> {
        Ok(Out(r[0]))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = self.0;
    }
}

/// A range of a device (ADR-0054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeIndex(pub u32);

impl Arg for RangeIndex {
    const PARAM: Param = Param::RangeIndex;

    fn decode(r: &[u64]) -> Result<RangeIndex, Status> {
        Ok(RangeIndex(word(r[0])?))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = u64::from(self.0);
    }
}

/// An address in the caller's space (ADR-0051).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Address(pub u64);

impl Arg for Address {
    const PARAM: Param = Param::Address;

    fn decode(r: &[u64]) -> Result<Address, Status> {
        Ok(Address(r[0]))
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = self.0;
    }
}

/// `memory_create`'s flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryFlags(u32);

impl MemoryFlags {
    pub const NONE: MemoryFlags = MemoryFlags(0);
    /// Physically contiguous frames, for a device's queues (ADR-0051).
    pub const CONTIGUOUS: MemoryFlags = MemoryFlags(1);

    pub const fn is_contiguous(self) -> bool {
        self.0 & 1 != 0
    }
}

impl Arg for MemoryFlags {
    const PARAM: Param = Param::Flags;

    fn decode(r: &[u64]) -> Result<MemoryFlags, Status> {
        match word(r[0])? {
            raw @ (0 | 1) => Ok(MemoryFlags(raw)),
            _ => Err(Status::InvalidArgument),
        }
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = u64::from(self.0);
    }
}

/// `memory_map`'s flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapFlags(u32);

impl MapFlags {
    pub const READ: MapFlags = MapFlags(0);
    pub const WRITE: MapFlags = MapFlags(1);

    pub const fn is_write(self) -> bool {
        self.0 & 1 != 0
    }
}

impl Arg for MapFlags {
    const PARAM: Param = Param::Flags;

    fn decode(r: &[u64]) -> Result<MapFlags, Status> {
        match word(r[0])? {
            raw @ (0 | 1) => Ok(MapFlags(raw)),
            _ => Err(Status::InvalidArgument),
        }
    }

    fn encode(&self, r: &mut [u64]) {
        r[0] = u64::from(self.0);
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
buffer_arg!(SnapshotBuffer, Param::SpawnArgs);
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
    /// Usable only by `posix` jobs: `#[personality_only]` (INV-0002-2).
    pub personality_only: bool,
}

/// What `#[personality_only]` on a call means; any other attribute does not
/// compile.
macro_rules! personality {
    () => {
        false
    };
    (personality_only) => {
        true
    };
}

macro_rules! syscalls {
    ($($(#[$tag:ident])? $number:literal $variant:ident $name:ident($($arg:ident: $type:ty),*) $blocking:literal;)*) => {
        /// System call numbers.
        #[repr(u64)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Syscall {
            $($variant = $number,)*
        }

        /// True if call `number` is `#[personality_only]` (INV-0002-2): what
        /// a dispatcher checks before it runs the call.
        pub const fn is_personality_only(number: u64) -> bool {
            match number {
                $($number => personality!($($tag)?),)*
                _ => false,
            }
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
            personality_only: personality!($($tag)?),
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
    27 MemoryCreate memory_create(size: Size, flags: MemoryFlags) false;
    28 MemoryMap memory_map(object: Handle, offset: Size, length: Size, flags: MapFlags) false;
    29 MemoryUnmap memory_unmap(address: Address, length: Size) false;
    30 ChannelCreate channel_create(ends: Bytes) false;
    31 ChannelWrite channel_write(end: Handle, bytes: Bytes, handles: Bytes) false;
    32 ChannelRead channel_read(end: Handle, bytes: Bytes, handles: Bytes, sizes: Out) false;
    33 DeviceMap device_map(device: Handle, index: RangeIndex, out: Bytes) false;
    34 DeviceInterrupt device_interrupt(device: Handle, index: RangeIndex) false;
    35 InterruptAck interrupt_ack(interrupt: Handle) false;
    36 DmaPin dma_pin(memory: Handle, device: Handle, out: Bytes) false;
    37 DmaUnpin dma_unpin(pin: Handle) false;
    38 DmaReleaseQuarantine dma_release_quarantine(device: Handle) false;
    39 HandleType handle_type(handle: Handle) false;
    40 EventCreate event_create() false;
    41 ObjectSame object_same(a: Handle, b: Handle) false;
    42 StreamCreate stream_create(ends: Bytes) false;
    43 StreamRead stream_read(end: Handle, bytes: Bytes) false;
    44 StreamWrite stream_write(end: Handle, bytes: Bytes) false;
    #[personality_only]
    45 SnapshotProcess snapshot_process(args: SnapshotBuffer) false;
    46 ImageCreate image_create(memory: Handle, length: Size) false;
    47 JobSuspend job_suspend(job: Handle) false;
    48 JobResume job_resume(job: Handle) false;
    49 ObjectGetSignals object_get_signals(handle: Handle) false;
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

/// One entry of a message's handle list: `spawn`'s, without the slot (ADR-0050).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelEntry {
    pub handle: Handle,
    pub rights: Rights,
    pub transfer: spawn::Transfer,
}

impl ChannelEntry {
    pub const SIZE: usize = 16;

    pub fn to_bytes(self) -> [u8; ChannelEntry::SIZE] {
        let mut b = [0; ChannelEntry::SIZE];
        b[0..8].copy_from_slice(&self.handle.raw().to_le_bytes());
        b[8..12].copy_from_slice(&self.rights.raw().to_le_bytes());
        b[12..16].copy_from_slice(&(self.transfer as u32).to_le_bytes());
        b
    }

    /// Total: any bytes give an entry or a status.
    pub fn from_bytes(b: &[u8; ChannelEntry::SIZE]) -> Result<ChannelEntry, Status> {
        let raw = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        let word = |at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        Ok(ChannelEntry {
            handle: Handle::new(raw).ok_or(Status::BadHandle)?,
            rights: Rights::from_raw(word(8)).ok_or(Status::InvalidArgument)?,
            transfer: match word(12) {
                0 => spawn::Transfer::Move,
                1 => spawn::Transfer::Duplicate,
                _ => return Err(Status::InvalidArgument),
            },
        })
    }
}

/// What `channel_read` writes through its `sizes`: the next message's sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelSizes {
    pub bytes: u32,
    pub handles: u32,
}

impl ChannelSizes {
    pub const SIZE: usize = 8;

    pub fn to_bytes(self) -> [u8; ChannelSizes::SIZE] {
        let mut b = [0; ChannelSizes::SIZE];
        b[0..4].copy_from_slice(&self.bytes.to_le_bytes());
        b[4..8].copy_from_slice(&self.handles.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8; ChannelSizes::SIZE]) -> ChannelSizes {
        ChannelSizes {
            bytes: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            handles: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        }
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

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

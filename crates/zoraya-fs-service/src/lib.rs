//! The filesystem service (ADR-0055) around `zoraya_fs::Volume`: one loop, one
//! request at a time. All IPC goes through [`Transport`], so the loop runs on
//! the host. With one thread and synchronous engine calls, the loop's order is
//! ADR-0010's completion order.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod block;
pub mod memory;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_fs::{
    Attr, BLOCK_SIZE, BlockDevice, Clock, FailReason, FsError, Handle, MAX_ATOMIC_WRITE, ObjKind,
    OpenOptions, PublishMode, RenameMode, Timestamp, Volume,
};
use zoraya_proto_file::{self as proto, Code, Kind as WireKind, status};
use zoraya_protocol::{
    Below, Data, ERROR_ORDINAL, ErrorReply, Header, MAX_WIRE_MESSAGE, Message, Name, Path, Seq,
    Text, decode, encode,
};

/// Handles the service holds for clients (ADR-0055).
pub const MAX_OPEN: usize = 896;
/// Client buffers mapped at once (ADR-0055).
pub const MAX_BUFFERS: usize = 240;
/// Replies held for clients that do not read them, for all accounts
/// together: one per connection at most, so as many as connections (ADR-0262
/// item 2's table; the session's account keeps what it has today).
pub const MAX_HELD: usize = MAX_OPEN;
/// Accounts carved from the session's, at once (ADR-0262 item 2: four
/// untrusted rooms, ADR-0269).
pub const MAX_UNTRUSTED_ACCOUNTS: usize = 4;
/// The most open handles one untrusted account may be given.
pub const UNTRUSTED_OPEN: usize = 32;
/// The mapped buffers an account of `budget` open handles may hold: an
/// eighth, at least one (4 for the untrusted 32).
pub const fn buffers_for(budget: usize) -> usize {
    let n = budget / 8;
    if n == 0 { 1 } else { n }
}
/// The held replies an account of `budget` open handles may have: a
/// thirty-second, at least one (1 for the untrusted 32).
pub const fn held_for(budget: usize) -> usize {
    let n = budget / 32;
    if n == 0 { 1 } else { n }
}
/// How long a reply that met `NoMemory` waits before the next try.
const RETRY_NS: u64 = 10_000_000;

/// How many times a supervisor may start the stack again (ADR-0080).
pub const MAX_RESTARTS: u32 = 3;

/// A supervisor's count of restarts.
#[derive(Debug, Default)]
pub struct Restarts(u32);

impl Restarts {
    pub const fn new() -> Restarts {
        Restarts(0)
    }

    /// True if one more restart is allowed, and counts it.
    pub fn take(&mut self) -> bool {
        if self.0 < MAX_RESTARTS {
            self.0 += 1;
            true
        } else {
            false
        }
    }
}

/// What a port should wake the service for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interest {
    /// `READABLE | PEER_CLOSED`.
    Requests,
    /// `WRITABLE | PEER_CLOSED`.
    Reply,
    /// `PEER_CLOSED` alone.
    Closed,
}

/// What came on a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incoming {
    /// `bytes` bytes, and the handles.
    Message(usize),
    Empty,
    Closed,
    /// It cannot be read: close the connection (ADR-0050).
    Refused,
}

/// What became of a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outgoing {
    Sent,
    /// The queue is full.
    Full,
    NoMemory,
    Closed,
}

/// What a handle is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    Channel,
    Memory,
    Token,
    Other,
}

/// What woke the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// The connection with this key has something.
    Ready(u64),
    /// The deadline passed.
    Deadline,
    /// The channel to the driver closed.
    DriverClosed,
    /// Stop: only tests and a supervisor's shutdown ask this.
    Stop,
}

/// The kernel, as far as the loop sees it.
pub trait Transport {
    type End: Copy + PartialEq;

    fn read(&mut self, end: Self::End, bytes: &mut [u8], handles: &mut Vec<Self::End>) -> Incoming;
    fn write(&mut self, end: Self::End, bytes: &[u8], handles: &[Self::End]) -> Outgoing;
    fn interest(&mut self, end: Self::End, key: u64, interest: Interest) -> bool;
    fn close(&mut self, end: Self::End);
    fn kind(&mut self, end: Self::End) -> HandleKind;
    fn duplicate(&mut self, end: Self::End) -> Option<Self::End>;
    /// A new token object (ADR-0080).
    fn new_token(&mut self) -> Option<Self::End>;
    fn same(&mut self, a: Self::End, b: Self::End) -> bool;
    /// Maps `length` bytes of a memory object, and gives its address.
    fn map(&mut self, memory: Self::End, length: u64) -> Option<u64>;
    fn unmap(&mut self, address: u64, length: u64);
    /// Copies out of client memory: once, before anything looks at it.
    fn copy_from(&mut self, address: u64, out: &mut [u8]) -> bool;
    fn copy_to(&mut self, address: u64, bytes: &[u8]) -> bool;
    /// Tells the supervisor, once.
    fn report(&mut self, reason: u32);
    /// Waits for work, up to the monotonic time `deadline`.
    fn wait(&mut self, deadline: Option<u64>) -> Wake;
    fn now(&mut self) -> u64;
    /// A byte stream's two ends, read end first (ADR-0101), for a FIFO.
    /// A service with no FIFOs never asks.
    fn stream(&mut self) -> Option<(Self::End, Self::End)> {
        None
    }
}

/// The device a service runs on: lost for good once the driver is.
pub trait Device: BlockDevice {
    fn is_lost(&self) -> bool;
    fn mark_lost(&mut self);
}

impl<L: block::Link> Device for block::ChannelDevice<L> {
    fn is_lost(&self) -> bool {
        block::ChannelDevice::is_lost(self)
    }

    fn mark_lost(&mut self) {
        block::ChannelDevice::mark_lost(self);
    }
}

/// Open-file state: a record a connection refers to, not part of it
/// (INV-0055-9). It is the open description of ADR-0103: `share` attaches
/// another connection to it, and every connection of it sees one position
/// and one append flag.
/// A FIFO node's streams and its open sides (ADR-0107 items 14 and 15).
/// The service holds its own end of each side while that side has a
/// connection, so that the other side sees an end only when the last one
/// goes.
struct Fifo<E> {
    read: Option<E>,
    write: Option<E>,
    readers: u32,
    writers: u32,
}

/// Where a record lies in relation to the sealed directory (ADR-0289 item 8).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seal {
    /// Anywhere else.
    Outside,
    /// The volume's root: the sealed name is the one entry it must keep.
    Root,
    /// The sealed directory or anything below it: nothing there can change.
    Sealed,
}

/// The rights that change what a volume holds. A record under the seal never
/// has them, on any connection (ADR-0289 item 8).
const MUTATING: u32 = proto::rights::WRITE
    | proto::rights::CREATE
    | proto::rights::REMOVE
    | proto::rights::RENAME
    | proto::rights::PUBLISH;

struct Record {
    handle: Handle,
    seal: Seal,
    connections: u32,
    /// Where the requests without an offset read and write.
    position: u64,
    /// `description_flags`.
    flags: u32,
}

/// The largest position: a file's size fits `off_t` (ADR-0014 rule 1).
const MAX_POSITION: u64 = i64::MAX as u64;

struct Buffer<E> {
    end: E,
    address: u64,
    length: u64,
}

struct HeldReply<E> {
    bytes: Vec<u8>,
    handles: Vec<E>,
}

/// The service's worst-case memory for a transport whose handle is `E`,
/// from its bounds (ADR-0262 "A service's size", INV-0262-3's size test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorstCase {
    /// The service's own buffers, whatever the clients do.
    pub fixed: usize,
    /// One open handle at its worst: a connection with its description and
    /// the names it keeps (`only`, the directory position).
    pub per_open: usize,
    /// One held reply at its worst: a full message and its handle.
    pub per_held: usize,
    /// One account's bookkeeping.
    pub per_account: usize,
}

impl WorstCase {
    /// What an account with `open` handles and `held` held replies can make
    /// the service keep.
    pub const fn of_account(&self, open: usize, held: usize) -> usize {
        self.per_account + open * self.per_open + held * self.per_held
    }

    /// The whole service with every bound used (the session's account and
    /// the untrusted ones together are the global bounds).
    pub const fn total(&self) -> usize {
        self.fixed
            + self.per_account * MAX_OPEN
            + MAX_OPEN * self.per_open
            + MAX_HELD * self.per_held
    }
}

/// The worst case of a service over transport handles of type `E`.
pub const fn worst_case<E>() -> WorstCase {
    let name = zoraya_fs::name::MAX_NAME;
    WorstCase {
        fixed: 2 * MAX_WIRE_MESSAGE + proto::MAX_INLINE,
        per_open: core::mem::size_of::<Option<Connection<E>>>()
            + core::mem::size_of::<Option<Record>>()
            // `only` and the directory position's name.
            + 2 * name,
        per_held: MAX_WIRE_MESSAGE + core::mem::size_of::<E>(),
        per_account: core::mem::size_of::<Option<Account>>(),
    }
}

/// What one client may make the service hold (ADR-0262 item 2): the
/// connections reached from one launcher grant. Each limit is the capacity
/// left for the account's own use; what it carved for sub-accounts is in
/// theirs, so the limits of all live accounts add up to the service's
/// bounds (INV-0262-3).
struct Account {
    /// The account this one was carved from, while it lives.
    parent: Option<usize>,
    /// False only for the session's account; `sync` of the others folds
    /// into the next commit (ADR-0265 item 9).
    untrusted: bool,
    /// Carved from the session's account: in the untrusted column.
    top: bool,
    connections: u32,
    open_limit: usize,
    open: usize,
    buffers_limit: usize,
    buffers: usize,
    held_limit: usize,
    held: usize,
    /// Requests refused at one of this account's bounds, for the launcher
    /// (ADR-0262 item 2, ADR-0263 item 3).
    refused: u64,
}

/// The accounts' totals, for tests and for `top` (ADR-0262).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AccountTotals {
    pub accounts: usize,
    pub open_limit: usize,
    pub open: usize,
    pub buffers_limit: usize,
    pub buffers: usize,
    pub held_limit: usize,
    pub held: usize,
    /// The accounts carved from the session's, and their limits.
    pub top_accounts: usize,
    pub top_open_limit: usize,
    pub top_buffers_limit: usize,
    /// The limits of every untrusted account, sub-accounts included: the
    /// untrusted column (128 open, 16 buffers, 4 held).
    pub untrusted_open_limit: usize,
    pub untrusted_buffers_limit: usize,
    pub untrusted_held_limit: usize,
    /// Refusals at all accounts' bounds so far, of accounts that live.
    pub refused: u64,
}

struct Connection<E> {
    end: E,
    record: usize,
    /// The account that pays for this connection and what it holds.
    account: usize,
    /// A `sync` (or a durable write) waiting for a commit: its transaction,
    /// the commit count when it came and the count its `Done` carries
    /// (ADR-0265 item 9).
    syncing: Option<(u32, u64, u64)>,
    rights: u32,
    buffer: Option<Buffer<E>>,
    token: Option<E>,
    /// A reply that did not fit: the service reads nothing more until it goes.
    held: Option<HeldReply<E>>,
    /// The name `read_directory` goes on from, and its cookie.
    position: Option<(u64, String)>,
    /// A narrowed connection's one name (ADR-0055's note): every request
    /// that names something must name exactly it.
    only: Option<String>,
    /// A FIFO side this connection opened: the object and the side.
    fifo: Option<(u64, u32)>,
    /// A `fifo_open` held until the other side comes (its transaction).
    opening: Option<u32>,
}

pub struct Service<D: Device, C: Clock, T: Transport> {
    volume: Volume<D, C>,
    transport: T,
    /// The FIFO nodes of a temporary directory service, by object id; `None`
    /// on a volume whose type bits are stored verbatim (ADR-0107).
    fifos: Option<BTreeMap<u64, Fifo<T::End>>>,
    /// The objects made as FIFOs. ADR-0014's format holds 12 mode bits, so
    /// the type bits ADR-0107 item 13 puts in `posix_mode` stay here, in the
    /// service of a volume that lives no longer than it (ADR-0190).
    fifo_nodes: BTreeSet<u64>,
    /// The `open_with_mode` being served makes a FIFO if it creates.
    fifo_create: bool,
    /// What `time_base` answers (ADR-0191).
    time_base: u64,
    /// The name of the root's entry that is read-only for everyone, with all
    /// below it (ADR-0289 item 8); `None` for a volume that has none.
    sealed: Option<String>,
    connections: Vec<Option<Connection<T::End>>>,
    records: Vec<Option<Record>>,
    /// Handles held for clients (ADR-0055).
    open: usize,
    buffers: usize,
    accounts: Vec<Option<Account>>,
    /// Connections with a `sync` waiting for a commit.
    syncs: Vec<usize>,
    /// A durable write of the request being served has deferred its `sync`.
    folding: bool,
    /// Reads reported as `StorageFailed`, not to be counted as corrupt.
    outage_reads: u64,
    told_lost: bool,
    /// The most blocks the space map held in use since the mount, sampled
    /// after every request; `status` answers it in bytes (ADR-0110 item 7's
    /// peak `/tmp`, ADR-0211).
    peak_used: u64,
    retry_at: Option<u64>,
    next_cookie: u64,
    /// Bytes every read so far gave a client: the requests at the position
    /// move it by what this grew.
    read_count: u64,
    input: Vec<u8>,
    output: Vec<u8>,
    data: Vec<u8>,
    handles: Vec<T::End>,
}

fn code(status: u32) -> Code {
    Below(status)
}

fn fs_status(e: &FsError) -> u32 {
    match e {
        FsError::NotFound => status::NOT_FOUND,
        FsError::Exists => status::EXISTS,
        FsError::NoSpace => status::NO_SPACE,
        FsError::NotEmpty => status::NOT_EMPTY,
        FsError::InvalidName(_) => status::INVALID_NAME,
        FsError::NotDir => status::NOT_DIRECTORY,
        FsError::IsDir => status::IS_DIRECTORY,
        FsError::IsLink { .. } => status::IS_LINK,
        FsError::InvalidArgument => status::INVALID_ARGUMENT,
        FsError::Corrupt => status::CORRUPT,
        FsError::OverQuota => status::OVER_QUOTA,
        FsError::CrossesLimit => status::CROSSES_LIMIT,
        FsError::ScanTooLarge => status::LIMIT_REACHED,
        _ => status::STORAGE_FAILED,
    }
}

fn wire_kind(kind: ObjKind) -> WireKind {
    Below(match kind {
        ObjKind::File => proto::kind::FILE,
        ObjKind::Directory => proto::kind::DIRECTORY,
        ObjKind::Link => proto::kind::LINK,
    })
}

/// A time in nanoseconds, held at the largest past the year 2554.
fn nanoseconds(t: Timestamp) -> u64 {
    (t.secs.max(0) as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(t.nanos))
}

/// The boot's time base from the root volume (ADR-0191, ADR-0226 item 3): the
/// later of the latest time the volume holds plus one nanosecond and the
/// `floor` the real-time clock gave (0 for none), less the monotonic clock
/// now, so that the base plus the monotonic clock goes on from that time and
/// never falls below the volume's latest time or the floor. A volume made
/// before 1970 or by a clock ahead of this one gives 0 at least.
pub fn time_base(latest: Timestamp, monotonic_ns: u64, floor: u64) -> u64 {
    nanoseconds(latest)
        .saturating_add(1)
        .max(floor)
        .saturating_sub(monotonic_ns)
}

/// The wall clock of a service on the boot's time base.
pub fn wall_time(base: u64, monotonic_ns: u64) -> Timestamp {
    let t = base.saturating_add(monotonic_ns);
    Timestamp {
        secs: (t / 1_000_000_000) as i64,
        nanos: (t % 1_000_000_000) as u32,
    }
}

fn failure_code(reason: FailReason) -> u32 {
    use proto::failure::*;
    match reason {
        FailReason::DeviceWrite => DEVICE_WRITE,
        FailReason::DeviceFlush => DEVICE_FLUSH,
        FailReason::MetadataRead => METADATA_READ,
        FailReason::MetadataCorrupt => METADATA_CORRUPT,
        FailReason::DeviceLost => DEVICE_LOST,
        _ => INTERNAL,
    }
}

impl<D: Device, C: Clock, T: Transport> Service<D, C, T> {
    pub fn new(volume: Volume<D, C>, transport: T) -> Service<D, C, T> {
        Service {
            volume,
            transport,
            fifos: None,
            fifo_nodes: BTreeSet::new(),
            fifo_create: false,
            time_base: 0,
            sealed: None,
            connections: Vec::new(),
            records: Vec::new(),
            open: 0,
            buffers: 0,
            accounts: Vec::new(),
            syncs: Vec::new(),
            folding: false,
            outage_reads: 0,
            told_lost: false,
            peak_used: 0,
            retry_at: None,
            next_cookie: 1,
            read_count: 0,
            input: vec![0; MAX_WIRE_MESSAGE],
            output: vec![0; MAX_WIRE_MESSAGE],
            data: vec![0; proto::MAX_INLINE],
            handles: Vec::with_capacity(64),
        }
    }

    /// Makes the root's entry `name`, and everything below it, read-only on
    /// every connection from now on: no write, create, remove, rename,
    /// publish or limit, and no clone or narrowing can add one (ADR-0289
    /// item 8). The entry cannot be removed, renamed or replaced either.
    pub fn seal(&mut self, name: &str) {
        self.sealed = Some(name.into());
    }

    /// The time base `time_base` gives (ADR-0191).
    pub fn set_time_base(&mut self, base: u64) {
        self.time_base = base;
    }

    /// Gives `S_IFIFO` its meaning: `fifo_open` (the temporary directory
    /// service, ADR-0107 item 14).
    pub fn enable_fifos(&mut self) {
        self.fifos = Some(BTreeMap::new());
    }

    /// The FIFO nodes with an open side, for tests.
    pub fn open_fifos(&self) -> usize {
        self.fifos.as_ref().map_or(0, BTreeMap::len)
    }

    /// The mode a reply gives: the stored bits, and `S_IFIFO` for a FIFO.
    fn reported_mode(&self, a: &Attr) -> u32 {
        let fifo = if self.fifo_nodes.contains(&a.id) {
            proto::S_IFIFO
        } else {
            0
        };
        u32::from(a.posix_mode) | fifo
    }

    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn volume(&mut self) -> &mut Volume<D, C> {
        &mut self.volume
    }

    /// Handles held for clients.
    pub fn open_count(&self) -> usize {
        self.open
    }

    /// Takes the connection a supervisor made, to the volume's root.
    pub fn add_root(&mut self, end: T::End) -> bool {
        let handle = self.volume.root();
        let record = self.new_record(handle);
        if let Some(r) = self.records[record].as_mut() {
            r.seal = Seal::Root;
        }
        let account = self.session_account();
        self.add_connection(end, record, proto::rights::ALL, account)
            .is_some()
    }

    /// The session's account, made with what the live ones leave of each
    /// bound if it is gone (ADR-0262 item 2).
    fn session_account(&mut self) -> usize {
        if let Some(i) = self
            .accounts
            .iter()
            .position(|a| a.as_ref().is_some_and(|a| !a.untrusted))
        {
            return i;
        }
        let (mut open, mut buffers, mut held) = (0, 0, 0);
        for a in self.accounts.iter().flatten() {
            open += a.open_limit;
            buffers += a.buffers_limit;
            held += a.held_limit;
        }
        self.new_account(Account {
            parent: None,
            untrusted: false,
            top: false,
            connections: 0,
            open_limit: MAX_OPEN.saturating_sub(open),
            open: 0,
            buffers_limit: MAX_BUFFERS.saturating_sub(buffers),
            buffers: 0,
            held_limit: MAX_HELD.saturating_sub(held),
            held: 0,
            refused: 0,
        })
    }

    fn new_account(&mut self, account: Account) -> usize {
        match self.accounts.iter().position(Option::is_none) {
            Some(i) => {
                self.accounts[i] = Some(account);
                i
            }
            None => {
                self.accounts.push(Some(account));
                self.accounts.len() - 1
            }
        }
    }

    fn account(&self, a: usize) -> Option<&Account> {
        self.accounts.get(a)?.as_ref()
    }

    fn account_mut(&mut self, a: usize) -> Option<&mut Account> {
        self.accounts.get_mut(a)?.as_mut()
    }

    /// The account of connection `i`; the session's if the slot is gone,
    /// which `ready` never asks.
    fn account_of(&self, i: usize) -> usize {
        self.connections
            .get(i)
            .and_then(Option::as_ref)
            .map_or(0, |c| c.account)
    }

    /// `n` handles more held for account `a` and the service.
    fn charge(&mut self, a: usize, n: usize) {
        self.open += n;
        if let Some(x) = self.account_mut(a) {
            x.open += n;
        }
    }

    /// `n` handles less.
    fn release(&mut self, a: usize, n: usize) {
        self.open -= n;
        if let Some(x) = self.account_mut(a) {
            x.open = x.open.saturating_sub(n);
        }
    }

    /// Carves a new account of `budget` open handles from `parent`'s
    /// (ADR-0262 item 2): `clone` with a budget. The status is the refusal.
    fn carve(&mut self, parent: usize, budget: usize) -> Result<usize, u32> {
        let Some(p) = self.account(parent) else {
            return Err(status::STORAGE_FAILED);
        };
        let top = !p.untrusted;
        if budget == 0 || (top && budget > UNTRUSTED_OPEN) {
            return Err(status::INVALID_ARGUMENT);
        }
        let (buffers, held) = (buffers_for(budget), held_for(budget));
        let tops = self.accounts.iter().flatten().filter(|a| a.top).count();
        if p.open_limit.saturating_sub(p.open) < budget
            || p.buffers_limit.saturating_sub(p.buffers) < buffers
            || p.held_limit.saturating_sub(p.held) < held
            || (top && tops >= MAX_UNTRUSTED_ACCOUNTS)
        {
            self.refused_by(parent);
            return Err(status::TOO_MANY_OPEN);
        }
        if let Some(p) = self.account_mut(parent) {
            p.open_limit -= budget;
            p.buffers_limit -= buffers;
            p.held_limit -= held;
        }
        Ok(self.new_account(Account {
            parent: Some(parent),
            untrusted: true,
            top,
            connections: 0,
            open_limit: budget,
            open: 0,
            buffers_limit: buffers,
            buffers: 0,
            held_limit: held,
            held: 0,
            refused: 0,
        }))
    }

    /// Frees account `a` if it has no connection and no account carved from
    /// it: what it was given goes back to the account it was carved from
    /// (INV-0262-2). An account with no connection whose sub-accounts live
    /// stays, with only the limits it has left, so that the sub-accounts
    /// stay in its column (the untrusted column is a property of the whole
    /// subtree, ADR-0262 item 2) and its carve stays in its parent's
    /// books; it goes when the last of them does.
    fn free_account(&mut self, a: usize) {
        let Some(x) = self.account(a) else {
            return;
        };
        if x.connections > 0 || self.accounts.iter().flatten().any(|c| c.parent == Some(a)) {
            return;
        }
        let Some(x) = self.accounts.get_mut(a).and_then(Option::take) else {
            return;
        };
        debug_assert!(x.open == 0 && x.buffers == 0 && x.held == 0);
        if let Some(p) = x.parent.and_then(|p| self.account_mut(p)) {
            p.open_limit += x.open_limit;
            p.buffers_limit += x.buffers_limit;
            p.held_limit += x.held_limit;
        }
        // The account it was carved from may have been waiting for this one.
        if let Some(p) = x.parent {
            self.free_account(p);
        }
    }

    /// The accounts' totals.
    pub fn account_totals(&self) -> AccountTotals {
        let mut t = AccountTotals::default();
        for a in self.accounts.iter().flatten() {
            t.accounts += 1;
            t.open_limit += a.open_limit;
            t.open += a.open;
            t.buffers_limit += a.buffers_limit;
            t.buffers += a.buffers;
            t.held_limit += a.held_limit;
            t.held += a.held;
            t.refused += a.refused;
            if a.untrusted {
                t.untrusted_open_limit += a.open_limit;
                t.untrusted_buffers_limit += a.buffers_limit;
                t.untrusted_held_limit += a.held_limit;
            }
            if a.top {
                t.top_accounts += 1;
                t.top_open_limit += a.open_limit;
                t.top_buffers_limit += a.buffers_limit;
            }
        }
        t
    }

    /// Counts a request refused at the bound of connection `i`'s account.
    fn refused_by(&mut self, account: usize) {
        if let Some(x) = self.account_mut(account) {
            x.refused += 1;
        }
    }

    /// Requests refused so far at the bounds of `account`, for its
    /// launcher (ADR-0262 item 2); `None` once it is freed.
    pub fn account_refusals(&self, account: usize) -> Option<u64> {
        Some(self.account(account)?.refused)
    }

    /// The account connection `i` belongs to, for tests.
    pub fn account_index(&self, connection: usize) -> Option<usize> {
        Some(self.connections.get(connection)?.as_ref()?.account)
    }

    /// Whether the account has been freed, for tests.
    pub fn account_live(&self, account: usize) -> bool {
        self.account(account).is_some()
    }

    fn new_record(&mut self, handle: Handle) -> usize {
        let record = Some(Record {
            handle,
            seal: Seal::Outside,
            connections: 0,
            position: 0,
            flags: 0,
        });
        match self.records.iter().position(Option::is_none) {
            Some(i) => {
                self.records[i] = record;
                i
            }
            None => {
                self.records.push(record);
                self.records.len() - 1
            }
        }
    }

    fn add_connection(
        &mut self,
        end: T::End,
        record: usize,
        rights: u32,
        account: usize,
    ) -> Option<usize> {
        let connection = Connection {
            end,
            record,
            rights,
            account,
            syncing: None,
            buffer: None,
            token: None,
            held: None,
            position: None,
            only: None,
            fifo: None,
            opening: None,
        };
        if let Some(r) = self.records[record].as_mut() {
            r.connections += 1;
        }
        let index = match self.connections.iter().position(Option::is_none) {
            Some(i) => {
                self.connections[i] = Some(connection);
                i
            }
            None => {
                self.connections.push(Some(connection));
                self.connections.len() - 1
            }
        };
        self.charge(account, 1);
        if let Some(x) = self.account_mut(account) {
            x.connections += 1;
        }
        // A connection the loop cannot hear would hang its client: refused.
        if !self
            .transport
            .interest(end, index as u64, Interest::Requests)
        {
            self.close_connection(index);
            return None;
        }
        Some(index)
    }

    /// A second connection to the same record, which no stage 4 request can
    /// ask for. It shows that the record does not depend on the connection.
    pub fn attach_second(&mut self, connection: usize, end: T::End) -> Option<usize> {
        let (record, rights, only, account) = {
            let c = self.connections.get(connection)?.as_ref()?;
            (c.record, c.rights, c.only.clone(), c.account)
        };
        let added = self.add_connection(end, record, rights, account)?;
        self.set_only(added, only);
        Some(added)
    }

    fn set_only(&mut self, index: usize, only: Option<String>) {
        if let Some(c) = self.connections[index].as_mut() {
            c.only = only;
        }
    }

    fn only_of(&self, index: usize) -> Option<String> {
        self.connections.get(index)?.as_ref()?.only.clone()
    }

    /// Whether connection `index` may name `name`: always, unless it is
    /// narrowed to another name.
    fn may_name(&self, index: usize, name: &str) -> bool {
        self.connections
            .get(index)
            .and_then(Option::as_ref)
            .is_none_or(|c| c.only.as_deref().is_none_or(|only| only == name))
    }

    /// Reads with no effect on the record, for tests.
    pub fn record_of(&self, connection: usize) -> Option<usize> {
        Some(self.connections.get(connection)?.as_ref()?.record)
    }

    /// Open descriptions, for tests: records with a connection.
    pub fn descriptions(&self) -> usize {
        self.records.iter().flatten().count()
    }

    fn close_connection(&mut self, index: usize) {
        let Some(mut c) = self.connections.get_mut(index).and_then(Option::take) else {
            return;
        };
        self.syncs.retain(|x| *x != index);
        let account = c.account;
        self.transport.close(c.end);
        self.release(account, 1);
        if let Some(b) = c.buffer.take() {
            self.drop_buffer(account, b);
        }
        if let Some(token) = c.token.take() {
            self.transport.close(token);
            self.release(account, 1);
        }
        if let Some(held) = c.held.take() {
            for handle in held.handles {
                self.transport.close(handle);
                self.release(account, 1);
            }
            if let Some(x) = self.account_mut(account) {
                x.held -= 1;
            }
        }
        if let Some((id, side)) = c.fifo.take() {
            self.fifo_closed(id, side);
        }
        let last = match self.records.get_mut(c.record).and_then(Option::as_mut) {
            Some(r) => {
                r.connections -= 1;
                r.connections == 0
            }
            None => false,
        };
        if last && let Some(record) = self.records[c.record].take() {
            let _ = self.volume.close(record.handle);
            self.after_engine();
        }
        if let Some(x) = self.account_mut(account) {
            x.connections -= 1;
        }
        self.free_account(account);
    }

    fn drop_buffer(&mut self, account: usize, b: Buffer<T::End>) {
        self.transport.unmap(b.address, b.length);
        self.transport.close(b.end);
        self.release(account, 1);
        self.buffers -= 1;
        if let Some(x) = self.account_mut(account) {
            x.buffers -= 1;
        }
    }

    /// Runs until the transport says stop.
    pub fn run(&mut self) {
        while self.step() {}
    }

    /// One turn of the loop. False to stop.
    pub fn step(&mut self) -> bool {
        // A failed volume commits nothing more: its old deadline would wake
        // the loop at once, for ever.
        let commit = match self.volume.failure() {
            None => self.volume.commit_deadline(),
            Some(_) => None,
        };
        let deadline = match (commit, self.retry_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match self.transport.wait(deadline) {
            Wake::Ready(key) => {
                self.ready(key as usize);
                // A wait gives ready requests before its deadline, so a busy
                // loop would never see `Deadline`: ADR-0010 rule 5's bound
                // and the retry are checked here too.
                if self.volume.failure().is_none() {
                    let _ = self.volume.tick();
                    self.after_engine();
                }
                if self.retry_at.is_some_and(|at| self.transport.now() >= at) {
                    self.retry();
                }
            }
            Wake::Deadline => {
                if self.volume.failure().is_none() {
                    let _ = self.volume.tick();
                    self.after_engine();
                }
                self.retry();
            }
            Wake::DriverClosed => {
                self.volume.device_mut().mark_lost();
                self.after_engine();
            }
            Wake::Stop => return false,
        }
        true
    }

    /// ADR-0265 item 9: a `sync` on a connection of an untrusted account
    /// does not commit at once. It waits for the next commit the volume
    /// makes (the timed one or another client's `sync`), so that a loop of
    /// them costs the volume nothing beyond its timed commits. True if the
    /// reply will come then; false if the caller syncs now (a trusted
    /// account, nothing to commit, a failed or read-only volume).
    fn may_fold(&self, i: usize) -> bool {
        self.account(self.account_of(i))
            .is_some_and(|a| a.untrusted)
            && self.volume.failure().is_none()
            && !self.volume.is_read_only()
            && self.volume.commit_deadline().is_some()
    }

    /// Defers the reply `Done { count }` of connection `i`'s request `t`
    /// to the next commit, if `may_fold`. A durable write is a write and
    /// then a `sync` (ADR-0010 rule 4), so it folds the same way (review
    /// Q3, finding 4; ADR-0291).
    fn fold_sync(&mut self, i: usize, t: u32, count: u64) -> bool {
        if !self.may_fold(i) {
            return false;
        }
        let commits = self.volume.commits();
        let Some(c) = self.connections[i].as_mut() else {
            return false;
        };
        // One at a time: the connection is not read until this one is
        // answered, so the waiting `sync`s are at most the connections.
        c.syncing = Some((t, commits, count));
        let end = c.end;
        self.syncs.push(i);
        self.transport.interest(end, i as u64, Interest::Closed);
        true
    }

    /// Answers the `sync`s a commit has made durable, or a failed volume.
    fn answer_syncs(&mut self) {
        if self.syncs.is_empty() {
            return;
        }
        let failed = self.volume.failure().is_some();
        let commits = self.volume.commits();
        let clean = self.volume.commit_deadline().is_none();
        let mut waiting = core::mem::take(&mut self.syncs);
        let mut kept = Vec::new();
        for index in waiting.drain(..) {
            let Some((t, at, count)) = self.connections[index].as_ref().and_then(|c| c.syncing)
            else {
                continue;
            };
            if !(failed || clean || commits > at) {
                kept.push(index);
                continue;
            }
            let end = {
                let Some(c) = self.connections[index].as_mut() else {
                    continue;
                };
                c.syncing = None;
                c.end
            };
            self.transport
                .interest(end, index as u64, Interest::Requests);
            if failed {
                self.done(index, t, status::STORAGE_FAILED, 0);
            } else {
                self.done(index, t, status::OK, count);
            }
        }
        kept.append(&mut self.syncs);
        self.syncs = kept;
    }

    /// After every engine call: ADR-0054's loss, then ADR-0016 rule 8's report.
    fn after_engine(&mut self) {
        if let Some(used) = self.volume.used_blocks() {
            self.peak_used = self.peak_used.max(used);
        }
        if self.volume.device().is_lost() && !self.told_lost {
            self.told_lost = true;
            self.volume.device_lost();
        }
        if let Some(reason) = self.volume.take_failure_report() {
            self.transport.report(failure_code(reason));
        }
        // Last: a commit, or the volume failing just now (a lost driver),
        // answers the `sync`s that wait for one (review Q3, finding 2).
        self.answer_syncs();
    }

    fn retry(&mut self) {
        self.retry_at = None;
        for index in 0..self.connections.len() {
            let held = self.connections[index]
                .as_ref()
                .is_some_and(|c| c.held.is_some());
            if held {
                self.flush_held(index);
            }
        }
    }

    fn ready(&mut self, index: usize) {
        let Some(c) = self.connections.get(index).and_then(Option::as_ref) else {
            return;
        };
        if c.held.is_some() {
            self.flush_held(index);
            return;
        }
        if c.syncing.is_some() {
            // Only the peer's closing wakes a connection waiting for a
            // commit (its interest is `Closed`).
            self.close_connection(index);
            return;
        }
        let end = c.end;
        let mut input = core::mem::take(&mut self.input);
        let mut handles = core::mem::take(&mut self.handles);
        handles.clear();
        match self.transport.read(end, &mut input, &mut handles) {
            Incoming::Message(length) => self.request(index, &input[..length], &mut handles),
            Incoming::Empty => {}
            Incoming::Closed | Incoming::Refused => self.close_connection(index),
        }
        for handle in handles.drain(..) {
            self.transport.close(handle);
        }
        self.input = input;
        self.handles = handles;
    }

    fn flush_held(&mut self, index: usize) {
        let Some(c) = self.connections[index].as_mut() else {
            return;
        };
        let Some(held) = c.held.as_ref() else {
            return;
        };
        let account = c.account;
        match self.transport.write(c.end, &held.bytes, &held.handles) {
            Outgoing::Sent => {
                let sent = held.handles.len();
                let end = c.end;
                c.held = None;
                self.release(account, sent);
                if let Some(x) = self.account_mut(account) {
                    x.held -= 1;
                }
                self.transport
                    .interest(end, index as u64, Interest::Requests);
            }
            Outgoing::Full => {
                let end = c.end;
                self.transport.interest(end, index as u64, Interest::Reply);
            }
            Outgoing::NoMemory => {
                let end = c.end;
                self.transport.interest(end, index as u64, Interest::Closed);
                self.retry_at = Some(self.transport.now() + RETRY_NS);
            }
            Outgoing::Closed => self.close_connection(index),
        }
    }

    /// Sends `length` bytes of `output`. A reply goes after the effect.
    fn send(&mut self, index: usize, length: usize, handles: &[T::End]) {
        let Some(c) = self.connections[index].as_mut() else {
            return;
        };
        let account = c.account;
        let full = match self.transport.write(c.end, &self.output[..length], handles) {
            Outgoing::Sent => {
                // Moved to the client: no longer the service's.
                self.release(account, handles.len());
                return;
            }
            Outgoing::Closed => {
                self.drop_handles(account, handles);
                self.close_connection(index);
                return;
            }
            Outgoing::Full => true,
            Outgoing::NoMemory => false,
        };
        // Held: one reply, and this connection is not read until it goes.
        // Its handles stay counted until then. An account at its bound of
        // held replies loses the connection instead (ADR-0262 item 2): the
        // reply a client does not read is the only state it could make the
        // service keep beyond its open handles.
        let mut bytes = Vec::new();
        let mut held = Vec::new();
        // Only a client's own full queue counts against its account's bound;
        // `NoMemory` is the machine's, and another room's load can cause it:
        // that reply is held and retried as before accounts (review Q3,
        // finding 3), bounded by the connections.
        let may_hold = !full || self.account(account).is_some_and(|x| x.held < x.held_limit);
        if !may_hold {
            self.refused_by(account);
        }
        if !may_hold
            || bytes.try_reserve_exact(length).is_err()
            || held.try_reserve_exact(handles.len()).is_err()
        {
            self.drop_handles(account, handles);
            self.close_connection(index);
            return;
        }
        bytes.extend_from_slice(&self.output[..length]);
        held.extend_from_slice(handles);
        let Some(c) = self.connections[index].as_mut() else {
            return;
        };
        c.held = Some(HeldReply {
            bytes,
            handles: held,
        });
        let end = c.end;
        if let Some(x) = self.account_mut(account) {
            x.held += 1;
        }
        self.transport.interest(end, index as u64, Interest::Reply);
    }

    /// Closes handles made for a reply that will not be sent.
    fn drop_handles(&mut self, account: usize, handles: &[T::End]) {
        for &handle in handles {
            self.transport.close(handle);
        }
        self.release(account, handles.len());
    }

    fn reply<'a, M: Message<'a>>(&mut self, index: usize, transaction: u32, m: &M) {
        match encode(m, transaction, &mut self.output) {
            Ok(length) => self.send(index, length, &[]),
            Err(_) => self.close_connection(index),
        }
    }

    fn done(&mut self, index: usize, transaction: u32, status: u32, count: u64) {
        let m = proto::Done {
            status: code(status),
            count,
        };
        self.reply(index, transaction, &m);
    }

    fn done_result(&mut self, index: usize, t: u32, r: Result<u64, FsError>) {
        match r {
            Ok(count) => self.done(index, t, status::OK, count),
            Err(e) => {
                let s = self.engine_status(&e);
                self.done(index, t, s, 0);
            }
        }
    }

    /// An engine error as the client sees it. `Corrupt` after the driver is
    /// gone is an outage, not corruption (ADR-0054).
    fn engine_status(&mut self, e: &FsError) -> u32 {
        if matches!(e, FsError::Corrupt) && self.volume.device().is_lost() {
            self.outage_reads += 1;
            return status::STORAGE_FAILED;
        }
        fs_status(e)
    }

    fn has(&self, index: usize, right: u32) -> bool {
        if right & MUTATING != 0 && self.seal_of(index) == Seal::Sealed {
            return false;
        }
        self.connections[index]
            .as_ref()
            .is_some_and(|c| c.rights & right == right)
    }

    fn seal_of(&self, index: usize) -> Seal {
        self.connections
            .get(index)
            .and_then(Option::as_ref)
            .and_then(|c| self.records.get(c.record)?.as_ref())
            .map_or(Seal::Outside, |r| r.seal)
    }

    /// Whether `name`, in the directory connection `index` holds, is the
    /// sealed entry: the root's entry cannot be changed as a whole either.
    fn is_sealed_name(&self, index: usize, name: &str) -> bool {
        self.seal_of(index) == Seal::Root && self.sealed.as_deref() == Some(name)
    }

    /// The first component of `path`, as the engine reads it.
    fn first_component(path: &str) -> &str {
        path.split('/').find(|c| !c.is_empty()).unwrap_or("")
    }

    /// The seal of what `path` opens from connection `index`.
    fn seal_after(&self, index: usize, path: &str) -> Seal {
        match self.seal_of(index) {
            Seal::Sealed => Seal::Sealed,
            Seal::Root => match Self::first_component(path) {
                "" => Seal::Root,
                first if self.sealed.as_deref() == Some(first) => Seal::Sealed,
                _ => Seal::Outside,
            },
            Seal::Outside => Seal::Outside,
        }
    }

    fn handle_of(&self, index: usize) -> Option<Handle> {
        let c = self.connections.get(index)?.as_ref()?;
        Some(self.records.get(c.record)?.as_ref()?.handle)
    }

    /// The handles `handles` holds are this function's to take: what it leaves
    /// there, `ready` closes.
    fn request(&mut self, index: usize, bytes: &[u8], handles: &mut Vec<T::End>) {
        let Ok((header, _)) = Header::decode(bytes) else {
            // Not even a message: there is no one to answer.
            self.close_connection(index);
            return;
        };
        let t = header.transaction;
        let result = self.dispatch(index, header, bytes, handles);
        self.after_engine();
        if result.is_err() {
            let m = ErrorReply {
                status: status::INVALID_ARGUMENT,
            };
            self.reply(index, t, &m);
        }
    }

    fn dispatch(
        &mut self,
        i: usize,
        header: Header,
        bytes: &[u8],
        handles: &mut Vec<T::End>,
    ) -> Result<(), zoraya_protocol::WireError> {
        let n = handles.len();
        let t = header.transaction;
        match header.ordinal {
            1 => {
                let (_, m) = decode::<proto::Open<'_>>(bytes, n)?;
                self.open(i, t, &m, handles);
            }
            2 => {
                let (_, m) = decode::<proto::Clone>(bytes, n)?;
                self.clone_connection(i, t, m.rights.0, None, None, handles);
            }
            36 => {
                let (_, m) = decode::<proto::Narrow<'_>>(bytes, n)?;
                // `Name` is one component already (zoraya-protocol).
                let name = m.name.0;
                if !self.may_name(i, name) {
                    return self.denied(i, t);
                }
                self.clone_connection(i, t, m.rights.0, Some(name.into()), None, handles);
            }
            40 => {
                let (_, m) = decode::<proto::CloneAccount>(bytes, n)?;
                self.clone_connection(i, t, m.rights.0, None, Some(m.budget as usize), handles);
            }
            3 => {
                let (_, m) = decode::<proto::Read>(bytes, n)?;
                self.read(i, t, m.offset, m.length);
            }
            4 => {
                let (_, m) = decode::<proto::Write<'_>>(bytes, n)?;
                self.write(i, t, m.offset, m.flags.0, m.data.0);
            }
            5 => {
                let (_, m) = decode::<proto::Attach>(bytes, n)?;
                self.attach(i, t, m.size, handles);
            }
            6 => {
                decode::<proto::Detach>(bytes, n)?;
                let buffer = self.connections[i].as_mut().and_then(|c| c.buffer.take());
                let found = buffer.is_some();
                if let Some(b) = buffer {
                    let account = self.account_of(i);
                    self.drop_buffer(account, b);
                }
                let s = if found {
                    status::OK
                } else {
                    status::INVALID_ARGUMENT
                };
                self.done(i, t, s, 0);
            }
            7 => {
                let (_, m) = decode::<proto::ReadBuffer>(bytes, n)?;
                self.read_buffer(i, t, &m);
            }
            8 => {
                let (_, m) = decode::<proto::WriteBuffer>(bytes, n)?;
                self.write_buffer(i, t, &m);
            }
            9 => {
                decode::<proto::GetAttributes>(bytes, n)?;
                self.attributes(i, t);
            }
            10 => {
                let (_, m) = decode::<proto::ReadDirectory>(bytes, n)?;
                self.read_directory(i, t, m.cookie, false);
            }
            11 => {
                let (_, m) = decode::<proto::Rename<'_>>(bytes, n)?;
                self.rename(i, t, &m, handles);
            }
            12 => {
                let (_, m) = decode::<proto::Publish<'_>>(bytes, n)?;
                self.publish(i, t, &m, handles);
            }
            13 => {
                let (_, m) = decode::<proto::Remove<'_>>(bytes, n)?;
                if !self.may_name(i, m.name.0) || self.is_sealed_name(i, m.name.0) {
                    return self.denied(i, t);
                }
                self.remove(i, t, m.name.0);
            }
            14 => {
                let (_, m) = decode::<proto::MakeDirectory<'_>>(bytes, n)?;
                if !self.may_name(i, m.name.0) || self.is_sealed_name(i, m.name.0) {
                    return self.denied(i, t);
                }
                self.make_directory(i, t, m.mode, m.name.0);
            }
            15 => {
                decode::<proto::Sync>(bytes, n)?;
                if !self.has(i, proto::rights::SYNC) {
                    return self.denied(i, t);
                }
                if self.fold_sync(i, t, 0) {
                    return Ok(());
                }
                let r = self.volume.sync().map(|()| 0);
                self.after_engine();
                self.done_result(i, t, r);
            }
            16 => {
                decode::<proto::Status>(bytes, n)?;
                let s = if self.volume.failure().is_some() {
                    status::STORAGE_FAILED
                } else {
                    status::OK
                };
                // The count: the peak of the space in use since the mount.
                let peak = self.peak_used.max(self.volume.used_blocks().unwrap_or(0));
                self.done(i, t, s, peak * BLOCK_SIZE as u64);
            }
            37 => {
                decode::<proto::FreeSpace>(bytes, n)?;
                // Free blocks, as the volume's space map counts them; on any
                // connection (ADR-0248), like `Status`.
                let free = self.volume.free_blocks().unwrap_or(0);
                self.done(i, t, status::OK, free * BLOCK_SIZE as u64);
            }
            17 => {
                decode::<proto::CorruptReads>(bytes, n)?;
                let count = self
                    .volume
                    .corrupt_reads()
                    .saturating_sub(self.outage_reads);
                self.done(i, t, status::OK, count);
            }
            18 => {
                decode::<proto::GetToken>(bytes, n)?;
                self.get_token(i, t);
            }
            19 => {
                let (_, m) = decode::<proto::SetSize>(bytes, n)?;
                let r = match self.writable_handle(i) {
                    Some(h) => self.volume.set_size(h, m.size).map(|()| 0),
                    None => return self.denied(i, t),
                };
                self.after_engine();
                self.done_result(i, t, r);
            }
            20 => {
                let (_, m) = decode::<proto::SetAttributes>(bytes, n)?;
                self.set_attributes(i, t, &m);
            }
            21 => {
                let (_, m) = decode::<proto::MakeLink<'_>>(bytes, n)?;
                if !self.may_name(i, m.name.0) || self.is_sealed_name(i, m.name.0) {
                    return self.denied(i, t);
                }
                self.make_link(i, t, m.name.0, m.target.0);
            }
            22 => {
                decode::<proto::ReadLink>(bytes, n)?;
                self.read_link(i, t);
            }
            23 => {
                let (_, m) = decode::<proto::Share>(bytes, n)?;
                self.share(i, t, m.rights.0, handles);
            }
            24 => {
                let (_, m) = decode::<proto::Seek>(bytes, n)?;
                self.seek(i, t, m.whence.0, m.offset as i64);
            }
            25 => {
                let (_, m) = decode::<proto::SetFlags>(bytes, n)?;
                if let Some(r) = self.record_mut(i) {
                    r.flags = m.flags.0;
                }
                self.done(i, t, status::OK, 0);
            }
            26 => {
                decode::<proto::GetFlags>(bytes, n)?;
                let flags = self.record_mut(i).map_or(0, |r| r.flags);
                self.done(i, t, status::OK, u64::from(flags));
            }
            27 => {
                let (_, m) = decode::<proto::OpenWithMode<'_>>(bytes, n)?;
                // A FIFO's type bits are the temporary service's to keep;
                // every other type bit is refused, as before.
                let fifo = self.fifos.is_some() && m.mode & proto::S_IFMT == proto::S_IFIFO;
                let bits = if fifo {
                    m.mode & !proto::S_IFMT
                } else {
                    m.mode
                };
                let Ok(mode) = u16::try_from(bits) else {
                    let s = status::INVALID_ARGUMENT;
                    self.open_answer(i, t, true, s, ("", ""), None);
                    return Ok(());
                };
                let open = proto::Open {
                    flags: m.flags,
                    rights: m.rights,
                    path: m.path,
                };
                self.fifo_create = fifo;
                self.open_as(i, t, &open, mode, m.description.0, true, handles);
                self.fifo_create = false;
            }
            28 => {
                let (_, m) = decode::<proto::ReadCurrent>(bytes, n)?;
                self.read_current(i, t, m.length);
            }
            29 => {
                let (_, m) = decode::<proto::WriteCurrent<'_>>(bytes, n)?;
                self.write_current(i, t, m.flags.0, m.data.0);
            }
            30 => {
                let (_, m) = decode::<proto::ReadBufferCurrent>(bytes, n)?;
                self.read_buffer_current(i, t, m.buffer_offset, m.length);
            }
            31 => {
                let (_, m) = decode::<proto::WriteBufferCurrent>(bytes, n)?;
                self.write_buffer_current(i, t, &m);
            }
            32 => {
                decode::<proto::Stat>(bytes, n)?;
                self.stat(i, t);
            }
            33 => {
                let (_, m) = decode::<proto::ReadDirectoryIds>(bytes, n)?;
                self.read_directory(i, t, m.cookie, true);
            }
            34 => {
                let (_, m) = decode::<proto::FifoOpen>(bytes, n)?;
                self.fifo_open(i, t, m.side.0, m.nonblock.0 == 1);
            }
            38 => {
                let (_, m) = decode::<proto::SetLimit<'_>>(bytes, n)?;
                // A limit is set from above, on a full directory connection
                // (proposed ADR-0292): a connection narrowed to one name,
                // which carries create's `WRITE`, must not change it.
                if self.only_of(i).is_some() || self.is_sealed_name(i, m.name.0) {
                    return self.denied(i, t);
                }
                self.set_limit(i, t, m.name.0, (m.remove.0 == 0).then_some(m.limit));
            }
            39 => {
                decode::<proto::GetLimit>(bytes, n)?;
                // Like a listing: `READ`, and not on a narrowed connection.
                if self.only_of(i).is_some() || !self.has(i, proto::rights::READ) {
                    return self.denied(i, t);
                }
                self.get_limit(i, t);
            }
            35 => {
                decode::<proto::TimeBase>(bytes, n)?;
                self.done(i, t, status::OK, self.time_base);
            }
            ERROR_ORDINAL..=u32::MAX => {
                let m = ErrorReply {
                    status: status::UNSUPPORTED,
                };
                self.reply(i, t, &m);
            }
        }
        Ok(())
    }

    fn error_reply(&mut self, i: usize, t: u32, s: u32) {
        self.reply(i, t, &ErrorReply { status: s });
    }

    /// `fifo_open` (ADR-0107 item 14, INV-0006-9).
    fn fifo_open(&mut self, i: usize, t: u32, side: u32, nonblock: bool) {
        use proto::fifo_side::{READ, WRITE};
        if self.fifos.is_none() {
            return self.error_reply(i, t, status::UNSUPPORTED);
        }
        if self.connections[i]
            .as_ref()
            .is_some_and(|c| c.fifo.is_some())
        {
            return self.error_reply(i, t, status::INVALID_ARGUMENT);
        }
        let Some(handle) = self.handle_of(i) else {
            return self.error_reply(i, t, status::STORAGE_FAILED);
        };
        let attr = self.volume.stat(handle);
        self.after_engine();
        let id = match attr {
            Ok(a) if self.fifo_nodes.contains(&a.id) => a.id,
            Ok(_) => return self.error_reply(i, t, status::INVALID_ARGUMENT),
            Err(e) => {
                let s = self.engine_status(&e);
                return self.error_reply(i, t, s);
            }
        };
        let right = if side == READ {
            proto::rights::READ
        } else {
            proto::rights::WRITE
        };
        if !self.has(i, right) {
            return self.error_reply(i, t, status::ACCESS_DENIED);
        }
        // The reply's end, and a fresh pair, take places in the table.
        // The pair's ends are the service's own (FIFOs exist only in a
        // temporary directory service, which has one account, ADR-0262
        // item 2's table); the reply's end is the connection's account's.
        if self.open + 3 > MAX_OPEN || !self.room(i) {
            return self.error_reply(i, t, status::TOO_MANY_OPEN);
        }
        let Some(fifos) = self.fifos.as_mut() else {
            return;
        };
        let f = fifos.entry(id).or_insert(Fifo {
            read: None,
            write: None,
            readers: 0,
            writers: 0,
        });
        if side == WRITE && nonblock && f.readers == 0 {
            // ENXIO: no reader (POSIX's open of a FIFO).
            if f.read.is_none() && f.write.is_none() {
                fifos.remove(&id);
            }
            return self.error_reply(i, t, status::NOT_FOUND);
        }
        // A side whose end was dropped starts a fresh pair (item 15).
        let present = if side == READ {
            f.read.is_some()
        } else {
            f.write.is_some()
        };
        if !present {
            let old = (f.read.take(), f.write.take());
            let Some((r, w)) = self.transport.stream() else {
                if let Some(fifos) = self.fifos.as_mut()
                    && let Some(f) = fifos.get_mut(&id)
                {
                    (f.read, f.write) = old;
                    if f.read.is_none() && f.write.is_none() {
                        fifos.remove(&id);
                    }
                }
                return self.error_reply(i, t, status::LIMIT_REACHED);
            };
            for e in [old.0, old.1].into_iter().flatten() {
                self.transport.close(e);
                self.open -= 1;
            }
            self.open += 2;
            if let Some(f) = self.fifos.as_mut().and_then(|x| x.get_mut(&id)) {
                f.read = Some(r);
                f.write = Some(w);
            }
        }
        let Some(f) = self.fifos.as_mut().and_then(|x| x.get_mut(&id)) else {
            return;
        };
        if side == READ {
            f.readers += 1;
        } else {
            f.writers += 1;
        }
        let now = if side == READ {
            nonblock || f.writers > 0
        } else {
            f.readers > 0
        };
        if let Some(c) = self.connections[i].as_mut() {
            c.fifo = Some((id, side));
            if !now {
                c.opening = Some(t);
            }
        }
        if now {
            self.fifo_answer(i, t);
        }
        self.fifo_wake(id);
    }

    /// The reply to a `fifo_open`: a duplicate of the side's end.
    fn fifo_answer(&mut self, i: usize, t: u32) {
        use proto::fifo_side::READ;
        let Some((id, side)) = self.connections[i].as_ref().and_then(|c| c.fifo) else {
            return;
        };
        let end = self
            .fifos
            .as_ref()
            .and_then(|x| x.get(&id))
            .and_then(|f| if side == READ { f.read } else { f.write });
        let dup = end.and_then(|e| self.transport.duplicate(e));
        let Some(dup) = dup else {
            return self.error_reply(i, t, status::LIMIT_REACHED);
        };
        let account = self.account_of(i);
        self.charge(account, 1);
        let m = proto::FifoOpenReply {
            status: code(status::OK),
        };
        match encode(&m, t, &mut self.output) {
            Ok(length) => self.send(i, length, &[dup]),
            Err(_) => {
                let account = self.account_of(i);
                self.drop_handles(account, &[dup]);
                self.close_connection(i);
            }
        }
    }

    /// Answers the held opens of FIFO `id` whose other side is there now.
    fn fifo_wake(&mut self, id: u64) {
        use proto::fifo_side::READ;
        let Some(f) = self.fifos.as_ref().and_then(|x| x.get(&id)) else {
            return;
        };
        let (readers, writers) = (f.readers, f.writers);
        for index in 0..self.connections.len() {
            let ready = self.connections[index].as_mut().and_then(|c| match c.fifo {
                Some((fid, side)) if fid == id => {
                    let other = if side == READ { writers } else { readers };
                    if other > 0 { c.opening.take() } else { None }
                }
                _ => None,
            });
            if let Some(t) = ready {
                self.fifo_answer(index, t);
            }
        }
    }

    /// A connection that had opened a side of FIFO `id` is gone (ADR-0107
    /// item 15): the last writer's going drops the service's write end, so
    /// readers see end of file; the last reader's drops the read end, so
    /// writers see `PeerClosed`.
    fn fifo_closed(&mut self, id: u64, side: u32) {
        use proto::fifo_side::READ;
        let Some(fifos) = self.fifos.as_mut() else {
            return;
        };
        let Some(f) = fifos.get_mut(&id) else {
            return;
        };
        let mut drop = Vec::new();
        if side == READ {
            f.readers -= 1;
            if f.readers == 0 {
                drop.extend(f.read.take());
            }
        } else {
            f.writers -= 1;
            if f.writers == 0 {
                drop.extend(f.write.take());
            }
        }
        if f.readers == 0 && f.writers == 0 {
            drop.extend(f.read.take());
            drop.extend(f.write.take());
            fifos.remove(&id);
        }
        for e in drop {
            self.transport.close(e);
            self.open -= 1;
        }
    }

    fn denied(&mut self, i: usize, t: u32) -> Result<(), zoraya_protocol::WireError> {
        self.done(i, t, status::ACCESS_DENIED, 0);
        Ok(())
    }

    fn writable_handle(&self, i: usize) -> Option<Handle> {
        if self.has(i, proto::rights::WRITE) {
            self.handle_of(i)
        } else {
            None
        }
    }

    fn open_reply(&mut self, i: usize, t: u32, s: u32, remaining: &str, target: &str) {
        let m = proto::OpenReply {
            status: code(s),
            remaining: Path(remaining),
            target: Text(target),
        };
        self.reply(i, t, &m);
    }

    /// A handle takes a place in the table, and past `MAX_OPEN` it does not
    /// (ADR-0055). The refused handle is closed by `ready`.
    fn room(&self, i: usize) -> bool {
        self.open < MAX_OPEN
            && self
                .account(self.account_of(i))
                .is_some_and(|a| a.open < a.open_limit)
    }

    /// `open`: a native client's create gets `0o644` (ADR-0014 rule 6).
    fn open(&mut self, i: usize, t: u32, m: &proto::Open<'_>, handles: &mut Vec<T::End>) {
        self.open_as(i, t, m, 0o644, 0, false, handles);
    }

    /// `open` with the mode of a create and the new description's flags;
    /// `full` answers with `OpenWithModeReply`.
    #[allow(clippy::too_many_arguments)] // Reason: the request's parts, each used once.
    fn open_as(
        &mut self,
        i: usize,
        t: u32,
        m: &proto::Open<'_>,
        mode: u16,
        description: u32,
        full: bool,
        handles: &mut Vec<T::End>,
    ) {
        use proto::{open_flags as f, rights as r};
        if !self.room(i) {
            let a = self.account_of(i);
            self.refused_by(a);
            return self.open_answer(i, t, full, status::TOO_MANY_OPEN, ("", ""), None);
        }
        let flags = m.flags.0;
        let creates = flags & (f::CREATE | f::UNNAMED) != 0;
        // A directory is opened, never made or truncated here: refused before
        // the engine runs, so that a refusal changes nothing (ADR-0010 rule 2).
        if flags & f::DIRECTORY != 0 && (creates || flags & f::TRUNCATE != 0) {
            return self.open_answer(i, t, full, status::INVALID_ARGUMENT, ("", ""), None);
        }
        let mut needed = m.rights.0;
        if creates {
            needed |= r::CREATE;
        }
        if flags & f::TRUNCATE != 0 {
            needed |= r::WRITE;
        }
        let Some(dir) = self.handle_of(i) else {
            return self.open_answer(i, t, full, status::STORAGE_FAILED, ("", ""), None);
        };
        // What lies under the seal is opened to read: a write right, a
        // create or a truncation is refused before the engine runs.
        let seal = self.seal_after(i, m.path.0);
        if seal == Seal::Sealed && needed & MUTATING != 0 {
            return self.open_answer(i, t, full, status::ACCESS_DENIED, ("", ""), None);
        }
        if !self.has(i, needed) {
            return self.open_answer(i, t, full, status::ACCESS_DENIED, ("", ""), None);
        }
        // A narrowed connection opens its one name and nothing else, nor
        // itself by the empty path; an unnamed file has no name until it is
        // published, which is checked then.
        if flags & f::UNNAMED == 0 && !self.may_name(i, m.path.0) {
            return self.open_answer(i, t, full, status::ACCESS_DENIED, ("", ""), None);
        }
        let Some(end) = handles.first().copied() else {
            return self.open_answer(i, t, full, status::INVALID_ARGUMENT, ("", ""), None);
        };
        if self.transport.kind(end) != HandleKind::Channel {
            return self.open_answer(i, t, full, status::INVALID_ARGUMENT, ("", ""), None);
        }
        // Ids are never reused and come in order (ADR-0014 rule 1), so an
        // object at or past this id is one this request made.
        let first_new = self.volume.next_id();
        let opened = if flags & f::UNNAMED != 0 {
            self.volume.create_unnamed(dir, mode)
        } else {
            self.volume.open(
                dir,
                m.path.0,
                OpenOptions {
                    create: (flags & f::CREATE != 0).then_some(mode),
                    exclusive: flags & f::EXCLUSIVE != 0,
                    truncate: flags & f::TRUNCATE != 0,
                    link_itself: flags & f::NO_FOLLOW != 0,
                },
            )
        };
        self.after_engine();
        let handle = match opened {
            Ok(handle) => handle,
            Err(FsError::IsLink { remaining, target }) => {
                let s = status::IS_LINK;
                return self.open_answer(i, t, full, s, (&remaining, &target), None);
            }
            Err(e) => {
                let s = self.engine_status(&e);
                return self.open_answer(i, t, full, s, ("", ""), None);
            }
        };
        if flags & f::DIRECTORY != 0
            && !matches!(
                self.volume.stat(handle).map(|a| a.kind),
                Ok(ObjKind::Directory)
            )
        {
            let _ = self.volume.close(handle);
            return self.open_answer(i, t, full, status::NOT_DIRECTORY, ("", ""), None);
        }
        // A narrowed connection gives its one name, not what lies in it: a
        // directory of that name opens to read only (review S2; ADR-0055's
        // note, INV-0153-2).
        let reading = proto::rights::READ | proto::rights::CLONE;
        if self.only_of(i).is_some()
            && m.rights.0 & !reading != 0
            && matches!(
                self.volume.stat(handle).map(|a| a.kind),
                Ok(ObjKind::Directory)
            )
        {
            let _ = self.volume.close(handle);
            self.after_engine();
            return self.open_answer(i, t, full, status::ACCESS_DENIED, ("", ""), None);
        }
        handles.clear();
        let record = self.new_record(handle);
        if let Some(r) = self.records[record].as_mut() {
            r.flags = description;
            r.seal = seal;
        }
        // What the full reply tells of the object, read before the
        // connection exists: a connection the transport refuses closes the
        // record, and with it the handle.
        let attr = if full {
            let a = self.volume.stat(handle);
            self.after_engine();
            match a {
                Ok(a) => Some(a),
                // An answer without the mode would read as a mode of 0.
                Err(e) => {
                    self.records[record] = None;
                    self.transport.close(end);
                    let _ = self.volume.close(handle);
                    self.after_engine();
                    let s = self.engine_status(&e);
                    return self.open_answer(i, t, full, s, ("", ""), None);
                }
            }
        } else {
            None
        };
        let account = self.account_of(i);
        let s = match self.add_connection(end, record, m.rights.0, account) {
            Some(_) => status::OK,
            None => status::LIMIT_REACHED,
        };
        if self.fifo_create
            && let Some(a) = attr.filter(|a| a.id >= first_new)
        {
            self.fifo_nodes.insert(a.id);
        }
        let made = attr.map(|a| (a.id >= first_new, a));
        self.open_answer(i, t, full, s, ("", ""), made.filter(|_| s == status::OK));
    }

    /// `open`'s reply, or `open_with_mode`'s with what it knows of the
    /// object: `(created, attributes)`.
    fn open_answer(
        &mut self,
        i: usize,
        t: u32,
        full: bool,
        s: u32,
        (remaining, target): (&str, &str),
        made: Option<(bool, Attr)>,
    ) {
        if !full {
            return self.open_reply(i, t, s, remaining, target);
        }
        let volume = self.volume.durable_root().volume_id;
        let (created, a) = match made {
            Some((created, a)) => (created, Some(a)),
            None => (false, None),
        };
        let m = proto::OpenWithModeReply {
            status: code(s),
            remaining: Path(remaining),
            target: Text(target),
            created: Below(u32::from(created)),
            volume: if a.is_some() { volume } else { 0 },
            id: a.map_or(0, |a| a.id),
            size: a.map_or(0, |a| a.size),
            modified_seconds: a.map_or(0, |a| a.mtime.secs as u64),
            modified_nanos: a.map_or(0, |a| a.mtime.nanos),
            changed_seconds: a.map_or(0, |a| a.ctime.secs as u64),
            changed_nanos: a.map_or(0, |a| a.ctime.nanos),
            mode: a.map_or(0, |a| self.reported_mode(&a)),
            kind: a.map_or(Below(0), |a| wire_kind(a.kind)),
        };
        self.reply(i, t, &m);
    }

    /// A new connection to the same object; `narrow` gives it one name,
    /// and a narrowed connection's clone keeps its own.
    fn clone_connection(
        &mut self,
        i: usize,
        t: u32,
        rights: u32,
        narrow: Option<String>,
        budget: Option<usize>,
        handles: &mut Vec<T::End>,
    ) {
        if !self.room(i) {
            let a = self.account_of(i);
            self.refused_by(a);
            return self.done(i, t, status::TOO_MANY_OPEN, 0);
        }
        if !self.has(i, proto::rights::CLONE) || !self.has(i, rights) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let Some(end) = handles.first().copied() else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        if self.transport.kind(end) != HandleKind::Channel {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        }
        let Some(handle) = self.handle_of(i) else {
            return self.done(i, t, status::STORAGE_FAILED, 0);
        };
        // A budget makes a new account for the new connection (ADR-0262
        // item 2); without one it joins this connection's. A service with
        // FIFOs (`fsd --memory`) has the session's account only: its
        // FIFO answers are charged when they are made (review Q3, note 7).
        if budget.is_some() && self.fifos.is_some() {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        }
        let account = match budget {
            Some(budget) => match self.carve(self.account_of(i), budget) {
                Ok(account) => account,
                Err(s) => return self.done(i, t, s, 0),
            },
            None => self.account_of(i),
        };
        match self.volume.reopen(handle) {
            Ok(handle) => {
                handles.clear();
                let record = self.new_record(handle);
                let seal = self.seal_of(i);
                if let Some(r) = self.records[record].as_mut() {
                    r.seal = seal;
                }
                let only = narrow.or_else(|| self.only_of(i));
                let s = match self.add_connection(end, record, rights, account) {
                    Some(added) => {
                        self.set_only(added, only);
                        status::OK
                    }
                    None => status::LIMIT_REACHED,
                };
                self.done(i, t, s, 0);
            }
            Err(e) => {
                if budget.is_some() {
                    self.free_account(account);
                }
                self.done_result(i, t, Err(e));
            }
        }
    }

    fn read(&mut self, i: usize, t: u32, offset: u64, length: u32) {
        let reply = |s: u32, data: &[u8], this: &mut Self| {
            let m = proto::ReadReply {
                status: code(s),
                data: Data(data),
            };
            match encode(&m, t, &mut this.output) {
                Ok(size) => this.send(i, size, &[]),
                Err(_) => this.close_connection(i),
            }
        };
        if !self.has(i, proto::rights::READ) {
            return reply(status::ACCESS_DENIED, &[], self);
        }
        let Some(handle) = self.handle_of(i) else {
            return reply(status::STORAGE_FAILED, &[], self);
        };
        let wanted = (length as usize).min(proto::MAX_INLINE);
        let mut data = core::mem::take(&mut self.data);
        match self.volume.read(handle, offset, &mut data[..wanted]) {
            Ok(got) => {
                self.after_engine();
                self.read_count += got as u64;
                reply(status::OK, &data[..got], self);
            }
            Err(e) => {
                self.after_engine();
                let s = self.engine_status(&e);
                reply(s, &[], self);
            }
        }
        self.data = data;
    }

    /// The `sync` of a durable write: at once, or, from an untrusted
    /// account, deferred to the next commit with the reply (`write_reply`).
    fn durable(&mut self, i: usize, flags: u32) -> Result<(), FsError> {
        if flags & proto::write_flags::DURABLE == 0 {
            return Ok(());
        }
        if self.may_fold(i) {
            self.folding = true;
            return Ok(());
        }
        self.volume.sync()
    }

    /// The reply of a write: `Done { count }`, at the next commit if its
    /// `sync` was deferred.
    fn write_reply(&mut self, i: usize, t: u32, r: Result<u64, FsError>) {
        let fold = core::mem::take(&mut self.folding);
        match r {
            Ok(count) if fold => {
                if !self.fold_sync(i, t, count) {
                    let r = self.volume.sync().map(|()| count);
                    self.after_engine();
                    self.done_result(i, t, r);
                }
            }
            r => self.done_result(i, t, r),
        }
    }

    /// One atomic write, with what the flags ask (ADR-0010 rule 2).
    fn write_flagged(
        &mut self,
        i: usize,
        handle: Handle,
        offset: u64,
        flags: u32,
        data: &[u8],
    ) -> Result<u64, FsError> {
        let at = if flags & proto::write_flags::APPEND != 0 {
            self.volume.stat(handle)?.size
        } else {
            offset
        };
        let written = self.volume.write(handle, at, data)? as u64;
        self.durable(i, flags)?;
        Ok(written)
    }

    /// A write's handle: `WRITE`, and `SYNC` too if it is durable, since that
    /// is a write followed by `sync` (ADR-0010 rule 4).
    fn write_handle(&self, i: usize, flags: u32) -> Option<Handle> {
        let durable = flags & proto::write_flags::DURABLE != 0;
        if durable && !self.has(i, proto::rights::SYNC) {
            return None;
        }
        self.writable_handle(i)
    }

    fn write(&mut self, i: usize, t: u32, offset: u64, flags: u32, data: &[u8]) {
        let Some(handle) = self.write_handle(i, flags) else {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        };
        let r = self.write_flagged(i, handle, offset, flags, data);
        self.after_engine();
        self.write_reply(i, t, r);
    }

    fn attach(&mut self, i: usize, t: u32, length: u64, handles: &mut Vec<T::End>) {
        if !self.room(i) {
            let a = self.account_of(i);
            self.refused_by(a);
            return self.done(i, t, status::TOO_MANY_OPEN, 0);
        }
        let account = self.account_of(i);
        let buffered = self
            .account(account)
            .is_none_or(|a| a.buffers >= a.buffers_limit);
        if self.buffers >= MAX_BUFFERS || buffered {
            self.refused_by(account);
            return self.done(i, t, status::LIMIT_REACHED, 0);
        }
        let attached = self.connections[i]
            .as_ref()
            .is_some_and(|c| c.buffer.is_some());
        let Some(end) = handles.first().copied() else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        if attached || self.transport.kind(end) != HandleKind::Memory {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        }
        let Some(address) = self.transport.map(end, length) else {
            return self.done(i, t, status::LIMIT_REACHED, 0);
        };
        handles.clear();
        if let Some(c) = self.connections[i].as_mut() {
            c.buffer = Some(Buffer {
                end,
                address,
                length,
            });
        }
        self.charge(account, 1);
        self.buffers += 1;
        if let Some(x) = self.account_mut(account) {
            x.buffers += 1;
        }
        self.done(i, t, status::OK, length);
    }

    fn buffer_range(&self, i: usize, offset: u64, length: u32) -> Option<u64> {
        let b = self.connections[i].as_ref()?.buffer.as_ref()?;
        let end = offset.checked_add(u64::from(length))?;
        if end > b.length || length as usize > MAX_ATOMIC_WRITE {
            return None;
        }
        Some(b.address + offset)
    }

    fn read_buffer(&mut self, i: usize, t: u32, m: &proto::ReadBuffer) {
        if !self.has(i, proto::rights::READ) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let (Some(address), Some(handle)) = (
            self.buffer_range(i, m.buffer_offset, m.length),
            self.handle_of(i),
        ) else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        let mut data = core::mem::take(&mut self.data);
        let mut done = 0u64;
        let mut failure = None;
        while done < u64::from(m.length) {
            let want = (u64::from(m.length) - done).min(data.len() as u64) as usize;
            match self.volume.read(handle, m.offset + done, &mut data[..want]) {
                Ok(0) => break,
                Ok(got) => {
                    if !self.transport.copy_to(address + done, &data[..got]) {
                        failure = Some(status::INVALID_ARGUMENT);
                        break;
                    }
                    done += got as u64;
                }
                Err(e) => {
                    self.after_engine();
                    failure = Some(self.engine_status(&e));
                    break;
                }
            }
        }
        self.data = data;
        self.after_engine();
        self.read_count += done;
        match failure {
            Some(s) => self.done(i, t, s, done),
            None => self.done(i, t, status::OK, done),
        }
    }

    fn write_buffer(&mut self, i: usize, t: u32, m: &proto::WriteBuffer) {
        let Some(handle) = self.write_handle(i, m.flags.0) else {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        };
        let Some(address) = self.buffer_range(i, m.buffer_offset, m.length) else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        // Copied out once, before anything uses it: the client can change the
        // buffer while the engine works (ADR-0051, INV-0055-5).
        let mut copy = Vec::new();
        if copy.try_reserve_exact(m.length as usize).is_err() {
            return self.done(i, t, status::NO_SPACE, 0);
        }
        copy.resize(m.length as usize, 0);
        if !self.transport.copy_from(address, &mut copy) {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        }
        let r = self.write_flagged(i, handle, m.offset, m.flags.0, &copy);
        self.after_engine();
        self.write_reply(i, t, r);
    }

    fn attributes(&mut self, i: usize, t: u32) {
        let attr: Result<Attr, _> = match self.handle_of(i) {
            Some(handle) => self.volume.stat(handle),
            None => Err(FsError::StorageFailed),
        };
        self.after_engine();
        let m = match attr {
            Ok(a) => proto::Attributes {
                status: code(status::OK),
                size: a.size,
                modified: nanoseconds(a.mtime),
                changed: nanoseconds(a.ctime),
                mode: self.reported_mode(&a),
                kind: wire_kind(a.kind),
            },
            Err(e) => proto::Attributes {
                status: code(self.engine_status(&e)),
                size: 0,
                modified: 0,
                changed: 0,
                mode: 0,
                kind: Below(0),
            },
        };
        self.reply(i, t, &m);
    }

    /// `read_directory`, or with `ids` `read_directory_ids`: one listing,
    /// one cookie per connection, two reply shapes.
    fn read_directory(&mut self, i: usize, t: u32, cookie: u64, ids: bool) {
        let fail = |s: u32, this: &mut Self| {
            if ids {
                let m = proto::ReadDirectoryIdsReply {
                    status: code(s),
                    entries: Seq::Of(&[]),
                };
                this.reply(i, t, &m);
            } else {
                let m = proto::ReadDirectoryReply {
                    status: code(s),
                    entries: Seq::Of(&[]),
                };
                this.reply(i, t, &m);
            }
        };
        // A narrowed connection's listing would show the names it may not use.
        if !self.has(i, proto::rights::READ) || self.only_of(i).is_some() {
            return fail(status::ACCESS_DENIED, self);
        }
        let Some(handle) = self.handle_of(i) else {
            return fail(status::STORAGE_FAILED, self);
        };
        let mut after = if cookie == 0 {
            None
        } else {
            match self.connections[i]
                .as_ref()
                .and_then(|c| c.position.as_ref())
            {
                Some((held, name)) if *held == cookie => Some(name.clone()),
                _ => return fail(status::INVALID_ARGUMENT, self),
            }
        };
        let mut found: Vec<(String, WireKind, u64)> = Vec::new();
        let mut bytes = 0;
        while found.len() < proto::MAX_ENTRIES && bytes < 32 * 1024 {
            match self.volume.readdir(handle, after.as_deref()) {
                Ok(Some(e)) => {
                    after = Some(e.name.clone());
                    bytes += e.name.len() + 32;
                    found.push((e.name, wire_kind(e.kind), e.id));
                }
                Ok(None) => break,
                Err(e) => {
                    self.after_engine();
                    let s = self.engine_status(&e);
                    return fail(s, self);
                }
            }
        }
        self.after_engine();
        // The cookie after the last name: 0 only at the end.
        let more = found.len() == proto::MAX_ENTRIES || bytes >= 32 * 1024;
        let next = if more {
            self.next_cookie += 1;
            self.next_cookie
        } else {
            0
        };
        let encoded = if ids {
            let entries: Vec<proto::IdEntry<'_>> = found
                .iter()
                .map(|(name, kind, id)| proto::IdEntry {
                    next,
                    kind: *kind,
                    id: *id,
                    name: Name(name),
                })
                .collect();
            let m = proto::ReadDirectoryIdsReply {
                status: code(status::OK),
                entries: Seq::Of(&entries),
            };
            encode(&m, t, &mut self.output)
        } else {
            let entries: Vec<proto::Entry<'_>> = found
                .iter()
                .map(|(name, kind, _)| proto::Entry {
                    next,
                    kind: *kind,
                    name: Name(name),
                })
                .collect();
            let m = proto::ReadDirectoryReply {
                status: code(status::OK),
                entries: Seq::Of(&entries),
            };
            encode(&m, t, &mut self.output)
        };
        match encoded {
            Ok(size) => {
                if let Some(c) = self.connections[i].as_mut() {
                    c.position = after.filter(|_| more).map(|name| (next, name));
                }
                self.send(i, size, &[]);
            }
            Err(_) => self.close_connection(i),
        }
    }

    /// The connection a token names, if it names one.
    fn token_target(&mut self, token: T::End) -> Option<usize> {
        for index in 0..self.connections.len() {
            if let Some(held) = self.connections[index].as_ref().and_then(|c| c.token)
                && self.transport.same(held, token)
            {
                return Some(index);
            }
        }
        None
    }

    fn get_token(&mut self, i: usize, t: u32) {
        let existing = self.connections[i].as_ref().and_then(|c| c.token);
        let token = match existing {
            Some(token) => Some(token),
            None if self.room(i) => self.transport.new_token().inspect(|token| {
                let account = self.account_of(i);
                self.charge(account, 1);
                if let Some(c) = self.connections[i].as_mut() {
                    c.token = Some(*token);
                }
            }),
            None => {
                let a = self.account_of(i);
                self.refused_by(a);
                let m = proto::TokenReply {
                    status: code(status::TOO_MANY_OPEN),
                };
                return self.reply_without_handle(i, t, &m);
            }
        };
        // The copy is a handle the service holds until it is sent: it needs
        // room, and is counted.
        if !self.room(i) {
            let a = self.account_of(i);
            self.refused_by(a);
            let m = proto::TokenReply {
                status: code(status::TOO_MANY_OPEN),
            };
            return self.reply_without_handle(i, t, &m);
        }
        let copy = token.and_then(|token| self.transport.duplicate(token));
        if copy.is_some() {
            let account = self.account_of(i);
            self.charge(account, 1);
        }
        let Some(copy) = copy else {
            let m = proto::TokenReply {
                status: code(status::STORAGE_FAILED),
            };
            return self.reply_without_handle(i, t, &m);
        };
        let m = proto::TokenReply {
            status: code(status::OK),
        };
        match encode(&m, t, &mut self.output) {
            Ok(size) => self.send(i, size, &[copy]),
            Err(_) => {
                let account = self.account_of(i);
                self.drop_handles(account, &[copy]);
                self.close_connection(i);
            }
        }
    }

    /// `TokenReply` declares a handle, and a refusal has none to give, so a
    /// refusal goes as the generic reply.
    fn reply_without_handle(&mut self, i: usize, t: u32, m: &proto::TokenReply) {
        let m = ErrorReply { status: m.status.0 };
        self.reply(i, t, &m);
    }

    /// Whether the connection a token names may name `name`.
    fn token_may_name(&mut self, token: T::End, name: &str) -> bool {
        if self.transport.kind(token) != HandleKind::Token {
            return true; // refused by token_directory
        }
        match self.token_target(token) {
            Some(target) => self.may_name(target, name),
            None => true,
        }
    }

    /// Whether the connection a token names is the root and `name` is the
    /// sealed entry.
    fn token_is_sealed_name(&mut self, token: T::End, name: &str) -> bool {
        if self.transport.kind(token) != HandleKind::Token {
            return false;
        }
        self.token_target(token)
            .is_some_and(|target| self.is_sealed_name(target, name))
    }

    fn token_directory(&mut self, token: T::End, right: u32) -> Result<Handle, u32> {
        if self.transport.kind(token) != HandleKind::Token {
            return Err(status::INVALID_ARGUMENT);
        }
        let target = self.token_target(token).ok_or(status::INVALID_ARGUMENT)?;
        if !self.has(target, right) {
            return Err(status::ACCESS_DENIED);
        }
        self.handle_of(target).ok_or(status::STORAGE_FAILED)
    }

    fn rename(&mut self, i: usize, t: u32, m: &proto::Rename<'_>, handles: &mut [T::End]) {
        use proto::rename_mode as modes;
        let right = proto::rights::RENAME;
        if !self.has(i, right) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let (Some(source), Some(token)) = (self.handle_of(i), handles.first().copied()) else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        if !self.may_name(i, m.old.0)
            || !self.token_may_name(token, m.new.0)
            || self.is_sealed_name(i, m.old.0)
            || self.token_is_sealed_name(token, m.new.0)
        {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let target = match self.token_directory(token, right) {
            Ok(target) => target,
            Err(s) => return self.done(i, t, s, 0),
        };
        let mode = match m.mode.0 {
            modes::NO_REPLACE => RenameMode::NoReplace,
            modes::EXCHANGE => RenameMode::Exchange,
            _ => RenameMode::Replace,
        };
        let r = self.volume.rename(source, m.old.0, target, m.new.0, mode);
        self.after_engine();
        self.done_result(i, t, r.map(|()| 0));
    }

    fn publish(&mut self, i: usize, t: u32, m: &proto::Publish<'_>, handles: &mut [T::End]) {
        let right = proto::rights::PUBLISH;
        if !self.has(i, right) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let (Some(file), Some(token)) = (self.handle_of(i), handles.first().copied()) else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        if !self.token_may_name(token, m.name.0) || self.token_is_sealed_name(token, m.name.0) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let directory = match self.token_directory(token, right) {
            Ok(directory) => directory,
            Err(s) => return self.done(i, t, s, 0),
        };
        let mode = if m.mode.0 == proto::publish_mode::REPLACE {
            PublishMode::Replace
        } else {
            PublishMode::NoReplace
        };
        let r = self.volume.publish(file, directory, m.name.0, mode);
        self.after_engine();
        self.done_result(i, t, r.map(|()| 0));
    }

    fn remove(&mut self, i: usize, t: u32, name: &str) {
        if !self.has(i, proto::rights::REMOVE) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let Some(directory) = self.handle_of(i) else {
            return self.done(i, t, status::STORAGE_FAILED, 0);
        };
        let r = match self.volume.unlink(directory, name) {
            Err(FsError::IsDir) => self.volume.rmdir(directory, name),
            r => r,
        };
        self.after_engine();
        self.done_result(i, t, r.map(|()| 0));
    }

    fn make_directory(&mut self, i: usize, t: u32, mode: u32, name: &str) {
        if !self.has(i, proto::rights::CREATE) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let Some(directory) = self.handle_of(i) else {
            return self.done(i, t, status::STORAGE_FAILED, 0);
        };
        let Ok(mode) = u16::try_from(mode) else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        let r = self.volume.mkdir(directory, name, mode);
        self.after_engine();
        self.done_result(i, t, r.map(|()| 0));
    }

    fn set_attributes(&mut self, i: usize, t: u32, m: &proto::SetAttributes) {
        let Some(handle) = self.writable_handle(i) else {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        };
        let Ok(mode) = u16::try_from(m.mode) else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        let Ok(secs) = i64::try_from(m.seconds) else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        let mode = (m.which.0 & 1 != 0).then_some(mode);
        let mtime = (m.which.0 & 2 != 0).then_some(Timestamp {
            secs,
            nanos: m.nanos,
        });
        let r = self.volume.set_attr(handle, mode, mtime);
        self.after_engine();
        self.done_result(i, t, r.map(|()| 0));
    }

    /// `set_limit` (ADR-0265 rule 4): needs `WRITE` on the directory's
    /// connection, so a limit is set from above.
    fn set_limit(&mut self, i: usize, t: u32, name: &str, limit: Option<u64>) {
        // The rights of a whole tree, not only `WRITE`: a `write` operand
        // that names a directory, or two `create` operands in one directory,
        // give a full connection with `WRITE` but not `REMOVE` (re-check W2).
        let tree = proto::rights::WRITE
            | proto::rights::CREATE
            | proto::rights::REMOVE
            | proto::rights::RENAME;
        if !self.has(i, tree) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let Some(directory) = self.handle_of(i) else {
            return self.done(i, t, status::STORAGE_FAILED, 0);
        };
        let r = self.volume.set_limit(directory, name, limit);
        self.after_engine();
        self.done_result(i, t, r.map(|()| 0));
    }

    fn get_limit(&mut self, i: usize, t: u32) {
        let r = match self.handle_of(i) {
            Some(handle) => self.volume.limit_of(handle),
            None => Err(FsError::StorageFailed),
        };
        self.after_engine();
        let m = match r {
            Ok(l) => proto::LimitReply {
                status: code(status::OK),
                limited: Below(u32::from(l.is_some())),
                limit: l.map_or(0, |l| l.limit),
                used: l.map_or(0, |l| l.used),
            },
            Err(e) => proto::LimitReply {
                status: code(self.engine_status(&e)),
                limited: Below(0),
                limit: 0,
                used: 0,
            },
        };
        self.reply(i, t, &m);
    }

    fn make_link(&mut self, i: usize, t: u32, name: &str, target: &str) {
        if !self.has(i, proto::rights::CREATE) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let Some(directory) = self.handle_of(i) else {
            return self.done(i, t, status::STORAGE_FAILED, 0);
        };
        let r = self.volume.symlink(directory, name, target);
        self.after_engine();
        self.done_result(i, t, r.map(|()| 0));
    }

    fn read_link(&mut self, i: usize, t: u32) {
        if !self.has(i, proto::rights::READ) {
            let m = proto::ReadLinkReply {
                status: code(status::ACCESS_DENIED),
                target: Text(""),
            };
            return self.reply(i, t, &m);
        }
        let r = match self.handle_of(i) {
            Some(handle) => self.volume.readlink(handle),
            None => Err(FsError::StorageFailed),
        };
        self.after_engine();
        let (s, target) = match r {
            Ok(target) => (status::OK, target),
            Err(e) => (self.engine_status(&e), String::new()),
        };
        let m = proto::ReadLinkReply {
            status: code(s),
            target: Text(&target),
        };
        self.reply(i, t, &m);
    }

    fn stat(&mut self, i: usize, t: u32) {
        let attr: Result<Attr, _> = match self.handle_of(i) {
            Some(handle) => self.volume.stat(handle),
            None => Err(FsError::StorageFailed),
        };
        self.after_engine();
        let volume = self.volume.durable_root().volume_id;
        let m = match attr {
            Ok(a) => proto::StatReply {
                status: code(status::OK),
                volume,
                id: a.id,
                size: a.size,
                modified_seconds: a.mtime.secs as u64,
                modified_nanos: a.mtime.nanos,
                changed_seconds: a.ctime.secs as u64,
                changed_nanos: a.ctime.nanos,
                mode: self.reported_mode(&a),
                kind: wire_kind(a.kind),
            },
            Err(e) => proto::StatReply {
                status: code(self.engine_status(&e)),
                volume: 0,
                id: 0,
                size: 0,
                modified_seconds: 0,
                modified_nanos: 0,
                changed_seconds: 0,
                changed_nanos: 0,
                mode: 0,
                kind: Below(0),
            },
        };
        self.reply(i, t, &m);
    }

    fn record_mut(&mut self, i: usize) -> Option<&mut Record> {
        let c = self.connections.get(i)?.as_ref()?;
        self.records.get_mut(c.record)?.as_mut()
    }

    /// `clone`'s rules, but the new connection joins this one's record.
    fn share(&mut self, i: usize, t: u32, rights: u32, handles: &mut Vec<T::End>) {
        if !self.room(i) {
            let a = self.account_of(i);
            self.refused_by(a);
            return self.done(i, t, status::TOO_MANY_OPEN, 0);
        }
        if !self.has(i, proto::rights::CLONE) || !self.has(i, rights) {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let Some(end) = handles.first().copied() else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        if self.transport.kind(end) != HandleKind::Channel {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        }
        let Some(record) = self.connections[i].as_ref().map(|c| c.record) else {
            return self.done(i, t, status::STORAGE_FAILED, 0);
        };
        handles.clear();
        let only = self.only_of(i);
        let account = self.account_of(i);
        let s = match self.add_connection(end, record, rights, account) {
            Some(added) => {
                self.set_only(added, only);
                status::OK
            }
            None => status::LIMIT_REACHED,
        };
        self.done(i, t, s, 0);
    }

    /// The position after a seek, if it is one a file can have.
    fn sought(&mut self, i: usize, whence: u32, offset: i64) -> Result<u64, u32> {
        let (handle, position) = match self.record_mut(i) {
            Some(r) => (r.handle, r.position),
            None => return Err(status::STORAGE_FAILED),
        };
        let base = match whence {
            proto::whence::START => 0,
            proto::whence::CURRENT => position,
            _ => match self.volume.stat(handle) {
                Ok(a) => a.size,
                Err(e) => return Err(self.engine_status(&e)),
            },
        };
        let to = i128::from(base) + i128::from(offset);
        if !(0..=i128::from(MAX_POSITION)).contains(&to) {
            return Err(status::INVALID_ARGUMENT);
        }
        Ok(to as u64)
    }

    fn seek(&mut self, i: usize, t: u32, whence: u32, offset: i64) {
        let r = self.sought(i, whence, offset);
        self.after_engine();
        match r {
            Ok(to) => {
                if let Some(r) = self.record_mut(i) {
                    r.position = to;
                }
                self.done(i, t, status::OK, to);
            }
            Err(s) => self.done(i, t, s, 0),
        }
    }

    /// Where a write at the position goes: the end for an append, which the
    /// description or the request may ask.
    fn write_at(&mut self, i: usize, flags: u32) -> Result<(Handle, u64), FsError> {
        let (handle, position, append) = match self.record_mut(i) {
            Some(r) => (
                r.handle,
                r.position,
                r.flags & proto::description_flags::APPEND != 0,
            ),
            None => return Err(FsError::StorageFailed),
        };
        if append || flags & proto::write_flags::APPEND != 0 {
            Ok((handle, self.volume.stat(handle)?.size))
        } else {
            Ok((handle, position))
        }
    }

    /// A write at the position: the position moves past it in the same
    /// request, so no other connection of the description sees it between.
    fn write_current_bytes(&mut self, i: usize, flags: u32, data: &[u8]) -> Result<u64, FsError> {
        let (handle, at) = self.write_at(i, flags)?;
        if at
            .checked_add(data.len() as u64)
            .is_none_or(|end| end > MAX_POSITION)
        {
            return Err(FsError::InvalidArgument);
        }
        let written = self.volume.write(handle, at, data)? as u64;
        if let Some(r) = self.record_mut(i) {
            r.position = at + written;
        }
        self.durable(i, flags)?;
        Ok(written)
    }

    fn write_current(&mut self, i: usize, t: u32, flags: u32, data: &[u8]) {
        if self.write_handle(i, flags).is_none() {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let r = self.write_current_bytes(i, flags, data);
        self.after_engine();
        self.write_reply(i, t, r);
    }

    fn read_current(&mut self, i: usize, t: u32, length: u32) {
        let position = match self.record_mut(i) {
            Some(r) => r.position,
            None => 0,
        };
        let before = self.read_count;
        self.read(i, t, position, length);
        let got = self.read_count - before;
        if let Some(r) = self.record_mut(i) {
            r.position = position + got;
        }
    }

    fn read_buffer_current(&mut self, i: usize, t: u32, buffer_offset: u64, length: u32) {
        let position = match self.record_mut(i) {
            Some(r) => r.position,
            None => 0,
        };
        let m = proto::ReadBuffer {
            offset: position,
            buffer_offset,
            length,
        };
        let before = self.read_count;
        self.read_buffer(i, t, &m);
        let got = self.read_count - before;
        if let Some(r) = self.record_mut(i) {
            r.position = position + got;
        }
    }

    fn write_buffer_current(&mut self, i: usize, t: u32, m: &proto::WriteBufferCurrent) {
        if self.write_handle(i, m.flags.0).is_none() {
            return self.done(i, t, status::ACCESS_DENIED, 0);
        }
        let Some(address) = self.buffer_range(i, m.buffer_offset, m.length) else {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        };
        let mut copy = Vec::new();
        if copy.try_reserve_exact(m.length as usize).is_err() {
            return self.done(i, t, status::NO_SPACE, 0);
        }
        copy.resize(m.length as usize, 0);
        if !self.transport.copy_from(address, &mut copy) {
            return self.done(i, t, status::INVALID_ARGUMENT, 0);
        }
        let r = self.write_current_bytes(i, m.flags.0, &copy);
        self.after_engine();
        self.write_reply(i, t, r);
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

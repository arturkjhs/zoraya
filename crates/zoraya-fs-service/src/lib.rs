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

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_fs::{
    Attr, BlockDevice, Clock, FailReason, FsError, Handle, MAX_ATOMIC_WRITE, ObjKind, OpenOptions,
    PublishMode, RenameMode, Timestamp, Volume,
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
/// (INV-0055-9), so that a later method can share one.
struct Record {
    handle: Handle,
    connections: u32,
}

struct Buffer<E> {
    end: E,
    address: u64,
    length: u64,
}

struct HeldReply<E> {
    bytes: Vec<u8>,
    handles: Vec<E>,
}

struct Connection<E> {
    end: E,
    record: usize,
    rights: u32,
    buffer: Option<Buffer<E>>,
    token: Option<E>,
    /// A reply that did not fit: the service reads nothing more until it goes.
    held: Option<HeldReply<E>>,
    /// The name `read_directory` goes on from, and its cookie.
    position: Option<(u64, String)>,
}

pub struct Service<D: Device, C: Clock, T: Transport> {
    volume: Volume<D, C>,
    transport: T,
    connections: Vec<Option<Connection<T::End>>>,
    records: Vec<Option<Record>>,
    /// Handles held for clients (ADR-0055).
    open: usize,
    buffers: usize,
    /// Reads reported as `StorageFailed`, not to be counted as corrupt.
    outage_reads: u64,
    told_lost: bool,
    retry_at: Option<u64>,
    next_cookie: u64,
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
            connections: Vec::new(),
            records: Vec::new(),
            open: 0,
            buffers: 0,
            outage_reads: 0,
            told_lost: false,
            retry_at: None,
            next_cookie: 1,
            input: vec![0; MAX_WIRE_MESSAGE],
            output: vec![0; MAX_WIRE_MESSAGE],
            data: vec![0; proto::MAX_INLINE],
            handles: Vec::with_capacity(64),
        }
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
        self.add_connection(end, record, proto::rights::ALL)
            .is_some()
    }

    fn new_record(&mut self, handle: Handle) -> usize {
        let record = Some(Record {
            handle,
            connections: 0,
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

    fn add_connection(&mut self, end: T::End, record: usize, rights: u32) -> Option<usize> {
        let connection = Connection {
            end,
            record,
            rights,
            buffer: None,
            token: None,
            held: None,
            position: None,
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
        self.open += 1;
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
        let (record, rights) = {
            let c = self.connections.get(connection)?.as_ref()?;
            (c.record, c.rights)
        };
        self.add_connection(end, record, rights)
    }

    /// Reads with no effect on the record, for tests.
    pub fn record_of(&self, connection: usize) -> Option<usize> {
        Some(self.connections.get(connection)?.as_ref()?.record)
    }

    fn close_connection(&mut self, index: usize) {
        let Some(mut c) = self.connections.get_mut(index).and_then(Option::take) else {
            return;
        };
        self.transport.close(c.end);
        self.open -= 1;
        if let Some(b) = c.buffer.take() {
            self.drop_buffer(b);
        }
        if let Some(token) = c.token.take() {
            self.transport.close(token);
            self.open -= 1;
        }
        if let Some(held) = c.held.take() {
            for handle in held.handles {
                self.transport.close(handle);
                self.open -= 1;
            }
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
    }

    fn drop_buffer(&mut self, b: Buffer<T::End>) {
        self.transport.unmap(b.address, b.length);
        self.transport.close(b.end);
        self.open -= 1;
        self.buffers -= 1;
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

    /// After every engine call: ADR-0054's loss, then ADR-0016 rule 8's report.
    fn after_engine(&mut self) {
        if self.volume.device().is_lost() && !self.told_lost {
            self.told_lost = true;
            self.volume.device_lost();
        }
        if let Some(reason) = self.volume.take_failure_report() {
            self.transport.report(failure_code(reason));
        }
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
        match self.transport.write(c.end, &held.bytes, &held.handles) {
            Outgoing::Sent => {
                let sent = held.handles.len();
                c.held = None;
                self.open -= sent;
                let end = c.end;
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
        match self.transport.write(c.end, &self.output[..length], handles) {
            Outgoing::Sent => {
                // Moved to the client: no longer the service's.
                self.open -= handles.len();
                return;
            }
            Outgoing::Closed => {
                self.drop_handles(handles);
                self.close_connection(index);
                return;
            }
            Outgoing::Full | Outgoing::NoMemory => {}
        }
        // Held: one reply, and this connection is not read until it goes.
        // Its handles stay counted until then.
        let mut bytes = Vec::new();
        let mut held = Vec::new();
        if bytes.try_reserve_exact(length).is_err()
            || held.try_reserve_exact(handles.len()).is_err()
        {
            self.drop_handles(handles);
            self.close_connection(index);
            return;
        }
        bytes.extend_from_slice(&self.output[..length]);
        held.extend_from_slice(handles);
        c.held = Some(HeldReply {
            bytes,
            handles: held,
        });
        let end = c.end;
        self.transport.interest(end, index as u64, Interest::Reply);
    }

    /// Closes handles made for a reply that will not be sent.
    fn drop_handles(&mut self, handles: &[T::End]) {
        for &handle in handles {
            self.transport.close(handle);
        }
        self.open -= handles.len();
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
        self.connections[index]
            .as_ref()
            .is_some_and(|c| c.rights & right == right)
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
                self.clone_connection(i, t, m.rights.0, handles);
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
                    self.drop_buffer(b);
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
                self.read_directory(i, t, m.cookie);
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
                self.remove(i, t, m.name.0);
            }
            14 => {
                let (_, m) = decode::<proto::MakeDirectory<'_>>(bytes, n)?;
                self.make_directory(i, t, m.mode, m.name.0);
            }
            15 => {
                decode::<proto::Sync>(bytes, n)?;
                let r = if self.has(i, proto::rights::SYNC) {
                    self.volume.sync().map(|()| 0)
                } else {
                    return self.denied(i, t);
                };
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
                self.done(i, t, s, 0);
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
                self.make_link(i, t, m.name.0, m.target.0);
            }
            22 => {
                decode::<proto::ReadLink>(bytes, n)?;
                self.read_link(i, t);
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
    fn room(&self) -> bool {
        self.open < MAX_OPEN
    }

    fn open(&mut self, i: usize, t: u32, m: &proto::Open<'_>, handles: &mut Vec<T::End>) {
        use proto::{open_flags as f, rights as r};
        if !self.room() {
            return self.open_reply(i, t, status::TOO_MANY_OPEN, "", "");
        }
        let flags = m.flags.0;
        let creates = flags & (f::CREATE | f::UNNAMED) != 0;
        // A directory is opened, never made or truncated here: refused before
        // the engine runs, so that a refusal changes nothing (ADR-0010 rule 2).
        if flags & f::DIRECTORY != 0 && (creates || flags & f::TRUNCATE != 0) {
            return self.open_reply(i, t, status::INVALID_ARGUMENT, "", "");
        }
        let mut needed = m.rights.0;
        if creates {
            needed |= r::CREATE;
        }
        if flags & f::TRUNCATE != 0 {
            needed |= r::WRITE;
        }
        let Some(dir) = self.handle_of(i) else {
            return self.open_reply(i, t, status::STORAGE_FAILED, "", "");
        };
        if !self.has(i, needed) {
            return self.open_reply(i, t, status::ACCESS_DENIED, "", "");
        }
        let Some(end) = handles.first().copied() else {
            return self.open_reply(i, t, status::INVALID_ARGUMENT, "", "");
        };
        if self.transport.kind(end) != HandleKind::Channel {
            return self.open_reply(i, t, status::INVALID_ARGUMENT, "", "");
        }
        let opened = if flags & f::UNNAMED != 0 {
            self.volume.create_unnamed(dir, 0o644)
        } else {
            self.volume.open(
                dir,
                m.path.0,
                OpenOptions {
                    create: (flags & f::CREATE != 0).then_some(0o644),
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
                return self.open_reply(i, t, s, &remaining, &target);
            }
            Err(e) => {
                let s = self.engine_status(&e);
                return self.open_reply(i, t, s, "", "");
            }
        };
        if flags & f::DIRECTORY != 0
            && !matches!(
                self.volume.stat(handle).map(|a| a.kind),
                Ok(ObjKind::Directory)
            )
        {
            let _ = self.volume.close(handle);
            return self.open_reply(i, t, status::NOT_DIRECTORY, "", "");
        }
        handles.clear();
        let record = self.new_record(handle);
        let s = match self.add_connection(end, record, m.rights.0) {
            Some(_) => status::OK,
            None => status::LIMIT_REACHED,
        };
        self.open_reply(i, t, s, "", "");
    }

    fn clone_connection(&mut self, i: usize, t: u32, rights: u32, handles: &mut Vec<T::End>) {
        if !self.room() {
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
        match self.volume.reopen(handle) {
            Ok(handle) => {
                handles.clear();
                let record = self.new_record(handle);
                let s = match self.add_connection(end, record, rights) {
                    Some(_) => status::OK,
                    None => status::LIMIT_REACHED,
                };
                self.done(i, t, s, 0);
            }
            Err(e) => self.done_result(i, t, Err(e)),
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

    /// One atomic write, with what the flags ask (ADR-0010 rule 2).
    fn write_flagged(
        &mut self,
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
        if flags & proto::write_flags::DURABLE != 0 {
            self.volume.sync()?;
        }
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
        let r = self.write_flagged(handle, offset, flags, data);
        self.after_engine();
        self.done_result(i, t, r);
    }

    fn attach(&mut self, i: usize, t: u32, length: u64, handles: &mut Vec<T::End>) {
        if !self.room() {
            return self.done(i, t, status::TOO_MANY_OPEN, 0);
        }
        if self.buffers >= MAX_BUFFERS {
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
        self.open += 1;
        self.buffers += 1;
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
        let r = self.write_flagged(handle, m.offset, m.flags.0, &copy);
        self.after_engine();
        self.done_result(i, t, r);
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
                mode: u32::from(a.posix_mode),
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

    fn read_directory(&mut self, i: usize, t: u32, cookie: u64) {
        let fail = |s: u32, this: &mut Self| {
            let m = proto::ReadDirectoryReply {
                status: code(s),
                entries: Seq::Of(&[]),
            };
            this.reply(i, t, &m);
        };
        if !self.has(i, proto::rights::READ) {
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
        let mut found: Vec<(String, WireKind)> = Vec::new();
        let mut bytes = 0;
        while found.len() < proto::MAX_ENTRIES && bytes < 32 * 1024 {
            match self.volume.readdir(handle, after.as_deref()) {
                Ok(Some(e)) => {
                    after = Some(e.name.clone());
                    bytes += e.name.len() + 32;
                    found.push((e.name, wire_kind(e.kind)));
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
        let entries: Vec<proto::Entry<'_>> = found
            .iter()
            .map(|(name, kind)| proto::Entry {
                next,
                kind: *kind,
                name: Name(name),
            })
            .collect();
        let m = proto::ReadDirectoryReply {
            status: code(status::OK),
            entries: Seq::Of(&entries),
        };
        match encode(&m, t, &mut self.output) {
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
            None if self.room() => self.transport.new_token().inspect(|token| {
                self.open += 1;
                if let Some(c) = self.connections[i].as_mut() {
                    c.token = Some(*token);
                }
            }),
            None => {
                let m = proto::TokenReply {
                    status: code(status::TOO_MANY_OPEN),
                };
                return self.reply_without_handle(i, t, &m);
            }
        };
        // The copy is a handle the service holds until it is sent: it needs
        // room, and is counted.
        if !self.room() {
            let m = proto::TokenReply {
                status: code(status::TOO_MANY_OPEN),
            };
            return self.reply_without_handle(i, t, &m);
        }
        let copy = token.and_then(|token| self.transport.duplicate(token));
        if copy.is_some() {
            self.open += 1;
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
                self.drop_handles(&[copy]);
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
}

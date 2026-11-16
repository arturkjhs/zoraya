//! `AF_UNIX` sockets, the parts that need no kernel (ADR-0412, plan part F2):
//! the constants, the wire form of the descriptors a message carries, and the
//! book of tagged stream offsets that keeps a `SCM_RIGHTS` descriptor with the
//! byte it was sent with. The calls that use handles are `unix_io.rs`; this
//! file is total on any bytes a peer sends and is host tested.
//!
//! What a Unix socket is (ADR-0412 items 1 and 2):
//!
//! - `SOCK_SEQPACKET` and `SOCK_DGRAM`: one channel end. One `sendmsg` is one
//!   channel message: `count`, one [`Record`] for each descriptor, the payload.
//! - `SOCK_STREAM`: two crossed byte streams (ADR-0101) and one *ancillary
//!   channel*. A `sendmsg` with `SCM_RIGHTS` first queues an ancillary message
//!   (the magic, the stream offset of its first byte, the records) and then
//!   writes its bytes. A receiver tags each offset and cuts its reads there.

#![forbid(unsafe_code)]

use alloc::collections::VecDeque;
use alloc::vec::Vec;

pub const AF_UNIX: i32 = 1;
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;
pub const SOCK_SEQPACKET: i32 = 5;
pub const SCM_RIGHTS: i32 = 1;
/// `ETOOMANYREFS`, the error of too many descriptors in one message.
pub const ETOOMANYREFS: i32 = 109;

pub const MSG_CTRUNC: i32 = 0x8;
pub const MSG_TRUNC: i32 = 0x20;
pub const MSG_EOR: i32 = 0x80;
pub const MSG_CMSG_CLOEXEC: i32 = 0x4000_0000;

/// A channel message carries at most this many bytes and handles
/// (`zoraya_channel`'s limits, ADR-0412 item 3).
pub const MAX_MESSAGE: usize = 64 * 1024;
pub const MAX_HANDLES: usize = 64;

/// The bytes of one descriptor's record.
pub const RECORD: usize = 24;
/// The ancillary message's header: magic, offset, count and three bytes of
/// padding.
pub const ANC_HEADER: usize = 16;
const MAGIC: [u8; 4] = *b"ZAN1";

/// What a descriptor is on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A file: a fresh connection to the same description (ADR-0103 `share`).
    File = 1,
    Dir = 2,
    PipeRead = 3,
    PipeWrite = 4,
    /// A `SOCK_STREAM` Unix socket: its byte streams and ancillary channel.
    Stream = 5,
    /// A `SOCK_SEQPACKET` or `SOCK_DGRAM` Unix socket: its channel end.
    Packet = 6,
    /// A memory descriptor (ADR-0413): its memory object. The record's
    /// `read_off` is the descriptor's size and `write_off` its seals (bits 0
    /// to 4) and `MEMORY_SEALABLE`.
    Memory = 7,
}

/// In a memory record's `write_off`: the descriptor accepts `F_ADD_SEALS`.
pub const MEMORY_SEALABLE: u64 = 1 << 8;
/// The seal bits a memory record can carry (`F_SEAL_SEAL` to `F_SEAL_FUTURE_WRITE`).
pub const MEMORY_SEALS: u64 = 0x1f;
/// The largest size a memory record can claim (the descriptor's own limit,
/// `memfd::MAX_SIZE`).
pub const MEMORY_MAX_SIZE: u64 = 1 << 40;

impl Kind {
    fn from_byte(b: u8) -> Option<Kind> {
        Some(match b {
            1 => Kind::File,
            2 => Kind::Dir,
            3 => Kind::PipeRead,
            4 => Kind::PipeWrite,
            5 => Kind::Stream,
            6 => Kind::Packet,
            7 => Kind::Memory,
            _ => return None,
        })
    }
}

const NONBLOCK: u8 = 1;
const HAS_READ: u8 = 2;
const HAS_WRITE: u8 = 4;
const HAS_ANC: u8 = 8;
const SHARED: u8 = 16;

/// One descriptor of a message, without its handles (they travel beside the
/// bytes, in this order: read, write, ancillary for a stream socket).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub kind: Kind,
    /// `O_RDONLY`, `O_WRONLY` or `O_RDWR` of a file; 0 otherwise.
    pub access: u8,
    pub nonblock: bool,
    /// For a socket, its `SOCK_*` type.
    pub sock_type: u8,
    pub has_read: bool,
    pub has_write: bool,
    pub has_anc: bool,
    /// A stream socket whose ancillary channel is not in this record:
    /// `SCM_RIGHTS` on it stays `EOPNOTSUPP` (item 2).
    pub shared: bool,
    /// A stream socket's offsets, which its new holder carries on.
    pub read_off: u64,
    pub write_off: u64,
}

impl Record {
    /// A record of a kind that has one handle.
    pub fn one(kind: Kind, access: u8, nonblock: bool) -> Record {
        Record {
            kind,
            access,
            nonblock,
            sock_type: 0,
            has_read: false,
            has_write: false,
            has_anc: false,
            shared: false,
            read_off: 0,
            write_off: 0,
        }
    }

    /// How many handles this descriptor brings.
    pub fn handles(&self) -> usize {
        match self.kind {
            Kind::Stream => {
                usize::from(self.has_read) + usize::from(self.has_write) + usize::from(self.has_anc)
            }
            _ => 1,
        }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        let mut flags = 0;
        for (on, bit) in [
            (self.nonblock, NONBLOCK),
            (self.has_read, HAS_READ),
            (self.has_write, HAS_WRITE),
            (self.has_anc, HAS_ANC),
            (self.shared, SHARED),
        ] {
            if on {
                flags |= bit;
            }
        }
        out.extend_from_slice(&[self.kind as u8, self.access, self.sock_type, flags]);
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&self.read_off.to_le_bytes());
        out.extend_from_slice(&self.write_off.to_le_bytes());
    }

    fn decode(b: &[u8]) -> Option<Record> {
        let b: &[u8; RECORD] = b.get(..RECORD)?.try_into().ok()?;
        let kind = Kind::from_byte(b[0])?;
        let flags = b[3];
        if flags & !(NONBLOCK | HAS_READ | HAS_WRITE | HAS_ANC | SHARED) != 0 || b[4..8] != [0; 4] {
            return None;
        }
        let r = Record {
            kind,
            access: b[1],
            nonblock: flags & NONBLOCK != 0,
            sock_type: b[2],
            has_read: flags & HAS_READ != 0,
            has_write: flags & HAS_WRITE != 0,
            has_anc: flags & HAS_ANC != 0,
            shared: flags & SHARED != 0,
            read_off: u64::from_le_bytes(b[8..16].try_into().ok()?),
            write_off: u64::from_le_bytes(b[16..24].try_into().ok()?),
        };
        // A record says only what its kind can have.
        let sane = match kind {
            Kind::Stream => {
                r.sock_type == 1 && r.access == 0 && r.has_anc != r.shared && r.handles() > 0
            }
            Kind::Packet => {
                matches!(r.sock_type, 2 | 5)
                    && r.access == 0
                    && !r.has_read
                    && !r.has_write
                    && !r.has_anc
                    && !r.shared
                    && r.read_off == 0
                    && r.write_off == 0
            }
            // Read-only or read-write, a size, the known seals: how big and
            // what the object really is, the handle says.
            Kind::Memory => {
                matches!(r.access, 0 | 2)
                    && r.sock_type == 0
                    && !r.nonblock
                    && !r.has_read
                    && !r.has_write
                    && !r.has_anc
                    && !r.shared
                    && r.read_off <= MEMORY_MAX_SIZE
                    && r.write_off & !(MEMORY_SEALS | MEMORY_SEALABLE) == 0
            }
            Kind::File | Kind::Dir | Kind::PipeRead | Kind::PipeWrite => {
                r.access <= 2
                    && r.sock_type == 0
                    && !r.has_read
                    && !r.has_write
                    && !r.has_anc
                    && !r.shared
                    && r.read_off == 0
                    && r.write_off == 0
            }
        };
        sane.then_some(r)
    }
}

/// The handles a list of records brings.
pub fn handle_count(records: &[Record]) -> usize {
    records.iter().map(Record::handles).sum()
}

/// Of the handles of one descriptor, those a `fork` child holds too (ADR-0102
/// item 3 duplicates stream ends and cannot duplicate channel ends or file
/// connections): a pipe end, and a stream socket's read and write streams.
pub fn child_keeps<H: Copy>(r: &Record, handles: &[H]) -> Vec<H> {
    match r.kind {
        // A memory object is duplicable: the snapshot gave the child its own
        // handle at the same slot.
        Kind::PipeRead | Kind::PipeWrite | Kind::Memory => handles.to_vec(),
        Kind::Stream => handles
            .iter()
            .take(usize::from(r.has_read) + usize::from(r.has_write))
            .copied()
            .collect(),
        Kind::File | Kind::Dir | Kind::Packet => Vec::new(),
    }
}

// ------------------------------------------------------------ packets

/// The most payload a packet message carries beside `descriptors` records
/// (the `count` byte and the records come out of the channel's 64 KiB).
pub fn packet_room(descriptors: usize) -> usize {
    MAX_MESSAGE.saturating_sub(1 + RECORD * descriptors)
}

/// A packet message: `count`, the records, the payload.
pub fn encode_packet(records: &[Record], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + RECORD * records.len() + payload.len());
    out.push(records.len() as u8);
    for r in records {
        r.encode(&mut out);
    }
    out.extend_from_slice(payload);
    out
}

/// Splits a packet message: the records and where the payload starts. `None`
/// for bytes no sender of this library makes, or whose handles do not number
/// what the records say.
pub fn decode_packet(bytes: &[u8], handles: usize) -> Option<(Vec<Record>, usize)> {
    let count = usize::from(*bytes.first()?);
    let end = 1 + RECORD * count;
    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        records.push(Record::decode(bytes.get(1 + RECORD * i..end)?)?);
    }
    (handle_count(&records) == handles).then_some((records, end))
}

// ------------------------------------------------------------ a spawn's descriptors

/// The bootstrap message that precedes `SELF` when a spawn gives sockets or memory (ADR-0433): the
/// descriptors a spawn gives a child that a record's fd list cannot hold, a
/// Unix socket and a memory descriptor. The magic, then [`encode_packet`]'s
/// records without a payload, then each descriptor's fd number as a `u32`.
pub const PASSED_MAGIC: [u8; 4] = *b"ZPBF";

/// The message for `records`, given to the fds `fds` (one each).
pub fn encode_passed(fds: &[u32], records: &[Record]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 1 + RECORD * records.len() + 4 * fds.len());
    out.extend_from_slice(&PASSED_MAGIC);
    out.extend_from_slice(&encode_packet(records, &[]));
    for fd in fds {
        out.extend_from_slice(&fd.to_le_bytes());
    }
    out
}

/// The inverse: the fds in increasing order and below `limit`, the records
/// (a Unix socket or a memory descriptor each) and nothing else, whose
/// handles number `handles`. `None` for anything else, whoever sent it.
pub fn decode_passed(bytes: &[u8], handles: usize, limit: u32) -> Option<(Vec<u32>, Vec<Record>)> {
    if bytes.get(..4)? != PASSED_MAGIC {
        return None;
    }
    let body = &bytes[4..];
    let (records, end) = decode_packet(body, handles)?;
    let tail = &body[end..];
    if tail.len() != 4 * records.len() {
        return None;
    }
    let mut fds: Vec<u32> = Vec::with_capacity(records.len());
    for raw in tail.as_chunks::<4>().0 {
        let fd = u32::from_le_bytes(*raw);
        if fd >= limit || fds.last().is_some_and(|&l| fd <= l) {
            return None;
        }
        fds.push(fd);
    }
    records
        .iter()
        .all(|r| matches!(r.kind, Kind::Stream | Kind::Packet | Kind::Memory))
        .then_some((fds, records))
}

// ------------------------------------------------------------ the stream's tags

/// An ancillary message for a stream: the offset of the first byte its
/// descriptors belong to, and the records.
pub fn encode_anc(offset: u64, records: &[Record]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ANC_HEADER + RECORD * records.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&offset.to_le_bytes());
    out.push(records.len() as u8);
    out.extend_from_slice(&[0; 3]);
    for r in records {
        r.encode(&mut out);
    }
    out
}

/// The inverse of [`encode_anc`]; `None` as for [`decode_packet`].
pub fn decode_anc(bytes: &[u8], handles: usize) -> Option<(u64, Vec<Record>)> {
    if bytes.get(..4)? != MAGIC || bytes.get(13..16)? != [0; 3] {
        return None;
    }
    let offset = u64::from_le_bytes(bytes.get(4..12)?.try_into().ok()?);
    let count = usize::from(bytes[12]);
    if bytes.len() != ANC_HEADER + RECORD * count || count == 0 {
        return None;
    }
    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let at = ANC_HEADER + RECORD * i;
        records.push(Record::decode(&bytes[at..at + RECORD])?);
    }
    (handle_count(&records) == handles).then_some((offset, records))
}

/// Descriptors waiting for the byte at `offset` of a stream.
#[derive(Debug)]
pub struct Tagged<H> {
    pub offset: u64,
    pub items: Vec<(Record, Vec<H>)>,
}

/// What a stream socket knows of its two byte streams beyond their bytes:
/// how many it has written and read, the descriptors that came before their
/// bytes, and bytes read past a tag (ADR-0412 item 2).
#[derive(Debug)]
pub struct StreamBook<H> {
    pub read_off: u64,
    pub write_off: u64,
    pending: VecDeque<Tagged<H>>,
    /// Bytes taken from the stream beyond the next tag, to give first.
    pub rest: VecDeque<u8>,
}

impl<H> Default for StreamBook<H> {
    fn default() -> Self {
        StreamBook {
            read_off: 0,
            write_off: 0,
            pending: VecDeque::new(),
            rest: VecDeque::new(),
        }
    }
}

impl<H> StreamBook<H> {
    /// Files an ancillary message. Offsets come in order; one that is behind
    /// the bytes already read has lost its byte (a plain `read` took it) and
    /// is given back to be closed.
    pub fn push(&mut self, offset: u64, items: Vec<(Record, Vec<H>)>) -> Option<Tagged<H>> {
        let tag = Tagged { offset, items };
        if offset < self.read_off {
            return Some(tag);
        }
        self.pending.push_back(tag);
        None
    }

    /// The most of `want` bytes a read may take: up to the next tag past the
    /// first byte, so that descriptors arrive with the byte they were sent with.
    pub fn cap(&self, want: usize) -> usize {
        match self.pending.iter().find(|t| t.offset > self.read_off) {
            Some(t) => want.min((t.offset - self.read_off).min(usize::MAX as u64) as usize),
            None => want,
        }
    }

    /// `n` bytes were read: the tags whose byte was among them, and the
    /// offset moves on.
    pub fn consumed(&mut self, n: usize) -> Vec<Tagged<H>> {
        let end = self.read_off + n as u64;
        let mut out = Vec::new();
        while self.pending.front().is_some_and(|t| t.offset < end) {
            out.extend(self.pending.pop_front());
        }
        self.read_off = end;
        out
    }

    /// How many descriptors the next read would bring, at most `max`: those
    /// of the tag at the read offset.
    pub fn due_at_read(&self, max: usize) -> usize {
        match self.pending.front() {
            Some(t) if t.offset <= self.read_off => t.items.len().min(max),
            _ => 0,
        }
    }

    /// How many bytes after the first `n` lie past the next tag, if the bytes
    /// of a read were taken before a tag arrived.
    pub fn excess(&self, n: usize) -> usize {
        n.saturating_sub(self.cap(n))
    }

    /// Everything still held, to close when the socket goes.
    pub fn drain(&mut self) -> Vec<H> {
        let mut out = Vec::new();
        for t in self.pending.drain(..) {
            for (_, hs) in t.items {
                out.extend(hs);
            }
        }
        out
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.rest.is_empty()
    }

    /// In a `fork` child: what the library held is the parent's (its
    /// descriptors' handles are the parent's, its bytes are given there).
    /// The handles the child duplicated, which it must close (stream ends:
    /// see [`child_keeps`]); the channel ends are the parent's alone.
    pub fn after_fork(&mut self) -> Vec<H>
    where
        H: Copy,
    {
        let mut out = Vec::new();
        for t in self.pending.drain(..) {
            for (r, hs) in &t.items {
                out.extend(child_keeps(r, hs));
            }
        }
        self.rest.clear();
        out
    }

    /// Bytes the library holds, which a read gives before any stream byte.
    pub fn has_rest(&self) -> bool {
        !self.rest.is_empty()
    }
}

// ------------------------------------------------------------ socket files

/// The longest `sun_path` (`struct sockaddr_un`).
pub const SUN_PATH: usize = 108;
/// Backlog is a hint: a listener's queue is the channel's, 64 messages
/// (ADR-0050; proposed ADR-0439).
pub const LISTEN_QUEUE: usize = 64;

/// The path of a `struct sockaddr_un` of `addr` (INV-0412-3's input): the
/// bytes up to the first NUL or the end. `EINVAL` if the structure is
/// shorter than its family or longer than `sockaddr_un`, `EAFNOSUPPORT` for
/// another family or for the abstract namespace (no path, or one that starts
/// with NUL: ADR-0412 item 4).
pub fn parse_sockaddr_un(addr: &[u8]) -> Result<&[u8], i32> {
    use crate::errno::{EAFNOSUPPORT, EINVAL};
    if addr.len() < 2 || addr.len() > 2 + SUN_PATH {
        return Err(EINVAL);
    }
    if i32::from(u16::from_ne_bytes([addr[0], addr[1]])) != AF_UNIX {
        return Err(EAFNOSUPPORT);
    }
    let body = &addr[2..];
    if body.first().is_none_or(|&b| b == 0) {
        return Err(EAFNOSUPPORT);
    }
    let n = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    Ok(&body[..n])
}

/// The `struct sockaddr_un` of a bound `path`: the family, the path and a
/// NUL, as Linux's `getsockname` gives it (length `2 + path + 1`).
pub fn sockaddr_un(path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + path.len());
    out.extend_from_slice(&(AF_UNIX as u16).to_ne_bytes());
    out.extend_from_slice(path);
    out.push(0);
    out
}

/// INV-0412-3: where `bind` may make a socket file. The resolved path of the
/// call is `rest` inside table entry `entry`; only the line's `/tmp`, the
/// `Tmp` entry, with a name inside it, qualifies (proposed ADR-0439: a
/// program's own directory is not built). `EACCES` anywhere else; `EINVAL`
/// for the entry's root itself or a path that names a directory.
pub fn bind_place(
    entry: zoraya_abi::posix::EntryKind,
    rest: &str,
    directory: bool,
) -> Result<(), i32> {
    use crate::errno::{EACCES, EINVAL};
    if entry != zoraya_abi::posix::EntryKind::Tmp {
        return Err(EACCES);
    }
    if rest.is_empty() || directory {
        return Err(EINVAL);
    }
    Ok(())
}

/// The far end of a new connected pair, as the connector describes it to
/// the listener: one record (`unix_file.rs` builds the handles).
pub fn connect_record(sock_type: i32) -> Record {
    if sock_type == SOCK_STREAM {
        Record {
            sock_type: SOCK_STREAM as u8,
            has_read: true,
            has_write: true,
            has_anc: true,
            ..Record::one(Kind::Stream, 0, false)
        }
    } else {
        Record {
            sock_type: sock_type as u8,
            ..Record::one(Kind::Packet, 0, false)
        }
    }
}

/// The bytes of a connection record.
pub fn encode_connect(r: &Record) -> Vec<u8> {
    let mut out = Vec::with_capacity(RECORD);
    r.encode(&mut out);
    out
}

/// What a listener accepts from its channel: exactly one record of its own
/// socket type, whole (a stream's three handles, a packet socket's one),
/// with the offsets and flags of a new pair and the handle count it says.
/// Anything else, whoever sent it (the sender is any program that could
/// open the socket file for writing), is `None` and the caller closes the
/// handles.
pub fn decode_connect(bytes: &[u8], handles: usize, sock_type: i32) -> Option<Record> {
    if bytes.len() != RECORD {
        return None;
    }
    let r = Record::decode(bytes)?;
    let ok = match r.kind {
        Kind::Stream => {
            sock_type == SOCK_STREAM
                && r.has_read
                && r.has_write
                && r.has_anc
                && !r.shared
                && r.read_off == 0
                && r.write_off == 0
        }
        Kind::Packet => sock_type == SOCK_SEQPACKET && i32::from(r.sock_type) == sock_type,
        _ => false,
    };
    (ok && handles == r.handles()).then_some(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn stream_record(shared: bool) -> Record {
        Record {
            sock_type: 1,
            has_read: true,
            has_write: true,
            has_anc: !shared,
            shared,
            read_off: 9,
            write_off: 77,
            ..Record::one(Kind::Stream, 0, true)
        }
    }

    fn packet_record() -> Record {
        Record {
            sock_type: SOCK_SEQPACKET as u8,
            ..Record::one(Kind::Packet, 0, false)
        }
    }

    fn un(path: &[u8]) -> Vec<u8> {
        let mut a = (AF_UNIX as u16).to_ne_bytes().to_vec();
        a.extend_from_slice(path);
        a
    }

    #[test]
    fn a_socket_address_is_a_path_or_a_refusal() {
        use crate::errno::{EAFNOSUPPORT, EINVAL};
        assert_eq!(parse_sockaddr_un(&un(b"/tmp/s\0")), Ok(&b"/tmp/s"[..]));
        assert_eq!(
            parse_sockaddr_un(&un(b"/tmp/s")),
            Ok(&b"/tmp/s"[..]),
            "no NUL"
        );
        assert_eq!(parse_sockaddr_un(&un(b"/tmp/s\0junk")), Ok(&b"/tmp/s"[..]));
        // Exactly `sun_path` long with no NUL is a path; one more is not.
        let full = [b'a'; SUN_PATH];
        assert_eq!(parse_sockaddr_un(&un(&full)), Ok(&full[..]));
        assert_eq!(parse_sockaddr_un(&un(&[b'a'; SUN_PATH + 1])), Err(EINVAL));
        // The abstract namespace and the unnamed socket are not offered.
        assert_eq!(parse_sockaddr_un(&un(b"\0abstract")), Err(EAFNOSUPPORT));
        assert_eq!(parse_sockaddr_un(&un(b"")), Err(EAFNOSUPPORT));
        assert_eq!(parse_sockaddr_un(&un(b"\0")), Err(EAFNOSUPPORT));
        assert_eq!(parse_sockaddr_un(&[1]), Err(EINVAL));
        assert_eq!(parse_sockaddr_un(&[]), Err(EINVAL));
        let mut inet = un(b"/x");
        inet[0] = 2;
        assert_eq!(parse_sockaddr_un(&inet), Err(EAFNOSUPPORT));
        // The address `getsockname` gives parses back to the path.
        assert_eq!(
            parse_sockaddr_un(&sockaddr_un(b"/tmp/s")),
            Ok(&b"/tmp/s"[..])
        );
        assert_eq!(sockaddr_un(b"/tmp/s").len(), 2 + 6 + 1);
        // Total on any bytes.
        for len in 0..=(SUN_PATH + 4) {
            for fill in [0u8, 1, b'/', 255] {
                let mut b = alloc::vec![fill; len];
                if len >= 2 {
                    b[..2].copy_from_slice(&(AF_UNIX as u16).to_ne_bytes());
                }
                let _ = parse_sockaddr_un(&b);
            }
        }
    }

    /// INV-0412-3's path rule: only the `Tmp` entry, with a name inside it.
    #[test]
    fn bind_is_allowed_only_in_the_tmp_entry() {
        use crate::errno::{EACCES, EINVAL};
        use zoraya_abi::posix::EntryKind::*;
        assert_eq!(bind_place(Tmp, "sock", false), Ok(()));
        assert_eq!(bind_place(Tmp, "a/b/sock", false), Ok(()));
        for kind in [Directory, Null, Terminal, Image] {
            assert_eq!(bind_place(kind, "sock", false), Err(EACCES), "{kind:?}");
            assert_eq!(bind_place(kind, "", false), Err(EACCES), "{kind:?}");
        }
        assert_eq!(bind_place(Tmp, "", false), Err(EINVAL), "the entry itself");
        assert_eq!(bind_place(Tmp, "d", true), Err(EINVAL), "a trailing slash");
    }

    #[test]
    fn a_connection_record_decodes_only_in_the_form_its_listener_takes() {
        let stream = encode_connect(&connect_record(SOCK_STREAM));
        let packet = encode_connect(&connect_record(SOCK_SEQPACKET));
        assert_eq!(stream.len(), RECORD);
        assert!(decode_connect(&stream, 3, SOCK_STREAM).is_some());
        assert!(decode_connect(&packet, 1, SOCK_SEQPACKET).is_some());
        // The wrong type, the wrong handle count, a datagram record.
        assert!(decode_connect(&stream, 3, SOCK_SEQPACKET).is_none());
        assert!(decode_connect(&packet, 1, SOCK_STREAM).is_none());
        assert!(decode_connect(&stream, 2, SOCK_STREAM).is_none());
        assert!(decode_connect(&packet, 0, SOCK_SEQPACKET).is_none());
        // A datagram socket has no listener: not taken whatever the type says.
        assert!(
            decode_connect(&encode_connect(&connect_record(SOCK_DGRAM)), 1, SOCK_DGRAM).is_none()
        );
        // A record that says more than a fresh pair is: shared, with offsets,
        // a file, a memory descriptor, a stream with a handle left out.
        let mut shared = connect_record(SOCK_STREAM);
        shared.shared = true;
        shared.has_anc = false;
        assert!(decode_connect(&encode_connect(&shared), 2, SOCK_STREAM).is_none());
        let mut moved = connect_record(SOCK_STREAM);
        moved.read_off = 5;
        assert!(decode_connect(&encode_connect(&moved), 3, SOCK_STREAM).is_none());
        let mut half = connect_record(SOCK_STREAM);
        half.has_write = false;
        assert!(decode_connect(&encode_connect(&half), 2, SOCK_STREAM).is_none());
        let file = encode_connect(&Record::one(Kind::File, 2, false));
        assert!(decode_connect(&file, 1, SOCK_STREAM).is_none());
        assert!(decode_connect(&file, 1, SOCK_SEQPACKET).is_none());
        // Longer or shorter bytes, whatever they hold.
        let mut long = stream.clone();
        long.push(0);
        assert!(decode_connect(&long, 3, SOCK_STREAM).is_none());
        assert!(decode_connect(&stream[..23], 3, SOCK_STREAM).is_none());
    }

    #[test]
    fn a_spawns_descriptors_round_trip_and_nothing_else_decodes() {
        let rs = [
            stream_record(false),
            packet_record(),
            Record::one(Kind::Memory, 2, false),
        ];
        let bytes = encode_passed(&[3, 9, 40], &rs);
        let (fds, back) = decode_passed(&bytes, 5, 512).unwrap();
        assert_eq!((fds, back), (vec![3, 9, 40], rs.to_vec()));
        // The handles must number what the records say.
        assert!(decode_passed(&bytes, 4, 512).is_none());
        assert!(decode_passed(&bytes, 6, 512).is_none());
        // Fds out of order, repeated, or past the table.
        for bad in [[9, 3, 40], [3, 3, 40], [3, 9, 512]] {
            assert!(decode_passed(&encode_passed(&bad, &rs), 5, 512).is_none());
        }
        // A file or a pipe has a record, and no place in this message.
        let file = [Record::one(Kind::File, 0, false)];
        assert!(decode_passed(&encode_passed(&[3], &file), 1, 512).is_none());
        // Wrong magic, a short fd list, bytes after it.
        let mut wrong = bytes.clone();
        wrong[0] ^= 1;
        assert!(decode_passed(&wrong, 5, 512).is_none());
        assert!(decode_passed(&bytes[..bytes.len() - 1], 5, 512).is_none());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode_passed(&longer, 5, 512).is_none());
        // Total on any bytes.
        for n in 0..bytes.len() {
            let _ = decode_passed(&bytes[..n], 5, 512);
        }
        for i in 0..bytes.len() {
            for bit in 0..8 {
                let mut m = bytes.clone();
                m[i] ^= 1 << bit;
                let _ = decode_passed(&m, 5, 512);
            }
        }
    }

    #[test]
    fn records_round_trip_and_count_their_handles() {
        let rs = [
            Record::one(Kind::File, 2, true),
            Record::one(Kind::Dir, 0, false),
            Record::one(Kind::PipeRead, 0, false),
            Record::one(Kind::PipeWrite, 0, true),
            stream_record(false),
            stream_record(true),
            packet_record(),
        ];
        assert_eq!(handle_count(&rs), 1 + 1 + 1 + 1 + 3 + 2 + 1);
        let bytes = encode_packet(&rs, b"hi");
        assert_eq!(bytes.len(), 1 + RECORD * rs.len() + 2);
        let (back, at) = decode_packet(&bytes, 10).unwrap();
        assert_eq!(back, rs);
        assert_eq!(&bytes[at..], b"hi");
        assert!(decode_packet(&bytes, 9).is_none(), "handles must match");
        let anc = encode_anc(1234, &rs);
        assert_eq!(decode_anc(&anc, 10), Some((1234, rs.to_vec())));
        assert_eq!(decode_anc(&anc, 11), None);
    }

    /// A memory descriptor's record carries its size and seals, and says
    /// nothing a memory descriptor cannot have (ADR-0413).
    #[test]
    fn a_memory_record_round_trips_and_a_lie_about_its_kind_is_refused() {
        let mut m = Record::one(Kind::Memory, 2, false);
        m.read_off = 4 * 1024 * 1024;
        m.write_off = 0x12 | MEMORY_SEALABLE;
        let bytes = encode_packet(&[m], b"x");
        assert_eq!(decode_packet(&bytes, 1), Some((vec![m], 1 + RECORD)));
        assert_eq!(m.handles(), 1);
        let mut ro = m;
        ro.access = 0;
        assert!(decode_packet(&encode_packet(&[ro], b""), 1).is_some());
        let breaks: [fn(&mut Record); 9] = [
            |r| r.access = 1,
            |r| r.access = 3,
            |r| r.nonblock = true,
            |r| r.sock_type = 1,
            |r| r.has_read = true,
            |r| r.has_anc = true,
            |r| r.shared = true,
            |r| r.read_off = MEMORY_MAX_SIZE + 1,
            |r| r.write_off = 0x20,
        ];
        for (i, f) in breaks.iter().enumerate() {
            let mut odd = m;
            f(&mut odd);
            assert!(
                decode_packet(&encode_packet(&[odd], b""), odd.handles()).is_none(),
                "break {i}"
            );
        }
        // A memory record is one handle, which a fork child keeps.
        assert_eq!(child_keeps(&m, &[7u8]), vec![7u8]);
    }

    #[test]
    fn a_packet_with_no_descriptor_is_one_byte_and_the_payload() {
        let bytes = encode_packet(&[], b"abc");
        assert_eq!(bytes, vec![0, b'a', b'b', b'c']);
        assert_eq!(decode_packet(&bytes, 0), Some((vec![], 1)));
        assert_eq!(packet_room(0), MAX_MESSAGE - 1);
        assert_eq!(packet_room(2), MAX_MESSAGE - 1 - 2 * RECORD);
        assert_eq!(packet_room(10_000), 0);
    }

    /// The decoders take any bytes, and a record that says what its kind
    /// cannot have is refused.
    #[test]
    fn decoders_are_total_and_strict() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..3000 {
            let n = (next() % 120) as usize;
            let bytes: Vec<u8> = (0..n).map(|_| next() as u8).collect();
            for handles in 0..4 {
                let _ = decode_packet(&bytes, handles);
                let _ = decode_anc(&bytes, handles);
            }
        }
        assert!(decode_packet(&[], 0).is_none());
        assert!(decode_anc(&[], 0).is_none());
        let good = encode_anc(5, &[Record::one(Kind::PipeRead, 0, false)]);
        for i in 0..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 0x80;
            let r = decode_anc(&bad, 1);
            // Flipping a bit may give another valid message (the offset, a
            // flag the kind allows); it must never give the same.
            assert_ne!(r, decode_anc(&good, 1), "byte {i}");
        }
        // A pipe with a socket's offsets, a stream with no way to carry rights.
        let mut odd = Record::one(Kind::PipeRead, 0, false);
        odd.read_off = 1;
        assert!(decode_packet(&encode_packet(&[odd], b""), 1).is_none());
        let mut odd = stream_record(false);
        odd.has_anc = false;
        odd.shared = false;
        assert!(decode_packet(&encode_packet(&[odd], b""), 2).is_none());
        let mut odd = Record::one(Kind::File, 3, false);
        odd.access = 3;
        assert!(decode_packet(&encode_packet(&[odd], b""), 1).is_none());
        assert!(decode_anc(&encode_anc(0, &[]), 0).is_none(), "empty");
    }

    #[test]
    fn a_tag_holds_a_read_at_its_byte() {
        let mut b: StreamBook<u32> = StreamBook::default();
        let rec = Record::one(Kind::PipeRead, 0, false);
        // Bytes 0..10 have no descriptor, 10..20 have one, 20.. another.
        assert!(b.push(10, vec![(rec, vec![1])]).is_none());
        assert!(b.push(20, vec![(rec, vec![2])]).is_none());
        assert_eq!(b.cap(100), 10, "stops before the tag at 10");
        assert!(b.consumed(10).is_empty());
        assert_eq!(b.read_off, 10);
        assert_eq!(
            b.cap(100),
            10,
            "the tag at 10 is this read's; the next is 20"
        );
        let got = b.consumed(7);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].items[0].1, vec![1]);
        assert_eq!(b.cap(100), 3);
        assert!(b.consumed(3).is_empty());
        assert_eq!(b.cap(100), 100, "the tag at 20 is the next read's, last");
        let got = b.consumed(1);
        assert_eq!(got[0].items[0].1, vec![2]);
        assert!(!b.has_pending());
    }

    #[test]
    fn a_tag_behind_the_read_offset_is_given_back() {
        let mut b: StreamBook<u32> = StreamBook::default();
        b.consumed(50);
        let rec = Record::one(Kind::PipeRead, 0, false);
        let lost = b.push(10, vec![(rec, vec![9])]);
        assert_eq!(lost.unwrap().items[0].1, vec![9]);
        assert!(b.push(50, vec![(rec, vec![8])]).is_none());
        assert_eq!(b.drain(), vec![8]);
        assert!(!b.has_pending());
    }

    #[test]
    fn excess_counts_the_bytes_taken_past_a_tag() {
        let mut b: StreamBook<u32> = StreamBook::default();
        let rec = Record::one(Kind::PipeRead, 0, false);
        b.push(6, vec![(rec, vec![1])]);
        assert_eq!(b.excess(4), 0);
        assert_eq!(b.excess(6), 0);
        assert_eq!(b.excess(10), 4);
    }
}

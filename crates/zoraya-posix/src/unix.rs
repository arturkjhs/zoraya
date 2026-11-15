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
}

impl Kind {
    fn from_byte(b: u8) -> Option<Kind> {
        Some(match b {
            1 => Kind::File,
            2 => Kind::Dir,
            3 => Kind::PipeRead,
            4 => Kind::PipeWrite,
            5 => Kind::Stream,
            6 => Kind::Packet,
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
        Kind::PipeRead | Kind::PipeWrite => handles.to_vec(),
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

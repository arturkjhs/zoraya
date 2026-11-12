//! DNS messages for the resolver in `netd` (ADR-0223 item 1): one question,
//! type A, class IN, and the decoder of the answer.
//!
//! The decoder reads bytes from the network, that is from anyone who can
//! answer a UDP packet. It is total and bounded: whatever the input it
//! returns an [`Outcome`] or a [`ParseError`], never panics, never indexes
//! outside the message, follows a compression pointer only backwards and at
//! most [`MAX_JUMPS`] times per name (so a loop is impossible), reads at most
//! [`MAX_RECORDS`] records, keeps at most [`MAX_ADDRESSES`] addresses and
//! copies no name into anything larger than [`MAX_NAME`] bytes. It accepts an
//! address only from a record whose owner is the name asked for, or a name a
//! CNAME chain from it leads to, and only from a message that repeats our
//! question and carries our identifier: that is what an off-path sender must
//! guess (the identifier and port are not secret randomness yet, ADR-0227).
//!
//! `#![no_std]` and no allocation: the caller supplies the buffers.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

/// The longest name, in text, without a trailing dot (RFC 1035).
pub const MAX_NAME: usize = 253;
/// The longest label.
pub const MAX_LABEL: usize = 63;
/// The largest message the decoder reads; the resolver's receive buffer.
pub const MAX_MESSAGE: usize = 1232;
/// The largest query [`encode_query`] writes: 12 + 255 + 4.
pub const MAX_QUERY: usize = 12 + 255 + 4;
/// Addresses kept from one answer.
pub const MAX_ADDRESSES: usize = 8;
/// Records read from one answer; later ones are ignored (not an error: the
/// answer we want comes first in a resolver's reply, and a bound is the
/// point).
pub const MAX_RECORDS: usize = 32;
/// Compression pointers followed per name.
pub const MAX_JUMPS: usize = 8;
/// The longest TTL kept, in seconds.
pub const MAX_TTL: u32 = 3600;

const TYPE_A: u16 = 1;
const TYPE_CNAME: u16 = 5;
const CLASS_IN: u16 = 1;
const HEADER: usize = 12;

/// Why [`encode_query`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryError {
    /// Empty, over [`MAX_NAME`], an empty label, a label over [`MAX_LABEL`],
    /// or a byte that is not a letter, a digit, `-` or `_`.
    BadName,
    /// The output is too short for this name.
    NoRoom,
}

/// Whether `name` (no trailing dot) is a name [`encode_query`] sends.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= MAX_LABEL
                && l.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// A query for the A records of `name`, with identifier `id` and recursion
/// desired. A trailing dot is allowed and dropped. Returns the length.
pub fn encode_query(id: u16, name: &str, out: &mut [u8]) -> Result<usize, QueryError> {
    let name = name.strip_suffix('.').unwrap_or(name);
    if !valid_name(name) {
        return Err(QueryError::BadName);
    }
    // Labels add one length byte each and the root one more: name + 2.
    if out.len() < HEADER + name.len() + 2 + 4 {
        return Err(QueryError::NoRoom);
    }
    out[..2].copy_from_slice(&id.to_be_bytes());
    // Flags: a query, opcode 0, RD. One question, nothing else.
    out[2..HEADER].copy_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    let mut at = HEADER;
    for label in name.split('.') {
        out[at] = label.len() as u8;
        out[at + 1..at + 1 + label.len()].copy_from_slice(label.as_bytes());
        at += 1 + label.len();
    }
    out[at] = 0;
    out[at + 1..at + 3].copy_from_slice(&TYPE_A.to_be_bytes());
    out[at + 3..at + 5].copy_from_slice(&CLASS_IN.to_be_bytes());
    Ok(at + 5)
}

/// What a well-formed answer says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// At least one address for the name; `ttl` is the smallest of their
    /// TTLs, at most [`MAX_TTL`].
    Addresses(Answers),
    /// The server says the name does not exist.
    NoSuchName,
    /// The name exists and has no A record (or none that belongs to it).
    NoData,
    /// The server failed or refused (SERVFAIL, REFUSED, ...).
    ServerFailure,
    /// The answer did not fit the server's UDP size; TCP is not tried yet.
    Truncated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answers {
    pub addresses: [[u8; 4]; MAX_ADDRESSES],
    pub count: usize,
    pub ttl: u32,
}

impl Answers {
    pub fn as_slice(&self) -> &[[u8; 4]] {
        &self.addresses[..self.count.min(MAX_ADDRESSES)]
    }
}

/// Why a message is not an answer to our query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Shorter than its own fields say, or over [`MAX_MESSAGE`].
    Malformed,
    /// Not a response, or another opcode.
    NotAnAnswer,
    /// Another identifier than ours.
    WrongId,
    /// The question is not the one we asked.
    WrongQuestion,
    /// A name that points forward, loops, is too long or uses a reserved
    /// label type.
    BadName,
}

/// A name read out of a message, lower-cased, as text with dots, in a fixed
/// buffer.
#[derive(Clone, Copy)]
struct NameBuf {
    bytes: [u8; MAX_NAME],
    len: usize,
}

impl NameBuf {
    const fn new() -> NameBuf {
        NameBuf {
            bytes: [0; MAX_NAME],
            len: 0,
        }
    }

    fn push_label(&mut self, label: &[u8]) -> Result<(), ParseError> {
        let dot = usize::from(self.len > 0);
        if self.len + dot + label.len() > MAX_NAME {
            return Err(ParseError::BadName);
        }
        if dot == 1 {
            self.bytes[self.len] = b'.';
        }
        for (i, b) in label.iter().enumerate() {
            self.bytes[self.len + dot + i] = b.to_ascii_lowercase();
        }
        self.len += dot + label.len();
        Ok(())
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// Reads the name at `at` in `msg` into `out`; returns the position after
/// it in the record (after the first pointer, if there is one).
fn read_name(msg: &[u8], at: usize, out: &mut NameBuf) -> Result<usize, ParseError> {
    out.len = 0;
    let mut pos = at;
    let mut end = None;
    let mut jumps = 0;
    loop {
        let len = *msg.get(pos).ok_or(ParseError::Malformed)?;
        match len & 0xc0 {
            0 => {
                if len == 0 {
                    return Ok(end.unwrap_or(pos + 1));
                }
                let l = usize::from(len);
                let label = msg.get(pos + 1..pos + 1 + l).ok_or(ParseError::Malformed)?;
                out.push_label(label)?;
                pos += 1 + l;
            }
            0xc0 => {
                let low = *msg.get(pos + 1).ok_or(ParseError::Malformed)?;
                let target = usize::from(len & 0x3f) << 8 | usize::from(low);
                // Only backwards, and a few times: no loop can run.
                if target >= pos {
                    return Err(ParseError::BadName);
                }
                jumps += 1;
                if jumps > MAX_JUMPS {
                    return Err(ParseError::BadName);
                }
                end.get_or_insert(pos + 2);
                pos = target;
            }
            // 0x40 and 0x80 are extended and reserved label types.
            _ => return Err(ParseError::BadName),
        }
    }
}

fn be16(msg: &[u8], at: usize) -> Result<u16, ParseError> {
    let b = msg.get(at..at + 2).ok_or(ParseError::Malformed)?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(msg: &[u8], at: usize) -> Result<u32, ParseError> {
    let b = msg.get(at..at + 4).ok_or(ParseError::Malformed)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Decodes `msg` as the answer to the query `encode_query(id, name, ..)`.
pub fn parse_response(msg: &[u8], id: u16, name: &str) -> Result<Outcome, ParseError> {
    if msg.len() < HEADER || msg.len() > MAX_MESSAGE {
        return Err(ParseError::Malformed);
    }
    if be16(msg, 0)? != id {
        return Err(ParseError::WrongId);
    }
    let flags = be16(msg, 2)?;
    // QR set, opcode 0.
    if flags & 0x8000 == 0 || flags & 0x7800 != 0 {
        return Err(ParseError::NotAnAnswer);
    }
    let (qd, an) = (be16(msg, 4)?, be16(msg, 6)?);
    // The question must be ours, exactly one, repeated.
    if qd != 1 {
        return Err(ParseError::WrongQuestion);
    }
    let mut asked = NameBuf::new();
    let mut at = read_name(msg, HEADER, &mut asked)?;
    let name = name.strip_suffix('.').unwrap_or(name);
    if be16(msg, at)? != TYPE_A
        || be16(msg, at + 2)? != CLASS_IN
        || !asked.as_bytes().eq_ignore_ascii_case(name.as_bytes())
    {
        return Err(ParseError::WrongQuestion);
    }
    at += 4;
    match flags & 0x000f {
        0 => {}
        3 => return Ok(Outcome::NoSuchName),
        _ => return Ok(Outcome::ServerFailure),
    }
    if flags & 0x0200 != 0 {
        return Ok(Outcome::Truncated);
    }
    let mut answers = Answers {
        addresses: [[0; 4]; MAX_ADDRESSES],
        count: 0,
        ttl: u32::MAX,
    };
    let mut current = asked;
    let mut owner = NameBuf::new();
    let mut target = NameBuf::new();
    for _ in 0..usize::from(an).min(MAX_RECORDS) {
        let after = read_name(msg, at, &mut owner)?;
        let kind = be16(msg, after)?;
        let class = be16(msg, after + 2)?;
        let ttl = be32(msg, after + 4)?;
        let rdlength = usize::from(be16(msg, after + 8)?);
        let rdata = after + 10;
        let end = rdata + rdlength;
        if end > msg.len() {
            return Err(ParseError::Malformed);
        }
        at = end;
        if class != CLASS_IN || owner.as_bytes() != current.as_bytes() {
            continue;
        }
        match kind {
            TYPE_A => {
                if rdlength != 4 {
                    return Err(ParseError::Malformed);
                }
                if answers.count < MAX_ADDRESSES {
                    answers.addresses[answers.count] =
                        [msg[rdata], msg[rdata + 1], msg[rdata + 2], msg[rdata + 3]];
                    answers.count += 1;
                    answers.ttl = answers.ttl.min(ttl);
                }
            }
            TYPE_CNAME => {
                // The name in the rdata must end inside the rdata.
                let stop = read_name(msg, rdata, &mut target)?;
                if stop > end {
                    return Err(ParseError::Malformed);
                }
                current = target;
            }
            _ => {}
        }
    }
    if answers.count == 0 {
        return Ok(Outcome::NoData);
    }
    answers.ttl = answers.ttl.min(MAX_TTL);
    Ok(Outcome::Addresses(answers))
}

#[cfg(test)]
zoraya_test_alloc::cap_memory!();

#[cfg(test)]
mod tests;

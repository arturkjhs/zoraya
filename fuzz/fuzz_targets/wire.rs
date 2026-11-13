//! Shared by the Q7 fuzz targets (ADR-0298): how the fuzzer's bytes become
//! the messages of a protocol. A message is `sel, flags, ordinal, length`
//! and `length` bytes: with flag bit 2 set the bytes are the whole message as
//! they are (the header's checks are fuzzed); otherwise they are the body of
//! a message with a valid header and that ordinal, so that the fuzzer reaches
//! the decoders and not only the magic number.

use zoraya_protocol::{HEADER_SIZE, MAGIC_VERSION, MAX_WIRE_MESSAGE};

pub struct Msg {
    /// Which channel, queue or connection the message goes to.
    pub sel: u8,
    /// How many handles it carries (0 to 3).
    pub handles: usize,
    pub bytes: Vec<u8>,
}

/// The messages in `data`, at most `limit`.
pub fn messages(data: &[u8], limit: usize) -> Vec<Msg> {
    let mut out = Vec::new();
    let mut rest = data;
    while out.len() < limit && rest.len() >= 4 {
        let (sel, flags, ordinal, len) = (rest[0], rest[1], rest[2], rest[3] as usize);
        let body_len = len.min(rest.len() - 4);
        let body = &rest[4..4 + body_len];
        rest = &rest[4 + body_len..];
        let bytes = if flags & 4 != 0 {
            body.to_vec()
        } else {
            frame(u32::from(ordinal), u32::from(flags >> 3), body)
        };
        out.push(Msg { sel, handles: usize::from(flags & 3), bytes });
    }
    out
}

/// A message with a valid header around `body`.
pub fn frame(ordinal: u32, transaction: u32, body: &[u8]) -> Vec<u8> {
    let body = &body[..body.len().min(MAX_WIRE_MESSAGE - HEADER_SIZE)];
    let mut v = Vec::with_capacity(HEADER_SIZE + body.len());
    v.extend_from_slice(&MAGIC_VERSION.to_le_bytes());
    v.extend_from_slice(&ordinal.to_le_bytes());
    v.extend_from_slice(&transaction.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(body);
    v
}

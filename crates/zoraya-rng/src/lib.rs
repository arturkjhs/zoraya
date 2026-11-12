//! The random service's logic (ADR-0227): the generator, the protocol, and
//! the service loop's decisions, with no I/O so that host tests cover them.
//!
//! **The generator** is ChaCha20 (RFC 8439's block function) used as a
//! generator with fast key erasure: each request derives its output from a
//! block stream under the current key, and the first 32 bytes of that
//! stream replace the key before any output is given, so a later compromise
//! of the state does not reveal earlier output. **Mixing** entropy in (the
//! boot seed, a device's bytes) absorbs it 32 bytes at a time: the key is
//! XORed with the chunk and replaced by the first half of the block under
//! it (with a counter in the nonce and a domain word), then the total length
//! is absorbed the same way. This is a deliberately small construction
//! recorded in ADR-0288 for the reviewer; the rest of the system's
//! cryptography is `ring`'s.
//!
//! **The protocol**: `Bytes { count }` answers `BytesReply` with exactly
//! `count` bytes for `count` up to [`MAX_BYTES`] (INV-0227-3: no waiting, no
//! short reply); `Share` carries the server end of a new connection and
//! answers `ShareReply` (a fork child, a spawned program, ADR-0227 item 3).
//! Anything else gets `ErrorReply` and the connection stays; a message that
//! does not decode ends the connection.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Data, ERROR_ORDINAL, ErrorReply, Header, decode, encode, protocol};

/// The most bytes one `Bytes` request asks for.
pub const MAX_BYTES: usize = 4096;

pub mod status {
    pub const OK: u32 = 0;
    /// A `Bytes` over [`super::MAX_BYTES`].
    pub const TOO_MANY: u32 = 1;
    /// The service's table of connections is full.
    pub const NO_SPACE: u32 = 2;
    /// An ordinal the service does not know.
    pub const UNKNOWN: u32 = 3;
    pub const COUNT: u32 = 4;
}

pub type Code = Below<{ status::COUNT }>;

protocol! {
    message Bytes = 1, handles 0 {
        count: u32,
    }
    message BytesReply<'a> = 1, handles 0 {
        bytes: Data<'a, MAX_BYTES>,
    }

    /// Carries the server end of a new connection.
    message Share = 2, handles 1 {}
    message ShareReply = 2, handles 0 {
        status: Code,
    }
}

const BYTES: u32 = 1;
const SHARE: u32 = 2;

// ---- ChaCha20 -------------------------------------------------------------

fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

/// RFC 8439's block function: 64 bytes for `key`, `counter` and `nonce`.
pub fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for i in 0..8 {
        state[4 + i] =
            u32::from_le_bytes([key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]]);
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[4 * i],
            nonce[4 * i + 1],
            nonce[4 * i + 2],
            nonce[4 * i + 3],
        ]);
    }
    let mut work = state;
    for _ in 0..10 {
        quarter(&mut work, 0, 4, 8, 12);
        quarter(&mut work, 1, 5, 9, 13);
        quarter(&mut work, 2, 6, 10, 14);
        quarter(&mut work, 3, 7, 11, 15);
        quarter(&mut work, 0, 5, 10, 15);
        quarter(&mut work, 1, 6, 11, 12);
        quarter(&mut work, 2, 7, 8, 13);
        quarter(&mut work, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[4 * i..4 * i + 4].copy_from_slice(&work[i].wrapping_add(state[i]).to_le_bytes());
    }
    out
}

// ---- The generator ----------------------------------------------------------

/// Domain words in the nonce: output and absorption never use one block
/// function input twice.
const OUTPUT: u32 = 0x4f55_5450;
const ABSORB: u32 = 0x4142_5342;

fn nonce(domain: u32, count: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..4].copy_from_slice(&domain.to_le_bytes());
    n[4..].copy_from_slice(&count.to_le_bytes());
    n
}

/// The generator. Its state is a key; nothing else is secret.
pub struct Generator {
    key: [u8; 32],
    /// Mixes so far: part of the nonce of an absorption.
    mixes: u64,
    /// Requests so far: part of the nonce of an output.
    requests: u64,
}

impl Generator {
    /// A generator that has absorbed `seed` (a device tree's `rng-seed`).
    pub fn new(seed: &[u8]) -> Generator {
        let mut g = Generator {
            key: [0; 32],
            mixes: 0,
            requests: 0,
        };
        g.mix(seed);
        g
    }

    /// Absorbs `input`; the key afterwards depends on every byte of it and
    /// on the state before.
    pub fn mix(&mut self, input: &[u8]) {
        for chunk in input.chunks(32) {
            for (k, b) in self.key.iter_mut().zip(chunk) {
                *k ^= b;
            }
            self.rekey();
        }
        // The length, so that inputs which differ by trailing zeros differ.
        for (k, b) in self.key.iter_mut().zip((input.len() as u64).to_le_bytes()) {
            *k ^= b;
        }
        self.rekey();
    }

    fn rekey(&mut self) {
        let block = chacha20_block(&self.key, 0, &nonce(ABSORB, self.mixes));
        self.mixes = self.mixes.wrapping_add(1);
        self.key.copy_from_slice(&block[..32]);
    }

    /// Fills `out` with output; any length, never fails, never waits.
    pub fn fill(&mut self, out: &mut [u8]) {
        let n = nonce(OUTPUT, self.requests);
        self.requests = self.requests.wrapping_add(1);
        // Block 0: the next key and 32 bytes of output; then whole blocks.
        let first = chacha20_block(&self.key, 0, &n);
        let old = self.key;
        self.key.copy_from_slice(&first[..32]);
        let mut counter = 1u32;
        let mut rest = &mut out[..];
        let take = rest.len().min(32);
        let (head, tail) = rest.split_at_mut(take);
        head.copy_from_slice(&first[32..32 + take]);
        rest = tail;
        while !rest.is_empty() {
            let block = chacha20_block(&old, counter, &n);
            counter = counter.wrapping_add(1);
            let take = rest.len().min(64);
            let (head, tail) = rest.split_at_mut(take);
            head.copy_from_slice(&block[..take]);
            rest = tail;
        }
    }
}

// ---- The service ------------------------------------------------------------

/// What `rngd` does with one message.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Write `out[..n]` to the connection.
    Reply(usize),
    /// Write `out[..n]` to the connection and keep the handle as a new
    /// connection; if `attach` is false close it (the table is full).
    Share { reply: usize, attach: bool },
    /// The message was not one: end this connection.
    Close,
}

/// The service: a generator and a bound on its connections.
pub struct Service {
    generator: Generator,
    connections: usize,
    max_connections: usize,
}

/// The smallest `out` that [`Service::handle`] needs.
pub const OUT_SIZE: usize = MAX_BYTES + 64;

impl Service {
    pub fn new(generator: Generator, max_connections: usize) -> Service {
        Service {
            generator,
            connections: 0,
            max_connections,
        }
    }

    /// Mixes bytes from the device in (reseeding).
    pub fn reseed(&mut self, bytes: &[u8]) {
        self.generator.mix(bytes);
    }

    pub fn connections(&self) -> usize {
        self.connections
    }

    /// A connection the loop opened (the first one, `init`'s).
    pub fn attached(&mut self) -> bool {
        if self.connections < self.max_connections {
            self.connections += 1;
            true
        } else {
            false
        }
    }

    pub fn detached(&mut self) {
        self.connections = self.connections.saturating_sub(1);
    }

    fn error(code: u32, transaction: u32, out: &mut [u8]) -> Action {
        match encode(&ErrorReply { status: code }, transaction, out) {
            Ok(n) => Action::Reply(n),
            Err(_) => Action::Close,
        }
    }

    /// Decides on `message` that came with `handles` handles. `out` is at
    /// least [`OUT_SIZE`] bytes.
    pub fn handle(&mut self, message: &[u8], handles: usize, out: &mut [u8]) -> Action {
        let Ok((header, _)) = Header::decode(message) else {
            return Action::Close;
        };
        match header.ordinal {
            BYTES => {
                let Ok((h, m)) = decode::<Bytes>(message, handles) else {
                    return Action::Close;
                };
                let count = m.count as usize;
                if count > MAX_BYTES {
                    return Self::error(status::TOO_MANY, h.transaction, out);
                }
                // The reply is the header, a length, then the bytes: fill a
                // scratch area and encode from it.
                let mut bytes = [0u8; MAX_BYTES];
                self.generator.fill(&mut bytes[..count]);
                match encode(
                    &BytesReply {
                        bytes: Data(&bytes[..count]),
                    },
                    h.transaction,
                    out,
                ) {
                    Ok(n) => Action::Reply(n),
                    Err(_) => Action::Close,
                }
            }
            SHARE => {
                let Ok((h, _)) = decode::<Share>(message, handles) else {
                    return Action::Close;
                };
                let attach = self.attached();
                let code = if attach { status::OK } else { status::NO_SPACE };
                match encode(
                    &ShareReply {
                        status: Below(code),
                    },
                    h.transaction,
                    out,
                ) {
                    Ok(n) => Action::Share { reply: n, attach },
                    Err(_) => Action::Close,
                }
            }
            ERROR_ORDINAL => Action::Close,
            _ => Self::error(status::UNKNOWN, header.transaction, out),
        }
    }
}

#[cfg(test)]
zoraya_test_alloc::cap_memory!();

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

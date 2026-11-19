//! SHA-256 (FIPS 180-4), written here because the record of an installed
//! program binds its name to a hash (ADR-0396 item 4) and the only other
//! implementation in the tree is `ring` inside `zoraya-tls`, which the shell
//! does not link. Streaming: a program of 64 MiB is hashed a block at a time
//! from the memory object the loader reads (ADR-0396 item 8).

use alloc::string::String;

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const INIT: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// The digest of a program and its description: 32 bytes.
pub type Digest = [u8; 32];

/// An incremental hash.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    filled: usize,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256 {
            state: INIT,
            block: [0; 64],
            filled: 0,
            length: 0,
        }
    }

    fn compress(state: &mut [u32; 8], block: &[u8]) {
        let mut w = [0u32; 64];
        let (words, _) = block.as_chunks::<4>();
        for (i, c) in words.iter().enumerate() {
            w[i] = u32::from_be_bytes(*c);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.filled > 0 {
            let take = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled < 64 {
                return;
            }
            let block = self.block;
            Self::compress(&mut self.state, &block);
            self.filled = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for c in blocks {
            Self::compress(&mut self.state, c);
        }
        self.block[..rest.len()].copy_from_slice(rest);
        self.filled = rest.len();
    }

    pub fn finish(mut self) -> Digest {
        let bits = self.length.wrapping_mul(8);
        let mut pad = [0u8; 72];
        pad[0] = 0x80;
        let zeros = (119 - self.filled) % 64;
        // 1 byte of 0x80, `zeros` of 0, then the 8-byte length.
        let total = 1 + zeros + 8;
        pad[1 + zeros..total].copy_from_slice(&bits.to_be_bytes());
        // `update` would add to `length`; the padding is not part of it.
        let keep = self.length;
        self.update(&pad[..total]);
        self.length = keep;
        let mut out = [0u8; 32];
        let (words, _) = out.as_chunks_mut::<4>();
        for (o, s) in words.iter_mut().zip(self.state) {
            *o = s.to_be_bytes();
        }
        out
    }
}

/// The digest of `data`.
pub fn hash(data: &[u8]) -> Digest {
    let mut h = Sha256::new();
    h.update(data);
    h.finish()
}

/// Lower-case hex of a digest.
pub fn hex(d: &Digest) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for b in d {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    s
}

/// A digest from 64 hex digits (either case), or `None`.
pub fn from_hex(text: &str) -> Option<Digest> {
    let b = text.as_bytes();
    if b.len() != 64 {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = nibble(b[2 * i])? << 4 | nibble(b[2 * i + 1])?;
    }
    Some(out)
}

/// What the record binds a name to (ADR-0396 item 4): the program's bytes
/// then the description's, each with its length as eight bytes first, so no
/// move of the boundary between them gives the same digest.
pub fn identity(program: &[u8], description: &[u8]) -> Digest {
    let mut h = Sha256::new();
    h.update(&(program.len() as u64).to_le_bytes());
    h.update(program);
    h.update(&(description.len() as u64).to_le_bytes());
    h.update(description);
    h.finish()
}

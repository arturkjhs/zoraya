//! A PNG encoder with its own deflate (LZ77 over a 32 KiB window with
//! fixed Huffman codes), so that the tool needs no crate. Flat UI images
//! compress well enough this way; a dynamic-Huffman encoder would be
//! smaller still and is not worth its size here.

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (n, e) in t.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *e = c;
    }
    t
}

fn crc32(table: &[u32; 256], data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter {
            out: Vec::new(),
            acc: 0,
            n: 0,
        }
    }
    /// Writes `count` bits of `value`, least significant first.
    fn bits(&mut self, value: u32, count: u32) {
        self.acc |= (value as u64) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// Writes a Huffman code, most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        let mut rev = 0u32;
        for i in 0..len {
            if code & (1 << i) != 0 {
                rev |= 1 << (len - 1 - i);
            }
        }
        self.bits(rev, len);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

fn literal(w: &mut BitWriter, v: u16) {
    match v {
        0..=143 => w.code(0x30 + v as u32, 8),
        144..=255 => w.code(0x190 + (v as u32 - 144), 9),
        256..=279 => w.code(v as u32 - 256, 7),
        _ => w.code(0xC0 + (v as u32 - 280), 8),
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn emit_match(w: &mut BitWriter, len: usize, dist: usize) {
    let li = LEN_BASE
        .iter()
        .rposition(|&b| b as usize <= len)
        .unwrap_or(0);
    literal(w, 257 + li as u16);
    w.bits((len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li]);
    let di = DIST_BASE
        .iter()
        .rposition(|&b| b as usize <= dist)
        .unwrap_or(0);
    w.code(di as u32, 5);
    w.bits((dist - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di]);
}

/// Deflate with one fixed-Huffman block.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32768;
    const MAX_CHAIN: usize = 48;
    let mut w = BitWriter::new();
    w.bits(1, 1); // final block
    w.bits(1, 2); // fixed Huffman
    let mut head = vec![usize::MAX; 1 << 15];
    let mut prev = vec![usize::MAX; data.len()];
    let hash = |i: usize| -> usize {
        let h = (data[i] as usize) << 10 ^ (data[i + 1] as usize) << 5 ^ data[i + 2] as usize;
        (h.wrapping_mul(2654435761)) >> 17 & 0x7FFF
    };
    let mut i = 0;
    while i < data.len() {
        let mut best_len = 0;
        let mut best_dist = 0;
        if i + 2 < data.len() {
            let h = hash(i);
            let mut cand = head[h];
            let mut chain = 0;
            while cand != usize::MAX && i - cand <= WINDOW && chain < MAX_CHAIN {
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == max {
                        break;
                    }
                }
                cand = prev[cand];
                chain += 1;
            }
            prev[i] = head[h];
            head[h] = i;
        }
        if best_len >= 3 {
            emit_match(&mut w, best_len, best_dist);
            // insert the skipped positions into the chains
            let mut k = i + 1;
            while k < i + best_len {
                if k + 2 < data.len() {
                    let h = hash(k);
                    prev[k] = head[h];
                    head[h] = k;
                }
                k += 1;
            }
            i += best_len;
        } else {
            literal(&mut w, data[i] as u16);
            i += 1;
        }
    }
    literal(&mut w, 256);
    w.finish()
}

fn chunk(table: &[u32; 256], out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = Vec::with_capacity(4 + data.len());
    body.extend_from_slice(kind);
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(table, &body).to_be_bytes());
}

/// Encodes an RGB image (pixels as `0x00RRGGBB`) as PNG bytes.
pub fn encode(width: usize, height: usize, pixels: &[u32]) -> Vec<u8> {
    assert_eq!(pixels.len(), width * height);
    let table = crc_table();
    let stride = width * 3;
    let mut raw = Vec::with_capacity((stride + 1) * height);
    let mut row = vec![0u8; stride];
    let mut prev_row = vec![0u8; stride];
    for y in 0..height {
        for x in 0..width {
            let p = pixels[y * width + x];
            row[3 * x] = (p >> 16) as u8;
            row[3 * x + 1] = (p >> 8) as u8;
            row[3 * x + 2] = p as u8;
        }
        // filter type 2 (Up); the first row's "up" is all zero, which is
        // the same as no filter.
        raw.push(2);
        for x in 0..stride {
            raw.push(row[x].wrapping_sub(prev_row[x]));
        }
        std::mem::swap(&mut row, &mut prev_row);
    }
    let mut z = vec![0x78, 0x01];
    z.extend_from_slice(&deflate(&raw));
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&table, &mut out, b"IHDR", &ihdr);
    chunk(&table, &mut out, b"IDAT", &z);
    chunk(&table, &mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal inflate for one fixed-Huffman block, so that the test
    /// checks what the encoder wrote without a crate.
    fn inflate_fixed(data: &[u8]) -> Vec<u8> {
        struct R<'a> {
            d: &'a [u8],
            pos: usize,
            bit: u32,
        }
        impl R<'_> {
            fn bit(&mut self) -> u32 {
                let b = (self.d[self.pos] >> self.bit) & 1;
                self.bit += 1;
                if self.bit == 8 {
                    self.bit = 0;
                    self.pos += 1;
                }
                b as u32
            }
            fn bits(&mut self, n: u32) -> u32 {
                (0..n).fold(0, |acc, i| acc | self.bit() << i)
            }
            fn code(&mut self, n: u32) -> u32 {
                (0..n).fold(0, |acc, _| acc << 1 | self.bit())
            }
        }
        let mut r = R {
            d: data,
            pos: 0,
            bit: 0,
        };
        assert_eq!(r.bits(1), 1);
        assert_eq!(r.bits(2), 1);
        let mut out = Vec::new();
        loop {
            // decode a fixed literal/length code
            let mut c = r.code(7);
            let sym = if c <= 0b0010111 {
                256 + c
            } else {
                c = c << 1 | r.bit();
                if (0b00110000..=0b10111111).contains(&c) {
                    c - 0b00110000
                } else if (0b11000000..=0b11000111).contains(&c) {
                    280 + (c - 0b11000000)
                } else {
                    c = c << 1 | r.bit();
                    144 + (c - 0b110010000)
                }
            };
            if sym < 256 {
                out.push(sym as u8);
            } else if sym == 256 {
                break;
            } else {
                let li = (sym - 257) as usize;
                let len = LEN_BASE[li] as usize + r.bits(LEN_EXTRA[li]) as usize;
                let di = r.code(5) as usize;
                let dist = DIST_BASE[di] as usize + r.bits(DIST_EXTRA[di]) as usize;
                let start = out.len() - dist;
                for k in 0..len {
                    out.push(out[start + k]);
                }
            }
        }
        out
    }

    #[test]
    fn deflate_round_trips() {
        let mut data = Vec::new();
        for i in 0..5000u32 {
            data.push((i % 7) as u8);
            data.push((i / 100) as u8);
        }
        data.extend_from_slice(b"the quick brown fox jumps over the lazy dog, the quick brown fox");
        let z = deflate(&data);
        assert!(z.len() < data.len() / 4, "{} vs {}", z.len(), data.len());
        assert_eq!(inflate_fixed(&z), data);
    }

    #[test]
    fn png_has_the_right_chunks_and_crcs() {
        let px = vec![0x00FF8000u32; 16 * 8];
        let png = encode(16, 8, &px);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let table = crc_table();
        let mut p = 8;
        let mut kinds = Vec::new();
        while p < png.len() {
            let len = u32::from_be_bytes([png[p], png[p + 1], png[p + 2], png[p + 3]]) as usize;
            let kind = &png[p + 4..p + 8];
            let crc = u32::from_be_bytes([
                png[p + 8 + len],
                png[p + 9 + len],
                png[p + 10 + len],
                png[p + 11 + len],
            ]);
            assert_eq!(crc, crc32(&table, &png[p + 4..p + 8 + len]));
            kinds.push(String::from_utf8_lossy(kind).to_string());
            p += 12 + len;
        }
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
    }

    #[test]
    fn crc_and_adler_known_values() {
        let t = crc_table();
        assert_eq!(crc32(&t, b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }
}

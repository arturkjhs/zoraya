//! CRC32C (Castagnoli), ADR-0012 rule 2.
//!
//! Table-driven, slicing by one byte. The ADR asks for the CPU's CRC32C
//! instructions where the CPU reports them; this crate is
//! `#![forbid(unsafe_code)]` (INV-0012-3) and the intrinsics are `unsafe`, so
//! the hardware path is deferred (ADR-0016, item 6). The result is
//! the same either way; only speed differs.

/// Reflected Castagnoli polynomial.
const POLY: u32 = 0x82F6_3B78;

const TABLE: [u32; 256] = make_table();

const fn make_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Continues a CRC32C over `data`. Start with `0`.
pub fn update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        c = TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// CRC32C of `data`.
pub fn crc32c(data: &[u8]) -> u32 {
    update(0, data)
}

/// The 64-bit checksum field of ADR-0012 rule 2: CRC32C in the low half,
/// zero in the high half (algorithm 1).
pub fn checksum64(data: &[u8]) -> u64 {
    u64::from(crc32c(data))
}

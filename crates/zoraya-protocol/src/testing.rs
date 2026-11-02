//! Decoder tests for a protocol crate: every truncation, every single bit
//! flipped, several bytes changed at once, and wholly random bodies under the
//! valid header. A decoder under test must not panic; what it
//! accepts it must be able to read back in full (`decode` does that).

/// Feeds `decode` every damaged form of `valid`.
pub fn exercise(valid: &[u8], decode: impl Fn(&[u8])) {
    for cut in 0..valid.len() {
        decode(&valid[..cut]);
    }
    let mut copy = [0u8; 4096];
    let copy = &mut copy[..valid.len()];
    copy.copy_from_slice(valid);
    for at in 0..copy.len() {
        for bit in 0..8 {
            copy[at] ^= 1 << bit;
            decode(copy);
            copy[at] ^= 1 << bit;
        }
    }
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..20_000 {
        for _ in 0..1 + next() % 4 {
            let at = (next() as usize) % copy.len().max(1);
            copy[at] = next() as u8;
        }
        decode(copy);
        copy.copy_from_slice(valid);
    }
    for _ in 0..20_000 {
        for byte in copy.iter_mut().skip(crate::HEADER_SIZE) {
            *byte = next() as u8;
        }
        decode(copy);
    }
}

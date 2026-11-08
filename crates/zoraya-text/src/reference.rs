//! INV-0162-2's reference: digests of one text rendered with each system
//! font at the sizes the desktop uses. The host test and the test kernel's
//! `t_text` (under HVF and TCG) both compare against the same values, so
//! the coverage is shown to be the same on every host and in QEMU.

use alloc::vec::Vec;

use crate::{Font, FontList, Text};

/// The text: Latin, Cyrillic with the Ukrainian letters, Czech diacritics
/// and punctuation.
pub const SAMPLE: &str = "Zoraya: Ґанок, Žluťoučký kůň, ёлка {}[]";

/// Which font of `zoraya-fonts`, at which size, for each digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    InterRegular,
    InterMedium,
    MonoRegular,
    MonoBold,
}

/// The fonts and sizes, in the order of [`REFERENCE`].
pub const CASES: [(Which, u32); 6] = [
    (Which::InterRegular, 13),
    (Which::InterRegular, 26),
    (Which::MonoRegular, 13),
    (Which::MonoRegular, 26),
    (Which::MonoBold, 13),
    (Which::InterMedium, 20),
];

/// The digests, FNV-1a 64 over every glyph's size, placement and coverage.
/// Change them only with a deliberate change of the rasteriser.
pub const REFERENCE: [u64; 6] = [
    0x785f_a41a_377a_bab0,
    0xee28_ef4b_b1ab_981a,
    0xc412_3613_9e4f_9453,
    0x2d22_e867_86eb_3a0d,
    0xa6b6_ebd2_aba6_4999,
    0xe4ca_528b_cc86_aa02,
];

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The digest of `s` rendered with the font `data` at `px`, or None if the
/// font does not parse.
pub fn digest(data: &[u8], px: u32, s: &str) -> Option<u64> {
    let mut text = Text::new(FontList::new([Font::parse(data).ok()?]));
    let mut h = Vec::new();
    for c in s.chars() {
        let (source, glyph) = text.fonts.lookup(c);
        let b = text.cache.get(&text.fonts, source, glyph, px);
        h.extend_from_slice(&(b.width as u32).to_le_bytes());
        h.extend_from_slice(&(b.height as u32).to_le_bytes());
        h.extend_from_slice(&b.left.to_le_bytes());
        h.extend_from_slice(&b.top.to_le_bytes());
        h.extend_from_slice(&b.coverage);
    }
    Some(fnv(&h))
}

#![no_std]
#![no_main]

//! Text in QEMU (ADR-0162), run by the test kernel under HVF and TCG:
//!
//! - INV-0162-2: the system fonts render `zoraya_text::reference::SAMPLE`
//!   to the same digests as on the host.
//! - `plan.md`'s measurement for 7a, printed and not judged: the time to
//!   draw a full 80 × 24 grid of Cyrillic in the terminal's look, first
//!   with an empty glyph cache and then from it, and the cache's hits and
//!   misses.

extern crate alloc;

use alloc::vec;

use zoraya_term::{Cell, DEFAULT_BG, DEFAULT_FG, Look};
use zoraya_text::reference::{CASES, REFERENCE, SAMPLE, Which, digest};
use zoraya_tokens::{Appearance, Scale};
use zoraya_user::{clock_monotonic, println};

fn font(which: Which) -> &'static [u8] {
    match which {
        Which::InterRegular => zoraya_fonts::INTER_REGULAR,
        Which::InterMedium => zoraya_fonts::INTER_MEDIUM,
        Which::MonoRegular => zoraya_fonts::MONO_REGULAR,
        Which::MonoBold => zoraya_fonts::MONO_BOLD,
    }
}

/// Draws the grid once; gives the time it took in microseconds.
fn grid(look: &mut Look, pixels: &mut [u32], stride: usize, shift: usize) -> u64 {
    let start = clock_monotonic();
    for y in 0..24 {
        for x in 0..80 {
            // А to я: 64 letters, each on many cells.
            let c = char::from_u32(0x410 + ((y * 80 + x + shift) % 64) as u32).unwrap_or('?');
            let cell = Cell {
                ch: c,
                fg: DEFAULT_FG,
                bg: DEFAULT_BG,
                bold: false,
            };
            look.draw_cell(pixels, stride, x, y, cell);
        }
    }
    (clock_monotonic() - start) / 1000
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut failed = false;
    for ((which, px), want) in CASES.iter().zip(REFERENCE) {
        let got = digest(font(*which), *px, SAMPLE);
        if got != Some(want) {
            println!("t_text: {which:?} at {px} px: digest {got:x?}, expected {want:#x}");
            failed = true;
        }
    }
    let mut look = Look::new(Scale::ONE, Appearance::Light);
    let (w, h) = look.surface_size(80, 24);
    let mut pixels = vec![0u32; w * h];
    let cold = grid(&mut look, &mut pixels, w, 0);
    let warm = grid(&mut look, &mut pixels, w, 1);
    let (hits, misses) = (look.regular.cache.hits, look.regular.cache.misses);
    println!(
        "t_text: measure: an 80 x 24 grid of Cyrillic: {cold} us with an empty glyph cache, {warm} us from it; cache {hits} hits, {misses} misses"
    );
    if failed {
        return 1;
    }
    println!("t_text: all passed");
    0
}

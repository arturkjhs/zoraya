use std::vec;
use std::vec::Vec;

use super::*;

const FG: u32 = 0x00c0_c0c0;
const BG: u32 = 0x0000_0000;
const GUARD: u32 = 0xdead_beef;

fn printable() -> impl Iterator<Item = char> {
    (0x20u8..0x7f).map(char::from)
}

/// INV-0095-2.
#[test]
fn every_printable_glyph_is_distinct_and_not_the_box() {
    let all: Vec<char> = printable().collect();
    for (i, a) in all.iter().enumerate() {
        assert_ne!(glyph(*a), glyph('\u{100}'), "{a:?} looks like the box");
        for b in &all[i + 1..] {
            assert_ne!(glyph(*a), glyph(*b), "{a:?} and {b:?} look alike");
        }
    }
    assert_eq!(glyph(' '), &[0; 9]);
}

#[test]
fn only_printable_ascii_has_its_own_glyph() {
    let boxed = glyph('\u{1}');
    for c in ['\0', '\n', '\x1b', '\x7f', '\u{80}', 'é', '€', '\u{10ffff}'] {
        assert_eq!(glyph(c), boxed, "{c:?}");
    }
}

/// INV-0095-1: a cell writes only itself, and only inside the slice.
#[test]
fn drawing_stays_inside_its_cell_and_the_slice() {
    let (w, h) = (40usize, 30usize);
    let chars = ['A', 'g', '\u{1}', 'é', '\u{10ffff}', ' ', '~'];
    let places = [
        (0, 0),
        (w - CELL_WIDTH, h - CELL_HEIGHT),
        (w - 3, 4),
        (5, h - 2),
        (w, 0),
        (0, h),
        (usize::MAX - 3, usize::MAX - 3),
    ];
    for c in chars {
        for (x, y) in places {
            let mut target = vec![GUARD; w * h];
            draw(&mut target, w, x, y, c, FG, BG);
            for row in 0..h {
                for column in 0..w {
                    let inside = column >= x
                        && column < x.saturating_add(CELL_WIDTH)
                        && row >= y
                        && row < y.saturating_add(CELL_HEIGHT);
                    let p = target[row * w + column];
                    if inside {
                        assert!(p == FG || p == BG, "{c:?} at {x},{y}");
                    } else {
                        assert_eq!(p, GUARD, "{c:?} at {x},{y} wrote {column},{row}");
                    }
                }
            }
        }
    }
    // A zero stride, or a slice shorter than a row, draws nothing and lives.
    let mut tiny = vec![GUARD; 5];
    draw(&mut tiny, 0, 0, 0, 'A', FG, BG);
    draw(&mut tiny, 10, 0, 0, 'A', FG, BG);
    assert!(tiny.iter().all(|p| *p == GUARD));
}

#[test]
fn every_glyph_reads_back_as_itself() {
    let mut cell = vec![0u32; CELL_WIDTH * CELL_HEIGHT];
    for c in printable() {
        for fg in [FG, 0x00ff_ffff, 0x0012_3456] {
            draw(&mut cell, CELL_WIDTH, 0, 0, c, fg, BG);
            let got = recognise(|x, y| cell[y * CELL_WIDTH + x], BG);
            assert_eq!(got, Some(c));
        }
    }
}

#[test]
fn damaged_or_foreign_cells_are_not_read() {
    let mut cell = vec![0u32; CELL_WIDTH * CELL_HEIGHT];
    let read = |cell: &[u32]| recognise(|x, y| cell[y * CELL_WIDTH + x], BG);
    draw(&mut cell, CELL_WIDTH, 0, 0, '\u{1}', FG, BG);
    assert_eq!(read(&cell), None, "the box");
    draw(&mut cell, CELL_WIDTH, 0, 0, 'A', FG, BG);
    cell[0] = FG;
    assert_eq!(read(&cell), None, "a pixel in the margin");
    draw(&mut cell, CELL_WIDTH, 0, 0, 'A', FG, BG);
    // Half a dot: the left pixel of A's top-left dot is lit, the right is not.
    let first = (0..CELL_WIDTH * CELL_HEIGHT)
        .find(|i| cell[*i] == FG)
        .unwrap();
    cell[first + 1] = BG;
    assert_eq!(read(&cell), None, "half a dot");
    draw(&mut cell, CELL_WIDTH, 0, 0, 'A', FG, BG);
    cell[first] = 0x0011_1111;
    assert_eq!(read(&cell), None, "two text colours");
}

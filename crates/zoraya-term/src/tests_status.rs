//! The status row (ADR-0337, INV-0337-5): its text is the shell's alone, and
//! it is drawn as plain cells inside its row.

use std::string::String;
use std::vec;
use std::vec::Vec;

use super::*;
use zoraya_proto_term::{STATUS_FIELD, STATUS_NOTICE, STATUS_PLAIN};
use zoraya_tokens::{Appearance, Scale};

fn text_of(row: &StatusRow, columns: usize) -> String {
    row.chars(columns)
        .into_iter()
        .collect::<String>()
        .trim_end()
        .into()
}

#[test]
fn a_notice_has_its_words_and_the_keys_at_the_right_end() {
    let mut r = StatusRow::default();
    assert!(r.is_empty());
    r.set(STATUS_NOTICE, b"vi was not given ~/other.txt (to read)");
    let row = r.chars(80);
    assert_eq!(row.len(), 80);
    let s: String = row.iter().collect();
    assert!(s.starts_with("vi was not given ~/other.txt (to read)"));
    assert!(s.ends_with("System+G: give"));
    // Too narrow for both: the words win, cut at the row, with no hint.
    assert_eq!(text_of(&r, 20), "vi was not given ~/o");
}

#[test]
fn plain_text_and_a_field_have_no_hint_and_a_field_has_a_cursor() {
    let mut r = StatusRow::default();
    r.set(STATUS_PLAIN, b"given until vi ends: +r ~/other.txt");
    assert_eq!(text_of(&r, 80), "given until vi ends: +r ~/other.txt");
    assert_eq!(r.cursor(80), None);
    r.set(STATUS_FIELD, b"give vi: +r ~/o");
    assert_eq!(text_of(&r, 80), "give vi: +r ~/o");
    assert_eq!(r.cursor(80), Some(15));
    // A long field keeps its end, where the cursor is, in view.
    r.set(STATUS_FIELD, "x".repeat(100).as_bytes());
    assert_eq!(r.chars(40).len(), 40);
    assert_eq!(r.cursor(40), Some(39));
    r.set(STATUS_PLAIN, b"");
    assert!(r.is_empty());
}

#[test]
fn the_text_is_plain_cells_whatever_the_shell_sent() {
    // A control character is a space, ESC included; bidi and zero-width
    // characters are gone; a wide character is a `?`; bad UTF-8 is the
    // replacement character, one column.
    let mut r = StatusRow::default();
    r.set(
        STATUS_PLAIN,
        "a\x1b[2Jb\x07c\nd\u{202e}e\u{200b}f\u{5e9}\u{4e2d}\u{fffd}".as_bytes(),
    );
    assert_eq!(r.text(), "a [2Jb c def\u{5e9}?\u{fffd}");
    assert!(r.text().chars().all(|c| !c.is_control()));
}

#[test]
fn the_output_of_a_program_cannot_reach_the_row() {
    // INV-0337-5: the grid is another object; nothing the parser reads
    // touches the row's cells (the fuzz of INV-0096-1, with the row's text
    // compared before and after).
    let mut r = StatusRow::default();
    r.set(STATUS_NOTICE, b"vi was not given ~/o (to read)");
    let before = r.clone();
    let mut t = Term::new(20, 5);
    let mut seed = 0x1234_5678_9abc_def0u64;
    let pieces: [&[u8]; 8] = [
        b"\x1b[24;1H",
        b"\x1b]0;System+G\x07",
        b"\x1b[2J\x1b[?1049h",
        b"\x1b[6n",
        b"\x1bc",
        b"\x1b[999;999H",
        b"vi was not given /etc (to read)",
        b"\x1b[1;1H\x1b[K",
    ];
    for _ in 0..5000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut chunk: Vec<u8> = Vec::new();
        for _ in 0..(seed >> 58) {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            if seed >> 62 == 0 {
                chunk.extend_from_slice(pieces[(seed >> 40) as usize % pieces.len()]);
            } else {
                chunk.push((seed >> 33) as u8);
            }
        }
        t.write(&chunk);
    }
    assert_eq!(r, before);
}

#[test]
fn drawing_the_row_touches_only_its_own_row() {
    let mut look = Look::new(Scale::ONE, Appearance::Light);
    let (columns, rows) = (20, 3);
    let (w, h) = look.surface_size(columns, rows + 1);
    let mut pixels = vec![0xdead_beefu32; w * h];
    let mut r = StatusRow::default();
    r.set(STATUS_NOTICE, b"vi was not given ~/o (to read)");
    assert!(r.render(&mut look, &mut pixels, w, columns, rows));
    assert!(
        !r.render(&mut look, &mut pixels, w, columns, rows),
        "drawn once"
    );
    let top = look.pad + rows * look.cell_height();
    let bottom = top + look.cell_height();
    for y in 0..h {
        for x in 0..w {
            let touched = pixels[y * w + x] != 0xdead_beef;
            let inside = (top..bottom).contains(&y)
                && (look.pad..look.pad + columns * look.cell_width()).contains(&x);
            assert!(!touched || inside, "({x}, {y}) is outside the row");
        }
    }
    // A change draws again, a repeat does not.
    r.set(STATUS_NOTICE, b"vi was not given ~/o (to read)");
    assert!(!r.render(&mut look, &mut pixels, w, columns, rows));
    r.set(STATUS_PLAIN, b"");
    assert!(r.render(&mut look, &mut pixels, w, columns, rows));
}

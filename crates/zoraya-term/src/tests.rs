use std::format;
use std::string::String;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::read::Reader;
use zoraya_tokens::{Appearance, Scale};

fn look() -> Look {
    Look::new(Scale::ONE, Appearance::Light)
}

/// The rows of a surface of `columns` cells as the reader reads them.
fn read_rows(pixels: &[u32], w: usize, h: usize, columns: usize, rows: usize) -> Vec<String> {
    let mut reader = Reader::new(look());
    reader.rows(columns, rows, (0, 0), (w, h), |x, y| pixels[y * w + x])
}

fn term(text: &[u8]) -> Term {
    let mut t = Term::new(20, 5);
    t.write(text);
    t
}

fn screen(t: &Term) -> Vec<String> {
    (0..t.size().1).map(|y| t.line(y)).collect()
}

#[test]
fn text_newlines_and_the_shell_s_erase() {
    let t = term(b"zosh$ lx\x08 \x08s\nREADME\n");
    assert_eq!(screen(&t), ["zosh$ ls", "README", "", "", ""]);
    assert_eq!(t.cursor(), (0, 2));
}

#[test]
fn carriage_return_tab_and_bell() {
    let t = term(b"abc\rX\tY\x07");
    assert_eq!(t.line(0), "Xbc     Y");
    assert_eq!(t.cursor(), (9, 0));
}

#[test]
fn long_lines_wrap_and_the_bottom_scrolls() {
    let mut t = term(b"");
    let line = [b'x'; 25];
    t.write(&line);
    assert_eq!(t.line(0), "x".repeat(20));
    assert_eq!(t.line(1), "x".repeat(5));
    for i in 0..10 {
        t.write(format!("line {i}\n").as_bytes());
    }
    assert_eq!(t.line(3), "line 9");
    assert_eq!(t.cursor(), (0, 4));
    assert_eq!(t.history_len(), 7);
}

#[test]
fn a_full_last_column_wraps_only_when_more_comes() {
    let mut t = term(b"");
    t.write(&[b'a'; 20]);
    assert_eq!(t.cursor(), (19, 0));
    t.write(b"\n");
    assert_eq!(t.cursor(), (0, 1), "no blank line from the pending wrap");
}

#[test]
fn cursor_movement_and_erasing() {
    let t = term(b"hello\x1b[2Dxy\x1b[3;4HZ\x1b[1;1H\x1b[K");
    assert_eq!(t.line(0), "");
    assert_eq!(t.line(2), "   Z");
    let t = term(b"abcdef\x1b[1;3H\x1b[1K");
    assert_eq!(t.line(0), "   def");
    let t = term(b"abc\nxyz\x1b[2J");
    assert!(screen(&t).iter().all(String::is_empty));
    let t = term(b"abc\nxyz\x1b[1;2H\x1b[J");
    assert_eq!(screen(&t), ["a", "", "", "", ""]);
    // Moves are clamped to the grid.
    let t = term(b"\x1b[99B\x1b[99C\x1b[99;99H");
    assert_eq!(t.cursor(), (19, 4));
    let t = term(b"\x1b[99A\x1b[99D");
    assert_eq!(t.cursor(), (0, 0));
}

/// ADR-0154's addition to the subset: SGR 2 is faint, 22 ends bold and faint.
#[test]
fn faint_and_normal_intensity() {
    let t = term(b"a\x1b[2mb\x1b[22mc\x1b[1;2md\x1b[22me\x1b[31;2mf\x1b[0mg");
    let fg = |x| t.cell(x, 0).fg;
    assert_eq!(fg(0), DEFAULT_FG);
    assert_eq!(fg(1), FAINT_DEFAULT);
    assert_eq!(fg(2), DEFAULT_FG);
    assert_eq!(fg(3), FAINT_DEFAULT, "faint wins over bold");
    assert!(!t.cell(3, 0).bold);
    assert_eq!(fg(4), DEFAULT_FG);
    assert_eq!(fg(5), FAINT + 1);
    assert_eq!(fg(6), DEFAULT_FG);
    let p = palette(Appearance::Light);
    let tokens = Appearance::Light.palette();
    assert_eq!(
        p[usize::from(FAINT_DEFAULT)],
        tokens.term_fg.blend(tokens.term_bg, 128)
    );
    assert_eq!(
        p[usize::from(FAINT + 1)],
        tokens.term_red.blend(tokens.term_bg, 128)
    );
    assert_eq!(
        p[usize::from(BRIGHT + 1)],
        tokens.term_red.blend(tokens.term_fg, 64)
    );
    assert_eq!(p[usize::from(DEFAULT_BG)], tokens.term_bg);
    assert_eq!(
        palette(Appearance::Dark)[usize::from(DEFAULT_FG)],
        Appearance::Dark.palette().term_fg
    );
}

#[test]
fn colours_and_reverse() {
    let t = term(b"\x1b[31;44mA\x1b[1mB\x1b[0;7mC\x1b[mD");
    assert_eq!(
        t.cell(0, 0),
        Cell {
            ch: 'A',
            fg: 1,
            bg: 4,
            bold: false
        }
    );
    assert_eq!(
        t.cell(1, 0),
        Cell {
            ch: 'B',
            fg: 9,
            bg: 4,
            bold: true
        }
    );
    assert_eq!(
        t.cell(2, 0),
        Cell {
            ch: 'C',
            fg: DEFAULT_BG,
            bg: DEFAULT_FG,
            bold: false
        }
    );
    assert_eq!(
        t.cell(3, 0),
        Cell {
            ch: 'D',
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            bold: false
        }
    );
}

#[test]
fn utf8_and_bad_bytes() {
    let t = term("é€x".as_bytes());
    assert_eq!(t.line(0), "é€x");
    let t = term(b"\xc3x\xff\xe2\x82");
    assert_eq!(t.line(0), "\u{fffd}x\u{fffd}");
}

#[test]
fn strings_and_unknown_sequences_are_skipped() {
    let t = term(b"a\x1b]0;title\x07b\x1bP1$q\x1b\\c\x1b[?25ld\x1b(Be\x1b[5ns");
    assert_eq!(t.line(0), "abcdes");
}

/// INV-0096-2: no output produces input. The parser has no path to the
/// shell; these sequences, which a VT answers, change only the screen.
#[test]
fn queries_get_no_answer() {
    let queries: [&[u8]; 6] = [
        b"\x1b[6n",
        b"\x1b[c",
        b"\x1b[>c",
        b"\x1b]10;?\x07",
        b"\x1bP$qm\x1b\\",
        b"\x1b[18t",
    ];
    for q in queries {
        let mut t = Term::new(20, 5);
        t.write(q);
        assert_eq!(screen(&t), ["", "", "", "", ""]);
        assert_eq!(t.cursor(), (0, 0));
    }
}

/// INV-0096-3: memory is fixed whatever the output.
#[test]
fn memory_is_bounded() {
    let mut t = Term::new(80, 24);
    let before = t.memory();
    for i in 0..5000 {
        t.write(format!("line {i}\n").as_bytes());
    }
    assert_eq!(t.history_len(), SCROLLBACK);
    let mut osc = vec![b'x'; 64 * 1024];
    osc[0] = 0x1b;
    osc[1] = b']';
    t.write(&osc);
    t.write(b"after");
    let shown: String = (0..24).map(|y| t.line(y)).collect();
    assert!(
        shown.ends_with("xxxafter"),
        "the string was cut and the rest printed"
    );
    assert_eq!(t.memory(), before);
    // An escape inside a string still counts (review R2), and so does a
    // control sequence that never ends.
    for opening in [&b"\x1b]"[..], b"\x1b["] {
        let mut t = Term::new(80, 24);
        t.write(opening);
        for _ in 0..100_000 {
            t.write(if opening[1] == b']' { b"\x1bA" } else { b"1;" });
        }
        t.write(b"\n\nvisible");
        let shown: String = (0..24).map(|y| t.line(y)).collect();
        assert!(shown.contains("visible"), "{opening:?}");
    }
}

/// INV-0096-1: random and structured bytes never panic and keep the cursor
/// inside the grid.
#[test]
fn the_parser_is_total() {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let pieces: [&[u8]; 12] = [
        b"\x1b[",
        b"\x1b]",
        b"\x1bP",
        b";",
        b"9999999999",
        b"\x1b\\",
        b"\x07",
        b"\n",
        b"\x08",
        b"\xe2\x82",
        b"m",
        b"H",
    ];
    let mut look = look();
    for size in [(1, 1), (2, 1), (80, 24), (3, 50)] {
        let mut t = Term::new(size.0, size.1);
        let mut pixels = vec![
            0u32;
            {
                let (w, h) = look.surface_size(size.0, size.1);
                w * h
            }
        ];
        for _ in 0..2000 {
            let mut chunk = Vec::new();
            for _ in 0..next() % 64 {
                if next() % 3 == 0 {
                    chunk.extend_from_slice(pieces[(next() % 12) as usize]);
                } else {
                    chunk.push(next() as u8);
                }
            }
            t.write(&chunk);
            let (cx, cy) = t.cursor();
            assert!(cx < size.0 && cy < size.1);
            if next() % 7 == 0 {
                t.scroll_view((next() % 21) as isize - 10);
            }
            let stride = look.surface_size(size.0, size.1).0;
            let _ = t.render(&mut look, &mut pixels, stride, true);
        }
    }
}

#[test]
fn the_view_scrolls_back_and_output_returns_it() {
    let mut t = Term::new(10, 3);
    for i in 0..6 {
        t.write(format!("{i}\n").as_bytes());
    }
    assert_eq!(t.history_len(), 4);
    t.scroll_view(2);
    assert_eq!(t.view(), 2);
    let mut look = look();
    let (w, h) = look.surface_size(10, 3);
    let mut pixels = vec![0u32; w * h];
    t.render(&mut look, &mut pixels, w, true);
    assert_eq!(read_rows(&pixels, w, h, 10, 3), ["2", "3", "4"]);
    t.scroll_view(100);
    assert_eq!(t.view(), 4);
    t.scroll_view(-100);
    assert_eq!(t.view(), 0);
    t.scroll_view(3);
    t.write(b"x");
    assert_eq!(t.view(), 0);
}

#[test]
fn rendering_draws_changed_rows_and_the_cursor() {
    let mut t = Term::new(10, 3);
    let mut look = look();
    let (w, h) = look.surface_size(10, 3);
    let mut pixels = vec![0x00ab_cdefu32; w * h];
    assert_eq!(t.render(&mut look, &mut pixels, w, true), Some((0, 2)));
    assert_eq!(t.render(&mut look, &mut pixels, w, true), None);
    t.write(b"hi");
    assert_eq!(t.render(&mut look, &mut pixels, w, true), Some((0, 0)));
    // The cursor, after "hi", is a reversed blank: all text colour.
    assert_eq!(read_rows(&pixels, w, h, 10, 1), ["hi_"]);
    let fg = look.colors[usize::from(DEFAULT_FG)].pixel();
    let (cx, cy) = look.cell_origin(2, 0);
    assert!(
        (0..look.cell_height())
            .all(|dy| (0..look.cell_width()).all(|dx| pixels[(cy + dy) * w + cx + dx] == fg))
    );
    // The padding is never drawn.
    assert_eq!(pixels[0], 0x00ab_cdef);
    // Backspace redraws the row so the cursor moves.
    t.write(b"\x08");
    assert_eq!(t.render(&mut look, &mut pixels, w, true), Some((0, 0)));
}

#[test]
fn ukrainian_czech_colours_and_bold_read_back() {
    let mut t = Term::new(30, 3);
    let mut look = look();
    let (w, h) = look.surface_size(30, 3);
    let mut pixels = vec![0u32; w * h];
    look.clear(&mut pixels);
    t.write("звіт-2026.txt příklad.txt\n\x1b[31mґанок\x1b[m \x1b[1mЖ\x1b[22m \x1b[2mfaint\x1b[m\n\x1b[7mrev".as_bytes());
    t.render(&mut look, &mut pixels, w, false);
    let mut reader = Reader::new(Look::new(Scale::ONE, Appearance::Light));
    let rows = reader.rows(30, 3, (0, 0), (w, h), |x, y| pixels[y * w + x]);
    // Look-alikes read as the Latin letter: compare in the reader's classes.
    assert_eq!(rows[0], reader.canonical("звіт-2026.txt příklad.txt"));
    assert_eq!(rows[0], "звiт-2026.txt příklad.txt");
    assert_eq!(rows[1], reader.canonical("ґанок Ж faint"));
    assert_eq!(rows[2], "rev");
}

#[test]
fn the_dark_set_and_scale_two_read_back_too() {
    let mut t = Term::new(12, 2);
    let mut look = Look::new(Scale::TWO, Appearance::Dark);
    let (w, h) = look.surface_size(12, 2);
    assert_eq!(look.cell_width(), 16);
    let mut pixels = vec![0u32; w * h];
    t.write("Ґудзик ěščř".as_bytes());
    t.render(&mut look, &mut pixels, w, true);
    let mut reader = Reader::new(Look::new(Scale::TWO, Appearance::Dark));
    let rows = reader.rows(12, 2, (0, 0), (w, h), |x, y| pixels[y * w + x]);
    assert_eq!(rows[0], reader.canonical("Ґудзик ěščř_"));
}

#[test]
fn keys_become_bytes() {
    let mut out = [0u8; 8];
    let mut bytes = |key, shift, text| {
        let n = key_bytes(key, shift, text, &mut out);
        out[..n].to_vec()
    };
    assert_eq!(bytes(30, false, Some('a')), b"a");
    assert_eq!(bytes(46, false, Some('\u{3}')), b"\x03");
    assert_eq!(bytes(code::ENTER, false, None), b"\r");
    assert_eq!(bytes(code::BACKSPACE, false, None), b"\x7f");
    assert_eq!(bytes(code::UP, false, None), b"\x1b[A");
    assert_eq!(bytes(code::PAGE_UP, false, None), b"\x1b[5~");
    assert_eq!(bytes(code::PAGE_UP, true, None), b"");
    assert_eq!(bytes(code::F12, false, None), b"");
    assert_eq!(bytes(0x200, false, Some('€')), "€".as_bytes());
}

#[test]
fn the_grid_fits_the_display() {
    let look = look();
    assert_eq!(look.grid_for(1024, 768), (80, 24));
    let (c, r) = look.grid_for(640, 480);
    let (w, h) = look.surface_size(c, r);
    assert!(w + 32 <= 640 && h + 32 <= 480);
    assert_eq!(look.grid_for(0, 0), (1, 1));
}

// Tests for the holes cargo-mutants found (TASK-14 review).

#[test]
fn sizes_are_pinned_to_the_padding() {
    // JetBrains Mono at 13 px: 8 × 18 cells; padding.terminal is 14.
    let look = look();
    assert_eq!(look.pad, 14);
    assert_eq!(look.grid_for(166, 146), (12, 4));
    assert_eq!(look.grid_for(640, 480), (71, 22));
    assert_eq!(look.surface_size(1, 1), (8 + 28, 18 + 28));
    assert_eq!(look.cell_origin(2, 3), (14 + 16, 14 + 54));
    let two = Look::new(Scale::TWO, Appearance::Light);
    assert_eq!((two.pad, two.cell_width(), two.cell_height()), (28, 16, 35));
}

#[test]
fn cells_are_addressed_by_row() {
    let t = term(b"a\nb");
    assert_eq!(t.cell(0, 1).ch, 'b');
}

#[test]
fn memory_is_what_the_terminal_holds() {
    let t = Term::new(80, 24);
    let expected =
        (t.screen.capacity() + t.history.capacity()) * size_of::<Cell>() + t.dirty.capacity();
    assert_eq!(t.memory(), expected);
}

#[test]
fn scrolling_and_cursor_moves_redraw_their_rows() {
    let mut t = Term::new(10, 3);
    let mut look = look();
    let (w, h) = look.surface_size(10, 3);
    let mut pixels = vec![0u32; w * h];
    t.write(b"a\nb\nc");
    t.render(&mut look, &mut pixels, w, true);
    t.write(b"\n");
    assert_eq!(t.render(&mut look, &mut pixels, w, true), Some((0, 2)));
    t.write(b"\x1b[2;1H");
    assert_eq!(t.render(&mut look, &mut pixels, w, true), Some((0, 2)));
    assert_eq!(t.render(&mut look, &mut pixels, w, true), None);
    t.redraw();
    assert_eq!(t.render(&mut look, &mut pixels, w, true), Some((0, 2)));
    // A row below the first is drawn from its own row.
    let mut t = Term::new(10, 3);
    t.write(b"h\nZ");
    look.clear(&mut pixels);
    t.render(&mut look, &mut pixels, w, false);
    assert_eq!(read_rows(&pixels, w, h, 10, 2), ["h", "Z"]);
}

/// The terminal in both sets, for TASK-24's report: a PPM per set into the
/// directory `ZORAYA_TERM_SAMPLES` names (the dark set cannot be shown in
/// QEMU until 7f's settings).
#[test]
#[ignore = "writes screens: ZORAYA_TERM_SAMPLES=dir cargo test -p zoraya-term -- --ignored samples"]
fn samples() {
    let Ok(dir) = std::env::var("ZORAYA_TERM_SAMPLES") else {
        return;
    };
    let session = concat!(
        "Type help to begin, or help tour.\n",
        "~> ls /texts/uk\n",
        "звіт-2026.txt\n",
        "~> cat /texts/uk/звіт-2026.txt\n",
        "Привіт, світе! Ґанок, їжак, єнот.\n",
        "~> cat /texts/cz/příklad.txt\n",
        "Příliš žluťoučký kůň úpěl ďábelské ódy.\n",
        "~> cat нема.txt\n",
        "\x1b[31mcat: нема.txt: not found\x1b[m\n",
        "~> \x1b[1mbold\x1b[22m and \x1b[2mfaint\x1b[22m, \x1b[32mgreen\x1b[m, \x1b[34mblue\x1b[m, \x1b[35mmagenta\x1b[m\n",
        "~> echo \x1b[2mhi > a.txt\x1b[22m",
    );
    for (name, appearance) in [
        ("terminal-light", Appearance::Light),
        ("terminal-dark", Appearance::Dark),
    ] {
        let mut look = Look::new(Scale::ONE, appearance);
        let mut t = Term::new(80, 24);
        t.write(session.as_bytes());
        let (w, h) = look.surface_size(80, 24);
        let mut pixels = vec![0u32; w * h];
        look.clear(&mut pixels);
        t.render(&mut look, &mut pixels, w, true);
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        for p in &pixels {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
        }
        std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
    }
}

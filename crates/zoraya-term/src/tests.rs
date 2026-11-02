use std::format;
use std::string::String;
use std::vec;
use std::vec::Vec;

use super::*;

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

#[test]
fn colours_and_reverse() {
    let t = term(b"\x1b[31;44mA\x1b[1mB\x1b[0;7mC\x1b[mD");
    assert_eq!(
        t.cell(0, 0),
        Cell {
            ch: 'A',
            fg: 1,
            bg: 4
        }
    );
    assert_eq!(
        t.cell(1, 0),
        Cell {
            ch: 'B',
            fg: 9,
            bg: 4
        }
    );
    assert_eq!(
        t.cell(2, 0),
        Cell {
            ch: 'C',
            fg: DEFAULT_BG,
            bg: DEFAULT_FG
        }
    );
    assert_eq!(
        t.cell(3, 0),
        Cell {
            ch: 'D',
            fg: DEFAULT_FG,
            bg: DEFAULT_BG
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
    for size in [(1, 1), (2, 1), (80, 24), (3, 50)] {
        let mut t = Term::new(size.0, size.1);
        let mut pixels = vec![
            0u32;
            {
                let (w, h) = surface_size(size.0, size.1);
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
            let stride = surface_size(size.0, size.1).0;
            let _ = t.render(&mut pixels, stride, true);
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
    let (w, h) = surface_size(10, 3);
    let mut pixels = vec![0u32; w * h];
    t.render(&mut pixels, w, true);
    let row = |p: &[u32], y: usize| -> String {
        (0..10)
            .map(|x| {
                zoraya_font::recognise(
                    |dx, dy| p[(PAD + y * CELL_HEIGHT + dy) * w + PAD + x * CELL_WIDTH + dx],
                    PALETTE[0],
                )
                .unwrap_or('?')
            })
            .collect::<String>()
            .trim_end()
            .into()
    };
    assert_eq!(
        [row(&pixels, 0), row(&pixels, 1), row(&pixels, 2)],
        ["2", "3", "4"]
    );
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
    let (w, h) = surface_size(10, 3);
    let mut pixels = vec![0x00ab_cdefu32; w * h];
    assert_eq!(t.render(&mut pixels, w, true), Some((0, 2)));
    assert_eq!(t.render(&mut pixels, w, true), None);
    t.write(b"hi");
    assert_eq!(t.render(&mut pixels, w, true), Some((0, 0)));
    let at = |x: usize, y: usize| pixels[y * w + x];
    // 'h' in the text colour on black.
    let read = zoraya_font::recognise(|dx, dy| at(PAD + dx, PAD + dy), PALETTE[0]);
    assert_eq!(read, Some('h'));
    // The cursor, after "hi", is a reversed blank: all text colour.
    let cx = PAD + 2 * CELL_WIDTH;
    assert!(
        (0..CELL_HEIGHT).all(|dy| (0..CELL_WIDTH).all(|dx| at(cx + dx, PAD + dy) == PALETTE[7]))
    );
    // The padding is never drawn.
    assert_eq!(at(0, 0), 0x00ab_cdef);
    // Backspace redraws the row so the cursor moves.
    t.write(b"\x08");
    assert_eq!(t.render(&mut pixels, w, true), Some((0, 0)));
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
    assert_eq!(grid_for(1024, 768), (80, 24));
    let (c, r) = grid_for(640, 480);
    let (w, h) = surface_size(c, r);
    assert!(w + 32 <= 640 && h + 32 <= 480);
    assert_eq!(grid_for(0, 0), (1, 1));
}

// Tests for the holes cargo-mutants found (TASK-14 review).

#[test]
fn sizes_are_pinned_to_the_padding() {
    assert_eq!(grid_for(166, 146), (9, 4));
    assert_eq!(grid_for(640, 480), (49, 21));
    assert_eq!(
        surface_size(1, 1),
        (CELL_WIDTH + 2 * PAD, CELL_HEIGHT + 2 * PAD)
    );
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
    let (w, h) = surface_size(10, 3);
    let mut pixels = vec![0u32; w * h];
    t.write(b"a\nb\nc");
    t.render(&mut pixels, w, true);
    t.write(b"\n");
    assert_eq!(t.render(&mut pixels, w, true), Some((0, 2)));
    t.write(b"\x1b[2;1H");
    assert_eq!(t.render(&mut pixels, w, true), Some((0, 2)));
    assert_eq!(t.render(&mut pixels, w, true), None);
    t.redraw();
    assert_eq!(t.render(&mut pixels, w, true), Some((0, 2)));
    // A row below the first is drawn from its own row.
    let mut t = Term::new(10, 3);
    t.write(b"h\nZ");
    t.render(&mut pixels, w, false);
    let read = zoraya_font::recognise(
        |dx, dy| pixels[(PAD + CELL_HEIGHT + dy) * w + PAD + dx],
        PALETTE[0],
    );
    assert_eq!(read, Some('Z'));
}

#[test]
fn four_byte_utf8_reset_and_intermediates() {
    assert_eq!(term("\u{1F600}".as_bytes()).line(0), "\u{1F600}");
    let t = term(b"abc\x1bc");
    assert_eq!((t.line(0), t.cursor()), (String::new(), (0, 0)));
    assert_eq!(term(b"a\x1b[5 qb").line(0), "ab");
    assert_eq!(term(b"ab\x1b[1\ncd").line(1), "d");
    assert_eq!(term(b"ab\x1b[9\x1b[Hc").line(0), "cb");
}

#[test]
fn parameters_and_relative_moves() {
    assert_eq!(term(b"\x1b[1;2;3H").cursor(), (1, 0));
    let t = term(b"\x1b[0;0;0;0;0;0;0;0;31mA");
    assert_eq!(t.cell(0, 0).fg, DEFAULT_FG);
    assert_eq!(term(b"\n\n\x1b[A").cursor(), (0, 1));
    assert_eq!(term(b"\x1b[B").cursor(), (0, 1));
    assert_eq!(term(b"\x1b[C").cursor(), (1, 0));
    assert_eq!(term(b"ab\x1b[D").cursor(), (1, 0));
}

#[test]
fn every_graphics_rendition_and_erase_mode() {
    assert_eq!(term(b"\x1b[1mA\x1b[22mB").cell(1, 0).fg, 7);
    assert_eq!(term(b"\x1b[7mA\x1b[27mB").cell(1, 0).fg, 7);
    assert_eq!(term(b"\x1b[31mA\x1b[39mB").cell(1, 0).fg, 7);
    assert_eq!(term(b"\x1b[44mA\x1b[49mB").cell(1, 0).bg, 0);
    assert_eq!(term(b"\x1b[32mA").cell(0, 0).fg, 2);
    assert_eq!(term(b"abc\x1b[2K").line(0), "");
    assert_eq!(term(b"abc\nxyz\x1b[2K").line(0), "abc");
    let t = term(b"abc\nxyz\x1b[2;2H\x1b[J");
    assert_eq!(screen(&t), ["abc", "x", "", "", ""]);
    assert_eq!(term(b"abcdef\x1b[1;3H\x1b[1J").line(0), "   def");
}

#[test]
fn every_named_key_has_its_sequence() {
    let mut out = [0u8; 8];
    let cases: [(u32, &[u8]); 10] = [
        (code::TAB, b"\t"),
        (code::ESC, b"\x1b"),
        (code::DOWN, b"\x1b[B"),
        (code::RIGHT, b"\x1b[C"),
        (code::LEFT, b"\x1b[D"),
        (code::HOME, b"\x1b[H"),
        (code::END, b"\x1b[F"),
        (code::INSERT, b"\x1b[2~"),
        (code::DELETE, b"\x1b[3~"),
        (code::PAGE_DOWN, b"\x1b[6~"),
    ];
    for (key, bytes) in cases {
        let n = key_bytes(key, false, None, &mut out);
        assert_eq!(&out[..n], bytes, "{key}");
    }
}

/// Bytes outside ASCII (ADR-0013 allows UTF-8 names): valid characters are
/// kept, and everything malformed is one U+FFFD per bad sequence, with no
/// control acted on and the text after it intact. The font draws every
/// non-ASCII character as its box.
#[test]
fn bytes_outside_ascii_are_shown_safely() {
    let cases: [(&[u8], &str); 12] = [
        ("naïve.txt".as_bytes(), "naïve.txt"),
        ("файл 文件 \u{1F600}".as_bytes(), "файл 文件 \u{1F600}"),
        // An overlong ESC followed by what would clear the screen.
        (b"a\xe0\x80\x9b[2Jb", "a\u{fffd}[2Jb"),
        (b"a\xc0\xafb", "a\u{fffd}\u{fffd}b"),
        (b"a\xf0\x80\x80\xafb", "a\u{fffd}b"),
        // C1 controls, CSI among them.
        (b"a\xc2\x9b2Jb", "a\u{fffd}2Jb"),
        (b"a\xc2\x85b", "a\u{fffd}b"),
        // A surrogate, and a value past U+10FFFF.
        (b"a\xed\xa0\x80b", "a\u{fffd}b"),
        (b"a\xf4\x90\x80\x80b", "a\u{fffd}b"),
        // Stray continuations, a cut-off sequence, impossible bytes.
        (b"a\x80\xbfb", "a\u{fffd}\u{fffd}b"),
        (b"a\xe2\x82b", "a\u{fffd}b"),
        (b"a\xf8\xffb", "a\u{fffd}\u{fffd}b"),
    ];
    for (bytes, shown) in cases {
        let t = term(bytes);
        assert_eq!(t.line(0), shown, "{bytes:x?}");
        assert_eq!(t.line(1), "", "{bytes:x?}: no line was cleared or moved");
        assert!((0..20).all(|x| !t.cell(x, 0).ch.is_control()), "{bytes:x?}");
    }
    // And a sequence cut by the end of one write and finished by the next
    // is the character.
    let mut t = Term::new(20, 5);
    t.write(b"\xc3");
    t.write(b"\xa9");
    assert_eq!(t.line(0), "é");
}

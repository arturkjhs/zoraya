//! What the parser and the grid do beyond the entry's own strings (ADR-0332
//! items 2, 3, 6, 9 and 11, INV-0331-4, INV-0332-2).

use std::string::String;
use std::vec::Vec;

use crate::cell::{self, Colour};
use crate::{Cell, MAX_SCROLLBACK, Term};

fn term(columns: usize, rows: usize) -> Term {
    Term::new(columns, rows)
}

fn run(columns: usize, rows: usize, text: &[u8]) -> Term {
    let mut t = term(columns, rows);
    t.write(text);
    t
}

fn rows(t: &Term) -> Vec<String> {
    (0..t.size().1).map(|y| t.line(y)).collect()
}

#[test]
fn extended_colours_are_one_unit_in_both_forms() {
    let fg = |t: &Term| t.cell(0, 0).fg();
    let bg = |t: &Term| t.cell(0, 0).bg();
    assert_eq!(fg(&run(5, 1, b"\x1b[38;5;200mx")), Colour::Index(200));
    assert_eq!(fg(&run(5, 1, b"\x1b[38:5:200mx")), Colour::Index(200));
    assert_eq!(bg(&run(5, 1, b"\x1b[48;5;17mx")), Colour::Index(17));
    assert_eq!(bg(&run(5, 1, b"\x1b[48:5:17mx")), Colour::Index(17));
    assert_eq!(
        fg(&run(5, 1, b"\x1b[38;2;10;20;30mx")),
        Colour::Rgb(10, 20, 30)
    );
    assert_eq!(
        fg(&run(5, 1, b"\x1b[38:2::10:20:30mx")),
        Colour::Rgb(10, 20, 30)
    );
    assert_eq!(
        fg(&run(5, 1, b"\x1b[38:2:10:20:30mx")),
        Colour::Rgb(10, 20, 30)
    );
    assert_eq!(
        fg(&run(5, 1, b"\x1b[38:2:7:10:20:30mx")),
        Colour::Rgb(10, 20, 30)
    );
    assert_eq!(bg(&run(5, 1, b"\x1b[48;2;1;2;3mx")), Colour::Rgb(1, 2, 3));
    // What follows an extended colour is read as itself, not as part of it:
    // the old parser took the 5 of `38;5;N` for blink and N for a colour.
    let t = run(5, 1, b"\x1b[38;5;1;1;4mx");
    assert_eq!(t.cell(0, 0).fg(), Colour::Index(1));
    assert_eq!(t.cell(0, 0).attrs(), cell::BOLD | cell::UNDERLINE);
    let t = run(5, 1, b"\x1b[1;38:5:9;3;48;2;4;5;6mx");
    assert_eq!(t.cell(0, 0).fg(), Colour::Index(9));
    assert_eq!(t.cell(0, 0).bg(), Colour::Rgb(4, 5, 6));
    assert_eq!(t.cell(0, 0).attrs(), cell::BOLD | cell::ITALIC);
    // The colour just before a literal 5 or 2 that is not an introducer.
    let t = run(5, 1, b"\x1b[31;5mx");
    assert_eq!(
        (t.cell(0, 0).fg(), t.cell(0, 0).attrs()),
        (Colour::Index(1), cell::BLINK)
    );
}

#[test]
fn a_malformed_extended_colour_changes_no_colour_and_eats_its_parameters() {
    for bad in [
        &b"\x1b[38;5mx"[..],
        b"\x1b[38;5;256mx",
        b"\x1b[38;2;1;2mx",
        b"\x1b[38;2;1;2;300mx",
        b"\x1b[38;9;1;2;3mx",
        b"\x1b[38mx",
        b"\x1b[38:5:999mx",
        b"\x1b[38:2:1:2mx",
    ] {
        let t = run(5, 1, bad);
        assert_eq!(t.cell(0, 0).fg(), Colour::Default, "{bad:?}");
        assert_eq!(
            t.cell(0, 0).attrs(),
            0,
            "{bad:?}: nothing leaks into attributes"
        );
    }
    // The underline colour (58) is consumed and not drawn.
    let t = run(5, 1, b"\x1b[58;2;1;2;3;1mx\x1b[58:5:7;3my");
    assert_eq!(
        (t.cell(0, 0).attrs(), t.cell(0, 0).fg()),
        (cell::BOLD, Colour::Default)
    );
    assert_eq!(
        t.cell(1, 0).attrs(),
        cell::BOLD | cell::ITALIC,
        "bold from before stays"
    );
}

#[test]
fn every_sgr_code_of_the_adr() {
    let on = |n: u16| -> Cell {
        let mut t = term(4, 1);
        t.write(std::format!("\x1b[{n}mx").as_bytes());
        t.cell(0, 0)
    };
    assert!(on(1).has(cell::BOLD));
    assert!(on(2).has(cell::FAINT));
    assert!(on(3).has(cell::ITALIC));
    assert!(on(4).has(cell::UNDERLINE));
    assert!(on(5).has(cell::BLINK) && on(6).has(cell::BLINK));
    assert!(on(7).has(cell::REVERSE));
    assert!(on(8).has(cell::INVISIBLE));
    assert!(on(9).has(cell::STRIKE));
    assert!(on(21).has(cell::UNDERLINE));
    for n in 30..=37 {
        assert_eq!(on(n).fg(), Colour::Index((n - 30) as u8));
        assert_eq!(on(n + 10).bg(), Colour::Index((n - 30) as u8));
    }
    for n in 90..=97 {
        assert_eq!(on(n).fg(), Colour::Index((n - 90 + 8) as u8));
        assert_eq!(on(n + 10).bg(), Colour::Index((n - 90 + 8) as u8));
    }
    // 22 to 29 and 39, 49 undo.
    let t = run(
        20,
        1,
        b"\x1b[1;2;3;4;5;7;8;9;31;41m\x1b[22;23;24;25;27;28;29;39;49mx",
    );
    assert_eq!(t.cell(0, 0), Cell::new('x'));
    let t = run(5, 1, b"\x1b[1;2m\x1b[22mx");
    assert_eq!(t.cell(0, 0).attrs(), 0, "22 ends both bold and faint");
    // 26, 10 to 20, 50 to 89 and the rest change nothing.
    for n in [10u16, 20, 26, 50, 53, 60, 75, 89, 98, 99, 108, 255] {
        assert_eq!(on(n), Cell::new('x'), "{n}");
    }
    // 4:0 off, 4:3 curly drawn as an underline.
    let t = run(5, 1, b"\x1b[4:3mx\x1b[4:0my");
    assert!(t.cell(0, 0).has(cell::UNDERLINE) && !t.cell(1, 0).has(cell::UNDERLINE));
    // An empty SGR is a reset; `0;` too.
    let t = run(5, 1, b"\x1b[1m\x1b[mx");
    assert_eq!(t.cell(0, 0).attrs(), 0);
    let t = run(5, 1, b"\x1b[1m\x1b[;1mx");
    assert_eq!(t.cell(0, 0).attrs(), cell::BOLD);
}

#[test]
fn bold_does_not_brighten() {
    let t = run(5, 1, b"\x1b[31;1mx");
    assert_eq!(t.cell(0, 0).fg(), Colour::Index(1));
}

#[test]
fn a_sequence_with_more_than_32_parameters_is_ignored_whole() {
    let mut s = String::from("\x1b[1");
    for _ in 0..31 {
        s.push_str(";1");
    }
    s.push('m');
    let t = run(5, 1, std::format!("{s}x").as_bytes());
    assert!(t.cell(0, 0).has(cell::BOLD), "32 parameters are read");
    let mut s = String::from("\x1b[1");
    for _ in 0..32 {
        s.push_str(";1");
    }
    s.push('m');
    let t = run(5, 1, std::format!("{s}x").as_bytes());
    assert_eq!(
        t.cell(0, 0).attrs(),
        0,
        "33 are not: the sequence is ignored"
    );
    // The next sequence is read as usual.
    let t = run(5, 1, std::format!("{s}\x1b[3mx").as_bytes());
    assert_eq!(t.cell(0, 0).attrs(), cell::ITALIC);
}

#[test]
fn parameters_are_clamped_and_defaults_apply() {
    let t = run(10, 5, b"\x1b[99999999999;99999999999H");
    assert_eq!(t.cursor(), (9, 4));
    let t = run(10, 5, b"\x1b[0;0H");
    assert_eq!(t.cursor(), (0, 0));
    let t = run(10, 5, b"\x1b[;5H");
    assert_eq!(t.cursor(), (4, 0));
    let t = run(10, 5, b"\x1b[3;H");
    assert_eq!(t.cursor(), (0, 2));
    let t = run(10, 5, b"\x1b[H\x1b[0C\x1b[0B");
    assert_eq!(t.cursor(), (1, 1), "0 means 1");
}

#[test]
fn control_characters_act_inside_a_sequence() {
    let t = run(10, 3, b"ab\x1b[1\x08C");
    assert_eq!(t.cursor(), (2, 0), "BS moved to column 1, then CUF 1");
    let t = run(10, 3, b"\x1b[2\r\nm");
    assert_eq!(
        t.cursor(),
        (0, 1),
        "CR and LF acted, then the final byte ended SGR 2"
    );
    assert!(!t.cell(0, 0).has(cell::FAINT));
    // CAN and SUB end a sequence, and what follows is text.
    let t = run(10, 3, b"\x1b[31\x18m");
    assert_eq!(t.line(0), "m");
    let t = run(10, 3, b"\x1b]0;ti\x1atle\x07x");
    assert_eq!(t.line(0), "tlex");
}

#[test]
fn line_feed_vertical_tab_and_form_feed_keep_the_column() {
    let t = run(10, 4, b"ab\ncd\x0bef\x0cgh");
    assert_eq!(rows(&t), ["ab", "  cd", "    ef", "      gh"]);
    // `\r\n` is what a line makes (ADR-0331 item 8).
    let t = run(10, 3, b"ab\r\ncd");
    assert_eq!(rows(&t), ["ab", "cd", ""]);
}

#[test]
fn a_character_set_is_chosen_for_g0_and_g1() {
    let t = run(10, 1, b"\x1b(0lqk\x1b(Bq");
    assert_eq!(t.line(0), "┌─┐q");
    // G1 is used while shifted out (SO), G0 after SI.
    let t = run(10, 1, b"\x1b)0q\x0eq\x0fq");
    assert_eq!(t.line(0), "q─q");
    // Another designation is ignored; ASCII again with B.
    let t = run(10, 1, b"\x1b(0\x1b(Aq\x1b(Bq");
    assert_eq!(t.line(0), "─q");
    // Characters outside ASCII are not remapped.
    let t = run(10, 1, "\x1b(0é".as_bytes());
    assert_eq!(t.line(0), "é");
    // The whole set.
    let mut t = term(40, 1);
    t.write(b"\x1b(0");
    for b in 0x5f..=0x7eu8 {
        t.write(&[b]);
    }
    let want = "\u{a0}◆▒␉␌␍␊°±␤␋┘┐┌└┼⎺⎻─⎼⎽├┤┴┬│≤≥π≠£·";
    let got: String = (0..32).map(|x| t.cell(x, 0).ch).collect();
    assert_eq!(got, want);
}

#[test]
fn a_wide_character_takes_two_cells_and_wraps_whole() {
    let t = run(6, 2, "a中b".as_bytes());
    assert_eq!(t.cell(1, 0).ch, '中');
    assert!(t.cell(1, 0).has(cell::WIDE) && t.cell(2, 0).has(cell::TAIL));
    assert_eq!(t.cell(3, 0).ch, 'b');
    assert_eq!(t.cursor(), (4, 0));
    assert_eq!(t.line(0), "a中b");
    // At the last column it does not fit: the cell stays blank, and the
    // character starts the next row.
    let t = run(5, 2, "abcd中".as_bytes());
    assert_eq!(t.cell(4, 0).ch, ' ');
    assert_eq!(rows(&t), ["abcd", "中"]);
    assert_eq!(t.cursor(), (2, 1));
    // Without autowrap it is dropped.
    let t = run(5, 2, "\x1b[?7labcd中".as_bytes());
    assert_eq!(rows(&t), ["abcd", ""]);
    // In a grid one cell wide it takes the one cell.
    let t = run(1, 2, "中".as_bytes());
    assert_eq!(t.cell(0, 0).ch, '中');
    assert!(!t.cell(0, 0).has(cell::WIDE));
}

#[test]
fn overwriting_half_a_wide_character_blanks_the_other_half() {
    let t = run(6, 1, "中\x1b[1Gx".as_bytes());
    assert_eq!(t.line(0), "x");
    assert!(!t.cell(1, 0).has(cell::TAIL));
    let t = run(6, 1, "中\x1b[2Gx".as_bytes());
    assert_eq!(t.cell(0, 0).ch, ' ');
    assert_eq!(t.cell(1, 0).ch, 'x');
    assert!(!t.cell(0, 0).has(cell::WIDE));
    // Deleting and erasing across the middle of one.
    let t = run(6, 1, "a中b\x1b[3G\x1b[P".as_bytes());
    assert!(t.cell(1, 0).ch == ' ' && !t.cell(1, 0).has(cell::WIDE));
    let t = run(6, 1, "a中b\x1b[3G\x1b[1K".as_bytes());
    assert!(t.cell(1, 0).ch == ' ' && !t.cell(1, 0).has(cell::WIDE));
    let t = run(6, 1, "a中b\x1b[3G\x1b[0K".as_bytes());
    assert!(t.cell(1, 0).ch == ' ' && !t.cell(1, 0).has(cell::WIDE));
    let t = run(4, 1, "ab中\x1b[1G\x1b[@".as_bytes());
    // The insertion pushes the right half off the edge: no half is left.
    assert!(!t.cell(3, 0).has(cell::TAIL) || t.cell(2, 0).has(cell::WIDE));
}

#[test]
fn a_combining_character_joins_the_cell_before_it() {
    let t = run(6, 1, "e\u{301}x".as_bytes());
    assert_eq!(t.cursor(), (2, 0));
    assert_eq!(t.cell(0, 0).marks().collect::<Vec<_>>(), ['\u{301}']);
    assert_eq!(t.line(0), "e\u{301}x");
    // Two fit, a third is dropped.
    let t = run(6, 1, "e\u{301}\u{323}\u{302}".as_bytes());
    assert_eq!(t.cell(0, 0).marks().count(), 2);
    assert_eq!(t.cursor(), (1, 0));
    // At the start of a row it is dropped; after a wide character it joins it.
    let t = run(6, 1, "\u{301}x".as_bytes());
    assert_eq!(t.cell(0, 0).marks().count(), 0);
    assert_eq!(t.line(0), "x");
    let t = run(6, 1, "中\u{301}".as_bytes());
    assert_eq!(t.cell(0, 0).marks().count(), 1);
    // After a full row, with the wrap pending, it joins the last cell.
    let t = run(3, 2, "abc\u{301}d".as_bytes());
    assert_eq!(t.cell(2, 0).marks().count(), 1);
    assert_eq!(t.line(1), "d");
    // A mark outside the BMP is dropped (the cell holds 16 bits).
    let t = run(6, 1, "e\u{e0100}".as_bytes());
    assert_eq!(t.cell(0, 0).marks().count(), 0);
}

#[test]
fn the_grid_counts_columns_as_the_width_table_does() {
    // Every code point: the cursor moves by the table's width (INV-0332-4, the
    // grid's half).
    let mut t = term(6, 1);
    let mut buf = [0u8; 4];
    for c in (0..=0x10ffffu32).filter_map(char::from_u32) {
        if c.is_control() {
            continue;
        }
        t.write(b"\x1b[1G");
        t.write(b"\x1b[0m");
        let before = t.cursor().0;
        assert_eq!(before, 0);
        t.write(c.encode_utf8(&mut buf).as_bytes());
        let want = zoraya_width::columns(c);
        // A mark at the start of a row is dropped: it moves nothing, as width 0 says.
        assert_eq!(t.cursor().0, want, "U+{:04X}", u32::from(c));
    }
}

#[test]
fn the_alternate_screen_has_its_own_grid_and_no_scrollback() {
    let mut t = term(10, 3);
    t.write(b"primary\r\nline2");
    t.write(b"\x1b[?1049h");
    assert!(t.on_alternate_screen());
    assert_eq!(rows(&t), ["", "", ""]);
    t.write(b"\x1b[H\x1b[2Jvim");
    assert_eq!(t.cursor(), (3, 0));
    t.write(b"\x1b[?1049l");
    assert_eq!(rows(&t), ["primary", "line2", ""]);
    assert_eq!(t.cursor(), (5, 1), "the cursor is where 1049 saved it");
    // 47 and 1047 switch without a cursor; each enters a blank grid.
    t.write(b"\x1b[?47h");
    assert_eq!(rows(&t), ["", "", ""]);
    t.write(b"x\x1b[?47l");
    assert_eq!(rows(&t), ["primary", "line2", ""]);
    t.write(b"\x1b[?1047h\x1b[?1047l");
    assert_eq!(rows(&t), ["primary", "line2", ""]);
    // 1048 saves and restores the cursor alone.
    t.write(b"\x1b[1;1H\x1b[?1048h\x1b[3;5H\x1b[?1048l");
    assert_eq!(t.cursor(), (0, 0));
    // Entering twice and leaving twice is harmless.
    t.write(b"\x1b[?1049h\x1b[?1049h\x1b[?1049l\x1b[?1049l");
    assert!(!t.on_alternate_screen());
    // Scrolling on it keeps nothing.
    let before = t.history_len();
    t.write(b"\x1b[?1049h");
    for _ in 0..20 {
        t.write(b"x\r\n");
    }
    assert_eq!(t.history_len(), before);
    t.write(b"\x1b[?1049l");
    // The view cannot go back while it is shown.
    t.write(b"\x1b[?1049h");
    t.scroll_view(5);
    assert_eq!(t.view(), 0);
}

#[test]
fn a_scroll_region_and_origin_mode() {
    let mut t = run(10, 6, b"\x1b[2;4r");
    assert_eq!(t.scroll_region(), (1, 3));
    assert_eq!(t.cursor(), (0, 0), "setting the region homes the cursor");
    // A region of one row, or past the screen, is refused.
    t.write(b"\x1b[3;3r");
    assert_eq!(t.scroll_region(), (1, 3));
    t.write(b"\x1b[2;99r");
    assert_eq!(t.scroll_region(), (1, 3));
    t.write(b"\x1b[r");
    assert_eq!(t.scroll_region(), (0, 5));
    // Origin mode counts rows from the region's top and stays inside it.
    t.write(b"\x1b[2;4r\x1b[?6h");
    assert_eq!(t.cursor(), (0, 1));
    t.write(b"\x1b[1;1Hx\x1b[99;1Hy");
    assert_eq!((t.cell(0, 1).ch, t.cell(0, 3).ch), ('x', 'y'));
    t.write(b"\x1b[?6l");
    assert_eq!(t.cursor(), (0, 0));
    // Above the region the cursor moves freely; scrolling is the region's.
    let mut t = run(10, 5, b"\x1b[2;3r\x1b[1;1HA\r\nB\r\nC\r\nD");
    // A is on row 0; B on row 1 (the region's top); C row 2; D scrolls the region.
    assert_eq!(rows(&t), ["A", "C", "D", "", ""]);
    t.write(b"\x1b[5;1H\r\nZ");
    assert_eq!(
        t.line(4),
        "Z",
        "below the region, a line feed does not scroll it"
    );
}

#[test]
fn the_scrollback_is_a_setting() {
    let mut t = Term::with_scrollback(5, 2, 3);
    for i in 0..10 {
        t.write(std::format!("{i}\r\n").as_bytes());
    }
    assert_eq!((t.history_len(), t.scrollback()), (3, 3));
    t.scroll_view(3);
    let top = t.shown(0).iter().map(|c| c.ch).collect::<String>();
    assert_eq!(top.trim_end(), "6", "the oldest kept line is the 7th");
    // The setting changes at run time and keeps the newest lines.
    let mut t = Term::with_scrollback(5, 2, 10);
    for i in 0..8 {
        t.write(std::format!("{i}\r\n").as_bytes());
    }
    assert_eq!(t.history_len(), 7);
    t.set_scrollback(2);
    assert_eq!(t.history_len(), 2);
    t.scroll_view(2);
    assert_eq!(
        t.shown(0)
            .iter()
            .map(|c| c.ch)
            .collect::<String>()
            .trim_end(),
        "5"
    );
    t.set_scrollback(0);
    assert_eq!(t.history_len(), 0);
    t.write(b"a\r\nb\r\nc\r\n");
    assert_eq!(t.history_len(), 0, "no scrollback at all");
    t.set_scrollback(4);
    t.write(b"a\r\nb\r\nc\r\n");
    assert_eq!(t.history_len(), 3);
    // The maximum.
    let t = Term::with_scrollback(5, 2, usize::MAX);
    assert_eq!(t.scrollback(), MAX_SCROLLBACK);
    // ED 3 clears it.
    let mut t = Term::with_scrollback(5, 2, 10);
    t.write(b"a\r\nb\r\nc\r\nd\r\n\x1b[3J");
    assert_eq!(t.history_len(), 0);
    assert_eq!(t.view(), 0);
}

#[test]
fn resizing_keeps_the_history_in_order_and_the_alternate_screen_sane() {
    let mut t = Term::with_scrollback(6, 2, 4);
    for i in 0..8 {
        t.write(std::format!("{i}\r\n").as_bytes());
    }
    t.resize(3, 2);
    t.scroll_view(4);
    let lines: Vec<String> = (0..2)
        .map(|y| {
            t.shown(y)
                .iter()
                .map(|c| c.ch)
                .collect::<String>()
                .trim_end()
                .into()
        })
        .collect();
    assert_eq!(
        lines,
        ["3", "4"],
        "the four newest history lines are 3 to 6, oldest first, two show"
    );
    t.write(b"\x1b[?1049h\x1b[2;2Hxyz");
    t.resize(4, 5);
    assert!(t.on_alternate_screen());
    let (x, y) = t.cursor();
    assert!(x < 4 && y < 5);
    t.resize(2, 1);
    assert!(t.cursor().0 < 2 && t.cursor().1 < 1);
    t.write(b"\x1b[?1049l");
    assert!(!t.on_alternate_screen());
}

/// INV-0331-4: after `Restore` the screen is the primary, the cursor shown,
/// every mode of ADR-0332 clear and the primary screen's text kept; the
/// reinstated modes are what they were.
#[test]
fn restore_gives_the_shell_a_working_screen_back() {
    let mut t = term(10, 4);
    t.write(b"keep\r\nthis");
    t.write(
        b"\x1b[?1049h\x1b[?25l\x1b[?1h\x1b=\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?1004h",
    );
    t.write(b"\x1b[?2026h\x1b[?5h\x1b[?6h\x1b[?7l\x1b[4h\x1b[2;3r\x1b[1;31;44m\x1b(0\x0e\x1b[5 q");
    t.write(b"\x1b]4;5;rgb:ff/00/00\x07\x1b[3");
    let was = t.restore();
    assert!(!t.on_alternate_screen());
    assert_eq!(*t.modes(), crate::Modes::DEFAULT);
    assert_eq!(rows(&t), ["keep", "this", "", ""]);
    assert_eq!(t.scroll_region(), (0, 3));
    assert_eq!(t.cursor_shape(), 0);
    assert_eq!(t.palette_override(5), None);
    // The unfinished sequence is gone, and text is text again, plain.
    t.write(b"m!");
    assert_eq!(t.cell(4, 1).ch, 'm');
    assert_eq!(t.cell(4, 1).attrs(), 0);
    assert_eq!(t.cell(4, 1).fg(), Colour::Default);
    assert_eq!(t.cell(5, 1).ch, '!');
    t.write(b"q");
    assert_eq!(t.cell(6, 1).ch, 'q', "the character set is ASCII");
    // A stopped line comes back: its modes are set again.
    t.reinstate(was);
    assert!(t.on_alternate_screen());
    assert!(!t.modes().cursor_visible && t.modes().app_cursor && t.modes().bracketed_paste);
    assert!(t.modes().mouse_press && t.modes().mouse_drag && t.modes().mouse_sgr);
    assert!(t.modes().focus_events && t.modes().origin && !t.modes().autowrap);
    assert_eq!(t.scroll_region(), (1, 2));
    assert_eq!(t.cursor_shape(), 5);
    // Restore twice is harmless and the second gives back plain defaults.
    let _ = t.restore();
    let again = t.restore();
    t.reinstate(again);
    assert_eq!(*t.modes(), crate::Modes::DEFAULT);
    assert!(!t.on_alternate_screen());
}

#[test]
fn palette_entries_are_set_and_reset_by_osc() {
    let mut t = term(10, 1);
    t.write(b"\x1b]4;1;rgb:12/34/56\x07");
    assert_eq!(t.palette_override(1), Some((0x12, 0x34, 0x56)));
    t.write(b"\x1b]4;2;#ff8000;3;rgb:0/0/0\x1b\\");
    assert_eq!(t.palette_override(2), Some((255, 128, 0)));
    assert_eq!(t.palette_override(3), Some((0, 0, 0)));
    t.write(b"\x1b]104;2\x07");
    assert_eq!(t.palette_override(2), None);
    assert_eq!(t.palette_override(1), Some((0x12, 0x34, 0x56)));
    t.write(b"\x1b]104\x07");
    assert_eq!(t.palette_override(1), None);
    assert_eq!(t.palette_override(3), None);
    // Bad entries are skipped, a query is not answered, an index past 255 is not an entry.
    t.write(b"\x1b]4;1;?\x07\x1b]4;256;#000\x07\x1b]4;x;#000\x07\x1b]4;7;nonsense\x07");
    for i in 0..=255 {
        assert_eq!(t.palette_override(i), None);
    }
    // The colour is used when drawing.
    let mut look = crate::Look::new(zoraya_tokens::Scale::ONE, zoraya_tokens::Appearance::Light);
    t.write(b"\x1b]4;1;rgb:aa/bb/cc\x07\x1b[31mx");
    let (fg, _) = t.colours_of(&look, &t.cell(0, 0));
    assert_eq!(fg.pixel(), 0x00aa_bbcc);
    look.palette[1] = look.fg;
    t.write(b"\x1b]104\x07");
    let (fg, _) = t.colours_of(&look, &t.cell(0, 0));
    assert_eq!(fg, look.fg, "back to the palette's own");
}

#[test]
fn strings_are_cut_at_their_bound_and_not_run() {
    let mut t = term(10, 1);
    let mut s = std::vec![0x1b, b']'];
    s.extend_from_slice(b"4;1;rgb:12/34/56;");
    s.extend(std::iter::repeat_n(b'1', 5000));
    s.extend_from_slice(b"\x07");
    t.write(&s);
    assert_eq!(
        t.palette_override(1),
        None,
        "a string over the bound is dropped whole"
    );
    // `ESC` then anything but `\` ends a string without running it and starts a sequence.
    let mut t = term(10, 1);
    t.write(b"\x1b]4;1;rgb:12/34/56\x1b[1mx");
    assert_eq!(t.palette_override(1), None);
    assert!(t.cell(0, 0).has(cell::BOLD));
}

#[test]
fn the_modes_the_adr_recognises_without_effect_change_nothing() {
    for m in [
        12u16, 1003, 1005, 1015, 1034, 2027, 3, 4, 8, 40, 45, 66, 69, 80, 95, 1010, 1011, 1036,
        1037, 1042, 1070, 2001, 2005, 2006,
    ] {
        for set in ["h", "l"] {
            let mut t = term(10, 3);
            t.write(b"abc\x1b[2;3Hx\x1b[1;31m");
            let before = t.clone();
            t.write(std::format!("\x1b[?{m}{set}").as_bytes());
            assert!(t.same_state(&before), "?{m}{set}");
        }
    }
    // Several modes in one sequence are all applied.
    let mut t = term(10, 3);
    t.write(b"\x1b[?2004;1006;1000h");
    assert!(t.modes().bracketed_paste && t.modes().mouse_sgr && t.modes().mouse_press);
    t.write(b"\x1b[?2004;1000l");
    assert!(!t.modes().bracketed_paste && !t.modes().mouse_press && t.modes().mouse_sgr);
    // Insert mode is the one ANSI mode that acts; 20 (LNM) and the rest do not.
    let mut t = term(10, 1);
    t.write(b"\x1b[4h\x1b[20h");
    assert!(t.modes().insert);
    t.write(b"\x1b[4l");
    assert!(!t.modes().insert);
    // Reverse screen reverses what is drawn, not what is stored.
    let look = crate::Look::new(zoraya_tokens::Scale::ONE, zoraya_tokens::Appearance::Light);
    let mut t = term(4, 1);
    t.write(b"x");
    let plain = t.colours_of(&look, &t.cell(0, 0));
    t.write(b"\x1b[?5h");
    let swapped = t.colours_of(&look, &t.cell(0, 0));
    assert_eq!((swapped.0, swapped.1), (plain.1, plain.0));
    assert_eq!(t.cell(0, 0), Cell::new('x'));
}

/// INV-0096-2 and INV-0332-2: the terminal answers nothing. It has no output
/// path: `write` returns nothing, and a query changes at most what any
/// well-formed unknown sequence changes, which is nothing.
#[test]
fn no_query_is_answered_and_none_changes_anything() {
    // Compiles only while `write` gives nothing back.
    let _: () = term(2, 2).write(b"");
    let queries: &[&[u8]] = &[
        b"\x1b[5n",
        b"\x1b[6n",
        b"\x1b[?6n",
        b"\x1b[?15n",
        b"\x1b[?25n",
        b"\x1b[?26n",
        b"\x1b[?53n",
        b"\x1b[?62n",
        b"\x1b[?75n",
        b"\x1b[c",
        b"\x1b[0c",
        b"\x1b[>c",
        b"\x1b[>0c",
        b"\x1b[=c",
        b"\x1bZ",
        b"\x05",
        b"\x1b[>q",
        b"\x1b[>0q",
        b"\x1b[0x",
        b"\x1b[1x",
        b"\x1b[?996n",
        b"\x1b[?u",
        b"\x1b[?1;1;0S",
        b"\x1b[?2026$p",
        b"\x1b[?25$p",
        b"\x1b[4$p",
        b"\x1b[2;1;1;1;1;1*y",
        b"\x1b[14t",
        b"\x1b[16t",
        b"\x1b[18t",
        b"\x1b[19t",
        b"\x1b[20t",
        b"\x1b[21t",
        b"\x1b[22;0;0t",
        b"\x1b[23;0;0t",
        b"\x1b]10;?\x07",
        b"\x1b]11;?\x07",
        b"\x1b]12;?\x07",
        b"\x1b]4;1;?\x07",
        b"\x1b]4;1;?;2;?\x1b\\",
        b"\x1b]52;c;?\x07",
        b"\x1b]52;c;SGVsbG8=\x07",
        b"\x1b]0;a title\x07",
        b"\x1b]2;a title\x1b\\",
        b"\x1b]1;an icon\x07",
        b"\x1b]7;file:///etc\x07",
        b"\x1b]133;A\x07",
        b"\x1bP$qm\x1b\\",
        b"\x1bP$q\"p\x1b\\",
        b"\x1bP+q544e\x1b\\",
        b"\x1bP1+r\x1b\\",
        b"\x1b_Gi=1;OK\x1b\\",
        b"\x1b^private\x1b\\",
        b"\x1bXsos\x1b\\",
        b"\x1b[?1;2c",
        b"\x1b[>41;354;0c",
    ];
    for q in queries {
        let mut t = term(20, 5);
        t.write(b"text\x1b[2;3H\x1b[1m");
        let before = t.clone();
        t.write(q);
        // The visible text is only what the sequence may print: none of it.
        assert_eq!(rows(&t), rows(&before), "{q:?}");
        assert_eq!(t.cursor(), before.cursor(), "{q:?}");
        // Window operations and the title stack are ignored too.
        if !q.starts_with(b"\x05") {
            assert!(t.same_state(&before), "{q:?}");
        }
    }
}

#[test]
fn a_hyperlink_prints_its_text_and_nothing_else() {
    let t = run(10, 2, b"\x1b]8;;http://x\x07link\x1b]8;;\x07!");
    assert_eq!(t.line(0), "link!");
}

#[test]
fn osc_52_and_titles_are_ignored() {
    // ADR-0332 item 4: nothing a program prints sets the title or the clipboard.
    let mut t = term(10, 2);
    t.write(b"\x1b]0;x\x07\x1b]2;y\x07\x1b]52;c;aGk=\x07ok");
    assert_eq!(t.line(0), "ok");
}

#[test]
fn insert_and_delete_stay_inside_the_row_and_the_region() {
    let mut t = run(5, 3, b"abcde");
    t.write(b"\x1b[1G\x1b[99@");
    assert_eq!(t.line(0), "");
    let t = run(5, 3, b"abcde\x1b[1G\x1b[99P");
    assert_eq!(t.line(0), "");
    let t = run(5, 3, b"abcde\x1b[3G\x1b[99X");
    assert_eq!(t.line(0), "ab");
    // IL and DL outside the region do nothing; inside they leave the column.
    let mut t = run(5, 5, b"1\r\n2\r\n3\r\n4\r\n5\x1b[2;4r\x1b[1;1H\x1b[L\x1b[M");
    assert_eq!(rows(&t), ["1", "2", "3", "4", "5"]);
    t.write(b"\x1b[3;3H\x1b[L");
    assert_eq!(rows(&t), ["1", "2", "", "3", "5"]);
    t.write(b"\x1b[2;1H\x1b[M");
    assert_eq!(rows(&t), ["1", "", "3", "", "5"]);
}

#[test]
fn rep_repeats_the_last_character_and_is_bounded() {
    let t = run(10, 1, b"\x1b[3b");
    assert_eq!(t.line(0), "", "nothing printed, nothing repeated");
    let t = run(10, 1, b"a\x1b[3b");
    assert_eq!(t.line(0), "aaaa");
    // Huge counts cost a bounded amount of work and keep the grid sane.
    let mut t = run(10, 3, b"a\x1b[99999999b");
    assert!(t.cursor().1 < 3);
    t.write(b"z");
}

#[test]
fn tab_stops_are_set_and_cleared() {
    let mut t = term(30, 1);
    t.write(b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;12H\x1bH\x1b[1G\t");
    assert_eq!(t.cursor().0, 4);
    t.write(b"\t");
    assert_eq!(t.cursor().0, 11);
    t.write(b"\x1b[I");
    assert_eq!(t.cursor().0, 29, "no stop left past 11");
    t.write(b"\x1b[2Z");
    assert_eq!(t.cursor().0, 4);
    // TBC 0 clears the stop at the cursor.
    t.write(b"\x1b[0g\x1b[1G\t");
    assert_eq!(t.cursor().0, 11);
    // CHT with a count.
    let mut t = term(40, 1);
    t.write(b"\x1b[3I");
    assert_eq!(t.cursor().0, 24);
    // A resize gives new columns their stops.
    t.resize(60, 1);
    t.write(b"\x1b[1G\x1b[5I");
    assert_eq!(t.cursor().0, 40);
}

#[test]
fn the_cursor_stays_put_through_sgr_and_modes_while_a_wrap_is_pending() {
    let t = run(3, 2, b"abc\x1b[1m\x1b[?25h\x1b[?1hd");
    assert_eq!(rows(&t), ["abc", "d"]);
    // A cursor movement ends the pending wrap.
    let t = run(3, 2, b"abc\x1b[Dd");
    assert_eq!(rows(&t), ["adc", ""]);
}

#[test]
fn full_reset_clears_the_screen_and_modes_but_keeps_the_scrollback() {
    let mut t = Term::with_scrollback(5, 2, 10);
    t.write(b"a\r\nb\r\nc\r\n\x1b[?1049h\x1b[?2004h\x1b[31mx\x1bc");
    assert!(!t.on_alternate_screen());
    assert_eq!(*t.modes(), crate::Modes::DEFAULT);
    assert_eq!(rows(&t), ["", ""]);
    assert!(t.history_len() > 0);
    t.write(b"y");
    assert_eq!(t.cell(0, 0).attrs(), 0);
    assert_eq!(t.cell(0, 0).fg(), Colour::Default);
}

#[test]
fn soft_reset_keeps_the_screen_and_resets_modes() {
    let mut t = run(
        10,
        3,
        b"abc\x1b[?1h\x1b[4h\x1b[2;3r\x1b[31m\x1b(0\x1b[3;3H\x1b[!p",
    );
    assert_eq!(t.line(0), "abc");
    assert_eq!(t.cursor(), (2, 2));
    assert_eq!(*t.modes(), crate::Modes::DEFAULT);
    assert_eq!(t.scroll_region(), (0, 2));
    t.write(b"q");
    assert_eq!(t.cell(2, 2).ch, 'q');
    assert_eq!(t.cell(2, 2).attrs(), 0);
    assert_eq!(t.cell(2, 2).fg(), Colour::Default);
}

#[test]
fn utf8_is_decoded_in_every_state() {
    // A character inside a sequence ends the sequence and is text.
    let t = run(10, 1, "\x1b[1é".as_bytes());
    assert_eq!(t.line(0), "é");
    let t = run(10, 1, "\x1bé".as_bytes());
    assert_eq!(t.line(0), "é");
    // C1 controls as UTF-8 (U+0080 to U+009F) are not controls here.
    let t = run(10, 1, "a\u{9b}6nb".as_bytes());
    assert_eq!(t.line(0), "a\u{fffd}6nb");
    // An overlong NUL and a surrogate.
    let t = run(10, 1, b"\xc0\x80\xed\xa0\x80");
    assert!(t.line(0).chars().all(|c| c == '\u{fffd}'));
}

#[test]
fn a_newline_scrolls_into_the_history_only_from_the_whole_screen() {
    let mut t = Term::with_scrollback(5, 3, 10);
    t.write(b"\x1b[1;2r\x1b[1;1Ha\r\nb\r\nc\r\nd");
    assert_eq!(
        t.history_len(),
        0,
        "a region that is not the screen keeps nothing"
    );
    t.write(b"\x1b[r\x1b[3;1H\r\n\r\n");
    assert_eq!(t.history_len(), 2);
}

#[test]
fn rendering_draws_attributes_wide_characters_marks_and_the_cursor_shapes() {
    use crate::Look;
    use zoraya_tokens::{Appearance, Scale};
    let mut look = Look::new(Scale::ONE, Appearance::Light);
    let mut t = term(12, 2);
    t.write("\x1b[3mi\x1b[0m\x1b[4mu\x1b[0m\x1b[9ms\x1b[0m中e\u{301}\x1b[38;2;200;10;20mR\x1b[48;5;21mB".as_bytes());
    let (w, h) = look.surface_size(12, 2);
    let mut pixels = std::vec![0u32; w * h];
    look.clear(&mut pixels);
    assert!(t.render(&mut look, &mut pixels, w, false).is_some());
    let cw = look.cell_width();
    // The direct colour reaches the pixels, so does the 256-colour background.
    let (rx, ry) = look.cell_origin(6, 0);
    let red = (0..look.cell_height())
        .any(|dy| (0..cw).any(|dx| pixels[(ry + dy) * w + rx + dx] == 0x00c8_0a14));
    assert!(red, "the glyph of the direct colour is drawn in it");
    let (bx, by) = look.cell_origin(7, 0);
    assert_eq!(
        pixels[by * w + bx],
        0x0000_00ff,
        "256-colour index 21 is blue"
    );
    // The wide character covers two cells: the box is drawn across them.
    let (wx, wy) = look.cell_origin(3, 0);
    let ink = (0..look.cell_height())
        .any(|dy| (0..2 * cw).any(|dx| pixels[(wy + dy) * w + wx + dx] != look.bg.pixel()));
    assert!(ink, "the wide character is drawn (as the box)");
    // An underline is a row of the cell in the foreground colour.
    let (ux, uy) = look.cell_origin(1, 0);
    let underlined = (0..look.cell_height())
        .any(|dy| (0..cw).all(|dx| pixels[(uy + dy) * w + ux + dx] == look.fg.pixel()));
    assert!(underlined);
    // Cursor shapes: a block inverts the cell, an underline and a bar mark part of it.
    for (shape, whole) in [(2u8, true), (4, false), (6, false)] {
        t.write(std::format!("\x1b[{shape} q\x1b[2;1H").as_bytes());
        t.redraw();
        look.clear(&mut pixels);
        t.render(&mut look, &mut pixels, w, true);
        let (cx, cy) = look.cell_origin(0, 1);
        let count = (0..look.cell_height())
            .flat_map(|dy| (0..cw).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| pixels[(cy + dy) * w + cx + dx] == look.fg.pixel())
            .count();
        if whole {
            assert_eq!(count, cw * look.cell_height());
        } else {
            assert!(
                count > 0 && count < cw * look.cell_height(),
                "shape {shape}"
            );
        }
    }
    // A hidden cursor is not drawn.
    t.write(b"\x1b[?25l");
    t.redraw();
    look.clear(&mut pixels);
    t.render(&mut look, &mut pixels, w, true);
    let (cx, cy) = look.cell_origin(0, 1);
    assert_eq!(pixels[cy * w + cx], look.bg.pixel());
}

/// DEL is ignored wherever it comes, and the sequence it came inside goes on
/// (mutation testing, 10a).
#[test]
fn delete_is_ignored_in_every_state() {
    let t = run(10, 1, b"a\x7fb");
    assert_eq!(t.line(0), "ab");
    // After ESC: the escape goes on, so `[1m` is still a sequence.
    let t = run(10, 1, b"\x1b\x7f[1mx");
    assert_eq!(t.line(0), "x");
    assert!(t.cell(0, 0).has(cell::BOLD));
    // After ESC and an intermediate.
    let t = run(10, 1, b"\x1b(\x7f0q");
    assert_eq!(t.line(0), "─", "ESC ( DEL 0 selects the line set");
    // Inside a CSI, and inside one that is ignored.
    let t = run(10, 1, b"\x1b[1\x7f;4mx");
    assert!(t.cell(0, 0).has(cell::BOLD) && t.cell(0, 0).has(cell::UNDERLINE));
    let t = run(10, 1, b"\x1b[ 1\x7f;2qx");
    assert_eq!(t.cursor_shape(), 0);
    assert_eq!(t.line(0), "x");
}

/// A control character inside `ESC` or `ESC` and an intermediate acts and
/// leaves the sequence going; a second `ESC` starts it again.
#[test]
fn controls_and_escapes_inside_an_escape_sequence() {
    let t = run(10, 3, b"ab\x1b\r[1mx");
    assert_eq!(t.line(0), "xb", "CR acted, then [1m was a sequence");
    assert!(t.cell(0, 0).has(cell::BOLD));
    let t = run(10, 3, b"\x1b\n[1mx");
    assert_eq!(t.cursor(), (1, 1));
    assert!(t.cell(0, 1).has(cell::BOLD));
    let t = run(10, 3, b"\x1b(\n0q");
    assert_eq!(t.cell(0, 1).ch, '─', "LF inside ESC ( then the final 0");
    let t = run(10, 3, b"\x1b\x1b[1mx");
    assert!(t.cell(0, 0).has(cell::BOLD));
    // ESC inside ESC ( starts a new sequence: `[` is no final of the old one.
    let t = run(10, 3, b"\x1b(\x1b[1mx");
    assert!(t.cell(0, 0).has(cell::BOLD), "{:?}", t.line(0));
    assert_eq!(t.line(0), "x");
    // Another intermediate makes the sequence one this terminal does not know.
    let t = run(10, 3, b"\x1b((0q");
    assert_eq!(t.line(0), "q");
}

/// What follows an intermediate byte of a `CSI` may be intermediates and the
/// final byte: a parameter, a separator or a private marker there makes the
/// sequence invalid, and it changes nothing.
#[test]
fn a_csi_with_a_parameter_after_an_intermediate_is_ignored() {
    for bad in [
        &b"\x1b[ 1q"[..],
        b"\x1b[ ;1q",
        b"\x1b[ :1q",
        b"\x1b[ ?1q",
        b"\x1b[1 ?q",
        b"\x1b[ \"1q",
        b"\x1b[!1p",
    ] {
        let mut t = run(10, 2, b"ab\x1b[5 q");
        assert_eq!(t.cursor_shape(), 5);
        t.write(b"\x1b[?25l");
        let before = t.clone();
        t.write(bad);
        assert!(t.same_state(&before), "{bad:?}");
        t.write(b"x");
        assert_eq!(t.line(0), "abx", "{bad:?}: the text after it is text");
    }
}

/// A private marker is valid only first.
#[test]
fn a_private_marker_after_a_parameter_ignores_the_sequence() {
    for bad in [
        &b"\x1b[1?h"[..],
        b"\x1b[;?h",
        b"\x1b[??h",
        b"\x1b[1:?h",
        b"\x1b[>?h",
        b"\x1b[1;>2004h",
    ] {
        let mut t = term(10, 2);
        t.write(bad);
        assert_eq!(*t.modes(), crate::Modes::DEFAULT, "{bad:?}");
    }
    let mut t = term(10, 2);
    t.write(b"\x1b[?2004h");
    assert!(t.modes().bracketed_paste);
    // `?` first is the marker; after `;` it is not.
    let mut t = term(10, 2);
    t.write(b"\x1b[?25;2004h");
    assert!(t.modes().bracketed_paste);
}

/// An ignored `CSI` takes its parameters and intermediates and ends at its
/// final byte; an `ESC` or a control inside it acts; it is cut at the bound.
#[test]
fn an_ignored_csi_ends_where_a_csi_ends() {
    let t = run(10, 1, b"\x1b[ 1;23:4?p x");
    assert_eq!(
        t.line(0),
        " x",
        "the digits, `;`, `:` and `?` were all part of it"
    );
    let t = run(10, 3, b"\x1b[ 1\nmx");
    assert_eq!(
        t.cursor(),
        (1, 1),
        "a control inside it acts, and m ends it"
    );
    assert_eq!(t.cell(0, 1).ch, 'x');
    let t = run(10, 1, b"\x1b[ 1\x1b[1mx");
    assert!(
        t.cell(0, 0).has(cell::BOLD),
        "ESC inside it starts a new sequence"
    );
    // A long one is cut at the bound, and the rest is text.
    let mut long = std::vec![0x1b, b'[', b' ', b'1'];
    long.extend(std::iter::repeat_n(b'9', 5000));
    long.extend_from_slice(b"mA");
    let mut t = term(80, 24);
    t.write(&long);
    let shown: String = (0..24).map(|y| t.line(y)).collect();
    let nines = shown.chars().filter(|c| *c == '9').count();
    assert!(
        shown.ends_with("mA"),
        "{}",
        &shown[shown.len().saturating_sub(10)..]
    );
    assert!((500..1500).contains(&nines), "{nines} digits were text");
}

#[test]
fn control_characters_inside_an_osc_are_not_part_of_it() {
    let mut t = term(10, 1);
    t.write(b"\x1b]4;1;rgb:12/\x0134/\x7f56\x07");
    assert_eq!(t.palette_override(1), Some((0x12, 0x34, 0x56)));
    // An empty one runs nothing; a number with no entry in it is not run.
    t.write(b"\x1b]\x07\x1b]4\x07\x1b]4;\x07\x1b]104;\x07");
    assert_eq!(t.palette_override(1), Some((0x12, 0x34, 0x56)));
}

#[test]
fn palette_indices_are_numbers_of_at_most_four_digits_below_256() {
    let mut t = term(10, 1);
    t.write(b"\x1b]4;0001;#010203\x07");
    assert_eq!(t.palette_override(1), Some((1, 2, 3)));
    t.write(b"\x1b]4;00001;#040506\x07\x1b]4;;#070809\x07\x1b]4;1x;#0a0b0c\x07");
    assert_eq!(
        t.palette_override(1),
        Some((1, 2, 3)),
        "none of them is an entry"
    );
    for i in [0u8, 2, 255] {
        assert_eq!(t.palette_override(i), None);
    }
    t.write(b"\x1b]4;255;#0d0e0f\x07\x1b]4;256;#101112\x07");
    assert_eq!(t.palette_override(255), Some((13, 14, 15)));
    // 104 takes the same numbers, and one past 255 is no entry to reset.
    t.write(b"\x1b]104;256\x07\x1b]104;00255;\x07\x1b]104;x\x07");
    assert_eq!(t.palette_override(255), Some((13, 14, 15)));
    t.write(b"\x1b]104;0255\x07");
    assert_eq!(t.palette_override(255), None);
    assert_eq!(t.palette_override(1), Some((1, 2, 3)));
}

#[test]
fn next_line_and_previous_line_go_to_column_zero() {
    let t = run(10, 5, b"abc\x1b[2E");
    assert_eq!(t.cursor(), (0, 2));
    let t = run(10, 5, b"\x1b[4;5H\x1b[2F");
    assert_eq!(t.cursor(), (0, 1));
    let t = run(10, 5, b"\x1b[4;5H\x1b[E\x1b[F\x1b[F");
    assert_eq!(t.cursor(), (0, 2));
}

#[test]
fn csi_t_with_more_than_one_parameter_is_not_a_scroll() {
    let mut t = run(10, 3, b"a\r\nb\r\nc");
    t.write(b"\x1b[1;2;3;4;5T");
    assert_eq!(rows(&t), ["a", "b", "c"]);
    t.write(b"\x1b[T");
    assert_eq!(rows(&t), ["", "a", "b"]);
    t.write(b"\x1b[S");
    assert_eq!(rows(&t), ["a", "b", ""]);
}

#[test]
fn only_mode_4_of_the_ansi_modes_acts_and_only_as_a_parameter_of_its_own() {
    let mut t = term(10, 1);
    t.write(b"\x1b[20h\x1b[2h\x1b[12h");
    assert!(!t.modes().insert);
    t.write(b"\x1b[1:4h");
    assert!(!t.modes().insert, "a sub-parameter is not a mode");
    t.write(b"\x1b[20;4h");
    assert!(t.modes().insert);
    t.write(b"\x1b[2;4l");
    assert!(!t.modes().insert);
    // A marked SM is not the ANSI mode.
    t.write(b"\x1b[?4h");
    assert!(!t.modes().insert);
}

#[test]
fn dec_modes_take_parameters_not_sub_parameters_and_each_acts() {
    let mut t = term(10, 2);
    t.write(b"\x1b[?2004:1h");
    assert!(t.modes().bracketed_paste && !t.modes().app_cursor);
    t.write(b"\x1b[?2026h");
    assert!(t.modes().synchronized);
    t.write(b"\x1b[?2026l");
    assert!(!t.modes().synchronized);
    t.write(b"\x1b[?1004h\x1b[?1006h\x1b[?1000h\x1b[?1002h\x1b[?5h\x1b[?6h");
    let m = *t.modes();
    assert!(
        m.focus_events
            && m.mouse_sgr
            && m.mouse_press
            && m.mouse_drag
            && m.reverse_screen
            && m.origin
    );
    t.write(b"\x1b[?1004l\x1b[?1006l\x1b[?1000l\x1b[?1002l\x1b[?5l\x1b[?6l\x1b[?2004l");
    assert_eq!(*t.modes(), crate::Modes::DEFAULT);
    // Turning autowrap on ends a wrap that was pending; off leaves it harmless.
    let t = run(5, 2, b"abcde\x1b[?7h");
    assert!(t.modes().autowrap);
    let mut t = run(5, 2, b"abcde\x1b[?7lf");
    assert_eq!(rows(&t), ["abcdf", ""]);
    t.write(b"\x1b[?7hg");
    assert_eq!(
        rows(&t),
        ["abcdg", ""],
        "after turning wrap on, the last cell is not pending"
    );
    let t = run(5, 2, b"abcde\x1b[?7hx");
    assert_eq!(
        rows(&t),
        ["abcde", "x"],
        "a pending wrap survives a mode that was already on"
    );
}

#[test]
fn a_wide_character_fits_exactly_and_two_cells_hold_one() {
    let t = run(4, 2, "ab中".as_bytes());
    assert_eq!(rows(&t), ["ab中", ""]);
    assert!(t.cell(2, 0).has(cell::WIDE) && t.cell(3, 0).has(cell::TAIL));
    let t = run(2, 2, "中".as_bytes());
    assert!(t.cell(0, 0).has(cell::WIDE) && t.cell(1, 0).has(cell::TAIL));
    let t = run(2, 2, "a中".as_bytes());
    assert_eq!(rows(&t), ["a", "中"], "it did not fit after a, and wrapped");
}

#[test]
fn erasing_and_editing_elsewhere_leaves_a_wide_character_whole() {
    for edit in [
        "\x1b[5G\x1b[K",
        "\x1b[5G\x1b[2X",
        "\x1b[5G\x1b[P",
        "\x1b[5G\x1b[@",
        "\x1b[2J",
    ] {
        let t = run(8, 1, std::format!("a中b cd{edit}").as_bytes());
        if edit == "\x1b[2J" {
            assert_eq!(t.line(0), "");
            continue;
        }
        assert!(
            t.cell(1, 0).has(cell::WIDE) && t.cell(2, 0).has(cell::TAIL),
            "{edit:?}"
        );
        assert_eq!(t.cell(1, 0).ch, '中');
    }
    // A wide character at the very end of the row survives an edit before it.
    let t = run(6, 1, "abcd中\x1b[1G\x1b[X".as_bytes());
    assert!(t.cell(4, 0).has(cell::WIDE) && t.cell(5, 0).has(cell::TAIL));
}

#[test]
fn lines_are_inserted_and_deleted_on_the_regions_first_and_last_rows() {
    let base = |top: usize| {
        let mut t = run(4, 6, b"0\r\n1\r\n2\r\n3\r\n4\r\n5");
        t.write(b"\x1b[2;5r");
        t.write(std::format!("\x1b[{top};1H").as_bytes());
        t
    };
    // The last row of the region (row 4, written 5;1).
    let mut t = base(5);
    t.write(b"\x1b[M");
    assert_eq!(rows(&t), ["0", "1", "2", "3", "", "5"]);
    let mut t = base(5);
    t.write(b"\x1b[L");
    assert_eq!(rows(&t), ["0", "1", "2", "3", "", "5"]);
    // The first row of the region.
    let mut t = base(2);
    t.write(b"\x1b[M");
    assert_eq!(rows(&t), ["0", "2", "3", "4", "", "5"]);
    let mut t = base(2);
    t.write(b"\x1b[L");
    assert_eq!(rows(&t), ["0", "", "1", "2", "3", "5"]);
    // Just outside: nothing.
    for outside in [1, 6] {
        let mut t = base(outside);
        t.write(b"\x1b[M\x1b[L");
        assert_eq!(rows(&t), ["0", "1", "2", "3", "4", "5"]);
    }
}

#[test]
fn erase_in_line_and_in_display_in_all_their_modes() {
    let fill = || run(6, 4, b"aaaaaa\r\nbbbbbb\r\ncccccc\r\ndddddd\x1b[3;3H");
    // EL: to the end, from the start (the cursor's cell included), the whole line.
    let mut t = fill();
    t.write(b"\x1b[K");
    assert_eq!(rows(&t), ["aaaaaa", "bbbbbb", "cc", "dddddd"]);
    let mut t = fill();
    t.write(b"\x1b[1K");
    assert_eq!(rows(&t), ["aaaaaa", "bbbbbb", "   ccc", "dddddd"]);
    let mut t = fill();
    t.write(b"\x1b[2K");
    assert_eq!(rows(&t), ["aaaaaa", "bbbbbb", "", "dddddd"]);
    assert_eq!(t.cursor(), (2, 2), "the cursor stays");
    let mut t = fill();
    t.write(b"\x1b[3K");
    assert_eq!(
        rows(&t),
        ["aaaaaa", "bbbbbb", "cccccc", "dddddd"],
        "mode 3 is none"
    );
    // ED: below, above (the cursor's cell included), all.
    let mut t = fill();
    t.write(b"\x1b[J");
    assert_eq!(rows(&t), ["aaaaaa", "bbbbbb", "cc", ""]);
    let mut t = fill();
    t.write(b"\x1b[1J");
    assert_eq!(rows(&t), ["", "", "   ccc", "dddddd"]);
    let mut t = fill();
    t.write(b"\x1b[2J");
    assert_eq!(rows(&t), ["", "", "", ""]);
    assert_eq!(t.cursor(), (2, 2));
    let mut t = fill();
    t.write(b"\x1b[4J");
    assert_eq!(
        rows(&t),
        ["aaaaaa", "bbbbbb", "cccccc", "dddddd"],
        "mode 4 is none"
    );
}

#[test]
fn a_scroll_by_more_than_the_region_clears_it_and_keeps_what_is_outside() {
    let base = || run(4, 6, b"0\r\n1\r\n2\r\n3\r\n4\r\n5\x1b[2;4r");
    let mut t = base();
    t.write(b"\x1b[99S");
    assert_eq!(rows(&t), ["0", "", "", "", "4", "5"]);
    let mut t = base();
    t.write(b"\x1b[99T");
    assert_eq!(rows(&t), ["0", "", "", "", "4", "5"]);
    // By exactly the height; by one less leaves one row.
    let mut t = base();
    t.write(b"\x1b[3S");
    assert_eq!(rows(&t), ["0", "", "", "", "4", "5"]);
    let mut t = base();
    t.write(b"\x1b[2S");
    assert_eq!(rows(&t), ["0", "3", "", "", "4", "5"]);
    let mut t = base();
    t.write(b"\x1b[2T");
    assert_eq!(rows(&t), ["0", "", "", "1", "4", "5"]);
    let mut t = base();
    t.write(b"\x1b[3T");
    assert_eq!(rows(&t), ["0", "", "", "", "4", "5"]);
    // The whole screen: every row goes to the scrollback.
    let mut t = Term::with_scrollback(4, 3, 10);
    t.write(b"a\r\nb\r\nc\x1b[3S");
    assert_eq!(t.history_len(), 3);
    assert_eq!(rows(&t), ["", "", ""]);
    t.write(b"\x1b[99S");
    assert_eq!(
        t.history_len(),
        6,
        "a count past the height scrolls only the height"
    );
}

#[test]
fn the_cursor_moves_freely_outside_the_region_and_stops_at_its_edges_inside() {
    let at = |text: &[u8]| run(6, 8, text).cursor();
    // Inside the region 3..6 (rows 2..5), up and down stop at its edges.
    assert_eq!(at(b"\x1b[3;6r\x1b[5;1H\x1b[9A"), (0, 2));
    assert_eq!(at(b"\x1b[3;6r\x1b[4;1H\x1b[9B"), (0, 5));
    // Above it, up reaches the top of the screen and down goes as far as the region's bottom.
    assert_eq!(at(b"\x1b[3;6r\x1b[2;1H\x1b[9A"), (0, 0));
    assert_eq!(at(b"\x1b[3;6r\x1b[2;1H\x1b[9B"), (0, 5));
    // Below it, down reaches the bottom of the screen, up goes to the region's top.
    assert_eq!(at(b"\x1b[3;6r\x1b[8;1H\x1b[9B"), (0, 7));
    assert_eq!(at(b"\x1b[3;6r\x1b[8;1H\x1b[9A"), (0, 2));
    // On the edge rows themselves.
    assert_eq!(at(b"\x1b[3;6r\x1b[3;1H\x1b[A"), (0, 2));
    assert_eq!(at(b"\x1b[3;6r\x1b[6;1H\x1b[B"), (0, 5));
    assert_eq!(at(b"\x1b[3;6r\x1b[6;1H\x1b[A"), (0, 4));
    assert_eq!(at(b"\x1b[3;6r\x1b[3;1H\x1b[B"), (0, 3));
    assert_eq!(
        at(b"\x1b[3;6r\x1b[2;1H\x1b[A"),
        (0, 0),
        "from row 1 (0-based) to 0"
    );
}

#[test]
fn a_combining_mark_joins_the_cell_before_it_on_any_row() {
    let t = run(5, 3, "ab\r\ncde\u{301}f\r\n\u{301}g".as_bytes());
    assert_eq!(t.cell(2, 1).marks().count(), 1, "on the second row");
    assert_eq!(t.line(1), "cde\u{301}f");
    assert_eq!(t.cell(0, 2).marks().count(), 0, "at the start of the third");
    assert_eq!(t.line(2), "g");
    assert_eq!(
        t.cell(0, 0).marks().count() + t.cell(1, 0).marks().count(),
        0
    );
}

/// `same_state` sees a change of each thing it names: the tests of the
/// unknown sequences rely on it (mutation testing, 10a).
#[test]
fn same_state_tells_every_difference() {
    let base = || {
        let mut t = Term::with_scrollback(8, 4, 10);
        t.write(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\x1b[2;3H\x1b7\x1b[?1049h\x1b[?1049l");
        t
    };
    assert!(base().same_state(&base()));
    let changes: [&[u8]; 18] = [
        b"x",                       // screen
        b"\x1b[2;2H",               // cursor
        b"\x1b[1m",                 // pen
        b"\x1b[?1h",                // modes
        b"\x1b(0",                  // character set G0
        b"\x1b)0\x0e",              // shift
        b"\x1b[2;3r",               // region
        b"\x1bH\x1b[3g",            // tab stops
        b"\x1b[1;1H\x1b7",          // saved cursor
        b"\x1b]4;3;#112233\x07",    // palette
        b"\x1b[4 q",                // shape
        b"\x1b[?1049h",             // the other screen shown
        b"\r\nx\r\n",               // history and its length
        b"\x1b[?1049hy\x1b[?1049l", // the screen not shown
        b"\x1b[?25l",               // the cursor's visibility is a mode
        b"\x1b[4h",                 // insert
        b"\x1b[6;5H",               // cursor row
        b"12345678",                // a pending wrap
    ];
    for change in changes {
        let mut t = base();
        t.write(change);
        assert!(!t.same_state(&base()), "{change:?}");
    }
    // A different size is a different state.
    let mut t = base();
    t.resize(9, 4);
    assert!(!t.same_state(&base()));
    let mut t = base();
    t.resize(8, 5);
    assert!(!t.same_state(&base()));
}

#[test]
fn memory_is_what_the_cells_and_registers_take_and_its_bound_is_stated() {
    let t = Term::new(80, 24);
    // Two screens of 80 x 24 cells of 16 bytes, 24 dirty flags and 80 tab stops.
    assert_eq!(t.memory(), 2 * 80 * 24 * 16 + 24 + 80);
    // At most: two screens and the full scrollback, the flags and stops, and the
    // longest string.
    assert_eq!(
        t.memory_bound(),
        (2 * 80 * 24 + 80 * 2000) * 16 + 24 + 80 + 4096
    );
    let t = Term::with_scrollback(3, 2, 0);
    assert_eq!(t.memory_bound(), 2 * 3 * 2 * 16 + 2 + 3 + 4096);
    let mut t = Term::new(2, 2);
    t.write(b"\x1b]4;1;#000000");
    assert!(
        t.memory() > 2 * 2 * 2 * 16 + 2 + 2,
        "an OSC being collected is held"
    );
}

#[test]
fn a_resize_resets_the_region_and_keeps_saved_cursors_inside() {
    let mut t = run(
        10,
        6,
        b"\x1b[2;4r\x1b[6;9H\x1b7\x1b[?1049h\x1b[6;9H\x1b7\x1b[?1048h",
    );
    t.resize(5, 3);
    assert_eq!(
        t.scroll_region(),
        (0, 2),
        "the region is the whole screen again"
    );
    t.write(b"\x1b8");
    let (x, y) = t.cursor();
    assert!(x < 5 && y < 3, "restored inside the grid: ({x}, {y})");
    assert_eq!((x, y), (4, 2), "clamped to the corner it was in");
    t.write(b"\x1b[?1048l");
    assert_eq!(t.cursor(), (4, 2));
    // Growing keeps a saved place as it was.
    let mut t = run(10, 6, b"\x1b[2;3H\x1b7");
    t.resize(20, 12);
    t.write(b"\x1b[1;1H\x1b8");
    assert_eq!(t.cursor(), (2, 1));
}

#[test]
fn reinstate_clamps_the_region_to_the_screen() {
    let mut t = run(10, 8, b"\x1b[3;7r");
    let was = t.restore();
    t.resize(10, 4);
    t.reinstate(was);
    let (top, bottom) = t.scroll_region();
    assert!(top <= bottom && bottom < 4, "{top}..{bottom}");
    assert_eq!((top, bottom), (2, 3));
    // A region of one row after the clamp keeps top <= bottom.
    let mut t = run(10, 8, b"\x1b[6;8r");
    let was = t.restore();
    t.resize(10, 3);
    t.reinstate(was);
    assert_eq!(t.scroll_region(), (2, 2));
}

#[test]
fn default_tab_stops_every_eight_and_a_region_must_fit() {
    let t = term(20, 6);
    assert!(t.tab_stop(0) && t.tab_stop(8) && t.tab_stop(16));
    assert!(!t.tab_stop(1) && !t.tab_stop(7) && !t.tab_stop(19));
    assert!(!t.tab_stop(99), "no column, no stop");
    // The bottom of a region is a row of the screen; one past it is refused.
    let mut t = term(10, 6);
    t.write(b"\x1b[2;7r");
    assert_eq!(t.scroll_region(), (0, 5));
    t.write(b"\x1b[2;6r");
    assert_eq!(t.scroll_region(), (1, 5));
    t.write(b"\x1b[5;6r");
    assert_eq!(t.scroll_region(), (4, 5), "two rows are enough");
    t.write(b"\x1b[6;6r");
    assert_eq!(t.scroll_region(), (4, 5), "one is not");
}

#[test]
fn a_full_reset_gives_the_default_tab_stops_back() {
    let mut t = term(20, 2);
    t.write(b"\x1b[3g\x1b[1;5H\x1bH\x1bc");
    for x in 0..20 {
        assert_eq!(t.tab_stop(x), x % 8 == 0, "{x}");
    }
    t.write(b"\t");
    assert_eq!(t.cursor(), (8, 0));
}

#[test]
fn the_cursor_is_drawn_in_each_shape_over_a_wide_character_too() {
    use crate::Look;
    use zoraya_tokens::{Appearance, Scale};
    let mut look = Look::new(Scale::ONE, Appearance::Light);
    let cw = look.cell_width();
    let ch = look.cell_height();
    for (shape, columns_inked) in [(0u8, 2usize), (1, 2), (2, 2)] {
        // A block over the left half and over the right half of a wide character.
        for x in [1usize, 2] {
            let mut t = term(6, 1);
            t.write(std::format!("a中b\x1b[{shape} q\x1b[1;{}H", x + 1).as_bytes());
            let (w, h) = look.surface_size(6, 1);
            let mut pixels = std::vec![0u32; w * h];
            look.clear(&mut pixels);
            t.redraw();
            t.render(&mut look, &mut pixels, w, true);
            let (px, py) = look.cell_origin(1, 0);
            // The glyph (a box) is drawn over the block, in the other colour: most of
            // each half is still the cursor's.
            let inked = (0..2).filter(|cell| {
                let n = (0..ch)
                    .flat_map(|dy| (0..cw).map(move |dx| (dx, dy)))
                    .filter(|(dx, dy)| {
                        pixels[(py + dy) * w + px + cell * cw + dx] == look.fg.pixel()
                    })
                    .count();
                n * 10 >= cw * ch * 6
            });
            assert_eq!(
                inked.count(),
                columns_inked,
                "shape {shape}, cursor on column {x}: both halves are the block"
            );
        }
    }
    // An underline and a bar mark part of the wide cell only.
    for shape in [3u8, 4, 5, 6] {
        let mut t = term(6, 1);
        t.write(std::format!("a中b\x1b[{shape} q\x1b[1;2H").as_bytes());
        let (w, h) = look.surface_size(6, 1);
        let mut pixels = std::vec![0u32; w * h];
        look.clear(&mut pixels);
        t.redraw();
        t.render(&mut look, &mut pixels, w, true);
        let (px, py) = look.cell_origin(1, 0);
        let mark = (0..ch)
            .flat_map(|dy| (0..2 * cw).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| pixels[(py + dy) * w + px + dx] == look.fg.pixel())
            .count();
        assert!(mark > 0 && mark < 2 * cw * ch / 2, "shape {shape}: {mark}");
        // The underline spans both cells of the wide character; the bar is at its left.
        let right_half =
            (0..ch).any(|dy| pixels[(py + dy) * w + px + cw + cw / 2] == look.fg.pixel());
        assert_eq!(right_half, shape <= 4, "shape {shape}");
    }
}

/// The cursor is the block of its own cell only (for a narrow character).
#[test]
fn the_cursor_block_covers_its_own_cell_alone() {
    use crate::Look;
    use zoraya_tokens::{Appearance, Scale};
    let mut look = Look::new(Scale::ONE, Appearance::Light);
    let mut t = term(6, 1);
    t.write(b"abc");
    let (w, h) = look.surface_size(6, 1);
    let mut pixels = std::vec![0u32; w * h];
    look.clear(&mut pixels);
    t.render(&mut look, &mut pixels, w, true);
    let corner = |x: usize| {
        let (px, py) = look.cell_origin(x, 0);
        pixels[py * w + px]
    };
    assert_eq!(
        corner(2),
        look.bg.pixel(),
        "the character before the cursor"
    );
    assert_eq!(corner(3), look.fg.pixel(), "the cursor");
    assert_eq!(corner(4), look.bg.pixel(), "the cell after it");
    // Where a program says it is not shown, no cell is.
    t.write(b"\x1b[?25l");
    t.redraw();
    look.clear(&mut pixels);
    t.render(&mut look, &mut pixels, w, true);
    let (px, py) = look.cell_origin(3, 0);
    assert_eq!(pixels[py * w + px], look.bg.pixel());
}

/// Review of 10a: `CSI 65535 b` must not cost 65,535 characters of work, and
/// its result is that of the full repeat but for the scrollback.
#[test]
fn rep_is_bounded_by_the_screen_and_gives_the_full_result() {
    let mut t = Term::with_scrollback(80, 24, 5000);
    t.write(b"a\x1b[65535b");
    // 65,536 characters end at column 16 of the bottom row; the work was a
    // screenful and a row or two (a few scrolls, not 800).
    assert!(t.history_len() <= 3, "{}", t.history_len());
    assert_eq!(t.line(22).len(), 80);
    assert_eq!(t.line(23), "a".repeat(16));
    assert_eq!(t.cursor(), (16, 23));
    let mut t = Term::with_scrollback(80, 24, 5000);
    for _ in 0..100 {
        t.write(b"\x1b[65535b");
    }
    assert!(t.history_len() <= 100 * 3);
    // A repeat inside a screenful is exact.
    let t = run(10, 3, b"x\x1b[29b");
    assert_eq!(rows(&t), ["xxxxxxxxxx"; 3]);
}

/// `CSI n b` equals `n` plain prints for every count past the bound too, on
/// small grids, for narrow and wide characters, with and without autowrap and
/// insert mode, with and without a region (the scrollback is left out: it is
/// the one thing the bound changes).
#[test]
fn rep_equals_plain_prints_at_every_count() {
    for (columns, rows) in [
        (1usize, 1usize),
        (1, 3),
        (2, 2),
        (3, 2),
        (4, 3),
        (5, 2),
        (7, 3),
        (8, 2),
    ] {
        for text in ["a", "中"] {
            for modes in ["", "\x1b[?7l", "\x1b[4h", "\x1b[2;3r\x1b[?6h", "\x1b[1;31m"] {
                let full = rows * columns + 2 * columns;
                for n in 1..4 * full + 3 {
                    let setup = std::format!("{modes}\x1b[1;2H{text}");
                    let mut by_rep = Term::with_scrollback(columns, rows, 0);
                    by_rep.write(setup.as_bytes());
                    by_rep.write(std::format!("\x1b[{n}b").as_bytes());
                    let mut plain = Term::with_scrollback(columns, rows, 0);
                    plain.write(setup.as_bytes());
                    for _ in 0..n {
                        plain.write(text.as_bytes());
                    }
                    assert!(
                        by_rep.same_state(&plain),
                        "{columns}x{rows} {text:?} {modes:?} n={n}: {:?} vs {:?}, cursor {:?} vs {:?}",
                        rows_of(&by_rep),
                        rows_of(&plain),
                        by_rep.cursor(),
                        plain.cursor()
                    );
                }
            }
        }
    }
}

fn rows_of(t: &Term) -> Vec<String> {
    (0..t.size().1).map(|y| t.line(y)).collect()
}

/// Review of 10a: a `DCS`, `APC`, `PM` or `SOS` longer than the bound is
/// swallowed whole and nothing of it is printed.
#[test]
fn a_long_string_of_any_kind_is_swallowed_whole() {
    for opener in [
        &b"\x1bP"[..],
        b"\x1b_G",
        b"\x1b^",
        b"\x1bX",
        b"\x1bP1;2|",
        b"\x1b]8;;",
    ] {
        let mut t = term(80, 4);
        t.write(b"top\r\n");
        t.write(opener);
        for _ in 0..40 {
            t.write(&[b'Q'; 4096]);
        }
        t.write(b"\x1b\\after");
        assert_eq!(rows(&t), ["top", "after", "", ""], "{opener:?}");
        assert!(t.cursor() == (5, 1));
    }
    // An OSC that overran is not run, even when it ends properly.
    let mut t = term(10, 1);
    let mut s = std::vec![0x1b, b']'];
    s.extend_from_slice(b"4;1;rgb:12/34/56;");
    s.extend(std::iter::repeat_n(b'1', 5000));
    s.push(7);
    t.write(&s);
    t.write(b"\x1b]4;2;#010203\x07");
    assert_eq!(t.palette_override(1), None);
    assert_eq!(t.palette_override(2), Some((1, 2, 3)), "the next one runs");
    // An OSC at exactly the bound runs; the string's own bytes count, not its end.
    let mut t = term(10, 1);
    let mut body = std::vec![b'4', b';', b'1', b';'];
    body.extend_from_slice(b"#010203");
    body.extend(std::iter::repeat_n(b' ', crate::MAX_STRING - body.len()));
    assert_eq!(body.len(), crate::MAX_STRING);
    t.write(b"\x1b]");
    t.write(&body);
    t.write(b"\x07");
    assert_eq!(
        t.palette_override(1),
        None,
        "spaces after the spec make it no colour"
    );
    let mut t = term(10, 1);
    let mut body = std::vec![b'4', b';', b'1', b';'];
    body.extend(std::iter::repeat_n(b'0', 7));
    t.write(b"\x1b]");
    t.write(&body);
    t.write(b"\x07");
    assert_eq!(t.palette_override(1), None);
}

/// Review of 10a: with autowrap off a combining mark joins the character in
/// the last column, not the one before it.
#[test]
fn a_mark_after_a_character_in_the_last_column_without_autowrap() {
    let t = run(4, 1, "\x1b[?7labcd\u{301}".as_bytes());
    assert_eq!(t.cell(3, 0).marks().count(), 1);
    assert_eq!(t.cell(2, 0).marks().count(), 0);
    assert_eq!(t.line(0), "abcd\u{301}");
}

/// Review of 10a (finding 3): the lists of ADR-0332 items 2 and 3, written out
/// here and not taken from the code. The parser acts on exactly these, and the
/// sweep of unknown sequences uses this table to say which are unknown.
pub(crate) const ADR_CSI: &[(u8, u8, u8)] = &[
    // (private marker, intermediate, final); 0 for none.
    // CUU CUD CUF CUB CNL CPL CHA CUP VPA HVP, CHT CBT, ED EL, ECH ICH DCH IL DL REP,
    // SU SD, TBC, SM RM, SGR, DECSTBM.
    (0, 0, b'A'),
    (0, 0, b'B'),
    (0, 0, b'C'),
    (0, 0, b'D'),
    (0, 0, b'E'),
    (0, 0, b'F'),
    (0, 0, b'G'),
    (0, 0, b'H'),
    (0, 0, b'd'),
    (0, 0, b'f'),
    (0, 0, b'I'),
    (0, 0, b'Z'),
    (0, 0, b'J'),
    (0, 0, b'K'),
    (0, 0, b'X'),
    (0, 0, b'@'),
    (0, 0, b'P'),
    (0, 0, b'L'),
    (0, 0, b'M'),
    (0, 0, b'b'),
    (0, 0, b'S'),
    (0, 0, b'T'),
    (0, 0, b'g'),
    (0, 0, b'h'),
    (0, 0, b'l'),
    (0, 0, b'm'),
    (0, 0, b'r'),
    // The modes: DECSET and DECRST.
    (b'?', 0, b'h'),
    (b'?', 0, b'l'),
    // The cursor shape, and the soft reset.
    (0, b' ', b'q'),
    (0, b'!', b'p'),
];

pub(crate) const ADR_ESC: &[(u8, u8)] = &[
    // DECSC DECRC IND NEL HTS RI RIS, the keypad modes.
    (0, b'7'),
    (0, b'8'),
    (0, b'D'),
    (0, b'E'),
    (0, b'H'),
    (0, b'M'),
    (0, b'c'),
    (0, b'='),
    (0, b'>'),
    // G0 and G1: ASCII and the line-drawing set.
    (b'(', b'B'),
    (b'(', b'0'),
    (b')', b'B'),
    (b')', b'0'),
];

#[test]
fn the_parsers_lists_are_the_adr_s() {
    for marker in [0u8, b'?', b'>', b'=', b'<'] {
        for inter in [0u8, b' ', b'!', b'$', b'"', b'\'', b'#'] {
            for fin in 0x40u8..=0x7e {
                let listed = ADR_CSI.contains(&(marker, inter, fin));
                assert_eq!(
                    crate::parse::csi_known(marker, inter, fin),
                    listed,
                    "CSI {} {} {}",
                    marker as char,
                    inter as char,
                    fin as char
                );
            }
        }
    }
    for inter in [0u8, b' ', b'#', b'(', b')', b'*', b'+', b'%', b'$'] {
        for fin in 0x30u8..=0x7e {
            assert_eq!(
                crate::parse::escape_known(inter, fin),
                ADR_ESC.contains(&(inter, fin)),
                "ESC {} {}",
                inter as char,
                fin as char
            );
        }
    }
}

/// Review of 10a (C2): shrinking the scrollback keeps the memory within its bound.
#[test]
fn set_scrollback_keeps_memory_within_the_bound() {
    for (from, to) in [
        (2000usize, 1500usize),
        (2000, 3),
        (1500, 2000),
        (10, 0),
        (0, 10),
    ] {
        let mut t = Term::with_scrollback(80, 24, from);
        for i in 0..from + 100 {
            t.write(std::format!("line {i}\r\n").as_bytes());
        }
        // The `OSC` buffer full, the other part of the bound.
        t.write(b"\x1b]");
        t.write(&std::vec![b'x'; crate::MAX_STRING]);
        t.write(b"\x18");
        t.set_scrollback(to);
        assert!(
            t.memory() <= t.memory_bound(),
            "{from} to {to}: {} > {}",
            t.memory(),
            t.memory_bound()
        );
        for i in 0..to + 50 {
            t.write(std::format!("more {i}\r\n").as_bytes());
        }
        assert!(
            t.memory() <= t.memory_bound(),
            "{from} to {to} after more output"
        );
        assert_eq!(t.history_len(), to.min(t.history_len()));
    }
}

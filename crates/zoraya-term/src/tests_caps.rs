//! The capabilities of the pinned entry, each fed to the terminal as the
//! entry spells it (ADR-0332 items 2 and 3, INV-0332-1). `cap`, `flag` and
//! `num` are the coverage `tools/term-conformance/check.sh` counts.

use std::string::String;
use std::vec::Vec;

use crate::cell::{self, Colour};
use crate::entry::{cap, flag, num};
use crate::{Look, Term};
use zoraya_tokens::{Appearance, Scale};

fn term(columns: usize, rows: usize) -> Term {
    Term::new(columns, rows)
}

fn feed(t: &mut Term, parts: &[&[u8]]) {
    for p in parts {
        t.write(p);
    }
}

fn rows(t: &Term) -> Vec<String> {
    (0..t.size().1).map(|y| t.line(y)).collect()
}

/// A terminal whose row `y` holds `{y}12345678`.
fn filled() -> Term {
    let mut t = term(10, 5);
    for y in 0..5 {
        t.write(&cap("cup", &[y, 0]));
        t.write(std::format!("{y}12345678").as_bytes());
    }
    t.write(&cap("home", &[]));
    t
}

#[test]
fn cursor_motion() {
    let mut t = term(20, 8);
    feed(&mut t, &[&cap("cup", &[4, 9])]);
    assert_eq!(t.cursor(), (9, 4));
    feed(&mut t, &[&cap("home", &[])]);
    assert_eq!(t.cursor(), (0, 0));
    feed(&mut t, &[&cap("cud", &[3]), &cap("cuf", &[5])]);
    assert_eq!(t.cursor(), (5, 3));
    feed(&mut t, &[&cap("cuu", &[2]), &cap("cub", &[4])]);
    assert_eq!(t.cursor(), (1, 1));
    feed(&mut t, &[&cap("cuf1", &[]), &cap("cud1", &[])]);
    assert_eq!(t.cursor(), (2, 2), "cud1 is a line feed: the column stays");
    feed(&mut t, &[&cap("cuu1", &[]), &cap("cub1", &[])]);
    assert_eq!(t.cursor(), (1, 1));
    feed(&mut t, &[&cap("cr", &[])]);
    assert_eq!(t.cursor(), (0, 1));
    feed(&mut t, &[&cap("hpa", &[7]), &cap("vpa", &[6])]);
    assert_eq!(t.cursor(), (7, 6));
    // Moves stop at the edges.
    feed(&mut t, &[&cap("cuf", &[99]), &cap("cud", &[99])]);
    assert_eq!(t.cursor(), (19, 7));
    feed(&mut t, &[&cap("cuu", &[99]), &cap("cub", &[99])]);
    assert_eq!(t.cursor(), (0, 0));
    assert!(
        flag("OTbs"),
        "the entry says backspace is ^H, and it moves left"
    );
    feed(&mut t, &[b"ab", b"\x08", b"c"]);
    assert_eq!(t.line(0), "ac");
}

#[test]
fn line_feed_index_next_line_and_reverse_index() {
    let mut t = term(10, 3);
    feed(
        &mut t,
        &[b"ab", &cap("ind", &[]), b"c", &cap("nel", &[]), b"d"],
    );
    assert_eq!(rows(&t), ["ab", "  c", "d"]);
    // At the bottom `ind` scrolls; `ri` at the top scrolls back down.
    feed(&mut t, &[&cap("ind", &[]), b"e"]);
    assert_eq!(rows(&t), ["  c", "d", " e"]);
    feed(&mut t, &[&cap("home", &[]), &cap("ri", &[]), b"Z"]);
    assert_eq!(rows(&t), ["Z", "  c", "d"]);
}

#[test]
fn save_and_restore_the_cursor_and_its_rendition() {
    let mut t = term(10, 4);
    feed(
        &mut t,
        &[&cap("cup", &[2, 3]), &cap("setaf", &[1]), &cap("sc", &[])],
    );
    feed(
        &mut t,
        &[&cap("home", &[]), &cap("sgr0", &[]), &cap("rc", &[]), b"x"],
    );
    assert_eq!(t.cursor(), (4, 2));
    assert_eq!(
        t.cell(3, 2).ch,
        'x',
        "x is written where the cursor was saved"
    );
    assert_eq!(t.cell(3, 2).fg(), Colour::Index(1), "the pen came back too");
}

#[test]
fn erasing() {
    let mut t = filled();
    feed(&mut t, &[&cap("cup", &[1, 4]), &cap("el", &[])]);
    assert_eq!(t.line(1), "1123");
    feed(&mut t, &[&cap("cup", &[2, 4]), &cap("el1", &[])]);
    assert_eq!(t.line(2), "     5678".trim_end());
    feed(&mut t, &[&cap("cup", &[3, 2]), &cap("ech", &[3])]);
    assert_eq!(t.line(3), "31   5678");
    feed(&mut t, &[&cap("cup", &[3, 0]), &cap("ed", &[])]);
    assert_eq!(rows(&t), ["012345678", "1123", "     5678", "", ""]);
    feed(&mut t, &[&cap("clear", &[])]);
    assert_eq!(rows(&t), ["", "", "", "", ""]);
    assert_eq!(t.cursor(), (0, 0));
}

#[test]
fn erased_cells_take_the_background_bce() {
    assert!(flag("bce"));
    let mut t = term(8, 2);
    feed(
        &mut t,
        &[&cap("setab", &[4]), &cap("el", &[]), &cap("sgr0", &[])],
    );
    assert_eq!(t.cell(5, 0).bg(), Colour::Index(4));
    assert_eq!(t.cell(5, 0).fg(), Colour::Default);
    feed(&mut t, &[&cap("setab", &[2]), &cap("clear", &[])]);
    assert_eq!(t.cell(7, 1).bg(), Colour::Index(2));
}

#[test]
fn inserting_and_deleting_characters() {
    let mut t = filled();
    feed(&mut t, &[&cap("cup", &[0, 2]), &cap("ich", &[3])]);
    assert_eq!(t.line(0), "01   23456");
    feed(&mut t, &[&cap("dch", &[2])]);
    assert_eq!(t.line(0), "01 23456");
    feed(&mut t, &[&cap("dch1", &[])]);
    assert_eq!(t.line(0), "0123456");
}

#[test]
fn inserting_and_deleting_lines() {
    let mut t = filled();
    feed(&mut t, &[&cap("cup", &[1, 3]), &cap("il", &[2])]);
    assert_eq!(rows(&t), ["012345678", "", "", "112345678", "212345678"]);
    assert_eq!(
        t.cursor(),
        (0, 1),
        "il leaves the cursor in the first column"
    );
    let mut t = filled();
    feed(&mut t, &[&cap("cup", &[1, 3]), &cap("dl", &[2])]);
    assert_eq!(rows(&t), ["012345678", "312345678", "412345678", "", ""]);
    let mut t = filled();
    feed(&mut t, &[&cap("cup", &[1, 0]), &cap("il1", &[])]);
    assert_eq!(t.line(1), "");
    assert_eq!(t.line(2), "112345678");
    feed(&mut t, &[&cap("dl1", &[])]);
    assert_eq!(t.line(1), "112345678");
}

#[test]
fn a_scroll_region_scrolls_alone() {
    let mut t = filled();
    feed(
        &mut t,
        &[
            &cap("csr", &[1, 3]),
            &cap("cup", &[1, 0]),
            &cap("indn", &[1]),
        ],
    );
    assert_eq!(
        rows(&t),
        ["012345678", "212345678", "312345678", "", "412345678"]
    );
    feed(&mut t, &[&cap("rin", &[2])]);
    assert_eq!(rows(&t), ["012345678", "", "", "212345678", "412345678"]);
    // A line feed at the region's bottom scrolls the region, not the screen.
    feed(&mut t, &[&cap("cup", &[3, 0]), b"\n", b"x"]);
    assert_eq!(rows(&t), ["012345678", "", "212345678", "x", "412345678"]);
    assert_eq!(t.scroll_region(), (1, 3));
    // The scrolled-out line of a region is not kept as scrollback.
    assert_eq!(t.history_len(), 0);
}

#[test]
fn tab_stops() {
    assert_eq!(num("it"), 8);
    let mut t = term(40, 2);
    feed(&mut t, &[&cap("ht", &[])]);
    assert_eq!(t.cursor(), (8, 0));
    feed(
        &mut t,
        &[
            &cap("cup", &[0, 3]),
            &cap("hts", &[]),
            &cap("home", &[]),
            &cap("ht", &[]),
        ],
    );
    assert_eq!(t.cursor(), (3, 0), "a stop set at column 3");
    feed(&mut t, &[&cap("cbt", &[])]);
    assert_eq!(t.cursor(), (0, 0));
    feed(&mut t, &[&cap("tbc", &[]), &cap("ht", &[])]);
    assert_eq!(t.cursor(), (39, 0), "no stops left: the last column");
    assert!(!t.tab_stop(8));
}

#[test]
fn attributes() {
    // The capability that turns each attribute on, the one that ends it
    // and the attribute, spelled out so that the conformance tool sees them.
    let pairs: [(Vec<u8>, Vec<u8>, u16); 9] = [
        (cap("bold", &[]), cap("sgr0", &[]), cell::BOLD),
        (cap("dim", &[]), cap("sgr0", &[]), cell::FAINT),
        (cap("blink", &[]), cap("sgr0", &[]), cell::BLINK),
        (cap("invis", &[]), cap("sgr0", &[]), cell::INVISIBLE),
        (cap("rev", &[]), cap("sgr0", &[]), cell::REVERSE),
        (cap("smso", &[]), cap("rmso", &[]), cell::REVERSE),
        (cap("smul", &[]), cap("rmul", &[]), cell::UNDERLINE),
        (cap("sitm", &[]), cap("ritm", &[]), cell::ITALIC),
        (cap("smxx", &[]), cap("rmxx", &[]), cell::STRIKE),
    ];
    for (on, off, flag) in pairs {
        let mut t = term(10, 1);
        feed(&mut t, &[&on, b"x", &off, b"y"]);
        assert!(t.cell(0, 0).has(flag), "{on:?}");
        assert!(!t.cell(1, 0).has(flag), "{off:?}");
    }
    // `sgr` sets them all at once, and clears what it does not name.
    let mut t = term(10, 1);
    feed(
        &mut t,
        &[
            &cap("bold", &[]),
            &cap("sgr", &[1, 1, 0, 1, 1, 1, 1, 0, 0]),
            b"x",
        ],
    );
    let all =
        cell::REVERSE | cell::UNDERLINE | cell::BLINK | cell::FAINT | cell::BOLD | cell::INVISIBLE;
    assert_eq!(t.cell(0, 0).attrs(), all);
    feed(&mut t, &[&cap("sgr", &[0; 9]), b"y"]);
    assert_eq!(t.cell(1, 0).attrs(), 0);
}

#[test]
fn colours() {
    assert_eq!(num("colors"), 256);
    let mut t = term(10, 1);
    feed(
        &mut t,
        &[
            &cap("setaf", &[1]),
            b"a",
            &cap("setaf", &[9]),
            b"b",
            &cap("setaf", &[200]),
            b"c",
        ],
    );
    assert_eq!(t.cell(0, 0).fg(), Colour::Index(1));
    assert_eq!(t.cell(1, 0).fg(), Colour::Index(9));
    assert_eq!(t.cell(2, 0).fg(), Colour::Index(200));
    feed(
        &mut t,
        &[
            &cap("setab", &[4]),
            b"d",
            &cap("setab", &[12]),
            b"e",
            &cap("setab", &[255]),
            b"f",
        ],
    );
    assert_eq!(t.cell(3, 0).bg(), Colour::Index(4));
    assert_eq!(t.cell(4, 0).bg(), Colour::Index(12));
    assert_eq!(t.cell(5, 0).bg(), Colour::Index(255));
    // The default pair, the one `AX` promises.
    assert!(flag("AX"));
    feed(&mut t, &[&cap("op", &[]), b"g"]);
    assert_eq!(
        (t.cell(6, 0).fg(), t.cell(6, 0).bg()),
        (Colour::Default, Colour::Default)
    );
    // Redefining and restoring entries: `initc` takes 0 to 1000 a channel.
    assert!(flag("ccc"));
    feed(&mut t, &[&cap("initc", &[17, 1000, 0, 500])]);
    assert_eq!(t.palette_override(17), Some((255, 0, 127)));
    feed(&mut t, &[&cap("initc", &[200, 0, 0, 0])]);
    assert_eq!(t.palette_override(200), Some((0, 0, 0)));
    feed(&mut t, &[&cap("oc", &[])]);
    assert_eq!(
        (t.palette_override(17), t.palette_override(200)),
        (None, None)
    );
}

#[test]
fn the_line_drawing_set() {
    // `acsc` is pairs: the VT100 name of a shape and the character that
    // draws it in the alternate set.
    let pairs = cap("acsc", &[]);
    assert_eq!(pairs.len() % 2, 0);
    let mut t = term(40, 2);
    feed(&mut t, &[&cap("smacs", &[])]);
    for (i, pair) in pairs.chunks(2).enumerate() {
        t.write(&[pair[1]]);
        assert!(
            !t.cell(i, 0).ch.is_ascii(),
            "{} is drawn in the line set",
            pair[1] as char
        );
    }
    feed(&mut t, &[&cap("rmacs", &[]), b"q"]);
    assert_eq!(t.cell(pairs.len() / 2, 0).ch, 'q', "rmacs is ASCII again");
    // `q` is the horizontal line, `x` the vertical, `l`, `k`, `m`, `j` corners.
    let mut t = term(10, 1);
    feed(&mut t, &[&cap("smacs", &[]), b"lqkxmj"]);
    assert_eq!(t.line(0), "┌─┐│└┘");
    // `sgr` selects the set with its ninth parameter.
    let mut t = term(10, 1);
    feed(&mut t, &[&cap("sgr", &[0, 0, 0, 0, 0, 0, 0, 0, 1]), b"q"]);
    assert_eq!(t.cell(0, 0).ch, '─');
    feed(&mut t, &[&cap("sgr0", &[]), b"q"]);
    assert_eq!(t.cell(1, 0).ch, 'q');
}

#[test]
fn the_cursor_and_the_keypad() {
    let mut t = term(10, 2);
    assert!(t.modes().cursor_visible);
    feed(&mut t, &[&cap("civis", &[])]);
    assert!(!t.modes().cursor_visible);
    feed(&mut t, &[&cap("cnorm", &[])]);
    assert!(t.modes().cursor_visible);
    feed(&mut t, &[&cap("civis", &[]), &cap("cvvis", &[])]);
    assert!(t.modes().cursor_visible);
    feed(&mut t, &[&cap("smkx", &[])]);
    assert!(t.modes().app_cursor && t.modes().app_keypad);
    feed(&mut t, &[&cap("rmkx", &[])]);
    assert!(!t.modes().app_cursor && !t.modes().app_keypad);
    // The shape the program asks for.
    feed(&mut t, &[&cap("Ss", &[4])]);
    assert_eq!(t.cursor_shape(), 4);
    feed(&mut t, &[&cap("Se", &[])]);
    assert_eq!(t.cursor_shape(), 2);
    feed(&mut t, &[&cap("Ss", &[6])]);
    assert_eq!(t.cursor_shape(), 6);
    feed(&mut t, &[b"\x1b[9 q"]);
    assert_eq!(
        t.cursor_shape(),
        6,
        "a shape that does not exist changes nothing"
    );
}

#[test]
fn automatic_margins_and_the_pending_wrap() {
    assert!(flag("am") && flag("xenl"));
    let mut t = term(5, 3);
    feed(&mut t, &[b"abcde"]);
    assert_eq!(
        t.cursor(),
        (4, 0),
        "xenl: the cursor waits at the last column"
    );
    feed(&mut t, &[b"f"]);
    assert_eq!(rows(&t), ["abcde", "f", ""]);
    // After a full row, a line end makes one new line, not two.
    let mut t = term(5, 3);
    feed(&mut t, &[b"abcde\r\n", b"g"]);
    assert_eq!(rows(&t), ["abcde", "g", ""]);
    // rmam: no wrap, the last cell is overwritten; smam wraps again.
    let mut t = term(5, 2);
    feed(&mut t, &[&cap("rmam", &[]), b"abcdefg"]);
    assert_eq!(rows(&t), ["abcdg", ""]);
    feed(&mut t, &[&cap("smam", &[]), &cap("home", &[]), b"abcdefg"]);
    assert_eq!(rows(&t), ["abcde", "fg"]);
}

#[test]
fn insert_mode() {
    assert!(flag("mir"), "the cursor may move in insert mode");
    let mut t = term(8, 2);
    feed(
        &mut t,
        &[b"abc", &cap("home", &[]), &cap("smir", &[]), b"XY"],
    );
    assert_eq!(t.line(0), "XYabc");
    // Moving in insert mode inserts nothing.
    feed(&mut t, &[&cap("cuf", &[1]), &cap("cub", &[1])]);
    assert_eq!(t.line(0), "XYabc");
    feed(&mut t, &[&cap("rmir", &[]), &cap("home", &[]), b"Z"]);
    assert_eq!(t.line(0), "ZYabc");
}

#[test]
fn the_alternate_screen() {
    let mut t = term(10, 3);
    feed(&mut t, &[b"one\r\ntwo", &cap("smcup", &[])]);
    assert!(t.on_alternate_screen());
    assert_eq!(rows(&t), ["", "", ""], "the alternate screen starts blank");
    feed(&mut t, &[&cap("cup", &[0, 0]), b"vi screen"]);
    for _ in 0..10 {
        feed(&mut t, &[b"\r\n\r\n\r\nscrolls"]);
    }
    assert_eq!(t.history_len(), 0, "the alternate screen has no scrollback");
    feed(&mut t, &[&cap("rmcup", &[])]);
    assert!(!t.on_alternate_screen());
    assert_eq!(rows(&t), ["one", "two", ""]);
    assert_eq!(t.cursor(), (3, 1), "rmcup brings the cursor back");
}

#[test]
fn bracketed_paste_mouse_and_focus_modes() {
    let mut t = term(10, 2);
    feed(&mut t, &[&cap("BE", &[])]);
    assert!(t.modes().bracketed_paste);
    feed(&mut t, &[&cap("BD", &[])]);
    assert!(!t.modes().bracketed_paste);
    feed(&mut t, &[&cap("XM", &[1])]);
    assert!(t.modes().mouse_press && t.modes().mouse_sgr);
    feed(&mut t, &[&cap("XM", &[0])]);
    assert!(!t.modes().mouse_press && !t.modes().mouse_sgr);
    assert!(flag("XF"));
    feed(&mut t, &[&cap("fe", &[])]);
    assert!(t.modes().focus_events);
    feed(&mut t, &[&cap("fd", &[])]);
    assert!(!t.modes().focus_events);
}

#[test]
fn flashing_the_screen_leaves_it_as_it_was() {
    let mut t = term(10, 2);
    let flash = cap("flash", &[]);
    let (on, _) = flash.split_at(flash.len() / 2);
    feed(&mut t, &[on]);
    assert!(t.modes().reverse_screen);
    feed(&mut t, &[&flash[on.len()..]]);
    assert!(!t.modes().reverse_screen);
}

#[test]
fn the_initialisation_and_reset_strings() {
    let mut t = filled();
    feed(
        &mut t,
        &[
            &cap("csr", &[1, 2]),
            &cap("smir", &[]),
            &cap("smkx", &[]),
            &cap("smso", &[]),
        ],
    );
    feed(&mut t, &[&cap("cup", &[3, 3])]);
    feed(&mut t, &[&cap("is2", &[])]);
    assert_eq!(t.scroll_region(), (0, 4), "is2 starts with a soft reset");
    assert!(!t.modes().insert && !t.modes().app_cursor && !t.modes().app_keypad);
    assert_eq!(t.line(0), "012345678", "a soft reset keeps the screen");
    assert_eq!(t.cursor(), (3, 3));
    t.write(b"x");
    assert_eq!(t.cell(3, 3).attrs(), 0, "and the rendition is the default");
    // rs2 is the same string; rs1 is a full reset, and sets the palette back.
    feed(&mut t, &[&cap("initc", &[9, 1000, 1000, 1000])]);
    feed(&mut t, &[&cap("rs2", &[])]);
    assert!(t.palette_override(9).is_some());
    feed(&mut t, &[&cap("rs1", &[])]);
    assert_eq!(rows(&t), ["", "", "", "", ""]);
    assert_eq!(t.cursor(), (0, 0));
    assert_eq!(t.palette_override(9), None);
}

#[test]
fn clearing_the_scrollback() {
    let mut t = term(10, 2);
    for i in 0..8 {
        feed(&mut t, &[std::format!("{i}\r\n").as_bytes()]);
    }
    assert!(t.history_len() > 0);
    feed(&mut t, &[&cap("E3", &[])]);
    assert_eq!(t.history_len(), 0);
}

#[test]
fn repeating_a_character() {
    let mut t = term(10, 1);
    feed(&mut t, &[&cap("rep", &[i32::from(b'x'), 5])]);
    assert_eq!(t.line(0), "xxxxx");
    feed(&mut t, &[b"\r", b"ab", &cap("rep", &[i32::from(b'b'), 3])]);
    assert_eq!(t.line(0), "abbbb");
}

#[test]
fn the_bell_and_padding_characters_do_nothing() {
    assert!(flag("npc"), "no pad character: NUL is not one");
    let mut t = term(10, 1);
    feed(&mut t, &[b"a", &cap("bel", &[]), b"\0", b"b"]);
    assert_eq!(t.line(0), "ab");
    assert_eq!(t.cursor(), (2, 0));
}

#[test]
fn moving_while_standing_out_changes_nothing() {
    assert!(flag("msgr"));
    let mut t = term(10, 3);
    feed(
        &mut t,
        &[
            &cap("smso", &[]),
            &cap("cup", &[1, 4]),
            &cap("cuu1", &[]),
            &cap("cub1", &[]),
            b"x",
        ],
    );
    assert!(
        t.cell(3, 0).has(cell::REVERSE),
        "the rendition survived the moves"
    );
    assert_eq!(t.cell(4, 1).ch, ' ');
}

#[test]
fn the_default_size_is_the_entrys() {
    assert_eq!((num("cols"), num("lines")), (80, 24));
    let look = Look::new(Scale::ONE, Appearance::Light);
    assert_eq!(look.grid_for(1024, 768), (80, 24));
}

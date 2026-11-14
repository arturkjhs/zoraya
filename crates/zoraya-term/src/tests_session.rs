//! Keys, the pointer, focus, selection and paste as the window runs them
//! (ADR-0332 items 3 and 8, ADR-0383 to ADR-0385; INV-0332-1's mouse and
//! paste capabilities, INV-0332-2's emission points, INV-0332-7).

use std::string::String;
use std::vec;
use std::vec::Vec;

use zoraya_keymap::code;
use zoraya_proto_term::paste::{BEGIN, END, MAX_CHUNK, MAX_PASTE_BYTES};
use zoraya_tokens::{Appearance, Scale};

use crate::entry::cap;
use crate::mouse::{LEFT, MIDDLE, RIGHT};
use crate::{Look, Outgoing, Session, Term};

fn look() -> Look {
    Look::new(Scale::ONE, Appearance::Light)
}

fn session(columns: usize, rows: usize) -> Session {
    Session::new(Term::new(columns, rows))
}

fn input(s: &mut Session) -> Vec<u8> {
    s.take_outbox()
        .into_iter()
        .flat_map(|o| match o {
            Outgoing::Input(b) => b,
            Outgoing::Paste { .. } => panic!("a paste where input was expected"),
        })
        .collect()
}

const SHIFT: u32 = 1;
const CTRL: u32 = 2;
const ALT: u32 = 4;

// ---- the entry's mouse and focus capabilities ---------------------------

/// `xm` is the report's format as `terminfo` writes it (with its own
/// trailing `;`, which xterm's reports do not have), `kmous` its first bytes.
fn xm_of(report: &[u8]) -> Vec<u8> {
    // `ESC [ < b ; x ; y M` is `xm`'s expansion without the `;` before the end.
    let mut v = report[..report.len() - 1].to_vec();
    v.push(b';');
    v.push(report[report.len() - 1]);
    v
}

#[test]
fn a_report_is_the_entrys_xm_and_begins_with_its_kmous() {
    let mut s = session(80, 24);
    s.output(&cap("XM", &[1]));
    assert!(s.term.reports_mouse() && s.term.modes().mouse_sgr);
    s.pointer(5, 3, LEFT, 0);
    let press = input(&mut s);
    // xm's parameters: x and y (counted from 0: `%i` makes them 1), the button, 1 for a press.
    assert_eq!(xm_of(&press), cap("xm", &[5, 3, 0, 1]));
    assert!(press.starts_with(&cap("kmous", &[])));
    s.pointer(5, 3, 0, 0);
    let release = input(&mut s);
    assert_eq!(xm_of(&release), cap("xm", &[5, 3, 0, 0]));
    assert_eq!(release, b"\x1b[<0;6;4m");
}

#[test]
fn focus_in_and_out_are_the_entrys_kxin_and_kxout() {
    let mut s = session(10, 2);
    // Not asked for: nothing.
    s.focus(true);
    s.focus(false);
    assert!(s.outbox_is_empty());
    s.output(&cap("fe", &[]));
    s.focus(true);
    assert_eq!(input(&mut s), cap("kxIN", &[]));
    s.focus(false);
    assert_eq!(input(&mut s), cap("kxOUT", &[]));
    s.output(&cap("fd", &[]));
    s.focus(true);
    assert!(s.outbox_is_empty());
}

// ---- the reports ---------------------------------------------------------

#[test]
fn presses_releases_and_the_buttons_numbers() {
    let mut s = session(80, 24);
    s.output(b"\x1b[?1000h\x1b[?1006h");
    s.pointer(0, 0, LEFT, 0);
    assert_eq!(input(&mut s), b"\x1b[<0;1;1M");
    s.pointer(0, 0, LEFT | MIDDLE, 0);
    assert_eq!(input(&mut s), b"\x1b[<1;1;1M");
    s.pointer(0, 0, LEFT | MIDDLE | RIGHT, 0);
    assert_eq!(input(&mut s), b"\x1b[<2;1;1M");
    s.pointer(79, 23, 0, 0);
    assert_eq!(input(&mut s), b"\x1b[<0;80;24m\x1b[<1;80;24m\x1b[<2;80;24m");
    // With a modifier: Alt 8, Ctrl 16; Shift is never in a report.
    s.pointer(1, 1, LEFT, ALT | CTRL);
    assert_eq!(input(&mut s), b"\x1b[<24;2;2M");
    s.pointer(1, 1, 0, ALT | CTRL);
    assert_eq!(input(&mut s), b"\x1b[<24;2;2m");
}

#[test]
fn motion_is_reported_only_with_1002_and_a_button_down() {
    let mut s = session(80, 24);
    s.output(b"\x1b[?1000h");
    s.pointer(1, 1, LEFT, 0);
    input(&mut s);
    s.pointer(2, 1, LEFT, 0);
    assert!(input(&mut s).is_empty(), "1000 reports no motion");
    s.pointer(2, 1, 0, 0);
    input(&mut s);
    s.output(b"\x1b[?1002h");
    s.pointer(3, 1, 0, 0);
    assert!(
        input(&mut s).is_empty(),
        "no button, no motion (1003 is not supported)"
    );
    s.pointer(3, 1, RIGHT, 0);
    input(&mut s);
    s.pointer(4, 2, RIGHT, 0);
    assert_eq!(input(&mut s), b"\x1b[<34;5;3M", "motion adds 32");
    s.pointer(4, 2, RIGHT, 0);
    assert!(input(&mut s).is_empty(), "the same cell again is no motion");
    s.pointer(4, 2, RIGHT | LEFT, 0);
    input(&mut s);
    s.pointer(5, 2, RIGHT | LEFT, 0);
    assert_eq!(input(&mut s), b"\x1b[<32;6;3M", "the lowest button held");
}

#[test]
fn a_release_is_reported_only_for_a_press_that_was() {
    let mut s = session(80, 24);
    // The press came before the program asked: a release is not a surprise
    // to it.
    s.pointer(1, 1, LEFT, 0);
    s.output(b"\x1b[?1000h");
    s.pointer(1, 1, 0, 0);
    assert!(input(&mut s).is_empty());
    // And a program that stops asking gets nothing more.
    s.pointer(1, 1, LEFT, 0);
    input(&mut s);
    s.output(b"\x1b[?1000l");
    s.pointer(1, 1, 0, 0);
    assert!(input(&mut s).is_empty());
}

#[test]
fn the_wheel_is_reported_to_a_program_that_asked_and_scrolls_otherwise() {
    let mut s = session(80, 24);
    for _ in 0..40 {
        s.output(b"line\r\n");
    }
    // Not asked: the view moves (three lines a notch), nothing is sent.
    s.wheel(2, 2, -120, 0);
    assert!(s.outbox_is_empty());
    assert_eq!(s.term.view(), 3);
    s.output(b"\x1b[?1000h");
    s.wheel(2, 2, -120, 0);
    assert_eq!(input(&mut s), b"\x1b[<64;3;3M", "away from the person");
    s.wheel(2, 2, 120, 0);
    assert_eq!(input(&mut s), b"\x1b[<65;3;3M");
    s.wheel(0, 0, 360, CTRL);
    assert_eq!(input(&mut s), b"\x1b[<81;1;1M\x1b[<81;1;1M\x1b[<81;1;1M");
    // A turn of a few 120ths gathers into a notch; a large one is bounded.
    s.wheel(0, 0, 60, 0);
    assert!(input(&mut s).is_empty());
    s.wheel(0, 0, 60, 0);
    assert_eq!(input(&mut s), b"\x1b[<65;1;1M");
    s.wheel(0, 0, 120 * 50, 0);
    assert_eq!(input(&mut s).len(), 8 * b"\x1b[<65;1;1M".len());
    // Shift scrolls the view even then.
    let view = s.term.view();
    s.wheel(0, 0, -120, SHIFT);
    assert!(s.outbox_is_empty());
    assert!(s.term.view() > view);
}

// ---- selecting ------------------------------------------------------------

fn text_of(s: &Session) -> String {
    s.copy(&look(), 32 * 1024).unwrap_or_default()
}

#[test]
fn a_drag_selects_and_a_click_does_not() {
    let mut s = session(20, 3);
    s.output(b"hello world\r\nsecond line\r\nthird");
    s.pointer(6, 0, LEFT, 0);
    s.pointer(10, 0, LEFT, 0);
    s.pointer(10, 0, 0, 0);
    assert!(s.outbox_is_empty(), "no reporting: nothing is sent");
    assert_eq!(text_of(&s), "world");
    // Across rows, backwards: reading order, a row's end trimmed.
    s.pointer(5, 1, LEFT, 0);
    s.pointer(2, 0, LEFT, 0);
    s.pointer(2, 0, 0, 0);
    assert_eq!(text_of(&s), "llo world\nsecond");
    // A click ends the selection and selects nothing.
    s.pointer(3, 2, LEFT, 0);
    s.pointer(3, 2, 0, 0);
    assert_eq!(s.copy(&look(), 100), None);
    assert!(!s.term.has_selection());
}

#[test]
fn shift_selects_while_a_program_has_the_mouse() {
    let mut s = session(20, 2);
    s.output(b"abcdef");
    s.output(b"\x1b[?1000h");
    s.pointer(1, 0, LEFT, 0);
    assert_eq!(input(&mut s), b"\x1b[<0;2;1M");
    s.pointer(1, 0, 0, 0);
    input(&mut s);
    assert!(!s.term.has_selection());
    s.pointer(1, 0, LEFT, SHIFT);
    s.pointer(3, 0, LEFT, SHIFT);
    s.pointer(3, 0, 0, 0);
    assert!(s.outbox_is_empty(), "a Shift drag is not reported");
    assert_eq!(text_of(&s), "bcd");
}

#[test]
fn what_changes_under_a_selection_ends_it() {
    let mut s = session(20, 3);
    s.output(b"one\r\ntwo\r\n");
    s.pointer(0, 0, LEFT, 0);
    s.pointer(2, 0, LEFT, 0);
    s.pointer(2, 0, 0, 0);
    assert_eq!(text_of(&s), "one");
    // Output on another row leaves it.
    s.output(b"three");
    assert_eq!(text_of(&s), "one");
    // A change on its row ends it, so that what is copied is what was seen.
    s.output(b"\x1b[1;1Hx");
    assert_eq!(s.copy(&look(), 100), None);
    // So does a scroll of the view, and a resize.
    s.pointer(0, 1, LEFT, 0);
    s.pointer(1, 1, LEFT, 0);
    s.pointer(1, 1, 0, 0);
    assert!(s.term.has_selection());
    s.term.scroll_view(1);
    assert!(!s.term.has_selection());
    s.pointer(0, 1, LEFT, 0);
    s.pointer(1, 1, LEFT, 0);
    s.pointer(1, 1, 0, 0);
    s.term.resize(10, 3);
    assert!(!s.term.has_selection());
}

/// INV-0332-7: a space for every cell drawn invisible or with equal
/// foreground and background.
#[test]
fn invisible_cells_are_copied_as_spaces() {
    let mut s = session(30, 3);
    // Row 0: invisible text between two visible words.
    s.output(b"see \x1b[8mSECRET\x1b[28m end");
    // Row 1: the same foreground and background, an index and a direct colour.
    s.output(b"\r\n\x1b[31;41mred\x1b[0m \x1b[38;2;1;2;3;48;2;1;2;3mrgb\x1b[0m ok");
    // Row 2: reversed (seen), faint (seen), and the default colours.
    s.output(b"\r\n\x1b[7mrev\x1b[0m \x1b[2mfaint\x1b[0m plain");
    s.pointer(0, 0, LEFT, 0);
    s.pointer(29, 2, LEFT, 0);
    s.pointer(29, 2, 0, 0);
    let text = text_of(&s);
    // Six hidden letters between the spaces around them; two hidden words
    // and the spaces between and after them; the rest as seen.
    let want = std::format!(
        "see{}end\n{}ok\nrev faint plain",
        " ".repeat(8),
        " ".repeat(8)
    );
    assert_eq!(text, want);
    assert!(!text.contains("SECRET") && !text.contains("red") && !text.contains("rgb"));
}

#[test]
fn a_hidden_run_keeps_its_length_and_is_not_trimmed() {
    let mut s = session(20, 1);
    s.output(b"pw: \x1b[8mhunter2\x1b[0m");
    s.pointer(0, 0, LEFT, 0);
    s.pointer(19, 0, LEFT, 0);
    s.pointer(19, 0, 0, 0);
    assert_eq!(text_of(&s), "pw:        ");
    // The blank cells after it are trimmed, the hidden ones are not.
    assert_eq!(text_of(&s).len(), 4 + 7);
}

#[test]
fn control_characters_in_cells_are_copied_as_spaces() {
    let mut s = session(10, 1);
    // A C1 control and a DEL as characters of the output.
    s.output("a\u{85}b".as_bytes());
    s.pointer(0, 0, LEFT, 0);
    s.pointer(2, 0, LEFT, 0);
    s.pointer(2, 0, 0, 0);
    let text = text_of(&s);
    assert!(
        text.chars().all(|c| !c.is_control() || c == '\n'),
        "{text:?}"
    );
}

#[test]
fn wide_and_combining_characters_are_whole() {
    let mut s = session(10, 1);
    s.output("a\u{4e2d}e\u{301}b".as_bytes());
    // Select from the right half of the wide character to the end.
    s.pointer(2, 0, LEFT, 0);
    s.pointer(9, 0, LEFT, 0);
    s.pointer(9, 0, 0, 0);
    assert_eq!(text_of(&s), "\u{4e2d}e\u{301}b");
    assert!(s.term.selected(1, 0) && s.term.selected(2, 0));
    assert!(!s.term.selected(0, 0));
}

#[test]
fn the_text_copied_is_bounded() {
    let mut s = session(20, 2);
    s.output("ééééééééééééééééééé".as_bytes());
    s.pointer(0, 0, LEFT, 0);
    s.pointer(18, 0, LEFT, 0);
    s.pointer(18, 0, 0, 0);
    let all = s.copy(&look(), 1000).unwrap();
    let cut = s.copy(&look(), 7).unwrap();
    assert!(cut.len() <= 7 && all.starts_with(&cut) && cut.chars().count() == 3);
}

// ---- paste -----------------------------------------------------------------

#[test]
fn the_brackets_are_the_entrys_ps_and_pe() {
    assert_eq!(BEGIN, cap("PS", &[]));
    assert_eq!(END, cap("PE", &[]));
}

#[test]
fn a_paste_goes_in_parts_of_whole_characters_in_order() {
    let mut s = session(80, 24);
    s.paste(b"");
    assert!(s.outbox_is_empty(), "an empty paste sends nothing");
    s.paste(b"hello");
    assert_eq!(
        s.take_outbox(),
        vec![Outgoing::Paste {
            bytes: b"hello".to_vec(),
            last: true
        }]
    );
    // Parts of at most MAX_CHUNK, never inside a character, `last` on the final one.
    let text: String = core::iter::repeat_n('\u{20ac}', 3000).collect();
    s.paste(text.as_bytes());
    let parts = s.take_outbox();
    assert!(parts.len() >= 3);
    let mut joined = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let Outgoing::Paste { bytes, last } = p else {
            panic!("not a paste")
        };
        assert!(bytes.len() <= MAX_CHUNK && core::str::from_utf8(bytes).is_ok());
        assert_eq!(*last, i + 1 == parts.len());
        joined.extend_from_slice(bytes);
    }
    assert_eq!(joined, text.as_bytes());
}

#[test]
fn a_paste_is_bounded_and_invalid_bytes_are_replaced() {
    let mut s = session(80, 24);
    let big = vec![b'x'; MAX_PASTE_BYTES + 5000];
    s.paste(&big);
    let total: usize = s
        .take_outbox()
        .iter()
        .map(|p| match p {
            Outgoing::Paste { bytes, .. } => bytes.len(),
            Outgoing::Input(_) => 0,
        })
        .sum();
    assert_eq!(total, MAX_PASTE_BYTES);
    s.paste(b"a\xffb");
    let Outgoing::Paste { bytes, .. } = &s.take_outbox()[0] else {
        panic!()
    };
    assert_eq!(core::str::from_utf8(bytes).unwrap(), "a\u{fffd}b");
}

/// A paste is never input, even when the text is a sequence and the program has
/// the mouse, paste and focus modes set.
#[test]
fn a_paste_is_not_input_and_is_not_filtered_here() {
    let mut s = session(80, 24);
    s.output(b"\x1b[?2004h\x1b[?1000h\x1b[?1004h");
    s.paste(b"\x1b[201~\x1b[<0;1;1M");
    let out = s.take_outbox();
    assert!(out.iter().all(|o| matches!(o, Outgoing::Paste { .. })));
}

// ---- INV-0332-2: what the terminal sends comes from gestures only -----------

/// Every query the entry or xterm's document defines, and noise, as output:
/// nothing is sent. Sending starts at a gesture.
#[test]
fn output_never_produces_anything_to_send() {
    let queries: &[&[u8]] = &[
        b"\x1b[5n",
        b"\x1b[6n",
        b"\x1b[?6n",
        b"\x1b[c",
        b"\x1b[>c",
        b"\x1b[=c",
        b"\x1bZ",
        b"\x1b[>q",
        b"\x1b[?1$p",
        b"\x1b[?2004$p",
        b"\x1b[18t",
        b"\x1b[14t",
        b"\x1b[21t",
        b"\x1b]10;?\x07",
        b"\x1b]11;?\x1b\\",
        b"\x1b]4;1;?\x07",
        b"\x1b]52;c;?\x07",
        b"\x1b]0;title\x07",
        b"\x1bP$qm\x1b\\",
        b"\x1bP+q544e\x1b\\",
        b"\x05",
        b"\x1b[?1000h\x1b[?1004h",
        b"\x1b[?2004h",
        b"\x1b[?1002h\x1b[?1006h",
        b"\x1b[?1049h\x1b[?1049l",
    ];
    let mut s = session(80, 24);
    for q in queries {
        s.output(q);
        assert!(s.outbox_is_empty(), "{q:?}");
    }
    // Noise, from a generator that is the same every run.
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..2000 {
        let mut chunk = Vec::new();
        for _ in 0..64 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            chunk.push(x as u8);
        }
        s.output(&chunk);
        assert!(s.outbox_is_empty());
    }
    // Then the gestures send, and only they: a key, the pointer (with a mode a
    // program set), focus, a paste.
    s.output(b"\x1b[?1000h\x1b[?1004h");
    s.key(30, 0, Some('a'));
    s.pointer(0, 0, LEFT, 0);
    s.focus(true);
    s.paste(b"p");
    assert_eq!(s.take_outbox().len(), 4);
}

/// The only emission points are the gesture methods: the same session given
/// the same gestures sends the same whatever output came between them.
#[test]
fn what_is_sent_depends_on_gestures_and_modes_not_on_output_text() {
    let gestures = |s: &mut Session| {
        s.key(30, 0, Some('a'));
        s.key(code::UP, CTRL, None);
        s.pointer(2, 2, LEFT, 0);
        s.pointer(2, 2, 0, 0);
        s.wheel(1, 1, -120, 0);
        s.focus(false);
        s.paste(b"text");
        s.take_outbox()
    };
    let mut quiet = session(40, 10);
    let mut loud = session(40, 10);
    loud.output(b"\x1b[6n\x1b[c\x1b]52;c;?\x07 lots of ordinary text \x1b[31mred");
    assert_eq!(gestures(&mut quiet), gestures(&mut loud));
}

#[test]
fn keys_go_through_the_modes_and_shift_page_keys_scroll() {
    let mut s = session(20, 10);
    for _ in 0..30 {
        s.output(b"x\r\n");
    }
    s.key(code::UP, 0, None);
    assert_eq!(input(&mut s), b"\x1b[A");
    s.output(&cap("smkx", &[]));
    s.key(code::UP, 0, None);
    assert_eq!(input(&mut s), b"\x1bOA");
    s.key(code::KP5, 0, None);
    assert_eq!(input(&mut s), b"\x1bOu");
    s.output(&cap("rmkx", &[]));
    s.key(code::KP5, 0, None);
    assert_eq!(input(&mut s), b"5");
    s.key(code::PAGE_UP, SHIFT, None);
    assert!(s.outbox_is_empty());
    assert_eq!(s.term.view(), 5);
    s.key(code::PAGE_DOWN, SHIFT, None);
    assert_eq!(s.term.view(), 0);
    s.key(code::PAGE_UP, SHIFT | CTRL, None);
    assert_eq!(input(&mut s), b"\x1b[5;6~");
}

#[test]
fn a_cell_is_found_under_a_point_of_the_surface() {
    let l = look();
    let (w, h) = (l.cell_width(), l.cell_height());
    let p = l.pad;
    assert_eq!(l.cell_at(0, 0, 80, 24), (0, 0), "padding is the first cell");
    assert_eq!(l.cell_at(p + w, p + h, 80, 24), (1, 1));
    assert_eq!(l.cell_at(p + w - 1, p + h - 1, 80, 24), (0, 0));
    assert_eq!(l.cell_at(10_000, 10_000, 80, 24), (79, 23));
    assert_eq!(l.cell_at(10_000, 10_000, 0, 0), (0, 0));
}

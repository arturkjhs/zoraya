//! Totality and bounds (INV-0096-1, INV-0096-3, INV-0332-3): whatever the
//! terminal is written, it does not panic, the cursor stays in the grid, the
//! grids stay well formed, memory stays within its bound, and a well-formed
//! sequence outside the ADR's lists changes nothing.

use std::string::{String, ToString};
use std::vec::Vec;

use crate::cell::{TAIL, WIDE};
use crate::tests_parser::{ADR_CSI, ADR_ESC};
use crate::{Look, MAX_STRING, Term};
use zoraya_tokens::{Appearance, Scale};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// What must hold of any terminal after any input.
fn check(t: &Term) {
    let (columns, rows) = t.size();
    let (x, y) = t.cursor();
    assert!(
        x < columns && y < rows,
        "the cursor ({x}, {y}) is outside {columns} x {rows}"
    );
    let (top, bottom) = t.scroll_region();
    assert!(
        top < bottom || (top == bottom && rows == 1),
        "region {top}..{bottom}"
    );
    assert!(bottom < rows);
    assert!(t.view() <= t.history_len());
    assert!(t.history_len() <= t.scrollback());
    assert_eq!(t.history.len(), t.kept * columns);
    assert_eq!(t.tabs.len(), columns);
    assert_eq!(t.dirty.len(), rows);
    assert_eq!(
        (t.screen.len(), t.other.len()),
        (columns * rows, columns * rows)
    );
    assert!(
        t.memory() <= t.memory_bound(),
        "{} > {}",
        t.memory(),
        t.memory_bound()
    );
    assert!(t.osc.len() <= MAX_STRING);
    // Every left half of a wide character has its right half, and no right
    // half stands alone: on both screens and in the history.
    let paired = |row: &[crate::Cell]| {
        (0..row.len()).all(|i| {
            let head = row[i].has(WIDE);
            let tail = row[i].has(TAIL);
            let next_tail = i + 1 < row.len() && row[i + 1].has(TAIL);
            let prev_head = i > 0 && row[i - 1].has(WIDE);
            !(head && tail) && (!head || next_tail) && (!tail || prev_head)
        })
    };
    for grid in [&t.screen, &t.other, &t.history] {
        for row in grid.chunks(columns) {
            assert!(
                paired(row),
                "a wide character was cut in {:?}",
                row.iter().map(|c| c.ch).collect::<String>()
            );
        }
    }
}

const WORDS: [&[u8]; 40] = [
    b"\x1b[",
    b"\x1b]",
    b"\x1bP",
    b"\x1b_",
    b"\x1b^",
    b"\x1bX",
    b"\x1b",
    b"\x1b\\",
    b"\x07",
    b"\n",
    b"\r",
    b"\x08",
    b"\t",
    b"\x0e",
    b"\x0f",
    b"\x18",
    b"\x1a",
    b"\xe2\x82",
    b"\xf0\x9f",
    b"\xc3",
    b";",
    b":",
    b"?",
    b">",
    b"<",
    b"=",
    b"!",
    b" ",
    b"$",
    b"9999999999",
    b"1",
    b"0",
    b"38;5;",
    b"38:2::",
    b"m",
    b"H",
    b"h",
    b"l",
    b"4",
    b"2004",
];

const TEXTS: [&str; 10] = [
    "a",
    "中",
    "e\u{301}",
    "😀",
    "ü",
    "\u{fffd}",
    "x",
    "\u{200d}",
    "ｱ",
    "\u{e0100}",
];

fn chunk(rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..rng.below(48) {
        match rng.below(10) {
            0..=3 => out.extend_from_slice(rng.pick(&WORDS)),
            4 => out.extend_from_slice(rng.pick(&TEXTS).as_bytes()),
            5 => {
                // A whole sequence of a known family.
                let finals = b"@ABCDEFGHIJKLMPSTXZbdfghlmrq";
                out.extend_from_slice(b"\x1b[");
                if rng.below(4) == 0 {
                    out.push(*rng.pick(b"?<=>"));
                }
                for _ in 0..rng.below(5) {
                    out.extend_from_slice(rng.below(300).to_string().as_bytes());
                    out.push(*rng.pick(b";:;"));
                }
                if rng.below(5) == 0 {
                    out.push(*rng.pick(b" !$\""));
                }
                out.push(*rng.pick(finals));
            }
            6 => {
                // An OSC or DCS with an end.
                out.extend_from_slice(rng.pick(&[&b"\x1b]"[..], b"\x1bP", b"\x1b_"]));
                for _ in 0..rng.below(20) {
                    out.push(rng.below(256) as u8);
                }
                out.extend_from_slice(rng.pick(&[&b"\x07"[..], b"\x1b\\", b""]));
            }
            _ => out.push(rng.below(256) as u8),
        }
    }
    out
}

/// INV-0096-1: random and structured bytes never panic and keep the cursor
/// inside the grid; the grids stay well formed; memory stays bounded.
#[test]
fn the_parser_is_total_over_random_and_structured_bytes() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut look = Look::new(Scale::ONE, Appearance::Light);
    for (columns, rows, scrollback) in [
        (1, 1, 3),
        (2, 1, 0),
        (1, 5, 4),
        (3, 50, 10),
        (7, 3, 5),
        (80, 24, 50),
    ] {
        let mut t = Term::with_scrollback(columns, rows, scrollback);
        let (w, h) = look.surface_size(columns, rows);
        let mut pixels = std::vec![0u32; w * h];
        for step in 0..1500 {
            let bytes = chunk(&mut rng);
            t.write(&bytes);
            check(&t);
            match rng.below(40) {
                0 => t.scroll_view(rng.below(21) as isize - 10),
                1 => {
                    let was = t.restore();
                    check(&t);
                    if rng.below(2) == 0 {
                        t.reinstate(was);
                    }
                }
                2 => {
                    // A resize and a new surface.
                    let (c, r) = (1 + rng.below(9) as usize, 1 + rng.below(9) as usize);
                    t.resize(c, r);
                    pixels = std::vec![0u32; look.surface_size(c, r).0 * look.surface_size(c, r).1];
                }
                3 => t.set_scrollback(if rng.below(4) == 0 {
                    rng.below(3000) as usize
                } else {
                    rng.below(12) as usize
                }),
                _ => {}
            }
            check(&t);
            if step % 7 == 0 {
                let (columns, rows) = t.size();
                let stride = look.surface_size(columns, rows).0;
                if pixels.len() < stride * look.surface_size(columns, rows).1 {
                    pixels = std::vec![0u32; stride * look.surface_size(columns, rows).1];
                }
                let _ = t.render(&mut look, &mut pixels, stride, rng.below(2) == 0);
            }
            // CAN ends whatever is going on: the next character is text.
            if step % 50 == 0 {
                let mut probe = t.clone();
                probe.write(b"\x18\x18\x1b\\Q");
                assert!(
                    probe.screen.iter().any(|c| c.ch == 'Q'),
                    "stuck after {bytes:?}"
                );
            }
        }
    }
}

/// A terminal in a state where any change would show: a scribbled screen, a
/// region, modes, a pen, tab stops, a saved cursor and a palette entry.
fn scribbled(alt: bool) -> Term {
    let mut t = Term::with_scrollback(12, 6, 20);
    for i in 0..10 {
        t.write(std::format!("line {i}\r\n").as_bytes());
    }
    t.write(b"\x1b[2;5r\x1b[?7h\x1b[?25l\x1b[?1h\x1b[1;4;31;42m\x1b(0\x1b7\x1b[3;4HQ\x1bH");
    t.write(b"\x1b]4;9;rgb:1/2/3\x07\x1b[5 q\x1b[?2004h\x1b[?1000h\x1b[3;3H");
    if alt {
        t.write(b"\x1b[?1049h\x1b[3;3Hzzz");
    }
    t.write(b"\x1b(B");
    t
}

/// INV-0332-3: a well-formed sequence outside items 2 and 3 of ADR-0332
/// changes no cell, no mode and no cursor position.
#[test]
fn an_unknown_control_sequence_changes_nothing() {
    for alt in [false, true] {
        let base = scribbled(alt);
        let mut count = 0;
        // CSI: every private marker, up to two intermediates, every final.
        let markers: [Option<u8>; 5] = [None, Some(b'?'), Some(b'>'), Some(b'='), Some(b'<')];
        let intermediates: [&[u8]; 6] = [b"", b" ", b"!", b"$", b"\"", b" !"];
        let params = ["", "0", "1", "1;2", "38;5;9", "99999", "1:2:3", ";", "2;;3"];
        for marker in markers {
            for inter in intermediates {
                for fin in 0x40u8..=0x7e {
                    let first = inter.first().copied().unwrap_or(0);
                    if inter.len() < 2 && ADR_CSI.contains(&(marker.unwrap_or(0), first, fin)) {
                        continue;
                    }
                    for p in params {
                        let mut t = base.clone();
                        let mut seq = std::vec![0x1b, b'['];
                        seq.extend(marker);
                        seq.extend_from_slice(p.as_bytes());
                        seq.extend_from_slice(inter);
                        seq.push(fin);
                        t.write(&seq);
                        assert!(
                            t.same_state(&base),
                            "{:?} changed the terminal",
                            String::from_utf8_lossy(&seq)
                        );
                        count += 1;
                    }
                }
            }
        }
        // ESC: every final, with none and with one or two intermediates.
        for inter in [&b""[..], b" ", b"#", b"(", b")", b"*", b"%", b"(%", b"  "] {
            for fin in 0x30u8..=0x7e {
                if matches!(fin, b'[' | b']' | b'P' | b'X' | b'^' | b'_') && inter.is_empty() {
                    continue;
                }
                let first = inter.first().copied().unwrap_or(0);
                if inter.len() < 2 && ADR_ESC.contains(&(first, fin)) {
                    continue;
                }
                let mut t = base.clone();
                let mut seq = std::vec![0x1b];
                seq.extend_from_slice(inter);
                seq.push(fin);
                t.write(&seq);
                assert!(
                    t.same_state(&base),
                    "{:?} changed the terminal",
                    String::from_utf8_lossy(&seq)
                );
                count += 1;
            }
        }
        // OSC other than 4 and 104, with both terminators; DCS, APC, PM, SOS.
        for n in (0..300).filter(|n| *n != 4 && *n != 104) {
            for end in [&b"\x07"[..], b"\x1b\\"] {
                let mut t = base.clone();
                t.write(std::format!("\x1b]{n};some;text").as_bytes());
                t.write(end);
                assert!(t.same_state(&base), "OSC {n}");
                count += 1;
            }
        }
        for opener in [
            &b"\x1bP"[..],
            b"\x1bP1;2|",
            b"\x1bP$q",
            b"\x1bP+q",
            b"\x1b_",
            b"\x1b^",
            b"\x1bX",
        ] {
            let mut t = base.clone();
            t.write(opener);
            t.write(b"payload with ; and \x07 and [31m inside\x01");
            t.write(b"\x1b\\");
            assert!(t.same_state(&base), "{opener:?}");
            count += 1;
        }
        assert!(count > 5000, "{count}");
    }
}

/// What an unknown sequence consumes, it consumes whole: the text after it
/// prints, and an unknown sequence cut by a control or an `ESC` is handled
/// as the sequence the `ESC` starts.
#[test]
fn text_after_an_unknown_sequence_is_text() {
    for seq in [
        &b"\x1b[?9999z"[..],
        b"\x1b[>1;2c",
        b"\x1b]9999;x\x07",
        b"\x1bP1;2|data\x1b\\",
        b"\x1b(Z",
        b"\x1b#8",
        b"\x1b[ !\"p",
    ] {
        let mut t = Term::new(10, 2);
        t.write(seq);
        t.write(b"ok");
        assert_eq!(t.line(0), "ok", "{seq:?}");
    }
    let mut t = Term::new(10, 2);
    t.write(b"\x1b[?9\x1b[31mx");
    assert_eq!(t.cell(0, 0).fg(), crate::Colour::Index(1));
    t.write(b"\x1b]0;title\x1b[1my");
    assert!(t.cell(1, 0).has(crate::cell::BOLD));
}

/// INV-0096-3: memory is bounded whatever the output, and a string or a
/// sequence that never ends is cut at its bound.
#[test]
fn memory_stays_within_its_bound() {
    let mut t = Term::new(80, 24);
    let bound = t.memory_bound();
    let mut rng = Rng(77);
    for _ in 0..300 {
        let mut big = Vec::new();
        for _ in 0..4096 {
            big.push(rng.below(256) as u8);
        }
        t.write(&big);
        t.write(&[0x1b, b']']);
        t.write(&std::vec![b'x'; 10_000]);
        assert!(t.memory() <= bound);
    }
    check(&t);
    // A megabyte of an unterminated string of each kind.
    for opener in [&b"\x1b]"[..], b"\x1bP", b"\x1b_", b"\x1b[", b"\x1b[?"] {
        let mut t = Term::new(80, 24);
        t.write(opener);
        for _ in 0..256 {
            t.write(&[b'1'; 4096]);
        }
        assert!(t.memory() <= t.memory_bound());
        t.write(b"\r\nafter");
        let swallowed = !(0..24).any(|y| t.line(y).ends_with("after"));
        // A string is swallowed whole (a sixel is tens of KiB); a sequence that is
        // not a string is cut at its bound and what follows is text.
        assert_eq!(swallowed, !opener.starts_with(b"\x1b["), "{opener:?}");
        t.write(b"\x18\r\nafter");
        assert!(
            (0..24).any(|y| t.line(y).ends_with("after")),
            "CAN ends it: {opener:?}"
        );
    }
}

/// The sizes ADR-0332 item 1 names: 32 parameters, a string of 4096 bytes.
#[test]
fn the_bounds_are_the_adr_s() {
    assert_eq!(crate::MAX_PARAMS, 32);
    assert_eq!(MAX_STRING, 4096);
    assert_eq!(crate::MAX_SCROLLBACK, 10_000);
    assert_eq!(crate::SCROLLBACK, 2_000);
    assert_eq!(crate::SYNC_HOLD_MS, 150);
}

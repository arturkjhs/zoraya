//! Host tests of the line editor (ADR-0154).

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::editor::*;

struct Fake {
    names: Vec<&'static str>,
    completion: Completion,
}

impl Assist for Fake {
    fn known(&mut self, name: &str) -> bool {
        self.names.contains(&name)
    }

    fn incomplete(&mut self, text: &str) -> bool {
        matches!(
            crate::syntax::parse(text),
            Err(crate::syntax::Error::Incomplete)
        )
    }

    fn complete(&mut self, line: &str, cursor: usize) -> Completion {
        let start = line[..cursor].rfind(' ').map_or(0, |n| n + 1);
        let word = &line[start..cursor];
        let mut c = self.completion.clone();
        c.start = start;
        c.candidates.retain(|(t, _)| t.starts_with(word));
        c
    }
}

fn fake() -> Fake {
    Fake {
        names: vec!["echo", "ls", "cp", "make"],
        completion: Completion::default(),
    }
}

fn editor() -> (Editor, String) {
    let mut e = Editor::new(Options::default());
    let mut out = String::new();
    e.start("~> ", &mut out);
    (e, out)
}

/// Feeds `keys`, giving the screen output and the last key's result.
fn feed(e: &mut Editor, a: &mut Fake, keys: &[u8]) -> (String, Key) {
    let mut out = String::new();
    let mut last = Key::Edited;
    for &k in keys {
        last = e.key(k, &mut out, a);
    }
    (out, last)
}

/// The screen as text, after a simple terminal 80 columns wide (see
/// [`screen_wide`]). Gives the last line.
pub(crate) fn screen(bytes: &str) -> String {
    screen_lines(bytes).pop().unwrap_or_default()
}

/// [`screen`]'s every line.
pub(crate) fn screen_lines(bytes: &str) -> Vec<String> {
    screen_wide(bytes, 80)
}

/// The rows of a terminal `width` columns wide after `bytes`: `\r` to
/// column 0, `\n` to the next row's start, `\x08` and `ESC [ n D` left,
/// `ESC [ n C` right, `ESC [ n A` up, `ESC [ K` clears to the row's end,
/// `ESC [ J` to the screen's end, `ESC [ H` home; a character past the last
/// column goes to the next row first (a terminal that waits to wrap). SGR
/// is ignored.
pub(crate) fn screen_wide(bytes: &str, width: usize) -> Vec<String> {
    let mut rows: Vec<Vec<char>> = vec![Vec::new()];
    let (mut row, mut col) = (0usize, 0usize);
    let mut chars = bytes.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => col = 0,
            '\n' => {
                row += 1;
                col = 0;
            }
            '\x08' => col = col.min(width - 1).saturating_sub(1),
            '\x1b' => {
                chars.next(); // [
                let mut n = String::new();
                let mut fin = ' ';
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        fin = d;
                        break;
                    }
                    n.push(d);
                }
                let n: usize = n.parse().unwrap_or(1);
                match fin {
                    'A' => row = row.saturating_sub(n),
                    'C' => col = (col + n).min(width - 1),
                    'D' => col = col.min(width - 1).saturating_sub(n),
                    'H' => (row, col) = (0, 0),
                    'K' | 'J' => {
                        if let Some(r) = rows.get_mut(row) {
                            r.truncate(col);
                        }
                        if fin == 'J' {
                            rows.truncate(row + 1);
                        }
                    }
                    _ => {}
                }
            }
            c => {
                if col >= width {
                    row += 1;
                    col = 0;
                }
                while rows.len() <= row {
                    rows.push(Vec::new());
                }
                let line = &mut rows[row];
                while line.len() < col {
                    line.push(' ');
                }
                if col < line.len() {
                    line[col] = c;
                } else {
                    line.push(c);
                }
                col += 1;
            }
        }
        while rows.len() <= row {
            rows.push(Vec::new());
        }
    }
    rows.iter().map(|l| l.iter().collect()).collect()
}

pub(crate) fn without_sgr(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            let rest: String = chars.clone().take(8).collect();
            if let Some(end) = rest.find(|c: char| c.is_ascii_alphabetic())
                && rest.as_bytes()[end] == b'm'
            {
                for _ in 0..=end {
                    chars.next();
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[test]
fn typing_and_enter() {
    let (mut e, mut out) = editor();
    let mut a = fake();
    let (o, k) = feed(&mut e, &mut a, b"echo hi\r");
    out.push_str(&o);
    assert_eq!(k, Key::Line("echo hi".into()));
    assert!(out.ends_with('\n'));
    let shown = without_sgr(&out);
    assert_eq!(screen(&shown[..shown.len() - 1]), "~> echo hi");
}

#[test]
fn editing_keys() {
    let (mut e, _) = editor();
    let mut a = fake();
    feed(&mut e, &mut a, b"echo wxyld");
    feed(&mut e, &mut a, b"\x1b[D\x1b[D\x7f\x7f");
    feed(&mut e, &mut a, b"or");
    assert_eq!(e.text(), "echo world");
    feed(&mut e, &mut a, &[CTRL_A]);
    assert_eq!(e.cursor(), 0);
    feed(&mut e, &mut a, &[CTRL_E, CTRL_W]);
    assert_eq!(e.text(), "echo ");
    feed(&mut e, &mut a, b"a b\x1b[D\x1b[D");
    feed(&mut e, &mut a, &[CTRL_K]);
    assert_eq!(e.text(), "echo a");
    feed(&mut e, &mut a, &[CTRL_U]);
    assert_eq!(e.text(), "");
    feed(&mut e, &mut a, b"xy\x1b[H\x1b[3~");
    assert_eq!(e.text(), "y");
    feed(&mut e, &mut a, b"\x1b[F\x1bOD\x1b[1~z");
    assert_eq!(e.text(), "zy");
}

#[test]
fn utf8_is_kept_and_bad_bytes_refused() {
    let (mut e, _) = editor();
    let mut a = fake();
    feed(&mut e, &mut a, "echo café".as_bytes());
    assert_eq!(e.text(), "echo café");
    feed(&mut e, &mut a, b"\x7f");
    assert_eq!(e.text(), "echo caf");
    let (out, _) = feed(&mut e, &mut a, &[0xff]);
    assert!(out.contains("not UTF-8"));
    assert_eq!(e.text(), "echo caf");
    feed(&mut e, &mut a, &[0xc3]);
    feed(&mut e, &mut a, &[CTRL_C]);
    assert_eq!(e.text(), "");
}

#[test]
fn history_with_prefix_search() {
    let (mut e, _) = editor();
    let mut a = fake();
    for h in ["make -C src", "ls", "make test", "ls"] {
        e.remember(h);
    }
    assert_eq!(
        e.history(),
        ["make -C src", "make test", "ls"],
        "kept once, at the end"
    );
    feed(&mut e, &mut a, b"ma\x1b[A");
    assert_eq!(e.text(), "make test");
    feed(&mut e, &mut a, b"\x1b[A");
    assert_eq!(e.text(), "make -C src");
    feed(&mut e, &mut a, b"\x1b[A");
    assert_eq!(e.text(), "make -C src", "the oldest stays");
    feed(&mut e, &mut a, b"\x1b[B\x1b[B");
    assert_eq!(e.text(), "ma");
}

#[test]
fn ctrl_r_finds_and_accepts() {
    let (mut e, _) = editor();
    let mut a = fake();
    for h in ["tcc main.c -o main", "ls", "grants tcc main.c"] {
        e.remember(h);
    }
    let (out, _) = feed(&mut e, &mut a, &[CTRL_R]);
    assert!(out.contains("search: "));
    feed(&mut e, &mut a, b"main");
    let (_, k) = feed(&mut e, &mut a, &[CTRL_R]);
    assert_eq!(k, Key::Edited);
    let (_, k) = feed(&mut e, &mut a, b"\r");
    assert_eq!(k, Key::Line("tcc main.c -o main".into()));
}

#[test]
fn suggestions_are_faint_accepted_by_right_and_erased_before_enter() {
    let (mut e, _) = editor();
    let mut a = fake();
    e.remember("make -C src");
    let (out, _) = feed(&mut e, &mut a, b"mak");
    assert!(out.contains("\x1b[2me -C src\x1b[22m"), "{out:?}");
    assert_eq!(screen(&without_sgr(&out)), "~> make -C src");
    // Enter without it runs `mak`, and the screen keeps only `mak`.
    let (out2, k) = feed(&mut e, &mut a, b"\r");
    assert_eq!(k, Key::Line("mak".into()));
    let all = without_sgr(&(out + &out2));
    assert_eq!(screen(&all[..all.len() - 1]), "~> mak");
    let mut out = String::new();
    e.start("~> ", &mut out);
    feed(&mut e, &mut a, b"mak\x1b[C");
    assert_eq!(e.text(), "make -C src");
    e.options.suggestions = false;
    let mut out = String::new();
    e.start("~> ", &mut out);
    let (out, _) = feed(&mut e, &mut a, b"mak");
    assert!(!out.contains("\x1b[2m"));
}

#[test]
fn unknown_commands_and_open_quotes_are_red() {
    let (mut e, _) = editor();
    let mut a = fake();
    let (out, _) = feed(&mut e, &mut a, b"sl");
    assert!(out.contains("\x1b[31msl\x1b[m"), "{out:?}");
    let (mut e, _) = editor();
    let (out, _) = feed(&mut e, &mut a, b"ls");
    let last = &out[out.rfind('\r').unwrap()..];
    assert!(!last.contains("\x1b[31m"), "{last:?}");
    let (mut e, _) = editor();
    let (out, _) = feed(&mut e, &mut a, b"echo 'open");
    assert!(
        out.ends_with("\x1b[31m'open\x1b[m\x1b[K") || out.contains("\x1b[31m'open"),
        "{out:?}"
    );
    let (mut e, _) = editor();
    let (out, _) = feed(&mut e, &mut a, b"+r ~/include make");
    let last = &out[out.rfind('\r').unwrap()..];
    assert!(!last.contains("\x1b[31m"), "{last:?}");
    e.options.colour = false;
    let (mut e2, _) = editor();
    e2.options.colour = false;
    let (out, _) = feed(&mut e2, &mut a, b"sl");
    assert!(!out.contains('\x1b') || !out.contains("31m"));
}

#[test]
fn completion_of_options_is_the_tour() {
    let (mut e, _) = editor();
    let mut a = fake();
    a.completion.candidates = vec![
        ("--force".into(), Some("replace TO if it exists".into())),
        ("--help".into(), Some("show this help".into())),
    ];
    let (o1, _) = feed(&mut e, &mut a, b"cp --");
    let (o2, _) = feed(&mut e, &mut a, b"\t");
    let all = without_sgr(&(o1 + &o2));
    let text: Vec<String> = {
        // Lines as a terminal leaves them.
        let mut lines = Vec::new();
        for part in all.split('\n') {
            lines.push(screen(part));
        }
        lines
    };
    assert_eq!(
        text,
        [
            "~> cp --",
            "--force    replace TO if it exists",
            "--help     show this help",
            "~> cp --"
        ]
    );
    // A second Tab does nothing more.
    let (o3, _) = feed(&mut e, &mut a, b"\t");
    assert!(!o3.contains('\n'));
    // One match is inserted, with a space.
    feed(&mut e, &mut a, b"f\t");
    assert_eq!(e.text(), "cp --force ");
}

#[test]
fn completion_of_paths() {
    let (mut e, _) = editor();
    let mut a = fake();
    a.completion.candidates = vec![("notes/".into(), None), ("notes.txt".into(), None)];
    let (_, _) = feed(&mut e, &mut a, b"ls no\t");
    assert_eq!(e.text(), "ls notes");
    let (out, _) = feed(&mut e, &mut a, b"\t");
    assert!(
        without_sgr(&out).contains("\nnotes/  notes.txt\n"),
        "{out:?}"
    );
    feed(&mut e, &mut a, b"/\t");
    assert_eq!(e.text(), "ls notes/");
}

#[test]
fn continuation_lines() {
    let (mut e, _) = editor();
    let mut a = fake();
    let (out, k) = feed(&mut e, &mut a, b"for f in a b { echo $f\r");
    assert_eq!(k, Key::Edited);
    assert!(out.ends_with(CONTINUATION));
    let (_, k) = feed(&mut e, &mut a, b"}\r");
    assert_eq!(k, Key::Line("for f in a b { echo $f\n}".into()));
    let (_, k) = feed(&mut e, &mut a, b"echo 'unclosed\r'\r");
    assert_eq!(k, Key::Line("echo 'unclosed\n'".into()));
}

#[test]
fn ctrl_d_and_ctrl_c() {
    let (mut e, _) = editor();
    let mut a = fake();
    let (out, k) = feed(&mut e, &mut a, &[CTRL_D]);
    assert_eq!(k, Key::Edited);
    assert_eq!(out, "^D\ntype exit to end the shell\n~> ");
    let (out, k) = feed(&mut e, &mut a, b"ls\x03");
    assert_eq!(k, Key::Interrupt);
    assert!(out.ends_with("^C\n"));
    assert_eq!(e.text(), "");
}

#[test]
fn long_lines_stay_correct() {
    let mut e = Editor::new(Options {
        width: 20,
        ..Options::default()
    });
    let mut out = String::new();
    e.start("~> ", &mut out);
    let mut a = fake();
    let (_, _) = feed(&mut e, &mut a, b"echo aaaaaaaaaaaaaaaaaaaaaaaaaaa");
    feed(&mut e, &mut a, b"\x7f\x1b[Db\r");
    assert_eq!(e.text(), "");
}

/// A line wider than the terminal is redrawn in place (ADR-0154 rule 8 as
/// changed on 2026-11-04): edits away from its end leave exactly its rows,
/// prompt and text, with nothing above or below, and a line that ends at
/// the right edge is followed by a line end.
#[test]
fn long_lines_are_redrawn_in_place() {
    for width in [7, 20, 33] {
        let mut e = Editor::new(Options {
            width,
            suggestions: false,
            ..Options::default()
        });
        let mut screen_bytes = String::from("before\n");
        e.start("~> ", &mut screen_bytes);
        let mut a = fake();
        let mut text = String::from("echo ");
        text.push_str(&"a".repeat(2 * width));
        let (o, _) = feed(&mut e, &mut a, text.as_bytes());
        screen_bytes.push_str(&o);
        // Into the middle, insert, delete, and to the start and back.
        for keys in [
            b"\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D".as_slice(),
            b"XY",
            b"\x7f",
            &[CTRL_A],
            b"Z",
            &[CTRL_E],
        ] {
            let (o, _) = feed(&mut e, &mut a, keys);
            in_subset(&o).unwrap();
            screen_bytes.push_str(&o);
            let rows = screen_wide(&without_sgr(&screen_bytes), width);
            assert_eq!(rows[0], "before", "nothing above is touched ({width})");
            let shown: String = rows[1..].concat();
            let want = alloc::format!("~> {}", e.text());
            assert_eq!(shown, want, "{width}: {rows:?}");
            let cells = want.chars().count();
            assert_eq!(rows.len() - 1, cells / width + 1, "{width}: {rows:?}");
        }
        // Typing at the end writes only what is typed (and a line end at
        // the edge), and the rows stay right.
        for _ in 0..width + 1 {
            let (o, _) = feed(&mut e, &mut a, b"b");
            assert_eq!(o.trim_end_matches('\n'), "b", "{width}");
            screen_bytes.push_str(&o);
            let rows = screen_wide(&without_sgr(&screen_bytes), width);
            assert_eq!(rows[1..].concat(), alloc::format!("~> {}", e.text()));
        }
        assert!(e.text().starts_with("Zecho a"), "{}", e.text());
        assert!(e.text().contains("aX"), "{}", e.text());
    }
    // Ending at the edge: the cursor goes to the next row at once.
    let mut e = Editor::new(Options {
        width: 10,
        suggestions: false,
        ..Options::default()
    });
    let mut out = String::new();
    e.start("~> ", &mut out);
    let mut a = fake();
    let (o, _) = feed(&mut e, &mut a, b"echo xy");
    out.push_str(&o);
    assert!(out.ends_with('\n'), "{out:?}");
    assert_eq!(screen_wide(&without_sgr(&out), 10), ["~> echo xy", ""]);
}

#[test]
fn a_parse_error_has_a_caret_and_a_hint() {
    let line = "if test -f a.txt { echo yes } else echo no }";
    let at = line.find("echo no").unwrap();
    assert_eq!(
        parse_error(
            line,
            at,
            "'else' needs a block",
            Some("write: else { echo no }")
        ),
        "zosh: line 1: 'else' needs a block
  if test -f a.txt { echo yes } else echo no }
                                     ^
  hint: write: else { echo no }
"
    );
}

/// Checks that every byte the editor wrote is in ADR-0154 rule 8's subset.
fn in_subset(out: &str) -> Result<(), String> {
    let mut chars = out.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' | '\n' | '\x08' => {}
            '\x1b' => {
                if chars.next() != Some('[') {
                    return Err("ESC not followed by [".into());
                }
                let mut params = String::new();
                let fin = loop {
                    match chars.next() {
                        Some(d) if d.is_ascii_digit() => params.push(d),
                        Some(d) => break d,
                        None => return Err("cut sequence".into()),
                    }
                };
                let ok = match fin {
                    'A' | 'C' | 'D' => !params.is_empty(),
                    'K' | 'J' | 'H' => params.is_empty(),
                    'm' => matches!(params.as_str(), "" | "0" | "1" | "2" | "22" | "31"),
                    _ => false,
                };
                if !ok {
                    return Err(alloc::format!("ESC [ {params} {fin}"));
                }
            }
            c if c.is_control() || ('\u{80}'..='\u{9f}').contains(&c) => {
                return Err(alloc::format!("control {:?}", c));
            }
            _ => {}
        }
    }
    Ok(())
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// INV-0154-1, -2, -3: any keys at any width leave a valid line and cursor,
/// write only the subset, and only Enter ends a line.
#[test]
fn random_keys() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let interesting: [&[u8]; 16] = [
        b"\x1b[A",
        b"\x1b[B",
        b"\x1b[C",
        b"\x1b[D",
        b"\x1b[3~",
        b"\x1b[H",
        b"\x1bOF",
        b"\t",
        &[CTRL_R],
        &[CTRL_W],
        &[CTRL_K],
        &[CTRL_U],
        &[CTRL_L],
        &[CTRL_D],
        "é".as_bytes(),
        b"'",
    ];
    for round in 0..300 {
        let width = 1 + (rng.next() % 500) as usize;
        let mut e = Editor::new(Options {
            width,
            ..Options::default()
        });
        for h in ["make -C src", "echo 'x y'", "ls"] {
            e.remember(h);
        }
        let mut a = fake();
        a.completion.candidates = vec![("alpha".into(), Some("a".into())), ("alps".into(), None)];
        let mut out = String::new();
        e.start("~> ", &mut out);
        for _ in 0..200 {
            let r = rng.next();
            let mut keys: Vec<u8> = match r % 4 {
                0 => interesting[(r >> 8) as usize % interesting.len()].to_vec(),
                1 => vec![b'a' + (r >> 8) as u8 % 26],
                2 => vec![(r >> 8) as u8],
                _ => vec![b' '],
            };
            keys.retain(|k| *k != b'\r' && *k != b'\n');
            for k in keys {
                let mut o = String::new();
                let result = e.key(k, &mut o, &mut a);
                assert!(!matches!(result, Key::Line(_)), "only Enter ends a line");
                in_subset(&o).unwrap_or_else(|p| panic!("round {round}: {p} in {o:?}"));
                assert!(e.cursor() <= e.text().chars().count());
                assert!(e.text().chars().count() <= MAX_CHARS);
            }
        }
        let mut o = String::new();
        let _ = e.key(b'\r', &mut o, &mut a);
        in_subset(&o).unwrap();
        let _ = e.text().to_string();
    }
}

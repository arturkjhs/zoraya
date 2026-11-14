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

fn editor_with(o: Options) -> Editor {
    let mut e = Editor::new(o);
    let mut out = String::new();
    e.start("~> ", &mut out);
    e
}

fn narrow(width: usize) -> Editor {
    editor_with(Options {
        width,
        ..Options::default()
    })
}

/// The bytes of the last redraw in `out` (from its last `\r`).
fn last_draw(out: &str) -> &str {
    out.rfind("\r~> ").map_or(out, |at| &out[at..])
}

/// Mutation testing: moving keys, each alone.
#[test]
fn every_moving_key_moves() {
    let mut a = fake();
    let cases: [(&[u8], &str, usize); 9] = [
        (b"ab\x1bOHz", "zab", 1),
        (b"ab\x1bOH\x1bOFc", "abc", 3),
        (b"xy\x1bODz", "xzy", 2),
        (b"ab\x02c", "acb", 2),
        (b"ab\x02\x06c", "abc", 3),
        (b"ab\x1b[D\x1b[Cc", "abc", 3),
        (b"xy\x1b[D\x04", "x", 1),
        (b"echo a \x17", "echo ", 5),
        (b"\x0e", "", 0),
    ];
    for (keys, text, cursor) in cases {
        let mut e = editor().0;
        feed(&mut e, &mut a, keys);
        assert_eq!((e.text().as_str(), e.cursor()), (text, cursor), "{keys:?}");
    }
    let mut e = editor().0;
    feed(&mut e, &mut a, b"echo wor");
    assert_eq!(e.cursor(), 8);
}

/// Mutation testing: the history's bounds, from the file and as it grows.
#[test]
fn the_history_keeps_its_size() {
    let lines = |n: usize| (0..n).map(|i| alloc::format!("l{i}")).collect::<Vec<_>>();
    let mut e = editor_with(Options {
        history_size: 2,
        ..Options::default()
    });
    e.set_history(lines(5));
    assert_eq!(e.history(), ["l3", "l4"]);
    let mut e = editor().0;
    e.set_history(vec!["ls".into()]);
    assert_eq!(e.history(), ["ls"]);
    e.set_history(vec!["make -C src".into(), "ls".into()]);
    feed(&mut e, &mut fake(), b"ma\x1b[A");
    assert_eq!(e.text(), "make -C src");
    let mut e = editor_with(Options {
        history_size: 2,
        ..Options::default()
    });
    for h in ["a", "b", "c"] {
        e.remember(h);
    }
    assert_eq!(e.history(), ["b", "c"]);
    let mut e = editor_with(Options {
        history_size: 1,
        ..Options::default()
    });
    e.remember("a");
    e.remember("b");
    assert_eq!(e.history(), ["b"]);
    let mut e = editor().0;
    e.remember("   ");
    assert!(e.history().is_empty());
}

/// Mutation testing: Down after several Ups, and a multi-line entry is
/// not a prefix match.
#[test]
fn down_comes_back_one_at_a_time() {
    let mut a = fake();
    let mut e = editor().0;
    for h in ["make -C src", "make all", "make test"] {
        e.remember(h);
    }
    feed(&mut e, &mut a, b"ma\x1b[A\x1b[A\x1b[A");
    assert_eq!(e.text(), "make -C src");
    feed(&mut e, &mut a, b"\x1b[B");
    assert_eq!(e.text(), "make all");
    let mut e = editor().0;
    for h in ["make", "m\nx", "mo"] {
        e.remember(h);
    }
    feed(&mut e, &mut a, b"m\x1b[A\x1b[A\x1b[B");
    assert_eq!(e.text(), "mo");
}

/// Mutation testing: what is red, what is not.
#[test]
fn what_is_red() {
    let red = |keys: &[u8]| {
        let mut e = editor().0;
        let (out, _) = feed(&mut e, &mut fake(), keys);
        String::from(last_draw(&out))
    };
    for keys in [&b" sl"[..], b"ls; sl", b"+r ~/x sl", b"if xyz"] {
        let d = red(keys);
        let word = core::str::from_utf8(keys)
            .unwrap()
            .rsplit(' ')
            .next()
            .unwrap();
        assert!(
            d.contains(&alloc::format!("\x1b[31m{word}")),
            "{keys:?}: {d:?}"
        );
    }
    for keys in [&b"a=b"[..], b"$x", b"for f in a b", b"echo 'a\\'"] {
        let d = red(keys);
        assert!(!d.contains("\x1b[31m"), "{keys:?}: {d:?}");
    }
    let d = red(b"echo \"a\\\"b");
    assert!(d.contains("\x1b[31m\"a\\\"b"), "{d:?}");
}

/// Mutation testing: the bytes a redraw writes, which the screen model
/// cannot tell apart (it reads `ESC [ 0 A` as no move).
#[test]
fn a_redraw_writes_no_move_of_zero() {
    let mut a = fake();
    let mut e = editor().0;
    let (out, _) = feed(&mut e, &mut a, b"e\x7f");
    assert!(
        !out.contains("\x1b[0A") && !out.contains("\x1b[0C"),
        "{out:?}"
    );
    let mut e = editor().0;
    let (out, _) = feed(&mut e, &mut a, b"ec\x7f");
    assert!(
        out.ends_with("\r~> e\x1b[J") || out.ends_with("\x1b[J"),
        "{out:?}"
    );
    assert!(!out.ends_with("C"), "no move back at the end: {out:?}");
    let mut e = editor().0;
    let (out, _) = feed(&mut e, &mut a, b"ab\x1b[D");
    assert!(out.ends_with("\r\x1b[4C"), "{out:?}");
    let mut e = narrow(10);
    let (out, _) = feed(&mut e, &mut a, b"echo xy\x7f");
    assert!(
        !out.contains("\x1b[0C") && !out.contains("\x1b[0A"),
        "{out:?}"
    );
    let mut e = narrow(10);
    let (out, _) = feed(&mut e, &mut a, b"echo xyz\x7f");
    assert!(!out.is_empty());
    let mut e = Editor::new(Options::default());
    let mut out = String::new();
    e.start("", &mut out);
    let (out, _) = feed(&mut e, &mut a, &[CTRL_L]);
    assert!(out.starts_with("\x1b[H\x1b[J"), "{out:?}");
    assert!(!out.contains('\n'), "{out:?}");
}

/// Mutation testing: a suggestion's place, length and cursor.
#[test]
fn a_suggestion_keeps_to_its_row() {
    let mut a = fake();
    let mut e = editor().0;
    e.remember("make -C src");
    let (out, _) = feed(&mut e, &mut a, b"mak");
    assert!(out.ends_with("\x1b[22m\x1b[J\r\x1b[6C"), "{out:?}");
    let (out, _) = feed(&mut e, &mut a, b"\x1b[D");
    assert!(!out.contains("\x1b[2m"), "none in mid-line: {out:?}");
    let (out, _) = feed(&mut e, &mut a, b"\x1b[Cx");
    assert!(
        without_sgr(&out).contains("\r~> makx"),
        "a full redraw: {out:?}"
    );
    let mut e = editor().0;
    e.remember("ls -l");
    feed(&mut e, &mut a, b"ls -l");
    let (out, _) = feed(&mut e, &mut a, b"x");
    assert_eq!(out, "x", "the whole entry typed: no suggestion is left");
    let mut e = editor().0;
    e.remember("make -C src");
    e.remember("cat");
    let (out, _) = feed(&mut e, &mut a, b"m");
    assert!(out.contains("\x1b[2make -C src"), "{out:?}");
    let mut e = narrow(10);
    e.remember("echo hello world");
    let (out, _) = feed(&mut e, &mut a, b"ech");
    assert!(last_draw(&out).contains("\x1b[2mo h\x1b[22m"), "{out:?}");
    let mut e = editor().0;
    e.remember("echo x");
    let (out, _) = feed(&mut e, &mut a, b"if ok {\rec");
    assert!(
        !out.contains("\x1b[2m"),
        "no suggestion on a continuation line: {out:?}"
    );
    let mut e = editor().0;
    let (out, _) = feed(&mut e, &mut a, b"aa\x1b[Da");
    assert!(
        without_sgr(&out).ends_with("\r~> aaa\x1b[J\r\x1b[5C"),
        "a full redraw: {out:?}"
    );
}

/// Mutation testing: `above` draws the line again, and printable hides
/// control characters.
#[test]
fn above_and_printable() {
    let mut a = fake();
    let mut e = editor().0;
    feed(&mut e, &mut a, b"echo half");
    let mut out = String::new();
    e.above("note\n", &mut out);
    assert_eq!(screen(&without_sgr(&out)), "~> echo half");
    assert_eq!(printable("\x01\x7f\u{9b}a"), "???a");
}

/// Mutation testing: the bounds of an entry, a search query, and the
/// continuation lines.
#[test]
fn an_entry_and_a_search_have_their_bounds() {
    let mut a = fake();
    let mut e = editor().0;
    let (_, k) = feed(&mut e, &mut a, b"if ok {\r\ra");
    assert_eq!(
        (k, e.text()),
        (Key::Edited, "a".into()),
        "an empty line among them"
    );
    let mut e = editor().0;
    feed(&mut e, &mut a, b"if ok {\r");
    let (out, _) = feed(&mut e, &mut a, &[CTRL_D]);
    assert!(out.is_empty(), "{out:?}");
    let mut e = editor().0;
    e.remember(&"a".repeat(256));
    feed(&mut e, &mut a, &[CTRL_R]);
    feed(&mut e, &mut a, &[b'a'; 256]);
    let (_, k) = feed(&mut e, &mut a, b"b\r");
    assert_eq!(k, Key::Line("a".repeat(256)));
    let mut e = editor().0;
    e.remember("tcc main.c -o main");
    feed(&mut e, &mut a, &[CTRL_R]);
    let (_, k) = feed(&mut e, &mut a, b"mainx\x7f\r");
    assert_eq!(k, Key::Line("tcc main.c -o main".into()));
    feed(&mut e, &mut a, &[CTRL_R]);
    feed(&mut e, &mut a, b"main");
    let (_, k) = feed(&mut e, &mut a, &[CTRL_C]);
    assert_eq!(k, Key::Interrupt);
    let mut e = editor().0;
    e.remember("tcc main.c -o main");
    feed(&mut e, &mut a, &[CTRL_R]);
    feed(&mut e, &mut a, b"main");
    feed(&mut e, &mut a, &[ESC]);
    feed(&mut e, &mut a, b"x");
    assert!(e.text() != "tcc main.c -o mainx", "{}", e.text());
}

/// Mutation testing: the listing of completions wraps at the width and
/// stops at its limit.
#[test]
fn a_listing_wraps_and_stops() {
    let list = |width: usize, names: &[String], summaries: bool| {
        let mut e = narrow(width);
        let mut a = fake();
        a.completion.candidates = names
            .iter()
            .map(|n| (n.clone(), summaries.then(|| String::from("s"))))
            .collect();
        let (out, _) = feed(&mut e, &mut a, b"ls \t\t");
        without_sgr(&out)
    };
    let three: Vec<String> = ["aaaaaa", "bbbbbb", "cccccc"].map(String::from).to_vec();
    let out = list(20, &three, false);
    assert!(out.contains("\naaaaaa  bbbbbb\ncccccc\n"), "{out:?}");
    let four: Vec<String> = ["aaaaa", "bbbbb", "ccccc", "ddddd"]
        .map(String::from)
        .to_vec();
    let out = list(26, &four, false);
    assert!(out.contains("\naaaaa  bbbbb  ccccc\nddddd\n"), "{out:?}");
    let many: Vec<String> = (0..30).map(|i| alloc::format!("{i:0>40}")).collect();
    let out = list(80, &many, false);
    assert_eq!(out.matches("0000").count() / 10, 24, "{out:?}");
    let out = list(80, &many, true);
    assert!(out.contains("(and 6 more)"), "{out:?}");
    let out = list(80, &many[..24], true);
    assert!(!out.contains("more)"), "{out:?}");
    let one: Vec<String> = vec!["notes/".into()];
    let mut e = editor().0;
    let mut a = fake();
    a.completion.candidates = vec![("notes/".into(), None)];
    feed(&mut e, &mut a, b"ls notes/");
    let (out, _) = feed(&mut e, &mut a, b"\t");
    assert!(without_sgr(&out).contains("\nnotes/\n"), "{out:?} {one:?}");
}

/// Mutation testing: the newest entry that extends the typed text is
/// suggested, even when an entry equal to the text is newer still.
#[test]
fn a_suggestion_skips_an_entry_equal_to_the_text() {
    let mut a = fake();
    let mut e = editor().0;
    e.remember("make -C src");
    e.remember("make");
    let (out, _) = feed(&mut e, &mut a, b"make");
    assert!(out.contains("\x1b[2m -C src\x1b[22m"), "{out:?}");
}

/// Mutation testing: where the cursor goes after a suggestion on a narrow
/// screen, and the line end at a multiple of the width.
#[test]
fn rows_and_columns_on_a_narrow_screen() {
    let mut a = fake();
    let mut e = narrow(20);
    e.remember("echo xxxxxxxx");
    let (out, _) = feed(&mut e, &mut a, b"echo ");
    let d = last_draw(&out);
    assert!(d.ends_with("\x1b[22m\x1b[J\r\x1b[8C"), "{d:?}");
    assert!(!d.contains("\x1b[3A"), "{d:?}");
    // "~> echo xy" fills 10 columns: the cursor moves to the next row.
    let mut e = narrow(10);
    let (out, _) = feed(&mut e, &mut a, b"echo xy\x1b[D");
    assert_eq!(screen_wide(&without_sgr(&out), 10), ["~> echo xy", ""]);
    let mut e = narrow(10);
    let (out, _) = feed(&mut e, &mut a, b"echo xyz\x1b[D");
    assert!(!out.contains("\x1b[0C"), "{out:?}");
}

/// Mutation testing: an entry holds MAX_CHARS characters, a continuation
/// line counting one more for its line end.
#[test]
fn an_entry_holds_max_chars() {
    let mut a = fake();
    // A long entry from the history, then keys past the bound: typing
    // each of 4096 keys would redraw a long line 4096 times.
    let mut e = editor().0;
    e.remember(&"a".repeat(MAX_CHARS - 4));
    feed(&mut e, &mut a, b"\x1b[A");
    feed(&mut e, &mut a, &[b'a'; 10]);
    assert_eq!(e.text().chars().count(), MAX_CHARS);
    let mut e = editor().0;
    e.remember(&"a".repeat(MAX_CHARS - 20));
    feed(&mut e, &mut a, b"if ok {\r\x1b[A");
    feed(&mut e, &mut a, &[b'a'; 20]);
    assert_eq!(e.text().chars().count(), MAX_CHARS - "if ok {".len() - 1);
}

/// Mutation testing: typing writes the character alone only at the line's
/// end with nothing faint shown; elsewhere the line is redrawn.
#[test]
fn a_typed_character_redraws_unless_it_only_extends_the_line() {
    let mut a = fake();
    let mut e = editor().0;
    let (mut all, _) = feed(&mut e, &mut a, b"echo aa\x1b[D\x1b[D");
    all.push_str(&feed(&mut e, &mut a, b"a").0);
    assert_eq!(screen(&without_sgr(&all)), "~> echo aaa");
    let mut e = editor().0;
    e.remember("echo hello");
    let (mut all, _) = feed(&mut e, &mut a, b"echo he");
    all.push_str(&feed(&mut e, &mut a, b"x").0);
    assert_eq!(screen(&without_sgr(&all)), "~> echo hex");
}

/// Mutation testing: where a listing's rows wrap.
#[test]
fn a_listing_wraps_where_its_names_end() {
    let list = |width: usize, names: &[&str]| {
        let mut e = narrow(width);
        let mut a = fake();
        a.completion.candidates = names.iter().map(|n| (String::from(*n), None)).collect();
        let (out, _) = feed(&mut e, &mut a, b"ls \t\t");
        without_sgr(&out)
    };
    let (a18, b18) = ("a".repeat(18), "b".repeat(18));
    let out = list(20, &[&a18, &b18]);
    assert_eq!(out.split('\n').nth(1), Some(a18.as_str()), "{out:?}");
    let d12 = "d".repeat(12);
    let out = list(20, &["abc", &d12]);
    assert!(out.contains("\nabc  dddddddddddd\n"), "{out:?}");
    let out = list(20, &["aaaaa", "bbbbb", "ccccc"]);
    assert!(out.contains("\naaaaa  bbbbb  ccccc\n"), "{out:?}");
}

/// Mutation testing: an entry takes at most 256 continuation lines; the
/// next Enter gives the entry as it is.
#[test]
fn an_entry_takes_at_most_256_continuation_lines() {
    let mut a = fake();
    let mut e = editor().0;
    feed(&mut e, &mut a, b"if ok {");
    for n in 0..256 {
        let (_, k) = feed(&mut e, &mut a, b"\r");
        assert!(matches!(k, Key::Edited), "Enter {n}");
    }
    let (_, k) = feed(&mut e, &mut a, b"\r");
    assert!(matches!(k, Key::Line(_)));
}

/// ADR-0332 item 9: the editor counts columns with the terminal's width
/// table, so a wide character takes two cells, a combining mark none, and
/// the cursor is put where the terminal has it.
#[test]
fn the_cursor_follows_the_width_table() {
    let (mut e, _) = editor();
    let mut a = fake();
    // "~> " is 3 cells; the ideograph takes 2, the combining acute none.
    feed(&mut e, &mut a, "中e\u{301}".as_bytes());
    assert_eq!(e.text(), "中e\u{301}");
    // Left once: the cursor goes before the acute.
    let (out2, _) = feed(&mut e, &mut a, b"\x1b[D");
    // Before the acute is the same cell as after `e` (it takes none): the
    // cursor stays where the text ended, at column 6.
    assert!(!out2.contains('C'), "{out2:?}");
    // Left again, before `e`: column 5; left again, before the ideograph: 3.
    let (out3, _) = feed(&mut e, &mut a, b"\x1b[D");
    assert!(out3.ends_with("\x1b[5C"), "{out3:?}");
    let (out4, _) = feed(&mut e, &mut a, b"\x1b[D");
    assert!(out4.ends_with("\x1b[3C"), "{out4:?}");
}

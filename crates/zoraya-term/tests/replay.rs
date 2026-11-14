//! Recorded sessions replay to their committed grids (ADR-0332 item 10,
//! INV-0332-6).
//!
//! `recordings/NAME.rec` holds the bytes a program wrote to a terminal, made
//! once on a reference system by `tools/term-record/record.py` from
//! `NAME.session`; `NAME.marks` the offsets at which the screen is checked;
//! `NAME.grids` the grids `zoraya-term` produces there, which a person
//! compared with the same recording in xterm when they were first made, and
//! which are frozen (as alacritty freezes its reference tests). A grid is
//! its rows as text, a row of style letters under each, and a legend.
//!
//! `ZORAYA_BLESS=1 cargo test -p zoraya-term --test replay` rewrites the
//! `.grids` files; review the diff before committing it.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

use zoraya_term::cell::{TAIL, WIDE};
use zoraya_term::{Colour, Term};

zoraya_test_alloc::cap_memory!();

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recordings")
}

fn style_of(cell: &zoraya_term::Cell) -> String {
    let fg = |c: Colour| match c {
        Colour::Default => String::from("-"),
        Colour::Index(i) => format!("{i}"),
        Colour::Rgb(r, g, b) => format!("{r}.{g}.{b}"),
    };
    format!(
        "fg={} bg={} attrs={:#x}",
        fg(cell.fg()),
        fg(cell.bg()),
        cell.attrs() & !(WIDE | TAIL)
    )
}

fn snapshot(t: &Term, offset: usize, label: &str, legend: &mut BTreeMap<String, char>) -> String {
    let (columns, rows) = t.size();
    let letters: Vec<char> = ('a'..='z').chain('A'..='Z').chain('0'..='9').collect();
    let mut out = String::new();
    let (x, y) = t.cursor();
    writeln!(out, "@ {offset} {label}").unwrap();
    writeln!(
        out,
        "cursor {x} {y} screen={} view={}",
        if t.on_alternate_screen() {
            "alternate"
        } else {
            "primary"
        },
        t.view()
    )
    .unwrap();
    writeln!(out, "modes {:?}", t.modes()).unwrap();
    writeln!(
        out,
        "region {:?} shape {} history {}",
        t.scroll_region(),
        t.cursor_shape(),
        t.history_len()
    )
    .unwrap();
    for row in 0..rows {
        let mut text = String::new();
        let mut styles = String::new();
        for col in 0..columns {
            let cell = t.cell(col, row);
            let style = style_of(&cell);
            let letter = if style == "fg=- bg=- attrs=0x0" {
                '.'
            } else {
                let next = legend.len();
                *legend
                    .entry(style)
                    .or_insert_with(|| letters.get(next).copied().unwrap_or('?'))
            };
            styles.push(letter);
            if !cell.has(TAIL) {
                text.push(cell.ch);
                text.extend(cell.marks());
            }
        }
        writeln!(out, "|{text}|").unwrap();
        writeln!(out, "|{styles}|").unwrap();
    }
    out
}

fn replay(name: &str) -> String {
    let rec = fs::read(dir().join(format!("{name}.rec"))).unwrap();
    let marks = fs::read_to_string(dir().join(format!("{name}.marks"))).unwrap();
    let mut lines = marks.lines();
    let size: Vec<usize> = lines
        .next()
        .unwrap()
        .split(' ')
        .skip(1)
        .map(|v| v.parse().unwrap())
        .collect();
    let mut t = Term::new(size[0], size[1]);
    let mut legend = BTreeMap::new();
    let mut body = String::new();
    let mut at = 0;
    for line in lines {
        let (offset, label) = line.split_once(' ').unwrap();
        let offset: usize = offset.parse().unwrap();
        t.write(&rec[at..offset]);
        at = offset;
        body.push_str(&snapshot(&t, offset, label, &mut legend));
    }
    let mut out = format!(
        "# {name}: {} bytes replayed through zoraya-term {}x{}\n",
        rec.len(),
        size[0],
        size[1]
    );
    let mut by_letter: Vec<_> = legend.iter().map(|(s, c)| (*c, s.clone())).collect();
    by_letter.sort();
    for (c, s) in by_letter {
        writeln!(out, "# {c} = {s}").unwrap();
    }
    out.push_str(&body);
    out
}

fn names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".rec").map(String::from))
        .collect();
    names.sort();
    names
}

#[test]
fn every_recording_replays_to_its_committed_grids() {
    let names = names();
    assert!(names.len() >= 4, "the recordings are missing: {names:?}");
    let bless = std::env::var("ZORAYA_BLESS").is_ok();
    for name in names {
        let got = replay(&name);
        let path = dir().join(format!("{name}.grids"));
        if bless {
            fs::write(&path, &got).unwrap();
            continue;
        }
        let want = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{name}.grids is missing: ZORAYA_BLESS=1"));
        if got != want {
            let first = got
                .lines()
                .zip(want.lines())
                .enumerate()
                .find(|(_, (g, w))| g != w);
            panic!(
                "{name} replays differently; first difference {first:?}\n(ZORAYA_BLESS=1 rewrites the grids)"
            );
        }
    }
}

/// A recording must be the whole of its program's output: no mark past the
/// end, in order, and a session script beside it.
#[test]
fn recordings_are_well_formed() {
    for name in names() {
        let rec = fs::read(dir().join(format!("{name}.rec"))).unwrap();
        let marks = fs::read_to_string(dir().join(format!("{name}.marks"))).unwrap();
        let mut last = 0;
        for line in marks.lines().skip(1) {
            let offset: usize = line.split(' ').next().unwrap().parse().unwrap();
            assert!(
                offset >= last && offset <= rec.len(),
                "{name}: mark at {offset}"
            );
            last = offset;
        }
        assert!(
            dir().join(format!("{name}.session")).exists(),
            "{name} has no session script"
        );
    }
}

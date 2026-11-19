//! The grant line and the kill offer (ADR-0337 items 4 to 6, ADR-0387 item 4):
//! what the status row becomes while the person writes a grant for the
//! running program, or answers `kill PROGRAM?`. Pure, so every rule is
//! tested on the host; [`crate::tty_host::Host`] holds one and routes keys
//! to it, `zosh` carries out what it confirms.
//!
//! Nothing here reads a program's output, a report or a byte typed to a
//! program: a line opens only by [`GrantLine::open`], which the host calls
//! on the terminal's attention message (INV-0337-1), and its text is typed
//! or pasted by the person (INV-0337-2). What is confirmed is parsed with
//! [`parse_give`] as literal words and resolved against the shell's own
//! table (INV-0337-3).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_resolve::join;

use crate::netgrant;
use crate::refusal::program_name;

/// How long Enter does nothing after the line opens, so that a key already
/// on its way cannot confirm anything (ADR-0337 item 4, ADR-0245).
pub const GUARD_MS: u64 = 500;

/// The longest text of the field, in bytes (the row's cells are fewer).
pub const MAX_TEXT: usize = 160;

/// What the attention key opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// System+G: the person writes grants for one element.
    Give,
    /// System+K: `kill PROGRAM?`, Enter kills the line's group.
    Kill,
}

/// What a key did to the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The field's text or element changed: show it again.
    Redraw,
    /// Escape: the line closes and gives nothing.
    Close,
    /// Enter, after the guard: the text of a grant line to parse and give.
    Give(String),
    /// Enter on a kill offer, after the guard.
    Kill,
    /// Tab on a non-empty field: complete this last word from the table.
    Complete(String),
}

/// The grant line or the kill offer, open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantLine {
    mode: Mode,
    /// The line's elements, by name; `element` is the one that receives.
    elements: Vec<String>,
    element: usize,
    text: String,
    opened: u64,
}

impl GrantLine {
    /// Opens the line at `now` (milliseconds). `element` is the one the
    /// status row's refusal came from, or the last (ADR-0337 item 5); it
    /// changes only by Tab on an empty field.
    pub fn open(mode: Mode, elements: Vec<String>, element: Option<usize>, now: u64) -> GrantLine {
        let last = elements.len().saturating_sub(1);
        GrantLine {
            mode,
            element: element.filter(|e| *e <= last).unwrap_or(last),
            elements,
            text: String::new(),
            opened: now,
        }
    }

    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// The index of the element that receives the grant.
    pub const fn element(&self) -> usize {
        self.element
    }

    /// The name of the element that receives.
    pub fn program(&self) -> String {
        self.elements
            .get(self.element)
            .map(|n| program_name(n))
            .unwrap_or_default()
    }

    /// The text typed so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What the status row shows: `give vi: +r ~/o` or `kill vi?`.
    pub fn field(&self) -> String {
        match self.mode {
            Mode::Give => format!("give {}: {}", self.program(), self.text),
            Mode::Kill => format!("kill {}? Enter kills, Escape keeps", self.program()),
        }
    }

    /// Whether Enter would be ignored at `now`.
    pub const fn guarded(&self, now: u64) -> bool {
        now.saturating_sub(self.opened) < GUARD_MS
    }

    /// Keys from the terminal, as bytes. The person's own: printable text,
    /// Backspace, Ctrl-U, Tab, Enter and Escape; any other key, and any
    /// escape sequence, does nothing. A lone ESC at the end of the read is
    /// Escape; one followed by more is a key sequence and is skipped whole.
    pub fn keys(&mut self, bytes: &[u8], now: u64) -> Vec<Step> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            i += 1;
            match b {
                0x1b => {
                    match bytes.get(i) {
                        None => {
                            out.push(Step::Close);
                            return out;
                        }
                        Some(b'[') => {
                            i += 1;
                            while let Some(c) = bytes.get(i) {
                                i += 1;
                                if (0x40..=0x7e).contains(c) {
                                    break;
                                }
                            }
                        }
                        // SS3 and its final byte.
                        Some(b'O') => i += 2,
                        // Alt and a key.
                        Some(_) => i += 1,
                    }
                }
                b'\r' | b'\n' => {
                    if self.guarded(now) {
                        continue;
                    }
                    match self.mode {
                        Mode::Kill => {
                            out.push(Step::Kill);
                            return out;
                        }
                        Mode::Give if self.text.trim().is_empty() => {}
                        Mode::Give => {
                            out.push(Step::Give(self.text.clone()));
                            return out;
                        }
                    }
                }
                0x7f | 0x08 if self.mode == Mode::Give => {
                    if self.text.pop().is_some() {
                        out.push(Step::Redraw);
                    }
                }
                0x15 if self.mode == Mode::Give => {
                    if !self.text.is_empty() {
                        self.text.clear();
                        out.push(Step::Redraw);
                    }
                }
                b'\t' if self.mode == Mode::Give => {
                    if self.text.is_empty() {
                        if self.elements.len() > 1 {
                            self.element = (self.element + 1) % self.elements.len();
                            out.push(Step::Redraw);
                        }
                    } else {
                        let word = self.text.rsplit(' ').next().unwrap_or("");
                        out.push(Step::Complete(String::from(word)));
                    }
                }
                0x20..=0x7e if self.mode == Mode::Give => self.push(char::from(b), &mut out),
                0xc2..=0xf4 if self.mode == Mode::Give => {
                    // One UTF-8 character; a broken one is dropped.
                    let n = match b {
                        0xc2..=0xdf => 1,
                        0xe0..=0xef => 2,
                        _ => 3,
                    };
                    let Some(rest) = bytes.get(i..i + n) else {
                        break;
                    };
                    let mut buf = [b; 4];
                    buf[1..=n].copy_from_slice(rest);
                    if let Ok(s) = core::str::from_utf8(&buf[..=n]) {
                        if let Some(c) = s.chars().next() {
                            self.push(c, &mut out);
                        }
                        i += n;
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn push(&mut self, c: char, out: &mut Vec<Step>) {
        if self.text.len() + c.len_utf8() <= MAX_TEXT {
            self.text.push(c);
            out.push(Step::Redraw);
        }
    }

    /// Pasted text (ADR-0332 item 8): into the field, its newlines as
    /// spaces, never Enter. A kill offer takes none.
    pub fn paste(&mut self, text: &str) -> Vec<Step> {
        let mut out = Vec::new();
        if self.mode != Mode::Give {
            return out;
        }
        for c in text.chars() {
            let c = if c.is_control() { ' ' } else { c };
            self.push(c, &mut out);
        }
        if !out.is_empty() {
            out.truncate(1);
        }
        out
    }

    /// The shell's completion of the last word: `candidates` are the whole
    /// words that begin with it. One candidate replaces the word; several
    /// extend it to what they share.
    pub fn complete(&mut self, candidates: &[String]) -> Vec<Step> {
        let Some(first) = candidates.first() else {
            return Vec::new();
        };
        let mut common = first.clone();
        for c in &candidates[1..] {
            let n = common
                .chars()
                .zip(c.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a.len_utf8())
                .sum();
            common.truncate(n);
        }
        let start = self.text.rfind(' ').map_or(0, |i| i + 1);
        if common.len() <= self.text.len() - start || start + common.len() > MAX_TEXT {
            return Vec::new();
        }
        self.text.truncate(start);
        self.text.push_str(&common);
        alloc::vec![Step::Redraw]
    }
}

/// One thing a grant line gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Give {
    /// `+r PATH` (`write` false) or `+w PATH`, absolute and normal.
    Path { path: String, write: bool },
    /// `+net HOST[:PORT]`, as written.
    Net(String),
}

/// What the shell holds: the grant line gives no more (ADR-0337 item 6).
pub trait Held {
    /// Whether the shell's table covers `path` with the right (`write`).
    fn path(&self, path: &str, write: bool) -> bool;
    /// Whether the shell's network scope covers the rule `word`.
    fn net(&self, word: &str) -> bool;
}

/// Characters a literal word never holds: a variable, a substitution, a
/// glob, a quote, a pipe or anything else the shell's language gives
/// a meaning to (ADR-0153 rule 3: the grant is a literal word).
fn literal(word: &str) -> bool {
    !word.is_empty()
        && !word.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '$' | '`'
                        | '('
                        | ')'
                        | '{'
                        | '}'
                        | '['
                        | ']'
                        | '*'
                        | '?'
                        | ';'
                        | '|'
                        | '&'
                        | '<'
                        | '>'
                        | '\''
                        | '"'
                        | '\\'
                        | '!'
                        | '#'
                        | '%'
                )
        })
}

/// What a grant line's text gives, or why it gives nothing (ADR-0337 items 5
/// and 6; INV-0337-3). The text is read as literal words only: `+r PATH`,
/// `+w PATH`, `+net HOST[:PORT]`, one or more; `~` or `~/` at the start of a
/// path is the home directory; a relative path is joined to `cwd`. Each is
/// resolved by the shell's own table and refused if `held` does not cover it.
/// All or nothing: one refused word gives none.
pub fn parse_give(text: &str, cwd: &str, home: &str, held: &dyn Held) -> Result<Vec<Give>, String> {
    let mut words = text.split_ascii_whitespace();
    let mut out = Vec::new();
    while let Some(token) = words.next() {
        let Some(operand) = words.next() else {
            return Err(format!("{token}: needs a path or a host after it"));
        };
        if !literal(token) || !literal(operand) {
            return Err(format!(
                "{operand}: only plain words are given, no variable, quote or pattern"
            ));
        }
        match token {
            "+r" | "+w" => {
                let write = token == "+w";
                let expanded = if operand == "~" {
                    String::from(home)
                } else if let Some(rest) = operand.strip_prefix("~/") {
                    format!("{}/{rest}", home.trim_end_matches('/'))
                } else if operand.starts_with('~') {
                    return Err(format!("{operand}: only your own home is written with ~"));
                } else {
                    String::from(operand)
                };
                if expanded.starts_with('+') {
                    return Err(format!("{token} needs a path, not {operand}"));
                }
                let path = join(cwd, 0, &expanded);
                if !held.path(&path, write) {
                    return Err(format!("{operand}: this shell does not hold it"));
                }
                out.push(Give::Path { path, write });
            }
            "+net" => {
                if let Err(why) = netgrant::parse_word(operand) {
                    return Err(format!("{operand}: {why}"));
                }
                if !held.net(operand) {
                    return Err(format!("{operand}: this shell does not hold it"));
                }
                out.push(Give::Net(String::from(operand)));
            }
            _ => {
                return Err(format!(
                    "{token}: only +r PATH, +w PATH and +net HOST are given here"
                ));
            }
        }
    }
    if out.is_empty() {
        return Err(String::from("nothing to give"));
    }
    Ok(out)
}

/// The grants as the row and `jobs` say them: `+r ~/other.txt, +net host`.
pub fn words(gives: &[Give], home: &str) -> String {
    let parts: Vec<String> = gives
        .iter()
        .map(|g| match g {
            Give::Path { path, write } => format!(
                "{} {}",
                if *write { "+w" } else { "+r" },
                crate::refusal::hint_path(path, home)
            ),
            Give::Net(w) => format!("+net {w}"),
        })
        .collect();
    parts.join(", ")
}

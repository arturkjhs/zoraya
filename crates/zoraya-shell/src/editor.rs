//! The line editor (ADR-0154): a UTF-8 line with a cursor, history with
//! prefix search and Ctrl-R, completion, suggestions from history in faint
//! text, the command word in red while it names nothing, and continuation
//! lines (`...> `) while a quote or block is open.
//!
//! It writes only ADR-0096's subset as extended by ADR-0154 (rule 8, with
//! the architect's change of 2026-11-04): text, `\r`, `\n`, `\x08`,
//! `ESC [ n A`, `ESC [ n C`, `ESC [ n D`, `ESC [ K`, `ESC [ J`, `ESC [ H` and
//! `ESC [ m` with 0, 1, 2, 22 and 31 (INV-0154-2). Every redraw goes up to
//! the row the entry's line starts on, writes it again, clears below and puts
//! the cursor back, so a line wider than the terminal is redrawn in place. A
//! line that ends exactly at the right edge is followed by a line end, so
//! that the cursor's row is the same on terminals that wrap at once and on
//! those that wait for the next character.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Most characters in one entry, continuation lines included.
pub const MAX_CHARS: usize = 4096;
/// The prompt of a continuation line.
pub const CONTINUATION: &str = "...> ";

pub const CTRL_A: u8 = 1;
pub const CTRL_B: u8 = 2;
pub const CTRL_C: u8 = 3;
pub const CTRL_D: u8 = 4;
pub const CTRL_E: u8 = 5;
pub const CTRL_F: u8 = 6;
pub const BACKSPACE: u8 = 8;
pub const TAB: u8 = 9;
pub const CTRL_K: u8 = 11;
pub const CTRL_L: u8 = 12;
pub const CTRL_R: u8 = 18;
pub const CTRL_U: u8 = 21;
pub const CTRL_W: u8 = 23;
pub const CTRL_Z: u8 = 26;
pub const ESC: u8 = 27;
pub const CTRL_BACKSLASH: u8 = 28;
pub const DELETE: u8 = 127;

const RED: &str = "\x1b[31m";
const FAINT: &str = "\x1b[2m";
const NORMAL: &str = "\x1b[22m";
const RESET: &str = "\x1b[m";

/// What the editor needs to know from the shell.
pub trait Assist {
    /// Whether `name` names a builtin, function or program.
    fn known(&mut self, name: &str) -> bool;
    /// Whether the text so far needs more lines (an open quote or block).
    fn incomplete(&mut self, text: &str) -> bool;
    /// Candidates for the word that ends at `cursor` (a byte offset in
    /// `line`): where the word starts, and each candidate with an optional
    /// summary. Every candidate begins with the word as typed.
    fn complete(&mut self, line: &str, cursor: usize) -> Completion;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Byte offset in the line where the replaced word starts.
    pub start: usize,
    /// `(text, summary)`. A text ending in `/` is a directory: nothing is
    /// added after it when it is the one match.
    pub candidates: Vec<(String, Option<String>)>,
    /// The listing ran out of time (`(listing too slow)`).
    pub slow: bool,
}

/// What a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Nothing for the shell; the screen is up to date.
    Edited,
    /// Enter ended a complete entry: its text, lines joined by `\n`.
    Line(String),
    /// Ctrl-C at the prompt: the entry was dropped.
    Interrupt,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Escape {
    None,
    Started,
    /// `ESC [` with its parameter bytes so far.
    Csi,
    /// `ESC O`: one more byte.
    Single,
}

struct Search {
    query: String,
    /// Index into history of the shown match, if any.
    found: Option<usize>,
}

/// Options from the settings (ADR-0157).
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub width: usize,
    pub suggestions: bool,
    pub colour: bool,
    pub history_size: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            width: 80,
            suggestions: true,
            colour: true,
            history_size: 1000,
        }
    }
}

pub struct Editor {
    prompt: String,
    /// Completed lines of this entry (continuations).
    lines: Vec<String>,
    line: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    /// How far back Up went (0: the line being typed), and what was typed.
    back: usize,
    typed: Option<String>,
    escape: Escape,
    params: String,
    utf8: Vec<u8>,
    search: Option<Search>,
    tabbed: bool,
    pub options: Options,
    /// The terminal's state: the line shown, the cursor's column, and
    /// whether the editor still knows where the line starts.
    shown_suggestion: bool,
    /// The row, counted from the line's first, the cursor is on.
    row: usize,
}

fn width_of(s: &str) -> usize {
    s.chars().count()
}

/// The prompt's visible width: SGR sequences in it take no columns.
fn visible(s: &str) -> usize {
    let mut n = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            n += 1;
        }
    }
    n
}

/// Replaces control characters by `?` (ADR-0154 rule 7).
pub fn printable(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || ('\u{80}'..='\u{9f}').contains(&c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

impl Editor {
    pub fn new(options: Options) -> Editor {
        Editor {
            prompt: String::new(),
            lines: Vec::new(),
            line: Vec::new(),
            cursor: 0,
            history: Vec::new(),
            back: 0,
            typed: None,
            escape: Escape::None,
            params: String::new(),
            utf8: Vec::new(),
            search: None,
            tabbed: false,
            options,
            shown_suggestion: false,
            row: 0,
        }
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Replaces the history (from `/config/history`).
    pub fn set_history(&mut self, lines: Vec<String>) {
        self.history = lines;
        let size = self.options.history_size;
        if self.history.len() > size {
            let cut = self.history.len() - size;
            self.history.drain(..cut);
        }
    }

    /// Keeps a line run: once, at the end (rule 2).
    pub fn remember(&mut self, entry: &str) {
        if entry.trim().is_empty() || self.options.history_size == 0 {
            return;
        }
        self.history.retain(|h| h != entry);
        self.history.push(entry.into());
        let size = self.options.history_size;
        if self.history.len() > size {
            let cut = self.history.len() - size;
            self.history.drain(..cut);
        }
    }

    /// The text being edited.
    pub fn text(&self) -> String {
        self.line.iter().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn current_prompt(&self) -> &str {
        if self.lines.is_empty() {
            &self.prompt
        } else {
            CONTINUATION
        }
    }

    /// Starts a new entry and draws its prompt.
    pub fn start(&mut self, prompt: &str, out: &mut String) {
        self.prompt = prompt.into();
        self.lines.clear();
        self.line.clear();
        self.cursor = 0;
        self.back = 0;
        self.typed = None;
        self.search = None;
        self.tabbed = false;
        self.row = 0;
        self.shown_suggestion = false;
        out.push_str(&self.prompt);
    }

    /// Draws the prompt and line again on a new row, as after output that
    /// came while editing (rule 7).
    pub fn redraw(&mut self, out: &mut String) {
        self.row = 0;
        self.refresh(out);
    }

    /// Shows `text` (whole lines) above the line being edited, which is
    /// drawn again below it (ADR-0154 rule 7, ADR-0158 rule 6).
    pub fn above(&mut self, text: &str, out: &mut String) {
        if text.is_empty() {
            return;
        }
        self.hide(out);
        out.push_str(text);
        self.redraw(out);
    }

    /// Erases the entry's line, so output can be printed where it was;
    /// follow with [`Editor::redraw`].
    pub fn hide(&mut self, out: &mut String) {
        self.up(out);
        out.push_str("\r\x1b[J");
        self.row = 0;
    }

    /// To the row the line starts on.
    fn up(&self, out: &mut String) {
        if self.row > 0 {
            out.push_str(&format!("\x1b[{}A", self.row));
        }
    }

    fn suggestion(&self) -> Option<String> {
        if !self.options.suggestions || self.cursor != self.line.len() || self.line.is_empty() {
            return None;
        }
        if !self.lines.is_empty() || self.search.is_some() {
            return None;
        }
        let typed = self.text();
        self.history
            .iter()
            .rev()
            .find(|h| h.len() > typed.len() && h.starts_with(&typed) && !h.contains('\n'))
            .map(|h| h[typed.len()..].to_string())
    }

    /// The line with colour: the command words red while they name nothing,
    /// an unclosed quote red to its end (rule 5).
    fn highlighted(&self, assist: &mut dyn Assist) -> String {
        let text = self.text();
        if !self.options.colour {
            return printable(&text);
        }
        let mut red = alloc::vec![false; text.len()];
        // Command words: the first word, and the first after ; | & { } and
        // the keywords that take a command.
        let mut at = 0;
        let mut expect_command = self.lines.is_empty()
            || self
                .lines
                .last()
                .is_some_and(|l| l.trim_end().ends_with(['{', ';', '|', '&']));
        let bytes = text.as_bytes();
        let mut quote: Option<(u8, usize)> = None;
        while at < bytes.len() {
            let b = bytes[at];
            if let Some((q, _)) = quote {
                if b == q {
                    quote = None;
                } else if b == b'\\' && q == b'"' {
                    at += 1;
                }
                at += 1;
                continue;
            }
            match b {
                b' ' | b'\t' => at += 1,
                b';' | b'|' | b'&' | b'{' | b'}' | b'(' => {
                    expect_command = true;
                    at += 1;
                }
                b'\'' | b'"' => {
                    quote = Some((b, at));
                    at += 1;
                }
                _ => {
                    let end = text[at..]
                        .find([' ', '\t', ';', '|', '&', '\'', '"'])
                        .map_or(text.len(), |n| at + n);
                    let word = &text[at..end];
                    if expect_command {
                        let keyword =
                            matches!(word, "if" | "while" | "else" | "try" | "catch" | "!");
                        let skip = keyword
                            || word.starts_with("+r")
                            || word.starts_with("+w")
                            || word.contains('=')
                            || word.starts_with('$')
                            || matches!(word, "for" | "fn" | "break" | "continue" | "return");
                        if !skip && !word.is_empty() && !assist.known(word) {
                            for r in &mut red[at..end] {
                                *r = true;
                            }
                        }
                        let grant = word == "+r" || word == "+w";
                        expect_command = keyword || grant;
                        if grant {
                            // The path after a grant is not a command.
                            let rest = &text[end..];
                            let skip_to = rest.trim_start();
                            let next_end = skip_to.find([' ', '\t']).unwrap_or(skip_to.len());
                            at = end + (rest.len() - skip_to.len()) + next_end;
                            continue;
                        }
                    }
                    at = end.max(at + 1);
                }
            }
        }
        if let Some((_, start)) = quote {
            for r in &mut red[start..] {
                *r = true;
            }
        }
        let mut s = String::new();
        let mut on = false;
        for (i, c) in text.char_indices() {
            if red[i] != on {
                on = red[i];
                s.push_str(if on { RED } else { RESET });
            }
            let shown = printable(c.encode_utf8(&mut [0; 4]));
            s.push_str(&shown);
        }
        if on {
            s.push_str(RESET);
        }
        s
    }

    /// Redraws the line: prompt, text, suggestion, and the cursor in place.
    fn refresh_with(&mut self, out: &mut String, assist: Option<&mut dyn Assist>) {
        self.draw(out, assist, false);
    }

    /// Draws the line from its first row; `at_end` leaves the cursor after
    /// it, for a line end to follow.
    fn draw(&mut self, out: &mut String, assist: Option<&mut dyn Assist>, at_end: bool) {
        let width = self.options.width.max(1);
        if let Some(search) = &self.search {
            let found = search
                .found
                .and_then(|i| self.history.get(i))
                .cloned()
                .unwrap_or_default();
            let line = format!("search: {}  {}", search.query, printable(&found));
            let line: String = line.chars().take(width.saturating_sub(1)).collect();
            out.push('\r');
            out.push_str(&line);
            out.push_str("\x1b[K");
            return;
        }
        self.up(out);
        out.push('\r');
        let prompt = visible(self.current_prompt());
        out.push_str(self.current_prompt());
        match assist {
            Some(a) => out.push_str(&self.highlighted(a)),
            None => out.push_str(&printable(&self.text())),
        }
        let mut cells = prompt + self.line.len();
        self.shown_suggestion = false;
        if let Some(s) = self.suggestion().filter(|_| !at_end) {
            // On the row the line ends on only, so it never wraps.
            let room = width - 1 - cells % width;
            let s: String = printable(&s).chars().take(room).collect();
            if !s.is_empty() {
                cells += width_of(&s);
                if self.options.colour {
                    out.push_str(FAINT);
                    out.push_str(&s);
                    out.push_str(NORMAL);
                } else {
                    out.push_str(&s);
                }
                self.shown_suggestion = true;
            }
        }
        out.push_str("\x1b[J");
        if cells > 0 && cells.is_multiple_of(width) {
            out.push('\n');
        }
        let (end_row, end_col) = (cells / width, cells % width);
        let target = prompt + if at_end { self.line.len() } else { self.cursor };
        let (row, col) = (target / width, target % width);
        if end_row > row {
            out.push_str(&format!("\x1b[{}A", end_row - row));
        }
        if (end_row, end_col) != (row, col) {
            out.push('\r');
            if col > 0 {
                out.push_str(&format!("\x1b[{col}C"));
            }
        }
        self.row = row;
    }

    fn refresh(&mut self, out: &mut String) {
        self.refresh_with(out, None);
    }

    /// Draws the line without a suggestion or search, with the cursor after
    /// it, before the line runs or is dropped.
    fn settle(&mut self, out: &mut String, assist: &mut dyn Assist) {
        self.search = None;
        self.draw(out, Some(assist), true);
    }

    fn insert(&mut self, c: char, out: &mut String, assist: &mut dyn Assist) {
        if self.lines.iter().map(|l| width_of(l) + 1).sum::<usize>() + self.line.len() >= MAX_CHARS
        {
            return;
        }
        // Typing at the end writes just the character when nothing drawn
        // changes with it, so that a long line costs no redraw per key.
        let before = (self.cursor == self.line.len()
            && self.search.is_none()
            && !self.shown_suggestion
            && !c.is_control())
        .then(|| self.highlighted(assist));
        self.line.insert(self.cursor, c);
        self.cursor += 1;
        self.tabbed = false;
        if let Some(before) = before
            && self.suggestion().is_none()
        {
            let after = self.highlighted(assist);
            let typed = printable(c.encode_utf8(&mut [0; 4]));
            if after.strip_prefix(before.as_str()) == Some(typed.as_str()) {
                out.push_str(&typed);
                let width = self.options.width.max(1);
                let cells = visible(self.current_prompt()) + self.line.len();
                if cells.is_multiple_of(width) {
                    out.push('\n');
                }
                self.row = cells / width;
                return;
            }
        }
        self.refresh_with(out, Some(assist));
    }

    fn set_line(&mut self, text: &str, out: &mut String, assist: &mut dyn Assist) {
        self.line = text.chars().take(MAX_CHARS).collect();
        self.cursor = self.line.len();
        self.refresh_with(out, Some(assist));
    }

    fn older(&mut self, out: &mut String, assist: &mut dyn Assist) {
        let typed = self
            .typed
            .get_or_insert_with(|| self.line.iter().collect())
            .clone();
        let mut back = self.back;
        while back < self.history.len() {
            back += 1;
            let h = &self.history[self.history.len() - back];
            if h.starts_with(&typed) && !h.contains('\n') {
                self.back = back;
                let h = h.clone();
                self.set_line(&h, out, assist);
                return;
            }
        }
    }

    fn newer(&mut self, out: &mut String, assist: &mut dyn Assist) {
        let Some(typed) = self.typed.clone() else {
            return;
        };
        let mut back = self.back;
        while back > 1 {
            back -= 1;
            let h = &self.history[self.history.len() - back];
            if h.starts_with(&typed) && !h.contains('\n') {
                self.back = back;
                let h = h.clone();
                self.set_line(&h, out, assist);
                return;
            }
        }
        self.back = 0;
        self.typed = None;
        self.set_line(&typed, out, assist);
    }

    fn search_step(&mut self, older_than: Option<usize>) {
        let Some(s) = &mut self.search else { return };
        let end = older_than.unwrap_or(self.history.len());
        s.found = self.history[..end.min(self.history.len())]
            .iter()
            .rposition(|h| !s.query.is_empty() && h.contains(s.query.as_str()));
    }

    fn end_search(&mut self, accept: bool, out: &mut String, assist: &mut dyn Assist) {
        let Some(s) = self.search.take() else { return };
        if accept && let Some(i) = s.found {
            let h = self.history[i].clone();
            self.line = h.chars().collect();
            self.cursor = self.line.len();
        }
        out.push('\r');
        out.push_str("\x1b[K");
        self.row = 0;
        self.refresh_with(out, Some(assist));
    }

    fn word_start(&self) -> usize {
        let mut i = self.cursor;
        while i > 0 && self.line[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && self.line[i - 1] != ' ' {
            i -= 1;
        }
        i
    }

    fn complete(&mut self, out: &mut String, assist: &mut dyn Assist) {
        let text = self.text();
        let cursor_byte: usize = self.line[..self.cursor].iter().map(|c| c.len_utf8()).sum();
        let c = assist.complete(&text, cursor_byte);
        let start = c.start.min(cursor_byte);
        let typed = &text[start..cursor_byte];
        if c.candidates.is_empty() {
            if c.slow {
                self.list(&["(listing too slow)".into()], &[], out, assist);
            }
            return;
        }
        let common = c
            .candidates
            .iter()
            .map(|(t, _)| t.as_str())
            .reduce(|a, b| {
                let n = a
                    .char_indices()
                    .zip(b.chars())
                    .take_while(|((_, x), y)| x == y)
                    .last()
                    .map_or(0, |((i, x), _)| i + x.len_utf8());
                &a[..n]
            })
            .unwrap_or("");
        let mut insert = String::from(common);
        if c.candidates.len() == 1 && !insert.ends_with('/') {
            insert.push(' ');
        }
        if insert.len() > typed.len() || (c.candidates.len() == 1 && insert != typed) {
            let before: String = text[..start].into();
            let after: String = text[cursor_byte..].into();
            let new = format!("{before}{insert}{after}");
            self.line = new.chars().collect();
            self.cursor = before.chars().count() + insert.chars().count();
            self.tabbed = false;
            self.refresh_with(out, Some(assist));
            return;
        }
        if self.tabbed {
            return; // a second Tab does nothing more
        }
        self.tabbed = true;
        let texts: Vec<String> = c.candidates.iter().map(|(t, _)| t.clone()).collect();
        let summaries: Vec<Option<String>> = c.candidates.iter().map(|(_, s)| s.clone()).collect();
        self.list(&texts, &summaries, out, assist);
    }

    /// Lists candidates below the line, at most one screen, then draws the
    /// line again.
    fn list(
        &mut self,
        texts: &[String],
        summaries: &[Option<String>],
        out: &mut String,
        assist: &mut dyn Assist,
    ) {
        self.settle(out, assist);
        out.push('\n');
        let with_summary = summaries.iter().any(Option::is_some);
        let limit = 24usize;
        if with_summary {
            let items: Vec<(String, String)> = texts
                .iter()
                .zip(summaries)
                .map(|(t, s)| (printable(t), s.clone().unwrap_or_default()))
                .collect();
            let shown = zoraya_command::columns(&items[..items.len().min(limit)], "", 4);
            out.push_str(&shown);
        } else {
            let mut row = 0;
            let mut rows = 0;
            for (i, t) in texts.iter().enumerate() {
                let t = printable(t);
                let w = width_of(&t);
                if i > 0 && row + 2 + w >= self.options.width {
                    out.push('\n');
                    rows += 1;
                    row = 0;
                    if rows >= limit {
                        break;
                    }
                } else if i > 0 {
                    out.push_str("  ");
                    row += 2;
                }
                out.push_str(&t);
                row += w;
            }
            out.push('\n');
        }
        if texts.len() > limit && with_summary {
            out.push_str(&format!("(and {} more)\n", texts.len() - limit));
        }
        self.row = 0;
        out.push_str(self.current_prompt());
        self.refresh_with(out, Some(assist));
    }

    /// Takes one input byte, and writes what the screen needs to `out`.
    pub fn key(&mut self, byte: u8, out: &mut String, assist: &mut dyn Assist) -> Key {
        if byte == CTRL_C {
            self.escape = Escape::None;
            self.utf8.clear();
        }
        match self.escape {
            Escape::Started => {
                self.escape = match byte {
                    b'[' => {
                        self.params.clear();
                        Escape::Csi
                    }
                    b'O' => Escape::Single,
                    _ => {
                        // Esc alone ends a search.
                        if self.search.is_some() {
                            self.end_search(false, out, assist);
                        }
                        Escape::None
                    }
                };
                return Key::Edited;
            }
            Escape::Single => {
                self.escape = Escape::None;
                let b = match byte {
                    b'H' => b'H',
                    b'F' => b'F',
                    b'A' | b'B' | b'C' | b'D' => byte,
                    _ => return Key::Edited,
                };
                return self.csi(b, out, assist);
            }
            Escape::Csi => {
                if (0x40..=0x7e).contains(&byte) {
                    self.escape = Escape::None;
                    return self.csi(byte, out, assist);
                }
                if self.params.len() < 16 {
                    self.params.push(byte as char);
                }
                return Key::Edited;
            }
            Escape::None => {}
        }
        if !self.utf8.is_empty() || byte >= 0x80 {
            self.utf8.push(byte);
            match core::str::from_utf8(&self.utf8) {
                Ok(s) => {
                    let c = s.chars().next().unwrap_or('?');
                    self.utf8.clear();
                    if !c.is_control() && !('\u{80}'..='\u{9f}').contains(&c) {
                        self.typed_char(c, out, assist);
                    }
                }
                Err(e) if e.error_len().is_some() || self.utf8.len() >= 4 => {
                    self.utf8.clear();
                    self.settle(out, assist);
                    out.push_str("\nzosh: input: not UTF-8, ignored\n");
                    self.redraw(out);
                }
                Err(_) => {} // more bytes to come
            }
            return Key::Edited;
        }
        if self.search.is_some() {
            return self.search_key(byte, out, assist);
        }
        match byte {
            b'\r' | b'\n' => self.enter(out, assist),
            CTRL_C => {
                self.settle(out, assist);
                out.push_str("^C\n");
                self.lines.clear();
                self.line.clear();
                self.cursor = 0;
                self.back = 0;
                self.typed = None;
                Key::Interrupt
            }
            CTRL_D => {
                if self.line.is_empty() && self.lines.is_empty() {
                    out.push_str("^D\ntype exit to end the shell\n");
                    out.push_str(self.current_prompt());
                    self.row = 0;
                } else if self.cursor < self.line.len() {
                    self.line.remove(self.cursor);
                    self.refresh_with(out, Some(assist));
                }
                Key::Edited
            }
            CTRL_A => self.csi(b'H', out, assist),
            CTRL_E => self.csi(b'F', out, assist),
            CTRL_B => self.csi(b'D', out, assist),
            CTRL_F => self.csi(b'C', out, assist),
            CTRL_K => {
                self.line.truncate(self.cursor);
                self.refresh_with(out, Some(assist));
                Key::Edited
            }
            CTRL_U => {
                self.line.drain(..self.cursor);
                self.cursor = 0;
                self.refresh_with(out, Some(assist));
                Key::Edited
            }
            CTRL_W => {
                let start = self.word_start();
                self.line.drain(start..self.cursor);
                self.cursor = start;
                self.refresh_with(out, Some(assist));
                Key::Edited
            }
            CTRL_L => {
                out.push_str("\x1b[H\x1b[J");
                self.row = 0;
                out.push_str(self.current_prompt());
                self.refresh_with(out, Some(assist));
                Key::Edited
            }
            CTRL_R => {
                self.settle(out, assist);
                out.push('\n');
                self.search = Some(Search {
                    query: String::new(),
                    found: None,
                });
                self.refresh(out);
                Key::Edited
            }
            TAB => {
                self.complete(out, assist);
                Key::Edited
            }
            BACKSPACE | DELETE => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.line.remove(self.cursor);
                    self.refresh_with(out, Some(assist));
                }
                Key::Edited
            }
            ESC => {
                self.escape = Escape::Started;
                Key::Edited
            }
            b if (0x20..0x7f).contains(&b) => {
                self.typed_char(b as char, out, assist);
                Key::Edited
            }
            _ => Key::Edited,
        }
    }

    fn typed_char(&mut self, c: char, out: &mut String, assist: &mut dyn Assist) {
        if let Some(s) = &mut self.search {
            if s.query.len() < 256 {
                s.query.push(c);
            }
            self.search_step(None);
            self.refresh(out);
            return;
        }
        self.back = 0;
        self.typed = None;
        self.insert(c, out, assist);
    }

    fn search_key(&mut self, byte: u8, out: &mut String, assist: &mut dyn Assist) -> Key {
        match byte {
            CTRL_R => {
                let older = self.search.as_ref().and_then(|s| s.found);
                self.search_step(older);
                self.refresh(out);
                Key::Edited
            }
            BACKSPACE | DELETE => {
                if let Some(s) = &mut self.search {
                    s.query.pop();
                }
                self.search_step(None);
                self.refresh(out);
                Key::Edited
            }
            b'\r' | b'\n' => {
                self.end_search(true, out, assist);
                self.enter(out, assist)
            }
            CTRL_C => {
                self.end_search(false, out, assist);
                self.key(CTRL_C, out, assist)
            }
            ESC => {
                self.escape = Escape::Started;
                Key::Edited
            }
            b if (0x20..0x7f).contains(&b) => {
                self.typed_char(b as char, out, assist);
                Key::Edited
            }
            _ => {
                self.end_search(true, out, assist);
                Key::Edited
            }
        }
    }

    fn csi(&mut self, final_byte: u8, out: &mut String, assist: &mut dyn Assist) -> Key {
        if self.search.is_some() {
            self.end_search(true, out, assist);
        }
        let params = core::mem::take(&mut self.params);
        match (final_byte, params.as_str()) {
            (b'A', _) => self.older(out, assist),
            (b'B', _) => self.newer(out, assist),
            (b'C', _) => {
                if self.cursor < self.line.len() {
                    self.cursor += 1;
                    self.refresh_with(out, Some(assist));
                } else if let Some(s) = self.suggestion() {
                    // Right at the end accepts the suggestion (rule 4).
                    let mut t = self.text();
                    t.push_str(&s);
                    self.set_line(&t, out, assist);
                }
            }
            (b'D', _) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.refresh_with(out, Some(assist));
                }
            }
            (b'H', _) | (b'~', "1" | "7") => {
                self.cursor = 0;
                self.refresh_with(out, Some(assist));
            }
            (b'F', _) | (b'~', "4" | "8") => {
                self.cursor = self.line.len();
                self.refresh_with(out, Some(assist));
            }
            (b'~', "3") if self.cursor < self.line.len() => {
                self.line.remove(self.cursor);
                self.refresh_with(out, Some(assist));
            }
            _ => {}
        }
        Key::Edited
    }

    fn enter(&mut self, out: &mut String, assist: &mut dyn Assist) -> Key {
        self.settle(out, assist);
        out.push('\n');
        let mut entry = self.lines.join("\n");
        if !self.lines.is_empty() {
            entry.push('\n');
        }
        entry.push_str(&self.text());
        if assist.incomplete(&entry) && self.lines.len() < 256 {
            self.lines.push(self.text());
            self.line.clear();
            self.cursor = 0;
            self.row = 0;
            out.push_str(CONTINUATION);
            return Key::Edited;
        }
        self.lines.clear();
        self.line.clear();
        self.cursor = 0;
        self.back = 0;
        self.typed = None;
        Key::Line(entry)
    }
}

/// A parse error as the shell shows it (rule 6): the message, the line, a
/// caret under the place, and a hint.
pub fn parse_error(src: &str, at: usize, problem: &str, hint: Option<&str>) -> String {
    let (line, column) = crate::syntax::position(src, at);
    let text = printable(crate::syntax::line_text(src, line));
    let mut s = format!("zosh: line {line}: {problem}\n  {text}\n  ");
    for _ in 1..column {
        s.push(' ');
    }
    s.push_str("^\n");
    if let Some(h) = hint {
        s.push_str("  hint: ");
        s.push_str(h);
        s.push('\n');
    }
    s
}

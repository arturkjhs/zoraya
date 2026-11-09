//! The text editor's buffer (plan 7f): the text as characters, a cursor
//! and an anchor (the selection is between them), edits that can be
//! undone and redone, and find. Positions are character indices, so a
//! cursor never splits a Czech or Ukrainian letter. No I/O and no drawing:
//! `edit` reads and writes files through the chooser and draws with
//! `zoraya-ui`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub mod view;

/// The most characters a buffer holds (16 MiB of text at most in UTF-8).
pub const MAX_CHARS: usize = 4 << 20;
/// The most edits kept for undo.
pub const MAX_UNDO: usize = 1000;

/// One change: at `at`, `removed` gave way to `inserted`; the cursor and
/// anchor before it, to put back on undo.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    at: usize,
    removed: Vec<char>,
    inserted: Vec<char>,
    before: (usize, usize),
}

/// How the cursor moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    Start,
    End,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Buffer {
    text: Vec<char>,
    cursor: usize,
    anchor: usize,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// The undo depth when last saved; `None` when that state is gone.
    saved_at: Option<usize>,
    /// The column a vertical move keeps.
    column: Option<usize>,
}

impl Buffer {
    /// A buffer holding `text`, unmodified, the cursor at its start.
    pub fn new(text: &str) -> Buffer {
        let mut b = Buffer {
            text: text.chars().take(MAX_CHARS).collect(),
            saved_at: Some(0),
            ..Buffer::default()
        };
        b.cursor = 0;
        b.anchor = 0;
        b
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The selection as a range, empty when there is none.
    pub fn selection(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }

    pub fn selected(&self) -> String {
        let (a, b) = self.selection();
        self.text[a..b].iter().collect()
    }

    /// Whether the text differs from what was last saved (or opened).
    pub fn modified(&self) -> bool {
        self.saved_at != Some(self.undo.len())
    }

    /// The text was saved as it is.
    pub fn saved(&mut self) {
        self.saved_at = Some(self.undo.len());
    }

    /// The lines, without their line ends.
    pub fn lines(&self) -> Vec<String> {
        self.text().split('\n').map(String::from).collect()
    }

    /// The line and the column of character `at`.
    pub fn line_col(&self, at: usize) -> (usize, usize) {
        let before = &self.text[..at.min(self.text.len())];
        let line = before.iter().filter(|c| **c == '\n').count();
        let col = before.iter().rev().take_while(|c| **c != '\n').count();
        (line, col)
    }

    /// The character index of `(line, col)`, the column clamped to the line.
    pub fn at(&self, line: usize, col: usize) -> usize {
        let mut start = 0;
        for _ in 0..line {
            match self.text[start..].iter().position(|c| *c == '\n') {
                Some(p) => start += p + 1,
                None => return self.text.len(),
            }
        }
        let len = self.text[start..]
            .iter()
            .position(|c| *c == '\n')
            .unwrap_or(self.text.len() - start);
        start + col.min(len)
    }

    /// Moves the cursor; with `extend` the anchor stays and the selection
    /// grows. Without, a selection collapses first to its side.
    pub fn move_cursor(&mut self, m: Move, extend: bool) {
        let (a, b) = self.selection();
        let keep_column = matches!(m, Move::Up | Move::Down);
        let to = match m {
            Move::Left if !extend && a != b => a,
            Move::Right if !extend && a != b => b,
            Move::Left => self.cursor.saturating_sub(1),
            Move::Right => (self.cursor + 1).min(self.text.len()),
            Move::Up | Move::Down => {
                let (line, col) = self.line_col(self.cursor);
                let col = *self.column.get_or_insert(col);
                match m {
                    Move::Up if line == 0 => 0,
                    Move::Up => self.at(line - 1, col),
                    _ if line + 1 >= self.lines().len() => self.text.len(),
                    _ => self.at(line + 1, col),
                }
            }
            Move::LineStart => {
                let (line, _) = self.line_col(self.cursor);
                self.at(line, 0)
            }
            Move::LineEnd => {
                let (line, _) = self.line_col(self.cursor);
                self.at(line, usize::MAX)
            }
            Move::Start => 0,
            Move::End => self.text.len(),
        };
        if !keep_column {
            self.column = None;
        }
        self.cursor = to;
        if !extend {
            self.anchor = to;
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// Selects `from..to` (clamped), the cursor at `to`.
    pub fn select(&mut self, from: usize, to: usize) {
        self.anchor = from.min(self.text.len());
        self.cursor = to.min(self.text.len());
    }

    fn apply(&mut self, at: usize, remove: usize, insert: &[char]) -> bool {
        let remove = remove.min(self.text.len() - at);
        if remove == 0 && insert.is_empty() {
            return false;
        }
        if self.text.len() - remove + insert.len() > MAX_CHARS {
            return false;
        }
        let before = (self.cursor, self.anchor);
        let removed: Vec<char> = self
            .text
            .splice(at..at + remove, insert.iter().copied())
            .collect();
        self.cursor = at + insert.len();
        self.anchor = self.cursor;
        self.column = None;
        if self.saved_at.is_some_and(|s| s > self.undo.len()) {
            self.saved_at = None;
        }
        self.redo.clear();
        if self.undo.len() >= MAX_UNDO {
            self.undo.remove(0);
            self.saved_at = self.saved_at.and_then(|s| s.checked_sub(1));
        }
        self.undo.push(Edit {
            at,
            removed,
            inserted: Vec::from(insert),
            before,
        });
        true
    }

    /// Types `s` over the selection.
    pub fn insert(&mut self, s: &str) -> bool {
        let (a, b) = self.selection();
        let chars: Vec<char> = s
            .chars()
            .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
            .collect();
        self.apply(a, b - a, &chars)
    }

    /// Backspace: the selection, or the character before the cursor.
    pub fn backspace(&mut self) -> bool {
        let (a, b) = self.selection();
        if a != b {
            return self.apply(a, b - a, &[]);
        }
        if a == 0 {
            return false;
        }
        self.apply(a - 1, 1, &[])
    }

    /// Delete: the selection, or the character after the cursor.
    pub fn delete(&mut self) -> bool {
        let (a, b) = self.selection();
        if a != b {
            return self.apply(a, b - a, &[]);
        }
        self.apply(a, 1, &[])
    }

    /// Cut: the selection, removed; `None` when nothing is selected.
    pub fn cut(&mut self) -> Option<String> {
        let s = self.selected();
        if s.is_empty() {
            return None;
        }
        self.backspace();
        Some(s)
    }

    pub fn undo(&mut self) -> bool {
        let Some(e) = self.undo.pop() else {
            return false;
        };
        self.text
            .splice(e.at..e.at + e.inserted.len(), e.removed.iter().copied());
        (self.cursor, self.anchor) = e.before;
        self.column = None;
        self.redo.push(e);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(e) = self.redo.pop() else {
            return false;
        };
        self.text
            .splice(e.at..e.at + e.removed.len(), e.inserted.iter().copied());
        self.cursor = e.at + e.inserted.len();
        self.anchor = self.cursor;
        self.column = None;
        self.undo.push(e);
        true
    }

    /// Finds `needle` after the cursor (from the start again past the
    /// end) and selects it; whether it was found.
    pub fn find(&mut self, needle: &str) -> bool {
        let n: Vec<char> = needle.chars().collect();
        if n.is_empty() || n.len() > self.text.len() {
            return false;
        }
        let from = self.selection().1;
        let last = self.text.len() - n.len();
        let found = (from..=last)
            .chain(0..from.min(last + 1))
            .find(|i| self.text[*i..*i + n.len()] == n[..]);
        match found {
            Some(i) => {
                self.select(i, i + n.len());
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

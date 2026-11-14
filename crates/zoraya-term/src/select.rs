//! Selecting text with the pointer and copying it (ADR-0332 item 8,
//! ADR-0384, INV-0332-7). A selection is a stretch of the view in reading
//! order, between two cells inclusive. It lives while the cells under it do:
//! any change to a row it covers, a scroll of the view or a resize ends it
//! (`Term::touch`), so what is copied is what was seen. The text copied holds a
//! space for every cell drawn invisible or with the foreground the same as
//! the background, and for every control character.

use alloc::string::String;

use zoraya_proto_term::paste::invisible_format;

use crate::Look;
use crate::cell::TAIL;
use crate::term::Term;

/// A selection: where it began, where it is now, and whether the button
/// that makes it is still down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Selection {
    anchor: (usize, usize),
    head: (usize, usize),
    live: bool,
}

impl Selection {
    /// The ends in reading order, as `(column, row)`.
    fn ends(&self) -> ((usize, usize), (usize, usize)) {
        let key = |p: (usize, usize)| (p.1, p.0);
        if key(self.anchor) <= key(self.head) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub(crate) fn covers_row(&self, y: usize) -> bool {
        let (first, last) = self.ends();
        (first.1..=last.1).contains(&y)
    }

    fn contains(&self, x: usize, y: usize) -> bool {
        let (first, last) = self.ends();
        (first.1, first.0) <= (y, x) && (y, x) <= (last.1, last.0)
    }

    const fn is_empty(&self) -> bool {
        self.anchor.0 == self.head.0 && self.anchor.1 == self.head.1
    }
}

/// The most a channel may differ for a foreground that is on its background to
/// be taken as unseen (ADR-0384): colours that close read as one to a person,
/// and a program that draws text one unit from its ground is hiding it.
pub const UNSEEN_WITHIN: u8 = 8;

/// Whether text drawn in `fg` on `bg` (pixels, `0x00RRGGBB`) can be seen: some
/// channel differs by more than [`UNSEEN_WITHIN`].
pub fn seen(fg: u32, bg: u32) -> bool {
    [0, 8, 16].into_iter().any(|shift| {
        ((fg >> shift) & 0xff).abs_diff((bg >> shift) & 0xff) > u32::from(UNSEEN_WITHIN)
    })
}

impl Term {
    /// Begins a selection at a cell of the view, ending any earlier one.
    pub fn select_start(&mut self, x: usize, y: usize) {
        let at = self.clamp_cell(x, y);
        self.select_clear();
        self.selection = Some(Selection {
            anchor: at,
            head: at,
            live: true,
        });
        self.mark(at.1);
    }

    /// Moves the selection's moving end to a cell, while it is being made.
    pub fn select_extend(&mut self, x: usize, y: usize) {
        let at = self.clamp_cell(x, y);
        let Some(s) = self.selection.as_mut().filter(|s| s.live) else {
            return;
        };
        let (first, last) = s.ends();
        s.head = at;
        let (a, b) = s.ends();
        let (lo, hi) = (first.1.min(a.1), last.1.max(b.1));
        for row in lo..=hi {
            self.mark(row);
        }
    }

    /// The button is up: the selection stays, unless it is one cell (a click).
    pub fn select_end(&mut self) {
        if let Some(s) = self.selection.as_mut() {
            s.live = false;
            if s.is_empty() {
                self.select_clear();
            }
        }
    }

    /// No selection.
    pub fn select_clear(&mut self) {
        if let Some(s) = self.selection.take() {
            let (first, last) = s.ends();
            for row in first.1..=last.1 {
                self.mark(row);
            }
        }
    }

    /// Whether anything is selected.
    pub fn has_selection(&self) -> bool {
        self.selection.is_some_and(|s| !s.is_empty())
    }

    /// Whether the cell at `(x, y)` of the view is in the selection; the right
    /// half of a wide character is where its left half is.
    pub fn selected(&self, x: usize, y: usize) -> bool {
        let Some(s) = self.selection.filter(|s| !s.is_empty()) else {
            return false;
        };
        let x = if x > 0 && x < self.columns && y < self.rows && self.shown(y)[x].has(TAIL) {
            x - 1
        } else {
            x
        };
        s.contains(x, y)
    }

    fn clamp_cell(&self, x: usize, y: usize) -> (usize, usize) {
        let (x, y) = (x.min(self.columns - 1), y.min(self.rows - 1));
        // The right half of a wide character is the character.
        if x > 0 && self.shown(y)[x].has(TAIL) {
            (x - 1, y)
        } else {
            (x, y)
        }
    }

    /// The selected text: rows end with a line feed, a blank cell at a row's
    /// end is dropped, a wide character is one character, its marks follow
    /// it; a cell drawn invisible (or with the foreground of its background,
    /// as `look` draws it) and a control character are a space
    /// (INV-0332-7). Nothing when nothing is selected.
    pub fn selected_text(&self, look: &Look) -> String {
        let Some(s) = self.selection.filter(|s| !s.is_empty()) else {
            return String::new();
        };
        let (first, last) = s.ends();
        let mut text = String::new();
        for y in first.1..=last.1 {
            let from = if y == first.1 { first.0 } else { 0 };
            let to = if y == last.1 {
                last.0
            } else {
                self.columns - 1
            };
            let row = self.shown(y);
            let mut line = String::new();
            // How long the line was when the last cell that is not a plain
            // blank was added: what follows is trimmed.
            let mut keep = 0;
            for cell in &row[from..=to] {
                if cell.has(TAIL) {
                    continue;
                }
                let (fg, bg) = self.colours_of(look, cell);
                let shown = seen(fg.pixel(), bg.pixel())
                    && !cell.ch.is_control()
                    && !invisible_format(cell.ch);
                let c = if shown { cell.ch } else { ' ' };
                line.push(c);
                if shown {
                    line.extend(
                        cell.marks()
                            .filter(|m| !m.is_control() && !invisible_format(*m)),
                    );
                }
                // A hidden cell that held something is not trimmed (it was
                // there, and its place in the line with it); a blank is.
                if c != ' ' || cell.ch != ' ' {
                    keep = line.len();
                }
            }
            line.truncate(keep);
            text.push_str(&line);
            if y != last.1 {
                text.push('\n');
            }
        }
        text
    }
}

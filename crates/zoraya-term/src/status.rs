//! The status row (ADR-0337 items 3 and 5, INV-0337-5): one row below the
//! grid that belongs to the window. Its cells change only by the shell's
//! `Status` message ([`StatusRow::set`]); no output sequence reaches them,
//! because the row is not in the grid the parser writes. The text is the
//! shell's, drawn as plain cells: a control character is a space, a
//! character that does not take one column is a `?`, and what does not fit
//! is cut, so that nothing in it can move the cursor, change a colour or draw
//! outside the row.

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_proto_term::{STATUS_FIELD, STATUS_NOTICE};

use crate::Look;
use crate::cell::Cell;

/// The words at the right end of a notice (ADR-0337 item 3).
pub const HINT: &str = "System+G: give";

/// The row's text and how it is drawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusRow {
    kind: u32,
    text: String,
    dirty: bool,
}

/// Whether `c` is dropped from the text: bidirectional controls and
/// zero-width characters can make a shown text read as another.
fn dropped(c: char) -> bool {
    matches!(
        u32::from(c),
        0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x2064 | 0x2066..=0x2069 | 0x061c | 0xfeff
    )
}

/// The text as cells may show it: a control character a space, any
/// character of other than one column a `?`, bidirectional and zero-width
/// characters gone.
fn clean(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !dropped(*c))
        .map(|c| {
            if c.is_control() {
                ' '
            } else if zoraya_width::columns(c) == 1 {
                c
            } else {
                '?'
            }
        })
        .collect()
}

impl StatusRow {
    /// The shell's `Status`: `kind` and `bytes` (UTF-8, cleaned here).
    pub fn set(&mut self, kind: u32, bytes: &[u8]) {
        let text = clean(bytes);
        if (kind, &text) != (self.kind, &self.text) {
            self.kind = kind;
            self.text = text;
            self.dirty = true;
        }
    }

    /// Nothing is shown.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The text as it is (cleaned).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Marks the row to be drawn again (the window changed size).
    pub fn redraw(&mut self) {
        self.dirty = true;
    }

    /// The row's characters for a row of `columns` cells: the text from the
    /// left, cut at the row; a notice has [`HINT`] at its right end when the
    /// two fit with a space between; a field ends in the cursor, a block.
    pub fn chars(&self, columns: usize) -> Vec<char> {
        let mut row: Vec<char> = alloc::vec![' '; columns];
        if self.text.is_empty() {
            return row;
        }
        let hint: Vec<char> = HINT.chars().collect();
        let text: Vec<char> = self.text.chars().collect();
        let room = if self.kind == STATUS_NOTICE && text.len() + 1 + hint.len() <= columns {
            columns - hint.len() - 1
        } else {
            columns
        };
        // A field keeps its end in view, where the cursor is.
        let from = if self.kind == STATUS_FIELD && text.len() >= columns {
            text.len() + 1 - columns
        } else {
            0
        };
        for (cell, c) in row.iter_mut().zip(text.iter().skip(from).take(room)) {
            *cell = *c;
        }
        if self.kind == STATUS_NOTICE && room < columns {
            for (i, c) in hint.iter().enumerate() {
                row[columns - hint.len() + i] = *c;
            }
        }
        row
    }

    /// Where the field's cursor is, in a row of `columns` cells.
    pub fn cursor(&self, columns: usize) -> Option<usize> {
        (self.kind == STATUS_FIELD && columns > 0)
            .then(|| self.text.chars().count().min(columns - 1))
    }

    /// Draws the row at grid row `y` of the surface if it changed; says
    /// whether it did. The row is drawn in the selection's colours so that
    /// it reads as the shell's and not as the program's screen.
    pub fn render(
        &mut self,
        look: &mut Look,
        pixels: &mut [u32],
        stride: usize,
        columns: usize,
        y: usize,
    ) -> bool {
        if !self.dirty {
            return false;
        }
        self.dirty = false;
        let chars = self.chars(columns);
        let cursor = self.cursor(columns);
        // An empty row is the window's background, as the padding is.
        let (fg, bg) = if self.text.is_empty() {
            (look.fg, look.bg)
        } else {
            (look.sel_fg, look.sel_bg)
        };
        for (x, c) in chars.iter().enumerate() {
            let colours = if cursor == Some(x) {
                (bg, fg)
            } else {
                (fg, bg)
            };
            look.draw_cell_coloured(pixels, stride, x, y, &Cell::new(*c), colours, false);
        }
        true
    }
}

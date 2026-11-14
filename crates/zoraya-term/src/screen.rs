//! Editing the grid: what the sequences of ADR-0332 item 2 do to cells, the
//! cursor, the scroll region and the screens.

use crate::cell::{Cell, TAIL, WIDE};
use crate::term::{Charset, Pen, Saved, Term};

/// A cut wide character leaves no half: a left half without its right, or a
/// right half without its left, becomes a blank with its background.
pub(crate) fn fix_cells(row: &mut [Cell]) {
    for x in 0..row.len() {
        let cell = row[x];
        let head_ok = x + 1 < row.len() && row[x + 1].has(TAIL);
        let tail_ok = x > 0 && row[x - 1].has(WIDE);
        if (cell.has(WIDE) && !head_ok) || (cell.has(TAIL) && !tail_ok) {
            row[x] = Cell::erased(cell.bg());
        }
    }
}

/// xterm's DEC special graphics for 0x5f to 0x7e (`acsc`).
fn graphics(b: u8) -> char {
    const SET: [char; 32] = [
        '\u{a0}', '\u{25c6}', '\u{2592}', '\u{2409}', '\u{240c}', '\u{240d}', '\u{240a}', '\u{b0}',
        '\u{b1}', '\u{2424}', '\u{240b}', '\u{2518}', '\u{2510}', '\u{250c}', '\u{2514}',
        '\u{253c}', '\u{23ba}', '\u{23bb}', '\u{2500}', '\u{23bc}', '\u{23bd}', '\u{251c}',
        '\u{2524}', '\u{2534}', '\u{252c}', '\u{2502}', '\u{2264}', '\u{2265}', '\u{3c0}',
        '\u{2260}', '\u{a3}', '\u{b7}',
    ];
    match b {
        0x5f..=0x7e => SET[usize::from(b - 0x5f)],
        _ => char::from(b),
    }
}

impl Term {
    /// A blank as an erase leaves it: the pen's background, nothing else.
    pub(crate) fn blank(&self) -> Cell {
        Cell::erased(self.pen.bg)
    }

    pub(crate) fn fix_row(&mut self, y: usize) {
        let c = self.columns;
        fix_cells(&mut self.screen[y * c..(y + 1) * c]);
    }

    /// The character for byte `b` under the set in use.
    pub(crate) fn map_ascii(&self, b: u8) -> char {
        let set = self.g[usize::from(self.shifted)];
        match set {
            Charset::Graphics => graphics(b),
            Charset::Ascii => char::from(b),
        }
    }

    /// A cut wide character at `x` of the cursor's row leaves no half.
    fn unwide(&mut self, x: usize) {
        let at = self.y * self.columns + x;
        let cell = self.screen[at];
        if cell.has(TAIL) && x > 0 {
            self.screen[at - 1] = Cell::erased(self.screen[at - 1].bg());
        }
        if cell.has(WIDE) && x + 1 < self.columns {
            self.screen[at + 1] = Cell::erased(self.screen[at + 1].bg());
        }
    }

    /// Prints one character at the cursor (ADR-0332 item 9): a wide one
    /// takes two cells and wraps whole, a combining one joins the cell
    /// before it.
    pub(crate) fn put(&mut self, ch: char) {
        let mut w = zoraya_width::columns(ch);
        if w == 2 && self.columns < 2 {
            // It can never fit: it takes the one cell there is.
            w = 1;
        }
        if w == 0 {
            self.attach(ch);
            return;
        }
        self.last = Some(ch);
        if self.wrap {
            if self.modes.autowrap {
                self.wrap_line();
            }
            self.wrap = false;
        }
        if w == 2 && self.x + 2 > self.columns {
            if !self.modes.autowrap {
                return;
            }
            // The last cell stays blank and the character goes to the next row.
            self.unwide(self.x);
            let at = self.y * self.columns + self.x;
            self.screen[at] = self.blank();
            self.wrap_line();
        }
        if self.modes.insert {
            self.insert_cells(w);
        }
        self.unwide(self.x);
        if w == 2 {
            self.unwide(self.x + 1);
        }
        let at = self.y * self.columns + self.x;
        let base = Cell::new(ch)
            .with_fg(self.pen.fg)
            .with_bg(self.pen.bg)
            .with_attrs(self.pen.attrs);
        if w == 2 {
            self.screen[at] = base.with_attrs(self.pen.attrs | WIDE);
            self.screen[at + 1] = Cell::new(' ')
                .with_fg(self.pen.fg)
                .with_bg(self.pen.bg)
                .with_attrs(self.pen.attrs | TAIL);
        } else {
            self.screen[at] = base;
        }
        self.touch(self.y);
        if self.x + w >= self.columns {
            self.x = self.columns - 1;
            self.wrap = self.modes.autowrap;
        } else {
            self.x += w;
        }
    }

    /// A combining mark joins the cell before the cursor, or the last cell
    /// of the row when a wrap is pending; at the start of a row it is
    /// dropped, as is a mark that does not fit.
    fn attach(&mut self, mark: char) {
        let x = if self.wrap {
            self.x
        } else if self.x > 0 {
            self.x - 1
        } else {
            return;
        };
        let mut at = self.y * self.columns + x;
        if self.screen[at].has(TAIL) && x > 0 {
            at -= 1;
        }
        if self.screen[at].push_mark(mark) {
            self.touch(self.y);
        }
    }

    /// Wraps to the start of the next row, scrolling at the region's end.
    pub(crate) fn wrap_line(&mut self) {
        self.x = 0;
        self.wrap = false;
        self.index();
    }

    /// Moves down one row, scrolling the region up at its bottom (IND, and
    /// what a line feed does: the column stays).
    pub(crate) fn index(&mut self) {
        self.touch(self.y);
        if self.y == self.bottom {
            let (top, bottom) = (self.top, self.bottom);
            self.scroll_up_in(top, bottom, 1, true);
        } else if self.y + 1 < self.rows {
            self.y += 1;
        }
        self.touch(self.y);
    }

    /// Moves up one row, scrolling the region down at its top (RI).
    pub(crate) fn reverse_index(&mut self) {
        self.touch(self.y);
        if self.y == self.top {
            let (top, bottom) = (self.top, self.bottom);
            self.scroll_down_in(top, bottom, 1);
        } else {
            self.y = self.y.saturating_sub(1);
        }
        self.touch(self.y);
    }

    pub(crate) fn carriage_return(&mut self) {
        self.x = 0;
        self.wrap = false;
        self.touch(self.y);
    }

    fn push_history(&mut self, row: usize) {
        let c = self.columns;
        if self.capacity == 0 {
            return;
        }
        let line = &self.screen[row * c..(row + 1) * c];
        if self.kept < self.capacity {
            self.history.extend_from_slice(line);
            self.kept += 1;
            self.next = self.kept % self.capacity;
        } else {
            self.history[self.next * c..(self.next + 1) * c].copy_from_slice(line);
            self.next = (self.next + 1) % self.capacity;
        }
    }

    /// Scrolls rows `top..=bottom` up by `n`. Rows that leave the top of the
    /// whole primary screen enter the scrollback when `history` is set.
    pub(crate) fn scroll_up_in(&mut self, top: usize, bottom: usize, n: usize, history: bool) {
        let n = n.min(bottom + 1 - top);
        if n == 0 {
            return;
        }
        let c = self.columns;
        if history && top == 0 && bottom == self.rows - 1 && !self.alt {
            for r in 0..n {
                self.push_history(r);
            }
        }
        self.screen
            .copy_within((top + n) * c..(bottom + 1) * c, top * c);
        let blank = self.blank();
        self.screen[(bottom + 1 - n) * c..(bottom + 1) * c].fill(blank);
        for y in top..=bottom {
            self.touch(y);
        }
    }

    pub(crate) fn scroll_down_in(&mut self, top: usize, bottom: usize, n: usize) {
        let n = n.min(bottom + 1 - top);
        if n == 0 {
            return;
        }
        let c = self.columns;
        self.screen
            .copy_within(top * c..(bottom + 1 - n) * c, (top + n) * c);
        let blank = self.blank();
        self.screen[top * c..(top + n) * c].fill(blank);
        for y in top..=bottom {
            self.touch(y);
        }
    }

    /// IL: inserts `n` blank rows at the cursor's, inside the region.
    pub(crate) fn insert_lines(&mut self, n: usize) {
        if self.y < self.top || self.y > self.bottom {
            return;
        }
        let (y, bottom) = (self.y, self.bottom);
        self.scroll_down_in(y, bottom, n);
        self.carriage_return();
    }

    /// DL.
    pub(crate) fn delete_lines(&mut self, n: usize) {
        if self.y < self.top || self.y > self.bottom {
            return;
        }
        let (y, bottom) = (self.y, self.bottom);
        self.scroll_up_in(y, bottom, n, false);
        self.carriage_return();
    }

    /// Opens `n` blank cells at the cursor, pushing the rest of the row
    /// right and off its end (ICH, and IRM's insertion).
    pub(crate) fn insert_cells(&mut self, n: usize) {
        let c = self.columns;
        let n = n.min(c - self.x);
        let row = self.y * c;
        let blank = self.blank();
        self.screen
            .copy_within(row + self.x..row + c - n, row + self.x + n);
        self.screen[row + self.x..row + self.x + n].fill(blank);
        self.fix_row(self.y);
        self.touch(self.y);
    }

    /// DCH.
    pub(crate) fn delete_cells(&mut self, n: usize) {
        let c = self.columns;
        let n = n.min(c - self.x);
        let row = self.y * c;
        let blank = self.blank();
        self.screen
            .copy_within(row + self.x + n..row + c, row + self.x);
        self.screen[row + c - n..row + c].fill(blank);
        self.fix_row(self.y);
        self.touch(self.y);
    }

    /// ECH: blanks `n` cells from the cursor, which stays.
    pub(crate) fn erase_cells(&mut self, n: usize) {
        let to = (self.x + n).min(self.columns);
        self.erase_in_row(self.y, self.x, to);
    }

    /// Blanks cells `from..to` of row `y`.
    pub(crate) fn erase_in_row(&mut self, y: usize, from: usize, to: usize) {
        let c = self.columns;
        let blank = self.blank();
        self.screen[y * c + from..y * c + to].fill(blank);
        self.fix_row(y);
        self.touch(y);
    }

    /// EL.
    pub(crate) fn erase_line(&mut self, mode: usize) {
        match mode {
            0 => self.erase_in_row(self.y, self.x, self.columns),
            1 => self.erase_in_row(self.y, 0, self.x + 1),
            2 => self.erase_in_row(self.y, 0, self.columns),
            _ => {}
        }
    }

    /// ED; 3 also drops the scrollback.
    pub(crate) fn erase_display(&mut self, mode: usize) {
        let (c, rows, y) = (self.columns, self.rows, self.y);
        match mode {
            0 => {
                self.erase_in_row(y, self.x, c);
                for r in y + 1..rows {
                    self.erase_in_row(r, 0, c);
                }
            }
            1 => {
                for r in 0..y {
                    self.erase_in_row(r, 0, c);
                }
                self.erase_in_row(y, 0, self.x + 1);
            }
            2 | 3 => {
                for r in 0..rows {
                    self.erase_in_row(r, 0, c);
                }
                if mode == 3 {
                    self.history = alloc::vec::Vec::new();
                    self.kept = 0;
                    self.next = 0;
                    self.view = 0;
                }
            }
            _ => {}
        }
        self.touch_all();
    }

    /// HT, `n` times forward.
    pub(crate) fn tab_forward(&mut self, n: usize) {
        for _ in 0..n {
            let from = self.x + 1;
            self.x = (from..self.columns)
                .find(|&i| self.tabs[i])
                .unwrap_or(self.columns - 1);
        }
        self.wrap = false;
        self.touch(self.y);
    }

    /// CBT, `n` times back.
    pub(crate) fn tab_back(&mut self, n: usize) {
        for _ in 0..n {
            self.x = (0..self.x).rev().find(|&i| self.tabs[i]).unwrap_or(0);
        }
        self.wrap = false;
        self.touch(self.y);
    }

    /// The cursor at row `row` and column `col`, counted from 0 and, with
    /// origin mode, from the region's top.
    pub(crate) fn move_to(&mut self, row: usize, col: usize) {
        self.y = if self.modes.origin {
            (self.top + row).min(self.bottom)
        } else {
            row.min(self.rows - 1)
        };
        self.x = col.min(self.columns - 1);
        self.wrap = false;
    }

    pub(crate) fn cursor_up(&mut self, n: usize) {
        let limit = if self.y >= self.top { self.top } else { 0 };
        self.y = self.y.saturating_sub(n).max(limit);
        self.wrap = false;
    }

    pub(crate) fn cursor_down(&mut self, n: usize) {
        let limit = if self.y <= self.bottom {
            self.bottom
        } else {
            self.rows - 1
        };
        self.y = (self.y + n).min(limit);
        self.wrap = false;
    }

    /// DECSTBM: rows `top..=bottom` (counted from 0) become the region, if
    /// they make at least two rows, and the cursor goes home.
    pub(crate) fn set_region(&mut self, top: usize, bottom: usize) {
        if top < bottom && bottom < self.rows {
            self.top = top;
            self.bottom = bottom;
            self.move_to(0, 0);
        }
    }

    pub(crate) fn save_cursor(&mut self, slot: usize) {
        self.saved[slot] = Saved {
            x: self.x,
            y: self.y,
            pen: self.pen,
            wrap: self.wrap,
            origin: self.modes.origin,
            g: self.g,
            shifted: self.shifted,
        };
    }

    pub(crate) fn restore_cursor(&mut self, slot: usize) {
        let s = self.saved[slot];
        self.x = s.x.min(self.columns - 1);
        self.y = s.y.min(self.rows - 1);
        self.pen = s.pen;
        self.wrap = s.wrap;
        self.modes.origin = s.origin;
        self.g = s.g;
        self.shifted = s.shifted;
    }

    /// Switches to the alternate screen, blank, with no scrollback. With
    /// `save` the cursor is kept for [`Term::leave_alt`] (mode 1049).
    pub(crate) fn enter_alt(&mut self, save: bool) {
        if self.alt {
            return;
        }
        if save {
            self.save_cursor(2);
        }
        self.alt_saved_cursor = save;
        core::mem::swap(&mut self.screen, &mut self.other);
        let blank = self.blank();
        self.screen.fill(blank);
        self.alt = true;
        self.view = 0;
        self.wrap = false;
        self.touch_all();
    }

    pub(crate) fn leave_alt(&mut self) {
        if !self.alt {
            return;
        }
        core::mem::swap(&mut self.screen, &mut self.other);
        self.alt = false;
        if self.alt_saved_cursor {
            self.restore_cursor(2);
        }
        self.alt_saved_cursor = false;
        self.wrap = false;
        self.touch_all();
    }

    /// DECSTR: modes, pen, sets, region and saved cursors back to their
    /// defaults; the screen and the cursor's place stay.
    pub(crate) fn soft_reset(&mut self) {
        self.modes = crate::term::Modes::DEFAULT;
        self.pen = Pen::DEFAULT;
        self.g = [Charset::Ascii; 2];
        self.shifted = false;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.saved = [Saved::HOME; 3];
        self.shape = 0;
        self.wrap = false;
        self.touch_all();
    }

    /// RIS: as new, except the scrollback and the size.
    pub(crate) fn full_reset(&mut self) {
        self.leave_alt();
        self.soft_reset();
        self.overrides = [None; 256];
        self.tabs = (0..self.columns).map(|x| x % 8 == 0).collect();
        self.pen = Pen::DEFAULT;
        let blank = Cell::BLANK;
        self.screen.fill(blank);
        self.other.fill(blank);
        self.x = 0;
        self.y = 0;
        self.last = None;
        self.touch_all();
    }
}

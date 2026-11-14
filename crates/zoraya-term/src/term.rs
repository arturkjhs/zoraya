//! The terminal's state: the grids, the cursor, the modes and the parser's
//! registers (ADR-0332). The parser is in `parse`, the editing of the grid
//! in `screen`, drawing here.

use alloc::vec;
use alloc::vec::Vec;

use zoraya_tokens::Color;

use crate::cell::{Cell, Colour, FAINT, REVERSE, TAIL, WIDE};
use crate::{Look, MAX_PARAMS, MAX_SCROLLBACK, SCROLLBACK};

/// The modes a program sets (ADR-0332 items 2 and 3), which `term` reads to
/// encode keys, mice, focus and paste and to hold its drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    /// DECCKM (1): arrows, Home and End in their `SS3` forms.
    pub app_cursor: bool,
    /// DECSCNM (5): the whole screen drawn reversed.
    pub reverse_screen: bool,
    /// DECOM (6): the cursor is addressed from the scroll region's top.
    pub origin: bool,
    /// DECAWM (7): a character past the last column wraps.
    pub autowrap: bool,
    /// DECTCEM (25).
    pub cursor_visible: bool,
    /// `ESC =` and `ESC >`.
    pub app_keypad: bool,
    /// IRM (4): characters are inserted, not written over.
    pub insert: bool,
    /// 2004: a paste is bracketed.
    pub bracketed_paste: bool,
    /// 1000: button presses and releases are reported.
    pub mouse_press: bool,
    /// 1002: and motion with a button held.
    pub mouse_drag: bool,
    /// 1006: reports use the SGR encoding, the only one there is.
    pub mouse_sgr: bool,
    /// 1004: focus in and out are reported.
    pub focus_events: bool,
    /// 2026: drawing is held until it is reset, for at most
    /// [`crate::SYNC_HOLD_MS`].
    pub synchronized: bool,
}

impl Modes {
    pub const DEFAULT: Modes = Modes {
        app_cursor: false,
        reverse_screen: false,
        origin: false,
        autowrap: true,
        cursor_visible: true,
        app_keypad: false,
        insert: false,
        bracketed_paste: false,
        mouse_press: false,
        mouse_drag: false,
        mouse_sgr: false,
        focus_events: false,
        synchronized: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Charset {
    Ascii,
    /// DEC special graphics: the line-drawing set (`acsc`).
    Graphics,
}

/// The rendition the next characters get (SGR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pen {
    pub fg: Colour,
    pub bg: Colour,
    pub attrs: u16,
}

impl Pen {
    pub const DEFAULT: Pen = Pen {
        fg: Colour::Default,
        bg: Colour::Default,
        attrs: 0,
    };
}

/// What `ESC 7` and `CSI ? 1048 h` keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Saved {
    pub x: usize,
    pub y: usize,
    pub pen: Pen,
    pub wrap: bool,
    pub origin: bool,
    pub g: [Charset; 2],
    pub shifted: bool,
}

impl Saved {
    pub const HOME: Saved = Saved {
        x: 0,
        y: 0,
        pen: Pen::DEFAULT,
        wrap: false,
        origin: false,
        g: [Charset::Ascii; 2],
        shifted: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Ground,
    /// After `ESC`.
    Escape,
    /// After `ESC` and an intermediate byte: one more byte ends it.
    EscapeIntermediate,
    /// `ESC [`, collecting parameters and intermediates.
    Csi,
    /// A `CSI` that cannot be valid: its final byte is skipped.
    CsiIgnore,
    /// `OSC`: collected up to a bound, run at its end.
    Osc,
    /// `DCS`, `SOS`, `PM` or `APC`: skipped to the string terminator.
    Skip,
}

/// What [`Term::restore`] cleared, to be set again by [`Term::reinstate`]
/// when a stopped line comes back to the front (ADR-0331 item 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restored {
    modes: Modes,
    alt: bool,
    top: usize,
    bottom: usize,
    shape: u8,
}

/// The terminal's state.
#[derive(Clone)]
pub struct Term {
    pub(crate) columns: usize,
    pub(crate) rows: usize,
    /// The screen shown: the primary, or the alternate while `alt` is set.
    pub(crate) screen: Vec<Cell>,
    /// The screen not shown.
    pub(crate) other: Vec<Cell>,
    pub(crate) alt: bool,
    /// The alternate screen was entered with 1049: leaving it restores the
    /// cursor.
    pub(crate) alt_saved_cursor: bool,
    /// Lines that scrolled off the top of the primary screen, a ring of at
    /// most `capacity`, grown as lines arrive.
    pub(crate) history: Vec<Cell>,
    pub(crate) capacity: usize,
    pub(crate) kept: usize,
    pub(crate) next: usize,
    /// How far back the view is, in lines; 0 is the live screen.
    pub(crate) view: usize,
    pub(crate) x: usize,
    pub(crate) y: usize,
    /// The cursor is past the last column: the next character wraps first.
    pub(crate) wrap: bool,
    pub(crate) pen: Pen,
    pub(crate) modes: Modes,
    pub(crate) g: [Charset; 2],
    pub(crate) shifted: bool,
    /// The scroll region, first and last row.
    pub(crate) top: usize,
    pub(crate) bottom: usize,
    pub(crate) tabs: Vec<bool>,
    /// `ESC 7` on the primary and on the alternate screen, and 1048/1049.
    pub(crate) saved: [Saved; 3],
    /// `OSC 4`'s palette entries, undone by `OSC 104` and [`Term::restore`].
    pub(crate) overrides: [Option<(u8, u8, u8)>; 256],
    /// `CSI Ps SP q`: 0 to 6.
    pub(crate) shape: u8,
    /// The last character printed, for `REP`.
    pub(crate) last: Option<char>,
    // The parser.
    pub(crate) state: State,
    pub(crate) params: [u16; MAX_PARAMS],
    /// Bit `i`: parameter `i` was introduced by a colon.
    pub(crate) sub: u32,
    pub(crate) nparams: usize,
    pub(crate) cur_open: bool,
    pub(crate) more: bool,
    pub(crate) next_is_sub: bool,
    pub(crate) overflow: bool,
    pub(crate) marker: u8,
    pub(crate) inter: [u8; 2],
    pub(crate) ninter: usize,
    /// Bytes of the sequence or string so far.
    pub(crate) seq_len: usize,
    /// An `ESC` came inside a string: `\` ends it, anything else abandons it.
    pub(crate) string_escape: bool,
    pub(crate) osc: Vec<u8>,
    /// The `OSC` being collected overran its bound: it is swallowed, not run.
    pub(crate) osc_over: bool,
    /// UTF-8 being decoded: the value so far and the bytes still needed.
    pub(crate) utf8: u32,
    pub(crate) needed: u8,
    /// The smallest value a sequence of this length may encode: anything
    /// below it is an overlong encoding.
    pub(crate) utf8_min: u32,
    /// Rows of the view that changed since the last render.
    pub(crate) dirty: Vec<bool>,
}

fn default_tabs(columns: usize) -> Vec<bool> {
    (0..columns).map(|x| x % 8 == 0).collect()
}

impl Term {
    /// A terminal of `columns × rows`, each at least 1, with the default
    /// scrollback.
    pub fn new(columns: usize, rows: usize) -> Term {
        Term::with_scrollback(columns, rows, SCROLLBACK)
    }

    /// A terminal keeping up to `lines` lines of scrollback, at most
    /// [`MAX_SCROLLBACK`] (the setting `term.scrollback`, ADR-0332 item 11).
    pub fn with_scrollback(columns: usize, rows: usize, lines: usize) -> Term {
        let (columns, rows) = (columns.max(1), rows.max(1));
        Term {
            columns,
            rows,
            screen: vec![Cell::BLANK; columns * rows],
            other: vec![Cell::BLANK; columns * rows],
            alt: false,
            alt_saved_cursor: false,
            history: Vec::new(),
            capacity: lines.min(MAX_SCROLLBACK),
            kept: 0,
            next: 0,
            view: 0,
            x: 0,
            y: 0,
            wrap: false,
            pen: Pen::DEFAULT,
            modes: Modes::DEFAULT,
            g: [Charset::Ascii; 2],
            shifted: false,
            top: 0,
            bottom: rows - 1,
            tabs: default_tabs(columns),
            saved: [Saved::HOME; 3],
            overrides: [None; 256],
            shape: 0,
            last: None,
            state: State::Ground,
            params: [0; MAX_PARAMS],
            sub: 0,
            nparams: 0,
            cur_open: false,
            more: false,
            next_is_sub: false,
            overflow: false,
            marker: 0,
            inter: [0; 2],
            ninter: 0,
            seq_len: 0,
            string_escape: false,
            osc: Vec::new(),
            osc_over: false,
            utf8: 0,
            needed: 0,
            utf8_min: 0,
            dirty: vec![true; rows],
        }
    }

    /// Lines of scrollback kept at most.
    pub const fn scrollback(&self) -> usize {
        self.capacity
    }

    /// Sets how many lines of scrollback are kept (at most
    /// [`MAX_SCROLLBACK`]); the newest are kept if there are more now.
    pub fn set_scrollback(&mut self, lines: usize) {
        let lines = lines.min(MAX_SCROLLBACK);
        if lines == self.capacity {
            return;
        }
        let old = self.lines_oldest_first();
        self.capacity = lines;
        self.history = Vec::new();
        self.kept = 0;
        self.next = 0;
        let skip = old.len().saturating_sub(lines);
        for line in old.into_iter().skip(skip) {
            self.history.extend_from_slice(&line);
            self.kept += 1;
        }
        self.next = if lines == 0 { 0 } else { self.kept % lines };
        self.view = self.view.min(self.kept);
        self.touch_all();
    }

    /// The history's lines, oldest first.
    fn lines_oldest_first(&self) -> Vec<Vec<Cell>> {
        let c = self.columns;
        (0..self.kept)
            .map(|i| {
                let slot = (self.next + self.capacity - self.kept + i) % self.capacity.max(1);
                self.history[slot * c..(slot + 1) * c].to_vec()
            })
            .collect()
    }

    /// A new grid of `columns × rows` (each at least 1), keeping what fits
    /// of the screen from its top left and the cursor inside; each line of
    /// the scrollback is cut or padded to the new width. Everything is
    /// drawn again.
    pub fn resize(&mut self, columns: usize, rows: usize) {
        let (columns, rows) = (columns.max(1), rows.max(1));
        if (columns, rows) == (self.columns, self.rows) {
            return;
        }
        // Rows that would fall off the bottom of the primary screen scroll
        // into the history first, so that the cursor's line stays on screen.
        if !self.alt {
            while self.y >= rows {
                self.scroll_up_in(0, self.rows - 1, 1, true);
                self.y -= 1;
            }
        } else {
            self.y = self.y.min(rows - 1);
        }
        let resize_grid = |old: &[Cell], (oc, or): (usize, usize)| {
            let mut grid = vec![Cell::BLANK; columns * rows];
            for y in 0..rows.min(or) {
                for x in 0..columns.min(oc) {
                    grid[y * columns + x] = old[y * oc + x];
                }
            }
            grid
        };
        let old = (self.columns, self.rows);
        let screen = resize_grid(&self.screen, old);
        let other = resize_grid(&self.other, old);
        let lines = self.lines_oldest_first();
        let mut history = Vec::with_capacity(lines.len() * columns);
        for line in &lines {
            let mut row = vec![Cell::BLANK; columns];
            let n = columns.min(line.len());
            row[..n].copy_from_slice(&line[..n]);
            crate::screen::fix_cells(&mut row);
            history.extend_from_slice(&row);
        }
        self.history = history;
        self.next = if self.capacity == 0 {
            0
        } else {
            self.kept % self.capacity
        };
        self.screen = screen;
        self.other = other;
        let old_tabs = core::mem::take(&mut self.tabs);
        self.tabs = (0..columns)
            .map(|x| old_tabs.get(x).copied().unwrap_or(x % 8 == 0))
            .collect();
        self.columns = columns;
        self.rows = rows;
        self.top = 0;
        self.bottom = rows - 1;
        self.x = self.x.min(columns - 1);
        self.wrap = false;
        self.view = 0;
        // The saved cursors are clamped when they are restored.
        self.dirty = vec![true; rows];
        for row in self.screen.chunks_mut(columns) {
            crate::screen::fix_cells(row);
        }
        for row in self.other.chunks_mut(columns) {
            crate::screen::fix_cells(row);
        }
    }

    pub const fn size(&self) -> (usize, usize) {
        (self.columns, self.rows)
    }

    pub const fn cursor(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    /// The modes set now.
    pub const fn modes(&self) -> &Modes {
        &self.modes
    }

    /// Whether the alternate screen is shown.
    pub const fn on_alternate_screen(&self) -> bool {
        self.alt
    }

    /// The cursor shape `CSI Ps SP q` asked for: 0 and 1 a blinking and 2 a
    /// steady block, 3 and 4 an underline, 5 and 6 a bar (a blink is stored,
    /// not drawn).
    pub const fn cursor_shape(&self) -> u8 {
        self.shape
    }

    /// The scroll region's first and last row.
    pub const fn scroll_region(&self) -> (usize, usize) {
        (self.top, self.bottom)
    }

    /// Whether column `x` has a tab stop.
    pub fn tab_stop(&self, x: usize) -> bool {
        self.tabs.get(x).copied().unwrap_or(false)
    }

    /// Palette entry `i` as `OSC 4` set it, if it did.
    pub fn palette_override(&self, i: u8) -> Option<(u8, u8, u8)> {
        self.overrides[usize::from(i)]
    }

    /// The cell at a place on the live screen.
    pub fn cell(&self, x: usize, y: usize) -> Cell {
        self.screen[y * self.columns + x]
    }

    /// A row of the live screen as text, without trailing spaces: a wide
    /// character once, its combining marks after it.
    pub fn line(&self, y: usize) -> alloc::string::String {
        let row = &self.screen[y * self.columns..(y + 1) * self.columns];
        let mut text = alloc::string::String::new();
        for cell in row.iter().filter(|c| !c.has(TAIL)) {
            text.push(cell.ch);
            text.extend(cell.marks());
        }
        alloc::string::String::from(text.trim_end())
    }

    /// Lines in the scrollback.
    pub const fn history_len(&self) -> usize {
        self.kept
    }

    /// The bytes the terminal holds for cells and rows: at most what the
    /// grids and a full scrollback take (INV-0096-3).
    pub fn memory(&self) -> usize {
        (self.screen.capacity() + self.other.capacity() + self.history.capacity())
            * size_of::<Cell>()
            + self.dirty.capacity()
            + self.tabs.capacity()
            + self.osc.capacity()
    }

    /// The most [`Term::memory`] can reach for this size and scrollback.
    pub fn memory_bound(&self) -> usize {
        // Both screens and the scrollback's lines (reserved exactly, not grown
        // by doubling), the flags and stops, and the longest string held.
        (2 * self.columns * self.rows + self.columns * self.capacity) * size_of::<Cell>()
            + self.rows
            + self.columns
            + crate::MAX_STRING
    }

    /// Whether `self` and `other` are in the same state as far as a person
    /// or a program can tell: grids, cursor, modes, pen, tabs, region,
    /// character sets, saved cursors, palette and shape (not the parser's
    /// registers or what is marked for drawing).
    pub fn same_state(&self, other: &Term) -> bool {
        self.columns == other.columns
            && self.rows == other.rows
            && self.screen == other.screen
            && self.other == other.other
            && self.alt == other.alt
            && self.history == other.history
            && self.kept == other.kept
            && (self.x, self.y, self.wrap) == (other.x, other.y, other.wrap)
            && self.pen == other.pen
            && self.modes == other.modes
            && self.g == other.g
            && self.shifted == other.shifted
            && (self.top, self.bottom) == (other.top, other.bottom)
            && self.tabs == other.tabs
            && self.saved == other.saved
            && self.overrides == other.overrides
            && self.shape == other.shape
            && self.last == other.last
            && self.alt_saved_cursor == other.alt_saved_cursor
    }

    pub(crate) fn touch(&mut self, y: usize) {
        if let Some(d) = self.dirty.get_mut(y) {
            *d = true;
        }
    }

    pub(crate) fn touch_all(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = true);
    }

    /// Moves the view `lines` back (positive) or forward, within the
    /// scrollback. The alternate screen has none.
    pub fn scroll_view(&mut self, lines: isize) {
        let view = if self.alt {
            0
        } else {
            self.view as isize + lines
        };
        self.view = view.clamp(0, self.kept as isize) as usize;
        self.touch_all();
    }

    pub const fn view(&self) -> usize {
        self.view
    }

    /// The row `y` of what is shown: scrollback lines while the view is
    /// back, then the screen.
    pub(crate) fn shown(&self, y: usize) -> &[Cell] {
        let c = self.columns;
        if y < self.view {
            // The view's first line is `view` lines back from the newest.
            let back = self.view - y;
            let index = (self.next + self.capacity - back) % self.capacity.max(1);
            &self.history[index * c..(index + 1) * c]
        } else {
            let row = y - self.view;
            &self.screen[row * c..(row + 1) * c]
        }
    }

    /// Returns the screen to what a new shell expects, without erasing the
    /// primary screen (ADR-0331 item 9, INV-0331-4): an unfinished sequence
    /// is dropped, the alternate screen left, the cursor shown, the scroll
    /// region, keypad, paste, mouse, focus and synchronized-output modes, the
    /// character sets, the pen and the palette cleared. What was cleared is
    /// given back, for [`Term::reinstate`].
    pub fn restore(&mut self) -> Restored {
        let was = Restored {
            modes: self.modes,
            alt: self.alt,
            top: self.top,
            bottom: self.bottom,
            shape: self.shape,
        };
        self.state = State::Ground;
        self.needed = 0;
        self.osc.clear();
        self.leave_alt();
        self.modes = Modes::DEFAULT;
        self.pen = Pen::DEFAULT;
        self.g = [Charset::Ascii; 2];
        self.shifted = false;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.shape = 0;
        self.overrides = [None; 256];
        self.wrap = false;
        self.touch_all();
        was
    }

    /// Sets again what [`Term::restore`] cleared, when the line that had it
    /// is back in front. A program that was on the alternate screen finds it
    /// blank and repaints at the `SIGWINCH` the shell sends.
    pub fn reinstate(&mut self, was: Restored) {
        if was.alt {
            self.enter_alt(false);
        }
        self.modes = was.modes;
        self.top = was.top.min(self.rows - 1);
        self.bottom = was.bottom.min(self.rows - 1).max(self.top);
        self.shape = was.shape;
        self.touch_all();
    }

    /// The colour a cell asks for, in `look`'s palette and with this
    /// terminal's `OSC 4` changes.
    fn colour(&self, look: &Look, c: Colour, foreground: bool) -> Color {
        match c {
            Colour::Default if foreground => look.fg,
            Colour::Default => look.bg,
            Colour::Index(i) => match self.overrides[usize::from(i)] {
                Some((r, g, b)) => rgb(r, g, b),
                None => look.palette[usize::from(i)],
            },
            Colour::Rgb(r, g, b) => rgb(r, g, b),
        }
    }

    /// The foreground and background a cell is drawn in: reverse (also of
    /// the whole screen) swaps them, faint mixes the foreground towards the
    /// background, invisible makes them one.
    pub fn colours_of(&self, look: &Look, cell: &Cell) -> (Color, Color) {
        let mut fg = self.colour(look, cell.fg(), true);
        let mut bg = self.colour(look, cell.bg(), false);
        if cell.has(FAINT) {
            fg = fg.blend(bg, 128);
        }
        if cell.has(REVERSE) != self.modes.reverse_screen {
            core::mem::swap(&mut fg, &mut bg);
        }
        if cell.has(crate::cell::INVISIBLE) {
            fg = bg;
        }
        (fg, bg)
    }

    /// Draws the rows that changed into `pixels`, a surface of `stride`
    /// pixels per row made with [`Look::surface_size`], and gives the rows
    /// drawn, first and last, if any. `cursor` draws the cursor, in the
    /// shape the program asked for, when the program has not hidden it.
    pub fn render(
        &mut self,
        look: &mut Look,
        pixels: &mut [u32],
        stride: usize,
        cursor: bool,
    ) -> Option<(usize, usize)> {
        let mut drawn: Option<(usize, usize)> = None;
        let show_cursor = cursor && self.view == 0 && self.modes.cursor_visible;
        for y in 0..self.rows {
            if !self.dirty[y] {
                continue;
            }
            self.dirty[y] = false;
            for x in 0..self.columns {
                let cell = self.shown(y)[x];
                if cell.has(TAIL) {
                    // Its left half drew it.
                    continue;
                }
                let wide = cell.has(WIDE);
                let here = show_cursor && y == self.y && (x == self.x || (wide && x + 1 == self.x));
                let (mut fg, mut bg) = self.colours_of(look, &cell);
                let block = here && self.shape < 3;
                if block {
                    core::mem::swap(&mut fg, &mut bg);
                }
                look.draw_cell_coloured(pixels, stride, x, y, &cell, (fg, bg), wide);
                if here && !block {
                    let color = self.colour(look, Colour::Default, true);
                    look.draw_cursor_mark(pixels, stride, x, y, color, self.shape, wide);
                }
            }
            drawn = Some(match drawn {
                None => (y, y),
                Some((first, _)) => (first, y),
            });
        }
        drawn
    }

    /// Marks every row for the next render.
    pub fn redraw(&mut self) {
        self.touch_all();
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::of_pixel(u32::from_be_bytes([0, r, g, b]))
}

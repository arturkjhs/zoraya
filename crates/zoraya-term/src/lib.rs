//! The terminal's logic (ADR-0096), with no IPC: a parser for the VT subset
//! the shell may send, a grid of cells with a fixed scrollback, the bytes
//! keys produce, and rendering into a surface with `zoraya-text` in the
//! colours and sizes of `zoraya-tokens` (ADR-0161, ADR-0162).
//!
//! Output is hostile input here. The parser is total, every string it skips
//! is cut at a bound, the cursor stays inside the grid, and memory is fixed
//! when the terminal is made (INV-0096-1, INV-0096-3). Nothing the parser
//! reads can produce input: there is no path from output to the shell
//! (INV-0096-2).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod read;

use alloc::vec;
use alloc::vec::Vec;

use zoraya_keymap::code;
use zoraya_text::{CellMetrics, Font, FontList, Rect, Target, Text};
use zoraya_tokens::{Appearance, Color, Scale, space, text};

/// Lines kept above the screen `[assumption, ADR-0096]`.
pub const SCROLLBACK: usize = 500;
/// Numeric parameters of one control sequence.
pub const MAX_PARAMS: usize = 8;
/// Bytes of an `OSC`, `DCS` or similar string before it is cut.
pub const MAX_STRING: usize = 4096;

/// Colour indices of a cell: the 8 colours (`term.black` to `term.white`),
/// their bright forms for bold, their faint forms (SGR 2, ADR-0154), then
/// the default foreground and background (`term.fg`, `term.bg`) and the
/// faint default foreground.
pub const COLORS: usize = 27;
/// Where the bright colours start.
pub const BRIGHT: u8 = 8;
/// Where the faint colours start.
pub const FAINT: u8 = 16;
pub const DEFAULT_FG: u8 = 24;
pub const DEFAULT_BG: u8 = 25;
pub const FAINT_DEFAULT: u8 = 26;

/// The terminal's colours in one appearance, indexed as [`COLORS`] says.
/// The bright forms are derived (ADR-0161: "the bright eight are derived
/// by the terminal until a direction wants them as tokens"): a quarter of
/// the way towards `term.fg`, so they gain contrast on `term.bg` in both
/// sets; the faint ones are half way towards `term.bg`.
pub fn palette(appearance: Appearance) -> [Color; COLORS] {
    let p = appearance.palette();
    let base = [
        p.term_black,
        p.term_red,
        p.term_green,
        p.term_yellow,
        p.term_blue,
        p.term_magenta,
        p.term_cyan,
        p.term_white,
    ];
    let mut out = [p.term_bg; COLORS];
    for (i, c) in base.iter().enumerate() {
        out[i] = *c;
        out[BRIGHT as usize + i] = c.blend(p.term_fg, 64);
        out[FAINT as usize + i] = c.blend(p.term_bg, 128);
    }
    out[DEFAULT_FG as usize] = p.term_fg;
    out[DEFAULT_BG as usize] = p.term_bg;
    out[FAINT_DEFAULT as usize] = p.term_fg.blend(p.term_bg, 128);
    out
}

/// How the terminal draws: its fonts, cell, padding and colours at one
/// scale and appearance. The test runner reads screens with the same.
pub struct Look {
    pub regular: Text<'static>,
    pub bold: Text<'static>,
    pub cell: CellMetrics,
    /// The text size in physical pixels (`text.mono` at the scale).
    pub px: u32,
    /// Pixels between the grid and the surface's edge (`padding.terminal`).
    pub pad: usize,
    pub colors: [Color; COLORS],
}

/// A monospace font list: JetBrains Mono, then Inter for what it lacks
/// (ADR-0162's monospace fallback), then the box.
fn mono(first: &'static [u8]) -> FontList<'static> {
    let fonts = [first, zoraya_fonts::INTER_REGULAR]
        .into_iter()
        .filter_map(|d| Font::parse(d).ok());
    FontList::new(fonts)
}

impl Look {
    pub fn new(scale: Scale, appearance: Appearance) -> Look {
        let regular = Text::new(mono(zoraya_fonts::MONO_REGULAR));
        let bold = Text::new(mono(zoraya_fonts::MONO_BOLD));
        let px = text::MONO.physical(scale);
        let cell = CellMetrics::of(&regular.fonts, px);
        Look {
            regular,
            bold,
            cell,
            px,
            pad: space::PADDING_TERMINAL.physical(scale) as usize,
            colors: palette(appearance),
        }
    }

    pub fn cell_width(&self) -> usize {
        self.cell.width as usize
    }

    pub fn cell_height(&self) -> usize {
        self.cell.height as usize
    }

    /// The grid on a display of `width × height` pixels: 80 × 24, less on
    /// a small display, at least 1 × 1. The GUI test reads the screen with
    /// the same numbers.
    pub fn grid_for(&self, width: u32, height: u32) -> (usize, usize) {
        // Room for the padding and the compositor's frame and placement.
        let fit = |side: u32, cell: usize| (side as usize).saturating_sub(2 * self.pad + 40) / cell;
        let columns = 80.min(fit(width, self.cell_width())).max(1);
        let rows = 24.min(fit(height, self.cell_height())).max(1);
        (columns, rows)
    }

    /// The surface size, in pixels, of a grid of `columns × rows`.
    pub fn surface_size(&self, columns: usize, rows: usize) -> (usize, usize) {
        (
            columns * self.cell_width() + 2 * self.pad,
            rows * self.cell_height() + 2 * self.pad,
        )
    }

    /// The top left of cell `(x, y)` in the surface.
    pub fn cell_origin(&self, x: usize, y: usize) -> (usize, usize) {
        (
            self.pad + x * self.cell_width(),
            self.pad + y * self.cell_height(),
        )
    }

    /// Draws one cell into `pixels` (rows of `stride`).
    pub fn draw_cell(&mut self, pixels: &mut [u32], stride: usize, x: usize, y: usize, cell: Cell) {
        let (px, py) = self.cell_origin(x, y);
        let fg = self.colors[usize::from(cell.fg) % COLORS];
        let bg = self.colors[usize::from(cell.bg) % COLORS];
        let mut target = Target {
            pixels,
            stride,
            clip: Rect::new(0, 0, stride as u32, u32::MAX),
        };
        let text = if cell.bold {
            &mut self.bold
        } else {
            &mut self.regular
        };
        text.draw_cell(
            &mut target,
            self.cell,
            (px as i32, py as i32),
            cell.ch,
            self.px,
            (fg, bg),
        );
    }

    /// Fills the whole surface with the background, padding included.
    pub fn clear(&self, pixels: &mut [u32]) {
        let bg = self.colors[DEFAULT_BG as usize].pixel();
        pixels.fill(bg);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: u8,
    pub bg: u8,
    /// Drawn with the bold font (SGR 1).
    pub bold: bool,
}

const BLANK: Cell = Cell {
    ch: ' ',
    fg: DEFAULT_FG,
    bg: DEFAULT_BG,
    bold: false,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    /// `ESC [`, collecting parameters.
    Csi,
    /// A string that ends at BEL or `ESC \`, skipped.
    Str,
    /// `ESC` inside a string: `\` ends it.
    StrEscape,
    /// `ESC` then an intermediate byte: one more byte ends it.
    EscapeIntermediate,
}

/// The terminal's state.
pub struct Term {
    columns: usize,
    rows: usize,
    screen: Vec<Cell>,
    /// Lines that scrolled off the top, a ring of `SCROLLBACK`.
    history: Vec<Cell>,
    /// Lines in `history`, up to `SCROLLBACK`, and where the next goes.
    kept: usize,
    next: usize,
    /// How far back the view is, in lines; 0 is the live screen.
    view: usize,
    x: usize,
    y: usize,
    /// The cursor is past the last column: the next character wraps first.
    wrap: bool,
    fg: u8,
    bg: u8,
    bold: bool,
    faint: bool,
    reverse: bool,
    state: State,
    params: [u16; MAX_PARAMS],
    count: usize,
    /// A private marker (`?`, `>`, ...) makes the sequence one this
    /// terminal ignores.
    private: bool,
    skipped: usize,
    /// UTF-8 being decoded: the value so far and the bytes still needed.
    utf8: u32,
    needed: u8,
    /// The smallest value a sequence of this length may encode: anything
    /// below it is an overlong encoding.
    utf8_min: u32,
    /// Rows of the view that changed since the last render.
    dirty: Vec<bool>,
}

impl Term {
    /// A terminal of `columns × rows`, each at least 1.
    pub fn new(columns: usize, rows: usize) -> Term {
        let (columns, rows) = (columns.max(1), rows.max(1));
        Term {
            columns,
            rows,
            screen: vec![BLANK; columns * rows],
            history: vec![BLANK; columns * SCROLLBACK],
            kept: 0,
            next: 0,
            view: 0,
            x: 0,
            y: 0,
            wrap: false,
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            bold: false,
            faint: false,
            reverse: false,
            state: State::Ground,
            params: [0; MAX_PARAMS],
            count: 0,
            private: false,
            skipped: 0,
            utf8: 0,
            needed: 0,
            utf8_min: 0,
            dirty: vec![true; rows],
        }
    }

    /// A new grid of `columns × rows` (each at least 1), keeping what fits
    /// of the screen from its top left and the cursor inside; the
    /// scrollback is kept at its old width, cut or padded per line when
    /// shown. Everything is drawn again.
    pub fn resize(&mut self, columns: usize, rows: usize) {
        let (columns, rows) = (columns.max(1), rows.max(1));
        if (columns, rows) == (self.columns, self.rows) {
            return;
        }
        // Rows that would fall off the bottom scroll into the history first,
        // so that the cursor's line stays on screen.
        while self.y >= rows {
            self.scroll();
            self.y -= 1;
        }
        let mut screen = vec![BLANK; columns * rows];
        for y in 0..rows.min(self.rows) {
            for x in 0..columns.min(self.columns) {
                screen[y * columns + x] = self.screen[y * self.columns + x];
            }
        }
        let mut history = vec![BLANK; columns * SCROLLBACK];
        for line in 0..SCROLLBACK {
            for x in 0..columns.min(self.columns) {
                history[line * columns + x] = self.history[line * self.columns + x];
            }
        }
        self.screen = screen;
        self.history = history;
        self.columns = columns;
        self.rows = rows;
        self.x = self.x.min(columns - 1);
        self.wrap = false;
        self.view = 0;
        self.dirty = vec![true; rows];
    }

    pub const fn size(&self) -> (usize, usize) {
        (self.columns, self.rows)
    }

    pub const fn cursor(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    /// The cell at a place on the live screen.
    pub fn cell(&self, x: usize, y: usize) -> Cell {
        self.screen[y * self.columns + x]
    }

    /// A row of the live screen as text, without trailing spaces.
    pub fn line(&self, y: usize) -> alloc::string::String {
        let row = &self.screen[y * self.columns..(y + 1) * self.columns];
        let text: alloc::string::String = row.iter().map(|c| c.ch).collect();
        alloc::string::String::from(text.trim_end())
    }

    /// Lines in the scrollback.
    pub const fn history_len(&self) -> usize {
        self.kept
    }

    /// The bytes the terminal holds for cells, which never change after
    /// [`Term::new`] (INV-0096-3).
    pub fn memory(&self) -> usize {
        (self.screen.capacity() + self.history.capacity()) * size_of::<Cell>()
            + self.dirty.capacity()
    }

    fn touch(&mut self, y: usize) {
        if let Some(d) = self.dirty.get_mut(y) {
            *d = true;
        }
    }

    fn touch_all(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = true);
    }

    /// Takes output bytes from the shell.
    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        // Output moves the view back to the live screen.
        if self.view != 0 {
            self.view = 0;
            self.touch_all();
        }
        match self.state {
            State::Ground => self.ground(b),
            State::Escape => self.escape(b),
            State::Csi => self.csi(b),
            State::Str => match b {
                0x07 => self.state = State::Ground,
                0x1b => self.state = State::StrEscape,
                _ => self.skip(),
            },
            State::StrEscape => {
                if b == b'\\' {
                    self.state = State::Ground;
                } else {
                    // An escape pair inside a string counts once towards its bound, so
                    // a string is cut within twice `MAX_STRING` bytes.
                    self.state = State::Str;
                    self.skip();
                }
            }
            State::EscapeIntermediate => self.state = State::Ground,
        }
    }

    fn skip(&mut self) {
        self.skipped += 1;
        if self.skipped >= MAX_STRING {
            self.state = State::Ground;
        }
    }

    fn ground(&mut self, b: u8) {
        if self.needed > 0 {
            if b & 0xc0 == 0x80 {
                self.utf8 = self.utf8 << 6 | u32::from(b & 0x3f);
                self.needed -= 1;
                if self.needed == 0 {
                    // Overlong encodings, surrogates, values past U+10FFFF
                    // and C1 controls all show as U+FFFD: nothing decoded
                    // is ever a control (ADR-0013 allows UTF-8 names).
                    let c = char::from_u32(self.utf8)
                        .filter(|_| self.utf8 >= self.utf8_min)
                        .filter(|c| !c.is_control())
                        .unwrap_or('\u{fffd}');
                    self.put(c);
                }
                return;
            }
            // A sequence cut short: one replacement, then this byte anew.
            self.needed = 0;
            self.put('\u{fffd}');
        }
        match b {
            0x1b => {
                self.state = State::Escape;
            }
            b'\n' => self.newline(),
            b'\r' => self.carriage_return(),
            0x08 => {
                self.wrap = false;
                self.x = self.x.saturating_sub(1);
                self.touch(self.y);
            }
            b'\t' => {
                let stop = (self.x / 8 + 1) * 8;
                self.x = stop.min(self.columns - 1);
                self.wrap = false;
                self.touch(self.y);
            }
            0x00..=0x1f | 0x7f => {}
            0x20..=0x7e => self.put(char::from(b)),
            0xc2..=0xdf => self.lead(b & 0x1f, 1, 0x80),
            0xe0..=0xef => self.lead(b & 0x0f, 2, 0x800),
            0xf0..=0xf4 => self.lead(b & 0x07, 3, 0x1_0000),
            _ => self.put('\u{fffd}'),
        }
    }

    fn lead(&mut self, value: u8, needed: u8, min: u32) {
        self.utf8 = u32::from(value);
        self.needed = needed;
        self.utf8_min = min;
    }

    fn escape(&mut self, b: u8) {
        self.state = State::Ground;
        match b {
            b'[' => {
                self.state = State::Csi;
                self.skipped = 0;
                self.params = [0; MAX_PARAMS];
                self.count = 0;
                self.private = false;
            }
            b']' | b'P' | b'X' | b'^' | b'_' => {
                self.state = State::Str;
                self.skipped = 0;
            }
            b'c' => self.reset(),
            0x20..=0x2f => self.state = State::EscapeIntermediate,
            _ => {}
        }
    }

    fn csi(&mut self, b: u8) {
        // A sequence that never ends is cut at the string bound too.
        self.skipped += 1;
        if self.skipped >= MAX_STRING {
            self.state = State::Ground;
            return;
        }
        match b {
            b'0'..=b'9' => {
                if self.count == 0 {
                    self.count = 1;
                }
                if let Some(p) = self.params.get_mut(self.count - 1) {
                    *p = p.saturating_mul(10).saturating_add(u16::from(b - b'0'));
                }
            }
            b';' => {
                if self.count == 0 {
                    self.count = 1;
                }
                self.count = (self.count + 1).min(MAX_PARAMS + 1);
            }
            b'<'..=b'?' => self.private = true,
            0x20..=0x2f => {}
            0x40..=0x7e => {
                self.state = State::Ground;
                if !self.private {
                    self.dispatch(b);
                }
            }
            // A control byte inside a sequence: acted on, as a VT does.
            0x08 | b'\n' | b'\r' => self.ground(b),
            0x1b => self.state = State::Escape,
            _ => self.state = State::Ground,
        }
    }

    /// Parameter `i`, with 0 and absent meaning `default`.
    fn param(&self, i: usize, default: usize) -> usize {
        match self.params.get(i) {
            Some(&p) if i < self.count && p != 0 => usize::from(p),
            _ => default,
        }
    }

    fn dispatch(&mut self, final_byte: u8) {
        self.wrap = false;
        let n = self.param(0, 1);
        match final_byte {
            b'A' => self.y = self.y.saturating_sub(n),
            b'B' => self.y = (self.y + n).min(self.rows - 1),
            b'C' => self.x = (self.x + n).min(self.columns - 1),
            b'D' => self.x = self.x.saturating_sub(n),
            b'H' | b'f' => {
                self.y = (self.param(0, 1) - 1).min(self.rows - 1);
                self.x = (self.param(1, 1) - 1).min(self.columns - 1);
            }
            b'J' => self.erase_display(self.param(0, 0)),
            b'K' => self.erase_line(self.param(0, 0)),
            b'm' => self.graphics(),
            _ => {}
        }
        // A cursor move changes where the cursor is drawn.
        self.touch_all_cursor();
    }

    fn touch_all_cursor(&mut self) {
        // The cursor's old and new rows are among these; rendering a few
        // more rows than needed is cheap.
        self.touch_all();
    }

    fn graphics(&mut self) {
        let count = self.count.clamp(1, MAX_PARAMS);
        for i in 0..count {
            let p = if i < self.count { self.params[i] } else { 0 };
            match p {
                0 => {
                    self.fg = DEFAULT_FG;
                    self.bg = DEFAULT_BG;
                    self.bold = false;
                    self.faint = false;
                    self.reverse = false;
                }
                1 => self.bold = true,
                2 => self.faint = true,
                7 => self.reverse = true,
                22 => {
                    self.bold = false;
                    self.faint = false;
                }
                27 => self.reverse = false,
                30..=37 => self.fg = (p - 30) as u8,
                39 => self.fg = DEFAULT_FG,
                40..=47 => self.bg = (p - 40) as u8,
                49 => self.bg = DEFAULT_BG,
                _ => {}
            }
        }
    }

    fn blank(&self) -> Cell {
        Cell {
            ch: ' ',
            fg: DEFAULT_FG,
            bg: self.bg,
            bold: false,
        }
    }

    fn erase_line(&mut self, mode: usize) {
        let (from, to) = match mode {
            0 => (self.x, self.columns),
            1 => (0, self.x + 1),
            2 => (0, self.columns),
            _ => return,
        };
        let blank = self.blank();
        let row = self.y * self.columns;
        self.screen[row + from..row + to].fill(blank);
        self.touch(self.y);
    }

    fn erase_display(&mut self, mode: usize) {
        let at = self.y * self.columns + self.x;
        let (from, to) = match mode {
            0 => (at, self.screen.len()),
            1 => (0, at + 1),
            2 | 3 => (0, self.screen.len()),
            _ => return,
        };
        let blank = self.blank();
        self.screen[from..to].fill(blank);
        self.touch_all();
    }

    fn reset(&mut self) {
        self.screen.fill(BLANK);
        self.x = 0;
        self.y = 0;
        self.wrap = false;
        self.fg = DEFAULT_FG;
        self.bg = DEFAULT_BG;
        self.bold = false;
        self.faint = false;
        self.reverse = false;
        self.touch_all();
    }

    fn put(&mut self, ch: char) {
        if self.wrap {
            self.carriage_return();
            self.newline();
        }
        let fg = match (self.fg, self.faint) {
            (DEFAULT_FG, true) => FAINT_DEFAULT,
            (c, true) => c % 8 + FAINT,
            (c, false) if self.bold && c < BRIGHT => c + BRIGHT,
            (c, false) => c,
        };
        let bold = self.bold && !self.faint;
        let (fg, bg) = if self.reverse {
            (self.bg, fg)
        } else {
            (fg, self.bg)
        };
        self.screen[self.y * self.columns + self.x] = Cell { ch, fg, bg, bold };
        self.touch(self.y);
        if self.x + 1 == self.columns {
            self.wrap = true;
        } else {
            self.x += 1;
        }
    }

    fn carriage_return(&mut self) {
        self.x = 0;
        self.wrap = false;
        self.touch(self.y);
    }

    /// `\n` moves to the start of the next line, as the kernel console does
    /// (ADR-0096), scrolling at the bottom.
    fn newline(&mut self) {
        self.touch(self.y);
        self.x = 0;
        self.wrap = false;
        if self.y + 1 < self.rows {
            self.y += 1;
        } else {
            self.scroll();
        }
        self.touch(self.y);
    }

    fn scroll(&mut self) {
        let c = self.columns;
        let at = self.next * c;
        self.history[at..at + c].copy_from_slice(&self.screen[..c]);
        self.next = (self.next + 1) % SCROLLBACK;
        self.kept = (self.kept + 1).min(SCROLLBACK);
        self.screen.copy_within(c.., 0);
        let blank = self.blank();
        let last = self.screen.len() - c;
        self.screen[last..].fill(blank);
        self.touch_all();
    }

    /// Moves the view `lines` back (positive) or forward, within the
    /// scrollback.
    pub fn scroll_view(&mut self, lines: isize) {
        let view = self.view as isize + lines;
        self.view = view.clamp(0, self.kept as isize) as usize;
        self.touch_all();
    }

    pub const fn view(&self) -> usize {
        self.view
    }

    /// The row `y` of what is shown: scrollback lines while the view is
    /// back, then the screen.
    fn shown(&self, y: usize) -> &[Cell] {
        let c = self.columns;
        if y < self.view {
            // The view's first line is `view` lines back from the newest.
            let back = self.view - y;
            let index = (self.next + SCROLLBACK - back) % SCROLLBACK;
            &self.history[index * c..(index + 1) * c]
        } else {
            let row = y - self.view;
            &self.screen[row * c..(row + 1) * c]
        }
    }

    /// Draws the rows that changed into `pixels`, a surface of `stride`
    /// pixels per row made with [`Look::surface_size`], and gives the rows
    /// drawn, first and last, if any. `cursor` draws the cursor as a
    /// reversed cell.
    pub fn render(
        &mut self,
        look: &mut Look,
        pixels: &mut [u32],
        stride: usize,
        cursor: bool,
    ) -> Option<(usize, usize)> {
        let mut drawn: Option<(usize, usize)> = None;
        for y in 0..self.rows {
            if !self.dirty[y] {
                continue;
            }
            self.dirty[y] = false;
            for x in 0..self.columns {
                let mut cell = self.shown(y)[x];
                let here = cursor && self.view == 0 && x == self.x && y == self.y;
                if here {
                    core::mem::swap(&mut cell.fg, &mut cell.bg);
                }
                look.draw_cell(pixels, stride, x, y, cell);
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

/// The bytes a key sends to the shell (ADR-0096), written to `out`; gives
/// how many. `text` is what the keymap gave. Shift with Page Up or Page Down
/// is the terminal's own and sends nothing.
pub fn key_bytes(key: u32, shift: bool, text: Option<char>, out: &mut [u8; 8]) -> usize {
    let sequence: &[u8] = match key {
        code::ENTER => b"\r",
        code::BACKSPACE => b"\x7f",
        code::TAB => b"\t",
        code::ESC => b"\x1b",
        code::UP => b"\x1b[A",
        code::DOWN => b"\x1b[B",
        code::RIGHT => b"\x1b[C",
        code::LEFT => b"\x1b[D",
        code::HOME => b"\x1b[H",
        code::END => b"\x1b[F",
        code::INSERT => b"\x1b[2~",
        code::DELETE => b"\x1b[3~",
        code::PAGE_UP | code::PAGE_DOWN if shift => b"",
        code::PAGE_UP => b"\x1b[5~",
        code::PAGE_DOWN => b"\x1b[6~",
        _ => {
            return match text {
                Some(c) => c.encode_utf8(&mut out[..]).len(),
                None => 0,
            };
        }
    };
    out[..sequence.len()].copy_from_slice(sequence);
    sequence.len()
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

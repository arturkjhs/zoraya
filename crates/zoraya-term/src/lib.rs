//! The terminal's logic (ADR-0096, ADR-0332), with no IPC: a parser for the
//! sequences of the `xterm-256color` entry less a short list and a few beside
//! it, a grid of cells with a scrollback and an alternate screen, the bytes
//! keys produce, and rendering into a surface with `zoraya-text` in the
//! colours and sizes of `zoraya-tokens` (ADR-0161, ADR-0162).
//!
//! Output is hostile input here. The parser is total, every string it skips
//! is cut at a bound, the cursor stays inside the grid, and memory is bounded
//! by the size and the scrollback setting (INV-0096-1, INV-0096-3). Nothing
//! the parser reads can produce input: there is no path from output to the
//! shell, and no query is answered (INV-0096-2, INV-0332-2).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod cell;
pub mod colour;
pub mod keys;
pub mod mouse;
pub mod parse;
pub mod queue;
pub mod read;
mod screen;
pub mod select;
pub mod session;
pub mod status;
mod term;

use alloc::vec;
use alloc::vec::Vec;

use zoraya_text::{CellMetrics, Font, FontList, Rect, Target, Text};
use zoraya_tokens::{Appearance, Color, Scale, space, text};

pub use cell::{Cell, Colour};
pub use keys::{KeyModes, MAX_KEY_BYTES, Mods, key_bytes};
pub use mouse::{Pointer, PointerEvent, focus_bytes};
pub use queue::{Next, SendQueue};
pub use session::{Outgoing, Session};
pub use status::StatusRow;
pub use term::{Modes, Restored, Term};

/// Lines of scrollback kept by default: the setting `term.scrollback`
/// (ADR-0332 item 11) `[assumption]`.
pub const SCROLLBACK: usize = 2000;
/// The most lines of scrollback the setting may ask for `[assumption]`.
pub const MAX_SCROLLBACK: usize = 10_000;
/// Lines one notch of a wheel scrolls the view (proposed ADR-0322).
pub const WHEEL_LINES: i32 = 3;
/// Milliseconds a synchronized update (mode 2026) holds the drawing, at
/// most, before it is shown anyway `[assumption]` (ADR-0332 item 3).
pub const SYNC_HOLD_MS: u32 = 150;

/// What `term` does when it could draw (ADR-0332 item 3, mode 2026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hold {
    /// Draw now.
    pub draw: bool,
    /// When the hold on the drawing began (nanoseconds), if it is held.
    pub since: Option<u64>,
    /// When to wake to draw anyway; none when nothing is waiting for time.
    pub deadline: Option<u64>,
}

/// Whether to draw at `now` (nanoseconds), given when a synchronized update
/// began to hold the drawing (`since`), whether the program has the mode set,
/// and whether a commit still waits for its reply. A synchronized update is
/// held for at most [`SYNC_HOLD_MS`] and then shown; the end of the update
/// shows it at once. While a commit is outstanding nothing is drawn and there
/// is no deadline to wake for: the reply is what comes next. A program that
/// sets the mode and never ends it has each later update held for the
/// whole time (known limit, until `Restore`).
pub fn sync_hold(now: u64, since: Option<u64>, synchronized: bool, outstanding: bool) -> Hold {
    let hold = u64::from(SYNC_HOLD_MS) * 1_000_000;
    let begun = synchronized.then(|| since.unwrap_or(now));
    if outstanding {
        return Hold {
            draw: false,
            since: begun,
            deadline: None,
        };
    }
    match begun {
        None => Hold {
            draw: true,
            since: None,
            deadline: None,
        },
        Some(start) if now >= start.saturating_add(hold) => Hold {
            draw: true,
            since: None,
            deadline: None,
        },
        Some(start) => Hold {
            draw: false,
            since: Some(start),
            deadline: Some(start.saturating_add(hold)),
        },
    }
}

/// A wheel's turns as lines of the view (proposed ADR-0322): 120ths of a
/// notch are gathered until they make a line, so a high-resolution wheel
/// scrolls as far as one with notches.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WheelLines {
    rest: i32,
}

impl WheelLines {
    /// The lines `dy` (above 0: toward the person) moves the view, for
    /// [`Term::scroll_view`]: positive back into the scrollback, so a turn
    /// toward the person is negative.
    pub fn turn(&mut self, dy: i32) -> isize {
        let per_line = 120 / WHEEL_LINES;
        // At most the whole scrollback either way in one turn.
        let bound = per_line * MAX_SCROLLBACK as i32;
        let sum = self.rest.saturating_add(dy).clamp(-bound, bound);
        self.rest = sum % per_line;
        -(sum / per_line) as isize
    }
}
/// Parameters and sub-parameters of one control sequence (ADR-0332 item 1);
/// a sequence with more is consumed and ignored.
pub const MAX_PARAMS: usize = 32;
/// Bytes of a control sequence or string before it is cut.
pub const MAX_STRING: usize = 4096;

/// The terminal's 256 palette entries in one appearance (ADR-0332 item 6):
/// 0 to 15 are the `term.*` tokens' eight colours and their bright forms
/// (ADR-0161: "the bright eight are derived by the terminal until a
/// direction wants them as tokens", a quarter of the way towards `term.fg`,
/// so they gain contrast on `term.bg` in both sets), 16 to 255 xterm's cube
/// and grey ramp.
pub fn palette(appearance: Appearance) -> [Color; 256] {
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
    let mut out = [p.term_bg; 256];
    for (i, c) in base.iter().enumerate() {
        out[i] = *c;
        out[8 + i] = c.blend(p.term_fg, 64);
    }
    for i in 16..=255u8 {
        let (r, g, b) = colour::xterm_rgb(i);
        out[usize::from(i)] =
            Color::of_pixel(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b));
    }
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
    /// The palette, indexed as [`palette`] says.
    pub palette: [Color; 256],
    /// The default foreground and background (`term.fg`, `term.bg`).
    pub fg: Color,
    pub bg: Color,
    /// A selected cell's text and ground (`fg.text.on-selection` on
    /// `bg.selection`).
    pub sel_fg: Color,
    pub sel_bg: Color,
    /// A cell drawn apart, for slanting and for two-cell characters.
    scratch: Vec<u32>,
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
        let p = appearance.palette();
        Look {
            regular,
            bold,
            cell,
            px,
            pad: space::PADDING_TERMINAL.physical(scale) as usize,
            palette: palette(appearance),
            fg: p.term_fg,
            bg: p.term_bg,
            sel_fg: p.fg_text_on_selection,
            sel_bg: p.bg_selection,
            scratch: Vec::new(),
        }
    }

    /// The colours a screen reader tries for a cell's foreground: the
    /// defaults (the background is one: reverse video), the sixteen of the first rows of the palette, and their
    /// faint forms (half way to the background).
    pub fn reader_colours(&self) -> Vec<Color> {
        let mut out = vec![self.fg, self.bg];
        out.extend_from_slice(&self.palette[..16]);
        out.extend(self.palette[..8].iter().map(|c| c.blend(self.bg, 128)));
        out.push(self.fg.blend(self.bg, 128));
        out
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

    /// The cell under a point of the surface, in physical pixels, in a grid
    /// of `columns × rows`; a point in the padding or past the grid is the
    /// nearest cell.
    pub fn cell_at(&self, px: usize, py: usize, columns: usize, rows: usize) -> (usize, usize) {
        let x = px.saturating_sub(self.pad) / self.cell_width();
        let y = py.saturating_sub(self.pad) / self.cell_height();
        (
            x.min(columns.saturating_sub(1)),
            y.min(rows.saturating_sub(1)),
        )
    }

    /// The top left of cell `(x, y)` in the surface.
    pub fn cell_origin(&self, x: usize, y: usize) -> (usize, usize) {
        (
            self.pad + x * self.cell_width(),
            self.pad + y * self.cell_height(),
        )
    }

    /// Draws one cell into `pixels` (rows of `stride`) in the default
    /// palette: for programs that draw a grid without a [`Term`].
    pub fn draw_cell(&mut self, pixels: &mut [u32], stride: usize, x: usize, y: usize, cell: Cell) {
        let pick = |c: Colour, fg: bool| match c {
            Colour::Default if fg => self.fg,
            Colour::Default => self.bg,
            Colour::Index(i) => self.palette[usize::from(i)],
            Colour::Rgb(r, g, b) => {
                Color::of_pixel(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
            }
        };
        let colours = (pick(cell.fg(), true), pick(cell.bg(), false));
        let wide = cell.has(cell::WIDE);
        self.draw_cell_coloured(pixels, stride, x, y, &cell, colours, wide);
    }

    /// Draws one cell in the colours `(fg, bg)`; a `wide` cell covers two
    /// columns.
    #[allow(clippy::too_many_arguments)] // a cell, its two colours, its place and its surface
    pub fn draw_cell_coloured(
        &mut self,
        pixels: &mut [u32],
        stride: usize,
        x: usize,
        y: usize,
        cell: &Cell,
        (fg, bg): (Color, Color),
        wide: bool,
    ) {
        let (px, py) = self.cell_origin(x, y);
        let plain = !wide
            && !cell.has(cell::ITALIC | cell::UNDERLINE | cell::STRIKE)
            && cell.marks().next().is_none();
        if plain {
            let mut target = Target {
                pixels,
                stride,
                clip: Rect::new(0, 0, stride as u32, u32::MAX),
            };
            let text = if cell.bold() {
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
            return;
        }
        // Drawn apart in a buffer of the cell's size, then slanted and
        // copied: the glyph, its marks, the lines through it.
        let metrics = CellMetrics {
            width: self.cell.width * if wide { 2 } else { 1 },
            ..self.cell
        };
        let (w, h) = (metrics.width as usize, metrics.height as usize);
        let mut scratch = core::mem::take(&mut self.scratch);
        scratch.clear();
        scratch.resize(w * h, bg.pixel());
        {
            let mut target = Target {
                pixels: &mut scratch,
                stride: w,
                clip: Rect::new(0, 0, metrics.width, metrics.height),
            };
            let text = if cell.bold() {
                &mut self.bold
            } else {
                &mut self.regular
            };
            text.draw_cell(&mut target, metrics, (0, 0), cell.ch, self.px, (fg, bg));
            for mark in cell.marks() {
                text.draw_glyph_over(&mut target, metrics, (0, 0), mark, self.px, fg);
            }
            let thick = (self.px / 14).max(1);
            if cell.has(cell::UNDERLINE) {
                let top = (metrics.baseline + thick).min(metrics.height - 1);
                target.fill(Rect::new(0, top as i32, metrics.width, thick), fg);
            }
            if cell.has(cell::STRIKE) {
                let at = metrics.baseline * 6 / 10;
                target.fill(Rect::new(0, at as i32, metrics.width, thick), fg);
            }
        }
        for row in 0..h {
            // The top leans right: half the height above the middle.
            let lean: i32 = if cell.has(cell::ITALIC) {
                (h as i32 / 2 - row as i32) / 4
            } else {
                0
            };
            let py = py + row;
            for col in 0..w {
                let sx = col as i32 - lean;
                let value = if (0..w as i32).contains(&sx) {
                    scratch[row * w + sx as usize]
                } else {
                    bg.pixel()
                };
                if let Some(slot) = pixels.get_mut(py * stride + px + col)
                    && px + col < stride
                {
                    *slot = value;
                }
            }
        }
        self.scratch = scratch;
    }

    /// Draws the underline or bar cursor of `shape` (3 to 6) over a cell.
    #[allow(clippy::too_many_arguments)] // a cell's place, a colour, a shape and a surface
    pub fn draw_cursor_mark(
        &self,
        pixels: &mut [u32],
        stride: usize,
        x: usize,
        y: usize,
        color: Color,
        shape: u8,
        wide: bool,
    ) {
        let (px, py) = self.cell_origin(x, y);
        let thick = (self.px / 8).max(2) as usize;
        let (w, h) = (
            self.cell_width() * if wide { 2 } else { 1 },
            self.cell_height(),
        );
        let (rx, ry, rw, rh) = if shape <= 4 {
            (px, py + h - thick.min(h), w, thick.min(h))
        } else {
            (px, py, thick.min(w), h)
        };
        let mut target = Target {
            pixels,
            stride,
            clip: Rect::new(0, 0, stride as u32, u32::MAX),
        };
        target.fill(Rect::new(rx as i32, ry as i32, rw as u32, rh as u32), color);
    }

    /// Fills the whole surface with the background, padding included.
    pub fn clear(&self, pixels: &mut [u32]) {
        pixels.fill(self.bg.pixel());
    }
}

/// What one wait on the compositor's channel gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// A message of this many bytes.
    Message(usize),
    /// Nothing within the wait: the compositor is alive but slow.
    Nothing,
    /// The channel is closed: the compositor is gone.
    Gone,
}

/// Waits for a reply, skipping the events that may come first
/// (`is_event`), for as long as the channel is open: a compositor that is
/// slow to start (its wallpaper, its blur, its first frame: seconds under
/// TCG) is waited for, and `on_slow` runs once, when the first wait gives
/// nothing; one that is gone gives `None`. `term` never panics on either.
pub fn await_reply(
    mut wait: impl FnMut(&mut [u8]) -> Waited,
    reply: &mut [u8],
    is_event: impl Fn(&[u8]) -> bool,
    mut on_slow: impl FnMut(),
) -> Option<usize> {
    let mut told = false;
    loop {
        match wait(reply) {
            Waited::Message(n) if n <= reply.len() && !is_event(&reply[..n]) => return Some(n),
            Waited::Message(_) => {}
            Waited::Nothing => {
                if !told {
                    told = true;
                    on_slow();
                }
            }
            Waited::Gone => return None,
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod entry;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_caps;
#[cfg(test)]
mod tests_keys;
#[cfg(test)]
mod tests_parser;
#[cfg(test)]
mod tests_session;
#[cfg(test)]
mod tests_status;
#[cfg(test)]
mod tests_total;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

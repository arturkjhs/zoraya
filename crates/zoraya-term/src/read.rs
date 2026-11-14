//! Reading a terminal's cells back from its pixels, exactly (ADR-0162):
//! every character a cell may hold is drawn with the same fonts, size and
//! colours as the terminal draws it, and a cell reads as the character
//! whose pixels are identical, with a tolerance of zero. Rendering is
//! deterministic (INV-0162-2), so a screendump of the real terminal reads
//! back as its text. Used by the GUI test runner and by this crate's tests.
//!
//! Letters every font draws alike (Latin `A` and Cyrillic `А`, ...) read as
//! the first one drawn, which is the Latin; [`Reader::canonical`] maps a
//! text into the same classes so that it can be compared with what was
//! read (INV-0162-4 lists the classes).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_text::{Rect, Target};
use zoraya_tokens::Color;

use crate::Look;

/// The characters a cell is matched against, in the order that decides a
/// class of look-alikes: printable ASCII, Latin-1, Latin Extended-A, and
/// Cyrillic with the Ukrainian letters.
pub fn candidates() -> impl Iterator<Item = char> {
    let ranges = [
        (0x21, 0x7E),
        (0xA1, 0xFF),
        (0x100, 0x17F),
        (0x400, 0x45F),
        (0x490, 0x491),
    ];
    ranges
        .into_iter()
        .flat_map(|(a, b)| (a..=b).filter_map(char::from_u32))
}

/// What a cell that matches no candidate reads as: not a candidate, so
/// that it is never taken for a real `?` (review finding 10).
pub const NO_MATCH: char = '\u{FFFD}';

/// One channel of a colour.
type Channel = fn(Color) -> u8;

/// A cell's pixels, row by row.
type Pixels = Vec<u32>;

/// One candidate's coverage, pixel by pixel, as the rasteriser gives it
/// (white on black reads back as the coverage itself).
type Coverage = Vec<u8>;

pub struct Reader {
    look: Look,
    /// For regular and bold: the candidates in their order, each with its
    /// coverage. A cell is read by finding the candidate whose coverage its
    /// pixels are in proportion to and a foreground that reproduces every
    /// pixel exactly, so any foreground is read, not only the palette's.
    /// Keeping a cell's pixels for every colour took gigabytes at scale 2
    /// (6.5 GB for one script, 2026-11-09); this keeps 600 small vectors a
    /// face.
    shapes: Option<[Vec<(char, Coverage)>; 2]>,
}

/// How far a pixel is from the background: the sum of its channels' differences.
fn distance(pixel: u32, bg: Color) -> i32 {
    let c = Color::of_pixel(pixel);
    let d = |a: u8, b: u8| i32::from(a.abs_diff(b));
    d(c.r(), bg.r()) + d(c.g(), bg.g()) + d(c.b(), bg.b())
}

impl Reader {
    pub fn new(look: Look) -> Reader {
        Reader { look, shapes: None }
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    fn draw(&mut self, c: char, fg: Color, bg: Color, bold: bool) -> Pixels {
        let cell = self.look.cell;
        let mut pixels = vec![0u32; (cell.width * cell.height) as usize];
        let mut t = Target {
            pixels: &mut pixels,
            stride: cell.width as usize,
            clip: Rect::new(0, 0, cell.width, cell.height),
        };
        let px = self.look.px;
        let text = if bold {
            &mut self.look.bold
        } else {
            &mut self.look.regular
        };
        text.draw_cell(&mut t, cell, (0, 0), c, px, (fg, bg));
        pixels
    }

    fn shapes(&mut self) -> &[Vec<(char, Coverage)>; 2] {
        if self.shapes.is_none() {
            let (white, black) = (Color::of_pixel(u32::MAX), Color::of_pixel(0));
            let mut both = [Vec::new(), Vec::new()];
            for (i, bold) in [false, true].into_iter().enumerate() {
                for c in candidates() {
                    let pixels = self.draw(c, white, black, bold);
                    let coverage: Coverage = pixels.iter().map(|p| (*p & 0xff) as u8).collect();
                    both[i].push((c, coverage));
                }
            }
            self.shapes = Some(both);
        }
        match &self.shapes {
            Some(shapes) => shapes,
            None => unreachable!("the shapes were just made"),
        }
    }

    /// The foreground that draws `coverage` on `bg` as exactly `pixels`, if
    /// there is one. Each pixel bounds each channel of the foreground:
    /// `mix(bg, fg, a) = (bg * (255 - a) + fg * a + 127) / 255` has to give
    /// the pixel's value, which leaves `fg` in an interval; the intervals of
    /// every pixel are intersected, and what is left is checked pixel for
    /// pixel with the rasteriser's own blend.
    fn fit(pixels: &[u32], bg: Color, coverage: &[u8]) -> Option<Color> {
        let channels: [(Channel, u32); 3] = [(Color::r, 16), (Color::g, 8), (Color::b, 0)];
        let mut fg = 0u32;
        for (channel, shift) in channels {
            let (mut lo, mut hi) = (0i64, 255i64);
            let s = i64::from(channel(bg));
            for (pixel, a) in pixels.iter().zip(coverage) {
                let (p, a) = (i64::from(channel(Color::of_pixel(*pixel))), i64::from(*a));
                if a == 0 {
                    if p != s {
                        return None;
                    }
                    continue;
                }
                let base = s * (255 - a);
                // o * a >= p * 255 - base - 127 and o * a <= (p + 1) * 255 - base - 128.
                lo = lo.max(-((base + 127 - p * 255).div_euclid(a)));
                hi = hi.min(((p + 1) * 255 - base - 128).div_euclid(a));
            }
            if lo > hi {
                return None;
            }
            fg |= (lo as u32) << shift;
        }
        let fg = Color::of_pixel(fg);
        pixels
            .iter()
            .zip(coverage)
            .all(|(p, a)| bg.blend(fg, *a).pixel() == *p)
            .then_some(fg)
    }

    /// The character of a cell whose pixels `pixel(x, y)` gives: `' '` for
    /// a blank cell in the default background, `'_'` for a cell of one
    /// other colour (the block cursor, or a blank cell with an SGR
    /// background: the two read alike), and [`NO_MATCH`] for anything else
    /// that matches no character in any colour.
    pub fn read(&mut self, pixel: impl Fn(usize, usize) -> u32) -> char {
        let cell = self.look.cell;
        let (w, h) = (cell.width as usize, cell.height as usize);
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                pixels.push(pixel(x, y));
            }
        }
        self.read_pixels(&pixels)
    }

    fn read_pixels(&mut self, pixels: &[u32]) -> char {
        // The background is the most common colour of the cell.
        let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
        for p in pixels {
            *counts.entry(*p).or_default() += 1;
        }
        let bg = counts
            .iter()
            .max_by_key(|(p, n)| (**n, core::cmp::Reverse(**p)))
            .map(|(p, _)| *p)
            .unwrap_or(0);
        if counts.len() == 1 {
            let default_bg = self.look.bg.pixel();
            return if bg == default_bg { ' ' } else { '_' };
        }
        let bg = Color::of_pixel(bg);
        // How far each pixel is from the background is, for the right
        // candidate, in proportion to its coverage (to within rounding).
        let far: Vec<i32> = pixels.iter().map(|p| distance(*p, bg)).collect();
        let (top, top_far) = far
            .iter()
            .enumerate()
            .max_by_key(|(_, d)| **d)
            .map(|(i, d)| (i, *d))
            .unwrap_or((0, 0));
        for bold in 0..2 {
            for (c, coverage) in &self.shapes()[bold] {
                let a_top = i32::from(coverage[top]);
                // The pixel farthest from the background is among the most
                // covered of the candidate.
                if a_top == 0 {
                    continue;
                }
                let proportional = far.iter().zip(coverage).all(|(d, a)| {
                    // d is about top_far * a / a_top.
                    (d * a_top - top_far * i32::from(*a)).abs() <= 6 * a_top
                });
                if proportional && Self::fit(pixels, bg, coverage).is_some() {
                    return *c;
                }
            }
        }
        NO_MATCH
    }

    /// `text` as the reader would read it back from the terminal's default
    /// colours: each look-alike replaced by the first of its class.
    pub fn canonical(&mut self, text: &str) -> String {
        let (fg, bg) = (self.look.fg, self.look.bg);
        text.chars()
            .map(|c| {
                if c == ' ' || c == '_' {
                    return c;
                }
                let pixels = self.draw(c, fg, bg, false);
                match self.read_pixels(&pixels) {
                    NO_MATCH | ' ' | '_' => c,
                    read => read,
                }
            })
            .collect()
    }

    /// The rows of a terminal surface whose top left is at `(left, top)`
    /// in a picture `pixel(x, y)` of `width × height`, as text without
    /// trailing spaces; a cell past the picture reads as [`NO_MATCH`].
    pub fn rows(
        &mut self,
        columns: usize,
        rows: usize,
        (left, top): (usize, usize),
        (width, height): (usize, usize),
        pixel: impl Fn(usize, usize) -> u32,
    ) -> Vec<String> {
        let (cw, ch) = (self.look.cell_width(), self.look.cell_height());
        (0..rows)
            .map(|r| {
                let line: String = (0..columns)
                    .map(|c| {
                        let (ox, oy) = self.look.cell_origin(c, r);
                        let (x, y) = (left + ox, top + oy);
                        if x + cw > width || y + ch > height {
                            return NO_MATCH;
                        }
                        self.read(|dx, dy| pixel(x + dx, y + dy))
                    })
                    .collect();
                String::from(line.trim_end())
            })
            .collect()
    }
}

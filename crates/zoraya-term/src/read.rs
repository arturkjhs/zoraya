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

use crate::{COLORS, DEFAULT_BG, DEFAULT_FG, Look};

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

/// A cell's pixels, row by row.
type Pixels = Vec<u32>;

pub struct Reader {
    look: Look,
    /// For each (foreground, background, bold), every candidate's pixels.
    maps: BTreeMap<(u32, u32, bool), BTreeMap<Pixels, char>>,
}

impl Reader {
    pub fn new(look: Look) -> Reader {
        Reader {
            look,
            maps: BTreeMap::new(),
        }
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

    fn map(&mut self, fg: Color, bg: Color, bold: bool) -> &BTreeMap<Pixels, char> {
        let key = (fg.pixel(), bg.pixel(), bold);
        if !self.maps.contains_key(&key) {
            let mut map = BTreeMap::new();
            for c in candidates() {
                let pixels = self.draw(c, fg, bg, bold);
                map.entry(pixels).or_insert(c);
            }
            self.maps.insert(key, map);
        }
        &self.maps[&key]
    }

    /// The character of a cell whose pixels `pixel(x, y)` gives: `' '` for
    /// a blank cell in the default background, `'_'` for a cell of one
    /// other colour (the block cursor), `'?'` for anything else that
    /// matches no character in any of the terminal's colours.
    pub fn read(&mut self, pixel: impl Fn(usize, usize) -> u32) -> char {
        let cell = self.look.cell;
        let (w, h) = (cell.width as usize, cell.height as usize);
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                pixels.push(pixel(x, y));
            }
        }
        // The background is the most common colour of the cell.
        let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
        for p in &pixels {
            *counts.entry(*p).or_default() += 1;
        }
        let bg = counts
            .iter()
            .max_by_key(|(p, n)| (**n, core::cmp::Reverse(**p)))
            .map(|(p, _)| *p)
            .unwrap_or(0);
        if counts.len() == 1 {
            let default_bg = self.look.colors[DEFAULT_BG as usize].pixel();
            return if bg == default_bg { ' ' } else { '_' };
        }
        let bg = Color::of_pixel(bg);
        let mut fgs: Vec<Color> = Vec::with_capacity(COLORS);
        for c in self.look.colors {
            if c != bg && !fgs.contains(&c) {
                fgs.push(c);
            }
        }
        for bold in [false, true] {
            for fg in &fgs {
                if let Some(c) = self.map(*fg, bg, bold).get(&pixels) {
                    return *c;
                }
            }
        }
        '?'
    }

    /// `text` as the reader would read it back from the terminal's default
    /// colours: each look-alike replaced by the first of its class.
    pub fn canonical(&mut self, text: &str) -> String {
        let p = &self.look.colors;
        let (fg, bg) = (p[DEFAULT_FG as usize], p[DEFAULT_BG as usize]);
        text.chars()
            .map(|c| {
                if c == ' ' || c == '_' {
                    return c;
                }
                let pixels = self.draw(c, fg, bg, false);
                self.map(fg, bg, false).get(&pixels).copied().unwrap_or(c)
            })
            .collect()
    }

    /// The rows of a terminal surface whose top left is at `(left, top)`
    /// in a picture `pixel(x, y)` of `width × height`, as text without
    /// trailing spaces; a cell past the picture reads as `'?'`.
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
                            return '?';
                        }
                        self.read(|dx, dy| pixel(x + dx, y + dy))
                    })
                    .collect();
                String::from(line.trim_end())
            })
            .collect()
    }
}

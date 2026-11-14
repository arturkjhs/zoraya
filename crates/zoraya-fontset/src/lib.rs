//! The system's fonts for a POSIX program that draws its own text (ADR-0326):
//! NetSurf's font library over `zoraya-text`. Four faces (sans, sans bold,
//! monospace, monospace bold), each a fallback list of the system's fonts;
//! the fonts are read only as `/system/fonts/NAME` for the four names of
//! [`FILES`], through a [`Source`] the caller gives (in `libposix`, `open`
//! and `read`). Width, position and split keep the meaning NetSurf's
//! front end documents (`frontends/framebuffer/font.h`); glyphs come from
//! `zoraya-text`'s cache, as 8-bit coverage placed from the pen.
//!
//! Total over any bytes: text that is not UTF-8 counts one U+FFFD per
//! invalid sequence and keeps its byte offsets; a font that is missing or
//! does not parse leaves its place empty (the next font of the list draws,
//! and with none the box).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_text::{FontList, LineMetrics, Positioned, Shaper, Simple, Text};

pub use zoraya_text::Font;

/// The one directory the fonts are read from (ADR-0326 item 3).
pub const DIR: &str = "/system/fonts";

/// The files read, in this order: Inter Regular and Medium, JetBrains Mono
/// Regular and Bold (`fonts/`, `docs/dependencies.md`).
pub const FILES: [&str; 4] = [
    "Inter-Regular.ttf",
    "Inter-Medium.ttf",
    "JetBrainsMono-Regular.ttf",
    "JetBrainsMono-Bold.ttf",
];

/// A file larger than this is not read: the largest of [`FILES`] is
/// 417,300 bytes.
pub const MAX_FILE: usize = 4 << 20;

const INTER: usize = 0;
const INTER_MEDIUM: usize = 1;
const MONO: usize = 2;
const MONO_BOLD: usize = 3;

/// Where the bytes of a font come from.
pub trait Source {
    /// The whole file at `path`, or `None`. Asked only for
    /// `/system/fonts/NAME` with NAME of [`FILES`] (INV-0326-1).
    fn read(&mut self, path: &str) -> Option<Vec<u8>>;
}

/// The path of [`FILES`]`[i]`.
pub fn path(i: usize) -> Option<String> {
    FILES.get(i).map(|name| format!("{DIR}/{name}"))
}

/// A face: a fallback list of the system's fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Sans,
    SansBold,
    Mono,
    MonoBold,
}

/// NetSurf's `plot_font_generic_family_t` for monospace; every other
/// family (sans-serif, serif, cursive, fantasy) takes sans.
pub const FAMILY_MONOSPACE: u32 = 2;

/// A weight from this one up takes the bold face.
pub const BOLD_WEIGHT: i32 = 600;

impl Face {
    pub const ALL: [Face; 4] = [Face::Sans, Face::SansBold, Face::Mono, Face::MonoBold];

    /// The face for NetSurf's family and CSS weight. Italic has no face of
    /// its own and draws upright.
    pub fn of(family: u32, weight: i32) -> Face {
        let bold = weight >= BOLD_WEIGHT;
        match (family == FAMILY_MONOSPACE, bold) {
            (true, true) => Face::MonoBold,
            (true, false) => Face::Mono,
            (false, true) => Face::SansBold,
            (false, false) => Face::Sans,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Face::Sans => 0,
            Face::SansBold => 1,
            Face::Mono => 2,
            Face::MonoBold => 3,
        }
    }

    pub fn from_index(i: u32) -> Option<Face> {
        Face::ALL.get(i as usize).copied()
    }

    /// The files of [`FILES`] in fallback order.
    pub fn files(self) -> &'static [usize] {
        match self {
            Face::Sans => &[INTER, MONO],
            Face::SansBold => &[INTER_MEDIUM, INTER, MONO_BOLD],
            Face::Mono => &[MONO, INTER],
            Face::MonoBold => &[MONO_BOLD, MONO, INTER_MEDIUM],
        }
    }
}

/// NetSurf's `PLOT_STYLE_SCALE`: sizes are points in 22.10 fixed point.
pub const PLOT_STYLE_SCALE: i64 = 1 << 10;

/// The size in pixels per em of `size` points (22.10) at `dpi`, rounded to
/// the nearest, within `zoraya-text`'s 1 to 512 (ADR-0326 item 6).
pub fn px(size: i32, dpi: u32) -> u32 {
    let den = 72 * PLOT_STYLE_SCALE;
    let n = (size.max(0) as i64).saturating_mul(dpi as i64);
    let p = n.saturating_add(den / 2) / den;
    zoraya_text::clamp_px(p.clamp(0, u32::MAX as i64) as u32)
}

/// NetSurf's dpi at a window's scale: its default 90 at scale 1, within
/// the 72 to 250 `browser_set_dpi` takes.
pub fn dpi(scale: u32) -> u32 {
    90u32.saturating_mul(scale.max(1)).clamp(72, 250)
}

/// Characters drawn as nothing at no advance, as NetSurf's internal font
/// does (`codepoint_displayable`): U+200B to U+200F.
pub const fn is_invisible(c: char) -> bool {
    matches!(c as u32, 0x200B..=0x200F)
}

/// A glyph to plot: its coverage, `width` by `height`, row by row, with its
/// top left at (`x`, `y`) from the text's origin on the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph<'a> {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub coverage: &'a [u8],
}

/// Text laid out on one line: each character's byte offset in the input and
/// the pen's x before it, and the whole advance.
struct Line {
    /// (offset, x) for every character, then (len, advance).
    bounds: Vec<(usize, i32)>,
    /// The offsets of the spaces (U+0020).
    spaces: Vec<usize>,
    glyphs: Vec<Positioned>,
    advance: i32,
}

/// The four faces, loaded.
pub struct Fontset {
    faces: Vec<Text<'static>>,
    loaded: [bool; 4],
}

impl Fontset {
    /// Reads the fonts of [`FILES`] from `source`, each at most once. The
    /// bytes live for the rest of the process (a program has one set).
    pub fn load(source: &mut impl Source) -> Fontset {
        let mut fonts: [Option<Font<'static>>; 4] = [None, None, None, None];
        for (i, slot) in fonts.iter_mut().enumerate() {
            let Some(p) = path(i) else { continue };
            let Some(bytes) = source.read(&p) else {
                continue;
            };
            if bytes.len() > MAX_FILE {
                continue;
            }
            let bytes: &'static [u8] = Vec::leak(bytes);
            *slot = Font::parse(bytes).ok();
        }
        Fontset::of(fonts)
    }

    /// The faces over parsed fonts in the order of [`FILES`].
    pub fn of(fonts: [Option<Font<'static>>; 4]) -> Fontset {
        let loaded = [0, 1, 2, 3].map(|i| fonts[i].is_some());
        let faces = Face::ALL
            .iter()
            .map(|f| {
                Text::new(FontList::new(
                    f.files().iter().filter_map(|&i| fonts[i].clone()),
                ))
            })
            .collect();
        Fontset { faces, loaded }
    }

    /// How many of [`FILES`] were read and parsed.
    pub fn loaded(&self) -> usize {
        self.loaded.iter().filter(|l| **l).count()
    }

    fn text(&mut self, face: Face) -> &mut Text<'static> {
        &mut self.faces[face.index()]
    }

    /// The fonts of a face, in fallback order.
    pub fn fonts(&self, face: Face) -> &FontList<'static> {
        &self.faces[face.index()].fonts
    }

    /// The ascent, descent and gap of a face at `px`.
    pub fn metrics(&self, face: Face, px: u32) -> LineMetrics {
        self.fonts(face).metrics(px)
    }

    fn line(&mut self, face: Face, px: u32, bytes: &[u8]) -> Line {
        // The shaped text, without the invisible characters, and for each
        // input character the index of its glyph in it (or none).
        let mut shaped = String::new();
        let mut chars: Vec<(usize, Option<usize>, bool)> = Vec::new();
        let mut spaces = Vec::new();
        let mut n = 0usize;
        for (offset, c) in decode(bytes) {
            if c == ' ' && offset > 0 {
                spaces.push(offset);
            }
            if is_invisible(c) {
                chars.push((offset, None, false));
                continue;
            }
            shaped.push(c);
            chars.push((offset, Some(n), zoraya_text::is_combining(c)));
            n += 1;
        }
        let mut glyphs = Vec::with_capacity(n);
        let text = self.text(face);
        let advance = Simple.shape(&text.fonts, &shaped, px, &mut glyphs);
        // The pen before a character: its glyph's origin, or for a mark or
        // an invisible character that of the next base (a mark sits over the
        // glyph before it, so the boundary before it is after that glyph).
        let mut bounds = Vec::with_capacity(chars.len() + 1);
        bounds.resize(chars.len() + 1, (bytes.len(), advance));
        let mut next = advance;
        for (k, (offset, glyph, mark)) in chars.iter().enumerate().rev() {
            if let (Some(j), false) = (glyph, mark)
                && let Some(g) = glyphs.get(*j)
            {
                next = g.x.min(next);
            }
            bounds[k] = (*offset, next);
        }
        Line {
            bounds,
            spaces,
            glyphs,
            advance,
        }
    }

    /// The advance of `bytes` (UTF-8) in pixels.
    pub fn width(&mut self, face: Face, px: u32, bytes: &[u8]) -> i32 {
        self.line(face, px, bytes).advance
    }

    /// The character boundary nearest to `x` (the first of two as near):
    /// its byte offset in `0..=len` and its x.
    pub fn position(&mut self, face: Face, px: u32, bytes: &[u8], x: i32) -> (usize, i32) {
        let line = self.line(face, px, bytes);
        let mut best = (0usize, 0i32);
        let mut distance = i64::MAX;
        for &(o, bx) in &line.bounds {
            let d = (bx as i64 - x as i64).abs();
            if d < distance {
                distance = d;
                best = (o, bx);
            }
        }
        best
    }

    /// Where to break `bytes` to fit `x` (NetSurf's `split`): the offset of
    /// the last space whose start the text up to the first character that
    /// passes `x` reaches, else of the first space after it, else `len`;
    /// never 0. With the x of that offset.
    pub fn split(&mut self, face: Face, px: u32, bytes: &[u8], x: i32) -> (usize, i32) {
        let line = self.line(face, px, bytes);
        if line.advance <= x {
            return (bytes.len(), line.advance);
        }
        let x_of = |offset: usize| {
            line.bounds
                .iter()
                .find(|(o, _)| *o == offset)
                .map_or(line.advance, |(_, bx)| *bx)
        };
        // The first character whose end passes x.
        let over = line
            .bounds
            .windows(2)
            .find(|w| w[1].1 > x)
            .map_or(bytes.len(), |w| w[0].0);
        if let Some(&s) = line.spaces.iter().rev().find(|&&s| s <= over) {
            return (s, x_of(s));
        }
        if let Some(&s) = line.spaces.iter().find(|&&s| s > over) {
            return (s, x_of(s));
        }
        (bytes.len(), line.advance)
    }

    /// Calls `plot` with every glyph of `bytes`, placed from the origin on
    /// the baseline, and gives the advance. Empty glyphs (a space) are not
    /// given.
    pub fn glyphs(
        &mut self,
        face: Face,
        px: u32,
        bytes: &[u8],
        mut plot: impl FnMut(Glyph<'_>),
    ) -> i32 {
        let line = self.line(face, px, bytes);
        let text = self.text(face);
        for g in &line.glyphs {
            let b = text.cache.get(&text.fonts, g.source, g.glyph, px);
            if b.width == 0 || b.height == 0 {
                continue;
            }
            plot(Glyph {
                x: g.x.saturating_add(b.left),
                y: g.y.saturating_add(b.top),
                width: b.width as u32,
                height: b.height as u32,
                coverage: &b.coverage,
            });
        }
        line.advance
    }
}

/// The characters of `bytes` with their byte offsets; each invalid
/// sequence is one U+FFFD at its offset.
pub fn decode(bytes: &[u8]) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut at = 0usize;
    bytes.utf8_chunks().flat_map(move |chunk| {
        let start = at;
        let valid = chunk.valid();
        at += valid.len();
        let bad = if chunk.invalid().is_empty() {
            None
        } else {
            let o = at;
            at += chunk.invalid().len();
            Some((o, char::REPLACEMENT_CHARACTER))
        };
        valid
            .char_indices()
            .map(move |(i, c)| (start + i, c))
            .chain(bad)
    })
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

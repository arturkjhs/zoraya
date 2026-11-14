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

#[cfg(doc)]
use zoraya_text::Simple;
use zoraya_text::{FontList, GlyphCache, LineMetrics, Source as GlyphSource, Text, raster};

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

/// The largest size whose glyphs stay in a face's cache: a glyph above it is
/// rendered into [`LARGE_SLOTS`] slots of its own and soon replaced, so a
/// page that sets a huge `font-size` cannot fill the caches with glyphs of
/// about 100 KB each (T3 of the 9f-text review). At scale 2, NetSurf's
/// 2em heading is about 60 pixels, so ordinary text stays cached.
pub const CACHE_MAX_PX: u32 = 128;

/// The bytes of coverage a face's cache holds before it is emptied and
/// starts again: with [`CACHE_MAX_PX`], the caches of the four faces stay
/// under about 4 × (`CACHE_BYTES` + one glyph) whatever a page draws.
pub const CACHE_BYTES: usize = 4 << 20;

/// Slots for the glyphs above [`CACHE_MAX_PX`], per face.
pub const LARGE_SLOTS: usize = 2;

/// A glyph as the walk shapes it: its source and id, and its origin's x in
/// pixels from the text's origin; `mark` for a combining mark, which sits
/// over the glyph before it.
#[derive(Clone, Copy)]
struct Shaped {
    source: GlyphSource,
    glyph: u16,
    x: i64,
    mark: bool,
}

/// `zoraya-text`'s [`Simple`] shaper, left to right one character at a
/// time, so that a call can stop where it has its answer: one glyph per
/// character, its advance and `kern` format 0 pairs, marks at no advance
/// over the glyph before. The pen is 26.6 in `i64`, so it does not
/// saturate as `Simple`'s `i32` does past 33 million pixels; below that the
/// two agree.
struct Walk<'f> {
    fonts: &'f FontList<'static>,
    px: u32,
    pen: i64,
    last_origin: i64,
    previous: Option<(GlyphSource, u16)>,
    steps: u64,
}

impl<'f> Walk<'f> {
    fn new(fonts: &'f FontList<'static>, px: u32) -> Walk<'f> {
        Walk {
            fonts,
            px: zoraya_text::clamp_px(px),
            pen: 0,
            last_origin: 0,
            previous: None,
            steps: 0,
        }
    }

    /// Shapes `c`, which is not invisible.
    fn step(&mut self, c: char) -> Shaped {
        self.steps += 1;
        let (source, glyph) = self.fonts.lookup(c);
        if zoraya_text::is_combining(c) {
            return Shaped {
                source,
                glyph,
                x: self.last_origin,
                mark: true,
            };
        }
        if let (Some((ps, pg)), GlyphSource::Font(i)) = (self.previous, source)
            && ps == source
            && let Some(f) = self.fonts.fonts().get(i as usize)
        {
            self.pen += i64::from(raster::scale(
                f.kerning(pg, glyph) as i32,
                self.px,
                f.units_per_em,
            ));
        }
        let x = round(self.pen);
        self.last_origin = x;
        self.pen += i64::from(self.fonts.advance(source, glyph, self.px));
        self.previous = Some((source, glyph));
        Shaped {
            source,
            glyph,
            x,
            mark: false,
        }
    }

    /// The advance of what was shaped, in pixels.
    fn advance(&self) -> i64 {
        round(self.pen)
    }
}

/// A 26.6 value to the nearest pixel, halves up, as `Simple` rounds.
fn round(v: i64) -> i64 {
    (v + i64::from(raster::ONE / 2)).div_euclid(i64::from(raster::ONE))
}

fn to_i32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// A face: its text (fonts and cache) and the slots of its large glyphs.
struct FaceText {
    text: Text<'static>,
    large: GlyphCache,
}

/// The four faces, loaded.
pub struct Fontset {
    faces: Vec<FaceText>,
    loaded: [bool; 4],
    steps: u64,
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
            .map(|f| FaceText {
                text: Text::new(FontList::new(
                    f.files().iter().filter_map(|&i| fonts[i].clone()),
                )),
                large: GlyphCache::new(LARGE_SLOTS),
            })
            .collect();
        Fontset {
            faces,
            loaded,
            steps: 0,
        }
    }

    /// How many of [`FILES`] were read and parsed.
    pub fn loaded(&self) -> usize {
        self.loaded.iter().filter(|l| **l).count()
    }

    /// The fonts of a face, in fallback order.
    pub fn fonts(&self, face: Face) -> &FontList<'static> {
        &self.faces[face.index()].text.fonts
    }

    /// The ascent, descent and gap of a face at `px`.
    pub fn metrics(&self, face: Face, px: u32) -> LineMetrics {
        self.fonts(face).metrics(px)
    }

    /// The characters shaped since the set was made: the work of the calls,
    /// for tests and measurement.
    pub fn shaped(&self) -> u64 {
        self.steps
    }

    /// The bytes of coverage the glyph caches of the four faces hold.
    pub fn cache_bytes(&self) -> usize {
        self.faces
            .iter()
            .map(|f| f.text.cache.bytes() + f.large.bytes())
            .sum()
    }

    /// Calls `at` with every character boundary of `bytes` in order: its
    /// byte offset, its x, and whether a space (U+0020, not at offset 0)
    /// starts there; then with `len` and the advance. The x of a boundary is
    /// the pen before the character's glyph, or for a mark or an invisible
    /// character that of the next glyph that is neither (a mark sits over
    /// the glyph before it, so the boundary before it is after that glyph);
    /// it never goes back. Stops when `at` gives false. Gives the advance
    /// of the part shaped.
    fn boundaries(
        &mut self,
        face: Face,
        px: u32,
        bytes: &[u8],
        mut at: impl FnMut(usize, i64, bool) -> bool,
    ) -> i64 {
        let mut walk = Walk::new(&self.faces[face.index()].text.fonts, px);
        let mut pending: Option<usize> = None;
        let mut last = 0i64;
        let mut done = false;
        for (offset, c) in decode(bytes) {
            if is_invisible(c) {
                pending.get_or_insert(offset);
                continue;
            }
            let g = walk.step(c);
            if g.mark {
                pending.get_or_insert(offset);
                continue;
            }
            last = g.x.max(last);
            if let Some(p) = pending.take() {
                // The marks and invisible characters before this glyph,
                // decoded again: no buffer that grows with them.
                for (o, _) in decode(&bytes[p..offset]) {
                    if !at(p + o, last, false) {
                        done = true;
                        break;
                    }
                }
                if done {
                    break;
                }
            }
            if !at(offset, last, c == ' ' && offset > 0) {
                done = true;
                break;
            }
        }
        if !done {
            let end = walk.advance().max(last);
            if let Some(p) = pending {
                for (o, _) in decode(&bytes[p..]) {
                    if !at(p + o, end, false) {
                        done = true;
                        break;
                    }
                }
            }
            if !done {
                at(bytes.len(), end, false);
            }
        }
        let advance = walk.advance();
        self.steps += walk.steps;
        advance
    }

    /// The advance of `bytes` (UTF-8) in pixels.
    pub fn width(&mut self, face: Face, px: u32, bytes: &[u8]) -> i32 {
        let mut walk = Walk::new(&self.faces[face.index()].text.fonts, px);
        for (_, c) in decode(bytes) {
            if !is_invisible(c) {
                walk.step(c);
            }
        }
        self.steps += walk.steps;
        to_i32(walk.advance())
    }

    /// The character boundary nearest to `x` (the first of two as near):
    /// its byte offset in `0..=len` and its x. Shapes the text only up to
    /// the first boundary past `x`.
    pub fn position(&mut self, face: Face, px: u32, bytes: &[u8], x: i32) -> (usize, i32) {
        let x = i64::from(x);
        let mut best = (0usize, 0i64);
        let mut distance = i64::MAX;
        self.boundaries(face, px, bytes, |o, bx, _| {
            let d = (bx - x).abs();
            if d < distance {
                distance = d;
                best = (o, bx);
            }
            // The boundaries never go back: past `x`, none is nearer.
            bx <= x
        });
        (best.0, to_i32(best.1))
    }

    /// Where to break `bytes` to fit `x` (NetSurf's `split`): the offset of
    /// the last space whose start the text up to the first character that
    /// passes `x` reaches, else of the first space after it, else `len`;
    /// never 0. With the x of that offset. Shapes the text only up to the
    /// first character past `x`, or to the first space after it.
    pub fn split(&mut self, face: Face, px: u32, bytes: &[u8], x: i32) -> (usize, i32) {
        let x = i64::from(x);
        let len = bytes.len();
        // The boundary before; the start of the character that passes x,
        // once found; the last space before it; the answer.
        let mut before: Option<usize> = None;
        let mut over: Option<usize> = None;
        let mut space: Option<(usize, i64)> = None;
        let mut answer: Option<(usize, i64)> = None;
        let advance = self.boundaries(face, px, bytes, |o, bx, is_space| {
            if over.is_none() {
                match before {
                    Some(b) if bx > x => {
                        over = Some(b);
                        if space.is_some() {
                            answer = space;
                            return false;
                        }
                    }
                    _ => {
                        if is_space {
                            space = Some((o, bx));
                        }
                        before = Some(o);
                        return true;
                    }
                }
            }
            // Past the character that passes x: the first space after it.
            if is_space {
                answer = Some((o, bx));
                return false;
            }
            true
        });
        // Without an answer the walk went to the end: `advance` is the
        // whole text's.
        match answer {
            Some((o, bx)) => (o, to_i32(bx)),
            None => (len, to_i32(advance)),
        }
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
        let px = zoraya_text::clamp_px(px);
        let FaceText { text, large } = &mut self.faces[face.index()];
        let fonts = &text.fonts;
        let cache = &mut text.cache;
        let mut walk = Walk::new(fonts, px);
        for (_, c) in decode(bytes) {
            if is_invisible(c) {
                continue;
            }
            let g = walk.step(c);
            let b = if px > CACHE_MAX_PX {
                large.get(fonts, g.source, g.glyph, px)
            } else {
                let misses = cache.misses;
                cache.get(fonts, g.source, g.glyph, px);
                if cache.misses != misses && cache.bytes() > CACHE_BYTES {
                    // Full: start again with the glyph just drawn.
                    *cache = GlyphCache::default();
                }
                cache.get(fonts, g.source, g.glyph, px)
            };
            if b.width == 0 || b.height == 0 {
                continue;
            }
            plot(Glyph {
                x: to_i32(g.x).saturating_add(b.left),
                y: b.top,
                width: b.width as u32,
                height: b.height as u32,
                coverage: &b.coverage,
            });
        }
        self.steps += walk.steps;
        to_i32(walk.advance())
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

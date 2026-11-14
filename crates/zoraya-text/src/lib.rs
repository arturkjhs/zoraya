//! Zoraya's text (ADR-0162, Option C): a total TrueType `glyf` reader, a
//! fixed-point scan-line rasteriser, a glyph cache of fixed capacity, a
//! simple shaper behind the [`Shaper`] trait, fallback along a font list,
//! and the box glyph. No dependency but the tokens, no `unsafe`, no
//! floating point: the pixels are a function of the font bytes, the text,
//! the size and the position (INV-0162-2), and any bytes as a font or as
//! text draw inside the target and never panic (INV-0162-1).
//!
//! Sizes here are physical pixels: a caller turns a [`zoraya_tokens`] size
//! into them with its scale (ADR-0164).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod font;
pub mod raster;
pub mod reference;

use alloc::vec::Vec;

pub use font::{Error, Font, Outline, Point};
pub use raster::Bitmap;
use zoraya_tokens::Color;

/// The largest size in pixels per em drawn; a larger request draws at it.
/// Bounds every glyph's raster and every product of a size (review finding
/// 3; ADR-0162's "any size" read as any size up to this).
pub const MAX_PX: u32 = 512;

/// A size within `1..=MAX_PX`.
pub const fn clamp_px(px: u32) -> u32 {
    if px == 0 {
        1
    } else if px > MAX_PX {
        MAX_PX
    } else {
        px
    }
}

/// Fonts in a fallback list (ADR-0162: UI, then mono, then the box).
pub const MAX_FONTS: usize = 4;

/// Where a glyph comes from: a font of the list, or the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Font(u8),
    Box,
}

/// A text style's fonts, in fallback order.
#[derive(Debug, Clone)]
pub struct FontList<'a> {
    fonts: Vec<Font<'a>>,
}

impl<'a> FontList<'a> {
    /// The fonts after the first `MAX_FONTS` are ignored.
    pub fn new(fonts: impl IntoIterator<Item = Font<'a>>) -> FontList<'a> {
        FontList {
            fonts: fonts.into_iter().take(MAX_FONTS).collect(),
        }
    }

    pub fn fonts(&self) -> &[Font<'a>] {
        &self.fonts
    }

    /// The first font that maps `c`, and its glyph; the box if none does.
    pub fn lookup(&self, c: char) -> (Source, u16) {
        for (i, f) in self.fonts.iter().enumerate() {
            if let Some(g) = f.glyph_index(c) {
                return (Source::Font(i as u8), g);
            }
        }
        (Source::Box, 0)
    }

    fn font(&self, source: Source) -> Option<&Font<'a>> {
        match source {
            Source::Font(i) => self.fonts.get(i as usize),
            Source::Box => None,
        }
    }

    /// The advance of a glyph at `px`, in 26.6.
    pub fn advance(&self, source: Source, glyph: u16, px: u32) -> i32 {
        let px = clamp_px(px);
        match self.font(source) {
            Some(f) => raster::scale(f.advance(glyph) as i32, px, f.units_per_em),
            None => box_advance(px) as i32 * raster::ONE,
        }
    }

    /// The first font's ascent, descent (positive, below the baseline)
    /// and line gap at `px`, rounded up, in pixels; for an empty list, the
    /// box's.
    pub fn metrics(&self, px: u32) -> LineMetrics {
        let px = clamp_px(px);
        let Some(f) = self.fonts.first() else {
            return LineMetrics {
                ascent: px,
                descent: px / 4,
                gap: 0,
            };
        };
        let up = |v: i32| {
            raster::scale(v, px, f.units_per_em)
                .saturating_add(raster::ONE - 1)
                .div_euclid(raster::ONE)
        };
        LineMetrics {
            ascent: up(f.ascent as i32).max(0) as u32,
            descent: up(-(f.descent as i32)).max(0) as u32,
            gap: up(f.line_gap as i32).max(0) as u32,
        }
    }
}

/// A line's vertical metrics in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineMetrics {
    pub ascent: u32,
    pub descent: u32,
    pub gap: u32,
}

impl LineMetrics {
    pub const fn height(&self) -> u32 {
        self.ascent + self.descent + self.gap
    }
}

/// The box glyph's advance at `px`.
pub const fn box_advance(px: u32) -> u32 {
    let a = px / 2 + 1;
    if a < 3 { 3 } else { a }
}

/// One glyph placed by a shaper: its source, its id, its pen position in
/// whole pixels from the text's origin (on the baseline), and the byte
/// index in the text of the character it draws (the output a HarfBuzz-like
/// shaper gives, so a second shaper changes nothing else, ADR-0162).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Positioned {
    pub source: Source,
    pub glyph: u16,
    pub x: i32,
    pub y: i32,
    pub cluster: u32,
}

/// Maps text to positioned glyphs.
pub trait Shaper {
    /// Appends the glyphs of `text` at `px` to `out` and returns the
    /// advance of the whole, in pixels.
    fn shape(&self, fonts: &FontList<'_>, text: &str, px: u32, out: &mut Vec<Positioned>) -> i32;
}

/// ADR-0162's simple shaper: one glyph per character with its advance and
/// `kern` format 0 pairs, fallback along the list, combining marks
/// (U+0300 to U+036F) at no advance over the glyph before. Enough for
/// Latin and Cyrillic, whose letters are all precomposed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Simple;

/// A combining diacritical mark, drawn over the glyph before it.
pub const fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F)
}

impl Shaper for Simple {
    fn shape(&self, fonts: &FontList<'_>, text: &str, px: u32, out: &mut Vec<Positioned>) -> i32 {
        let px = clamp_px(px);
        // The pen in 26.6; each glyph's origin is snapped to a whole pixel.
        let mut pen: i32 = 0;
        let mut last_origin: i32 = 0;
        let mut previous: Option<(Source, u16)> = None;
        for (i, c) in text.char_indices() {
            let (source, glyph) = fonts.lookup(c);
            if is_combining(c) {
                out.push(Positioned {
                    source,
                    glyph,
                    x: last_origin,
                    y: 0,
                    cluster: i as u32,
                });
                continue;
            }
            if let (Some((ps, pg)), Source::Font(_)) = (previous, source)
                && ps == source
                && let Some(f) = fonts.font(source)
            {
                pen = pen.saturating_add(raster::scale(
                    f.kerning(pg, glyph) as i32,
                    px,
                    f.units_per_em,
                ));
            }
            let x = pen.saturating_add(raster::ONE / 2).div_euclid(raster::ONE);
            out.push(Positioned {
                source,
                glyph,
                x,
                y: 0,
                cluster: i as u32,
            });
            last_origin = x;
            pen = pen.saturating_add(fonts.advance(source, glyph, px));
            previous = Some((source, glyph));
        }
        pen.saturating_add(raster::ONE / 2).div_euclid(raster::ONE)
    }
}

/// A key of the glyph cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    source: Source,
    glyph: u16,
    px: u16,
}

/// Rendered glyphs, direct-mapped by (font, glyph, size), of a fixed
/// capacity: a new glyph replaces the one in its slot. Counts its hits
/// and misses (the measurement `plan.md` asks of 7a).
pub struct GlyphCache {
    slots: Vec<Option<(Key, Bitmap)>>,
    scratch: Outline,
    pub hits: u64,
    pub misses: u64,
}

/// Slots of a cache made by [`GlyphCache::default`].
pub const CACHE_SLOTS: usize = 1024;

impl Default for GlyphCache {
    fn default() -> GlyphCache {
        GlyphCache::new(CACHE_SLOTS)
    }
}

impl GlyphCache {
    /// A cache of `slots` entries, rounded up to a power of two.
    pub fn new(slots: usize) -> GlyphCache {
        let n = slots.max(1).next_power_of_two();
        let mut v = Vec::with_capacity(n);
        v.resize_with(n, || None);
        GlyphCache {
            slots: v,
            scratch: Outline::default(),
            hits: 0,
            misses: 0,
        }
    }

    /// The bytes of coverage the cached glyphs hold.
    pub fn bytes(&self) -> usize {
        self.slots
            .iter()
            .flatten()
            .map(|(_, b)| b.coverage.len())
            .sum()
    }

    /// The key's first slot (Fibonacci hashing); its second is the
    /// neighbour, `slot ^ 1`: two ways, the newer glyph in the first.
    fn slot(&self, key: &Key) -> usize {
        let s = match key.source {
            Source::Font(i) => i as u64 + 1,
            Source::Box => 0,
        };
        let k = key.glyph as u64 | s << 16 | (key.px as u64) << 24;
        let bits = self.slots.len().trailing_zeros();
        if bits == 0 {
            return 0;
        }
        (k.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - bits)) as usize
    }

    /// The bitmap of a glyph at `px`, rendered on a miss; a glyph that
    /// cannot be read is the box.
    pub fn get(&mut self, fonts: &FontList<'_>, source: Source, glyph: u16, px: u32) -> &Bitmap {
        let key = Key {
            source,
            glyph,
            px: clamp_px(px) as u16,
        };
        let first = self.slot(&key);
        let second = if self.slots.len() > 1 {
            first ^ 1
        } else {
            first
        };
        let found = |s: &Option<(Key, Bitmap)>| s.as_ref().is_some_and(|(k, _)| *k == key);
        let at = if found(&self.slots[first]) {
            self.hits += 1;
            first
        } else if found(&self.slots[second]) {
            self.hits += 1;
            second
        } else {
            self.misses += 1;
            let px = key.px as u32;
            let bitmap = fonts
                .font(source)
                .and_then(|f| raster::render_glyph(f, glyph, px, &mut self.scratch))
                .unwrap_or_else(|| box_glyph(fonts, px));
            // A free way takes it; else the first way's glyph moves to
            // the second and the new one takes the first.
            if self.slots[first].is_none() {
                self.slots[first] = Some((key, bitmap));
                first
            } else if self.slots[second].is_none() {
                self.slots[second] = Some((key, bitmap));
                second
            } else {
                if second != first {
                    self.slots[second] = self.slots[first].take();
                }
                self.slots[first] = Some((key, bitmap));
                first
            }
        };
        match &self.slots[at] {
            Some((_, b)) => b,
            None => unreachable!("the slot was just filled"),
        }
    }
}

/// The box at `px`: as high as the first font's ascent's seven tenths,
/// one pixel inside its advance, standing on the baseline.
pub fn box_glyph(fonts: &FontList<'_>, px: u32) -> Bitmap {
    let px = clamp_px(px);
    let ascent = fonts.metrics(px).ascent;
    let height = (ascent * 7 / 10).max(3);
    let width = box_advance(px).saturating_sub(2).max(2);
    let stroke = (px / 12).max(1);
    raster::render_box(width, height, stroke, 1, -(height as i32))
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn right(&self) -> i64 {
        self.x as i64 + self.width as i64
    }

    fn bottom(&self) -> i64 {
        self.y as i64 + self.height as i64
    }
}

/// Pixels to draw into: rows of `stride`, and the rectangle drawing may
/// touch (clipped again to the slice).
pub struct Target<'p> {
    pub pixels: &'p mut [u32],
    pub stride: usize,
    pub clip: Rect,
}

impl Target<'_> {
    /// The drawable area: the clip within the slice, as `[x0, x1) × [y0, y1)`.
    fn bounds(&self) -> (i64, i64, i64, i64) {
        if self.stride == 0 {
            return (0, 0, 0, 0);
        }
        let rows = (self.pixels.len() / self.stride) as i64;
        let x0 = (self.clip.x as i64).max(0);
        let y0 = (self.clip.y as i64).max(0);
        let x1 = self.clip.right().min(self.stride as i64);
        let y1 = self.clip.bottom().min(rows);
        (x0, y0, x1.max(x0), y1.max(y0))
    }

    /// Fills `r` (within the clip) with `color`.
    pub fn fill(&mut self, r: Rect, color: Color) {
        let (x0, y0, x1, y1) = self.bounds();
        let (a0, b0) = ((r.x as i64).max(x0), (r.y as i64).max(y0));
        let (a1, b1) = (r.right().min(x1), r.bottom().min(y1));
        for y in b0..b1 {
            let row = y as usize * self.stride;
            for x in a0..a1 {
                self.pixels[row + x as usize] = color.pixel();
            }
        }
    }

    /// Draws `bitmap` with the pen at `(x, y)`: each pixel blended with
    /// `fg` by its coverage, nothing outside the clip or the slice.
    pub fn draw(&mut self, bitmap: &Bitmap, x: i32, y: i32, fg: Color) {
        let (x0, y0, x1, y1) = self.bounds();
        let left = x as i64 + bitmap.left as i64;
        let top = y as i64 + bitmap.top as i64;
        for by in 0..bitmap.height {
            let py = top + by as i64;
            if py < y0 || py >= y1 {
                continue;
            }
            let row = py as usize * self.stride;
            for bx in 0..bitmap.width {
                let px = left + bx as i64;
                if px < x0 || px >= x1 {
                    continue;
                }
                // A caller's own bitmap may be short: never index past it.
                let c = bitmap
                    .coverage
                    .get(by * bitmap.width + bx)
                    .copied()
                    .unwrap_or(0);
                if c == 0 {
                    continue;
                }
                let at = row + px as usize;
                self.pixels[at] = Color::of_pixel(self.pixels[at]).blend(fg, c).pixel();
            }
        }
    }
}

/// Text with its fonts, shaper and cache.
pub struct Text<'a, S: Shaper = Simple> {
    pub fonts: FontList<'a>,
    pub shaper: S,
    pub cache: GlyphCache,
    glyphs: Vec<Positioned>,
}

impl<'a> Text<'a, Simple> {
    pub fn new(fonts: FontList<'a>) -> Text<'a, Simple> {
        Text {
            fonts,
            shaper: Simple,
            cache: GlyphCache::default(),
            glyphs: Vec::new(),
        }
    }
}

impl<S: Shaper> Text<'_, S> {
    /// The advance of `text` at `px`, in pixels.
    pub fn measure(&mut self, text: &str, px: u32) -> i32 {
        self.glyphs.clear();
        self.shaper.shape(&self.fonts, text, px, &mut self.glyphs)
    }

    /// Draws `text` at `px` with the pen starting at `(x, baseline)`;
    /// returns its advance.
    pub fn draw(
        &mut self,
        target: &mut Target<'_>,
        text: &str,
        px: u32,
        x: i32,
        baseline: i32,
        fg: Color,
    ) -> i32 {
        self.glyphs.clear();
        let advance = self.shaper.shape(&self.fonts, text, px, &mut self.glyphs);
        for g in &self.glyphs {
            let b = self.cache.get(&self.fonts, g.source, g.glyph, px);
            target.draw(b, x.saturating_add(g.x), baseline.saturating_add(g.y), fg);
        }
        advance
    }
}

/// A terminal's cell (ADR-0162): the monospace font's advance at the size,
/// rounded up, and the line's height; the baseline from the cell's top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellMetrics {
    pub width: u32,
    pub height: u32,
    pub baseline: u32,
}

impl CellMetrics {
    /// The cell of the first font of `fonts` at `px`.
    pub fn of(fonts: &FontList<'_>, px: u32) -> CellMetrics {
        let line = fonts.metrics(px);
        let (source, glyph) = fonts.lookup('M');
        let advance = fonts.advance(source, glyph, px);
        let width = (advance + raster::ONE - 1).div_euclid(raster::ONE).max(1) as u32;
        CellMetrics {
            width,
            height: line.height().max(1),
            baseline: line.ascent,
        }
    }
}

impl<S: Shaper> Text<'_, S> {
    /// Draws one terminal cell at `(x, y)` (its top left): `bg`, then `c`
    /// in `fg` (the last argument is `(fg, bg)`). A glyph of the first font starts at the cell's start; a
    /// fallback glyph or the box is centred in the cell at the cell's
    /// advance (ADR-0162). Nothing is drawn outside the cell or the clip.
    pub fn draw_cell(
        &mut self,
        target: &mut Target<'_>,
        cell: CellMetrics,
        (x, y): (i32, i32),
        c: char,
        px: u32,
        (fg, bg): (Color, Color),
    ) {
        let r = Rect::new(x, y, cell.width, cell.height);
        let saved = target.clip;
        target.clip = intersect(saved, r);
        target.fill(r, bg);
        if c != ' ' {
            let (source, glyph) = self.fonts.lookup(c);
            let pen = if source == Source::Font(0) {
                x
            } else {
                let advance = self
                    .fonts
                    .advance(source, glyph, px)
                    .div_euclid(raster::ONE);
                x.saturating_add((cell.width as i32 - advance) / 2)
            };
            let b = self.cache.get(&self.fonts, source, glyph, px);
            target.draw(b, pen, y.saturating_add(cell.baseline as i32), fg);
        }
        target.clip = saved;
    }
}

/// The overlap of two rectangles (empty if none).
pub fn intersect(a: Rect, b: Rect) -> Rect {
    let x0 = (a.x as i64).max(b.x as i64);
    let y0 = (a.y as i64).max(b.y as i64);
    let x1 = a.right().min(b.right()).max(x0);
    let y1 = a.bottom().min(b.bottom()).max(y0);
    Rect::new(
        x0.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        y0.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        (x1 - x0).min(u32::MAX as i64) as u32,
        (y1 - y0).min(u32::MAX as i64) as u32,
    )
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_holes;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

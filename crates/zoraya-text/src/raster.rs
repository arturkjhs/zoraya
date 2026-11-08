//! The scan-line rasteriser (ADR-0162): outlines flattened to segments in
//! 26.6 fixed point, the signed area of each segment accumulated into the
//! two cells it touches per row, then a prefix sum per row gives 8-bit
//! coverage (the font-rs method that `ab_glyph_rasterizer` and `fontdue`
//! use, in integers). No floating point anywhere, so the coverage is a
//! function of the input alone (INV-0162-2), on any host and in QEMU.
//!
//! Units: a coordinate is 1/64 of a pixel. A segment of height `d` (1/64
//! pixels) whose x runs from `x0` to `x1` inside one cell adds
//! `d * (128 - (fx0 + fx1))` to that cell and `d * (fx0 + fx1)` to the
//! next, where `fx` are the x offsets inside the cell: twice the area to
//! the right of the segment, so that a full pixel is `64 * 128 = 8192`
//! and every value is exact.

use alloc::vec;
use alloc::vec::Vec;

use crate::font::{Font, Outline, Point};

/// One pixel in 26.6.
pub const ONE: i32 = 64;
/// Full coverage in accumulator units.
const FULL: i64 = 64 * 128;

/// A coverage accumulator of `width × height` pixels.
pub struct Raster {
    width: usize,
    height: usize,
    stride: usize,
    acc: Vec<i32>,
}

/// Floor division that is the same on every host.
fn div_floor(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}

impl Raster {
    pub fn new(width: usize, height: usize) -> Raster {
        let stride = width + 2;
        Raster {
            width,
            height,
            stride,
            acc: vec![0; stride * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Adds a piece of a segment that lies inside one column of row `row`:
    /// x from `x0` to `x1` (26.6, clamped to the raster), height `d`.
    fn cell(&mut self, row: usize, x0: i32, x1: i32, d: i32) {
        let lo = x0.min(x1);
        let column = (lo / ONE) as usize;
        let column = column.min(self.width);
        let base = column as i32 * ONE;
        let f = (x0 - base) + (x1 - base);
        let at = row * self.stride + column;
        let d = d as i64;
        let here = d * (2 * ONE as i64) - d * f as i64;
        let next = d * f as i64;
        // Bounded by the clamping above: column <= width, so at + 1 is
        // inside the row's stride of width + 2.
        if let Some(a) = self.acc.get_mut(at) {
            *a = a.saturating_add(here as i32);
        }
        if let Some(a) = self.acc.get_mut(at + 1) {
            *a = a.saturating_add(next as i32);
        }
    }

    /// One row's part of a segment, from `(x0, 0)` to `(x1, d)`: split at
    /// every column boundary it crosses.
    fn row(&mut self, row: usize, x0: i32, x1: i32, d: i32) {
        let (c0, c1) = (x0 / ONE, x1 / ONE);
        // Inside one column, or ending exactly on the next boundary.
        let same = c0 == c1
            || (x1 > x0 && x1 == c1 * ONE && c1 == c0 + 1)
            || (x0 > x1 && x0 == c0 * ONE && c0 == c1 + 1);
        if same {
            self.cell(row, x0, x1, d);
            return;
        }
        let dx = (x1 - x0) as i64;
        let mut x = x0;
        let mut done: i32 = 0;
        loop {
            // The next boundary in the direction of travel.
            let boundary = if x1 > x0 {
                (x / ONE + 1) * ONE
            } else if x % ONE == 0 {
                x - ONE
            } else {
                (x / ONE) * ONE
            };
            let last = if x1 > x0 {
                boundary >= x1
            } else {
                boundary <= x1
            };
            if last {
                self.cell(row, x, x1, d - done);
                return;
            }
            // The height reached at the boundary, from the segment's start,
            // so that rounding does not accumulate.
            let reached = div_floor(d as i64 * (boundary - x0) as i64, dx) as i32;
            self.cell(row, x, boundary, reached - done);
            done = reached;
            x = boundary;
        }
    }

    /// Accumulates a line segment, coordinates in 26.6 with y down. Rows
    /// outside the raster are skipped; x is clamped to `[0, width]`, which
    /// keeps the winding right for what is visible.
    pub fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32)) {
        if y0 == y1 {
            return;
        }
        let (dir, (ax, ay), (bx, by)) = if y0 < y1 {
            (1, (x0, y0), (x1, y1))
        } else {
            (-1, (x1, y1), (x0, y0))
        };
        let w = (self.width as i32).saturating_mul(ONE);
        let h = (self.height as i32).saturating_mul(ONE);
        let top = ay.max(0);
        let bottom = by.min(h);
        if top >= bottom {
            return;
        }
        let (ax64, ay64, dx, dy) = (
            ax as i64,
            ay as i64,
            bx as i64 - ax as i64,
            by as i64 - ay as i64,
        );
        let x_at = |y: i32| -> i32 {
            // i128: a hostile segment spans the whole i32 range both ways.
            let t = (dx as i128 * (y as i64 - ay64) as i128).div_euclid(dy as i128);
            (ax64 as i128 + t).clamp(0, w as i128) as i32
        };
        let mut y = top;
        while y < bottom {
            let row = (y / ONE) as usize;
            let next = (y / ONE).saturating_add(1).saturating_mul(ONE).min(bottom);
            self.row(row, x_at(y), x_at(next), (next - y) * dir);
            y = next;
        }
    }

    /// A quadratic Bézier, flattened into segments: enough that each is
    /// within a fraction of a pixel of the curve, at most 32.
    pub fn quad(&mut self, p0: (i32, i32), c: (i32, i32), p1: (i32, i32)) {
        let ddx = (p0.0 as i64 - 2 * c.0 as i64 + p1.0 as i64).abs();
        let ddy = (p0.1 as i64 - 2 * c.1 as i64 + p1.1 as i64).abs();
        // n = ceil(2 * sqrt(dd in pixels)) = ceil(sqrt(dd / 16)).
        let q = (ddx + ddy) / 16;
        let mut n = isqrt(q);
        if n * n < q {
            n += 1;
        }
        let n = n.clamp(1, 32);
        let mut prev = p0;
        for i in 1..=n {
            let (t, mt) = (i, n - i);
            let nn = n * n;
            let at = |a: i32, b: i32, e: i32| -> i32 {
                let v = a as i64 * mt * mt + 2 * b as i64 * mt * t + e as i64 * t * t;
                div_floor(v + nn / 2, nn) as i32
            };
            let p = (at(p0.0, c.0, p1.0), at(p0.1, c.1, p1.1));
            self.line(prev, p);
            prev = p;
        }
    }

    /// 8-bit coverage, row-major, `width × height`.
    pub fn finish(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.width * self.height];
        for y in 0..self.height {
            let mut sum: i64 = 0;
            for x in 0..self.width {
                sum += self.acc[y * self.stride + x] as i64;
                let c = sum.abs().min(FULL);
                out[y * self.width + x] = ((c * 255 + FULL / 2) / FULL) as u8;
            }
        }
        out
    }

    /// Draws the contours of `points`, already in 26.6 raster coordinates
    /// (y down), with TrueType's implied on-curve points between two
    /// off-curve ones.
    pub fn contour(&mut self, points: &[(i32, i32, bool)]) {
        let n = points.len();
        if n < 2 {
            return;
        }
        let mid = |a: (i32, i32), b: (i32, i32)| -> (i32, i32) {
            (
                ((a.0 as i64 + b.0 as i64) >> 1) as i32,
                ((a.1 as i64 + b.1 as i64) >> 1) as i32,
            )
        };
        // Start on an on-curve point, or on the midpoint of the last and
        // the first when every point is off the curve.
        let first_on = points.iter().position(|p| p.2);
        let (start, begin) = match first_on {
            Some(i) => ((points[i].0, points[i].1), i),
            None => (
                mid(
                    (points[n - 1].0, points[n - 1].1),
                    (points[0].0, points[0].1),
                ),
                n - 1,
            ),
        };
        let mut prev = start;
        let mut control: Option<(i32, i32)> = None;
        for k in 1..=n {
            let p = points[(begin + k) % n];
            let at = (p.0, p.1);
            if p.2 {
                match control.take() {
                    Some(c) => self.quad(prev, c, at),
                    None => self.line(prev, at),
                }
                prev = at;
            } else {
                if let Some(c) = control {
                    let m = mid(c, at);
                    self.quad(prev, c, m);
                    prev = m;
                }
                control = Some(at);
            }
        }
        match control {
            Some(c) => self.quad(prev, c, start),
            None => self.line(prev, start),
        }
    }
}

fn isqrt(v: i64) -> i64 {
    if v <= 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// A rendered glyph: coverage, and where its top left lies from the pen
/// (x right, y down; the pen is on the baseline at a whole pixel).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub left: i32,
    pub top: i32,
    pub coverage: Vec<u8>,
}

/// A font unit value at `px` pixels per em, in 26.6, rounded half up.
/// Saturates at the ends of `i32` (review finding 1).
pub fn scale(v: i32, px: u32, units_per_em: u16) -> i32 {
    let n = v as i64 * px as i64 * ONE as i64;
    let d = (units_per_em as i64).max(1);
    div_floor(2 * n + d, 2 * d).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// The largest bitmap side, in pixels, at `px` pixels per em: a hostile
/// font with points far outside the em is clipped, not allocated for.
pub fn max_side(px: u32) -> i32 {
    (px as i32).saturating_mul(4).saturating_add(8)
}

/// Rasterises an outline at `px` pixels per em with the pen at the origin.
pub fn render_outline(outline: &Outline, px: u32, units_per_em: u16) -> Bitmap {
    let px = crate::clamp_px(px);
    let map = |p: &Point| {
        (
            scale(p.x, px, units_per_em),
            scale(p.y, px, units_per_em).saturating_neg(),
        )
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in &outline.points {
        let (x, y) = map(p);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if outline.points.is_empty() || outline.ends.is_empty() {
        return Bitmap::default();
    }
    let floor = |v: i32| div_floor(v as i64, ONE as i64) as i32;
    let ceil = |v: i32| div_floor(v as i64 + ONE as i64 - 1, ONE as i64) as i32;
    let limit = max_side(px);
    let left = floor(min_x).clamp(-limit, limit);
    let top = floor(min_y).clamp(-limit, limit);
    let right = ceil(max_x).clamp(left, left.saturating_add(limit));
    let bottom = ceil(max_y).clamp(top, top.saturating_add(limit));
    let width = (right - left).max(1) as usize;
    let height = (bottom - top).max(1) as usize;
    let mut r = Raster::new(width, height);
    let mut points = Vec::new();
    for contour in outline.contours() {
        points.clear();
        for p in contour {
            let (x, y) = map(p);
            points.push((
                x.saturating_sub(left.saturating_mul(ONE)),
                y.saturating_sub(top.saturating_mul(ONE)),
                p.on_curve,
            ));
        }
        r.contour(&points);
    }
    Bitmap {
        width,
        height,
        left,
        top,
        coverage: r.finish(),
    }
}

/// Rasterises `glyph` of `font`; None if the glyph cannot be read (the
/// caller draws the box).
pub fn render_glyph(font: &Font<'_>, glyph: u16, px: u32, scratch: &mut Outline) -> Option<Bitmap> {
    font.outline(glyph, scratch).ok()?;
    Some(render_outline(scratch, px, font.units_per_em))
}

/// The box glyph (ADR-0162), drawn by the rasteriser itself: a hollow
/// rectangle `width × height` pixels with a stroke of `stroke`, its top
/// left at `(left, top)` from the pen.
/// Sides are at most [`crate::MAX_PX`] times 4 plus 8 (review finding 5).
pub fn render_box(width: u32, height: u32, stroke: u32, left: i32, top: i32) -> Bitmap {
    let side = max_side(crate::MAX_PX) as u32;
    let (width, height, stroke) = (width.min(side), height.min(side), stroke.clamp(1, side));
    let (w, h, s) = (width as usize, height as usize, stroke as i32);
    let mut r = Raster::new(w, h);
    let (x1, y1) = (width as i32 * ONE, height as i32 * ONE);
    let rect = |r: &mut Raster, a: (i32, i32), b: (i32, i32), clockwise: bool| {
        let c = [(a.0, a.1), (b.0, a.1), (b.0, b.1), (a.0, b.1)];
        let order: [usize; 4] = if clockwise {
            [0, 1, 2, 3]
        } else {
            [0, 3, 2, 1]
        };
        for i in 0..4 {
            r.line(c[order[i]], c[order[(i + 1) % 4]]);
        }
    };
    rect(&mut r, (0, 0), (x1, y1), true);
    let inner = s * ONE;
    if x1 > 2 * inner && y1 > 2 * inner {
        rect(&mut r, (inner, inner), (x1 - inner, y1 - inner), false);
    }
    Bitmap {
        width: w,
        height: h,
        left,
        top,
        coverage: r.finish(),
    }
}

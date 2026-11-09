//! The compositor's drawing (ADR-0160, ADR-0161): rounded shapes with the
//! direction's corner, soft shadows, the procedural wallpaper, box blurs,
//! and the trusted material, all in integer arithmetic so that a scene is
//! the same bytes on every host and in QEMU (ADR-0161 driver 5). Sizes are
//! physical pixels; callers turn tokens into them with their scale.
//!
//! The mockup tool (`tools/mockups`) draws the same things in `f32`; this
//! crate is the system's version, and close to it rather than equal.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod wallpaper;

use alloc::vec;
use alloc::vec::Vec;

use zoraya_text::raster::{ONE, Raster};
use zoraya_tokens::{Color, CornerShape};

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub const fn right(&self) -> i32 {
        self.x.saturating_add(self.w)
    }

    pub const fn bottom(&self) -> i32 {
        self.y.saturating_add(self.h)
    }

    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// The overlap, or an empty rectangle.
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 <= x0 || y1 <= y0 {
            return Rect::default();
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Grown by `d` on every side.
    pub fn grow(&self, d: i32) -> Rect {
        Rect::new(
            self.x.saturating_sub(d),
            self.y.saturating_sub(d),
            self.w.saturating_add(2 * d).max(0),
            self.h.saturating_add(2 * d).max(0),
        )
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

/// 8-bit coverage over a rectangle of `w × h` pixels.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub cov: Vec<u8>,
}

impl Mask {
    pub fn new(w: usize, h: usize) -> Mask {
        Mask {
            w,
            h,
            cov: vec![0; w * h],
        }
    }

    pub fn at(&self, x: usize, y: usize) -> u8 {
        if x < self.w && y < self.h {
            self.cov[y * self.w + x]
        } else {
            0
        }
    }
}

/// The largest side of a mask: a shape larger than any display is clipped
/// to it, never allocated for.
pub const MAX_SIDE: usize = 8192;

/// Supersamples per pixel side for a corner's coverage.
const SAMPLES: i64 = 4;

/// Whether the subsample at `(x, y)` (in 1/`SAMPLES` pixels, from the
/// corner's outer point, inward) is inside a corner of radius `r` pixels.
pub(crate) fn inside_corner(shape: CornerShape, r: i64, x: i64, y: i64) -> bool {
    match shape {
        CornerShape::Circle => {
            // The quarter circle centred r in from the corner.
            let rs = r * SAMPLES;
            let (dx, dy) = (rs - x, rs - y);
            dx <= 0 || dy <= 0 || dx * dx + dy * dy <= rs * rs
        }
        CornerShape::Smooth => {
            // ADR-0161's continuous corner: a superellipse spanning 1.6
            // radii, here of exponent 3 so that it is integer arithmetic.
            let s = (r * 16 + 5) / 10 * SAMPLES;
            let (dx, dy) = (s - x, s - y);
            dx <= 0 || dy <= 0 || dx * dx * dx + dy * dy * dy <= s * s * s
        }
    }
}

/// How far a corner of radius `r` reaches along each edge, in pixels.
pub fn corner_span(shape: CornerShape, r: u32) -> u32 {
    match shape {
        CornerShape::Circle => r,
        CornerShape::Smooth => (r * 16 + 5) / 10,
    }
}

/// The coverage of a rounded rectangle of `w × h` with corners of radius
/// `r` in the direction's shape. The corners are clamped to half the
/// shorter side.
pub fn rounded_rect(w: usize, h: usize, r: u32, shape: CornerShape) -> Mask {
    let (w, h) = (w.min(MAX_SIDE), h.min(MAX_SIDE));
    let mut m = Mask::new(w, h);
    m.cov.fill(255);
    let half = (w.min(h) / 2) as u32;
    let span = corner_span(shape, r).min(half);
    // The radius that gives that span.
    let r = match shape {
        CornerShape::Circle => span,
        CornerShape::Smooth => span * 10 / 16,
    } as i64;
    if r == 0 {
        return m;
    }
    let span = span as usize;
    for cy in 0..span {
        for cx in 0..span {
            let mut n = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = cx as i64 * SAMPLES + sx;
                    let y = cy as i64 * SAMPLES + sy;
                    if inside_corner(shape, r, x, y) {
                        n += 1;
                    }
                }
            }
            let c = (n * 255 / (SAMPLES * SAMPLES)) as u8;
            // The four corners, mirrored.
            for (x, y) in [
                (cx, cy),
                (w - 1 - cx, cy),
                (cx, h - 1 - cy),
                (w - 1 - cx, h - 1 - cy),
            ] {
                let at = y * w + x;
                m.cov[at] = m.cov[at].min(c);
            }
        }
    }
    m
}

/// The outline of a rounded rectangle, `t` pixels wide, inside its edge.
pub fn rounded_outline(w: usize, h: usize, r: u32, t: u32, shape: CornerShape) -> Mask {
    let mut outer = rounded_rect(w, h, r, shape);
    let t = t as usize;
    if w <= 2 * t || h <= 2 * t {
        return outer;
    }
    let inner = rounded_rect(w - 2 * t, h - 2 * t, r.saturating_sub(t as u32), shape);
    for y in 0..inner.h {
        for x in 0..inner.w {
            let at = (y + t) * outer.w + x + t;
            let i = inner.cov[y * inner.w + x] as u32;
            let o = outer.cov[at] as u32;
            outer.cov[at] = (o * (255 - i) / 255) as u8;
        }
    }
    outer
}

/// One box pass of radius `r` over a channel of `w × h`, the edges
/// repeated (`clamp`) or faded to nothing.
pub(crate) fn box_pass(src: &[u32], dst: &mut [u32], w: usize, h: usize, r: usize, clamp: bool) {
    let win = (2 * r + 1) as u32;
    let mut tmp = vec![0u32; w * h];
    let get = |v: &[u32], i: isize, n: usize, stride: usize, base: usize| -> u32 {
        if i < 0 || i >= n as isize {
            if !clamp {
                return 0;
            }
            let c = i.clamp(0, n as isize - 1) as usize;
            v[base + c * stride]
        } else {
            v[base + i as usize * stride]
        }
    };
    for y in 0..h {
        let base = y * w;
        let mut sum: u32 = (-(r as isize)..=r as isize)
            .map(|i| get(src, i, w, 1, base))
            .sum();
        for x in 0..w {
            tmp[base + x] = (sum + win / 2) / win;
            sum += get(src, x as isize + r as isize + 1, w, 1, base);
            sum -= get(src, x as isize - r as isize, w, 1, base);
        }
    }
    for x in 0..w {
        let mut sum: u32 = (-(r as isize)..=r as isize)
            .map(|i| get(&tmp, i, h, w, x))
            .sum();
        for y in 0..h {
            dst[y * w + x] = (sum + win / 2) / win;
            sum += get(&tmp, y as isize + r as isize + 1, h, w, x);
            sum -= get(&tmp, y as isize - r as isize, h, w, x);
        }
    }
}

/// Three box passes of radius `radius / 3` (at least 1), which approximate
/// a Gaussian of about `radius`: ADR-0161's blur, deterministic.
fn blur_channel(c: &mut [u32], w: usize, h: usize, radius: usize, clamp: bool) {
    let r = (radius / 3).max(1);
    let mut out = vec![0u32; w * h];
    for _ in 0..3 {
        box_pass(c, &mut out, w, h, r, clamp);
        c.copy_from_slice(&out);
    }
}

/// Blurs the pixels `r` of an image of `width` (rows of that stride) in
/// place, each channel apart, the edges of `r` repeated. A radius of 0
/// leaves them.
pub fn blur_region(pixels: &mut [u32], width: usize, r: Rect, radius: usize) {
    if radius == 0 || width == 0 {
        return;
    }
    let height = pixels.len() / width;
    let r = r.intersect(&Rect::new(0, 0, width as i32, height as i32));
    if r.is_empty() {
        return;
    }
    let (w, h) = (r.w as usize, r.h as usize);
    for shift in [16u32, 8, 0] {
        let mut c: Vec<u32> = Vec::with_capacity(w * h);
        for y in 0..h {
            let row = (r.y as usize + y) * width + r.x as usize;
            c.extend(pixels[row..row + w].iter().map(|p| (p >> shift) & 0xff));
        }
        blur_channel(&mut c, w, h, radius, true);
        for y in 0..h {
            let row = (r.y as usize + y) * width + r.x as usize;
            for x in 0..w {
                let p = &mut pixels[row + x];
                *p = (*p & !(0xff << shift)) | (c[y * w + x].min(255) << shift);
            }
        }
    }
}

/// `src`, `sw × sh` pixels, scaled to `w × h` bilinearly, each channel
/// apart: corner to corner, so the source's corners are the result's. For
/// smooth images made at a fraction of the size (the wallpaper and its
/// blur: a sixteenth of the work at a quarter of the size each way).
pub fn upscale(src: &[u32], sw: usize, sh: usize, w: usize, h: usize) -> Vec<u32> {
    let mut out = vec![0u32; w * h];
    if sw == 0 || sh == 0 || src.len() < sw * sh || w == 0 || h == 0 {
        return out;
    }
    // Fixed point, 16 bits: the source position of each column and row.
    let pos = |i: usize, n: usize, sn: usize| -> (usize, u32) {
        if n <= 1 || sn <= 1 {
            return (0, 0);
        }
        let p = (i as u64 * (sn as u64 - 1) << 16) / (n as u64 - 1);
        let whole = ((p >> 16) as usize).min(sn - 1);
        (whole, (p & 0xffff) as u32)
    };
    let cols: Vec<(usize, u32)> = (0..w).map(|x| pos(x, w, sw)).collect();
    let lerp = |a: u32, b: u32, t: u32| -> u32 {
        let mut c = 0;
        for shift in [0u32, 8, 16, 24] {
            let (ca, cb) = ((a >> shift) & 0xff, (b >> shift) & 0xff);
            let v = (ca * (0x10000 - t) + cb * t + 0x8000) >> 16;
            c |= v.min(255) << shift;
        }
        c
    };
    for y in 0..h {
        let (sy, ty) = pos(y, h, sh);
        let sy1 = (sy + 1).min(sh - 1);
        let (r0, r1) = (&src[sy * sw..sy * sw + sw], &src[sy1 * sw..sy1 * sw + sw]);
        for (x, &(sx, tx)) in cols.iter().enumerate() {
            let sx1 = (sx + 1).min(sw - 1);
            let top = lerp(r0[sx], r0[sx1], tx);
            let bottom = lerp(r1[sx], r1[sx1], tx);
            out[y * w + x] = lerp(top, bottom, ty);
        }
    }
    out
}

/// A soft shadow: the coverage of a rounded rectangle `w × h` blurred by
/// `blur` pixels, in a mask `margin` larger on every side; `margin` is
/// returned with it.
pub fn shadow(w: usize, h: usize, r: u32, shape: CornerShape, blur: usize) -> (Mask, usize) {
    let margin = blur * 2 + 2;
    let (mw, mh) = (
        (w + 2 * margin).min(MAX_SIDE),
        (h + 2 * margin).min(MAX_SIDE),
    );
    let shape_mask = rounded_rect(w, h, r, shape);
    let mut c = vec![0u32; mw * mh];
    for y in 0..shape_mask.h.min(mh.saturating_sub(margin)) {
        for x in 0..shape_mask.w.min(mw.saturating_sub(margin)) {
            c[(y + margin) * mw + x + margin] = shape_mask.cov[y * shape_mask.w + x] as u32;
        }
    }
    if blur > 0 {
        blur_channel(&mut c, mw, mh, blur, false);
    }
    let mut m = Mask::new(mw, mh);
    for (d, s) in m.cov.iter_mut().zip(c) {
        *d = s.min(255) as u8;
    }
    (m, margin)
}

/// The coverage of a convex or simple polygon, its points in 26.6 relative
/// to the mask's top left, through the text rasteriser (ADR-0161: the same
/// scan-line code draws glyphs and icons).
pub fn polygon(w: usize, h: usize, points: &[(i32, i32)]) -> Mask {
    let (w, h) = (w.min(MAX_SIDE), h.min(MAX_SIDE));
    let mut r = Raster::new(w, h);
    for i in 0..points.len() {
        r.line(points[i], points[(i + 1) % points.len()]);
    }
    Mask {
        w,
        h,
        cov: r.finish(),
    }
}

/// A line of `thickness` pixels from `a` to `b` (pixel coordinates in a
/// `w × h` mask), as a polygon. Integer: the normal is found with an
/// integer square root.
pub fn segment(w: usize, h: usize, a: (i32, i32), b: (i32, i32), thickness: u32) -> Mask {
    let (dx, dy) = (
        (b.0 - a.0) as i64 * ONE as i64,
        (b.1 - a.1) as i64 * ONE as i64,
    );
    let len = isqrt(dx * dx + dy * dy).max(1);
    let half = thickness as i64 * ONE as i64 / 2;
    let (nx, ny) = (-dy * half / len, dx * half / len);
    let p = |x: i32, y: i32, sx: i64, sy: i64| {
        (
            (x as i64 * ONE as i64 + sx) as i32,
            (y as i64 * ONE as i64 + sy) as i32,
        )
    };
    let points = [
        p(a.0, a.1, nx, ny),
        p(b.0, b.1, nx, ny),
        p(b.0, b.1, -nx, -ny),
        p(a.0, a.1, -nx, -ny),
    ];
    polygon(w, h, &points)
}

/// The integer square root of a non-negative number.
pub fn isqrt(v: i64) -> i64 {
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

/// Pixels to draw into, with a clip.
pub struct Canvas<'a> {
    pub pixels: &'a mut [u32],
    pub width: usize,
    pub height: usize,
    pub clip: Rect,
}

impl<'a> Canvas<'a> {
    /// A canvas over `pixels`, rows of `width`.
    pub fn new(pixels: &'a mut [u32], width: usize) -> Canvas<'a> {
        let height = pixels.len().checked_div(width).unwrap_or(0);
        Canvas {
            pixels,
            width,
            height,
            clip: Rect::new(0, 0, width as i32, height as i32),
        }
    }

    /// The clip within the pixels.
    fn bounds(&self) -> Rect {
        self.clip
            .intersect(&Rect::new(0, 0, self.width as i32, self.height as i32))
    }

    /// Sets the clip to `r` within the pixels and gives the old one back.
    pub fn set_clip(&mut self, r: Rect) -> Rect {
        let old = self.clip;
        self.clip = r.intersect(&Rect::new(0, 0, self.width as i32, self.height as i32));
        old
    }

    pub fn fill(&mut self, r: Rect, c: Color) {
        let r = r.intersect(&self.bounds());
        for y in r.y..r.bottom() {
            let row = y as usize * self.width;
            self.pixels[row + r.x as usize..row + r.right() as usize].fill(c.pixel());
        }
    }

    /// `c` over the rectangle at `alpha` of 255.
    pub fn blend(&mut self, r: Rect, c: Color, alpha: u8) {
        let r = r.intersect(&self.bounds());
        for y in r.y..r.bottom() {
            let row = y as usize * self.width;
            for p in &mut self.pixels[row + r.x as usize..row + r.right() as usize] {
                *p = Color::of_pixel(*p).blend(c, alpha).pixel();
            }
        }
    }

    /// `c` through `mask` placed at `(x, y)`, each pixel at its coverage
    /// times `alpha`.
    pub fn mask(&mut self, m: &Mask, x: i32, y: i32, c: Color, alpha: u8) {
        let area = Rect::new(x, y, m.w as i32, m.h as i32).intersect(&self.bounds());
        for py in area.y..area.bottom() {
            let row = py as usize * self.width;
            let my = (py - y) as usize;
            for px in area.x..area.right() {
                let cov = m.cov[my * m.w + (px - x) as usize] as u32;
                if cov == 0 {
                    continue;
                }
                let a = (cov * alpha as u32 + 127) / 255;
                let p = &mut self.pixels[row + px as usize];
                *p = Color::of_pixel(*p).blend(c, a as u8).pixel();
            }
        }
    }

    /// The pixels of `src` (rows of `src_width`) at `(x, y)`, where `m`
    /// (same size, if given) covers them: client pixels drawn unchanged
    /// where the coverage is full (INV-0164-1), blended at the edge.
    pub fn image(&mut self, src: &[u32], src_width: usize, x: i32, y: i32, m: Option<&Mask>) {
        if src_width == 0 {
            return;
        }
        let src_h = src.len() / src_width;
        let area = Rect::new(x, y, src_width as i32, src_h as i32).intersect(&self.bounds());
        for py in area.y..area.bottom() {
            let row = py as usize * self.width;
            let sy = (py - y) as usize;
            for px in area.x..area.right() {
                let sx = (px - x) as usize;
                let s = src[sy * src_width + sx] & 0x00ff_ffff;
                let cov = m.map_or(255, |m| m.at(sx, sy));
                let p = &mut self.pixels[row + px as usize];
                *p = match cov {
                    255 => s,
                    0 => *p,
                    a => Color::of_pixel(*p).blend(Color::of_pixel(s), a).pixel(),
                };
            }
        }
    }

    /// A vertical band over `r` of `c` going from `top` alpha at its top
    /// row to `bottom` at its last, each row at its own alpha.
    pub fn band(&mut self, r: Rect, c: Color, top: u8, bottom: u8) {
        let rows = r.h.max(1);
        for i in 0..r.h {
            let a = top as i32 + (bottom as i32 - top as i32) * i / rows;
            self.blend(Rect::new(r.x, r.y + i, r.w, 1), c, a.clamp(0, 255) as u8);
        }
    }
}

/// ADR-0161's trusted material over `r` of a canvas whose pixels already
/// hold what is under it: the halo outside, the glass (what is under,
/// blurred, tinted), the band of light from the top, the rim, the border.
/// `under` is the scene already drawn there (it is blurred in place).
#[derive(Debug, Clone, Copy)]
pub struct Material {
    pub radius: u32,
    pub shape: CornerShape,
    /// The blur of the glass, `blur.panel` at the scale.
    pub blur: usize,
    /// The halo's blur, `glow.blur` at the scale.
    pub halo_blur: usize,
    pub tint: Color,
    pub tint_alpha: u8,
    pub glow: Color,
    pub glow_alpha: u8,
    pub halo_alpha: u8,
    pub border: Color,
    /// 1 or 2: the rim and the border are this many pixels.
    pub scale: u32,
}

impl Material {
    pub fn draw(&self, c: &mut Canvas<'_>, r: Rect) {
        if r.is_empty() {
            return;
        }
        let (w, h) = (r.w as usize, r.h as usize);
        if self.halo_alpha > 0 && self.halo_blur > 0 {
            let (m, margin) = shadow(w, h, self.radius, self.shape, self.halo_blur);
            c.mask(
                &m,
                r.x - margin as i32,
                r.y - margin as i32,
                self.glow,
                self.halo_alpha,
            );
        }
        let shape = rounded_rect(w, h, self.radius, self.shape);
        // The glass: what is under, blurred, then the tint over it, both
        // within the shape.
        let bounds = c.bounds();
        let area = r.intersect(&bounds);
        if !area.is_empty() {
            let mut glass: Vec<u32> = Vec::with_capacity(area.w as usize * area.h as usize);
            for y in area.y..area.bottom() {
                let row = y as usize * c.width;
                glass.extend_from_slice(
                    &c.pixels[row + area.x as usize..row + area.right() as usize],
                );
            }
            let gw = area.w as usize;
            blur_region(&mut glass, gw, Rect::new(0, 0, area.w, area.h), self.blur);
            for p in &mut glass {
                *p = Color::of_pixel(*p)
                    .blend(self.tint, self.tint_alpha)
                    .pixel();
            }
            // Through the shape's coverage.
            let mut cut = Mask::new(gw, area.h as usize);
            for y in 0..area.h as usize {
                for x in 0..gw {
                    cut.cov[y * gw + x] =
                        shape.at((area.x - r.x) as usize + x, (area.y - r.y) as usize + y);
                }
            }
            c.image(&glass, gw, area.x, area.y, Some(&cut));
        }
        if self.glow_alpha > 0 {
            // A third of the height, at most 48 logical pixels.
            let band = (r.h / 3).min(48 * self.scale as i32);
            let old = c.set_clip(r.intersect(&c.clip));
            let mut lit = Mask::new(w, band.max(0) as usize);
            for y in 0..lit.h {
                let a = (self.glow_alpha as u32 / 2) * (lit.h - y) as u32 / lit.h.max(1) as u32;
                for x in 0..w {
                    lit.cov[y * w + x] = (shape.at(x, y) as u32 * a / 255) as u8;
                }
            }
            c.mask(&lit, r.x, r.y, self.glow, 255);
            c.clip = old;
            let rim_color = self.tint.blend(self.glow, self.glow_alpha);
            let t = self.scale;
            if w > 2 * t as usize && h > 2 * t as usize {
                let rim = rounded_outline(
                    w - 2 * t as usize,
                    h - 2 * t as usize,
                    self.radius.saturating_sub(t),
                    t,
                    self.shape,
                );
                c.mask(&rim, r.x + t as i32, r.y + t as i32, rim_color, 255);
            }
        }
        let edge = rounded_outline(w, h, self.radius, self.scale, self.shape);
        c.mask(&edge, r.x, r.y, self.border, 255);
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

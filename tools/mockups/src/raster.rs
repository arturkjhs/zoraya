//! Scan-line coverage rasterisation of closed quadratic outlines: signed
//! area accumulated per pixel, then a prefix sum per row (the font-rs
//! method, which `ab_glyph_rasterizer` and `fontdue` also use). Output is
//! 8-bit coverage, no hinting, no subpixel positioning.

use crate::ttf::{Font, Outline};

pub struct Raster {
    width: usize,
    height: usize,
    stride: usize,
    acc: Vec<f32>,
}

impl Raster {
    pub fn new(width: usize, height: usize) -> Raster {
        let stride = width + 2;
        Raster {
            width,
            height,
            stride,
            acc: vec![0.0; stride * height + stride],
        }
    }

    /// Accumulates the signed area of one line segment, in pixel
    /// coordinates (y down). Segments are clipped to the raster: x is
    /// clamped to `[0, width]`, which keeps the winding correct for what is
    /// visible, and rows outside `[0, height)` are skipped.
    pub fn line(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32)) {
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return;
        }
        if (y0 - y1).abs() <= 1e-6 {
            return;
        }
        let w = self.width as f32;
        let (dir, (ax, ay), (bx, by)) = if y0 < y1 {
            (1.0, (x0.clamp(0.0, w), y0), (x1.clamp(0.0, w), y1))
        } else {
            (-1.0, (x1.clamp(0.0, w), y1), (x0.clamp(0.0, w), y0))
        };
        let dxdy = (bx - ax) / (by - ay);
        let mut x = ax;
        let y_start = ay.max(0.0) as usize;
        if ay < 0.0 {
            x -= ay * dxdy;
        }
        let y_end = (by.ceil().max(0.0) as usize).min(self.height);
        for y in y_start..y_end {
            let row = y * self.stride;
            let dy = ((y + 1) as f32).min(by) - (y as f32).max(ay);
            let xnext = (x + dxdy * dy).clamp(0.0, w);
            let d = dy * dir;
            let (lo, hi) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let lo_floor = lo.floor();
            let lo_i = lo_floor as usize;
            let hi_ceil = hi.ceil();
            let hi_i = hi_ceil as usize;
            if hi_i <= lo_i + 1 {
                let xmf = 0.5 * (x + xnext) - lo_floor;
                self.acc[row + lo_i] += d - d * xmf;
                self.acc[row + lo_i + 1] += d * xmf;
            } else {
                let s = (hi - lo).recip();
                let lo_f = lo - lo_floor;
                let a0 = 0.5 * s * (1.0 - lo_f) * (1.0 - lo_f);
                let hi_f = hi - hi_ceil + 1.0;
                let am = 0.5 * s * hi_f * hi_f;
                self.acc[row + lo_i] += d * a0;
                if hi_i == lo_i + 2 {
                    self.acc[row + lo_i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - lo_f);
                    self.acc[row + lo_i + 1] += d * (a1 - a0);
                    for xi in lo_i + 2..hi_i - 1 {
                        self.acc[row + xi] += d * s;
                    }
                    let a2 = a1 + (hi_i - lo_i - 3) as f32 * s;
                    self.acc[row + hi_i - 1] += d * (1.0 - a2 - am);
                }
                self.acc[row + hi_i] += d * am;
            }
            x = xnext;
        }
    }

    /// A quadratic Bézier, flattened into segments.
    pub fn quad(&mut self, p0: (f32, f32), c: (f32, f32), p1: (f32, f32)) {
        let dd = ((p0.0 - 2.0 * c.0 + p1.0).abs() + (p0.1 - 2.0 * c.1 + p1.1).abs()).sqrt();
        let n = ((dd * 2.0).ceil() as usize).clamp(1, 32);
        let mut prev = p0;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let p = (
                mt * mt * p0.0 + 2.0 * mt * t * c.0 + t * t * p1.0,
                mt * mt * p0.1 + 2.0 * mt * t * c.1 + t * t * p1.1,
            );
            self.line(prev, p);
            prev = p;
        }
    }

    /// 8-bit coverage, row-major, `width × height`.
    pub fn finish(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.width * self.height];
        for y in 0..self.height {
            let mut sum = 0.0f32;
            for x in 0..self.width {
                sum += self.acc[y * self.stride + x];
                let c = sum.abs().min(1.0);
                out[y * self.width + x] = (c * 255.0 + 0.5) as u8;
            }
        }
        out
    }

    /// Draws an outline given in font units with the given scale (pixels
    /// per unit) and origin (the baseline's left end, in pixels, y down).
    pub fn outline(&mut self, outline: &Outline, scale: f32, origin: (f32, f32)) {
        let map = |x: f32, y: f32| (origin.0 + x * scale, origin.1 - y * scale);
        for contour in &outline.contours {
            let n = contour.len();
            if n < 2 {
                continue;
            }
            // find a starting on-curve point (or synthesise one)
            let start = contour.iter().position(|p| p.on_curve);
            let start_pt = match start {
                Some(i) => (contour[i].x, contour[i].y),
                None => (
                    (contour[0].x + contour[n - 1].x) / 2.0,
                    (contour[0].y + contour[n - 1].y) / 2.0,
                ),
            };
            let first = start.unwrap_or(0);
            let mut prev = start_pt;
            let mut ctrl: Option<(f32, f32)> = None;
            for k in 1..=n {
                let p = contour[(first + k) % n];
                let pt = (p.x, p.y);
                let is_last = k == n;
                let pt = if is_last && start.is_some() {
                    start_pt
                } else {
                    pt
                };
                let on = if is_last && start.is_some() {
                    true
                } else {
                    p.on_curve
                };
                if on {
                    match ctrl.take() {
                        Some(c) => self.quad(map(prev.0, prev.1), map(c.0, c.1), map(pt.0, pt.1)),
                        None => self.line(map(prev.0, prev.1), map(pt.0, pt.1)),
                    }
                    prev = pt;
                } else {
                    if let Some(c) = ctrl {
                        let mid = ((c.0 + pt.0) / 2.0, (c.1 + pt.1) / 2.0);
                        self.quad(map(prev.0, prev.1), map(c.0, c.1), map(mid.0, mid.1));
                        prev = mid;
                    }
                    ctrl = Some(pt);
                }
            }
            if let Some(c) = ctrl {
                self.quad(
                    map(prev.0, prev.1),
                    map(c.0, c.1),
                    map(start_pt.0, start_pt.1),
                );
            }
        }
    }
}

/// A rendered glyph: coverage with its placement relative to the pen.
#[derive(Clone)]
pub struct GlyphBitmap {
    pub width: usize,
    pub height: usize,
    /// Offset of the bitmap's top-left from the pen position (x right, y down).
    pub left: i32,
    pub top: i32,
    pub coverage: Vec<u8>,
}

/// Rasterises one glyph at `px` pixels per em, with the pen at a whole pixel.
pub fn render_glyph(font: &Font, glyph: u16, px: f32) -> GlyphBitmap {
    let outline = font.outline(glyph);
    let scale = px / font.units_per_em as f32;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for c in &outline.contours {
        for p in c {
            min_x = min_x.min(p.x * scale);
            max_x = max_x.max(p.x * scale);
            min_y = min_y.min(-p.y * scale);
            max_y = max_y.max(-p.y * scale);
        }
    }
    if min_x > max_x || min_y > max_y {
        return GlyphBitmap {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            coverage: Vec::new(),
        };
    }
    let left = min_x.floor() as i32 - 1;
    let top = min_y.floor() as i32 - 1;
    let width = (max_x.ceil() as i32 - left + 1).max(1) as usize;
    let height = (max_y.ceil() as i32 - top + 1).max(1) as usize;
    let mut r = Raster::new(width, height);
    r.outline(&outline, scale, (-(left as f32), -(top as f32)));
    GlyphBitmap {
        width,
        height,
        left,
        top,
        coverage: r.finish(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_covers_its_pixels_fully() {
        let mut r = Raster::new(8, 8);
        let p = [(2.0, 2.0), (6.0, 2.0), (6.0, 6.0), (2.0, 6.0)];
        for i in 0..4 {
            r.line(p[i], p[(i + 1) % 4]);
        }
        let c = r.finish();
        assert_eq!(c[3 * 8 + 3], 255);
        assert_eq!(c[0], 0);
        assert_eq!(c[7 * 8 + 7], 0);
        assert_eq!(c[2 * 8 + 1], 0);
    }

    #[test]
    fn half_pixel_edges_give_half_coverage() {
        let mut r = Raster::new(8, 8);
        let p = [(2.5, 2.0), (6.0, 2.0), (6.0, 6.0), (2.5, 6.0)];
        for i in 0..4 {
            r.line(p[i], p[(i + 1) % 4]);
        }
        let c = r.finish();
        assert!((c[3 * 8 + 2] as i32 - 128).abs() <= 1, "{}", c[3 * 8 + 2]);
    }

    #[test]
    fn segments_outside_the_raster_do_not_panic_or_leak() {
        let mut r = Raster::new(4, 4);
        r.line((-10.0, -10.0), (20.0, 20.0));
        r.line((20.0, 20.0), (-10.0, 30.0));
        r.line((f32::NAN, 0.0), (1.0, 1.0));
        let _ = r.finish();
    }
}

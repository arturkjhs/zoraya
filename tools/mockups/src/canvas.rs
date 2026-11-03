//! Drawing in logical pixels onto a physical pixel buffer: rectangles,
//! rounded rectangles, strokes, soft shadows and text. Every call takes
//! logical coordinates and multiplies by the scale once (ADR-0164); text
//! is rasterised at the physical size, never scaled (ADR-0162).

use std::collections::HashMap;

use crate::raster::{GlyphBitmap, Raster, render_glyph};
use crate::ttf::Font;

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub scale: f32,
    pub px: Vec<u32>,
    clip: (i32, i32, i32, i32),
    /// Continuous corners (a superellipse that runs smoothly into the
    /// straight edge) instead of circular arcs.
    pub smooth: bool,
}

/// Physical pixels of a region, as `Canvas::snapshot` returns them.
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

/// Which font of the set to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Ui,
    UiMedium,
    Mono,
    MonoBold,
}

pub struct FontSet {
    fonts: HashMap<Face, Font>,
    cache: HashMap<(Face, u16, u32), GlyphBitmap>,
}

fn blend_channel(dst: u32, src: u32, a: u32) -> u32 {
    (dst * (255 - a) + src * a + 127) / 255
}

pub fn blend(dst: u32, src: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    if a == 255 {
        return src & 0xFF_FFFF;
    }
    let r = blend_channel((dst >> 16) & 0xFF, (src >> 16) & 0xFF, a);
    let g = blend_channel((dst >> 8) & 0xFF, (src >> 8) & 0xFF, a);
    let b = blend_channel(dst & 0xFF, src & 0xFF, a);
    (r << 16) | (g << 8) | b
}

/// Mixes two colours, `t` towards `b`.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    blend(a, b, (t.clamp(0.0, 1.0) * 255.0) as u8)
}

// The geometry calls take a rectangle as five numbers on purpose: a mockup
// tool's call sites read better flat than through a struct per call.
#[allow(clippy::too_many_arguments)]
impl Canvas {
    pub fn new(logical_width: usize, logical_height: usize, scale: usize) -> Canvas {
        let (width, height) = (logical_width * scale, logical_height * scale);
        Canvas {
            width,
            height,
            scale: scale as f32,
            px: vec![0; width * height],
            clip: (0, 0, width as i32, height as i32),
            smooth: false,
        }
    }

    /// Restricts drawing to a logical rectangle; returns the previous clip.
    pub fn clip(&mut self, x: f32, y: f32, w: f32, h: f32) -> (i32, i32, i32, i32) {
        let old = self.clip;
        let s = self.scale;
        let nx0 = ((x * s).round() as i32).max(old.0);
        let ny0 = ((y * s).round() as i32).max(old.1);
        let nx1 = (((x + w) * s).round() as i32).min(old.2);
        let ny1 = (((y + h) * s).round() as i32).min(old.3);
        self.clip = (nx0, ny0, nx1.max(nx0), ny1.max(ny0));
        old
    }

    pub fn restore_clip(&mut self, clip: (i32, i32, i32, i32)) {
        self.clip = clip;
    }

    pub(crate) fn put(&mut self, x: i32, y: i32, color: u32, alpha: u8) {
        if alpha == 0 || x < self.clip.0 || y < self.clip.1 || x >= self.clip.2 || y >= self.clip.3
        {
            return;
        }
        let i = y as usize * self.width + x as usize;
        self.px[i] = blend(self.px[i], color, alpha);
    }

    pub fn fill(&mut self, color: u32) {
        self.px.fill(color & 0xFF_FFFF);
    }

    /// An axis-aligned rectangle in logical pixels (edges snapped to physical pixels).
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        let s = self.scale;
        let x0 = (x * s).round() as i32;
        let y0 = (y * s).round() as i32;
        let x1 = ((x + w) * s).round() as i32;
        let y1 = ((y + h) * s).round() as i32;
        for py in y0.max(self.clip.1)..y1.min(self.clip.3) {
            for px in x0.max(self.clip.0)..x1.min(self.clip.2) {
                let i = py as usize * self.width + px as usize;
                self.px[i] = color & 0xFF_FFFF;
            }
        }
    }

    /// A rectangle with alpha.
    pub fn rect_alpha(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32, alpha: u8) {
        let s = self.scale;
        let x0 = (x * s).round() as i32;
        let y0 = (y * s).round() as i32;
        let x1 = ((x + w) * s).round() as i32;
        let y1 = ((y + h) * s).round() as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                self.put(px, py, color, alpha);
            }
        }
    }

    fn rounded_path(&self, x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<(f32, f32)> {
        if self.smooth {
            return Canvas::smooth_path(x, y, w, h, r);
        }
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let mut pts = Vec::new();
        let n = (r as usize).clamp(3, 24);
        let corner = |cx: f32, cy: f32, a0: f32, pts: &mut Vec<(f32, f32)>| {
            for i in 0..=n {
                let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
                pts.push((cx + r * a.cos(), cy + r * a.sin()));
            }
        };
        use std::f32::consts::PI;
        corner(x + w - r, y + r, -PI / 2.0, &mut pts);
        corner(x + w - r, y + h - r, 0.0, &mut pts);
        corner(x + r, y + h - r, PI / 2.0, &mut pts);
        corner(x + r, y + r, PI, &mut pts);
        pts
    }

    /// A corner that is a superellipse (exponent 2.9) over an extent of
    /// 1.6 radii, so that the curve runs into the straight edge with no
    /// visible kink, as continuous ("smooth") corners do.
    fn smooth_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<(f32, f32)> {
        let e = (r * 1.6).min(w / 2.0).min(h / 2.0).max(0.0);
        let n = 2.9f32;
        let steps = ((e as usize) * 2).clamp(8, 48);
        let mut pts = Vec::new();
        let corner = |cx: f32, cy: f32, sx: f32, sy: f32, pts: &mut Vec<(f32, f32)>| {
            // from the edge parallel to x (t = 0) to the edge parallel to y
            for i in 0..=steps {
                let t = std::f32::consts::FRAC_PI_2 * i as f32 / steps as f32;
                let (c, s) = (t.cos(), t.sin());
                let px = c.abs().powf(2.0 / n) * e;
                let py = s.abs().powf(2.0 / n) * e;
                pts.push((cx + sx * px, cy + sy * py));
            }
        };
        // clockwise from the top-right corner, matching rounded_path's order
        corner(x + w - e, y + e, 1.0, -1.0, &mut pts); // top right: starts on the top edge
        let mut tr = pts.split_off(0);
        tr.reverse(); // make it run from the top edge down the right side
        pts.extend(tr);
        corner(x + w - e, y + h - e, 1.0, 1.0, &mut pts); // right side to bottom edge
        corner(x + e, y + h - e, -1.0, 1.0, &mut pts);
        let n0 = pts.len();
        let mut bl = pts.split_off(n0 - (steps + 1));
        bl.reverse();
        pts.extend(bl);
        corner(x + e, y + e, -1.0, -1.0, &mut pts);
        pts
    }

    /// Fills coverage from a closed polygon with a colour taken per pixel
    /// from `color_at(x, y)` in physical pixels, scaled by `alpha`.
    fn fill_polygon_fn<F: Fn(i32, i32) -> u32>(
        &mut self,
        pts: &[(f32, f32)],
        color_at: F,
        alpha: u8,
    ) {
        let s = self.scale;
        let min_x = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let max_x = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let max_y = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        let x0 = (min_x * s).floor() as i32 - 1;
        let y0 = (min_y * s).floor() as i32 - 1;
        let w = ((max_x * s).ceil() as i32 - x0 + 2).max(1) as usize;
        let h = ((max_y * s).ceil() as i32 - y0 + 2).max(1) as usize;
        if w > 8192 || h > 8192 {
            return;
        }
        let mut r = Raster::new(w, h);
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            r.line(
                (a.0 * s - x0 as f32, a.1 * s - y0 as f32),
                (b.0 * s - x0 as f32, b.1 * s - y0 as f32),
            );
        }
        let cov = r.finish();
        for yy in 0..h {
            for xx in 0..w {
                let c = cov[yy * w + xx] as u32;
                if c > 0 {
                    let (px, py) = (x0 + xx as i32, y0 + yy as i32);
                    let a = (c * alpha as u32 / 255) as u8;
                    self.put(px, py, color_at(px, py), a);
                }
            }
        }
    }

    /// Copies a whole image (same size as the canvas) in.
    pub fn fill_image(&mut self, img: &[u32]) {
        assert_eq!(img.len(), self.px.len());
        self.px.copy_from_slice(img);
    }

    /// A rounded rectangle filled from an image (same size as the canvas)
    /// tinted with `tint` at `tint_alpha`: the translucent-panel look over a
    /// pre-blurred wallpaper.
    pub fn rounded_rect_from(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        img: &[u32],
        tint: u32,
        tint_alpha: u8,
    ) {
        let pts = if r <= 0.5 {
            vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        } else {
            self.rounded_path(x, y, w, h, r)
        };
        let width = self.width as i32;
        let height = self.height as i32;
        let at = |px: i32, py: i32| -> u32 {
            if px < 0 || py < 0 || px >= width || py >= height {
                return tint;
            }
            blend(
                img[py as usize * width as usize + px as usize],
                tint,
                tint_alpha,
            )
        };
        self.fill_polygon_fn(&pts, at, 255);
    }

    /// The physical pixels of a logical rectangle, for a thumbnail.
    pub fn snapshot(&self, x: f32, y: f32, w: f32, h: f32) -> Image {
        let s = self.scale;
        let x0 = ((x * s).round() as i32).clamp(0, self.width as i32) as usize;
        let y0 = ((y * s).round() as i32).clamp(0, self.height as i32) as usize;
        let x1 = (((x + w) * s).round() as i32).clamp(0, self.width as i32) as usize;
        let y1 = (((y + h) * s).round() as i32).clamp(0, self.height as i32) as usize;
        let (iw, ih) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let mut px = Vec::with_capacity(iw * ih);
        for yy in y0..y1 {
            px.extend_from_slice(&self.px[yy * self.width + x0..yy * self.width + x1]);
        }
        Image { w: iw, h: ih, px }
    }

    /// Draws an image into a rounded logical rectangle, shrinking it by
    /// averaging the source pixels under each destination pixel (a box
    /// filter), which is how a compositor makes a true thumbnail of a
    /// window's pixels.
    pub fn image_scaled(&mut self, img: &Image, x: f32, y: f32, w: f32, h: f32, r: f32) {
        if img.w == 0 || img.h == 0 {
            return;
        }
        let s = self.scale;
        let pts = if r <= 0.5 {
            vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        } else {
            self.rounded_path(x, y, w, h, r)
        };
        let (dx, dy, dw, dh) = (x * s, y * s, w * s, h * s);
        let at = |px: i32, py: i32| -> u32 {
            let u0 = ((px as f32 - dx) / dw * img.w as f32).floor().max(0.0) as usize;
            let v0 = ((py as f32 - dy) / dh * img.h as f32).floor().max(0.0) as usize;
            let u1 = (((px + 1) as f32 - dx) / dw * img.w as f32).ceil() as usize;
            let v1 = (((py + 1) as f32 - dy) / dh * img.h as f32).ceil() as usize;
            let (u0, v0) = (u0.min(img.w - 1), v0.min(img.h - 1));
            let (u1, v1) = (u1.clamp(u0 + 1, img.w), v1.clamp(v0 + 1, img.h));
            let (mut rs, mut gs, mut bs, mut n) = (0u32, 0u32, 0u32, 0u32);
            for v in v0..v1 {
                for u in u0..u1 {
                    let p = img.px[v * img.w + u];
                    rs += (p >> 16) & 0xFF;
                    gs += (p >> 8) & 0xFF;
                    bs += p & 0xFF;
                    n += 1;
                }
            }
            let n = n.max(1);
            ((rs / n) << 16) | ((gs / n) << 8) | (bs / n)
        };
        self.fill_polygon_fn(&pts, at, 255);
    }

    /// A rounded rectangle filled with a blur of what the canvas already
    /// holds under it, tinted: the live form of the glass, which blurs
    /// windows as well as the wallpaper. `radius` is the blur in logical
    /// pixels. Returns the number of physical pixels blurred, for the cost.
    pub fn rounded_rect_blurred(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        radius: f32,
        tint: u32,
        tint_alpha: u8,
    ) -> usize {
        let s = self.scale;
        let pad = radius * 1.5 + 2.0;
        let region = self.snapshot(x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad);
        if region.w == 0 || region.h == 0 {
            return 0;
        }
        let blurred = blur_image(&region.px, region.w, region.h, (radius * s) as usize);
        let ox = (((x - pad) * s).round() as i32).max(0);
        let oy = (((y - pad) * s).round() as i32).max(0);
        let pts = if r <= 0.5 {
            vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        } else {
            self.rounded_path(x, y, w, h, r)
        };
        let (rw, rh) = (region.w as i32, region.h as i32);
        let at = |px: i32, py: i32| -> u32 {
            let (u, v) = (px - ox, py - oy);
            if u < 0 || v < 0 || u >= rw || v >= rh {
                return tint;
            }
            blend(
                blurred[v as usize * region.w + u as usize],
                tint,
                tint_alpha,
            )
        };
        self.fill_polygon_fn(&pts, at, 255);
        region.w * region.h
    }

    /// A rectangle whose alpha runs from `a0` at the top to `a1` at the
    /// bottom: the inner light of the trusted material.
    /// A rounded rectangle filled with a vertical gradient from `top` to
    /// `bottom`: the material of a tile.
    pub fn rounded_rect_gradient_v(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        top: u32,
        bottom: u32,
    ) {
        let pts = if r <= 0.5 {
            vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        } else {
            self.rounded_path(x, y, w, h, r)
        };
        let s = self.scale;
        let y0 = y * s;
        let rows = (h * s).max(1.0);
        let at = |_px: i32, py: i32| -> u32 {
            let t = ((py as f32 - y0) / rows).clamp(0.0, 1.0);
            mix(top, bottom, t)
        };
        self.fill_polygon_fn(&pts, at, 255);
    }

    pub fn rect_gradient_v(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32, a0: u8, a1: u8) {
        let s = self.scale;
        let y0 = (y * s).round() as i32;
        let y1 = ((y + h) * s).round() as i32;
        let rows = (y1 - y0).max(1) as f32;
        for py in y0..y1 {
            let t = (py - y0) as f32 / rows;
            let a = (a0 as f32 + (a1 as f32 - a0 as f32) * t).round() as u8;
            self.rect_alpha(x, py as f32 / s, w, 1.0 / s, color, a);
        }
    }

    /// Fills coverage from a closed polygon (logical coordinates) with a colour and alpha.
    fn fill_polygon(&mut self, pts: &[(f32, f32)], color: u32, alpha: u8) {
        let s = self.scale;
        let min_x = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let max_x = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let max_y = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        let x0 = (min_x * s).floor() as i32 - 1;
        let y0 = (min_y * s).floor() as i32 - 1;
        let w = ((max_x * s).ceil() as i32 - x0 + 2).max(1) as usize;
        let h = ((max_y * s).ceil() as i32 - y0 + 2).max(1) as usize;
        if w > 8192 || h > 8192 {
            return;
        }
        let mut r = Raster::new(w, h);
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            r.line(
                (a.0 * s - x0 as f32, a.1 * s - y0 as f32),
                (b.0 * s - x0 as f32, b.1 * s - y0 as f32),
            );
        }
        let cov = r.finish();
        for yy in 0..h {
            for xx in 0..w {
                let c = cov[yy * w + xx] as u32;
                if c > 0 {
                    let a = (c * alpha as u32 / 255) as u8;
                    self.put(x0 + xx as i32, y0 + yy as i32, color, a);
                }
            }
        }
    }

    pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
        if r <= 0.5 {
            return self.rect(x, y, w, h, color);
        }
        let pts = self.rounded_path(x, y, w, h, r);
        self.fill_polygon(&pts, color, 255);
    }

    pub fn rounded_rect_alpha(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        color: u32,
        alpha: u8,
    ) {
        if r <= 0.5 {
            return self.rect_alpha(x, y, w, h, color, alpha);
        }
        let pts = self.rounded_path(x, y, w, h, r);
        self.fill_polygon(&pts, color, alpha);
    }

    /// A stroke of `width` logical pixels just inside the rounded rectangle.
    pub fn stroke_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        width: f32,
        color: u32,
    ) {
        if r <= 0.5 {
            self.rect(x, y, w, width, color);
            self.rect(x, y + h - width, w, width, color);
            self.rect(x, y, width, h, color);
            self.rect(x + w - width, y, width, h, color);
            return;
        }
        let outer = self.rounded_path(x, y, w, h, r);
        let mut inner = self.rounded_path(
            x + width,
            y + width,
            w - 2.0 * width,
            h - 2.0 * width,
            (r - width).max(0.5),
        );
        inner.reverse();
        let s = self.scale;
        let x0 = (x * s).floor() as i32 - 1;
        let y0 = (y * s).floor() as i32 - 1;
        let pw = (((x + w) * s).ceil() as i32 - x0 + 2).max(1) as usize;
        let ph = (((y + h) * s).ceil() as i32 - y0 + 2).max(1) as usize;
        let mut rr = Raster::new(pw, ph);
        for pts in [&outer, &inner] {
            for i in 0..pts.len() {
                let a = pts[i];
                let b = pts[(i + 1) % pts.len()];
                rr.line(
                    (a.0 * s - x0 as f32, a.1 * s - y0 as f32),
                    (b.0 * s - x0 as f32, b.1 * s - y0 as f32),
                );
            }
        }
        let cov = rr.finish();
        for yy in 0..ph {
            for xx in 0..pw {
                let c = cov[yy * pw + xx];
                if c > 0 {
                    self.put(x0 + xx as i32, y0 + yy as i32, color, c);
                }
            }
        }
    }

    /// A soft shadow: the rounded rectangle's coverage, box-blurred three
    /// times (an integer kernel, so deterministic), blended with alpha.
    pub fn shadow(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        blur: f32,
        offset_y: f32,
        color: u32,
        alpha: u8,
    ) {
        if blur <= 0.0 || alpha == 0 {
            return;
        }
        let s = self.scale;
        let b = (blur * s).round() as i32;
        let margin = b * 2 + 2;
        let x0 = (x * s).floor() as i32 - margin;
        let y0 = ((y + offset_y) * s).floor() as i32 - margin;
        let pw = (((x + w) * s).ceil() as i32 - x0 + margin + 1).max(1) as usize;
        let ph = (((y + offset_y + h) * s).ceil() as i32 - y0 + margin + 1).max(1) as usize;
        if pw > 8192 || ph > 8192 {
            return;
        }
        let pts = self.rounded_path(x, y + offset_y, w, h, r);
        let mut rr = Raster::new(pw, ph);
        for i in 0..pts.len() {
            let a = pts[i];
            let c = pts[(i + 1) % pts.len()];
            rr.line(
                (a.0 * s - x0 as f32, a.1 * s - y0 as f32),
                (c.0 * s - x0 as f32, c.1 * s - y0 as f32),
            );
        }
        let mut cov: Vec<u32> = rr.finish().into_iter().map(|c| c as u32).collect();
        let radius = (b / 3).max(1) as usize;
        for _ in 0..3 {
            cov = box_blur(&cov, pw, ph, radius);
        }
        for yy in 0..ph {
            for xx in 0..pw {
                let c = cov[yy * pw + xx];
                if c > 0 {
                    let a = (c * alpha as u32 / 255).min(255) as u8;
                    self.put(x0 + xx as i32, y0 + yy as i32, color, a);
                }
            }
        }
    }

    pub fn line_h(&mut self, x: f32, y: f32, w: f32, thickness: f32, color: u32) {
        self.rect(x, y, w, thickness, color);
    }

    /// A filled circle of logical radius.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: u32) {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..n)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / n as f32;
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        self.fill_polygon(&pts, color, 255);
    }

    /// A line segment of logical thickness between two logical points.
    pub fn segment(&mut self, a: (f32, f32), b: (f32, f32), thickness: f32, color: u32) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (nx, ny) = (-dy / len * thickness / 2.0, dx / len * thickness / 2.0);
        let pts = [
            (a.0 + nx, a.1 + ny),
            (b.0 + nx, b.1 + ny),
            (b.0 - nx, b.1 - ny),
            (a.0 - nx, a.1 - ny),
        ];
        self.fill_polygon(&pts, color, 255);
    }

    fn glyph(&mut self, g: &GlyphBitmap, pen_x: i32, pen_y: i32, color: u32) {
        for yy in 0..g.height {
            for xx in 0..g.width {
                let c = g.coverage[yy * g.width + xx];
                if c > 0 {
                    self.put(
                        pen_x + g.left + xx as i32,
                        pen_y + g.top + yy as i32,
                        color,
                        c,
                    );
                }
            }
        }
    }
}

/// A separable box blur of an RGB image, three passes per channel, so that
/// it approximates a Gaussian of about `radius` pixels; the edges repeat
/// their outermost pixel (unlike the shadow's blur, which fades to
/// nothing); deterministic integer arithmetic.
pub fn blur_image(img: &[u32], w: usize, h: usize, radius: usize) -> Vec<u32> {
    let r = (radius / 3).max(1);
    let mut chans: Vec<Vec<u32>> = (0..3)
        .map(|c| img.iter().map(|p| (p >> (16 - 8 * c)) & 0xFF).collect())
        .collect();
    for ch in chans.iter_mut() {
        for _ in 0..3 {
            *ch = box_blur_clamped(ch, w, h, r);
        }
    }
    (0..w * h)
        .map(|i| (chans[0][i] << 16) | (chans[1][i] << 8) | chans[2][i])
        .collect()
}

/// One box pass over a channel with edge pixels repeated outside the image.
fn box_blur_clamped(src: &[u32], w: usize, h: usize, r: usize) -> Vec<u32> {
    let win = (2 * r + 1) as u32;
    let mut tmp = vec![0u32; w * h];
    let mut out = vec![0u32; w * h];
    let at = |v: &[u32], i: isize, n: usize| v[i.clamp(0, n as isize - 1) as usize];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut sum: u32 = (-(r as isize)..=r as isize).map(|i| at(row, i, w)).sum();
        for x in 0..w {
            tmp[y * w + x] = (sum + win / 2) / win;
            sum += at(row, x as isize + r as isize + 1, w);
            sum -= at(row, x as isize - r as isize, w);
        }
    }
    for x in 0..w {
        let col: Vec<u32> = (0..h).map(|y| tmp[y * w + x]).collect();
        let mut sum: u32 = (-(r as isize)..=r as isize).map(|i| at(&col, i, h)).sum();
        for y in 0..h {
            out[y * w + x] = (sum + win / 2) / win;
            sum += at(&col, y as isize + r as isize + 1, h);
            sum -= at(&col, y as isize - r as isize, h);
        }
    }
    out
}

fn box_blur(src: &[u32], w: usize, h: usize, r: usize) -> Vec<u32> {
    let mut tmp = vec![0u32; w * h];
    let mut out = vec![0u32; w * h];
    let win = (2 * r + 1) as u32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut sum: u32 = row[..w.min(r)].iter().sum();
        for x in 0..w {
            if x + r < w {
                sum += row[x + r];
            }
            tmp[y * w + x] = sum / win;
            if x >= r {
                sum -= row[x - r];
            }
        }
    }
    for x in 0..w {
        let mut sum: u32 = 0;
        for y in 0..h.min(r) {
            sum += tmp[y * w + x];
        }
        for y in 0..h {
            if y + r < h {
                sum += tmp[(y + r) * w + x];
            }
            out[y * w + x] = sum / win;
            if y >= r {
                sum -= tmp[(y - r) * w + x];
            }
        }
    }
    out
}

// Same reason as for `Canvas`: text calls take their position and size flat.
#[allow(clippy::too_many_arguments)]
impl FontSet {
    pub fn new() -> FontSet {
        FontSet {
            fonts: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    pub fn insert(&mut self, face: Face, font: Font) {
        self.fonts.insert(face, font);
    }

    pub fn has_face(&self, face: Face) -> bool {
        self.fonts.contains_key(&face)
    }

    /// The fallback order for a face: itself, then the other family.
    fn chain(face: Face) -> [Face; 4] {
        match face {
            Face::Ui => [Face::Ui, Face::UiMedium, Face::Mono, Face::MonoBold],
            Face::UiMedium => [Face::UiMedium, Face::Ui, Face::MonoBold, Face::Mono],
            Face::Mono => [Face::Mono, Face::MonoBold, Face::Ui, Face::UiMedium],
            Face::MonoBold => [Face::MonoBold, Face::Mono, Face::UiMedium, Face::Ui],
        }
    }

    fn lookup(&self, face: Face, c: char) -> Option<(Face, u16)> {
        for f in FontSet::chain(face) {
            if let Some(font) = self.fonts.get(&f)
                && let Some(g) = font.glyph_index(c)
            {
                return Some((f, g));
            }
        }
        None
    }

    fn bitmap(&mut self, face: Face, glyph: u16, px: f32) -> &GlyphBitmap {
        let key = (face, glyph, px.to_bits());
        if !self.cache.contains_key(&key) {
            let bm = render_glyph(&self.fonts[&face], glyph, px);
            self.cache.insert(key, bm);
        }
        &self.cache[&key]
    }

    /// Line height in logical pixels for a face at a logical size. Without
    /// the face (a test without fonts) the metrics are nominal, as the box
    /// glyphs `draw` falls back to are.
    pub fn line_height(&self, face: Face, size: f32) -> f32 {
        match self.fonts.get(&face) {
            Some(f) => {
                (f.ascent as f32 - f.descent as f32 + f.line_gap as f32) * size
                    / f.units_per_em as f32
            }
            None => size * 1.2,
        }
    }

    pub fn ascent(&self, face: Face, size: f32) -> f32 {
        match self.fonts.get(&face) {
            Some(f) => f.ascent as f32 * size / f.units_per_em as f32,
            None => size * 0.75,
        }
    }

    /// The advance of one monospace cell at a logical size.
    pub fn cell_width(&self, face: Face, size: f32) -> f32 {
        match self.fonts.get(&face) {
            Some(f) => {
                let g = f.glyph_index('M').unwrap_or(0);
                f.advance(g) as f32 * size / f.units_per_em as f32
            }
            None => size * 0.6,
        }
    }

    /// Width of a string in logical pixels.
    pub fn width(&self, face: Face, size: f32, text: &str) -> f32 {
        let mut w = 0.0;
        let mut prev: Option<(Face, u16)> = None;
        for c in text.chars() {
            let Some((f, g)) = self.lookup(face, c) else {
                w += size * 0.6;
                prev = None;
                continue;
            };
            let font = &self.fonts[&f];
            let scale = size / font.units_per_em as f32;
            if let Some((pf, pg)) = prev
                && pf == f
            {
                w += font.kerning(pg, g) as f32 * scale;
            }
            w += font.advance(g) as f32 * scale;
            prev = Some((f, g));
        }
        w
    }

    /// Draws text with its baseline at logical `(x, y)`; returns the width.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        face: Face,
        size: f32,
        x: f32,
        y: f32,
        text: &str,
        color: u32,
    ) -> f32 {
        let s = canvas.scale;
        let px = size * s;
        let mut pen = x * s;
        let base_y = (y * s).round() as i32;
        let mut prev: Option<(Face, u16)> = None;
        for c in text.chars() {
            let Some((f, g)) = self.lookup(face, c) else {
                // the box glyph for a missing character
                let bw = px * 0.5;
                let bx = pen.round() / s;
                canvas.stroke_rounded_rect(
                    bx + 1.0 / s,
                    y - size * 0.7,
                    bw / s - 2.0 / s,
                    size * 0.75,
                    0.0,
                    1.0 / s,
                    color,
                );
                pen += px * 0.6;
                prev = None;
                continue;
            };
            let font = &self.fonts[&f];
            let scale = px / font.units_per_em as f32;
            if let Some((pf, pg)) = prev
                && pf == f
            {
                pen += font.kerning(pg, g) as f32 * scale;
            }
            let advance = font.advance(g) as f32 * scale;
            let pen_x = pen.round() as i32;
            let bm = self.bitmap(f, g, px).clone();
            canvas.glyph(&bm, pen_x, base_y, color);
            pen += advance;
            prev = Some((f, g));
        }
        (pen - x * s) / s
    }

    /// Draws monospace text one cell at a time (each glyph centred in its cell).
    pub fn draw_cells(
        &mut self,
        canvas: &mut Canvas,
        face: Face,
        size: f32,
        x: f32,
        y: f32,
        cell: f32,
        text: &str,
        color: u32,
    ) {
        let s = canvas.scale;
        let px = size * s;
        let base_y = (y * s).round() as i32;
        for (i, c) in text.chars().enumerate() {
            let cx = x + i as f32 * cell;
            let Some((f, g)) = self.lookup(face, c) else {
                canvas.stroke_rounded_rect(
                    cx + 1.0,
                    y - size * 0.7,
                    cell - 2.0,
                    size * 0.75,
                    0.0,
                    1.0 / s,
                    color,
                );
                continue;
            };
            let font = &self.fonts[&f];
            let adv = font.advance(g) as f32 * px / font.units_per_em as f32;
            let pen_x = ((cx * s) + (cell * s - adv) / 2.0).round() as i32;
            let bm = self.bitmap(f, g, px).clone();
            canvas.glyph(&bm, pen_x, base_y, color);
        }
    }
}

impl Default for FontSet {
    fn default() -> Self {
        FontSet::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_corners_stay_inside_and_touch_the_edges() {
        let pts = Canvas::smooth_path(10.0, 20.0, 100.0, 60.0, 14.0);
        assert!(pts.len() > 16);
        for (x, y) in &pts {
            assert!(
                (10.0..=110.0).contains(x) && (20.0..=80.0).contains(y),
                "({x}, {y})"
            );
        }
        // the path reaches every edge: a straight run exists on each side
        assert!(pts.iter().any(|p| (p.1 - 20.0).abs() < 0.01));
        assert!(pts.iter().any(|p| (p.1 - 80.0).abs() < 0.01));
        assert!(pts.iter().any(|p| (p.0 - 10.0).abs() < 0.01));
        assert!(pts.iter().any(|p| (p.0 - 110.0).abs() < 0.01));
        // consecutive points are close: no jump across the shape
        for w in pts.windows(2) {
            let d = ((w[0].0 - w[1].0).powi(2) + (w[0].1 - w[1].1).powi(2)).sqrt();
            assert!(d < 100.0 - 2.0 * 14.0 * 1.6 + 1.0, "jump of {d}");
        }
    }

    #[test]
    fn smooth_rect_covers_more_than_circular_at_the_corner() {
        let mut a = Canvas::new(40, 40, 1);
        a.fill(0x000000);
        a.rounded_rect(0.0, 0.0, 40.0, 40.0, 10.0, 0xFFFFFF);
        let mut b = Canvas::new(40, 40, 1);
        b.smooth = true;
        b.fill(0x000000);
        b.rounded_rect(0.0, 0.0, 40.0, 40.0, 10.0, 0xFFFFFF);
        let lit = |c: &Canvas| c.px.iter().filter(|p| (**p & 0xFF) > 128).count();
        let mut c = Canvas::new(40, 40, 1);
        c.fill(0x000000);
        c.rounded_rect(0.0, 0.0, 40.0, 40.0, 16.0, 0xFFFFFF);
        // the smooth corner spans 1.6 radii and cuts more than a circle of
        // the radius, less than a circle of its span; the centre is untouched
        assert!(lit(&b) < lit(&a), "{} vs {}", lit(&b), lit(&a));
        assert!(lit(&b) > lit(&c), "{} vs {}", lit(&b), lit(&c));
        assert_eq!(a.px[20 * 40 + 20], 0xFFFFFF);
        assert_eq!(b.px[20 * 40 + 20], 0xFFFFFF);
        assert_eq!(a.px[0], 0x000000);
        assert_eq!(b.px[0], 0x000000);
    }

    #[test]
    fn blur_keeps_a_flat_image_and_is_deterministic() {
        let flat = vec![0x336699u32; 64 * 48];
        assert_eq!(blur_image(&flat, 64, 48, 12), flat);
        let img: Vec<u32> = (0..64 * 48)
            .map(|i| (i as u32).wrapping_mul(2654435761) & 0xFFFFFF)
            .collect();
        let a = blur_image(&img, 64, 48, 12);
        let b = blur_image(&img, 64, 48, 12);
        assert_eq!(a, b);
        assert_ne!(a, img);
    }

    #[test]
    fn rounded_rect_from_samples_the_image() {
        let mut c = Canvas::new(20, 20, 1);
        c.fill(0x000000);
        let img: Vec<u32> = (0..400)
            .map(|i| if i % 2 == 0 { 0xFF0000 } else { 0x0000FF })
            .collect();
        c.rounded_rect_from(0.0, 0.0, 20.0, 20.0, 0.0, &img, 0xFFFFFF, 0);
        assert_eq!(c.px, img);
        c.rounded_rect_from(0.0, 0.0, 20.0, 20.0, 0.0, &img, 0xFFFFFF, 255);
        assert!(c.px.iter().all(|p| *p == 0xFFFFFF));
    }

    /// The cost estimate for ADR-0161: `cargo test -p zoraya-mockups --release -- --ignored --nocapture blur_cost`.
    #[test]
    #[ignore]
    fn blur_cost() {
        for (w, h, r) in [(1024usize, 768usize, 24usize), (2048, 1536, 48)] {
            let img: Vec<u32> = (0..w * h)
                .map(|i| (i as u32).wrapping_mul(2654435761) & 0xFFFFFF)
                .collect();
            let t = std::time::Instant::now();
            let n = 5;
            for _ in 0..n {
                std::hint::black_box(blur_image(std::hint::black_box(&img), w, h, r));
            }
            let per = t.elapsed() / n;
            println!("blur {w}x{h} radius {r}: {per:?} per image");
        }
    }
}

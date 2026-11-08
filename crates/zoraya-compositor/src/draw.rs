//! Composition (ADR-0160, ADR-0161): the wallpaper, the top bar on its
//! glass, every visible window with the frame the compositor draws for it,
//! popups, the switcher in the trusted material, the pointer. Everything
//! but a window's body comes from the tokens and the compositor's own
//! records, never from a client's pixels (INV-0160-1).

use alloc::vec::Vec;

use zoraya_paint::{self as paint, Canvas, Mask, Material};
use zoraya_text::{Rect as TextRect, Target};
use zoraya_tokens::{Color, Palette, glow, shadow, shape, space, text};

use crate::window::{BUTTON_HALF, CLOSE_X, Chrome, MINIMISE_X};
use crate::{Compositor, Platform, Rect, Window};

/// The pointer: 1 is the fill, 2 the outline, 0 clear.
pub const POINTER: [[u8; 8]; 11] = [
    [2, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 0, 0, 0, 0, 0, 0],
    [2, 1, 2, 0, 0, 0, 0, 0],
    [2, 1, 1, 2, 0, 0, 0, 0],
    [2, 1, 1, 1, 2, 0, 0, 0],
    [2, 1, 1, 1, 1, 2, 0, 0],
    [2, 1, 1, 1, 1, 1, 2, 0],
    [2, 1, 1, 1, 1, 1, 1, 2],
    [2, 1, 1, 2, 2, 2, 2, 2],
    [2, 1, 2, 0, 0, 0, 0, 0],
    [2, 2, 0, 0, 0, 0, 0, 0],
];

fn prect(r: Rect) -> paint::Rect {
    let c = |v: i64| v.clamp(i32::MIN as i64 / 2, i32::MAX as i64 / 2) as i32;
    paint::Rect::new(c(r.x), c(r.y), c(r.width), c(r.height))
}

/// The switcher's panel, logical pixels: as wide as the mockup's, a row
/// per window under a group line per room.
pub const SWITCHER_WIDTH: u32 = 640;
pub const SWITCHER_ROW: u32 = 64;
pub const SWITCHER_GROUP: u32 = 32;
pub const THUMB: (u32, u32) = (80, 52);

impl<P: Platform> Compositor<P> {
    fn palette(&self) -> &'static Palette {
        self.appearance.palette()
    }

    /// The wallpaper and its blurred copy, once per display size or
    /// appearance (ADR-0161: never per frame).
    pub(crate) fn make_wallpaper(&mut self) {
        let (w, h) = (self.width as usize, self.height as usize);
        self.wallpaper = paint::wallpaper::deep(w, h, self.palette());
        self.blurred = self.wallpaper.clone();
        let radius = glow::PANEL_BLUR.physical(self.scale) as usize;
        paint::blur_region(
            &mut self.blurred,
            w,
            paint::Rect::new(0, 0, w as i32, h as i32),
            radius,
        );
        for w in &mut self.windows {
            w.chrome = None;
        }
    }

    /// The pixels of `r` of the screen, from the wallpaper up.
    pub(crate) fn compose(&mut self, r: Rect) {
        let width = self.width as usize;
        let height = self.height as usize;
        if self.wallpaper.len() != width * height {
            return;
        }
        let p = self.palette();
        let scale = self.scale;
        let f = self.frame();
        // Make the masks of windows that need them first.
        for w in &mut self.windows {
            if w.framed() && w.chrome.as_ref().is_none_or(|c| c.size != w.body) {
                w.chrome = Some(chrome(w, &f, scale.factor()));
            }
        }
        let Compositor {
            platform,
            windows,
            focus,
            pointer,
            switcher,
            wallpaper,
            blurred,
            ui,
            title_text,
            layout,
            ..
        } = self;
        let pixels = platform.pixels();
        if pixels.len() < width * height {
            return;
        }
        let mut c = Canvas::new(&mut pixels[..width * height], width);
        c.set_clip(prect(r));
        c.image(wallpaper, width, 0, 0, None);
        // The bar: the blurred wallpaper under it, tinted, with its light.
        let bar = space::BAR_HEIGHT.physical(scale) as i32;
        let bar_rect = paint::Rect::new(0, 0, width as i32, bar);
        if !bar_rect.intersect(&c.clip).is_empty() {
            let rows = (bar as usize).min(height);
            c.image(&blurred[..rows * width], width, 0, 0, None);
            c.blend(bar_rect, p.bg_overlay, glow::PANEL_ALPHA);
            c.band(
                paint::Rect::new(0, 0, width as i32, bar),
                p.glow,
                glow::ALPHA / 2,
                0,
            );
            let s = scale.factor() as i32;
            c.fill(paint::Rect::new(0, bar - s, width as i32, s), p.border);
            let px = text::BODY.physical(scale);
            let baseline = bar / 2 + (px as i32 * 7 / 20);
            let pad = 14 * s;
            draw_text(&mut c, title_text, "Zoraya", px, pad, baseline, p.fg_text);
            let lw = title_text.measure(layout, px);
            draw_text(
                &mut c,
                title_text,
                layout,
                px,
                width as i32 - pad - lw,
                baseline,
                p.fg_text,
            );
        }
        for w in windows.iter().filter(|w| !w.hidden) {
            let focused = *focus
                == Some(crate::Key {
                    client: w.client,
                    id: w.id,
                });
            draw_window(&mut c, w, &f, scale.factor(), focused, p, title_text);
        }
        if let Some(s) = switcher.as_ref() {
            draw_switcher(&mut c, s, windows, scale.factor(), p, ui, title_text);
        }
        if let Some((px, py)) = *pointer {
            let s = scale.factor() as i32;
            for (dy, line) in POINTER.iter().enumerate() {
                for (dx, dot) in line.iter().enumerate() {
                    if *dot == 0 {
                        continue;
                    }
                    let color = if *dot == 1 { p.bg_control } else { p.fg_text };
                    let x = px as i32 + dx as i32 * s;
                    let y = py as i32 + dy as i32 * s;
                    c.fill(paint::Rect::new(x, y, s, s), color);
                }
            }
        }
    }
}

/// Draws `text` with the pen at `(x, baseline)` within the canvas's clip.
fn draw_text(
    c: &mut Canvas<'_>,
    t: &mut zoraya_text::Text<'static>,
    s: &str,
    px: u32,
    x: i32,
    baseline: i32,
    color: Color,
) -> i32 {
    let clip = c.clip;
    let mut target = Target {
        pixels: c.pixels,
        stride: c.width,
        clip: TextRect::new(clip.x, clip.y, clip.w.max(0) as u32, clip.h.max(0) as u32),
    };
    t.draw(&mut target, s, px, x, baseline, color)
}

/// The masks of a framed window at its body size.
fn chrome(w: &Window, f: &crate::Frame, s: u32) -> Chrome {
    let o = w.outer(f);
    let (ow, oh) = (o.width.max(0) as usize, o.height.max(0) as usize);
    let outer = paint::rounded_rect(ow, oh, f.radius, shape::CORNER);
    let outline = paint::rounded_outline(
        ow,
        oh,
        f.radius,
        space::FRAME_BORDER.physical(zoraya_tokens::Scale::ONE) * s,
        shape::CORNER,
    );
    let layers = shadow::WINDOW_LAYERS.max(1);
    let blur = shadow::WINDOW_BLUR.physical(zoraya_tokens::Scale::ONE) * s;
    let shadows = (1..=layers)
        .map(|i| {
            paint::shadow(
                ow,
                oh,
                f.radius,
                shape::CORNER,
                (blur * i / layers) as usize,
            )
        })
        .collect();
    Chrome {
        size: w.body,
        outer,
        outline,
        shadows,
    }
}

/// One window: shadow, frame, body, border, title and buttons.
fn draw_window(
    c: &mut Canvas<'_>,
    w: &Window,
    f: &crate::Frame,
    s: u32,
    focused: bool,
    p: &Palette,
    title: &mut zoraya_text::Text<'static>,
) {
    let body = prect(w.body_rect());
    if !w.framed() {
        // A popup: a soft shadow and its pixels.
        let (m, margin) = paint::shadow(
            body.w.max(0) as usize,
            body.h.max(0) as usize,
            0,
            shape::CORNER,
            (shadow::POPUP_BLUR.physical(zoraya_tokens::Scale::ONE) * s) as usize,
        );
        let off = (shadow::POPUP_OFFSET.physical(zoraya_tokens::Scale::ONE) * s) as i32;
        c.mask(
            &m,
            body.x - margin as i32,
            body.y + off - margin as i32,
            p.shadow,
            shadow::POPUP_ALPHA,
        );
        c.image(&w.copy, w.buffer.width as usize, body.x, body.y, None);
        return;
    }
    let Some(ch) = w.chrome.as_ref() else { return };
    let o = prect(w.outer(f));
    if o.intersect(&c.clip).is_empty() && prect(w.reach(f)).intersect(&c.clip).is_empty() {
        return;
    }
    // The shadow, in its layers, at a growing fraction of the offset.
    let alpha = if focused {
        shadow::WINDOW_ALPHA
    } else {
        shadow::WINDOW_UNFOCUSED_ALPHA
    };
    let layers = ch.shadows.len().max(1) as u32;
    let offset = (shadow::WINDOW_OFFSET.physical(zoraya_tokens::Scale::ONE) * s) as i32;
    for (i, (m, margin)) in ch.shadows.iter().enumerate() {
        let dy = offset * (i as i32 + 1) / layers as i32;
        c.mask(
            m,
            o.x - *margin as i32,
            o.y + dy - *margin as i32,
            p.shadow,
            (alpha as u32 / layers) as u8,
        );
    }
    // The ground: the title bar's colour, then the body's paper.
    let bar_color = if focused {
        p.bg_frame_focused
    } else {
        p.bg_frame
    };
    c.mask(&ch.outer, o.x, o.y, bar_color, 255);
    // The body: the client's pixels where its buffer reaches, the paper
    // where a resize has made the body larger than the buffer; inside the
    // window's shape (its lower corners are round).
    let mut body_mask = Mask::new(body.w.max(0) as usize, body.h.max(0) as usize);
    for y in 0..body_mask.h {
        for x in 0..body_mask.w {
            body_mask.cov[y * body_mask.w + x] = ch
                .outer
                .at(x + (body.x - o.x) as usize, y + (body.y - o.y) as usize);
        }
    }
    c.mask(&body_mask, body.x, body.y, p.bg_window, 255);
    let bw = (w.buffer.width as usize).min(body.w.max(0) as usize);
    let bh = (w.buffer.height as usize).min(body.h.max(0) as usize);
    let old = c.set_clip(paint::Rect::new(body.x, body.y, bw as i32, bh as i32).intersect(&c.clip));
    let mut cut = Mask::new(w.buffer.width as usize, w.buffer.height as usize);
    for y in 0..bh {
        for x in 0..bw {
            cut.cov[y * cut.w + x] = body_mask.at(x, y);
        }
    }
    c.image(&w.copy, w.buffer.width as usize, body.x, body.y, Some(&cut));
    c.clip = old;
    c.mask(&ch.outline, o.x, o.y, p.border, 255);
    // The buttons: a cross and a dash, coloured on the focused window and
    // grey elsewhere (`frame.buttons = glyphs`); a close the client has not
    // obeyed for five seconds is filled, offering to close anyway.
    let si = s as i32;
    let cy = o.y + f.title as i32 / 2;
    let (close, minimise) = if focused {
        (p.button_close, p.button_minimise)
    } else {
        (p.fg_text_disabled, p.fg_text_disabled)
    };
    let g = (9 * si) / 2;
    let t = (16 * s).div_ceil(10).max(1);
    let cx = o.x + CLOSE_X as i32 * si;
    if w.close_asked.is_some() {
        let half = BUTTON_HALF as i32 * si;
        let dot = paint::rounded_rect(
            (2 * half) as usize,
            (2 * half) as usize,
            half as u32,
            zoraya_tokens::CornerShape::Circle,
        );
        c.mask(&dot, cx - half, cy - half, p.button_close, 60);
    }
    let size = (2 * g + 2 * si) as usize;
    let base = (cx - g - si, cy - g - si);
    for (a, b) in [
        ((si, si), (2 * g + si, 2 * g + si)),
        ((2 * g + si, si), (si, 2 * g + si)),
    ] {
        let m = paint::segment(size, size, a, b, t);
        c.mask(&m, base.0, base.1, close, 255);
    }
    let mx = o.x + MINIMISE_X as i32 * si;
    c.fill(
        paint::Rect::new(mx - g, cy - (t as i32) / 2, 2 * g, t as i32),
        minimise,
    );
    // The title, centred, after the buttons when the window is narrow,
    // clipped to the title bar (INV-0160-6).
    let px = text::BODY.physical(zoraya_tokens::Scale::ONE) * s;
    let tw = title.measure(&w.title, px);
    let start = o.x + (MINIMISE_X as i32 + 18) * si;
    let tx = (o.x + o.w / 2 - tw / 2).max(start);
    let baseline = o.y + f.title as i32 / 2 + px as i32 * 7 / 20;
    let bar = paint::Rect::new(
        start,
        o.y,
        (o.right() - 8 * si - start).max(0),
        f.title as i32,
    );
    let old = c.set_clip(bar.intersect(&c.clip));
    draw_text(c, title, &w.title, px, tx, baseline, p.fg_text);
    c.clip = old;
}

/// The switcher (ADR-0160): the trusted material, a group line per room
/// (one, `home`, until 7d builds rooms), and a row per window with its
/// pixels shrunk and its title; the chosen row highlighted.
fn draw_switcher(
    c: &mut Canvas<'_>,
    s: &crate::Switcher,
    windows: &[Window],
    k: u32,
    p: &Palette,
    ui: &mut zoraya_text::Text<'static>,
    medium: &mut zoraya_text::Text<'static>,
) {
    let ki = k as i32;
    let w = (SWITCHER_WIDTH * k) as i32;
    let h = ((SWITCHER_GROUP + SWITCHER_ROW * s.windows.len() as u32 + 16) * k) as i32;
    let x = (c.width as i32 - w) / 2;
    let y = ((c.height as i32 - h) / 2)
        .max(space::BAR_HEIGHT.physical(zoraya_tokens::Scale::ONE) as i32 * ki);
    let material = Material {
        radius: space::RADIUS_POPUP.physical(zoraya_tokens::Scale::ONE) * k,
        shape: shape::CORNER,
        blur: (glow::PANEL_BLUR.physical(zoraya_tokens::Scale::ONE) * k) as usize,
        halo_blur: (glow::BLUR.physical(zoraya_tokens::Scale::ONE) * k) as usize,
        tint: p.bg_overlay,
        tint_alpha: glow::PANEL_ALPHA,
        glow: p.glow,
        glow_alpha: glow::ALPHA,
        halo_alpha: glow::HALO,
        border: p.border,
        scale: k,
    };
    material.draw(c, paint::Rect::new(x, y, w, h));
    let body_px = text::BODY.physical(zoraya_tokens::Scale::ONE) * k;
    let caption_px = text::CAPTION.physical(zoraya_tokens::Scale::ONE) * k;
    // The group line: the room's dot (home is grey) and its name.
    let gy = y + 8 * ki;
    let dot = paint::rounded_rect(
        (8 * k) as usize,
        (8 * k) as usize,
        4 * k,
        zoraya_tokens::CornerShape::Circle,
    );
    c.mask(&dot, x + 20 * ki, gy + 10 * ki, p.fg_text_secondary, 255);
    draw_text(
        c,
        medium,
        "home",
        body_px,
        x + 34 * ki,
        gy + 18 * ki,
        p.fg_text,
    );
    for (i, key) in s.windows.iter().enumerate() {
        let Some(win) = windows
            .iter()
            .find(|w| w.client == key.client && w.id == key.id)
        else {
            continue;
        };
        let ry = y + ((SWITCHER_GROUP + SWITCHER_ROW * i as u32) * k) as i32 + 8 * ki;
        let row = paint::Rect::new(
            x + 12 * ki,
            ry,
            w - 24 * ki,
            (SWITCHER_ROW * k) as i32 - 4 * ki,
        );
        if i == s.chosen {
            let m = paint::rounded_rect(
                row.w as usize,
                row.h as usize,
                space::RADIUS_CONTROL.physical(zoraya_tokens::Scale::ONE) * k,
                shape::CORNER,
            );
            c.mask(&m, row.x, row.y, p.bg_selection, 255);
        }
        // The thumbnail: the window's own pixels, box-filtered.
        let (tw, th) = ((THUMB.0 * k) as usize, (THUMB.1 * k) as usize);
        let thumb = shrink(
            &win.copy,
            win.buffer.width as usize,
            win.buffer.height as usize,
            tw,
            th,
        );
        let tx = row.x + 8 * ki;
        let ty = row.y + (row.h - th as i32) / 2;
        c.image(&thumb, tw, tx, ty, None);
        let edge = paint::rounded_outline(tw, th, 0, k, zoraya_tokens::CornerShape::Circle);
        c.mask(&edge, tx, ty, p.border, 255);
        let label = if win.title.is_empty() {
            "window"
        } else {
            win.title.as_str()
        };
        let lx = tx + tw as i32 + 16 * ki;
        let old = c.set_clip(
            paint::Rect::new(lx, row.y, row.right() - lx - 8 * ki, row.h).intersect(&c.clip),
        );
        draw_text(
            c,
            medium,
            label,
            body_px,
            lx,
            row.y + row.h / 2 + body_px as i32 * 7 / 20 - (if win.hidden { 6 * ki } else { 0 }),
            p.fg_text,
        );
        if win.hidden {
            draw_text(
                c,
                ui,
                "hidden",
                caption_px,
                lx,
                row.y + row.h / 2 + 14 * ki,
                p.fg_text_secondary,
            );
        }
        c.clip = old;
    }
}

/// `src` (`sw × sh`) shrunk to `dw × dh` by averaging the source pixels
/// under each destination pixel (a box filter, as the mockup's
/// `image_scaled`); letterboxed in nothing: the aspect is the caller's.
pub fn shrink(src: &[u32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u32> {
    let mut out = alloc::vec![0u32; dw * dh];
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 || src.len() < sw * sh {
        return out;
    }
    for y in 0..dh {
        let (y0, y1) = (
            y * sh / dh,
            ((y + 1) * sh / dh).max(y * sh / dh + 1).min(sh),
        );
        for x in 0..dw {
            let (x0, x1) = (
                x * sw / dw,
                ((x + 1) * sw / dw).max(x * sw / dw + 1).min(sw),
            );
            let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = src[sy * sw + sx];
                    r += ((p >> 16) & 0xff) as u64;
                    g += ((p >> 8) & 0xff) as u64;
                    b += (p & 0xff) as u64;
                    n += 1;
                }
            }
            let n = n.max(1);
            out[y * dw + x] = (((r / n) << 16) | ((g / n) << 8) | (b / n)) as u32;
        }
    }
    out
}

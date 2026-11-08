//! ADR-0161's `deep` wallpaper (and `rooms`, which is `deep` with the
//! fields of ADR-0172 drawn over it, 7d): a diagonal gradient from
//! `wallpaper.a` to `wallpaper.b`, four soft blobs (`wallpaper.c`,
//! `wallpaper.d`, a darker `a`, a lighter `b`), a faint fold and a
//! vignette, computed per pixel in 16.16 fixed point. The mockup tool's
//! `wallpaper_deep` is the reference; this is its integer form.

use alloc::vec;
use alloc::vec::Vec;

use zoraya_tokens::{Color, Palette};

use crate::isqrt;

/// 1.0 in 16.16.
const Q: i64 = 1 << 16;

/// `a` towards `b` by `t` (16.16, 0 to 1), per channel, rounded.
fn mix(a: Color, b: Color, t: i64) -> Color {
    let t = t.clamp(0, Q);
    let ch = |x: u8, y: u8| -> u32 {
        let v = x as i64 * (Q - t) + y as i64 * t;
        ((v + Q / 2) >> 16) as u32
    };
    Color::of_pixel(ch(a.r(), b.r()) << 16 | ch(a.g(), b.g()) << 8 | ch(a.b(), b.b()))
}

/// sin of an angle in 16.16 radians, in 16.16: the parabola with its
/// usual correction, within about 0.1 % of the true value.
pub fn sin(theta: i64) -> i64 {
    const PI: i64 = 205_887; // π in 16.16
    let two_pi = 2 * PI;
    let mut t = theta.rem_euclid(two_pi);
    if t > PI {
        t -= two_pi;
    }
    // y = 4/π t − 4/π² t |t|
    let y = 4 * t * Q / PI - 4 * t * t.abs() / PI * Q / PI;
    // Correction: y = P (y|y| − y) + y, P = 0.225.
    let p = 14_746; // 0.225 in 16.16
    p * (y * y.abs() / Q - y) / Q + y
}

/// The four colours of the wallpaper and the dark and light ends.
struct Colours {
    a: Color,
    b: Color,
    c: Color,
    d: Color,
    dark_a: Color,
    light_b: Color,
    black: Color,
    white: Color,
}

/// A smooth bump of radius `r` (16.16 of the width) around `(cx, cy)`,
/// with `aspect` = height / width in 16.16.
fn blob(fx: i64, fy: i64, cx: i64, cy: i64, r: i64, aspect: i64) -> i64 {
    let dx = fx - cx;
    let dy = (fy - cy) * aspect / Q;
    let d = isqrt(dx * dx + dy * dy);
    let k = (Q - d * Q / r).clamp(0, Q);
    // k² (3 − 2k)
    k * k / Q * (3 * Q - 2 * k) / Q
}

/// The wallpaper of a `w × h` display in the colours of `p`.
pub fn deep(w: usize, h: usize, p: &Palette) -> Vec<u32> {
    let mut img = vec![0u32; w * h];
    if w == 0 || h == 0 {
        return img;
    }
    // Black and white as the darkest and lightest colours of the set:
    // `shadow` and `glow` are the direction's ends.
    let k = Colours {
        a: p.wallpaper_a,
        b: p.wallpaper_b,
        c: p.wallpaper_c,
        d: p.wallpaper_d,
        dark_a: mix(p.wallpaper_a, p.shadow, Q * 35 / 100),
        light_b: mix(p.wallpaper_b, p.glow, Q * 35 / 100),
        black: p.shadow,
        white: p.glow,
    };
    let aspect = h as i64 * Q / w as i64;
    for y in 0..h {
        let fy = y as i64 * Q / h as i64;
        for x in 0..w {
            let fx = x as i64 * Q / w as i64;
            let t = (fx + fy) / 2;
            let mut col = mix(k.a, k.b, t);
            col = mix(
                col,
                k.c,
                85 * blob(fx, fy, Q * 68 / 100, Q * 22 / 100, Q * 42 / 100, aspect) / 100,
            );
            col = mix(
                col,
                k.d,
                75 * blob(fx, fy, Q * 22 / 100, Q * 78 / 100, Q * 40 / 100, aspect) / 100,
            );
            col = mix(
                col,
                k.dark_a,
                55 * blob(fx, fy, Q * 92 / 100, Q * 88 / 100, Q * 30 / 100, aspect) / 100,
            );
            col = mix(
                col,
                k.light_b,
                50 * blob(fx, fy, Q * 40 / 100, Q * 38 / 100, Q * 22 / 100, aspect) / 100,
            );
            // A gentle fold across the field.
            let fold = (sin(9 * fx + 4 * fy) / 2 + Q / 2) * 6 / 100;
            col = mix(col, k.white, fold);
            // The vignette.
            let (vx, vy) = ((fx - Q / 2).abs(), (fy - Q / 2).abs());
            let v = (isqrt(vx * vx + vy * vy) - Q * 35 / 100).clamp(0, Q * 40 / 100) * 35 / 100;
            col = mix(col, k.black, v);
            img[y * w + x] = col.pixel();
        }
    }
    img
}

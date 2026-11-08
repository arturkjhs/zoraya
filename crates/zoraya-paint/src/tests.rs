use std::vec;
use std::vec::Vec;

use zoraya_tokens::{CornerShape, DARK, LIGHT};

use crate::wallpaper::{deep, sin};
use crate::*;

#[test]
fn rect_arithmetic() {
    let a = Rect::new(0, 0, 10, 10);
    assert_eq!(a.intersect(&Rect::new(5, 5, 10, 10)), Rect::new(5, 5, 5, 5));
    assert!(a.intersect(&Rect::new(10, 0, 5, 5)).is_empty());
    assert_eq!(a.grow(2), Rect::new(-2, -2, 14, 14));
    assert!(a.contains(9, 9) && !a.contains(10, 9));
    assert_eq!(Rect::new(i32::MAX - 1, 0, 10, 1).right(), i32::MAX);
}

#[test]
fn a_rounded_rect_is_full_inside_and_empty_at_the_corner_points() {
    for shape in [CornerShape::Circle, CornerShape::Smooth] {
        let m = rounded_rect(40, 30, 10, shape);
        assert_eq!(m.at(20, 15), 255);
        assert_eq!(m.at(20, 0), 255, "edge middles are full");
        assert_eq!(m.at(0, 15), 255);
        for (x, y) in [(0, 0), (39, 0), (0, 29), (39, 29)] {
            assert_eq!(m.at(x, y), 0, "{shape:?} corner {x},{y}");
        }
        // Symmetric.
        for y in 0..30 {
            for x in 0..40 {
                assert_eq!(m.at(x, y), m.at(39 - x, y));
                assert_eq!(m.at(x, y), m.at(x, 29 - y));
            }
        }
    }
    // The smooth corner reaches further along the edge than the circle.
    let c = rounded_rect(80, 80, 10, CornerShape::Circle);
    let s = rounded_rect(80, 80, 10, CornerShape::Smooth);
    assert_eq!(c.at(11, 0), 255);
    assert!(s.at(11, 0) < 255);
    // A radius of 0 and one larger than the rectangle.
    assert!(
        rounded_rect(5, 5, 0, CornerShape::Smooth)
            .cov
            .iter()
            .all(|c| *c == 255)
    );
    let big = rounded_rect(6, 6, 100, CornerShape::Circle);
    assert_eq!(big.at(0, 0), 0);
    assert_eq!(rounded_rect(0, 0, 4, CornerShape::Smooth).cov.len(), 0);
}

#[test]
fn an_outline_is_empty_inside() {
    let m = rounded_outline(40, 30, 8, 2, CornerShape::Smooth);
    assert_eq!(m.at(20, 15), 0);
    assert_eq!(m.at(20, 0), 255);
    assert_eq!(m.at(20, 1), 255);
    assert_eq!(m.at(20, 2), 0);
}

#[test]
fn a_flat_region_stays_flat_under_the_blur() {
    let mut px = vec![0x0012_3456u32; 50 * 40];
    blur_region(&mut px, 50, Rect::new(5, 5, 30, 20), 24);
    assert!(px.iter().all(|p| *p == 0x0012_3456));
    // A step is softened and stays within its two values.
    let mut px: Vec<u32> = (0..100 * 10)
        .map(|i| if i % 100 < 50 { 0 } else { 0x00ff_ffff })
        .collect();
    blur_region(&mut px, 100, Rect::new(0, 0, 100, 10), 12);
    assert!(px[5 * 100 + 49] > 0 && px[5 * 100 + 49] < 0x00ff_ffff);
    assert_eq!(px[5 * 100], 0);
    // Outside the region nothing changes; a region off the image is safe.
    let mut px: Vec<u32> = (0..100).map(|i| i as u32).collect();
    let before = px.clone();
    blur_region(&mut px, 10, Rect::new(20, 20, 5, 5), 9);
    blur_region(&mut px, 10, Rect::new(0, 0, 10, 10), 0);
    assert_eq!(px, before);
}

#[test]
fn a_shadow_fades_and_is_symmetric() {
    let (m, margin) = shadow(40, 20, 6, CornerShape::Smooth, 12);
    assert_eq!(margin, 26);
    assert_eq!((m.w, m.h), (40 + 52, 20 + 52));
    assert_eq!(m.at(0, 0), 0);
    let centre = m.at(margin + 20, margin + 10);
    assert!(centre > 200, "{centre}");
    assert!(m.at(margin - 6, margin + 10) < centre);
    for y in 0..m.h {
        for x in 0..m.w {
            assert_eq!(m.at(x, y), m.at(m.w - 1 - x, y));
        }
    }
    let (sharp, _) = shadow(10, 10, 0, CornerShape::Circle, 0);
    assert_eq!(sharp.at(2 + 5, 2 + 5), 255);
}

#[test]
fn a_segment_covers_its_line() {
    let m = segment(20, 20, (2, 10), (18, 10), 2);
    assert_eq!(m.at(10, 9), 255);
    assert_eq!(m.at(10, 10), 255);
    assert_eq!(m.at(10, 12), 0);
    let d = segment(20, 20, (4, 4), (16, 16), 2);
    assert!(d.at(10, 10) > 200);
    assert_eq!(d.at(16, 4), 0);
}

#[test]
fn the_canvas_stays_in_its_clip() {
    const GUARD: u32 = 0x0055_5555;
    let mut px = vec![GUARD; 30 * 30];
    {
        let mut c = Canvas::new(&mut px, 30);
        c.set_clip(Rect::new(5, 5, 20, 20));
        c.fill(Rect::new(-10, -10, 100, 100), LIGHT.bg_window);
        c.blend(Rect::new(0, 0, 30, 30), LIGHT.shadow, 128);
        let m = rounded_rect(50, 50, 10, CornerShape::Smooth);
        c.mask(&m, -10, -10, LIGHT.accent, 255);
        let src = vec![0x00ab_cdefu32; 40 * 40];
        c.image(&src, 40, -5, -5, None);
        c.band(Rect::new(0, 0, 30, 30), LIGHT.glow, 0, 200);
        let material = Material {
            radius: 8,
            shape: CornerShape::Smooth,
            blur: 12,
            halo_blur: 6,
            tint: LIGHT.bg_overlay,
            tint_alpha: 190,
            glow: LIGHT.glow,
            glow_alpha: 150,
            halo_alpha: 110,
            border: LIGHT.border,
            scale: 1,
        };
        material.draw(&mut c, Rect::new(-3, 2, 40, 20));
        material.draw(&mut c, Rect::new(10, 10, 0, 5));
    }
    for y in 0..30 {
        for x in 0..30 {
            if !(5..25).contains(&x) || !(5..25).contains(&y) {
                assert_eq!(px[y * 30 + x], GUARD, "{x},{y}");
            }
        }
    }
    let mut empty: [u32; 0] = [];
    let mut c = Canvas::new(&mut empty, 0);
    c.fill(Rect::new(0, 0, 5, 5), LIGHT.glow);
}

#[test]
fn an_image_is_copied_unchanged_where_covered() {
    // INV-0164-1: client pixels reach the canvas as they are.
    let src: Vec<u32> = (0..64).map(|i| 0x0001_0101 * i).collect();
    let mut px = vec![0u32; 16 * 16];
    let mut c = Canvas::new(&mut px, 16);
    c.image(&src, 8, 4, 4, None);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(px[(y + 4) * 16 + x + 4], src[y * 8 + x]);
        }
    }
}

#[test]
fn sin_is_close_to_the_true_sine() {
    let q = 65536.0f64;
    for i in -40..40 {
        let theta = i as f64 * 0.37;
        let got = sin((theta * q) as i64) as f64 / q;
        assert!((got - theta.sin()).abs() < 0.003, "{theta}: {got}");
    }
}

#[test]
fn the_wallpaper_is_deterministic_varied_and_in_range() {
    let a = deep(64, 48, &LIGHT);
    let b = deep(64, 48, &LIGHT);
    assert_eq!(a, b);
    let mut distinct = a.clone();
    distinct.sort();
    distinct.dedup();
    assert!(distinct.len() > 50);
    assert!(a.iter().all(|p| p >> 24 == 0));
    let d = deep(64, 48, &DARK);
    let avg = |v: &[u32]| v.iter().map(|p| ((p >> 8) & 0xff) as u64).sum::<u64>() / v.len() as u64;
    assert!(avg(&d) < avg(&a), "the dark set is darker");
    assert!(deep(0, 10, &LIGHT).is_empty());
}

#[test]
fn the_wallpaper_matches_its_reference_digest() {
    // INV-0161-4's host half for the compositor's drawing.
    let w = deep(128, 96, &LIGHT);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in w {
        for b in p.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    assert_eq!(h, WALLPAPER_DIGEST, "{h:#018x}");
}

const WALLPAPER_DIGEST: u64 = 0xfeef_f317_9ce5_c8d0;

/// The wallpaper and the material at 1024 × 768 for looking at:
/// `ZORAYA_PAINT_SAMPLES=dir cargo test -p zoraya-paint -- --ignored samples`.
#[test]
#[ignore = "writes images"]
fn samples() {
    let Ok(dir) = std::env::var("ZORAYA_PAINT_SAMPLES") else {
        return;
    };
    for (name, p) in [("light", &LIGHT), ("dark", &DARK)] {
        let (w, h) = (1024usize, 768usize);
        let mut px = deep(w, h, p);
        {
            let mut c = Canvas::new(&mut px, w);
            let (s, margin) = shadow(600, 400, 10, CornerShape::Smooth, 22);
            c.mask(
                &s,
                100 - margin as i32,
                120 + 8 - margin as i32,
                p.shadow,
                96,
            );
            let body = rounded_rect(600, 400, 10, CornerShape::Smooth);
            c.mask(&body, 100, 120, p.bg_window, 255);
            let bar = rounded_rect(600, 36, 10, CornerShape::Smooth);
            c.mask(&bar, 100, 120, p.bg_frame_focused, 255);
            let edge = rounded_outline(600, 400, 10, 1, CornerShape::Smooth);
            c.mask(&edge, 100, 120, p.border, 255);
            let material = Material {
                radius: 12,
                shape: CornerShape::Smooth,
                blur: 36,
                halo_blur: 30,
                tint: p.bg_overlay,
                tint_alpha: 190,
                glow: p.glow,
                glow_alpha: 150,
                halo_alpha: 110,
                border: p.border,
                scale: 1,
            };
            material.draw(&mut c, Rect::new(300, 300, 420, 200));
            material.draw(&mut c, Rect::new(-12, -12, 1048, 40));
        }
        let mut out = std::format!("P6\n{w} {h}\n255\n").into_bytes();
        for q in &px {
            out.extend_from_slice(&[(q >> 16) as u8, (q >> 8) as u8, *q as u8]);
        }
        std::fs::write(std::format!("{dir}/paint-{name}.ppm"), out).unwrap();
    }
}

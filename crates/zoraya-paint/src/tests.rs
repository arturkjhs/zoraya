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

// Tests for the holes cargo-mutants found in 7b (TASK-24), from the
// mutant-triage subagent's list.

#[test]
fn rect_edges_and_disjoint_boxes() {
    let a = Rect::new(0, 0, 10, 10);
    assert_eq!(a.intersect(&Rect::new(20, 0, 5, 5)), Rect::default());
    assert_eq!(a.intersect(&Rect::new(0, 20, 5, 5)), Rect::default());
    assert_eq!(a.grow(3), Rect::new(-3, -3, 16, 16));
    assert!(!a.contains(9, 10) && !a.contains(10, 9) && a.contains(0, 0));
}

#[test]
fn a_mask_reads_zero_outside_its_size() {
    let m = Mask {
        w: 4,
        h: 2,
        cov: vec![255; 8],
    };
    assert_eq!((m.at(3, 1), m.at(4, 0), m.at(0, 2)), (255, 0, 0));
}

#[test]
fn a_partly_covered_corner_pixel_has_its_subsample_coverage() {
    // Exact subsample counts of 16, from the corner tests in lib.rs.
    let count = |shape, r: i64, cx: i64, cy: i64| {
        let mut n = 0;
        for sy in 0..4 {
            for sx in 0..4 {
                if super::inside_corner(shape, r, cx * 4 + sx, cy * 4 + sy) {
                    n += 1;
                }
            }
        }
        (n * 255 / 16) as u8
    };
    let c = rounded_rect(20, 20, 4, CornerShape::Circle);
    assert_eq!(c.at(1, 1), count(CornerShape::Circle, 4, 1, 1));
    assert!(c.at(1, 1) > 0 && c.at(1, 1) < 255);
    let s = rounded_rect(40, 40, 10, CornerShape::Smooth);
    assert_eq!(s.at(3, 3), count(CornerShape::Smooth, 10, 3, 3));
    assert!(s.at(3, 3) > 0 && s.at(3, 3) < 255);
    assert_eq!(corner_span(CornerShape::Smooth, 10), 16);
    assert_eq!(corner_span(CornerShape::Circle, 10), 10);
}

#[test]
fn a_thin_outline_is_the_frame_inside_its_edge() {
    assert_eq!(
        rounded_outline(3, 10, 0, 1, CornerShape::Circle).at(1, 5),
        0
    );
    assert_eq!(
        rounded_outline(10, 3, 0, 1, CornerShape::Circle).at(5, 1),
        0
    );
    assert!(
        rounded_outline(3, 10, 0, 2, CornerShape::Circle)
            .cov
            .iter()
            .all(|c| *c == 255)
    );
    assert!(
        rounded_outline(10, 3, 0, 2, CornerShape::Circle)
            .cov
            .iter()
            .all(|c| *c == 255)
    );
    let o = rounded_outline(6, 6, 0, 1, CornerShape::Circle);
    assert_eq!((o.at(4, 4), o.at(0, 4), o.at(5, 4)), (0, 255, 255));
    let o = rounded_outline(8, 8, 0, 2, CornerShape::Circle);
    assert_eq!(
        (o.at(0, 3), o.at(1, 3), o.at(2, 3), o.at(5, 5)),
        (255, 255, 0, 0)
    );
    // A round outline: outer coverage times one less the inner's.
    let o = rounded_outline(20, 20, 4, 1, CornerShape::Circle);
    let outer = rounded_rect(20, 20, 4, CornerShape::Circle);
    let inner = rounded_rect(18, 18, 3, CornerShape::Circle);
    for (x, y) in [(1, 1), (1, 2), (2, 2), (2, 1), (3, 1)] {
        let want = outer.at(x, y) as u32 * (255 - inner.at(x - 1, y - 1) as u32) / 255;
        assert_eq!(o.at(x, y) as u32, want, "({x}, {y})");
    }
}

#[test]
fn a_blur_repeats_the_edge_of_its_region() {
    // Two columns, the lower half white, blurred down the columns: each
    // column is what the same step gives along a row, the ends repeated
    // (the bottom stays near white, the top near black).
    let mut px: Vec<u32> = (0..16)
        .map(|i| if i >= 8 { 0x00ff_ffff } else { 0 })
        .collect();
    blur_region(&mut px, 2, Rect::new(0, 0, 2, 8), 6);
    let mut row: Vec<u32> = (0..8)
        .map(|i| if i >= 4 { 0x00ff_ffff } else { 0 })
        .collect();
    blur_region(&mut row, 8, Rect::new(0, 0, 8, 1), 6);
    for y in 0..8 {
        assert_eq!(px[y * 2], row[y], "row {y}");
        assert_eq!(px[y * 2 + 1], row[y], "row {y}");
    }
    assert!(
        row[7] & 0xff > 200 && row[0] & 0xff < 55,
        "{:x} {:x}",
        row[7],
        row[0]
    );
}

/// Three box passes of radius 2 over one impulse of 0xfd.
fn impulse_row() -> Vec<u32> {
    let mut v = vec![0u32; 9];
    v[4] = 0xfd;
    let mut out = vec![0u32; 9];
    super::box_pass(&v.clone(), &mut out, 9, 1, 2, true);
    v.copy_from_slice(&out);
    super::box_pass(&v.clone(), &mut out, 9, 1, 2, true);
    v.copy_from_slice(&out);
    super::box_pass(&v.clone(), &mut out, 9, 1, 2, true);
    out
}

#[test]
fn one_impulse_blurs_the_same_in_a_row_and_a_column() {
    let want = impulse_row();
    assert_eq!(want, [16, 22, 31, 37, 39, 37, 31, 22, 16]);
    let mut row = vec![0u32; 9];
    row[4] = 0xfd;
    blur_region(&mut row, 9, Rect::new(0, 0, 9, 1), 6);
    assert_eq!(row, want);
    let mut col = vec![0u32; 9];
    col[4] = 0xfd;
    blur_region(&mut col, 1, Rect::new(0, 0, 1, 9), 6);
    assert_eq!(col, want);
}

#[test]
fn a_region_inside_the_image_is_blurred_in_place_and_clipped_at_the_bottom() {
    let mut px = vec![0u32; 20 * 5];
    px[2 * 20 + 7] = 0x00fd_fdfd;
    blur_region(&mut px, 20, Rect::new(3, 2, 9, 1), 6);
    let want = impulse_row();
    for x in 0..20 {
        for y in 0..5 {
            let p = px[y * 20 + x];
            if y == 2 && (3..=11).contains(&x) {
                let v = want[x - 3];
                assert_eq!(p, v << 16 | v << 8 | v, "({x}, {y})");
            } else {
                assert_eq!(p, 0, "({x}, {y})");
            }
        }
    }
    let mut px: Vec<u32> = (0..50).map(|i| i as u32).collect();
    let before = px.clone();
    blur_region(&mut px, 10, Rect::new(0, 3, 10, 5), 6);
    assert_eq!(&px[..30], &before[..30]);
}

#[test]
fn a_shadow_without_blur_is_the_shape_and_one_with_it_spills() {
    let (s, margin) = shadow(40, 20, 6, CornerShape::Smooth, 0);
    assert_eq!(margin, 2);
    let r = rounded_rect(40, 20, 6, CornerShape::Smooth);
    for y in 0..20 {
        for x in 0..40 {
            assert_eq!(s.at(x + 2, y + 2), r.at(x, y));
        }
    }
    let (s, margin) = shadow(40, 20, 6, CornerShape::Smooth, 12);
    assert_eq!(margin, 26);
    assert!(s.at(25, 36) > 0, "past the left edge");
}

#[test]
fn a_vertical_segment_and_isqrt() {
    let m = segment(20, 20, (10, 2), (10, 12), 2);
    assert_eq!(
        (m.at(9, 6), m.at(10, 6), m.at(8, 6), m.at(11, 6)),
        (255, 255, 0, 0)
    );
    assert_eq!(
        (isqrt(1), isqrt(2), isqrt(4), isqrt(99), isqrt(0), isqrt(-3)),
        (1, 1, 2, 9, 0, 0)
    );
}

#[test]
fn fill_blend_mask_and_band_paint_what_they_say() {
    const GUARD: u32 = 0x0055_5555;
    let mut px = vec![GUARD; 30 * 30];
    {
        let mut c = Canvas::new(&mut px, 30);
        c.set_clip(Rect::new(5, 5, 20, 20));
        c.fill(Rect::new(0, 0, 30, 30), LIGHT.bg_window);
    }
    assert_eq!(px[10 * 30 + 10], LIGHT.bg_window.pixel());
    assert_eq!(px[2 * 30 + 2], GUARD);
    let black = Color::of_pixel(0);
    let mut px = vec![0u32; 4 * 6];
    {
        let mut c = Canvas::new(&mut px, 4);
        c.blend(Rect::new(0, 0, 2, 1), LIGHT.shadow, 255);
        let m = Mask {
            w: 2,
            h: 2,
            cov: vec![10, 20, 30, 40],
        };
        c.mask(&m, 1, 1, LIGHT.accent, 255);
        c.band(Rect::new(3, 0, 1, 5), LIGHT.accent, 50, 200);
    }
    assert_eq!(px[0], LIGHT.shadow.pixel());
    for (i, (x, y)) in [(1, 1), (2, 1), (1, 2), (2, 2)].iter().enumerate() {
        let cov = [10u8, 20, 30, 40][i];
        assert_eq!(
            px[y * 4 + x],
            black.blend(LIGHT.accent, cov).pixel(),
            "({x}, {y})"
        );
    }
    for (row, a) in [50u8, 80, 110, 140, 170].iter().enumerate() {
        assert_eq!(
            px[row * 4 + 3],
            black.blend(LIGHT.accent, *a).pixel(),
            "row {row}"
        );
    }
}

fn material(radius: u32) -> Material {
    Material {
        radius,
        shape: CornerShape::Smooth,
        blur: 0,
        halo_blur: 0,
        tint: LIGHT.bg_overlay,
        tint_alpha: 128,
        glow: LIGHT.glow,
        glow_alpha: 0,
        halo_alpha: 0,
        border: LIGHT.border,
        scale: 1,
    }
}

/// A canvas whose every row has its own colour.
fn rows(w: usize, h: usize) -> Vec<u32> {
    (0..w * h)
        .map(|i| ((i / w) as u32 * 0x0003_0507) & 0x00ff_ffff)
        .collect()
}

#[test]
fn the_material_glass_halo_glow_and_rim() {
    let under = rows(60, 260);
    let at = |v: &[u32], x: usize, y: usize| v[y * 60 + x];
    // Glass: what is under, tinted, inside the shape; the corner outside.
    let mut px = under.clone();
    material(8).draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 10, 20, 20));
    assert_eq!(
        at(&px, 15, 15),
        Color::of_pixel(at(&under, 15, 15))
            .blend(LIGHT.bg_overlay, 128)
            .pixel()
    );
    assert_eq!(at(&px, 5, 10), at(&under, 5, 10));
    // A halo only with alpha and blur.
    let mut m = material(4);
    m.halo_alpha = 110;
    m.halo_blur = 6;
    let mut px = under.clone();
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(10, 10, 20, 20));
    assert_ne!(at(&px, 8, 20), at(&under, 8, 20));
    m.halo_blur = 0;
    let mut px = under.clone();
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(10, 10, 20, 20));
    assert_eq!(at(&px, 8, 20), at(&under, 8, 20));
    // The band of light: a third of the height, from half the alpha.
    let mut m = material(4);
    m.tint_alpha = 0;
    m.glow_alpha = 150;
    let mut px = under.clone();
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 5, 40, 30));
    let band = |y: u32, h: u32| ((150u32 / 2) * (h - y) / h) as u8;
    assert_eq!(
        at(&px, 25, 7),
        Color::of_pixel(at(&under, 25, 7))
            .blend(LIGHT.glow, band(2, 10))
            .pixel()
    );
    // The rim, one pixel inside the edge, in tint towards glow.
    let rim = LIGHT.bg_overlay.blend(LIGHT.glow, 150).pixel();
    assert_eq!(at(&px, 6, 20), rim);
    assert_eq!(at(&px, 43, 20), rim);
    assert_eq!(at(&px, 46, 20), at(&under, 46, 20));
    // The band stops at 48 pixels on a tall surface.
    let mut px = under.clone();
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 5, 40, 200));
    assert_eq!(
        at(&px, 25, 37),
        Color::of_pixel(at(&under, 25, 37))
            .blend(LIGHT.glow, band(32, 48))
            .pixel()
    );
    // Without glow, no rim.
    let mut m = material(4);
    m.glow_alpha = 0;
    let mut px = under.clone();
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 5, 40, 30));
    assert_ne!(at(&px, 6, 20), rim);
    // Narrow and scaled surfaces: rims where they fit, no panic.
    let mut m = material(0);
    m.glow_alpha = 150;
    let mut px = under.clone();
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 5, 3, 20));
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 40, 20, 3));
    m.scale = 2;
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 60, 3, 20));
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 90, 10, 3));
    let mut px = under.clone();
    m.draw(&mut Canvas::new(&mut px, 60), Rect::new(5, 120, 40, 30));
    assert_eq!(
        at(&px, 7, 135),
        LIGHT.bg_overlay.blend(LIGHT.glow, 150).pixel(),
        "two pixels in at scale 2"
    );
}

/// The corners are the source's; a 2 × 1 ramp becomes a ramp; one pixel
/// fills everything; nothing out of bounds on odd sizes.
#[test]
fn upscale_is_bilinear_corner_to_corner() {
    let src = [0x00_00_00_00u32, 0xff_c8_64_00];
    let out = upscale(&src, 2, 1, 5, 3);
    assert_eq!(out.len(), 15);
    for y in 0..3 {
        assert_eq!(out[y * 5], src[0]);
        assert_eq!(out[y * 5 + 4], src[1]);
        assert_eq!(out[y * 5 + 2], 0x80_64_32_00, "the middle");
    }
    let ramp: std::vec::Vec<u32> = (0..5).map(|x| out[x] & 0xff00).collect();
    assert!(ramp.windows(2).all(|p| p[0] <= p[1]));
    assert_eq!(
        upscale(&[0x0012_3456], 1, 1, 3, 2),
        std::vec![0x0012_3456; 6]
    );
    let big = upscale(&src, 2, 1, 1023, 767);
    assert_eq!(big.len(), 1023 * 767);
    assert!(
        upscale(&src, 3, 1, 4, 4).iter().all(|p| *p == 0),
        "a short source"
    );
}

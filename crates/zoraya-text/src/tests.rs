use std::collections::BTreeMap;
use std::string::String;
use std::vec;
use std::vec::Vec;

use crate::raster::{ONE, Raster, render_box};
use crate::*;

pub const INTER: &[u8] = include_bytes!("../../../fonts/Inter-Regular.ttf");
pub const INTER_MEDIUM: &[u8] = include_bytes!("../../../fonts/Inter-Medium.ttf");
pub const MONO: &[u8] = include_bytes!("../../../fonts/JetBrainsMono-Regular.ttf");
pub const MONO_BOLD: &[u8] = include_bytes!("../../../fonts/JetBrainsMono-Bold.ttf");

fn mono_list() -> FontList<'static> {
    FontList::new([Font::parse(MONO).unwrap(), Font::parse(INTER).unwrap()])
}

fn ui_list() -> FontList<'static> {
    FontList::new([Font::parse(INTER).unwrap(), Font::parse(MONO).unwrap()])
}

fn table(tag: &[u8; 4], data: &[u8]) -> ([u8; 4], Vec<u8>) {
    let mut d = data.to_vec();
    while !d.len().is_multiple_of(4) {
        d.push(0);
    }
    (*tag, d)
}

fn be(v: &[i32]) -> Vec<u8> {
    v.iter().flat_map(|v| (*v as u16).to_be_bytes()).collect()
}

/// A font of three glyphs: 0 empty, 1 a triangle (from 'A'), 2 a
/// composite of glyph 1 moved right by 100 (from 'B'). cmap format 4.
pub fn tiny_font() -> Vec<u8> {
    let mut triangle = be(&[1, 0, 0, 0, 0, 2, 0]);
    triangle.extend_from_slice(&[1, 1, 1]);
    triangle.extend(be(&[0, 600, -300, 0, 0, 700]));
    // flags ARGS_ARE_WORDS | ARGS_ARE_XY, glyph 1, dx 100, dy 0
    let composite = be(&[-1, 0, 0, 0, 0, 0x0003, 1, 100, 0]);
    build_font(1000, 800, [500, 600, 700], triangle, composite)
}

/// A font of glyph 0 (empty), glyph 1 from 'A' and glyph 2 from 'B', with
/// these units per em, ascent and advances, and a kern pair (1, 2) = -50.
pub fn build_font(
    units: u16,
    ascent: i16,
    advances: [u16; 3],
    mut one: Vec<u8>,
    mut two: Vec<u8>,
) -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&units.to_be_bytes());
    let mut hhea = vec![0u8; 36];
    hhea[4..6].copy_from_slice(&ascent.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
    hhea[34..36].copy_from_slice(&3u16.to_be_bytes());
    let mut maxp = vec![0u8; 6];
    maxp[4..6].copy_from_slice(&3u16.to_be_bytes());
    let hmtx: Vec<u8> = advances
        .iter()
        .flat_map(|a| [a.to_be_bytes(), [0, 0]].concat())
        .collect();
    if one.len() % 2 == 1 {
        one.push(0);
    }
    if two.len() % 2 == 1 {
        two.push(0);
    }
    let mut glyf = one.clone();
    glyf.extend_from_slice(&two);
    let mut loca = Vec::new();
    for o in [0, 0, one.len() / 2, (one.len() + two.len()) / 2] {
        loca.extend_from_slice(&(o as u16).to_be_bytes());
    }
    let mut sub = be(&[4, 32, 0, 4, 0, 0, 0]);
    sub.extend(be(&[66, 0xFFFF, 0, 65, 0xFFFF]));
    sub.extend(be(&[1 - 65, 1]));
    sub.extend(be(&[0, 0]));
    let mut cmap = be(&[0, 1, 3, 1]);
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);
    // kern: version 0, one subtable, format 0 horizontal, pair (1, 2) = -50
    let mut kern = be(&[0, 1, 0, 20, 0x0001, 1, 6, 0, 0]);
    kern.extend(be(&[1, 2, -50]));
    let tables = [
        table(b"cmap", &cmap),
        table(b"glyf", &glyf),
        table(b"head", &head),
        table(b"hhea", &hhea),
        table(b"hmtx", &hmtx),
        table(b"kern", &kern),
        table(b"loca", &loca),
        table(b"maxp", &maxp),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut off = 12 + 16 * tables.len();
    for (tag, d) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(off as u32).to_be_bytes());
        out.extend_from_slice(&(d.len() as u32).to_be_bytes());
        off += d.len();
    }
    for (_, d) in &tables {
        out.extend_from_slice(d);
    }
    out
}

/// A simple glyph of `points` on-curve points, every x and y delta the
/// given 16-bit value: coordinates run to the ends of the range.
fn extreme_glyph(points: usize, dx: i16, dy: i16) -> Vec<u8> {
    let mut g = be(&[1, 0, 0, 0, 0, points as i32 - 1, 0]);
    g.extend(std::iter::repeat_n(1u8, points));
    for _ in 0..points {
        g.extend_from_slice(&dx.to_be_bytes());
    }
    for _ in 0..points {
        g.extend_from_slice(&dy.to_be_bytes());
    }
    g
}

/// A composite of glyph `of` scaled by `scale` (2.14) and moved by
/// (-32768, 32767).
fn scaled_composite(of: i32, scale: i16) -> Vec<u8> {
    be(&[
        -1,
        0,
        0,
        0,
        0,
        0x0003 | 0x0008,
        of,
        -32768,
        32767,
        scale as i32,
    ])
}

#[test]
fn hostile_fonts_and_sizes_draw_inside_and_never_panic() {
    // INV-0162-1 at the edges the review found (TASK-24 7a review, 1 to 5):
    // coordinates and sums at the ends of i32, a tiny em, a huge advance,
    // composites that saturate, long text, sizes up to u32::MAX, a huge
    // ascent for the box.
    let fonts = [
        build_font(
            16,
            800,
            [0, 65535, 65535],
            extreme_glyph(1024, -32768, -32768),
            scaled_composite(1, -0x8000),
        ),
        build_font(
            16,
            32767,
            [65535, 65535, 0],
            extreme_glyph(1024, 32767, 32767),
            scaled_composite(2, 0x7fff),
        ),
        build_font(
            16384,
            -32768,
            [1, 0, 65535],
            extreme_glyph(4096, 32767, -32768),
            scaled_composite(1, 0x4000),
        ),
    ];
    let long: String = "A".repeat(4000) + &"B".repeat(4000) + "\u{10FFFD}AB";
    for data in &fonts {
        let f = Font::parse(data).unwrap();
        let mut text = Text::new(FontList::new([f]));
        for px in [0, 1, 16, 512, 513, 41_943_039, u32::MAX] {
            guarded(|t| {
                text.draw(t, "AB\u{10FFFD}", px, 10, 30, zoraya_tokens::LIGHT.fg_text);
                text.draw(
                    t,
                    &long,
                    px,
                    i32::MIN + 1,
                    i32::MAX - 1,
                    zoraya_tokens::LIGHT.fg_text,
                );
                let cell = CellMetrics::of(&text.fonts, px);
                text.draw_cell(
                    t,
                    cell,
                    (10, 10),
                    '\u{10FFFD}',
                    px,
                    (zoraya_tokens::LIGHT.fg_text, zoraya_tokens::LIGHT.bg_view),
                );
            });
            let _ = text.measure(&long, px);
            let b = box_glyph(&text.fonts, px);
            let side = raster::max_side(MAX_PX) as usize;
            assert!(b.width <= side && b.height <= side);
        }
    }
}

#[test]
fn random_strings_through_the_real_fonts_stay_inside() {
    // INV-0162-1's random strings: seeded, over every block the fonts map
    // and some they do not.
    let mut seed = 0x9e37_79b9u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for list in [mono_list(), ui_list()] {
        let mut text = Text::new(list);
        for _ in 0..300 {
            let s: String = (0..next() % 24)
                .filter_map(|_| char::from_u32(next() % 0x3000))
                .collect();
            let px = 1 + next() % 80;
            let (x, y) = ((next() % 120) as i32 - 30, (next() % 120) as i32 - 30);
            guarded(|t| {
                text.draw(t, &s, px, x, y, zoraya_tokens::DARK.fg_text);
            });
        }
    }
}

#[test]
fn a_short_bitmap_draws_what_it_has() {
    let b = Bitmap {
        width: 4,
        height: 4,
        left: 0,
        top: 0,
        coverage: vec![255; 3],
    };
    guarded(|t| t.draw(&b, 10, 10, zoraya_tokens::LIGHT.fg_text));
}

#[test]
fn the_tiny_font_reads_its_metrics_cmap_outlines_and_kerning() {
    let data = tiny_font();
    let f = Font::parse(&data).unwrap();
    assert_eq!((f.units_per_em, f.ascent, f.descent), (1000, 800, -200));
    assert_eq!(f.glyph_index('A'), Some(1));
    assert_eq!(f.glyph_index('B'), Some(2));
    assert_eq!(f.glyph_index('C'), None);
    assert_eq!(f.glyph_index('Ж'), None);
    assert_eq!(f.advance(1), 600);
    assert_eq!(f.advance(2), 700);
    assert_eq!(f.kerning(1, 2), -50);
    assert_eq!(f.kerning(2, 1), 0);
    let mut o = Outline::default();
    f.outline(1, &mut o).unwrap();
    let pts: Vec<(i32, i32)> = o.points.iter().map(|p| (p.x, p.y)).collect();
    assert_eq!(pts, [(0, 0), (600, 0), (300, 700)]);
    assert_eq!(o.ends, [3]);
    f.outline(2, &mut o).unwrap();
    let pts: Vec<(i32, i32)> = o.points.iter().map(|p| (p.x, p.y)).collect();
    assert_eq!(pts, [(100, 0), (700, 0), (400, 700)]);
    f.outline(0, &mut o).unwrap();
    assert!(o.points.is_empty());
    assert!(f.outline(7, &mut o).is_err());
}

#[test]
fn not_a_font_is_an_error() {
    assert_eq!(Font::parse(b"").unwrap_err(), Error::NotTrueType);
    assert_eq!(
        Font::parse(b"OTTO\0\0\0\0").unwrap_err(),
        Error::NotTrueType
    );
    let mut data = tiny_font();
    // units per em of 0
    let record = (0..8)
        .map(|i| 12 + 16 * i)
        .find(|r| &data[*r..r + 4] == b"head")
        .unwrap();
    let head_at = u32::from_be_bytes(data[record + 8..record + 12].try_into().unwrap()) as usize;
    data[head_at + 18] = 0;
    data[head_at + 19] = 0;
    assert_eq!(Font::parse(&data).unwrap_err(), Error::BadValue);
}

#[test]
fn truncated_and_mutated_fonts_never_panic_and_draw_inside() {
    // INV-0162-1's deterministic fuzz: every truncation of the tiny font,
    // and seeded mutations of it and of the real fonts' bytes, read and
    // drawn into a target with a guard border.
    let tiny = tiny_font();
    for cut in 0..tiny.len() {
        let _ = Font::parse(&tiny[..cut]);
    }
    let mut seed = 0x1234_5678u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for (data, rounds) in [(&tiny[..], 3000), (MONO, 120), (INTER, 120)] {
        for _ in 0..rounds {
            let mut m = data.to_vec();
            for _ in 0..8 {
                let i = next() as usize % m.len();
                m[i] = next() as u8;
            }
            if let Ok(f) = Font::parse(&m) {
                let list = FontList::new([f]);
                let mut text = Text::new(list);
                guarded(|t| {
                    text.draw(
                        t,
                        "AB Жжřů ґ",
                        13 + next() % 40,
                        3,
                        20,
                        zoraya_tokens::LIGHT.fg_text,
                    );
                });
            }
        }
    }
}

#[test]
fn every_glyph_of_both_fonts_reads_or_fails_cleanly() {
    let mut o = Outline::default();
    for data in [INTER, INTER_MEDIUM, MONO, MONO_BOLD] {
        let f = Font::parse(data).unwrap();
        let mut complex = 0;
        for g in 0..f.glyph_count() as u16 {
            match f.outline(g, &mut o) {
                Ok(()) => assert!(o.points.len() <= font::MAX_POINTS),
                // ADR-0162's bound on contours or points: drawn as the box.
                // Inter has one such glyph, outside the stage 7 blocks.
                Err(Error::TooComplex) => complex += 1,
                Err(e) => panic!("glyph {g}: {e:?}"),
            }
        }
        assert!(complex <= 1, "{complex} glyphs past the bounds");
        assert_eq!(
            f.outline(f.glyph_count() as u16, &mut o),
            Err(Error::OutOfRange)
        );
    }
}

/// A 64 × 48 target framed by a guard of 8 pixels on every side; `f`
/// draws into the inner area. Panics if the guard changed.
fn guarded(f: impl FnOnce(&mut Target<'_>)) -> Vec<u32> {
    const GUARD: u32 = 0x00_5a_5a_5a;
    let (w, h, g) = (80usize, 64usize, 8usize);
    let mut pixels = vec![GUARD; w * h];
    {
        let mut t = Target {
            pixels: &mut pixels,
            stride: w,
            clip: Rect::new(g as i32, g as i32, (w - 2 * g) as u32, (h - 2 * g) as u32),
        };
        f(&mut t);
    }
    for y in 0..h {
        for x in 0..w {
            let inside = (g..w - g).contains(&x) && (g..h - g).contains(&y);
            if !inside {
                assert_eq!(pixels[y * w + x], GUARD, "guard touched at ({x}, {y})");
            }
        }
    }
    pixels
}

#[test]
fn drawing_at_and_past_every_edge_stays_inside_the_clip() {
    // INV-0162-1 over the real fonts and sizes.
    for list in [mono_list(), ui_list()] {
        let mut text = Text::new(list);
        for px in [6, 13, 26, 60] {
            for (x, y) in [
                (-100, -100),
                (-20, 10),
                (0, 0),
                (8, 8),
                (60, 30),
                (70, 56),
                (72, 120),
                (i32::MAX - 10, i32::MAX - 10),
                (i32::MIN + 10, i32::MIN + 10),
            ] {
                guarded(|t| {
                    text.draw(t, "Wй ЖQ|gř█", px, x, y, zoraya_tokens::DARK.fg_text);
                    let cell = CellMetrics::of(&text.fonts, px);
                    text.draw_cell(
                        t,
                        cell,
                        (x, y),
                        'Ж',
                        px,
                        (zoraya_tokens::DARK.fg_text, zoraya_tokens::DARK.bg_view),
                    );
                });
            }
        }
    }
    // A target whose clip is larger than its slice.
    let mut pixels = vec![0u32; 10 * 10];
    let mut t = Target {
        pixels: &mut pixels,
        stride: 10,
        clip: Rect::new(-50, -50, 500, 500),
    };
    let mut text = Text::new(mono_list());
    text.draw(&mut t, "MMMMMM", 30, 2, 20, zoraya_tokens::LIGHT.fg_text);
    t.fill(Rect::new(-5, -5, 100, 100), zoraya_tokens::LIGHT.bg_view);
    let mut empty: [u32; 0] = [];
    let mut t = Target {
        pixels: &mut empty,
        stride: 0,
        clip: Rect::new(0, 0, 10, 10),
    };
    text.draw(&mut t, "M", 13, 0, 10, zoraya_tokens::LIGHT.fg_text);
}

#[test]
fn a_square_covers_its_pixels_and_a_half_pixel_edge_half() {
    let square = |x0: i32| {
        let mut r = Raster::new(8, 8);
        let p = [
            (x0, 2 * ONE),
            (6 * ONE, 2 * ONE),
            (6 * ONE, 6 * ONE),
            (x0, 6 * ONE),
        ];
        for i in 0..4 {
            r.line(p[i], p[(i + 1) % 4]);
        }
        r.finish()
    };
    let c = square(2 * ONE);
    for y in 0..8 {
        for x in 0..8 {
            let inside = (2..6).contains(&x) && (2..6).contains(&y);
            assert_eq!(c[y * 8 + x], if inside { 255 } else { 0 }, "({x}, {y})");
        }
    }
    let c = square(2 * ONE + ONE / 2);
    assert_eq!(c[3 * 8 + 2], 128);
    assert_eq!(c[3 * 8 + 3], 255);
}

#[test]
fn a_diagonal_halves_its_pixels() {
    // A right triangle over the whole 4 × 4 raster: the diagonal's pixels
    // are half covered, those below it fully, those above not at all.
    let mut r = Raster::new(4, 4);
    let p = [(0, 0), (4 * ONE, 4 * ONE), (0, 4 * ONE)];
    for i in 0..3 {
        r.line(p[i], p[(i + 1) % 3]);
    }
    let c = r.finish();
    for y in 0..4 {
        for x in 0..4 {
            let want = match x.cmp(&y) {
                core::cmp::Ordering::Less => 255,
                core::cmp::Ordering::Equal => 128,
                core::cmp::Ordering::Greater => 0,
            };
            assert_eq!(c[y * 4 + x], want, "({x}, {y})");
        }
    }
}

#[test]
fn segments_outside_the_raster_neither_panic_nor_leak() {
    let mut r = Raster::new(4, 4);
    r.line((-10 * ONE, -10 * ONE), (20 * ONE, 20 * ONE));
    r.line((20 * ONE, 20 * ONE), (-10 * ONE, 30 * ONE));
    r.line((i32::MIN, i32::MIN), (i32::MAX, i32::MAX));
    r.quad((i32::MIN, 0), (i32::MAX, i32::MAX), (0, i32::MIN));
    let _ = r.finish();
    let mut r = Raster::new(0, 0);
    r.line((0, 0), (ONE, ONE));
    assert!(r.finish().is_empty());
}

#[test]
fn the_box_is_a_hollow_rectangle() {
    let b = render_box(6, 9, 1, 1, -9);
    assert_eq!((b.width, b.height), (6, 9));
    for y in 0..9 {
        for x in 0..6 {
            let edge = x == 0 || x == 5 || y == 0 || y == 8;
            assert_eq!(
                b.coverage[y * 6 + x],
                if edge { 255 } else { 0 },
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn the_required_letters_are_in_both_fonts() {
    // INV-0162-3: the letters the layouts make, both cases, in the UI and
    // the monospace font, and every code point of the stage 7 blocks that
    // a font maps reads as an outline.
    let required = "ґєіїҐЄІЇěščřžýáíéúůďťňóĚŠČŘŽÝÁÍÉÚŮĎŤŇÓыъэёЫЪЭЁ";
    let mut o = Outline::default();
    for data in [INTER, INTER_MEDIUM, MONO, MONO_BOLD] {
        let f = Font::parse(data).unwrap();
        for c in required.chars() {
            assert!(f.glyph_index(c).is_some(), "{c} missing");
        }
        let blocks = [
            (0x20, 0x7E),
            (0xA0, 0xFF),
            (0x100, 0x17F),
            (0x180, 0x24F),
            (0x400, 0x4FF),
            (0x2000, 0x206F),
        ];
        for (a, b) in blocks {
            for u in a..=b {
                let Some(c) = char::from_u32(u) else { continue };
                if let Some(g) = f.glyph_index(c) {
                    f.outline(g, &mut o)
                        .unwrap_or_else(|e| panic!("U+{u:04X}: {e:?}"));
                }
            }
        }
        // All of Cyrillic's U+0410..U+044F.
        for u in 0x410..=0x44F {
            assert!(
                f.glyph_index(char::from_u32(u).unwrap()).is_some(),
                "U+{u:04X}"
            );
        }
    }
}

/// What the GUI tests type and read back (INV-0162-4).
pub const TYPED: &str = concat!(
    " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
    "АБВГҐДЕЄЖЗИІЇЙКЛМНОПРСТУФХЦЧШЩЬЮЯабвгґдеєжзиіїйклмнопрстуфхцчшщьюя",
    "ЁЫЪЭёыъэ",
    "ÁČĎÉĚÍŇÓŘŠŤÚŮÝŽáčďéěíňóřšťúůýž",
);

/// The pixels of a terminal cell drawing `c`, `fg` on `bg`.
fn cell_pixels(text: &mut Text<'_>, cell: CellMetrics, px: u32, c: char) -> Vec<u32> {
    let p = &zoraya_tokens::LIGHT;
    let mut pixels = vec![0u32; (cell.width * cell.height) as usize];
    let mut t = Target {
        pixels: &mut pixels,
        stride: cell.width as usize,
        clip: Rect::new(0, 0, cell.width, cell.height),
    };
    text.draw_cell(&mut t, cell, (0, 0), c, px, (p.term_fg, p.term_bg));
    pixels
}

/// Letters that every font draws alike, Cyrillic and Latin (and the
/// Cyrillic and Latin `i` with two dots): the screen reader reads each
/// class as its first letter, so a test's text compares in the same
/// classes (`tools/test-runner`).
pub const HOMOGLYPHS: &[&str] = &[
    "AА", "BВ", "CС", "EЕ", "HН", "IІ", "KК", "MМ", "OО", "PР", "TТ", "XХ", "aа", "cс", "eе", "iі",
    "oо", "pр", "xх", "yу",
];

#[test]
fn every_typed_glyph_is_distinct_in_the_terminal_font_but_the_homoglyphs() {
    // INV-0162-4, at the terminal size at scale 1 and 2, and from the box:
    // two characters draw alike exactly when HOMOGLYPHS says so.
    for scale in [zoraya_tokens::Scale::ONE, zoraya_tokens::Scale::TWO] {
        let px = zoraya_tokens::text::MONO.physical(scale);
        let mut text = Text::new(mono_list());
        let cell = CellMetrics::of(&text.fonts, px);
        let mut classes: BTreeMap<Vec<u32>, String> = BTreeMap::new();
        let boxed = cell_pixels(&mut text, cell, px, '\u{10FFFD}');
        classes.entry(boxed).or_default().push('□');
        for c in TYPED.chars() {
            let pixels = cell_pixels(&mut text, cell, px, c);
            classes.entry(pixels).or_default().push(c);
        }
        let mut alike: Vec<String> = classes
            .into_values()
            .filter(|c| c.chars().count() > 1)
            .collect();
        alike.sort();
        let mut want: Vec<String> = HOMOGLYPHS.iter().map(|s| String::from(*s)).collect();
        want.sort();
        assert_eq!(alike, want, "at {px} px");
    }
}

#[test]
fn the_terminal_cell_is_the_monospace_advance() {
    let text = Text::new(mono_list());
    let px = zoraya_tokens::text::MONO.physical(zoraya_tokens::Scale::ONE);
    let cell = CellMetrics::of(&text.fonts, px);
    // JetBrains Mono: 600 of 1000 units wide, ascent 1020, descent 300.
    assert_eq!(
        cell,
        CellMetrics {
            width: 8,
            height: 18,
            baseline: 14
        }
    );
}

#[test]
fn rendering_matches_the_reference_digests() {
    // INV-0162-2: the committed digests, the same on every host; the test
    // kernel's t_text checks the same in QEMU.
    use crate::reference::{CASES, REFERENCE, SAMPLE, Which, digest};
    for ((which, px), want) in CASES.iter().zip(REFERENCE) {
        let data = match which {
            Which::InterRegular => INTER,
            Which::InterMedium => INTER_MEDIUM,
            Which::MonoRegular => MONO,
            Which::MonoBold => MONO_BOLD,
        };
        let got = digest(data, *px, SAMPLE).unwrap();
        assert_eq!(got, want, "{which:?} at {px}: {got:#018x}");
    }
}

#[test]
fn two_renders_are_identical() {
    // INV-0161-4 and INV-0162-2 within one process.
    use crate::reference::{SAMPLE, digest};
    for (d, px) in [(MONO, 13), (INTER, 17)] {
        assert_eq!(digest(d, px, SAMPLE), digest(d, px, SAMPLE));
    }
}

#[test]
fn the_shaper_places_falls_back_and_puts_marks_over_their_base() {
    let list = FontList::new([Font::parse(MONO).unwrap()]);
    let mut out = Vec::new();
    // JetBrains Mono at 10 px: 6 px a character.
    let w = Simple.shape(&list, "ab\u{0301}c\u{10FFFD}", 10, &mut out);
    let xs: Vec<i32> = out.iter().map(|g| g.x).collect();
    assert_eq!(xs, [0, 6, 6, 12, 18]);
    let clusters: Vec<u32> = out.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, [0, 1, 2, 4, 5]);
    assert_eq!(out[4].source, Source::Box);
    assert_eq!(w, 18 + box_advance(10) as i32);
    // Fallback: the second font draws what the first lacks.
    let tiny = tiny_font();
    let list = FontList::new([Font::parse(&tiny).unwrap(), Font::parse(MONO).unwrap()]);
    assert_eq!(list.lookup('A'), (Source::Font(0), 1));
    assert_eq!(list.lookup('Ж').0, Source::Font(1));
    assert_eq!(list.lookup('\u{10FFFD}'), (Source::Box, 0));
    // Kerning of the pair (A, B) at 20 px: 12 px of advance less 1.
    out.clear();
    Simple.shape(&list, "AB", 20, &mut out);
    assert_eq!(out[1].x, 11);
}

#[test]
fn the_cache_hits_on_a_second_draw_and_evicts_within_its_capacity() {
    let mut text = Text::new(mono_list());
    text.cache = GlyphCache::new(64);
    let mut pixels = vec![0u32; 200 * 40];
    let mut t = Target {
        pixels: &mut pixels,
        stride: 200,
        clip: Rect::new(0, 0, 200, 40),
    };
    text.draw(&mut t, "abc", 13, 0, 20, zoraya_tokens::LIGHT.fg_text);
    assert_eq!((text.cache.hits, text.cache.misses), (0, 3));
    text.draw(&mut t, "abc", 13, 0, 20, zoraya_tokens::LIGHT.fg_text);
    assert_eq!((text.cache.hits, text.cache.misses), (3, 3));
    let many: String = ('А'..='я').collect();
    text.draw(&mut t, &many, 13, 0, 20, zoraya_tokens::LIGHT.fg_text);
    assert_eq!(text.cache.slots.len(), 64);
    assert!(text.cache.misses >= 3 + 64);
}

#[test]
fn which_fonts_have_kern_tables() {
    // ADR-0162's open question: whether kern format 0 is enough. Inter keeps
    // its kerning in GPOS only; JetBrains Mono, being monospace, has none.
    for (name, data) in [("Inter", INTER), ("JetBrains Mono", MONO)] {
        let f = Font::parse(data).unwrap();
        std::println!("{name}: kern table {}", f.has_kerning());
    }
}

#[test]
#[ignore = "a measurement: cargo test -p zoraya-text --release -- --ignored --nocapture grid_cost"]
fn grid_cost() {
    // plan.md's measurement on the host: a full 80 × 24 grid of Cyrillic,
    // first with an empty cache, then from the cache.
    let px = zoraya_tokens::text::MONO.physical(zoraya_tokens::Scale::ONE);
    let mut text = Text::new(mono_list());
    let cell = CellMetrics::of(&text.fonts, px);
    let (w, h) = (80 * cell.width as usize, 24 * cell.height as usize);
    let mut pixels = vec![0u32; w * h];
    let letters: Vec<char> = ('А'..='я').collect();
    let p = &zoraya_tokens::LIGHT;
    for round in 0..3 {
        let start = std::time::Instant::now();
        let mut t = Target {
            pixels: &mut pixels,
            stride: w,
            clip: Rect::new(0, 0, w as u32, h as u32),
        };
        for row in 0..24 {
            for col in 0..80 {
                let c = letters[(row * 80 + col) % letters.len()];
                text.draw_cell(
                    &mut t,
                    cell,
                    (
                        (col as u32 * cell.width) as i32,
                        (row as u32 * cell.height) as i32,
                    ),
                    c,
                    px,
                    (p.term_fg, p.term_bg),
                );
            }
        }
        std::println!(
            "round {round}: {:?}, cache {} hits {} misses",
            start.elapsed(),
            text.cache.hits,
            text.cache.misses
        );
    }
}

#[test]
#[ignore = "writes a sample for looking at: ZORAYA_TEXT_SAMPLE=path cargo test -p zoraya-text -- --ignored sample"]
fn sample() {
    let Ok(path) = std::env::var("ZORAYA_TEXT_SAMPLE") else {
        return;
    };
    let (w, h) = (640usize, 200usize);
    let p = &zoraya_tokens::LIGHT;
    let mut pixels = vec![p.bg_window.pixel(); w * h];
    let mut t = Target {
        pixels: &mut pixels,
        stride: w,
        clip: Rect::new(0, 0, w as u32, h as u32),
    };
    let mut ui = Text::new(ui_list());
    ui.draw(
        &mut t,
        "Zoraya: Ґанок, Žluťoučký kůň úpěl ďábelské ódy",
        13,
        10,
        24,
        p.fg_text,
    );
    ui.draw(
        &mut t,
        "Тиха ніч, їжак, ґудзик, єнот; příklad.txt",
        20,
        10,
        60,
        p.fg_text,
    );
    let mut mono = Text::new(mono_list());
    let cell = CellMetrics::of(&mono.fonts, 13);
    for (i, c) in "~> echo привіт > звіт-2026.txt {}[]|".chars().enumerate() {
        mono.draw_cell(
            &mut t,
            cell,
            (10 + (i as u32 * cell.width) as i32, 90),
            c,
            13,
            (p.term_fg, p.term_bg),
        );
    }
    mono.draw(
        &mut t,
        "JetBrains Mono 26: Жжřů ґ □\u{10FFFD}",
        26,
        10,
        160,
        p.fg_text,
    );
    let mut out = std::vec::Vec::new();
    for px in &pixels {
        out.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, *px as u8]);
    }
    let mut f = std::format!("P6\n{w} {h}\n255\n").into_bytes();
    f.extend_from_slice(&out);
    std::fs::write(path, f).unwrap();
}

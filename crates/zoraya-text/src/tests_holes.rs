//! Tests for the holes cargo-mutants found in 7a (TASK-24): font tables
//! the synthetic fonts did not reach, the raster's splitting against exact
//! areas, the box, the cache's two ways, drawing helpers.

use std::vec;
use std::vec::Vec;

use crate::font::{Error, MAX_COMPONENTS, MAX_DEPTH};
use crate::raster::{ONE, Raster, render_box};
use crate::*;

fn be16s(v: &[i32]) -> Vec<u8> {
    v.iter().flat_map(|v| (*v as u16).to_be_bytes()).collect()
}

fn pad4(mut d: Vec<u8>) -> Vec<u8> {
    while !d.len().is_multiple_of(4) {
        d.push(0);
    }
    d
}

/// A font from its tables, the directory sorted as given.
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let tables: Vec<([u8; 4], Vec<u8>)> =
        tables.iter().map(|(t, d)| (*t, pad4(d.clone()))).collect();
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

fn head(units: u16, long: bool) -> Vec<u8> {
    let mut h = vec![0u8; 54];
    h[18..20].copy_from_slice(&units.to_be_bytes());
    h[50..52].copy_from_slice(&(long as u16).to_be_bytes());
    h
}

fn hhea(metrics: u16) -> Vec<u8> {
    let mut h = vec![0u8; 36];
    h[4..6].copy_from_slice(&800u16.to_be_bytes());
    h[6..8].copy_from_slice(&(-200i16).to_be_bytes());
    h[8..10].copy_from_slice(&100u16.to_be_bytes());
    h[34..36].copy_from_slice(&metrics.to_be_bytes());
    h
}

fn maxp(glyphs: u16) -> Vec<u8> {
    let mut m = vec![0u8; 6];
    m[4..6].copy_from_slice(&glyphs.to_be_bytes());
    m
}

/// A unit square of side 100 at the origin, on-curve points.
fn square() -> Vec<u8> {
    let mut g = be16s(&[1, 0, 0, 100, 100, 3, 0]);
    g.extend_from_slice(&[1, 1, 1, 1]);
    g.extend(be16s(&[0, 100, 0, -100]));
    g.extend(be16s(&[0, 0, 100, 0]));
    g
}

/// A font whose glyph 1 is `square()` and whose other glyphs are given;
/// a format 12 cmap maps U+0041.. to glyphs 1.. (or format 4 with
/// `format4`), and `extra` tables are added.
fn font_with(
    glyphs: &[Vec<u8>],
    long: bool,
    cmap: Vec<u8>,
    extra: &[([u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let mut all = vec![Vec::new(), square()];
    all.extend(glyphs.iter().cloned());
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for g in &all {
        let mut g = g.clone();
        while g.len() % 4 != 0 {
            g.push(0);
        }
        if long {
            loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        } else {
            loca.extend_from_slice(&((glyf.len() / 2) as u16).to_be_bytes());
        }
        glyf.extend_from_slice(&g);
    }
    if long {
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
    } else {
        loca.extend_from_slice(&((glyf.len() / 2) as u16).to_be_bytes());
    }
    let n = all.len() as u16;
    let hmtx: Vec<u8> = (0..n)
        .flat_map(|i| [(500 + 10 * i).to_be_bytes(), 0u16.to_be_bytes()].concat())
        .collect();
    let mut tables = vec![
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head(1000, long)),
        (*b"hhea", hhea(n)),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp(n)),
    ];
    tables.extend(extra.iter().cloned());
    assemble(&tables)
}

/// A cmap table of the given (platform, encoding, subtable) records.
fn cmap_of(records: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = be16s(&[0, records.len() as i32]);
    let mut at = 4 + 8 * records.len();
    let mut subs = Vec::new();
    for (p, e, sub) in records {
        out.extend_from_slice(&p.to_be_bytes());
        out.extend_from_slice(&e.to_be_bytes());
        out.extend_from_slice(&(at as u32).to_be_bytes());
        at += sub.len();
        subs.extend_from_slice(sub);
    }
    out.extend(subs);
    out
}

/// Format 12 groups (first, last, glyph).
fn cmap12(groups: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut s = be16s(&[12, 0]);
    s.extend_from_slice(&((16 + 12 * groups.len()) as u32).to_be_bytes());
    s.extend_from_slice(&0u32.to_be_bytes());
    s.extend_from_slice(&(groups.len() as u32).to_be_bytes());
    for (a, b, g) in groups {
        for v in [a, b, g] {
            s.extend_from_slice(&v.to_be_bytes());
        }
    }
    s
}

/// Format 4 segments (start, end, delta), sorted by end, the 0xFFFF
/// segment added; with `ids`, the last given segment's glyphs come from an
/// id array instead of the delta.
fn cmap4(segments: &[(u16, u16, i16)], ids: Option<&[u16]>) -> Vec<u8> {
    let mut segs: Vec<(u16, u16, i16)> = segments.to_vec();
    segs.push((0xFFFF, 0xFFFF, 1));
    let n = segs.len();
    let mut s = be16s(&[4, 0, 0, (2 * n) as i32, 0, 0, 0]);
    for (_, e, _) in &segs {
        s.extend_from_slice(&e.to_be_bytes());
    }
    s.extend_from_slice(&[0, 0]);
    for (st, _, _) in &segs {
        s.extend_from_slice(&st.to_be_bytes());
    }
    for (_, _, d) in &segs {
        s.extend_from_slice(&d.to_be_bytes());
    }
    // idRangeOffset: from its own place to the id array after the offsets.
    let with_ids = segments.len().wrapping_sub(1);
    for i in 0..n {
        let off = match ids {
            Some(_) if i == with_ids => (2 * (n - i)) as u16,
            _ => 0,
        };
        s.extend_from_slice(&off.to_be_bytes());
    }
    if let Some(ids) = ids {
        for id in ids {
            s.extend_from_slice(&id.to_be_bytes());
        }
    }
    let len = s.len() as u16;
    s[2..4].copy_from_slice(&len.to_be_bytes());
    s
}

fn points(f: &Font<'_>, g: u16) -> Vec<(i32, i32)> {
    let mut o = Outline::default();
    f.outline(g, &mut o).unwrap();
    o.points.iter().map(|p| (p.x, p.y)).collect()
}

/// A composite of components (flags, glyph, args, transform words).
fn composite(parts: &[(u16, u16, Vec<i32>)]) -> Vec<u8> {
    let mut g = be16s(&[-1, 0, 0, 0, 0]);
    for (i, (flags, glyph, rest)) in parts.iter().enumerate() {
        let more = if i + 1 < parts.len() { 0x20 } else { 0 };
        g.extend_from_slice(&(flags | more).to_be_bytes());
        g.extend_from_slice(&glyph.to_be_bytes());
        if flags & 1 != 0 {
            g.extend(be16s(rest));
        } else {
            g.extend(rest.iter().map(|v| *v as u8));
        }
    }
    g
}

#[test]
fn composites_place_scale_and_transform_their_parts() {
    // Words and bytes for the offsets, a uniform scale, an x/y scale, a
    // 2 × 2 matrix, two parts, and a part without ARGS_ARE_XY.
    let words = composite(&[(0x0003, 1, vec![10, -20])]);
    let bytes = composite(&[(0x0002, 1, vec![5, -6])]);
    let scaled = composite(&[(0x000B, 1, vec![0, 0, 0x2000])]); // 0.5
    let xy = composite(&[(0x0043, 1, vec![0, 0, 0x4000, 0x2000])]);
    let matrix = composite(&[(0x0083, 1, vec![0, 0, 0, 0x4000, 0x4000, 0])]); // swap x and y
    let two = composite(&[(0x0003, 1, vec![0, 0]), (0x0003, 1, vec![200, 0])]);
    let matched = composite(&[(0x0001, 1, vec![3, 4])]);
    let data = font_with(
        &[words, bytes, scaled, xy, matrix, two, matched],
        false,
        cmap_of(&[(3, 1, cmap4(&[(0x41, 0x48, -0x40)], None))]),
        &[],
    );
    let f = Font::parse(&data).unwrap();
    assert_eq!(points(&f, 2), [(10, -20), (110, -20), (110, 80), (10, 80)]);
    assert_eq!(points(&f, 3), [(5, -6), (105, -6), (105, 94), (5, 94)]);
    assert_eq!(points(&f, 4), [(0, 0), (50, 0), (50, 50), (0, 50)]);
    assert_eq!(points(&f, 5), [(0, 0), (100, 0), (100, 50), (0, 50)]);
    assert_eq!(points(&f, 6), [(0, 0), (0, 100), (100, 100), (100, 0)]);
    let p = points(&f, 7);
    assert_eq!(p.len(), 8);
    assert_eq!(p[4..], [(200, 0), (300, 0), (300, 100), (200, 100)]);
    // Point matching is not supported: the part stays at the origin.
    assert_eq!(points(&f, 8), points(&f, 1));
    let mut o = Outline::default();
    f.outline(7, &mut o).unwrap();
    assert_eq!(o.ends, [4, 8]);
    assert_eq!(o.contours().count(), 2);
    assert_eq!(f.glyph_index('A'), Some(1));
    assert_eq!(f.glyph_index('H'), Some(8));
    assert_eq!(f.glyph_index('I'), None);
    assert_eq!(f.glyph_count(), 9);
    assert_eq!(f.advance(3), 530);
    // Past the last long metric, the last one's advance.
    assert_eq!(f.advance(500), 580);
}

#[test]
fn deep_and_wide_composites_are_refused() {
    // Each glyph n+1 refers to glyph n: nine levels is one too many.
    let mut glyphs = Vec::new();
    for n in 1..=MAX_DEPTH as u16 + 1 {
        glyphs.push(composite(&[(0x0003, n, vec![1, 1])]));
    }
    // A glyph that names glyph 1 more times than the budget allows.
    let parts: Vec<(u16, u16, Vec<i32>)> = (0..MAX_COMPONENTS + 1)
        .map(|_| (0x0003, 1, vec![0, 0]))
        .collect();
    glyphs.push(composite(&parts));
    let data = font_with(
        &glyphs,
        true,
        cmap_of(&[(0, 3, cmap12(&[(0x41, 0x41, 1)]))]),
        &[],
    );
    let f = Font::parse(&data).unwrap();
    let mut o = Outline::default();
    let depth = MAX_DEPTH as u16;
    assert!(
        f.outline(1 + depth, &mut o).is_ok(),
        "eight levels are fine"
    );
    assert_eq!(f.outline(2 + depth, &mut o), Err(Error::TooComplex));
    assert!(o.points.is_empty(), "a refused outline is empty");
    assert_eq!(f.outline(3 + depth, &mut o), Err(Error::TooComplex));
}

#[test]
fn the_best_unicode_cmap_is_chosen() {
    let four = cmap4(&[(0x41, 0x41, 1 - 0x41)], None);
    let twelve = cmap12(&[(0x41, 0x41, 1), (0x1F600, 0x1F601, 1)]);
    // A Mac Roman table is skipped; format 12 wins over 4 whatever their
    // order; Windows Unicode BMP and full repertoire both count.
    for records in [
        vec![(1, 0, twelve.clone()), (3, 1, four.clone())],
        vec![(3, 1, four.clone()), (3, 10, twelve.clone())],
        vec![(3, 10, twelve.clone()), (0, 3, four.clone())],
    ] {
        let data = font_with(&[], false, cmap_of(&records), &[]);
        let f = Font::parse(&data).unwrap();
        let has_emoji = records.iter().any(|(p, _, s)| *p != 1 && s[1] == 12);
        assert_eq!(f.glyph_index('A'), Some(1));
        assert_eq!(
            f.glyph_index('\u{1F600}').is_some(),
            has_emoji && f.glyph_count() > 1
        );
    }
    // Only a non-Unicode table, or an unknown format: no cmap.
    let data = font_with(&[], false, cmap_of(&[(1, 0, four.clone())]), &[]);
    assert_eq!(Font::parse(&data).unwrap_err(), Error::NoCmap);
    let mut six = four.clone();
    six[1] = 6;
    let data = font_with(&[], false, cmap_of(&[(3, 1, six)]), &[]);
    assert_eq!(Font::parse(&data).unwrap_err(), Error::NoCmap);
    // A record whose offset is past the table is skipped.
    let mut table = cmap_of(&[(3, 1, four.clone()), (3, 10, twelve.clone())]);
    table[16..20].copy_from_slice(&0x7fff_0000u32.to_be_bytes());
    let data = font_with(&[], false, table, &[]);
    assert_eq!(Font::parse(&data).unwrap().glyph_index('A'), Some(1));
}

#[test]
fn format_4_and_12_lookups_find_exactly_their_ranges() {
    // Segments 'B'..'D' by delta, 'K'..'M' through the id array.
    let ids = [1u16, 0, 1];
    let four = cmap4(&[(0x42, 0x44, 1 - 0x42), (0x4B, 0x4D, 0)], Some(&ids));
    let data = font_with(&[], false, cmap_of(&[(3, 1, four)]), &[]);
    let f = Font::parse(&data).unwrap();
    for (c, g) in [
        ('A', None),
        ('B', Some(1)),
        ('E', None),
        ('K', Some(1)),
        ('L', None),
        ('M', Some(1)),
        ('N', None),
    ] {
        assert_eq!(f.glyph_index(c), g, "{c}");
    }
    assert_eq!(f.glyph_index('\u{10000}'), None, "format 4 is the BMP");
    let twelve = cmap12(&[(0x30, 0x31, 1), (0x41, 0x41, 1), (0x100, 0x1FF, 70_000)]);
    let data = font_with(&[], false, cmap_of(&[(0, 4, twelve)]), &[]);
    let f = Font::parse(&data).unwrap();
    for (c, g) in [
        ('/', None),
        ('0', Some(1)),
        ('1', Some(2)),
        ('2', None),
        ('A', Some(1)),
        ('B', None),
        ('Ā', None),
    ] {
        let got = f.glyph_index(c);
        // Glyph 2 does not exist in this font: refused as "missing".
        let want = g.filter(|g| (*g as usize) < f.glyph_count());
        assert_eq!(got, want, "{c}");
    }
}

/// A kern table: version, then subtables of (coverage, pairs).
/// A kern subtable: its coverage and its pairs (left, right, value).
type Subtable = (u16, Vec<(u16, u16, i16)>);

fn kern_of(version: u16, subtables: &[Subtable]) -> Vec<u8> {
    let mut k = Vec::new();
    k.extend_from_slice(&version.to_be_bytes());
    k.extend_from_slice(&(subtables.len() as u16).to_be_bytes());
    for (coverage, pairs) in subtables {
        let len = 14 + 6 * pairs.len();
        k.extend(be16s(&[
            0,
            len as i32,
            *coverage as i32,
            pairs.len() as i32,
            0,
            0,
            0,
        ]));
        for (l, r, v) in pairs {
            k.extend(be16s(&[*l as i32, *r as i32, *v as i32]));
        }
    }
    k
}

#[test]
fn kern_takes_the_first_horizontal_format_0_table() {
    let pairs = vec![(1, 1, -7), (1, 2, -8), (2, 1, 9), (3, 3, 4)];
    let cmap = || cmap_of(&[(0, 3, cmap12(&[(0x41, 0x41, 1)]))]);
    // A vertical table and a cross-stream one are skipped before the one
    // that counts.
    let kern = kern_of(
        0,
        &[
            (0x0000, vec![(1, 1, 100)]),
            (0x0005, vec![(1, 1, 200)]),
            (0x0001, pairs.clone()),
        ],
    );
    let data = font_with(&[], false, cmap(), &[(*b"kern", kern)]);
    let f = Font::parse(&data).unwrap();
    assert!(f.has_kerning());
    assert_eq!(f.kerning(1, 1), -7);
    assert_eq!(f.kerning(1, 2), -8);
    assert_eq!(f.kerning(2, 1), 9);
    assert_eq!(f.kerning(3, 3), 4);
    assert_eq!(f.kerning(2, 2), 0);
    assert_eq!(f.kerning(0, 0), 0);
    assert_eq!(f.kerning(4, 4), 0);
    // A version 1 (Apple) table, or a format 2 one, kerns nothing.
    for kern in [
        kern_of(1, &[(0x0001, pairs.clone())]),
        kern_of(0, &[(0x0201, pairs.clone())]),
    ] {
        let data = font_with(&[], false, cmap(), &[(*b"kern", kern)]);
        let f = Font::parse(&data).unwrap();
        assert!(!f.has_kerning());
        assert_eq!(f.kerning(1, 1), 0);
    }
    // Pairs that run past the table: no kerning, not a panic.
    let mut kern = kern_of(0, &[(0x0001, pairs.clone())]);
    kern.truncate(kern.len() - 4);
    let data = font_with(&[], false, cmap(), &[(*b"kern", kern)]);
    assert!(!Font::parse(&data).unwrap().has_kerning());
    // A first subtable whose length is too small stops the search.
    let mut kern = kern_of(0, &[(0x0000, vec![]), (0x0001, pairs)]);
    kern[6..8].copy_from_slice(&2u16.to_be_bytes());
    let data = font_with(&[], false, cmap(), &[(*b"kern", kern)]);
    assert!(!Font::parse(&data).unwrap().has_kerning());
}

#[test]
fn header_errors_are_named() {
    let ok = font_with(
        &[],
        false,
        cmap_of(&[(3, 1, cmap4(&[(0x41, 0x41, -0x40)], None))]),
        &[],
    );
    assert!(Font::parse(&ok).is_ok());
    // No hmtx metrics at all.
    let mut bad = ok.clone();
    let rec = (0..7)
        .map(|i| 12 + 16 * i)
        .find(|r| &bad[*r..r + 4] == b"hhea")
        .unwrap();
    let at = u32::from_be_bytes(bad[rec + 8..rec + 12].try_into().unwrap()) as usize;
    bad[at + 34] = 0;
    bad[at + 35] = 0;
    assert_eq!(Font::parse(&bad).unwrap_err(), Error::OutOfRange);
    // An unknown loca format.
    let mut bad = ok.clone();
    let rec = (0..7)
        .map(|i| 12 + 16 * i)
        .find(|r| &bad[*r..r + 4] == b"head")
        .unwrap();
    let at = u32::from_be_bytes(bad[rec + 8..rec + 12].try_into().unwrap()) as usize;
    bad[at + 51] = 2;
    assert_eq!(Font::parse(&bad).unwrap_err(), Error::BadValue);
    // A table that runs past the file, and one that is missing.
    let mut bad = ok.clone();
    bad[12 + 12..12 + 16].copy_from_slice(&0x00ff_ffffu32.to_be_bytes());
    assert_eq!(Font::parse(&bad).unwrap_err(), Error::OutOfRange);
    let mut bad = ok.clone();
    let rec = (0..7)
        .map(|i| 12 + 16 * i)
        .find(|r| &bad[*r..r + 4] == b"maxp")
        .unwrap();
    bad[rec..rec + 4].copy_from_slice(b"xxxx");
    assert_eq!(
        Font::parse(&bad).unwrap_err(),
        Error::MissingTable(*b"maxp")
    );
    // The "true" tag is a TrueType font too.
    let mut t = ok.clone();
    t[..4].copy_from_slice(b"true");
    assert!(Font::parse(&t).is_ok());
}

#[test]
fn glyph_ranges_are_checked_in_both_loca_formats() {
    for long in [false, true] {
        let data = font_with(
            &[square()],
            long,
            cmap_of(&[(3, 1, cmap4(&[(0x41, 0x42, -0x40)], None))]),
            &[],
        );
        let f = Font::parse(&data).unwrap();
        assert_eq!(points(&f, 2), points(&f, 1));
        let mut o = Outline::default();
        assert_eq!(f.outline(3, &mut o), Err(Error::OutOfRange));
        assert!(f.outline(0, &mut o).is_ok() && o.points.is_empty());
    }
}

#[test]
fn contours_that_start_or_stay_off_the_curve_close() {
    // Four off-curve points: a closed curve through their midpoints.
    let mut r = Raster::new(12, 12);
    let p = |x: i32, y: i32| (x * ONE, y * ONE, false);
    r.contour(&[p(1, 1), p(11, 1), p(11, 11), p(1, 11)]);
    let c = r.finish();
    assert_eq!(c[6 * 12 + 6], 255, "the middle is inside");
    assert_eq!(c[0], 0);
    let mut area: u32 = c.iter().map(|v| *v as u32).sum();
    // The curve through (6,1) (11,6) (6,11) (1,6) with the corners as
    // controls: between the diamond (50) and the square (100).
    area /= 255;
    assert!((60..95).contains(&area), "{area}");
    // Starting off the curve then on: the same as starting on it.
    let mut a = Raster::new(12, 12);
    a.contour(&[
        (ONE, ONE, false),
        (11 * ONE, ONE, true),
        (11 * ONE, 11 * ONE, true),
        (ONE, 11 * ONE, true),
    ]);
    let mut b = Raster::new(12, 12);
    b.contour(&[
        (11 * ONE, ONE, true),
        (11 * ONE, 11 * ONE, true),
        (ONE, 11 * ONE, true),
        (ONE, ONE, false),
    ]);
    assert_eq!(a.finish(), b.finish());
    // One point is no contour.
    let mut one = Raster::new(4, 4);
    one.contour(&[(ONE, ONE, true)]);
    assert!(one.finish().iter().all(|c| *c == 0));
    assert_eq!((one.width(), one.height()), (4, 4));
}

/// The exact area of the polygon `points` (pixels) inside each pixel of a
/// `w × h` grid, by clipping in f64: the reference the integer raster is
/// held to.
fn exact(points: &[(f64, f64)], w: usize, h: usize) -> Vec<f64> {
    let mut out = vec![0.0; w * h];
    for py in 0..h {
        for px in 0..w {
            let (x0, y0, x1, y1) = (px as f64, py as f64, px as f64 + 1.0, py as f64 + 1.0);
            let mut poly: Vec<(f64, f64)> = points.to_vec();
            for (axis, bound, keep_less) in
                [(0, x0, false), (0, x1, true), (1, y0, false), (1, y1, true)]
            {
                let mut next = Vec::new();
                for i in 0..poly.len() {
                    let a = poly[i];
                    let b = poly[(i + 1) % poly.len()];
                    let va = if axis == 0 { a.0 } else { a.1 };
                    let vb = if axis == 0 { b.0 } else { b.1 };
                    let ina = if keep_less { va <= bound } else { va >= bound };
                    let inb = if keep_less { vb <= bound } else { vb >= bound };
                    if ina {
                        next.push(a);
                    }
                    if ina != inb {
                        let t = (bound - va) / (vb - va);
                        next.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
                    }
                }
                poly = next;
                if poly.is_empty() {
                    break;
                }
            }
            let mut area = 0.0;
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                area += a.0 * b.1 - b.0 * a.1;
            }
            out[py * w + px] = (area / 2.0).abs();
        }
    }
    out
}

#[test]
fn slanted_edges_cover_each_pixel_by_its_exact_area() {
    // Shallow and steep edges in both directions, crossing many columns in
    // one row: the cell splitting of `Raster::row`.
    let shapes: [&[(f64, f64)]; 4] = [
        &[(0.25, 0.5), (15.75, 2.25), (15.5, 7.0), (0.5, 6.0)],
        &[(15.75, 0.25), (0.25, 3.75), (0.75, 7.5), (15.0, 7.25)],
        &[(3.0, 0.0), (5.5, 8.0), (1.0, 8.0)],
        &[(0.0, 0.0), (16.0, 1.0), (16.0, 1.5), (0.0, 0.5)],
    ];
    for pts in shapes {
        let (w, h) = (16, 8);
        let mut r = Raster::new(w, h);
        let fixed: Vec<(i32, i32)> = pts
            .iter()
            .map(|(x, y)| ((x * 64.0) as i32, (y * 64.0) as i32))
            .collect();
        for i in 0..fixed.len() {
            r.line(fixed[i], fixed[(i + 1) % fixed.len()]);
        }
        let got = r.finish();
        let want = exact(pts, w, h);
        for i in 0..w * h {
            let e = (want[i] * 255.0).round() as i32;
            assert!(
                (got[i] as i32 - e).abs() <= 2,
                "{pts:?} pixel {i}: {} against {e}",
                got[i]
            );
        }
    }
}

#[test]
fn the_box_and_its_advance_follow_the_size() {
    assert_eq!(box_advance(0), 3);
    assert_eq!(box_advance(4), 3);
    assert_eq!(box_advance(5), 3);
    assert_eq!(box_advance(6), 4);
    assert_eq!(box_advance(13), 7);
    assert_eq!(box_advance(26), 14);
    let list = FontList::new([Font::parse(crate::tests::MONO).unwrap()]);
    let b = box_glyph(&list, 13);
    // JetBrains Mono's ascent at 13 px is 14: seven tenths is 9; 7 - 2 wide.
    assert_eq!((b.width, b.height, b.left, b.top), (5, 9, 1, -9));
    let b26 = box_glyph(&list, 26);
    assert_eq!((b26.width, b26.height), (12, 18));
    // Its stroke is a twelfth of the size, at least one.
    assert_eq!(b26.coverage[b26.width + 1], 255);
    assert_eq!(b26.coverage[2 * b26.width + 2], 0);
    let empty = FontList::new([]);
    assert_eq!(box_glyph(&empty, 10).height, 7);
    let thin = render_box(2, 2, 5, 0, 0);
    assert!(thin.coverage.iter().all(|c| *c == 255));
    let m = empty.metrics(10);
    assert_eq!((m.ascent, m.descent, m.gap, m.height()), (10, 2, 0, 12));
    assert!(empty.fonts().is_empty());
}

#[test]
fn the_cache_keeps_two_glyphs_per_slot() {
    let list = FontList::new([Font::parse(crate::tests::MONO).unwrap()]);
    let mut cache = GlyphCache::new(1);
    let (sa, ga) = list.lookup('a');
    let (sb, gb) = list.lookup('b');
    let (sc, gc) = list.lookup('c');
    // One slot: with `slot ^ 1` it is the same slot, so each new glyph
    // replaces the last.
    let a = cache.get(&list, sa, ga, 13).clone();
    cache.get(&list, sb, gb, 13);
    cache.get(&list, sa, ga, 13);
    assert_eq!((cache.hits, cache.misses), (0, 3));
    assert_eq!(cache.get(&list, sa, ga, 13), &a);
    assert_eq!(cache.hits, 1);
    // Two slots: both glyphs of a pair stay, a third evicts the older.
    let mut two = GlyphCache::new(2);
    for (s, g) in [(sa, ga), (sb, gb), (sa, ga), (sb, gb)] {
        two.get(&list, s, g, 13);
    }
    assert_eq!((two.hits, two.misses), (2, 2));
    two.get(&list, sc, gc, 13);
    assert_eq!(two.misses, 3);
    // c took a way: of a and b exactly one is still there.
    two.get(&list, sa, ga, 13);
    two.get(&list, sb, gb, 13);
    assert!((4..=5).contains(&two.misses), "{}", two.misses);
    // The same glyph at two sizes, and in two fonts, are two entries.
    let mut big = GlyphCache::new(64);
    big.get(&list, sa, ga, 13);
    big.get(&list, sa, ga, 14);
    big.get(&list, Source::Box, 0, 13);
    assert_eq!(big.misses, 3);
    assert_eq!(GlyphCache::default().slots.len(), CACHE_SLOTS);
    assert_eq!(GlyphCache::new(0).slots.len(), 1);
    assert_eq!(GlyphCache::new(100).slots.len(), 128);
}

#[test]
fn fill_measure_and_cells_draw_what_they_say() {
    let p = &zoraya_tokens::LIGHT;
    let mut pixels = vec![0u32; 20 * 10];
    let mut t = Target {
        pixels: &mut pixels,
        stride: 20,
        clip: Rect::new(0, 0, 20, 10),
    };
    t.fill(Rect::new(2, 3, 4, 2), p.accent);
    assert_eq!(pixels[3 * 20 + 2], p.accent.pixel());
    assert_eq!(pixels[4 * 20 + 5], p.accent.pixel());
    assert_eq!(pixels[5 * 20 + 2], 0);
    assert_eq!(pixels[3 * 20 + 6], 0);
    let mut text = Text::new(FontList::new([
        Font::parse(crate::tests::MONO).unwrap(),
        Font::parse(crate::tests::INTER).unwrap(),
    ]));
    assert_eq!(text.measure("abcd", 10), 24);
    assert_eq!(text.measure("", 10), 0);
    // A glyph of the first font starts at the cell; one from the fallback
    // is centred.
    let cell = CellMetrics::of(&text.fonts, 13);
    assert_eq!(cell.width, 8);
    let mut px = vec![0u32; 8 * 18];
    let mut t = Target {
        pixels: &mut px,
        stride: 8,
        clip: Rect::new(0, 0, 8, 18),
    };
    text.draw_cell(&mut t, cell, (0, 0), '|', 13, (p.term_fg, p.term_bg));
    let lit: Vec<usize> = (0..8)
        .filter(|x| px[9 * 8 + x] != p.term_bg.pixel())
        .collect();
    assert!(
        !lit.is_empty() && lit.iter().all(|x| (2..6).contains(x)),
        "{lit:?}"
    );
    // A wide fallback glyph (an Inter letter JetBrains Mono lacks) is
    // centred on the cell, clipped to it.
    let fallback = (0x0250..0x02FF)
        .filter_map(char::from_u32)
        .find(|c| matches!(text.fonts.lookup(*c).0, Source::Font(1)));
    if let Some(c) = fallback {
        let mut px = vec![0u32; 8 * 18];
        let mut t = Target {
            pixels: &mut px,
            stride: 8,
            clip: Rect::new(0, 0, 8, 18),
        };
        text.draw_cell(&mut t, cell, (0, 0), c, 13, (p.term_fg, p.term_bg));
        assert!(px.iter().any(|v| *v != p.term_bg.pixel()), "{c}");
    }
    let m = text.fonts.metrics(13);
    assert_eq!((m.ascent, m.descent), (14, 4));
}

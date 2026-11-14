use std::string::String;
use std::vec;
use std::vec::Vec;

use super::*;
use zoraya_text::Source as GlyphSource;

/// Serves the system's fonts (`fonts/`, through `zoraya-fonts`) at
/// `/system/fonts/NAME` and records every path asked for.
#[derive(Default)]
struct System {
    asked: Vec<String>,
    refuse: bool,
}

impl Source for System {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        self.asked.push(String::from(path));
        if self.refuse {
            return None;
        }
        let name = path.strip_prefix("/system/fonts/")?;
        zoraya_fonts::FILES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, b)| b.to_vec())
    }
}

fn system() -> Fontset {
    Fontset::load(&mut System::default())
}

/// 12.8 points, NetSurf's default `font_size` of 128 tenths.
const DEFAULT_SIZE: i32 = 128 * 1024 / 10;

/// Every Russian and Ukrainian letter (INV-0326-2).
fn cyrillic() -> Vec<char> {
    let mut v: Vec<char> = ('\u{0410}'..='\u{044F}').collect();
    v.extend(['Ё', 'ё', 'Ґ', 'ґ', 'Є', 'є', 'І', 'і', 'Ї', 'ї']);
    v
}

#[test]
fn the_fonts_are_read_only_as_system_fonts_names_each_once() {
    let mut s = System::default();
    let f = Fontset::load(&mut s);
    assert_eq!(f.loaded(), 4);
    let want: Vec<String> = FILES.iter().map(|n| format!("/system/fonts/{n}")).collect();
    assert_eq!(s.asked, want);
    for p in &s.asked {
        assert!(
            p.starts_with("/system/fonts/") && !p[14..].contains('/'),
            "{p}"
        );
    }
    assert_eq!(DIR, "/system/fonts");
    assert_eq!(path(4), None);
}

#[test]
fn without_any_font_text_is_measured_and_drawn_as_boxes() {
    let mut s = System {
        refuse: true,
        ..System::default()
    };
    let mut f = Fontset::load(&mut s);
    assert_eq!(
        s.asked.len(),
        4,
        "every name is asked for once, and only those"
    );
    assert_eq!(f.loaded(), 0);
    let w = f.width(Face::Sans, 16, "абв".as_bytes());
    assert_eq!(w, 3 * zoraya_text::box_advance(16) as i32);
    let mut ink = 0;
    f.glyphs(Face::Sans, 16, "а".as_bytes(), |g| {
        ink += g.coverage.iter().filter(|c| **c > 0).count()
    });
    assert!(ink > 0);
}

#[test]
fn a_font_that_does_not_parse_or_is_too_large_leaves_its_place_empty() {
    struct Bad;
    impl Source for Bad {
        fn read(&mut self, path: &str) -> Option<Vec<u8>> {
            if path.ends_with("Inter-Regular.ttf") {
                Some(vec![0u8; 64])
            } else if path.ends_with("Inter-Medium.ttf") {
                Some(vec![0u8; MAX_FILE + 1])
            } else {
                Some(zoraya_fonts::MONO_REGULAR.to_vec())
            }
        }
    }
    let mut f = Fontset::load(&mut Bad);
    assert_eq!(f.loaded(), 2);
    // Sans falls back to the monospace font: the cyrillic letters still draw.
    let (src, _) = f.fonts(Face::Sans).lookup('ж');
    assert_eq!(src, GlyphSource::Font(0));
    assert!(f.width(Face::Sans, 16, "ж".as_bytes()) > 0);
}

#[test]
fn every_russian_and_ukrainian_letter_maps_to_a_glyph_in_the_sans_faces() {
    let f = system();
    for face in [Face::Sans, Face::SansBold] {
        let fonts = f.fonts(face);
        for c in cyrillic() {
            let (src, glyph) = fonts.lookup(c);
            assert_eq!(
                src,
                GlyphSource::Font(0),
                "{c} in {face:?} is not in its first font"
            );
            assert!(glyph != 0, "{c} maps to .notdef in {face:?}");
        }
    }
    // The monospace faces map them too (JetBrains Mono, INV-0162-3).
    for face in [Face::Mono, Face::MonoBold] {
        for c in cyrillic() {
            assert_ne!(
                f.fonts(face).lookup(c).0,
                GlyphSource::Box,
                "{c} in {face:?}"
            );
        }
    }
}

/// The coverage of one character's glyph, as plotted.
fn plotted(f: &mut Fontset, face: Face, px: u32, c: char) -> (u32, u32, Vec<u8>) {
    let mut s = String::new();
    s.push(c);
    let mut got = Vec::new();
    f.glyphs(face, px, s.as_bytes(), |g| {
        got.push((g.width, g.height, g.coverage.to_vec()))
    });
    assert_eq!(got.len(), 1, "{c}: one glyph");
    got.remove(0)
}

#[test]
fn cyrillic_draws_non_blank_glyphs_that_are_not_the_box_at_scale_one_and_two() {
    let mut f = system();
    for scale in [1u32, 2] {
        let px = px(DEFAULT_SIZE, dpi(scale));
        assert_eq!(px, 16 * scale, "12.8 pt at 90 dpi times the scale");
        let boxed = zoraya_text::box_glyph(f.fonts(Face::Sans), px);
        for face in [Face::Sans, Face::SansBold] {
            for c in cyrillic() {
                let (w, h, coverage) = plotted(&mut f, face, px, c);
                let ink = coverage.iter().filter(|v| **v > 0).count();
                assert!(ink > 0, "{c} at {px} px in {face:?} is blank");
                assert!(
                    (w as usize, h as usize, &coverage)
                        != (boxed.width, boxed.height, &boxed.coverage),
                    "{c} at {px} px in {face:?} is the box"
                );
            }
        }
        // At twice the scale a letter has more ink: it is drawn bigger, not
        // the same bitmap.
        let (_, h1, _) = plotted(&mut f, Face::Sans, px, 'Ж');
        let (_, h2, _) = plotted(&mut f, Face::Sans, px * 2, 'Ж');
        assert!(h2 >= 2 * h1 - 2, "{h1} then {h2}");
    }
}

#[test]
fn letters_have_their_own_widths_not_one_cell() {
    let mut f = system();
    let i = f.width(Face::Sans, 32, "іііііііііі".as_bytes());
    let zh = f.width(Face::Sans, 32, "жжжжжжжжжж".as_bytes());
    assert!(i > 0 && zh > 2 * i, "і {i}, ж {zh}");
    // ґ є ї and their capitals are as wide as the font says, not boxes.
    let b = zoraya_text::box_advance(32) as i32;
    for c in ['ґ', 'є', 'ї', 'Ґ', 'Є', 'Ї'] {
        let mut s = String::new();
        s.push(c);
        let w = f.width(Face::Sans, 32, s.as_bytes());
        assert!(w > 0 && w != b, "{c}: {w}");
    }
}

#[test]
fn sizes_follow_points_dpi_and_the_scale() {
    assert_eq!(dpi(1), 90);
    assert_eq!(dpi(2), 180);
    assert_eq!(dpi(0), 90);
    assert_eq!(dpi(3), 250);
    assert_eq!(px(12 * 1024, 72), 12);
    assert_eq!(px(12 * 1024, 96), 16);
    assert_eq!(px(DEFAULT_SIZE, 90), 16);
    assert_eq!(px(0, 90), 1);
    assert_eq!(px(-5, 90), 1);
    assert_eq!(px(i32::MAX, 250), zoraya_text::MAX_PX);
    // Rounded to the nearest: 10.5 pixels and a little more is 11.
    assert_eq!(px(7 * 1024 + 1, 108), 11);
    assert_eq!(px(7 * 1024 - 8, 108), 10);
}

#[test]
fn faces_by_family_and_weight() {
    assert_eq!(Face::of(0, 400), Face::Sans);
    assert_eq!(Face::of(1, 400), Face::Sans, "serif draws sans");
    assert_eq!(Face::of(3, 700), Face::SansBold, "cursive bold");
    assert_eq!(Face::of(4, 599), Face::Sans);
    assert_eq!(Face::of(0, 600), Face::SansBold);
    assert_eq!(Face::of(FAMILY_MONOSPACE, 400), Face::Mono);
    assert_eq!(Face::of(FAMILY_MONOSPACE, 900), Face::MonoBold);
    for (i, f) in Face::ALL.iter().enumerate() {
        assert_eq!(f.index(), i);
        assert_eq!(Face::from_index(i as u32), Some(*f));
    }
    assert_eq!(Face::from_index(4), None);
    let f = system();
    // The heavier face draws with Inter Medium first.
    let medium = Font::parse(zoraya_fonts::INTER_MEDIUM).unwrap();
    assert_eq!(
        f.fonts(Face::SansBold).fonts()[0].glyph_count(),
        medium.glyph_count()
    );
    let bold = Font::parse(zoraya_fonts::MONO_BOLD).unwrap();
    assert_eq!(
        f.fonts(Face::MonoBold).fonts()[0].glyph_count(),
        bold.glyph_count()
    );
}

#[test]
fn position_is_the_nearest_boundary() {
    let mut f = system();
    let s = "жіж".as_bytes();
    let whole = f.width(Face::Sans, 32, s);
    assert_eq!(f.position(Face::Sans, 32, s, -10), (0, 0));
    assert_eq!(f.position(Face::Sans, 32, s, 0), (0, 0));
    assert_eq!(f.position(Face::Sans, 32, s, whole + 50), (s.len(), whole));
    let zh = f.width(Face::Sans, 32, "ж".as_bytes());
    let zhi = f.width(Face::Sans, 32, "жі".as_bytes());
    // Just past the first letter: the boundary after it (offset 2: ж is two bytes).
    assert_eq!(f.position(Face::Sans, 32, s, zh + 1), (2, zh));
    assert_eq!(f.position(Face::Sans, 32, s, zhi - 1), (4, zhi));
    // Every answer is a boundary of the text.
    for x in -5..whole + 5 {
        let (o, _) = f.position(Face::Sans, 32, s, x);
        assert!([0, 2, 4, 6].contains(&o), "{x}: {o}");
    }
}

#[test]
fn split_breaks_at_the_last_space_that_fits_else_the_first_after() {
    let mut f = system();
    let s = b"ab cd ef";
    let ab_cd = f.width(Face::Sans, 16, b"ab cd");
    let ab = f.width(Face::Sans, 16, b"ab");
    let whole = f.width(Face::Sans, 16, s);
    // All of it fits: no split.
    assert_eq!(f.split(Face::Sans, 16, s, whole), (8, whole));
    // "ab cd" fits, "ab cd e" does not: at the second space.
    assert_eq!(f.split(Face::Sans, 16, s, ab_cd + 1), (5, ab_cd));
    // Only "ab" fits: at the first space.
    assert_eq!(f.split(Face::Sans, 16, s, ab + 1), (2, ab));
    // Not even "a" fits: the first space after, never offset 0.
    assert_eq!(f.split(Face::Sans, 16, s, 0), (2, ab));
    assert_eq!(f.split(Face::Sans, 16, s, -100), (2, ab));
    // No space: the whole.
    assert_eq!(f.split(Face::Sans, 16, b"abcdef", 1).0, 6);
    // A leading space is not a split point.
    assert_eq!(f.split(Face::Sans, 16, b" abcdef", 1).0, 7);
    // Cyrillic words split the same way, at byte offsets.
    let w = f.width(Face::Sans, 16, "привіт".as_bytes());
    assert_eq!(
        f.split(Face::Sans, 16, "привіт світ".as_bytes(), w + 2),
        (12, w)
    );
}

#[test]
fn bytes_that_are_not_utf8_keep_their_offsets() {
    let d: Vec<(usize, char)> = decode(b"a\xffb\xe2\x82").collect();
    assert_eq!(
        d,
        vec![(0, 'a'), (1, '\u{FFFD}'), (2, 'b'), (3, '\u{FFFD}')]
    );
    let mut f = system();
    let s = b"a\xff\xfeb";
    let w = f.width(Face::Sans, 16, s);
    assert!(w > 0);
    for x in -2..w + 2 {
        let (o, _) = f.position(Face::Sans, 16, s, x);
        assert!(o <= s.len());
        let (o, _) = f.split(Face::Sans, 16, s, x);
        assert!(o > 0 && o <= s.len());
    }
    assert_eq!(f.width(Face::Sans, 16, b""), 0);
    assert_eq!(f.position(Face::Sans, 16, b"", 5), (0, 0));
    assert_eq!(f.split(Face::Sans, 16, b"", 5), (0, 0));
}

#[test]
fn zero_width_characters_take_no_room_and_draw_nothing() {
    let mut f = system();
    let plain = f.width(Face::Sans, 16, b"ab");
    let marked = "a\u{200B}\u{200F}b";
    assert_eq!(f.width(Face::Sans, 16, marked.as_bytes()), plain);
    let mut n = 0;
    f.glyphs(Face::Sans, 16, marked.as_bytes(), |_| n += 1);
    assert_eq!(n, 2);
    // A boundary inside them is at the x of the next letter.
    let a = f.width(Face::Sans, 16, b"a");
    let (o, x) = f.position(Face::Sans, 16, marked.as_bytes(), a);
    assert_eq!(x, a);
    assert!(o == 1 || o == 4 || o == 7, "{o}");
}

#[test]
fn glyphs_are_placed_from_the_pen_and_the_advance_is_the_width() {
    let mut f = system();
    let s = "Їжак".as_bytes();
    let mut placed = Vec::new();
    let advance = f.glyphs(Face::Sans, 24, s, |g| {
        assert_eq!(g.coverage.len(), (g.width * g.height) as usize);
        placed.push((g.x, g.y, g.width, g.height));
    });
    assert_eq!(advance, f.width(Face::Sans, 24, s));
    assert_eq!(placed.len(), 4);
    // Left to right, each glyph above the baseline, none past the advance by much.
    for w in placed.windows(2) {
        assert!(w[1].0 > w[0].0);
    }
    let m = f.metrics(Face::Sans, 24);
    for (x, y, gw, h) in placed {
        assert!(y < 0 && y >= -(m.ascent as i32) - 2, "top {y}");
        assert!(y + (h as i32) <= m.descent as i32 + 1);
        assert!(x >= -2 && x + gw as i32 <= advance + 2);
    }
    // A space is advance with nothing to plot.
    let mut n = 0;
    let w = f.glyphs(Face::Sans, 24, b" ", |_| n += 1);
    assert_eq!(n, 0);
    assert!(w > 0);
}

#[test]
fn any_bytes_and_any_x_are_answered_without_a_panic() {
    let mut f = system();
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..300 {
        let n = (next() % 40) as usize;
        let bytes: Vec<u8> = (0..n).map(|_| next() as u8).collect();
        let face = Face::ALL[(next() % 4) as usize];
        let px = (next() % 80) as u32;
        let x = (next() % 2000) as i32 - 500;
        let w = f.width(face, px, &bytes);
        assert!(w >= 0);
        let (o, _) = f.position(face, px, &bytes, x);
        assert!(o <= bytes.len());
        let (o, _) = f.split(face, px, &bytes, x);
        assert!(o <= bytes.len());
        f.glyphs(face, px, &bytes, |g| {
            assert_eq!(g.coverage.len(), (g.width * g.height) as usize)
        });
    }
}

/// The calls as they were before the walk (9f-text): the whole text shaped
/// by `zoraya-text`'s `Simple`, every boundary kept. The walk must give the
/// same answers.
mod reference {
    use super::*;
    use zoraya_text::{Positioned, Shaper, Simple};

    pub struct Line {
        pub bounds: Vec<(usize, i32)>,
        pub spaces: Vec<usize>,
        pub glyphs: Vec<Positioned>,
        pub advance: i32,
    }

    pub fn line(f: &Fontset, face: Face, px: u32, bytes: &[u8]) -> Line {
        let mut shaped = String::new();
        let mut chars: Vec<(usize, Option<usize>, bool)> = Vec::new();
        let mut spaces = Vec::new();
        let mut n = 0usize;
        for (offset, c) in decode(bytes) {
            if c == ' ' && offset > 0 {
                spaces.push(offset);
            }
            if is_invisible(c) {
                chars.push((offset, None, false));
                continue;
            }
            shaped.push(c);
            chars.push((offset, Some(n), zoraya_text::is_combining(c)));
            n += 1;
        }
        let mut glyphs = Vec::new();
        let advance = Simple.shape(f.fonts(face), &shaped, px, &mut glyphs);
        let mut bounds = vec![(bytes.len(), advance); chars.len() + 1];
        let mut next = advance;
        for (k, (offset, glyph, mark)) in chars.iter().enumerate().rev() {
            if let (Some(j), false) = (glyph, mark)
                && let Some(g) = glyphs.get(*j)
            {
                next = g.x.min(next);
            }
            bounds[k] = (*offset, next);
        }
        Line {
            bounds,
            spaces,
            glyphs,
            advance,
        }
    }

    pub fn position(l: &Line, x: i32) -> (usize, i32) {
        let mut best = (0usize, 0i32);
        let mut distance = i64::MAX;
        for &(o, bx) in &l.bounds {
            let d = (bx as i64 - x as i64).abs();
            if d < distance {
                distance = d;
                best = (o, bx);
            }
        }
        best
    }

    pub fn split(l: &Line, len: usize, x: i32) -> (usize, i32) {
        if l.advance <= x {
            return (len, l.advance);
        }
        let x_of = |offset: usize| {
            l.bounds
                .iter()
                .find(|(o, _)| *o == offset)
                .map_or(l.advance, |(_, bx)| *bx)
        };
        let over = l
            .bounds
            .windows(2)
            .find(|w| w[1].1 > x)
            .map_or(len, |w| w[0].0);
        if let Some(&s) = l.spaces.iter().rev().find(|&&s| s <= over) {
            return (s, x_of(s));
        }
        if let Some(&s) = l.spaces.iter().find(|&&s| s > over) {
            return (s, x_of(s));
        }
        (len, l.advance)
    }
}

#[test]
fn the_walk_answers_as_shaping_the_whole_text_did() {
    let mut f = system();
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let pieces: [&[u8]; 12] = [
        b"a",
        b"W",
        b"V",
        b" ",
        "ж".as_bytes(),
        "Ї".as_bytes(),
        "\u{0301}".as_bytes(),
        "\u{200B}".as_bytes(),
        b"\xff",
        b"\xe2\x82",
        "\u{4E2D}".as_bytes(),
        b"To",
    ];
    for _ in 0..600 {
        let n = (next() % 24) as usize;
        let mut bytes = Vec::new();
        for _ in 0..n {
            bytes.extend_from_slice(pieces[(next() % 12) as usize]);
        }
        let face = Face::ALL[(next() % 4) as usize];
        let px = (next() % 80) as u32;
        let l = reference::line(&f, face, px, &bytes);
        assert_eq!(f.width(face, px, &bytes), l.advance, "{bytes:?}");
        for _ in 0..6 {
            let x = (next() % (l.advance as u64 + 40)) as i32 - 20;
            assert_eq!(
                f.position(face, px, &bytes, x),
                reference::position(&l, x),
                "position {bytes:?} {face:?} {px} {x}"
            );
            assert_eq!(
                f.split(face, px, &bytes, x),
                reference::split(&l, bytes.len(), x),
                "split {bytes:?} {face:?} {px} {x}"
            );
        }
        let mut xs = Vec::new();
        f.glyphs(face, px, &bytes, |g| xs.push(g.x));
        let mut want = Vec::new();
        let px_c = zoraya_text::clamp_px(px);
        let fonts = f.fonts(face).clone();
        let mut cache = zoraya_text::GlyphCache::new(4);
        for g in &l.glyphs {
            let b = cache.get(&fonts, g.source, g.glyph, px_c);
            if b.width != 0 && b.height != 0 {
                want.push(g.x + b.left);
            }
        }
        assert_eq!(xs, want, "glyphs {bytes:?}");
    }
}

/// T4 of the 9f-text review: NetSurf breaks a paragraph by calling `split`
/// on what is left of the box for every line. Each call shapes only up to
/// the line's end and one word, so the work is linear in the text; shaping
/// the whole rest each time, as before, was quadratic (about 1,800 lines
/// over 120,000 characters here: some 10^8 characters, 1.1 s for 300 lines
/// in a host release build).
#[test]
fn breaking_a_long_paragraph_into_lines_is_linear() {
    let mut f = system();
    let text = "слово ".repeat(20_000);
    let chars = text.chars().count() as u64;
    let px = px(DEFAULT_SIZE, 90);
    let before = f.shaped();
    let whole = f.width(Face::Sans, px, text.as_bytes());
    assert!(whole > 600);
    let mut rest = text.as_bytes();
    let mut lines = 0u64;
    while !rest.is_empty() {
        let (o, w) = f.split(Face::Sans, px, rest, 600);
        assert!(o > 0 && o <= rest.len());
        assert!(w <= 600 || !rest[..o].contains(&b' '), "line {lines}: {w}");
        rest = &rest[(o + 1).min(rest.len())..];
        lines += 1;
    }
    assert!(lines > 1_000, "{lines} lines");
    let work = f.shaped() - before;
    // The whole once, then each line and one word past it: about twice the
    // characters. Quadratic would be about 60,000 times them.
    assert!(work <= 4 * chars, "{work} characters shaped for {chars}");
}

/// T3 of the 9f-text review: a page chooses the size, and a glyph at 512
/// pixels holds about 100 KB of coverage. Above `CACHE_MAX_PX` a face keeps
/// only `LARGE_SLOTS` glyphs, and below it a cache past `CACHE_BYTES` starts
/// again, so drawing many glyphs at any size stays bounded.
#[test]
fn the_glyph_caches_stay_bounded_whatever_the_size() {
    let mut f = system();
    let many: String = ('\u{0021}'..='\u{007E}')
        .chain('\u{0391}'..='\u{03C9}')
        .chain('\u{0400}'..='\u{045F}')
        .collect();
    let mut largest = 0usize;
    f.glyphs(Face::Sans, zoraya_text::MAX_PX, many.as_bytes(), |g| {
        largest = largest.max(g.coverage.len());
    });
    assert!(largest > 50_000, "a glyph at 512 pixels: {largest} bytes");
    assert!(
        f.cache_bytes() <= LARGE_SLOTS * largest,
        "{} bytes cached at 512 pixels",
        f.cache_bytes()
    );
    // At the largest size kept, the cache empties itself when full.
    let mut big = 0usize;
    for face in [Face::Sans, Face::Mono] {
        f.glyphs(face, CACHE_MAX_PX, many.as_bytes(), |g| {
            big = big.max(g.coverage.len());
        });
    }
    assert!(
        f.cache_bytes() <= 4 * (CACHE_BYTES + big) + 4 * LARGE_SLOTS * largest,
        "{} bytes cached",
        f.cache_bytes()
    );
}

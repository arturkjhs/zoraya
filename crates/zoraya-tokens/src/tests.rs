use std::collections::BTreeSet;
use std::string::String;
use std::vec::Vec;

use crate::parse::{Set, lines, parse_color, parse_int, parse_milli};
use crate::*;

const MOCKUP: &str = include_str!("../../../docs/research/desktop/mockups/clear-field/tokens.txt");

fn names(text: &str, set: Set) -> BTreeSet<String> {
    lines(text)
        .map(|l| l.expect("every line reads"))
        .filter(|(s, _, _)| *s == set)
        .map(|(_, n, _)| n.into())
        .collect()
}

#[test]
fn the_file_is_the_chosen_direction_unchanged() {
    // ADR-0161: tokens.txt is clear-field's file, so the mockups the
    // architect chose and the system cannot drift.
    assert_eq!(SOURCE, MOCKUP);
}

#[test]
fn every_line_reads_and_no_name_is_defined_twice() {
    let mut seen = BTreeSet::new();
    for line in lines(SOURCE) {
        let (set, name, _) = line.expect("a token line");
        assert!(seen.insert((set as u8, String::from(name))), "{name} twice");
    }
}

#[test]
fn light_and_dark_name_the_same_colours_and_the_palette_reads_them_all() {
    // INV-0161-2.
    let light = names(SOURCE, Set::Light);
    let dark = names(SOURCE, Set::Dark);
    assert_eq!(light, dark);
    let read: BTreeSet<String> = COLOR_NAMES.iter().map(|n| String::from(*n)).collect();
    assert_eq!(read, light);
    for (set, name, value) in lines(SOURCE).map(Result::unwrap) {
        if set != Set::Plain {
            assert!(parse_color(value.as_bytes()).is_some(), "{name} = {value}");
        }
    }
}

#[test]
fn the_palette_holds_the_files_values() {
    assert_eq!(LIGHT.bg_window.pixel(), 0xf7f7f8);
    assert_eq!(DARK.bg_window.pixel(), 0x26282d);
    assert_eq!(LIGHT.room_untrusted.pixel(), 0xc2641c);
    assert_eq!(DARK.term_white.pixel(), 0xe7e8ea);
    assert_eq!(Appearance::Dark.palette(), &DARK);
    assert_eq!(text::MONO.physical(Scale::ONE), 13);
    assert_eq!(text::MONO.physical(Scale::TWO), 26);
    assert_eq!(space::PADDING_TERMINAL.physical(Scale::ONE), 14);
    assert_eq!(space::ROOM_UNTRUSTED_EDGE.physical(Scale::ONE), 2);
    assert_eq!(shadow::WINDOW_ALPHA, 96);
    assert_eq!(motion::MEDIUM.millis(), 200);
    assert_eq!(motion::MEDIUM.nanos(), 200_000_000);
    assert_eq!(
        motion::STANDARD,
        Ease {
            x1: 200,
            y1: 0,
            x2: 0,
            y2: 1000
        }
    );
    assert_eq!(fonts::MONO, "JetBrainsMono-Regular.ttf");
    assert_eq!(NAME, "clear-field");
    assert_eq!(shape::ROOM_STYLE, RoomStyle::Ground);
    assert_eq!(shape::WALLPAPER, Wallpaper::Rooms);
    assert_eq!(shape::CORNER, CornerShape::Smooth);
    assert_eq!(shape::FRAME_BUTTONS, FrameButtons::Glyphs);
    const { assert!(shape::DOCK && shape::CHOOSER_SHEET && !shape::FRAME_SEPARATOR) };
}

/// The plain names ADR-0161's vocabulary lists.
const VOCABULARY: &[&str] = &[
    "name",
    "appearance",
    "concept",
    "screens",
    "screens.2x",
    "font.ui",
    "font.ui.medium",
    "font.mono",
    "font.mono.bold",
    "text.caption",
    "text.body",
    "text.heading",
    "text.title",
    "text.large",
    "text.mono",
    "space.unit",
    "padding.control",
    "padding.window",
    "padding.terminal",
    "gap.controls",
    "control.height",
    "radius.control",
    "radius.window",
    "radius.popup",
    "radius.shape",
    "frame.height",
    "frame.border",
    "frame.border.focused",
    "frame.separator",
    "frame.grip",
    "frame.buttons",
    "shadow.window.blur",
    "shadow.window.offset",
    "shadow.window.alpha",
    "shadow.window.layers",
    "shadow.window.unfocused.alpha",
    "shadow.popup.blur",
    "shadow.popup.offset",
    "shadow.popup.alpha",
    "desktop.layout",
    "panel.height",
    "panel.position",
    "bar.height",
    "dock.icon",
    "dock.radius",
    "dock.padding",
    "panel.alpha",
    "blur.panel",
    "wallpaper",
    "chooser.style",
    "settings.style",
    "room.style",
    "room.edge",
    "room.untrusted.edge",
    "field.padding",
    "field.gap",
    "room.hue-distance",
    "glow.alpha",
    "glow.halo",
    "glow.blur",
    "motion.short",
    "motion.medium",
    "motion.long",
    "ease.standard",
    "ease.decelerate",
];

#[test]
fn the_plain_names_are_the_vocabulary() {
    let want: BTreeSet<String> = VOCABULARY.iter().map(|n| String::from(*n)).collect();
    assert_eq!(names(SOURCE, Set::Plain), want);
}

/// WCAG 2.1 relative luminance, in millionths, and the contrast ratio of
/// two colours times 100. Floating point is fine in a test.
fn luminance(c: Color) -> f64 {
    let ch = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

fn contrast(a: Color, b: Color) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn text_on_its_backgrounds_has_contrast_of_4_5_in_both_sets() {
    // INV-0161-3, the pairs the code composes from tokens alone; the
    // composed backgrounds of the concept (fields, tints, the material)
    // are the mockup tool's clear_field_text_contrast.
    for p in [&LIGHT, &DARK] {
        let pairs = [
            ("fg.text on bg.window", p.fg_text, p.bg_window),
            ("fg.text on bg.view", p.fg_text, p.bg_view),
            ("fg.text on bg.control", p.fg_text, p.bg_control),
            ("fg.text on bg.frame", p.fg_text, p.bg_frame),
            ("fg.text on bg.frame.focused", p.fg_text, p.bg_frame_focused),
            ("fg.text.on-accent on accent", p.fg_text_on_accent, p.accent),
            (
                "fg.text.on-selection on bg.selection",
                p.fg_text_on_selection,
                p.bg_selection,
            ),
            (
                "fg.text.secondary on bg.window",
                p.fg_text_secondary,
                p.bg_window,
            ),
            ("term.fg on term.bg", p.term_fg, p.term_bg),
        ];
        for (what, fg, bg) in pairs {
            let ratio = contrast(fg, bg);
            assert!(ratio >= 4.5, "{what}: {ratio:.2}");
        }
    }
}

/// HSL hue in degrees and saturation from 0 to 1.
fn hue_saturation(c: Color) -> (f64, f64) {
    let (r, g, b) = (
        c.r() as f64 / 255.0,
        c.g() as f64 / 255.0,
        c.b() as f64 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    ((h + 360.0) % 360.0, s)
}

#[test]
fn colour_means_a_room() {
    // INV-0161-5: no saturated colour outside the room, button, terminal
    // and status tokens.
    for (set, p) in [("light", &LIGHT), ("dark", &DARK)] {
        for (name, set_of, value) in lines(SOURCE).map(Result::unwrap).map(|(s, n, v)| (n, s, v)) {
            if set_of == Set::Plain {
                continue;
            }
            let exempt = name.starts_with("room.")
                || name.starts_with("button.")
                || name.starts_with("term.")
                || matches!(name, "success" | "warning" | "error");
            if exempt {
                continue;
            }
            let c = parse_color(value.as_bytes()).unwrap();
            let (_, s) = hue_saturation(c);
            assert!(s <= 0.4, "{set} {name} = {value}: saturation {s:.2}");
        }
        let _ = p;
    }
}

#[test]
fn the_untrusted_hue_is_reserved() {
    // INV-0161-6.
    for p in [&LIGHT, &DARK] {
        let (untrusted, _) = hue_saturation(p.room_untrusted);
        for room in [p.room_1, p.room_2, p.room_3, p.room_4] {
            let (h, _) = hue_saturation(room);
            let d = (h - untrusted).abs();
            let d = d.min(360.0 - d);
            assert!(
                d >= shape::ROOM_HUE_DISTANCE as f64,
                "{room:?}: {d:.1} degrees"
            );
        }
    }
}

#[test]
fn logical_arithmetic_keeps_the_type_and_saturates() {
    let a = space::UNIT;
    assert_eq!((a + a).physical(Scale::ONE), 8);
    assert_eq!((a - a.times(2)), Logical::ZERO);
    assert_eq!(a.times(3).divided(2).physical(Scale::TWO), 12);
    assert_eq!(a.divided(0), Logical::ZERO);
    assert_eq!(a.max(a.times(2)), a.times(2));
    assert_eq!(Scale::new(2), Some(Scale::TWO));
    assert_eq!(Scale::new(3), None);
    assert_eq!(Scale::TWO.logical(27).physical(Scale::ONE), 13);
    assert_eq!(space::UNIT.times(2) - space::UNIT, space::UNIT);
    assert_eq!((Scale::ONE.factor(), Scale::TWO.factor()), (1, 2));
}

#[test]
fn blend_is_exact_at_the_ends_and_rounds_between() {
    let a = LIGHT.bg_window;
    let b = LIGHT.fg_text;
    assert_eq!(a.blend(b, 0), a);
    assert_eq!(a.blend(b, 255), b);
    let mid = Color(0).blend(Color(0x00ff_ffff), 128);
    assert_eq!(mid.pixel(), 0x808080);
    assert_eq!(Color::of_pixel(0xff12_3456).pixel(), 0x123456);
}

#[test]
fn the_value_readers_refuse_what_is_not_their_form() {
    assert_eq!(parse_int(b"42"), Some(42));
    assert_eq!(parse_int(b""), None);
    assert_eq!(parse_int(b"4x"), None);
    assert_eq!(parse_int(b"1234567890"), None);
    assert_eq!(parse_int(b"123456789"), Some(123_456_789));
    assert_eq!(parse_color(b"#00000F").map(Color::pixel), Some(0x0f));
    assert_eq!(parse_milli(b"12.5"), Some(12_500));
    assert_eq!(parse_milli(b"123456"), Some(123_456_000));
    assert_eq!(parse_milli(b"1234567"), None);
    assert_eq!(parse_color(b"#a0B0c0").map(Color::pixel), Some(0xa0b0c0));
    assert_eq!(parse_color(b"#a0b0c"), None);
    assert_eq!(parse_color(b"a0b0c0f"), None);
    assert_eq!(parse_color(b"#g0b0c0"), None);
    assert_eq!(parse_milli(b"0.2"), Some(200));
    assert_eq!(parse_milli(b"1"), Some(1000));
    assert_eq!(parse_milli(b"0.125"), Some(125));
    assert_eq!(parse_milli(b"0.1255"), None);
    assert_eq!(parse_milli(b".5"), None);
    assert_eq!(parse_milli(b"1."), None);
    assert_eq!(parse_milli(b"1,5"), None);
}

#[test]
fn comments_and_prefixes_read_as_the_mockup_tool_reads_them() {
    let text = "# a comment\n a = 1 # note\nlight: c = #102030\ndark: c = #405060 \n\nbad line\nb = \n  #x=1\nd = 1 #\ne =\t1\n";
    let got: Vec<_> = lines(text).collect();
    assert_eq!(
        got,
        [
            Ok((Set::Plain, "a", "1")),
            Ok((Set::Light, "c", "#102030")),
            Ok((Set::Dark, "c", "#405060")),
            Err(6),
            Err(7),
            Ok((Set::Plain, "d", "1")),
            Ok((Set::Plain, "e", "1")),
        ]
    );
}

#[test]
fn a_line_at_the_end_without_a_newline_is_found() {
    assert_eq!(crate::parse::find(b"a = 1", Set::Plain, b"a"), Some((4, 5)));
    assert_eq!(crate::parse::find(b"a = 1", Set::Light, b"a"), None);
}

#[test]
#[should_panic(expected = "fewer words")]
fn reading_a_word_past_the_last_panics() {
    let _ = crate::parse::word("ease.standard", 4);
}

#[test]
fn random_text_never_panics_the_line_reader() {
    // Deterministic: a fixed xorshift seed.
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let alphabet = b"ab.=#: \n\tlight:dark:\xd0\x96#ff00ff12";
    for _ in 0..2000 {
        let mut s = Vec::new();
        for _ in 0..(x % 64) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            s.push(alphabet[(x % alphabet.len() as u64) as usize]);
        }
        let text = String::from_utf8_lossy(&s);
        for line in lines(&text) {
            let _ = line;
        }
    }
}

#[test]
fn room_slots_name_their_colours() {
    for p in [&crate::LIGHT, &crate::DARK] {
        assert_eq!(
            [p.room(1), p.room(2), p.room(3), p.room(4), p.room(0), p.room(5)],
            [p.room_1, p.room_2, p.room_3, p.room_4, p.room_untrusted, p.room_untrusted]
        );
    }
}

//! Key codes to text under one layout, US (ADR-0092). The compositor holds
//! the only instance: it tracks the modifiers and gives a focused client the
//! code, the modifiers and the character the key produces, if any.
//!
//! Codes are Linux evdev codes, as virtio-input sends them.

#![no_std]
#![forbid(unsafe_code)]

/// evdev codes the layout or the compositor name.
pub mod code {
    pub const ESC: u32 = 1;
    pub const BACKSPACE: u32 = 14;
    pub const TAB: u32 = 15;
    pub const ENTER: u32 = 28;
    pub const LEFT_CTRL: u32 = 29;
    pub const LEFT_SHIFT: u32 = 42;
    pub const RIGHT_SHIFT: u32 = 54;
    pub const LEFT_ALT: u32 = 56;
    pub const SPACE: u32 = 57;
    pub const CAPS_LOCK: u32 = 58;
    pub const F12: u32 = 88;
    pub const W: u32 = 17;
    pub const R: u32 = 19;
    pub const M: u32 = 50;
    pub const SLASH: u32 = 53;
    /// The system key of ADR-0160 (Super; the Command keys on a Mac
    /// keyboard under QEMU).
    pub const LEFT_META: u32 = 125;
    pub const RIGHT_META: u32 = 126;
    pub const RIGHT_CTRL: u32 = 97;
    pub const RIGHT_ALT: u32 = 100;
    pub const HOME: u32 = 102;
    pub const UP: u32 = 103;
    pub const PAGE_UP: u32 = 104;
    pub const LEFT: u32 = 105;
    pub const RIGHT: u32 = 106;
    pub const END: u32 = 107;
    pub const DOWN: u32 = 108;
    pub const PAGE_DOWN: u32 = 109;
    pub const INSERT: u32 = 110;
    pub const DELETE: u32 = 111;
    pub const BTN_LEFT: u32 = 0x110;
    pub const BTN_RIGHT: u32 = 0x111;
    pub const BTN_MIDDLE: u32 = 0x112;
}

/// Modifier bits, as `zoraya-proto-gui` carries them.
pub const SHIFT: u32 = 1;
pub const CTRL: u32 = 2;
pub const ALT: u32 = 4;
pub const CAPS: u32 = 8;

pub mod layouts;

pub use layouts::{LAYOUTS, Layout, Out, by_name};

/// A dead key's accent (ADR-0163).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    Acute,
    Caron,
    Ring,
    Diaeresis,
    Circumflex,
    Grave,
    Tilde,
    Breve,
    Ogonek,
    Dot,
    DoubleAcute,
    Cedilla,
    Macron,
}

impl Accent {
    pub const ALL: [Accent; 13] = [
        Accent::Acute,
        Accent::Caron,
        Accent::Ring,
        Accent::Diaeresis,
        Accent::Circumflex,
        Accent::Grave,
        Accent::Tilde,
        Accent::Breve,
        Accent::Ogonek,
        Accent::Dot,
        Accent::DoubleAcute,
        Accent::Cedilla,
        Accent::Macron,
    ];

    /// The spacing character a dead key followed by Space gives.
    pub const fn spacing(self) -> char {
        match self {
            Accent::Acute => '´',
            Accent::Caron => 'ˇ',
            Accent::Ring => '˚',
            Accent::Diaeresis => '¨',
            Accent::Circumflex => '^',
            Accent::Grave => '`',
            Accent::Tilde => '~',
            Accent::Breve => '˘',
            Accent::Ogonek => '˛',
            Accent::Dot => '˙',
            Accent::DoubleAcute => '˝',
            Accent::Cedilla => '¸',
            Accent::Macron => '¯',
        }
    }

    /// The bases this accent composes with and their letters, lower case
    /// then upper case, over Latin-1 and Latin Extended-A.
    const fn pairs(self) -> &'static str {
        match self {
            Accent::Acute => "aáeéiíoóuúyýcćnńsśzźrŕlĺAÁEÉIÍOÓUÚYÝCĆNŃSŚZŹRŔLĹ",
            Accent::Caron => "cčdďeěnňrřsštťzžlľCČDĎEĚNŇRŘSŠTŤZŽLĽ",
            Accent::Ring => "uůaåUŮAÅ",
            Accent::Diaeresis => "aäeëiïoöuüyÿAÄEËIÏOÖUÜYŸ",
            Accent::Circumflex => "aâeêiîoôuûcĉgĝhĥjĵsŝwŵyŷAÂEÊIÎOÔUÛCĈGĜHĤJĴSŜWŴYŶ",
            Accent::Grave => "aàeèiìoòuùAÀEÈIÌOÒUÙ",
            Accent::Tilde => "aãnñoõiĩuũAÃNÑOÕIĨUŨ",
            Accent::Breve => "aăgğuŭAĂGĞUŬ",
            Accent::Ogonek => "aąeęiįuųAĄEĘIĮUŲ",
            Accent::Dot => "zżeėgġcċIİZŻEĖGĠCĊ",
            Accent::DoubleAcute => "oőuűOŐUŰ",
            Accent::Cedilla => "cçsştţgģkķlļnņrŗCÇSŞTŢGĢKĶLĻNŅRŖ",
            Accent::Macron => "aāeēiīoōuūAĀEĒIĪOŌUŪ",
        }
    }

    /// The letter this accent makes with `base`, if any.
    pub fn compose(self, base: char) -> Option<char> {
        let mut chars = self.pairs().chars();
        while let (Some(b), Some(l)) = (chars.next(), chars.next()) {
            if b == base {
                return Some(l);
            }
        }
        None
    }
}

/// The US positions for Ctrl: a control character comes from the key's
/// Latin letter whatever the layout, as on every desktop.
fn us_plain(code: u32) -> Option<char> {
    match layouts::US.levels(code)[0] {
        Out::Char(c) => Some(c),
        _ => None,
    }
}

/// One key as a client gets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: u32,
    pub pressed: bool,
    pub modifiers: u32,
    pub text: Option<char>,
}

/// The active layout, the modifier state and the dead-key state.
#[derive(Debug, Default)]
pub struct Keymap {
    shift: u8,
    ctrl: u8,
    alt: u8,
    /// The right Alt as AltGr, in a layout that has a third level.
    altgr: bool,
    caps: bool,
    layout: usize,
    dead: Option<Accent>,
}

fn side(held: &mut u8, bit: u8, pressed: bool) {
    if pressed {
        *held |= bit;
    } else {
        *held &= !bit;
    }
}

impl Keymap {
    pub const fn new() -> Keymap {
        Keymap {
            shift: 0,
            ctrl: 0,
            alt: 0,
            altgr: false,
            caps: false,
            layout: 0,
            dead: None,
        }
    }

    pub fn modifiers(&self) -> u32 {
        let mut m = 0;
        if self.shift != 0 {
            m |= SHIFT;
        }
        if self.ctrl != 0 {
            m |= CTRL;
        }
        if self.alt != 0 {
            m |= ALT;
        }
        if self.caps {
            m |= CAPS;
        }
        m
    }

    /// The active layout.
    pub fn layout(&self) -> &'static Layout {
        &LAYOUTS[self.layout % LAYOUTS.len()]
    }

    pub fn layout_index(&self) -> usize {
        self.layout
    }

    /// Makes `index` of [`LAYOUTS`] active; a switch clears the dead key
    /// and nothing else (ADR-0163: held modifiers stay held).
    pub fn set_layout(&mut self, index: usize) {
        if index < LAYOUTS.len() {
            self.layout = index;
            self.dead = None;
            if !self.layout().has_altgr() && self.altgr {
                // The right Alt held across the switch is Alt again.
                self.altgr = false;
                self.alt |= 2;
            }
        }
    }

    /// The next of `enabled` (indices of [`LAYOUTS`]) after the active
    /// one, or the one before with `back`; the first if the active one is
    /// not among them.
    pub fn switch(&mut self, enabled: &[usize], back: bool) {
        if enabled.is_empty() {
            return;
        }
        let next = match enabled.iter().position(|i| *i == self.layout) {
            Some(at) if back => enabled[(at + enabled.len() - 1) % enabled.len()],
            Some(at) => enabled[(at + 1) % enabled.len()],
            None => enabled[0],
        };
        self.set_layout(next);
    }

    /// Forgets a dead key typed but not completed (on a focus change).
    pub fn clear_dead(&mut self) {
        self.dead = None;
    }

    /// Takes one key event: `value` 0 released, 1 pressed, 2 repeated.
    pub fn key(&mut self, code: u32, value: u32) -> Key {
        let pressed = value != 0;
        match code {
            code::LEFT_SHIFT => side(&mut self.shift, 1, pressed),
            code::RIGHT_SHIFT => side(&mut self.shift, 2, pressed),
            code::LEFT_CTRL => side(&mut self.ctrl, 1, pressed),
            code::RIGHT_CTRL => side(&mut self.ctrl, 2, pressed),
            code::LEFT_ALT => side(&mut self.alt, 1, pressed),
            code::RIGHT_ALT if self.layout().has_altgr() => self.altgr = pressed,
            code::RIGHT_ALT => side(&mut self.alt, 2, pressed),
            code::CAPS_LOCK if value == 1 => self.caps = !self.caps,
            _ => {}
        }
        if !pressed && code == code::RIGHT_ALT {
            // Released whichever role it had.
            self.altgr = false;
            self.alt &= !2;
        }
        let modifiers = self.modifiers();
        let text = if pressed { self.text(code) } else { None };
        Key {
            code,
            pressed,
            modifiers,
            text,
        }
    }

    fn text(&mut self, code: u32) -> Option<char> {
        if self.ctrl != 0 {
            if code == code::SPACE {
                return Some('\0');
            }
            // Ctrl with a letter, or with the punctuation of 0x1b..0x1f, by
            // the key's US position.
            return match us_plain(code)? {
                c @ 'a'..='z' => Some(char::from(c as u8 - b'a' + 1)),
                '[' => Some('\u{1b}'),
                '\\' => Some('\u{1c}'),
                ']' => Some('\u{1d}'),
                _ => None,
            };
        }
        if code == code::SPACE {
            // A dead key then Space: the spacing accent.
            return Some(self.dead.take().map_or(' ', Accent::spacing));
        }
        let levels = self.layout().levels(code);
        if levels == [Out::None; 4] {
            return None;
        }
        // Caps Lock inverts Shift for keys whose two levels are a case pair.
        let pair = match (levels[0], levels[1]) {
            (Out::Char(a), Out::Char(b)) => a != b && a.to_uppercase().eq(core::iter::once(b)),
            _ => false,
        };
        let shifted = (self.shift != 0) ^ (pair && self.caps);
        let level = usize::from(shifted) + if self.altgr { 2 } else { 0 };
        let out = match levels[level] {
            // A level with nothing falls back to the same level without
            // AltGr, as XKB's types do for keys of two levels.
            Out::None if level >= 2 => levels[level - 2],
            o => o,
        };
        match out {
            Out::Dead(a) => {
                // A second dead key replaces the first.
                self.dead = Some(a);
                None
            }
            Out::Char(ch) => match self.dead.take() {
                // The composed letter; else the key as it is, losing
                // nothing typed (ADR-0163).
                Some(a) => Some(a.compose(ch).unwrap_or(ch)),
                None => Some(ch),
            },
            Out::None => None,
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(map: &mut Keymap, code: u32) -> Option<char> {
        let k = map.key(code, 1);
        map.key(code, 0);
        k.text
    }

    #[test]
    fn letters_digits_and_shift() {
        let mut m = Keymap::new();
        assert_eq!(typed(&mut m, 30), Some('a'));
        assert_eq!(typed(&mut m, 2), Some('1'));
        assert_eq!(typed(&mut m, code::SPACE), Some(' '));
        m.key(code::LEFT_SHIFT, 1);
        assert_eq!(m.modifiers(), SHIFT);
        assert_eq!(typed(&mut m, 30), Some('A'));
        assert_eq!(typed(&mut m, 2), Some('!'));
        assert_eq!(typed(&mut m, 53), Some('?'));
        m.key(code::RIGHT_SHIFT, 1);
        m.key(code::LEFT_SHIFT, 0);
        assert_eq!(typed(&mut m, 30), Some('A'), "the other shift still held");
        m.key(code::RIGHT_SHIFT, 0);
        assert_eq!(typed(&mut m, 30), Some('a'));
    }

    #[test]
    fn caps_lock_toggles_letters_only() {
        let mut m = Keymap::new();
        m.key(code::CAPS_LOCK, 1);
        m.key(code::CAPS_LOCK, 2);
        m.key(code::CAPS_LOCK, 0);
        assert_eq!(m.modifiers(), CAPS);
        assert_eq!(typed(&mut m, 30), Some('A'));
        assert_eq!(typed(&mut m, 2), Some('1'));
        m.key(code::LEFT_SHIFT, 1);
        assert_eq!(typed(&mut m, 30), Some('a'));
        assert_eq!(typed(&mut m, 2), Some('!'));
        m.key(code::LEFT_SHIFT, 0);
        m.key(code::CAPS_LOCK, 1);
        assert_eq!(typed(&mut m, 30), Some('a'));
    }

    #[test]
    fn ctrl_gives_control_characters() {
        let mut m = Keymap::new();
        m.key(code::RIGHT_CTRL, 1);
        assert_eq!(typed(&mut m, 46), Some('\u{3}'));
        assert_eq!(typed(&mut m, 30), Some('\u{1}'));
        assert_eq!(typed(&mut m, 44), Some('\u{1a}'));
        assert_eq!(typed(&mut m, 43), Some('\u{1c}'));
        assert_eq!(typed(&mut m, 2), None);
        m.key(code::RIGHT_CTRL, 0);
        assert_eq!(typed(&mut m, 46), Some('c'));
    }

    /// Holes cargo-mutants found (TASK-14 review).
    #[test]
    fn every_modifier_and_control_punctuation() {
        let mut m = Keymap::new();
        m.key(code::CAPS_LOCK, 0);
        assert_eq!(m.modifiers(), 0, "a release does not toggle");
        m.key(code::LEFT_CTRL, 1);
        assert_eq!(m.modifiers(), CTRL);
        assert_eq!(typed(&mut m, 46), Some('\u{3}'));
        assert_eq!(typed(&mut m, 26), Some('\u{1b}'));
        assert_eq!(typed(&mut m, 27), Some('\u{1d}'));
        m.key(code::LEFT_CTRL, 0);
        m.key(code::LEFT_ALT, 1);
        assert_eq!(m.modifiers(), ALT);
        m.key(code::LEFT_ALT, 0);
        m.key(code::RIGHT_ALT, 1);
        assert_eq!(m.modifiers(), ALT);
    }

    #[test]
    fn released_keys_and_keys_without_text() {
        let mut m = Keymap::new();
        assert_eq!(m.key(30, 0).text, None);
        assert_eq!(m.key(30, 2).text, Some('a'));
        for c in [code::ENTER, code::ESC, code::UP, code::F12, code::BTN_LEFT] {
            assert_eq!(typed(&mut m, c), None);
        }
    }

    /// INV-0092-3: every code, value and modifier state gives a key, and
    /// text only when pressed or repeated, and only ASCII.
    #[test]
    fn translation_is_total() {
        let held = [
            code::LEFT_SHIFT,
            code::RIGHT_SHIFT,
            code::LEFT_CTRL,
            code::RIGHT_CTRL,
            code::LEFT_ALT,
            code::RIGHT_ALT,
            code::CAPS_LOCK,
        ];
        for state in 0..1u32 << held.len() {
            let mut m = Keymap::new();
            for (i, c) in held.iter().enumerate() {
                if state & 1 << i != 0 {
                    m.key(*c, 1);
                }
            }
            for code in 0..0x300 {
                for value in 0..=2 {
                    let before = m.modifiers();
                    let k = m.key(code, value);
                    assert_eq!(k.code, code);
                    assert_eq!(k.pressed, value != 0);
                    if value == 0 {
                        assert_eq!(k.text, None);
                    }
                    if let Some(t) = k.text {
                        assert!(t.is_ascii());
                    }
                    if !held.contains(&code) {
                        assert_eq!(m.modifiers(), before);
                    }
                    // Put modifiers back as they were for the next value.
                    if held.contains(&code) && value != 1 {
                        m = Keymap::new();
                        for (i, c) in held.iter().enumerate() {
                            if state & 1 << i != 0 {
                                m.key(*c, 1);
                            }
                        }
                    }
                }
            }
        }
        let mut m = Keymap::new();
        for code in [u32::MAX, 0x300, 0x1000] {
            for value in [0, 1, 2, u32::MAX] {
                let _ = m.key(code, value);
            }
        }
    }

    fn on(m: &mut Keymap, name: &str) {
        m.set_layout(by_name(name).unwrap());
    }

    /// INV-0163-1: total over every code, modifier state, dead-key state
    /// and layout; at most one character a key.
    #[test]
    fn translation_is_total_over_every_layout() {
        for layout in 0..LAYOUTS.len() {
            for dead in core::iter::once(None).chain(Accent::ALL.iter().copied().map(Some)) {
                for mods in 0..16u32 {
                    let mut m = Keymap::new();
                    m.set_layout(layout);
                    for (bit, code) in [
                        (1, code::LEFT_SHIFT),
                        (2, code::RIGHT_ALT),
                        (4, code::CAPS_LOCK),
                        (8, code::LEFT_CTRL),
                    ] {
                        if mods & bit != 0 {
                            m.key(code, 1);
                            if code == code::CAPS_LOCK {
                                m.key(code, 0);
                            }
                        }
                    }
                    for code in 0..0x300 {
                        m.dead = dead;
                        let k = m.key(code, 1);
                        let _ = k.text;
                        m.key(code, 0);
                    }
                }
            }
        }
    }

    fn word(m: &mut Keymap, keys: &[(u32, bool, bool)]) -> std::string::String {
        let mut out = std::string::String::new();
        for (code, shift, altgr) in keys {
            if *shift {
                m.key(code::LEFT_SHIFT, 1);
            }
            if *altgr {
                m.key(code::RIGHT_ALT, 1);
            }
            if let Some(c) = typed(m, *code) {
                out.push(c);
            }
            if *altgr {
                m.key(code::RIGHT_ALT, 0);
            }
            if *shift {
                m.key(code::LEFT_SHIFT, 0);
            }
        }
        out
    }

    /// INV-0163-2: the letters of the reference layouts, both cases.
    #[test]
    fn the_reference_letters_are_produced() {
        let mut m = Keymap::new();
        on(&mut m, "cz");
        let row: std::string::String = (2..=11).filter_map(|c| typed(&mut m, c)).collect();
        assert_eq!(row, "+ěščřžýáíé");
        m.key(code::LEFT_SHIFT, 1);
        let digits: std::string::String = (2..=11).filter_map(|c| typed(&mut m, c)).collect();
        assert_eq!(digits, "1234567890");
        assert_eq!(typed(&mut m, 26), Some('/'));
        m.key(code::LEFT_SHIFT, 0);
        assert_eq!(
            (typed(&mut m, 26), typed(&mut m, 39)),
            (Some('ú'), Some('ů'))
        );
        assert_eq!(
            (typed(&mut m, 21), typed(&mut m, 44)),
            (Some('z'), Some('y')),
            "QWERTZ"
        );
        // ď ť ň through the dead caron (Shift with =), ó through the acute.
        assert_eq!(
            word(
                &mut m,
                &[
                    (13, true, false),
                    (32, false, false),
                    (13, true, false),
                    (20, false, false),
                    (13, true, false),
                    (49, false, false),
                    (13, false, false),
                    (24, false, false)
                ]
            ),
            "ďťňó"
        );
        assert_eq!(
            word(
                &mut m,
                &[
                    (13, true, false),
                    (32, true, false),
                    (13, true, false),
                    (49, true, false),
                    (13, false, false),
                    (24, true, false)
                ]
            ),
            "ĎŇÓ"
        );
        assert_eq!(word(&mut m, &[(3, false, true), (18, false, true)]), "@€");
        on(&mut m, "ua");
        assert_eq!(
            word(
                &mut m,
                &[
                    (43, false, false),
                    (40, false, false),
                    (31, false, false),
                    (27, false, false),
                    (41, false, false)
                ]
            ),
            "ґєії'"
        );
        assert_eq!(
            word(
                &mut m,
                &[
                    (43, true, false),
                    (40, true, false),
                    (31, true, false),
                    (27, true, false),
                    (41, true, false)
                ]
            ),
            "ҐЄІЇʼ"
        );
        assert_eq!(word(&mut m, &[(22, false, true), (22, true, true)]), "ґҐ");
        assert_eq!(
            word(
                &mut m,
                &[(35, false, false), (25, false, false), (22, false, false)]
            ),
            "рзг"
        );
        on(&mut m, "ru");
        assert_eq!(
            word(
                &mut m,
                &[
                    (41, false, false),
                    (31, false, false),
                    (27, false, false),
                    (40, false, false)
                ]
            ),
            "ёыъэ"
        );
        assert_eq!(
            word(
                &mut m,
                &[
                    (41, true, false),
                    (31, true, false),
                    (27, true, false),
                    (40, true, false)
                ]
            ),
            "ЁЫЪЭ"
        );
        assert_eq!(
            word(
                &mut m,
                &[(4, true, false), (5, true, false), (7, true, false)]
            ),
            "№;:"
        );
        // Caps Lock gives capitals of Cyrillic letters too, not of digits.
        m.key(code::CAPS_LOCK, 1);
        m.key(code::CAPS_LOCK, 0);
        assert_eq!(word(&mut m, &[(16, false, false), (2, false, false)]), "Й1");
    }

    /// INV-0163-3: a dead key emits at most one character and never loses
    /// the base; with Space, the spacing accent.
    #[test]
    fn dead_keys_compose_or_pass_the_base() {
        let bases: std::vec::Vec<char> = ('a'..='z')
            .chain('A'..='Z')
            .chain(['ě', 'ж', '1'])
            .collect();
        for accent in Accent::ALL {
            let mut m = Keymap::new();
            on(&mut m, "cz");
            for base in &bases {
                m.dead = Some(accent);
                // Type the base through the US layout's key for it, if any.
                let Some(code) = (0..0x80).find(|c| layouts::US.levels(*c)[0] == Out::Char(*base))
                else {
                    continue;
                };
                m.set_layout(0);
                m.dead = Some(accent);
                let got = typed(&mut m, code);
                assert_eq!(
                    got,
                    Some(accent.compose(*base).unwrap_or(*base)),
                    "{accent:?} {base}"
                );
            }
            m.dead = Some(accent);
            assert_eq!(typed(&mut m, code::SPACE), Some(accent.spacing()));
            assert_eq!(
                typed(&mut m, code::SPACE),
                Some(' '),
                "the dead key is used once"
            );
        }
        // A second dead key replaces the first.
        let mut m = Keymap::new();
        on(&mut m, "cz");
        assert_eq!(
            word(
                &mut m,
                &[(13, false, false), (13, true, false), (19, false, false)]
            ),
            "ř"
        );
        // A dead key then a key with no letter: the dead key goes, the key
        // passes.
        assert_eq!(word(&mut m, &[(13, false, false), (53, false, false)]), "-");
        assert_eq!(Accent::Caron.compose('r'), Some('ř'));
        assert_eq!(Accent::Ring.compose('U'), Some('Ů'));
        assert_eq!(Accent::Acute.compose('q'), None);
    }

    /// INV-0163-4: a switch changes no modifier and clears only the dead key.
    #[test]
    fn a_switch_keeps_modifiers_and_clears_the_dead_key() {
        let mut m = Keymap::new();
        let enabled = [0, by_name("ua").unwrap(), by_name("cz").unwrap()];
        m.key(code::LEFT_SHIFT, 1);
        m.switch(&enabled, false);
        assert_eq!(m.layout().name, "ua");
        assert_eq!(m.modifiers(), SHIFT);
        assert_eq!(typed(&mut m, 16), Some('Й'));
        m.key(code::LEFT_SHIFT, 0);
        m.switch(&enabled, false);
        assert_eq!(m.layout().short, "CZ");
        m.key(13, 1);
        m.key(13, 0);
        m.switch(&enabled, true);
        m.switch(&enabled, false);
        assert_eq!(typed(&mut m, 18), Some('e'), "the dead acute was cleared");
        m.switch(&enabled, false);
        assert_eq!(m.layout().name, "us", "round");
        m.switch(&enabled, true);
        assert_eq!(m.layout().name, "cz");
        m.set_layout(by_name("ru").unwrap());
        m.switch(&enabled, false);
        assert_eq!(m.layout().name, "us", "from one not enabled, the first");
        m.switch(&[], false);
        assert_eq!(m.layout().name, "us");
        m.set_layout(99);
        assert_eq!(m.layout().name, "us");
        // The right Alt held across a switch to a layout without AltGr is
        // Alt there.
        on(&mut m, "cz");
        m.key(code::RIGHT_ALT, 1);
        assert_eq!(m.modifiers(), 0, "AltGr is no modifier for clients");
        m.set_layout(0);
        assert_eq!(m.modifiers(), ALT);
        m.key(code::RIGHT_ALT, 0);
        assert_eq!(m.modifiers(), 0);
        assert!(!layouts::US.has_altgr() && layouts::CZ.has_altgr());
        m.key(code::LEFT_CTRL, 1);
        on(&mut m, "ua");
        assert_eq!(
            typed(&mut m, 46),
            Some('\u{3}'),
            "Ctrl+C by the US position"
        );
        assert_eq!(m.layout_index(), by_name("ua").unwrap());
        m.clear_dead();
    }
}

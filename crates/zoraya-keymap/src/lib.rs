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

/// The US layout: code, unshifted, shifted. Codes not here give no text.
const US: [(u32, u8, u8); 47] = [
    (2, b'1', b'!'),
    (3, b'2', b'@'),
    (4, b'3', b'#'),
    (5, b'4', b'$'),
    (6, b'5', b'%'),
    (7, b'6', b'^'),
    (8, b'7', b'&'),
    (9, b'8', b'*'),
    (10, b'9', b'('),
    (11, b'0', b')'),
    (12, b'-', b'_'),
    (13, b'=', b'+'),
    (16, b'q', b'Q'),
    (17, b'w', b'W'),
    (18, b'e', b'E'),
    (19, b'r', b'R'),
    (20, b't', b'T'),
    (21, b'y', b'Y'),
    (22, b'u', b'U'),
    (23, b'i', b'I'),
    (24, b'o', b'O'),
    (25, b'p', b'P'),
    (26, b'[', b'{'),
    (27, b']', b'}'),
    (30, b'a', b'A'),
    (31, b's', b'S'),
    (32, b'd', b'D'),
    (33, b'f', b'F'),
    (34, b'g', b'G'),
    (35, b'h', b'H'),
    (36, b'j', b'J'),
    (37, b'k', b'K'),
    (38, b'l', b'L'),
    (39, b';', b':'),
    (40, b'\'', b'"'),
    (41, b'`', b'~'),
    (43, b'\\', b'|'),
    (44, b'z', b'Z'),
    (45, b'x', b'X'),
    (46, b'c', b'C'),
    (47, b'v', b'V'),
    (48, b'b', b'B'),
    (49, b'n', b'N'),
    (50, b'm', b'M'),
    (51, b',', b'<'),
    (52, b'.', b'>'),
    (53, b'/', b'?'),
];

/// One key as a client gets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: u32,
    pub pressed: bool,
    pub modifiers: u32,
    pub text: Option<char>,
}

/// The layout and the modifier state.
#[derive(Debug, Default)]
pub struct Keymap {
    shift: u8,
    ctrl: u8,
    alt: u8,
    caps: bool,
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
            caps: false,
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

    /// Takes one key event: `value` 0 released, 1 pressed, 2 repeated.
    /// Total over every code and value (INV-0092-3).
    pub fn key(&mut self, code: u32, value: u32) -> Key {
        let pressed = value != 0;
        match code {
            code::LEFT_SHIFT => side(&mut self.shift, 1, pressed),
            code::RIGHT_SHIFT => side(&mut self.shift, 2, pressed),
            code::LEFT_CTRL => side(&mut self.ctrl, 1, pressed),
            code::RIGHT_CTRL => side(&mut self.ctrl, 2, pressed),
            code::LEFT_ALT => side(&mut self.alt, 1, pressed),
            code::RIGHT_ALT => side(&mut self.alt, 2, pressed),
            code::CAPS_LOCK if value == 1 => self.caps = !self.caps,
            _ => {}
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

    fn text(&self, code: u32) -> Option<char> {
        if code == code::SPACE {
            return Some(if self.ctrl != 0 { '\0' } else { ' ' });
        }
        let &(_, plain, shifted) = US.iter().find(|(c, _, _)| *c == code)?;
        let letter = plain.is_ascii_lowercase();
        if self.ctrl != 0 {
            // Ctrl with a letter, or with the punctuation of 0x1b..0x1f.
            return match plain {
                b'a'..=b'z' => Some(char::from(plain - b'a' + 1)),
                b'[' => Some('\u{1b}'),
                b'\\' => Some('\u{1c}'),
                b']' => Some('\u{1d}'),
                _ => None,
            };
        }
        let upper = (self.shift != 0) ^ (letter && self.caps);
        Some(char::from(if upper { shifted } else { plain }))
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
}

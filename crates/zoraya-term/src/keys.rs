//! The bytes a key sends to the shell (ADR-0096, ADR-0332 item 7,
//! ADR-0383): text as UTF-8, Alt as an `ESC` prefix, the named keys as the
//! `k*` strings of the entry the terminal claims (`xterm-256color`), with
//! xterm's modifier parameter, and the cursor keys, Home and End in their
//! `SS3` forms while DECCKM is set. The tests read the entry and compare
//! (INV-0332-1's key half). A pure function of the key and the two modes.

use zoraya_keymap::code;

/// The modifiers that change a key's bytes. The system key never reaches
/// a client (INV-0160-3), and Caps Lock is in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Mods {
    /// From `zoraya-proto-gui`'s modifier bits (`SHIFT` 1, `CTRL` 2, `ALT` 4).
    pub const fn from_bits(bits: u32) -> Mods {
        Mods {
            shift: bits & 1 != 0,
            ctrl: bits & 2 != 0,
            alt: bits & 4 != 0,
        }
    }

    /// xterm's parameter: 1 plus Shift 1, Alt 2, Ctrl 4; 1 is none.
    pub const fn parameter(self) -> u8 {
        1 + self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8
    }

    const fn any(self) -> bool {
        self.shift || self.alt || self.ctrl
    }
}

/// The two modes that change what a key sends: DECCKM and the keypad's
/// application mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModes {
    pub app_cursor: bool,
    pub app_keypad: bool,
}

/// The longest sequence `key_bytes` writes: `ESC [ 2 4 ; 7 ~` is 7 bytes,
/// `ESC` and a four-byte character 5.
pub const MAX_KEY_BYTES: usize = 16;

/// A small buffer the sequences are written in.
struct Out<'a> {
    bytes: &'a mut [u8; MAX_KEY_BYTES],
    len: usize,
}

impl Out<'_> {
    fn put(&mut self, s: &[u8]) {
        self.bytes[self.len..self.len + s.len()].copy_from_slice(s);
        self.len += s.len();
    }

    fn decimal(&mut self, n: u8) {
        if n >= 10 {
            self.put(&[b'0' + n / 10]);
        }
        self.put(&[b'0' + n % 10]);
    }

    /// `CSI final` (no modifiers), `CSI 1 ; M final` (with).
    fn csi_final(&mut self, mods: Mods, final_byte: u8) {
        self.put(b"\x1b[");
        if mods.any() {
            self.put(b"1;");
            self.decimal(mods.parameter());
        }
        self.put(&[final_byte]);
    }

    /// `SS3 final` without modifiers, `CSI 1 ; M final` with.
    fn ss3_or_csi(&mut self, mods: Mods, application: bool, final_byte: u8) {
        if application && !mods.any() {
            self.put(&[0x1b, b'O', final_byte]);
        } else {
            self.csi_final(mods, final_byte);
        }
    }

    /// `CSI n ~`, `CSI n ; M ~`.
    fn tilde(&mut self, n: u8, mods: Mods) {
        self.put(b"\x1b[");
        self.decimal(n);
        if mods.any() {
            self.put(b";");
            self.decimal(mods.parameter());
        }
        self.put(b"~");
    }
}

/// The numbers of F5 to F12 in `CSI n ~`.
const FUNCTION_TILDE: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];

/// The final letter of a keypad key in application mode (`ESC O x`) and its
/// character otherwise.
fn keypad(key: u32) -> Option<(u8, u8)> {
    Some(match key {
        code::KP0 => (b'p', b'0'),
        code::KP1 => (b'q', b'1'),
        code::KP2 => (b'r', b'2'),
        code::KP3 => (b's', b'3'),
        code::KP4 => (b't', b'4'),
        code::KP5 => (b'u', b'5'),
        code::KP6 => (b'v', b'6'),
        code::KP7 => (b'w', b'7'),
        code::KP8 => (b'x', b'8'),
        code::KP9 => (b'y', b'9'),
        code::KP_DOT => (b'n', b'.'),
        code::KP_PLUS => (b'k', b'+'),
        code::KP_MINUS => (b'm', b'-'),
        code::KP_ASTERISK => (b'j', b'*'),
        code::KP_SLASH => (b'o', b'/'),
        code::KP_COMMA => (b'l', b','),
        code::KP_ENTER => (b'M', b'\r'),
        _ => return None,
    })
}

/// The bytes a key sends, written to `out`; gives how many. `text` is what
/// the keymap gave. Shift alone with Page Up or Page Down is the terminal's
/// own (it scrolls the view) and sends nothing; a key the entry and this
/// table do not name sends its text, and with Alt an `ESC` before it.
pub fn key_bytes(
    key: u32,
    mods: Mods,
    text: Option<char>,
    modes: KeyModes,
    out: &mut [u8; MAX_KEY_BYTES],
) -> usize {
    let mut o = Out { bytes: out, len: 0 };
    match key {
        code::UP => o.ss3_or_csi(mods, modes.app_cursor, b'A'),
        code::DOWN => o.ss3_or_csi(mods, modes.app_cursor, b'B'),
        code::RIGHT => o.ss3_or_csi(mods, modes.app_cursor, b'C'),
        code::LEFT => o.ss3_or_csi(mods, modes.app_cursor, b'D'),
        code::HOME => o.ss3_or_csi(mods, modes.app_cursor, b'H'),
        code::END => o.ss3_or_csi(mods, modes.app_cursor, b'F'),
        code::INSERT => o.tilde(2, mods),
        code::DELETE => o.tilde(3, mods),
        code::PAGE_UP | code::PAGE_DOWN
            if mods
                == Mods {
                    shift: true,
                    ..Mods::default()
                } =>
        {
            return 0;
        }
        code::PAGE_UP => o.tilde(5, mods),
        code::PAGE_DOWN => o.tilde(6, mods),
        code::F1..=code::F4 => {
            let letter = b'P' + (key - code::F1) as u8;
            if mods.any() {
                o.csi_final(mods, letter);
            } else {
                o.put(&[0x1b, b'O', letter]);
            }
        }
        code::F5..=code::F10 => o.tilde(FUNCTION_TILDE[(key - code::F5) as usize], mods),
        code::F11 => o.tilde(FUNCTION_TILDE[6], mods),
        code::F12 => o.tilde(FUNCTION_TILDE[7], mods),
        code::TAB if mods.shift && !mods.alt && !mods.ctrl => o.put(b"\x1b[Z"),
        code::ENTER => prefixed(&mut o, mods, b"\r"),
        code::BACKSPACE => prefixed(&mut o, mods, b"\x7f"),
        code::TAB => prefixed(&mut o, mods, b"\t"),
        code::ESC => prefixed(&mut o, mods, b"\x1b"),
        // Ctrl+Space and Ctrl+@ (Ctrl+Shift+2) are NUL (the keymap gives no
        // text for NUL).
        code::SPACE if mods.ctrl => prefixed(&mut o, mods, b"\0"),
        code::DIGIT_2 if mods.ctrl && mods.shift => prefixed(&mut o, mods, b"\0"),
        _ => {
            if let Some((letter, character)) = keypad(key) {
                if modes.app_keypad {
                    if mods.alt {
                        o.put(b"\x1b");
                    }
                    o.put(&[0x1b, b'O', letter]);
                } else {
                    prefixed(&mut o, mods, &[character]);
                }
            } else if let Some(c) = text {
                if mods.alt {
                    o.put(b"\x1b");
                }
                let mut utf8 = [0u8; 4];
                o.put(c.encode_utf8(&mut utf8).as_bytes());
            }
        }
    }
    o.len
}

/// A key with a fixed byte: with Alt, an `ESC` before it.
fn prefixed(o: &mut Out<'_>, mods: Mods, bytes: &[u8]) {
    if mods.alt {
        o.put(b"\x1b");
    }
    o.put(bytes);
}

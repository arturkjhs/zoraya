//! The key table against the entry (INV-0332-1's key half, ADR-0332 item 7):
//! every `k*` capability of the pinned `xterm-256color` entry that the
//! terminal sends is called by name below, and the table must send exactly
//! the bytes the entry says. The capabilities the terminal does not send are
//! in `entry/exceptions.txt` with their reasons.

use std::vec::Vec;

use zoraya_keymap::code;

use crate::entry::cap;
use crate::keys::{KeyModes, MAX_KEY_BYTES, Mods, key_bytes};

const NONE: Mods = Mods {
    shift: false,
    alt: false,
    ctrl: false,
};
const SHIFT: Mods = Mods {
    shift: true,
    alt: false,
    ctrl: false,
};
const ALT: Mods = Mods {
    shift: false,
    alt: true,
    ctrl: false,
};
const CTRL: Mods = Mods {
    shift: false,
    alt: false,
    ctrl: true,
};
const ALT_SHIFT: Mods = Mods {
    shift: true,
    alt: true,
    ctrl: false,
};
const CTRL_SHIFT: Mods = Mods {
    shift: true,
    alt: false,
    ctrl: true,
};
const CTRL_ALT: Mods = Mods {
    shift: false,
    alt: true,
    ctrl: true,
};

fn send(key: u32, mods: Mods, text: Option<char>, modes: KeyModes) -> Vec<u8> {
    let mut out = [0u8; MAX_KEY_BYTES];
    let n = key_bytes(key, mods, text, modes, &mut out);
    out[..n].to_vec()
}

type Row = (Vec<u8>, u32, Mods, KeyModes, &'static str);

/// Every key capability the terminal sends, with the key and modes that
/// send it: DECCKM and the keypad's application mode are on where the entry
/// (whose `smkx` sets both) names the `SS3` forms.
fn keys() -> Vec<Row> {
    std::vec![
        (
            cap("kcuu1", &[]),
            code::UP,
            NONE,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kcuu1"
        ),
        (
            cap("kcud1", &[]),
            code::DOWN,
            NONE,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kcud1"
        ),
        (
            cap("kcuf1", &[]),
            code::RIGHT,
            NONE,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kcuf1"
        ),
        (
            cap("kcub1", &[]),
            code::LEFT,
            NONE,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kcub1"
        ),
        (
            cap("khome", &[]),
            code::HOME,
            NONE,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "khome"
        ),
        (
            cap("kend", &[]),
            code::END,
            NONE,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kend"
        ),
        (
            cap("kdch1", &[]),
            code::DELETE,
            NONE,
            KeyModes::default(),
            "kdch1"
        ),
        (
            cap("kich1", &[]),
            code::INSERT,
            NONE,
            KeyModes::default(),
            "kich1"
        ),
        (
            cap("knp", &[]),
            code::PAGE_DOWN,
            NONE,
            KeyModes::default(),
            "knp"
        ),
        (
            cap("kpp", &[]),
            code::PAGE_UP,
            NONE,
            KeyModes::default(),
            "kpp"
        ),
        (
            cap("kcbt", &[]),
            code::TAB,
            SHIFT,
            KeyModes::default(),
            "kcbt"
        ),
        (
            cap("kind", &[]),
            code::DOWN,
            SHIFT,
            KeyModes::default(),
            "kind"
        ),
        (cap("kri", &[]), code::UP, SHIFT, KeyModes::default(), "kri"),
        (
            cap("kent", &[]),
            code::KP_ENTER,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kent"
        ),
        (
            cap("ka1", &[]),
            code::KP7,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "ka1"
        ),
        (
            cap("ka2", &[]),
            code::KP8,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "ka2"
        ),
        (
            cap("ka3", &[]),
            code::KP9,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "ka3"
        ),
        (
            cap("kb1", &[]),
            code::KP4,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kb1"
        ),
        (
            cap("kb2", &[]),
            code::KP5,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kb2"
        ),
        (
            cap("kb3", &[]),
            code::KP6,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kb3"
        ),
        (
            cap("kc1", &[]),
            code::KP1,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kc1"
        ),
        (
            cap("kc2", &[]),
            code::KP2,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kc2"
        ),
        (
            cap("kc3", &[]),
            code::KP3,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kc3"
        ),
        (
            cap("kpZRO", &[]),
            code::KP0,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kpZRO"
        ),
        (
            cap("kpDOT", &[]),
            code::KP_DOT,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kpDOT"
        ),
        (
            cap("kpADD", &[]),
            code::KP_PLUS,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kpADD"
        ),
        (
            cap("kpCMA", &[]),
            code::KP_COMMA,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kpCMA"
        ),
        (
            cap("kpDIV", &[]),
            code::KP_SLASH,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kpDIV"
        ),
        (
            cap("kpMUL", &[]),
            code::KP_ASTERISK,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kpMUL"
        ),
        (
            cap("kpSUB", &[]),
            code::KP_MINUS,
            NONE,
            KeyModes {
                app_cursor: false,
                app_keypad: true
            },
            "kpSUB"
        ),
        (cap("kf1", &[]), code::F1, NONE, KeyModes::default(), "kf1"),
        (cap("kf2", &[]), code::F2, NONE, KeyModes::default(), "kf2"),
        (cap("kf3", &[]), code::F3, NONE, KeyModes::default(), "kf3"),
        (cap("kf4", &[]), code::F4, NONE, KeyModes::default(), "kf4"),
        (cap("kf5", &[]), code::F5, NONE, KeyModes::default(), "kf5"),
        (cap("kf6", &[]), code::F6, NONE, KeyModes::default(), "kf6"),
        (cap("kf7", &[]), code::F7, NONE, KeyModes::default(), "kf7"),
        (cap("kf8", &[]), code::F8, NONE, KeyModes::default(), "kf8"),
        (cap("kf9", &[]), code::F9, NONE, KeyModes::default(), "kf9"),
        (
            cap("kf10", &[]),
            code::F10,
            NONE,
            KeyModes::default(),
            "kf10"
        ),
        (
            cap("kf11", &[]),
            code::F11,
            NONE,
            KeyModes::default(),
            "kf11"
        ),
        (
            cap("kf12", &[]),
            code::F12,
            NONE,
            KeyModes::default(),
            "kf12"
        ),
        (
            cap("kf13", &[]),
            code::F1,
            SHIFT,
            KeyModes::default(),
            "kf13"
        ),
        (
            cap("kf14", &[]),
            code::F2,
            SHIFT,
            KeyModes::default(),
            "kf14"
        ),
        (
            cap("kf15", &[]),
            code::F3,
            SHIFT,
            KeyModes::default(),
            "kf15"
        ),
        (
            cap("kf16", &[]),
            code::F4,
            SHIFT,
            KeyModes::default(),
            "kf16"
        ),
        (
            cap("kf17", &[]),
            code::F5,
            SHIFT,
            KeyModes::default(),
            "kf17"
        ),
        (
            cap("kf18", &[]),
            code::F6,
            SHIFT,
            KeyModes::default(),
            "kf18"
        ),
        (
            cap("kf19", &[]),
            code::F7,
            SHIFT,
            KeyModes::default(),
            "kf19"
        ),
        (
            cap("kf20", &[]),
            code::F8,
            SHIFT,
            KeyModes::default(),
            "kf20"
        ),
        (
            cap("kf21", &[]),
            code::F9,
            SHIFT,
            KeyModes::default(),
            "kf21"
        ),
        (
            cap("kf22", &[]),
            code::F10,
            SHIFT,
            KeyModes::default(),
            "kf22"
        ),
        (
            cap("kf23", &[]),
            code::F11,
            SHIFT,
            KeyModes::default(),
            "kf23"
        ),
        (
            cap("kf24", &[]),
            code::F12,
            SHIFT,
            KeyModes::default(),
            "kf24"
        ),
        (
            cap("kf25", &[]),
            code::F1,
            CTRL,
            KeyModes::default(),
            "kf25"
        ),
        (
            cap("kf26", &[]),
            code::F2,
            CTRL,
            KeyModes::default(),
            "kf26"
        ),
        (
            cap("kf27", &[]),
            code::F3,
            CTRL,
            KeyModes::default(),
            "kf27"
        ),
        (
            cap("kf28", &[]),
            code::F4,
            CTRL,
            KeyModes::default(),
            "kf28"
        ),
        (
            cap("kf29", &[]),
            code::F5,
            CTRL,
            KeyModes::default(),
            "kf29"
        ),
        (
            cap("kf30", &[]),
            code::F6,
            CTRL,
            KeyModes::default(),
            "kf30"
        ),
        (
            cap("kf31", &[]),
            code::F7,
            CTRL,
            KeyModes::default(),
            "kf31"
        ),
        (
            cap("kf32", &[]),
            code::F8,
            CTRL,
            KeyModes::default(),
            "kf32"
        ),
        (
            cap("kf33", &[]),
            code::F9,
            CTRL,
            KeyModes::default(),
            "kf33"
        ),
        (
            cap("kf34", &[]),
            code::F10,
            CTRL,
            KeyModes::default(),
            "kf34"
        ),
        (
            cap("kf35", &[]),
            code::F11,
            CTRL,
            KeyModes::default(),
            "kf35"
        ),
        (
            cap("kf36", &[]),
            code::F12,
            CTRL,
            KeyModes::default(),
            "kf36"
        ),
        (
            cap("kf37", &[]),
            code::F1,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf37"
        ),
        (
            cap("kf38", &[]),
            code::F2,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf38"
        ),
        (
            cap("kf39", &[]),
            code::F3,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf39"
        ),
        (
            cap("kf40", &[]),
            code::F4,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf40"
        ),
        (
            cap("kf41", &[]),
            code::F5,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf41"
        ),
        (
            cap("kf42", &[]),
            code::F6,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf42"
        ),
        (
            cap("kf43", &[]),
            code::F7,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf43"
        ),
        (
            cap("kf44", &[]),
            code::F8,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf44"
        ),
        (
            cap("kf45", &[]),
            code::F9,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf45"
        ),
        (
            cap("kf46", &[]),
            code::F10,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf46"
        ),
        (
            cap("kf47", &[]),
            code::F11,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf47"
        ),
        (
            cap("kf48", &[]),
            code::F12,
            CTRL_SHIFT,
            KeyModes::default(),
            "kf48"
        ),
        (cap("kf49", &[]), code::F1, ALT, KeyModes::default(), "kf49"),
        (cap("kf50", &[]), code::F2, ALT, KeyModes::default(), "kf50"),
        (cap("kf51", &[]), code::F3, ALT, KeyModes::default(), "kf51"),
        (cap("kf52", &[]), code::F4, ALT, KeyModes::default(), "kf52"),
        (cap("kf53", &[]), code::F5, ALT, KeyModes::default(), "kf53"),
        (cap("kf54", &[]), code::F6, ALT, KeyModes::default(), "kf54"),
        (cap("kf55", &[]), code::F7, ALT, KeyModes::default(), "kf55"),
        (cap("kf56", &[]), code::F8, ALT, KeyModes::default(), "kf56"),
        (cap("kf57", &[]), code::F9, ALT, KeyModes::default(), "kf57"),
        (
            cap("kf58", &[]),
            code::F10,
            ALT,
            KeyModes::default(),
            "kf58"
        ),
        (
            cap("kf59", &[]),
            code::F11,
            ALT,
            KeyModes::default(),
            "kf59"
        ),
        (
            cap("kf60", &[]),
            code::F12,
            ALT,
            KeyModes::default(),
            "kf60"
        ),
        (
            cap("kf61", &[]),
            code::F1,
            ALT_SHIFT,
            KeyModes::default(),
            "kf61"
        ),
        (
            cap("kf62", &[]),
            code::F2,
            ALT_SHIFT,
            KeyModes::default(),
            "kf62"
        ),
        (
            cap("kf63", &[]),
            code::F3,
            ALT_SHIFT,
            KeyModes::default(),
            "kf63"
        ),
        (
            cap("kUP", &[]),
            code::UP,
            SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kUP"
        ),
        (
            cap("kUP3", &[]),
            code::UP,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kUP3"
        ),
        (
            cap("kUP4", &[]),
            code::UP,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kUP4"
        ),
        (
            cap("kUP5", &[]),
            code::UP,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kUP5"
        ),
        (
            cap("kUP6", &[]),
            code::UP,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kUP6"
        ),
        (
            cap("kUP7", &[]),
            code::UP,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kUP7"
        ),
        (
            cap("kDN", &[]),
            code::DOWN,
            SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDN"
        ),
        (
            cap("kDN3", &[]),
            code::DOWN,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDN3"
        ),
        (
            cap("kDN4", &[]),
            code::DOWN,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDN4"
        ),
        (
            cap("kDN5", &[]),
            code::DOWN,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDN5"
        ),
        (
            cap("kDN6", &[]),
            code::DOWN,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDN6"
        ),
        (
            cap("kDN7", &[]),
            code::DOWN,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDN7"
        ),
        (
            cap("kLFT", &[]),
            code::LEFT,
            SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kLFT"
        ),
        (
            cap("kLFT3", &[]),
            code::LEFT,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kLFT3"
        ),
        (
            cap("kLFT4", &[]),
            code::LEFT,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kLFT4"
        ),
        (
            cap("kLFT5", &[]),
            code::LEFT,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kLFT5"
        ),
        (
            cap("kLFT6", &[]),
            code::LEFT,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kLFT6"
        ),
        (
            cap("kLFT7", &[]),
            code::LEFT,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kLFT7"
        ),
        (
            cap("kRIT", &[]),
            code::RIGHT,
            SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kRIT"
        ),
        (
            cap("kRIT3", &[]),
            code::RIGHT,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kRIT3"
        ),
        (
            cap("kRIT4", &[]),
            code::RIGHT,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kRIT4"
        ),
        (
            cap("kRIT5", &[]),
            code::RIGHT,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kRIT5"
        ),
        (
            cap("kRIT6", &[]),
            code::RIGHT,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kRIT6"
        ),
        (
            cap("kRIT7", &[]),
            code::RIGHT,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kRIT7"
        ),
        (
            cap("kHOM", &[]),
            code::HOME,
            SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kHOM"
        ),
        (
            cap("kHOM3", &[]),
            code::HOME,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kHOM3"
        ),
        (
            cap("kHOM4", &[]),
            code::HOME,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kHOM4"
        ),
        (
            cap("kHOM5", &[]),
            code::HOME,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kHOM5"
        ),
        (
            cap("kHOM6", &[]),
            code::HOME,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kHOM6"
        ),
        (
            cap("kHOM7", &[]),
            code::HOME,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kHOM7"
        ),
        (
            cap("kEND", &[]),
            code::END,
            SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kEND"
        ),
        (
            cap("kEND3", &[]),
            code::END,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kEND3"
        ),
        (
            cap("kEND4", &[]),
            code::END,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kEND4"
        ),
        (
            cap("kEND5", &[]),
            code::END,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kEND5"
        ),
        (
            cap("kEND6", &[]),
            code::END,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kEND6"
        ),
        (
            cap("kEND7", &[]),
            code::END,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kEND7"
        ),
        (
            cap("kIC", &[]),
            code::INSERT,
            SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kIC"
        ),
        (
            cap("kIC3", &[]),
            code::INSERT,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kIC3"
        ),
        (
            cap("kIC4", &[]),
            code::INSERT,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kIC4"
        ),
        (
            cap("kIC5", &[]),
            code::INSERT,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kIC5"
        ),
        (
            cap("kIC6", &[]),
            code::INSERT,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kIC6"
        ),
        (
            cap("kIC7", &[]),
            code::INSERT,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kIC7"
        ),
        (
            cap("kDC", &[]),
            code::DELETE,
            SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDC"
        ),
        (
            cap("kDC3", &[]),
            code::DELETE,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDC3"
        ),
        (
            cap("kDC4", &[]),
            code::DELETE,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDC4"
        ),
        (
            cap("kDC5", &[]),
            code::DELETE,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDC5"
        ),
        (
            cap("kDC6", &[]),
            code::DELETE,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDC6"
        ),
        (
            cap("kDC7", &[]),
            code::DELETE,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kDC7"
        ),
        (
            cap("kNXT3", &[]),
            code::PAGE_DOWN,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kNXT3"
        ),
        (
            cap("kNXT4", &[]),
            code::PAGE_DOWN,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kNXT4"
        ),
        (
            cap("kNXT5", &[]),
            code::PAGE_DOWN,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kNXT5"
        ),
        (
            cap("kNXT6", &[]),
            code::PAGE_DOWN,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kNXT6"
        ),
        (
            cap("kNXT7", &[]),
            code::PAGE_DOWN,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kNXT7"
        ),
        (
            cap("kPRV3", &[]),
            code::PAGE_UP,
            ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kPRV3"
        ),
        (
            cap("kPRV4", &[]),
            code::PAGE_UP,
            ALT_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kPRV4"
        ),
        (
            cap("kPRV5", &[]),
            code::PAGE_UP,
            CTRL,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kPRV5"
        ),
        (
            cap("kPRV6", &[]),
            code::PAGE_UP,
            CTRL_SHIFT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kPRV6"
        ),
        (
            cap("kPRV7", &[]),
            code::PAGE_UP,
            CTRL_ALT,
            KeyModes {
                app_cursor: true,
                app_keypad: true
            },
            "kPRV7"
        ),
    ]
}

#[test]
fn the_table_sends_what_the_entry_names() {
    let rows = keys();
    assert_eq!(rows.len(), 151);
    for (want, key, mods, modes, name) in rows {
        assert_eq!(send(key, mods, None, modes), want, "{name}");
    }
}

/// Without `smkx` (the terminal's own default) the cursor keys, Home and End
/// are the `CSI` forms, which the entry's `cuu1` family writes the other way;
/// with a modifier the `CSI 1 ; M` form holds even while DECCKM is set.
#[test]
fn the_cursor_keys_follow_decckm_and_modifiers_make_them_csi() {
    let normal = KeyModes::default();
    let app = KeyModes {
        app_cursor: true,
        app_keypad: false,
    };
    for (key, letter) in [
        (code::UP, b'A'),
        (code::DOWN, b'B'),
        (code::RIGHT, b'C'),
        (code::LEFT, b'D'),
        (code::HOME, b'H'),
        (code::END, b'F'),
    ] {
        assert_eq!(send(key, NONE, None, normal), [0x1b, b'[', letter]);
        assert_eq!(send(key, NONE, None, app), [0x1b, b'O', letter]);
        assert_eq!(
            send(key, CTRL, None, app),
            [0x1b, b'[', b'1', b';', b'5', letter]
        );
        assert_eq!(
            send(key, CTRL, None, normal),
            [0x1b, b'[', b'1', b';', b'5', letter]
        );
    }
    // The keypad mode is separate from DECCKM.
    let keypad = KeyModes {
        app_cursor: false,
        app_keypad: true,
    };
    assert_eq!(send(code::UP, NONE, None, keypad), b"\x1b[A");
    assert_eq!(send(code::KP0, NONE, None, app), b"0");
}

/// F1 to F4 are `SS3` bare and `CSI 1 ; M` modified; F5 and the rest are
/// `CSI n ~` and `CSI n ; M ~` with the two-digit numbers.
#[test]
fn function_keys_bare_and_modified() {
    let any = KeyModes {
        app_cursor: true,
        app_keypad: true,
    };
    assert_eq!(send(code::F1, NONE, None, any), b"\x1bOP");
    assert_eq!(send(code::F4, NONE, None, any), b"\x1bOS");
    assert_eq!(send(code::F1, CTRL, None, any), b"\x1b[1;5P");
    assert_eq!(send(code::F5, NONE, None, any), b"\x1b[15~");
    assert_eq!(send(code::F12, CTRL_ALT, None, any), b"\x1b[24;7~");
    assert_eq!(send(code::F11, SHIFT, None, any), b"\x1b[23;2~");
    // The longest sequence fits the buffer.
    assert!(send(code::F12, CTRL_ALT, None, any).len() <= MAX_KEY_BYTES);
}

/// Alt is an `ESC` before what the key would send (ADR-0332 item 4: never
/// the eighth bit), on text and on Enter, Backspace, Tab and Escape.
#[test]
fn alt_is_an_escape_prefix() {
    let m = KeyModes::default();
    assert_eq!(send(30, ALT, Some('a'), m), b"\x1ba");
    assert_eq!(send(30, ALT, Some('é'), m), "\x1bé".as_bytes());
    assert_eq!(send(30, ALT, Some('\u{3}'), m), b"\x1b\x03");
    assert_eq!(send(code::ENTER, ALT, None, m), b"\x1b\r");
    assert_eq!(send(code::BACKSPACE, ALT, None, m), b"\x1b\x7f");
    assert_eq!(send(code::TAB, ALT, None, m), b"\x1b\t");
    assert_eq!(send(code::ESC, ALT, None, m), b"\x1b\x1b");
    assert_eq!(send(code::ESC, NONE, None, m), b"\x1b");
    // Shift alone is no prefix; the keymap's text carries it.
    assert_eq!(send(30, SHIFT, Some('A'), m), b"A");
    // A key with no text and no entry sends nothing.
    assert_eq!(send(30, ALT, None, m), b"");
    assert_eq!(send(code::LEFT_SHIFT, NONE, None, m), b"");
}

/// Ctrl+Space and Ctrl+@ are NUL, Ctrl+^ and Ctrl+_ their codes (the keymap
/// gives the text of the last two), and Shift+Tab is `CSI Z`.
#[test]
fn control_codes_nul_and_back_tab() {
    let m = KeyModes::default();
    assert_eq!(send(code::SPACE, CTRL, None, m), b"\0");
    assert_eq!(send(code::SPACE, NONE, Some(' '), m), b" ");
    assert_eq!(send(code::SPACE, ALT, Some(' '), m), b"\x1b ");
    assert_eq!(send(code::DIGIT_2, CTRL_SHIFT, None, m), b"\0");
    assert_eq!(send(code::DIGIT_2, SHIFT, Some('@'), m), b"@");
    assert_eq!(send(code::DIGIT_6, CTRL_SHIFT, Some('\u{1e}'), m), b"\x1e");
    assert_eq!(send(code::MINUS, CTRL_SHIFT, Some('\u{1f}'), m), b"\x1f");
    assert_eq!(send(code::TAB, SHIFT, None, m), b"\x1b[Z");
    assert_eq!(send(code::TAB, NONE, None, m), b"\t");
    assert_eq!(send(code::TAB, CTRL, None, m), b"\t");
}

/// Backspace is DEL, ADR-0096's, and the pinned entry's `kbs` says `^?`
/// (ADR-0383 Option C: the entry is compiled with the line changed, so there
/// is no exception); Shift with Page Up or Page Down scrolls the view and
/// sends nothing, with any other modifier it sends.
#[test]
fn backspace_and_the_views_own_keys() {
    let m = KeyModes::default();
    assert_eq!(
        send(code::BACKSPACE, NONE, None, m),
        cap("kbs", &[]).as_slice()
    );
    assert_eq!(send(code::BACKSPACE, NONE, None, m), b"\x7f");
    assert_eq!(send(code::BACKSPACE, SHIFT, None, m), b"\x7f");
    assert_eq!(send(code::PAGE_UP, SHIFT, None, m), b"");
    assert_eq!(send(code::PAGE_DOWN, SHIFT, None, m), b"");
    assert_eq!(send(code::PAGE_UP, ALT_SHIFT, None, m), b"\x1b[5;4~");
    assert_eq!(send(code::PAGE_DOWN, CTRL, None, m), b"\x1b[6;5~");
    assert_eq!(send(code::PAGE_UP, NONE, None, m), b"\x1b[5~");
}

/// The keypad sends its characters until the program asks for the
/// application mode, then `SS3` letters; Alt prefixes either.
#[test]
fn the_keypad_follows_its_mode() {
    let numeric = KeyModes::default();
    let app = KeyModes {
        app_cursor: false,
        app_keypad: true,
    };
    for (key, ch, letter) in [
        (code::KP0, b'0', b'p'),
        (code::KP1, b'1', b'q'),
        (code::KP2, b'2', b'r'),
        (code::KP3, b'3', b's'),
        (code::KP4, b'4', b't'),
        (code::KP5, b'5', b'u'),
        (code::KP6, b'6', b'v'),
        (code::KP7, b'7', b'w'),
        (code::KP8, b'8', b'x'),
        (code::KP9, b'9', b'y'),
        (code::KP_DOT, b'.', b'n'),
        (code::KP_PLUS, b'+', b'k'),
        (code::KP_MINUS, b'-', b'm'),
        (code::KP_ASTERISK, b'*', b'j'),
        (code::KP_SLASH, b'/', b'o'),
        (code::KP_COMMA, b',', b'l'),
        (code::KP_ENTER, b'\r', b'M'),
    ] {
        assert_eq!(send(key, NONE, None, numeric), [ch]);
        assert_eq!(send(key, NONE, None, app), [0x1b, b'O', letter]);
        assert_eq!(send(key, ALT, None, numeric), [0x1b, ch]);
        assert_eq!(send(key, ALT, None, app), [0x1b, 0x1b, b'O', letter]);
    }
}

#[test]
fn modifier_parameters_follow_xterm() {
    assert_eq!(NONE.parameter(), 1);
    assert_eq!(SHIFT.parameter(), 2);
    assert_eq!(ALT.parameter(), 3);
    assert_eq!(ALT_SHIFT.parameter(), 4);
    assert_eq!(CTRL.parameter(), 5);
    assert_eq!(CTRL_SHIFT.parameter(), 6);
    assert_eq!(CTRL_ALT.parameter(), 7);
    assert_eq!(Mods::from_bits(1 | 2 | 4).parameter(), 8);
    assert_eq!(
        Mods::from_bits(16 | 8),
        NONE,
        "System and Caps are no modifier here"
    );
}

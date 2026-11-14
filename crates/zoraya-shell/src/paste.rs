//! What the shell does with a paste (ADR-0332 item 8, ADR-0384, INV-0332-5):
//! the parts `term` sends in `Paste` messages are joined until the last,
//! cleaned (no `ESC` and no control character but tab and newline, carriage
//! returns as line ends), and kept until the key stream reaches the place the
//! paste came in. That place is one byte of the stream that no key can make:
//! the keys of `Input` are UTF-8 and the bytes 0xF8 to 0xFF are never in UTF-8,
//! so a paste is one of those bytes between the keys typed before it and the
//! keys typed after it. The prompt inserts the text into its line and does
//! not run it; a program in front gets nothing in 10b (a paste to a program
//! is part 10c's).

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_proto_term::paste::{MAX_PASTE_BYTES, clean};

/// The first of the eight bytes that stand for a paste in the key stream.
pub const MARK_BASE: u8 = 0xF8;

/// Whether a byte of the key stream is a paste's place, not a key.
pub const fn is_mark(byte: u8) -> bool {
    byte >= MARK_BASE
}

/// The parts of one paste, until the last.
#[derive(Debug, Default)]
pub struct Assembler {
    bytes: Vec<u8>,
}

impl Assembler {
    /// A part of a paste; the cleaned text when `last` ends it.
    pub fn part(&mut self, part: &[u8], last: bool) -> Option<String> {
        // More than a paste may hold is dropped.
        let room = MAX_PASTE_BYTES - self.bytes.len();
        self.bytes.extend_from_slice(&part[..part.len().min(room)]);
        if !last {
            return None;
        }
        let bytes = core::mem::take(&mut self.bytes);
        // A character cut in half (a part at the bound) or broken is the
        // replacement character.
        Some(clean(&String::from_utf8_lossy(&bytes)).collect())
    }
}

/// The pastes waiting for their place in the key stream: eight, oldest
/// replaced first.
#[derive(Debug, Default)]
pub struct Store {
    slots: [Option<String>; 8],
    next: usize,
}

impl Store {
    /// Keeps a paste; the byte that stands for it in the key stream.
    pub fn put(&mut self, text: String) -> u8 {
        let slot = self.next;
        self.next = (self.next + 1) % self.slots.len();
        self.slots[slot] = Some(text);
        MARK_BASE + slot as u8
    }

    /// The paste a mark stands for, once; none for a byte that is no mark or
    /// a paste already taken.
    pub fn take(&mut self, byte: u8) -> Option<String> {
        if !is_mark(byte) {
            return None;
        }
        self.slots[usize::from(byte - MARK_BASE)].take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_joined_until_the_last_and_cleaned() {
        let mut a = Assembler::default();
        assert_eq!(a.part(b"he\x1b[20", false), None);
        assert_eq!(a.part("1~llo\r\nw\u{e9}".as_bytes(), false), None);
        assert_eq!(
            a.part(b"rld\x07", true).as_deref(),
            Some("he[201~llo\nw\u{e9}rld")
        );
        // The assembler starts again.
        assert_eq!(a.part(b"x", true).as_deref(), Some("x"));
    }

    #[test]
    fn a_paste_past_the_bound_is_cut_there() {
        let mut a = Assembler::default();
        let part = [b'a'; 4096];
        for _ in 0..(MAX_PASTE_BYTES / 4096) {
            assert_eq!(a.part(&part, false), None);
        }
        let text = a.part(b"more", true).unwrap();
        assert_eq!(text.len(), MAX_PASTE_BYTES);
        // And a new paste is not short by what the last one left.
        assert_eq!(a.part(b"ok", true).as_deref(), Some("ok"));
    }

    #[test]
    fn invalid_utf8_is_replaced_not_kept() {
        let mut a = Assembler::default();
        assert_eq!(
            a.part(b"a\xffb\xc3", true).as_deref(),
            Some("a\u{fffd}b\u{fffd}")
        );
    }

    #[test]
    fn marks_stand_for_pastes_once_and_no_key_is_a_mark() {
        let mut s = Store::default();
        let a = s.put("one".into());
        let b = s.put("two".into());
        assert!(is_mark(a) && is_mark(b) && a != b);
        assert_eq!(s.take(b).as_deref(), Some("two"));
        assert_eq!(s.take(b), None, "once");
        assert_eq!(s.take(a).as_deref(), Some("one"));
        assert_eq!(s.take(b'a'), None);
        // No valid UTF-8 byte is a mark, so no key makes one.
        for c in ['a', '\u{7ff}', '\u{ffff}', '\u{10ffff}'] {
            let mut buf = [0u8; 4];
            assert!(c.encode_utf8(&mut buf).bytes().all(|b| !is_mark(b)));
        }
        assert!((0x00..=0x7f).all(|b| !is_mark(b)));
    }

    #[test]
    fn the_ninth_paste_replaces_the_first() {
        let mut s = Store::default();
        let first = s.put("first".into());
        for i in 0..7 {
            s.put(alloc::format!("{i}"));
        }
        let ninth = s.put("ninth".into());
        assert_eq!(ninth, first);
        assert_eq!(s.take(first).as_deref(), Some("ninth"));
    }
}

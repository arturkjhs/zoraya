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
    /// A part of a paste; the cleaned text when `last` ends it. `first` begins
    /// a paste: what an earlier one left (it was cut and never ended) is
    /// dropped, not joined to this one.
    pub fn part(&mut self, part: &[u8], first: bool, last: bool) -> Option<String> {
        if first {
            self.bytes.clear();
        }
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
    /// Keeps a paste; the byte that stands for it in the key stream. When all
    /// eight places hold a paste not yet taken the new one is refused (`None`):
    /// the oldest are never replaced, so the order is kept.
    pub fn put(&mut self, text: String) -> Option<u8> {
        let n = self.slots.len();
        let slot = (0..n)
            .map(|i| (self.next + i) % n)
            .find(|s| self.slots[*s].is_none())?;
        self.next = (slot + 1) % n;
        self.slots[slot] = Some(text);
        Some(MARK_BASE + slot as u8)
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

/// Where a key of the stream is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// At the prompt: a paste is inserted into the line.
    Prompt,
    /// While a program runs, or a builtin waits for a key: a paste is dropped.
    Elsewhere,
}

/// What a byte of the key stream is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// A key, for the editor or the program in front.
    Key(u8),
    /// A paste, to insert into the line (cleaned).
    Insert(String),
    /// A paste that is not used (it was read where it cannot be, or it is
    /// gone): the shell says so.
    Dropped,
}

/// The shell's whole paste path, apart from the terminal's messages: the
/// parts of a paste in, keys out, and the place a paste has in them. Nothing
/// else makes a byte 0xF8 to 0xFF, and no `Input` can (INV-0384-3): the shell
/// binary calls these and tests nothing of its own.
#[derive(Debug, Default)]
pub struct Feed {
    assembler: Assembler,
    store: Store,
}

impl Feed {
    /// The keys of an `Input` message, appended to `keys`: every byte but one
    /// that stands for a paste (a peer's, not a key's).
    pub fn input(&mut self, bytes: &[u8], keys: &mut Vec<u8>) {
        keys.extend(bytes.iter().copied().filter(|b| !is_mark(*b)));
    }

    /// A `Paste` message: when it ends a paste, the paste's place is appended
    /// to `keys`. Gives `false` when the paste was refused for want of room
    /// (the shell says so).
    pub fn part(&mut self, bytes: &[u8], first: bool, last: bool, keys: &mut Vec<u8>) -> bool {
        match self.assembler.part(bytes, first, last) {
            Some(text) if !text.is_empty() => match self.store.put(text) {
                Some(mark) => {
                    keys.push(mark);
                    true
                }
                None => false,
            },
            _ => true,
        }
    }

    /// What a byte of the stream is, read `at` the prompt or elsewhere. A
    /// paste is taken (once) whichever it is.
    pub fn route(&mut self, key: u8, at: Reading) -> Routed {
        if !is_mark(key) {
            return Routed::Key(key);
        }
        match (self.store.take(key), at) {
            (Some(text), Reading::Prompt) => Routed::Insert(text),
            _ => Routed::Dropped,
        }
    }

    /// Keys thrown away unread: the pastes among them are freed.
    pub fn discard(&mut self, keys: &[u8]) {
        for k in keys {
            let _ = self.store.take(*k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_joined_until_the_last_and_cleaned() {
        let mut a = Assembler::default();
        assert_eq!(a.part(b"he\x1b[20", true, false), None);
        assert_eq!(a.part("1~llo\r\nw\u{e9}".as_bytes(), false, false), None);
        assert_eq!(
            a.part(b"rld\x07", false, true).as_deref(),
            Some("he[201~llo\nw\u{e9}rld")
        );
        // The assembler starts again.
        assert_eq!(a.part(b"x", true, true).as_deref(), Some("x"));
    }

    #[test]
    fn a_paste_past_the_bound_is_cut_there() {
        let mut a = Assembler::default();
        let part = [b'a'; 4096];
        for _ in 0..(MAX_PASTE_BYTES / 4096) {
            assert_eq!(a.part(&part, false, false), None);
        }
        let text = a.part(b"more", false, true).unwrap();
        assert_eq!(text.len(), MAX_PASTE_BYTES);
        // And a new paste is not short by what the last one left.
        assert_eq!(a.part(b"ok", true, true).as_deref(), Some("ok"));
    }

    #[test]
    fn invalid_utf8_is_replaced_not_kept() {
        let mut a = Assembler::default();
        assert_eq!(
            a.part(b"a\xffb\xc3", true, true).as_deref(),
            Some("a\u{fffd}b\u{fffd}")
        );
    }

    #[test]
    fn marks_stand_for_pastes_once_and_no_key_is_a_mark() {
        let mut s = Store::default();
        let a = s.put("one".into()).unwrap();
        let b = s.put("two".into()).unwrap();
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
    fn the_ninth_paste_is_refused_and_the_first_is_kept() {
        let mut s = Store::default();
        let first = s.put("first".into()).unwrap();
        for i in 0..7 {
            s.put(alloc::format!("{i}")).unwrap();
        }
        assert_eq!(s.put("ninth".into()), None);
        assert_eq!(s.take(first).as_deref(), Some("first"));
        // A place taken is a place free.
        let again = s.put("ninth".into()).unwrap();
        assert_eq!(s.take(again).as_deref(), Some("ninth"));
    }

    /// The path as `zosh` runs it, on hostile input.
    #[test]
    fn no_input_can_be_a_pastes_place_and_a_paste_is_used_once() {
        let mut f = Feed::default();
        let mut keys = Vec::new();
        // Every byte a peer might send, the marks, bracket ends, ESC and C0 among them.
        let hostile: Vec<u8> = (0..=255u8).collect();
        f.input(&hostile, &mut keys);
        assert_eq!(keys.len(), 256 - 8);
        assert!(keys.iter().all(|k| !is_mark(*k)));
        for k in &keys {
            assert_eq!(f.route(*k, Reading::Prompt), Routed::Key(*k));
            assert_eq!(f.route(*k, Reading::Elsewhere), Routed::Key(*k));
        }
        f.input(b"\x1b[201~\xf8\xff\x1b", &mut keys);
        assert!(keys.iter().all(|k| !is_mark(*k)));
        // A forged mark finds no paste.
        for m in MARK_BASE..=255 {
            assert_eq!(f.route(m, Reading::Prompt), Routed::Dropped);
        }
        // A real paste: its place in the stream, inserted at the prompt once,
        // cleaned (the closing bracket mark has lost its ESC).
        let mut stream = Vec::new();
        f.input(b"ab", &mut stream);
        assert!(f.part(b"x\x1b[201~\x1b[200~y\r\n", true, true, &mut stream));
        f.input(b"cd", &mut stream);
        assert_eq!(stream.len(), 5);
        assert_eq!(
            f.route(stream[2], Reading::Prompt),
            Routed::Insert("x[201~[200~y\n".into())
        );
        assert_eq!(f.route(stream[2], Reading::Prompt), Routed::Dropped, "once");
        // Read where a program runs, it is dropped and never a key.
        assert!(f.part(b"p", true, true, &mut stream));
        let m = stream[5];
        assert_eq!(f.route(m, Reading::Elsewhere), Routed::Dropped);
        assert_eq!(f.route(m, Reading::Prompt), Routed::Dropped);
        // Discarded keys free their pastes: eight fit again.
        let mut s2 = Vec::new();
        for _ in 0..8 {
            assert!(f.part(b"q", true, true, &mut s2));
        }
        assert!(!f.part(b"q", true, true, &mut s2), "the ninth is refused");
        f.discard(&s2);
        assert!(f.part(b"q", true, true, &mut s2));
        assert_eq!(s2.len(), 9);
        // An empty paste leaves no place.
        let mut s3 = Vec::new();
        assert!(f.part(b"\x1b\x07", true, true, &mut s3) && s3.is_empty());
    }

    #[test]
    fn a_cut_paste_is_not_joined_to_the_next() {
        let mut a = Assembler::default();
        assert_eq!(a.part(b"head of a cut paste", true, false), None);
        // The next paste begins: nothing of the first is in it.
        assert_eq!(a.part(b"next", true, true).as_deref(), Some("next"));
    }
}

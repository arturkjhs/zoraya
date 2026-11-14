//! What a paste may carry (ADR-0332 item 8, INV-0332-5): the text the person
//! asked for, with `ESC` and every other control character gone except tab
//! and newline, so that it cannot end a bracket, start a sequence or type a
//! Ctrl key. Carriage returns are line ends. The marks that bracket a paste
//! for a program that set mode 2004 are the entry's `PS` and `PE`.

/// The mark that opens a bracketed paste (`PS` of the entry).
pub const BEGIN: &[u8] = b"\x1b[200~";
/// The mark that closes it (`PE`).
pub const END: &[u8] = b"\x1b[201~";
/// The most bytes of one `Paste` message.
pub const MAX_CHUNK: usize = 4096;
/// The most bytes of one paste `[assumption]` (ADR-0332 item 8).
pub const MAX_PASTE_BYTES: usize = 64 * 1024;

/// Whether a character may be in pasted text: not a control character
/// (C0, DEL and C1 are `is_control`), with tab and newline excepted.
pub const fn keeps(c: char) -> bool {
    c == '\t' || c == '\n' || !c.is_control()
}

/// The text of a paste as a program or a line may take it: `\r\n` and a lone
/// `\r` are `\n`, a character that is not [`keeps`] is dropped.
pub fn clean(text: &str) -> Clean<'_> {
    Clean {
        chars: text.chars(),
        carry: None,
    }
}

/// The iterator of [`clean`].
#[derive(Clone)]
pub struct Clean<'a> {
    chars: core::str::Chars<'a>,
    /// A character read past a `\r` that was not its `\n`.
    carry: Option<char>,
}

impl Iterator for Clean<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            let c = self.carry.take().or_else(|| self.chars.next())?;
            if c == '\r' {
                match self.chars.next() {
                    Some('\n') | None => {}
                    Some(other) => self.carry = Some(other),
                }
                return Some('\n');
            }
            if keeps(c) {
                return Some(c);
            }
        }
    }
}

/// The length of the longest prefix of `text` that is at most `limit` bytes
/// and ends at a character boundary.
pub fn boundary(text: &str, limit: usize) -> usize {
    if text.len() <= limit {
        return text.len();
    }
    let mut at = limit;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::String;

    fn cleaned(s: &str) -> String {
        clean(s).collect()
    }

    #[test]
    fn control_characters_and_the_escape_go() {
        assert_eq!(
            cleaned("a\x1b[201~b"),
            "a[201~b",
            "the closing mark is no mark"
        );
        assert_eq!(cleaned("\x1b\x07\x08x\x7f\u{9b}y\u{85}"), "xy");
        assert_eq!(cleaned("a\tb\nc"), "a\tb\nc");
        assert_eq!(cleaned("\0\x01\x1f z"), " z");
        assert_eq!(cleaned("é€😀"), "é€😀");
    }

    #[test]
    fn carriage_returns_are_line_ends() {
        assert_eq!(cleaned("a\r\nb"), "a\nb");
        assert_eq!(cleaned("a\rb"), "a\nb");
        assert_eq!(cleaned("a\r"), "a\n");
        assert_eq!(cleaned("a\r\r\nb"), "a\n\nb");
        assert_eq!(cleaned("\r\x1bx"), "\nx");
        assert_eq!(cleaned("a\r\x1b\nb"), "a\n\nb");
    }

    #[test]
    fn nothing_cleaned_holds_an_escape_or_a_control_but_tab_and_newline() {
        for c in (0u32..0x200).filter_map(char::from_u32) {
            let mut text = String::new();
            text.push('a');
            text.push(c);
            text.push('b');
            for k in clean(&text) {
                assert!(k == '\t' || k == '\n' || !k.is_control(), "{:?}", c);
            }
        }
    }

    #[test]
    fn the_marks_are_the_entry_s() {
        assert_eq!(BEGIN, b"\x1b[200~");
        assert_eq!(END, b"\x1b[201~");
    }

    #[test]
    fn a_boundary_is_never_inside_a_character() {
        let t = "aé€😀";
        for limit in 0..=t.len() + 2 {
            let at = boundary(t, limit);
            assert!(at <= limit && at <= t.len(), "{limit} {at}");
            assert!(t.is_char_boundary(at));
            // The largest boundary that fits.
            assert!(
                (at + 1..=t.len().min(limit)).all(|i| !t.is_char_boundary(i)),
                "{limit} {at}"
            );
        }
        assert_eq!(boundary("abc", 2), 2);
        assert_eq!(boundary("é", 1), 0);
        assert_eq!(boundary("abc", 9), 3);
    }
}

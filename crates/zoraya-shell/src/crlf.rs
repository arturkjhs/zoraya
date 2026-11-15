//! The carriage return the shell writes (ADR-0331 item 8, INV-0331-7).
//!
//! The terminal's line feed keeps the column (ADR-0332 item 2), so every line
//! feed the shell passes to `term` must come after a carriage return: the
//! shell's own output, a native program's, and a POSIX program's exactly
//! when its `OPOST` and `ONLCR` are set (`onlcr` below). Output that already
//! has the carriage return, and a lone one, are left alone. The state is the
//! last byte, so that a chunk that ends with `\r` and the next that starts
//! with `\n` do not get a second one.

use alloc::vec::Vec;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Crlf {
    after_cr: bool,
}

impl Crlf {
    pub const fn new() -> Crlf {
        Crlf { after_cr: false }
    }

    /// A converter that knows whether the last byte written was a carriage
    /// return, for a caller that keeps the state between calls itself.
    pub const fn after(carriage_return: bool) -> Crlf {
        Crlf {
            after_cr: carriage_return,
        }
    }

    /// Whether the last byte seen was a carriage return.
    pub const fn ended_with_cr(&self) -> bool {
        self.after_cr
    }

    /// Appends `bytes` to `out`, with a carriage return before each line
    /// feed that has none when `onlcr` is set, and untouched when it is not.
    pub fn convert(&mut self, bytes: &[u8], onlcr: bool, out: &mut Vec<u8>) {
        for &b in bytes {
            if b == b'\n' && onlcr && !self.after_cr {
                out.push(b'\r');
            }
            out.push(b);
            self.after_cr = b == b'\r';
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    fn run(chunks: &[&[u8]], onlcr: bool) -> Vec<u8> {
        let mut c = Crlf::new();
        let mut out = Vec::new();
        for chunk in chunks {
            c.convert(chunk, onlcr, &mut out);
        }
        out
    }

    /// INV-0331-7: a native program's line feed, and the shell's own, get the
    /// carriage return; a POSIX program's get it exactly when `ONLCR` (under
    /// `OPOST`) is on.
    #[test]
    fn a_line_feed_is_preceded_by_a_carriage_return() {
        assert_eq!(run(&[b"a\nb\n"], true), b"a\r\nb\r\n");
        assert_eq!(run(&[b"a\nb\n"], false), b"a\nb\n", "output processing off");
        assert_eq!(run(&[b"\n\n"], true), b"\r\n\r\n");
    }

    #[test]
    fn what_has_its_carriage_return_keeps_one() {
        assert_eq!(run(&[b"a\r\nb"], true), b"a\r\nb");
        assert_eq!(run(&[b"\r\r\n"], true), b"\r\r\n");
        assert_eq!(
            run(&[b"a\rb"], true),
            b"a\rb",
            "a lone carriage return stays"
        );
        // Split across two writes.
        assert_eq!(run(&[b"a\r", b"\nb"], true), b"a\r\nb");
        assert_eq!(run(&[b"a", b"\nb"], true), b"a\r\nb");
        // The state follows the last byte only.
        assert_eq!(run(&[b"a\r", b"b\nc"], true), b"a\rb\r\nc");
    }

    /// A caller that keeps the state between calls itself (zosh's static) hands it
    /// back with `after` and reads it with `ended_with_cr`.
    #[test]
    fn the_state_can_be_kept_by_the_caller() {
        let mut out = Vec::new();
        let mut c = Crlf::after(true);
        assert!(c.ended_with_cr());
        c.convert(b"\nb", true, &mut out);
        assert_eq!(out, b"\nb", "the carriage return was written before");
        assert!(!c.ended_with_cr());
        let mut c = Crlf::after(false);
        out.clear();
        c.convert(b"a\r", true, &mut out);
        assert!(c.ended_with_cr());
        c.convert(b"\n", true, &mut out);
        assert_eq!(out, b"a\r\n");
        assert!(!c.ended_with_cr());
        assert_eq!(Crlf::new(), Crlf::after(false));
        assert_eq!(Crlf::default(), Crlf::new());
    }

    #[test]
    fn every_line_feed_of_any_bytes_has_one_before_it() {
        let mut x = 0x1234_5678_9abc_def1u64;
        for _ in 0..200 {
            let mut bytes = vec![];
            for _ in 0..64 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                bytes.push([b'\n', b'\r', b'a', 0x1b][(x % 4) as usize]);
            }
            let split = (x >> 8) as usize % 64;
            let out = run(&[&bytes[..split], &bytes[split..]], true);
            for (i, b) in out.iter().enumerate() {
                if *b == b'\n' {
                    assert!(i > 0 && out[i - 1] == b'\r', "{out:?}");
                }
            }
            // Nothing else changes: only carriage returns were added.
            assert_eq!(
                out.iter().filter(|b| **b != b'\r').count(),
                bytes.iter().filter(|b| **b != b'\r').count()
            );
        }
    }
}

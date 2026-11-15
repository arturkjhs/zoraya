//! The protocol between the terminal and the shell (ADR-0096). Output is a
//! VT subset in `Output`; input is the bytes keys produce. The size goes out
//! of band. Nothing the shell sends can make the terminal send input.
//!
//! A paste is not input: it is the text of the clipboard, which the person
//! asked for with System+V, in `Paste` messages that the shell cleans
//! ([`paste::clean`]) and inserts into its line without running it
//! (ADR-0332 item 8, ADR-0384).

#![no_std]
#![forbid(unsafe_code)]

pub mod paste;

use zoraya_protocol::{Data, protocol};

pub const MAX_OUTPUT: usize = 4096;
pub const MAX_INPUT: usize = 256;
/// The longest `Title`, in bytes (a window title's limit less `zosh — `).
pub const MAX_TITLE: usize = 240;
/// The most bytes one `Paste` carries.
pub const MAX_PASTE: usize = paste::MAX_CHUNK;

protocol! {
    /// Shell to terminal, with no reply.
    message Output<'a> = 1, handles 0 {
        bytes: Data<'a, MAX_OUTPUT>,
    }

    /// Terminal to shell: what keys produced, and what was dropped before.
    message Input<'a> = 2, handles 0 {
        dropped: u32,
        bytes: Data<'a, MAX_INPUT>,
    }

    /// Terminal to shell, first and on any change.
    message Size = 3, handles 0 {
        columns: u32,
        rows: u32,
    }

    /// Shell to terminal, out of band (ADR-0168): the shell's current
    /// directory as its prompt shows it, at start and after each `cd`; the
    /// terminal titles its window with it and never from output.
    message Title<'a> = 4, handles 0 {
        text: Data<'a, MAX_TITLE>,
    }

    /// Terminal to shell: a part of the text the person pasted with System+V
    /// (ADR-0332 item 8), in order, each a whole number of characters;
    /// `first` begins a paste (what the shell held of an earlier
    /// one that was cut is dropped) and `last` ends it. Never `Input`: the shell inserts it into its
    /// line, or holds it for a program, after cleaning it. A new ordinal
    /// under ADR-0052.
    message Paste<'a> = 5, handles 0 {
        first: bool,
        last: bool,
        bytes: Data<'a, MAX_PASTE>,
    }

    /// Shell to terminal, out of band, no reply (ADR-0331 item 9): the line
    /// in front ended, stopped or went to the background. The terminal drops
    /// an unfinished sequence, leaves the alternate screen, shows the cursor
    /// and clears the modes of ADR-0332 without erasing the primary screen.
    /// With `keep` (a stop) it holds what it cleared for `Reinstate`. A new
    /// ordinal under ADR-0052; `ttyd` ignores it.
    message Restore = 6, handles 0 {
        keep: bool,
    }

    /// Shell to terminal, no reply: a stopped line is back in front (`fg`);
    /// the terminal sets again what the last `Restore` with `keep` cleared.
    /// With nothing kept it does nothing.
    message Reinstate = 7, handles 0 {}

    /// Shell to terminal, no reply (ADR-0332 item 11, ADR-0389): the setting
    /// `term.scrollback`, which the terminal cannot read itself. At start
    /// and after each change; the terminal keeps at most this many lines
    /// (and never more than its own bound). A new ordinal under ADR-0052;
    /// `ttyd` ignores it.
    message Scrollback = 8, handles 0 {
        lines: u32,
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_protocol::testing::exercise;
    use zoraya_protocol::{decode, encode};

    #[test]
    fn every_message_round_trips_and_decodes_totally() {
        let mut out = [0; 8192];
        let text = b"zosh$ \x1b[2J";
        let length = encode(&Output { bytes: Data(text) }, 0, &mut out).unwrap();
        let (_, m) = decode::<Output<'_>>(&out[..length], 0).unwrap();
        assert_eq!(m.bytes.0, text);
        exercise(&out[..length], |b| {
            let _ = decode::<Output<'_>>(b, 0);
        });
        let length = encode(
            &Input {
                dropped: 2,
                bytes: Data(b"ls\r"),
            },
            0,
            &mut out,
        )
        .unwrap();
        let (_, m) = decode::<Input<'_>>(&out[..length], 0).unwrap();
        assert_eq!((m.dropped, m.bytes.0), (2, &b"ls\r"[..]));
        exercise(&out[..length], |b| {
            let _ = decode::<Input<'_>>(b, 0);
        });
        let size = Size {
            columns: 80,
            rows: 24,
        };
        let length = encode(&size, 0, &mut out).unwrap();
        assert_eq!(decode::<Size>(&out[..length], 0).unwrap().1, size);
        let r = Restore { keep: true };
        let length = encode(&r, 0, &mut out).unwrap();
        assert_eq!(decode::<Restore>(&out[..length], 0).unwrap().1, r);
        exercise(&out[..length], |b| {
            let _ = decode::<Restore>(b, 0);
        });
        let length = encode(&Reinstate {}, 0, &mut out).unwrap();
        decode::<Reinstate>(&out[..length], 0).unwrap();
        let length = encode(&Scrollback { lines: 2000 }, 0, &mut out).unwrap();
        let (_, back) = decode::<Scrollback>(&out[..length], 0).unwrap();
        assert_eq!(back, Scrollback { lines: 2000 });
    }

    #[test]
    fn a_title_round_trips_and_a_long_one_is_refused() {
        let mut out = [0u8; 512];
        let t = Title {
            text: Data("~/проект".as_bytes()),
        };
        let n = encode(&t, NO_REPLY_FOR_TESTS, &mut out).unwrap();
        assert_eq!(decode::<Title<'_>>(&out[..n], 0).unwrap().1, t);
        exercise(&out[..n], |b| {
            let _ = decode::<Title<'_>>(b, 0);
        });
        let long = [b'x'; MAX_TITLE + 1];
        assert!(encode(&Title { text: Data(&long) }, 0, &mut out).is_err());
    }

    const NO_REPLY_FOR_TESTS: u32 = 0;

    #[test]
    fn output_and_input_past_their_limits_are_refused() {
        let mut out = [0; 8192];
        let big = [b'x'; MAX_OUTPUT + 1];
        assert!(encode(&Output { bytes: Data(&big) }, 0, &mut out).is_err());
        let big = [b'x'; MAX_INPUT + 1];
        let m = Input {
            dropped: 0,
            bytes: Data(&big),
        };
        assert!(encode(&m, 0, &mut out).is_err());
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

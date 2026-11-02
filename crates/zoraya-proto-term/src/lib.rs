//! The protocol between the terminal and the shell (ADR-0096). Output is a
//! VT subset in `Output`; input is the bytes keys produce. The size goes out
//! of band. Nothing the shell sends can make the terminal send input.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Data, protocol};

pub const MAX_OUTPUT: usize = 4096;
pub const MAX_INPUT: usize = 256;

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
    }

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

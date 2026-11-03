//! The display protocol between the compositor and the GPU driver
//! (ADR-0090). The compositor attaches a memory object that becomes the
//! scanout's backing, then asks for rectangles to be shown. The driver treats
//! every message as hostile.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, protocol};

/// One of `status`.
pub type Status = Below<6>;

pub mod status {
    pub const OK: u32 = 0;
    /// A rectangle outside the display, or empty.
    pub const BAD_RANGE: u32 = 1;
    /// `Flush` before `Attach`.
    pub const NOT_ATTACHED: u32 = 2;
    /// The device failed the command.
    pub const DEVICE: u32 = 3;
    /// Not a memory object, too small, or attached already.
    pub const BAD_BUFFER: u32 = 4;
    /// The driver does not know the ordinal.
    pub const UNSUPPORTED: u32 = 5;
}

/// `B8G8R8X8`: a pixel is the little-endian word `0x00RRGGBB`.
pub const FORMAT_XRGB: u32 = 1;

protocol! {
    /// The display's size and pixel format.
    message Info = 1, handles 0 {}
    message InfoReply = 1, handles 0 {
        status: Status,
        width: u32,
        height: u32,
        format: u32,
    }

    /// The memory object to show: `width × height × 4` bytes, row by row.
    message Attach = 2, handles 1 {}
    message Done = 2, handles 0 {
        status: Status,
    }

    /// Shows a rectangle of the attached memory. The reply is `Done`.
    message Flush = 3, handles 0 {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }
}

/// Every request's ordinal, for a test that none repeats.
pub const REQUESTS: [(&str, u32); 3] = [("info", 1), ("attach", 2), ("flush", 3)];

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_protocol::testing::exercise;
    use zoraya_protocol::{Message, decode, encode};

    fn total<M>(message: M)
    where
        M: for<'b> Message<'b> + PartialEq + core::fmt::Debug,
    {
        let mut out = [0; 256];
        let length = encode(&message, 1, &mut out).unwrap();
        let (_, back) = decode::<M>(&out[..length], M::HANDLES).unwrap();
        assert_eq!(back, message);
        exercise(&out[..length], |bytes| {
            let _ = decode::<M>(bytes, M::HANDLES);
        });
    }

    #[test]
    fn every_message_round_trips_and_decodes_totally() {
        total(Info {});
        total(InfoReply {
            status: Below(status::OK),
            width: 1024,
            height: 768,
            format: FORMAT_XRGB,
        });
        total(Attach {});
        total(Done {
            status: Below(status::BAD_RANGE),
        });
        total(Flush {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        });
    }

    #[test]
    fn a_status_out_of_range_and_wrong_handles_are_refused() {
        let mut out = [0; 64];
        let length = encode(&Done { status: Below(5) }, 1, &mut out).unwrap();
        out[16] = 6;
        assert!(decode::<Done>(&out[..length], 0).is_err());
        let length = encode(&Attach {}, 1, &mut out).unwrap();
        assert!(decode::<Attach>(&out[..length], 0).is_err());
        assert!(decode::<Attach>(&out[..length], 2).is_err());
    }

    #[test]
    fn no_ordinal_repeats() {
        for (i, (_, a)) in REQUESTS.iter().enumerate() {
            assert_ne!(*a, zoraya_protocol::ERROR_ORDINAL);
            assert!(REQUESTS[i + 1..].iter().all(|(_, b)| a != b));
        }
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

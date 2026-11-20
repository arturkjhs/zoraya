//! The display protocol between the compositor and the GPU driver
//! (ADR-0090). The compositor attaches a memory object that becomes the
//! scanout's backing, then asks for rectangles to be shown. The driver treats
//! every message as hostile.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Data, protocol};

/// The side of the hardware cursor's image: 64 by 64 pixels of
/// `0xAARRGGBB` words (TASK-31, proposed ADR-0467).
pub const CURSOR_SIDE: u32 = 64;
/// The cursor image's bytes.
pub const CURSOR_BYTES: usize = (CURSOR_SIDE * CURSOR_SIDE * 4) as usize;

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
    /// The display's size and pixel format, and its physical size in
    /// millimetres, 0 by 0 when the driver cannot say (ADR-0247).
    message Info = 1, handles 0 {}
    message InfoReply = 1, handles 0 {
        status: Status,
        width: u32,
        height: u32,
        format: u32,
        width_mm: u32,
        height_mm: u32,
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

    /// From the driver, unasked, after an `Attach`: the display now has this
    /// size (ADR-0246). The compositor answers with `Info` and a new `Attach`;
    /// the driver sends no other `Resized` before that `Info`.
    message Resized = 4, handles 0 {
        width: u32,
        height: u32,
    }

    /// The pointer's image, which the compositor draws (never a client:
    /// INV-0467-1), for the host to draw: `CURSOR_BYTES` of `0xAARRGGBB`
    /// words, the hot spot inside the 64 by 64 square, and where the pointer
    /// is now. The reply is `Done`: `OK` once the host has it, `DEVICE` when
    /// it has not (the compositor then draws the pointer itself).
    message SetCursor<'a> = 5, handles 0 {
        hot_x: u32,
        hot_y: u32,
        x: u32,
        y: u32,
        pixels: Data<'a, CURSOR_BYTES>,
    }

    /// The pointer is at `(x, y)` of the display. No reply (send it with
    /// transaction 0): a move is told again with the next position.
    message MoveCursor = 6, handles 0 {
        x: u32,
        y: u32,
    }
}

/// Every ordinal, the driver's `Resized` included, for a test that none
/// repeats.
pub const REQUESTS: [(&str, u32); 6] = [
    ("info", 1),
    ("attach", 2),
    ("flush", 3),
    ("resized", 4),
    ("set-cursor", 5),
    ("move-cursor", 6),
];

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
            width_mm: 330,
            height_mm: 206,
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
        total(Resized {
            width: 1600,
            height: 1000,
        });
        total(MoveCursor { x: 5, y: 6 });
        let pixels = [0x5Au8; CURSOR_BYTES];
        let mut out = std::vec![0; CURSOR_BYTES + 64];
        let set = SetCursor {
            hot_x: 1,
            hot_y: 2,
            x: 3,
            y: 4,
            pixels: Data(&pixels),
        };
        let length = encode(&set, 1, &mut out).unwrap();
        let (_, back) = decode::<SetCursor>(&out[..length], 0).unwrap();
        assert_eq!(back, set);
        // One byte more than an image is refused.
        let long = [0u8; CURSOR_BYTES + 1];
        let big = SetCursor {
            pixels: Data(&long),
            ..set
        };
        let mut out = std::vec![0; CURSOR_BYTES + 64];
        assert!(encode(&big, 1, &mut out).is_err());
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

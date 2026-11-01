//! The block protocol between a filesystem service and a driver (ADR-0054).
//! Data lives in a memory object the service attaches. The driver treats every
//! message here as hostile.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Seq, protocol};

/// Sectors in one request.
pub const MAX_BATCH: usize = 256;

/// The status of every reply.
pub mod status {
    pub const OK: u32 = 0;
    /// The device failed the request.
    pub const FAILED: u32 = 1;
    /// A range lies outside the device or the attached memory.
    pub const BAD_RANGE: u32 = 2;
    /// No memory object is attached.
    pub const NOT_ATTACHED: u32 = 3;
    /// The driver does not know the ordinal.
    pub const UNSUPPORTED: u32 = 4;
}

protocol! {
    /// A piece of the transfer: `count` sectors at `sector`, at `offset` in
    /// the attached memory object.
    record Range {
        sector: u64,
        offset: u64,
        count: u32,
    }

    /// What the device is.
    message Info = 1, handles 0 {}
    message InfoReply = 1, handles 0 {
        status: u32,
        sectors: u64,
        flush: bool,
    }

    /// Attaches the memory object that transfers use.
    message Attach = 2, handles 1 {}
    message Done = 2, handles 0 {
        status: u32,
    }

    message Read = 3, handles 0 {
        range: Range,
    }

    /// A batch, which fails whole, as `BlockDevice::write` does.
    message Write<'a> = 4, handles 0 {
        ranges: Seq<'a, Range, MAX_BATCH>,
    }

    message Flush = 5, handles 0 {}
}

/// Every request's ordinal, for a test that none repeats.
pub const REQUESTS: [(&str, u32); 5] = [
    ("info", 1),
    ("attach", 2),
    ("read", 3),
    ("write", 4),
    ("flush", 5),
];

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_protocol::testing::exercise;
    use zoraya_protocol::{Message, decode, encode};

    const RANGE: Range = Range {
        sector: 8,
        offset: 4096,
        count: 8,
    };

    fn total<M>(message: M)
    where
        M: for<'b> Message<'b> + PartialEq + core::fmt::Debug,
    {
        let mut out = [0; 2048];
        let length = encode(&message, 1, &mut out).unwrap();
        let (_, back) = decode::<M>(&out[..length], M::HANDLES).unwrap();
        assert_eq!(back, message);
        exercise(&out[..length], |bytes| {
            let _ = decode::<M>(bytes, M::HANDLES);
        });
    }

    #[test]
    fn every_fixed_message_round_trips_and_decodes_totally() {
        total(Info {});
        total(InfoReply {
            status: status::OK,
            sectors: 2048,
            flush: true,
        });
        total(Attach {});
        total(Done { status: 1 });
        total(Read { range: RANGE });
        total(Flush {});
    }

    #[test]
    fn a_batch_round_trips_and_decodes_totally() {
        let ranges = [RANGE; 5];
        let mut out = [0; 2048];
        let length = encode(
            &Write {
                ranges: Seq::Of(&ranges),
            },
            1,
            &mut out,
        )
        .unwrap();
        let (_, m) = decode::<Write<'_>>(&out[..length], 0).unwrap();
        assert_eq!(m.ranges.len(), 5);
        assert!(m.ranges.iter().all(|r| r == RANGE));
        exercise(&out[..length], |bytes| {
            if let Ok((_, m)) = decode::<Write<'_>>(bytes, 0) {
                assert!(m.ranges.iter().count() <= MAX_BATCH);
            }
        });
    }

    #[test]
    fn a_batch_past_its_limit_is_refused() {
        let ranges = [RANGE; MAX_BATCH + 1];
        let mut out = [0; 16 * 1024];
        let refused = encode(
            &Write {
                ranges: Seq::Of(&ranges),
            },
            1,
            &mut out,
        );
        assert_eq!(refused, Err(zoraya_protocol::WireError::TooLong));
    }

    #[test]
    fn no_ordinal_repeats() {
        for (i, (_, a)) in REQUESTS.iter().enumerate() {
            assert_ne!(*a, zoraya_protocol::ERROR_ORDINAL);
            assert!(REQUESTS[i + 1..].iter().all(|(_, b)| a != b));
        }
    }
}

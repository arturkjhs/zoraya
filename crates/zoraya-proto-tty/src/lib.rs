//! The terminal service's protocol (ADR-0105). A *terminal connection* is a
//! process's view of the terminal, tagged with the process group its creator
//! declared; the *control connection* is the broker's, and is the only one on
//! which the foreground can change (INV-0105-2). Output does not travel here:
//! it is a byte stream (ADR-0101).
//!
//! Requests are answered in order with the reply of the same ordinal, or
//! with the generic [`zoraya_protocol::ErrorReply`] carrying one of
//! [`status`]. `Read` is held until a line is complete (one pending read per
//! connection). On the control connection `ttyd` also sends the broker
//! [`Suspend`] with transaction [`zoraya_protocol::NO_REPLY`].

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Data, protocol};

/// The largest `Read` reply.
pub const MAX_READ: usize = 4096;
/// `termios`'s `c_cc` (musl's `NCCS`).
pub const NCCS: usize = 32;

pub mod status {
    pub const OK: u32 = 0;
    /// The connection's group is not the foreground group (ADR-0105 item 4).
    pub const NOT_FOREGROUND: u32 = 1;
    pub const INVALID: u32 = 2;
    /// A control request on a terminal connection.
    pub const NOT_ALLOWED: u32 = 3;
    /// A second `Read` or `PollReadable` while one is held.
    pub const BUSY: u32 = 4;
    /// The service's table of connections is full.
    pub const NO_SPACE: u32 = 5;
    pub const UNSUPPORTED: u32 = 6;
    pub const COUNT: u32 = 7;
}

pub type Code = Below<{ status::COUNT }>;

protocol! {
    /// Up to `max` bytes of the next line (canonical mode); an empty reply
    /// is end of file.
    message Read = 1, handles 0 {
        max: u32,
    }
    message ReadReply<'a> = 1, handles 0 {
        bytes: Data<'a, MAX_READ>,
    }

    /// Answered when a `Read` would not block.
    message PollReadable = 2, handles 0 {}
    message PollReadableReply = 2, handles 0 {
        status: Code,
    }

    message WindowSize = 3, handles 0 {}
    message WindowSizeReply = 3, handles 0 {
        rows: u32,
        columns: u32,
    }

    /// The `termios` subset (musl's layout of the four flag words and
    /// `c_cc`).
    message GetAttr = 4, handles 0 {}
    message Attr<'a> = 4, handles 0 {
        iflag: u32,
        oflag: u32,
        cflag: u32,
        lflag: u32,
        cc: Data<'a, NCCS>,
    }

    message SetAttr<'a> = 5, handles 0 {
        iflag: u32,
        oflag: u32,
        cflag: u32,
        lflag: u32,
        cc: Data<'a, NCCS>,
    }
    message SetAttrReply = 5, handles 0 {
        status: Code,
    }

    /// A new terminal connection on the given server end, for a child of
    /// the caller, tagged with `group`.
    message Clone = 6, handles 1 {
        group: u32,
    }
    message CloneReply = 6, handles 0 {
        status: Code,
    }

    /// Control only: the foreground group and the job its signals go to
    /// (`SIGNAL | KILL`).
    message Foreground = 20, handles 1 {
        group: u32,
    }
    message ForegroundReply = 20, handles 0 {
        status: Code,
    }

    /// Control only: the session job, for hangup.
    message Session = 21, handles 1 {}
    message SessionReply = 21, handles 0 {
        status: Code,
    }

    /// Control only: no foreground group; input characters are data.
    message ClearForeground = 22, handles 0 {}
    message ClearForegroundReply = 22, handles 0 {
        status: Code,
    }

    /// Control only: what the service has counted.
    message Statistics = 23, handles 0 {}
    message StatisticsReply = 23, handles 0 {
        lines: u32,
        interrupts: u32,
        suspends: u32,
        not_foreground: u32,
        refused: u32,
        connections: u32,
    }

    /// `ttyd` to the broker, no reply: the suspend character was typed with
    /// a foreground group (ADR-0105 item 3).
    message Suspend = 30, handles 0 {
        signal: u32,
        group: u32,
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_protocol::testing::exercise;
    use zoraya_protocol::{Message, decode, encode};

    macro_rules! round_trip {
        ($t:ty => $m:expr) => {{
            let m = $m;
            let mut out = [0u8; 8192];
            let length = encode(&m, 3, &mut out).unwrap();
            exercise(&out[..length], |b| {
                let _ = decode::<$t>(b, <$t as Message<'_>>::HANDLES);
            });
            let (_, back) = decode::<$t>(&out[..length], <$t as Message<'_>>::HANDLES).unwrap();
            assert_eq!(back, m);
        }};
    }

    #[test]
    fn every_message_round_trips_and_decodes_totally() {
        let cc = [3u8; NCCS];
        round_trip!(Read => Read { max: 4096 });
        round_trip!(ReadReply<'_> => ReadReply {
            bytes: Data(b"echo hi\n")
        });
        round_trip!(PollReadable => PollReadable {});
        round_trip!(PollReadableReply => PollReadableReply {
            status: Below(status::OK)
        });
        round_trip!(WindowSize => WindowSize {});
        round_trip!(WindowSizeReply => WindowSizeReply {
            rows: 24,
            columns: 80
        });
        round_trip!(GetAttr => GetAttr {});
        round_trip!(Attr<'_> => Attr {
            iflag: 1,
            oflag: 2,
            cflag: 3,
            lflag: 4,
            cc: Data(&cc)
        });
        round_trip!(SetAttr<'_> => SetAttr {
            iflag: 1,
            oflag: 2,
            cflag: 3,
            lflag: 4,
            cc: Data(&cc)
        });
        round_trip!(SetAttrReply => SetAttrReply {
            status: Below(status::INVALID)
        });
        round_trip!(Clone => Clone { group: 5 });
        round_trip!(CloneReply => CloneReply {
            status: Below(status::OK)
        });
        round_trip!(Foreground => Foreground { group: 5 });
        round_trip!(ForegroundReply => ForegroundReply {
            status: Below(status::NOT_ALLOWED)
        });
        round_trip!(Session => Session {});
        round_trip!(SessionReply => SessionReply {
            status: Below(status::OK)
        });
        round_trip!(ClearForeground => ClearForeground {});
        round_trip!(ClearForegroundReply => ClearForegroundReply {
            status: Below(status::OK)
        });
        round_trip!(Statistics => Statistics {});
        round_trip!(StatisticsReply => StatisticsReply {
            lines: 1,
            interrupts: 2,
            suspends: 3,
            not_foreground: 4,
            refused: 5,
            connections: 6
        });
        round_trip!(Suspend => Suspend {
            signal: 20,
            group: 5
        });
    }

    #[test]
    fn a_read_past_its_limit_and_a_long_cc_are_refused() {
        let mut out = [0; 8192];
        let big = [b'x'; MAX_READ + 1];
        assert!(encode(&ReadReply { bytes: Data(&big) }, 0, &mut out).is_err());
        let cc = [0u8; NCCS + 1];
        let m = SetAttr {
            iflag: 0,
            oflag: 0,
            cflag: 0,
            lflag: 0,
            cc: Data(&cc),
        };
        assert!(encode(&m, 0, &mut out).is_err());
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

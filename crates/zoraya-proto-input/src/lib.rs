//! The input protocol from an input driver to the compositor (ADR-0092): one
//! message per frame of checked evdev events. No replies: the driver never
//! waits for the compositor, and counts what did not fit.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Seq, protocol};

/// Records in one frame.
pub const MAX_FRAME: usize = 64;

/// Record kinds.
pub mod kind {
    /// `code` is an evdev key or button code below `0x300`; `value` 0
    /// released, 1 pressed, 2 repeated.
    pub const KEY: u32 = 0;
    /// `code` 0 is x, 1 is y; `value` is scaled to `0..=65535`.
    pub const AXIS: u32 = 1;
}

pub const KEY_LIMIT: u32 = 0x300;
pub const AXIS_MAX: u32 = 65535;

protocol! {
    record Event {
        kind: Below<2>,
        code: u32,
        value: u32,
    }

    /// One frame, and the events the driver dropped before it.
    message Frame<'a> = 1, handles 0 {
        dropped: u32,
        events: Seq<'a, Event, MAX_FRAME>,
    }
}

impl Event {
    /// True if the fields are in the ranges ADR-0092 lists. The decoder
    /// checks the kind; a receiver checks the rest with this.
    pub fn is_valid(&self) -> bool {
        match self.kind.0 {
            kind::KEY => self.code < KEY_LIMIT && self.value <= 2,
            _ => self.code <= 1 && self.value <= AXIS_MAX,
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_protocol::testing::exercise;
    use zoraya_protocol::{decode, encode};

    const KEY_A: Event = Event {
        kind: Below(kind::KEY),
        code: 30,
        value: 1,
    };

    #[test]
    fn a_frame_round_trips_and_decodes_totally() {
        let events = [KEY_A; 5];
        let mut out = [0; 4096];
        let length = encode(
            &Frame {
                dropped: 3,
                events: Seq::Of(&events),
            },
            0,
            &mut out,
        )
        .unwrap();
        let (_, m) = decode::<Frame<'_>>(&out[..length], 0).unwrap();
        assert_eq!(m.dropped, 3);
        assert!(m.events.iter().eq(events.iter().copied()));
        exercise(&out[..length], |bytes| {
            if let Ok((_, m)) = decode::<Frame<'_>>(bytes, 0) {
                assert!(m.events.iter().count() <= MAX_FRAME);
                assert!(m.events.iter().all(|e| e.kind.0 < 2));
            }
        });
    }

    #[test]
    fn a_frame_past_its_limit_is_refused() {
        let events = [KEY_A; MAX_FRAME + 1];
        let mut out = [0; 4096];
        let refused = encode(
            &Frame {
                dropped: 0,
                events: Seq::Of(&events),
            },
            0,
            &mut out,
        );
        assert_eq!(refused, Err(zoraya_protocol::WireError::TooLong));
    }

    #[test]
    fn validity_follows_the_adr() {
        let e = |kind, code, value| Event {
            kind: Below(kind),
            code,
            value,
        };
        assert!(e(kind::KEY, 0x2ff, 2).is_valid());
        assert!(!e(kind::KEY, KEY_LIMIT, 1).is_valid());
        assert!(!e(kind::KEY, 30, 3).is_valid());
        assert!(e(kind::AXIS, 1, AXIS_MAX).is_valid());
        assert!(!e(kind::AXIS, 2, 0).is_valid());
        assert!(!e(kind::AXIS, 0, AXIS_MAX + 1).is_valid());
    }
}

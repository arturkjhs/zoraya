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
    /// A wheel turned (proposed ADR-0322): `code` 0 is the vertical wheel,
    /// 1 the horizontal one; `value` is an `i32` in its two's complement,
    /// in 120ths of a notch (`zoraya_proto_gui::WHEEL_STEP`), not 0 and at
    /// most [`WHEEL_MAX`] either way. Above 0 is toward the person (the
    /// view moves down) and to the right: the driver turns evdev's sign of
    /// `REL_WHEEL`, which is the other way.
    pub const WHEEL: u32 = 2;
}

pub const KEY_LIMIT: u32 = 0x300;
pub const AXIS_MAX: u32 = 65535;
/// The most one wheel record carries either way: 16 notches.
pub const WHEEL_MAX: i32 = 16 * 120;

protocol! {
    record Event {
        kind: Below<3>,
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
            kind::AXIS => self.code <= 1 && self.value <= AXIS_MAX,
            _ => {
                let v = self.value as i32;
                self.code <= 1 && v != 0 && v.unsigned_abs() <= WHEEL_MAX.unsigned_abs()
            }
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
                assert!(m.events.iter().all(|e| e.kind.0 < 3));
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
        // Wheels (proposed ADR-0322): signed, not zero, bounded, two codes.
        let w = |code, v: i32| e(kind::WHEEL, code, v as u32);
        assert!(w(0, 120).is_valid() && w(0, -120).is_valid() && w(1, 1).is_valid());
        assert!(w(0, WHEEL_MAX).is_valid() && w(1, -WHEEL_MAX).is_valid());
        assert!(!w(0, 0).is_valid() && !w(2, 120).is_valid());
        assert!(!w(0, WHEEL_MAX + 1).is_valid() && !w(0, -WHEEL_MAX - 1).is_valid());
        assert!(!w(0, i32::MIN).is_valid());
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

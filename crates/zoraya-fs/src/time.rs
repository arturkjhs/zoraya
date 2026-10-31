//! Timestamps and the clock the engine reads (ADR-0014 rules 2 and 3).

/// Signed seconds since 1970-01-01T00:00:00 UTC plus nanoseconds
/// (0 to 999,999,999), the shape of `struct timespec` (ADR-0014 rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp {
    pub secs: i64,
    pub nanos: u32,
}

pub const NANOS_PER_SEC: u32 = 1_000_000_000;

impl Timestamp {
    pub const MIN: Timestamp = Timestamp {
        secs: i64::MIN,
        nanos: 0,
    };
    pub const MAX: Timestamp = Timestamp {
        secs: i64::MAX,
        nanos: NANOS_PER_SEC - 1,
    };

    /// A timestamp, or `None` if `nanos` is out of range (INV-0014-4).
    pub const fn new(secs: i64, nanos: u32) -> Option<Timestamp> {
        if nanos < NANOS_PER_SEC {
            Some(Timestamp { secs, nanos })
        } else {
            None
        }
    }

    pub const fn is_valid(&self) -> bool {
        self.nanos < NANOS_PER_SEC
    }

    /// The next representable time, one nanosecond later; saturates at
    /// [`Timestamp::MAX`].
    pub const fn succ(self) -> Timestamp {
        if self.nanos + 1 < NANOS_PER_SEC {
            Timestamp {
                secs: self.secs,
                nanos: self.nanos + 1,
            }
        } else if self.secs < i64::MAX {
            Timestamp {
                secs: self.secs + 1,
                nanos: 0,
            }
        } else {
            self
        }
    }
}

/// What the engine needs from its environment's clocks.
pub trait Clock {
    /// The system wall clock, UTC (ADR-0014 rule 3). May stand still or step
    /// backwards; the engine copes.
    fn wall(&mut self) -> Timestamp;
    /// A monotonic clock in nanoseconds, used only to decide when `T_COMMIT`
    /// has passed (ADR-0010 rule 5, ADR-0011 rule 2).
    fn monotonic_ns(&mut self) -> u64;
}

//! Test support for `zoraya-fs` (TASK-07): block devices that model the
//! ADR-0015 contract, the ADR-0010 reference model, the crash harness and
//! the structure map used by the fuzz tests.
//!
//! Host-only (`std`). Deterministic: every random choice comes from a seeded
//! [`Rng`], so a failure reproduces from its seed.

pub mod clock;
pub mod crash;
pub mod devices;
pub mod fields;
pub mod model;
pub mod ops;
pub mod rng;
pub mod state;
pub mod walk;

pub use clock::TestClock;
pub use devices::{Event, FaultDevice, FaultPlan, ImageDevice, MemDevice, RecordingDevice};
pub use rng::Rng;

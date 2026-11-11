//! The network service's logic (ADR-0221 item 3, ADR-0229): smoltcp's
//! interface over a frame [`Link`], the DHCP client, an ICMP echo run for
//! diagnosis, `status`, the `net.*` settings, and the per-connection scope
//! and rights records. Nothing here needs the machine: `netd` supplies a
//! link made of the driver's channel and slots, the host tests supply scripted
//! peers. Every frame is parsed by smoltcp from a private copy (see `link`).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod config;
pub mod connection;
pub mod link;
pub mod nic;
pub mod service;
pub mod stack;

pub use config::{Config, Mode};
pub use connection::{Connection, Request, Verdict};
pub use link::{Counters, Link, LinkDevice, MAX_FRAME};
pub use stack::{Event, Lease, NewError, PingError, PingResult, Source, Stack, Status};

#[cfg(test)]
mod tests;

#[cfg(test)]
zoraya_test_alloc::cap_memory!();

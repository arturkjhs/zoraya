//! What `zosh` does without the kernel (TASK-11, TASK-18).
//!
//! The language of ADR-0150 and ADR-0151 is in [`syntax`] (the parser),
//! [`glob`], [`record`] (exit records in words) and [`eval`] (the
//! evaluator over a [`eval::Host`]); the line editor of ADR-0154 is
//! [`editor`]; the job table and its notices of ADR-0158 are [`jobs`].

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod crlf;
pub mod delivery;
pub mod editor;
pub mod eval;
pub mod glob;
pub mod grantline;
pub mod grants;
pub mod installed;
pub mod jobs;
pub mod netgrant;
pub mod paste;
pub mod record;
pub mod refusal;
pub mod repository;
pub mod syntax;
pub mod terminal;
pub mod tty_host;

#[cfg(test)]
mod editor_tests;
#[cfg(test)]
mod grants_tests;
#[cfg(test)]
mod installed_tests;
#[cfg(test)]
mod jobs_tests;
#[cfg(test)]
mod lang_tests;
#[cfg(test)]
#[path = "netgrant_tests.rs"]
mod netgrant_tests;
#[cfg(test)]
mod repository_tests;
#[cfg(test)]
mod status_tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

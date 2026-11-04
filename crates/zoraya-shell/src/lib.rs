//! What `zosh` does without the kernel (TASK-11, TASK-18).
//!
//! The language of ADR-0150 and ADR-0151 is in [`syntax`] (the parser),
//! [`glob`], [`record`] (exit records in words) and [`eval`] (the
//! evaluator over a [`eval::Host`]); the line editor of ADR-0154 is
//! [`editor`]; the job table and its notices of ADR-0158 are [`jobs`].

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod editor;
pub mod eval;
pub mod glob;
pub mod grants;
pub mod jobs;
pub mod record;
pub mod syntax;

#[cfg(test)]
mod editor_tests;
#[cfg(test)]
mod grants_tests;
#[cfg(test)]
mod jobs_tests;
#[cfg(test)]
mod lang_tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

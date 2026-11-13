//! The Rust half of `libposix` (ADR-0100): the functions of the tier list
//! that touch the machine, on native handles. musl's retained C is the other
//! half; both are archived into `libc.a` by `ports/Makefile`.
//!
//! Only the `ffi` module has `unsafe` (INV-0100-3): every other module
//! forbids it, and is the logic, tested on the host against a fake kernel
//! and the real filesystem service. `ffi` exists only for Zoraya's target.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod errno;
mod fcntl_locks;
pub mod fd;
pub mod jobs;
pub mod lock;
pub mod net;
pub mod path;
pub mod process;
pub mod random;
pub mod records;
pub mod signals;
pub mod socket;
pub mod stat;
pub mod sys;
pub mod threads;
pub mod wait;

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
mod ffi;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

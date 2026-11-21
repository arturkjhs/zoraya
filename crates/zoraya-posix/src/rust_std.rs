//! What libposix answers for Rust's `std` (ADR-0451): the fixed `syscall()`
//! table, the `futex` forms `std` sends and the once-per-number report of an
//! unanswered call. The decisions are here, on the host; the C boundary
//! (`ffi.rs`) only carries pointers and results.
//!
//! `std` reaches these through its `syscall!` macro, which falls back to
//! `libc::syscall` because a weak reference never pulls a function out of a
//! static archive (rust-std/survey.md#what-std-calls-through-syscall). The
//! numbers are Linux's AArch64 numbers, the few `std` makes, and a list (CP-27,
//! DM-057): a new number is a new ADR, never a silent row.

#![forbid(unsafe_code)]

use core::sync::atomic::{AtomicI64, Ordering};

use crate::errno::{EINVAL, ENOSYS, ETIMEDOUT};

/// `SYS_futex` on Linux AArch64.
pub const SYS_FUTEX: i64 = 98;
/// `SYS_getrandom`.
pub const SYS_GETRANDOM: i64 = 278;
/// `SYS_gettid`.
pub const SYS_GETTID: i64 = 178;
/// The calls `std` makes through `syscall()` and falls back from on `ENOSYS`
/// (ADR-0451 item 1): not reported.
pub const SYS_SPLICE: i64 = 76;
pub const SYS_COPY_FILE_RANGE: i64 = 285;
pub const SYS_STATX: i64 = 291;
pub const SYS_PIDFD_SEND_SIGNAL: i64 = 424;
pub const SYS_PIDFD_OPEN: i64 = 434;

/// What `syscall(number, ...)` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Number {
    Futex,
    Getrandom,
    Gettid,
    /// `ENOSYS`, which `std` falls back from: no report.
    Fallback,
    /// `ENOSYS`, reported once per number.
    Unanswered,
}

/// The answer to `number`.
pub const fn classify(number: i64) -> Number {
    match number {
        SYS_FUTEX => Number::Futex,
        SYS_GETRANDOM => Number::Getrandom,
        SYS_GETTID => Number::Gettid,
        SYS_STATX | SYS_COPY_FILE_RANGE | SYS_SPLICE | SYS_PIDFD_OPEN | SYS_PIDFD_SEND_SIGNAL => {
            Number::Fallback
        }
        _ => Number::Unanswered,
    }
}

const FUTEX_WAIT: i64 = 0;
const FUTEX_WAKE: i64 = 1;
const FUTEX_WAIT_BITSET: i64 = 9;
const FUTEX_PRIVATE_FLAG: i64 = 128;
/// `FUTEX_BITSET_MATCH_ANY`: the only mask `std` sends.
pub const FUTEX_BITSET_MATCH_ANY: u32 = 0xffff_ffff;

/// The `futex` forms `syscall()` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Futex {
    /// `FUTEX_WAIT`: the timeout, if any, is relative.
    Wait,
    Wake,
    /// `FUTEX_WAIT_BITSET` with the full mask: the timeout, if any, is an
    /// absolute time on `CLOCK_MONOTONIC`.
    WaitAbsolute,
}

/// The form `op` and `mask` (the sixth argument) name, with or without
/// `FUTEX_PRIVATE_FLAG`; anything else (a mask other than the full one,
/// `FUTEX_CLOCK_REALTIME`, requeues, priority inheritance) is `ENOSYS`.
pub fn futex_form(op: i64, mask: i64) -> Result<Futex, i32> {
    match op & !FUTEX_PRIVATE_FLAG {
        FUTEX_WAIT => Ok(Futex::Wait),
        FUTEX_WAKE => Ok(Futex::Wake),
        FUTEX_WAIT_BITSET if mask as u32 == FUTEX_BITSET_MATCH_ANY => Ok(Futex::WaitAbsolute),
        _ => Err(ENOSYS),
    }
}

/// The deadline, in nanoseconds of the monotonic clock, of an absolute
/// `struct timespec` on `CLOCK_MONOTONIC` (`FUTEX_WAIT_BITSET`): in the past
/// or negative is `ETIMEDOUT`, a nanosecond count outside a second `EINVAL`.
/// `infinite` is the clock's "never"; a time beyond it waits just short of it.
pub fn absolute_deadline(sec: i64, nsec: i64, infinite: u64) -> Result<u64, i32> {
    if !(0..1_000_000_000).contains(&nsec) {
        return Err(EINVAL);
    }
    if sec < 0 {
        return Err(ETIMEDOUT);
    }
    Ok((sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(nsec as u64)
        .min(infinite - 1))
}

/// `ioctl`'s `FIONBIO` on Linux AArch64.
pub const FIONBIO: i64 = 0x5421;

/// How many numbers are remembered: a process reports at most 16 things
/// (`MAX_REPORTS`), so a seventeenth number would not be shown anyway.
const REMEMBERED: usize = 16;
/// The empty slot: a number that is none of `Seen`'s (`i64::MIN` is outside the numbers it
/// remembers, which are `0..=u32::MAX`; `first_time` says false for the rest).
const EMPTY: i64 = i64::MIN;

/// The numbers `syscall()` has reported, so that each is reported once.
pub struct Seen {
    numbers: [AtomicI64; REMEMBERED],
}

impl Default for Seen {
    fn default() -> Seen {
        Seen::new()
    }
}

impl Seen {
    pub const fn new() -> Seen {
        Seen {
            numbers: [const { AtomicI64::new(EMPTY) }; REMEMBERED],
        }
    }

    /// True the first time `number` is offered. A table that is full says
    /// false for a new number: a program that tries more numbers than it
    /// can be told about is not told about the later ones.
    pub fn first_time(&self, number: i64) -> bool {
        // A number outside `0..=u32::MAX` is not a Linux number: never remembered, so never reported.
        if !(0..=i64::from(u32::MAX)).contains(&number) {
            return false;
        }
        for slot in &self.numbers {
            match slot.compare_exchange(EMPTY, number, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return true,
                Err(now) if now == number => return false,
                Err(_) => {}
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_three_numbers_and_five_fallbacks() {
        assert_eq!(classify(98), Number::Futex);
        assert_eq!(classify(278), Number::Getrandom);
        assert_eq!(classify(178), Number::Gettid);
        for n in [291, 285, 76, 434, 424] {
            assert_eq!(classify(n), Number::Fallback, "{n}");
        }
        for n in [0, 1, 9999, -1, 279, (1 << 32) | 98, i64::MIN, i64::MAX] {
            assert_eq!(classify(n), Number::Unanswered, "{n}");
        }
    }

    #[test]
    fn futex_forms_are_the_ones_std_sends() {
        assert_eq!(futex_form(0, 0), Ok(Futex::Wait));
        assert_eq!(futex_form(128, 0), Ok(Futex::Wait));
        assert_eq!(futex_form(1, 0), Ok(Futex::Wake));
        assert_eq!(futex_form(129, 7), Ok(Futex::Wake));
        let full = i64::from(FUTEX_BITSET_MATCH_ANY);
        assert_eq!(futex_form(9, full), Ok(Futex::WaitAbsolute));
        assert_eq!(futex_form(9 | 128, full), Ok(Futex::WaitAbsolute));
        // `std` passes a `u32` in a `long`: the upper half is not part of the mask.
        assert_eq!(futex_form(9, -1), Ok(Futex::WaitAbsolute));
        assert_eq!(futex_form(9 | 128, 0xffff_ffff), Ok(Futex::WaitAbsolute));
    }

    #[test]
    fn every_other_futex_form_is_enosys() {
        for (op, mask) in [
            (9, 1),
            (9, 0),
            (9, 0x7fff_ffff),
            (9 | 256, i64::from(FUTEX_BITSET_MATCH_ANY)),
            (256, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (10, i64::from(FUTEX_BITSET_MATCH_ANY)),
            (11, 0),
            (13, 0),
            (-1, 0),
            (1 << 40, 0),
        ] {
            assert_eq!(futex_form(op, mask), Err(ENOSYS), "{op} {mask}");
        }
    }

    #[test]
    fn an_absolute_deadline_is_the_timespec_in_nanoseconds() {
        let never = u64::MAX;
        assert_eq!(absolute_deadline(0, 0, never), Ok(0));
        assert_eq!(absolute_deadline(3, 5, never), Ok(3_000_000_005));
        assert_eq!(absolute_deadline(-1, 0, never), Err(ETIMEDOUT));
        assert_eq!(absolute_deadline(0, -1, never), Err(EINVAL));
        assert_eq!(absolute_deadline(0, 1_000_000_000, never), Err(EINVAL));
        // A time past the clock's range waits, short of "never".
        assert_eq!(absolute_deadline(i64::MAX, 0, never), Ok(never - 1));
        assert_eq!(absolute_deadline(1 << 40, 0, 1000), Ok(999));
    }

    #[test]
    fn a_number_is_reported_once() {
        let seen = Seen::new();
        assert!(seen.first_time(279));
        assert!(!seen.first_time(279));
        assert!(seen.first_time(1));
        assert!(seen.first_time(0));
        assert!(!seen.first_time(0));
        // Outside `0..=u32::MAX`: never remembered, never first (including i64::MIN, the empty slot).
        for n in [-1, i64::MIN, i64::MAX, 1 << 32] {
            assert!(!seen.first_time(n), "{n}");
        }
        // The table holds 16; a seventeenth number is not remembered.
        for n in 1000..1013 {
            assert!(seen.first_time(n), "{n}");
        }
        assert!(!seen.first_time(5000));
        assert!(!seen.first_time(279));
    }
}

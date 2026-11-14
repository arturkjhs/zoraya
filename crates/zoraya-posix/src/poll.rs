//! `poll` and `ppoll` (ADR-0342 item 5, ADR-0414 item 1): the readiness of a
//! list of descriptors, over the same wait as `pselect` (ADR-0315's deferred
//! protocol: this method looks, and if nothing is ready asks `wait_for`, which
//! returns `PENDING` in a process with threads and is run again with the
//! outcome). The C boundary (`ffi/procs.rs`) owns the timeout, the signal mask
//! and the copy of the caller's array.
//!
//! What it reports, by kind of descriptor (`INV-0414-1`: `POLLIN` exactly when
//! a non-blocking read would not fail with `EAGAIN`, `POLLOUT` likewise):
//!
//! - a pipe or FIFO end and a connected socket: the handle's signals.
//!   Readable, or the peer closed (the read returns 0), is `POLLIN` and, for
//!   a closed peer, `POLLHUP`. Writable is `POLLOUT`; a closed reader is
//!   `POLLERR` with `POLLOUT` (the write fails with `EPIPE` at once).
//! - a connecting socket: its control channel's message, for both directions.
//! - a file, a directory, `/dev/null`, a socket with no connection and a
//!   direction with no end: ready at once, as `select` has it.
//! - a terminal: ready at once, as `select` has it, until the readiness
//!   request of ADR-0331 item 5 exists (stage 10 part 10c replaces that
//!   branch; INV-0342-2 then holds for it too).
//! - a descriptor that is not open: `POLLNVAL`. A negative `fd` is skipped
//!   (`revents` 0), as POSIX has it.
//!
//! `POLLPRI` is never reported (no out-of-band data exists), and a request for
//! it alone waits for `POLLHUP`/`POLLERR` only.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use crate::process::{Desc, Posix};
use crate::sys::{Sys, signals};

pub const POLLIN: i16 = 0x001;
pub const POLLPRI: i16 = 0x002;
pub const POLLOUT: i16 = 0x004;
pub const POLLERR: i16 = 0x008;
pub const POLLHUP: i16 = 0x010;
pub const POLLNVAL: i16 = 0x020;
pub const POLLRDNORM: i16 = 0x040;
pub const POLLRDBAND: i16 = 0x080;
pub const POLLWRNORM: i16 = 0x100;
pub const POLLWRBAND: i16 = 0x200;

/// `struct pollfd` as the C library lays it out.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

/// The most entries a call takes: a table holds `OPEN_MAX` descriptors, so a
/// longer array has repeats at best (POSIX: `EINVAL` above `RLIMIT_NOFILE`).
pub const NFDS_MAX: u64 = zoraya_abi::posix::OPEN_MAX as u64;

const IN_BITS: i16 = POLLIN | POLLRDNORM;
const OUT_BITS: i16 = POLLOUT | POLLWRNORM;

/// The length of the caller's array: `EINVAL` above `NFDS_MAX`, `EFAULT` for
/// a missing array that has entries (no array and no entries is a plain wait).
pub fn array_len(nfds: u64, array_is_null: bool) -> Result<usize, i32> {
    if nfds > NFDS_MAX {
        Err(crate::errno::EINVAL)
    } else if nfds > 0 && array_is_null {
        Err(crate::errno::EFAULT)
    } else {
        Ok(nfds as usize)
    }
}

/// `poll`'s timeout: milliseconds from `now`, none (wait for ever) if
/// negative; saturates instead of wrapping.
pub fn deadline_after_ms(now: u64, ms: i32) -> Option<u64> {
    u64::try_from(ms)
        .ok()
        .map(|ms| now.saturating_add(ms.saturating_mul(1_000_000)))
}

/// `ppoll`'s `struct timespec`: `EINVAL` for a negative field or more than
/// 999,999,999 nanoseconds; saturates instead of wrapping.
pub fn deadline_after_timespec(now: u64, sec: i64, nsec: i64) -> Result<u64, i32> {
    if sec < 0 || !(0..1_000_000_000).contains(&nsec) {
        return Err(crate::errno::EINVAL);
    }
    Ok(now.saturating_add(
        (sec as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(nsec as u64),
    ))
}

/// One handle that tells whether a descriptor is ready.
struct Probe<H> {
    end: H,
    /// The signals that mean "do not wait any more".
    mask: u32,
    /// Added to `revents` when one of `mask` is asserted.
    ready: i16,
    /// Added to `revents` when the peer has closed.
    closed: i16,
}

impl<S: Sys> Posix<S> {
    /// One look at `fds` and, if nothing is ready and the deadline has not
    /// passed, one wait: `Some(n)` with `revents` filled in (`n` entries have
    /// any, 0 for a timeout), `None` to look again after a wait that ended
    /// for a reason other than a source (the caller loops).
    pub fn poll_once(
        &mut self,
        fds: &mut [PollFd],
        deadline: Option<u64>,
    ) -> Result<Option<i32>, i32> {
        let mut count = 0;
        let mut sources: Vec<(S::H, u32)> = Vec::new();
        for f in fds.iter_mut() {
            f.revents = 0;
            if f.fd < 0 {
                continue;
            }
            let (now, probes) = match self.probes(f.fd, f.events) {
                Some(x) => x,
                None => {
                    f.revents = POLLNVAL;
                    count += 1;
                    continue;
                }
            };
            let mut rev = now;
            for pr in &probes {
                match self.sys.signals_of(pr.end) {
                    Ok(s) => {
                        if s & pr.mask != 0 {
                            rev |= pr.ready;
                        }
                        if s & signals::PEER_CLOSED != 0 {
                            rev |= pr.closed;
                        }
                    }
                    // A kernel that cannot say (the host's fakes): ready.
                    Err(_) => rev |= pr.ready,
                }
            }
            if rev == 0 {
                sources.extend(probes.iter().map(|pr| (pr.end, pr.mask)));
            } else {
                f.revents = rev;
                count += 1;
            }
        }
        if count > 0 {
            return Ok(Some(count));
        }
        if deadline.is_some_and(|d| self.sys.now() >= d) {
            return Ok(Some(0));
        }
        match self.wait_for(&sources, deadline)? {
            (None, None) => Ok(Some(0)),
            _ => Ok(None),
        }
    }

    /// What an open descriptor is ready for at once (`revents` bits) and the
    /// handles that tell the rest; `None` if `fd` is not open.
    #[allow(clippy::type_complexity)] // a pair, used once
    fn probes(&self, fd: i32, events: i16) -> Option<(i16, Vec<Probe<S::H>>)> {
        let ins = events & IN_BITS;
        let outs = events & OUT_BITS;
        let (want_in, want_out) = (ins != 0, outs != 0);
        let mut now = 0;
        let mut probes = Vec::new();
        let read_probe = |end, mask| Probe {
            end,
            mask,
            ready: ins,
            closed: POLLHUP,
        };
        let write_probe = |end, mask| Probe {
            end,
            mask,
            ready: outs,
            closed: POLLERR | outs,
        };
        match self.fds.get(fd)? {
            Desc::Stream {
                read, write, tty, ..
            } if !*tty => {
                if want_in {
                    match read {
                        Some(e) => {
                            probes.push(read_probe(*e, signals::READABLE | signals::PEER_CLOSED))
                        }
                        None => now |= ins,
                    }
                }
                if want_out {
                    match write {
                        Some(e) => {
                            probes.push(write_probe(*e, signals::WRITABLE | signals::PEER_CLOSED))
                        }
                        None => now |= outs,
                    }
                }
                if !want_in && !want_out {
                    // Only a hang-up or an error can be told.
                    match (read, write) {
                        (Some(e), _) => probes.push(read_probe(*e, signals::PEER_CLOSED)),
                        (None, Some(e)) => probes.push(write_probe(*e, signals::PEER_CLOSED)),
                        (None, None) => {}
                    }
                }
            }
            Desc::Socket(sk) => {
                if want_in {
                    match sk.wait_source(false) {
                        Some((e, m)) => probes.push(read_probe(e, m)),
                        None => now |= ins,
                    }
                }
                if want_out {
                    match sk.wait_source(true) {
                        Some((e, m)) => probes.push(write_probe(e, m)),
                        None => now |= outs,
                    }
                }
                if !want_in
                    && !want_out
                    && let Some((e, _)) = sk.wait_source(false)
                {
                    probes.push(read_probe(e, signals::PEER_CLOSED));
                }
            }
            // Files, directories, `/dev/null`, terminals: ready at once.
            _ => now |= ins | outs,
        }
        Some((now, probes))
    }
}

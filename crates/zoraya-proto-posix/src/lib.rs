//! The broker's protocol (ADR-0104): one connection per POSIX process, made
//! by the broker before the process exists and moved into it at slot
//! `BROKER`; a client connection with no pid for a native shell (ADR-0153's
//! note on ADR-0104). The broker serves requests in arrival order and answers
//! each with the reply of the same ordinal, or with the generic
//! [`zoraya_protocol::ErrorReply`] carrying one of [`status`] when the reply
//! would carry handles or data it does not have.
//!
//! Pids are positive and below 2^31 (ADR-0104 item 2); a target of `kill`
//! that C writes as a negative or zero pid travels as a [`target`] and a
//! positive number.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, protocol};

/// The status of every reply.
pub mod status {
    pub const OK: u32 = 0;
    /// `ESRCH`: no such pid, or it was reaped.
    pub const NO_SUCH_PROCESS: u32 = 1;
    /// `EPERM`: outside the session, or a move between groups.
    pub const NOT_PERMITTED: u32 = 2;
    /// `EACCES`: `setpgid` on a child after its `exec`.
    pub const ACCESS_DENIED: u32 = 3;
    /// `EAGAIN`: pids exhausted, a full request queue, no memory for a job.
    pub const AGAIN: u32 = 4;
    /// `EINVAL`: a malformed request, a bad signal number.
    pub const INVALID: u32 = 5;
    /// `ENOTTY`: the session has no terminal.
    pub const NO_TERMINAL: u32 = 6;
    /// `ECHILD`: `reap` of a pid that is not the caller's child.
    pub const NOT_CHILD: u32 = 7;
    /// `reap` before the process job is `TERMINATED`.
    pub const NOT_TERMINATED: u32 = 8;
    /// The broker does not know the ordinal.
    pub const UNSUPPORTED: u32 = 9;
    pub const COUNT: u32 = 10;
}

pub type Code = Below<{ status::COUNT }>;

/// `spawn_prepare`'s group forms (ADR-0104 item 4); the fourth, a job of the
/// caller's (`Group(job)`), is [`SpawnPrepareIn`].
pub mod group {
    /// The caller's group.
    pub const INHERIT: u32 = 0;
    /// A new group led by the child.
    pub const NEW: u32 = 1;
    /// The group `pgid` names, which must be in the session.
    pub const JOIN: u32 = 2;
    pub const COUNT: u32 = 3;
}

/// What a `lookup` asks.
pub mod lookup {
    pub const PGID: u32 = 0;
    pub const PPID: u32 = 1;
    pub const SID: u32 = 2;
    /// The signal that stopped the pid (`WSTOPSIG`), 0 when it is not
    /// stopped. Allowed to the parent.
    pub const STOP_SIGNAL: u32 = 3;
    pub const COUNT: u32 = 4;
}

/// A stop or continue signal's POSIX disposition (ADR-0104 item 8a).
pub mod stop {
    pub const DEFAULT: u32 = 0;
    pub const IGNORE: u32 = 1;
    pub const HANDLER: u32 = 2;
    pub const COUNT: u32 = 3;
}

/// What `kill` names (ADR-0104 item 9): C's `pid > 0`, `0`, `-1` and
/// `-pgid`.
pub mod target {
    pub const PID: u32 = 0;
    pub const OWN_GROUP: u32 = 1;
    pub const ALL: u32 = 2;
    pub const GROUP: u32 = 3;
    pub const COUNT: u32 = 4;
}

/// The ordinals the broker counts, for the counting broker of INV-0002-3 and
/// INV-0104-5: [`Statistics`] gives one count per ordinal below this.
pub const ORDINALS: usize = 16;

protocol! {
    /// A pid, its process job and the child's broker connection, before the
    /// child exists. Handles of the reply: the parent's job handle (`WAIT |
    /// DUPLICATE | TRANSFER`), the child's (`CREATE_PROCESS | TRANSFER`), the
    /// child's broker end.
    message SpawnPrepare = 1, handles 0 {
        group: Below<{ group::COUNT }>,
        pgid: u32,
        /// For `posix_spawn`: the stop signals the child starts ignoring
        /// (bit n for signal n); for `fork` the caller's records are copied
        /// and this is ignored.
        ignored: u32,
        /// `fork` (1) or `posix_spawn` (0): what the child's record starts as.
        fork: Below<2>,
    }
    message SpawnPrepared = 1, handles 3 {
        pid: u32,
        pgid: u32,
        sid: u32,
    }

    /// `spawn_prepare` in a `posix` job of the caller's tree, given as the
    /// one handle (`Group(job)`, ADR-0153): the broker creates a group job
    /// in it, led by the child. The reply is [`SpawnPrepared`]'s shape.
    message SpawnPrepareIn = 2, handles 1 {
        ignored: u32,
    }
    message SpawnPreparedIn = 2, handles 3 {
        pid: u32,
        pgid: u32,
        sid: u32,
    }

    /// The process `spawn` or `snapshot_process` made for `pid`, with
    /// `WAIT | SIGNAL`.
    message SpawnCommit = 3, handles 1 {
        pid: u32,
    }
    message SpawnCommitReply = 3, handles 0 {
        status: Code,
    }

    /// The spawn failed: the pid, its job and its channel go.
    message SpawnAbort = 4, handles 0 {
        pid: u32,
    }
    message SpawnAbortReply = 4, handles 0 {
        status: Code,
    }

    /// The caller has spawned its new image into its own process job and
    /// exits after the reply (ADR-0104 item 6).
    message ExecCommit = 5, handles 1 {}
    message ExecCommitReply = 5, handles 0 {
        status: Code,
    }

    /// The exit record of a terminated child, which is then gone.
    message Reap = 6, handles 0 {
        pid: u32,
    }
    message Reaped = 6, handles 0 {
        status: Code,
        code: u32,
        reason: u32,
        detail: u32,
        /// `RequestKind::raw() + 1`, or 0 (`ExitRecord`'s byte form).
        kind: u32,
    }

    message SetPgid = 7, handles 0 {
        pid: u32,
        pgid: u32,
    }
    message SetPgidReply = 7, handles 0 {
        status: Code,
    }

    /// `getpgid`, `getppid`, `getsid` and a child's stop signal; pid 0 is
    /// the caller.
    message Lookup = 8, handles 0 {
        what: Below<{ lookup::COUNT }>,
        pid: u32,
    }
    message LookupReply = 8, handles 0 {
        status: Code,
        value: u32,
    }

    /// `sigaction` on `SIGTSTP`, `SIGTTIN`, `SIGTTOU` or `SIGCONT`.
    message SetStopDisposition = 9, handles 0 {
        signal: u32,
        disposition: Below<{ stop::COUNT }>,
    }
    message SetStopDispositionReply = 9, handles 0 {
        status: Code,
    }

    message Kill = 10, handles 0 {
        target: Below<{ target::COUNT }>,
        /// The pid or the group's pid.
        id: u32,
        /// 0 is an existence check.
        signal: u32,
    }
    message KillReply = 10, handles 0 {
        status: Code,
    }

    message Tcsetpgrp = 11, handles 0 {
        pgid: u32,
    }
    message TcsetpgrpReply = 11, handles 0 {
        status: Code,
    }

    message Tcgetpgrp = 12, handles 0 {}
    message TcgetpgrpReply = 12, handles 0 {
        status: Code,
        pgid: u32,
    }

    /// Stop the caller (`raise(SIGTSTP)`, a background `read`'s `SIGTTIN`).
    /// The reply comes after the broker's `job_suspend`, and is read when
    /// the caller runs again.
    message StopSelf = 13, handles 0 {
        signal: u32,
    }
    message StopSelfReply = 13, handles 0 {
        status: Code,
    }

    /// What the broker has counted: the messages by ordinal, and the pids.
    message Statistics = 14, handles 0 {}
    message StatisticsReply = 14, handles 0 {
        prepares: u32,
        commits: u32,
        aborts: u32,
        execs: u32,
        reaps: u32,
        kills: u32,
        others: u32,
        /// Messages the broker refused as unknown or malformed.
        refused: u32,
        live: u32,
        next_pid: u32,
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

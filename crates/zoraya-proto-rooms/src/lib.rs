//! The protocol of the room keeper (ADR-0269 point 2, proposed ADR-0293).
//!
//! Three kinds of channel speak it ([`ChannelKind`]); which messages each may
//! send is the keeper's decision (`zoraya-rooms`), from the kind it
//! made the channel with, never from anything the message says
//! (INV-0269-2). A launcher asks for a launch job in a room and launches in
//! it as it does today; the keeper owns the room's job and its limits.
//!
//! Requests are 1 to 15, replies 64 and up, what the keeper sends unasked
//! 80, and the messages between `init` and the keeper 96 and up.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Data, Text, protocol};

/// The longest room name (the rooms crate's `MAX_NAME`).
pub const MAX_ROOM: usize = 24;
/// The longest refusal text.
pub const MAX_TEXT: usize = 160;
/// Rooms in a usage list: the file's 32 and `home`.
pub const MAX_ROOMS: usize = 33;
/// The services a launcher reports on (`Report`).
pub const SERVICES: u32 = 4;

/// What a keeper channel is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelKind {
    /// The panel or the console's shell: every request.
    Manage(Launcher),
    /// A shell launched in the room, and `top` run in it: `Launch` and
    /// `Usage` of that room, and its `Refused` notices.
    Room(alloc_free::RoomName),
    /// `top` run in `home` or a trusted room: `Usage` of any room, and every
    /// `Refused`.
    Usage,
}

/// Who a `Manage` holder is, for `room: agent-1: running from the desktop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launcher {
    Desktop,
    Console,
}

impl Launcher {
    /// `from the desktop` and `from the console`.
    pub const fn from_words(self) -> &'static str {
        match self {
            Launcher::Desktop => "from the desktop",
            Launcher::Console => "from the console",
        }
    }
    pub const fn raw(self) -> u32 {
        match self {
            Launcher::Desktop => 1,
            Launcher::Console => 2,
        }
    }
    pub const fn from_raw(raw: u32) -> Option<Launcher> {
        match raw {
            1 => Some(Launcher::Desktop),
            2 => Some(Launcher::Console),
            _ => None,
        }
    }
}

/// A fixed-size room name, so that [`ChannelKind`] needs no allocation.
pub mod alloc_free {
    use super::MAX_ROOM;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RoomName {
        bytes: [u8; MAX_ROOM],
        len: u8,
    }

    impl RoomName {
        pub fn new(name: &str) -> Option<RoomName> {
            if name.is_empty() || name.len() > MAX_ROOM {
                return None;
            }
            let mut bytes = [0; MAX_ROOM];
            bytes[..name.len()].copy_from_slice(name.as_bytes());
            Some(RoomName {
                bytes,
                len: name.len() as u8,
            })
        }

        pub fn as_str(&self) -> &str {
            // The bytes came from a `&str`.
            core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
        }
    }
}

/// The request ordinals.
pub mod ordinal {
    pub const LAUNCH: u32 = 1;
    pub const LEAVE: u32 = 2;
    pub const SET: u32 = 3;
    pub const USAGE: u32 = 4;
    pub const REPORT: u32 = 5;
    pub const STOP: u32 = 6;
}

/// Why a request was not done (`Denied`).
pub mod status {
    /// The channel's kind does not allow this message.
    pub const NOT_ALLOWED: u32 = 0;
    /// No such room (not in `/config/rooms`).
    pub const NO_SUCH_ROOM: u32 = 1;
    /// An untrusted room is being used from the other launcher.
    pub const BUSY: u32 = 2;
    /// Four untrusted rooms are running.
    pub const TOO_MANY: u32 = 3;
    /// The kernel refused a job or its limits; `text` says which.
    pub const KERNEL: u32 = 4;
    /// The line is bad, or a limit is above its parent's.
    pub const BAD_LINE: u32 = 5;
    /// The room is not running.
    pub const NOT_RUNNING: u32 = 6;
    /// The keeper could not read the room table.
    pub const NO_TABLE: u32 = 7;
    pub const COUNT: u32 = 8;
}

/// Which limit refused (`Refused`): the values of `QuotaField`, and the CPU.
pub mod field {
    pub const MEMORY: u32 = 1;
    pub const PROCESSES: u32 = 2;
    pub const COUNT: u32 = 3;
}

/// `Attach`'s kinds.
pub mod attach {
    pub const MANAGE_DESKTOP: u32 = 0;
    pub const MANAGE_CONSOLE: u32 = 1;
    pub const USAGE: u32 = 2;
    pub const ROOM: u32 = 3;
    pub const COUNT: u32 = 4;
}

protocol! {
    /// A launcher to the keeper: a launch job in `room`, a sub-job of the
    /// room's job with every right but `LIMIT`. Answered by `Launched` or
    /// `Denied`.
    message Launch<'a> = 1, handles 0 {
        room: Text<'a, MAX_ROOM>,
    }

    /// `Manage` only: this launcher is done with an untrusted room for now
    /// (a shell leaving it; the panel after a spawn). The room is free for
    /// the other launcher once its processes have ended.
    message Leave<'a> = 2, handles 0 {
        room: Text<'a, MAX_ROOM>,
    }

    /// `Manage` only: re-read the room's line and apply its limits to the
    /// running room's job (`room set`).
    message Set<'a> = 3, handles 0 {
        room: Text<'a, MAX_ROOM>,
    }

    /// Each room's usage record; `room` empty is every room that runs.
    message Usage<'a> = 4, handles 0 {
        room: Text<'a, MAX_ROOM>,
    }

    /// `Manage` only: a launcher's forward of one service account's counts.
    message Report<'a> = 5, handles 0 {
        service: Below<4>,
        used: u32,
        limit: u32,
        refused: u32,
        room: Text<'a, MAX_ROOM>,
    }

    /// `Manage` only: `job_kill` on the room's job.
    message Stop<'a> = 6, handles 0 {
        room: Text<'a, MAX_ROOM>,
    }

    /// The answer to `Launch`: the launch job.
    message Launched = 64, handles 1 {}

    /// The answer to any request that was done and has nothing to carry.
    message Done = 65, handles 0 {}

    /// The answer to a request that was not done; `text` is the sentence
    /// the shell prints after `room: ` (`agent-1: running from the desktop`).
    message Denied<'a> = 66, handles 0 {
        status: Below<8>,
        text: Text<'a, MAX_TEXT>,
    }

    /// The answer to `Usage`: `count` records of [`RECORD_SIZE`] bytes.
    message UsageList<'a> = 67, handles 0 {
        count: Below<34>,
        records: Data<'a, { MAX_ROOMS * RECORD_SIZE }>,
    }

    /// The keeper to `Room(R)` and `Usage` channels, unasked, when a room's
    /// job reached a limit (ADR-0263 point 3): the field, and the use and
    /// the limit then.
    message Refused<'a> = 80, handles 0 {
        field: Below<3>,
        used: u64,
        limit: u64,
        room: Text<'a, MAX_ROOM>,
    }

    /// Between `init` and the keeper: the keeper hands `init` a duplicate
    /// (with `LIMIT`) of a room's job (`room` empty: the untrusted job), or
    /// `init` hands a restarted keeper those it kept.
    message Keep<'a> = 96, handles 1 {
        untrusted: Below<2>,
        room: Text<'a, MAX_ROOM>,
    }

    /// `init` to the keeper: serve this end as a channel of the kind
    /// (`attach::*`); `room` names the room of a `ROOM` channel.
    message Attach<'a> = 97, handles 1 {
        kind: Below<4>,
        room: Text<'a, MAX_ROOM>,
    }
}

/// The requests a `Manage` channel may send.
pub const MANAGE_REQUESTS: [u32; 6] = [1, 2, 3, 4, 5, 6];
/// The requests a `Room(R)` channel may send, and only for R.
pub const ROOM_REQUESTS: [u32; 2] = [1, 4];
/// The requests a `Usage` channel may send.
pub const USAGE_REQUESTS: [u32; 1] = [4];

/// Size of one usage record.
pub const RECORD_SIZE: usize = 192;
/// A value that does not apply, as in `zoraya_abi::quota::NONE`.
pub const NONE: u64 = u64::MAX;

/// One room's usage (`UsageList`). Little-endian: the name (24 bytes, zero
/// padded), `flags`, `launcher`, twelve u64 numbers, four service reports of
/// four u32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UsageRecord {
    pub name: [u8; MAX_ROOM],
    pub flags: u32,
    /// `Launcher::raw()` of the last to launch in the room, or 0.
    pub launcher: u32,
    pub mem_used: u64,
    pub mem_limit: u64,
    pub mem_peak: u64,
    pub mem_refused: u64,
    pub procs_used: u64,
    pub procs_limit: u64,
    pub procs_refused: u64,
    /// Thousandths of the machine (`NONE` for no ceiling).
    pub cpu_limit: u64,
    pub cpu_used_ns: u64,
    pub disk_limit: u64,
    pub conns_limit: u64,
    pub tmp_limit: u64,
    /// `(service, used, limit, refused)`; `service` 0 is an empty slot, the
    /// services are numbered from 1.
    pub reports: [[u32; 4]; SERVICES as usize],
}

/// `flags`.
pub mod flags {
    pub const RUNNING: u32 = 1;
    pub const UNTRUSTED: u32 = 2;
    /// A launcher holds a launch job it has not given back with `Leave`.
    pub const LEASED: u32 = 4;
}

impl UsageRecord {
    pub fn name(&self) -> &str {
        let end = self.name.iter().position(|b| *b == 0).unwrap_or(MAX_ROOM);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = [0; MAX_ROOM];
        let n = name.len().min(MAX_ROOM);
        self.name[..n].copy_from_slice(&name.as_bytes()[..n]);
    }

    pub fn encode(&self, out: &mut [u8]) -> Option<()> {
        let out = out.get_mut(..RECORD_SIZE)?;
        out[..MAX_ROOM].copy_from_slice(&self.name);
        out[24..28].copy_from_slice(&self.flags.to_le_bytes());
        out[28..32].copy_from_slice(&self.launcher.to_le_bytes());
        let nums = [
            self.mem_used,
            self.mem_limit,
            self.mem_peak,
            self.mem_refused,
            self.procs_used,
            self.procs_limit,
            self.procs_refused,
            self.cpu_limit,
            self.cpu_used_ns,
            self.disk_limit,
            self.conns_limit,
            self.tmp_limit,
        ];
        for (i, n) in nums.iter().enumerate() {
            out[32 + i * 8..40 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        let mut at = 32 + nums.len() * 8;
        for r in &self.reports {
            for v in r {
                out[at..at + 4].copy_from_slice(&v.to_le_bytes());
                at += 4;
            }
        }
        Some(())
    }

    /// Total: a short slice is `None`, a name that is not UTF-8 reads as
    /// empty.
    pub fn decode(bytes: &[u8]) -> Option<UsageRecord> {
        let b = bytes.get(..RECORD_SIZE)?;
        let u32_at = |at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        let u64_at = |i: usize| {
            let at = 32 + i * 8;
            let mut x = [0u8; 8];
            x.copy_from_slice(&b[at..at + 8]);
            u64::from_le_bytes(x)
        };
        let mut name = [0u8; MAX_ROOM];
        name.copy_from_slice(&b[..MAX_ROOM]);
        let mut reports = [[0u32; 4]; SERVICES as usize];
        let mut at = 32 + 12 * 8;
        for r in reports.iter_mut() {
            for v in r.iter_mut() {
                *v = u32_at(at);
                at += 4;
            }
        }
        Some(UsageRecord {
            name,
            flags: u32_at(24),
            launcher: u32_at(28),
            mem_used: u64_at(0),
            mem_limit: u64_at(1),
            mem_peak: u64_at(2),
            mem_refused: u64_at(3),
            procs_used: u64_at(4),
            procs_limit: u64_at(5),
            procs_refused: u64_at(6),
            cpu_limit: u64_at(7),
            cpu_used_ns: u64_at(8),
            disk_limit: u64_at(9),
            conns_limit: u64_at(10),
            tmp_limit: u64_at(11),
            reports,
        })
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

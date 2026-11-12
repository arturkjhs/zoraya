//! The room keeper's decisions (ADR-0269, proposed ADR-0293), apart from
//! the machine: `rooms` (the process) reads channels and calls the kernel;
//! everything it decides is here, under host test with a model of the kernel.
//!
//! One job per room: the keeper creates a room's job at the first `Launch`
//! (under the untrusted job for an untrusted room, under the session job for
//! a trusted one) with the limits of the room's line, gives each launcher a
//! *launch job*, a sub-job without `LIMIT`, and keeps the room's job while
//! the room has live processes or a launcher still holds it (a lease). It
//! trusts no launcher with `LIMIT`, and it decides what a message may do
//! from the kind of the channel it came on, never from the message
//! (INV-0269-2).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_abi::Status;
use zoraya_abi::quota::{JobLimits, JobUsage};
use zoraya_proto_rooms::{
    ChannelKind, Denied, Done, Launch, Launched, Launcher, Leave, MAX_ROOMS, NONE, Open, Opened,
    RECORD_SIZE, Refused, Report, Set, Stop, Usage, UsageList, UsageRecord, field, flags, ordinal,
    status,
};
use zoraya_protocol::{Below, Data, Header, Text, decode, encode};
use zoraya_settings::rooms::{self, Limits, Machine, Room};

/// What the environment calls a job: the process maps it to a handle.
pub type JobId = u32;
/// What the process calls a channel.
pub type ChanId = usize;

/// At most this many untrusted rooms run at once (ADR-0269 point 2, ADR-0262's
/// carve).
pub const MAX_UNTRUSTED: usize = 4;
/// A room with no processes and no lease is let go after this long without a
/// launch (a launcher between `Launch` and its spawn holds nothing the
/// keeper can see).
pub const IDLE_MS: u64 = 5_000;
/// A resumed room counts as leased by its old holder until the holder sends
/// `Leave` or its channel closes; a holder that never comes back is given up
/// after this long, so that the room does not count against
/// [`MAX_UNTRUSTED`] for ever.
pub const ORPHAN_MS: u64 = 30_000;

/// Where a job goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parent {
    Session,
    /// A job the keeper holds: the untrusted job.
    Job(JobId),
}

/// The kernel and the files, as the keeper uses them.
pub trait Env {
    /// A job under the session or the untrusted job, with these limits.
    fn create_job(&mut self, parent: Parent, limits: &JobLimits) -> Result<JobId, Status>;
    /// A sub-job of `room` without `LIMIT`; the process hands its handle to
    /// the launcher and closes its own.
    fn launch_job(&mut self, room: JobId) -> Result<JobId, Status>;
    fn set_limits(&mut self, job: JobId, limits: &JobLimits) -> Result<(), Status>;
    /// The job's use; `clear` also clears its `OVER_QUOTA` signal.
    fn usage(&mut self, job: JobId, clear: bool) -> Option<JobUsage>;
    fn kill(&mut self, job: JobId);
    /// The keeper lets go of its handle.
    fn close(&mut self, job: JobId);
    fn now_ms(&mut self) -> u64;
    /// The room's line in `/config/rooms`: `Ok(None)` if there is none, `Err`
    /// if the table cannot be read.
    fn room_line(&mut self, name: &str) -> Result<Option<Room>, TableError>;
    /// The session's size, from its job.
    fn machine(&mut self) -> Machine;
}

/// The room table could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableError;

/// What the process does with a `handle` call.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// Write `out[..n]`.
    Send(usize),
    /// Write `out[..n]` with this job (a launch job) and then close it.
    SendJob(usize, JobId),
    /// Write `out[..n]` with the client end of a new channel of this kind,
    /// which the process makes and serves.
    Open(usize, ChannelKind),
    /// The message was not one this kind of channel may send, or was
    /// malformed: close the channel.
    Close,
}

/// What `init` is told, so that a restarted keeper finds the jobs again
/// (ADR-0269 point 5).
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// A job was made: `init` gets a duplicate with `LIMIT`. `room` is empty
    /// for the untrusted job.
    Created {
        room: String,
        job: JobId,
        untrusted: bool,
        /// Who launched last, for a restarted keeper. The event is sent again
        /// (and `init` replaces its duplicate) when this changes.
        launcher: Option<Launcher>,
    },
    /// The keeper let a room's job go.
    Gone { room: String },
}

/// A room reached a limit (`Refused`).
#[derive(Debug, PartialEq, Eq)]
pub struct Notice {
    pub room: String,
    pub field: u32,
    pub used: u64,
    pub limit: u64,
}

struct Running {
    name: String,
    job: JobId,
    untrusted: bool,
    limits: Limits,
    /// The kind of the last `Manage` holder to launch (kept after it left, for
    /// as long as the room has processes).
    holder: Option<Launcher>,
    /// One lease per `Manage` channel that holds a launch job it has not
    /// given back.
    leases: Vec<(ChanId, Launcher)>,
    /// A resumed room's old holder, when the keeper started: leased until it
    /// leaves, its channel closes, or [`ORPHAN_MS`] pass.
    orphan: Option<(Launcher, u64)>,
    last_launch_ms: u64,
    reports: [[u32; 4]; 4],
    seen_mem: u64,
    seen_procs: u64,
}

impl Running {
    fn leased(&self) -> bool {
        !self.leases.is_empty() || self.orphan.is_some()
    }
}

pub struct Keeper<E: Env> {
    env: E,
    untrusted: Option<JobId>,
    rooms: Vec<Running>,
    events: Vec<Event>,
}

fn denied(code: u32, text: &str, tx: u32, out: &mut [u8]) -> Reply {
    match encode(
        &Denied {
            status: Below(code),
            text: Text(text),
        },
        tx,
        out,
    ) {
        Ok(n) => Reply::Send(n),
        Err(_) => Reply::Close,
    }
}

fn done(tx: u32, out: &mut [u8]) -> Reply {
    match encode(&Done {}, tx, out) {
        Ok(n) => Reply::Send(n),
        Err(_) => Reply::Close,
    }
}

/// The job limits a line carries (memory, processes, CPU in thousandths).
pub fn job_limits(l: &Limits) -> JobLimits {
    JobLimits {
        memory: l.mem,
        processes: l.procs,
        cpu: l.cpu.map(|p| p * 10),
    }
}

/// A usage record's six limit words, for `grants` and `room`: the same
/// rendering as the line's (INV-0266-3).
pub fn record_limits(r: &UsageRecord) -> Limits {
    let some = |v: u64| (v != NONE).then_some(v);
    Limits {
        mem: some(r.mem_limit),
        cpu: some(r.cpu_limit).map(|t| (t / 10) as u32),
        procs: some(r.procs_limit).map(|v| v as u32),
        disk: some(r.disk_limit),
        conns: some(r.conns_limit).map(|v| v as u32),
        tmp: some(r.tmp_limit),
    }
}

impl<E: Env> Keeper<E> {
    /// A keeper with the untrusted job `init` kept for it, if any. Without
    /// one it makes its own, with ADR-0266 point 4's ceilings.
    pub fn new(mut env: E, untrusted: Option<JobId>) -> Keeper<E> {
        let mut events = Vec::new();
        let untrusted = untrusted.or_else(|| {
            let m = env.machine();
            let u = m.untrusted();
            let limits = JobLimits {
                memory: Some(u.mem),
                processes: Some(u.procs),
                cpu: Some(u.cpu * 10),
            };
            let job = env.create_job(Parent::Session, &limits).ok()?;
            events.push(Event::Created {
                room: String::new(),
                job,
                untrusted: true,
                launcher: None,
            });
            Some(job)
        });
        Keeper {
            env,
            untrusted,
            rooms: Vec::new(),
            events,
        }
    }

    pub fn env(&mut self) -> &mut E {
        &mut self.env
    }

    /// A room `init` kept a job for (a restart): running again. `untrusted`
    /// and `holder` are what `init` kept; a holder is treated as leasing the
    /// room until it leaves or its channel closes.
    pub fn resume(&mut self, room: &str, job: JobId, untrusted: bool, holder: Option<Launcher>) {
        let now = self.env.now_ms();
        let limits = match self.env.room_line(room) {
            Ok(Some(r)) => self.effective(&r),
            _ => Limits::default(),
        };
        // The refusals that happened before are not new.
        let (seen_mem, seen_procs) = self
            .env
            .usage(job, false)
            .map_or((0, 0), |u| (u.memory.refused, u.processes.refused));
        self.rooms.retain(|r| r.name != room);
        self.rooms.push(Running {
            name: room.to_string(),
            job,
            untrusted,
            limits,
            holder,
            leases: Vec::new(),
            orphan: holder.map(|h| (h, now)),
            last_launch_ms: now,
            reports: [[0; 4]; 4],
            seen_mem,
            seen_procs,
        });
    }

    /// What `init` should be told since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        core::mem::take(&mut self.events)
    }

    /// Names of the rooms that run.
    pub fn running(&self) -> Vec<String> {
        self.rooms.iter().map(|r| r.name.clone()).collect()
    }

    pub fn untrusted_running(&self) -> usize {
        self.rooms.iter().filter(|r| r.untrusted).count()
    }

    pub fn job_of(&self, room: &str) -> Option<JobId> {
        self.rooms.iter().find(|r| r.name == room).map(|r| r.job)
    }

    /// A room's line; `home` has none and is a trusted room without limits
    /// (its launch jobs put the panel's terminals under the session job).
    fn line(&mut self, room: &str) -> Result<Option<Room>, TableError> {
        if room == rooms::HOME {
            return Ok(Some(Room {
                name: room.to_string(),
                untrusted: false,
                colour: 0,
                limits: Limits::default(),
                grants: Vec::new(),
            }));
        }
        self.env.room_line(room)
    }

    /// The line's limits with an untrusted room's missing words filled in
    /// (the line is rewritten by the next `room set`).
    fn effective(&mut self, room: &Room) -> Limits {
        let mut l = room.limits;
        if room.untrusted {
            let m = self.env.machine();
            l.fill_defaults(&m);
        }
        l
    }

    /// A channel went away: it holds no lease any more, and nobody else's is
    /// touched. The closing of a `Manage` channel also ends the lease a
    /// resumed room has for that kind of holder.
    pub fn channel_closed(&mut self, chan: ChanId, kind: &ChannelKind) {
        for r in &mut self.rooms {
            r.leases.retain(|(c, _)| *c != chan);
            if let ChannelKind::Manage(l) = kind
                && r.orphan.is_some_and(|(o, _)| o == *l)
            {
                r.orphan = None;
            }
        }
    }

    /// Lets go of rooms with no processes, no lease and no launch for
    /// [`IDLE_MS`].
    pub fn collect_idle(&mut self) {
        let now = self.env.now_ms();
        let mut i = 0;
        while i < self.rooms.len() {
            let r = &mut self.rooms[i];
            if r.orphan
                .is_some_and(|(_, since)| now.saturating_sub(since) >= ORPHAN_MS)
            {
                r.orphan = None;
            }
            let r = &self.rooms[i];
            let idle = !r.leased()
                && now.saturating_sub(r.last_launch_ms) >= IDLE_MS
                && self
                    .env
                    .usage(r.job, false)
                    .is_some_and(|u| u.processes.used == 0);
            if idle {
                let gone = self.rooms.remove(i);
                self.env.close(gone.job);
                self.events.push(Event::Gone { room: gone.name });
            } else {
                i += 1;
            }
        }
    }

    /// Rooms whose jobs reached a limit since the last call (`Refused`).
    pub fn refusals(&mut self) -> Vec<Notice> {
        let mut out = Vec::new();
        // The untrusted job's own signal is cleared too, or its level would
        // wake the keeper without end; the rooms under it say what happened.
        if let Some(u) = self.untrusted {
            let _ = self.env.usage(u, true);
        }
        for r in &mut self.rooms {
            let Some(u) = self.env.usage(r.job, true) else {
                continue;
            };
            if u.memory.refused > r.seen_mem {
                out.push(Notice {
                    room: r.name.clone(),
                    field: field::MEMORY,
                    used: u.memory.used,
                    limit: u.memory.limit.unwrap_or(NONE),
                });
            }
            if u.processes.refused > r.seen_procs {
                out.push(Notice {
                    room: r.name.clone(),
                    field: field::PROCESSES,
                    used: u.processes.used,
                    limit: u.processes.limit.unwrap_or(NONE),
                });
            }
            r.seen_mem = u.memory.refused;
            r.seen_procs = u.processes.refused;
        }
        out
    }

    /// The bytes of a `Refused` for `notice`.
    pub fn notice_bytes(notice: &Notice, out: &mut [u8]) -> Option<usize> {
        encode(
            &Refused {
                field: Below(notice.field),
                used: notice.used,
                limit: notice.limit,
                room: Text(&notice.room),
            },
            0,
            out,
        )
        .ok()
    }

    /// May a notice of `room` go to a channel of this kind (ADR-0269
    /// point 2's table)?
    pub fn wants_notice(kind: &ChannelKind, room: &str) -> bool {
        match kind {
            ChannelKind::Manage(_) | ChannelKind::Usage => true,
            ChannelKind::Room(r) => r.as_str() == room,
        }
    }

    /// Decides on one message from channel `chan` of `kind`.
    pub fn handle(
        &mut self,
        chan: ChanId,
        kind: &ChannelKind,
        message: &[u8],
        handles: usize,
        out: &mut [u8],
    ) -> Reply {
        let Ok((header, _)) = Header::decode(message) else {
            return Reply::Close;
        };
        let tx = header.transaction;
        // Which messages the kind may send, before any is read.
        let allowed = match kind {
            ChannelKind::Manage(_) => zoraya_proto_rooms::MANAGE_REQUESTS.as_slice(),
            ChannelKind::Room(_) => zoraya_proto_rooms::ROOM_REQUESTS.as_slice(),
            ChannelKind::Usage => zoraya_proto_rooms::USAGE_REQUESTS.as_slice(),
        };
        if !(1..16).contains(&header.ordinal) {
            return Reply::Close;
        }
        if !allowed.contains(&header.ordinal) {
            return denied(status::NOT_ALLOWED, "not allowed on this channel", tx, out);
        }
        match header.ordinal {
            ordinal::LAUNCH => {
                let Ok((_, m)) = decode::<Launch<'_>>(message, handles) else {
                    return Reply::Close;
                };
                if !self.room_allowed(kind, m.room.0) {
                    return denied(status::NOT_ALLOWED, "not this room", tx, out);
                }
                self.launch(chan, kind, m.room.0, tx, out)
            }
            ordinal::LEAVE => {
                let Ok((_, m)) = decode::<Leave<'_>>(message, handles) else {
                    return Reply::Close;
                };
                let now = self.env.now_ms();
                for r in &mut self.rooms {
                    if r.name == m.room.0 {
                        r.leases.retain(|(c, _)| *c != chan);
                        if let ChannelKind::Manage(l) = kind
                            && r.orphan.is_some_and(|(o, _)| o == *l)
                        {
                            r.orphan = None;
                        }
                        r.last_launch_ms = now;
                    }
                }
                done(tx, out)
            }
            ordinal::SET => {
                let Ok((_, m)) = decode::<Set<'_>>(message, handles) else {
                    return Reply::Close;
                };
                self.set(m.room.0, tx, out)
            }
            ordinal::USAGE => {
                let Ok((_, m)) = decode::<Usage<'_>>(message, handles) else {
                    return Reply::Close;
                };
                if !self.room_allowed(kind, m.room.0) {
                    return denied(status::NOT_ALLOWED, "not this room", tx, out);
                }
                self.usage(m.room.0, tx, out)
            }
            ordinal::REPORT => {
                let Ok((_, m)) = decode::<Report<'_>>(message, handles) else {
                    return Reply::Close;
                };
                let Some(r) = self.rooms.iter_mut().find(|r| r.name == m.room.0) else {
                    return denied(status::NOT_RUNNING, "not running", tx, out);
                };
                // Service numbers are 1 to 4; slot i holds service i + 1.
                r.reports[m.service.0 as usize] = [m.service.0 + 1, m.used, m.limit, m.refused];
                done(tx, out)
            }
            ordinal::OPEN => {
                let Ok((_, m)) = decode::<Open<'_>>(message, handles) else {
                    return Reply::Close;
                };
                let kind = match m.kind.0 {
                    0 => zoraya_proto_rooms::alloc_free::RoomName::new(m.room.0)
                        .map(ChannelKind::Room),
                    _ => Some(ChannelKind::Usage),
                };
                let Some(kind) = kind else {
                    return denied(status::NO_SUCH_ROOM, "not a room name", tx, out);
                };
                match encode(&Opened {}, tx, out) {
                    Ok(n) => Reply::Open(n, kind),
                    Err(_) => Reply::Close,
                }
            }
            ordinal::STOP => {
                let Ok((_, m)) = decode::<Stop<'_>>(message, handles) else {
                    return Reply::Close;
                };
                if m.room.0 == rooms::HOME {
                    // `home` is the session's own terminals, not a room to kill.
                    return denied(status::NOT_ALLOWED, "home cannot be stopped", tx, out);
                }
                let Some(i) = self.rooms.iter().position(|r| r.name == m.room.0) else {
                    return denied(status::NOT_RUNNING, "not running", tx, out);
                };
                let gone = self.rooms.remove(i);
                self.env.kill(gone.job);
                self.env.close(gone.job);
                self.events.push(Event::Gone { room: gone.name });
                done(tx, out)
            }
            _ => Reply::Close,
        }
    }

    /// A `Room(R)` channel may name only R; a `Usage` channel any room, but
    /// only to read; an empty name is "every room" for `Usage`.
    fn room_allowed(&self, kind: &ChannelKind, room: &str) -> bool {
        match kind {
            ChannelKind::Room(r) => r.as_str() == room,
            _ => true,
        }
    }

    fn launch(
        &mut self,
        chan: ChanId,
        kind: &ChannelKind,
        room: &str,
        tx: u32,
        out: &mut [u8],
    ) -> Reply {
        // `Usage` cannot reach here: it may not send `Launch`.
        let manage = match kind {
            ChannelKind::Manage(l) => Some(*l),
            _ => None,
        };
        self.collect_idle();
        let now = self.env.now_ms();
        if let Some(i) = self.rooms.iter().position(|r| r.name == room) {
            // An untrusted room is launched into by one `Manage` holder at a
            // time (ADR-0269 point 2); a `Room(R)` channel is inside it.
            if self.rooms[i].untrusted
                && let Some(me) = manage
            {
                let procs = self
                    .env
                    .usage(self.rooms[i].job, false)
                    .map_or(0, |u| u.processes.used);
                let r = &self.rooms[i];
                // Another kind of launcher holds a lease, or the room has
                // processes and its holder is another kind, or unknown (a
                // resumed room whose holder `init` did not keep).
                let other = r
                    .leases
                    .iter()
                    .map(|(_, l)| *l)
                    .chain(r.orphan.map(|(l, _)| l))
                    .find(|l| *l != me);
                let busy = match (other, procs > 0, r.holder) {
                    (Some(l), _, _) => Some(Some(l)),
                    (None, true, Some(h)) if h != me => Some(Some(h)),
                    (None, true, None) => Some(None),
                    _ => None,
                };
                if let Some(who) = busy {
                    let text = match who {
                        Some(l) => format!("running {}", l.phrase()),
                        None => "running, holder unknown".to_string(),
                    };
                    return denied(status::BUSY, &text, tx, out);
                }
            }
        } else {
            let line = match self.line(room) {
                Ok(Some(l)) => l,
                Ok(None) => return denied(status::NO_SUCH_ROOM, "no such room", tx, out),
                Err(TableError) => {
                    return denied(status::NO_TABLE, "the room table is unreadable", tx, out);
                }
            };
            if line.untrusted && self.untrusted_running() >= MAX_UNTRUSTED {
                return denied(
                    status::TOO_MANY,
                    "four untrusted rooms are running",
                    tx,
                    out,
                );
            }
            let limits = self.effective(&line);
            let m = self.env.machine();
            if let Err(why) = limits.check_within(&m.ceilings(line.untrusted)) {
                return denied(status::BAD_LINE, &why, tx, out);
            }
            let parent = if line.untrusted {
                match self.untrusted {
                    Some(u) => Parent::Job(u),
                    None => return denied(status::KERNEL, "no untrusted job", tx, out),
                }
            } else {
                Parent::Session
            };
            let job = match self.env.create_job(parent, &job_limits(&limits)) {
                Ok(j) => j,
                Err(s) => return denied(status::KERNEL, &format!("{s:?}"), tx, out),
            };
            self.events.push(Event::Created {
                room: room.to_string(),
                job,
                untrusted: line.untrusted,
                launcher: manage,
            });
            self.rooms.push(Running {
                name: room.to_string(),
                job,
                untrusted: line.untrusted,
                limits,
                holder: manage,
                leases: Vec::new(),
                orphan: None,
                last_launch_ms: now,
                reports: [[0; 4]; 4],
                seen_mem: 0,
                seen_procs: 0,
            });
        }
        let Some(i) = self.rooms.iter().position(|r| r.name == room) else {
            return Reply::Close;
        };
        let launch = match self.env.launch_job(self.rooms[i].job) {
            Ok(j) => j,
            Err(s) => return denied(status::KERNEL, &format!("{s:?}"), tx, out),
        };
        let r = &mut self.rooms[i];
        r.last_launch_ms = now;
        if let Some(who) = manage {
            r.leases.retain(|(c, _)| *c != chan);
            r.leases.push((chan, who));
            if r.orphan.is_some_and(|(o, _)| o == who) {
                r.orphan = None;
            }
            if r.holder != Some(who) {
                r.holder = Some(who);
                self.events.push(Event::Created {
                    room: r.name.clone(),
                    job: r.job,
                    untrusted: r.untrusted,
                    launcher: Some(who),
                });
            }
        }
        match encode(&Launched {}, tx, out) {
            Ok(n) => Reply::SendJob(n, launch),
            Err(_) => {
                self.env.close(launch);
                Reply::Close
            }
        }
    }

    fn set(&mut self, room: &str, tx: u32, out: &mut [u8]) -> Reply {
        if room == rooms::HOME {
            return denied(status::NOT_ALLOWED, "home has no limits to set", tx, out);
        }
        let running = self.rooms.iter().position(|r| r.name == room);
        let line = match self.line(room) {
            Ok(Some(l)) => l,
            Ok(None) => return denied(status::NO_SUCH_ROOM, "no such room", tx, out),
            Err(TableError) => {
                return denied(status::NO_TABLE, "the room table is unreadable", tx, out);
            }
        };
        // ADR-0293 Option A: the ceilings are checked also for a room that
        // does not run, so that `room set` says at once what a launch would.
        let limits = self.effective(&line);
        let m = self.env.machine();
        if let Err(why) = limits.check_within(&m.ceilings(line.untrusted)) {
            return denied(status::BAD_LINE, &why, tx, out);
        }
        // Nothing runs: the next `Launch` reads the line.
        let Some(i) = running else {
            return done(tx, out);
        };
        if line.untrusted != self.rooms[i].untrusted {
            return denied(
                status::BAD_LINE,
                "the room's trust cannot change while it runs",
                tx,
                out,
            );
        }
        if let Err(s) = self.env.set_limits(self.rooms[i].job, &job_limits(&limits)) {
            return denied(status::KERNEL, &format!("{s:?}"), tx, out);
        }
        self.rooms[i].limits = limits;
        done(tx, out)
    }

    fn record(&mut self, i: usize) -> UsageRecord {
        let u = self.env.usage(self.rooms[i].job, false).unwrap_or_default();
        let r = &self.rooms[i];
        let mut rec = UsageRecord::default();
        rec.set_name(&r.name);
        rec.flags = flags::RUNNING
            | if r.untrusted { flags::UNTRUSTED } else { 0 }
            | if r.leased() { flags::LEASED } else { 0 };
        rec.launcher = r.holder.map_or(0, Launcher::raw);
        rec.mem_used = u.memory.used;
        rec.mem_limit = u.memory.limit.unwrap_or(NONE);
        rec.mem_peak = u.memory.peak;
        rec.mem_refused = u.memory.refused;
        rec.procs_used = u.processes.used;
        rec.procs_limit = u.processes.limit.unwrap_or(NONE);
        rec.procs_refused = u.processes.refused;
        rec.cpu_limit = u.cpu.limit.map_or(NONE, u64::from);
        rec.cpu_used_ns = u.cpu.used_ns;
        rec.disk_limit = r.limits.disk.unwrap_or(NONE);
        rec.conns_limit = r.limits.conns.map_or(NONE, u64::from);
        rec.tmp_limit = r.limits.tmp.unwrap_or(NONE);
        rec.reports = r.reports;
        rec
    }

    fn usage(&mut self, room: &str, tx: u32, out: &mut [u8]) -> Reply {
        let mut bytes = Vec::new();
        let mut count = 0u32;
        for i in 0..self.rooms.len() {
            if (room.is_empty() || self.rooms[i].name == room) && (count as usize) < MAX_ROOMS {
                let rec = self.record(i);
                let mut b = [0u8; RECORD_SIZE];
                if rec.encode(&mut b).is_some() {
                    bytes.extend_from_slice(&b);
                    count += 1;
                }
            }
        }
        match encode(
            &UsageList {
                count: Below(count),
                records: Data(&bytes),
            },
            tx,
            out,
        ) {
            Ok(n) => Reply::Send(n),
            Err(_) => Reply::Close,
        }
    }
}

/// The size of the buffer `handle` needs for its replies.
pub const OUT_SIZE: usize = 16 + 8 + MAX_ROOMS * RECORD_SIZE + 64;

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

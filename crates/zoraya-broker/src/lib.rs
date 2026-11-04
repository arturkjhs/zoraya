//! The POSIX broker's logic (ADR-0104), apart from the machine: the pid
//! table, process groups as jobs, the session, the rules of `setpgid`,
//! `kill` and `tcsetpgrp`, and the protocol of `zoraya-proto-posix`. The
//! `broker` program feeds it the messages of its connections and the
//! termination of process jobs, carries out the [`Action`]s it returns, and
//! lends it the kernel through [`Kernel`].
//!
//! What the broker never knows (ADR-0104 item 13): file names, fds, cwd,
//! environment, `umask`, signal handlers.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod session;
pub mod totals;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::posix::job_rights;
use zoraya_abi::posix::signal::{self, SIGCONT, SIGKILL, SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU};
use zoraya_abi::{ExitRecord, RequestKind, Rights, Status, job_signals};
use zoraya_proto_posix::{
    ExecCommit, ExecCommitReply, Kill, KillReply, Lookup, LookupReply, ORDINALS, Reap, Reaped,
    ReportStats, ReportStatsReply, SetPgid, SetPgidReply, SetStopDisposition,
    SetStopDispositionReply, SpawnAbort, SpawnAbortReply, SpawnCommit, SpawnCommitReply,
    SpawnPrepare, SpawnPrepareIn, SpawnPrepared, SpawnPreparedIn, Statistics, StatisticsReply,
    StopSelf, StopSelfReply, Tcgetpgrp, TcgetpgrpReply, Tcsetpgrp, TcsetpgrpReply, group, lookup,
    status, stop, target,
};
use zoraya_protocol::{Below, ErrorReply, Header, MAX_WIRE_MESSAGE, Message, decode, encode};

/// The broker itself as a parent: `getppid` of the shell, and the parent of
/// every orphan (ADR-0104 item 2).
pub const BROKER_PID: u32 = 1;
/// The first pid given out.
pub const FIRST_PID: u32 = 2;
/// Pids are a 31-bit counter, never reused within a session.
pub const LAST_PID: u32 = 0x7fff_ffff;

/// The rights of a process job's handle in the table (ADR-0104 item 2).
pub const JOB_RIGHTS: Rights = job_rights::TABLE;
/// The parent's handle to a child's process job (item 4).
pub const PARENT_JOB_RIGHTS: Rights = job_rights::PARENT;
/// The child's handle to its own process job (item 4).
pub const CHILD_JOB_RIGHTS: Rights = job_rights::OWN;
/// The terminal's handle to the foreground group's job (item 10).
pub const FOREGROUND_RIGHTS: Rights = job_rights::TERMINAL;

/// What the broker needs of the kernel.
pub trait Kernel {
    type H: Copy + Eq + core::fmt::Debug;

    fn job_create(&mut self, parent: Self::H) -> Result<Self::H, Status>;
    fn duplicate(&mut self, h: Self::H, rights: Rights) -> Result<Self::H, Status>;
    fn close(&mut self, h: Self::H);
    /// A channel: the end the broker serves and the end it hands out.
    fn channel_create(&mut self) -> Result<(Self::H, Self::H), Status>;
    fn job_signal(&mut self, job: Self::H, kind: RequestKind, detail: u32) -> Result<(), Status>;
    fn job_kill(&mut self, job: Self::H) -> Result<(), Status>;
    fn job_suspend(&mut self, job: Self::H) -> Result<(), Status>;
    fn job_resume(&mut self, job: Self::H) -> Result<(), Status>;
    /// The signals asserted now (`object_get_signals`).
    fn signals(&mut self, h: Self::H) -> Result<u32, Status>;
    fn exit_record(&mut self, process: Self::H) -> Result<ExitRecord, Status>;
    /// The type of the object a handle names (ADR-0072), if it is valid.
    fn type_name(&mut self, h: Self::H) -> Option<&'static str>;
}

/// A connection's index.
pub type Conn = usize;

/// What the `broker` program must do.
#[derive(Debug, PartialEq, Eq)]
pub enum Action<H> {
    /// Send the bytes and move the handles on the connection.
    Reply {
        to: Conn,
        bytes: Vec<u8>,
        handles: Vec<H>,
    },
    /// Serve this server end as connection `conn`.
    Watch {
        conn: Conn,
        end: H,
    },
    /// Stop serving `conn` and close its end.
    Forget {
        conn: Conn,
    },
    /// Watch the pid's process job for `TERMINATED` and call
    /// [`Broker::terminated`] when it is; the handle stays the broker's.
    WatchJob {
        pid: u32,
        job: H,
    },
    UnwatchJob {
        pid: u32,
    },
    /// Tell the terminal the foreground group, moving this job handle.
    Foreground {
        group: u32,
        job: H,
    },
}

/// Who is at the other end of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Peer {
    Process(u32),
    /// A native shell's connection, with no pid (ADR-0153's note).
    Client,
    /// The image before an `exec`, until it ends.
    Retired,
}

/// The stop and continue signals' slots in a record.
fn stop_slot(sig: u32) -> Option<usize> {
    match sig {
        SIGTSTP => Some(0),
        SIGTTIN => Some(1),
        SIGTTOU => Some(2),
        SIGCONT => Some(3),
        _ => None,
    }
}

#[derive(Debug)]
struct Record<H> {
    job: H,
    process: Option<H>,
    /// `BROKER_PID` for an orphan, 0 for a native client's child.
    parent: u32,
    /// The parent is the native client.
    client_child: bool,
    pgid: u32,
    /// `stop::*` for `SIGTSTP`, `SIGTTIN`, `SIGTTOU`, `SIGCONT`.
    stops: [u32; 4],
    /// The signal that stopped it, or 0.
    stopped: u32,
    execed: bool,
    terminated: bool,
}

#[derive(Debug)]
struct Group<H> {
    job: H,
    members: usize,
    /// In the session job; a `Group(job)` group is not.
    session: bool,
    /// Stopped as a whole by `job_suspend` of the group job: it is resumed
    /// as a whole, since a member's resume leaves the group job suspended
    /// and every process created in it after would start stopped (review
    /// R1).
    suspended: bool,
}

/// What the broker has counted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub by_ordinal: [u32; ORDINALS],
    pub refused: u32,
    pub reaped_by_broker: u32,
}

pub struct Broker<K: Kernel> {
    pub kernel: K,
    session_job: K::H,
    terminal: bool,
    pids: BTreeMap<u32, Record<K::H>>,
    groups: BTreeMap<u32, Group<K::H>>,
    connections: Vec<Option<Peer>>,
    next_pid: u32,
    sid: u32,
    foreground: Option<u32>,
    pub counts: Counts,
    /// What the processes reported, and the peak of live pids (ADR-0110
    /// item 7).
    pub totals: totals::Totals,
    /// The shell's exit record, once the broker reaped it.
    pub shell_exit: Option<ExitRecord>,
    out: Vec<u8>,
}

/// The session's first process, prepared.
#[derive(Debug)]
pub struct First<H> {
    pub pid: u32,
    /// `CREATE_PROCESS | TRANSFER`, for the spawn and the child's slot.
    pub job: H,
    /// The child's broker end.
    pub broker_end: H,
}

type Result2<T> = core::result::Result<T, u32>;
/// What a call gives, with the actions it asks for.
pub type WithActions<T, H> = (T, Vec<Action<H>>);

impl<K: Kernel> Broker<K> {
    /// A broker for the session in `session_job`, which holds only POSIX
    /// processes; `terminal` says whether the session has one.
    pub fn new(kernel: K, session_job: K::H, terminal: bool) -> Broker<K> {
        Broker {
            kernel,
            session_job,
            terminal,
            pids: BTreeMap::new(),
            groups: BTreeMap::new(),
            connections: Vec::new(),
            next_pid: FIRST_PID,
            sid: 0,
            foreground: None,
            counts: Counts::default(),
            totals: totals::Totals::default(),
            shell_exit: None,
            out: vec![0; MAX_WIRE_MESSAGE],
        }
    }

    fn add_connection(&mut self, peer: Peer) -> Conn {
        if let Some(i) = self.connections.iter().position(Option::is_none) {
            self.connections[i] = Some(peer);
            return i;
        }
        self.connections.push(Some(peer));
        self.connections.len() - 1
    }

    /// The number of pids prepared or running and not yet terminated.
    pub fn live(&self) -> usize {
        self.pids.values().filter(|r| !r.terminated).count()
    }

    /// The pids in the table, reaped ones excluded, for the tests.
    pub fn pids(&self) -> Vec<u32> {
        self.pids.keys().copied().collect()
    }

    pub fn foreground(&self) -> Option<u32> {
        self.foreground
    }

    pub fn group_of(&self, pid: u32) -> Option<u32> {
        self.pids.get(&pid).map(|r| r.pgid)
    }

    pub fn is_stopped(&self, pid: u32) -> Option<u32> {
        self.pids.get(&pid).map(|r| r.stopped)
    }

    /// True when the session is over: its first process is reaped and no
    /// other pid is live (ADR-0104 item 12). Never in the mode with no
    /// first shell.
    pub fn finished(&self) -> bool {
        self.sid != 0 && !self.pids.contains_key(&self.sid) && self.live() == 0
    }

    /// Prepares the session's first process: pid 2, leader of a new group,
    /// a child of the broker, and the session's id.
    pub fn first(&mut self) -> Result<WithActions<First<K::H>, K::H>, u32> {
        let mut actions = Vec::new();
        let (pid, pgid, job, end) = self.prepare(
            Peer::Process(BROKER_PID),
            Form::New,
            [stop::DEFAULT; 4],
            &mut actions,
        )?;
        self.sid = pid;
        debug_assert_eq!(pid, pgid);
        // The parent's handle is the broker's own record.
        self.kernel.close(job.0);
        Ok((
            First {
                pid,
                job: job.1,
                broker_end: end,
            },
            actions,
        ))
    }

    /// Commits the first process and makes its group the foreground.
    pub fn commit_first(&mut self, process: K::H) -> Vec<Action<K::H>> {
        let mut actions = Vec::new();
        let pid = self.sid;
        if let Some(r) = self.pids.get_mut(&pid) {
            r.process = Some(process);
            actions.push(Action::WatchJob { pid, job: r.job });
        }
        if self.terminal {
            let _ = self.set_foreground(pid, &mut actions);
        }
        actions
    }

    /// The first process did not start: its pid goes.
    pub fn abort_first(&mut self) {
        let pid = self.sid;
        self.remove(pid, &mut Vec::new());
    }

    /// A client connection with no pid, for a native shell (the mode with no
    /// first shell): the end to hand over.
    pub fn add_client(&mut self) -> Result<WithActions<K::H, K::H>, Status> {
        let (server, client) = self.kernel.channel_create()?;
        let conn = self.add_connection(Peer::Client);
        Ok((client, vec![Action::Watch { conn, end: server }]))
    }

    fn encode<'m, M: Message<'m>>(&mut self, m: &M, t: u32) -> Vec<u8> {
        match encode(m, t, &mut self.out) {
            Ok(n) => self.out[..n].to_vec(),
            Err(_) => self.error(status::INVALID, t),
        }
    }

    fn error(&mut self, code: u32, t: u32) -> Vec<u8> {
        match encode(&ErrorReply { status: code }, t, &mut self.out) {
            Ok(n) => self.out[..n].to_vec(),
            Err(_) => Vec::new(),
        }
    }

    fn close_all(&mut self, handles: Vec<K::H>) {
        for h in handles {
            self.kernel.close(h);
        }
    }

    /// A message on connection `conn` with the handles it carried. Never
    /// panics on anything a peer sends; a handle the broker does not keep is
    /// closed.
    pub fn request(&mut self, conn: Conn, bytes: &[u8], handles: Vec<K::H>) -> Vec<Action<K::H>> {
        let mut actions = Vec::new();
        let Some(peer) = self.connections.get(conn).copied().flatten() else {
            self.close_all(handles);
            return actions;
        };
        let Ok((header, _)) = Header::decode(bytes) else {
            self.counts.refused += 1;
            self.close_all(handles);
            return actions;
        };
        let t = header.transaction;
        let ordinal = header.ordinal;
        if let Some(c) = self.counts.by_ordinal.get_mut(ordinal as usize) {
            *c += 1;
        }
        let n = handles.len();
        macro_rules! parse {
            ($m:ty) => {
                match decode::<$m>(bytes, n) {
                    Ok((_, m)) => m,
                    Err(_) => {
                        self.close_all(handles);
                        self.counts.refused += 1;
                        let bytes = self.error(status::INVALID, t);
                        actions.push(Action::Reply {
                            to: conn,
                            bytes,
                            handles: Vec::new(),
                        });
                        return actions;
                    }
                }
            };
        }
        macro_rules! answer {
            ($m:expr) => {{
                let bytes = self.encode(&$m, t);
                actions.push(Action::Reply {
                    to: conn,
                    bytes,
                    handles: Vec::new(),
                });
            }};
        }
        let caller = match peer {
            Peer::Process(pid) => Some(pid),
            Peer::Client => None,
            Peer::Retired => {
                // An image that has exec'd says nothing more that counts.
                self.close_all(handles);
                self.counts.refused += 1;
                let bytes = self.error(status::NOT_PERMITTED, t);
                actions.push(Action::Reply {
                    to: conn,
                    bytes,
                    handles: Vec::new(),
                });
                return actions;
            }
        };
        let code = |r: Result2<()>| Below(r.err().unwrap_or(status::OK));
        match ordinal {
            o if o == SpawnPrepare::ORDINAL => {
                let m = parse!(SpawnPrepare);
                let form = match m.group.0 {
                    group::INHERIT => Form::Inherit,
                    group::NEW => Form::New,
                    _ => Form::Join(m.pgid),
                };
                let stops = self.child_stops(caller, m.fork.0 == 1, m.ignored);
                let from = caller.map_or(Peer::Client, Peer::Process);
                match self.prepare(from, form, stops, &mut actions) {
                    Ok((pid, pgid, (parent_job, child_job), end)) => {
                        let sid = self.sid_of(pgid);
                        let bytes = self.encode(&SpawnPrepared { pid, pgid, sid }, t);
                        actions.push(Action::Reply {
                            to: conn,
                            bytes,
                            handles: vec![parent_job, child_job, end],
                        });
                    }
                    Err(e) => {
                        let bytes = self.error(e, t);
                        actions.push(Action::Reply {
                            to: conn,
                            bytes,
                            handles: Vec::new(),
                        });
                    }
                }
            }
            o if o == SpawnPrepareIn::ORDINAL => {
                let m = parse!(SpawnPrepareIn);
                let job = handles[0];
                let r = if caller.is_some() {
                    // Only a native shell names jobs outside the session.
                    self.kernel.close(job);
                    Err(status::NOT_PERMITTED)
                } else if self.kernel.type_name(job) != Some("job") {
                    self.kernel.close(job);
                    Err(status::INVALID)
                } else {
                    let stops = self.child_stops(None, false, m.ignored);
                    self.prepare(Peer::Client, Form::In(job), stops, &mut actions)
                };
                match r {
                    Ok((pid, pgid, (parent_job, child_job), end)) => {
                        let bytes = self.encode(&SpawnPreparedIn { pid, pgid, sid: 0 }, t);
                        actions.push(Action::Reply {
                            to: conn,
                            bytes,
                            handles: vec![parent_job, child_job, end],
                        });
                    }
                    Err(e) => {
                        let bytes = self.error(e, t);
                        actions.push(Action::Reply {
                            to: conn,
                            bytes,
                            handles: Vec::new(),
                        });
                    }
                }
            }
            o if o == SpawnCommit::ORDINAL => {
                let m = parse!(SpawnCommit);
                let r = self.commit(caller, m.pid, handles[0], &mut actions);
                answer!(SpawnCommitReply { status: code(r) });
            }
            o if o == SpawnAbort::ORDINAL => {
                let m = parse!(SpawnAbort);
                let r = self.abort(caller, m.pid, &mut actions);
                answer!(SpawnAbortReply { status: code(r) });
            }
            o if o == ExecCommit::ORDINAL => {
                let _ = parse!(ExecCommit);
                let r = match caller {
                    Some(pid) => self.exec(conn, pid, handles[0], handles[1], &mut actions),
                    None => {
                        self.close_all(handles);
                        Err(status::INVALID)
                    }
                };
                answer!(ExecCommitReply { status: code(r) });
            }
            o if o == Reap::ORDINAL => {
                let m = parse!(Reap);
                match self.reap(caller, m.pid, &mut actions) {
                    Ok(e) => {
                        let kind = e.request_kind.map_or(0, |k| k.raw() + 1);
                        answer!(Reaped {
                            status: Below(status::OK),
                            code: e.code as u32,
                            reason: e.reason as u32,
                            detail: e.detail,
                            kind,
                        });
                    }
                    Err(e) => answer!(Reaped {
                        status: Below(e),
                        code: 0,
                        reason: 0,
                        detail: 0,
                        kind: 0,
                    }),
                }
            }
            o if o == SetPgid::ORDINAL => {
                let m = parse!(SetPgid);
                let r = self.setpgid(caller, m.pid, m.pgid);
                answer!(SetPgidReply { status: code(r) });
            }
            o if o == Lookup::ORDINAL => {
                let m = parse!(Lookup);
                let (status, value) = match self.lookup(caller, m.what.0, m.pid) {
                    Ok(v) => (status::OK, v),
                    Err(e) => (e, 0),
                };
                answer!(LookupReply {
                    status: Below(status),
                    value
                });
            }
            o if o == SetStopDisposition::ORDINAL => {
                let m = parse!(SetStopDisposition);
                let r = match (
                    caller.and_then(|p| self.pids.get_mut(&p)),
                    stop_slot(m.signal),
                ) {
                    (Some(rec), Some(i)) => {
                        rec.stops[i] = m.disposition.0;
                        Ok(())
                    }
                    _ => Err(status::INVALID),
                };
                answer!(SetStopDispositionReply { status: code(r) });
            }
            o if o == Kill::ORDINAL => {
                let m = parse!(Kill);
                let r = self.kill(caller, m.target.0, m.id, m.signal);
                answer!(KillReply { status: code(r) });
            }
            o if o == Tcsetpgrp::ORDINAL => {
                let m = parse!(Tcsetpgrp);
                let r = match caller {
                    Some(pid) if self.in_session(pid) => self.set_foreground(m.pgid, &mut actions),
                    Some(_) => Err(status::NOT_PERMITTED),
                    // `zosh` owns its terminal (ADR-0153's note on ADR-0105).
                    None => Err(status::NO_TERMINAL),
                };
                answer!(TcsetpgrpReply { status: code(r) });
            }
            o if o == Tcgetpgrp::ORDINAL => {
                let _ = parse!(Tcgetpgrp);
                let (s, pgid) = if !self.terminal || caller.is_none() {
                    (status::NO_TERMINAL, 0)
                } else {
                    (status::OK, self.foreground.unwrap_or(0))
                };
                answer!(TcgetpgrpReply {
                    status: Below(s),
                    pgid
                });
            }
            o if o == StopSelf::ORDINAL => {
                let m = parse!(StopSelf);
                let r = match caller {
                    Some(pid) if matches!(m.signal, SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU) => {
                        self.stop_pid(pid, m.signal)
                    }
                    _ => Err(status::INVALID),
                };
                answer!(StopSelfReply { status: code(r) });
            }
            o if o == Statistics::ORDINAL => {
                let _ = parse!(Statistics);
                let s = self.statistics();
                answer!(s);
            }
            o if o == ReportStats::ORDINAL => {
                let m = parse!(ReportStats<'_>);
                let r = self.totals.add(m.counters.0, m.calls.0);
                if r.is_err() {
                    self.counts.refused += 1;
                }
                answer!(ReportStatsReply {
                    status: Below(if r.is_ok() {
                        status::OK
                    } else {
                        status::INVALID
                    }),
                });
            }
            _ => {
                self.close_all(handles);
                self.counts.refused += 1;
                let bytes = self.error(status::UNSUPPORTED, t);
                actions.push(Action::Reply {
                    to: conn,
                    bytes,
                    handles: Vec::new(),
                });
            }
        }
        if ordinal == SpawnPrepare::ORDINAL || ordinal == SpawnPrepareIn::ORDINAL {
            let live = self.live();
            self.totals.live(live);
        }
        actions
    }

    pub fn statistics(&self) -> StatisticsReply {
        let c = &self.counts.by_ordinal;
        let at = |m: u32| c[m as usize];
        let named = [
            SpawnPrepare::ORDINAL,
            SpawnPrepareIn::ORDINAL,
            SpawnCommit::ORDINAL,
            SpawnAbort::ORDINAL,
            ExecCommit::ORDINAL,
            Reap::ORDINAL,
            Kill::ORDINAL,
        ];
        let others = c
            .iter()
            .enumerate()
            .filter(|(i, _)| !named.contains(&(*i as u32)))
            .map(|(_, n)| n)
            .sum();
        StatisticsReply {
            prepares: at(SpawnPrepare::ORDINAL) + at(SpawnPrepareIn::ORDINAL),
            commits: at(SpawnCommit::ORDINAL),
            aborts: at(SpawnAbort::ORDINAL),
            execs: at(ExecCommit::ORDINAL),
            reaps: at(Reap::ORDINAL),
            kills: at(Kill::ORDINAL),
            others,
            refused: self.counts.refused,
            live: self.live() as u32,
            next_pid: self.next_pid,
        }
    }

    /// The peer of `conn` closed it.
    pub fn closed(&mut self, conn: Conn) -> Vec<Action<K::H>> {
        let mut actions = Vec::new();
        let Some(peer) = self.connections.get_mut(conn).and_then(Option::take) else {
            return actions;
        };
        actions.push(Action::Forget { conn });
        if peer == Peer::Client {
            // A native shell gone: its children are orphans.
            let children: Vec<u32> = self
                .pids
                .iter()
                .filter(|(_, r)| r.client_child)
                .map(|(p, _)| *p)
                .collect();
            for c in children {
                self.orphan(c, &mut actions);
            }
        }
        actions
    }

    /// The process job of `pid` is `TERMINATED`.
    pub fn terminated(&mut self, pid: u32) -> Vec<Action<K::H>> {
        let mut actions = Vec::new();
        let Some(r) = self.pids.get_mut(&pid) else {
            return actions;
        };
        if r.terminated {
            return actions;
        }
        r.terminated = true;
        r.stopped = 0;
        let parent = r.parent;
        actions.push(Action::UnwatchJob { pid });
        // Its children are the broker's now.
        let children: Vec<u32> = self
            .pids
            .iter()
            .filter(|(_, r)| r.parent == pid && !r.client_child)
            .map(|(p, _)| *p)
            .collect();
        for c in children {
            self.orphan(c, &mut actions);
        }
        if parent == BROKER_PID && !self.client_child(pid) {
            self.reap_by_broker(pid, &mut actions);
        }
        actions
    }

    fn client_child(&self, pid: u32) -> bool {
        self.pids.get(&pid).is_some_and(|r| r.client_child)
    }

    fn orphan(&mut self, pid: u32, actions: &mut Vec<Action<K::H>>) {
        let Some(r) = self.pids.get_mut(&pid) else {
            return;
        };
        r.parent = BROKER_PID;
        r.client_child = false;
        if r.terminated {
            self.reap_by_broker(pid, actions);
        } else if r.process.is_none() {
            // Prepared, never committed: its parent went in between. Whatever
            // runs in the job ends with it, and the pid goes (review R2), or
            // the session could never finish.
            let job = r.job;
            let _ = self.kernel.job_kill(job);
            self.remove(pid, actions);
        }
    }

    fn reap_by_broker(&mut self, pid: u32, actions: &mut Vec<Action<K::H>>) {
        let record = self
            .pids
            .get(&pid)
            .and_then(|r| r.process)
            .and_then(|p| self.kernel.exit_record(p).ok());
        if pid == self.sid {
            self.shell_exit = record;
        }
        self.counts.reaped_by_broker += 1;
        self.remove(pid, actions);
    }

    /// The stop records a new pid starts with (ADR-0104 item 8a).
    fn child_stops(&self, caller: Option<u32>, fork: bool, ignored: u32) -> [u32; 4] {
        if fork && let Some(r) = caller.and_then(|p| self.pids.get(&p)) {
            return r.stops;
        }
        let mut stops = [stop::DEFAULT; 4];
        for sig in [SIGTSTP, SIGTTIN, SIGTTOU, SIGCONT] {
            if ignored & (1 << sig) != 0
                && let Some(i) = stop_slot(sig)
            {
                stops[i] = stop::IGNORE;
            }
        }
        stops
    }

    fn sid_of(&self, pgid: u32) -> u32 {
        match self.groups.get(&pgid) {
            Some(g) if g.session => self.sid,
            _ => 0,
        }
    }

    fn in_session(&self, pid: u32) -> bool {
        self.pids
            .get(&pid)
            .and_then(|r| self.groups.get(&r.pgid))
            .is_some_and(|g| g.session)
    }

    /// A pid, its group and job, and the child's broker end (item 4).
    #[allow(clippy::type_complexity)] // the reply's three handles, by role
    fn prepare(
        &mut self,
        from: Peer,
        form: Form<K::H>,
        stops: [u32; 4],
        actions: &mut Vec<Action<K::H>>,
    ) -> Result2<(u32, u32, (K::H, K::H), K::H)> {
        if self.next_pid > LAST_PID {
            if let Form::In(job) = form {
                self.kernel.close(job);
            }
            return Err(status::AGAIN);
        }
        let pid = self.next_pid;
        let caller = match from {
            Peer::Process(p) => Some(p),
            _ => None,
        };
        // The group the child goes into, and whether this call makes it.
        let (pgid, new_group) = match form {
            Form::Inherit => {
                let pgid = caller
                    .filter(|&p| p != BROKER_PID)
                    .and_then(|p| self.pids.get(&p))
                    .map(|r| r.pgid)
                    .ok_or(status::INVALID)?;
                (pgid, None)
            }
            Form::Join(pgid) => {
                let g = self.groups.get(&pgid).ok_or(status::NOT_PERMITTED)?;
                // A session process joins session groups only.
                let caller_in_session =
                    caller.is_some_and(|p| p == BROKER_PID || self.in_session(p));
                if g.session != caller_in_session && caller.is_some() {
                    return Err(status::NOT_PERMITTED);
                }
                (pgid, None)
            }
            Form::New => {
                let job = self
                    .kernel
                    .job_create(self.session_job)
                    .map_err(|_| status::AGAIN)?;
                (pid, Some((job, true)))
            }
            Form::In(job) => (pid, Some((job, false))),
        };
        let group_job = match (&new_group, self.groups.get(&pgid)) {
            (Some((job, _)), _) => *job,
            (None, Some(g)) => g.job,
            (None, None) => return Err(status::INVALID),
        };
        let undo = |b: &mut Self| {
            if let Some((job, _)) = new_group {
                b.kernel.close(job);
            }
        };
        let job = match self.kernel.job_create(group_job) {
            Ok(j) => j,
            Err(_) => {
                undo(self);
                return Err(status::AGAIN);
            }
        };
        let handles = (|| {
            let parent_job = self.kernel.duplicate(job, PARENT_JOB_RIGHTS)?;
            let child_job = match self.kernel.duplicate(job, CHILD_JOB_RIGHTS) {
                Ok(c) => c,
                Err(e) => {
                    self.kernel.close(parent_job);
                    return Err(e);
                }
            };
            match self.kernel.channel_create() {
                Ok((server, client)) => Ok((parent_job, child_job, server, client)),
                Err(e) => {
                    self.kernel.close(parent_job);
                    self.kernel.close(child_job);
                    Err(e)
                }
            }
        })();
        let Ok((parent_job, child_job, server, client)) = handles else {
            self.kernel.close(job);
            undo(self);
            return Err(status::AGAIN);
        };
        if let Some((job, session)) = new_group {
            self.groups.insert(
                pgid,
                Group {
                    job,
                    members: 0,
                    session,
                    suspended: false,
                },
            );
        }
        if let Some(g) = self.groups.get_mut(&pgid) {
            g.members += 1;
        }
        let conn = self.add_connection(Peer::Process(pid));
        actions.push(Action::Watch { conn, end: server });
        self.pids.insert(
            pid,
            Record {
                job,
                process: None,
                parent: caller.unwrap_or(0),
                client_child: caller.is_none(),
                pgid,
                stops,
                stopped: 0,
                execed: false,
                terminated: false,
            },
        );
        self.next_pid += 1;
        Ok((pid, pgid, (parent_job, child_job), client))
    }

    fn is_parent(&self, caller: Option<u32>, r: &Record<K::H>) -> bool {
        match caller {
            Some(p) => !r.client_child && r.parent == p,
            None => r.client_child,
        }
    }

    fn commit(
        &mut self,
        caller: Option<u32>,
        pid: u32,
        process: K::H,
        actions: &mut Vec<Action<K::H>>,
    ) -> Result2<()> {
        let ok = match self.pids.get(&pid) {
            None => Err(status::NO_SUCH_PROCESS),
            Some(r) if !self.is_parent(caller, r) => Err(status::NOT_CHILD),
            Some(r) if r.process.is_some() => Err(status::INVALID),
            Some(_) if self.kernel.type_name(process) != Some("process") => Err(status::INVALID),
            Some(_) => Ok(()),
        };
        if let Err(e) = ok {
            self.kernel.close(process);
            return Err(e);
        }
        if let Some(r) = self.pids.get_mut(&pid) {
            r.process = Some(process);
            actions.push(Action::WatchJob { pid, job: r.job });
        }
        Ok(())
    }

    fn abort(
        &mut self,
        caller: Option<u32>,
        pid: u32,
        actions: &mut Vec<Action<K::H>>,
    ) -> Result2<()> {
        match self.pids.get(&pid) {
            None => Err(status::NO_SUCH_PROCESS),
            Some(r) if !self.is_parent(caller, r) => Err(status::NOT_CHILD),
            Some(r) if r.process.is_some() => Err(status::INVALID),
            Some(_) => {
                self.remove(pid, actions);
                Ok(())
            }
        }
    }

    /// The pid's record goes, with its handles and its connection; its
    /// group's too when it was the last.
    fn remove(&mut self, pid: u32, actions: &mut Vec<Action<K::H>>) {
        let Some(r) = self.pids.remove(&pid) else {
            return;
        };
        self.kernel.close(r.job);
        if let Some(p) = r.process {
            self.kernel.close(p);
        }
        if !r.terminated {
            actions.push(Action::UnwatchJob { pid });
        }
        for (i, c) in self.connections.iter_mut().enumerate() {
            if *c == Some(Peer::Process(pid)) {
                *c = None;
                actions.push(Action::Forget { conn: i });
            }
        }
        if let Some(g) = self.groups.get_mut(&r.pgid) {
            g.members -= 1;
            if g.members == 0
                && let Some(g) = self.groups.remove(&r.pgid)
            {
                self.kernel.close(g.job);
                if self.foreground == Some(r.pgid) {
                    // The terminal keeps its handle until the next
                    // foreground; the group is gone.
                    self.foreground = None;
                }
            }
        }
    }

    fn exec(
        &mut self,
        conn: Conn,
        pid: u32,
        process: K::H,
        end: K::H,
        actions: &mut Vec<Action<K::H>>,
    ) -> Result2<()> {
        if self.kernel.type_name(process) != Some("process")
            || self.kernel.type_name(end) != Some("channel")
        {
            self.kernel.close(process);
            self.kernel.close(end);
            return Err(status::INVALID);
        }
        let Some(r) = self.pids.get_mut(&pid) else {
            self.kernel.close(process);
            self.kernel.close(end);
            return Err(status::NO_SUCH_PROCESS);
        };
        let old = r.process.replace(process);
        r.execed = true;
        // `exec` resets caught signals and keeps ignored ones ([P7]).
        for s in &mut r.stops {
            if *s == stop::HANDLER {
                *s = stop::DEFAULT;
            }
        }
        if let Some(old) = old {
            self.kernel.close(old);
        }
        if let Some(c) = self.connections.get_mut(conn) {
            *c = Some(Peer::Retired);
        }
        let new = self.add_connection(Peer::Process(pid));
        actions.push(Action::Watch { conn: new, end });
        Ok(())
    }

    fn reap(
        &mut self,
        caller: Option<u32>,
        pid: u32,
        actions: &mut Vec<Action<K::H>>,
    ) -> Result2<ExitRecord> {
        let r = self.pids.get(&pid).ok_or(status::NO_SUCH_PROCESS)?;
        if !self.is_parent(caller, r) {
            return Err(status::NOT_CHILD);
        }
        let process = r.process.ok_or(status::NOT_TERMINATED)?;
        let job = r.job;
        let done = r.terminated
            || self
                .kernel
                .signals(job)
                .is_ok_and(|s| s & job_signals::TERMINATED != 0);
        if !done {
            return Err(status::NOT_TERMINATED);
        }
        let record = self
            .kernel
            .exit_record(process)
            .map_err(|_| status::NOT_TERMINATED)?;
        if !self.pids.get(&pid).is_some_and(|r| r.terminated) {
            // The parent saw `TERMINATED` before the broker did.
            let mut more = self.terminated(pid);
            actions.append(&mut more);
        }
        self.remove(pid, actions);
        Ok(record)
    }

    fn setpgid(&mut self, caller: Option<u32>, pid: u32, pgid: u32) -> Result2<()> {
        let target = match (pid, caller) {
            (0, Some(p)) => p,
            (0, None) => return Err(status::INVALID),
            (p, _) => p,
        };
        let r = self.pids.get(&target).ok_or(status::NO_SUCH_PROCESS)?;
        let own = caller == Some(target);
        if !own && !self.is_parent(caller, r) {
            return Err(status::NO_SUCH_PROCESS);
        }
        if !own && r.execed {
            return Err(status::ACCESS_DENIED);
        }
        let pgid = if pgid == 0 { target } else { pgid };
        if r.pgid == pgid {
            Ok(())
        } else {
            // Moving a process between jobs is never done (CP-07).
            Err(status::NOT_PERMITTED)
        }
    }

    fn lookup(&mut self, caller: Option<u32>, what: u32, pid: u32) -> Result2<u32> {
        let target = match (pid, caller) {
            (0, Some(p)) => p,
            (0, None) => return Err(status::INVALID),
            (p, _) => p,
        };
        let r = self.pids.get(&target).ok_or(status::NO_SUCH_PROCESS)?;
        Ok(match what {
            lookup::PGID => r.pgid,
            lookup::PPID if r.client_child => BROKER_PID,
            lookup::PPID => r.parent,
            lookup::SID => self.sid_of(r.pgid),
            _ => {
                if !self.is_parent(caller, r) {
                    return Err(status::NOT_CHILD);
                }
                r.stopped
            }
        })
    }

    /// The pids `kill`'s target names (ADR-0104 item 9), and the group job
    /// when it is one group.
    fn targets(&self, caller: Option<u32>, what: u32, id: u32) -> Result2<(Vec<u32>, Option<u32>)> {
        let caller_in_session = caller.is_some_and(|p| self.in_session(p));
        let group_members = |pgid: u32| -> Vec<u32> {
            self.pids
                .iter()
                .filter(|(_, r)| r.pgid == pgid)
                .map(|(p, _)| *p)
                .collect()
        };
        match what {
            target::PID => {
                let r = self.pids.get(&id).ok_or(status::NO_SUCH_PROCESS)?;
                let session = self.groups.get(&r.pgid).is_some_and(|g| g.session);
                if caller.is_some() && session != caller_in_session {
                    return Err(status::NOT_PERMITTED);
                }
                Ok((vec![id], None))
            }
            target::OWN_GROUP | target::GROUP => {
                let pgid = if what == target::GROUP {
                    id
                } else {
                    let p = caller.ok_or(status::INVALID)?;
                    self.pids.get(&p).ok_or(status::NO_SUCH_PROCESS)?.pgid
                };
                let g = self.groups.get(&pgid).ok_or(status::NO_SUCH_PROCESS)?;
                if caller.is_some() && g.session != caller_in_session {
                    return Err(status::NOT_PERMITTED);
                }
                Ok((group_members(pgid), Some(pgid)))
            }
            _ => {
                // Every process of the session but the caller ([P13]).
                let all: Vec<u32> = self
                    .pids
                    .iter()
                    .filter(|(p, r)| {
                        Some(**p) != caller && self.groups.get(&r.pgid).is_some_and(|g| g.session)
                    })
                    .map(|(p, _)| *p)
                    .collect();
                if all.is_empty() {
                    return Err(status::NO_SUCH_PROCESS);
                }
                Ok((all, None))
            }
        }
    }

    fn kill(&mut self, caller: Option<u32>, what: u32, id: u32, sig: u32) -> Result2<()> {
        if sig > signal::MAX || (what != target::ALL && what != target::OWN_GROUP && id == 0) {
            return Err(status::INVALID);
        }
        let (pids, group) = self.targets(caller, what, id)?;
        if sig == 0 {
            return Ok(());
        }
        self.signal_pids(&pids, group, sig)
    }

    /// The terminal's suspend character on the foreground group (ADR-0105
    /// item 3): as `kill(-group, SIGTSTP)` from the broker itself.
    pub fn suspend(&mut self, group: u32, sig: u32) {
        if self.foreground != Some(group) || !matches!(sig, SIGTSTP | SIGTTIN | SIGTTOU) {
            return;
        }
        let members: Vec<u32> = self
            .pids
            .iter()
            .filter(|(_, r)| r.pgid == group)
            .map(|(p, _)| *p)
            .collect();
        let _ = self.signal_pids(&members, Some(group), sig);
    }

    fn signal_pids(&mut self, pids: &[u32], group: Option<u32>, sig: u32) -> Result2<()> {
        let live: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|p| self.pids.get(p).is_some_and(|r| !r.terminated))
            .collect();
        let group_job = group.and_then(|g| self.groups.get(&g)).map(|g| g.job);
        let stop = matches!(sig, SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU);
        if let Some(job) = group_job {
            // One call on the group job when every member acts the same.
            if sig == SIGKILL {
                return sent(self.kernel.job_kill(job));
            }
            if !signal::is_stop_or_continue(sig) {
                let kind = signal::kind(sig).ok_or(status::INVALID)?;
                return sent(self.kernel.job_signal(job, kind, sig));
            }
            let all_default = live.iter().all(|p| {
                self.pids.get(p).is_some_and(|r| {
                    sig == SIGSTOP || stop_slot(sig).is_none_or(|i| r.stops[i] == stop::DEFAULT)
                })
            });
            if stop && all_default && !live.is_empty() {
                sent(self.kernel.job_suspend(job))?;
                if let Some(g) = group.and_then(|g| self.groups.get_mut(&g)) {
                    g.suspended = true;
                }
                for p in &live {
                    if let Some(r) = self.pids.get_mut(p) {
                        r.stopped = sig;
                    }
                }
                return Ok(());
            }
        }
        let mut result = Ok(());
        for p in live {
            let r = self.signal_pid(p, sig);
            if result.is_ok() {
                result = r;
            }
        }
        result
    }

    fn signal_pid(&mut self, pid: u32, sig: u32) -> Result2<()> {
        let Some(r) = self.pids.get(&pid) else {
            return Err(status::NO_SUCH_PROCESS);
        };
        let job = r.job;
        match sig {
            SIGKILL => sent(self.kernel.job_kill(job)),
            SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => {
                let disposition = stop_slot(sig).map_or(stop::DEFAULT, |i| r.stops[i]);
                match disposition {
                    stop::DEFAULT => self.stop_pid(pid, sig),
                    stop::HANDLER => {
                        sent(self.kernel.job_signal(job, RequestKind::Custom(sig), sig))
                    }
                    _ => Ok(()),
                }
            }
            SIGCONT => {
                let handler = r.stops[3] == stop::HANDLER;
                let pgid = r.pgid;
                // A group stopped whole is continued whole, every member.
                if let Some(g) = self.groups.get_mut(&pgid)
                    && g.suspended
                {
                    g.suspended = false;
                    let group_job = g.job;
                    sent(self.kernel.job_resume(group_job))?;
                    for r in self.pids.values_mut().filter(|r| r.pgid == pgid) {
                        r.stopped = 0;
                    }
                }
                let was = self.pids.get(&pid).map_or(0, |r| r.stopped);
                if was != 0 {
                    sent(self.kernel.job_resume(job))?;
                }
                if let Some(r) = self.pids.get_mut(&pid) {
                    r.stopped = 0;
                }
                if handler {
                    sent(
                        self.kernel
                            .job_signal(job, RequestKind::Custom(SIGCONT), SIGCONT),
                    )?;
                }
                Ok(())
            }
            n => {
                let kind = signal::kind(n).ok_or(status::INVALID)?;
                sent(self.kernel.job_signal(job, kind, n))
            }
        }
    }

    fn stop_pid(&mut self, pid: u32, sig: u32) -> Result2<()> {
        let r = self.pids.get_mut(&pid).ok_or(status::NO_SUCH_PROCESS)?;
        let job = r.job;
        r.stopped = sig;
        sent(self.kernel.job_suspend(job))
    }

    fn set_foreground(&mut self, pgid: u32, actions: &mut Vec<Action<K::H>>) -> Result2<()> {
        if !self.terminal {
            return Err(status::NO_TERMINAL);
        }
        let g = self.groups.get(&pgid).ok_or(status::NOT_PERMITTED)?;
        if !g.session {
            return Err(status::NOT_PERMITTED);
        }
        // TRANSFER so that it can be moved; the move lists the terminal's
        // rights, which it then holds.
        let job = self
            .kernel
            .duplicate(g.job, FOREGROUND_RIGHTS.union(Rights::TRANSFER))
            .map_err(|_| status::AGAIN)?;
        self.foreground = Some(pgid);
        actions.push(Action::Foreground { group: pgid, job });
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum Form<H> {
    Inherit,
    New,
    Join(u32),
    In(H),
}

/// `kill`'s errors from the kernel: a full request queue is `EAGAIN`
/// (ADR-0106's errors); a target already gone has nothing to do.
fn sent(r: Result<(), Status>) -> Result2<()> {
    match r {
        Err(Status::LimitReached) => Err(status::AGAIN),
        _ => Ok(()),
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

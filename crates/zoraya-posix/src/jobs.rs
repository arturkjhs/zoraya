//! The process half of the library's logic (ADR-0104 to ADR-0106): the wait
//! points, the broker's client, the children and `waitpid`, `kill`, the
//! terminal's requests, and what each taken request asks. The C boundary
//! (`ffi`) runs the handlers and makes `spawn`, `fork` and `exec`.

#![forbid(unsafe_code)]

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use zoraya_abi::posix::{EntryKind, FD_NONBLOCK, FD_READ, FD_WRITE, Fd, FdKind, NO_HANDLE};
use zoraya_proto_file::{self as fp, rights};
use zoraya_protocol::Flags;

use zoraya_abi::posix::signal::{self, SIGCHLD, SIGTTIN};
use zoraya_abi::{ExitReason, ExitRecord, RequestKind, Status, job_signals, process_signals};
use zoraya_proto_posix as bp;
use zoraya_proto_tty as tp;
use zoraya_protocol::{Below, ERROR_ORDINAL, ErrorReply, Header, Message, decode, encode};

use crate::errno::*;
use crate::process::{Desc, Posix};
use crate::signals::{DefaultAction, Taken, bit};
use crate::sys::{Sys, User, signals};
use crate::wait;

/// `waitpid`'s options.
pub const WNOHANG: i32 = 1;
pub const WUNTRACED: i32 = 2;

/// A child the process has not waited for.
#[derive(Debug, Clone, Copy)]
pub struct Child<H> {
    pub pid: u32,
    pub pgid: u32,
    /// The parent's handle to its process job (`WAIT | DUPLICATE | TRANSFER`).
    pub job: H,
    /// Its stop was reported by `waitpid(WUNTRACED)`.
    pub stop_reported: bool,
    /// Its end was taken as a `SIGCHLD` at a wait point.
    pub notified: bool,
}

/// What the C boundary must do next for a signal (ADR-0106 items 4 and 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Nothing more to take.
    None,
    /// Run this signal's handler.
    Run(u32, crate::signals::Action),
    /// End the process: `process_exit_with_reason(Terminated, n)`.
    Terminate(u32),
    /// Stop: the broker's `stop_self`.
    Stop(u32),
}

/// The status of a broker reply: its first field, or the error reply's.
fn broker_status(bytes: &[u8]) -> u32 {
    match Header::decode(bytes) {
        Ok((h, body)) if h.ordinal != ERROR_ORDINAL && body.len() >= 4 => {
            u32::from_le_bytes([body[0], body[1], body[2], body[3]])
        }
        Ok((h, _)) if h.ordinal == ERROR_ORDINAL => {
            decode::<ErrorReply>(bytes, 0).map_or(bp::status::INVALID, |(_, e)| e.status)
        }
        _ => bp::status::INVALID,
    }
}

/// The broker's statuses as `errno` values.
pub fn broker_errno(s: u32) -> i32 {
    use bp::status::*;
    match s {
        OK => 0,
        NO_SUCH_PROCESS => ESRCH,
        NOT_PERMITTED => EPERM,
        ACCESS_DENIED => EACCES,
        AGAIN => EAGAIN,
        NO_TERMINAL => ENOTTY,
        NOT_CHILD => ECHILD,
        _ => EINVAL,
    }
}

impl<S: Sys> Posix<S> {
    // ------------------------------------------------------------ wait points

    /// The sources every wait point listens to besides its own (item 3): the
    /// process's own handle for a request when a handler is installed, and
    /// the children's jobs while `SIGCHLD` is caught and unblocked.
    pub fn interrupt_sources(&self, out: &mut Vec<(S::H, u32)>) {
        if self.signals.handlers_installed()
            && let Some(me) = self.self_handle
        {
            out.push((me, process_signals::REQUEST_PENDING));
        }
        if self.signals.catches(SIGCHLD) {
            for c in self.children.iter().filter(|c| !c.notified) {
                out.push((c.job, job_signals::TERMINATED));
            }
        }
    }

    /// What woke a wait point, past its own sources: `EINTR` for the C
    /// boundary to deliver, or nothing to do (`Ok`).
    fn interrupted(&mut self, which: (S::H, u32)) -> Result<(), i32> {
        if Some(which.0) == self.self_handle && which.1 == process_signals::REQUEST_PENDING {
            return Err(EINTR);
        }
        if let Some(c) = self.children.iter_mut().find(|c| c.job == which.0) {
            c.notified = true;
            self.sigchld = true;
            return Err(EINTR);
        }
        Ok(())
    }

    /// Counts a kernel wait (ADR-0110 item 7's wake-ups).
    fn count_wait(&mut self) -> &mut Self {
        self.stats.waits += 1;
        self
    }

    /// Waits for `h` to assert one of `wanted` at a wait point: `EINTR` when a
    /// signal is to be delivered first (ADR-0106 item 3).
    pub fn wait_point(&mut self, h: S::H, wanted: u32) -> Result<u32, i32> {
        self.wait_for(&[(h, wanted)], None)
            .map(|(_, s)| s.unwrap_or(0))
    }

    /// Waits for one of `own` at a wait point, or the deadline (`None` for
    /// the index then). `EINTR` when a signal came first.
    pub fn wait_for(
        &mut self,
        own: &[(S::H, u32)],
        deadline: Option<u64>,
    ) -> Result<(Option<usize>, Option<u32>), i32> {
        let mut sources = Vec::with_capacity(own.len() + 4);
        sources.extend_from_slice(own);
        self.interrupt_sources(&mut sources);
        if sources.is_empty() {
            return match deadline {
                // Nothing to wait for and no end: a wait that never ends.
                None => Err(EINVAL),
                Some(_) => match self.count_wait().sys.wait_any(&[], deadline) {
                    Ok(_) | Err(Status::DeadlineExceeded) => Ok((None, None)),
                    Err(s) => Err(crate::errno::from_status(s, crate::errno::Call::Other)),
                },
            };
        }
        self.stats.waits += 1;
        match self.sys.wait_any(&sources, deadline) {
            Ok((i, s)) if i < own.len() => Ok((Some(i), Some(s))),
            Ok((i, _)) => {
                self.interrupted(sources[i])?;
                // A source that is no longer a reason: go round again.
                Ok((None, Some(0)))
            }
            Err(Status::DeadlineExceeded) => Ok((None, None)),
            Err(s) => Err(crate::errno::from_status(s, crate::errno::Call::Other)),
        }
    }

    // ------------------------------------------------------------ delivery

    /// The next signal to act on (ADR-0106 item 4): a child's end seen at a
    /// wait point, then the kernel's queue, then the pending set as the mask
    /// allows. Blocked ones go into the pending set on the way.
    pub fn next_delivery(&mut self) -> Next {
        loop {
            let n = if core::mem::take(&mut self.sigchld) {
                SIGCHLD
            } else if let Some((kind, detail)) = {
                self.stats.takes += 1;
                self.sys.take_request()
            } {
                signal::number(kind, detail)
            } else if let Some(n) = self.signals.next_unblocked() {
                n
            } else {
                return Next::None;
            };
            match self.signals.take(n) {
                Taken::Run(a) => return Next::Run(n, a),
                Taken::Default(DefaultAction::Terminate) => return Next::Terminate(n),
                Taken::Default(DefaultAction::Stop) => return Next::Stop(n),
                Taken::Default(_) | Taken::Drop | Taken::Pending => {}
            }
        }
    }

    /// Whether a library entry polls for requests (ADR-0106 item 5): only
    /// with a handler installed (INV-0106-2), or a pending signal the mask
    /// now lets through.
    pub fn should_poll(&self) -> bool {
        self.signals.handlers_installed() || self.signals.pending & !self.signals.blocked != 0
    }

    // ------------------------------------------------------------ the broker

    /// Sends `m` to the broker and waits for its reply (in `self.input`).
    pub fn broker_call<'m, M: Message<'m>>(&mut self, m: &M, moved: &[S::H]) -> Result<usize, i32> {
        let Some(broker) = self.broker else {
            for &h in moved {
                self.sys.close(h);
            }
            return Err(ENOSYS);
        };
        self.stats.broker += 1;
        self.call(broker, m, moved).map_err(|_| EAGAIN)
    }

    /// ADR-0110 item 7: the process's counters and call bitmap to the
    /// broker, which sums them for the session.
    pub fn report_stats(&mut self, counters: &[u8], calls: &[u8]) -> Result<(), i32> {
        self.broker_simple(
            &bp::ReportStats {
                counters: zoraya_protocol::Data(counters),
                calls: zoraya_protocol::Data(calls),
            },
            &[],
        )
    }

    /// A broker request answered by a status only.
    pub fn broker_simple<'m, M: Message<'m>>(&mut self, m: &M, moved: &[S::H]) -> Result<(), i32> {
        let n = self.broker_call(m, moved)?;
        match broker_errno(broker_status(&self.input[..n])) {
            0 => Ok(()),
            e => Err(e),
        }
    }

    /// `spawn_prepare` (ADR-0104 item 4): the pid, its group and session,
    /// the parent's job handle, the child's, and the child's broker end.
    pub fn prepare(
        &mut self,
        form: u32,
        pgid: u32,
        ignored: u32,
        fork: bool,
    ) -> Result<Prepared<S::H>, i32> {
        let m = bp::SpawnPrepare {
            group: Below(form),
            pgid,
            ignored,
            fork: Below(u32::from(fork)),
        };
        let start = self.sys.now();
        let n = self.broker_call(&m, &[])?;
        self.stats.prepares += 1;
        self.stats.prepare_ns += self.sys.now().saturating_sub(start);
        let handles = core::mem::take(&mut self.handles);
        match decode::<bp::SpawnPrepared>(&self.input[..n], handles.len()) {
            Ok((_, r)) => Ok(Prepared {
                pid: r.pid,
                pgid: r.pgid,
                sid: r.sid,
                parent_job: handles[0],
                child_job: handles[1],
                broker_end: handles[2],
            }),
            Err(_) => {
                for h in handles {
                    self.sys.close(h);
                }
                Err(broker_errno(broker_status(&self.input[..n])))
            }
        }
    }

    // ------------------------------------------------------------ children

    /// `waitpid`'s candidates: `pid > 0` that child, `-1` every child, `0`
    /// those of the caller's group, `< -1` those of group `-pid`.
    fn candidates(&self, pid: i32) -> Vec<usize> {
        let own = self.facts.pgid;
        (0..self.children.len())
            .filter(|&i| {
                let c = &self.children[i];
                match pid {
                    -1 => true,
                    0 => c.pgid == own,
                    p if p > 0 => c.pid == p as u32,
                    p => c.pgid == p.unsigned_abs(),
                }
            })
            .collect()
    }

    /// The child at `i` has ended: its exit record from the broker, and the
    /// child is gone from the list.
    fn reap(&mut self, i: usize) -> Result<(u32, i32), i32> {
        let c = self.children[i];
        let start = self.sys.now();
        let n = self.broker_call(&bp::Reap { pid: c.pid }, &[])?;
        self.stats.reaps += 1;
        self.stats.reap_ns += self.sys.now().saturating_sub(start);
        let (_, r) = decode::<bp::Reaped>(&self.input[..n], 0).map_err(|_| EINVAL)?;
        if r.status.0 != bp::status::OK {
            return Err(broker_errno(r.status.0));
        }
        let record = ExitRecord {
            code: r.code as i32,
            reason: ExitReason::from_raw(r.reason).unwrap_or(ExitReason::Exited),
            detail: r.detail,
            request_kind: r.kind.checked_sub(1).and_then(RequestKind::from_raw),
        };
        self.children.swap_remove(i);
        self.sys.close(c.job);
        Ok((c.pid, wait::status(&record)))
    }

    /// One look at the candidates: a reaped child, a stopped one to report,
    /// or nothing.
    fn look_children(&mut self, pid: i32, options: i32) -> Result<Option<(u32, i32)>, i32> {
        for i in self.candidates(pid) {
            let job = self.children[i].job;
            let s = self.sys.signals_of(job).map_err(|_| ECHILD)?;
            if s & job_signals::TERMINATED != 0 {
                return self.reap(i).map(Some);
            }
            if s & job_signals::SUSPENDED == 0 {
                self.children[i].stop_reported = false;
            } else if options & WUNTRACED != 0 && !self.children[i].stop_reported {
                let child = self.children[i].pid;
                let m = bp::Lookup {
                    what: Below(bp::lookup::STOP_SIGNAL),
                    pid: child,
                };
                let n = self.broker_call(&m, &[])?;
                let sig =
                    decode::<bp::LookupReply>(&self.input[..n], 0).map_or(0, |(_, r)| r.value);
                if sig != 0 {
                    self.children[i].stop_reported = true;
                    return Ok(Some((child, wait::stopped(sig))));
                }
            }
        }
        Ok(None)
    }

    /// `waitpid` (ADR-0104 item 11, ADR-0106 item 3): `None` for `WNOHANG`
    /// with nothing to report. `EINTR` when a signal is to be delivered.
    pub fn waitpid(&mut self, pid: i32, options: i32) -> Result<Option<(u32, i32)>, i32> {
        if options & !(WNOHANG | WUNTRACED | 8/* WCONTINUED */) != 0 {
            return Err(EINVAL);
        }
        loop {
            if self.candidates(pid).is_empty() {
                return Err(ECHILD);
            }
            if let Some(r) = self.look_children(pid, options)? {
                return Ok(Some(r));
            }
            if options & WNOHANG != 0 {
                return Ok(None);
            }
            let mut wanted = job_signals::TERMINATED;
            if options & WUNTRACED != 0 {
                wanted |= job_signals::SUSPENDED;
            }
            let own: Vec<(S::H, u32)> = self
                .candidates(pid)
                .into_iter()
                .map(|i| (self.children[i].job, wanted))
                .collect();
            self.wait_for(&own, None)?;
        }
    }

    // ------------------------------------------------------------ the terminal

    /// A terminal request: sends `m` on `conn` and gives the reply's size.
    pub fn tty_call<'m, M: Message<'m>>(&mut self, conn: S::H, m: &M) -> Result<usize, i32> {
        self.call(conn, m, &[]).map_err(|_| EIO)
    }

    /// The terminal connection of a tty fd.
    pub fn tty_of(&self, fd: i32) -> Result<S::H, i32> {
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(Desc::Stream {
                tty: true,
                conn: Some(c),
                ..
            }) => Ok(*c),
            Some(_) => Err(ENOTTY),
        }
    }

    /// `read` of a terminal (ADR-0105 items 4, 6 and 8): one `Read` held by
    /// the service until a line is complete, kept across an interrupted wait
    /// so that the next `read` takes its answer.
    pub(crate) fn read_tty(
        &mut self,
        fd: i32,
        conn: S::H,
        into: User,
        nonblock: bool,
    ) -> Result<usize, i32> {
        let _ = fd;
        if into.length == 0 {
            return Ok(0);
        }
        // What an earlier answer gave beyond its reader's buffer (review R3).
        if let Some(i) = self.tty_rest.iter().position(|(c, _)| *c == conn) {
            let rest = &mut self.tty_rest[i].1;
            let n = rest.len().min(into.length);
            let bytes: Vec<u8> = rest.drain(..n).collect();
            if rest.is_empty() {
                self.tty_rest.swap_remove(i);
            }
            self.sys.copy_out(into.part(0, n), &bytes);
            return Ok(n);
        }
        loop {
            let mut sent_now = false;
            let t = match self.tty_reads.iter().find(|(c, _)| *c == conn) {
                Some(&(_, t)) => t,
                None => {
                    self.transaction = self.transaction.wrapping_add(1).max(1);
                    let t = self.transaction;
                    let m = tp::Read {
                        max: into.length.min(tp::MAX_READ) as u32,
                    };
                    let n = encode(&m, t, &mut self.out).map_err(|_| EINVAL)?;
                    self.sys
                        .channel_write(conn, &self.out[..n], &[])
                        .map_err(|_| EIO)?;
                    self.tty_reads.push((conn, t));
                    sent_now = true;
                    t
                }
            };
            // The answer: one another request's wait kept, or the next on
            // the channel.
            let reply: Vec<u8> = match self.tty_stash.iter().position(|(c, _)| *c == conn) {
                Some(i) => self.tty_stash.swap_remove(i).1,
                None => loop {
                    for h in self.handles.drain(..) {
                        self.sys.close(h);
                    }
                    match self
                        .sys
                        .channel_read(conn, &mut self.input, &mut self.handles)
                    {
                        Ok(n) => break self.input[..n].to_vec(),
                        Err(Status::WouldBlock) if nonblock => return Err(EAGAIN),
                        Err(Status::WouldBlock) => {
                            let s =
                                self.wait_point(conn, signals::READABLE | signals::PEER_CLOSED)?;
                            if s & signals::READABLE == 0 && s & signals::PEER_CLOSED != 0 {
                                self.tty_reads.retain(|(c, _)| *c != conn);
                                return Ok(0);
                            }
                        }
                        Err(_) => {
                            self.tty_reads.retain(|(c, _)| *c != conn);
                            return Ok(0);
                        }
                    }
                },
            };
            let Ok((header, _)) = Header::decode(&reply) else {
                continue;
            };
            if header.transaction != t {
                // A reply to nothing outstanding: dropped.
                continue;
            }
            self.tty_reads.retain(|(c, _)| *c != conn);
            if header.ordinal == ERROR_ORDINAL {
                let s =
                    decode::<ErrorReply>(&reply, 0).map_or(tp::status::INVALID, |(_, e)| e.status);
                if s == tp::status::NOT_FOREGROUND {
                    if !sent_now {
                        // The answer to a read held from before: the group
                        // may be in the foreground again; ask afresh
                        // (review R10).
                        continue;
                    }
                    // SIGTTIN (ADR-0105 item 8): EIO if ignored or blocked.
                    let a = self.signals.action(SIGTTIN);
                    if a.handler == crate::signals::SIG_IGN
                        || self.signals.blocked & bit(SIGTTIN) != 0
                    {
                        return Err(EIO);
                    }
                    if a.is_handler() {
                        // The handler runs, then the read is EINTR.
                        self.signals.pending |= bit(SIGTTIN);
                        return Err(EINTR);
                    }
                    self.broker_simple(&bp::StopSelf { signal: SIGTTIN }, &[])?;
                    continue;
                }
                return Err(EIO);
            }
            let Ok((_, r)) = decode::<tp::ReadReply<'_>>(&reply, 0) else {
                return Err(EIO);
            };
            let bytes = r.bytes.0;
            let got = bytes.len().min(into.length);
            if got < bytes.len() {
                self.tty_rest.push((conn, bytes[got..].to_vec()));
            }
            let mine = bytes[..got].to_vec();
            self.sys.copy_out(into.part(0, got), &mine);
            return Ok(got);
        }
    }

    // ------------------------------------------------------------ inheritance

    /// Another connection to `conn`'s description (`share`, ADR-0103 item 1).
    pub fn share_conn(&mut self, conn: S::H, wanted: u32) -> Result<S::H, i32> {
        let (mine, theirs) = self.sys.channel_create().map_err(|_| EMFILE)?;
        self.stats.shares += 1;
        let m = fp::Share {
            rights: Flags(wanted),
        };
        match self.done(conn, &m, &[theirs]) {
            Ok(_) => Ok(mine),
            Err(e) => {
                self.sys.close(mine);
                Err(e)
            }
        }
    }

    /// A fresh connection to `conn`'s object (`clone`): every right first,
    /// then reading alone, for a connection that has fewer.
    pub fn clone_conn(&mut self, conn: S::H) -> Result<S::H, i32> {
        for wanted in [rights::ALL, rights::READ | rights::CLONE] {
            let (mine, theirs) = self.sys.channel_create().map_err(|_| EMFILE)?;
            let m = fp::Clone {
                rights: Flags(wanted),
            };
            match self.done(conn, &m, &[theirs]) {
                Ok(_) => return Ok(mine),
                Err(e) => {
                    self.sys.close(mine);
                    if e != EACCES {
                        return Err(e);
                    }
                }
            }
        }
        Err(EACCES)
    }

    /// A terminal connection for a child tagged with `group` (ADR-0105 item
    /// 4).
    pub fn clone_tty(&mut self, conn: S::H, group: u32) -> Result<S::H, i32> {
        let (mine, theirs) = self.sys.channel_create().map_err(|_| EMFILE)?;
        let m = tp::Clone { group };
        match self.call(conn, &m, &[theirs]) {
            Ok(n) if crate::process::reply_ok(&self.input[..n]) => Ok(mine),
            Ok(_) => {
                self.sys.close(mine);
                Err(EAGAIN)
            }
            Err(e) => {
                self.sys.close(mine);
                Err(e)
            }
        }
    }

    fn close_all(&mut self, handles: impl IntoIterator<Item = S::H>) {
        for h in handles {
            self.sys.close(h);
        }
    }

    /// The child's fds after the file actions: child fd → parent fd, each
    /// without `FD_CLOEXEC` at the end (POSIX: the actions run in the child
    /// before the exec).
    pub fn child_fds(&self, actions: &[FileAction]) -> Result<BTreeMap<u32, u32>, i32> {
        let mut map: BTreeMap<u32, (u32, bool)> = BTreeMap::new();
        for fd in self.fds.fds() {
            let cloexec = self.fds.cloexec(fd as i32).unwrap_or(true);
            map.insert(fd, (fd, cloexec));
        }
        for a in actions {
            match *a {
                FileAction::Dup2(from, to) => {
                    let (Ok(from), Ok(to)) = (u32::try_from(from), u32::try_from(to)) else {
                        return Err(EBADF);
                    };
                    let (source, _) = *map.get(&from).ok_or(EBADF)?;
                    map.insert(to, (source, false));
                }
                FileAction::Close(fd) => {
                    if let Ok(fd) = u32::try_from(fd) {
                        map.remove(&fd);
                    }
                }
            }
        }
        Ok(map
            .into_iter()
            .filter(|(_, (_, cloexec))| !cloexec)
            .map(|(child, (parent, _))| (child, parent))
            .collect())
    }

    /// What a child spawned in `group` inherits (ADR-0103 item 7): every fd
    /// left by the file actions, and the namespace table (ADR-0107). On an
    /// error every handle made for it is closed.
    pub fn inherit(&mut self, actions: &[FileAction], group: u32) -> Result<Inherit<S::H>, i32> {
        let map = self.child_fds(actions)?;
        let mut out = Inherit {
            slots: Vec::new(),
            fds: Vec::new(),
            message: Vec::new(),
            entries: Vec::new(),
        };
        let r = self.inherit_into(&map, group, &mut out);
        if let Err(e) = r {
            let made: Vec<S::H> = out
                .slots
                .iter()
                .filter(|s| s.2)
                .map(|s| s.1)
                .chain(out.message.iter().copied())
                .collect();
            self.close_all(made);
            return Err(e);
        }
        Ok(out)
    }

    fn inherit_into(
        &mut self,
        map: &BTreeMap<u32, u32>,
        group: u32,
        out: &mut Inherit<S::H>,
    ) -> Result<(), i32> {
        for (&child, &parent) in map {
            self.stats.inherit_failed_at = child;
            let desc = match self.fds.get(parent as i32) {
                Some(Desc::Object {
                    conn,
                    kind,
                    access,
                    nonblock,
                    ..
                }) => Some((*conn, *kind, *access, *nonblock)),
                _ => None,
            };
            if let Some((conn, kind, access, nonblock)) = desc {
                let dir = kind == crate::stat::Kind::Directory;
                let wanted = if dir {
                    rights::READ | rights::CLONE
                } else {
                    share_rights(access)
                };
                let end = self.share_conn(conn, wanted)?;
                out.slots.push((child, end, true));
                out.fds.push(Fd {
                    fd: child,
                    kind: if dir { FdKind::Dir } else { FdKind::File },
                    flags: (if nonblock { FD_NONBLOCK } else { 0 })
                        | match access & 3 {
                            0 => FD_READ,
                            1 => FD_WRITE,
                            _ => FD_READ | FD_WRITE,
                        },
                    tty: NO_HANDLE,
                });
                continue;
            }
            let (read, write, nonblock, tty, conn) = match self.fds.get(parent as i32) {
                Some(&Desc::Stream {
                    read,
                    write,
                    nonblock,
                    tty,
                    conn,
                }) => (read, write, nonblock, tty, conn),
                Some(Desc::Null { .. }) => {
                    out.fds.push(Fd {
                        fd: child,
                        kind: FdKind::Null,
                        flags: 0,
                        tty: NO_HANDLE,
                    });
                    continue;
                }
                _ => return Err(EBADF),
            };
            let flags = if nonblock { FD_NONBLOCK } else { 0 };
            if tty && let (Some(w), Some(c)) = (write, conn) {
                let index = out.message.len();
                if index >= zoraya_abi::posix::MAX_MESSAGE_HANDLES {
                    return Err(E2BIG);
                }
                let mine = self.clone_tty(c, group)?;
                out.message.push(mine);
                out.slots.push((child, w, false));
                out.fds.push(Fd {
                    fd: child,
                    kind: FdKind::Tty,
                    flags,
                    tty: index as u8,
                });
            } else if let Some(r) = read {
                out.slots.push((child, r, false));
                out.fds.push(Fd {
                    fd: child,
                    kind: FdKind::StreamRead,
                    flags,
                    tty: NO_HANDLE,
                });
            } else if let Some(w) = write {
                out.slots.push((child, w, false));
                out.fds.push(Fd {
                    fd: child,
                    kind: FdKind::StreamWrite,
                    flags,
                    tty: NO_HANDLE,
                });
            }
        }
        for i in 0..self.entries.len() {
            self.stats.inherit_failed_at = 1000 + i as u32;
            let (kind, handle) = (self.entries[i].kind, self.entries[i].handle);
            let made = match (kind, handle) {
                (EntryKind::Directory | EntryKind::Tmp, Some(h)) => Some(self.clone_conn(h)?),
                (EntryKind::Terminal, Some(h)) => Some(self.clone_tty(h, group)?),
                _ => None,
            };
            let index = match made {
                Some(h) => {
                    if out.message.len() >= zoraya_abi::posix::MAX_MESSAGE_HANDLES {
                        self.sys.close(h);
                        return Err(E2BIG);
                    }
                    out.message.push(h);
                    (out.message.len() - 1) as u8
                }
                None => NO_HANDLE,
            };
            // An image entry is the spawner's to fill (it holds the image).
            out.entries.push((i, kind, index));
        }
        Ok(())
    }

    /// `fork`'s move list (ADR-0102 item 4, ADR-0103 item 8): a new
    /// connection for every connection the process holds, at the slot the
    /// parent's own sits in, so the copied table is right in the child; the
    /// child's broker end and process job at theirs.
    pub fn fork_moves(
        &mut self,
        group: u32,
        prepared: &Prepared<S::H>,
    ) -> Result<Vec<(u32, S::H)>, i32> {
        let mut moves: Vec<(u32, S::H)> = Vec::new();
        let r = self.fork_moves_into(group, prepared, &mut moves);
        if let Err(e) = r {
            let made: Vec<S::H> = moves.iter().map(|m| m.1).collect();
            self.close_all(made);
            return Err(e);
        }
        Ok(moves)
    }

    fn fork_moves_into(
        &mut self,
        group: u32,
        prepared: &Prepared<S::H>,
        moves: &mut Vec<(u32, S::H)>,
    ) -> Result<(), i32> {
        let (Some(broker), Some(job)) = (self.broker, self.process_job) else {
            return Err(ENOSYS);
        };
        // Every connection once: descriptions, entries, cwd.
        let mut conns: Vec<(S::H, u32)> = Vec::new();
        for d in self.fds.descriptions() {
            match *d {
                Desc::Object {
                    conn, access, kind, ..
                } => {
                    let wanted = if kind == crate::stat::Kind::Directory {
                        rights::READ | rights::CLONE
                    } else {
                        share_rights(access)
                    };
                    conns.push((conn, wanted));
                }
                Desc::Stream {
                    conn: Some(c),
                    tty,
                    read,
                    ..
                } => {
                    let wanted = if tty {
                        u32::MAX
                    } else if read.is_some() {
                        rights::READ | rights::CLONE
                    } else {
                        rights::WRITE | rights::CLONE
                    };
                    conns.push((c, wanted));
                }
                // A socket's three handles do not cross `fork` yet
                // (ADR-0225 item 9; ADR-0287 lists it as not built): refused
                // rather than copied as a description with no handles.
                Desc::Socket(_) => return Err(crate::errno::ENOTSUP),
                _ => {}
            }
        }
        for (conn, wanted) in conns {
            if moves.len() + 4 >= zoraya_abi::spawn::MAX_SPAWN_HANDLES {
                return Err(EAGAIN);
            }
            let new = if wanted == u32::MAX {
                self.clone_tty(conn, group)?
            } else {
                self.share_conn(conn, wanted)?
            };
            moves.push((self.sys.slot_of(conn), new));
        }
        for i in 0..self.entries.len() {
            let (kind, handle) = (self.entries[i].kind, self.entries[i].handle);
            let made = match (kind, handle) {
                (EntryKind::Directory | EntryKind::Tmp, Some(h)) => Some((h, self.clone_conn(h)?)),
                (EntryKind::Terminal, Some(h)) => Some((h, self.clone_tty(h, group)?)),
                _ => None,
            };
            if let Some((h, new)) = made {
                moves.push((self.sys.slot_of(h), new));
            }
        }
        if let Some(c) = self.cwd_conn {
            let new = self.clone_conn(c)?;
            moves.push((self.sys.slot_of(c), new));
        }
        moves.push((self.sys.slot_of(broker), prepared.broker_end));
        moves.push((self.sys.slot_of(job), prepared.child_job));
        if moves.len() > zoraya_abi::spawn::MAX_SPAWN_HANDLES {
            return Err(EAGAIN);
        }
        Ok(())
    }

    /// A `fork` child puts right what its copied state says of its parent
    /// (ADR-0102's wrapper): its own facts, no children, no outstanding
    /// terminal reads, no attached buffer, no pending signals.
    pub fn after_fork_in_child(&mut self, pid: u32, pgid: u32) {
        let parent = self.facts.pid;
        self.facts.pid = pid;
        self.facts.ppid = parent;
        self.facts.pgid = pgid;
        let children: Vec<S::H> = self.children.drain(..).map(|c| c.job).collect();
        self.close_all(children);
        self.tty_reads.clear();
        self.tty_stash.clear();
        self.tty_rest.clear();
        self.signals.pending = 0;
        self.sigchld = false;
        self.fork_group = -1;
        // The parent's channels are not the child's: nothing to report on.
        self.report = None;
        // A channel end cannot be duplicated (ADR-0102 item 3), so the
        // connection to `netd` stays with the parent: the child's `net` would
        // name an empty slot that a later handle can take. The child has no
        // network, as a program without the grant (ADR-0287 item 3). The
        // slot is not closed: it is not the child's.
        self.net = None;
        // The same for the random service's connection: the child's
        // `getrandom` is `ENOSYS`, not a read of the parent's slot.
        self.random = None;
        self.addrs = crate::socket::AddrCache::default();
        for d in self.fds.descriptions_mut() {
            if let Desc::Object { attached, .. } = d {
                *attached = false;
            }
        }
        if let Some((object, address)) = self.scratch.take() {
            let _ = self
                .sys
                .memory_unmap(address, crate::process::MAX_BUFFER as u64);
            self.sys.close(object);
        }
    }

    // ------------------------------------------------------------ kill

    /// `kill` of another process (ADR-0104 item 9). The caller handles its
    /// own pid.
    pub fn kill_other(&mut self, pid: i32, sig: u32) -> Result<(), i32> {
        if sig > signal::MAX {
            return Err(EINVAL);
        }
        let (target, id) = match pid {
            p if p > 0 => (bp::target::PID, p as u32),
            0 => (bp::target::OWN_GROUP, 0),
            -1 => (bp::target::ALL, 0),
            p => (bp::target::GROUP, p.unsigned_abs()),
        };
        let m = bp::Kill {
            target: Below(target),
            id,
            signal: sig,
        };
        self.broker_simple(&m, &[])
    }
}

/// A `posix_spawn` file action (`adddup2`, `addclose`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    Dup2(i32, i32),
    Close(i32),
}

/// What a spawned child inherits (ADR-0103 item 7, ADR-0107): handles for
/// its fd slots, the record's fd list, and the message's handles with the
/// record's namespace entries.
#[derive(Debug)]
pub struct Inherit<H> {
    /// `(slot, handle, made)`: `made` is a new end for the child, moved; an
    /// existing stream end is duplicated.
    pub slots: Vec<(u32, H, bool)>,
    pub fds: Vec<Fd>,
    /// The message's handles: terminal connections and entries' handles.
    pub message: Vec<H>,
    /// `(index of the entry, kind, handle index or NO_HANDLE)`.
    pub entries: Vec<(usize, EntryKind, u8)>,
}

/// The rights a shared connection keeps, by its access.
fn share_rights(access: u32) -> u32 {
    match access & 3 {
        1 => rights::WRITE | rights::CLONE,
        2 => rights::READ | rights::WRITE | rights::CLONE,
        _ => rights::READ | rights::CLONE,
    }
}

/// What `spawn_prepare` gave.
#[derive(Debug, Clone, Copy)]
pub struct Prepared<H> {
    pub pid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub parent_job: H,
    pub child_job: H,
    pub broker_end: H,
}

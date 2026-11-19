//! The process half of the library's logic (ADR-0104 to ADR-0106): the wait
//! points, the broker's client, the children and `waitpid`, `kill`, the
//! terminal's requests, and what each taken request asks. The C boundary
//! (`ffi`) runs the handlers and makes `spawn`, `fork` and `exec`.

#![forbid(unsafe_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_abi::posix::{EntryKind, FD_NONBLOCK, FD_READ, FD_WRITE, Fd, FdKind, NO_HANDLE};
use zoraya_proto_file::{self as fp, rights};
use zoraya_protocol::Flags;

use zoraya_abi::posix::signal::{self, SIGCHLD, SIGTTIN};
use zoraya_abi::{
    Disposition, ExitReason, ExitRecord, RequestKind, Status, job_signals, process_signals,
};
use zoraya_proto_posix as bp;
use zoraya_proto_tty as tp;
use zoraya_protocol::{Below, ERROR_ORDINAL, ErrorReply, Header, Message, decode, encode};

use crate::errno::*;
use crate::process::{Desc, Posix};
use crate::signals::{DefaultAction, Taken, bit};
use crate::sys::{Sys, User, signals};
use crate::wait;

/// A long wait that a method asked the boundary to make (ADR-0302 item 9).
pub struct PendingWait<H> {
    pub sources: Vec<(H, u32)>,
    /// How many of `sources` are the method's own.
    pub own_len: usize,
    pub deadline: Option<u64>,
    /// How much of a write was done, for the second run.
    pub progress: usize,
}

/// The outcome of a [`PendingWait`], for the method's second run.
pub struct Resume<H> {
    pub sources: Vec<(H, u32)>,
    pub own_len: usize,
    pub result: Result<(usize, u32), Status>,
    pub progress: usize,
}

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

/// What a look at a terminal's input found (`Posix::tty_input_ready`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TtyLook {
    /// A read would not block.
    Ready,
    /// The service has gone: a read gives end of file.
    Hup,
    /// Nothing yet; the connection's channel tells when.
    Wait,
}

impl<S: Sys> Posix<S> {
    // ------------------------------------------------------------ wait points

    /// The sources every wait point listens to besides its own (item 3): the
    /// process's own handle for a request when a handler is installed, and
    /// the children's jobs while `SIGCHLD` is caught and unblocked.
    pub fn interrupt_sources(&self, out: &mut Vec<(S::H, u32)>) {
        // A thread in `sigwait` listens as a handler would (ADR-0315 item 10).
        if (self.signals.handlers_installed() || self.sigwait_active())
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

    /// Waits for `h` to assert one of `wanted` at a wait point: `EINTR` when a
    /// signal is to be delivered first (ADR-0106 item 3).
    pub fn wait_point(&mut self, h: S::H, wanted: u32) -> Result<u32, i32> {
        self.wait_for(&[(h, wanted)], None)
            .map(|(_, s)| s.unwrap_or(0))
    }

    /// Waits for one of `own` at a wait point, or the deadline (`None` for
    /// the index then). `EINTR` when a signal came first.
    ///
    /// In a process with threads (`defer_waits`) the wait is not made here:
    /// the method returns `Err(PENDING)` with the wait in `self.pending`,
    /// changing nothing it cannot repeat; the boundary releases the library
    /// lock, waits, and runs the method again with the outcome in
    /// `self.resume`, which this function then returns (ADR-0302 item 9).
    pub fn wait_for(
        &mut self,
        own: &[(S::H, u32)],
        deadline: Option<u64>,
    ) -> Result<(Option<usize>, Option<u32>), i32> {
        if let Some(r) = self.resume.take() {
            self.stats.waits += 1;
            if r.result == Ok((usize::MAX, 0)) {
                // Another thread interrupted this wait (a signal routed to
                // it, a cancel): the caller looks again (ADR-0304 item 11).
                return Err(EINTR);
            }
            return self.finish_wait(&r.sources, r.own_len, r.result);
        }
        if core::mem::take(&mut self.interrupt_wait) {
            // A handler ran on the way into this call, for a signal that came
            // before its wait: on Unix the signal would have ended that wait,
            // so the call does not block on it (ADR-0106 item 6).
            return Err(EINTR);
        }
        let mut sources = Vec::with_capacity(own.len() + 4);
        sources.extend_from_slice(own);
        self.interrupt_sources(&mut sources);
        if sources.is_empty() && deadline.is_none() {
            // Nothing to wait for and no end: a wait that never ends.
            return Err(EINVAL);
        }
        if self.defer_waits {
            self.pending = Some(PendingWait {
                sources,
                own_len: own.len(),
                deadline,
                progress: 0,
            });
            return Err(PENDING);
        }
        self.stats.waits += 1;
        let r = self.sys.wait_any(&sources, deadline);
        self.finish_wait(&sources, own.len(), r)
    }

    /// One run of `f` for the boundary's retry loop (ADR-0302 item 9): with
    /// the outcome of the wait the last run asked for, and the wait this run
    /// asks for, if `f` gave `Err(PENDING)`.
    pub fn run_deferred<R>(
        &mut self,
        resume: Option<Resume<S::H>>,
        f: impl FnOnce(&mut Self) -> Result<R, i32>,
    ) -> (Result<R, i32>, Option<PendingWait<S::H>>) {
        self.resume = resume;
        let r = f(self);
        let pending = self.end_run(&r);
        (r, pending)
    }

    /// The two halves of [`Posix::run_deferred`], for a caller whose closure
    /// cannot borrow the process: the outcome in, and the wait out.
    pub fn set_resume(&mut self, resume: Option<Resume<S::H>>) {
        self.resume = resume;
    }

    pub fn end_run<R>(&mut self, r: &Result<R, i32>) -> Option<PendingWait<S::H>> {
        self.resume = None;
        let pending = self.pending.take();
        if matches!(r, Err(PENDING)) {
            pending
        } else {
            None
        }
    }

    /// What a finished wait means to the method that asked for it.
    fn finish_wait(
        &mut self,
        sources: &[(S::H, u32)],
        own_len: usize,
        r: Result<(usize, u32), Status>,
    ) -> Result<(Option<usize>, Option<u32>), i32> {
        match r {
            Ok((i, s)) if i < own_len => Ok((Some(i), Some(s))),
            Ok(_) if sources.is_empty() => Ok((None, None)),
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
            let was_pending = self.signals.pending & crate::signals::bit(n) != 0;
            match self.signals.take(n) {
                Taken::Run(a) => return Next::Run(n, a),
                Taken::Default(DefaultAction::Terminate) => return Next::Terminate(n),
                Taken::Default(DefaultAction::Stop) => return Next::Stop(n),
                Taken::Pending => self.route(n, was_pending),
                Taken::Default(_) | Taken::Drop => {}
            }
        }
    }

    /// Takes every request off the kernel's queue into the signal sets, for
    /// a `sigwait` that ends (ADR-0315 item 10): a request left queued would
    /// end the process when the kernel disposition goes back to `Terminate`.
    /// A signal the thread blocks goes where a blocked one goes; any other is
    /// left pending for the next delivery.
    pub fn drain_requests(&mut self) {
        while let Some((kind, detail)) = self.sys.take_request() {
            let n = signal::number(kind, detail);
            if self.signals.blocked & crate::signals::bit(n) != 0 {
                let was_pending = self.signals.pending & crate::signals::bit(n) != 0;
                if self.signals.take(n) == Taken::Pending {
                    self.route(n, was_pending);
                }
            } else {
                self.signals.pending |= crate::signals::bit(n);
            }
        }
    }

    /// `execve`, before the spawn of the new image (ADR-0392 item 2): every
    /// signal whose kernel disposition is now `Terminate`, that ends a process
    /// by default and that the new image does not ignore (`ignored`, the
    /// record's form, bit n), is set to `Deliver`. A job signal that reaches
    /// this image while the kernel makes the new one then waits in this
    /// image's queue for [`Posix::exec_carry`], instead of ending this image
    /// before the new one, made after the walk (ADR-0004 rule 3), got it.
    /// Gives the signals held, in musl's form.
    pub fn exec_hold(&mut self, ignored: u32) -> u64 {
        let mut held = 0;
        for n in 1..=signal::MAX {
            let Some(kind) = signal::kind(n) else {
                continue;
            };
            if signal::default_terminates(n)
                && ignored & (1 << n) == 0
                && self.effective_disposition(n) == Disposition::Terminate
                && self.sys.set_disposition(kind, Disposition::Deliver).is_ok()
            {
                held |= bit(n);
            }
        }
        held
    }

    /// A spawn that failed after [`Posix::exec_hold`]: each held signal gets
    /// its disposition back. One queued meanwhile then ends this image at
    /// once (ADR-0004 rule 2), as it would have without the hold.
    pub fn exec_release(&mut self, held: u64) {
        for n in 1..=signal::MAX {
            if held & bit(n) != 0
                && let Some(kind) = signal::kind(n)
            {
                let _ = self
                    .sys
                    .set_disposition(kind, self.effective_disposition(n));
            }
        }
    }

    /// `execve`, just after the spawn of the new image (ADR-0392 item 1): the
    /// signals sent to this image that it has not acted on, which go on to
    /// the new image (POSIX keeps a pending signal across `execve`), in
    /// musl's form. The new image is a process of its own in this pid's job:
    /// a `job_signal` made before the kernel put it there reached this image
    /// only (ADR-0004 rule 3), and a request this image takes by `Deliver`
    /// (`sh -c` catches `SIGINT`, or [`Posix::exec_hold`] held it) waits in
    /// its queue, which ends with it. Takes the queue only when something
    /// can be in it (a handler, a `sigwait`, a hold: INV-0106-2), and adds the
    /// pending sets, blocked ones included. Keeps only those the new image
    /// ends on: it starts with every handler back to its default, and drops
    /// `ignored` as POSIX drops an ignored pending signal.
    pub fn exec_carry(&mut self, ignored: u32, held: u64) -> u64 {
        let mut set = self.signals.pending | self.signals.shared;
        if held != 0 || self.signals.handlers_installed() || self.sigwait_union() != 0 {
            loop {
                self.stats.takes += 1;
                let Some((kind, detail)) = self.sys.take_request() else {
                    break;
                };
                set |= bit(signal::number(kind, detail));
            }
        }
        let mut kept = 0;
        for n in 1..=signal::MAX {
            if set & bit(n) != 0 && signal::default_terminates(n) && ignored & (1 << n) == 0 {
                kept |= bit(n);
            }
        }
        kept
    }

    /// Whether a library entry polls for requests (ADR-0106 item 5): only
    /// with a handler installed (INV-0106-2), or a pending signal the mask
    /// now lets through.
    pub fn should_poll(&self) -> bool {
        self.signals.handlers_installed()
            || (self.signals.pending | self.signals.shared) & !self.signals.blocked != 0
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
        // A readiness the last look saw is spent by this read.
        self.tty_poll_ready.retain(|h| *h != conn);
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
                // The answer to a `PollReadable` this read overtook: the
                // data it announces is what this read takes. Any other
                // reply is to nothing outstanding: dropped.
                self.tty_polls
                    .retain(|&(c, pt)| !(c == conn && pt == header.transaction));
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

    /// Whether a `read` of the terminal connection `conn` would not block
    /// (ADR-0331 item 5): what the library kept, or the answer to a
    /// `PollReadable` sent to the service, which holds it until the
    /// discipline can answer a read and the connection's line is in front.
    ///
    /// The request is idempotent: the connection has at most one outstanding
    /// (`tty_polls`), so the same look run again after a deferred wait
    /// (ADR-0302 item 9), or by another `poll` call, reads the channel and
    /// does not send another. Any answer is "ready", an error status too: a
    /// read will then say why (`EIO` for a line that is not in front, a stop
    /// for a background group).
    pub(crate) fn tty_input_ready(&mut self, conn: S::H) -> TtyLook {
        if self.tty_rest.iter().any(|(c, _)| *c == conn)
            || self.tty_stash.iter().any(|(c, _)| *c == conn)
        {
            return TtyLook::Ready;
        }
        if let Some(i) = self.tty_poll_ready.iter().position(|c| *c == conn) {
            self.tty_poll_ready.swap_remove(i);
            return TtyLook::Ready;
        }
        let signals_now = self.sys.signals_of(conn).unwrap_or(0);
        if self.tty_reads.iter().any(|(c, _)| *c == conn) {
            // A `Read` is held (another thread's, or an interrupted one):
            // its answer is what a read takes, and arrives on this channel.
            return if signals_now & signals::READABLE != 0 {
                TtyLook::Ready
            } else if signals_now & signals::PEER_CLOSED != 0 {
                TtyLook::Hup
            } else {
                TtyLook::Wait
            };
        }
        // At most two rounds: send, then read what may already be there.
        for _ in 0..3 {
            let Some(&(_, t)) = self.tty_polls.iter().find(|(c, _)| *c == conn) else {
                self.transaction = self.transaction.wrapping_add(1).max(1);
                let t = self.transaction;
                let Ok(n) = encode(&tp::PollReadable {}, t, &mut self.out) else {
                    return TtyLook::Ready;
                };
                if self.sys.channel_write(conn, &self.out[..n], &[]).is_err() {
                    return TtyLook::Hup;
                }
                self.tty_polls.push((conn, t));
                continue;
            };
            for h in self.handles.drain(..) {
                self.sys.close(h);
            }
            match self
                .sys
                .channel_read(conn, &mut self.input, &mut self.handles)
            {
                Ok(n) => {
                    if let Ok((header, _)) = Header::decode(&self.input[..n])
                        && header.transaction == t
                    {
                        self.tty_polls.retain(|(c, _)| *c != conn);
                        return TtyLook::Ready;
                    }
                    // Something else (a reply to nothing outstanding): dropped.
                }
                Err(Status::WouldBlock) => return TtyLook::Wait,
                Err(_) => {
                    self.tty_polls.retain(|(c, _)| *c != conn);
                    return TtyLook::Hup;
                }
            }
        }
        TtyLook::Wait
    }

    // ------------------------------------------------------------ termios

    /// The errno of a terminal reply of `size` bytes: its status first
    /// (`SetAttrReply`, `FlushReply`, `DrainReply` and the generic error all
    /// have it there). `IO`, a connection of a line that is not in front, is
    /// `EIO` (ADR-0331 item 4); a control request on a program's connection
    /// is `EPERM`; anything else the service refuses is `EINVAL`.
    fn tty_reply_errno(&self, size: usize) -> i32 {
        match self.status_of(size) {
            s if s == tp::status::OK => 0,
            s if s == tp::status::IO => EIO,
            s if s == tp::status::NOT_ALLOWED => EPERM,
            _ => EINVAL,
        }
    }

    /// The errno of an `ioctl` a terminal never carries out, once `fd` is
    /// one (`ENOTTY`/`EBADF` first, as for any request).
    pub fn tty_ioctl_refusal(&mut self, fd: i32, request: i64) -> Result<(), i32> {
        self.tty_of(fd)?;
        match crate::termios::ioctl_refusal(request) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// `tcgetattr`: musl's `struct termios` of the terminal at `fd`.
    pub fn tty_get_attr(&mut self, fd: i32) -> Result<[u8; crate::termios::SIZE], i32> {
        let conn = self.tty_of(fd)?;
        let n = self.tty_call(conn, &tp::GetAttr {})?;
        let (_, a) = decode::<tp::Attr<'_>>(&self.input[..n], 0).map_err(|_| EIO)?;
        let mut t = [0u8; crate::termios::SIZE];
        for (i, w) in [a.iflag, a.oflag, a.cflag, a.lflag].iter().enumerate() {
            t[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        let cc = a.cc.0.len().min(32);
        t[17..17 + cc].copy_from_slice(&a.cc.0[..cc]);
        // B38400 in and out.
        t[52..56].copy_from_slice(&15u32.to_le_bytes());
        t[56..60].copy_from_slice(&15u32.to_le_bytes());
        Ok(t)
    }

    /// `tcsetattr` with its three actions (ADR-0331 item 5): `TCSADRAIN`
    /// waits until what was written has been shown (`Drain`), `TCSAFLUSH`
    /// does that and drops the unread input (`Flush`) before the change.
    pub fn tty_set_attr(
        &mut self,
        fd: i32,
        when: i32,
        t: &[u8; crate::termios::SIZE],
    ) -> Result<(), i32> {
        let when = crate::termios::check_action(when)?;
        let conn = self.tty_of(fd)?;
        if when != crate::termios::TCSANOW {
            self.tty_drain_conn(conn)?;
        }
        if when == crate::termios::TCSAFLUSH {
            self.tty_flush_conn(conn)?;
        }
        let word =
            |i: usize| u32::from_le_bytes([t[i * 4], t[i * 4 + 1], t[i * 4 + 2], t[i * 4 + 3]]);
        let m = tp::SetAttr {
            iflag: word(0),
            oflag: word(1),
            cflag: word(2),
            lflag: word(3),
            cc: zoraya_protocol::Data(&t[17..49]),
        };
        let n = self.tty_call(conn, &m)?;
        match self.tty_reply_errno(n) {
            0 => Ok(()),
            e => Err(e),
        }
    }

    fn tty_drain_conn(&mut self, conn: S::H) -> Result<(), i32> {
        let n = self.tty_call(conn, &tp::Drain {})?;
        match self.tty_reply_errno(n) {
            0 => Ok(()),
            e => Err(e),
        }
    }

    fn tty_flush_conn(&mut self, conn: S::H) -> Result<(), i32> {
        // What the library kept from an earlier answer is unread input too.
        self.tty_rest.retain(|(c, _)| *c != conn);
        self.tty_stash.retain(|(c, _)| *c != conn);
        self.tty_poll_ready.retain(|c| *c != conn);
        let n = self.tty_call(conn, &tp::Flush { queue: 0 })?;
        match self.tty_reply_errno(n) {
            0 => Ok(()),
            e => Err(e),
        }
    }

    /// `tcdrain`.
    pub fn tty_drain(&mut self, fd: i32) -> Result<(), i32> {
        let conn = self.tty_of(fd)?;
        self.tty_drain_conn(conn)
    }

    /// `tcflush`: the unread input goes for `TCIFLUSH` and `TCIOFLUSH`;
    /// output is a stream the library has already written into, so
    /// `TCOFLUSH` is a no-op that still checks the descriptor.
    pub fn tty_flush(&mut self, fd: i32, queue: i32) -> Result<(), i32> {
        let input = crate::termios::flushes_input(queue)?;
        let conn = self.tty_of(fd)?;
        if input {
            self.tty_flush_conn(conn)
        } else {
            Ok(())
        }
    }

    /// `open("/dev/tty")` (ADR-0331 item 2, ADR-0105 items 4 and 6): a new
    /// connection cloned from the namespace entry's, tagged with the
    /// caller's own group, and the write end of the terminal's output taken
    /// from a terminal descriptor the process still holds (none: the
    /// descriptor reads but its writes fail with `EBADF`).
    pub(crate) fn open_tty(&mut self, entry: usize, nonblock: bool) -> Result<i32, i32> {
        let Some(base) = self.entries.get(entry).and_then(|e| e.handle) else {
            return Err(ENXIO);
        };
        let output = self
            .fds
            .fds()
            .into_iter()
            .find_map(|fd| match self.fds.get(fd as i32) {
                Some(Desc::Stream {
                    write: Some(w),
                    tty: true,
                    ..
                }) => Some(*w),
                _ => None,
            });
        let write = match output {
            Some(w) => Some(self.sys.duplicate_stream_write(w).map_err(|_| EMFILE)?),
            None => None,
        };
        let group = self.facts.pgid;
        let conn = match self.clone_tty(base, group) {
            Ok(c) => c,
            Err(e) => {
                if let Some(w) = write {
                    self.sys.close(w);
                }
                return Err(e);
            }
        };
        let desc = Desc::Stream {
            read: None,
            write,
            nonblock,
            tty: true,
            conn: Some(conn),
        };
        self.install_desc(desc, false)
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
        self.clone_conn_asking(conn, 0)
    }

    /// `clone_conn` for the line's `/tmp`: asks for `BIND` first (ADR-0466), so that a child
    /// of the line binds in the `/tmp` it shares; a parent without it gives the child none.
    pub fn clone_tmp(&mut self, conn: S::H) -> Result<S::H, i32> {
        self.clone_conn_asking(conn, rights::BIND)
    }

    fn clone_conn_asking(&mut self, conn: S::H, extra: u32) -> Result<S::H, i32> {
        // What the shell gives a `+w` or `create-tree` entry is everything but `publish` (ADR-0153 rule 2): asking
        // for ALL is refused for it, and the child would be left with read alone and could not write what its parent
        // gave it to write (git's `index-pack` in a clone, TASK-31 10h).
        let mut asks = Vec::new();
        if extra != 0 {
            asks.push(rights::ALL | extra);
        }
        asks.extend([
            rights::ALL,
            rights::ALL & !rights::PUBLISH,
            rights::READ | rights::CLONE,
        ]);
        for wanted in asks {
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

    /// A fresh connection to `conn`'s object with reading alone (ADR-0341
    /// item 7): what a child gets of its parent's own directory. The file
    /// service enforces it: the connection holds no write, create, remove or
    /// rename right, and a clone of it cannot have more.
    pub fn clone_read_only(&mut self, conn: S::H) -> Result<S::H, i32> {
        let (mine, theirs) = self.sys.channel_create().map_err(|_| EMFILE)?;
        let m = fp::Clone {
            rights: Flags(rights::READ | rights::CLONE),
        };
        match self.done(conn, &m, &[theirs]) {
            Ok(_) => Ok(mine),
            Err(e) => {
                self.sys.close(mine);
                Err(e)
            }
        }
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
    /// before the exec); with `empty` (ADR-0433) the fds the actions
    /// make alone: a `dup2` takes its source from the parent's table and
    /// nothing else is inherited. An `Open` action is `Opened`: the file
    /// the caller opened for it, at the parent's own fd.
    pub fn child_fds_with(
        &self,
        actions: &[FileAction],
        empty: bool,
    ) -> Result<BTreeMap<u32, u32>, i32> {
        let mut map: BTreeMap<u32, (u32, bool)> = BTreeMap::new();
        if !empty {
            for fd in self.fds.fds() {
                let cloexec = self.fds.cloexec(fd as i32).unwrap_or(true);
                map.insert(fd, (fd, cloexec));
            }
        }
        for a in actions {
            match *a {
                FileAction::Dup2(from, to) => {
                    let (Ok(from), Ok(to)) = (u32::try_from(from), u32::try_from(to)) else {
                        return Err(EBADF);
                    };
                    let source = match map.get(&from) {
                        Some(&(source, _)) => source,
                        // In an empty child the parent's own fd is the source.
                        None if empty && self.fds.get(from as i32).is_some() => from,
                        None => return Err(EBADF),
                    };
                    map.insert(to, (source, false));
                }
                FileAction::Close(fd) => {
                    if let Ok(fd) = u32::try_from(fd) {
                        map.remove(&fd);
                    }
                }
                FileAction::Opened(parent, to) => {
                    let (Ok(parent), Ok(to)) = (u32::try_from(parent), u32::try_from(to)) else {
                        return Err(EBADF);
                    };
                    map.insert(to, (parent, false));
                }
                FileAction::CloseFrom(fd) => {
                    let first = u32::try_from(fd).map_err(|_| EBADF)?;
                    map.retain(|&child, _| child < first);
                }
                // Opened by the caller first (`inherit_shaped`).
                FileAction::Open(..) => return Err(EINVAL),
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
    /// error every handle made for it is closed. With no attribute of plan
    /// part F4 (`Posix::inherit_shaped` takes them).
    pub fn inherit(&mut self, actions: &[FileAction], group: u32) -> Result<Inherit<S::H>, i32> {
        self.inherit_shaped(actions, group, &crate::spawn::Shape::default())
    }

    pub(crate) fn inherit_into(
        &mut self,
        map: &BTreeMap<u32, u32>,
        group: u32,
        shape: &crate::spawn::Shape,
        listed: &[u32],
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
            // A Unix socket or a memory descriptor goes in the third
            // message (ADR-0433); a TCP socket cannot go.
            let unix =
                matches!(self.fds.get(parent as i32), Some(Desc::Socket(s)) if s.unix.is_some());
            // A socket moves only when the spawn's file actions name it
            // (ADR-0412 item 6): an inherited one stays with the parent and
            // is refused as a TCP socket is (`system()` of a program that holds
            // one fails, as it always did). A memory object is copied, so
            // it goes either way.
            if unix && !listed.contains(&child) {
                return Err(EBADF);
            }
            if unix || matches!(self.fds.get(parent as i32), Some(Desc::Memory(_))) {
                self.pass_to_child(child, parent as i32, &mut out.passed)?;
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
        if shape.empty {
            // `inherit_shaped` made them first.
            return Ok(());
        }
        for i in 0..self.entries.len() {
            self.stats.inherit_failed_at = 1000 + i as u32;
            let (kind, handle) = (self.entries[i].kind, self.entries[i].handle);
            let made = match (kind, handle) {
                (EntryKind::Directory, Some(h)) => Some(self.clone_conn(h)?),
                (EntryKind::Tmp, Some(h)) => Some(self.clone_tmp(h)?),
                // ADR-0341 item 7: a child never holds a connection to the
                // program's own directory that can write.
                (EntryKind::Own, Some(h)) => Some(self.clone_read_only(h)?),
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
            out.entries
                .push((self.entries[i].prefix.clone(), kind, index));
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
                // A Unix stream socket's byte streams are duplicated by the
                // snapshot and its ancillary channel stays with the parent
                // (ADR-0412 item 6); a packet socket is one channel, which
                // cannot be duplicated. A TCP socket's three handles do not
                // cross `fork` yet (ADR-0225 item 9; ADR-0287 lists it as not
                // built): refused rather than copied as a description with no
                // handles.
                // A socket bound to a file keeps the connection that makes
                // its listener (proposed ADR-0439): not copied, refused.
                Desc::Socket(ref s)
                    if s.unix.as_ref().is_some_and(|u| u.node.is_none())
                        && (s.kind == crate::unix::SOCK_STREAM || s.ctl.is_none()) => {}
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
                (EntryKind::Directory, Some(h)) => Some((h, self.clone_conn(h)?)),
                (EntryKind::Tmp, Some(h)) => Some((h, self.clone_tmp(h)?)),
                (EntryKind::Own, Some(h)) => Some((h, self.clone_read_only(h)?)),
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
        self.tty_polls.clear();
        self.tty_poll_ready.clear();
        self.signals.pending = 0;
        self.threads_after_fork();
        // The copy names the parent's locks and the parent's connections:
        // a fork child starts with none (ADR-0319 item 2).
        self.locks.clear();
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
        // A window channel is the same: one end, the parent's.
        self.window = None;
        self.addrs = crate::socket::AddrCache::default();
        self.unix_after_fork_in_child();
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

/// A `posix_spawn` file action (`adddup2`, `addclose`, `addopen`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAction {
    Dup2(i32, i32),
    Close(i32),
    /// `addopen(fd, path, flags, mode)`: the parent opens `path` as it would
    /// itself, and the child's `fd` is that file (ADR-0433).
    Open(i32, Vec<u8>, u32, u32),
    /// What `Open` became once the parent opened it: the parent's own `fd`
    /// (never looked up among the child's) is the child's `fd`.
    Opened(i32, i32),
    /// `addclosefrom_np(fd)`: every fd from `fd` up is closed in the child.
    CloseFrom(i32),
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
    /// `(prefix, kind, handle index or NO_HANDLE)`, longest prefix first: the
    /// parent's table, or the one a spawn chose (ADR-0433).
    pub entries: Vec<(String, EntryKind, u8)>,
    /// The child's working directory: an index into `entries` or `NO_ENTRY`,
    /// and the path in it.
    pub cwd: (u32, String),
    /// The Unix sockets and memory descriptors the child gets (the third
    /// bootstrap message, ADR-0433).
    pub passed: Passed<H>,
    /// The fds `addopen` opened in the parent for the child, to close once
    /// the spawn is over.
    pub temps: Vec<i32>,
}

impl<H> Inherit<H> {
    /// Nothing inherited (for a bookkeeping call that needs none).
    pub fn empty() -> Inherit<H> {
        Inherit {
            slots: Vec::new(),
            fds: Vec::new(),
            message: Vec::new(),
            entries: Vec::new(),
            cwd: (zoraya_abi::posix::NO_ENTRY, String::new()),
            passed: Passed::default(),
            temps: Vec::new(),
        }
    }
}

/// The descriptors a child gets that no slot can hold: for each, its fd, its
/// record and its handles, whether a handle is copied (`true`) or moved
/// (`false`) by the message that carries it. Nothing here is the child's
/// until the spawn happened: the parent's table is changed by
/// `Posix::spawned` alone.
#[derive(Debug)]
pub struct Passed<H> {
    pub fds: Vec<u32>,
    pub records: Vec<crate::unix::Record>,
    pub handles: Vec<(H, bool)>,
    /// The memory handles among `handles` that lost `WRITE` to a seal.
    pub read_only: Vec<H>,
    /// The parent's fds the descriptors come from, to refuse one description
    /// given twice.
    pub sources: Vec<i32>,
    /// Sockets that move (all kinds): dead in the parent afterwards.
    pub moved_packets: Vec<i32>,
    /// The parent's handles that stay behind and are closed once the spawn
    /// happened (the ancillary channel of a socket a `fork` shared).
    pub close_after: Vec<H>,
    /// Memory descriptors that go: their size stays (INV-0413-1).
    pub sent_memory: Vec<i32>,
}

impl<H> Default for Passed<H> {
    fn default() -> Self {
        Passed {
            fds: Vec::new(),
            records: Vec::new(),
            handles: Vec::new(),
            read_only: Vec::new(),
            sources: Vec::new(),
            moved_packets: Vec::new(),
            close_after: Vec::new(),
            sent_memory: Vec::new(),
        }
    }
}

impl<H> Passed<H> {
    pub fn is_empty(&self) -> bool {
        self.fds.is_empty()
    }
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

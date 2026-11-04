//! The terminal service's logic (ADR-0105), apart from the machine: the line
//! discipline, the connections with their group tags, the foreground group
//! and the job its signals go to, and the requests of `zoraya-proto-tty`.
//! `ttyd` feeds it the backend's bytes and the connections' messages, and
//! carries out the [`Action`]s it returns: replies, echo, signals, and the
//! suspend message to the broker. It never names a process (ADR-0004 rule
//! 8): it holds job handles the broker gave it, and group pids only as tags.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod discipline;

use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::RequestKind;
use zoraya_proto_tty::{
    Attr, ClearForeground, ClearForegroundReply, Clone, CloneReply, Foreground, ForegroundReply,
    GetAttr, MAX_READ, NCCS, PollReadable, PollReadableReply, Read, ReadReply, Session,
    SessionReply, SetAttr, SetAttrReply, Statistics, StatisticsReply, WindowSize, WindowSizeReply,
    status,
};
use zoraya_protocol::{Below, Data, ErrorReply, Header, MAX_WIRE_MESSAGE, Message, decode, encode};

pub use discipline::{Discipline, Signal, Termios};

/// The most connections the service holds (a quiet shell has one, a
/// pipeline a few, ADR-0103 item 13).
pub const MAX_CONNECTIONS: usize = 256;

/// A connection's index in the service.
pub type Id = usize;

/// What `ttyd` must do.
#[derive(Debug, PartialEq, Eq)]
pub enum Action<H> {
    /// Send these bytes on the connection.
    Reply { to: Id, bytes: Vec<u8> },
    /// Show these bytes (echo, already through output processing).
    Show(Vec<u8>),
    /// `job_signal(job, kind, detail)`.
    Signal {
        job: H,
        kind: RequestKind,
        detail: u32,
    },
    /// Tell the broker that the suspend character was typed (ADR-0105 item
    /// 3); the broker applies each member's disposition.
    Suspend { group: u32, signal: u32 },
    /// A new terminal connection on this server end, already counted as
    /// `id`: watch it.
    Watch { id: Id, end: H },
    /// Close this handle: a job replaced, a connection refused.
    Close(H),
}

#[derive(Debug)]
struct Connection {
    group: u32,
    control: bool,
    /// A held `Read`: its transaction and size.
    read: Option<(u32, u32)>,
    /// A held `PollReadable`.
    poll: Option<u32>,
}

/// Counts for the statistics request and the tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub lines: u32,
    pub interrupts: u32,
    pub suspends: u32,
    pub not_foreground: u32,
    pub refused: u32,
}

/// The terminal.
pub struct Terminal<H> {
    pub discipline: Discipline,
    connections: Vec<Option<Connection>>,
    /// The foreground group and the job its signals go to.
    foreground: Option<(u32, H)>,
    session: Option<H>,
    pub size: (u32, u32),
    pub counts: Counts,
    out: Vec<u8>,
}

impl<H: Copy + Eq + core::fmt::Debug> Default for Terminal<H> {
    fn default() -> Self {
        Terminal::new()
    }
}

impl<H: Copy + Eq + core::fmt::Debug> Terminal<H> {
    pub fn new() -> Terminal<H> {
        Terminal {
            discipline: Discipline::new(),
            connections: Vec::new(),
            foreground: None,
            session: None,
            size: (24, 80),
            counts: Counts::default(),
            out: vec![0; MAX_WIRE_MESSAGE],
        }
    }

    fn add(&mut self, group: u32, control: bool) -> Option<Id> {
        let c = Connection {
            group,
            control,
            read: None,
            poll: None,
        };
        if let Some(i) = self.connections.iter().position(Option::is_none) {
            self.connections[i] = Some(c);
            return Some(i);
        }
        if self.connections.len() >= MAX_CONNECTIONS {
            return None;
        }
        self.connections.push(Some(c));
        Some(self.connections.len() - 1)
    }

    /// The broker's control connection.
    pub fn add_control(&mut self) -> Option<Id> {
        self.add(0, true)
    }

    /// A terminal connection tagged with `group`: the session's first, which
    /// `init` hands on.
    pub fn add_terminal(&mut self, group: u32) -> Option<Id> {
        self.add(group, false)
    }

    /// The connection's peer closed it: a held request goes with it.
    pub fn closed(&mut self, id: Id) {
        if let Some(c) = self.connections.get_mut(id) {
            *c = None;
        }
    }

    pub fn connections(&self) -> usize {
        self.connections.iter().flatten().count()
    }

    pub fn foreground(&self) -> Option<u32> {
        self.foreground.map(|f| f.0)
    }

    /// The group of a connection, for the tests.
    pub fn group_of(&self, id: Id) -> Option<u32> {
        self.connections.get(id)?.as_ref().map(|c| c.group)
    }

    fn encode<'m, M: Message<'m>>(&mut self, m: &M, transaction: u32) -> Vec<u8> {
        match encode(m, transaction, &mut self.out) {
            Ok(n) => self.out[..n].to_vec(),
            // Every reply here fits; an error reply is the fallback.
            Err(_) => self.error(status::INVALID, transaction),
        }
    }

    fn error(&mut self, code: u32, transaction: u32) -> Vec<u8> {
        let m = ErrorReply { status: code };
        match encode(&m, transaction, &mut self.out) {
            Ok(n) => self.out[..n].to_vec(),
            Err(_) => Vec::new(),
        }
    }

    fn reply<'m, M: Message<'m>>(&mut self, to: Id, m: &M, t: u32, actions: &mut Vec<Action<H>>) {
        let bytes = self.encode(m, t);
        actions.push(Action::Reply { to, bytes });
    }

    fn refuse(&mut self, to: Id, code: u32, t: u32, actions: &mut Vec<Action<H>>) {
        self.counts.refused += 1;
        let bytes = self.error(code, t);
        actions.push(Action::Reply { to, bytes });
    }

    /// True if a connection of `group` may read now.
    fn may_read(&self, group: u32) -> bool {
        match self.foreground {
            Some((fg, _)) => fg == group,
            None => true,
        }
    }

    fn connection(&mut self, id: Id) -> Option<&mut Connection> {
        self.connections.get_mut(id)?.as_mut()
    }

    /// A message from connection `id`, with the handles it carried. Never
    /// panics on anything a peer sends.
    pub fn request(&mut self, id: Id, bytes: &[u8], mut handles: Vec<H>) -> Vec<Action<H>> {
        let mut actions = Vec::new();
        let Some(c) = self.connections.get(id).and_then(Option::as_ref) else {
            actions.extend(handles.drain(..).map(Action::Close));
            return actions;
        };
        let control = c.control;
        let group = c.group;
        let Ok((header, _)) = Header::decode(bytes) else {
            // Not even a header: nothing to answer.
            self.counts.refused += 1;
            actions.extend(handles.drain(..).map(Action::Close));
            return actions;
        };
        let t = header.transaction;
        let n = handles.len();
        macro_rules! parse {
            ($m:ty) => {
                match decode::<$m>(bytes, n) {
                    Ok((_, m)) => m,
                    Err(_) => {
                        actions.extend(handles.drain(..).map(Action::Close));
                        self.refuse(id, status::INVALID, t, &mut actions);
                        return actions;
                    }
                }
            };
        }
        match header.ordinal {
            o if o == Read::ORDINAL => {
                let m = parse!(Read);
                if self.connection(id).is_some_and(|c| c.read.is_some()) {
                    self.refuse(id, status::BUSY, t, &mut actions);
                } else if !self.may_read(group) {
                    self.counts.not_foreground += 1;
                    self.refuse(id, status::NOT_FOREGROUND, t, &mut actions);
                } else if let Some(c) = self.connection(id) {
                    c.read = Some((t, m.max.min(MAX_READ as u32)));
                    self.answer_reads(&mut actions);
                }
            }
            o if o == PollReadable::ORDINAL => {
                let _ = parse!(PollReadable);
                if self.connection(id).is_some_and(|c| c.poll.is_some()) {
                    self.refuse(id, status::BUSY, t, &mut actions);
                } else if let Some(c) = self.connection(id) {
                    c.poll = Some(t);
                    self.answer_reads(&mut actions);
                }
            }
            o if o == WindowSize::ORDINAL => {
                let _ = parse!(WindowSize);
                let (rows, columns) = self.size;
                self.reply(id, &WindowSizeReply { rows, columns }, t, &mut actions);
            }
            o if o == GetAttr::ORDINAL => {
                let _ = parse!(GetAttr);
                let a = self.discipline.attr;
                let m = Attr {
                    iflag: a.iflag,
                    oflag: a.oflag,
                    cflag: a.cflag,
                    lflag: a.lflag,
                    cc: Data(&a.cc),
                };
                let bytes = self.encode(&m, t);
                actions.push(Action::Reply { to: id, bytes });
            }
            o if o == SetAttr::ORDINAL => {
                let m = parse!(SetAttr<'_>);
                if m.cc.0.len() != NCCS {
                    self.refuse(id, status::INVALID, t, &mut actions);
                } else {
                    let mut cc = [0; NCCS];
                    cc.copy_from_slice(m.cc.0);
                    self.discipline.set(Termios {
                        iflag: m.iflag,
                        oflag: m.oflag,
                        cflag: m.cflag,
                        lflag: m.lflag,
                        cc,
                    });
                    let ok = SetAttrReply {
                        status: Below(status::OK),
                    };
                    self.reply(id, &ok, t, &mut actions);
                    self.answer_reads(&mut actions);
                }
            }
            o if o == Clone::ORDINAL => {
                let m = parse!(Clone);
                let end = handles[0];
                match self.add_terminal(m.group) {
                    Some(new) => {
                        actions.push(Action::Watch { id: new, end });
                        let ok = CloneReply {
                            status: Below(status::OK),
                        };
                        self.reply(id, &ok, t, &mut actions);
                    }
                    None => {
                        actions.push(Action::Close(end));
                        self.refuse(id, status::NO_SPACE, t, &mut actions);
                    }
                }
            }
            o if o == Foreground::ORDINAL => {
                let m = parse!(Foreground);
                let job = handles[0];
                if !control {
                    actions.push(Action::Close(job));
                    self.refuse(id, status::NOT_ALLOWED, t, &mut actions);
                } else {
                    if let Some((_, old)) = self.foreground.replace((m.group, job)) {
                        actions.push(Action::Close(old));
                    }
                    let ok = ForegroundReply {
                        status: Below(status::OK),
                    };
                    self.reply(id, &ok, t, &mut actions);
                    self.background_reads(&mut actions);
                    self.answer_reads(&mut actions);
                }
            }
            o if o == Session::ORDINAL => {
                let _ = parse!(Session);
                let job = handles[0];
                if !control {
                    actions.push(Action::Close(job));
                    self.refuse(id, status::NOT_ALLOWED, t, &mut actions);
                } else {
                    if let Some(old) = self.session.replace(job) {
                        actions.push(Action::Close(old));
                    }
                    let ok = SessionReply {
                        status: Below(status::OK),
                    };
                    self.reply(id, &ok, t, &mut actions);
                }
            }
            o if o == ClearForeground::ORDINAL => {
                let _ = parse!(ClearForeground);
                if !control {
                    self.refuse(id, status::NOT_ALLOWED, t, &mut actions);
                } else {
                    if let Some((_, old)) = self.foreground.take() {
                        actions.push(Action::Close(old));
                    }
                    let ok = ClearForegroundReply {
                        status: Below(status::OK),
                    };
                    self.reply(id, &ok, t, &mut actions);
                    self.answer_reads(&mut actions);
                }
            }
            o if o == Statistics::ORDINAL => {
                let _ = parse!(Statistics);
                let c = self.counts;
                let m = StatisticsReply {
                    lines: c.lines,
                    interrupts: c.interrupts,
                    suspends: c.suspends,
                    not_foreground: c.not_foreground,
                    refused: c.refused,
                    connections: self.connections() as u32,
                };
                self.reply(id, &m, t, &mut actions);
            }
            _ => {
                actions.extend(handles.drain(..).map(Action::Close));
                self.refuse(id, status::UNSUPPORTED, t, &mut actions);
            }
        }
        actions
    }

    /// Held reads of connections that are no longer in the foreground get
    /// `NotForeground` (ADR-0105 item 8: the reader stops itself and asks
    /// again when continued).
    fn background_reads(&mut self, actions: &mut Vec<Action<H>>) {
        for id in 0..self.connections.len() {
            let Some(c) = self.connections[id].as_ref() else {
                continue;
            };
            if self.may_read(c.group) {
                continue;
            }
            if let Some((t, _)) = self.connections[id].as_mut().and_then(|c| c.read.take()) {
                self.counts.not_foreground += 1;
                let bytes = self.error(status::NOT_FOREGROUND, t);
                actions.push(Action::Reply { to: id, bytes });
            }
        }
    }

    /// Answers held reads and polls that can be answered, in connection
    /// order: each read takes the next line.
    fn answer_reads(&mut self, actions: &mut Vec<Action<H>>) {
        for id in 0..self.connections.len() {
            let Some(c) = self.connections[id].as_ref() else {
                continue;
            };
            if !self.may_read(c.group) || !self.discipline.readable() {
                continue;
            }
            if let Some(t) = self.connections[id].as_mut().and_then(|c| c.poll.take()) {
                let m = PollReadableReply {
                    status: Below(status::OK),
                };
                self.reply(id, &m, t, actions);
            }
            if let Some((t, max)) = self.connections[id].as_mut().and_then(|c| c.read.take())
                && let Some(bytes) = self.discipline.read(max as usize)
            {
                self.counts.lines += 1;
                let m = ReadReply {
                    bytes: Data(&bytes),
                };
                let reply = self.encode(&m, t);
                actions.push(Action::Reply {
                    to: id,
                    bytes: reply,
                });
            }
        }
    }

    /// Bytes from the backend's input.
    pub fn keys(&mut self, bytes: &[u8]) -> Vec<Action<H>> {
        let mut actions = Vec::new();
        let mut echo = Vec::new();
        for &b in bytes {
            if let Some(signal) = self.discipline.input(b, &mut echo) {
                if !echo.is_empty() {
                    actions.push(Action::Show(core::mem::take(&mut echo)));
                }
                self.signal(signal, &mut actions);
            }
        }
        if !echo.is_empty() {
            actions.push(Action::Show(echo));
        }
        self.answer_reads(&mut actions);
        actions
    }

    /// A signal character (ADR-0105 item 3). With no foreground group
    /// nothing is sent; the discipline has flushed the line, as Linux does
    /// for a terminal with no foreground group.
    fn signal(&mut self, signal: Signal, actions: &mut Vec<Action<H>>) {
        let Some((group, job)) = self.foreground else {
            return;
        };
        match signal {
            Signal::Interrupt => {
                self.counts.interrupts += 1;
                actions.push(Action::Signal {
                    job,
                    kind: RequestKind::Interrupt,
                    detail: 2,
                });
            }
            Signal::Quit => {
                self.counts.interrupts += 1;
                actions.push(Action::Signal {
                    job,
                    kind: RequestKind::Quit,
                    detail: 3,
                });
            }
            Signal::Suspend => {
                self.counts.suspends += 1;
                actions.push(Action::Suspend {
                    group,
                    signal: signal.number(),
                });
            }
        }
    }

    /// The window changed size: `SIGWINCH` to the foreground job.
    pub fn resize(&mut self, rows: u32, columns: u32) -> Vec<Action<H>> {
        self.size = (rows, columns);
        match self.foreground {
            Some((_, job)) => vec![Action::Signal {
                job,
                kind: RequestKind::Custom(28),
                detail: 28,
            }],
            None => Vec::new(),
        }
    }

    /// The backend went: `SIGHUP` to the session.
    pub fn hangup(&mut self) -> Vec<Action<H>> {
        match self.session {
            Some(job) => vec![Action::Signal {
                job,
                kind: RequestKind::Hangup,
                detail: 1,
            }],
            None => Vec::new(),
        }
    }

    /// What `ttyd` writes for program output: `ONLCR`.
    pub fn output(&self, bytes: &[u8], out: &mut Vec<u8>) {
        self.discipline.output(bytes, out);
    }
}

/// The status of a terminal reply, for clients: `OK`, or the error reply's.
pub fn reply_status(bytes: &[u8]) -> u32 {
    match Header::decode(bytes) {
        Ok((h, _)) if h.ordinal == zoraya_protocol::ERROR_ORDINAL => {
            decode::<ErrorReply>(bytes, 0).map_or(status::INVALID, |(_, e)| e.status)
        }
        Ok(_) => status::OK,
        Err(_) => status::INVALID,
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

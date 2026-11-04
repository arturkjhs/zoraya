//! A process's POSIX state and the file, descriptor and pipe functions over
//! it (ADR-0103, ADR-0107): the fd table, the namespace table with cwd, the
//! `umask`, and the file protocol client. Every function returns an `errno`
//! on failure; the `ffi` module turns that into C's conventions.

#![forbid(unsafe_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::Status;
use zoraya_abi::posix::{
    Access, Bootstrap, EntryKind, FD_NONBLOCK, Facts, FdKind, MAX_REPORTS, Report,
};
use zoraya_proto_file::{
    self as proto, MAX_INLINE, description_flags, open_flags, rights, status, whence, write_flags,
};
use zoraya_protocol::{
    Below, Data, ERROR_ORDINAL, ErrorReply, Flags, HEADER_SIZE, Header, MAX_WIRE_MESSAGE, Message,
    Name, Path, Text, decode, encode,
};

use crate::errno::{self, Call, *};
use crate::fd::Table;
use crate::path::{self, Form, Links, Place, Refusal, Resolved};
use crate::stat::{self, Kind, Meta};
use crate::sys::{Sys, User, signals};

/// musl's AArch64 `open` flags.
pub mod o {
    pub const ACCMODE: u32 = 0o3 | PATH;
    pub const RDONLY: u32 = 0;
    pub const WRONLY: u32 = 1;
    pub const RDWR: u32 = 2;
    pub const CREAT: u32 = 0o100;
    pub const EXCL: u32 = 0o200;
    pub const NOCTTY: u32 = 0o400;
    pub const TRUNC: u32 = 0o1000;
    pub const APPEND: u32 = 0o2000;
    pub const NONBLOCK: u32 = 0o4000;
    pub const DSYNC: u32 = 0o10000;
    pub const DIRECTORY: u32 = 0o40000;
    pub const NOFOLLOW: u32 = 0o100000;
    pub const LARGEFILE: u32 = 0o400000;
    pub const CLOEXEC: u32 = 0o2000000;
    pub const SYNC: u32 = 0o4010000;
    pub const PATH: u32 = 0o10000000;
    pub const TMPFILE: u32 = 0o20040000;
}

pub const AT_FDCWD: i32 = -100;
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
pub const AT_REMOVEDIR: u32 = 0x200;
pub const AT_EACCESS: u32 = 0x200;
pub const AT_EMPTY_PATH: u32 = 0x1000;

pub const F_DUPFD: i32 = 0;
pub const F_GETFD: i32 = 1;
pub const F_SETFD: i32 = 2;
pub const F_GETFL: i32 = 3;
pub const F_SETFL: i32 = 4;
pub const F_DUPFD_CLOEXEC: i32 = 1030;
pub const FD_CLOEXEC: i32 = 1;

pub const R_OK: u32 = 4;
pub const W_OK: u32 = 2;
pub const X_OK: u32 = 1;

/// `SIGPIPE`, whose default ends a writer to a stream with no reader.
pub const SIGPIPE: u32 = 13;

/// Refused paths remembered so that none is counted twice (ADR-0180 item 6).
pub const REMEMBERED: usize = 256;

/// The largest transfer in one request through the buffer (ADR-0010 rule 2).
pub const MAX_BUFFER: usize = 1024 * 1024;

/// A listing in progress: the cookie is held per description entry, so
/// `dup`ed directory fds share one stream (ADR-0103 item 3).
#[derive(Debug, Default)]
pub struct Listing {
    cookie: u64,
    /// Each name with its kind and its object's id, which is `d_ino` as it
    /// is `st_ino` (ADR-0014 rule 7).
    names: Vec<(String, Kind, u64)>,
    at: usize,
    done: bool,
}

/// A description entry (ADR-0103 item 4).
#[derive(Debug)]
pub enum Desc<H> {
    /// A file or directory connection.
    Object {
        conn: H,
        kind: Kind,
        /// `O_RDONLY`, `O_WRONLY` or `O_RDWR`.
        access: u32,
        nonblock: bool,
        /// `O_SYNC`: every write is `durable` (ADR-0103 item 14).
        sync: bool,
        /// Written through: `close` asks `status` (ADR-0103 item 6).
        written: bool,
        /// The scratch buffer is attached to this connection.
        attached: bool,
        /// Where it was opened, for `openat` relative to it.
        place: Option<Place>,
        listing: Listing,
    },
    /// Stream ends: a pipe, a FIFO, or a terminal. A FIFO keeps its
    /// connection, whose closing tells the service the side has gone; a
    /// terminal's input is its connection (ADR-0105 item 6), its output the
    /// write end.
    Stream {
        read: Option<H>,
        write: Option<H>,
        nonblock: bool,
        tty: bool,
        conn: Option<H>,
    },
    /// `/dev/null`.
    Null { access: u32 },
}

/// A namespace entry (ADR-0107 item 4).
#[derive(Debug)]
pub struct Entry<H> {
    pub prefix: String,
    pub kind: EntryKind,
    pub handle: Option<H>,
}

/// What the library counts, for ADR-0110 item 7 and the task's
/// measurements.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub resolutions: u64,
    pub resolve_ns: u64,
    pub shares: u64,
    pub seeks: u64,
    pub requests: u64,
    pub reports: u64,
    /// Messages to the broker (INV-0104-5: none on the file paths).
    pub broker: u64,
    /// `process_take_request` calls (INV-0106-2).
    pub takes: u64,
}

/// An object opened, and what the open told of it.
struct Opened<H> {
    conn: H,
    kind: Kind,
    created: bool,
    meta: Meta,
}

enum Outcome<H> {
    Opened(Opened<H>),
    Link { remaining: String, target: String },
}

/// The process.
pub struct Posix<S: Sys> {
    pub sys: S,
    pub fds: Table<Desc<S::H>>,
    pub(crate) entries: Vec<Entry<S::H>>,
    pub(crate) form: Form,
    pub(crate) cwd: Place,
    pub(crate) cwd_conn: Option<S::H>,
    pub facts: Facts,
    pub exe: String,
    pub(crate) report: Option<S::H>,
    /// The process itself, with `WAIT`, from the bootstrap channel's second
    /// message (ADR-0107 item 5), for 6d's `REQUEST_PENDING` waits.
    pub self_handle: Option<S::H>,
    reported: Vec<String>,
    report_dropped: u32,
    reports_sent: u32,
    pub(crate) scratch: Option<(S::H, u64)>,
    pub(crate) out: Vec<u8>,
    pub(crate) input: Vec<u8>,
    data: Vec<u8>,
    pub(crate) handles: Vec<S::H>,
    pub(crate) transaction: u32,
    pub stats: Stats,
    /// The signal state (ADR-0106).
    pub signals: crate::signals::Signals,
    /// The broker's connection at slot `BROKER` (ADR-0104), if any.
    pub broker: Option<S::H>,
    /// The process's own job at slot `PROCESS_JOB`, for `exec`.
    pub process_job: Option<S::H>,
    /// The children not yet waited for.
    pub children: Vec<crate::jobs::Child<S::H>>,
    /// `zoraya_fork_group`'s hint for the next `fork`.
    pub fork_group: i32,
    /// A child's end seen at a wait point: `SIGCHLD` to take.
    pub sigchld: bool,
    /// What `Read` a terminal connection has outstanding, by connection.
    pub(crate) tty_reads: Vec<(S::H, u32)>,
}

pub(crate) fn native(s: Status, call: Call) -> i32 {
    errno::from_status(s, call)
}

/// The rights a connection opened with `flags` asks for.
fn rights_for(flags: u32) -> u32 {
    let mut r = rights::CLONE;
    match flags & 3 {
        o::WRONLY => r |= rights::WRITE,
        o::RDWR => r |= rights::READ | rights::WRITE,
        _ => r |= rights::READ,
    }
    if flags & o::DSYNC != 0 {
        r |= rights::SYNC;
    }
    r
}

impl<S: Sys> Posix<S> {
    pub fn new(sys: S, facts: Facts) -> Posix<S> {
        Posix {
            sys,
            fds: Table::new(),
            entries: Vec::new(),
            form: Form::EntryRelative,
            cwd: Place {
                entry: 0,
                path: String::new(),
            },
            cwd_conn: None,
            facts,
            exe: String::new(),
            report: None,
            self_handle: None,
            reported: Vec::new(),
            report_dropped: 0,
            reports_sent: 0,
            scratch: None,
            out: vec![0; MAX_WIRE_MESSAGE],
            input: vec![0; MAX_WIRE_MESSAGE],
            data: vec![0; MAX_INLINE],
            handles: Vec::with_capacity(4),
            transaction: 0,
            stats: Stats::default(),
            signals: crate::signals::Signals::new(facts.ignored),
            broker: None,
            process_job: None,
            children: Vec::new(),
            fork_group: -1,
            sigchld: false,
            tty_reads: Vec::new(),
        }
    }

    /// The state a bootstrap record describes (ADR-0107): `message` holds the
    /// record's handles, and `at_slot` names the handle the spawner put at a
    /// slot. The record was checked by its decoder.
    pub fn from_bootstrap(
        sys: S,
        record: &Bootstrap<'_>,
        message: &[S::H],
        mut at_slot: impl FnMut(u32) -> S::H,
        report: Option<S::H>,
    ) -> Posix<S> {
        let mut p = Posix::new(sys, record.facts);
        p.exe = record.exe.to_string();
        p.form = if record.flags & zoraya_abi::posix::ABSOLUTE != 0 {
            Form::Absolute
        } else {
            Form::EntryRelative
        };
        for e in record.entries() {
            p.entries.push(Entry {
                prefix: e.prefix.to_string(),
                kind: e.kind,
                handle: message.get(e.handle as usize).copied(),
            });
        }
        p.cwd = Place {
            entry: record.cwd_entry as usize,
            path: record.cwd.to_string(),
        };
        for fd in record.fds() {
            let nonblock = fd.flags & FD_NONBLOCK != 0;
            let desc = match fd.kind {
                FdKind::Null => Desc::Null { access: o::RDWR },
                FdKind::File | FdKind::Dir => Desc::Object {
                    conn: at_slot(fd.fd),
                    kind: if fd.kind == FdKind::Dir {
                        Kind::Directory
                    } else {
                        Kind::File
                    },
                    access: o::RDWR,
                    nonblock,
                    sync: false,
                    written: false,
                    attached: false,
                    place: None,
                    listing: Listing::default(),
                },
                FdKind::StreamRead => Desc::Stream {
                    read: Some(at_slot(fd.fd)),
                    write: None,
                    nonblock,
                    tty: false,
                    conn: None,
                },
                FdKind::StreamWrite => Desc::Stream {
                    read: None,
                    write: Some(at_slot(fd.fd)),
                    nonblock,
                    tty: false,
                    conn: None,
                },
                // The output stream's end at the slot, the terminal
                // connection in the message (ADR-0107 item 3).
                FdKind::Tty => Desc::Stream {
                    read: None,
                    write: Some(at_slot(fd.fd)),
                    nonblock,
                    tty: true,
                    conn: message.get(fd.tty as usize).copied(),
                },
            };
            let _ = p.fds.install_at(fd.fd, desc);
        }
        p.report = report;
        p
    }

    /// Adds a namespace entry, for tests and launchers; longest first is the
    /// record's order, but the resolver takes the longest wherever it is.
    pub fn add_entry(&mut self, prefix: &str, kind: EntryKind, handle: Option<S::H>) -> usize {
        self.entries.push(Entry {
            prefix: prefix.to_string(),
            kind,
            handle,
        });
        self.entries.len() - 1
    }

    pub fn set_cwd(&mut self, entry: usize, path: &str, form: Form) {
        self.cwd = Place {
            entry,
            path: path.to_string(),
        };
        self.form = form;
    }

    pub fn set_report(&mut self, end: Option<S::H>) {
        self.report = end;
    }

    pub fn umask(&mut self, mask: u32) -> u32 {
        core::mem::replace(&mut self.facts.umask, mask & 0o777)
    }

    // ------------------------------------------------------------ the client

    /// Sends `m` on `conn`, moving `moved`, and waits for the reply, which is
    /// then `self.input[..size]`, with its handles in `self.handles`.
    pub(crate) fn call<'m, M: Message<'m>>(
        &mut self,
        conn: S::H,
        m: &M,
        moved: &[S::H],
    ) -> Result<usize, i32> {
        self.transaction = self.transaction.wrapping_add(1).max(1);
        let t = self.transaction;
        self.stats.requests += 1;
        let size = match encode(m, t, &mut self.out) {
            Ok(size) => size,
            Err(_) => {
                for &h in moved {
                    self.sys.close(h);
                }
                return Err(EINVAL);
            }
        };
        if self
            .sys
            .channel_write(conn, &self.out[..size], moved)
            .is_err()
        {
            return Err(errno::from_file(errno::GONE));
        }
        for h in self.handles.drain(..) {
            self.sys.close(h);
        }
        loop {
            match self
                .sys
                .channel_read(conn, &mut self.input, &mut self.handles)
            {
                Ok(size) => {
                    let Ok((header, _)) = Header::decode(&self.input[..size]) else {
                        return Err(EIO);
                    };
                    if header.transaction != t {
                        return Err(EIO);
                    }
                    return Ok(size);
                }
                Err(Status::WouldBlock) => {
                    let s = self
                        .sys
                        .wait(conn, signals::READABLE | signals::PEER_CLOSED)
                        .map_err(|_| EIO)?;
                    if s & signals::READABLE == 0 {
                        return Err(errno::from_file(errno::GONE));
                    }
                }
                Err(_) => return Err(errno::from_file(errno::GONE)),
            }
        }
    }

    /// The status of a reply of `size` bytes: every reply has it first, and
    /// the generic error reply has only it.
    pub(crate) fn status_of(&self, size: usize) -> u32 {
        let bytes = &self.input[..size];
        match Header::decode(bytes) {
            Ok((h, _)) if h.ordinal == ERROR_ORDINAL => {
                decode::<ErrorReply>(bytes, 0).map_or(status::INVALID_ARGUMENT, |(_, e)| e.status)
            }
            _ => bytes
                .get(HEADER_SIZE..HEADER_SIZE + 4)
                .map_or(status::INVALID_ARGUMENT, |b| {
                    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
                }),
        }
    }

    /// A request answered by `Done`: its count, or the `errno`.
    pub(crate) fn done<'m, M: Message<'m>>(
        &mut self,
        conn: S::H,
        m: &M,
        moved: &[S::H],
    ) -> Result<u64, i32> {
        let size = self.call(conn, m, moved)?;
        match decode::<proto::Done>(&self.input[..size], 0) {
            Ok((_, d)) if d.status.0 == status::OK => Ok(d.count),
            Ok((_, d)) => Err(errno::from_file(d.status.0)),
            Err(_) => Err(errno::from_file(self.status_of(size))),
        }
    }

    /// A new channel for a request that attaches a connection: our end and
    /// the end to move.
    fn new_channel(&mut self) -> Result<(S::H, S::H), i32> {
        self.sys
            .channel_create()
            .map_err(|s| native(s, Call::Descriptor))
    }

    // ------------------------------------------------------------ resolution

    fn prefixes(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.prefix.as_str()).collect()
    }

    /// Reports a path no entry covers, once, and gives `ENOENT` (ADR-0153
    /// rule 11, INV-0153-7).
    fn refused(&mut self, refusal: Refusal, access: Access) -> i32 {
        let Refusal::Outside(path) = refusal else {
            return refusal.errno();
        };
        if self.reported.contains(&path) {
            return ENOENT;
        }
        let Some(end) = self.report else {
            return ENOENT;
        };
        if self.reports_sent >= MAX_REPORTS {
            // Remembered up to a bound, so a program that names many paths
            // outside its table costs neither memory nor time without end.
            self.report_dropped = self.report_dropped.saturating_add(1);
            self.remember(path);
            return ENOENT;
        }
        let r = Report {
            access,
            dropped: self.report_dropped,
            path: &path,
        };
        let mut bytes = [0u8; Report::HEADER_SIZE + path::PATH_MAX];
        let sent = r
            .encode(&mut bytes)
            .and_then(|size| self.sys.channel_write(end, &bytes[..size], &[]).ok());
        match sent {
            Some(()) => {
                self.reports_sent += 1;
                self.stats.reports += 1;
            }
            // Never waiting: a report that does not fit is dropped, counted.
            None => self.report_dropped = self.report_dropped.saturating_add(1),
        }
        self.remember(path);
        ENOENT
    }

    /// Keeps a refused path, so that it is never counted twice, up to
    /// `REMEMBERED` of them, whatever became of its report.
    fn remember(&mut self, path: String) {
        if self.reported.len() < REMEMBERED {
            self.reported.push(path);
        }
    }

    /// Refused paths remembered, for tests.
    pub fn remembered(&self) -> usize {
        self.reported.len()
    }

    /// Where `dirfd` names paths from.
    fn base(&self, dirfd: i32) -> Result<Place, i32> {
        if dirfd == AT_FDCWD {
            return Ok(self.cwd.clone());
        }
        match self.fds.get(dirfd) {
            Some(Desc::Object {
                kind: Kind::Directory,
                place: Some(place),
                ..
            }) => Ok(place.clone()),
            Some(Desc::Object {
                kind: Kind::Directory,
                ..
            }) => Err(ENOTDIR),
            Some(_) => Err(ENOTDIR),
            None => Err(EBADF),
        }
    }

    /// Resolves `path` from `dirfd`; a path outside the table is reported.
    pub fn resolve(&mut self, dirfd: i32, path: &[u8], access: Access) -> Result<Resolved, i32> {
        let started = self.sys.now();
        let from = if path.first() == Some(&b'/') {
            self.cwd.clone()
        } else {
            self.base(dirfd)?
        };
        let r = path::resolve(&self.prefixes(), self.form, &from, path);
        self.stats.resolutions += 1;
        self.stats.resolve_ns += self.sys.now().wrapping_sub(started);
        r.map_err(|e| self.refused(e, access))
    }

    /// Opens what `r` names in its entry's service. A link in the way comes
    /// back for the resolver.
    fn open_place(
        &mut self,
        r: &Resolved,
        flags: u32,
        wanted: u32,
        description: u32,
        mode: u32,
    ) -> Result<Outcome<S::H>, i32> {
        let entry = self.entries.get(r.place.entry).ok_or(ENOENT)?;
        let Some(dir) = entry.handle.filter(|_| entry.kind.is_directory()) else {
            return Err(ENOENT);
        };
        let (mine, theirs) = self.new_channel()?;
        let mut flags = flags;
        if r.directory {
            flags |= open_flags::DIRECTORY;
        }
        if r.place.path.is_empty() {
            // The entry's root: a clone, then what it is.
            if flags & (open_flags::CREATE | open_flags::EXCLUSIVE)
                == open_flags::CREATE | open_flags::EXCLUSIVE
            {
                self.sys.close(mine);
                self.sys.close(theirs);
                return Err(EEXIST);
            }
            let m = proto::Clone {
                rights: Flags(wanted),
            };
            if let Err(e) = self.done(dir, &m, &[theirs]) {
                self.sys.close(mine);
                return Err(e);
            }
            return match self.stat_conn(mine) {
                Ok((kind, meta)) => Ok(Outcome::Opened(Opened {
                    conn: mine,
                    kind,
                    created: false,
                    meta,
                })),
                Err(e) => {
                    self.sys.close(mine);
                    Err(e)
                }
            };
        }
        let m = proto::OpenWithMode {
            flags: Flags(flags),
            rights: Flags(wanted),
            description: Flags(description),
            mode,
            path: Path(&r.place.path),
        };
        let size = match self.call(dir, &m, &[theirs]) {
            Ok(size) => size,
            Err(e) => {
                self.sys.close(mine);
                return Err(e);
            }
        };
        let reply = match decode::<proto::OpenWithModeReply<'_>>(&self.input[..size], 0) {
            Ok((_, reply)) => reply,
            Err(_) => {
                let s = self.status_of(size);
                self.sys.close(mine);
                return Err(errno::from_file(s));
            }
        };
        match reply.status.0 {
            status::OK => Ok(Outcome::Opened(Opened {
                conn: mine,
                kind: Kind::of_wire(reply.kind.0),
                created: reply.created.0 == 1,
                meta: Meta {
                    volume: reply.volume,
                    id: reply.id,
                    size: reply.size,
                    modified: (reply.modified_seconds as i64, reply.modified_nanos),
                    changed: (reply.changed_seconds as i64, reply.changed_nanos),
                    mode: reply.mode,
                },
            })),
            status::IS_LINK => {
                let link = Outcome::Link {
                    remaining: reply.remaining.0.to_string(),
                    target: reply.target.0.to_string(),
                };
                self.sys.close(mine);
                Ok(link)
            }
            s => {
                self.sys.close(mine);
                Err(errno::from_file(s))
            }
        }
    }

    /// Resolves and opens, following links (ADR-0107 item 7).
    fn open_resolved(
        &mut self,
        mut r: Resolved,
        flags: u32,
        wanted: u32,
        description: u32,
        mode: u32,
        access: Access,
    ) -> Result<(Resolved, Opened<S::H>), i32> {
        let mut links = Links::default();
        loop {
            match self.open_place(&r, flags, wanted, description, mode)? {
                Outcome::Opened(o) => return Ok((r, o)),
                Outcome::Link { remaining, target } => {
                    links.take()?;
                    let prefixes = self.prefixes();
                    let next = path::follow(
                        &prefixes,
                        self.form,
                        &r.place,
                        &remaining,
                        &target,
                        r.directory,
                    );
                    r = next.map_err(|e| self.refused(e, access))?;
                }
            }
        }
    }

    /// The kind and metadata of a connection, by `stat`.
    fn stat_conn(&mut self, conn: S::H) -> Result<(Kind, Meta), i32> {
        let size = self.call(conn, &proto::Stat {}, &[])?;
        match decode::<proto::StatReply>(&self.input[..size], 0) {
            Ok((_, r)) if r.status.0 == status::OK => Ok((
                Kind::of_wire(r.kind.0),
                Meta {
                    volume: r.volume,
                    id: r.id,
                    size: r.size,
                    modified: (r.modified_seconds as i64, r.modified_nanos),
                    changed: (r.changed_seconds as i64, r.changed_nanos),
                    mode: r.mode,
                },
            )),
            Ok((_, r)) => Err(errno::from_file(r.status.0)),
            Err(_) => Err(errno::from_file(self.status_of(size))),
        }
    }

    /// For a call that names something in a directory: a name under a
    /// special entry (`/dev/null/x`) is `ENOTDIR`, as POSIX has it for a
    /// path through a non-directory; the special entry itself is `taken`.
    fn under_special(&self, r: &Resolved, taken: i32) -> Result<(), i32> {
        match self.special(r) {
            Some(_) if !r.place.path.is_empty() => Err(ENOTDIR),
            Some(_) => Err(taken),
            None => Ok(()),
        }
    }

    /// The entry `r` lands in, if it is not a directory: those are answered
    /// here (`/dev/null`, `/proc/self/exe`, `/dev/tty`).
    fn special(&self, r: &Resolved) -> Option<EntryKind> {
        let e = self.entries.get(r.place.entry)?;
        (!e.kind.is_directory()).then_some(e.kind)
    }

    // ------------------------------------------------------------ files

    /// `openat` (ADR-0103, ADR-0014 rule 6).
    pub fn open(&mut self, dirfd: i32, path: &[u8], flags: u32, mode: u32) -> Result<i32, i32> {
        let accmode = flags & o::ACCMODE;
        if accmode == 3
            || (flags & o::CREAT != 0
                && flags & o::DIRECTORY != 0
                && flags & o::TMPFILE != o::TMPFILE)
        {
            return Err(EINVAL);
        }
        let writes = accmode == o::WRONLY || accmode == o::RDWR;
        let access = if writes || flags & o::CREAT != 0 {
            Access::Write
        } else {
            Access::Read
        };
        let r = self.resolve(dirfd, path, access)?;
        if let Some(kind) = self.special(&r) {
            return self.open_special(kind, &r, flags);
        }
        if flags & o::TMPFILE == o::TMPFILE {
            if !writes {
                return Err(EINVAL);
            }
            // The directory's connection must hold what the file's will.
            let wanted = rights_for(accmode) | rights::CREATE | rights::PUBLISH;
            let (_, dir) = self.open_resolved(r, open_flags::DIRECTORY, wanted, 0, 0, access)?;
            let mode = mode & !self.facts.umask & stat::MODE_BITS;
            let result = self.unnamed_on(dir.conn, accmode, mode, flags & o::CLOEXEC != 0);
            self.sys.close(dir.conn);
            return result;
        }
        let mut wire = 0;
        {
            if flags & o::CREAT != 0 {
                wire |= open_flags::CREATE;
            }
            if flags & o::EXCL != 0 && flags & o::CREAT != 0 {
                wire |= open_flags::EXCLUSIVE;
            }
            if flags & o::DIRECTORY != 0 {
                wire |= open_flags::DIRECTORY;
            }
            if flags & o::NOFOLLOW != 0 {
                wire |= open_flags::NO_FOLLOW;
            }
        }
        let description = if flags & o::APPEND != 0 {
            description_flags::APPEND
        } else {
            0
        };
        let create_mode = mode & !self.facts.umask & stat::MODE_BITS;
        let (r, opened) =
            self.open_resolved(r, wire, rights_for(flags), description, create_mode, access)?;
        let conn = opened.conn;
        let fail = |this: &mut Self, e: i32| {
            this.sys.close(conn);
            Err(e)
        };
        if opened.kind == Kind::Link && flags & o::NOFOLLOW != 0 {
            return fail(self, ELOOP);
        }
        if opened.kind == Kind::Directory && writes {
            return fail(self, EISDIR);
        }
        // ADR-0014 rule 6: as for a file's owner who is not root; an object
        // this open made is its creator's whatever its mode.
        let read = accmode != o::WRONLY;
        if !opened.created && !stat::owner_allows(opened.meta.mode, read, writes, false) {
            return fail(self, EACCES);
        }
        // O_TRUNC after the owner check, so a protected file is not cut; an
        // empty or new file needs no request.
        if flags & o::TRUNC != 0
            && writes
            && !opened.created
            && opened.meta.size > 0
            && opened.kind == Kind::File
            && let Err(e) = self.done(conn, &proto::SetSize { size: 0 }, &[])
        {
            return fail(self, e);
        }
        let desc = Desc::Object {
            conn,
            kind: opened.kind,
            access: accmode,
            nonblock: flags & o::NONBLOCK != 0,
            sync: flags & o::DSYNC != 0,
            written: false,
            attached: false,
            place: Some(r.place),
            listing: Listing::default(),
        };
        self.install(desc, flags & o::CLOEXEC != 0)
    }

    fn open_special(&mut self, kind: EntryKind, r: &Resolved, flags: u32) -> Result<i32, i32> {
        if !r.place.path.is_empty() {
            return Err(ENOTDIR);
        }
        match kind {
            EntryKind::Null => {
                if flags & o::DIRECTORY != 0 {
                    return Err(ENOTDIR);
                }
                let desc = Desc::Null {
                    access: flags & o::ACCMODE,
                };
                self.install(desc, flags & o::CLOEXEC != 0)
            }
            // The terminal is 6d's (ADR-0105).
            EntryKind::Terminal => Err(ENXIO),
            // An image is not a file this library can read.
            _ => Err(EACCES),
        }
    }

    fn install(&mut self, desc: Desc<S::H>, cloexec: bool) -> Result<i32, i32> {
        match self.fds.install(0, desc, cloexec) {
            Ok(fd) => Ok(fd as i32),
            Err(desc) => {
                let _ = self.release(desc);
                Err(EMFILE)
            }
        }
    }

    /// Ends a description entry's handles; for a file written through, asks
    /// the service's `status` first (ADR-0103 item 6, ADR-0010).
    fn release(&mut self, desc: Desc<S::H>) -> Result<(), i32> {
        match desc {
            Desc::Object { conn, written, .. } => {
                let r = if written {
                    match self.done(conn, &proto::Status {}, &[]) {
                        Ok(_) => Ok(()),
                        Err(_) => Err(EIO),
                    }
                } else {
                    Ok(())
                };
                self.sys.close(conn);
                r
            }
            Desc::Stream {
                read, write, conn, ..
            } => {
                for h in [read, write, conn].into_iter().flatten() {
                    self.sys.close(h);
                }
                Ok(())
            }
            Desc::Null { .. } => Ok(()),
        }
    }

    pub fn close(&mut self, fd: i32) -> Result<(), i32> {
        match self.fds.close(fd)? {
            Some(desc) => self.release(desc),
            None => Ok(()),
        }
    }

    /// The scratch object for transfers above `MAX_INLINE`, attached to
    /// `conn`; `None` when it cannot be, and the caller goes inline.
    fn attach(&mut self, fd: i32) -> Option<u64> {
        let (conn, attached) = match self.fds.get(fd) {
            Some(Desc::Object { conn, attached, .. }) => (*conn, *attached),
            _ => return None,
        };
        if self.scratch.is_none() {
            let object = self.sys.memory_create(MAX_BUFFER as u64).ok()?;
            match self.sys.memory_map(object, MAX_BUFFER as u64) {
                Ok(address) => self.scratch = Some((object, address)),
                Err(_) => {
                    self.sys.close(object);
                    return None;
                }
            }
        }
        let (object, address) = self.scratch?;
        if !attached {
            let copy = self.sys.duplicate(object).ok()?;
            let m = proto::Attach {
                size: MAX_BUFFER as u64,
            };
            self.done(conn, &m, &[copy]).ok()?;
            if let Some(Desc::Object { attached, .. }) = self.fds.get_mut(fd) {
                *attached = true;
            }
        }
        Some(address)
    }

    pub fn read(&mut self, fd: i32, into: User) -> Result<usize, i32> {
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(Desc::Null { access }) => {
                if *access == o::WRONLY {
                    Err(EBADF)
                } else {
                    Ok(0)
                }
            }
            Some(&Desc::Stream {
                tty: true,
                conn: Some(conn),
                nonblock,
                ..
            }) => self.read_tty(fd, conn, into, nonblock),
            Some(Desc::Stream { read: None, .. }) => Err(EBADF),
            Some(&Desc::Stream {
                read: Some(end),
                nonblock,
                ..
            }) => self.read_stream(end, into, nonblock),
            Some(&Desc::Object { conn, access, .. }) => {
                if access == o::WRONLY {
                    return Err(EBADF);
                }
                if into.length == 0 {
                    return Ok(0);
                }
                if into.length > MAX_INLINE
                    && let Some(address) = self.attach(fd)
                {
                    let length = into.length.min(MAX_BUFFER);
                    let m = proto::ReadBufferCurrent {
                        buffer_offset: 0,
                        length: length as u32,
                    };
                    let got = self.done(conn, &m, &[])? as usize;
                    self.sys.copy(into.part(0, got), User::new(address, got));
                    return Ok(got);
                }
                let m = proto::ReadCurrent {
                    length: into.length.min(MAX_INLINE) as u32,
                };
                let size = self.call(conn, &m, &[])?;
                match decode::<proto::ReadReply<'_>>(&self.input[..size], 0) {
                    Ok((_, r)) if r.status.0 == status::OK => {
                        let got = r.data.0.len().min(into.length);
                        self.sys.copy_out(into.part(0, got), &r.data.0[..got]);
                        Ok(got)
                    }
                    Ok((_, r)) => Err(errno::from_file(r.status.0)),
                    Err(_) => Err(errno::from_file(self.status_of(size))),
                }
            }
        }
    }

    fn read_stream(&mut self, end: S::H, into: User, nonblock: bool) -> Result<usize, i32> {
        loop {
            match self.sys.stream_read(end, into) {
                Ok(n) => return Ok(n),
                Err(Status::WouldBlock) if nonblock => return Err(EAGAIN),
                Err(Status::WouldBlock) => {
                    self.wait_point(end, signals::READABLE | signals::PEER_CLOSED)?;
                }
                Err(s) => return Err(native(s, Call::Other)),
            }
        }
    }

    /// `write`: all of it, or what fit before an error; an error with nothing
    /// written is the error. `EPIPE` from a stream is for the caller to turn
    /// into `SIGPIPE` (ADR-0101).
    pub fn write(&mut self, fd: i32, from: User) -> Result<usize, i32> {
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(Desc::Null { access }) => {
                if *access == o::RDONLY {
                    Err(EBADF)
                } else {
                    Ok(from.length)
                }
            }
            Some(Desc::Stream { write: None, .. }) => Err(EBADF),
            Some(&Desc::Stream {
                write: Some(end),
                nonblock,
                ..
            }) => self.write_stream(end, from, nonblock),
            Some(&Desc::Object {
                conn, access, sync, ..
            }) => {
                if access == o::RDONLY {
                    return Err(EBADF);
                }
                if from.length > isize::MAX as usize {
                    return Err(EINVAL);
                }
                if let Some(Desc::Object { written, .. }) = self.fds.get_mut(fd) {
                    *written = true;
                }
                let flags = if sync { write_flags::DURABLE } else { 0 };
                let mut done = 0;
                while done < from.length {
                    let left = from.length - done;
                    let r = if left > MAX_INLINE
                        && let Some(address) = self.attach(fd)
                    {
                        let n = left.min(MAX_BUFFER);
                        self.sys.copy(User::new(address, n), from.part(done, n));
                        let m = proto::WriteBufferCurrent {
                            buffer_offset: 0,
                            length: n as u32,
                            flags: Flags(flags),
                        };
                        self.done(conn, &m, &[])
                    } else {
                        let n = left.min(MAX_INLINE);
                        let mut data = core::mem::take(&mut self.data);
                        self.sys.copy_in(from.part(done, n), &mut data[..n]);
                        let m = proto::WriteCurrent {
                            flags: Flags(flags),
                            data: Data(&data[..n]),
                        };
                        let r = self.done(conn, &m, &[]);
                        self.data = data;
                        r
                    };
                    match r {
                        Ok(0) => break,
                        Ok(n) => done += n as usize,
                        Err(e) if done == 0 => return Err(e),
                        Err(_) => break,
                    }
                }
                Ok(done)
            }
        }
    }

    fn write_stream(&mut self, end: S::H, from: User, nonblock: bool) -> Result<usize, i32> {
        let mut done = 0;
        while done < from.length {
            match self
                .sys
                .stream_write(end, from.part(done, from.length - done))
            {
                Ok(n) => done += n,
                Err(Status::WouldBlock) if nonblock => {
                    return if done > 0 { Ok(done) } else { Err(EAGAIN) };
                }
                Err(Status::WouldBlock) => {
                    let s = match self.wait_point(end, signals::WRITABLE | signals::PEER_CLOSED) {
                        Ok(s) => s,
                        // What was written counts, as for any later error.
                        Err(_) if done > 0 => return Ok(done),
                        Err(e) => return Err(e),
                    };
                    if s & signals::WRITABLE == 0 && s & signals::PEER_CLOSED != 0 {
                        return if done > 0 { Ok(done) } else { Err(EPIPE) };
                    }
                }
                // What was written counts; the next write meets EPIPE.
                Err(Status::PeerClosed) if done > 0 => break,
                Err(Status::PeerClosed) => return Err(EPIPE),
                Err(s) if done == 0 => return Err(native(s, Call::Other)),
                Err(_) => break,
            }
        }
        Ok(done)
    }

    /// `lseek` (ADR-0103 item 2): a round trip to the service's description.
    pub fn lseek(&mut self, fd: i32, offset: i64, from: i32) -> Result<i64, i32> {
        let origin = match from {
            0 => whence::START,
            1 => whence::CURRENT,
            2 => whence::END,
            _ => return Err(EINVAL),
        };
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(Desc::Stream { .. }) => Err(ESPIPE),
            Some(Desc::Null { .. }) => Ok(0),
            Some(&Desc::Object { conn, .. }) => {
                self.stats.seeks += 1;
                let m = proto::Seek {
                    whence: Below(origin),
                    offset: offset as u64,
                };
                Ok(self.done(conn, &m, &[])? as i64)
            }
        }
    }

    pub fn ftruncate(&mut self, fd: i32, length: i64) -> Result<(), i32> {
        let size = u64::try_from(length).map_err(|_| EINVAL)?;
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(&Desc::Object { conn, access, .. }) if access != o::RDONLY => {
                self.done(conn, &proto::SetSize { size }, &[]).map(|_| ())
            }
            Some(_) => Err(EINVAL),
        }
    }

    /// `fstat` (ADR-0103 item 10).
    pub fn fstat(&mut self, fd: i32) -> Result<[u8; stat::SIZE], i32> {
        let (uid, gid) = (self.facts.uid, self.facts.gid);
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(Desc::Stream { tty, .. }) => {
                let kind = if *tty { Kind::Character } else { Kind::Fifo };
                Ok(stat::encode(
                    kind,
                    &Meta {
                        mode: 0o600,
                        ..Meta::default()
                    },
                    uid,
                    gid,
                ))
            }
            Some(Desc::Null { .. }) => {
                let meta = Meta {
                    mode: 0o666,
                    ..Meta::default()
                };
                Ok(stat::encode(Kind::Character, &meta, uid, gid))
            }
            Some(&Desc::Object { conn, .. }) => {
                let (kind, meta) = self.stat_conn(conn)?;
                Ok(stat::encode(kind, &meta, uid, gid))
            }
        }
    }

    /// Opens what `path` names for a look at it: no rights, links followed
    /// unless `no_follow`.
    fn look(
        &mut self,
        dirfd: i32,
        path: &[u8],
        no_follow: bool,
        wanted: u32,
    ) -> Result<Look<S::H>, i32> {
        let r = self.resolve(dirfd, path, Access::Read)?;
        if let Some(kind) = self.special(&r) {
            if !r.place.path.is_empty() {
                return Err(ENOTDIR);
            }
            return Ok(Look::Special(kind));
        }
        let flags = if no_follow { open_flags::NO_FOLLOW } else { 0 };
        let (_, o) = self.open_resolved(r, flags, wanted, 0, 0, Access::Read)?;
        Ok(Look::Object(o))
    }

    /// `fstatat` (`stat`, `lstat`).
    pub fn stat_at(
        &mut self,
        dirfd: i32,
        path: &[u8],
        flags: u32,
    ) -> Result<[u8; stat::SIZE], i32> {
        if path.is_empty() && flags & AT_EMPTY_PATH != 0 {
            return self.fstat(dirfd);
        }
        let (uid, gid) = (self.facts.uid, self.facts.gid);
        match self.look(dirfd, path, flags & AT_SYMLINK_NOFOLLOW != 0, 0)? {
            Look::Special(EntryKind::Null) | Look::Special(EntryKind::Terminal) => {
                let meta = Meta {
                    mode: 0o666,
                    ..Meta::default()
                };
                Ok(stat::encode(Kind::Character, &meta, uid, gid))
            }
            Look::Special(_) => {
                let meta = Meta {
                    mode: 0o555,
                    ..Meta::default()
                };
                Ok(stat::encode(Kind::File, &meta, uid, gid))
            }
            Look::Object(o) => {
                self.sys.close(o.conn);
                Ok(stat::encode(o.kind, &o.meta, uid, gid))
            }
        }
    }

    /// `faccessat` (`access`, `eaccess`): existence and the owner bits
    /// (ADR-0103 item 11, ADR-0014 rule 6).
    pub fn access_at(
        &mut self,
        dirfd: i32,
        path: &[u8],
        amode: u32,
        flags: u32,
    ) -> Result<(), i32> {
        if amode & !(R_OK | W_OK | X_OK) != 0 || flags & !(AT_EACCESS | AT_SYMLINK_NOFOLLOW) != 0 {
            return Err(EINVAL);
        }
        let mode = match self.look(dirfd, path, flags & AT_SYMLINK_NOFOLLOW != 0, 0)? {
            Look::Special(EntryKind::Null) => 0o666,
            Look::Special(_) => 0o555,
            Look::Object(o) => {
                self.sys.close(o.conn);
                o.meta.mode
            }
        };
        if stat::owner_allows(
            mode,
            amode & R_OK != 0,
            amode & W_OK != 0,
            amode & X_OK != 0,
        ) {
            Ok(())
        } else {
            Err(EACCES)
        }
    }

    /// A connection to the directory that holds `r`'s last name, with
    /// `wanted`, and the name. The connection is the entry's own (`false`)
    /// or a new one to close (`true`).
    fn parent(&mut self, r: &Resolved, wanted: u32) -> Result<(S::H, bool, String, usize), i32> {
        let Some((parent, name)) = path::split(&r.place) else {
            return Err(EBUSY);
        };
        let name = name.to_string();
        let entry = r.place.entry;
        if parent.path.is_empty() {
            let e = self.entries.get(entry).ok_or(ENOENT)?;
            return match e.handle.filter(|_| e.kind.is_directory()) {
                Some(h) => Ok((h, false, name, entry)),
                None => Err(ENOTDIR),
            };
        }
        let at = Resolved {
            absolute: path::absolute_of(&self.entries[entry].prefix, &parent.path),
            place: parent,
            directory: true,
            dot: false,
        };
        let (at, o) = self.open_resolved(at, open_flags::DIRECTORY, wanted, 0, 0, Access::Write)?;
        Ok((o.conn, true, name, at.place.entry))
    }

    fn done_parent(&mut self, conn: S::H, temporary: bool) {
        if temporary {
            self.sys.close(conn);
        }
    }

    pub fn mkdir_at(&mut self, dirfd: i32, path: &[u8], mode: u32) -> Result<(), i32> {
        let r = self.resolve(dirfd, path, Access::Write)?;
        self.under_special(&r, EEXIST)?;
        if r.place.path.is_empty() || r.dot {
            return Err(EEXIST);
        }
        let (conn, temporary, name, _) = self.parent(&r, rights::CREATE)?;
        let m = proto::MakeDirectory {
            mode: mode & !self.facts.umask & stat::MODE_BITS,
            name: Name(&name),
        };
        let result = self.done(conn, &m, &[]).map(|_| ());
        self.done_parent(conn, temporary);
        result
    }

    /// `unlinkat`: a name that is not a directory, or with `AT_REMOVEDIR` an
    /// empty directory (`rmdir`).
    pub fn unlink_at(&mut self, dirfd: i32, path: &[u8], flags: u32) -> Result<(), i32> {
        if flags & !AT_REMOVEDIR != 0 {
            return Err(EINVAL);
        }
        let r = self.resolve(dirfd, path, Access::Write)?;
        if r.dot {
            return Err(EINVAL);
        }
        self.under_special(&r, EBUSY)?;
        if r.place.path.is_empty() {
            return Err(EBUSY);
        }
        let kind = match self.open_place(&r, open_flags::NO_FOLLOW, 0, 0, 0)? {
            Outcome::Opened(o) => {
                self.sys.close(o.conn);
                o.kind
            }
            // A link on the way to the name: resolve through it.
            Outcome::Link { .. } => {
                let (_, o) =
                    self.open_resolved(r.clone(), open_flags::NO_FOLLOW, 0, 0, 0, Access::Write)?;
                self.sys.close(o.conn);
                o.kind
            }
        };
        match (kind, flags & AT_REMOVEDIR != 0) {
            (Kind::Directory, false) => return Err(EISDIR),
            (Kind::Directory, true) => {}
            (_, true) => return Err(ENOTDIR),
            (_, false) => {}
        }
        let (conn, temporary, name, _) = self.parent(&r, rights::REMOVE)?;
        let result = self
            .done(conn, &proto::Remove { name: Name(&name) }, &[])
            .map(|_| ());
        self.done_parent(conn, temporary);
        result
    }

    /// `remove`: a file, or an empty directory.
    pub fn remove(&mut self, path: &[u8]) -> Result<(), i32> {
        match self.unlink_at(AT_FDCWD, path, 0) {
            Err(EISDIR) => self.unlink_at(AT_FDCWD, path, AT_REMOVEDIR),
            r => r,
        }
    }

    /// `rename`, within one entry (one service); across entries `EXDEV`.
    pub fn rename(&mut self, old: &[u8], new: &[u8]) -> Result<(), i32> {
        let from = self.resolve(AT_FDCWD, old, Access::Write)?;
        let to = self.resolve(AT_FDCWD, new, Access::Write)?;
        if self.special(&from).is_some() || self.special(&to).is_some() {
            return Err(EXDEV);
        }
        if from.dot || to.dot {
            return Err(EINVAL);
        }
        if from.place.path.is_empty() || to.place.path.is_empty() {
            return Err(EBUSY);
        }
        let (a, a_temp, old_name, a_entry) = self.parent(&from, rights::RENAME)?;
        let (b, b_temp, new_name, b_entry) = match self.parent(&to, rights::RENAME) {
            Ok(p) => p,
            Err(e) => {
                self.done_parent(a, a_temp);
                return Err(e);
            }
        };
        let result = if a_entry != b_entry {
            Err(EXDEV)
        } else {
            self.rename_with_token(a, b, &old_name, &new_name)
        };
        self.done_parent(a, a_temp);
        self.done_parent(b, b_temp);
        result
    }

    fn rename_with_token(&mut self, a: S::H, b: S::H, old: &str, new: &str) -> Result<(), i32> {
        let size = self.call(b, &proto::GetToken {}, &[])?;
        let token = match decode::<proto::TokenReply>(&self.input[..size], self.handles.len()) {
            Ok((_, r)) if r.status.0 == status::OK => self.handles.pop().ok_or(EIO)?,
            _ => return Err(errno::from_file(self.status_of(size))),
        };
        let m = proto::Rename {
            mode: Below(proto::rename_mode::REPLACE),
            old: Name(old),
            new: Name(new),
        };
        self.done(a, &m, &[token]).map(|_| ())
    }

    pub fn symlink_at(&mut self, target: &[u8], dirfd: i32, path: &[u8]) -> Result<(), i32> {
        let target = path::check(target)?.to_string();
        let r = self.resolve(dirfd, path, Access::Write)?;
        self.under_special(&r, EEXIST)?;
        if r.place.path.is_empty() || r.dot {
            return Err(EEXIST);
        }
        let (conn, temporary, name, _) = self.parent(&r, rights::CREATE)?;
        let m = proto::MakeLink {
            name: Name(&name),
            target: Text(&target),
        };
        let result = self.done(conn, &m, &[]).map(|_| ());
        self.done_parent(conn, temporary);
        result
    }

    pub fn readlink_at(&mut self, dirfd: i32, path: &[u8]) -> Result<String, i32> {
        match self.look(dirfd, path, true, rights::READ)? {
            Look::Special(EntryKind::Image) => Ok(self.exe.clone()),
            Look::Special(_) => Err(EINVAL),
            Look::Object(o) => {
                let result = if o.kind != Kind::Link {
                    Err(EINVAL)
                } else {
                    self.call(o.conn, &proto::ReadLink {}, &[])
                        .and_then(|size| {
                            match decode::<proto::ReadLinkReply<'_>>(&self.input[..size], 0) {
                                Ok((_, r)) if r.status.0 == status::OK => {
                                    Ok(r.target.0.to_string())
                                }
                                Ok((_, r)) => Err(errno::from_file(r.status.0)),
                                Err(_) => Err(errno::from_file(self.status_of(size))),
                            }
                        })
                };
                self.sys.close(o.conn);
                result
            }
        }
    }

    fn set_attributes(
        &mut self,
        conn: S::H,
        which: u32,
        mode: u32,
        mtime: (i64, u32),
    ) -> Result<(), i32> {
        let seconds = u64::try_from(mtime.0).map_err(|_| EINVAL)?;
        let m = proto::SetAttributes {
            which: Flags(which),
            mode: mode & stat::MODE_BITS,
            seconds,
            nanos: mtime.1,
        };
        self.done(conn, &m, &[]).map(|_| ())
    }

    /// Changes something of what `path` names through a connection with
    /// `WRITE`.
    fn change_at(
        &mut self,
        dirfd: i32,
        path: &[u8],
        no_follow: bool,
        which: u32,
        mode: u32,
        mtime: (i64, u32),
    ) -> Result<(), i32> {
        let r = self.resolve(dirfd, path, Access::Write)?;
        if self.special(&r).is_some() {
            return Err(EPERM);
        }
        let flags = if no_follow { open_flags::NO_FOLLOW } else { 0 };
        let (_, o) = self.open_resolved(r, flags, rights::WRITE, 0, 0, Access::Write)?;
        let result = self.set_attributes(o.conn, which, mode, mtime);
        self.sys.close(o.conn);
        result
    }

    pub fn fchmod(&mut self, fd: i32, mode: u32) -> Result<(), i32> {
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(Desc::Object { conn, place, .. }) => {
                let (conn, place) = (*conn, place.clone());
                match self.set_attributes(conn, 1, mode, (0, 0)) {
                    // A connection without `WRITE` (an fd opened to read):
                    // the owner may still change the mode, by its place.
                    Err(EACCES) => match place {
                        Some(place) => {
                            let absolute =
                                path::absolute_of(&self.entries[place.entry].prefix, &place.path);
                            let r = Resolved {
                                place,
                                absolute,
                                directory: false,
                                dot: false,
                            };
                            // Only if the name still holds this fd's object: after a
                            // rename or a replace it holds another (ADR-0180).
                            let (_, mine) = self.stat_conn(conn)?;
                            let (_, o) =
                                self.open_resolved(r, 0, rights::WRITE, 0, 0, Access::Write)?;
                            let same = (o.meta.volume, o.meta.id) == (mine.volume, mine.id);
                            let result = if same {
                                self.set_attributes(o.conn, 1, mode, (0, 0))
                            } else {
                                Err(EACCES)
                            };
                            self.sys.close(o.conn);
                            result
                        }
                        None => Err(EACCES),
                    },
                    r => r,
                }
            }
            Some(_) => Err(EINVAL),
        }
    }

    pub fn fchmod_at(&mut self, dirfd: i32, path: &[u8], mode: u32, flags: u32) -> Result<(), i32> {
        if flags & !AT_SYMLINK_NOFOLLOW != 0 {
            return Err(EINVAL);
        }
        // Links have no mode of their own to change (Linux: EOPNOTSUPP).
        if flags & AT_SYMLINK_NOFOLLOW != 0 {
            return Err(ENOTSUP);
        }
        self.change_at(dirfd, path, false, 1, mode, (0, 0))
    }

    /// `utimensat` and `futimens`: the modification time only (ADR-0014
    /// rule 4); `None` leaves it, `now` is the clock files are stamped with.
    pub fn set_mtime(
        &mut self,
        target: Target<'_>,
        mtime: Option<(i64, u32)>,
        flags: u32,
    ) -> Result<(), i32> {
        let Some(mtime) = mtime else {
            return match target {
                Target::Fd(fd) if self.fds.get(fd).is_none() => Err(EBADF),
                _ => Ok(()),
            };
        };
        if mtime.1 >= 1_000_000_000 {
            return Err(EINVAL);
        }
        match target {
            Target::Fd(fd) => match self.fds.get(fd) {
                None => Err(EBADF),
                Some(&Desc::Object { conn, .. }) => self.set_attributes(conn, 2, 0, mtime),
                Some(_) => Ok(()),
            },
            Target::Path(dirfd, path) => {
                self.change_at(dirfd, path, flags & AT_SYMLINK_NOFOLLOW != 0, 2, 0, mtime)
            }
        }
    }

    // ------------------------------------------------------------ directories

    pub fn chdir(&mut self, path: &[u8]) -> Result<(), i32> {
        let r = self.resolve(AT_FDCWD, path, Access::Read)?;
        if self.special(&r).is_some() {
            return Err(ENOTDIR);
        }
        let (r, o) = self.open_resolved(
            r,
            open_flags::DIRECTORY,
            rights::READ | rights::CLONE,
            0,
            0,
            Access::Read,
        )?;
        if o.kind != Kind::Directory {
            self.sys.close(o.conn);
            return Err(ENOTDIR);
        }
        if !stat::owner_allows(o.meta.mode, false, false, true) {
            self.sys.close(o.conn);
            return Err(EACCES);
        }
        if let Some(old) = self.cwd_conn.replace(o.conn) {
            self.sys.close(old);
        }
        self.cwd = r.place;
        Ok(())
    }

    /// cwd as a path: its entry's prefix and its path (ADR-0107 item 8).
    pub fn getcwd(&self) -> String {
        let prefix = self
            .entries
            .get(self.cwd.entry)
            .map_or("/", |e| e.prefix.as_str());
        path::absolute_of(prefix, &self.cwd.path)
    }

    /// The next name of a directory fd's listing: name, `d_type` and the
    /// object's id, which is `d_ino`.
    pub fn read_dir(&mut self, fd: i32) -> Result<Option<(String, u8, u64)>, i32> {
        loop {
            let (conn, cookie) = match self.fds.get_mut(fd) {
                None => return Err(EBADF),
                Some(Desc::Object {
                    kind: Kind::Directory,
                    conn,
                    listing,
                    ..
                }) => {
                    if listing.at < listing.names.len() {
                        let (name, kind, id) = core::mem::take(&mut listing.names[listing.at]);
                        listing.at += 1;
                        return Ok(Some((name, kind.dirent_type(), id)));
                    }
                    if listing.done {
                        return Ok(None);
                    }
                    (*conn, listing.cookie)
                }
                Some(_) => return Err(ENOTDIR),
            };
            let size = self.call(conn, &proto::ReadDirectoryIds { cookie }, &[])?;
            let (names, next) =
                match decode::<proto::ReadDirectoryIdsReply<'_>>(&self.input[..size], 0) {
                    Ok((_, r)) if r.status.0 == status::OK => {
                        let mut next = 0;
                        let names: Vec<(String, Kind, u64)> = r
                            .entries
                            .iter()
                            .map(|e| {
                                next = e.next;
                                (e.name.0.to_string(), Kind::of_wire(e.kind.0), e.id)
                            })
                            .collect();
                        (names, next)
                    }
                    Ok((_, r)) => return Err(errno::from_file(r.status.0)),
                    Err(_) => return Err(errno::from_file(self.status_of(size))),
                };
            if let Some(Desc::Object { listing, .. }) = self.fds.get_mut(fd) {
                listing.done = next == 0 || names.is_empty();
                listing.cookie = next;
                listing.names = names;
                listing.at = 0;
            }
        }
    }

    /// `fdopendir`'s check: an fd of a directory.
    pub fn is_directory(&self, fd: i32) -> Result<(), i32> {
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(Desc::Object {
                kind: Kind::Directory,
                ..
            }) => Ok(()),
            Some(_) => Err(ENOTDIR),
        }
    }

    // ------------------------------------------------------------ descriptors

    pub fn dup(&mut self, fd: i32) -> Result<i32, i32> {
        self.fds.dup(fd, 0, false).map(|fd| fd as i32)
    }

    /// `dup2`, and `dup3` with `O_CLOEXEC` (`dup3` refuses equal fds).
    pub fn dup3(&mut self, old: i32, new: i32, flags: u32, is_dup3: bool) -> Result<i32, i32> {
        if is_dup3 && (old == new || flags & !o::CLOEXEC != 0) {
            return Err(EINVAL);
        }
        if let Some(desc) = self.fds.dup2(old, new, flags & o::CLOEXEC != 0)? {
            // dup2 closes silently: an error of the old one is lost.
            let _ = self.release(desc);
        }
        Ok(new)
    }

    pub fn fcntl(&mut self, fd: i32, command: i32, argument: i64) -> Result<i32, i32> {
        match command {
            F_DUPFD | F_DUPFD_CLOEXEC => {
                let min = i32::try_from(argument).map_err(|_| EINVAL)?;
                self.fds
                    .dup(fd, min, command == F_DUPFD_CLOEXEC)
                    .map(|fd| fd as i32)
            }
            F_GETFD => Ok(if self.fds.cloexec(fd)? { FD_CLOEXEC } else { 0 }),
            F_SETFD => self
                .fds
                .set_cloexec(fd, argument & i64::from(FD_CLOEXEC) != 0)
                .map(|()| 0),
            F_GETFL => {
                let (access, nonblock, conn) = match self.fds.get(fd) {
                    None => return Err(EBADF),
                    Some(Desc::Null { access }) => (*access, false, None),
                    Some(Desc::Stream { read, nonblock, .. }) => (
                        if read.is_some() { o::RDONLY } else { o::WRONLY },
                        *nonblock,
                        None,
                    ),
                    Some(Desc::Object {
                        conn,
                        access,
                        nonblock,
                        ..
                    }) => (*access, *nonblock, Some(*conn)),
                };
                let mut flags = access;
                if nonblock {
                    flags |= o::NONBLOCK;
                }
                // O_APPEND lives in the service's description (ADR-0103).
                if let Some(conn) = conn
                    && self.done(conn, &proto::GetFlags {}, &[])?
                        & u64::from(description_flags::APPEND)
                        != 0
                {
                    flags |= o::APPEND;
                }
                Ok(flags as i32)
            }
            F_SETFL => {
                let flags = argument as u32;
                let conn = match self.fds.get_mut(fd) {
                    None => return Err(EBADF),
                    Some(Desc::Null { .. }) => None,
                    Some(Desc::Stream { nonblock, .. }) => {
                        *nonblock = flags & o::NONBLOCK != 0;
                        None
                    }
                    Some(Desc::Object { conn, nonblock, .. }) => {
                        *nonblock = flags & o::NONBLOCK != 0;
                        Some(*conn)
                    }
                };
                if let Some(conn) = conn {
                    let append = if flags & o::APPEND != 0 {
                        description_flags::APPEND
                    } else {
                        0
                    };
                    self.done(
                        conn,
                        &proto::SetFlags {
                            flags: Flags(append),
                        },
                        &[],
                    )?;
                }
                Ok(0)
            }
            _ => Err(EINVAL),
        }
    }

    /// `pipe2` (ADR-0101): a stream, read end at the lower fd.
    pub fn pipe(&mut self, flags: u32) -> Result<(i32, i32), i32> {
        if flags & !(o::CLOEXEC | o::NONBLOCK) != 0 {
            return Err(EINVAL);
        }
        let (read, write) = self
            .sys
            .stream_create()
            .map_err(|s| native(s, Call::Descriptor))?;
        let cloexec = flags & o::CLOEXEC != 0;
        let nonblock = flags & o::NONBLOCK != 0;
        let r = self.install(
            Desc::Stream {
                read: Some(read),
                write: None,
                nonblock,
                tty: false,
                conn: None,
            },
            cloexec,
        );
        let r = match r {
            Ok(fd) => fd,
            Err(e) => {
                self.sys.close(write);
                return Err(e);
            }
        };
        let w = self.install(
            Desc::Stream {
                read: None,
                write: Some(write),
                nonblock,
                tty: false,
                conn: None,
            },
            cloexec,
        );
        match w {
            Ok(w) => Ok((r, w)),
            Err(e) => {
                let _ = self.close(r);
                Err(e)
            }
        }
    }

    /// True if `fd` is a file or directory connection: `writev` gathers
    /// its pieces only then, so a stream still sees each piece's pointer.
    pub fn is_object(&self, fd: i32) -> bool {
        matches!(self.fds.get(fd), Some(Desc::Object { .. }))
    }

    pub fn isatty(&self, fd: i32) -> Result<bool, i32> {
        match self.fds.get(fd) {
            None => Err(EBADF),
            Some(Desc::Stream { tty, .. }) => Ok(*tty),
            Some(_) => Ok(false),
        }
    }

    /// `tmpfile` and `memfd_create`: an unnamed file on the temporary
    /// directory service, the `Tmp` entry (ADR-0107 item 17, ADR-0103 item
    /// 12).
    pub fn unnamed(&mut self, cloexec: bool) -> Result<i32, i32> {
        let tmp = self
            .entries
            .iter()
            .find(|e| e.kind == EntryKind::Tmp)
            .and_then(|e| e.handle);
        let Some(dir) = tmp else {
            return Err(ENOENT);
        };
        self.unnamed_on(dir, o::RDWR, 0o600, cloexec)
    }

    /// An unnamed file in the directory of `dir` (ADR-0010 rule 7).
    fn unnamed_on(
        &mut self,
        dir: S::H,
        accmode: u32,
        mode: u32,
        cloexec: bool,
    ) -> Result<i32, i32> {
        let (mine, theirs) = self.new_channel()?;
        let m = proto::OpenWithMode {
            flags: Flags(open_flags::UNNAMED),
            rights: Flags(rights_for(accmode) | rights::PUBLISH),
            description: Flags(0),
            mode,
            path: Path("unnamed"),
        };
        let status = self
            .call(dir, &m, &[theirs])
            .map(|size| self.status_of(size));
        match status {
            Ok(status::OK) => {}
            Ok(s) => {
                self.sys.close(mine);
                return Err(errno::from_file(s));
            }
            Err(e) => {
                self.sys.close(mine);
                return Err(e);
            }
        }
        let desc = Desc::Object {
            conn: mine,
            kind: Kind::File,
            access: accmode,
            nonblock: false,
            sync: false,
            written: false,
            attached: false,
            place: None,
            listing: Listing::default(),
        };
        self.install(desc, cloexec)
    }

    /// `linkat(fd, "", dirfd, path, AT_EMPTY_PATH)` on an unnamed file:
    /// `publish` (ADR-0100 item 6, ADR-0010 rule 7). There are no other
    /// hard links (ADR-0013).
    pub fn publish(&mut self, fd: i32, dirfd: i32, path: &[u8]) -> Result<(), i32> {
        // Only an unnamed file (one with no place) can be given a name.
        let file = match self.fds.get(fd) {
            Some(Desc::Object {
                conn,
                kind: Kind::File,
                place: None,
                ..
            }) => *conn,
            Some(_) => return Err(EPERM),
            None => return Err(EBADF),
        };
        let r = self.resolve(dirfd, path, Access::Write)?;
        self.under_special(&r, EEXIST)?;
        if r.place.path.is_empty() || r.dot {
            return Err(EEXIST);
        }
        let (dir, temporary, name, _) = self.parent(&r, rights::PUBLISH)?;
        let result = self.publish_with_token(file, dir, &name);
        self.done_parent(dir, temporary);
        result
    }

    fn publish_with_token(&mut self, file: S::H, dir: S::H, name: &str) -> Result<(), i32> {
        let size = self.call(dir, &proto::GetToken {}, &[])?;
        let token = match decode::<proto::TokenReply>(&self.input[..size], self.handles.len()) {
            Ok((_, r)) if r.status.0 == status::OK => self.handles.pop().ok_or(EIO)?,
            _ => return Err(errno::from_file(self.status_of(size))),
        };
        let m = proto::Publish {
            mode: Below(proto::publish_mode::NO_REPLACE),
            name: Name(name),
        };
        // A file the service holds as named refuses with INVALID_ARGUMENT:
        // no hard links, so EPERM, as for a named fd above.
        match self.done(file, &m, &[token]) {
            Ok(_) => Ok(()),
            Err(EINVAL) => Err(EPERM),
            Err(e) => Err(e),
        }
    }

    /// `share`: a second connection to `fd`'s description, as a spawn or a
    /// fork would make (ADR-0103 items 7 and 8). The library uses it at 6d;
    /// here it serves the measurement of its round trip.
    pub fn share(&mut self, fd: i32) -> Result<S::H, i32> {
        let conn = match self.fds.get(fd) {
            Some(Desc::Object { conn, .. }) => *conn,
            Some(_) => return Err(EINVAL),
            None => return Err(EBADF),
        };
        let (mine, theirs) = self.new_channel()?;
        self.stats.shares += 1;
        let m = proto::Share {
            rights: Flags(rights::READ | rights::WRITE | rights::CLONE),
        };
        match self.done(conn, &m, &[theirs]) {
            Ok(_) => Ok(mine),
            Err(e) => {
                self.sys.close(mine);
                Err(e)
            }
        }
    }
}

/// True if a reply is not the generic error reply.
pub(crate) fn reply_ok(bytes: &[u8]) -> bool {
    Header::decode(bytes).is_ok_and(|(h, _)| h.ordinal != ERROR_ORDINAL)
}

/// What `look` found.
enum Look<H> {
    Special(EntryKind),
    Object(Opened<H>),
}

/// What `set_mtime` changes.
pub enum Target<'a> {
    Fd(i32),
    Path(i32, &'a [u8]),
}

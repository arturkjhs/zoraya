//! A POSIX program started by a native program through the session's broker
//! (ADR-0153 rule 10, ADR-0104 items 4 and 6): `zosh`'s way. The broker
//! makes the pid and its process job inside a `posix` job of the caller's
//! (`SpawnPrepareIn`, `Group(job)`); the caller builds the bootstrap record
//! (ADR-0107) itself, spawns the image into the process job, commits it and,
//! once the job has ended, reaps the pid.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::posix::{
    self as abi, EXT_NET, EXT_RANDOM, EntryKind, FdKind, NO_HANDLE, SELF_EXT_MESSAGE, SELF_MESSAGE,
    SELF_NET_MESSAGE, slot,
};
use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, Disposition, ExitReason, ExitRecord, Handle,
    ImageObject, JobFlags, JobObject, Mode, ObjectType, Packet, RequestKind, Rights, Status,
    StreamReadObject, StreamWriteObject, channel_signals,
};
use zoraya_proto_posix as bp;
use zoraya_protocol::{Below, Header, MAX_WIRE_MESSAGE, Message, decode, encode};

use crate::{
    channel_create, channel_read, channel_write, handle_close, handle_duplicate, job_create,
    job_kill, port_create, port_subscribe, port_wait, spawn,
};

/// A client connection to the broker, with no pid (ADR-0153's note on
/// ADR-0104).
pub struct Broker {
    pub end: Handle,
    /// The last request's transaction number; a reply is read only if it
    /// carries its request's (review K5).
    pub transaction: core::cell::Cell<u32>,
}

impl Broker {
    /// The connection `end`.
    pub fn new(end: Handle) -> Broker {
        Broker {
            end,
            transaction: core::cell::Cell::new(0),
        }
    }
}

/// What `SpawnPrepareIn` gave.
pub struct Prepared {
    pub pid: u32,
    pub pgid: u32,
    pub sid: u32,
    /// The child's process job, as its parent holds it (`WAIT`).
    pub parent_job: Handle,
    /// The same job, for the spawn and the child's `PROCESS_JOB`.
    pub child_job: Handle,
    /// The child's broker connection.
    pub broker_end: Handle,
}

impl Broker {
    /// One request and its reply, with the reply's handles.
    fn call<'a, M: Message<'a>>(
        &self,
        m: &M,
        handles: &[ChannelEntry],
    ) -> Result<(Vec<u8>, Vec<Handle>), Status> {
        let mut bytes = vec![0u8; MAX_WIRE_MESSAGE];
        let unsent = || {
            for e in handles.iter().filter(|e| e.transfer == Transfer::Move) {
                let _ = handle_close(e.handle);
            }
        };
        let t = self.transaction.get().wrapping_add(1).max(1);
        self.transaction.set(t);
        let Ok(length) = encode(m, t, &mut bytes) else {
            unsent();
            return Err(Status::InvalidArgument);
        };
        // The port before the request, so that no failure after the write
        // leaves its reply to be read as the next one's (review K5).
        let port = match port_create() {
            Ok(port) => port,
            Err(s) => {
                unsent();
                return Err(s);
            }
        };
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        if let Err(s) = port_subscribe(port, self.end, wanted, 1, Mode::Level) {
            let _ = handle_close(port);
            unsent();
            return Err(s);
        }
        if let Err(s) = channel_write(self.end, &bytes[..length], handles) {
            let _ = handle_close(port);
            unsent();
            return Err(s);
        }
        let reply = loop {
            let mut packets = [Packet::default()];
            if let Err(s) = port_wait(port, DEADLINE_INFINITE, &mut packets) {
                break Err(s);
            }
            bytes.resize(MAX_WIRE_MESSAGE, 0);
            let mut got = [self.end; 4];
            let sizes = match channel_read(self.end, &mut bytes, &mut got) {
                Ok(sizes) => sizes,
                Err((s, _)) => break Err(s),
            };
            bytes.truncate(sizes.bytes as usize);
            let got = got[..sizes.handles as usize].to_vec();
            if Header::decode(&bytes).is_ok_and(|(h, _)| h.transaction == t) {
                break Ok((bytes, got));
            }
            // A reply to an earlier request, which gave up: dropped.
            for h in got {
                let _ = handle_close(h);
            }
        };
        let _ = handle_close(port);
        reply
    }

    /// `spawn_prepare(Group(job))`: a pid whose process job the broker makes
    /// inside `job`, which must have the `posix` flag.
    pub fn prepare_in(&self, job: Handle) -> Result<Prepared, Status> {
        let moved = [ChannelEntry {
            handle: job,
            rights: JobObject::RIGHTS,
            transfer: Transfer::Move,
        }];
        let (bytes, handles) = self.call(&bp::SpawnPrepareIn { ignored: 0 }, &moved)?;
        match decode::<bp::SpawnPreparedIn>(&bytes, handles.len()) {
            Ok((_, r)) if handles.len() == 3 => Ok(Prepared {
                pid: r.pid,
                pgid: r.pgid,
                sid: r.sid,
                parent_job: handles[0],
                child_job: handles[1],
                broker_end: handles[2],
            }),
            _ => {
                for h in handles {
                    let _ = handle_close(h);
                }
                Err(Status::BadState)
            }
        }
    }

    /// `spawn_commit`: the process spawned for `pid`.
    pub fn commit(&self, pid: u32, process: Handle) -> Result<(), Status> {
        let me = handle_duplicate(
            process,
            Rights::WAIT.union(Rights::SIGNAL).union(Rights::TRANSFER),
        )?;
        let moved = [ChannelEntry {
            handle: me,
            rights: Rights::WAIT.union(Rights::SIGNAL),
            transfer: Transfer::Move,
        }];
        let (bytes, _) = self.call(&bp::SpawnCommit { pid }, &moved)?;
        match decode::<bp::SpawnCommitReply>(&bytes, 0) {
            Ok((_, r)) if r.status.0 == bp::status::OK => Ok(()),
            _ => Err(Status::BadState),
        }
    }

    /// The spawn failed: the pid goes.
    pub fn abort(&self, pid: u32) {
        let _ = self.call(&bp::SpawnAbort { pid }, &[]);
    }

    /// The exit record of `pid`, whose process job has ended; the pid is
    /// then gone.
    pub fn reap(&self, pid: u32) -> Option<ExitRecord> {
        let (bytes, _) = self.call(&bp::Reap { pid }, &[]).ok()?;
        let (_, r) = decode::<bp::Reaped>(&bytes, 0).ok()?;
        (r.status.0 == bp::status::OK).then(|| ExitRecord {
            code: r.code as i32,
            reason: ExitReason::from_raw(r.reason).unwrap_or(ExitReason::Exited),
            detail: r.detail,
            request_kind: r.kind.checked_sub(1).and_then(RequestKind::from_raw),
        })
    }

    /// `kill(-pgid, signal)`: the broker applies each process's
    /// disposition (ADR-0104 item 9).
    pub fn signal_group(&self, pgid: u32, signal: u32) -> Result<(), Status> {
        let m = bp::Kill {
            target: Below(bp::target::GROUP),
            id: pgid,
            signal,
        };
        let (bytes, _) = self.call(&m, &[])?;
        match decode::<bp::KillReply>(&bytes, 0) {
            Ok((_, r)) if r.status.0 == bp::status::OK => Ok(()),
            _ => Err(Status::BadState),
        }
    }
}

/// One entry of the child's table; a `Directory` or `Tmp` entry moves its
/// connection, an `Image` entry a duplicate of the image.
pub struct Entry {
    pub prefix: String,
    pub kind: EntryKind,
    pub handle: Option<Handle>,
}

/// Everything a launch needs but the broker.
pub struct Launch<'a> {
    /// The line's job: the child's `posix` job is made inside it.
    pub line_job: Handle,
    pub image: Handle,
    /// The program's absolute path (`/proc/self/exe`, `argv[0]`'s source).
    pub exe: &'a str,
    /// The table, longest prefix first; moved with the record.
    pub entries: Vec<Entry>,
    /// The entry that holds the current directory, and the path below it.
    pub cwd_entry: u32,
    pub cwd: &'a str,
    pub args: &'a [&'a [u8]],
    pub env: &'a [&'a [u8]],
    /// fd 0's read end, fd 1's and fd 2's write ends; duplicated.
    pub streams: [Handle; 3],
    /// The refused-path channel's write end (ADR-0153 rule 11); moved.
    pub report: Handle,
    /// A connection to `netd` for the program (ADR-0225 item 2), moved in
    /// the second bootstrap message beside `SELF` (`SELF_NET_MESSAGE`);
    /// none for a program the line gave no network.
    pub net: Option<Handle>,
    /// A connection to the random service for `getrandom` (ADR-0227 item
    /// 3; proposed ADR-0310), moved in the same message; none when the
    /// launcher has no service to share.
    pub random: Option<Handle>,
    pub realtime_base: u64,
}

/// A started POSIX child.
pub struct Child {
    pub pid: u32,
    pub pgid: u32,
    pub process: Handle,
    /// Its process job, with `WAIT`: `SUSPENDED` says whether it stopped.
    pub job: Handle,
}

fn close_entries(entries: &[Entry]) {
    for e in entries {
        if let Some(h) = e.handle {
            let _ = handle_close(h);
        }
    }
}

/// Starts the program: prepare, record, spawn, commit. Every handle of
/// `l` that is moved is gone afterwards, also on failure.
pub fn launch(broker: &Broker, l: Launch<'_>) -> Result<Child, Status> {
    let sub = match job_create(l.line_job, JobFlags::POSIX) {
        Ok(j) => j,
        Err(s) => {
            close_entries(&l.entries);
            let _ = handle_close(l.report);
            if let Some(n) = l.net {
                let _ = handle_close(n);
            }
            if let Some(r) = l.random {
                let _ = handle_close(r);
            }
            return Err(s);
        }
    };
    let p = match broker.prepare_in(sub) {
        Ok(p) => p,
        Err(s) => {
            close_entries(&l.entries);
            let _ = handle_close(l.report);
            if let Some(n) = l.net {
                let _ = handle_close(n);
            }
            if let Some(r) = l.random {
                let _ = handle_close(r);
            }
            return Err(s);
        }
    };
    let fail = |s: Status, p: &Prepared| {
        broker.abort(p.pid);
        let _ = handle_close(p.parent_job);
        s
    };
    // The record.
    let mut moved: Vec<ChannelEntry> = Vec::new();
    let mut record_entries: Vec<abi::Entry<'_>> = Vec::new();
    for e in &l.entries {
        let handle = match e.handle {
            Some(h) => {
                let rights = if e.kind == EntryKind::Image {
                    ImageObject::RIGHTS
                } else {
                    ChannelObject::RIGHTS
                };
                moved.push(ChannelEntry {
                    handle: h,
                    rights,
                    transfer: Transfer::Move,
                });
                (moved.len() - 1) as u8
            }
            None => NO_HANDLE,
        };
        record_entries.push(abi::Entry {
            prefix: &e.prefix,
            kind: e.kind,
            handle,
        });
    }
    let fds = [
        abi::Fd {
            fd: 0,
            kind: FdKind::StreamRead,
            flags: 0,
            tty: NO_HANDLE,
        },
        abi::Fd {
            fd: 1,
            kind: FdKind::StreamWrite,
            flags: 0,
            tty: NO_HANDLE,
        },
        abi::Fd {
            fd: 2,
            kind: FdKind::StreamWrite,
            flags: 0,
            tty: NO_HANDLE,
        },
    ];
    let facts = abi::Facts {
        pid: p.pid,
        ppid: 0,
        pgid: p.pgid,
        sid: p.sid,
        uid: 1000,
        gid: 1000,
        umask: 0o022,
        ignored: 0,
        realtime_base: l.realtime_base,
        blocked: 0,
    };
    let mut record = vec![0u8; MAX_WIRE_MESSAGE];
    let Some(n) = abi::encode(
        &facts,
        abi::ABSOLUTE,
        l.exe,
        l.cwd_entry,
        l.cwd,
        &fds,
        &record_entries,
        moved.len(),
        &mut record,
    ) else {
        close_entries(&l.entries);
        let _ = handle_close(l.report);
        for h in [p.child_job, p.broker_end]
            .into_iter()
            .chain(l.net)
            .chain(l.random)
        {
            let _ = handle_close(h);
        }
        return Err(fail(Status::InvalidArgument, &p));
    };
    let (boot, boot_child) = match channel_create() {
        Ok(b) => b,
        Err(s) => {
            close_entries(&l.entries);
            let _ = handle_close(l.report);
            for h in [p.child_job, p.broker_end]
                .into_iter()
                .chain(l.net)
                .chain(l.random)
            {
                let _ = handle_close(h);
            }
            return Err(fail(s, &p));
        }
    };
    // The spawn, into the process job.
    let mut buffer = vec![0u8; zoraya_abi::spawn::MAX_SPAWN_SIZE];
    let mut e = Encoder::new(&mut buffer, p.child_job, l.image);
    // Every signal its default (ADR-0106 item 1, ADR-0190 item 11).
    for n in 1..=abi::signal::MAX {
        if let Some(kind) = abi::signal::kind(n) {
            let d = if abi::signal::default_terminates(n) {
                Disposition::Terminate
            } else {
                Disposition::Ignore
            };
            e.disposition(kind, d);
        }
    }
    let [input, output, error] = l.streams;
    for (fd, h, rights) in [
        (0, input, StreamReadObject::RIGHTS),
        (1, output, StreamWriteObject::RIGHTS),
        (2, error, StreamWriteObject::RIGHTS),
    ] {
        e.handle(HandleEntry {
            slot: slot::of_fd(fd),
            handle: h,
            rights,
            transfer: Transfer::Duplicate,
        });
    }
    for (at, h, rights) in [
        (slot::BOOTSTRAP.slot(), boot_child, ChannelObject::RIGHTS),
        (slot::BROKER.slot(), p.broker_end, ChannelObject::RIGHTS),
        (slot::REPORT.slot(), l.report, ChannelObject::RIGHTS),
        (slot::PROCESS_JOB.slot(), p.child_job, abi::job_rights::OWN),
    ] {
        e.handle(HandleEntry {
            slot: at,
            handle: h,
            rights,
            transfer: Transfer::Move,
        });
    }
    for a in l.args {
        e.arg(a);
    }
    for v in l.env {
        e.env(v);
    }
    let spawned = match e.finish() {
        Some(length) => spawn(&buffer[..length]),
        None => Err(Status::InvalidArgument),
    };
    let process = match spawned {
        Ok(process) => process,
        Err(s) => {
            close_entries(&l.entries);
            for h in [boot, boot_child, p.broker_end, l.report, p.child_job]
                .into_iter()
                .chain(l.net)
                .chain(l.random)
            {
                let _ = handle_close(h);
            }
            return Err(fail(s, &p));
        }
    };
    // The record with its connections, then the child's own handle.
    // A child that did not get its record, or that the broker did not take,
    // is ended and its pid given back (review K4).
    let gone = |s: Status| {
        let _ = job_kill(p.parent_job);
        let _ = handle_close(process);
        fail(s, &p)
    };
    if let Err(s) = channel_write(boot, &record[..n], &moved) {
        close_entries(&l.entries);
        let _ = handle_close(boot);
        if let Some(n) = l.net {
            let _ = handle_close(n);
        }
        if let Some(r) = l.random {
            let _ = handle_close(r);
        }
        return Err(gone(s));
    }
    if let Ok(me) = handle_duplicate(process, Rights::WAIT.union(Rights::TRANSFER)) {
        let mut entry = vec![ChannelEntry {
            handle: me,
            rights: Rights::WAIT,
            transfer: Transfer::Move,
        }];
        // The plain messages when only the network is given or nothing is;
        // `ZPBX` with flags when the random service's connection is (the
        // launcher's rule: handles in the order SELF, net, random).
        let mut flags = 0u8;
        if let Some(net) = l.net {
            entry.push(ChannelEntry {
                handle: net,
                rights: ChannelObject::RIGHTS,
                transfer: Transfer::Move,
            });
            flags |= EXT_NET;
        }
        if let Some(random) = l.random {
            entry.push(ChannelEntry {
                handle: random,
                rights: ChannelObject::RIGHTS,
                transfer: Transfer::Move,
            });
            flags |= EXT_RANDOM;
        }
        let mut message = [0u8; 5];
        let message: &[u8] = match flags {
            0 => SELF_MESSAGE,
            EXT_NET => SELF_NET_MESSAGE,
            _ => {
                message[..4].copy_from_slice(SELF_EXT_MESSAGE);
                message[4] = flags;
                &message
            }
        };
        // On a failed write the caller keeps what it moved (INV-0050-1).
        if channel_write(boot, message, &entry).is_err() {
            let _ = handle_close(me);
            for h in l.net.into_iter().chain(l.random) {
                let _ = handle_close(h);
            }
        }
    } else {
        for h in l.net.into_iter().chain(l.random) {
            let _ = handle_close(h);
        }
    }
    let _ = handle_close(boot);
    if let Err(s) = broker.commit(p.pid, process) {
        return Err(gone(s));
    }
    Ok(Child {
        pid: p.pid,
        pgid: p.pgid,
        process,
        job: p.parent_job,
    })
}

/// A program file of a volume as an image (ADR-0108), and whether it is a
/// native program (ADR-0153 rule 9): its bytes are read once for the mark
/// and once into the image's memory object.
pub fn load(file: &crate::files::File) -> Result<(Handle, bool), Status> {
    let gone = |_| Status::BadState;
    let bytes = crate::files::read_all(file).map_err(gone)?;
    let native = zoraya_elf::is_native(&bytes);
    let length = bytes.len() as u64;
    drop(bytes);
    let size = length.next_multiple_of(4096).max(4096);
    let object = crate::memory_create(size, zoraya_abi::MemoryFlags::NONE)?;
    let fill = || -> Result<(), Status> {
        let copy = handle_duplicate(object, <zoraya_abi::MemoryObject as ObjectType>::RIGHTS)?;
        file.attach(copy, size).map_err(gone)?;
        let mut at = 0;
        while at < length {
            let n = (length - at).min(1 << 20) as u32;
            match file.read_buffer(at, at, n) {
                Ok(0) | Err(_) => return Err(Status::BadState),
                Ok(got) => at += got,
            }
        }
        Ok(())
    };
    let image = fill().and_then(|()| crate::image_create(object, length));
    let _ = handle_close(object);
    Ok((image?, native))
}

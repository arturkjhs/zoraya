//! `AF_UNIX` sockets over channels (ADR-0412, plan part F2): `socketpair`,
//! `send`, `recv`, `sendmsg` and `recvmsg` with `SCM_RIGHTS` as handle
//! transfer, on the handles a Unix socket fd holds (`socket::UnixPart`).
//!
//! Every call that can wait waits first and acts after: a method that returns
//! `PENDING` (ADR-0315) is run again from its start, so nothing it did may
//! be left half done at a wait. A send that carries descriptors checks
//! everything it can before it queues anything (INV-0412-1), and queues the
//! message with `Sys::channel_send`, which is all or nothing.

#![forbid(unsafe_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;

use zoraya_abi::Status;

use crate::errno::{
    Call, EAFNOSUPPORT, EAGAIN, EBADF, EBUSY, EDESTADDRREQ, EINVAL, EMFILE, EMSGSIZE, ENOMEM,
    ENOTCONN, ENOTSOCK, EOPNOTSUPP, EPIPE, EPROTONOSUPPORT,
};
use crate::memfd::{Memory, seal};
use crate::process::{Desc, Listing, Posix, native, o};
use crate::socket::{Held, Socket, State};
use crate::stat::Kind as FileKind;
use crate::sys::{Sys, User, signals};
use crate::unix::{
    self, AF_UNIX, ETOOMANYREFS, Kind, MAX_HANDLES, MSG_CMSG_CLOEXEC, MSG_CTRUNC, MSG_TRUNC,
    Record, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM,
};
use zoraya_proto_file::rights;

/// What `recvmsg` gave.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Received {
    pub bytes: usize,
    /// The descriptors that came, installed at the lowest free numbers.
    pub fds: Vec<i32>,
    /// `MSG_TRUNC`, `MSG_CTRUNC`.
    pub flags: i32,
}

/// What a send with descriptors made ready, before it is queued.
struct Prepared<H> {
    records: Vec<Record>,
    /// Each handle and whether the message copies it (else it moves).
    handles: Vec<(H, bool)>,
    /// Handles made for this send alone (a file's new connection): closed if
    /// the send does not happen.
    created: Vec<H>,
    /// The fds of sockets that move: closed by the send.
    moved: Vec<i32>,
    /// Handles of the sender's that stay behind when the send happens and
    /// are closed then: the ancillary channel of a socket sent from a `fork`
    /// parent (the receiver holds streams alone, `shared`).
    close_after: Vec<H>,
    /// The fds of memory descriptors that go: their size is fixed once the
    /// send happens (INV-0413-1).
    sent_memory: Vec<i32>,
    /// The memory handles of `handles` that have no `WRITE`.
    read_only: Vec<H>,
}

fn total(iov: &[User]) -> usize {
    iov.iter().fold(0usize, |a, u| a.saturating_add(u.length))
}

/// `socket(AF_UNIX, ...)`'s checks: the type without its flags, and the flags.
pub fn check_unix(domain: i32, ty: i32, protocol: i32) -> Result<(i32, bool, bool), i32> {
    if domain != AF_UNIX {
        return Err(EAFNOSUPPORT);
    }
    let known = crate::socket::SOCK_NONBLOCK | crate::socket::SOCK_CLOEXEC;
    let kind = ty & !known;
    if !matches!(kind, SOCK_STREAM | SOCK_DGRAM | SOCK_SEQPACKET) {
        return Err(EPROTONOSUPPORT);
    }
    if protocol != 0 {
        return Err(EPROTONOSUPPORT);
    }
    Ok((
        kind,
        ty & crate::socket::SOCK_NONBLOCK != 0,
        ty & crate::socket::SOCK_CLOEXEC != 0,
    ))
}

/// `errno` for what `channel_send` refused.
fn send_errno(s: Status) -> i32 {
    match s {
        Status::PeerClosed => EPIPE,
        // An end that holds handles, or that the message travels on
        // (ADR-0050).
        Status::BadState => EBUSY,
        Status::BadSyscall => EOPNOTSUPP,
        Status::NoMemory => ENOMEM,
        s => native(s, Call::Other),
    }
}

impl<S: Sys> Posix<S> {
    fn unix_socket(&self, fd: i32) -> Result<&Socket<S::H>, i32> {
        match self.fds.get(fd) {
            Some(Desc::Socket(s)) if s.unix.is_some() => Ok(s),
            Some(_) => Err(ENOTSOCK),
            None => Err(EBADF),
        }
    }

    fn unix_mut(&mut self, fd: i32) -> Result<&mut Socket<S::H>, i32> {
        match self.fds.get_mut(fd) {
            Some(Desc::Socket(s)) if s.unix.is_some() => Ok(s),
            Some(_) => Err(ENOTSOCK),
            None => Err(EBADF),
        }
    }

    /// True if `fd` is an `AF_UNIX` socket.
    pub fn is_unix(&self, fd: i32) -> bool {
        self.unix_socket(fd).is_ok()
    }

    /// `socketpair(AF_UNIX, type, 0)` (ADR-0412 item 1): a connected pair.
    /// A stream pair is two crossed byte streams and one ancillary channel; a
    /// sequenced-packet or datagram pair is one channel.
    pub fn socketpair(&mut self, domain: i32, ty: i32, protocol: i32) -> Result<(i32, i32), i32> {
        let (kind, nonblock, cloexec) = check_unix(domain, ty, protocol)?;
        let mut a = Socket::fresh_unix(kind, nonblock);
        let mut b = Socket::fresh_unix(kind, nonblock);
        a.state = State::Connected;
        b.state = State::Connected;
        let (c1, c2) = self
            .sys
            .channel_create()
            .map_err(|s| native(s, Call::Descriptor))?;
        a.ctl = Some(c1);
        b.ctl = Some(c2);
        if kind == SOCK_STREAM {
            // a writes `one` and reads `two`; b the other way round.
            let one = self.sys.stream_create();
            let two = self.sys.stream_create();
            match (one, two) {
                (Ok(one), Ok(two)) => {
                    a.write = Some(one.1);
                    b.read = Some(one.0);
                    b.write = Some(two.1);
                    a.read = Some(two.0);
                }
                (one, two) => {
                    let mut e = EMFILE;
                    for r in [one, two] {
                        match r {
                            Ok((x, y)) => {
                                self.sys.close(x);
                                self.sys.close(y);
                            }
                            Err(s) => e = native(s, Call::Descriptor),
                        }
                    }
                    self.sys.close(c1);
                    self.sys.close(c2);
                    return Err(e);
                }
            }
        }
        let da = Desc::Socket(Box::new(a));
        let db = Desc::Socket(Box::new(b));
        let fa = match self.fds.install(0, da, cloexec) {
            Ok(fd) => fd as i32,
            Err(d) => {
                let _ = self.release(d);
                let _ = self.release(db);
                return Err(EMFILE);
            }
        };
        match self.fds.install(0, db, cloexec) {
            Ok(fb) => Ok((fa, fb as i32)),
            Err(d) => {
                let _ = self.release(d);
                let _ = self.close(fa);
                Err(EMFILE)
            }
        }
    }

    // ------------------------------------------------------------ sending

    /// The descriptors of a `SCM_RIGHTS` message made ready: validated, with
    /// the handles each brings. Nothing is queued or closed; the handles
    /// this made are `created`, for the caller to close if it does not send
    /// (INV-0412-1). `from` is the socket the message goes on.
    fn prepare_rights(&mut self, from: i32, fds: &[i32]) -> Result<Prepared<S::H>, i32> {
        let mut p = Prepared {
            records: Vec::new(),
            handles: Vec::new(),
            created: Vec::new(),
            moved: Vec::new(),
            close_after: Vec::new(),
            sent_memory: Vec::new(),
            read_only: Vec::new(),
        };
        if fds.len() > MAX_HANDLES {
            return Err(ETOOMANYREFS);
        }
        for &fd in fds {
            if let Err(e) = self.prepare_one(from, fd, &mut p) {
                for h in p.created.drain(..) {
                    self.sys.close(h);
                }
                return Err(e);
            }
            if p.handles.len() > MAX_HANDLES {
                for h in p.created.drain(..) {
                    self.sys.close(h);
                }
                return Err(ETOOMANYREFS);
            }
        }
        Ok(p)
    }

    fn prepare_one(&mut self, from: i32, fd: i32, p: &mut Prepared<S::H>) -> Result<(), i32> {
        enum Plan<H> {
            File(H, bool, u32, bool),
            Pipe(H, bool, bool),
            Socket,
            Memory(H, (u8, u64, u64)),
        }
        let plan = match self.fds.get(fd) {
            None => return Err(EBADF),
            Some(&Desc::Object {
                conn,
                kind,
                access,
                nonblock,
                ..
            }) => Plan::File(conn, kind == FileKind::Directory, access, nonblock),
            Some(Desc::Stream { tty: true, .. }) => return Err(EINVAL),
            Some(&Desc::Stream {
                read,
                write,
                nonblock,
                ..
            }) => match (read, write) {
                (Some(r), _) => Plan::Pipe(r, true, nonblock),
                (None, Some(w)) => Plan::Pipe(w, false, nonblock),
                (None, None) => return Err(EINVAL),
            },
            Some(Desc::Null { .. }) => return Err(EINVAL),
            Some(Desc::Socket(s)) if s.unix.is_none() => return Err(EINVAL),
            Some(Desc::Socket(_)) => Plan::Socket,
            // A descriptor with no object yet (size 0) has nothing to send.
            Some(Desc::Memory(m)) => match m.object() {
                Some(h) => Plan::Memory(h, m.wire()),
                None => return Err(EINVAL),
            },
        };
        match plan {
            Plan::File(conn, dir, access, nonblock) => {
                let wanted = if dir {
                    rights::READ | rights::CLONE
                } else {
                    match access & 3 {
                        1 => rights::WRITE | rights::CLONE,
                        2 => rights::READ | rights::WRITE | rights::CLONE,
                        _ => rights::READ | rights::CLONE,
                    }
                };
                let new = self.share_conn(conn, wanted)?;
                p.created.push(new);
                p.handles.push((new, false));
                p.records.push(Record::one(
                    if dir { Kind::Dir } else { Kind::File },
                    (access & 3) as u8,
                    nonblock,
                ));
            }
            Plan::Pipe(h, read, nonblock) => {
                p.handles.push((h, true));
                p.records.push(Record::one(
                    if read {
                        Kind::PipeRead
                    } else {
                        Kind::PipeWrite
                    },
                    0,
                    nonblock,
                ));
            }
            Plan::Memory(h, (access, size, flags)) => {
                // A copy of the handle, with the rights the descriptor has:
                // after a seal, none to write (INV-0413-2).
                p.handles.push((h, true));
                if access == 0 {
                    p.read_only.push(h);
                }
                let mut r = Record::one(Kind::Memory, access, false);
                r.read_off = size;
                r.write_off = flags;
                p.records.push(r);
                p.sent_memory.push(fd);
            }
            Plan::Socket => {
                // A description moves once, whatever fds name it.
                if p.moved.iter().any(|&m| self.fds.same(m, fd)) {
                    return Err(EINVAL);
                }
                let s = self.unix_socket(fd)?;
                let Some(u) = s.unix.as_ref() else {
                    return Err(ENOTSOCK);
                };
                if s.state != State::Connected {
                    return Err(EINVAL);
                }
                let mut r = Record::one(Kind::Packet, 0, s.nonblock);
                let mut hs: Vec<(S::H, bool)> = Vec::new();
                let mut leftover: Option<S::H> = None;
                if s.kind == SOCK_STREAM {
                    // What the library holds (descriptors that came before
                    // their bytes, bytes past a tag) cannot go with it.
                    if u.book.has_pending() || u.held.is_some() {
                        return Err(EBUSY);
                    }
                    r.kind = Kind::Stream;
                    r.sock_type = SOCK_STREAM as u8;
                    r.shared = u.shared;
                    r.read_off = u.book.read_off;
                    r.write_off = u.book.write_off;
                    for (h, flag) in [(s.read, &mut r.has_read), (s.write, &mut r.has_write)] {
                        if let Some(h) = h {
                            *flag = true;
                            hs.push((h, false));
                        }
                    }
                    // A socket whose streams another process holds too goes
                    // as its streams alone, marked shared; the ancillary
                    // channel stays behind and is closed with the send.
                    match s.ctl {
                        Some(c) if u.shared => leftover = Some(c),
                        Some(c) => {
                            r.has_anc = true;
                            hs.push((c, false));
                        }
                        None => {}
                    }
                    if r.handles() == 0 {
                        return Err(EINVAL);
                    }
                } else {
                    if u.held.is_some() {
                        return Err(EBUSY);
                    }
                    let Some(c) = s.ctl else {
                        return Err(EINVAL);
                    };
                    r.sock_type = s.kind as u8;
                    hs.push((c, false));
                }
                // The socket the message travels on cannot travel on itself.
                if self.fds.same(from, fd) {
                    return Err(EBUSY);
                }
                p.handles.extend(hs);
                p.close_after.extend(leftover);
                p.records.push(r);
                p.moved.push(fd);
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ a spawn's descriptors

    /// Makes ready the descriptor the child gets as fd `child` from this
    /// process's `parent`, a Unix socket or a memory descriptor (ADR-0433,
    /// ADR-0412 item 6, ADR-0432 item 5). Nothing is changed here: the
    /// handles a stream's byte streams and a memory object bring are copied
    /// by the message that carries them, the ancillary channel of a stream
    /// socket and the channel of a packet socket move, and
    /// [`Posix::finish_passed`] tells the parent so once the child holds
    /// them. A description given twice, an unconnected socket and a socket
    /// that holds bytes or a message back are refused (`EINVAL`, `EBADF`,
    /// `EBUSY`), as a send of it would be.
    pub(crate) fn pass_to_child(
        &mut self,
        child: u32,
        parent: i32,
        out: &mut crate::jobs::Passed<S::H>,
    ) -> Result<(), i32> {
        if out.sources.iter().any(|&s| self.fds.same(s, parent)) {
            return Err(EINVAL);
        }
        let mut handles: Vec<(S::H, bool)> = Vec::new();
        let mut read_only: Option<S::H> = None;
        let record = match self.fds.get(parent) {
            Some(Desc::Memory(m)) => {
                // A descriptor with no object yet has nothing to give.
                let Some(h) = m.object() else {
                    return Err(EINVAL);
                };
                let (access, size, flags) = m.wire();
                handles.push((h, true));
                if access == 0 {
                    read_only = Some(h);
                }
                let mut r = Record::one(Kind::Memory, access, false);
                r.read_off = size;
                r.write_off = flags;
                out.sent_memory.push(parent);
                r
            }
            Some(Desc::Socket(s)) => {
                let Some(u) = s.unix.as_ref() else {
                    return Err(EBADF);
                };
                if s.state != State::Connected {
                    return Err(EBADF);
                }
                if u.held.is_some() {
                    return Err(EBUSY);
                }
                if s.kind == SOCK_STREAM {
                    if u.book.has_pending() {
                        return Err(EBUSY);
                    }
                    let mut r = Record::one(Kind::Stream, 0, s.nonblock);
                    r.sock_type = SOCK_STREAM as u8;
                    r.read_off = u.book.read_off;
                    r.write_off = u.book.write_off;
                    // The whole socket moves, as a packet socket does: its
                    // byte streams and its ancillary channel, so that one
                    // process holds the stream's counts of bytes and
                    // descriptors. The parent's descriptor is dead until it is
                    // closed (ADR-0433 item 6, the architect's to accept).
                    for (h, flag) in [(s.read, &mut r.has_read), (s.write, &mut r.has_write)] {
                        if let Some(h) = h {
                            *flag = true;
                            handles.push((h, false));
                        }
                    }
                    match s.ctl {
                        Some(c) if !u.shared => {
                            r.has_anc = true;
                            handles.push((c, false));
                        }
                        // Shared by a `fork` already: the channel stays where
                        // `fork` left it and is closed once the child has the
                        // streams, which it holds as shared.
                        Some(c) => {
                            r.shared = true;
                            out.close_after.push(c);
                        }
                        None => r.shared = true,
                    }
                    out.moved_packets.push(parent);
                    if r.handles() == 0 {
                        return Err(EBADF);
                    }
                    r
                } else {
                    let Some(c) = s.ctl else {
                        return Err(EBADF);
                    };
                    let mut r = Record::one(Kind::Packet, 0, s.nonblock);
                    r.sock_type = s.kind as u8;
                    handles.push((c, false));
                    out.moved_packets.push(parent);
                    r
                }
            }
            _ => return Err(EBADF),
        };
        if out.handles.len() + handles.len() > MAX_HANDLES {
            // The whole `Inherit` is dropped on this error: nothing to roll back.
            return Err(crate::errno::E2BIG);
        }
        out.fds.push(child);
        out.records.push(record);
        out.handles.extend(handles);
        out.read_only.extend(read_only);
        out.sources.push(parent);
        Ok(())
    }

    /// The spawn happened: the sockets that moved are dead here, a stream
    /// socket's ancillary channel is the child's and the socket is shared,
    /// and a memory descriptor's size is fixed.
    pub(crate) fn finish_passed(&mut self, passed: &crate::jobs::Passed<S::H>) {
        for &fd in &passed.moved_packets {
            if let Some(Desc::Socket(s)) = self.fds.get_mut(fd) {
                s.read = None;
                s.write = None;
                s.ctl = None;
                s.state = State::Dead;
            }
        }
        for &h in &passed.close_after {
            self.sys.close(h);
        }
        self.memory_sent(&passed.sent_memory);
    }

    /// The child's half, at start-up: the descriptors of the `ZPBF` bootstrap
    /// message installed at their fds. A fd already taken, or a record that
    /// does not fit its handles, closes what came; the caller has checked
    /// the message with `unix::decode_passed`.
    pub fn install_passed(&mut self, fds: &[u32], records: Vec<Record>, handles: Vec<S::H>) {
        let mut it = handles.into_iter();
        for (&fd, record) in fds.iter().zip(records) {
            let mine: Vec<S::H> = it.by_ref().take(record.handles()).collect();
            if mine.len() != record.handles() {
                for h in mine {
                    self.sys.close(h);
                }
                continue;
            }
            let desc = self.desc_from(record, mine);
            if let Err(d) = self.fds.install_at(fd, desc) {
                let _ = self.release(d);
            }
        }
        for h in it {
            self.sys.close(h);
        }
    }

    /// The send happened: the sockets that moved are gone from this process.
    fn finish_moves(&mut self, moved: &[i32], close_after: Vec<S::H>) {
        for h in close_after {
            self.sys.close(h);
        }
        for &fd in moved {
            if let Some(Desc::Socket(s)) = self.fds.get_mut(fd) {
                s.read = None;
                s.write = None;
                s.ctl = None;
                s.state = State::Dead;
            }
            let _ = self.close(fd);
        }
    }

    fn drop_prepared(&mut self, p: Prepared<S::H>) {
        for h in p.created {
            self.sys.close(h);
        }
    }

    /// Copies `iov` into one buffer, in order.
    fn gather(&mut self, iov: &[User]) -> Vec<u8> {
        let mut out = alloc::vec![0u8; total(iov)];
        let mut at = 0;
        for u in iov.iter().filter(|u| u.length > 0) {
            self.sys.copy_in(*u, &mut out[at..at + u.length]);
            at += u.length;
        }
        out
    }

    /// Copies `bytes` out into `iov`, in order; how many went.
    fn scatter(&mut self, iov: &[User], bytes: &[u8]) -> usize {
        let mut at = 0;
        for u in iov {
            let n = u.length.min(bytes.len() - at);
            if n > 0 {
                self.sys.copy_out(u.part(0, n), &bytes[at..at + n]);
                at += n;
            }
        }
        at
    }

    /// Waits until the stream end `end` takes bytes: `Ok` when it does or the
    /// kernel cannot say; `EPIPE` when its reader is gone; `EAGAIN` for a
    /// non-blocking call that would wait.
    fn room_in_stream(&mut self, end: S::H, nonblock: bool) -> Result<(), i32> {
        loop {
            match self.sys.signals_of(end) {
                Ok(s) if s & signals::PEER_CLOSED != 0 => return Err(EPIPE),
                Ok(s) if s & signals::WRITABLE == 0 => {
                    if nonblock {
                        return Err(EAGAIN);
                    }
                    self.wait_point(end, signals::WRITABLE | signals::PEER_CLOSED)?;
                }
                _ => return Ok(()),
            }
        }
    }

    /// Queues `bytes` and `handles` on channel `end`, waiting for room unless
    /// `nonblock`. Nothing happens on an error.
    fn send_on_channel(
        &mut self,
        end: S::H,
        bytes: &[u8],
        handles: &[(S::H, bool)],
        read_only: &[S::H],
        nonblock: bool,
    ) -> Result<(), i32> {
        loop {
            match self.sys.channel_send(end, bytes, handles, read_only) {
                Ok(()) => return Ok(()),
                Err(Status::WouldBlock) => {
                    if nonblock {
                        return Err(EAGAIN);
                    }
                    self.wait_point(end, signals::WRITABLE | signals::PEER_CLOSED)?;
                }
                Err(s) => return Err(send_errno(s)),
            }
        }
    }

    /// `sendmsg` and `send` on a socket: `iov` is the data, `rights` the
    /// descriptors of a `SCM_RIGHTS` control message. `flags` is
    /// `MSG_DONTWAIT` and what `check_msg_flags` allows.
    pub fn sendmsg(
        &mut self,
        fd: i32,
        iov: &[User],
        rights: &[i32],
        flags: i32,
    ) -> Result<usize, i32> {
        let dontwait = flags & crate::socket::MSG_DONTWAIT != 0;
        if self.unix_socket(fd).is_err() {
            return self.sendmsg_plain(fd, iov, rights, dontwait);
        }
        let s = self.unix_socket(fd)?;
        let (kind, nonblock) = (s.kind, s.nonblock || dontwait);
        if kind == SOCK_STREAM {
            self.send_stream(fd, iov, rights, nonblock)
        } else {
            self.send_packet(fd, iov, rights, nonblock)
        }
    }

    /// `sendmsg` on a TCP socket: its pieces, no descriptors.
    fn sendmsg_plain(
        &mut self,
        fd: i32,
        iov: &[User],
        rights: &[i32],
        dontwait: bool,
    ) -> Result<usize, i32> {
        if !rights.is_empty() {
            return Err(EINVAL);
        }
        let mut done = 0;
        for u in iov.iter().filter(|u| u.length > 0) {
            match self.sock_write(fd, *u, dontwait) {
                Ok(n) => {
                    done += n;
                    if n < u.length {
                        break;
                    }
                }
                Err(e) if done == 0 => return Err(e),
                Err(_) => break,
            }
        }
        Ok(done)
    }

    fn send_stream(
        &mut self,
        fd: i32,
        iov: &[User],
        rights: &[i32],
        nonblock: bool,
    ) -> Result<usize, i32> {
        let s = self.unix_socket(fd)?;
        match s.state {
            State::Fresh | State::Connecting => return Err(ENOTCONN),
            State::Dead => return Err(EPIPE),
            State::Connected => {}
        }
        let (write, anc, shared) = (s.write, s.ctl, s.unix.as_ref().is_some_and(|u| u.shared));
        let Some(write) = write else {
            return Err(EPIPE);
        };
        if rights.is_empty() {
            return self.write_pieces(fd, write, iov, nonblock, false);
        }
        if shared {
            return Err(EOPNOTSUPP);
        }
        // A descriptor goes with a byte, at least one (ADR-0412 item 2).
        if total(iov) == 0 {
            return Err(EINVAL);
        }
        let Some(anc) = anc else {
            return Err(EOPNOTSUPP);
        };
        self.room_in_stream(write, nonblock)?;
        let prepared = self.prepare_rights(fd, rights)?;
        let offset = self
            .unix_socket(fd)?
            .unix
            .as_ref()
            .map_or(0, |u| u.book.write_off);
        let message = unix::encode_anc(offset, &prepared.records);
        if let Err(e) = self.send_on_channel(
            anc,
            &message,
            &prepared.handles,
            &prepared.read_only,
            nonblock,
        ) {
            self.drop_prepared(prepared);
            return Err(e);
        }
        let (moved, after) = (prepared.moved, prepared.close_after);
        self.memory_sent(&prepared.sent_memory);
        self.finish_moves(&moved, after);
        // The descriptors are queued; their byte follows at once.
        self.write_pieces(fd, write, iov, nonblock, true)
    }

    /// Writes `iov` to a stream socket's write end: the pieces in order, as
    /// many as go; counts the offset. `committed`: the descriptors that go with
    /// the first byte are already queued, so this call must not be run again
    /// (a deferred wait would send them twice): it waits where it stands.
    fn write_pieces(
        &mut self,
        fd: i32,
        end: S::H,
        iov: &[User],
        nonblock: bool,
        committed: bool,
    ) -> Result<usize, i32> {
        let mut done = 0;
        for u in iov.iter().filter(|u| u.length > 0) {
            // Only the first piece may wait: the rest must not hold a caller
            // that already has bytes sent.
            let step = loop {
                let r = self.write_stream(end, *u, nonblock || done > 0 || committed);
                if committed && !nonblock && done == 0 && r == Err(EAGAIN) {
                    match self.sys.wait(end, signals::WRITABLE | signals::PEER_CLOSED) {
                        Ok(_) => continue,
                        Err(_) => break Err(EPIPE),
                    }
                }
                break r;
            };
            match step {
                Ok(n) => {
                    done += n;
                    if n < u.length {
                        break;
                    }
                }
                Err(e) if done == 0 => return Err(e),
                Err(_) => break,
            }
        }
        if let Ok(s) = self.unix_mut(fd)
            && let Some(u) = s.unix.as_mut()
        {
            u.book.write_off += done as u64;
        }
        Ok(done)
    }

    fn send_packet(
        &mut self,
        fd: i32,
        iov: &[User],
        rights: &[i32],
        nonblock: bool,
    ) -> Result<usize, i32> {
        let s = self.unix_socket(fd)?;
        let (state, ctl, shut) = (s.state, s.ctl, s.unix.as_ref().is_some_and(|u| u.shut_wr));
        match state {
            State::Fresh | State::Connecting => {
                return Err(if s.kind == SOCK_DGRAM {
                    EDESTADDRREQ
                } else {
                    ENOTCONN
                });
            }
            State::Dead => return Err(EPIPE),
            State::Connected => {}
        }
        if shut {
            return Err(EPIPE);
        }
        let Some(ctl) = ctl else {
            return Err(EPIPE);
        };
        if rights.len() > MAX_HANDLES {
            return Err(ETOOMANYREFS);
        }
        let size = total(iov);
        if size > unix::packet_room(rights.len()) {
            return Err(EMSGSIZE);
        }
        // Wait for room before anything is made.
        loop {
            match self.sys.signals_of(ctl) {
                Ok(s) if s & signals::PEER_CLOSED != 0 => return Err(EPIPE),
                Ok(s) if s & signals::WRITABLE == 0 => {
                    if nonblock {
                        return Err(EAGAIN);
                    }
                    self.wait_point(ctl, signals::WRITABLE | signals::PEER_CLOSED)?;
                }
                _ => break,
            }
        }
        let prepared = if rights.is_empty() {
            Prepared {
                records: Vec::new(),
                handles: Vec::new(),
                created: Vec::new(),
                moved: Vec::new(),
                close_after: Vec::new(),
                sent_memory: Vec::new(),
                read_only: Vec::new(),
            }
        } else {
            self.prepare_rights(fd, rights)?
        };
        let payload = self.gather(iov);
        let message = unix::encode_packet(&prepared.records, &payload);
        if let Err(e) = self.send_on_channel(
            ctl,
            &message,
            &prepared.handles,
            &prepared.read_only,
            nonblock,
        ) {
            self.drop_prepared(prepared);
            return Err(e);
        }
        let (moved, after) = (prepared.moved, prepared.close_after);
        self.memory_sent(&prepared.sent_memory);
        self.finish_moves(&moved, after);
        Ok(size)
    }

    // ------------------------------------------------------------ receiving

    /// `recvmsg` and `recv` on a socket: the data into `iov`, up to `max_fds`
    /// descriptors installed (the others are closed and `MSG_CTRUNC` says so).
    /// `flags` is `MSG_DONTWAIT` and `MSG_CMSG_CLOEXEC`.
    pub fn recvmsg(
        &mut self,
        fd: i32,
        iov: &[User],
        max_fds: usize,
        flags: i32,
    ) -> Result<Received, i32> {
        let dontwait = flags & crate::socket::MSG_DONTWAIT != 0;
        if self.unix_socket(fd).is_err() {
            return self.recvmsg_plain(fd, iov, dontwait);
        }
        let s = self.unix_socket(fd)?;
        let (kind, nonblock) = (s.kind, s.nonblock || dontwait);
        let cloexec = flags & MSG_CMSG_CLOEXEC != 0;
        if kind == SOCK_STREAM {
            self.recv_stream(fd, iov, max_fds, nonblock, cloexec)
        } else {
            self.recv_packet(fd, iov, max_fds, nonblock, cloexec)
        }
    }

    fn recvmsg_plain(&mut self, fd: i32, iov: &[User], dontwait: bool) -> Result<Received, i32> {
        let mut done = 0;
        let mut dontwait = dontwait;
        for u in iov.iter().filter(|u| u.length > 0) {
            match self.sock_read(fd, *u, dontwait) {
                Ok(n) => {
                    done += n;
                    if n < u.length {
                        break;
                    }
                    // The first piece may wait; the rest must not.
                    dontwait = true;
                }
                Err(EAGAIN) if done > 0 => break,
                Err(e) if done == 0 => return Err(e),
                Err(_) => break,
            }
        }
        Ok(Received {
            bytes: done,
            ..Received::default()
        })
    }

    /// Takes every ancillary message that has come into the socket's book;
    /// a message that is not one, or whose byte has gone, is closed.
    fn drain_tags(&mut self, fd: i32) -> Result<(), i32> {
        let s = self.unix_socket(fd)?;
        let (Some(anc), shared) = (s.ctl, s.unix.as_ref().is_some_and(|u| u.shared)) else {
            return Ok(());
        };
        loop {
            let size = match self
                .sys
                .channel_read(anc, &mut self.input, &mut self.handles)
            {
                Ok(size) => size,
                Err(_) => return Ok(()),
            };
            let handles: Vec<S::H> = core::mem::take(&mut self.handles);
            let parsed = if shared {
                None
            } else {
                unix::decode_anc(&self.input[..size], handles.len())
            };
            let Some((offset, records)) = parsed else {
                // Not for this holder, or not a message of this library.
                for h in handles {
                    self.sys.close(h);
                }
                continue;
            };
            let mut items = Vec::with_capacity(records.len());
            let mut it = handles.into_iter();
            for r in records {
                let hs: Vec<S::H> = it.by_ref().take(r.handles()).collect();
                items.push((r, hs));
            }
            let lost = match self.unix_mut(fd)?.unix.as_mut() {
                Some(u) => u.book.push(offset, items),
                None => None,
            };
            if let Some(t) = lost {
                for (_, hs) in t.items {
                    for h in hs {
                        self.sys.close(h);
                    }
                }
            }
        }
    }

    /// The descriptors of a received message as fds: up to `max_fds` of them,
    /// the rest closed. The table has room (checked before the bytes were
    /// taken) or the surplus is closed too.
    fn install_received(
        &mut self,
        items: Vec<(Record, Vec<S::H>)>,
        max_fds: usize,
        cloexec: bool,
    ) -> (Vec<i32>, i32) {
        let mut fds = Vec::new();
        let mut flags = 0;
        for (record, handles) in items {
            if fds.len() >= max_fds {
                flags |= MSG_CTRUNC;
                for h in handles {
                    self.sys.close(h);
                }
                continue;
            }
            let desc = self.desc_from(record, handles);
            match self.fds.install(0, desc, cloexec) {
                Ok(fd) => fds.push(fd as i32),
                Err(d) => {
                    flags |= MSG_CTRUNC;
                    let _ = self.release(d);
                }
            }
        }
        (fds, flags)
    }

    /// The description a record and its handles make in the receiver.
    fn desc_from(&mut self, r: Record, handles: Vec<S::H>) -> Desc<S::H> {
        // Every kind but a stream socket has one handle, and a stream socket
        // at least one (`unix::Record::decode` and the handle count).
        let first = handles[0];
        match r.kind {
            Kind::File | Kind::Dir => Desc::Object {
                conn: first,
                kind: if r.kind == Kind::Dir {
                    FileKind::Directory
                } else {
                    FileKind::File
                },
                access: u32::from(r.access),
                nonblock: r.nonblock,
                sync: false,
                written: false,
                attached: false,
                place: None,
                listing: Listing::default(),
            },
            Kind::PipeRead => Desc::Stream {
                read: Some(first),
                write: None,
                nonblock: r.nonblock,
                tty: false,
                conn: None,
            },
            Kind::PipeWrite => Desc::Stream {
                read: None,
                write: Some(first),
                nonblock: r.nonblock,
                tty: false,
                conn: None,
            },
            Kind::Stream => {
                let mut s = Socket::fresh_unix(SOCK_STREAM, r.nonblock);
                s.state = State::Connected;
                let mut it = handles.into_iter();
                if r.has_read {
                    s.read = it.next();
                }
                if r.has_write {
                    s.write = it.next();
                }
                if r.has_anc {
                    s.ctl = it.next();
                }
                if let Some(u) = s.unix.as_mut() {
                    u.shared = r.shared;
                    u.book.read_off = r.read_off;
                    u.book.write_off = r.write_off;
                }
                Desc::Socket(Box::new(s))
            }
            Kind::Memory => {
                let access = if r.access == 2 { o::RDWR } else { o::RDONLY };
                // What the receiver can check: the object's size never
                // changes (`SHRINK`, `GROW`) and a seal against sealing only
                // takes from the receiver. `F_SEAL_WRITE` is the peer's word
                // that nobody writes, which the receiver cannot check: never
                // reported. `F_SEAL_FUTURE_WRITE` only if this handle is
                // read-only, which the kernel enforces.
                let mut seals = (r.write_off & unix::MEMORY_SEALS) as u32
                    & (seal::SEAL | seal::SHRINK | seal::GROW);
                if access == o::RDONLY {
                    seals |= seal::FUTURE_WRITE;
                }
                let sealable = r.write_off & unix::MEMORY_SEALABLE != 0;
                let id = self.memory_id();
                Desc::Memory(Box::new(Memory::received(
                    id, first, access, r.read_off, seals, sealable,
                )))
            }
            Kind::Packet => {
                let mut s = Socket::fresh_unix(i32::from(r.sock_type), r.nonblock);
                s.state = State::Connected;
                s.ctl = Some(first);
                Desc::Socket(Box::new(s))
            }
        }
    }

    /// Fds the table can still take.
    fn free_fds(&self) -> usize {
        (zoraya_abi::posix::OPEN_MAX as usize).saturating_sub(self.fds.count())
    }

    /// The bytes the stream holds at `rest`, copied out first.
    fn recv_stream(
        &mut self,
        fd: i32,
        iov: &[User],
        max_fds: usize,
        nonblock: bool,
        cloexec: bool,
    ) -> Result<Received, i32> {
        let s = self.unix_socket(fd)?;
        match s.state {
            State::Fresh | State::Connecting => return Err(ENOTCONN),
            State::Dead => return Ok(Received::default()),
            State::Connected => {}
        }
        let read = s.read;
        let size = total(iov);
        loop {
            self.drain_tags(fd)?;
            let Some(u) = self.unix_socket(fd)?.unix.as_ref() else {
                return Err(ENOTSOCK);
            };
            let want = u.book.cap(size);
            // The descriptors of the byte about to be read need room first
            // (the message stays queued, ADR-0412 item 3).
            let due = u.book.due_at_read(max_fds);
            if due > self.free_fds() {
                return Err(EMFILE);
            }
            if want == 0 {
                return Ok(Received::default());
            }
            if u.book.has_rest() {
                let n = u.book.rest.len().min(want);
                let bytes: Vec<u8> = {
                    let u = self.unix_mut(fd)?.unix.as_mut().ok_or(ENOTSOCK)?;
                    u.book.rest.drain(..n).collect()
                };
                self.scatter(iov, &bytes);
                return Ok(self.finish_stream(fd, n, max_fds, cloexec));
            }
            let Some(end) = read else {
                return Ok(Received::default());
            };
            let mut got = 0;
            let mut eof = false;
            for u in iov.iter().filter(|u| u.length > 0) {
                if got >= want {
                    break;
                }
                let take = u.length.min(want - got);
                match self.sys.stream_read(end, u.part(0, take)) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        got += n;
                        if n < take {
                            break;
                        }
                    }
                    Err(Status::WouldBlock) => break,
                    Err(s) if got == 0 => return Err(native(s, Call::Other)),
                    Err(_) => break,
                }
            }
            if got == 0 {
                if eof {
                    return Ok(Received::default());
                }
                if nonblock {
                    return Err(EAGAIN);
                }
                self.wait_point(end, signals::READABLE | signals::PEER_CLOSED)?;
                continue;
            }
            // A tag may have come while the bytes were taken: the bytes past
            // it are kept, to be given with the next call.
            self.drain_tags(fd)?;
            // A tag at the offset of this read that came meanwhile: if the
            // table has no room for its descriptors the message stays queued
            // (ADR-0412 item 3): the bytes go back to the library.
            let due = self
                .unix_socket(fd)?
                .unix
                .as_ref()
                .map_or(0, |u| u.book.due_at_read(max_fds));
            if due > self.free_fds() {
                let all = self.gather_range(iov, 0, got);
                if let Some(u) = self.unix_mut(fd)?.unix.as_mut() {
                    u.book.rest.extend(all);
                }
                return Err(EMFILE);
            }
            let excess = self
                .unix_socket(fd)?
                .unix
                .as_ref()
                .map_or(0, |u| u.book.excess(got));
            if excess > 0 {
                let kept = self.gather_range(iov, got - excess, excess);
                if let Some(u) = self.unix_mut(fd)?.unix.as_mut() {
                    u.book.rest.extend(kept);
                }
                got -= excess;
            }
            return Ok(self.finish_stream(fd, got, max_fds, cloexec));
        }
    }

    /// Bytes `from..from + len` of `iov`'s sequence.
    fn gather_range(&mut self, iov: &[User], from: usize, len: usize) -> Vec<u8> {
        let all = self.gather(iov);
        all[from..from + len].to_vec()
    }

    /// `n` bytes of a stream socket were handed over: its tags are due.
    fn finish_stream(&mut self, fd: i32, n: usize, max_fds: usize, cloexec: bool) -> Received {
        let tags = match self.unix_mut(fd).ok().and_then(|s| s.unix.as_mut()) {
            Some(u) => u.book.consumed(n),
            None => Vec::new(),
        };
        let items: Vec<(Record, Vec<S::H>)> = tags.into_iter().flat_map(|t| t.items).collect();
        let (fds, flags) = self.install_received(items, max_fds, cloexec);
        Received {
            bytes: n,
            fds,
            flags,
        }
    }

    fn recv_packet(
        &mut self,
        fd: i32,
        iov: &[User],
        max_fds: usize,
        nonblock: bool,
        cloexec: bool,
    ) -> Result<Received, i32> {
        let s = self.unix_socket(fd)?;
        match s.state {
            State::Fresh | State::Connecting => return Err(ENOTCONN),
            State::Dead => return Ok(Received::default()),
            State::Connected => {}
        }
        let Some(ctl) = s.ctl else {
            return Ok(Received::default());
        };
        if s.unix.as_ref().is_some_and(|u| u.shut_rd) {
            return Ok(Received::default());
        }
        let held = self.unix_mut(fd)?.unix.as_mut().and_then(|u| u.held.take());
        let mut message = match held {
            Some(h) => h,
            None => loop {
                match self
                    .sys
                    .channel_read(ctl, &mut self.input, &mut self.handles)
                {
                    Ok(size) => {
                        let handles: Vec<S::H> = core::mem::take(&mut self.handles);
                        match unix::decode_packet(&self.input[..size], handles.len()) {
                            Some((records, at)) => {
                                break Held {
                                    payload: self.input[at..size].to_vec(),
                                    records,
                                    handles,
                                };
                            }
                            None => {
                                for h in handles {
                                    self.sys.close(h);
                                }
                            }
                        }
                    }
                    Err(Status::WouldBlock) => {
                        if nonblock {
                            return Err(EAGAIN);
                        }
                        self.wait_point(ctl, signals::READABLE | signals::PEER_CLOSED)?;
                    }
                    // The sender closed and nothing is left: the end.
                    Err(Status::PeerClosed) => return Ok(Received::default()),
                    Err(s) => return Err(native(s, Call::Other)),
                }
            },
        };
        let want = message.records.len().min(max_fds);
        if want > self.free_fds() {
            if let Some(u) = self.unix_mut(fd)?.unix.as_mut() {
                u.held = Some(message);
            }
            return Err(EMFILE);
        }
        let mut flags = 0;
        let room = total(iov);
        if message.payload.len() > room {
            flags |= MSG_TRUNC;
        }
        let n = self.scatter(iov, &message.payload);
        let mut handles = core::mem::take(&mut message.handles).into_iter();
        let items: Vec<(Record, Vec<S::H>)> = message
            .records
            .into_iter()
            .map(|r| {
                let hs: Vec<S::H> = handles.by_ref().take(r.handles()).collect();
                (r, hs)
            })
            .collect();
        let (fds, more) = self.install_received(items, max_fds, cloexec);
        Ok(Received {
            bytes: n,
            fds,
            flags: flags | more,
        })
    }

    /// `read` and `recv` on a Unix socket: a packet message (truncated) or the
    /// stream's bytes; descriptors that arrive with them are closed.
    pub(crate) fn unix_read(&mut self, fd: i32, into: User, dontwait: bool) -> Result<usize, i32> {
        let flags = if dontwait {
            crate::socket::MSG_DONTWAIT
        } else {
            0
        };
        self.recvmsg(fd, &[into], 0, flags).map(|r| r.bytes)
    }

    /// `write` and `send` on a Unix socket.
    pub(crate) fn unix_write(&mut self, fd: i32, from: User, dontwait: bool) -> Result<usize, i32> {
        let flags = if dontwait {
            crate::socket::MSG_DONTWAIT
        } else {
            0
        };
        self.sendmsg(fd, &[from], &[], flags)
    }

    // ------------------------------------------------------------ the rest

    /// `getsockname` and `getpeername` of a Unix socket: it has no name (socket
    /// files are plan part F7's); a peer exists once it is connected.
    pub fn unix_name(&self, fd: i32, peer: bool) -> Result<(), i32> {
        let s = self.unix_socket(fd)?;
        if peer && s.state != State::Connected {
            return Err(ENOTCONN);
        }
        Ok(())
    }

    /// `shutdown` of a Unix socket. A stream socket closes its byte streams as
    /// TCP does; a packet socket (one channel, no half close) keeps a flag
    /// and ends when both halves are shut.
    pub(crate) fn unix_shutdown(&mut self, fd: i32, how: i32) -> Result<(), i32> {
        let s = self.unix_mut(fd)?;
        if s.state != State::Connected {
            return Err(ENOTCONN);
        }
        let rd = how == crate::socket::SHUT_RD || how == crate::socket::SHUT_RDWR;
        let wr = how == crate::socket::SHUT_WR || how == crate::socket::SHUT_RDWR;
        let mut gone = [None, None, None];
        if s.kind == SOCK_STREAM {
            if rd {
                gone[0] = s.read.take();
            }
            if wr {
                gone[1] = s.write.take();
            }
        } else if let Some(u) = s.unix.as_mut() {
            u.shut_rd |= rd;
            u.shut_wr |= wr;
            if u.shut_rd && u.shut_wr {
                gone[2] = s.ctl.take();
                s.state = State::Dead;
            }
        }
        for h in gone.into_iter().flatten() {
            self.sys.close(h);
        }
        Ok(())
    }

    /// After a `fork` that made a child: a stream socket's byte streams are
    /// held by two processes now, so `SCM_RIGHTS` on it is `EOPNOTSUPP` here
    /// too (ADR-0412 items 2 and 6).
    pub fn after_fork_in_parent(&mut self) {
        self.memory_after_fork();
        for d in self.fds.descriptions_mut() {
            if let Desc::Socket(s) = d
                && let Some(u) = s.unix.as_mut()
            {
                u.shared = true;
            }
        }
    }

    /// In the child of a `fork`: the ancillary channel and what the library
    /// held for it are the parent's (a channel end cannot be duplicated,
    /// ADR-0102 item 3); the byte streams are the child's own handles.
    pub(crate) fn unix_after_fork_in_child(&mut self) {
        self.memory_after_fork();
        let mut closing: Vec<S::H> = Vec::new();
        for d in self.fds.descriptions_mut() {
            if let Desc::Socket(s) = d
                && let Some(u) = s.unix.as_mut()
            {
                u.shared = true;
                // A channel end is not the child's (no close); a stream end
                // is, at the parent's slot: what the library held of those
                // is closed here, once (INV-0430-2).
                s.ctl = None;
                closing.extend(u.book.after_fork());
                if let Some(h) = u.held.take() {
                    let mut it = h.handles.into_iter();
                    for r in &h.records {
                        let hs: Vec<S::H> = it.by_ref().take(r.handles()).collect();
                        closing.extend(unix::child_keeps(r, &hs));
                    }
                }
            }
        }
        for h in closing {
            self.sys.close(h);
        }
    }
}

//! Socket files (ADR-0412 item 5, plan part F7, proposed ADR-0439): `bind`
//! makes an `S_IFSOCK` node in the line's `/tmp` (the temporary directory
//! service), `listen` gives it a listener channel, `connect` opens the node
//! through the connecting program's own table and hands one end of a new
//! pair to the listener, `accept` takes it. A program that cannot see the
//! directory cannot connect (no ambient authority), and one that sees it
//! read-only cannot either (`WRITE` is needed).
//!
//! The service reads none of what it forwards; every record a listener takes
//! from its channel is checked by `unix::decode_connect`, whoever sent it.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use zoraya_abi::Status;
use zoraya_abi::posix::{Access, EntryKind};
use zoraya_proto_file::{self as proto, open_flags, rights, status};
use zoraya_protocol::{Below, Data, ERROR_ORDINAL, decode};

use crate::errno::{
    self, Call, EACCES, EADDRINUSE, EAGAIN, ECONNABORTED, ECONNREFUSED, EEXIST, EINVAL, EISCONN,
    EMFILE, ENOTCONN, EOPNOTSUPP, EPROTOTYPE,
};
use crate::process::{AT_FDCWD, Desc, Posix, native};
use crate::socket::{Socket, State};
use crate::stat;
use crate::sys::{Sys, signals};
use crate::unix::{self, SOCK_SEQPACKET, SOCK_STREAM};

/// The status of a socket request as an `errno` (a refusal of `connect`,
/// `listen` or `bind`).
fn socket_errno(s: u32) -> i32 {
    match s {
        status::NOT_FOUND | status::UNSUPPORTED => ECONNREFUSED,
        status::LIMIT_REACHED => EAGAIN,
        status::INVALID_ARGUMENT => EPROTOTYPE,
        status::EXISTS => EADDRINUSE,
        s => errno::from_file(s),
    }
}

/// Our side of a new pair and the far end's handles.
type Ends<H> = (Socket<H>, Vec<H>);

impl<S: Sys> Posix<S> {
    /// A new pair's two sides: our socket, still unconnected as a `Socket`
    /// but holding its ends, and the far end's handles in record order (read,
    /// write, ancillary for a stream; the channel for the others). Nothing is
    /// left open on an error.
    fn new_pair_ends(&mut self, kind: i32) -> Result<Ends<S::H>, i32> {
        let mut near = Socket::fresh_unix(kind, false);
        let (c1, c2) = self
            .sys
            .channel_create()
            .map_err(|s| native(s, Call::Descriptor))?;
        near.ctl = Some(c1);
        if kind != SOCK_STREAM {
            return Ok((near, alloc::vec![c2]));
        }
        let one = self.sys.stream_create();
        let two = self.sys.stream_create();
        match (one, two) {
            (Ok(one), Ok(two)) => {
                near.write = Some(one.1);
                near.read = Some(two.0);
                Ok((near, alloc::vec![one.0, two.1, c2]))
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
                Err(e)
            }
        }
    }

    /// `bind(fd, sockaddr_un)`: an exclusive create of an `S_IFSOCK` node in
    /// the line's `/tmp` (INV-0412-3: `EACCES` in any other table entry, the
    /// refusal of a path no entry covers is the resolver's), the connection to
    /// it kept for `listen`.
    pub fn unix_bind(&mut self, fd: i32, addr: &[u8]) -> Result<(), i32> {
        let s = self.unix_socket_for_files(fd)?;
        if s.state != State::Fresh
            || s.unix
                .as_ref()
                .is_some_and(|u| u.node.is_some() || u.name.is_some())
        {
            return Err(EINVAL);
        }
        if s.kind != SOCK_STREAM && s.kind != SOCK_SEQPACKET {
            // A datagram socket would need every sender to reach it: not built.
            return Err(EOPNOTSUPP);
        }
        let path = unix::parse_sockaddr_un(addr)?.to_vec();
        let r = self.resolve(AT_FDCWD, &path, Access::Write)?;
        let entry = self.entries.get(r.place.entry).map(|e| e.kind);
        match entry {
            Some(kind) => unix::bind_place(kind, &r.place.path, r.directory)?,
            None => return Err(EACCES),
        }
        let mode = stat::S_IFSOCK | (0o777 & !self.facts.umask);
        // As `O_CREAT | O_EXCL`: a name that exists is taken, even a link,
        // which is never followed (a link out of `/tmp` could otherwise make
        // the node in another service).
        let wire = open_flags::CREATE | open_flags::EXCLUSIVE | open_flags::NO_FOLLOW;
        let wanted = rights::READ | rights::WRITE;
        let (made, opened) = self
            .open_resolved(r, wire, wanted, 0, mode, Access::Write)
            .map_err(|e| if e == EEXIST { EADDRINUSE } else { e })?;
        let in_tmp = self.entries.get(made.place.entry).map(|e| e.kind) == Some(EntryKind::Tmp);
        if !in_tmp || opened.meta().mode & stat::S_IFMT != stat::S_IFSOCK {
            // A service that keeps no socket nodes made a plain file.
            self.sys.close(opened.conn);
            let _ = self.unlink_at(AT_FDCWD, &path, 0);
            return Err(EACCES);
        }
        let conn = opened.conn;
        if let Some(u) = self.unix_file_mut(fd)?.unix.as_mut() {
            u.node = Some(conn);
            u.name = Some(path);
        } else {
            self.sys.close(conn);
        }
        Ok(())
    }

    /// `listen(fd, backlog)` on a bound socket: the service gets the listener
    /// channel's end. `backlog` is a hint; the queue is the channel's 64
    /// messages (`unix::LISTEN_QUEUE`).
    pub fn unix_listen(&mut self, fd: i32, backlog: i32) -> Result<(), i32> {
        let _ = backlog;
        let s = self.unix_socket_for_files(fd)?;
        if s.state != State::Fresh {
            return Err(EINVAL);
        }
        let Some(u) = s.unix.as_ref() else {
            return Err(EINVAL);
        };
        if u.listening {
            return Ok(());
        }
        let (Some(node), kind) = (u.node, s.kind) else {
            // Not bound: Linux gives `EINVAL` (no listen on an unbound socket).
            return Err(EINVAL);
        };
        if kind != SOCK_STREAM && kind != SOCK_SEQPACKET {
            return Err(EOPNOTSUPP);
        }
        let (mine, theirs) = self
            .sys
            .channel_create()
            .map_err(|s| native(s, Call::Descriptor))?;
        let m = proto::SocketListen {
            sock_type: Below(kind as u32),
        };
        if let Err(e) = self.socket_request(node, &m, &[theirs]) {
            self.sys.close(mine);
            return Err(e);
        }
        let s = self.unix_file_mut(fd)?;
        s.ctl = Some(mine);
        if let Some(u) = s.unix.as_mut() {
            u.listening = true;
        }
        Ok(())
    }

    /// One request of the socket protocol and its `Done`, with the refusal as
    /// an `errno` (`moved` handles are gone whatever happens).
    fn socket_request<'m, M: zoraya_protocol::Message<'m>>(
        &mut self,
        conn: S::H,
        m: &M,
        moved: &[S::H],
    ) -> Result<(), i32> {
        let size = self.call(conn, m, moved)?;
        let bytes = &self.input[..size];
        match zoraya_protocol::Header::decode(bytes) {
            Ok((h, _)) if h.ordinal == ERROR_ORDINAL => {
                return Err(socket_errno(self.status_of(size)));
            }
            Ok(_) => {}
            Err(_) => return Err(errno::EIO),
        }
        match decode::<proto::Done>(bytes, 0) {
            Ok((_, d)) if d.status.0 == status::OK => Ok(()),
            Ok((_, d)) => Err(socket_errno(d.status.0)),
            Err(_) => Err(errno::EIO),
        }
    }

    /// `connect(fd, sockaddr_un)`: the path through this program's own table,
    /// a connection to the node with `WRITE`, and the far end of a new pair
    /// handed to the listener. Queued, not waited for (a full queue is
    /// `EAGAIN`).
    pub fn unix_connect(&mut self, fd: i32, addr: &[u8]) -> Result<(), i32> {
        let s = self.unix_socket_for_files(fd)?;
        if s.state == State::Connected {
            return Err(EISCONN);
        }
        if s.state != State::Fresh || s.unix.as_ref().is_some_and(|u| u.listening) {
            return Err(EINVAL);
        }
        let kind = s.kind;
        if kind != SOCK_STREAM && kind != SOCK_SEQPACKET {
            return Err(EOPNOTSUPP);
        }
        let path = unix::parse_sockaddr_un(addr)?.to_vec();
        let r = self.resolve(AT_FDCWD, &path, Access::Write)?;
        let (_, opened) = self.open_resolved(r, 0, rights::WRITE, 0, 0, Access::Write)?;
        let conn = opened.conn;
        if opened.meta().mode & stat::S_IFMT != stat::S_IFSOCK {
            self.sys.close(conn);
            return Err(ECONNREFUSED);
        }
        let (near, far) = match self.new_pair_ends(kind) {
            Ok(x) => x,
            Err(e) => {
                self.sys.close(conn);
                return Err(e);
            }
        };
        let record = unix::encode_connect(&unix::connect_record(kind));
        let sock_type = Below(kind as u32);
        let sent = if kind == SOCK_STREAM {
            let m = proto::SocketConnectStream {
                sock_type,
                record: Data(&record),
            };
            self.socket_request(conn, &m, &far)
        } else {
            let m = proto::SocketConnect {
                sock_type,
                record: Data(&record),
            };
            self.socket_request(conn, &m, &far)
        };
        self.sys.close(conn);
        if let Err(e) = sent {
            // The far end went with the request (`call` moves it, and the
            // library's write closes what it could not send).
            let mut near = near;
            for h in [near.read.take(), near.write.take(), near.ctl.take()]
                .into_iter()
                .flatten()
            {
                self.sys.close(h);
            }
            return Err(e);
        }
        let s = self.unix_file_mut(fd)?;
        s.read = near.read;
        s.write = near.write;
        s.ctl = near.ctl;
        s.state = State::Connected;
        if let Some(u) = s.unix.as_mut() {
            u.peer_name = Some(path);
        }
        Ok(())
    }

    /// `accept4(fd, flags)` on a listening socket: the next connection, as a
    /// new connected socket whose `getsockname` is the listener's. Waits
    /// unless the socket or the call is non-blocking.
    pub fn unix_accept(&mut self, fd: i32, nonblock: bool, cloexec: bool) -> Result<i32, i32> {
        let s = self.unix_socket_for_files(fd)?;
        let (Some(u), Some(ctl)) = (s.unix.as_ref(), s.ctl) else {
            return Err(EINVAL);
        };
        if !u.listening {
            return Err(EINVAL);
        }
        let (kind, wait) = (s.kind, !s.nonblock);
        let name = u.name.clone();
        // Room first: a connection taken with no fd to hold it would be lost.
        if (zoraya_abi::posix::OPEN_MAX as usize).saturating_sub(self.fds.count()) == 0 {
            return Err(EMFILE);
        }
        loop {
            match self
                .sys
                .channel_read(ctl, &mut self.input, &mut self.handles)
            {
                Ok(size) => {
                    let handles: Vec<S::H> = core::mem::take(&mut self.handles);
                    let Some(r) = unix::decode_connect(&self.input[..size], handles.len(), kind)
                    else {
                        // Not what a connector sends: dropped, closed.
                        for h in handles {
                            self.sys.close(h);
                        }
                        continue;
                    };
                    let mut desc = self.desc_from(r, handles);
                    if let Desc::Socket(sk) = &mut desc {
                        sk.nonblock = nonblock;
                        if let Some(u) = sk.unix.as_mut() {
                            u.name = name;
                        }
                    }
                    return match self.install_desc(desc, cloexec) {
                        Ok(fd) => Ok(fd),
                        Err(e) => Err(e),
                    };
                }
                Err(Status::WouldBlock) => {
                    if !wait {
                        return Err(EAGAIN);
                    }
                    self.wait_point(ctl, signals::READABLE | signals::PEER_CLOSED)?;
                }
                // The service dropped the listener (its name was unlinked
                // and the listener's connection closed from its side).
                Err(Status::PeerClosed) => return Err(ECONNABORTED),
                Err(s) => return Err(native(s, Call::Other)),
            }
        }
    }

    /// `getsockname` and `getpeername` of a Unix socket as a
    /// `struct sockaddr_un`: the path it was bound or connected to, or the
    /// family alone for an unnamed socket.
    pub fn unix_sockaddr(&self, fd: i32, peer: bool) -> Result<Vec<u8>, i32> {
        let s = self.unix_socket_for_files(fd)?;
        if peer && s.state != State::Connected {
            return Err(ENOTCONN);
        }
        let u = s.unix.as_ref();
        let path = if peer {
            u.and_then(|u| u.peer_name.as_deref())
        } else {
            u.and_then(|u| u.name.as_deref())
        };
        Ok(match path {
            Some(p) => unix::sockaddr_un(p),
            None => (unix::AF_UNIX as u16).to_ne_bytes().to_vec(),
        })
    }

    fn unix_socket_for_files(&self, fd: i32) -> Result<&Socket<S::H>, i32> {
        match self.fds.get(fd) {
            Some(Desc::Socket(s)) if s.unix.is_some() => Ok(s),
            Some(_) => Err(errno::ENOTSOCK),
            None => Err(errno::EBADF),
        }
    }

    fn unix_file_mut(&mut self, fd: i32) -> Result<&mut Socket<S::H>, i32> {
        match self.fds.get_mut(fd) {
            Some(Desc::Socket(s)) if s.unix.is_some() => Ok(s),
            Some(_) => Err(errno::ENOTSOCK),
            None => Err(errno::EBADF),
        }
    }
}

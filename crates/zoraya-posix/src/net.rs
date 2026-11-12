//! Socket fds over `netd` (ADR-0225; N4): `socket`, `connect`, reads and
//! writes, `shutdown`, the options and `getaddrinfo`'s `resolve`, on the
//! handles of a connection to `netd` at slot `NET`. A stream socket is the
//! three handles of ADR-0222 item 3's note: the connection channel and the
//! two stream ends the library created, so the 128 KiB are this process's
//! (INV-0262-4). Data never goes through the broker or a request: `read` and
//! `write` are `stream_read` and `stream_write` (INV-0002-3).

#![forbid(unsafe_code)]

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_abi::Status;
use zoraya_proto_net::{Connect, Connected, Connecting, Failed, Reset, Resolve, Resolved, status};
use zoraya_protocol::{Below, Data, Header, decode, encode};

use crate::errno::{
    EACCES, EADDRNOTAVAIL, EALREADY, EBADF, ECONNABORTED, ECONNRESET, EAGAIN, EINPROGRESS,
    EINVAL, EIO, EISCONN, EMFILE, ENETDOWN, ENOTCONN, ENOTSOCK, EOPNOTSUPP, EPIPE, Call,
};
use crate::process::{Desc, Posix, native};
use crate::socket::{self as sock, Hints, Socket, State};
use crate::sys::{Sys, User, signals};

/// What a failed `resolve` was, for `getaddrinfo` (ADR-0225 item 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveError {
    /// One of `socket::EAI_*`.
    pub eai: i32,
    /// `errno` for `EAI_SYSTEM`, else 0.
    pub errno: i32,
}

impl<S: Sys> Posix<S> {
    /// `socket(domain, type, protocol)`: a library object only; it needs no
    /// grant (ADR-0225 item 2).
    pub fn socket(&mut self, domain: i32, ty: i32, protocol: i32) -> Result<i32, i32> {
        let (kind, nonblock, cloexec) = sock::check_socket(domain, ty, protocol)?;
        let desc = Desc::Socket(Box::new(Socket::fresh(kind, nonblock)));
        match self.fds.install(0, desc, cloexec) {
            Ok(fd) => Ok(fd as i32),
            Err(_) => Err(EMFILE),
        }
    }

    fn socket_of(&self, fd: i32) -> Result<&Socket<S::H>, i32> {
        match self.fds.get(fd) {
            Some(Desc::Socket(s)) => Ok(s),
            Some(_) => Err(ENOTSOCK),
            None => Err(EBADF),
        }
    }

    fn socket_mut(&mut self, fd: i32) -> Result<&mut Socket<S::H>, i32> {
        match self.fds.get_mut(fd) {
            Some(Desc::Socket(s)) => Ok(s),
            Some(_) => Err(ENOTSOCK),
            None => Err(EBADF),
        }
    }

    /// One request to `netd` and its reply, in `self.input`: the size.
    /// `moved` handles are gone whatever happens. The reply of a request
    /// that an earlier call gave up on is skipped by its transaction.
    fn net_call<'m, M: zoraya_protocol::Message<'m>>(
        &mut self,
        net: S::H,
        m: &M,
        moved: &[S::H],
    ) -> Result<usize, i32> {
        self.transaction = self.transaction.wrapping_add(1).max(1);
        let t = self.transaction;
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
            .channel_write(net, &self.out[..size], moved)
            .is_err()
        {
            return Err(ENETDOWN);
        }
        for h in self.handles.drain(..) {
            self.sys.close(h);
        }
        loop {
            match self.sys.channel_read(net, &mut self.input, &mut self.handles) {
                Ok(size) => {
                    for h in self.handles.drain(..) {
                        self.sys.close(h);
                    }
                    match Header::decode(&self.input[..size]) {
                        Ok((h, _)) if h.transaction == t => return Ok(size),
                        Ok(_) => continue,
                        Err(_) => return Err(EIO),
                    }
                }
                Err(Status::WouldBlock) => {
                    let s = self
                        .sys
                        .wait(net, signals::READABLE | signals::PEER_CLOSED)
                        .map_err(|_| ENETDOWN)?;
                    if s & signals::READABLE == 0 {
                        return Err(ENETDOWN);
                    }
                }
                Err(_) => return Err(ENETDOWN),
            }
        }
    }

    /// `connect(fd, addr)`: a request to `netd` with the far ends of two
    /// streams, then, unless the socket is non-blocking, the wait for
    /// `Connected` or `Failed` (ADR-0225 items 3 and 11).
    pub fn connect(&mut self, fd: i32, addr: &[u8]) -> Result<(), i32> {
        let (state, nonblock) = {
            let s = self.socket_of(fd)?;
            (s.state, s.nonblock)
        };
        match state {
            State::Fresh => {}
            State::Connecting => return self.finish_connect(fd, !nonblock).map_err(|e| {
                if e == EINPROGRESS { EALREADY } else { e }
            }),
            State::Connected => return Err(EISCONN),
            State::Dead => return Err(ECONNABORTED),
        }
        let (address, port) = sock::parse_sockaddr_in(addr)?;
        if port == 0 {
            return Err(EADDRNOTAVAIL);
        }
        // Without a grant (no slot), the first call that names a destination
        // fails (ADR-0225 item 2). The hint of ADR-0224 rule 5 is not built.
        let Some(net) = self.net else {
            return Err(EACCES);
        };
        // An address that came from a resolution of name N is N's.
        let name: String = self
            .addrs
            .name_of(address)
            .map(String::from)
            .unwrap_or_default();
        // The client creates what the connection holds (ADR-0262 item 1).
        let (control, server) = self
            .sys
            .channel_create()
            .map_err(|s| native(s, Call::Descriptor))?;
        let inbound = match self.sys.stream_create() {
            Ok(p) => p,
            Err(s) => {
                self.sys.close(control);
                self.sys.close(server);
                return Err(native(s, Call::Descriptor));
            }
        };
        let outbound = match self.sys.stream_create() {
            Ok(p) => p,
            Err(s) => {
                for h in [control, server, inbound.0, inbound.1] {
                    self.sys.close(h);
                }
                return Err(native(s, Call::Descriptor));
            }
        };
        let (in_read, in_write) = inbound;
        let (out_read, out_write) = outbound;
        let request = Connect {
            address: if name.is_empty() {
                u32::from_be_bytes(address)
            } else {
                0
            },
            port: Below(u32::from(port)),
            name: Data(name.as_bytes()),
        };
        let ours = [control, in_read, out_write];
        let reply = self.net_call(net, &request, &[server, in_write, out_read]);
        let code = match reply {
            Ok(size) => decode::<Connecting>(&self.input[..size], 0).map(|(_, c)| c.status.0),
            Err(e) => {
                for h in ours {
                    self.sys.close(h);
                }
                return Err(e);
            }
        };
        match code {
            Ok(status::OK) => {}
            Ok(other) => {
                for h in ours {
                    self.sys.close(h);
                }
                return Err(sock::connect_status(other));
            }
            Err(_) => {
                for h in ours {
                    self.sys.close(h);
                }
                return Err(EIO);
            }
        }
        let s = self.socket_mut(fd)?;
        s.read = Some(in_read);
        s.write = Some(out_write);
        s.ctl = Some(control);
        s.peer = Some((address, port));
        s.state = State::Connecting;
        if nonblock {
            return Err(EINPROGRESS);
        }
        self.finish_connect(fd, true)
    }

    /// Takes the connection channel's messages for a connecting socket:
    /// `Connected` makes it connected, `Failed` makes it dead with the
    /// `errno` of its reason. `block`: wait for one (`EINTR` if a signal
    /// comes first; the connection goes on). `EINPROGRESS` if there is none.
    pub fn finish_connect(&mut self, fd: i32, block: bool) -> Result<(), i32> {
        loop {
            let s = self.socket_of(fd)?;
            if s.state != State::Connecting {
                return match s.state {
                    State::Connected => Ok(()),
                    _ => Err(ECONNABORTED),
                };
            }
            let Some(ctl) = s.ctl else {
                return Err(EIO);
            };
            match self.sys.channel_read(ctl, &mut self.input, &mut self.handles) {
                Ok(size) => {
                    for h in self.handles.drain(..) {
                        self.sys.close(h);
                    }
                    let bytes = &self.input[..size];
                    match Header::decode(bytes).map(|(h, _)| h.ordinal) {
                        Ok(16) => {
                            let peer = decode::<Connected>(bytes, 0)
                                .map(|(_, c)| (c.address.to_be_bytes(), c.port.0 as u16));
                            let s = self.socket_mut(fd)?;
                            if let Ok(p) = peer {
                                s.peer = Some(p);
                            }
                            s.state = State::Connected;
                            return Ok(());
                        }
                        Ok(17) => {
                            let e = decode::<Failed>(bytes, 0)
                                .map_or(EIO, |(_, f)| sock::failed_reason(f.reason.0));
                            self.kill_socket(fd);
                            return Err(e);
                        }
                        Ok(18) => {
                            let e = decode::<Reset>(bytes, 0)
                                .map_or(ECONNRESET, |(_, r)| sock::reset_why(r.why.0));
                            self.kill_socket(fd);
                            return Err(e);
                        }
                        _ => continue,
                    }
                }
                Err(Status::WouldBlock) => {
                    if !block {
                        return Err(EINPROGRESS);
                    }
                    let s = self.wait_point(ctl, signals::READABLE | signals::PEER_CLOSED)?;
                    if s & signals::READABLE == 0 && s & signals::PEER_CLOSED != 0 {
                        // `netd` is gone with no word.
                        self.kill_socket(fd);
                        return Err(ECONNRESET);
                    }
                }
                Err(_) => {
                    self.kill_socket(fd);
                    return Err(ECONNRESET);
                }
            }
        }
    }

    /// Closes a socket's handles; the fd stays, dead.
    fn kill_socket(&mut self, fd: i32) {
        let handles = match self.socket_mut(fd) {
            Ok(s) => {
                s.state = State::Dead;
                [s.read.take(), s.write.take(), s.ctl.take()]
            }
            Err(_) => return,
        };
        for h in handles.into_iter().flatten() {
            self.sys.close(h);
        }
    }

    /// Looks at the connection channel of a connected socket for a `Reset`
    /// (ADR-0225 item 4): its `errno`, once.
    fn take_reset(&mut self, fd: i32) -> Option<i32> {
        let ctl = self.socket_of(fd).ok()?.ctl?;
        while let Ok(size) = self.sys.channel_read(ctl, &mut self.input, &mut self.handles) {
            for h in self.handles.drain(..) {
                self.sys.close(h);
            }
            let bytes = &self.input[..size];
            if Header::decode(bytes).is_ok_and(|(h, _)| h.ordinal == 18) {
                return Some(
                    decode::<Reset>(bytes, 0).map_or(ECONNRESET, |(_, r)| sock::reset_why(r.why.0)),
                );
            }
        }
        None
    }

    /// `read`, `recv` of a socket: `dontwait` is `MSG_DONTWAIT`.
    pub fn sock_read(&mut self, fd: i32, into: User, dontwait: bool) -> Result<usize, i32> {
        let (state, nonblock) = {
            let s = self.socket_of(fd)?;
            (s.state, s.nonblock || dontwait)
        };
        match state {
            State::Fresh | State::Dead => {
                let s = self.socket_mut(fd)?;
                let pending = core::mem::take(&mut s.error);
                return Err(if pending != 0 { pending } else { ENOTCONN });
            }
            State::Connecting => match self.finish_connect(fd, !nonblock) {
                Ok(()) => {}
                Err(EINPROGRESS) => return Err(EAGAIN),
                Err(e) => return Err(e),
            },
            State::Connected => {}
        }
        let (read, pending) = {
            let s = self.socket_mut(fd)?;
            (s.read, core::mem::take(&mut s.error))
        };
        if pending != 0 {
            return Err(pending);
        }
        let Some(end) = read else {
            // `shutdown(SHUT_RD)`.
            return Ok(0);
        };
        if into.length == 0 {
            return Ok(0);
        }
        match self.read_stream(end, into, nonblock) {
            // The end of the stream: a reset says why, once (the data came
            // first); otherwise the peer closed.
            Ok(0) => match self.take_reset(fd) {
                Some(e) => Err(e),
                None => Ok(0),
            },
            other => other,
        }
    }

    /// `write`, `send` of a socket.
    pub fn sock_write(&mut self, fd: i32, from: User, dontwait: bool) -> Result<usize, i32> {
        let (state, nonblock) = {
            let s = self.socket_of(fd)?;
            (s.state, s.nonblock || dontwait)
        };
        match state {
            State::Fresh => return Err(ENOTCONN),
            State::Dead => {
                let s = self.socket_mut(fd)?;
                let pending = core::mem::take(&mut s.error);
                return Err(if pending != 0 { pending } else { EPIPE });
            }
            State::Connecting => match self.finish_connect(fd, !nonblock) {
                Ok(()) => {}
                Err(EINPROGRESS) => return Err(EAGAIN),
                Err(e) => return Err(e),
            },
            State::Connected => {}
        }
        let (write, pending) = {
            let s = self.socket_mut(fd)?;
            (s.write, core::mem::take(&mut s.error))
        };
        if pending != 0 {
            return Err(pending);
        }
        // `shutdown(SHUT_WR)` leaves none: the writer is gone.
        let Some(end) = write else {
            return Err(EPIPE);
        };
        match self.write_stream(end, from, nonblock) {
            Err(EPIPE) => Err(self.take_reset(fd).unwrap_or(EPIPE)),
            other => other,
        }
    }

    /// `shutdown(fd, how)`: closing the write end is the FIN (`netd` sends
    /// it once the ring is dry); closing the read end ends reads.
    pub fn shutdown(&mut self, fd: i32, how: i32) -> Result<(), i32> {
        if !(sock::SHUT_RD..=sock::SHUT_RDWR).contains(&how) {
            return Err(EINVAL);
        }
        let s = self.socket_mut(fd)?;
        if s.state != State::Connected {
            return Err(ENOTCONN);
        }
        let mut gone = [None, None];
        if how == sock::SHUT_RD || how == sock::SHUT_RDWR {
            gone[0] = s.read.take();
        }
        if how == sock::SHUT_WR || how == sock::SHUT_RDWR {
            gone[1] = s.write.take();
        }
        for h in gone.into_iter().flatten() {
            self.sys.close(h);
        }
        Ok(())
    }

    /// `setsockopt` with an integer value.
    pub fn setsockopt(&mut self, fd: i32, level: i32, name: i32, value: i32) -> Result<(), i32> {
        let s = self.socket_mut(fd)?;
        sock::set_option(&mut s.options, level, name, value)
    }

    /// `getsockopt` with an integer value. `SO_ERROR` finishes a connect
    /// that is waiting (the answer to `poll`'s `POLLOUT`) and clears the
    /// pending error.
    pub fn getsockopt(&mut self, fd: i32, level: i32, name: i32) -> Result<i32, i32> {
        if (level, name) == (sock::SOL_SOCKET, sock::SO_ERROR) {
            let mut e = 0;
            if self.socket_of(fd)?.state == State::Connecting {
                e = match self.finish_connect(fd, false) {
                    Ok(()) | Err(EINPROGRESS) => 0,
                    Err(e) => e,
                };
            }
            let s = self.socket_mut(fd)?;
            let pending = core::mem::take(&mut s.error);
            return Ok(if e != 0 { e } else { pending });
        }
        let s = self.socket_of(fd)?;
        sock::get_option(&s.options, s.kind, false, level, name)
    }

    /// `getpeername`.
    pub fn getpeername(&self, fd: i32) -> Result<([u8; 4], u16), i32> {
        let s = self.socket_of(fd)?;
        match (s.state, s.peer) {
            (State::Connected, Some(p)) => Ok(p),
            _ => Err(ENOTCONN),
        }
    }

    /// `getsockname`: `netd` has no `local()` yet, so an unbound socket is
    /// the wildcard address and port 0 (open item of N4).
    pub fn getsockname(&self, fd: i32) -> Result<([u8; 4], u16), i32> {
        self.socket_of(fd)?;
        Ok(([0; 4], 0))
    }

    /// `MSG_PEEK` and `MSG_OOB` are not offered (ADR-0225 item 4).
    pub fn check_msg_flags(flags: i32) -> Result<(), i32> {
        if flags & (sock::MSG_PEEK | sock::MSG_OOB) != 0 {
            return Err(EOPNOTSUPP);
        }
        Ok(())
    }

    /// The addresses of `name`: one `resolve` request on `NET` (ADR-0225
    /// item 6, INV-0225-3), the results remembered for `connect`.
    pub fn resolve_name(&mut self, name: &str) -> Result<Vec<[u8; 4]>, ResolveError> {
        let Some(net) = self.net else {
            return Err(ResolveError {
                eai: sock::EAI_SYSTEM,
                errno: EACCES,
            });
        };
        let size = self
            .net_call(
                net,
                &Resolve {
                    name: Data(name.as_bytes()),
                },
                &[],
            )
            .map_err(|e| ResolveError {
                eai: sock::EAI_SYSTEM,
                errno: e,
            })?;
        let answer = decode::<Resolved>(&self.input[..size], 0).map_err(|_| ResolveError {
            eai: sock::EAI_FAIL,
            errno: 0,
        })?;
        let (_, r) = answer;
        if r.status.0 != status::OK {
            let (eai, errno) = sock::resolve_status(r.status.0);
            return Err(ResolveError { eai, errno });
        }
        let found: Vec<[u8; 4]> = r.addresses.iter().map(|a| a.address.to_be_bytes()).collect();
        if found.is_empty() {
            return Err(ResolveError {
                eai: sock::EAI_NONAME,
                errno: 0,
            });
        }
        for a in &found {
            self.addrs.remember(*a, name);
        }
        Ok(found)
    }

    /// `getaddrinfo`'s addresses: the plan's, asking `netd` only for a name.
    pub fn lookup(
        &mut self,
        node: Option<&[u8]>,
        service: Option<&[u8]>,
        hints: &Hints,
    ) -> Result<(Vec<[u8; 4]>, u16, Option<String>), ResolveError> {
        let plan = sock::plan(node, service, hints).map_err(|eai| ResolveError { eai, errno: 0 })?;
        match plan.host {
            sock::Host::Fixed(a) => Ok((alloc::vec![a], plan.port, None)),
            sock::Host::Name(n) => {
                let found = self.resolve_name(&n)?;
                Ok((found, plan.port, plan.canonical.then_some(n)))
            }
        }
    }
}

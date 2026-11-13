//! TCP client sockets on the stack (ADR-0222 item 3, N3): open, move bytes
//! with the closures smoltcp offers (so a byte is copied once between its
//! buffer and the stream ring, with no staging buffer here), half close,
//! abort. The state is a handful of indexes into the socket set; the policy
//! (what to do on a reset, when to give up) is in `conn`.

use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp::{self, RecvError, State};
use smoltcp::time::Duration;
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address};

use crate::dns::Resolver;
use crate::link::Link;
use crate::stack::Stack;

/// TCP sockets at once (each holds two buffers of [`TCP_BUFFER`]).
pub const MAX_TCP: usize = 8;
/// The size of each socket buffer; the receive buffer is the advertised
/// window's ceiling, and the stream ring of ADR-0101 (64 KiB) comes on top.
pub const TCP_BUFFER: usize = 32 * 1024;
/// How long a connection may go with data unacknowledged before smoltcp
/// closes it (the `Reset` with `TIMED_OUT`).
const TIMEOUT: Duration = Duration::from_secs(60);
/// First ephemeral port.
const PORT_FIRST: u16 = 49152;

/// A TCP socket of the stack; an index that stays valid until
/// [`Stack::tcp_release`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpError {
    /// The interface has no address.
    NoAddress,
    /// [`MAX_TCP`] sockets exist.
    TooMany,
    /// A port of zero or an address that is not a host.
    Unaddressable,
}

/// Where a socket is in its life, reduced to what `conn` decides on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpPhase {
    /// The SYN is out.
    Connecting,
    /// Both directions may be open (or closing, see `may_*`).
    Open,
    /// Not open any more (closed, reset, or waiting out the end).
    Ended,
}

/// What a read from the socket found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpRecv {
    /// The closure took this many bytes (possibly none).
    Got(usize),
    /// The peer closed its side after all the data; nothing more will come.
    Finished,
    /// The socket was reset or never opened: unread data is gone.
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpSend {
    Sent(usize),
    /// Our side is closed or the socket is not open.
    Closed,
}

/// The stack's TCP table and resolver.
#[derive(Default)]
pub struct Extra {
    sockets: Vec<Option<SocketHandle>>,
    /// Released sockets waiting for their reset to go out, and whether a
    /// `tcp_reap` has passed since.
    doomed: Vec<(usize, bool)>,
    /// The next port to try; 0 until the first socket (or `seed_ports`).
    next_port: u16,
    pub(crate) resolver: Resolver,
}

impl<L: Link> Stack<L> {
    fn tcp_socket(&mut self, id: TcpId) -> Option<&mut tcp::Socket<'static>> {
        let handle = (*self.extra.sockets.get(id.0)?)?;
        Some(self.sockets.get_mut::<tcp::Socket>(handle))
    }

    fn tcp_socket_ref(&self, id: TcpId) -> Option<&tcp::Socket<'static>> {
        let handle = (*self.extra.sockets.get(id.0)?)?;
        Some(self.sockets.get::<tcp::Socket>(handle))
    }

    /// How many TCP sockets exist.
    pub fn tcp_count(&self) -> usize {
        self.extra.sockets.iter().flatten().count()
    }

    /// Picks the first ephemeral port from `seed` (RFC 6056, randomized
    /// selection), so that a restarted stack does not reuse the four-tuple of
    /// its predecessor's connections, which a NAT may still hold.
    pub(crate) fn seed_ports(&mut self, seed: u64) {
        let range = u64::from(u16::MAX - PORT_FIRST) + 1;
        self.extra.next_port = PORT_FIRST + (seed % range) as u16;
    }

    pub(crate) fn ephemeral_port(&mut self) -> u16 {
        loop {
            let p = self.extra.next_port.max(PORT_FIRST);
            self.extra.next_port = if p == u16::MAX { PORT_FIRST } else { p + 1 };
            let used = self.extra.sockets.iter().flatten().any(|h| {
                self.sockets
                    .get::<tcp::Socket>(*h)
                    .local_endpoint()
                    .map(|e| e.port)
                    == Some(p)
            });
            if !used {
                return p;
            }
        }
    }

    /// Starts a connection to `to`:`port`.
    pub fn tcp_open(&mut self, to: [u8; 4], port: u16) -> Result<TcpId, TcpError> {
        if self.address().is_none() {
            return Err(TcpError::NoAddress);
        }
        if port == 0 || !crate::config::is_unicast(to) {
            return Err(TcpError::Unaddressable);
        }
        let slot = match self.extra.sockets.iter().position(Option::is_none) {
            Some(i) => i,
            None if self.extra.sockets.len() < MAX_TCP => {
                self.extra.sockets.push(None);
                self.extra.sockets.len() - 1
            }
            None => return Err(TcpError::TooMany),
        };
        let local = self.ephemeral_port();
        let mut socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0u8; TCP_BUFFER]),
            tcp::SocketBuffer::new(vec![0u8; TCP_BUFFER]),
        );
        socket.set_timeout(Some(TIMEOUT));
        let remote = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from(to)), port);
        if socket.connect(self.iface.context(), remote, local).is_err() {
            return Err(TcpError::Unaddressable);
        }
        let handle = self.sockets.add(socket);
        self.extra.sockets[slot] = Some(handle);
        Ok(TcpId(slot))
    }

    pub fn tcp_phase(&self, id: TcpId) -> TcpPhase {
        let Some(s) = self.tcp_socket_ref(id) else {
            return TcpPhase::Ended;
        };
        match s.state() {
            State::SynSent | State::SynReceived => TcpPhase::Connecting,
            State::Closed | State::TimeWait | State::Listen => TcpPhase::Ended,
            _ => TcpPhase::Open,
        }
    }

    /// Whether our side can still send (not closed by us, not reset).
    pub fn tcp_may_send(&self, id: TcpId) -> bool {
        self.tcp_socket_ref(id).is_some_and(|s| s.may_send())
    }

    /// Whether the receive buffer holds bytes.
    pub fn tcp_can_recv(&self, id: TcpId) -> bool {
        self.tcp_socket_ref(id).is_some_and(|s| s.can_recv())
    }

    /// Free room in the transmit buffer.
    pub fn tcp_send_room(&self, id: TcpId) -> usize {
        self.tcp_socket_ref(id)
            .map_or(0, |s| s.send_capacity() - s.send_queue())
    }

    /// Bytes sent and not yet acknowledged, or not yet sent.
    pub fn tcp_unsent(&self, id: TcpId) -> usize {
        self.tcp_socket_ref(id).map_or(0, |s| s.send_queue())
    }

    /// Offers the contiguous bytes at the head of the receive buffer to `f`,
    /// which returns how many it took; those are removed.
    pub fn tcp_recv(&mut self, id: TcpId, f: impl FnOnce(&[u8]) -> usize) -> TcpRecv {
        let Some(s) = self.tcp_socket(id) else {
            return TcpRecv::Reset;
        };
        match s.recv(|data| {
            let n = f(data).min(data.len());
            (n, n)
        }) {
            Ok(n) => TcpRecv::Got(n),
            Err(RecvError::Finished) => TcpRecv::Finished,
            Err(RecvError::InvalidState) => TcpRecv::Reset,
        }
    }

    /// Offers the free space at the end of the transmit buffer to `f`, which
    /// fills a prefix of it and returns its length; those bytes are queued.
    pub fn tcp_send(&mut self, id: TcpId, f: impl FnOnce(&mut [u8]) -> usize) -> TcpSend {
        let Some(s) = self.tcp_socket(id) else {
            return TcpSend::Closed;
        };
        match s.send(|space| {
            let n = f(space).min(space.len());
            (n, n)
        }) {
            Ok(n) => TcpSend::Sent(n),
            Err(_) => TcpSend::Closed,
        }
    }

    /// Our side is done: a FIN follows the queued bytes.
    pub fn tcp_close_write(&mut self, id: TcpId) {
        if let Some(s) = self.tcp_socket(id) {
            s.close();
        }
    }

    /// Drops the connection with a reset.
    pub fn tcp_abort(&mut self, id: TcpId) {
        if let Some(s) = self.tcp_socket(id) {
            s.abort();
        }
    }

    /// Gives the socket up: it is aborted if it is open (the peer hears of it
    /// with a reset) and removed by the second [`Stack::tcp_reap`] after this
    /// call, so that one poll between them sends the reset. It counts against
    /// [`MAX_TCP`] until then.
    pub fn tcp_release(&mut self, id: TcpId) {
        if self.extra.doomed.iter().any(|(i, _)| *i == id.0) {
            return;
        }
        if let Some(s) = self.tcp_socket(id) {
            s.abort();
            self.extra.doomed.push((id.0, false));
        }
    }

    /// Removes the sockets released before the previous call.
    pub fn tcp_reap(&mut self) {
        let mut keep = Vec::new();
        for (i, armed) in core::mem::take(&mut self.extra.doomed) {
            if armed {
                if let Some(slot) = self.extra.sockets.get_mut(i)
                    && let Some(handle) = slot.take()
                {
                    self.sockets.remove(handle);
                }
            } else {
                keep.push((i, true));
            }
        }
        self.extra.doomed = keep;
    }

    /// The peer of a socket, if it has one.
    pub fn tcp_peer(&self, id: TcpId) -> Option<([u8; 4], u16)> {
        let e = self.tcp_socket_ref(id)?.remote_endpoint()?;
        let IpAddress::Ipv4(a) = e.addr;
        Some((a.octets(), e.port))
    }
}

//! Client TCP connections (ADR-0222 item 3 with the client's stream ends,
//! its note of 2026-11-10; proposed ADR-0286): from the `connect` request to
//! the end, as a state machine `netd` drives once per turn.
//!
//! A connection resolves its name (or takes its address), opens a TCP socket,
//! tells the client `Connected` or `Failed` on the connection channel, then
//! copies bytes between smoltcp's buffers and the client's two stream ends.
//! The copy uses smoltcp's closures on one side and non-blocking stream calls
//! on the other, so a byte is copied once here and never staged; the
//! backpressure is the stream's and the socket's own: a full inbound ring
//! stops reading the socket (the window closes), an empty outbound ring stops
//! sending. The client's `shutdown(write)` (closing the write end it keeps) is
//! a FIN after the ring drains; the peer's FIN is end of file after the data
//! (`close_inbound`); a reset is a `Reset` message and both ends closed.
//!
//! What this module does not touch is the handles: [`Client`] is the
//! client's side as `netd` holds it (or a fake in the host tests).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use zoraya_abi::net::{Protocols, Scope};
use zoraya_proto_net::{Connected, Failed, Reset, reason, reset};
use zoraya_protocol::{Below, NO_REPLY, encode};

use crate::config::is_unicast;
use crate::dns::{Found, Owner, ResolveFail, Resolved, Start};
use crate::link::Link;
use crate::stack::Stack;
use crate::tcp::{TcpError, TcpId, TcpPhase, TcpRecv, TcpSend};

/// The first resolver token of a `resolve` request: below it a token is a
/// connection's slot.
pub const LOOKUP_BASE: u32 = 1 << 16;
/// Connections in any phase at once.
pub const MAX_CONNS: usize = 16;
/// How long one address gets to answer the SYN (smoltcp retries inside it):
/// the protocol's `zoraya_proto_net::CONNECT_ATTEMPT_US`, which clients wait by.
pub const CONNECT_US: i64 = zoraya_proto_net::CONNECT_ATTEMPT_US as i64;
/// How long a half-closed connection (one side's end of file seen) may go
/// without a byte moving before `netd` ends it with `Reset { Aborted }`: a
/// peer or a client that closes one side and then says nothing cannot hold a
/// socket for ever (the 8 sockets are global). 30 s; smoltcp's own idle timeout of 60 s ends any connection silent both ways, this one is the earlier bound for a half-closed one.
pub const LINGER_US: i64 = 30_000_000;
/// Rounds of copying per direction in one step, so one fast connection
/// cannot hold the loop of `netd` (the next turn continues).
const ROUNDS: usize = 16;

/// A read from the client's outbound stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rd {
    Data(usize),
    /// Nothing now; the client still holds the write end.
    Empty,
    /// The ring is empty and the client closed its write end.
    Eof,
}

/// A write to the client's inbound stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wr {
    Wrote(usize),
    Full,
    /// The client closed its read end.
    Gone,
}

/// The client's side of one connection, as the service holds it.
pub trait Client {
    /// Reads what the client wrote, at most `buf.len()`.
    fn read_outbound(&mut self, buf: &mut [u8]) -> Rd;
    /// Writes what the peer sent. A partial count is allowed.
    fn write_inbound(&mut self, data: &[u8]) -> Wr;
    /// Ends the inbound stream: the client reads the rest, then 0.
    fn close_inbound(&mut self);
    /// A message on the connection channel; `false` if the channel is gone.
    fn notify(&mut self, message: &[u8]) -> bool;
}

/// What a connection was asked to reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Name(String),
    Address([u8; 4]),
}

enum Phase {
    /// A query is out for the connection's token.
    Resolving,
    /// A SYN is out to `candidates[next - 1]`.
    Connecting {
        tcp: TcpId,
        deadline: i64,
    },
    Open(Open),
    Finished,
}

struct Open {
    tcp: TcpId,
    /// The client's write end is closed and our FIN is queued.
    out_eof: bool,
    /// The peer's FIN was seen, and the client was told.
    in_eof: bool,
    /// When bytes last moved or the state last changed, for the linger
    /// limit of a half-closed connection.
    last_activity: i64,
}

pub struct Conn<C> {
    client: C,
    /// The account charged for it (ADR-0262; N4).
    owner: Owner,
    phase: Phase,
    port: u16,
    /// The scope the request was checked against, to judge local addresses.
    scope: Vec<u8>,
    candidates: Vec<[u8; 4]>,
    next: usize,
    /// The candidates came from `net.hosts`.
    from_hosts: bool,
    /// Some candidate was dropped as local.
    saw_local: bool,
    last_failure: u32,
}

/// What one step found, for the loop of `netd`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Step {
    /// Bytes moved or state changed: the interface has something to send.
    pub progress: bool,
    /// Waiting for the outbound stream to have bytes.
    pub wants_out: bool,
    /// Waiting for the inbound stream to have room.
    pub wants_in: bool,
    /// The connection is over; its client can be dropped.
    pub done: bool,
    /// Come back in this many microseconds (a connect deadline).
    pub delay_us: Option<i64>,
}

fn message<'a, M: zoraya_protocol::Message<'a>>(m: &M) -> Vec<u8> {
    let mut out = vec![0u8; 64];
    // Fixed-size messages of a few bytes.
    let n = encode(m, NO_REPLY, &mut out).unwrap_or(0);
    out.truncate(n);
    out
}

/// Whether `a` is an address of this machine or its side of the network
/// (ADR-0223 item 4).
pub(crate) fn is_local<L: Link>(stack: &Stack<L>, a: [u8; 4], now_us: i64) -> bool {
    if !is_unicast(a) {
        return true;
    }
    let s = stack.status(now_us);
    a == s.address || a == s.gateway || s.dns.contains(&a)
}

impl<C: Client> Conn<C> {
    fn fail(&mut self, why: u32) -> Step {
        let sent = self.client.notify(&message(&Failed { reason: Below(why) }));
        let _ = sent;
        self.client.close_inbound();
        self.phase = Phase::Finished;
        Step {
            progress: true,
            done: true,
            ..Step::default()
        }
    }

    /// Opens the socket to the next candidate that may be tried.
    fn next_candidate<L: Link>(&mut self, stack: &mut Stack<L>, now_us: i64) -> Option<Step> {
        let scope = Scope::decode(&self.scope).unwrap_or(Scope::EMPTY);
        while self.next < self.candidates.len() {
            let a = self.candidates[self.next];
            self.next += 1;
            // ADR-0223 item 4: an answer that is local is refused unless an
            // address rule covers it; `net.hosts` is the person's own word.
            if is_local(stack, a, now_us)
                && !self.from_hosts
                && !scope.allows_address(a, self.port, Protocols::TCP)
            {
                self.saw_local = true;
                continue;
            }
            if Some(a) == stack.address() {
                self.last_failure = reason::UNREACHABLE;
                continue;
            }
            match stack.tcp_open(a, self.port) {
                Ok(tcp) => {
                    self.phase = Phase::Connecting {
                        tcp,
                        deadline: now_us + CONNECT_US,
                    };
                    return Some(Step {
                        progress: true,
                        delay_us: Some(CONNECT_US),
                        ..Step::default()
                    });
                }
                Err(TcpError::TooMany) => {
                    self.last_failure = reason::OVER_LIMIT;
                    break;
                }
                Err(_) => self.last_failure = reason::UNREACHABLE,
            }
        }
        None
    }

    fn all_tried(&mut self) -> Step {
        let why = if self.last_failure != reason::UNRESOLVABLE {
            self.last_failure
        } else if self.saw_local {
            reason::LOCAL
        } else {
            reason::UNRESOLVABLE
        };
        self.fail(why)
    }

    fn resolved<L: Link>(
        &mut self,
        stack: &mut Stack<L>,
        found: Result<Found, ResolveFail>,
        now_us: i64,
    ) -> Step {
        match found {
            Err(ResolveFail::Busy) => self.fail(reason::OVER_LIMIT),
            Err(ResolveFail::TimedOut) => self.fail(reason::LOOKUP_TIMED_OUT),
            // No server to ask is a network problem, not a name without an
            // address (ADR-0316).
            Err(ResolveFail::NoServer) => self.fail(reason::UNREACHABLE),
            Err(_) => self.fail(reason::UNRESOLVABLE),
            Ok(found) => {
                self.candidates = found.addresses;
                self.from_hosts = found.from_hosts;
                self.next = 0;
                self.next_candidate(stack, now_us)
                    .unwrap_or_else(|| self.all_tried())
            }
        }
    }

    /// Moves the connection as far as it goes now. `resolved` is the result
    /// of this connection's query, if one came in.
    fn step<L: Link>(
        &mut self,
        stack: &mut Stack<L>,
        resolved: Option<Result<Found, ResolveFail>>,
        now_us: i64,
    ) -> Step {
        match &mut self.phase {
            Phase::Finished => Step {
                done: true,
                ..Step::default()
            },
            Phase::Resolving => match resolved {
                Some(found) => self.resolved(stack, found, now_us),
                None => Step::default(),
            },
            Phase::Connecting { tcp, deadline } => {
                let (tcp, deadline) = (*tcp, *deadline);
                match stack.tcp_phase(tcp) {
                    TcpPhase::Open => {
                        let peer = stack.tcp_peer(tcp).unwrap_or(([0; 4], self.port));
                        let ok = self.client.notify(&message(&Connected {
                            address: u32::from_be_bytes(peer.0),
                            port: Below(u32::from(peer.1)),
                        }));
                        if !ok {
                            // Nobody to tell: the client gave up.
                            stack.tcp_release(tcp);
                            self.phase = Phase::Finished;
                            return Step {
                                progress: true,
                                done: true,
                                ..Step::default()
                            };
                        }
                        self.phase = Phase::Open(Open {
                            tcp,
                            out_eof: false,
                            in_eof: false,
                            last_activity: now_us,
                        });
                        let mut s = self.step(stack, None, now_us);
                        s.progress = true;
                        s
                    }
                    TcpPhase::Ended => {
                        stack.tcp_release(tcp);
                        self.last_failure = reason::REFUSED;
                        self.try_next_or_fail(stack, now_us)
                    }
                    TcpPhase::Connecting if now_us >= deadline => {
                        stack.tcp_release(tcp);
                        self.last_failure = reason::TIMED_OUT;
                        self.try_next_or_fail(stack, now_us)
                    }
                    TcpPhase::Connecting => Step {
                        delay_us: Some((deadline - now_us).max(0)),
                        ..Step::default()
                    },
                }
            }
            Phase::Open(_) => self.copy(stack, now_us),
        }
    }

    fn try_next_or_fail<L: Link>(&mut self, stack: &mut Stack<L>, now_us: i64) -> Step {
        match self.next_candidate(stack, now_us) {
            Some(s) => s,
            None => self.all_tried(),
        }
    }

    fn copy<L: Link>(&mut self, stack: &mut Stack<L>, now_us: i64) -> Step {
        let Phase::Open(open) = &mut self.phase else {
            return Step::default();
        };
        let tcp = open.tcp;
        let mut step = Step::default();
        // Client to network.
        if !open.out_eof {
            for _ in 0..ROUNDS {
                if !stack.tcp_may_send(tcp) {
                    break;
                }
                if stack.tcp_send_room(tcp) == 0 {
                    // Acknowledgements make room; they arrive as frames.
                    break;
                }
                let mut rd = Rd::Empty;
                let client = &mut self.client;
                let sent = stack.tcp_send(tcp, |space| {
                    rd = client.read_outbound(space);
                    match rd {
                        Rd::Data(n) => n.min(space.len()),
                        _ => 0,
                    }
                });
                match (sent, rd) {
                    (TcpSend::Sent(n), Rd::Data(_)) if n > 0 => step.progress = true,
                    (TcpSend::Sent(_), Rd::Eof) => {
                        open.out_eof = true;
                        stack.tcp_close_write(tcp);
                        step.progress = true;
                        break;
                    }
                    (TcpSend::Sent(_), Rd::Empty) => {
                        step.wants_out = true;
                        break;
                    }
                    _ => break,
                }
            }
        }
        // Network to client.
        let mut reset_seen = false;
        if !open.in_eof {
            for _ in 0..ROUNDS {
                let mut full = false;
                let mut gone = false;
                let client = &mut self.client;
                match stack.tcp_recv(tcp, |data| match client.write_inbound(data) {
                    Wr::Wrote(n) => n.min(data.len()),
                    Wr::Full => {
                        full = true;
                        0
                    }
                    Wr::Gone => {
                        gone = true;
                        0
                    }
                }) {
                    TcpRecv::Got(n) if n > 0 => step.progress = true,
                    TcpRecv::Got(_) => {
                        if gone {
                            // The client stopped reading: nothing more is
                            // wanted from this connection.
                            stack.tcp_release(tcp);
                            self.phase = Phase::Finished;
                            return Step {
                                progress: true,
                                done: true,
                                ..Step::default()
                            };
                        }
                        step.wants_in = full;
                        break;
                    }
                    TcpRecv::Finished => {
                        open.in_eof = true;
                        self.client.close_inbound();
                        step.progress = true;
                        break;
                    }
                    TcpRecv::Reset => {
                        reset_seen = true;
                        break;
                    }
                }
            }
        }
        if reset_seen {
            return self.reset_and_finish(stack, tcp, reset::PEER);
        }
        let Phase::Open(open) = &mut self.phase else {
            return step;
        };
        if step.progress {
            open.last_activity = now_us;
        }
        let ended = stack.tcp_phase(tcp) == TcpPhase::Ended;
        // Both directions over and the socket has left the connection.
        if open.out_eof && open.in_eof && ended {
            stack.tcp_release(tcp);
            self.phase = Phase::Finished;
            step.done = true;
            step.progress = true;
            return step;
        }
        // The socket is gone and we never closed our side: the peer reset it
        // after its FIN (the receive loop no longer looks once `in_eof` is
        // set), or smoltcp's timeout closed it. The client must hear of it.
        if ended && open.in_eof && !open.out_eof {
            return self.reset_and_finish(stack, tcp, reset::PEER);
        }
        // Half closed and silent for too long.
        if open.out_eof || open.in_eof {
            let idle = now_us - open.last_activity;
            if idle >= LINGER_US {
                stack.tcp_abort(tcp);
                return self.reset_and_finish(stack, tcp, reset::ABORTED);
            }
            step.delay_us = Some(LINGER_US - idle);
        }
        step
    }

    /// Tells the client the connection ended badly, closes the inbound end,
    /// gives the socket up and finishes (INV-0286-1).
    fn reset_and_finish<L: Link>(&mut self, stack: &mut Stack<L>, tcp: TcpId, why: u32) -> Step {
        // smoltcp does not say whether a timeout or a reset closed the
        // socket, so `reset::TIMED_OUT` stays unused; the linger limit sends
        // `reset::ABORTED`.
        let _ = self.client.notify(&message(&Reset { why: Below(why) }));
        self.client.close_inbound();
        stack.tcp_release(tcp);
        self.phase = Phase::Finished;
        Step {
            progress: true,
            done: true,
            ..Step::default()
        }
    }
}

/// The table of connections, with the resolver's tokens: token = slot.
pub struct Conns<C> {
    slots: Vec<Option<Conn<C>>>,
    /// Accounts of connections that ended since the last
    /// [`Conns::take_released`]: each is one `Connections` charge to give
    /// back.
    released: Vec<usize>,
    /// Answers of the resolver for `resolve` requests (tokens from
    /// [`LOOKUP_BASE`]), not for a connection of this table.
    lookups: Vec<Resolved>,
}

impl<C: Client> Default for Conns<C> {
    fn default() -> Conns<C> {
        Conns {
            slots: Vec::new(),
            released: Vec::new(),
            lookups: Vec::new(),
        }
    }
}

impl<C: Client> Conns<C> {
    pub fn new() -> Conns<C> {
        Conns::default()
    }

    pub fn count(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// The slot the next [`Conns::open`] uses, if there is one.
    pub fn free_slot(&self) -> Option<usize> {
        self.slots
            .iter()
            .position(Option::is_none)
            .or((self.slots.len() < MAX_CONNS).then_some(self.slots.len()))
    }

    /// The connections of `account` waiting for a name.
    pub fn resolving_for(&self, account: usize) -> usize {
        self.slots
            .iter()
            .flatten()
            .filter(|c| c.owner.account == account && matches!(c.phase, Phase::Resolving))
            .count()
    }

    /// The resolver's answers to `resolve` requests since the last call.
    pub fn take_lookups(&mut self) -> Vec<Resolved> {
        core::mem::take(&mut self.lookups)
    }

    /// The accounts of the connections that ended since the last call, one
    /// entry each.
    pub fn take_released(&mut self) -> Vec<usize> {
        core::mem::take(&mut self.released)
    }

    /// Starts a connection to `target`:`port` for `client`; `scope` is the
    /// encoded scope the request was allowed under. Returns its slot, or the
    /// client back with the reason when it cannot start (the caller tells
    /// it). `owner` is the account it is charged to, and `resolutions` the
    /// most connections of that account that may wait for a name at once:
    /// past it the reason is `OVER_QUOTA`. On `Err` nothing is recorded; the
    /// caller gives the account's charge back. On `Ok` the charge comes back
    /// through [`Conns::take_released`].
    #[allow(clippy::too_many_arguments)] // the request's parts and the account's
    pub fn open<L: Link>(
        &mut self,
        stack: &mut Stack<L>,
        client: C,
        owner: Owner,
        resolutions: usize,
        target: Target,
        port: u16,
        scope: &[u8],
        now_us: i64,
    ) -> Result<usize, (C, u32)> {
        let Some(slot) = self.free_slot() else {
            return Err((client, reason::OVER_LIMIT));
        };
        if slot == self.slots.len() {
            self.slots.push(None);
        }
        let mut conn = Conn {
            client,
            owner,
            phase: Phase::Resolving,
            port,
            scope: scope.to_vec(),
            candidates: Vec::new(),
            next: 0,
            from_hosts: false,
            saw_local: false,
            last_failure: reason::UNRESOLVABLE,
        };
        let first = match target {
            Target::Address(a) => Some(Ok(Found {
                addresses: vec![a],
                from_hosts: false,
            })),
            Target::Name(name) => match stack.resolve_as(slot as u32, owner, &name, now_us) {
                Start::Done(found) => Some(Ok(found)),
                Start::Pending if self.resolving_for(owner.account) >= resolutions => {
                    stack.resolve_cancel(slot as u32);
                    return Err((conn.client, reason::OVER_QUOTA));
                }
                Start::Pending => None,
                Start::Failed(f) => Some(Err(f)),
            },
        };
        if let Some(found) = first {
            // A failure is told to the client here; the next `step_all`
            // returns the finished connection.
            let _ = conn.resolved(stack, found, now_us);
        }
        self.slots[slot] = Some(conn);
        Ok(slot)
    }

    /// The client of slot `id`, for `netd`'s bookkeeping of its handles.
    pub fn client(&self, id: usize) -> Option<&C> {
        self.slots.get(id)?.as_ref().map(|c| &c.client)
    }

    /// Steps every connection. `visit` sees each one's slot, client and
    /// step (to adjust the subscriptions of its handles). Returns the sum of
    /// the steps (`progress`: the interface has something to send; `delay_us`:
    /// when to come back) and the connections that finished, with their
    /// clients (to close their handles).
    pub fn step_all<L: Link>(
        &mut self,
        stack: &mut Stack<L>,
        now_us: i64,
        mut visit: impl FnMut(usize, &mut C, &Step),
    ) -> (Step, Vec<(usize, C)>) {
        let mut results = stack.take_resolved();
        let mut total = Step::default();
        let mut finished = Vec::new();
        for id in 0..self.slots.len() {
            let Some(conn) = self.slots[id].as_mut() else {
                continue;
            };
            let found = results
                .iter()
                .position(|r| r.token as usize == id)
                .map(|i| results.swap_remove(i).result);
            let s = conn.step(stack, found, now_us);
            total.progress |= s.progress;
            total.delay_us = match (total.delay_us, s.delay_us) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            visit(id, &mut conn.client, &s);
            if s.done
                && let Some(c) = self.slots[id].take()
            {
                self.released.push(c.owner.account);
                finished.push((id, c.client));
            }
        }
        // What no connection claimed is a `resolve` request's answer (a
        // token below `LOOKUP_BASE` with no connection is a gone one's).
        self.lookups
            .extend(results.into_iter().filter(|r| r.token >= LOOKUP_BASE));
        stack.tcp_reap();
        (total, finished)
    }

    /// One turn of the stack and the connections, in the order `netd` needs:
    /// frames in and timers, answers of the resolver, the connections, and
    /// the frames those queued out. Returns when to come back (microseconds
    /// from `now_us`; `None` waits for a frame or a request) and the
    /// finished connections. The caller wraps it in the link's `pump` and
    /// `flush`.
    pub fn turn<L: Link>(
        &mut self,
        stack: &mut Stack<L>,
        now_us: i64,
        visit: impl FnMut(usize, &mut C, &Step),
    ) -> (Option<i64>, Vec<(usize, C)>) {
        let min = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let mut delay = stack.poll(now_us);
        let queried = stack.dns_poll(now_us);
        let (total, finished) = self.step_all(stack, now_us, visit);
        if queried || total.progress {
            // What the resolver and the connections queued goes out now;
            // the second poll's delay is the current one.
            delay = stack.poll(now_us);
        }
        delay = min(delay, stack.dns_delay(now_us));
        delay = min(delay, total.delay_us);
        (delay, finished)
    }

    /// Ends connection `id` at the client's request or because its handles
    /// are gone: the socket is aborted and the slot freed.
    pub fn abort<L: Link>(&mut self, stack: &mut Stack<L>, id: usize) -> Option<C> {
        let conn = self.slots.get_mut(id)?.take()?;
        self.released.push(conn.owner.account);
        stack.resolve_cancel(id as u32);
        match conn.phase {
            Phase::Connecting { tcp, .. } => stack.tcp_release(tcp),
            Phase::Open(o) => stack.tcp_release(o.tcp),
            _ => {}
        }
        Some(conn.client)
    }
}

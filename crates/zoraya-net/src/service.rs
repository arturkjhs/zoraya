//! `netd`'s requests: the messages of `zoraya-proto-net` on the scope
//! channels, decoded, checked against the connection's scope and rights, and
//! answered from the stack. The loop in `netd` owns the handles; this owns
//! the decisions, so that the host tests cover them (INV-0220-3, INV-0222-3,
//! INV-0229-3).
//!
//! A request that does not decode ends the connection (`Action::Close`), as
//! ADR-0055's servers do; an ordinal this service does not know gets
//! `ErrorReply` and the connection stays, so a later `netd` can add requests.

use alloc::vec::Vec;
use zoraya_proto_net::{
    Address, Connect, Connecting, GetStatus, Narrow, Narrowed, Ping, PingReply, StatusReply,
    source, status as code,
};
use zoraya_protocol::{ERROR_ORDINAL, ErrorReply, Header, Seq, decode, encode};

use crate::conn::Target;
use crate::connection::{ConnectTarget, Connection, Request, Verdict};
use crate::link::Link;
use crate::stack::{PingError, PingResult, Source, Stack};
use zoraya_proto_net::{Code, MAX_DNS, NO_LEASE};
use zoraya_protocol::{Below, Data};

/// The most connections `netd` keeps (ADR-0222's `[assumption]` bound until
/// N3 gives each scope its own).
pub const MAX_CONNECTIONS: usize = 64;

/// What the loop does with a request's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write these bytes to the same connection.
    Reply(Vec<u8>),
    /// A run started; its reply comes from [`Service::ping_done`].
    Deferred,
    /// Attach the handle that came with the request as connection `id`, then
    /// write `reply`.
    Narrowed { id: usize, reply: Vec<u8> },
    /// Open a TCP connection with the three handles that came with the
    /// request (channel, inbound write end, outbound read end) after writing
    /// `reply`. `scope` is the encoded scope it was allowed under.
    Connect {
        target: Target,
        port: u16,
        scope: Vec<u8>,
        reply: Vec<u8>,
    },
    /// The request was malformed: close this connection.
    Close,
}

pub struct Service {
    connections: Vec<Option<Connection>>,
    /// The connection and transaction a running `ping` answers.
    ping: Option<(usize, u32)>,
}

impl Default for Service {
    fn default() -> Service {
        Service::new()
    }
}

fn reply<'a, M: zoraya_protocol::Message<'a>>(m: &M, transaction: u32) -> Vec<u8> {
    let mut out = alloc::vec![0u8; 1024];
    // Every reply here is small and bounded; a message that did not fit would
    // be a bug in this file, and the empty reply closes the connection.
    match encode(m, transaction, &mut out) {
        Ok(n) => {
            out.truncate(n);
            out
        }
        Err(_) => Vec::new(),
    }
}

fn be(a: [u8; 4]) -> u32 {
    u32::from_be_bytes(a)
}

fn source_code(s: Source) -> u32 {
    match s {
        Source::None => source::NONE,
        Source::Dhcp => source::DHCP,
        Source::Static => source::STATIC,
    }
}

impl Service {
    pub fn new() -> Service {
        Service {
            connections: Vec::new(),
            ping: None,
        }
    }

    /// Adds a connection; its id, or `None` at the bound.
    pub fn add(&mut self, connection: Connection) -> Option<usize> {
        if let Some(i) = self.connections.iter().position(Option::is_none) {
            self.connections[i] = Some(connection);
            return Some(i);
        }
        if self.connections.len() >= MAX_CONNECTIONS {
            return None;
        }
        self.connections.push(Some(connection));
        Some(self.connections.len() - 1)
    }

    pub fn remove(&mut self, id: usize) {
        if let Some(slot) = self.connections.get_mut(id) {
            *slot = None;
        }
        if self.ping.is_some_and(|(owner, _)| owner == id) {
            // Its reply has nowhere to go; the run itself finishes on its own.
            self.ping = None;
        }
    }

    pub fn connection(&self, id: usize) -> Option<&Connection> {
        self.connections.get(id)?.as_ref()
    }

    pub fn count(&self) -> usize {
        self.connections.iter().flatten().count()
    }

    /// A request on connection `id`: `handles` is how many handles came with
    /// it (the loop closes any it does not use).
    pub fn handle<L: Link>(
        &mut self,
        id: usize,
        message: &[u8],
        handles: usize,
        stack: &mut Stack<L>,
        now_us: i64,
    ) -> Action {
        let Some(connection) = self.connection(id) else {
            return Action::Close;
        };
        let Ok((header, _)) = Header::decode(message) else {
            return Action::Close;
        };
        let t = header.transaction;
        match header.ordinal {
            // A reply-shaped message is not a request, and answering it would
            // let two servers (or a connection and its own peer) answer each
            // other's errors forever: the connection ends.
            ERROR_ORDINAL => Action::Close,
            1 => {
                if decode::<GetStatus>(message, handles).is_err() {
                    return Action::Close;
                }
                let allowed = connection.decide(Request::Status) == Verdict::Allowed;
                Action::Reply(status_reply(stack, now_us, allowed, t))
            }
            2 => {
                let Ok((_, m)) = decode::<Ping>(message, handles) else {
                    return Action::Close;
                };
                let address = m.address.to_be_bytes();
                let verdict = connection.decide(Request::Ping { address });
                let code = match verdict {
                    Verdict::Allowed => None,
                    _ => Some(code::NOT_GRANTED),
                };
                if let Some(c) = code {
                    return Action::Reply(ping_reply(c, None, t));
                }
                if self.ping.is_some() {
                    return Action::Reply(ping_reply(code::BUSY, None, t));
                }
                let timeout = i64::from(m.timeout_ms.0) * 1000;
                match stack.ping(address, m.count.0, timeout, now_us) {
                    Ok(()) => {
                        self.ping = Some((id, t));
                        Action::Deferred
                    }
                    Err(PingError::Busy) => Action::Reply(ping_reply(code::BUSY, None, t)),
                    Err(PingError::BadTarget) => Action::Reply(ping_reply(code::INVALID, None, t)),
                    Err(PingError::NoAddress) => {
                        Action::Reply(ping_reply(code::UNAVAILABLE, None, t))
                    }
                }
            }
            3 => {
                let Ok((_, m)) = decode::<Connect>(message, handles) else {
                    return Action::Close;
                };
                let answer = |c| Action::Reply(reply(&Connecting { status: Below(c) }, t));
                let port = m.port.0 as u16;
                let target = if m.name.0.is_empty() {
                    let a = m.address.to_be_bytes();
                    if m.address == 0 {
                        return answer(code::INVALID);
                    }
                    (ConnectTarget::Address(a), Target::Address(a))
                } else {
                    // A name is text a scope can match: valid, and not an
                    // address spelled as a name.
                    let Some(n) = core::str::from_utf8(m.name.0)
                        .ok()
                        .filter(|n| zoraya_abi::net::valid_name(n))
                    else {
                        return answer(code::INVALID);
                    };
                    (ConnectTarget::Name(n), Target::Name(n.into()))
                };
                match connection.decide(Request::Connect {
                    target: target.0,
                    port,
                }) {
                    Verdict::Allowed => Action::Connect {
                        target: target.1,
                        port,
                        scope: connection.scope_bytes().to_vec(),
                        reply: reply(
                            &Connecting {
                                status: Below(code::OK),
                            },
                            t,
                        ),
                    },
                    Verdict::NotGranted => answer(code::NOT_GRANTED),
                    Verdict::Invalid => answer(code::INVALID),
                }
            }
            6 => {
                let Ok((_, m)) = decode::<Narrow>(message, handles) else {
                    return Action::Close;
                };
                let verdict = connection.decide(Request::Narrow {
                    rights: m.rights,
                    scope: m.scope.0,
                });
                let answer = |c| Action::Reply(reply(&Narrowed { status: Below(c) }, t));
                match verdict {
                    Verdict::NotGranted => answer(code::NOT_GRANTED),
                    Verdict::Invalid => answer(code::INVALID),
                    Verdict::Allowed => {
                        let Some(child) = connection.narrowed(m.rights, m.scope.0) else {
                            return answer(code::INVALID);
                        };
                        match self.add(child) {
                            Some(new) => Action::Narrowed {
                                id: new,
                                reply: reply(
                                    &Narrowed {
                                        status: Below(code::OK),
                                    },
                                    t,
                                ),
                            },
                            None => answer(code::BUSY),
                        }
                    }
                }
            }
            _ => Action::Reply(reply(
                &ErrorReply {
                    status: code::INVALID,
                },
                t,
            )),
        }
    }

    /// The reply to the run that ended: the connection and the bytes, or
    /// `None` if its connection is gone.
    pub fn ping_done(&mut self, result: PingResult) -> Option<(usize, Vec<u8>)> {
        let (id, t) = self.ping.take()?;
        let body = PingReply {
            status: Below(code::OK),
            sent: result.sent,
            received: result.received,
            rtt_min_us: result.rtt_min_us,
            rtt_avg_us: result.rtt_avg_us,
            rtt_max_us: result.rtt_max_us,
        };
        Some((id, reply(&body, t)))
    }
}

fn ping_reply(status: u32, result: Option<PingResult>, t: u32) -> Vec<u8> {
    let r = result.unwrap_or(PingResult {
        target: [0; 4],
        sent: 0,
        received: 0,
        rtt_min_us: 0,
        rtt_avg_us: 0,
        rtt_max_us: 0,
    });
    reply(
        &PingReply {
            status: Below(status),
            sent: r.sent,
            received: r.received,
            rtt_min_us: r.rtt_min_us,
            rtt_avg_us: r.rtt_avg_us,
            rtt_max_us: r.rtt_max_us,
        },
        t,
    )
}

fn status_reply<L: Link>(stack: &Stack<L>, now_us: i64, allowed: bool, t: u32) -> Vec<u8> {
    let s = stack.status(now_us);
    let dns: Vec<Address> = s
        .dns
        .iter()
        .take(MAX_DNS)
        .map(|a| Address { address: be(*a) })
        .collect();
    let status: Code = Below(if allowed { code::OK } else { code::NOT_GRANTED });
    let body = if allowed {
        StatusReply {
            status,
            link: s.link,
            address_source: Below(source_code(s.source)),
            dns_source: Below(source_code(s.dns_source)),
            prefix: Below(u32::from(s.prefix.min(32))),
            address: be(s.address),
            gateway: be(s.gateway),
            lease_left: s.lease_left.unwrap_or(NO_LEASE),
            frames_in: s.counters.frames_in,
            frames_out: s.counters.frames_out,
            frames_dropped: s.counters.dropped,
            mac: Data(&s.mac),
            dns: Seq::Of(&dns),
        }
    } else {
        // Nothing of the state to a connection without the right.
        StatusReply {
            status,
            link: false,
            address_source: Below(source::NONE),
            dns_source: Below(source::NONE),
            prefix: Below(0),
            address: 0,
            gateway: 0,
            lease_left: NO_LEASE,
            frames_in: 0,
            frames_out: 0,
            frames_dropped: 0,
            mac: Data(&[]),
            dns: Seq::Of(&[]),
        }
    };
    reply(&body, t)
}

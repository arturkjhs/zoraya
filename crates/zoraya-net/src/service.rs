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
    Address, Connect, Connecting, GetStatus, MAX_ADDRESSES, Narrow, NarrowAccount, Narrowed, Ping,
    PingReply, Resolve, Resolved, StatusReply, source, status as code,
};
use zoraya_protocol::{ERROR_ORDINAL, ErrorReply, Header, Seq, decode, encode};

use crate::accounts::{Accounts, CarveError, Kind};
use crate::conn::{LOOKUP_BASE, Target, is_local};
use crate::connection::{ConnectTarget, Connection, Request, Verdict};
use crate::dns::{Found, Owner, ResolveFail, Resolved as Answer, Start};
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
        /// The account the connection is charged to (a `Connections`
        /// charge is taken; give it back with [`Service::connection_ended`]
        /// when the connection is over or never opened).
        account: usize,
        /// The account is at its bound: tell the client `Failed {
        /// OVER_QUOTA }` after `reply`, and open nothing.
        over_quota: bool,
    },
    /// The request was malformed: close this connection.
    Close,
}

/// A `resolve` whose answer the resolver has not given yet.
struct Lookup {
    token: u32,
    /// The connection to answer; `None` once it is gone.
    conn: Option<usize>,
    transaction: u32,
    account: usize,
    scope: Vec<u8>,
}

pub struct Service {
    connections: Vec<Option<Connection>>,
    accounts: Accounts,
    lookups: Vec<Lookup>,
    next_token: u32,
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
            accounts: Accounts::new(),
            lookups: Vec::new(),
            next_token: 0,
            ping: None,
        }
    }

    /// Adds a connection; its id, or `None` at the bound.
    pub fn add(&mut self, connection: Connection) -> Option<usize> {
        let account = connection.account();
        let id = if let Some(i) = self.connections.iter().position(Option::is_none) {
            self.connections[i] = Some(connection);
            i
        } else if self.connections.len() >= MAX_CONNECTIONS {
            return None;
        } else {
            self.connections.push(Some(connection));
            self.connections.len() - 1
        };
        self.accounts.join(account);
        Some(id)
    }

    /// The accounts, for the loop (the resolver's and the connections'
    /// bounds) and for the tests.
    pub fn accounts(&self) -> &Accounts {
        &self.accounts
    }

    /// A TCP connection of `account` is over (or never opened): its charge
    /// goes back.
    pub fn connection_ended(&mut self, account: usize) {
        self.accounts.release(account, Kind::Connections);
    }

    pub fn remove(&mut self, id: usize) {
        if let Some(slot) = self.connections.get_mut(id)
            && let Some(c) = slot.take()
        {
            self.accounts.leave(c.account());
        }
        // A lookup of this connection still runs; its charge goes with its
        // answer.
        for l in self.lookups.iter_mut().filter(|l| l.conn == Some(id)) {
            l.conn = None;
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
                let verdict = connection.decide(Request::Connect {
                    target: target.0,
                    port,
                });
                let account = connection.account();
                match verdict {
                    Verdict::Allowed => {
                        let scope = connection.scope_bytes().to_vec();
                        let over_quota = self.accounts.charge(account, Kind::Connections).is_err();
                        Action::Connect {
                            target: target.1,
                            port,
                            scope,
                            reply: reply(
                                &Connecting {
                                    status: Below(code::OK),
                                },
                                t,
                            ),
                            account,
                            over_quota,
                        }
                    }
                    Verdict::NotGranted => answer(code::NOT_GRANTED),
                    Verdict::Invalid => answer(code::INVALID),
                }
            }
            4 => {
                let Ok((_, m)) = decode::<Resolve>(message, handles) else {
                    return Action::Close;
                };
                let answer = |c| Action::Reply(resolved_reply(c, &[], t));
                let Some(name) = core::str::from_utf8(m.name.0)
                    .ok()
                    .filter(|n| zoraya_abi::net::valid_name(n))
                else {
                    return answer(code::INVALID);
                };
                if connection.decide(Request::Resolve { name }) != Verdict::Allowed {
                    return answer(code::NOT_GRANTED);
                }
                if self.lookups.iter().any(|l| l.conn == Some(id)) {
                    return answer(code::BUSY);
                }
                let account = connection.account();
                let scope = connection.scope_bytes().to_vec();
                if self.accounts.charge(account, Kind::Resolutions).is_err() {
                    return answer(code::OVER_QUOTA);
                }
                let token = LOOKUP_BASE + (self.next_token & 0xffff);
                self.next_token = self.next_token.wrapping_add(1);
                let owner = Owner {
                    account,
                    cache: self.accounts.limit(account, Kind::CacheEntries) as usize,
                };
                match stack.resolve_as(token, owner, name, now_us) {
                    Start::Pending => {
                        self.lookups.push(Lookup {
                            token,
                            conn: Some(id),
                            transaction: t,
                            account,
                            scope,
                        });
                        Action::Deferred
                    }
                    Start::Done(found) => {
                        self.accounts.release(account, Kind::Resolutions);
                        Action::Reply(found_reply(&found, &scope, stack, now_us, t))
                    }
                    Start::Failed(f) => {
                        self.accounts.release(account, Kind::Resolutions);
                        answer(fail_code(f))
                    }
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
            7 => {
                let Ok((_, m)) = decode::<NarrowAccount>(message, handles) else {
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
                        let parent = connection.account();
                        let carved = match self.accounts.carve(parent, m.budget.0) {
                            Ok(a) => a,
                            Err(CarveError::Invalid) => return answer(code::INVALID),
                            Err(CarveError::OverQuota) => return answer(code::OVER_QUOTA),
                        };
                        match self.add(child.in_account(carved)) {
                            Some(new) => Action::Narrowed {
                                id: new,
                                reply: reply(
                                    &Narrowed {
                                        status: Below(code::OK),
                                    },
                                    t,
                                ),
                            },
                            None => {
                                // Nothing joined it: it ends with no hold.
                                self.accounts.join(carved);
                                self.accounts.leave(carved);
                                answer(code::BUSY)
                            }
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

    /// The reply to a `resolve` the resolver has answered: the connection
    /// and the bytes, or `None` if its connection is gone (its charge is
    /// given back either way).
    pub fn resolve_done<L: Link>(
        &mut self,
        stack: &Stack<L>,
        answer: Answer,
        now_us: i64,
    ) -> Option<(usize, Vec<u8>)> {
        let at = self.lookups.iter().position(|l| l.token == answer.token)?;
        let l = self.lookups.swap_remove(at);
        self.accounts.release(l.account, Kind::Resolutions);
        let id = l.conn?;
        let bytes = match answer.result {
            Ok(found) => found_reply(&found, &l.scope, stack, now_us, l.transaction),
            Err(f) => resolved_reply(fail_code(f), &[], l.transaction),
        };
        Some((id, bytes))
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

fn fail_code(f: ResolveFail) -> u32 {
    match f {
        ResolveFail::BadName => code::INVALID,
        ResolveFail::Busy => code::BUSY,
        ResolveFail::NoServer => code::UNAVAILABLE,
        ResolveFail::NoSuchName | ResolveFail::NoAnswer | ResolveFail::Failure => {
            code::UNRESOLVABLE
        }
    }
}

fn resolved_reply(status: u32, addresses: &[Address], t: u32) -> Vec<u8> {
    reply(
        &Resolved {
            status: Below(status),
            addresses: Seq::Of(addresses),
        },
        t,
    )
}

/// The answer for the addresses of a name: the ones that are not local, or
/// that an address rule of the scope covers; none left is `LOCAL`
/// (ADR-0223 item 4). `net.hosts` is the person's own word and exempt.
fn found_reply<L: Link>(
    found: &Found,
    scope: &[u8],
    stack: &Stack<L>,
    now_us: i64,
    t: u32,
) -> Vec<u8> {
    let scope = zoraya_abi::net::Scope::decode(scope).unwrap_or(zoraya_abi::net::Scope::EMPTY);
    let usable: Vec<Address> = found
        .addresses
        .iter()
        .filter(|a| found.from_hosts || !is_local(stack, **a, now_us) || scope.reaches_address(**a))
        .take(MAX_ADDRESSES)
        .map(|a| Address { address: be(*a) })
        .collect();
    if usable.is_empty() {
        resolved_reply(code::LOCAL, &[], t)
    } else {
        resolved_reply(code::OK, &usable, t)
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

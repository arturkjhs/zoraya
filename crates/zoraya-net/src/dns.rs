//! The resolver (ADR-0223 items 1, 5, 6 and 7): `net.hosts` first, then a
//! cache, then A queries over one UDP socket to the servers of `status`
//! (`net.dns`, else the lease's), asked in turn with a retry schedule. A
//! query in flight is a record with a deadline; nothing blocks. Every answer
//! goes through `zoraya_dns::parse_response`, which is total (see that
//! crate). What this module adds is where the answer may come from (a
//! configured server, port 53, a matching identifier and question) and how
//! long it is kept.
//!
//! Not in N3: AAAA, the TCP fallback for a truncated answer, negative
//! caching, and randomness for ports and identifiers that an attacker cannot
//! guess (ADR-0227's service is not there; they come from smoltcp's seed and
//! the clock).

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::udp;
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address};
use zoraya_dns::{MAX_MESSAGE, MAX_QUERY, Outcome, ParseError};

use crate::link::Link;
use crate::stack::Stack;

/// Queries in flight at once.
pub const MAX_QUERIES: usize = 8;
/// Names kept from earlier answers.
pub const CACHE_ENTRIES: usize = 64;
/// When each attempt goes out, from the start (microseconds); the query
/// gives up at [`GIVE_UP_US`]. Attempt `i` goes to server `i % servers`.
const ATTEMPTS_US: [i64; 4] = [0, 1_000_000, 2_000_000, 4_000_000];
const GIVE_UP_US: i64 = 8_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveFail {
    /// Not a name the resolver sends.
    BadName,
    /// The server says there is no such name.
    NoSuchName,
    /// The name has no A record.
    NoAnswer,
    /// The server failed, or did not answer in time.
    Failure,
    /// [`MAX_QUERIES`] queries are in flight.
    Busy,
    /// No DNS server is known.
    NoServer,
}

/// Addresses for a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub addresses: Vec<[u8; 4]>,
    /// From `net.hosts`: the person's own authority, exempt from the local
    /// address rule (ADR-0223 item 4).
    pub from_hosts: bool,
}

/// What [`Stack::resolve`] did with a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    Done(Found),
    /// A query is out; [`Stack::take_resolved`] will carry the result.
    Pending,
    Failed(ResolveFail),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub token: u32,
    pub result: Result<Found, ResolveFail>,
}

struct Query {
    token: u32,
    name: String,
    id: u16,
    started_us: i64,
    /// Attempts sent.
    sent: usize,
}

struct CacheEntry {
    name: String,
    addresses: Vec<[u8; 4]>,
    expires_us: i64,
}

/// The resolver's state, inside the stack's `Extra`.
#[derive(Default)]
pub struct Resolver {
    socket: Option<SocketHandle>,
    queries: Vec<Query>,
    cache: Vec<CacheEntry>,
    done: Vec<Resolved>,
    rng: u64,
    /// Answers that were not accepted, for `status`-like counters.
    pub rejected: u64,
}

impl Resolver {
    fn next_u64(&mut self) -> u64 {
        // xorshift64*; not secret (see the module's note).
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

fn lower(name: &str) -> String {
    name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase()
}

impl<L: Link> Stack<L> {
    fn dns_socket(&mut self, now_us: i64) -> SocketHandle {
        if let Some(h) = self.extra.resolver.socket {
            return h;
        }
        let mac = self.mac();
        let r = &mut self.extra.resolver;
        r.rng = (now_us as u64)
            ^ u64::from_le_bytes([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], 0x44, 0x53])
            | 1;
        let port = 30000 + (r.next_u64() % 10000) as u16;
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 8 * 600]),
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY; 4],
                vec![0; 4 * (MAX_MESSAGE + 8)],
            ),
        );
        // A fresh socket binds; the port is nonzero.
        let _ = socket.bind(port);
        let h = self.sockets.add(socket);
        self.extra.resolver.socket = Some(h);
        h
    }

    /// Starts resolving `name` for `token`.
    pub fn resolve(&mut self, token: u32, name: &str, now_us: i64) -> Start {
        let name = lower(name);
        if !zoraya_dns::valid_name(&name) {
            return Start::Failed(ResolveFail::BadName);
        }
        if let Some((_, a)) = self.config().hosts.iter().find(|(n, _)| *n == name) {
            return Start::Done(Found {
                addresses: vec![*a],
                from_hosts: true,
            });
        }
        let r = &mut self.extra.resolver;
        r.cache.retain(|e| e.expires_us > now_us);
        if let Some(e) = r.cache.iter().find(|e| e.name == name) {
            return Start::Done(Found {
                addresses: e.addresses.clone(),
                from_hosts: false,
            });
        }
        if self.status(now_us).dns.is_empty() {
            return Start::Failed(ResolveFail::NoServer);
        }
        if self.extra.resolver.queries.len() >= MAX_QUERIES {
            return Start::Failed(ResolveFail::Busy);
        }
        // Seeds the generator on first use, so before the first identifier.
        self.dns_socket(now_us);
        let r = &mut self.extra.resolver;
        let id = loop {
            let id = (r.next_u64() >> 16) as u16;
            if r.queries.iter().all(|q| q.id != id) {
                break id;
            }
        };
        r.queries.push(Query {
            token,
            name,
            id,
            started_us: now_us,
            sent: 0,
        });
        Start::Pending
    }

    /// Forgets the query of `token`, if it is still out.
    pub fn resolve_cancel(&mut self, token: u32) {
        self.extra.resolver.queries.retain(|q| q.token != token);
    }

    /// The results since the last call.
    pub fn take_resolved(&mut self) -> Vec<Resolved> {
        core::mem::take(&mut self.extra.resolver.done)
    }

    /// Reads answers, sends what is due, gives up what is late. Returns
    /// whether it queued a packet (the caller polls the interface again).
    pub fn dns_poll(&mut self, now_us: i64) -> bool {
        let Some(handle) = self.extra.resolver.socket else {
            return false;
        };
        let servers = self.status(now_us).dns;
        let mut buf = [0u8; MAX_MESSAGE + 1];
        loop {
            let socket = self.sockets.get_mut::<udp::Socket>(handle);
            let Ok((n, meta)) = socket.recv_slice(&mut buf) else {
                break;
            };
            let IpAddress::Ipv4(from) = meta.endpoint.addr;
            let r = &mut self.extra.resolver;
            // Only a server we asked, from its DNS port.
            if meta.endpoint.port != 53 || !servers.contains(&from.octets()) || n < 2 {
                r.rejected += 1;
                continue;
            }
            let id = u16::from_be_bytes([buf[0], buf[1]]);
            let Some(at) = r.queries.iter().position(|q| q.id == id) else {
                r.rejected += 1;
                continue;
            };
            let outcome = zoraya_dns::parse_response(&buf[..n], id, &r.queries[at].name);
            match outcome {
                Ok(Outcome::Addresses(a)) => {
                    let q = r.queries.remove(at);
                    let addresses = a.as_slice().to_vec();
                    if r.cache.len() >= CACHE_ENTRIES {
                        // The entry closest to expiry goes.
                        if let Some(i) = (0..r.cache.len()).min_by_key(|i| r.cache[*i].expires_us) {
                            r.cache.remove(i);
                        }
                    }
                    r.cache.push(CacheEntry {
                        name: q.name,
                        addresses: addresses.clone(),
                        expires_us: now_us + i64::from(a.ttl) * 1_000_000,
                    });
                    r.done.push(Resolved {
                        token: q.token,
                        result: Ok(Found {
                            addresses,
                            from_hosts: false,
                        }),
                    });
                }
                Ok(other) => {
                    let q = r.queries.remove(at);
                    let fail = match other {
                        Outcome::NoSuchName => ResolveFail::NoSuchName,
                        Outcome::NoData => ResolveFail::NoAnswer,
                        _ => ResolveFail::Failure,
                    };
                    r.done.push(Resolved {
                        token: q.token,
                        result: Err(fail),
                    });
                }
                // Not an answer to this query: keep waiting for the real one.
                Err(ParseError::Malformed | ParseError::NotAnAnswer | ParseError::WrongId)
                | Err(ParseError::WrongQuestion | ParseError::BadName) => r.rejected += 1,
            }
        }
        // Late queries end; due attempts go out.
        let mut sent_any = false;
        let mut i = 0;
        while i < self.extra.resolver.queries.len() {
            let (due, give_up, attempt, id, name) = {
                let q = &self.extra.resolver.queries[i];
                let age = now_us - q.started_us;
                let due = q.sent < ATTEMPTS_US.len() && age >= ATTEMPTS_US[q.sent];
                (due, age >= GIVE_UP_US, q.sent, q.id, q.name.clone())
            };
            if give_up || servers.is_empty() {
                let q = self.extra.resolver.queries.remove(i);
                self.extra.resolver.done.push(Resolved {
                    token: q.token,
                    result: Err(ResolveFail::Failure),
                });
                continue;
            }
            if due {
                let mut out = [0u8; MAX_QUERY];
                if let Ok(n) = zoraya_dns::encode_query(id, &name, &mut out) {
                    let server = servers[attempt % servers.len()];
                    let to = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from(server)), 53);
                    let socket = self.sockets.get_mut::<udp::Socket>(handle);
                    // A refusal (no route, buffer full) is an attempt used.
                    sent_any |= socket.send_slice(&out[..n], to).is_ok();
                }
                self.extra.resolver.queries[i].sent += 1;
            }
            i += 1;
        }
        sent_any
    }

    /// Microseconds until a query needs attention, if one is out.
    pub fn dns_delay(&self, now_us: i64) -> Option<i64> {
        self.extra
            .resolver
            .queries
            .iter()
            .map(|q| {
                let next = ATTEMPTS_US
                    .get(q.sent)
                    .copied()
                    .unwrap_or(GIVE_UP_US)
                    .min(GIVE_UP_US);
                (q.started_us + next - now_us).max(0)
            })
            .min()
    }

    /// The local UDP port queries go out from, once there is one.
    pub fn dns_port(&self) -> Option<u16> {
        let h = self.extra.resolver.socket?;
        Some(self.sockets.get::<udp::Socket>(h).endpoint().port)
    }

    /// Queries in flight.
    pub fn dns_pending(&self) -> usize {
        self.extra.resolver.queries.len()
    }

    /// Answers dropped as not from a server we asked or not for a query.
    pub fn dns_rejected(&self) -> u64 {
        self.extra.resolver.rejected
    }

    /// The first name of the cache, for tests: whether `name` is cached.
    pub fn dns_cached(&self, name: &str) -> bool {
        let name = name.to_string().to_ascii_lowercase();
        self.extra.resolver.cache.iter().any(|e| e.name == name)
    }
}

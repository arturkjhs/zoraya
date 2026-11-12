//! Sockets, the parts that need no kernel (ADR-0225; N4): the constants, the
//! `sockaddr_in` and address text forms, the service names, the `errno` of
//! each status and reason `netd` gives, the `getaddrinfo` plan, the cache of
//! resolved names, the options table. The calls that use handles are
//! `net.rs`; this file is total on any bytes a program passes, and host
//! tested.

#![forbid(unsafe_code)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::errno::{
    EACCES, EAFNOSUPPORT, ECONNABORTED, ECONNREFUSED, ECONNRESET, EHOSTUNREACH, EINVAL, ENETDOWN,
    ENETUNREACH, ENOBUFS, ENOPROTOOPT, EPROTONOSUPPORT, ETIMEDOUT,
};
use zoraya_proto_net::{reason, reset, status};

pub const AF_UNSPEC: i32 = 0;
pub const AF_INET: i32 = 2;
pub const AF_INET6: i32 = 10;
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;
pub const SOCK_RAW: i32 = 3;
pub const SOCK_NONBLOCK: i32 = 0o4000;
pub const SOCK_CLOEXEC: i32 = 0o2000000;
pub const IPPROTO_IP: i32 = 0;
pub const IPPROTO_TCP: i32 = 6;

pub const SOL_SOCKET: i32 = 1;
pub const SO_REUSEADDR: i32 = 2;
pub const SO_TYPE: i32 = 3;
pub const SO_ERROR: i32 = 4;
pub const SO_DONTROUTE: i32 = 5;
pub const SO_BROADCAST: i32 = 6;
pub const SO_SNDBUF: i32 = 7;
pub const SO_RCVBUF: i32 = 8;
pub const SO_KEEPALIVE: i32 = 9;
pub const SO_OOBINLINE: i32 = 10;
pub const SO_REUSEPORT: i32 = 15;
pub const SO_RCVTIMEO: i32 = 20;
pub const SO_SNDTIMEO: i32 = 21;
pub const SO_ACCEPTCONN: i32 = 30;
pub const SO_PROTOCOL: i32 = 38;
pub const SO_DOMAIN: i32 = 39;
pub const TCP_NODELAY: i32 = 1;
pub const TCP_KEEPIDLE: i32 = 4;
pub const TCP_KEEPINTVL: i32 = 5;
pub const TCP_KEEPCNT: i32 = 6;

pub const MSG_OOB: i32 = 1;
pub const MSG_PEEK: i32 = 2;
pub const MSG_DONTWAIT: i32 = 0x40;
pub const MSG_WAITALL: i32 = 0x100;
pub const MSG_NOSIGNAL: i32 = 0x4000;

pub const SHUT_RD: i32 = 0;
pub const SHUT_WR: i32 = 1;
pub const SHUT_RDWR: i32 = 2;

/// The size of `struct sockaddr_in`.
pub const SOCKADDR_IN: usize = 16;
/// What a ring holds: `SO_RCVBUF` and `SO_SNDBUF` report it (ADR-0225 item 7).
pub const RING: i32 = 64 * 1024;
/// Resolved addresses remembered for `connect` by name (ADR-0225 item 3).
pub const CACHE: usize = 64;

// ------------------------------------------------------------ addresses

/// The address and port of a `struct sockaddr_in` of `bytes`: `EINVAL` if it
/// is shorter than the structure, `EAFNOSUPPORT` if it is not `AF_INET`.
pub fn parse_sockaddr_in(bytes: &[u8]) -> Result<([u8; 4], u16), i32> {
    if bytes.len() < SOCKADDR_IN {
        return Err(EINVAL);
    }
    if i32::from(u16::from_ne_bytes([bytes[0], bytes[1]])) != AF_INET {
        return Err(EAFNOSUPPORT);
    }
    Ok((
        [bytes[4], bytes[5], bytes[6], bytes[7]],
        u16::from_be_bytes([bytes[2], bytes[3]]),
    ))
}

/// A `struct sockaddr_in` for `address` and `port`.
pub fn sockaddr_in(address: [u8; 4], port: u16) -> [u8; SOCKADDR_IN] {
    let mut out = [0u8; SOCKADDR_IN];
    out[..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    out[2..4].copy_from_slice(&port.to_be_bytes());
    out[4..8].copy_from_slice(&address);
    out
}

/// `inet_pton(AF_INET)`: four decimal numbers, each 1 to 3 digits, 0 to 255,
/// no leading zero but a lone 0, nothing else. A spelling that other
/// functions read as an address (`0x7f000001`, `127.1`, `0177.0.0.1`) is not
/// one here (decided in N4; the N3 review asked).
pub fn inet_pton4(text: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = text.split(|&c| c == b'.');
    for slot in &mut out {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 || !p.iter().all(u8::is_ascii_digit) {
            return None;
        }
        if p.len() > 1 && p[0] == b'0' {
            return None;
        }
        let v = p.iter().fold(0u16, |a, d| a * 10 + u16::from(d - b'0'));
        *slot = u8::try_from(v).ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// One part of `inet_aton`: decimal, octal (leading 0) or hex (0x).
fn aton_number(p: &[u8]) -> Option<u64> {
    let (digits, radix) = match p {
        [b'0', b'x' | b'X', rest @ ..] => (rest, 16),
        [b'0', rest @ ..] if !rest.is_empty() => (rest, 8),
        _ => (p, 10),
    };
    if digits.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for &c in digits {
        let d = char::from(c).to_digit(radix)?;
        v = v.checked_mul(u64::from(radix))?.checked_add(u64::from(d))?;
        if v > u64::from(u32::MAX) {
            return None;
        }
    }
    Some(v)
}

/// `inet_aton`: one to four parts, each decimal, octal or hex; the last
/// fills the bytes that are left; text after the address may start with a
/// space (`inet_aton`'s contract). This is the C library's historical
/// reader, offered because programs call it; `getaddrinfo` does not use it.
pub fn inet_aton(text: &[u8]) -> Option<[u8; 4]> {
    let end = text
        .iter()
        .position(|c| c.is_ascii_whitespace())
        .unwrap_or(text.len());
    let text = &text[..end];
    let parts: Vec<&[u8]> = text.split(|&c| c == b'.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut numbers = Vec::with_capacity(4);
    for p in &parts {
        numbers.push(aton_number(p)?);
    }
    let (last, init) = numbers.split_last()?;
    if init.iter().any(|&v| v > 255) {
        return None;
    }
    // The last part has 8 bits for each byte not given before it.
    let room = 8 * (4 - init.len() as u32);
    if room < 32 && *last >= 1u64 << room {
        return None;
    }
    let mut value = *last;
    for (i, &v) in init.iter().enumerate() {
        value |= v << (24 - 8 * i as u32);
    }
    Some((value as u32).to_be_bytes())
}

/// Dotted decimal.
pub fn ntop4(a: [u8; 4]) -> String {
    alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

// ------------------------------------------------------------ services

/// The built-in table of service names (ADR-0225 item 6, `[assumption]`).
const SERVICES: &[(&str, u16)] = &[
    ("http", 80),
    ("https", 443),
    ("ftp", 21),
    ("ssh", 22),
    ("domain", 53),
    ("ntp", 123),
    ("smtp", 25),
    ("imap", 143),
    ("pop3", 110),
];

/// The port of a service name from the table, or of a decimal number.
pub fn service_port(service: &[u8]) -> Option<u16> {
    if let Some(n) = numeric_port(service) {
        return Some(n);
    }
    SERVICES
        .iter()
        .find(|(n, _)| n.as_bytes() == service)
        .map(|(_, p)| *p)
}

/// A port written in decimal digits only.
pub fn numeric_port(service: &[u8]) -> Option<u16> {
    if service.is_empty() || service.len() > 5 || !service.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = service
        .iter()
        .fold(0u32, |a, d| a * 10 + u32::from(d - b'0'));
    u16::try_from(v).ok()
}

// ------------------------------------------------------------ errors

/// `errno` for the status of a refused `connect` (`Connecting`).
pub fn connect_status(code: u32) -> i32 {
    match code {
        status::NOT_GRANTED => EACCES,
        status::INVALID => EINVAL,
        status::UNAVAILABLE => ENETUNREACH,
        status::BUSY => crate::errno::EAGAIN,
        status::OVER_QUOTA => ENOBUFS,
        _ => crate::errno::EIO,
    }
}

/// `errno` for a `Failed` reason (ADR-0225 item 3, ADR-0263 item 4).
pub fn failed_reason(why: u32) -> i32 {
    match why {
        reason::UNRESOLVABLE | reason::LOCAL => EHOSTUNREACH,
        reason::REFUSED => ECONNREFUSED,
        reason::UNREACHABLE => ENETUNREACH,
        reason::TIMED_OUT => ETIMEDOUT,
        reason::OVER_LIMIT | reason::OVER_QUOTA => ENOBUFS,
        _ => crate::errno::EIO,
    }
}

/// `errno` for a `Reset` (ADR-0225 item 4).
pub fn reset_why(why: u32) -> i32 {
    match why {
        reset::TIMED_OUT => ETIMEDOUT,
        reset::ABORTED => ECONNABORTED,
        _ => ECONNRESET,
    }
}

/// `errno` for a scope channel that shows `PEER_CLOSED` (the stack was
/// restarted, ADR-0221 item 5).
pub const NET_GONE: i32 = ENETDOWN;

// `getaddrinfo`'s error numbers (musl's).
pub const EAI_BADFLAGS: i32 = -1;
pub const EAI_NONAME: i32 = -2;
pub const EAI_AGAIN: i32 = -3;
pub const EAI_FAIL: i32 = -4;
pub const EAI_FAMILY: i32 = -6;
pub const EAI_SOCKTYPE: i32 = -7;
pub const EAI_SERVICE: i32 = -8;
pub const EAI_MEMORY: i32 = -10;
pub const EAI_SYSTEM: i32 = -11;
pub const EAI_OVERFLOW: i32 = -12;

/// `gai_strerror`'s text, NUL ended.
pub fn gai_message(code: i32) -> &'static [u8] {
    match code {
        0 => b"Success\0",
        EAI_BADFLAGS => b"Invalid flags\0",
        EAI_NONAME => b"Name does not resolve\0",
        EAI_AGAIN => b"Try again\0",
        EAI_FAIL => b"Non-recoverable error in name resolution\0",
        EAI_FAMILY => b"Unrecognized address family or invalid length\0",
        EAI_SOCKTYPE => b"Unrecognized socket type\0",
        EAI_SERVICE => b"Unrecognized service\0",
        EAI_MEMORY => b"Out of memory\0",
        EAI_SYSTEM => b"System error\0",
        EAI_OVERFLOW => b"Overflow\0",
        _ => b"Unknown error\0",
    }
}

/// `errno` for the status of a `resolve` that did not give addresses, and
/// the `getaddrinfo` code it becomes (ADR-0225 item 6): `EAI_SYSTEM` carries
/// `errno`.
pub fn resolve_status(code: u32) -> (i32, i32) {
    match code {
        status::NOT_GRANTED => (EAI_SYSTEM, EACCES),
        status::INVALID | status::UNRESOLVABLE | status::LOCAL => (EAI_NONAME, 0),
        status::UNAVAILABLE | status::BUSY | status::OVER_QUOTA => (EAI_AGAIN, 0),
        _ => (EAI_FAIL, 0),
    }
}

// ------------------------------------------------------------ getaddrinfo

pub const AI_PASSIVE: i32 = 0x01;
pub const AI_CANONNAME: i32 = 0x02;
pub const AI_NUMERICHOST: i32 = 0x04;
pub const AI_V4MAPPED: i32 = 0x08;
pub const AI_ALL: i32 = 0x10;
pub const AI_ADDRCONFIG: i32 = 0x20;
pub const AI_NUMERICSERV: i32 = 0x400;
const AI_KNOWN: i32 = AI_PASSIVE
    | AI_CANONNAME
    | AI_NUMERICHOST
    | AI_V4MAPPED
    | AI_ALL
    | AI_ADDRCONFIG
    | AI_NUMERICSERV;

/// The parts of `struct addrinfo` a request carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hints {
    pub flags: i32,
    pub family: i32,
    pub socktype: i32,
    pub protocol: i32,
}

/// Where the addresses come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// An address given in the library: a dotted quad, `AI_PASSIVE`'s
    /// wildcard, or the loopback of no node.
    Fixed([u8; 4]),
    /// A name for `netd`'s `resolve`.
    Name(String),
}

/// What `getaddrinfo` decided before it asks anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub host: Host,
    pub port: u16,
    /// Hints' `AI_CANONNAME`.
    pub canonical: bool,
}

/// The plan for `node` and `service` under `hints`, or the `EAI_*` code
/// (ADR-0225 item 6). Only `AF_INET` and `SOCK_STREAM` exist: other families
/// are `EAI_FAMILY`, datagram and raw sockets `EAI_SOCKTYPE`. The node is a
/// strict dotted quad or a name `netd` could resolve; a spelling of an
/// address that is neither (`127.1`, `1.2.3`, an uppercase name) is
/// `EAI_NONAME` without a request. (`0x7f000001` is a name by the scope's
/// grammar, since its last label is not all digits: `netd` is asked and the
/// resolver says no such name.)
pub fn plan(node: Option<&[u8]>, service: Option<&[u8]>, hints: &Hints) -> Result<Plan, i32> {
    if hints.flags & !AI_KNOWN != 0 {
        return Err(EAI_BADFLAGS);
    }
    if node.is_none() && service.is_none() {
        return Err(EAI_NONAME);
    }
    if hints.flags & AI_CANONNAME != 0 && node.is_none() {
        return Err(EAI_BADFLAGS);
    }
    match hints.family {
        AF_UNSPEC | AF_INET => {}
        _ => return Err(EAI_FAMILY),
    }
    match hints.socktype {
        0 | SOCK_STREAM => {}
        SOCK_DGRAM | SOCK_RAW => return Err(EAI_SOCKTYPE),
        _ => return Err(EAI_SOCKTYPE),
    }
    match hints.protocol {
        0 | IPPROTO_TCP => {}
        _ => return Err(EAI_SOCKTYPE),
    }
    let port = match service {
        None => 0,
        Some(s) if hints.flags & AI_NUMERICSERV != 0 => numeric_port(s).ok_or(EAI_NONAME)?,
        Some(s) => service_port(s).ok_or(EAI_SERVICE)?,
    };
    let host = match node {
        None if hints.flags & AI_PASSIVE != 0 => Host::Fixed([0; 4]),
        None => Host::Fixed([127, 0, 0, 1]),
        Some(n) => match inet_pton4(n) {
            Some(a) => Host::Fixed(a),
            None if hints.flags & AI_NUMERICHOST != 0 => return Err(EAI_NONAME),
            None => {
                let name = core::str::from_utf8(n)
                    .ok()
                    .filter(|n| zoraya_abi::net::valid_name(n))
                    .ok_or(EAI_NONAME)?;
                Host::Name(name.into())
            }
        },
    };
    Ok(Plan {
        host,
        port,
        canonical: hints.flags & AI_CANONNAME != 0,
    })
}

// ------------------------------------------------------------ the cache

/// The names of the last `getaddrinfo` results by address (ADR-0225 item 3):
/// a `connect` to an address that came from a resolution of name `N` is
/// sent as `connect(Name(N), port)`. Bounded; the oldest goes first. A
/// program that edits it gains nothing: a connect by name is checked by
/// name, and by address if it came from nowhere.
#[derive(Debug, Default, Clone)]
pub struct AddrCache {
    entries: Vec<([u8; 4], String)>,
    next: usize,
}

impl AddrCache {
    pub fn remember(&mut self, address: [u8; 4], name: &str) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.0 == address) {
            e.1 = name.into();
            return;
        }
        if self.entries.len() < CACHE {
            self.entries.push((address, name.into()));
        } else {
            self.entries[self.next] = (address, name.into());
            self.next = (self.next + 1) % CACHE;
        }
    }

    pub fn name_of(&self, address: [u8; 4]) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.0 == address)
            .map(|e| e.1.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ------------------------------------------------------------ options

/// What a socket knows about itself, for `getsockopt`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub reuse: bool,
    pub keepalive: bool,
    pub nodelay: bool,
}

/// `setsockopt`: the options of ADR-0225 item 7 that a stream socket takes.
/// `TCP_NODELAY`, `SO_KEEPALIVE` and the buffer sizes are accepted and kept
/// here (no control message to `netd` exists yet); the timeouts are
/// accepted and not honoured; anything else is `ENOPROTOOPT`.
pub fn set_option(o: &mut Options, level: i32, name: i32, value: i32) -> Result<(), i32> {
    match (level, name) {
        (SOL_SOCKET, SO_REUSEADDR) => o.reuse = value != 0,
        (SOL_SOCKET, SO_KEEPALIVE) => o.keepalive = value != 0,
        (IPPROTO_TCP, TCP_NODELAY) => o.nodelay = value != 0,
        (
            SOL_SOCKET,
            SO_REUSEPORT | SO_RCVBUF | SO_SNDBUF | SO_RCVTIMEO | SO_SNDTIMEO | SO_BROADCAST
            | SO_DONTROUTE | SO_OOBINLINE,
        )
        | (IPPROTO_TCP, TCP_KEEPIDLE | TCP_KEEPINTVL | TCP_KEEPCNT) => {}
        _ => return Err(ENOPROTOOPT),
    }
    Ok(())
}

/// `getsockopt` for the options with an integer value; `SO_ERROR` is the
/// caller's (it clears the pending error).
pub fn get_option(
    o: &Options,
    kind: i32,
    accepting: bool,
    level: i32,
    name: i32,
) -> Result<i32, i32> {
    Ok(match (level, name) {
        (SOL_SOCKET, SO_REUSEADDR) => i32::from(o.reuse),
        (SOL_SOCKET, SO_KEEPALIVE) => i32::from(o.keepalive),
        (IPPROTO_TCP, TCP_NODELAY) => i32::from(o.nodelay),
        (SOL_SOCKET, SO_TYPE) => kind,
        (SOL_SOCKET, SO_DOMAIN) => AF_INET,
        (SOL_SOCKET, SO_PROTOCOL) => IPPROTO_TCP,
        (SOL_SOCKET, SO_ACCEPTCONN) => i32::from(accepting),
        (SOL_SOCKET, SO_RCVBUF | SO_SNDBUF) => RING,
        (SOL_SOCKET, SO_REUSEPORT | SO_BROADCAST | SO_DONTROUTE | SO_OOBINLINE) => 0,
        _ => return Err(ENOPROTOOPT),
    })
}

/// Checks `socket(domain, type, protocol)`: the type without its flags, and
/// the flags.
pub fn check_socket(domain: i32, ty: i32, protocol: i32) -> Result<(i32, bool, bool), i32> {
    if domain != AF_INET {
        return Err(EAFNOSUPPORT);
    }
    let nonblock = ty & SOCK_NONBLOCK != 0;
    let cloexec = ty & SOCK_CLOEXEC != 0;
    let kind = ty & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
    if kind != SOCK_STREAM {
        // Datagram sockets are not built (N4); raw ones never (ADR-0225).
        return Err(EPROTONOSUPPORT);
    }
    if protocol != 0 && protocol != IPPROTO_TCP {
        return Err(EPROTONOSUPPORT);
    }
    Ok((kind, nonblock, cloexec))
}

/// The state of a socket fd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Made by `socket()`, nothing asked of `netd` yet.
    Fresh,
    /// `connect` was answered `Connecting`; `Connected` or `Failed` is
    /// still to come on the control channel.
    Connecting,
    Connected,
    /// A `connect` failed, or the socket was shut in both directions: only
    /// `close` is left.
    Dead,
}

/// A socket fd's description (ADR-0225 item 2): the three handles of a
/// stream socket when it is connecting or connected.
#[derive(Debug)]
pub struct Socket<H> {
    pub kind: i32,
    pub state: State,
    pub nonblock: bool,
    /// The stream `netd` writes the peer's bytes to.
    pub read: Option<H>,
    /// The stream `netd` reads our bytes from.
    pub write: Option<H>,
    /// The connection channel.
    pub ctl: Option<H>,
    /// A pending `SO_ERROR`, from a failed connect or a reset.
    pub error: i32,
    pub peer: Option<([u8; 4], u16)>,
    pub options: Options,
}

impl<H> Socket<H> {
    pub fn fresh(kind: i32, nonblock: bool) -> Socket<H> {
        Socket {
            kind,
            state: State::Fresh,
            nonblock,
            read: None,
            write: None,
            ctl: None,
            error: 0,
            peer: None,
            options: Options::default(),
        }
    }
}

impl<H: Copy> Socket<H> {
    /// What `select` waits on for this socket (ADR-0225 item 8): `None`
    /// when it is ready at once (a fresh or dead socket reports at once, as
    /// Linux does). A connecting socket is ready when its control channel
    /// has a message, for reading and writing alike; a connected one when
    /// its read end has bytes or an end, or its write end has room.
    pub fn wait_source(&self, writing: bool) -> Option<(H, u32)> {
        use crate::sys::signals::{PEER_CLOSED, READABLE, WRITABLE};
        match self.state {
            State::Fresh | State::Dead => None,
            State::Connecting => self.ctl.map(|c| (c, READABLE | PEER_CLOSED)),
            State::Connected if writing => self.write.map(|e| (e, WRITABLE | PEER_CLOSED)),
            State::Connected => self.read.map(|e| (e, READABLE | PEER_CLOSED)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sockaddr_round_trips_and_short_or_foreign_ones_are_refused() {
        let sa = sockaddr_in([10, 0, 2, 2], 8080);
        assert_eq!(parse_sockaddr_in(&sa), Ok(([10, 0, 2, 2], 8080)));
        assert_eq!(&sa[2..4], &[0x1f, 0x90]);
        assert_eq!(parse_sockaddr_in(&sa[..15]), Err(EINVAL));
        let mut v6 = sa;
        v6[..2].copy_from_slice(&(AF_INET6 as u16).to_ne_bytes());
        assert_eq!(parse_sockaddr_in(&v6), Err(EAFNOSUPPORT));
        // Total on any input.
        for n in 0..40 {
            let _ = parse_sockaddr_in(&[0xff; 40][..n]);
        }
    }

    #[test]
    fn inet_pton_takes_only_four_plain_decimals() {
        assert_eq!(inet_pton4(b"1.2.3.4"), Some([1, 2, 3, 4]));
        assert_eq!(inet_pton4(b"0.0.0.0"), Some([0; 4]));
        assert_eq!(inet_pton4(b"255.255.255.255"), Some([255; 4]));
        for bad in [
            &b""[..],
            b"1.2.3",
            b"1.2.3.4.5",
            b"256.1.1.1",
            b"01.2.3.4",
            b"1.2.3.",
            b".1.2.3",
            b"0x7f.0.0.1",
            b"127.1",
            b"1.2.3.4 ",
            b"1..3.4",
            b"-1.2.3.4",
            b"1.2.3.+4",
            b"1.2.3.4000",
        ] {
            assert_eq!(inet_pton4(bad), None, "{:?}", core::str::from_utf8(bad));
        }
    }

    #[test]
    fn inet_aton_takes_the_historical_forms() {
        assert_eq!(inet_aton(b"127.1"), Some([127, 0, 0, 1]));
        assert_eq!(inet_aton(b"0x7f000001"), Some([127, 0, 0, 1]));
        assert_eq!(inet_aton(b"0177.0.0.1"), Some([127, 0, 0, 1]));
        assert_eq!(inet_aton(b"1.2.3.4 trailing"), Some([1, 2, 3, 4]));
        assert_eq!(inet_aton(b"1.2.3.256"), None);
        assert_eq!(inet_aton(b"256.1"), None);
        assert_eq!(inet_aton(b"1.65536"), Some([1, 1, 0, 0]));
        assert_eq!(inet_aton(b"1.16777216"), None);
        assert_eq!(inet_aton(b"4294967296"), None);
        assert_eq!(inet_aton(b""), None);
        assert_eq!(inet_aton(b"1.2.3.4.5"), None);
        assert_eq!(inet_aton(b"08"), None);
    }

    #[test]
    fn dotted_text_is_what_ntop_gives() {
        assert_eq!(ntop4([192, 0, 2, 7]), "192.0.2.7");
    }

    #[test]
    fn services_are_the_table_or_a_number() {
        assert_eq!(service_port(b"http"), Some(80));
        assert_eq!(service_port(b"https"), Some(443));
        assert_eq!(service_port(b"8080"), Some(8080));
        assert_eq!(service_port(b"gopher"), None);
        assert_eq!(numeric_port(b"65535"), Some(65535));
        assert_eq!(numeric_port(b"65536"), None);
        assert_eq!(numeric_port(b"http"), None);
        assert_eq!(numeric_port(b""), None);
        assert_eq!(numeric_port(b"123456"), None);
    }

    #[test]
    fn statuses_and_reasons_have_their_errnos() {
        assert_eq!(connect_status(status::NOT_GRANTED), EACCES);
        assert_eq!(connect_status(status::OVER_QUOTA), ENOBUFS);
        assert_eq!(connect_status(status::UNAVAILABLE), ENETUNREACH);
        assert_eq!(connect_status(999), crate::errno::EIO);
        assert_eq!(failed_reason(reason::REFUSED), ECONNREFUSED);
        assert_eq!(failed_reason(reason::TIMED_OUT), ETIMEDOUT);
        assert_eq!(failed_reason(reason::LOCAL), EHOSTUNREACH);
        assert_eq!(failed_reason(999), crate::errno::EIO);
        assert_eq!(reset_why(reset::PEER), ECONNRESET);
        assert_eq!(reset_why(reset::TIMED_OUT), ETIMEDOUT);
        assert_eq!(reset_why(reset::ABORTED), ECONNABORTED);
        assert_eq!(resolve_status(status::NOT_GRANTED), (EAI_SYSTEM, EACCES));
        assert_eq!(resolve_status(status::UNRESOLVABLE), (EAI_NONAME, 0));
        assert_eq!(resolve_status(status::UNAVAILABLE), (EAI_AGAIN, 0));
        assert_eq!(resolve_status(77), (EAI_FAIL, 0));
        assert_eq!(gai_message(EAI_NONAME), b"Name does not resolve\0");
        assert_eq!(gai_message(-99), b"Unknown error\0");
    }

    #[test]
    fn the_plan_follows_the_hints() {
        let none = Hints::default();
        assert_eq!(
            plan(Some(b"1.2.3.4"), Some(b"80"), &none),
            Ok(Plan {
                host: Host::Fixed([1, 2, 3, 4]),
                port: 80,
                canonical: false
            })
        );
        let named = plan(Some(b"example.org"), Some(b"https"), &none).unwrap();
        assert_eq!(named.host, Host::Name("example.org".into()));
        assert_eq!(named.port, 443);
        assert_eq!(plan(None, None, &none), Err(EAI_NONAME));
        assert_eq!(
            plan(None, Some(b"80"), &none).unwrap().host,
            Host::Fixed([127, 0, 0, 1])
        );
        let passive = Hints {
            flags: AI_PASSIVE,
            ..none
        };
        assert_eq!(
            plan(None, Some(b"80"), &passive).unwrap().host,
            Host::Fixed([0; 4])
        );
        let numeric = Hints {
            flags: AI_NUMERICHOST,
            ..none
        };
        assert_eq!(plan(Some(b"example.org"), None, &numeric), Err(EAI_NONAME));
        let numserv = Hints {
            flags: AI_NUMERICSERV,
            ..none
        };
        assert_eq!(
            plan(Some(b"1.2.3.4"), Some(b"http"), &numserv),
            Err(EAI_NONAME)
        );
        assert_eq!(
            plan(
                Some(b"1.2.3.4"),
                None,
                &Hints {
                    flags: 0x8000,
                    ..none
                }
            ),
            Err(EAI_BADFLAGS)
        );
        assert_eq!(
            plan(
                None,
                Some(b"80"),
                &Hints {
                    flags: AI_CANONNAME,
                    ..none
                }
            ),
            Err(EAI_BADFLAGS)
        );
        assert_eq!(
            plan(
                Some(b"1.2.3.4"),
                None,
                &Hints {
                    family: AF_INET6,
                    ..none
                }
            ),
            Err(EAI_FAMILY)
        );
        assert_eq!(
            plan(
                Some(b"1.2.3.4"),
                None,
                &Hints {
                    socktype: SOCK_DGRAM,
                    ..none
                }
            ),
            Err(EAI_SOCKTYPE)
        );
        assert_eq!(
            plan(
                Some(b"1.2.3.4"),
                None,
                &Hints {
                    protocol: 17,
                    ..none
                }
            ),
            Err(EAI_SOCKTYPE)
        );
        assert_eq!(
            plan(Some(b"1.2.3.4"), Some(b"nosuch"), &none),
            Err(EAI_SERVICE)
        );
        // Uppercase, a trailing dot, a space, an address spelled oddly.
        for bad in [
            &b"Example.org"[..],
            b"example.org.",
            b"a b",
            b"127.1",
            b"1.2.3",
            b"\xff",
        ] {
            assert_eq!(plan(Some(bad), None, &none), Err(EAI_NONAME), "{bad:?}");
        }
        let canon = plan(
            Some(b"example.org"),
            None,
            &Hints {
                flags: AI_CANONNAME,
                ..none
            },
        )
        .unwrap();
        assert!(canon.canonical);
    }

    #[test]
    fn the_cache_is_bounded_and_keeps_the_newest() {
        let mut c = AddrCache::default();
        assert!(c.is_empty());
        for i in 0..(CACHE as u32 + 10) {
            c.remember(i.to_be_bytes(), &alloc::format!("h{i}"));
        }
        assert_eq!(c.len(), CACHE);
        assert_eq!(c.name_of((CACHE as u32 + 9).to_be_bytes()), Some("h73"));
        // The oldest went first.
        assert_eq!(c.name_of(0u32.to_be_bytes()), None);
        assert_eq!(c.name_of((CACHE as u32).to_be_bytes()), Some("h64"));
        // A name seen again replaces the old one for that address.
        c.remember(70u32.to_be_bytes(), "other");
        assert_eq!(c.name_of(70u32.to_be_bytes()), Some("other"));
        assert_eq!(c.len(), CACHE);
    }

    #[test]
    fn the_options_table_accepts_what_the_adr_says() {
        let mut o = Options::default();
        set_option(&mut o, SOL_SOCKET, SO_KEEPALIVE, 1).unwrap();
        set_option(&mut o, IPPROTO_TCP, TCP_NODELAY, 1).unwrap();
        set_option(&mut o, SOL_SOCKET, SO_REUSEADDR, 1).unwrap();
        set_option(&mut o, SOL_SOCKET, SO_RCVTIMEO, 0).unwrap();
        assert_eq!(set_option(&mut o, SOL_SOCKET, 99, 1), Err(ENOPROTOOPT));
        assert_eq!(
            get_option(&o, SOCK_STREAM, false, SOL_SOCKET, SO_KEEPALIVE),
            Ok(1)
        );
        assert_eq!(
            get_option(&o, SOCK_STREAM, false, IPPROTO_TCP, TCP_NODELAY),
            Ok(1)
        );
        assert_eq!(
            get_option(&o, SOCK_STREAM, false, SOL_SOCKET, SO_RCVBUF),
            Ok(RING)
        );
        assert_eq!(
            get_option(&o, SOCK_STREAM, true, SOL_SOCKET, SO_ACCEPTCONN),
            Ok(1)
        );
        assert_eq!(
            get_option(&o, SOCK_STREAM, false, SOL_SOCKET, 99),
            Err(ENOPROTOOPT)
        );
    }

    #[test]
    fn a_socket_call_is_checked_for_family_type_and_protocol() {
        assert_eq!(
            check_socket(AF_INET, SOCK_STREAM, 0),
            Ok((SOCK_STREAM, false, false))
        );
        assert_eq!(
            check_socket(
                AF_INET,
                SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
                IPPROTO_TCP
            ),
            Ok((SOCK_STREAM, true, true))
        );
        assert_eq!(check_socket(AF_INET6, SOCK_STREAM, 0), Err(EAFNOSUPPORT));
        assert_eq!(check_socket(AF_INET, SOCK_DGRAM, 0), Err(EPROTONOSUPPORT));
        assert_eq!(check_socket(AF_INET, SOCK_RAW, 0), Err(EPROTONOSUPPORT));
        assert_eq!(check_socket(AF_INET, SOCK_STREAM, 17), Err(EPROTONOSUPPORT));
    }

    #[test]
    fn what_a_socket_is_waited_on_by_follows_its_state() {
        use crate::sys::signals::{PEER_CLOSED, READABLE, WRITABLE};
        let mut s: Socket<u8> = Socket::fresh(SOCK_STREAM, false);
        assert_eq!(s.wait_source(false), None);
        s.ctl = Some(1);
        s.read = Some(2);
        s.write = Some(3);
        s.state = State::Connecting;
        assert_eq!(s.wait_source(true), Some((1, READABLE | PEER_CLOSED)));
        assert_eq!(s.wait_source(false), Some((1, READABLE | PEER_CLOSED)));
        s.state = State::Connected;
        assert_eq!(s.wait_source(false), Some((2, READABLE | PEER_CLOSED)));
        assert_eq!(s.wait_source(true), Some((3, WRITABLE | PEER_CLOSED)));
        s.read = None;
        assert_eq!(s.wait_source(false), None);
        s.state = State::Dead;
        assert_eq!(s.wait_source(true), None);
    }
}

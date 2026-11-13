//! The protocol on a network connection (ADR-0220, ADR-0222 item 1 and 6,
//! ADR-0229): the scope channel of `netd`. N2 declares what `netd` answers
//! now: `status`, `ping` (a diagnostic echo, proposed ADR-0281), `narrow`
//! and, since N3, `connect` for TCP with the client's stream ends
//! (ADR-0222 item 3 and its note of 2026-11-10; proposed ADR-0286).
//! `datagram`, `listen` and `resolve` come later, each with a new ordinal; an
//! unknown ordinal gets `ErrorReply` and the connection stays.
//!
//! One request at a time per connection (ADR-0055); `ping`'s reply comes when
//! the run is over, so the connection is busy until then. Addresses are IPv4
//! in a `u32`, most significant byte first. Every message decodes totally
//! (INV-0222-3).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use zoraya_abi::net::MAX_SCOPE_BYTES;
use zoraya_protocol::{Below, Data, Seq, protocol};

/// One of `status`.
pub type Code = Below<8>;

/// The status of every reply.
pub mod status {
    pub const OK: u32 = 0;
    /// The right or the scope does not allow it (ADR-0220 item 3).
    pub const NOT_GRANTED: u32 = 1;
    /// A `ping` is running on this connection already, or `netd` runs its
    /// bound of them.
    pub const BUSY: u32 = 2;
    /// The request is well formed but its content is not usable (a scope
    /// that does not decode, an address that is not unicast).
    pub const INVALID: u32 = 3;
    /// The network has no address yet, or no route.
    pub const UNAVAILABLE: u32 = 4;
    /// The request would pass a bound of the connection's account
    /// (ADR-0262; N4): `netd` carves no account, or a `Narrow` with a
    /// budget asks for more than the account or the column can give.
    pub const OVER_QUOTA: u32 = 5;
    /// A `resolve`: the name has no address (no such name, no A record, or
    /// no answer in time).
    pub const UNRESOLVABLE: u32 = 6;
    /// A `resolve`: every address the name has is local and no address rule
    /// covers it (ADR-0223 item 4).
    pub const LOCAL: u32 = 7;
}

/// Why a `connect` failed, as `Failed` says it on the connection channel
/// (ADR-0222 item 3).
pub mod reason {
    /// The name has no address, or the servers did not answer.
    pub const UNRESOLVABLE: u32 = 0;
    /// The peer answered with a reset.
    pub const REFUSED: u32 = 1;
    /// No route, or the interface has no address.
    pub const UNREACHABLE: u32 = 2;
    /// No answer from the peer in time.
    pub const TIMED_OUT: u32 = 3;
    /// The name resolved to an address of this machine or its side of the
    /// network and no address rule covers it (ADR-0223 item 4).
    pub const LOCAL: u32 = 4;
    /// A bound of `netd` (connections at once), until accounts (ADR-0056).
    pub const OVER_LIMIT: u32 = 5;
    /// A bound of the connection's account (ADR-0262 item 2, ADR-0263
    /// item 4; N4): connections or pending resolutions. `libposix` says
    /// `ENOBUFS`.
    pub const OVER_QUOTA: u32 = 6;
}

/// One of `reason`.
pub type Reason = Below<7>;

/// Why an established connection ended abnormally, as `Reset` says it.
pub mod reset {
    /// The peer sent a reset.
    pub const PEER: u32 = 0;
    /// Nothing came back for too long.
    pub const TIMED_OUT: u32 = 1;
    /// `netd` ended it (the client closed a stream end it must keep, or the
    /// network went away).
    pub const ABORTED: u32 = 2;
}

/// Where the interface's address came from.
pub mod source {
    pub const NONE: u32 = 0;
    pub const DHCP: u32 = 1;
    pub const STATIC: u32 = 2;
}

/// The most addresses one `resolve` answers.
pub const MAX_ADDRESSES: usize = 8;
/// The most DNS servers `status` carries.
pub const MAX_DNS: usize = 4;
/// The most echo requests of one `ping`.
pub const MAX_PINGS: u32 = 16;
/// `MAX_PINGS` as a length.
pub const MAX_PINGS_USIZE: usize = 16;
/// `Rtt::us` of an echo request that was not answered.
pub const NO_REPLY: u32 = u32::MAX;
/// The bytes of one echo request or reply: the 8-byte ICMP header and 32 of
/// data. What `ping` prints, as the other pings do, is this size.
pub const ECHO_BYTES: u32 = 40;
/// `StatusReply::lease_left` when there is no lease.
pub const NO_LEASE: u32 = u32::MAX;

protocol! {
    /// An IPv4 address.
    record Address {
        address: u32,
    }

    /// The round trip of one echo request, in microseconds, or `NO_REPLY`.
    record Rtt {
        us: u32,
    }

    /// The state of the interface (right `status`).
    message GetStatus = 1, handles 0 {}
    message StatusReply<'a> = 1, handles 0 {
        status: Code,
        link: bool,
        /// One of `source`: how the address was configured.
        address_source: Below<3>,
        /// One of `source`: where the DNS servers came from.
        dns_source: Below<3>,
        prefix: Below<33>,
        address: u32,
        gateway: u32,
        /// Seconds of the DHCP lease left, or `NO_LEASE`.
        lease_left: u32,
        frames_in: u64,
        frames_out: u64,
        frames_dropped: u64,
        mac: Data<'a, 6>,
        dns: Seq<'a, Address, MAX_DNS>,
    }

    /// `count` echo requests to `address`, one every 200 ms, answered when
    /// the last reply or the timeout is in (rights `reach`, and a reach rule
    /// covering the address). A diagnostic, not a socket (proposed
    /// ADR-0281).
    message Ping = 2, handles 0 {
        address: u32,
        count: Below<17>,
        /// How long to wait for the last reply, in milliseconds.
        timeout_ms: Below<30001>,
    }
    message PingReply<'a> = 2, handles 0 {
        status: Code,
        sent: u32,
        received: u32,
        /// Round trips in microseconds; zero when nothing came back.
        rtt_min_us: u32,
        rtt_avg_us: u32,
        rtt_max_us: u32,
        /// One entry per request sent, in order (proposed ADR-0300): the
        /// round trip of that request, or `NO_REPLY`.
        rtts: Seq<'a, Rtt, MAX_PINGS_USIZE>,
    }

    /// Opens a TCP connection to `name` (a host name; empty to use
    /// `address`) and `port` (right `reach`, and a reach rule covering the
    /// target). The three handles, in order: the server end of the
    /// connection channel the client made, the *write* end of the stream the
    /// client reads the peer's bytes from, and the *read* end of the stream
    /// the client writes its bytes to; the client keeps the other ends, so
    /// the 128 KiB of the connection are its own room's (ADR-0222's note of
    /// 2026-11-10). `netd` replies at once (`Connecting`), then resolves
    /// and connects, and sends `Connected` or `Failed` on the connection
    /// channel.
    message Connect<'a> = 3, handles 3 {
        address: u32,
        port: Below<65536>,
        name: Data<'a, 253>,
    }
    message Connecting = 3, handles 0 {
        status: Code,
    }

    /// The addresses of `name` (right `reach`, and a reach rule whose target
    /// covers the name on some port; ADR-0222 item 2, ADR-0223). One at a
    /// time per connection. Answered when the resolver has an answer, at most
    /// 8 addresses; addresses that are local and not covered by an address
    /// rule are left out.
    message Resolve<'a> = 4, handles 0 {
        name: Data<'a, 253>,
    }
    message Resolved<'a> = 4, handles 0 {
        status: Code,
        addresses: Seq<'a, Address, MAX_ADDRESSES>,
    }

    /// On the connection channel, from `netd`: the connection is open.
    message Connected = 16, handles 0 {
        address: u32,
        port: Below<65536>,
    }
    /// On the connection channel, from `netd`: it never opened. The stream
    /// ends netd holds are closed after this.
    message Failed = 17, handles 0 {
        reason: Reason,
    }
    /// On the connection channel, from `netd`: it ended badly; the stream
    /// ends are closed after this, data first (a reader sees what arrived
    /// before the reset, then the end).
    message Reset = 18, handles 0 {
        why: Below<3>,
    }

    /// Attaches a new connection with a scope inside this one's and rights no
    /// wider (right `narrow`, ADR-0220 item 5). The handle is the server
    /// end of a channel the caller made.
    message Narrow<'a> = 6, handles 1 {
        rights: u32,
        scope: Data<'a, MAX_SCOPE_BYTES>,
    }
    message Narrowed = 6, handles 0 {
        status: Code,
    }

    /// `Narrow` that also carves an *account* for the new connection and
    /// the ones narrowed from it (ADR-0262 item 2; N4): up to `budget`
    /// connections at once (1 to `MAX_BUDGET`), and the pending
    /// resolutions, listeners, datagram sockets and resolver cache entries
    /// that follow from it. `Narrow` is unchanged and joins the caller's
    /// account. Answered with `Narrowed`.
    message NarrowAccount<'a> = 7, handles 1 {
        rights: u32,
        budget: Below<17>,
        scope: Data<'a, MAX_SCOPE_BYTES>,
    }

    /// On a launcher's *services channel*, from `init` (ADR-0221 item 6,
    /// proposed ADR-0296): the network was restarted, and the handle is the
    /// launcher's new root connection. Not a request: `netd` never sees it.
    message NetworkRestarted = 32, handles 1 {}
    /// On the services channel, from `init`: the restarts of this boot are
    /// used up (at most three, ADR-0221 item 5); no root connection will come.
    message NetworkDown = 33, handles 0 {}

    /// Ends `netd` at once, so that a test can see `init`'s restart (INV-0221-3,
    /// INV-0221-4). Honoured only by a `netd` that `init` started with
    /// an event handle at slot 8, which it does only when the volume holds
    /// `/etc/zoraya/net-crash`; any other `netd` answers it as it answers
    /// an unknown ordinal. Proposed ADR-0296.
    message Crash = 40, handles 0 {}
}

/// `Crash`'s ordinal, for a `netd` that checks it before the service does.
pub const CRASH_ORDINAL: u32 = 40;

/// The most connections one carved account may be given (ADR-0262's
/// `--conns`, scaled to `netd`'s 16: a quarter).
pub const MAX_BUDGET: u32 = 4;

/// Every ordinal, for a test that none repeats.
pub const ORDINALS: [(&str, u32); 12] = [
    ("status", 1),
    ("ping", 2),
    ("connect", 3),
    ("resolve", 4),
    ("narrow", 6),
    ("narrow_account", 7),
    ("connected", 16),
    ("failed", 17),
    ("reset", 18),
    ("network_restarted", 32),
    ("network_down", 33),
    ("crash", 40),
];

fn dotted(a: u32) -> String {
    let b = a.to_be_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

fn duration(seconds: u32) -> String {
    let (h, m) = (seconds / 3600, seconds % 3600 / 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {}s", seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

/// `net status`'s text (ADR-0229 item 4): the state in words.
pub fn render(s: &StatusReply<'_>) -> String {
    let mac = s.mac.0;
    let mut out = String::new();
    let state = if s.link { "up" } else { "down" };
    if mac.len() == 6 {
        out += &format!(
            "net0  {state}  {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
    } else {
        out += &format!("net0  {state}\n");
    }
    out += &match s.address_source.0 {
        source::DHCP | source::STATIC if s.address != 0 => {
            let how = if s.address_source.0 == source::DHCP {
                "dhcp"
            } else {
                "static"
            };
            let lease = if s.lease_left == NO_LEASE {
                String::new()
            } else {
                format!(", lease {} left", duration(s.lease_left))
            };
            format!(
                "  address   {}/{} ({how}{lease})\n",
                dotted(s.address),
                s.prefix.0
            )
        }
        _ => String::from("  address   none\n"),
    };
    if s.gateway != 0 {
        out += &format!("  gateway   {}\n", dotted(s.gateway));
    }
    if !s.dns.is_empty() {
        let how = match s.dns_source.0 {
            source::DHCP => "dhcp",
            source::STATIC => "net.dns",
            _ => "none",
        };
        for (i, d) in s.dns.iter().enumerate() {
            let label = if i == 0 { "dns" } else { "" };
            out += &format!("  {label:<9} {} ({how})\n", dotted(d.address));
        }
    }
    out += &format!(
        "  frames    {} in, {} out, {} dropped\n",
        s.frames_in, s.frames_out, s.frames_dropped
    );
    out
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

#[cfg(test)]
zoraya_test_alloc::cap_memory!();

/// One run of `ping`, as the command shows it (proposed ADR-0300).
#[derive(Debug, Clone, Copy)]
pub struct PingView<'a> {
    pub address: [u8; 4],
    pub sent: u32,
    pub received: u32,
    pub rtt_min_us: u32,
    pub rtt_avg_us: u32,
    pub rtt_max_us: u32,
    /// Per request, in order; `NO_REPLY` for none.
    pub rtts: &'a [u32],
}

/// Microseconds as `1.234` milliseconds.
fn millis(us: u32) -> String {
    format!("{}.{:03}", us / 1000, us % 1000)
}

/// `ping`'s text: one line per reply (`40 bytes from A: seq=N time=T ms`),
/// then the summary with the loss in whole percent (rounded down) and the
/// shortest, mean and longest round trip. A request with no reply has no line.
pub fn render_ping(v: &PingView<'_>) -> String {
    let [a, b, c, d] = v.address;
    let mut out = String::new();
    for (seq, us) in v.rtts.iter().enumerate() {
        if *us != NO_REPLY {
            out += &format!(
                "{ECHO_BYTES} bytes from {a}.{b}.{c}.{d}: seq={seq} time={} ms\n",
                millis(*us)
            );
        }
    }
    let loss = if v.sent == 0 {
        100
    } else {
        100 - (u64::from(v.received.min(v.sent)) * 100 / u64::from(v.sent)) as u32
    };
    out += &format!("--- {a}.{b}.{c}.{d} ping statistics ---\n");
    out += &format!("{} sent, {} received, {loss}% loss\n", v.sent, v.received);
    if v.received > 0 {
        out += &format!(
            "rtt min/avg/max = {}/{}/{} ms\n",
            millis(v.rtt_min_us),
            millis(v.rtt_avg_us),
            millis(v.rtt_max_us)
        );
    }
    out
}

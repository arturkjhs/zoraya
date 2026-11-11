//! The protocol on a network connection (ADR-0220, ADR-0222 item 1 and 6,
//! ADR-0229): the scope channel of `netd`. N2 declares what `netd` answers
//! now: `status`, `ping` (a diagnostic echo, proposed ADR-0281) and `narrow`.
//! `connect`, `datagram`, `listen` and `resolve` come with N3, each with a
//! new ordinal; an unknown ordinal gets `ErrorReply` and the connection stays.
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
pub type Code = Below<5>;

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
}

/// Where the interface's address came from.
pub mod source {
    pub const NONE: u32 = 0;
    pub const DHCP: u32 = 1;
    pub const STATIC: u32 = 2;
}

/// The most DNS servers `status` carries.
pub const MAX_DNS: usize = 4;
/// The most echo requests of one `ping`.
pub const MAX_PINGS: u32 = 16;
/// `StatusReply::lease_left` when there is no lease.
pub const NO_LEASE: u32 = u32::MAX;

protocol! {
    /// An IPv4 address.
    record Address {
        address: u32,
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
    message PingReply = 2, handles 0 {
        status: Code,
        sent: u32,
        received: u32,
        /// Round trips in microseconds; zero when nothing came back.
        rtt_min_us: u32,
        rtt_avg_us: u32,
        rtt_max_us: u32,
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
}

/// Every ordinal, for a test that none repeats.
pub const ORDINALS: [(&str, u32); 3] = [("status", 1), ("ping", 2), ("narrow", 6)];

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

#![no_std]
#![no_main]

//! `ping [-c COUNT] [-W SECONDS] HOST` (ADR-0330): echo requests through the
//! network service's diagnostic `ping` (ADR-0281 item 3; there is no raw
//! socket). The shell gave this program a connection whose only right on HOST
//! is ICMP echo (the `host` operand, ADR-0224 item 2); a name is resolved by
//! the service, inside the grant. The service answers when the run ends, so
//! the lines come at the end.

use zoraya_proto_net::{MAX_PINGS, PingView, render_ping, status};
use zoraya_user::{command, namespace, netclient};

zoraya_user::describe!("commands", "ping");

/// A whole number in `low..=high`, or the problem in words.
fn number(
    c: &command::Command,
    value: Option<&str>,
    default: u32,
    (low, high): (u32, u32),
    what: &str,
) -> Result<u32, ()> {
    let Some(text) = value else {
        return Ok(default);
    };
    match zoraya_describe::parse_number(text)
        .filter(|n| (i64::from(low)..=i64::from(high)).contains(n))
    {
        Some(n) => Ok(n as u32),
        None => {
            c.error(Some(text), what);
            Err(())
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    let Some(host) = p.operand(&c.d, "HOST") else {
        c.error(None, "no host");
        return 2;
    };
    let Ok(count) = number(
        &c,
        p.value(&c.d, "count"),
        4,
        (1, MAX_PINGS),
        "not a count from 1 to 16",
    ) else {
        return 2;
    };
    let Ok(seconds) = number(
        &c,
        p.value(&c.d, "timeout"),
        2,
        (1, 30),
        "not a number of seconds from 1 to 30",
    ) else {
        return 2;
    };
    let lower = host.to_ascii_lowercase();
    let address = zoraya_http::parse_ipv4(&lower);
    if address.is_none() && !zoraya_abi::net::valid_name(&lower) {
        c.error(Some(host), "not a host name or a dotted IPv4 address");
        return 2;
    }
    let Some(net) = namespace::get().and_then(|n| n.net()) else {
        c.error(None, "the network is not available to this command");
        return 1;
    };
    let name = if address.is_some() {
        ""
    } else {
        lower.as_str()
    };
    // `netd` answers every ping, a lookup that timed out included; no reply
    // within its bound is a `netd` that is gone or stuck.
    let Some(r) = netclient::ping_host(net, name, address.unwrap_or([0; 4]), count, seconds * 1000)
    else {
        c.error(None, "the network service did not answer");
        return 1;
    };
    match r.status {
        status::OK => {}
        status::NOT_GRANTED => {
            c.error(
                Some(host),
                "the network is not granted to this command for that host",
            );
            return 1;
        }
        status::INVALID => {
            c.error(
                Some(host),
                "not a host that can be pinged (this machine, or not a unicast address)",
            );
            return 1;
        }
        status::UNRESOLVABLE => {
            c.error(Some(host), "the name has no address");
            return 1;
        }
        status::TIMED_OUT => {
            c.error(Some(host), "the name lookup timed out");
            return 1;
        }
        status::LOCAL => {
            c.error(Some(host), "the name points at this machine's own network");
            return 1;
        }
        status::BUSY => {
            c.error(None, "another ping is running; try again");
            return 1;
        }
        _ => {
            c.error(None, "the network is not available");
            return 1;
        }
    }
    let view = PingView {
        host: &lower,
        address: r.address,
        count,
        sent: r.sent,
        received: r.received,
        rtt_min_us: r.rtt_min_us,
        rtt_avg_us: r.rtt_avg_us,
        rtt_max_us: r.rtt_max_us,
        rtts: &r.rtts,
    };
    zoraya_user::print!("{}", render_ping(&view));
    if r.received == 0 { 1 } else { 0 }
}

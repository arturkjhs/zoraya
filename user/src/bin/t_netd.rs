#![no_std]
#![no_main]

//! N2's acceptance test (docs/research/network/plan.md, ADR-0221, ADR-0222,
//! ADR-0229, ADR-0230): `netd` over `ethd` on a real virtio-net device under
//! `--net`. This program plays `init`: it starts both as `init` does, makes
//! connections with `narrow`, and checks what `net status` prints (the DHCP
//! lease SLIRP gives), a ping of SLIRP's gateway through `netd`, the scope
//! of narrowed connections, hostile requests, and what happens to a client
//! and to the device when `netd` or `ethd` is killed with frames in flight
//! (INV-0221-3 for a native client; the `libposix` half comes with the
//! sockets). Without a network device it passes having done nothing.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_abi::net::{
    ListenRule, Ports, Protocols, ReachRule, Scope, Target, encode as encode_scope, rights,
};
use zoraya_abi::{ExitReason, ExitRecord, Handle, Mode, Packet, Status, process_signals};
use zoraya_proto_net::{GetStatus, StatusReply, render, source, status};
use zoraya_protocol::{ErrorReply, decode, encode};
use zoraya_user::gui::scan;
use zoraya_user::netclient::{self, Network, root_scope, start_network};
use zoraya_user::{
    channel_read, clock_monotonic, handle_close, port_create, port_subscribe, port_wait, println,
    process_exit_record, sleep_until,
};

const GATEWAY: [u8; 4] = [10, 0, 2, 2];
const DNS: [u8; 4] = [10, 0, 2, 3];
const OURS: [u8; 4] = [10, 0, 2, 15];
/// Every wait ends: TCG is slow, so seconds.
const LEASE_NS: u64 = 60_000_000_000;
const EXIT_NS: u64 = 15_000_000_000;

fn wait_exit_within(process: Handle, wait: u64) -> Option<ExitRecord> {
    let port = port_create().expect("port");
    port_subscribe(port, process, process_signals::TERMINATED, 1, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default()];
    let waited = port_wait(port, clock_monotonic() + wait, &mut packets);
    let _ = handle_close(port);
    waited.ok()?;
    process_exit_record(process).ok()
}

fn dotted(a: [u8; 4]) -> String {
    alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

/// A status reply, owned.
struct Seen {
    bytes: Vec<u8>,
}

impl Seen {
    fn get(&self) -> StatusReply<'_> {
        netclient::parse_status(&self.bytes).expect("a status reply")
    }
}

fn status_of(conn: Handle) -> Option<Seen> {
    Some(Seen {
        bytes: netclient::status(conn)?,
    })
}

fn one_rule(target: Target<'_>) -> Vec<u8> {
    rule_of(target, Protocols::ALL)
}

fn rule_of(target: Target<'_>, protocols: Protocols) -> Vec<u8> {
    let mut out = alloc::vec![0u8; 128];
    let rule = ReachRule {
        target,
        ports: Ports::ANY,
        protocols,
    };
    let n = encode_scope(&[rule], &[], &mut out).expect("a scope");
    out.truncate(n);
    out
}

/// Waits for the DHCP lease on `conn`; the status and how long it took.
fn lease(conn: Handle) -> (Seen, u64) {
    let started = clock_monotonic();
    loop {
        if let Some(seen) = status_of(conn)
            && seen.get().address != 0
        {
            let ms = (clock_monotonic() - started) / 1_000_000;
            return (seen, ms);
        }
        assert!(
            clock_monotonic() - started < LEASE_NS,
            "no DHCP lease in time"
        );
        sleep_until(clock_monotonic() + 100_000_000);
    }
}

/// Starts the network as `init` does and gives the first root connection.
fn bring_up(device: (Handle, u32)) -> (Network, Handle) {
    let net = start_network(device, None).expect("ethd and netd start");
    let root = netclient::narrow(net.control, rights::ALL, &root_scope()).expect("a root");
    (net, root)
}

fn check_lease(seen: &Seen, ms: u64) {
    let s = seen.get();
    assert_eq!(s.status.0, status::OK);
    assert_eq!(s.address, u32::from_be_bytes(OURS));
    assert_eq!(s.prefix.0, 24);
    assert_eq!(s.address_source.0, source::DHCP);
    assert!(s.link);
    assert_eq!(s.mac.0, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    // SLIRP under `restrict=on` (every test run, ADR-0230) leaves the router
    // and the DNS servers out of the lease, so the guest cannot reach beyond
    // the subnet; without `restrict` (`make run-net`) it gives 10.0.2.2 and
    // 10.0.2.3. Either is a correct lease, and nothing else is.
    assert!(s.gateway == 0 || s.gateway == u32::from_be_bytes(GATEWAY));
    let dns: Vec<u32> = s.dns.iter().map(|a| a.address).collect();
    assert!(
        dns.is_empty() || dns == [u32::from_be_bytes(DNS)],
        "{dns:?}"
    );
    let gateway = if s.gateway == 0 {
        String::from("none")
    } else {
        dotted(GATEWAY)
    };
    let servers = if dns.is_empty() {
        String::from("none")
    } else {
        dotted(DNS)
    };
    println!(
        "t_netd: lease {}/{} gateway {gateway} dns {servers} after {ms} ms",
        dotted(OURS),
        s.prefix.0,
    );
}

fn ping(conn: Handle, address: [u8; 4], count: u32) -> netclient::PingOutcome {
    netclient::ping(conn, address, count, 3000).expect("a ping reply")
}

/// The gate's `net-account-flood` (ADR-0268 point 7, INV-0262-1): a client
/// in an account of `BUDGET` connections opens connections without end, to an
/// on-link address that answers no ARP so that none ends by itself. `netd`
/// admits `BUDGET` and tells every other `Failed(OVER_QUOTA)` at once, while
/// the root connection (home) still connects, and the flooder's own
/// connections ending give its account back.
fn account_flood(root: Handle) {
    use zoraya_proto_net::{Failed, reason};
    const BUDGET: u32 = 3;
    const TRIES: u32 = 12;
    let account =
        netclient::narrow_account(root, rights::ALL, BUDGET, &root_scope()).expect("an account");
    let mut held = Vec::new();
    for _ in 0..TRIES {
        let ends = zoraya_user::netconn::start(account, "", [10, 0, 2, 99], 80)
            .expect("netd says Connecting");
        held.push(ends);
    }
    sleep_until(clock_monotonic() + 300_000_000);
    let (mut refused, mut pending, mut other) = (0u32, 0u32, 0u32);
    for (control, _, _) in &held {
        let mut buf = [0u8; 128];
        match zoraya_user::channel_read(*control, &mut buf, &mut []) {
            Ok(sizes) => {
                let m = &buf[..(sizes.bytes as usize).min(buf.len())];
                // Only a `Failed` (ordinal 17) can be waiting at this point.
                let (header, _) = zoraya_protocol::Header::decode(m).expect("a header");
                assert_eq!(header.ordinal, 17, "a Failed, nothing else");
                match decode::<Failed>(m, 0) {
                    Ok((_, f)) if f.reason.0 == reason::OVER_QUOTA => refused += 1,
                    _ => other += 1,
                }
            }
            Err(_) => pending += 1,
        }
    }
    println!(
        "t_netd: account of {BUDGET}: {TRIES} connections opened, {pending} pending, {refused} refused over quota, {other} ended otherwise"
    );
    assert_eq!(
        (pending + other, refused),
        (BUDGET, TRIES - BUDGET),
        "the account admits its budget and refuses the rest"
    );
    // Home is not affected: the root connection still connects (the gateway
    // refuses the port or accepts it; either way netd admitted it).
    match zoraya_user::netconn::connect(root, "", GATEWAY, 80, 3_000_000_000) {
        Ok(_) | Err(zoraya_user::netconn::ConnectError::Failed(reason::REFUSED)) => {}
        Err(e) => panic!("home's connection under the flood: {e:?}"),
    }
    for (control, a, b) in held {
        for h in [control, a, b] {
            let _ = handle_close(h);
        }
    }
    let _ = handle_close(account);
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let devices = scan();
    let Some(device) = devices.net else {
        println!("t_netd: no network device attached: skipped");
        return 0;
    };
    let began = clock_monotonic();
    let (net, root) = bring_up(device);

    // `net status`: the lease, as the command prints it.
    let (seen, ms) = lease(root);
    check_lease(&seen, ms);
    for line in render(&seen.get()).lines() {
        println!("t_netd: | {line}");
    }
    println!(
        "t_netd: network up in {} ms",
        (clock_monotonic() - began) / 1_000_000
    );

    // `ping 10.0.2.2`: SLIRP answers echo requests to its gateway.
    let r = ping(root, GATEWAY, 4);
    println!(
        "t_netd: ping {}: sent {} received {} rtt min/avg/max {}/{}/{} us",
        dotted(GATEWAY),
        r.sent,
        r.received,
        r.rtt_min_us,
        r.rtt_avg_us,
        r.rtt_max_us
    );
    assert_eq!((r.status, r.sent), (status::OK, 4));
    assert_eq!(r.rtts.len(), 4);
    assert_eq!(
        r.rtts
            .iter()
            .filter(|u| **u != zoraya_proto_net::NO_REPLY)
            .count() as u32,
        r.received
    );
    assert!(r.received >= 1, "the gateway answers");
    assert!(r.rtt_min_us > 0 && r.rtt_min_us <= r.rtt_max_us);
    // Requests that cannot leave (an on-link address that answers no ARP):
    // a run of more than the socket's four slots ends by its deadline with
    // everything lost, and the next run to the gateway still works (review
    // error 1; under `restrict=on` the lease has no router either).
    let lost = ping(root, [10, 0, 2, 99], 8);
    println!(
        "t_netd: ping 10.0.2.99: sent {} received {}",
        lost.sent, lost.received
    );
    assert_eq!((lost.status, lost.sent, lost.received), (status::OK, 8, 0));
    // INV-0330-1: every request is marked as without a reply.
    assert_eq!(lost.rtts, [zoraya_proto_net::NO_REPLY; 8]);
    let again = ping(root, GATEWAY, 4);
    assert_eq!(
        (again.status, again.sent, again.received),
        (status::OK, 4, 4)
    );
    // Not an address anyone can be pinged at.
    assert_eq!(
        ping(root, [0, 0, 0, 0], 1).status,
        status::INVALID,
        "the unspecified address"
    );

    // Scopes (ADR-0220): a status-only connection asks status and nothing
    // else; a connection for one address pings that address only; neither
    // can make a wider one.
    let status_only = netclient::narrow(root, rights::STATUS, Scope::EMPTY.as_bytes())
        .expect("a status-only connection");
    assert_eq!(
        status_of(status_only).expect("status").get().status.0,
        status::OK
    );
    assert_eq!(ping(status_only, GATEWAY, 1).status, status::NOT_GRANTED);
    assert_eq!(
        netclient::narrow(status_only, rights::ALL, &root_scope()).err(),
        Some(status::NOT_GRANTED)
    );
    let one = netclient::narrow(root, rights::REACH, &one_rule(Target::Address(GATEWAY)))
        .expect("a connection for one address");
    assert_eq!(ping(one, GATEWAY, 1).status, status::OK);
    assert_eq!(ping(one, DNS, 1).status, status::NOT_GRANTED);
    // ADR-0330: the grant `ping` gets is echo and nothing else. An echo-only
    // rule for the gateway answers an echo to it, and refuses an echo to any
    // other host, a name it does not name, and every connection (TCP) to the
    // gateway itself; a rule of TCP and UDP alone refuses the echo. (There is
    // no UDP request yet: the rule's UDP side is `zoraya-abi`'s test.)
    let echo_only = netclient::narrow(
        root,
        rights::REACH,
        &rule_of(Target::Address(GATEWAY), Protocols::ECHO),
    )
    .expect("an echo-only connection");
    let echoed = ping(echo_only, GATEWAY, 2);
    assert_eq!((echoed.status, echoed.received), (status::OK, 2));
    assert_eq!(echoed.address, GATEWAY);
    assert_eq!(ping(echo_only, DNS, 1).status, status::NOT_GRANTED);
    let named = netclient::ping_host(echo_only, "localhost", [0; 4], 1, 1000).expect("a reply");
    assert_eq!(named.status, status::NOT_GRANTED, "a name no rule names");
    for port in [80, 53, 8080] {
        assert!(
            matches!(
                zoraya_user::netconn::connect(echo_only, "", GATEWAY, port, 2_000_000_000),
                Err(zoraya_user::netconn::ConnectError::Refused(
                    status::NOT_GRANTED
                ))
            ),
            "an echo grant opened a TCP connection to port {port}"
        );
    }
    let tcp_only = netclient::narrow(
        root,
        rights::REACH,
        &rule_of(Target::Address(GATEWAY), Protocols::BOTH),
    )
    .expect("a TCP and UDP connection");
    assert_eq!(ping(tcp_only, GATEWAY, 1).status, status::NOT_GRANTED);
    // And the parent's echo-only scope cannot be widened by a child.
    assert_eq!(
        netclient::narrow(
            echo_only,
            rights::REACH,
            &one_rule(Target::Address(GATEWAY))
        )
        .err(),
        Some(status::NOT_GRANTED)
    );
    assert_eq!(
        status_of(one).expect("a reply").get().status.0,
        status::NOT_GRANTED,
        "no status right, no state"
    );
    assert_eq!(
        netclient::narrow(one, rights::REACH, &one_rule(Target::Any)).err(),
        Some(status::NOT_GRANTED),
        "narrow needs its right"
    );
    // A scope that does not decode.
    assert_eq!(
        netclient::narrow(root, rights::STATUS, &[1, 9, 9, 9]).err(),
        Some(status::INVALID)
    );
    // `listen` rules are scope too: a connection that only listens cannot ping.
    let mut bytes = alloc::vec![0u8; 64];
    let listen = ListenRule {
        ports: Ports::one(8080),
        protocols: Protocols::TCP,
    };
    let n = encode_scope(&[], &[listen], &mut bytes).expect("a listen scope");
    let listener =
        netclient::narrow(root, rights::LISTEN, &bytes[..n]).expect("a listening connection");
    assert_eq!(ping(listener, GATEWAY, 1).status, status::NOT_GRANTED);
    println!("t_netd: scopes held");

    // Hostile requests: an unknown ordinal gets an error and the connection
    // stays; a message that is not one ends that connection and no other.
    let mut unknown = alloc::vec![0u8; 64];
    let n = encode(&GetStatus {}, 5, &mut unknown).expect("fits");
    unknown[4..8].copy_from_slice(&77u32.to_le_bytes());
    let reply = netclient::exchange(one, &unknown[..n], &[], 5_000_000_000).expect("an error");
    assert_eq!(
        decode::<ErrorReply>(&reply, 0)
            .expect("error reply")
            .1
            .status,
        status::INVALID
    );
    assert!(netclient::exchange(one, &[1, 2, 3], &[], 2_000_000_000).is_none());
    assert!(
        netclient::exchange(one, &[1, 2, 3], &[], 2_000_000_000).is_none(),
        "it is closed"
    );
    assert_eq!(
        status_of(root).expect("root works").get().status.0,
        status::OK
    );
    println!("t_netd: hostile requests refused");

    // INV-0221-3, `netd` killed with frames in flight: the client's channel
    // shows PEER_CLOSED, `ethd` stops by itself with the device reset, and
    // a fresh start works.
    let before = clock_monotonic();
    let message = {
        let mut out = alloc::vec![0u8; 64];
        let n = encode(
            &zoraya_proto_net::Ping {
                address: u32::from_be_bytes(GATEWAY),
                count: zoraya_protocol::Below(16),
                timeout_ms: zoraya_protocol::Below(3000),
                name: zoraya_protocol::Data(b""),
            },
            1,
            &mut out,
        )
        .expect("fits");
        out.truncate(n);
        out
    };
    zoraya_user::channel_write(root, &message, &[]).expect("a long ping is on its way");
    sleep_until(clock_monotonic() + 300_000_000);
    net.netd.end();
    let mut bytes = alloc::vec![0u8; 256];
    let mut seen_closed = false;
    for _ in 0..200 {
        match channel_read(root, &mut bytes, &mut []) {
            Err((Status::PeerClosed, _)) => {
                seen_closed = true;
                break;
            }
            // A reply that was already written is fine; the end follows.
            Ok(_) => {}
            Err(_) => sleep_until(clock_monotonic() + 50_000_000),
        }
    }
    assert!(seen_closed, "a client of a dead netd sees PEER_CLOSED");
    let record = wait_exit_within(net.ethd.process, EXIT_NS).expect("ethd stops after netd");
    assert_eq!(
        (record.reason, record.code),
        (ExitReason::Exited, 0),
        "ethd reset the device and unpinned: {record:?}"
    );
    println!(
        "t_netd: netd killed: client closed, ethd stopped cleanly after {} ms",
        (clock_monotonic() - before) / 1_000_000
    );
    net.ethd.end();
    let _ = handle_close(net.control);
    let _ = handle_close(root);

    // The restart `init` makes: a new driver, a new service, a new lease.
    let restart = clock_monotonic();
    let (net, root) = bring_up(device);
    let (seen, ms) = lease(root);
    check_lease(&seen, ms);
    println!(
        "t_netd: restarted after netd: lease in {} ms",
        (clock_monotonic() - restart) / 1_000_000
    );
    assert_eq!(ping(root, GATEWAY, 2).sent, 2);

    // `ethd` killed with frames in flight (the receive buffers are always
    // with the device, and a ping is on the wire): the pins stay, the kernel
    // keeps the frames quarantined (ADR-0054), `netd` sees the driver go and
    // ends, and the next `ethd` resets the device and releases them.
    zoraya_user::channel_write(root, &message, &[]).expect("a long ping");
    sleep_until(clock_monotonic() + 300_000_000);
    net.ethd.end();
    let record = wait_exit_within(net.netd.process, EXIT_NS).expect("netd ends without ethd");
    assert_eq!(
        (record.reason, record.code),
        (ExitReason::Exited, 1),
        "netd: {record:?}"
    );
    net.netd.end();
    let _ = handle_close(net.control);
    let _ = handle_close(root);
    let restart = clock_monotonic();
    let (net, root) = bring_up(device);
    let (seen, ms) = lease(root);
    check_lease(&seen, ms);
    println!(
        "t_netd: restarted after ethd: quarantine released, lease in {} ms",
        (clock_monotonic() - restart) / 1_000_000
    );
    let r = ping(root, GATEWAY, 4);
    assert!(r.received >= 1, "frames flow after the quarantine");
    println!(
        "t_netd: ping after the restart: received {} of {}",
        r.received, r.sent
    );

    account_flood(root);

    // A clean end: the connection closes, the service stays.
    net.netd.end();
    net.ethd.end();
    println!("t_netd: all passed");
    0
}

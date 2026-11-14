//! The requests of `netd` end to end through the service: messages in,
//! messages out, over the scripted peers of the parent module.

use super::*;
use crate::connection::{ConnectTarget, Request, Verdict};
use crate::service::{Action, MAX_CONNECTIONS, Service};
use zoraya_abi::net::{Ports, Protocols, ReachRule, Scope, Target, encode as encode_scope, rights};
use zoraya_proto_net::{
    GetStatus, Narrow, Narrowed, Ping, PingReply, StatusReply, render, source, status,
};
use zoraya_protocol::{Below, Data, ErrorReply, decode, encode};

fn bytes<'a, M: zoraya_protocol::Message<'a>>(m: &M, t: u32) -> Vec<u8> {
    let mut out = vec![0u8; 4096];
    let n = encode(m, t, &mut out).unwrap();
    out.truncate(n);
    out
}

fn status_of(service: &mut Service, id: usize, net: &mut Net) -> Vec<u8> {
    let message = bytes(&GetStatus {}, 7);
    match service.handle(id, &message, 0, &mut net.client, net.now) {
        Action::Reply(r) => r,
        other => panic!("{other:?}"),
    }
}

fn ping_message(address: [u8; 4], count: u32) -> Vec<u8> {
    bytes(
        &Ping {
            address: u32::from_be_bytes(address),
            count: Below(count),
            timeout_ms: Below(2000),
            name: Data(b""),
        },
        9,
    )
}

/// Runs the client until its ping run ends, and hands the end to the service.
fn finish_ping(service: &mut Service, net: &mut Net) -> Option<(usize, Vec<u8>)> {
    let mut result = None;
    net.run_until(10_000_000, |n| !n.client.ping_running());
    for e in net.client.take_events() {
        if let Event::PingDone(r) = e {
            result = service.ping_done(r);
        }
    }
    result
}

#[test]
fn status_through_the_service_shows_the_lease_and_a_renewal_changes_it() {
    // INV-0229-3 through the protocol: `net status` is this reply, rendered.
    let mut net = Net::new(Config::default());
    let mut service = Service::new();
    let root = service.add(Connection::root()).unwrap();
    assert!(net.run_until(5_000_000, |n| n.client.address().is_some()));
    let first = status_of(&mut service, root, &mut net);
    let (h, s) = decode::<StatusReply<'_>>(&first, 0).unwrap();
    assert_eq!(h.transaction, 7, "the reply carries the transaction");
    assert_eq!(s.status.0, status::OK);
    assert_eq!(s.address_source.0, source::DHCP);
    let text = render(&s);
    assert!(
        text.contains("10.0.2.15/24 (dhcp, lease 1h 59m left)"),
        "{text}"
    );
    assert!(text.contains("gateway   10.0.2.2"), "{text}");
    assert!(text.contains("dns       10.0.2.3 (dhcp)"), "{text}");
    assert!(text.contains("52:54:00:12:34:56"), "{text}");
    // Renewal with another DNS server: the same request now says so.
    net.server.dns = [10, 0, 2, 4];
    let requests = net.server.requests;
    assert!(net.run_until(4_000 * 1_000_000, |n| n.server.requests > requests));
    assert!(net.run_until(5_000_000, |n| n.client.status(n.now).dns
        == vec![[10, 0, 2, 4]]));
    let second = status_of(&mut service, root, &mut net);
    let (_, s2) = decode::<StatusReply<'_>>(&second, 0).unwrap();
    let text2 = render(&s2);
    assert!(text2.contains("dns       10.0.2.4 (dhcp)"), "{text2}");
    assert_ne!(text, text2);
}

#[test]
fn a_connection_without_the_status_right_learns_nothing() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let reach_only = Connection::new(rights::REACH, Scope::EMPTY.as_bytes()).unwrap();
    let id = service.add(reach_only).unwrap();
    let reply = status_of(&mut service, id, &mut net);
    let (_, s) = decode::<StatusReply<'_>>(&reply, 0).unwrap();
    assert_eq!(s.status.0, status::NOT_GRANTED);
    assert_eq!((s.address, s.gateway, s.frames_in), (0, 0, 0));
    assert!(s.mac.0.is_empty() && s.dns.is_empty());
}

#[test]
fn ping_through_the_service_answers_when_the_run_ends() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let root = service.add(Connection::root()).unwrap();
    let message = ping_message(SERVER_IP, 3);
    assert_eq!(
        service.handle(root, &message, 0, &mut net.client, net.now),
        Action::Deferred
    );
    // A second run, from anyone, is busy until the first ends.
    let other = service.add(Connection::root()).unwrap();
    let Action::Reply(busy) = service.handle(other, &message, 0, &mut net.client, net.now) else {
        panic!("busy");
    };
    assert_eq!(
        decode::<PingReply>(&busy, 0).unwrap().1.status.0,
        status::BUSY
    );
    let (id, reply) = finish_ping(&mut service, &mut net).expect("the run ended");
    assert_eq!(id, root);
    let (h, r) = decode::<PingReply>(&reply, 0).unwrap();
    assert_eq!(
        (h.transaction, r.status.0, r.sent, r.received),
        (9, status::OK, 3, 3)
    );
}

#[test]
fn ping_needs_the_right_and_a_rule_for_the_address() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let rule_of = |target, protocols| {
        let mut out = vec![0u8; 128];
        let r = ReachRule {
            target,
            ports: Ports::ANY,
            protocols,
        };
        let n = encode_scope(&[r], &[], &mut out).unwrap();
        out.truncate(n);
        out
    };
    let rule = |target| rule_of(target, Protocols::ALL);
    let by_name = Connection::new(rights::REACH, &rule(Target::Name("gateway"))).unwrap();
    let elsewhere = Connection::new(rights::REACH, &rule(Target::Address([8, 8, 8, 8]))).unwrap();
    let no_right = Connection::new(rights::STATUS, &rule(Target::Any)).unwrap();
    // ADR-0330: a grant of TCP and UDP to exactly this address is no grant of
    // echo requests to it.
    let no_echo = Connection::new(
        rights::REACH,
        &rule_of(Target::Address(SERVER_IP), Protocols::BOTH),
    )
    .unwrap();
    let exact = Connection::new(
        rights::REACH,
        &rule_of(Target::Address(SERVER_IP), Protocols::ECHO),
    )
    .unwrap();
    for refused in [by_name, elsewhere, no_right, no_echo] {
        let id = service.add(refused).unwrap();
        let Action::Reply(r) =
            service.handle(id, &ping_message(SERVER_IP, 1), 0, &mut net.client, net.now)
        else {
            panic!("refused");
        };
        assert_eq!(
            decode::<PingReply>(&r, 0).unwrap().1.status.0,
            status::NOT_GRANTED
        );
        assert!(!net.client.ping_running(), "no frame left for it");
    }
    let id = service.add(exact).unwrap();
    assert_eq!(
        service.handle(id, &ping_message(SERVER_IP, 1), 0, &mut net.client, net.now),
        Action::Deferred
    );
    // A bad target and a missing address are statuses, not closes.
    let mut idle = Net::new(static_config());
    let mut service = Service::new();
    let root = service.add(Connection::root()).unwrap();
    let Action::Reply(r) = service.handle(root, &ping_message([0; 4], 1), 0, &mut idle.client, 0)
    else {
        panic!()
    };
    assert_eq!(
        decode::<PingReply>(&r, 0).unwrap().1.status.0,
        status::INVALID
    );
    let mut none = Net::new(Config::default());
    let Action::Reply(r) =
        service.handle(root, &ping_message(SERVER_IP, 1), 0, &mut none.client, 0)
    else {
        panic!()
    };
    assert_eq!(
        decode::<PingReply>(&r, 0).unwrap().1.status.0,
        status::UNAVAILABLE
    );
}

#[test]
fn a_connection_that_goes_while_its_run_goes_loses_only_the_reply() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let a = service.add(Connection::root()).unwrap();
    service.handle(a, &ping_message(SERVER_IP, 2), 0, &mut net.client, net.now);
    service.remove(a);
    assert!(finish_ping(&mut service, &mut net).is_none());
    // The run is over and a new one may start.
    let b = service.add(Connection::root()).unwrap();
    assert_eq!(
        service.handle(b, &ping_message(SERVER_IP, 1), 0, &mut net.client, net.now),
        Action::Deferred
    );
}

#[test]
fn narrow_makes_a_connection_inside_its_parent_and_nothing_wider() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let root = service.add(Connection::root()).unwrap();
    let narrow = |rights, scope: &[u8]| {
        bytes(
            &Narrow {
                rights,
                scope: Data(scope),
            },
            3,
        )
    };
    let Action::Narrowed { id, reply } = service.handle(
        root,
        &narrow(rights::STATUS, Scope::EMPTY.as_bytes()),
        1,
        &mut net.client,
        0,
    ) else {
        panic!("narrowed");
    };
    assert_eq!(
        decode::<Narrowed>(&reply, 0).unwrap().1.status.0,
        status::OK
    );
    assert_eq!(service.connection(id).unwrap().rights(), rights::STATUS);
    // The child cannot ping, cannot narrow, and cannot widen.
    let denied = |service: &mut Service, net: &mut Net, message: Vec<u8>, handles| match service
        .handle(id, &message, handles, &mut net.client, 0)
    {
        Action::Reply(r) => r,
        other => panic!("{other:?}"),
    };
    let r = denied(&mut service, &mut net, ping_message(SERVER_IP, 1), 0);
    assert_eq!(
        decode::<PingReply>(&r, 0).unwrap().1.status.0,
        status::NOT_GRANTED
    );
    let r = denied(
        &mut service,
        &mut net,
        narrow(0, Scope::EMPTY.as_bytes()),
        1,
    );
    assert_eq!(
        decode::<Narrowed>(&r, 0).unwrap().1.status.0,
        status::NOT_GRANTED
    );
    // From the root: an undecodable scope is invalid, not a close.
    let Action::Reply(r) = service.handle(root, &narrow(1, &[9, 9, 9, 9]), 1, &mut net.client, 0)
    else {
        panic!()
    };
    assert_eq!(
        decode::<Narrowed>(&r, 0).unwrap().1.status.0,
        status::INVALID
    );
    let count = service.count();
    // The bound on connections.
    let mut made = 0;
    while let Action::Narrowed { .. } = service.handle(
        root,
        &narrow(rights::STATUS, Scope::EMPTY.as_bytes()),
        1,
        &mut net.client,
        0,
    ) {
        made += 1;
    }
    assert_eq!(service.count(), MAX_CONNECTIONS);
    assert_eq!(made, MAX_CONNECTIONS - count);
}

#[test]
fn malformed_requests_close_the_connection_and_unknown_ones_do_not() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let id = service.add(Connection::root()).unwrap();
    let mut call =
        |message: &[u8], handles| service.handle(id, message, handles, &mut net.client, 0);
    // Not a message, a truncated one, a request with a handle it does not
    // take, and one without the handle it needs.
    assert_eq!(call(&[], 0), Action::Close);
    assert_eq!(call(&[1, 2, 3], 0), Action::Close);
    let status = bytes(&GetStatus {}, 1);
    assert_eq!(call(&status[..status.len() - 1], 0), Action::Close);
    assert_eq!(call(&status, 1), Action::Close);
    let narrow = bytes(
        &Narrow {
            rights: 0,
            scope: Data(Scope::EMPTY.as_bytes()),
        },
        1,
    );
    assert_eq!(call(&narrow, 0), Action::Close);
    assert_eq!(call(&narrow, 2), Action::Close);
    // An ordinal nobody declared: ErrorReply, connection stays.
    let mut unknown = status.clone();
    unknown[4..8].copy_from_slice(&200u32.to_le_bytes());
    let Action::Reply(r) = call(&unknown, 0) else {
        panic!("error reply");
    };
    assert_eq!(
        decode::<ErrorReply>(&r, 0).unwrap().1.status,
        status::INVALID
    );
    // An ErrorReply sent as a request is not answered: the connection ends
    // (two servers must not answer each other's errors forever).
    let error = bytes(&ErrorReply { status: 3 }, 1);
    assert_eq!(call(&error, 0), Action::Close);
    // A connection that is gone cannot ask.
    assert_eq!(
        service.handle(63, &status, 0, &mut net.client, 0),
        Action::Close
    );
}

#[test]
fn random_requests_never_panic() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let id = service.add(Connection::root()).unwrap();
    let mut rng = Rng(0xfeed_f00d_0042);
    let seeds = [
        bytes(&GetStatus {}, 1),
        ping_message(SERVER_IP, 1),
        bytes(
            &Narrow {
                rights: rights::STATUS,
                scope: Data(Scope::EMPTY.as_bytes()),
            },
            1,
        ),
    ];
    for _ in 0..20_000 {
        let mut m = if rng.next().is_multiple_of(4) {
            (0..(rng.next() % 120)).map(|_| rng.next() as u8).collect()
        } else {
            seeds[(rng.next() % 3) as usize].clone()
        };
        if !m.is_empty() {
            for _ in 0..(rng.next() % 4) {
                let at = (rng.next() as usize) % m.len();
                m[at] = rng.next() as u8;
            }
        }
        let handles = (rng.next() % 3) as usize;
        match service.handle(id, &m, handles, &mut net.client, net.now) {
            Action::Narrowed { id: new, .. } => service.remove(new),
            Action::Deferred => {
                net.run_until(3_000_000, |n| !n.client.ping_running());
                for e in net.client.take_events() {
                    if let Event::PingDone(r) = e {
                        let _ = service.ping_done(r);
                    }
                }
            }
            _ => {}
        }
    }
    assert!(service.connection(id).is_some());
}

fn connect_message(name: &str, address: [u8; 4], port: u32) -> Vec<u8> {
    bytes(
        &zoraya_proto_net::Connect {
            name: Data(name.as_bytes()),
            address: u32::from_be_bytes(address),
            port: Below(port),
        },
        4,
    )
}

#[test]
fn connect_is_checked_against_the_right_and_the_scope_before_anything_else() {
    use zoraya_abi::net::Target as ScopeTarget;
    use zoraya_proto_net::Connecting;
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let scope = |target, ports| {
        let mut out = vec![0u8; 128];
        let r = ReachRule {
            target,
            ports,
            protocols: Protocols::TCP,
        };
        let n = encode_scope(&[r], &[], &mut out).unwrap();
        out.truncate(n);
        out
    };
    let only_80 = || Ports::one(80);
    let by_name = Connection::new(
        rights::REACH,
        &scope(ScopeTarget::Name("test.zoraya"), only_80()),
    )
    .unwrap();
    let by_address = Connection::new(
        rights::REACH,
        &scope(ScopeTarget::Address([10, 0, 2, 2]), Ports::ANY),
    )
    .unwrap();
    let no_right = Connection::new(rights::STATUS, &scope(ScopeTarget::Any, Ports::ANY)).unwrap();
    let name_id = service.add(by_name).unwrap();
    let address_id = service.add(by_address).unwrap();
    let none_id = service.add(no_right).unwrap();
    let mut code = |id, message: &[u8]| match service.handle(id, message, 3, &mut net.client, 0) {
        Action::Reply(r) => decode::<Connecting>(&r, 0).unwrap().1.status.0,
        Action::Connect { reply, .. } => {
            assert_eq!(
                decode::<Connecting>(&reply, 0).unwrap().1.status.0,
                status::OK
            );
            status::OK
        }
        other => panic!("{other:?}"),
    };
    // Inside the scope.
    assert_eq!(
        code(name_id, &connect_message("test.zoraya", [0; 4], 80)),
        status::OK
    );
    assert_eq!(
        code(address_id, &connect_message("", [10, 0, 2, 2], 8080)),
        status::OK
    );
    // Outside it: another port, another name, a name where an address rule is,
    // an address where a name rule is, no right at all.
    assert_eq!(
        code(name_id, &connect_message("test.zoraya", [0; 4], 81)),
        status::NOT_GRANTED
    );
    assert_eq!(
        code(name_id, &connect_message("other.zoraya", [0; 4], 80)),
        status::NOT_GRANTED
    );
    assert_eq!(
        code(address_id, &connect_message("test.zoraya", [0; 4], 80)),
        status::NOT_GRANTED
    );
    assert_eq!(
        code(name_id, &connect_message("", [10, 0, 2, 2], 80)),
        status::NOT_GRANTED
    );
    assert_eq!(
        code(none_id, &connect_message("", [10, 0, 2, 2], 80)),
        status::NOT_GRANTED
    );
    // Not usable: port 0, address 0 with no name, a name that is not in the
    // scope's spelling (upper case, an address, a space), non-UTF-8.
    assert_eq!(
        code(address_id, &connect_message("", [10, 0, 2, 2], 0)),
        status::NOT_GRANTED
    );
    assert_eq!(
        code(address_id, &connect_message("", [0; 4], 80)),
        status::INVALID
    );
    for bad in ["Test.Zoraya", "10.0.2.2", "a b", "a..b"] {
        assert_eq!(
            code(name_id, &connect_message(bad, [0; 4], 80)),
            status::INVALID,
            "{bad}"
        );
    }
    let mut raw = connect_message("test.zoraya", [0; 4], 80);
    let n = raw.len();
    raw[n - 1] = 0xff;
    assert_eq!(code(name_id, &raw), status::INVALID);
    // Handles: exactly three, else the connection is closed.
    for handles in [0, 1, 2, 4] {
        let m = connect_message("test.zoraya", [0; 4], 80);
        assert_eq!(
            service.handle(name_id, &m, handles, &mut net.client, 0),
            Action::Close
        );
    }
}

fn ping_name_message(name: &str, count: u32) -> Vec<u8> {
    bytes(
        &Ping {
            address: 0,
            count: Below(count),
            timeout_ms: Below(2000),
            name: Data(name.as_bytes()),
        },
        11,
    )
}

fn scope_for(target: Target<'_>, protocols: Protocols) -> Vec<u8> {
    let mut out = vec![0u8; 128];
    let r = ReachRule {
        target,
        ports: Ports::ANY,
        protocols,
    };
    let n = encode_scope(&[r], &[], &mut out).unwrap();
    out.truncate(n);
    out
}

fn hosts_config() -> Config {
    Config {
        hosts: vec![
            ("gateway".into(), SERVER_IP),
            ("other".into(), [10, 0, 2, 9]),
        ],
        ..static_config()
    }
}

#[test]
fn ping_by_name_resolves_inside_the_service_and_needs_an_echo_rule_for_the_name() {
    // ADR-0330. The names come from net.hosts here; the resolver's own paths
    // are `conn_tests`.
    let mut net = Net::new(hosts_config());
    let mut service = Service::new();
    let refused = |service: &mut Service, net: &mut Net, scope: Vec<u8>, name: &str| {
        let id = service
            .add(Connection::new(rights::REACH, &scope).unwrap())
            .unwrap();
        let Action::Reply(r) =
            service.handle(id, &ping_name_message(name, 1), 0, &mut net.client, net.now)
        else {
            panic!("refused");
        };
        let code = decode::<PingReply>(&r, 0).unwrap().1.status.0;
        assert!(!net.client.ping_running(), "no frame left for it");
        service.remove(id);
        code
    };
    // A TCP and UDP rule for the name is no echo grant; nor is a rule for
    // another name or for the address the name resolves to (the grant names
    // the host the person wrote).
    for scope in [
        scope_for(Target::Name("gateway"), Protocols::BOTH),
        scope_for(Target::Name("other"), Protocols::ECHO),
        scope_for(Target::Address([8, 8, 8, 8]), Protocols::ECHO),
    ] {
        assert_eq!(
            refused(&mut service, &mut net, scope, "gateway"),
            status::NOT_GRANTED
        );
    }
    // A name that is not a name.
    assert_eq!(
        refused(
            &mut service,
            &mut net,
            scope_for(Target::Any, Protocols::ALL),
            "a..b"
        ),
        status::INVALID
    );
    // The echo rule for the name: the run starts, the reply names the address.
    let id = service
        .add(
            Connection::new(
                rights::REACH,
                &scope_for(Target::Name("gateway"), Protocols::ECHO),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        service.handle(
            id,
            &ping_name_message("gateway", 2),
            0,
            &mut net.client,
            net.now
        ),
        Action::Deferred
    );
    let (to, reply) = finish_ping(&mut service, &mut net).expect("the run ended");
    assert_eq!(to, id);
    let (_, r) = decode::<PingReply>(&reply, 0).unwrap();
    assert_eq!((r.status.0, r.sent, r.received), (status::OK, 2, 2));
    assert_eq!(r.address, u32::from_be_bytes(SERVER_IP));
    assert_eq!(r.rtts.len(), 2);
    // The same connection cannot open a TCP connection to the host.
    assert_eq!(
        service.connection(id).unwrap().decide(Request::Connect {
            target: ConnectTarget::Name("gateway"),
            port: 80
        }),
        Verdict::NotGranted
    );
    assert_eq!(
        service.connection(id).unwrap().decide(Request::Connect {
            target: ConnectTarget::Address(SERVER_IP),
            port: 80
        }),
        Verdict::NotGranted
    );
}

#[test]
fn a_run_whose_connection_went_is_not_given_to_the_next_asker() {
    // A pings (16 requests) and goes; B, with an echo rule for a name, asks
    // for a ping by name while A's run still goes. B is told Busy, and when
    // A's run ends nobody is handed its address and round trips.
    let mut net = Net::new(hosts_config());
    let mut service = Service::new();
    let a = service.add(Connection::root()).unwrap();
    assert_eq!(
        service.handle(a, &ping_message(SERVER_IP, 16), 0, &mut net.client, net.now),
        Action::Deferred
    );
    service.remove(a);
    let b = service
        .add(
            Connection::new(
                rights::REACH,
                &scope_for(Target::Name("other"), Protocols::ECHO),
            )
            .unwrap(),
        )
        .unwrap();
    let Action::Reply(r) = service.handle(
        b,
        &ping_name_message("other", 1),
        0,
        &mut net.client,
        net.now,
    ) else {
        panic!("busy");
    };
    assert_eq!(decode::<PingReply>(&r, 0).unwrap().1.status.0, status::BUSY);
    // The same by address: busy too.
    assert!(
        finish_ping(&mut service, &mut net).is_none(),
        "A's end goes to nobody"
    );
    // The slot is free again.
    assert_eq!(
        service.handle(
            b,
            &ping_name_message("other", 1),
            0,
            &mut net.client,
            net.now
        ),
        Action::Deferred
    );
}

fn web_scope(extra: &[ReachRule<'_>]) -> Vec<u8> {
    let mut rules = vec![ReachRule {
        target: Target::Public,
        ports: Ports::ranges(&[(80, 80), (443, 443)]).unwrap(),
        protocols: Protocols::TCP,
    }];
    rules.extend_from_slice(extra);
    let mut out = vec![0u8; 256];
    let n = encode_scope(&rules, &[], &mut out).unwrap();
    out.truncate(n);
    out
}

/// The status and the addresses of a `resolve` answered at once (a
/// `net.hosts` name).
fn resolve_now(service: &mut Service, net: &mut Net, id: usize, name: &str) -> (u32, Vec<u32>) {
    use zoraya_proto_net::{Resolve, Resolved};
    let m = bytes(
        &Resolve {
            name: Data(name.as_bytes()),
        },
        5,
    );
    let Action::Reply(r) = service.handle(id, &m, 0, &mut net.client, net.now) else {
        panic!("answered at once: {name}");
    };
    let (_, reply) = decode::<Resolved>(&r, 0).unwrap();
    let addresses = reply.addresses.iter().map(|a| a.address).collect();
    (reply.status.0, addresses)
}

/// ADR-0324 items 2 and 5 (INV-0324-1): a web rule resolves any name but
/// answers only with public addresses that are not local, `net.hosts`
/// included; none left is `NOT_GRANTED`. It connects to public addresses on
/// its ports, and to names on its ports, whose answers `Conns` then checks.
#[test]
fn a_web_rule_resolves_to_public_answers_only_and_connects_on_its_ports() {
    use zoraya_proto_net::Connecting;
    const PUBLIC: [u8; 4] = [93, 184, 215, 14];
    let mut net = Net::new(Config {
        hosts: vec![
            ("pub".into(), PUBLIC),
            ("lan".into(), [10, 0, 2, 9]),
            ("gateway".into(), SERVER_IP),
            ("loop".into(), [127, 0, 0, 1]),
            ("home".into(), [192, 168, 1, 1]),
        ],
        ..static_config()
    });
    let mut service = Service::new();
    let web = service
        .add(Connection::new(rights::REACH, &web_scope(&[])).unwrap())
        .unwrap();
    assert_eq!(
        resolve_now(&mut service, &mut net, web, "pub"),
        (status::OK, vec![u32::from_be_bytes(PUBLIC)])
    );
    for name in ["lan", "gateway", "loop", "home"] {
        assert_eq!(
            resolve_now(&mut service, &mut net, web, name),
            (status::NOT_GRANTED, vec![]),
            "{name}"
        );
    }
    // A rule that names the address lets it through; a rule that names the
    // name keeps ADR-0223's rule (net.hosts is the person's word).
    let lan = [10, 0, 2, 9];
    let named = ReachRule {
        target: Target::Address(lan),
        ports: Ports::one(8080),
        protocols: Protocols::TCP,
    };
    let with_address = service
        .add(Connection::new(rights::REACH, &web_scope(&[named])).unwrap())
        .unwrap();
    assert_eq!(
        resolve_now(&mut service, &mut net, with_address, "lan"),
        (status::OK, vec![u32::from_be_bytes(lan)])
    );
    let by_name = ReachRule {
        target: Target::Name("gateway"),
        ports: Ports::one(8080),
        protocols: Protocols::TCP,
    };
    let with_name = service
        .add(Connection::new(rights::REACH, &web_scope(&[by_name])).unwrap())
        .unwrap();
    assert_eq!(
        resolve_now(&mut service, &mut net, with_name, "gateway"),
        (status::OK, vec![u32::from_be_bytes(SERVER_IP)])
    );
    // Connections: public addresses and any name on 80 and 443 only; never a
    // local or private address.
    let mut code = |id, message: &[u8]| match service.handle(id, message, 3, &mut net.client, 0) {
        Action::Reply(r) => decode::<Connecting>(&r, 0).unwrap().1.status.0,
        Action::Connect { .. } => status::OK,
        other => panic!("{other:?}"),
    };
    for port in [80, 443] {
        assert_eq!(code(web, &connect_message("", PUBLIC, port)), status::OK);
        assert_eq!(
            code(web, &connect_message("", [203, 0, 113, 10], port)),
            status::OK
        );
        assert_eq!(
            code(web, &connect_message("example.org", [0; 4], port)),
            status::OK
        );
        for local in [
            SERVER_IP,
            [127, 0, 0, 1],
            [10, 0, 2, 9],
            [192, 168, 1, 1],
            [169, 254, 0, 1],
        ] {
            assert_eq!(
                code(web, &connect_message("", local, port)),
                status::NOT_GRANTED,
                "{local:?}"
            );
        }
    }
    for port in [22, 81, 444, 8080] {
        assert_eq!(
            code(web, &connect_message("", PUBLIC, port)),
            status::NOT_GRANTED
        );
        assert_eq!(
            code(web, &connect_message("example.org", [0; 4], port)),
            status::NOT_GRANTED
        );
    }
    assert_eq!(
        code(with_address, &connect_message("", lan, 8080)),
        status::OK
    );
    // No echo through it.
    let Action::Reply(r) = service.handle(web, &ping_message(PUBLIC, 1), 0, &mut net.client, 0)
    else {
        panic!("refused at once");
    };
    assert_eq!(
        decode::<PingReply>(&r, 0).unwrap().1.status.0,
        status::NOT_GRANTED
    );
}

/// INV-0324-2 through the service: a web connection narrows to a web rule
/// and to nothing that would name an address or a host.
#[test]
fn a_web_connection_narrows_only_to_a_web_rule() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let web = service
        .add(Connection::new(rights::REACH | rights::NARROW, &web_scope(&[])).unwrap())
        .unwrap();
    let narrow = |target, port| {
        let r = ReachRule {
            target,
            ports: Ports::one(port),
            protocols: Protocols::TCP,
        };
        let mut out = vec![0u8; 128];
        let n = encode_scope(&[r], &[], &mut out).unwrap();
        out.truncate(n);
        out
    };
    let mut ask = |scope: &[u8]| {
        let m = bytes(
            &Narrow {
                rights: rights::REACH,
                scope: Data(scope),
            },
            6,
        );
        match service.handle(web, &m, 1, &mut net.client, 0) {
            Action::Reply(r) => decode::<Narrowed>(&r, 0).unwrap().1.status.0,
            Action::Narrowed { .. } => status::OK,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(ask(&narrow(Target::Public, 443)), status::OK);
    for outside in [
        narrow(Target::Address([93, 184, 215, 14]), 443),
        narrow(Target::Address(SERVER_IP), 80),
        narrow(Target::Name("example.org"), 443),
        narrow(Target::Any, 443),
        narrow(Target::Public, 8080),
    ] {
        assert_eq!(ask(&outside), status::NOT_GRANTED);
    }
}

/// ADR-0324 and ADR-0325: a public address that is this side of the network
/// (here a DNS server) is refused at `connect` as not granted under a web
/// rule, and allowed with a rule that names it.
#[test]
fn a_web_rule_refuses_a_public_local_address_as_not_granted() {
    use zoraya_proto_net::Connecting;
    let mut net = Net::new(Config {
        dns: vec![[1, 1, 1, 1]],
        ..static_config()
    });
    let mut service = Service::new();
    let web = service
        .add(Connection::new(rights::REACH, &web_scope(&[])).unwrap())
        .unwrap();
    let named = ReachRule {
        target: Target::Address([1, 1, 1, 1]),
        ports: Ports::one(80),
        protocols: Protocols::TCP,
    };
    let with_address = service
        .add(Connection::new(rights::REACH, &web_scope(&[named])).unwrap())
        .unwrap();
    let mut code = |id, message: &[u8]| match service.handle(id, message, 3, &mut net.client, 0) {
        Action::Reply(r) => decode::<Connecting>(&r, 0).unwrap().1.status.0,
        Action::Connect { .. } => status::OK,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        code(web, &connect_message("", [1, 1, 1, 1], 80)),
        status::NOT_GRANTED
    );
    assert_eq!(
        code(web, &connect_message("", [1, 0, 0, 1], 80)),
        status::OK
    );
    assert_eq!(
        code(with_address, &connect_message("", [1, 1, 1, 1], 80)),
        status::OK
    );
}

/// INV-0324-1: `netd` holds a web rule to TCP itself. A scope with a web
/// rule over UDP or echo (made by hand: `encode` makes none) is refused as a
/// connection's scope and as `narrow`'s, even from `any`, so a ping by
/// address or by name can never pass through a web rule.
#[test]
fn netd_refuses_a_web_rule_over_udp_or_echo() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let any = service
        .add(
            Connection::new(
                rights::REACH | rights::NARROW,
                &scope_for(Target::Any, Protocols::ALL),
            )
            .unwrap(),
        )
        .unwrap();
    for raw in [2u8, 4, 5, 7] {
        // Version, one reach rule, kind 5 (Public), any port, the protocols.
        let scope = [1u8, 1, 0, 0, 5, 0xff, raw];
        assert!(Connection::new(rights::REACH, &scope).is_none(), "{raw}");
        let m = bytes(
            &Narrow {
                rights: rights::REACH,
                scope: Data(&scope),
            },
            6,
        );
        match service.handle(any, &m, 1, &mut net.client, 0) {
            Action::Reply(r) => assert_eq!(
                decode::<Narrowed>(&r, 0).unwrap().1.status.0,
                status::INVALID,
                "{raw}"
            ),
            other => panic!("{raw}: {other:?}"),
        }
    }
    // The same bytes over TCP are a web rule `narrow` gives.
    let scope = [1u8, 1, 0, 0, 5, 0xff, 1];
    assert!(Connection::new(rights::REACH, &scope).is_some());
}

/// ADR-0328 (INV-0328-1) through the service: a public-only name rule
/// resolves its own name to public answers only (`net.hosts` included; none
/// left is `NOT_GRANTED`, which NetSurf names with ADR-0325's page), connects
/// by that name on its port only, and to no address.
#[test]
fn a_public_name_rule_resolves_its_name_to_public_answers_only() {
    use zoraya_proto_net::Connecting;
    const PUBLIC: [u8; 4] = [203, 0, 113, 11];
    let mut net = Net::new(Config {
        hosts: vec![
            ("two.web.test".into(), PUBLIC),
            ("lan.web.test".into(), [10, 0, 2, 2]),
            ("home.web.test".into(), [192, 168, 1, 1]),
        ],
        ..static_config()
    });
    let mut service = Service::new();
    let rule = |name| ReachRule {
        target: Target::PublicName(name),
        ports: Ports::one(8080),
        protocols: Protocols::TCP,
    };
    let scope_of = |rules: &[ReachRule<'_>]| {
        let mut out = vec![0u8; 256];
        let n = encode_scope(rules, &[], &mut out).unwrap();
        out.truncate(n);
        out
    };
    let two = service
        .add(Connection::new(rights::REACH, &scope_of(&[rule("two.web.test")])).unwrap())
        .unwrap();
    assert_eq!(
        resolve_now(&mut service, &mut net, two, "two.web.test"),
        (status::OK, vec![u32::from_be_bytes(PUBLIC)])
    );
    let m = bytes(
        &zoraya_proto_net::Resolve {
            name: Data(b"lan.web.test"),
        },
        5,
    );
    let Action::Reply(r) = service.handle(two, &m, 0, &mut net.client, net.now) else {
        panic!("refused at once");
    };
    assert_eq!(
        decode::<zoraya_proto_net::Resolved>(&r, 0)
            .unwrap()
            .1
            .status
            .0,
        status::NOT_GRANTED,
        "another name"
    );
    for name in ["lan.web.test", "home.web.test"] {
        let id = service
            .add(Connection::new(rights::REACH, &scope_of(&[rule(name)])).unwrap())
            .unwrap();
        assert_eq!(
            resolve_now(&mut service, &mut net, id, name),
            (status::NOT_GRANTED, vec![]),
            "{name}"
        );
    }
    let mut code = |id, message: &[u8]| match service.handle(id, message, 3, &mut net.client, 0) {
        Action::Reply(r) => decode::<Connecting>(&r, 0).unwrap().1.status.0,
        Action::Connect { .. } => status::OK,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        code(two, &connect_message("two.web.test", [0; 4], 8080)),
        status::OK
    );
    for (name, port) in [
        ("two.web.test", 80),
        ("two.web.test", 8081),
        ("other", 8080),
    ] {
        assert_eq!(
            code(two, &connect_message(name, [0; 4], port)),
            status::NOT_GRANTED,
            "{name} {port}"
        );
    }
    assert_eq!(
        code(two, &connect_message("", PUBLIC, 8080)),
        status::NOT_GRANTED
    );
}

//! The requests of `netd` end to end through the service: messages in,
//! messages out, over the scripted peers of the parent module.

use super::*;
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
    let rule = |target| {
        let mut out = vec![0u8; 128];
        let r = ReachRule {
            target,
            ports: Ports::ANY,
            protocols: Protocols::BOTH,
        };
        let n = encode_scope(&[r], &[], &mut out).unwrap();
        out.truncate(n);
        out
    };
    let by_name = Connection::new(rights::REACH, &rule(Target::Name("gateway"))).unwrap();
    let elsewhere = Connection::new(rights::REACH, &rule(Target::Address([8, 8, 8, 8]))).unwrap();
    let no_right = Connection::new(rights::STATUS, &rule(Target::Any)).unwrap();
    let exact = Connection::new(rights::REACH, &rule(Target::Address(SERVER_IP))).unwrap();
    for refused in [by_name, elsewhere, no_right] {
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

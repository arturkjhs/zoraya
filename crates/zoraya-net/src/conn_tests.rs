//! Connections end to end over the scripted peer of `tests`: a TCP server at
//! 10.0.2.50 and a DNS server at 10.0.2.3, both on the far end of the frame
//! queues (INV-0222-1 and INV-0222-2, ADR-0223).

use super::*;
use crate::conn::{CONNECT_US, Client, Conns, LINGER_US, MAX_CONNS, Rd, Step, Target, Wr};
use crate::dns::{Owner, ResolveFail, Start};
use crate::tcp::MAX_TCP;
use smoltcp::socket::tcp;
use std::collections::VecDeque;
use zoraya_abi::net::{ListenRule, Ports, Protocols, ReachRule, encode as encode_scope};
use zoraya_proto_net::{Connected, Failed, Reset, reason, reset};
use zoraya_protocol::{Header, decode};

const WEB: [u8; 4] = [10, 0, 2, 50];
const DNS: [u8; 4] = [10, 0, 2, 3];

#[derive(Default)]
struct FakeState {
    outbound: VecDeque<u8>,
    out_closed: bool,
    inbound: Vec<u8>,
    in_cap: usize,
    /// Most bytes one call moves (0: no limit), to test partial transfers.
    chunk: usize,
    in_closed: bool,
    gone: bool,
    channel_open: bool,
    messages: Vec<Vec<u8>>,
}

#[derive(Clone)]
struct Fake(Rc<RefCell<FakeState>>);

impl Fake {
    fn new(in_cap: usize) -> Fake {
        Fake(Rc::new(RefCell::new(FakeState {
            in_cap,
            channel_open: true,
            ..FakeState::default()
        })))
    }

    fn write(&self, bytes: &[u8]) {
        self.0.borrow_mut().outbound.extend(bytes.iter().copied());
    }

    fn close_write(&self) {
        self.0.borrow_mut().out_closed = true;
    }

    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut self.0.borrow_mut().inbound)
    }

    fn ordinals(&self) -> Vec<u32> {
        self.0
            .borrow()
            .messages
            .iter()
            .map(|m| Header::decode(m).unwrap().0.ordinal)
            .collect()
    }

    fn message(&self, ordinal: u32) -> Option<Vec<u8>> {
        self.0
            .borrow()
            .messages
            .iter()
            .find(|m| Header::decode(m).unwrap().0.ordinal == ordinal)
            .cloned()
    }

    fn failed(&self) -> Option<u32> {
        let m = self.message(17)?;
        Some(decode::<Failed>(&m, 0).unwrap().1.reason.0)
    }
}

impl Client for Fake {
    fn read_outbound(&mut self, buf: &mut [u8]) -> Rd {
        let mut s = self.0.borrow_mut();
        if s.outbound.is_empty() {
            return if s.out_closed { Rd::Eof } else { Rd::Empty };
        }
        let mut n = buf.len().min(s.outbound.len());
        if s.chunk > 0 {
            n = n.min(s.chunk);
        }
        for b in buf.iter_mut().take(n) {
            *b = s.outbound.pop_front().unwrap();
        }
        Rd::Data(n)
    }

    fn write_inbound(&mut self, data: &[u8]) -> Wr {
        let mut s = self.0.borrow_mut();
        assert!(!s.in_closed, "a write after the inbound stream was closed");
        if s.gone {
            return Wr::Gone;
        }
        let mut n = s.in_cap.saturating_sub(s.inbound.len()).min(data.len());
        if s.chunk > 0 {
            n = n.min(s.chunk);
        }
        if n == 0 {
            return Wr::Full;
        }
        s.inbound.extend_from_slice(&data[..n]);
        Wr::Wrote(n)
    }

    fn close_inbound(&mut self) {
        self.0.borrow_mut().in_closed = true;
    }

    fn notify(&mut self, message: &[u8]) -> bool {
        let mut s = self.0.borrow_mut();
        if !s.channel_open {
            return false;
        }
        s.messages.push(message.to_vec());
        true
    }
}

enum Script {
    Echo,
    Sink,
    /// Send the bytes at once, then close.
    Greet(Vec<u8>),
    /// Send the bytes, keep the connection open, and close when we are.
    Silent,
    /// Reset the connection when the first byte arrives.
    ResetOnData,
    /// Send the bytes, close (a FIN), and reset when a byte arrives after
    /// that: a server that closes and then hears more.
    GreetThenReset(Vec<u8>),
}

type DnsScript = Box<dyn FnMut(&[u8]) -> Vec<Vec<u8>>>;

struct Peer {
    device: LinkDevice<End>,
    iface: Interface,
    sockets: SocketSet<'static>,
    tcp: smoltcp::iface::SocketHandle,
    dns: smoltcp::iface::SocketHandle,
    script: Script,
    /// Everything the server read, over all connections.
    log: Vec<u8>,
    received: Vec<u8>,
    to_send: Vec<u8>,
    sent: usize,
    finish_after: bool,
    greeted: bool,
    /// Does not answer at all.
    mute: bool,
    dns_script: DnsScript,
    dns_queries: u32,
}

impl Peer {
    fn new(end: End, script: Script) -> Peer {
        let mut device = LinkDevice::new(end);
        let mut config = IfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress(SERVER_MAC)));
        config.random_seed = 9;
        let mut iface = Interface::new(config, &mut device, Instant::from_micros(0));
        iface.update_ip_addrs(|a| {
            for ip in [DNS, WEB] {
                a.push(IpCidr::new(IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), 24))
                    .unwrap();
            }
        });
        let mut sockets = SocketSet::new(Vec::new());
        let mut t = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; 64 * 1024]),
            tcp::SocketBuffer::new(vec![0; 64 * 1024]),
        );
        let _ = t.listen(80);
        let tcp = sockets.add(t);
        let mut u = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 8 * 600]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 8 * 1300]),
        );
        u.bind(smoltcp::wire::IpListenEndpoint {
            addr: Some(IpAddress::v4(DNS[0], DNS[1], DNS[2], DNS[3])),
            port: 53,
        })
        .unwrap();
        let dns = sockets.add(u);
        Peer {
            device,
            iface,
            sockets,
            tcp,
            dns,
            script,
            log: Vec::new(),
            received: Vec::new(),
            to_send: Vec::new(),
            sent: 0,
            finish_after: false,
            greeted: false,
            mute: false,
            dns_script: Box::new(|_| Vec::new()),
            dns_queries: 0,
        }
    }

    fn poll(&mut self, now_us: i64) {
        if self.mute {
            // Frames pile up unread.
            return;
        }
        let now = Instant::from_micros(now_us);
        for _ in 0..2 {
            self.iface.poll(now, &mut self.device, &mut self.sockets);
            self.serve_tcp();
            self.serve_dns();
        }
        self.iface.poll(now, &mut self.device, &mut self.sockets);
    }

    fn serve_dns(&mut self) {
        let mut buf = [0u8; 600];
        loop {
            let socket = self.sockets.get_mut::<udp::Socket>(self.dns);
            let Ok((n, meta)) = socket.recv_slice(&mut buf) else {
                break;
            };
            self.dns_queries += 1;
            for reply in (self.dns_script)(&buf[..n]) {
                let socket = self.sockets.get_mut::<udp::Socket>(self.dns);
                let _ = socket.send_slice(&reply, meta.endpoint);
            }
        }
    }

    fn serve_tcp(&mut self) {
        let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
        if !s.is_open() {
            self.received.clear();
            self.to_send.clear();
            self.sent = 0;
            self.finish_after = false;
            self.greeted = false;
            let _ = s.listen(80);
            return;
        }
        if !matches!(
            s.state(),
            tcp::State::Established
                | tcp::State::CloseWait
                | tcp::State::FinWait1
                | tcp::State::FinWait2
        ) {
            return;
        }
        if let Script::Greet(bytes) | Script::GreetThenReset(bytes) = &self.script
            && !self.greeted
        {
            self.greeted = true;
            self.to_send = bytes.clone();
            self.finish_after = true;
        }
        let mut buf = [0u8; 4096];
        while s.can_recv() {
            let n = s.recv_slice(&mut buf).unwrap();
            match self.script {
                Script::ResetOnData | Script::GreetThenReset(_) => {
                    s.abort();
                    return;
                }
                Script::Echo => self.to_send.extend_from_slice(&buf[..n]),
                _ => {}
            }
            self.received.extend_from_slice(&buf[..n]);
            self.log.extend_from_slice(&buf[..n]);
        }
        if matches!(self.script, Script::Silent | Script::Echo | Script::Sink)
            && !s.may_recv()
            && self.sent == self.to_send.len()
        {
            // The client is done sending: we are done too.
            if matches!(self.script, Script::Silent) && !self.to_send.is_empty() {
                // fall through to send
            }
            s.close();
        }
        while self.sent < self.to_send.len() && s.can_send() {
            let n = s.send_slice(&self.to_send[self.sent..]).unwrap();
            if n == 0 {
                break;
            }
            self.sent += n;
        }
        if self.finish_after && self.sent == self.to_send.len() {
            s.close();
        }
    }
}

struct World {
    client: Stack<End>,
    conns: Conns<Fake>,
    peer: Peer,
    now: i64,
    finished: Vec<(usize, Fake)>,
    scope: Vec<u8>,
    steps: Vec<Step>,
    queues: Rc<RefCell<Queues>>,
}

fn scope_with(reach: &[ReachRule<'_>]) -> Vec<u8> {
    let listen: [ListenRule; 0] = [];
    let mut out = vec![0u8; 512];
    let n = encode_scope(reach, &listen, &mut out).unwrap();
    out.truncate(n);
    out
}

impl World {
    fn new(script: Script, hosts: &[(&str, [u8; 4])]) -> World {
        World::with_dns(script, hosts, vec![DNS])
    }

    fn with_dns(script: Script, hosts: &[(&str, [u8; 4])], dns: Vec<[u8; 4]>) -> World {
        let (a, b, queues) = End::pair();
        let mut config = static_config();
        config.dns = dns;
        config.hosts = hosts.iter().map(|(n, a)| (n.to_string(), *a)).collect();
        World {
            client: Stack::new(a, CLIENT_MAC, config, 0, 3).unwrap(),
            conns: Conns::new(),
            peer: Peer::new(b, script),
            now: 0,
            finished: Vec::new(),
            scope: Connection::root().scope_bytes().to_vec(),
            steps: Vec::new(),
            queues,
        }
    }

    fn connect(&mut self, fake: &Fake, target: Target, port: u16) -> usize {
        let scope = self.scope.clone();
        self.conns
            .open(
                &mut self.client,
                fake.clone(),
                Owner::SESSION,
                8,
                target,
                port,
                &scope,
                self.now,
            )
            .map_err(|e| e.1)
            .unwrap()
    }

    /// `connect` for an account: its owner and its bound of pending names.
    fn connect_as(
        &mut self,
        fake: &Fake,
        owner: Owner,
        resolutions: usize,
        target: Target,
        port: u16,
    ) -> Result<usize, u32> {
        let scope = self.scope.clone();
        self.conns
            .open(
                &mut self.client,
                fake.clone(),
                owner,
                resolutions,
                target,
                port,
                &scope,
                self.now,
            )
            .map_err(|e| e.1)
    }

    fn turn(&mut self) {
        self.peer.poll(self.now);
        let steps = &mut self.steps;
        let (delay, fin) = self
            .conns
            .turn(&mut self.client, self.now, |_, _, s| steps.push(*s));
        self.finished.extend(fin);
        self.peer.poll(self.now);
        let step = delay.unwrap_or(500_000).clamp(200, 500_000);
        self.now += step;
    }

    fn run_until(&mut self, limit_us: i64, done: impl Fn(&World) -> bool) -> bool {
        let end = self.now + limit_us;
        while self.now < end {
            self.turn();
            if done(self) {
                return true;
            }
        }
        false
    }
}

/// A few more turns, for the sockets released on the last one to be reaped.
fn settle(w: &mut World) {
    w.run_until(500_000, |_| false);
}

/// A scope that reaches names under `zoraya` only: an address answer that is
/// local has no address rule to cover it.
fn names_only() -> Vec<u8> {
    scope_with(&[ReachRule {
        target: zoraya_abi::net::Target::Wildcard("zoraya"),
        ports: Ports::ANY,
        protocols: Protocols::BOTH,
    }])
}

fn open_world(script: Script) -> (World, Fake, usize) {
    let mut w = World::new(script, &[]);
    let fake = Fake::new(65536);
    let id = w.connect(&fake, Target::Address(WEB), 80);
    assert!(
        w.run_until(2_000_000, |_| fake.message(16).is_some()),
        "connected"
    );
    (w, fake, id)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn pattern(n: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng(seed | 1);
    (0..n).map(|_| r.next() as u8).collect()
}

#[test]
fn a_connection_opens_and_tells_the_client_where_it_reached() {
    let (w, fake, _) = open_world(Script::Sink);
    let m = fake.message(16).unwrap();
    let c = decode::<Connected>(&m, 0).unwrap().1;
    assert_eq!((c.address, c.port.0), (u32::from_be_bytes(WEB), 80));
    assert_eq!(fake.ordinals(), vec![16]);
    assert_eq!(w.client.tcp_count(), 1);
}

#[test]
fn bytes_arrive_in_order_and_complete_both_ways_with_small_rings_and_odd_chunks() {
    // INV-0222-1: random data through the stack and the peer's echo, the
    // client's rings far smaller than the data, reads and writes of random
    // sizes.
    for (seed, cap, chunk) in [
        (1u64, 100usize, 7usize),
        (2, 4096, 1000),
        (3, 65536, 0),
        (4, 513, 61),
    ] {
        let (mut w, fake, _) = open_world(Script::Echo);
        fake.0.borrow_mut().chunk = chunk;
        fake.0.borrow_mut().in_cap = cap;
        let data = pattern(150_000, seed);
        let mut rng = Rng(seed * 77 + 5);
        let mut written = 0;
        let mut got = Vec::new();
        let mut turns = 0;
        while got.len() < data.len() && turns < 400_000 {
            turns += 1;
            if written < data.len() {
                let n = (rng.next() as usize % 5000 + 1).min(data.len() - written);
                if fake.0.borrow().outbound.len() < 8000 {
                    fake.write(&data[written..written + n]);
                    written += n;
                }
            }
            w.turn();
            got.extend(fake.take());
        }
        assert_eq!(got.len(), data.len(), "seed {seed}");
        assert!(got == data, "bytes differ, seed {seed}");
        assert_eq!(w.peer.log, data);
    }
}

#[test]
fn the_peers_fin_is_end_of_file_after_the_data_and_the_client_can_still_write() {
    let (mut w, fake, _) = open_world(Script::Greet(b"hello, client".to_vec()));
    assert!(w.run_until(5_000_000, |_| fake.0.borrow().in_closed));
    assert_eq!(fake.take(), b"hello, client");
    assert!(fake.message(18).is_none(), "an orderly close is no reset");
    // The inbound side is over; ours is open.
    fake.write(b"late words");
    assert!(w.run_until(5_000_000, |w| w.peer.log == b"late words"));
    fake.close_write();
    assert!(
        w.run_until(60_000_000, |w| w.conns.count() == 0),
        "released"
    );
    settle(&mut w);
    assert_eq!(w.finished.len(), 1);
    assert_eq!(w.client.tcp_count(), 0);
}

#[test]
fn closing_the_write_end_sends_fin_after_the_ring_drains() {
    let (mut w, fake, _) = open_world(Script::Sink);
    let data = pattern(40_000, 9);
    fake.write(&data);
    fake.close_write();
    assert!(w.run_until(10_000_000, |w| {
        w.client.address().is_some() && w.peer.log.len() == data.len()
    }));
    assert_eq!(w.peer.log, data);
    // The sink closes when it sees the FIN; the connection ends on both sides.
    assert!(w.run_until(60_000_000, |w| w.conns.count() == 0));
    assert!(fake.0.borrow().in_closed);
    assert_eq!(fake.take().len(), 0);
}

#[test]
fn a_reset_from_the_peer_is_a_message_and_both_ends_close() {
    let (mut w, fake, _) = open_world(Script::ResetOnData);
    fake.write(b"x");
    assert!(w.run_until(5_000_000, |_| fake.message(18).is_some()));
    let m = fake.message(18).unwrap();
    assert_eq!(decode::<Reset>(&m, 0).unwrap().1.why.0, reset::PEER);
    assert!(fake.0.borrow().in_closed);
    assert!(w.run_until(5_000_000, |w| w.conns.count() == 0));
    settle(&mut w);
    assert_eq!(w.client.tcp_count(), 0);
}

#[test]
fn a_closed_port_is_refused() {
    let mut w = World::new(Script::Sink, &[]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Address(WEB), 81);
    assert!(w.run_until(5_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::REFUSED));
    assert!(fake.0.borrow().in_closed);
    assert!(w.run_until(1_000_000, |w| w.conns.count() == 0));
    settle(&mut w);
    assert_eq!(w.client.tcp_count(), 0);
}

#[test]
fn a_silent_peer_times_out() {
    let mut w = World::new(Script::Sink, &[]);
    w.peer.mute = true;
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Address(WEB), 80);
    assert!(!w.run_until(CONNECT_US - 1_000_000, |_| fake.failed().is_some()));
    assert!(w.run_until(3_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::TIMED_OUT));
}

#[test]
fn a_client_that_stops_reading_ends_the_connection_with_a_reset_to_the_peer() {
    let (mut w, fake, _) = open_world(Script::Greet(pattern(10_000, 3)));
    fake.0.borrow_mut().gone = true;
    assert!(w.run_until(5_000_000, |w| w.conns.count() == 0));
    settle(&mut w);
    assert_eq!(w.client.tcp_count(), 0);
}

#[test]
fn a_client_whose_channel_is_gone_is_dropped_before_the_connection_opens() {
    let mut w = World::new(Script::Sink, &[]);
    let fake = Fake::new(1024);
    fake.0.borrow_mut().channel_open = false;
    w.connect(&fake, Target::Address(WEB), 80);
    assert!(w.run_until(5_000_000, |w| w.conns.count() == 0));
    settle(&mut w);
    assert_eq!(w.client.tcp_count(), 0);
}

#[test]
fn aborting_a_connection_frees_its_socket_and_sends_a_reset() {
    let (mut w, fake, id) = open_world(Script::Sink);
    let back = w.conns.abort(&mut w.client, id);
    assert!(back.is_some());
    w.run_until(2_000_000, |_| false);
    assert_eq!(w.client.tcp_count(), 0);
    assert_eq!(w.conns.count(), 0);
    drop(fake);
}

#[test]
fn too_many_connections_are_refused_with_a_reason_not_a_crash() {
    let mut w = World::new(Script::Sink, &[]);
    let mut fakes = Vec::new();
    for _ in 0..MAX_TCP {
        let f = Fake::new(1024);
        w.connect(&f, Target::Address(WEB), 80);
        fakes.push(f);
    }
    let extra = Fake::new(1024);
    w.connect(&extra, Target::Address(WEB), 80);
    w.run_until(500_000, |_| false);
    assert_eq!(extra.failed(), Some(reason::OVER_LIMIT));
    // And the slots are bounded too.
    let mut refused = 0;
    for _ in 0..MAX_CONNS + 2 {
        let f = Fake::new(1024);
        let scope = w.scope.clone();
        if w.conns
            .open(
                &mut w.client,
                f,
                Owner::SESSION,
                8,
                Target::Address(WEB),
                80,
                &scope,
                w.now,
            )
            .is_err()
        {
            refused += 1;
        }
    }
    assert!(refused >= 1);
    assert!(w.client.tcp_count() <= MAX_TCP);
}

fn a_record_reply(query: &[u8], address: [u8; 4], ttl: u32) -> Vec<u8> {
    let mut m = query.to_vec();
    m[2] = 0x81;
    m[3] = 0x80;
    m[7] = 1;
    m.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1]);
    m.extend_from_slice(&ttl.to_be_bytes());
    m.extend_from_slice(&[0, 4]);
    m.extend_from_slice(&address);
    m
}

#[test]
fn a_name_is_resolved_connected_and_cached() {
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, WEB, 300)]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("test.zoraya".into()), 80);
    assert!(w.run_until(5_000_000, |_| fake.message(16).is_some()));
    assert_eq!(w.peer.dns_queries, 1);
    assert!(w.client.dns_cached("Test.Zoraya"));
    // The server takes one connection at a time: end the first.
    fake.close_write();
    assert!(w.run_until(60_000_000, |w| w.conns.count() == 0));
    // A second connection to the name sends no query.
    let again = Fake::new(1024);
    w.connect(&again, Target::Name("test.zoraya".into()), 80);
    assert!(w.run_until(5_000_000, |_| again.message(16).is_some()));
    assert_eq!(w.peer.dns_queries, 1);
}

#[test]
fn net_hosts_answers_without_a_query_and_may_name_a_local_address() {
    let mut w = World::new(
        Script::Sink,
        &[("gw.zoraya", SERVER_IP), ("web.zoraya", WEB)],
    );
    w.peer.dns_script = Box::new(|_| panic!("a query for a name in net.hosts"));
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("web.zoraya".into()), 80);
    assert!(w.run_until(5_000_000, |_| fake.message(16).is_some()));
    // The gateway's address is local, but the person put it in net.hosts.
    let gw = Fake::new(1024);
    w.connect(&gw, Target::Name("gw.zoraya".into()), 80);
    // Nobody answers ARP for the gateway's address here, so it stays trying;
    // what matters is that it is tried at all and not refused as local.
    w.run_until(2_000_000, |_| false);
    assert_eq!(gw.failed(), None);
    assert_eq!(w.client.tcp_count(), 2);
}

#[test]
fn an_answer_that_is_local_is_refused_unless_an_address_rule_covers_it() {
    for (name, local) in [
        ("loop.zoraya", [127, 0, 0, 1]),
        ("self.zoraya", OFFERED),
        ("gate.zoraya", SERVER_IP),
        ("resolver.zoraya", DNS),
        ("link.zoraya", [169, 254, 1, 1]),
        ("multi.zoraya", [224, 0, 0, 1]),
        ("zero.zoraya", [0, 0, 0, 0]),
    ] {
        let mut w = World::new(Script::Sink, &[]);
        w.scope = names_only();
        w.peer.dns_script = Box::new(move |q| vec![a_record_reply(q, local, 60)]);
        let fake = Fake::new(1024);
        w.connect(&fake, Target::Name(name.into()), 80);
        assert!(
            w.run_until(5_000_000, |_| fake.failed().is_some()),
            "{name}"
        );
        assert_eq!(fake.failed(), Some(reason::LOCAL), "{name}");
    }
    // The gateway, with an address rule that covers it on the port.
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, SERVER_IP, 60)]);
    w.scope = scope_with(&[
        ReachRule {
            target: zoraya_abi::net::Target::Name("gate.zoraya"),
            ports: Ports::ANY,
            protocols: Protocols::BOTH,
        },
        ReachRule {
            target: zoraya_abi::net::Target::Address(SERVER_IP),
            ports: Ports::ANY,
            protocols: Protocols::BOTH,
        },
    ]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("gate.zoraya".into()), 80);
    // Nobody answers ARP for the gateway here, so it stays trying; the point
    // is that it was tried and not refused as local.
    w.run_until(2_000_000, |_| false);
    assert_eq!(fake.failed(), None);
    assert_eq!(w.client.tcp_count(), 1);
}

#[test]
fn a_name_that_does_not_exist_is_unresolvable_and_one_not_answered_times_out() {
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|q| {
        let mut m = q.to_vec();
        m[2] = 0x81;
        m[3] = 0x83;
        vec![m]
    });
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("nothing.zoraya".into()), 80);
    assert!(w.run_until(5_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::UNRESOLVABLE));
    // No answer at all: it gives up after its schedule, having asked again,
    // and says the lookup timed out (proposed ADR-0316), not that the name
    // has no address.
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|_| Vec::new());
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("silent.zoraya".into()), 80);
    assert!(!w.run_until(7_000_000, |_| fake.failed().is_some()));
    assert!(w.run_until(2_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::LOOKUP_TIMED_OUT));
    assert!(
        w.peer.dns_queries >= 3,
        "asked {} times",
        w.peer.dns_queries
    );
    assert_eq!(w.client.dns_pending(), 0);
}

#[test]
fn forged_answers_are_ignored_and_the_real_one_still_lands() {
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|q| {
        let mut wrong_id = a_record_reply(q, [6, 6, 6, 6], 60);
        wrong_id[1] ^= 0xff;
        let mut wrong_name = a_record_reply(q, [6, 6, 6, 6], 60);
        wrong_name[13] ^= 1;
        let mut query_bit = a_record_reply(q, [6, 6, 6, 6], 60);
        query_bit[2] = 0x01;
        vec![
            vec![1, 2, 3],
            wrong_id,
            wrong_name,
            query_bit,
            a_record_reply(q, WEB, 60),
        ]
    });
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("test.zoraya".into()), 80);
    assert!(w.run_until(5_000_000, |_| fake.message(16).is_some()));
    assert!(w.client.dns_rejected() >= 3);
    let c = decode::<Connected>(&fake.message(16).unwrap(), 0)
        .unwrap()
        .1;
    assert_eq!(c.address, u32::from_be_bytes(WEB));
}

#[test]
fn an_answer_from_a_host_that_is_not_the_server_is_ignored() {
    let mut w = World::new(Script::Sink, &[]);
    // The peer answers from 10.0.2.2, not from the configured 10.0.2.3: the
    // reply is a packet sent by a socket bound to that other address.
    let mut other = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 4 * 600]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 4 * 600]),
    );
    other
        .bind(smoltcp::wire::IpListenEndpoint {
            addr: Some(IpAddress::v4(WEB[0], WEB[1], WEB[2], WEB[3])),
            port: 53,
        })
        .unwrap();
    let handle = w.peer.sockets.add(other);
    // The real server stays silent.
    w.peer.dns_script = Box::new(|_| Vec::new());
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("test.zoraya".into()), 80);
    w.run_until(100_000, |_| false);
    let port = w.client.dns_port().expect("a query is out");
    let forged = {
        let mut q = [0u8; 300];
        let n = zoraya_dns::encode_query(0, "test.zoraya", &mut q).unwrap();
        a_record_reply(&q[..n], [6, 6, 6, 6], 60)
    };
    // A reply from 10.0.2.50:53, not the server, to the right port with every
    // identifier a pending query could have (all 65536 would be the point;
    // a few hundred suffice to show the source check comes first).
    for id in 0..300u16 {
        let mut m = forged.clone();
        m[..2].copy_from_slice(&id.to_be_bytes());
        let to = smoltcp::wire::IpEndpoint::new(
            IpAddress::v4(OFFERED[0], OFFERED[1], OFFERED[2], OFFERED[3]),
            port,
        );
        let s = w.peer.sockets.get_mut::<udp::Socket>(handle);
        let _ = s.send_slice(&m, to);
        w.turn();
    }
    assert!(w.run_until(10_000_000, |_| fake.failed().is_some()));
    // The real server never answered: the lookup timed out.
    assert_eq!(fake.failed(), Some(reason::LOOKUP_TIMED_OUT));
    assert!(w.client.dns_rejected() >= 1);
}

#[test]
fn resolve_refuses_bad_names_and_a_full_table_and_a_missing_server() {
    let mut w = World::new(Script::Sink, &[]);
    assert_eq!(
        w.client.resolve(0, "bad name", 0),
        Start::Failed(ResolveFail::BadName)
    );
    for i in 0..crate::dns::MAX_QUERIES {
        assert_eq!(
            w.client.resolve(i as u32, &format!("n{i}.zoraya"), 0),
            Start::Pending
        );
    }
    assert_eq!(
        w.client.resolve(99, "extra.zoraya", 0),
        Start::Failed(ResolveFail::Busy)
    );
    let (a, _, _) = End::pair();
    let mut config = static_config();
    config.dns = Vec::new();
    let mut no_server = Stack::new(a, CLIENT_MAC, config, 0, 3).unwrap();
    assert_eq!(
        no_server.resolve(0, "a.zoraya", 0),
        Start::Failed(ResolveFail::NoServer)
    );
}

#[test]
fn a_connect_by_name_with_no_server_is_unreachable_not_unresolvable() {
    // A static interface with no `net.dns`: nobody to ask, which is a
    // network problem and not "the name has no address" (ADR-0316).
    let mut w = World::with_dns(Script::Sink, &[], Vec::new());
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("a.zoraya".into()), 80);
    assert!(w.run_until(1_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::UNREACHABLE));
}

#[test]
fn many_random_connections_and_random_hostile_peers_never_panic() {
    // The peer sends garbage in the middle of connections; the stack and the
    // connections go on.
    let mut rng = Rng(0xfeed);
    for round in 0..30 {
        let script = match rng.next() % 4 {
            0 => Script::Echo,
            1 => Script::Greet(pattern(rng.next() as usize % 30_000, round)),
            2 => Script::ResetOnData,
            _ => Script::Silent,
        };
        let mut w = World::new(script, &[]);
        let fake = Fake::new(1 + rng.next() as usize % 5000);
        fake.0.borrow_mut().chunk = rng.next() as usize % 100;
        w.connect(&fake, Target::Address(WEB), 80);
        for _ in 0..300 {
            if rng.next().is_multiple_of(5) {
                fake.write(&pattern(rng.next() as usize % 3000, rng.next()));
            }
            if rng.next().is_multiple_of(50) {
                fake.close_write();
            }
            if rng.next().is_multiple_of(40) {
                // A garbage frame from the "wire".
                let mut frame = pattern(60 + rng.next() as usize % 200, rng.next());
                frame[..6].copy_from_slice(&CLIENT_MAC);
                w.peer
                    .device
                    .link
                    .queues
                    .borrow_mut()
                    .to_client
                    .push_back(frame);
            }
            w.turn();
            fake.take();
        }
    }
}

#[test]
fn a_reset_after_the_peers_fin_is_a_message_and_frees_the_slot_and_the_socket() {
    // The review's finding 1: the peer sends its data, closes, and resets when
    // the client's late byte arrives. The client closes its write end after
    // the reset or never.
    for close_after in [false, true] {
        let (mut w, fake, _) = open_world(Script::GreetThenReset(b"bye".to_vec()));
        assert!(w.run_until(5_000_000, |_| fake.0.borrow().in_closed));
        assert_eq!(fake.take(), b"bye");
        assert!(fake.message(18).is_none());
        fake.write(b"late");
        assert!(w.run_until(10_000_000, |_| fake.message(18).is_some()));
        if close_after {
            fake.close_write();
        }
        w.run_until(2_000_000, |_| false);
        assert_eq!(w.conns.count(), 0, "close_after {close_after}");
        assert_eq!(w.client.tcp_count(), 0, "close_after {close_after}");
        let m = fake.message(18).expect("a reset message");
        assert_eq!(decode::<Reset>(&m, 0).unwrap().1.why.0, reset::PEER);
        assert!(fake.0.borrow().in_closed);
        assert_eq!(fake.ordinals().iter().filter(|o| **o == 18).count(), 1);
    }
}

#[test]
fn a_half_closed_connection_that_goes_silent_is_ended_after_the_linger_limit() {
    // The peer closes its side and says nothing more; the client never
    // closes. Eight such connections must not hold the table for ever.
    let (mut w, fake, _) = open_world(Script::Silent);
    // Let the peer's side close: it closes when it sees our FIN.
    fake.close_write();
    assert!(w.run_until(5_000_000, |w| fake.0.borrow().in_closed
        || w.conns.count() == 0));
    // Now a connection that is half closed by the peer and idle: use Greet.
    let (mut w, fake, _) = open_world(Script::Greet(Vec::new()));
    assert!(w.run_until(5_000_000, |_| fake.0.borrow().in_closed));
    assert!(!w.run_until(LINGER_US - 10_000_000, |w| w.conns.count() == 0));
    assert!(w.run_until(30_000_000, |w| w.conns.count() == 0));
    let m = fake.message(18).expect("a reset message");
    assert_eq!(decode::<Reset>(&m, 0).unwrap().1.why.0, reset::ABORTED);
    w.run_until(2_000_000, |_| false);
    assert_eq!(w.client.tcp_count(), 0);
}

#[test]
fn pending_resolutions_are_bounded_per_account() {
    // The server never answers a query: names stay pending.
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|_| Vec::new());
    let (a, b) = (
        Owner {
            account: 1,
            cache: 4,
        },
        Owner {
            account: 2,
            cache: 4,
        },
    );
    let f = Fake::new(1024);
    assert!(
        w.connect_as(&f, a, 1, Target::Name("one.zoraya".into()), 80)
            .is_ok()
    );
    let g = Fake::new(1024);
    assert_eq!(
        w.connect_as(&g, a, 1, Target::Name("two.zoraya".into()), 80),
        Err(reason::OVER_QUOTA),
        "a second pending name of account 1"
    );
    // The refusal left no query behind, and account 2 has its own.
    assert_eq!(w.client.dns_pending(), 1);
    for name in ["x.zoraya", "y.zoraya", "z.zoraya"] {
        let h = Fake::new(1024);
        assert!(
            w.connect_as(&h, b, 3, Target::Name(name.into()), 80)
                .is_ok(),
            "{name}"
        );
    }
    assert_eq!(w.conns.resolving_for(1), 1);
    assert_eq!(w.conns.resolving_for(2), 3);
    // An address needs no resolution, so the bound does not touch it.
    let h = Fake::new(1024);
    assert!(w.connect_as(&h, a, 1, Target::Address(WEB), 80).is_ok());
}

#[test]
fn a_finished_connection_gives_its_account_back_once() {
    let mut w = World::new(Script::Sink, &[]);
    let a = Owner {
        account: 3,
        cache: 4,
    };
    let f = Fake::new(1024);
    w.connect_as(&f, a, 4, Target::Address(WEB), 80).unwrap();
    assert!(w.conns.take_released().is_empty());
    assert!(w.run_until(5_000_000, |_| f.message(16).is_some()));
    f.close_write();
    assert!(w.run_until(60_000_000, |w| w.conns.count() == 0));
    assert_eq!(w.conns.take_released(), vec![3]);
    assert!(w.conns.take_released().is_empty());
    // An abort gives it back too.
    let g = Fake::new(1024);
    let id = w.connect_as(&g, a, 4, Target::Address(WEB), 80).unwrap();
    assert!(w.conns.abort(&mut w.client, id).is_some());
    assert_eq!(w.conns.take_released(), vec![3]);
}

#[test]
fn an_account_that_fills_its_cache_evicts_its_own_entries_not_another_accounts() {
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, WEB, 300)]);
    let (a, b) = (
        Owner {
            account: 1,
            cache: 2,
        },
        Owner {
            account: 2,
            cache: 8,
        },
    );
    let ask = |w: &mut World, owner, name: &str| {
        let f = Fake::new(1024);
        w.connect_as(&f, owner, 4, Target::Name(name.into()), 80)
            .unwrap();
        assert!(
            w.run_until(5_000_000, |_| f.message(16).is_some()),
            "{name}"
        );
        f.close_write();
        assert!(w.run_until(60_000_000, |w| w.conns.count() == 0));
        w.now += 1_000;
    };
    ask(&mut w, b, "bee.zoraya");
    ask(&mut w, a, "a1.zoraya");
    ask(&mut w, a, "a2.zoraya");
    assert!(w.client.dns_cached("a1.zoraya") && w.client.dns_cached("a2.zoraya"));
    // A third name of account 1 pushes out its own earliest, not b's.
    ask(&mut w, a, "a3.zoraya");
    assert!(!w.client.dns_cached("a1.zoraya"), "a1 was a's oldest");
    assert!(w.client.dns_cached("a2.zoraya") && w.client.dns_cached("a3.zoraya"));
    assert!(
        w.client.dns_cached("bee.zoraya"),
        "another account's entry stays"
    );
    // An account with no cache room caches nothing.
    let none = Owner {
        account: 4,
        cache: 0,
    };
    ask(&mut w, none, "nothing.zoraya");
    assert!(!w.client.dns_cached("nothing.zoraya"));
}

/// `resolve` through the service over the scripted DNS server: the request
/// is deferred, the resolver's answer comes out of `take_lookups`, and the
/// service turns it into the reply (N4).
mod resolve {
    use super::*;
    use crate::accounts::Kind;
    use crate::service::{Action, Service};
    use zoraya_abi::net::rights;
    use zoraya_proto_net::{Address, NarrowAccount, Resolve, Resolved, status};
    use zoraya_protocol::{Below, Data, encode};

    fn ask(service: &mut Service, w: &mut World, id: usize, name: &str) -> Action {
        let mut out = vec![0u8; 512];
        let n = encode(
            &Resolve {
                name: Data(name.as_bytes()),
            },
            9,
            &mut out,
        )
        .unwrap();
        service.handle(id, &out[..n], 0, &mut w.client, w.now)
    }

    fn answer(bytes: &[u8]) -> (u32, Vec<[u8; 4]>) {
        let (_, r) = decode::<Resolved<'_>>(bytes, 0).unwrap();
        let list: Vec<Address> = r.addresses.iter().collect();
        (
            r.status.0,
            list.iter().map(|a| a.address.to_be_bytes()).collect(),
        )
    }

    /// Runs the world until the service has the reply for `id`.
    fn reply_for(service: &mut Service, w: &mut World, id: usize) -> Vec<u8> {
        for _ in 0..20_000 {
            w.turn();
            for a in w.conns.take_lookups() {
                if let Some((to, bytes)) = service.resolve_done(&mut w.client, a, w.now) {
                    assert_eq!(to, id);
                    return bytes;
                }
            }
        }
        panic!("no answer");
    }

    #[test]
    fn a_name_resolves_to_its_addresses_and_is_cached_on_the_account() {
        let mut w = World::new(Script::Sink, &[]);
        w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, WEB, 300)]);
        let mut service = Service::new();
        let root = service.add(Connection::root()).unwrap();
        let first = ask(&mut service, &mut w, root, "test.zoraya");
        let Action::Deferred = first else {
            panic!("deferred: {first:?}");
        };
        // The charge is held while it runs.
        assert_eq!(service.accounts().used(0, Kind::Resolutions), 1);
        let bytes = reply_for(&mut service, &mut w, root);
        assert_eq!(answer(&bytes), (status::OK, vec![WEB]));
        assert_eq!(service.accounts().used(0, Kind::Resolutions), 0);
        assert_eq!(w.peer.dns_queries, 1, "one query for one resolve");
        // A second resolve of the name is answered from the cache at once.
        let Action::Reply(bytes) = ask(&mut service, &mut w, root, "test.zoraya") else {
            panic!("cached");
        };
        assert_eq!(answer(&bytes), (status::OK, vec![WEB]));
        assert_eq!(w.peer.dns_queries, 1);
    }

    /// Lookups answered from the cache use tokens too; after 65,536 of them
    /// the 16-bit counter reaches the token of a slow lookup that is still
    /// pending. A new lookup must not take it (its answer would go to the
    /// other connection).
    #[test]
    fn a_wrapped_lookup_token_never_takes_a_pending_ones() {
        let mut w = World::new(Script::Sink, &[]);
        // The slow name is answered only once the test lets it, and the two
        // names get different addresses, so a reply says which lookup it
        // answers.
        let release = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let gate = release.clone();
        w.peer.dns_script = Box::new(move |q| {
            let slow = q.windows(4).any(|w| w == b"slow");
            if slow && !gate.load(std::sync::atomic::Ordering::SeqCst) {
                return vec![];
            }
            vec![a_record_reply(
                q,
                if slow { WEB } else { [10, 9, 9, 9] },
                300,
            )]
        });
        let open = release.clone();
        release.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut service = Service::new();
        let a = service.add(Connection::root()).unwrap();
        let b = service.add(Connection::root()).unwrap();
        // Token 0 fills the cache; token 1 is the slow lookup of `a`.
        let Action::Deferred = ask(&mut service, &mut w, a, "cached.zoraya") else {
            panic!("deferred");
        };
        reply_for(&mut service, &mut w, a);
        open.store(false, std::sync::atomic::Ordering::SeqCst);
        let Action::Deferred = ask(&mut service, &mut w, a, "slow.zoraya") else {
            panic!("deferred");
        };
        // Tokens 2..=65535 and 0 from the cache: the next would be 1 again.
        for _ in 0..65_535 {
            let Action::Reply(_) = ask(&mut service, &mut w, b, "cached.zoraya") else {
                panic!("cached");
            };
        }
        let Action::Deferred = ask(&mut service, &mut w, b, "other.zoraya") else {
            panic!("deferred");
        };
        let mut to = Vec::new();
        let mut seen = Vec::new();
        for _ in 0..20_000 {
            w.turn();
            for r in w.conns.take_lookups() {
                if let Some((id, bytes)) = service.resolve_done(&mut w.client, r, w.now) {
                    to.push(id);
                    seen.push((id, answer(&bytes).1));
                    open.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            if to.len() == 2 {
                break;
            }
        }
        seen.sort();
        let mut want = vec![(a, vec![WEB]), (b, vec![[10, 9, 9, 9]])];
        want.sort();
        assert_eq!(seen, want, "each answer to its own asker");
    }

    #[test]
    fn a_name_that_does_not_exist_and_one_that_is_not_in_scope_are_refused_apart() {
        let mut w = World::new(Script::Sink, &[]);
        w.peer.dns_script = Box::new(|q| {
            let mut m = q.to_vec();
            m[2] = 0x81;
            m[3] = 0x83;
            vec![m]
        });
        let mut service = Service::new();
        let only = Connection::new(
            rights::REACH,
            &scope_with(&[ReachRule {
                target: zoraya_abi::net::Target::Name("gone.zoraya"),
                ports: Ports::one(80),
                protocols: Protocols::TCP,
            }]),
        )
        .unwrap();
        let id = service.add(only).unwrap();
        let Action::Reply(bytes) = ask(&mut service, &mut w, id, "other.zoraya") else {
            panic!("refused at once");
        };
        assert_eq!(answer(&bytes).0, status::NOT_GRANTED);
        assert_eq!(
            w.peer.dns_queries, 0,
            "nothing was asked for the refused name"
        );
        let Action::Deferred = ask(&mut service, &mut w, id, "gone.zoraya") else {
            panic!("deferred");
        };
        let bytes = reply_for(&mut service, &mut w, id);
        assert_eq!(answer(&bytes), (status::UNRESOLVABLE, vec![]));
        // No `reach` right: not granted either.
        let status_only = service
            .add(Connection::new(rights::STATUS, &scope_with(&[])).unwrap())
            .unwrap();
        let Action::Reply(bytes) = ask(&mut service, &mut w, status_only, "gone.zoraya") else {
            panic!("refused");
        };
        assert_eq!(answer(&bytes).0, status::NOT_GRANTED);
    }

    #[test]
    fn a_local_answer_is_left_out_unless_an_address_rule_covers_it() {
        let mut w = World::new(Script::Sink, &[]);
        w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, [127, 0, 0, 1], 60)]);
        let mut service = Service::new();
        let names = service
            .add(Connection::new(rights::REACH, &names_only()).unwrap())
            .unwrap();
        let Action::Deferred = ask(&mut service, &mut w, names, "loop.zoraya") else {
            panic!("deferred");
        };
        let bytes = reply_for(&mut service, &mut w, names);
        assert_eq!(answer(&bytes), (status::LOCAL, vec![]));
        // With a rule for the address as well, it is given.
        let both = scope_with(&[
            ReachRule {
                target: zoraya_abi::net::Target::Wildcard("zoraya"),
                ports: Ports::ANY,
                protocols: Protocols::BOTH,
            },
            ReachRule {
                target: zoraya_abi::net::Target::Address([127, 0, 0, 1]),
                ports: Ports::ANY,
                protocols: Protocols::BOTH,
            },
        ]);
        let wide = service
            .add(Connection::new(rights::REACH, &both).unwrap())
            .unwrap();
        let Action::Deferred = ask(&mut service, &mut w, wide, "loop2.zoraya") else {
            panic!("deferred");
        };
        let bytes = reply_for(&mut service, &mut w, wide);
        assert_eq!(answer(&bytes), (status::OK, vec![[127, 0, 0, 1]]));
    }

    #[test]
    fn pending_names_are_bounded_per_account_and_one_per_connection() {
        let mut w = World::new(Script::Sink, &[]);
        w.peer.dns_script = Box::new(|_| Vec::new());
        let mut service = Service::new();
        let root = service.add(Connection::root()).unwrap();
        let mut out = vec![0u8; 512];
        let n = encode(
            &NarrowAccount {
                rights: rights::REACH | rights::NARROW,
                budget: Below(1),
                scope: Data(&names_only()),
            },
            3,
            &mut out,
        )
        .unwrap();
        let Action::Narrowed { id: small, .. } =
            service.handle(root, &out[..n], 1, &mut w.client, w.now)
        else {
            panic!("carved");
        };
        // A second connection of the same account.
        let mut out = vec![0u8; 512];
        let n = encode(
            &zoraya_proto_net::Narrow {
                rights: rights::REACH,
                scope: Data(&names_only()),
            },
            3,
            &mut out,
        )
        .unwrap();
        let Action::Narrowed { id: twin, .. } =
            service.handle(small, &out[..n], 1, &mut w.client, w.now)
        else {
            panic!("narrowed");
        };
        let Action::Deferred = ask(&mut service, &mut w, small, "a.zoraya") else {
            panic!("deferred");
        };
        // One at a time on a connection.
        let Action::Reply(bytes) = ask(&mut service, &mut w, small, "b.zoraya") else {
            panic!("busy");
        };
        assert_eq!(answer(&bytes).0, status::BUSY);
        // The budget of 1 allows one pending name in the account.
        let Action::Reply(bytes) = ask(&mut service, &mut w, twin, "c.zoraya") else {
            panic!("over quota");
        };
        assert_eq!(answer(&bytes).0, status::OVER_QUOTA);
        // The session still resolves.
        let Action::Deferred = ask(&mut service, &mut w, root, "d.zoraya") else {
            panic!("session deferred");
        };
        // The asking connections go: the charge goes with the answer, which
        // has nowhere to go (the resolver gives up at 8 s).
        service.remove(small);
        service.remove(twin);
        for _ in 0..20_000 {
            w.turn();
            for a in w.conns.take_lookups() {
                if let Some((to, _)) = service.resolve_done(&mut w.client, a, w.now) {
                    assert_eq!(to, root, "only the session's connection is left");
                }
            }
            if service.accounts().live() == 1 && service.accounts().used(0, Kind::Resolutions) == 0
            {
                return;
            }
        }
        panic!("the lookups did not end");
    }
}

/// `ping` by name through the service and the resolver (ADR-0330): the answer
/// of the DNS server, filtered as `resolve` filters it.
mod ping_by_name {
    use super::*;
    use crate::service::{Action, Service};
    use zoraya_abi::net::{Target as ScopeTarget, rights};
    use zoraya_proto_net::{Ping, PingReply, status};
    use zoraya_protocol::{Below, Data, encode};

    const GATEWAY: [u8; 4] = [10, 0, 2, 2];

    fn echo_scope(rules: &[(ScopeTarget<'_>, Protocols)]) -> Vec<u8> {
        let reach: Vec<ReachRule<'_>> = rules
            .iter()
            .map(|(target, protocols)| ReachRule {
                target: *target,
                ports: Ports::ANY,
                protocols: *protocols,
            })
            .collect();
        scope_with(&reach)
    }

    fn ask(service: &mut Service, w: &mut World, id: usize, name: &str) -> Action {
        let mut out = vec![0u8; 512];
        let n = encode(
            &Ping {
                address: 0,
                count: Below(1),
                timeout_ms: Below(1000),
                name: Data(name.as_bytes()),
            },
            9,
            &mut out,
        )
        .unwrap();
        service.handle(id, &out[..n], 0, &mut w.client, w.now)
    }

    /// Asks, and gives the reply to the asker: `None` when a run started
    /// (at once from the cache, or after the lookup).
    fn outcome(service: &mut Service, w: &mut World, id: usize, name: &str) -> Option<Vec<u8>> {
        match ask(service, w, id, name) {
            Action::Reply(bytes) => Some(bytes),
            Action::Deferred if w.client.ping_running() => None,
            Action::Deferred => lookup(service, w),
            other => panic!("{other:?}"),
        }
    }

    /// Runs until the lookup is answered: the reply to the asker, or `None`
    /// when the run started.
    fn lookup(service: &mut Service, w: &mut World) -> Option<Vec<u8>> {
        for _ in 0..20_000 {
            w.turn();
            if let Some(a) = w.conns.take_lookups().into_iter().next() {
                return service.resolve_done(&mut w.client, a, w.now).map(|r| r.1);
            }
        }
        panic!("no answer");
    }

    /// Lets a started run end and gives its end to the service.
    fn finish(service: &mut Service, w: &mut World) {
        assert!(w.client.ping_running(), "a run was started");
        assert!(w.run_until(10_000_000, |w| !w.client.ping_running()));
        for e in w.client.take_events() {
            if let Event::PingDone(r) = e {
                let _ = service.ping_done(r);
            }
        }
    }

    fn code(bytes: &[u8]) -> u32 {
        decode::<PingReply<'_>>(bytes, 0).unwrap().1.status.0
    }

    #[test]
    fn an_answer_on_this_side_of_the_network_is_pinged_only_under_an_echo_rule_for_it() {
        let mut w = World::new(Script::Sink, &[]);
        // The name decides the address: the gateway, the DNS server (both of
        // this side of the network) or a host beyond.
        w.peer.dns_script = Box::new(|q| {
            let address = if q.windows(2).any(|w| w == b"gw") {
                GATEWAY
            } else if q.windows(3).any(|w| w == b"dns") {
                DNS
            } else {
                WEB
            };
            vec![a_record_reply(q, address, 300)]
        });
        let mut service = Service::new();
        let narrow = |rules: &[(ScopeTarget<'_>, Protocols)]| {
            Connection::new(rights::REACH, &echo_scope(rules)).unwrap()
        };
        let zoraya = (ScopeTarget::Wildcard("zoraya"), Protocols::ECHO);
        let only_names = service.add(narrow(&[zoraya])).unwrap();
        // The gateway and the DNS server are refused as local.
        for name in ["gw.zoraya", "dns.zoraya"] {
            let reply = outcome(&mut service, &mut w, only_names, name).expect("a refusal");
            assert_eq!(code(&reply), status::LOCAL, "{name}");
            assert!(!w.client.ping_running());
        }
        // A host beyond them is pinged.
        assert!(
            outcome(&mut service, &mut w, only_names, "web.zoraya").is_none(),
            "the run started"
        );
        finish(&mut service, &mut w);
        // An echo rule for the gateway's address allows that answer.
        let both = service
            .add(narrow(&[
                zoraya,
                (ScopeTarget::Address(GATEWAY), Protocols::ECHO),
            ]))
            .unwrap();
        assert!(
            outcome(&mut service, &mut w, both, "gw.zoraya").is_none(),
            "the run started"
        );
        finish(&mut service, &mut w);
        // A TCP rule for that address is no echo rule.
        let tcp = service
            .add(narrow(&[
                zoraya,
                (ScopeTarget::Address(GATEWAY), Protocols::TCP),
            ]))
            .unwrap();
        let reply = outcome(&mut service, &mut w, tcp, "gw.zoraya").expect("a refusal");
        assert_eq!(code(&reply), status::LOCAL);
    }

    #[test]
    fn a_non_unicast_answer_is_not_pinged() {
        let mut w = World::new(Script::Sink, &[]);
        w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, [255, 255, 255, 255], 300)]);
        let mut service = Service::new();
        let id = service
            .add(
                Connection::new(
                    rights::REACH,
                    &echo_scope(&[(ScopeTarget::Any, Protocols::ECHO)]),
                )
                .unwrap(),
            )
            .unwrap();
        let reply = outcome(&mut service, &mut w, id, "any.zoraya").expect("a refusal");
        assert_ne!(code(&reply), status::OK);
        assert!(!w.client.ping_running());
    }

    /// A `ping` of an address, with a 1 ms wait: the run ends at once.
    fn ping_address(service: &mut Service, w: &mut World, id: usize) -> Action {
        let mut out = vec![0u8; 512];
        let n = encode(
            &Ping {
                address: u32::from_be_bytes(GATEWAY),
                count: Below(1),
                timeout_ms: Below(1),
                name: Data(b""),
            },
            9,
            &mut out,
        )
        .unwrap();
        service.handle(id, &out[..n], 0, &mut w.client, w.now)
    }

    /// B, with an echo rule for names under `zoraya`, and a resolver that
    /// never answers (so a named ping of B stays in its lookup).
    fn world_with_b() -> (World, Service, usize, usize) {
        let mut w = World::new(Script::Sink, &[]);
        w.peer.dns_script = Box::new(|_| Vec::new());
        let mut service = Service::new();
        let a = service.add(Connection::root()).unwrap();
        let scope = echo_scope(&[(ScopeTarget::Wildcard("zoraya"), Protocols::ECHO)]);
        let b = service
            .add(Connection::new(rights::REACH, &scope).unwrap())
            .unwrap();
        (w, service, a, b)
    }

    fn busy(action: Action) {
        let Action::Reply(bytes) = action else {
            panic!("busy, got {action:?}");
        };
        assert_eq!(code(&bytes), status::BUSY);
    }

    #[test]
    fn a_run_whose_connection_went_is_not_handed_to_a_named_ping_that_follows() {
        // Kills the removal of both defences together (the slot kept by
        // `remove`, and the stack's own check).
        let (mut w, mut service, a, b) = world_with_b();
        assert_eq!(ping_address(&mut service, &mut w, a), Action::Deferred);
        service.remove(a);
        busy(ask(&mut service, &mut w, b, "b.zoraya"));
        assert!(w.run_until(10_000_000, |w| !w.client.ping_running()));
        for e in w.client.take_events() {
            if let Event::PingDone(r) = e {
                assert!(service.ping_done(r).is_none(), "A's result goes to nobody");
            }
        }
        // The slot is free again.
        assert_eq!(ask(&mut service, &mut w, b, "b.zoraya"), Action::Deferred);
    }

    #[test]
    fn the_slot_of_a_gone_connection_stays_taken_until_its_run_is_delivered() {
        // The run is over in the stack, its end not yet delivered: only the
        // slot (not `ping_running`) keeps B out. Kills a `remove` that clears
        // the slot.
        let (mut w, mut service, a, b) = world_with_b();
        assert_eq!(ping_address(&mut service, &mut w, a), Action::Deferred);
        service.remove(a);
        assert!(w.run_until(10_000_000, |w| !w.client.ping_running()));
        busy(ask(&mut service, &mut w, b, "b.zoraya"));
        for e in w.client.take_events() {
            if let Event::PingDone(r) = e {
                assert!(service.ping_done(r).is_none(), "A's result goes to nobody");
            }
        }
        assert_eq!(ask(&mut service, &mut w, b, "b.zoraya"), Action::Deferred);
    }

    #[test]
    fn a_named_ping_is_refused_while_the_stack_runs_one_the_slot_does_not_know() {
        // Defence in depth: the slot is free but a run goes. Kills the removal
        // of the `ping_running` check.
        let (mut w, mut service, _a, b) = world_with_b();
        w.client.ping(GATEWAY, 1, 1_000, w.now).unwrap();
        busy(ask(&mut service, &mut w, b, "b.zoraya"));
        assert!(w.client.ping_running());
    }

    #[test]
    fn a_named_ping_whose_connection_went_in_its_lookup_frees_the_slot() {
        // A pings by name and goes; B takes A's id; the lookup ends (nobody
        // answers): no run for A, and B's own ping is not refused as busy.
        // Kills the removal of the slot release at the lookup's end.
        let (mut w, mut service, a, _b) = world_with_b();
        let scope = echo_scope(&[(ScopeTarget::Wildcard("zoraya"), Protocols::ECHO)]);
        service.remove(_b);
        service.remove(a);
        let a = service
            .add(Connection::new(rights::REACH, &scope).unwrap())
            .unwrap();
        assert_eq!(ask(&mut service, &mut w, a, "a.zoraya"), Action::Deferred);
        service.remove(a);
        let b = service
            .add(Connection::new(rights::REACH, &scope).unwrap())
            .unwrap();
        assert_eq!(b, a, "ids are reused");
        assert!(lookup(&mut service, &mut w).is_none(), "no reply, no run");
        assert!(!w.client.ping_running());
        assert_eq!(ask(&mut service, &mut w, b, "b.zoraya"), Action::Deferred);
    }
}

/// The targets of the ARP requests the client sent, in order.
fn arp_requests(frames: &[Vec<u8>]) -> Vec<[u8; 4]> {
    frames
        .iter()
        .filter(|f| f.len() >= 42 && f[12..14] == [0x08, 0x06] && f[20..22] == [0, 1])
        .map(|f| [f[38], f[39], f[40], f[41]])
        .collect()
}

/// Whether `frame` is a UDP datagram to port 53.
fn is_dns_query(frame: &[u8]) -> bool {
    frame.len() >= 42
        && frame[12..14] == [0x08, 0x00]
        && frame[23] == 17
        && frame[36..38] == [0, 53]
}

#[test]
fn the_first_query_to_a_server_whose_address_is_not_known_leaves_with_the_arp_reply() {
    // The architect's first hypothesis for "the first `ping NAME` fails": the
    // query sent while the server's MAC is unknown is dropped. It is not:
    // smoltcp keeps the datagram in the socket, asks with ARP, and sends it
    // as soon as the reply fills its cache, long before the resolver's
    // retry at 1 s. The query starts before the stack's first poll, so no
    // probe has asked for the server yet.
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, WEB, 60)]);
    let token = crate::conn::LOOKUP_BASE + 1;
    assert_eq!(
        w.client.resolve(token, "first.zoraya", w.now),
        Start::Pending
    );
    // Woken as `netd` is: by a frame, or at the delay the turn asked for.
    let mut answers = Vec::new();
    while answers.is_empty() && w.now < 1_000_000 {
        w.peer.poll(w.now);
        let (delay, _) = w.conns.turn(&mut w.client, w.now, |_, _, _| {});
        answers.extend(w.conns.take_lookups());
        w.peer.poll(w.now);
        if !answers.is_empty() {
            break;
        }
        w.now += if w.client.link().has_input() {
            100
        } else {
            delay.unwrap_or(1_000_000).max(1)
        };
    }
    assert!(w.now < 100_000, "answered at {} us", w.now);
    assert_eq!(w.peer.dns_queries, 1, "one query, no retry needed");
    assert_eq!(answers[0].token, token);
    assert_eq!(
        answers[0].result.as_ref().map(|f| f.addresses.clone()),
        Ok(vec![WEB])
    );
    // The query left after an ARP request for the server.
    let q = w.queues.borrow();
    let first_query = q.from_client.iter().position(|f| is_dns_query(f)).unwrap();
    assert!(arp_requests(&q.from_client[..first_query]).contains(&DNS));
}

/// ARP requests the client sent for `target`.
fn asked_for(w: &World, target: [u8; 4]) -> usize {
    arp_requests(&w.queues.borrow().from_client)
        .iter()
        .filter(|a| **a == target)
        .count()
}

#[test]
fn a_lookup_asks_for_the_gateway_unless_it_was_asked_for_recently() {
    // smoltcp asks for one neighbour a second: after the lookup's own ARP
    // request, the SYN or echo request to the gateway that follows the
    // answer waited a second when the gateway was not known. The stack asks
    // for the gateway and the servers itself when the interface is
    // configured, and for the gateway when a lookup goes out, at most once
    // per `PROBE_FRESH_US`.
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, WEB, 1)]);
    w.run_until(100_000, |_| false);
    assert_eq!(asked_for(&w, SERVER_IP), 1, "the gateway, when configured");
    assert_eq!(asked_for(&w, DNS), 1, "the server, when configured");
    let lookup = |w: &mut World, name: &str| {
        let token = crate::conn::LOOKUP_BASE + 7;
        assert_eq!(w.client.resolve(token, name, w.now), Start::Pending);
        w.run_until(500_000, |_| false);
        let _ = w.conns.take_lookups();
    };
    lookup(&mut w, "a.zoraya");
    assert_eq!(asked_for(&w, SERVER_IP), 1, "fresh: not asked again");
    assert_eq!(asked_for(&w, DNS), 1, "the server answered: known");
    w.run_until(crate::stack::PROBE_FRESH_US, |_| false);
    lookup(&mut w, "b.zoraya");
    assert_eq!(
        asked_for(&w, SERVER_IP),
        2,
        "stale: asked again with the lookup"
    );
}

fn ping_name_request(name: &str, count: u32) -> Vec<u8> {
    let mut out = vec![0u8; 512];
    let n = zoraya_protocol::encode(
        &zoraya_proto_net::Ping {
            address: 0,
            count: zoraya_protocol::Below(count),
            timeout_ms: zoraya_protocol::Below(2000),
            name: zoraya_protocol::Data(name.as_bytes()),
        },
        5,
        &mut out,
    )
    .unwrap();
    out.truncate(n);
    out
}

/// One turn of `netd`'s loop as `user/src/bin/netd.rs` makes it: the
/// connections' turn (which polls the stack), the lookups the service
/// answers, the events. The replies go to `replies`; returns the delay the
/// loop sleeps on.
fn netd_turn(
    w: &mut World,
    service: &mut crate::service::Service,
    replies: &mut Vec<(usize, Vec<u8>)>,
) -> Option<i64> {
    w.peer.poll(w.now);
    let (delay, finished) = w.conns.turn(&mut w.client, w.now, |_, _, _| {});
    w.finished.extend(finished);
    let lookups = w.conns.take_lookups();
    let (delay, r) = service.answer_lookups(&mut w.client, lookups, delay, w.now);
    replies.extend(r);
    for e in w.client.take_events() {
        if let Event::PingDone(result) = e {
            replies.extend(service.ping_done(result));
        }
    }
    w.peer.poll(w.now);
    delay
}

/// Runs `netd`'s loop for at most `limit_us` of time, waking only as `netd`
/// does: for a frame from the peer, or when the delay its last turn asked
/// for is over. `false` if it went to sleep with nothing that would wake it,
/// or the limit passed, before `done`.
fn run_as_netd(
    w: &mut World,
    service: &mut crate::service::Service,
    replies: &mut Vec<(usize, Vec<u8>)>,
    limit_us: i64,
    done: impl Fn(&[(usize, Vec<u8>)]) -> bool,
) -> bool {
    let end = w.now + limit_us;
    while w.now < end {
        let delay = netd_turn(w, service, replies);
        if done(replies) {
            return true;
        }
        if w.client.link().has_input() {
            w.now += 100;
            continue;
        }
        match delay {
            Some(d) => w.now += d.max(1),
            // `port_wait` with no deadline and no frame to come.
            None => return false,
        }
    }
    false
}

#[test]
fn a_ping_by_name_answered_over_the_wire_starts_its_run_without_another_wake_up() {
    // The cause of "the first `ping NAME` of a boot fails" (TASK-26's hand
    // run): the answer came in the turn's poll, the run started after it,
    // and the loop slept on the delay it had computed before the run, which
    // with no other timer was none at all. The run, and its reply, waited
    // for something else to wake `netd`; the client gave up first.
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, WEB, 300)]);
    // The stack settles first (its probes are answered or not): nothing is
    // due, the loop sleeps until a request comes.
    w.run_until(2_000_000, |_| false);
    let mut service = crate::service::Service::new();
    let id = service.add(Connection::root()).unwrap();
    let action = service.handle(
        id,
        &ping_name_request("web.zoraya", 2),
        0,
        &mut w.client,
        w.now,
    );
    assert_eq!(action, crate::service::Action::Deferred);
    let mut replies = Vec::new();
    assert!(
        run_as_netd(&mut w, &mut service, &mut replies, 5_000_000, |r| !r
            .is_empty()),
        "netd went to sleep with the run pending, or did not answer in time"
    );
    assert_eq!(w.peer.dns_queries, 1);
    let (to, bytes) = &replies[0];
    assert_eq!(*to, id);
    let (_, r) = zoraya_protocol::decode::<zoraya_proto_net::PingReply>(bytes, 0).unwrap();
    assert_eq!(r.status.0, zoraya_proto_net::status::OK);
    assert_eq!(r.address, u32::from_be_bytes(WEB));
    assert_eq!((r.sent, r.received), (2, 2));
    // A name from the cache starts its run inside the request; that path
    // never depended on the late answer.
    let action = service.handle(
        id,
        &ping_name_request("web.zoraya", 1),
        0,
        &mut w.client,
        w.now,
    );
    assert_eq!(action, crate::service::Action::Deferred);
    replies.clear();
    assert!(run_as_netd(
        &mut w,
        &mut service,
        &mut replies,
        3_000_000,
        |r| !r.is_empty()
    ));
    assert_eq!(w.peer.dns_queries, 1);
}

/// A web rule (ADR-0324): any public host on 80 and 443.
fn web_only(extra: &[ReachRule<'_>]) -> Vec<u8> {
    let mut rules = vec![ReachRule {
        target: zoraya_abi::net::Target::Public,
        ports: Ports::ranges(&[(80, 80), (443, 443)]).unwrap(),
        protocols: Protocols::TCP,
    }];
    rules.extend_from_slice(extra);
    scope_with(&rules)
}

/// INV-0324-1: a name only a web rule admits is connected to a public
/// answer, and never to one that is private, loopback, link-local, this
/// machine's or its network's (the gateway, a DNS server, public or not),
/// from the DNS or from `net.hosts`; an address rule beside it names one.
#[test]
fn a_name_only_a_web_rule_admits_uses_public_answers_that_are_not_local() {
    const PUBLIC: [u8; 4] = [93, 184, 215, 14];
    // A public answer is tried (nobody answers ARP for an off-link address
    // here, so it stays trying; the point is that it is tried).
    let mut w = World::new(Script::Sink, &[]);
    w.scope = web_only(&[]);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, PUBLIC, 60)]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("example.org".into()), 443);
    w.run_until(2_000_000, |_| false);
    assert_eq!(fake.failed(), None);
    assert_eq!(w.client.tcp_count(), 1);
    // Every local or private answer is refused as local, with no socket.
    for (name, refused) in [
        ("lan.example", WEB),
        ("gate.example", SERVER_IP),
        ("resolver.example", DNS),
        ("self.example", OFFERED),
        ("loop.example", [127, 0, 0, 1]),
        ("home.example", [192, 168, 1, 1]),
        ("link.example", [169, 254, 169, 254]),
        ("cgnat.example", [100, 64, 0, 1]),
    ] {
        let mut w = World::new(Script::Sink, &[]);
        w.scope = web_only(&[]);
        w.peer.dns_script = Box::new(move |q| vec![a_record_reply(q, refused, 60)]);
        let fake = Fake::new(1024);
        w.connect(&fake, Target::Name(name.into()), 80);
        assert!(
            w.run_until(5_000_000, |_| fake.failed().is_some()),
            "{name}"
        );
        assert_eq!(fake.failed(), Some(reason::LOCAL), "{name}");
        assert_eq!(w.client.tcp_count(), 0, "{name}");
    }
    // A public DNS server is this side of the network: refused too.
    let mut w = World::with_dns(Script::Sink, &[], vec![DNS, [1, 1, 1, 1]]);
    w.scope = web_only(&[]);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, [1, 1, 1, 1], 60)]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("dns.example".into()), 80);
    assert!(w.run_until(5_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::LOCAL));
    // And as an address: the web rule admits it (it is public), but never
    // names it, so the local rule still refuses it.
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Address([1, 1, 1, 1]), 80);
    assert!(w.run_until(1_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::LOCAL));
    assert_eq!(w.client.tcp_count(), 0);
    // `net.hosts` gives no exemption to a name only the web rule admits.
    let mut w = World::new(Script::Sink, &[("lan.zoraya", WEB)]);
    w.scope = web_only(&[]);
    w.peer.dns_script = Box::new(|_| panic!("a query for a name in net.hosts"));
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("lan.zoraya".into()), 80);
    assert!(w.run_until(1_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::LOCAL));
    // An address rule beside it names the address: connected.
    let mut w = World::new(Script::Sink, &[("lan.zoraya", WEB)]);
    w.scope = web_only(&[ReachRule {
        target: zoraya_abi::net::Target::Address(WEB),
        ports: Ports::one(80),
        protocols: Protocols::TCP,
    }]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("lan.zoraya".into()), 80);
    assert!(w.run_until(5_000_000, |_| fake.message(16).is_some()));
    // A rule that names the name keeps ADR-0223's rule: net.hosts may name a
    // local address.
    let mut w = World::new(Script::Sink, &[("lan.zoraya", WEB)]);
    w.scope = web_only(&[ReachRule {
        target: zoraya_abi::net::Target::Name("lan.zoraya"),
        ports: Ports::one(80),
        protocols: Protocols::TCP,
    }]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("lan.zoraya".into()), 80);
    assert!(w.run_until(5_000_000, |_| fake.message(16).is_some()));
}

/// A public-only name rule (ADR-0328): the URL's name under `net web` on a
/// port the web rule does not have.
fn public_name_only(name: &'static str, port: u16) -> Vec<u8> {
    scope_with(&[ReachRule {
        target: zoraya_abi::net::Target::PublicName(name),
        ports: Ports::one(port),
        protocols: Protocols::TCP,
    }])
}

/// INV-0328-1: a connection by a name that only a public-only name rule
/// admits is made to a public answer, and never to a private or local one,
/// from the DNS or from `net.hosts`; a rule that names the name beside it
/// keeps ADR-0223's rule.
#[test]
fn a_name_only_a_public_name_rule_admits_uses_public_answers_that_are_not_local() {
    const PUBLIC: [u8; 4] = [93, 184, 215, 14];
    let mut w = World::new(Script::Sink, &[]);
    w.scope = public_name_only("evil.example", 8080);
    w.peer.dns_script = Box::new(|q| vec![a_record_reply(q, PUBLIC, 60)]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("evil.example".into()), 8080);
    w.run_until(2_000_000, |_| false);
    assert_eq!(fake.failed(), None);
    assert_eq!(w.client.tcp_count(), 1);
    // The rebinding: the same name, a private answer the next time.
    for refused in [
        [192, 168, 1, 1],
        [10, 1, 2, 3],
        WEB,
        SERVER_IP,
        DNS,
        [127, 0, 0, 1],
        [169, 254, 169, 254],
    ] {
        let mut w = World::new(Script::Sink, &[]);
        w.scope = public_name_only("evil.example", 8080);
        w.peer.dns_script = Box::new(move |q| vec![a_record_reply(q, refused, 0)]);
        let fake = Fake::new(1024);
        w.connect(&fake, Target::Name("evil.example".into()), 8080);
        assert!(
            w.run_until(5_000_000, |_| fake.failed().is_some()),
            "{refused:?}"
        );
        assert_eq!(fake.failed(), Some(reason::LOCAL), "{refused:?}");
        assert_eq!(w.client.tcp_count(), 0, "{refused:?}");
    }
    // `net.hosts` gives no exemption either.
    let mut w = World::new(Script::Sink, &[("lan.web.test", WEB)]);
    w.scope = public_name_only("lan.web.test", 8080);
    w.peer.dns_script = Box::new(|_| panic!("a query for a name in net.hosts"));
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("lan.web.test".into()), 8080);
    assert!(w.run_until(1_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::LOCAL));
    assert_eq!(w.client.tcp_count(), 0);
    // A `+net` word's name rule beside it names the name: net.hosts may name
    // a local address (ADR-0223).
    let mut w = World::new(Script::Sink, &[("lan.web.test", WEB)]);
    w.scope = scope_with(&[
        ReachRule {
            target: zoraya_abi::net::Target::PublicName("lan.web.test"),
            ports: Ports::one(80),
            protocols: Protocols::TCP,
        },
        ReachRule {
            target: zoraya_abi::net::Target::Name("lan.web.test"),
            ports: Ports::one(80),
            protocols: Protocols::TCP,
        },
    ]);
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("lan.web.test".into()), 80);
    assert!(w.run_until(5_000_000, |_| fake.message(16).is_some()));
}

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
        let (a, b, _) = End::pair();
        let mut config = static_config();
        config.dns = vec![DNS];
        config.hosts = hosts.iter().map(|(n, a)| (n.to_string(), *a)).collect();
        World {
            client: Stack::new(a, CLIENT_MAC, config, 0, 3).unwrap(),
            conns: Conns::new(),
            peer: Peer::new(b, script),
            now: 0,
            finished: Vec::new(),
            scope: Connection::root().scope_bytes().to_vec(),
            steps: Vec::new(),
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
fn a_name_that_does_not_exist_or_is_not_answered_is_unresolvable() {
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
    // No answer at all: it gives up after its schedule, having asked again.
    let mut w = World::new(Script::Sink, &[]);
    w.peer.dns_script = Box::new(|_| Vec::new());
    let fake = Fake::new(1024);
    w.connect(&fake, Target::Name("silent.zoraya".into()), 80);
    assert!(!w.run_until(7_000_000, |_| fake.failed().is_some()));
    assert!(w.run_until(2_000_000, |_| fake.failed().is_some()));
    assert_eq!(fake.failed(), Some(reason::UNRESOLVABLE));
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
    assert_eq!(fake.failed(), Some(reason::UNRESOLVABLE));
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
                if let Some((to, bytes)) = service.resolve_done(&w.client, a, w.now) {
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
                if let Some((to, _)) = service.resolve_done(&w.client, a, w.now) {
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

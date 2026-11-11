//! Host tests of the stack over scripted peers: a DHCP server and an echo
//! responder made of a second smoltcp interface (INV-0229-3, INV-0221-2's
//! `netd` half), hostile frames, and the connection records (INV-0220-3's
//! enforcement half).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::string::String;
use std::vec;
use std::vec::Vec;

use smoltcp::iface::{Config as IfaceConfig, Interface, SocketSet};
use smoltcp::socket::udp;
use smoltcp::time::Instant;
use smoltcp::wire::{
    DhcpMessageType, DhcpPacket, DhcpRepr, EthernetAddress, HardwareAddress, IpAddress, IpCidr,
    IpEndpoint, Ipv4Address,
};

use crate::config::{self, Mode};
use crate::link::{Link, LinkDevice, MAX_FRAME};
use crate::stack::{Event, PingError, Source, Stack};
use crate::{Config, Connection, Request, Verdict};

const CLIENT_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const SERVER_MAC: [u8; 6] = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];
const SERVER_IP: [u8; 4] = [10, 0, 2, 2];
const OFFERED: [u8; 4] = [10, 0, 2, 15];

/// Two queues, one per direction, shared by the two ends.
#[derive(Default)]
struct Queues {
    to_client: VecDeque<Vec<u8>>,
    to_server: VecDeque<Vec<u8>>,
    /// Every frame the client sent, for the tests that mutate real ones.
    from_client: Vec<Vec<u8>>,
    from_server: Vec<Vec<u8>>,
}

struct End {
    queues: Rc<RefCell<Queues>>,
    client: bool,
    /// Free transmit slots; 0 stops the sender.
    room: usize,
    up: bool,
    /// Lies about received lengths.
    liar: bool,
}

impl End {
    fn pair() -> (End, End, Rc<RefCell<Queues>>) {
        let q = Rc::new(RefCell::new(Queues::default()));
        let end = |client| End {
            queues: q.clone(),
            client,
            room: usize::MAX,
            up: true,
            liar: false,
        };
        (end(true), end(false), q)
    }
}

impl Link for End {
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        let mut q = self.queues.borrow_mut();
        let frame = if self.client {
            q.to_client.pop_front()?
        } else {
            q.to_server.pop_front()?
        };
        if self.liar {
            return Some(buf.len() + 100);
        }
        let n = frame.len().min(buf.len());
        buf[..n].copy_from_slice(&frame[..n]);
        Some(n)
    }

    fn has_input(&self) -> bool {
        let q = self.queues.borrow();
        if self.client {
            !q.to_client.is_empty()
        } else {
            !q.to_server.is_empty()
        }
    }

    fn can_send(&self) -> bool {
        self.room > 0
    }

    fn send(&mut self, frame: &[u8]) -> bool {
        assert!(frame.len() <= MAX_FRAME);
        let mut q = self.queues.borrow_mut();
        if self.client {
            q.from_client.push(frame.to_vec());
            q.to_server.push_back(frame.to_vec());
        } else {
            q.from_server.push(frame.to_vec());
            q.to_client.push_back(frame.to_vec());
        }
        true
    }

    fn is_up(&self) -> bool {
        self.up
    }
}

/// The scripted peer: a DHCP server and, with the `echo-reply` feature, an
/// echo responder, at 10.0.2.2.
struct Server {
    device: LinkDevice<End>,
    iface: Interface,
    sockets: SocketSet<'static>,
    handle: smoltcp::iface::SocketHandle,
    dns: [u8; 4],
    lease: u32,
    /// Messages answered, by type.
    discovers: u32,
    requests: u32,
    /// Stops answering DHCP.
    mute: bool,
}

impl Server {
    fn new(end: End) -> Server {
        let mut device = LinkDevice::new(end);
        let mut config = IfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress(SERVER_MAC)));
        config.random_seed = 7;
        let mut iface = Interface::new(config, &mut device, Instant::from_micros(0));
        iface.update_ip_addrs(|a| {
            a.push(IpCidr::new(IpAddress::v4(10, 0, 2, 2), 24)).unwrap();
        });
        let mut sockets = SocketSet::new(Vec::new());
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 4 * 600]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 4 * 600]),
        );
        socket.bind(67).unwrap();
        let handle = sockets.add(socket);
        Server {
            device,
            iface,
            sockets,
            handle,
            dns: [10, 0, 2, 3],
            lease: 7200,
            discovers: 0,
            requests: 0,
            mute: false,
        }
    }

    fn poll(&mut self, now_us: i64) {
        let now = Instant::from_micros(now_us);
        self.iface.poll(now, &mut self.device, &mut self.sockets);
        let mut buf = [0u8; 600];
        loop {
            let socket = self.sockets.get_mut::<udp::Socket>(self.handle);
            let Ok((n, _)) = socket.recv_slice(&mut buf) else {
                break;
            };
            let Ok(packet) = DhcpPacket::new_checked(&buf[..n]) else {
                continue;
            };
            let Ok(request) = DhcpRepr::parse(&packet) else {
                continue;
            };
            let kind = match request.message_type {
                DhcpMessageType::Discover => {
                    self.discovers += 1;
                    DhcpMessageType::Offer
                }
                DhcpMessageType::Request => {
                    self.requests += 1;
                    DhcpMessageType::Ack
                }
                _ => continue,
            };
            if self.mute {
                continue;
            }
            let mut dns = heapless::Vec::new();
            let _ = dns.push(Ipv4Address::from(self.dns));
            let reply = DhcpRepr {
                message_type: kind,
                transaction_id: request.transaction_id,
                secs: 0,
                client_hardware_address: request.client_hardware_address,
                client_ip: if kind == DhcpMessageType::Ack {
                    request.client_ip
                } else {
                    Ipv4Address::UNSPECIFIED
                },
                your_ip: Ipv4Address::from(OFFERED),
                server_ip: Ipv4Address::from(SERVER_IP),
                router: Some(Ipv4Address::from(SERVER_IP)),
                subnet_mask: Some(Ipv4Address::new(255, 255, 255, 0)),
                relay_agent_ip: Ipv4Address::UNSPECIFIED,
                broadcast: false,
                requested_ip: None,
                client_identifier: None,
                server_identifier: Some(Ipv4Address::from(SERVER_IP)),
                parameter_request_list: None,
                dns_servers: Some(dns),
                max_size: None,
                lease_duration: Some(self.lease),
                renew_duration: None,
                rebind_duration: None,
                additional_options: &[],
            };
            let mut out = vec![0u8; reply.buffer_len()];
            reply
                .emit(&mut DhcpPacket::new_unchecked(&mut out[..]))
                .unwrap();
            let to = if request.client_ip == Ipv4Address::UNSPECIFIED {
                IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::BROADCAST), 68)
            } else {
                IpEndpoint::new(IpAddress::Ipv4(request.client_ip), 68)
            };
            let socket = self.sockets.get_mut::<udp::Socket>(self.handle);
            let _ = socket.send_slice(&out, to);
        }
        self.iface.poll(now, &mut self.device, &mut self.sockets);
    }
}

struct Net {
    client: Stack<End>,
    server: Server,
    queues: Rc<RefCell<Queues>>,
    now: i64,
}

impl Net {
    fn new(config: Config) -> Net {
        let (a, b, queues) = End::pair();
        Net {
            client: Stack::new(a, CLIENT_MAC, config, 0, 1).unwrap(),
            server: Server::new(b),
            queues,
            now: 0,
        }
    }

    /// One turn of both sides; the time jumps to what the client asked, at
    /// most one second.
    fn step(&mut self) {
        self.server.poll(self.now);
        let delay = self.client.poll(self.now);
        self.server.poll(self.now);
        let step = delay.unwrap_or(1_000_000).clamp(1_000, 1_000_000);
        self.now += step;
    }

    fn run_until(&mut self, limit_us: i64, done: impl Fn(&Net) -> bool) -> bool {
        let end = self.now + limit_us;
        while self.now < end {
            self.step();
            if done(self) {
                return true;
            }
        }
        false
    }
}

fn dhcp() -> Config {
    Config::default()
}

fn static_config() -> Config {
    Config {
        mode: Mode::Static {
            address: OFFERED,
            prefix: 24,
            gateway: SERVER_IP,
        },
        ..Config::default()
    }
}

#[test]
fn dhcp_gives_the_lease_and_status_shows_it() {
    let mut net = Net::new(dhcp());
    assert_eq!(net.client.status(0).source, Source::None);
    assert!(net.run_until(5_000_000, |n| n.client.address().is_some()));
    let status = net.client.status(net.now);
    assert_eq!(status.source, Source::Dhcp);
    assert_eq!((status.address, status.prefix), (OFFERED, 24));
    assert_eq!(status.gateway, SERVER_IP);
    assert_eq!(status.dns, vec![[10, 0, 2, 3]]);
    assert_eq!(status.dns_source, Source::Dhcp);
    assert!(status.link);
    assert_eq!(status.mac, CLIENT_MAC);
    let left = status.lease_left.unwrap();
    assert!((7190..=7200).contains(&left), "{left}");
    assert!(net.client.take_events().contains(&Event::Configured));
    assert!(status.counters.frames_in >= 2 && status.counters.frames_out >= 2);
    assert_eq!(net.server.discovers, 1);
}

#[test]
fn a_renewal_changes_the_status_without_any_configuration_write() {
    // INV-0229-3. The stack takes the settings as bytes handed to `config::read`
    // and holds no file: a renewal can only change what `status` returns.
    let mut net = Net::new(dhcp());
    assert!(net.run_until(5_000_000, |n| n.client.address().is_some()));
    let before = net.client.status(net.now);
    let _ = net.client.take_events();
    // The server now hands another DNS server; the client renews at half
    // the lease (3600 s).
    net.server.dns = [10, 0, 2, 4];
    let requests = net.server.requests;
    assert!(net.run_until(4_000 * 1_000_000, |n| n.server.requests > requests));
    assert!(net.run_until(5_000_000, |n| {
        n.client.status(n.now).dns == vec![[10, 0, 2, 4]]
    }));
    let after = net.client.status(net.now);
    assert_ne!(before, after);
    assert_eq!(after.address, OFFERED);
    let left = after.lease_left.unwrap();
    assert!(left > 7000, "the lease was renewed: {left}");
    assert!(net.client.take_events().contains(&Event::Configured));
    assert_eq!(net.server.discovers, 1, "a renewal is not a new discovery");
}

#[test]
fn the_client_asks_again_when_the_server_is_silent() {
    let mut net = Net::new(dhcp());
    net.server.mute = true;
    assert!(!net.run_until(30_000_000, |n| n.client.address().is_some()));
    assert!(net.server.discovers >= 2, "{}", net.server.discovers);
    net.server.mute = false;
    assert!(net.run_until(60_000_000, |n| n.client.address().is_some()));
}

#[test]
fn a_static_address_is_up_at_once_and_has_no_lease() {
    let mut net = Net::new(static_config());
    let status = net.client.status(0);
    assert_eq!(status.source, Source::Static);
    assert_eq!((status.address, status.prefix), (OFFERED, 24));
    assert_eq!(status.gateway, SERVER_IP);
    assert_eq!(status.lease_left, None);
    net.run_until(3_000_000, |_| false);
    assert_eq!(net.server.discovers, 0, "a static interface asks nobody");
}

#[test]
fn an_unconfigured_static_interface_has_no_address() {
    let net = Net::new(Config {
        mode: Mode::Unconfigured,
        ..Config::default()
    });
    let status = net.client.status(0);
    assert_eq!(status.source, Source::None);
    assert_eq!(status.address, [0; 4]);
    assert_eq!(net.client.address(), None);
}

#[test]
fn ping_gets_replies_from_the_peer() {
    let mut net = Net::new(static_config());
    net.client.ping(SERVER_IP, 4, 2_000_000, net.now).unwrap();
    assert_eq!(
        net.client.ping(SERVER_IP, 1, 1, net.now),
        Err(PingError::Busy)
    );
    assert!(net.run_until(10_000_000, |n| !n.client.ping_running()));
    let done = net
        .client
        .take_events()
        .into_iter()
        .find_map(|e| match e {
            Event::PingDone(r) => Some(r),
            _ => None,
        })
        .expect("the run ended");
    assert_eq!((done.sent, done.received), (4, 4));
    assert_eq!(done.target, SERVER_IP);
    assert!(done.rtt_min_us <= done.rtt_avg_us && done.rtt_avg_us <= done.rtt_max_us);
    // A second run is possible once the first is over.
    assert!(net.client.ping(SERVER_IP, 1, 1_000_000, net.now).is_ok());
}

#[test]
fn ping_to_nobody_ends_after_its_timeout_with_nothing_received() {
    let mut net = Net::new(static_config());
    net.client
        .ping([10, 0, 2, 99], 2, 1_000_000, net.now)
        .unwrap();
    assert!(net.run_until(10_000_000, |n| !n.client.ping_running()));
    let done = net
        .client
        .take_events()
        .into_iter()
        .find_map(|e| match e {
            Event::PingDone(r) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!((done.sent, done.received), (2, 0));
    assert_eq!(done.rtt_max_us, 0);
}

#[test]
fn ping_refuses_a_bad_target_and_a_missing_address() {
    let mut net = Net::new(static_config());
    for bad in [[0; 4], [255; 4], [127, 0, 0, 1], [224, 0, 0, 1], OFFERED] {
        assert_eq!(net.client.ping(bad, 1, 1, 0), Err(PingError::BadTarget));
    }
    assert_eq!(
        net.client.ping(SERVER_IP, 0, 1, 0),
        Err(PingError::BadTarget)
    );
    assert_eq!(
        net.client.ping(SERVER_IP, 65, 1, 0),
        Err(PingError::BadTarget)
    );
    let mut none = Net::new(dhcp());
    assert_eq!(
        none.client.ping(SERVER_IP, 1, 1, 0),
        Err(PingError::NoAddress)
    );
    let _ = &mut net;
}

#[test]
fn a_mac_that_is_not_unicast_is_refused() {
    let (a, _, _) = End::pair();
    let multicast = [0x01, 0, 0x5e, 0, 0, 1];
    assert!(Stack::new(a, multicast, Config::default(), 0, 1).is_err());
    let (a, _, _) = End::pair();
    assert!(Stack::new(a, [0; 6], Config::default(), 0, 1).is_err());
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

fn inject(net: &mut Net, frame: Vec<u8>) {
    net.queues.borrow_mut().to_client.push_back(frame);
}

/// INV-0221-2, `netd`'s half: no frame panics the stack, and the stack still
/// works after the hostile ones.
#[test]
fn hostile_frames_never_panic_and_the_stack_goes_on() {
    for config in [dhcp(), static_config()] {
        let mut net = Net::new(config);
        let mut rng = Rng(0x1234_5678_9abc_def1);
        // Real frames of both directions to mutate: the whole DHCP exchange
        // and some echoes.
        net.client.ping(SERVER_IP, 2, 500_000, 0).ok();
        net.run_until(3_000_000, |_| false);
        let real: Vec<Vec<u8>> = {
            let q = net.queues.borrow();
            q.from_server
                .iter()
                .chain(q.from_client.iter())
                .cloned()
                .collect()
        };
        assert!(real.len() >= 3, "{}", real.len());
        for round in 0..6000u32 {
            let frame = match round % 4 {
                // Pure noise, any length from 0 to the maximum and beyond.
                0 => {
                    let n = (rng.next() % 1700) as usize;
                    (0..n).map(|_| rng.next() as u8).collect()
                }
                // A real frame with bytes changed.
                1 | 2 => {
                    let mut f = real[(rng.next() as usize) % real.len()].clone();
                    for _ in 0..(1 + rng.next() % 6) {
                        let at = (rng.next() as usize) % f.len();
                        f[at] = rng.next() as u8;
                    }
                    f
                }
                // A real frame cut short or lengthened.
                _ => {
                    let mut f = real[(rng.next() as usize) % real.len()].clone();
                    let n = (rng.next() as usize) % (f.len() + 40);
                    f.resize(n, (rng.next() & 0xff) as u8);
                    f
                }
            };
            inject(&mut net, frame);
            if round % 8 == 0 {
                net.step();
            }
        }
        net.run_until(5_000_000, |_| false);
        // Still serving: a static client pings; a DHCP one is not stuck.
        let status = net.client.status(net.now);
        assert!(status.counters.frames_in > 1000);
        let mut fresh = Net::new(static_config());
        fresh.client.ping(SERVER_IP, 1, 1_000_000, 0).unwrap();
        assert!(fresh.run_until(5_000_000, |n| !n.client.ping_running()));
    }
}

#[test]
fn a_link_that_lies_about_a_length_loses_the_frame_not_the_stack() {
    let mut net = Net::new(static_config());
    net.client.link().liar = true;
    inject(&mut net, vec![0xff; 60]);
    net.step();
    let dropped = net.client.status(net.now).counters.dropped;
    assert_eq!(dropped, 1);
    net.client.link().liar = false;
    net.client.ping(SERVER_IP, 1, 1_000_000, net.now).unwrap();
    assert!(net.run_until(5_000_000, |n| !n.client.ping_running()));
}

#[test]
fn a_flood_is_served_a_bounded_burst_at_a_time() {
    let mut net = Net::new(static_config());
    for _ in 0..1000 {
        inject(&mut net, vec![0xaa; 100]);
    }
    let delay = net.client.poll(0);
    let taken = net.client.status(0).counters.frames_in;
    assert_eq!(taken, crate::stack::BURST as u64);
    assert_eq!(delay, Some(0), "it asks to be called again at once");
    while !net.queues.borrow().to_client.is_empty() {
        net.client.poll(0);
    }
}

#[test]
fn no_free_transmit_slot_leaves_received_frames_queued() {
    let mut net = Net::new(static_config());
    net.client.link().room = 0;
    inject(&mut net, vec![0xaa; 60]);
    net.client.poll(0);
    assert_eq!(net.client.status(0).counters.frames_in, 0);
    assert_eq!(net.queues.borrow().to_client.len(), 1);
    net.client.link().room = 10;
    net.client.poll(0);
    assert_eq!(net.client.status(0).counters.frames_in, 1);
}

// The connection records.

#[test]
fn the_root_connection_may_do_everything() {
    let root = Connection::root();
    assert_eq!(root.decide(Request::Status), Verdict::Allowed);
    assert_eq!(
        root.decide(Request::Ping { address: SERVER_IP }),
        Verdict::Allowed
    );
    let narrow = Connection::root();
    let bytes = zoraya_abi::net::Scope::EMPTY.as_bytes();
    assert_eq!(
        root.decide(Request::Narrow {
            rights: 4,
            scope: bytes
        }),
        Verdict::Allowed
    );
    assert_eq!(narrow.narrowed(4, bytes).unwrap().rights(), 4);
}

#[test]
fn a_status_only_connection_may_only_ask_status() {
    use zoraya_abi::net::rights;
    let root = Connection::root();
    let status = root
        .narrowed(rights::STATUS, zoraya_abi::net::Scope::EMPTY.as_bytes())
        .unwrap();
    assert_eq!(status.decide(Request::Status), Verdict::Allowed);
    assert_eq!(
        status.decide(Request::Ping { address: SERVER_IP }),
        Verdict::NotGranted
    );
    let bytes = zoraya_abi::net::Scope::EMPTY.as_bytes();
    // No narrow right: even to nothing.
    assert_eq!(
        status.decide(Request::Narrow {
            rights: 0,
            scope: bytes
        }),
        Verdict::NotGranted
    );
    assert!(status.narrowed(0, bytes).is_none());
}

#[test]
fn narrow_never_gives_a_right_or_a_scope_the_caller_lacked() {
    use zoraya_abi::net::{Ports, Protocols, ReachRule, Target, encode, rights};
    let one = |target: Target<'_>| {
        let mut out = vec![0u8; 256];
        let rule = ReachRule {
            target,
            ports: Ports::ANY,
            protocols: Protocols::BOTH,
        };
        let n = encode(&[rule], &[], &mut out).unwrap();
        out.truncate(n);
        out
    };
    let parent = Connection::new(
        rights::REACH | rights::NARROW | rights::STATUS,
        &one(Target::Prefix([10, 0, 2, 0], 24)),
    )
    .unwrap();
    // Inside: an address of the prefix, fewer rights.
    let inside = one(Target::Address(SERVER_IP));
    let child = parent.narrowed(rights::REACH, &inside).unwrap();
    assert_eq!(
        child.decide(Request::Ping { address: SERVER_IP }),
        Verdict::Allowed
    );
    assert_eq!(
        child.decide(Request::Ping {
            address: [10, 0, 2, 9]
        }),
        Verdict::NotGranted
    );
    assert_eq!(child.decide(Request::Status), Verdict::NotGranted);
    // Outside: a wider scope, an address outside, a right the parent lacks.
    for (r, scope) in [
        (rights::REACH, one(Target::Any)),
        (rights::REACH, one(Target::Address([192, 168, 0, 1]))),
        (rights::REACH | rights::LISTEN, inside.clone()),
    ] {
        assert_eq!(
            parent.decide(Request::Narrow {
                rights: r,
                scope: &scope
            }),
            Verdict::NotGranted
        );
        assert!(parent.narrowed(r, &scope).is_none());
    }
    // Malformed: a scope that does not decode, an unknown right bit.
    assert_eq!(
        parent.decide(Request::Narrow {
            rights: 1,
            scope: &[1, 9, 9, 9]
        }),
        Verdict::Invalid
    );
    assert_eq!(
        parent.decide(Request::Narrow {
            rights: 1 << 9,
            scope: &inside
        }),
        Verdict::Invalid
    );
    assert!(Connection::new(1 << 9, &inside).is_none());
    assert!(Connection::new(1, &[0]).is_none());
    // The ping to an address needs a rule for it: a name does not cover it.
    let by_name = Connection::new(rights::REACH, &one(Target::Name("gateway"))).unwrap();
    assert_eq!(
        by_name.decide(Request::Ping { address: SERVER_IP }),
        Verdict::NotGranted
    );
}

// The settings (ADR-0229, INV-0229-1).

#[test]
fn the_defaults_are_dhcp_and_zoraya() {
    let (c, problems) = config::read(b"");
    assert_eq!(c.mode, Mode::Dhcp);
    assert_eq!(c.hostname, "zoraya");
    assert!(c.dns.is_empty() && c.hosts.is_empty());
    assert!(problems.is_empty());
}

#[test]
fn a_good_static_file_is_read() {
    let file = b"net.address = static\nnet.ip = 10.0.2.15/24\nnet.gateway = 10.0.2.2\nnet.dns = 1.1.1.1 10.0.2.3\nnet.hosts = build=10.0.2.100\nnet.hostname = lab\n";
    let (c, problems) = config::read(file);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        c.mode,
        Mode::Static {
            address: [10, 0, 2, 15],
            prefix: 24,
            gateway: [10, 0, 2, 2]
        }
    );
    assert_eq!(c.dns, vec![[1, 1, 1, 1], [10, 0, 2, 3]]);
    assert_eq!(c.hosts, vec![(String::from("build"), [10, 0, 2, 100])]);
    assert_eq!(c.hostname, "lab");
}

#[test]
fn a_bad_net_line_is_reported_once_and_the_default_runs() {
    // Each bad line gives one problem and its key keeps the default.
    for (file, key) in [
        ("net.gateway = 10.0.2\n", "net.gateway"),
        ("net.ip = 10.0.2.15\n", "net.ip"),
        ("net.dns = 1.1.1\n", "net.dns"),
        ("net.address = auto\n", "net.address"),
    ] {
        let (c, problems) = config::read(file.as_bytes());
        assert_eq!(problems.len(), 1, "{file}: {problems:?}");
        assert!(problems[0].contains("line 1"), "{problems:?}");
        assert!(problems[0].starts_with("netd: "), "{problems:?}");
        assert_eq!(c.mode, Mode::Dhcp, "{key}");
        assert!(c.dns.is_empty());
    }
}

#[test]
fn static_without_what_it_needs_has_no_address_and_says_why() {
    let (c, problems) = config::read(b"net.address = static\n");
    assert_eq!(c.mode, Mode::Unconfigured);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("net.ip is not set"), "{problems:?}");
    assert!(
        problems[0].contains("net.gateway is not set"),
        "{problems:?}"
    );
    for file in [
        "net.address = static\nnet.ip = 10.0.2.15/24\nnet.gateway = 10.0.9.2\n",
        "net.address = static\nnet.ip = 10.0.2.15/24\nnet.gateway = 10.0.2.15\n",
        "net.address = static\nnet.ip = 0.0.0.0/24\nnet.gateway = 10.0.2.2\n",
        "net.address = static\nnet.ip = 10.0.2.15/31\nnet.gateway = 10.0.2.14\n",
        "net.address = static\nnet.ip = 224.0.0.5/24\nnet.gateway = 224.0.0.1\n",
    ] {
        let (c, p) = config::read(file.as_bytes());
        assert_eq!(c.mode, Mode::Unconfigured, "{file}");
        assert_eq!(p.len(), 1, "{file}: {p:?}");
    }
}

#[test]
fn hosts_and_hostname_are_checked() {
    let (c, p) = config::read(
        b"net.hosts = good=10.0.2.100 Bad=10.0.2.101 nokey 'x=1.2.3' a=1.1.1.1 a=2.2.2.2\nnet.hostname = Not.A.Label\n",
    );
    assert_eq!(
        c.hosts,
        vec![
            (String::from("good"), [10, 0, 2, 100]),
            (String::from("a"), [1, 1, 1, 1])
        ]
    );
    assert_eq!(c.hostname, "zoraya");
    assert_eq!(p.len(), 5, "{p:?}");
}

/// The settings reader is total: random bytes give a config and problems.
#[test]
fn random_settings_files_never_panic() {
    let mut rng = Rng(99);
    for _ in 0..2000 {
        let n = (rng.next() % 300) as usize;
        let words = [
            "net.address",
            "net.ip",
            "net.gateway",
            "net.dns",
            "net.hosts",
            "net.hostname",
            " = ",
            "static",
            "dhcp",
            "10.0.2.15/24",
            "10.0.2.2",
            "a=1.2.3.4",
            "\n",
            "'",
            "\"",
            "#",
            "\t",
            "net.",
        ];
        let mut bytes = Vec::new();
        for _ in 0..(n / 6) {
            let w = words[(rng.next() as usize) % words.len()];
            bytes.extend_from_slice(w.as_bytes());
            if rng.next().is_multiple_of(5) {
                bytes.push(rng.next() as u8);
            }
        }
        let (_, _) = config::read(&bytes);
    }
}

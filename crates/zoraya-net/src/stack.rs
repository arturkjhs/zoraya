//! The stack: smoltcp's interface over a [`Link`], the DHCP client, the echo
//! socket and `status` (ADR-0221 item 3). No time of its own: the caller
//! passes the monotonic time in microseconds to every call, and asks
//! [`Stack::poll`] when to come back.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketHandle, SocketSet};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::socket::dhcpv4::{self, Event as DhcpEvent};
use smoltcp::socket::icmp::{self, Endpoint as IcmpEndpoint};
use smoltcp::time::Instant;
use smoltcp::wire::{
    DhcpOption, DhcpRepr, EthernetAddress, HardwareAddress, Icmpv4Packet, Icmpv4Repr, IpAddress,
    IpCidr, Ipv4Address, Ipv4Cidr,
};

use crate::config::{Config, Mode, is_unicast};
use crate::link::{Counters, Link, LinkDevice, MAX_FRAME};

/// The identifier of our echo requests.
const PING_IDENT: u16 = 0x5a50;
/// The payload of an echo request: 32 bytes, as `ping` sends.
/// The most requests of one run: `zoraya_proto_net::MAX_PINGS`, which the
/// reply's `rtts` list holds (INV-0330-1).
const MAX_PING_COUNT: u32 = zoraya_proto_net::MAX_PINGS;
const PING_DATA: [u8; 32] = *b"zoraya echo request 0123456789ab";
/// Milliseconds between two echo requests of one run.
pub const PING_INTERVAL_US: i64 = 200_000;
/// DHCP option 12, the host name.
const OPTION_HOSTNAME: u8 = 12;
/// The most frames one `poll` moves each way, so that a flood cannot hold
/// the single loop of `netd` (the rest waits for the next turn).
pub const BURST: usize = 64;
/// The shortest delay `poll` asks for on account of a run, in microseconds.
pub const MIN_DELAY_US: i64 = 1_000;

/// Why a stack could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewError {
    /// The MAC address is not a unicast address.
    BadMac,
}

/// Where the interface's address came from (`zoraya_proto_net::source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    None,
    Dhcp,
    Static,
}

/// What the DHCP client holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub address: [u8; 4],
    pub prefix: u8,
    pub router: Option<[u8; 4]>,
    pub dns: Vec<[u8; 4]>,
    /// The lease time the server gave, in seconds.
    pub duration: Option<u32>,
    /// When it was (last) granted, in microseconds.
    pub since_us: i64,
}

/// What `netd` reports (`status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub link: bool,
    pub mac: [u8; 6],
    pub source: Source,
    pub address: [u8; 4],
    pub prefix: u8,
    pub gateway: [u8; 4],
    pub dns: Vec<[u8; 4]>,
    pub dns_source: Source,
    /// Seconds left of the DHCP lease.
    pub lease_left: Option<u32>,
    pub counters: Counters,
}

/// A change `netd` should print or act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Configured,
    Deconfigured,
    PingDone(PingResult),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingError {
    /// A run is going.
    Busy,
    /// The address is not a unicast address, or ours.
    BadTarget,
    /// The interface has no address.
    NoAddress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingResult {
    pub target: [u8; 4],
    pub sent: u32,
    pub received: u32,
    pub rtt_min_us: u32,
    pub rtt_avg_us: u32,
    pub rtt_max_us: u32,
    /// The round trip of each request, in order, `u32::MAX` for none.
    pub rtts: Vec<u32>,
}

struct PingRun {
    target: [u8; 4],
    count: u32,
    sent: Vec<i64>,
    rtt: Vec<Option<u32>>,
    next_at: i64,
    /// When the run ends whatever came back: fixed when it starts, the last
    /// request's turn plus the timeout.
    deadline: i64,
    /// This run's identifier: a late reply of an earlier run is not ours.
    ident: u16,
}

pub struct Stack<L: Link> {
    pub(crate) device: LinkDevice<L>,
    pub(crate) iface: Interface,
    pub(crate) sockets: SocketSet<'static>,
    dhcp: Option<SocketHandle>,
    icmp: SocketHandle,
    mac: [u8; 6],
    config: Config,
    lease: Option<Lease>,
    static_up: bool,
    ping: Option<PingRun>,
    /// Runs started, for their identifiers.
    runs: u16,
    events: Vec<Event>,
    /// TCP connections and the resolver (`tcp`, `dns`).
    pub(crate) extra: crate::tcp::Extra,
}

/// A fresh echo socket. Each run gets one, so that a request that could not
/// leave (no route, no ARP answer) does not stay at the head of the transmit
/// buffer and block the next run.
fn new_icmp_socket() -> icmp::Socket<'static> {
    icmp::Socket::new(
        icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY; 4], vec![0; 4 * 256]),
        icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY; 4], vec![0; 4 * 256]),
    )
}

fn octets(a: Ipv4Address) -> [u8; 4] {
    a.octets()
}

impl<L: Link> Stack<L> {
    /// A stack over `link` for the interface `mac`. `seed` is smoltcp's
    /// random seed (initial sequence numbers and ports; not a secret).
    pub fn new(
        link: L,
        mac: [u8; 6],
        config: Config,
        now_us: i64,
        seed: u64,
    ) -> Result<Stack<L>, NewError> {
        // The interface panics on a non-unicast address; the MAC came from
        // the driver, which checked it, and we do not rely on that.
        if mac[0] & 1 != 0 || mac == [0; 6] {
            return Err(NewError::BadMac);
        }
        let mut device = LinkDevice::new(link);
        let mut iface_config = IfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
        iface_config.random_seed = seed;
        let iface = Interface::new(iface_config, &mut device, Instant::from_micros(now_us));
        let mut sockets = SocketSet::new(Vec::new());
        let icmp = sockets.add(new_icmp_socket());
        let mut stack = Stack {
            device,
            iface,
            sockets,
            dhcp: None,
            icmp,
            mac,
            config,
            lease: None,
            static_up: false,
            ping: None,
            runs: 0,
            events: Vec::new(),
            extra: crate::tcp::Extra::default(),
        };
        stack.seed_ports(seed);
        match stack.config.mode.clone() {
            Mode::Dhcp => {
                let mut socket = dhcpv4::Socket::new();
                // The server's packet is kept to read the lease time from.
                let buffer: &'static mut [u8] = Box::leak(vec![0u8; MAX_FRAME].into_boxed_slice());
                socket.set_receive_packet_buffer(buffer);
                let name: &'static [u8] = Box::leak(
                    stack
                        .config
                        .hostname
                        .clone()
                        .into_bytes()
                        .into_boxed_slice(),
                );
                let options: &'static [DhcpOption<'static>] = Box::leak(Box::new([DhcpOption {
                    kind: OPTION_HOSTNAME,
                    data: name,
                }]));
                socket.set_outgoing_options(options);
                stack.dhcp = Some(stack.sockets.add(socket));
            }
            Mode::Static {
                address,
                prefix,
                gateway,
            } => {
                stack.apply(address, prefix, Some(gateway));
                stack.static_up = true;
            }
            Mode::Unconfigured => {}
        }
        Ok(stack)
    }

    fn apply(&mut self, address: [u8; 4], prefix: u8, router: Option<[u8; 4]>) {
        let cidr = Ipv4Cidr::new(Ipv4Address::from(address), prefix.min(32));
        self.iface.update_ip_addrs(|addrs| {
            addrs.clear();
            let _ = addrs.push(IpCidr::Ipv4(cidr));
        });
        self.iface.routes_mut().remove_default_ipv4_route();
        if let Some(r) = router {
            let _ = self
                .iface
                .routes_mut()
                .add_default_ipv4_route(Ipv4Address::from(r));
        }
    }

    fn clear(&mut self) {
        self.iface.update_ip_addrs(|addrs| addrs.clear());
        self.iface.routes_mut().remove_default_ipv4_route();
    }

    pub fn link(&mut self) -> &mut L {
        &mut self.device.link
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// Our address, if the interface has one.
    pub fn address(&self) -> Option<[u8; 4]> {
        self.iface.ipv4_addr().map(octets)
    }

    /// Moves frames and timers up to `now_us`. Returns when to call again
    /// (microseconds from `now_us`), or `None` to wait for a frame or a
    /// request.
    pub fn poll(&mut self, now_us: i64) -> Option<i64> {
        let now = Instant::from_micros(now_us);
        // Bounded work per call: at most BURST frames in and BURST out, then
        // the caller's loop serves its other ports before coming back
        // (a flood cannot hold netd's single thread).
        self.iface.poll_maintenance(now);
        for _ in 0..BURST {
            if self
                .iface
                .poll_ingress_single(now, &mut self.device, &mut self.sockets)
                == smoltcp::iface::PollIngressSingleResult::None
            {
                break;
            }
        }
        self.dhcp_events(now_us);
        self.ping_step(now_us);
        for _ in 0..BURST {
            if self
                .iface
                .poll_egress(now, &mut self.device, &mut self.sockets)
                == smoltcp::iface::PollResult::None
            {
                break;
            }
        }
        if self.device.link.has_input() && self.device.link.can_send() {
            // More frames than one turn takes, and room to answer them: come
            // back at once. With no room the loop waits for the channel to
            // become writable (or the timers) instead of spinning.
            return Some(0);
        }
        let by_stack = self
            .iface
            .poll_delay(now, &self.sockets)
            .map(|d| d.total_micros() as i64);
        // The next thing a run waits for: its next request, or its deadline.
        // Never zero: a run whose request cannot leave must not spin.
        let by_ping = self.ping.as_ref().map(|p| {
            let at = if (p.sent.len() as u32) < p.count {
                p.next_at.min(p.deadline)
            } else {
                p.deadline
            };
            (at - now_us).max(MIN_DELAY_US)
        });
        match (by_stack, by_ping) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn dhcp_events(&mut self, now_us: i64) {
        let Some(handle) = self.dhcp else { return };
        loop {
            let event = match self.sockets.get_mut::<dhcpv4::Socket>(handle).poll() {
                Some(DhcpEvent::Configured(c)) => {
                    let duration = c
                        .packet
                        .as_ref()
                        .and_then(|p| DhcpRepr::parse(p).ok())
                        .and_then(|r| r.lease_duration);
                    Some(Some(Lease {
                        address: octets(c.address.address()),
                        prefix: c.address.prefix_len(),
                        router: c.router.map(octets),
                        dns: c.dns_servers.iter().copied().map(octets).collect(),
                        duration,
                        since_us: now_us,
                    }))
                }
                Some(DhcpEvent::Deconfigured) => Some(None),
                None => None,
            };
            match event {
                Some(Some(lease)) => {
                    if is_unicast(lease.address) && lease.prefix <= 30 {
                        self.apply(
                            lease.address,
                            lease.prefix,
                            lease.router.filter(|r| is_unicast(*r)),
                        );
                        self.lease = Some(lease);
                        self.events.push(Event::Configured);
                    }
                }
                Some(None) => {
                    // smoltcp reports "no configuration" once at start.
                    if self.lease.is_some() {
                        self.clear();
                        self.lease = None;
                        self.events.push(Event::Deconfigured);
                    }
                }
                None => break,
            }
        }
    }

    /// Starts `count` echo requests to `target`.
    pub fn ping(
        &mut self,
        target: [u8; 4],
        count: u32,
        timeout_us: i64,
        now_us: i64,
    ) -> Result<(), PingError> {
        if self.ping.is_some() {
            return Err(PingError::Busy);
        }
        let own = self.address();
        if own.is_none() {
            return Err(PingError::NoAddress);
        }
        if !is_unicast(target) || own == Some(target) || count == 0 || count > MAX_PING_COUNT {
            return Err(PingError::BadTarget);
        }
        self.runs = self.runs.wrapping_add(1);
        let ident = PING_IDENT.wrapping_add(self.runs);
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        if socket.bind(IcmpEndpoint::Ident(ident)).is_err() {
            return Err(PingError::Busy);
        }
        let timeout_us = timeout_us.max(0);
        self.ping = Some(PingRun {
            target,
            count,
            sent: Vec::new(),
            rtt: Vec::new(),
            next_at: now_us,
            deadline: now_us
                .saturating_add(i64::from(count - 1) * PING_INTERVAL_US)
                .saturating_add(timeout_us),
            ident,
        });
        Ok(())
    }

    pub fn ping_running(&self) -> bool {
        self.ping.is_some()
    }

    fn ping_step(&mut self, now_us: i64) {
        let Some(run) = self.ping.as_mut() else {
            return;
        };
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        // Replies first.
        let mut buf = [0u8; 256];
        loop {
            let (n, from) = match socket.recv_slice(&mut buf) {
                Ok(r) => r,
                // The packet was dequeued and was too long for us: not ours.
                Err(icmp::RecvError::Truncated) => continue,
                Err(_) => break,
            };
            let IpAddress::Ipv4(from) = from;
            let caps = ChecksumCapabilities::default();
            let Ok(packet) = Icmpv4Packet::new_checked(&buf[..n]) else {
                continue;
            };
            let Ok(Icmpv4Repr::EchoReply { ident, seq_no, .. }) = Icmpv4Repr::parse(&packet, &caps)
            else {
                continue;
            };
            let seq = usize::from(seq_no);
            if ident != run.ident || octets(from) != run.target || seq >= run.sent.len() {
                continue;
            }
            if run.rtt[seq].is_none() {
                let rtt = (now_us - run.sent[seq]).max(0);
                run.rtt[seq] = Some(u32::try_from(rtt).unwrap_or(u32::MAX));
            }
        }
        // Then the next request, if it is due. One that cannot be queued
        // (the socket's four slots are held by requests that could not leave)
        // takes its turn and is lost: the run's end does not wait for it.
        if (run.sent.len() as u32) < run.count && now_us >= run.next_at {
            let repr = Icmpv4Repr::EchoRequest {
                ident: run.ident,
                seq_no: run.sent.len() as u16,
                data: &PING_DATA,
            };
            let mut bytes = [0u8; 8 + PING_DATA.len()];
            let mut packet = Icmpv4Packet::new_unchecked(&mut bytes[..]);
            repr.emit(&mut packet, &ChecksumCapabilities::default());
            if socket.can_send() {
                let _ = socket.send_slice(
                    &bytes[..repr.buffer_len()],
                    IpAddress::Ipv4(Ipv4Address::from(run.target)),
                );
            }
            run.sent.push(now_us);
            run.rtt.push(None);
            run.next_at = now_us.saturating_add(PING_INTERVAL_US);
        }
        let got = run.rtt.iter().flatten().count() as u32;
        let all_sent = run.sent.len() as u32 == run.count;
        let finished = got == run.count || (all_sent && now_us >= run.deadline);
        // Past the deadline nothing more is waited for, sent or not.
        let expired = now_us >= run.deadline;
        if finished || expired {
            let rtts: Vec<u32> = run.rtt.iter().flatten().copied().collect();
            let sum: u64 = rtts.iter().map(|r| u64::from(*r)).sum();
            let result = PingResult {
                target: run.target,
                sent: run.count,
                received: got,
                rtt_min_us: rtts.iter().copied().min().unwrap_or(0),
                rtt_avg_us: (sum / rtts.len().max(1) as u64) as u32,
                rtt_max_us: rtts.iter().copied().max().unwrap_or(0),
                rtts: (0..run.count as usize)
                    .map(|i| run.rtt.get(i).copied().flatten().unwrap_or(u32::MAX))
                    .collect(),
            };
            self.ping = None;
            // Whatever is still queued in the socket goes with it.
            self.sockets.remove(self.icmp);
            self.icmp = self.sockets.add(new_icmp_socket());
            self.events.push(Event::PingDone(result));
        }
    }

    /// The changes since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        core::mem::take(&mut self.events)
    }

    pub fn status(&self, now_us: i64) -> Status {
        let (source, prefix) = match (&self.lease, self.static_up) {
            (Some(l), _) => (Source::Dhcp, l.prefix),
            (None, true) => match self.config.mode {
                Mode::Static { prefix, .. } => (Source::Static, prefix),
                _ => (Source::None, 0),
            },
            _ => (Source::None, 0),
        };
        let address = if source == Source::None {
            [0; 4]
        } else {
            self.address().unwrap_or([0; 4])
        };
        let gateway = match (&self.lease, &self.config.mode) {
            (Some(l), _) => l.router.unwrap_or([0; 4]),
            (None, Mode::Static { gateway, .. }) if self.static_up => *gateway,
            _ => [0; 4],
        };
        // `net.dns` wins over the lease's servers (ADR-0229).
        let (dns, dns_source) = if !self.config.dns.is_empty() {
            (self.config.dns.clone(), Source::Static)
        } else if let Some(l) = &self.lease {
            (l.dns.clone(), Source::Dhcp)
        } else {
            (Vec::new(), Source::None)
        };
        let lease_left = self.lease.as_ref().and_then(|l| {
            let duration = i64::from(l.duration?);
            let left = duration - (now_us - l.since_us) / 1_000_000;
            Some(left.clamp(0, i64::from(u32::MAX - 1)) as u32)
        });
        Status {
            link: self.device.link.is_up(),
            mac: self.mac,
            source,
            address,
            prefix,
            gateway,
            dns,
            dns_source,
            lease_left,
            counters: self.device.counters,
        }
    }
}

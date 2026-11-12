//! `netd`'s side of everything that comes from outside the machine
//! (ADR-0221, INV-0221-2, INV-0220-3, INV-0229-1), on arbitrary bytes. The
//! first byte picks what the rest is:
//!
//! 0. a scope for `Scope::decode` (a decoded one must re-encode to the same
//!    bytes and lie within itself);
//! 1. frames for the stacks, length-prefixed (a static and a DHCP stack);
//! 2. a DHCP reply: first the bytes wrapped as they are, then an answer to the
//!    client's own last frame (its transaction id and hardware address) with
//!    the input as options, which reaches the lease handling;
//! 3. messages from the driver for `Nic::pump`, then whatever it queued;
//! 4. requests for `Service::handle` on a root connection and on a
//!    connection narrowed from it with an account (ordinal 7, then the
//!    requests go to both, and `narrow` on the child), then `resolve_done`
//!    with fuzzed tokens (ones in the lookups' range among them);
//! 5. a ping answered by echo replies with the run's identifier and the input
//!    as payload;
//! 6. a settings file for `config::read`.
//!
//! The stacks live for the whole process (state carries over between inputs).
//! Nothing may panic.

#![no_main]

use std::collections::VecDeque;

use libfuzzer_sys::fuzz_target;
use zoraya_abi::net::{ListenRule, Ports, Protocols, ReachRule, Scope, Target, encode};
use zoraya_net::config;
use zoraya_net::conn::LOOKUP_BASE;
use zoraya_net::dns::{Found, Resolved};
use zoraya_net::nic::{Nic, Read, SlotMemory, Wire, Write};
use zoraya_net::service::{Action, Service};
use zoraya_net::{Config, Connection, Link, Mode, Stack};

const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

#[derive(Default)]
struct Frames {
    incoming: VecDeque<Vec<u8>>,
    /// What the stack sent since the last look, to answer its own frames.
    sent: Vec<Vec<u8>>,
}

impl Link for Frames {
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        let f = self.incoming.pop_front()?;
        let n = f.len().min(buf.len());
        buf[..n].copy_from_slice(&f[..n]);
        Some(n)
    }

    fn has_input(&self) -> bool {
        !self.incoming.is_empty()
    }

    fn can_send(&self) -> bool {
        true
    }

    fn send(&mut self, frame: &[u8]) -> bool {
        if self.sent.len() < 16 {
            self.sent.push(frame.to_vec());
        }
        true
    }

    fn is_up(&self) -> bool {
        true
    }
}

/// The stacks live for the whole process: a DHCP stack keeps a few hundred
/// bytes it leaks by design (once per `netd`), so one per input would grow
/// without bound. Their state carries over from input to input.
struct Rig {
    dhcp: Stack<Frames>,
    fixed: Stack<Frames>,
    now: i64,
}

thread_local! {
    static RIG: std::cell::RefCell<Option<Rig>> = const { std::cell::RefCell::new(None) };
}

fn with_rig(f: impl FnOnce(&mut Rig)) {
    RIG.with(|cell| {
        let mut rig = cell.borrow_mut();
        let rig = rig.get_or_insert_with(|| Rig {
            dhcp: Stack::new(Frames::default(), MAC, Config::default(), 0, 1).unwrap(),
            fixed: Stack::new(Frames::default(), MAC, static_config(), 0, 1).unwrap(),
            now: 0,
        });
        f(rig);
    });
}

/// The UDP payload of the stack's last DHCP frame (to port 67), if any.
fn last_dhcp(sent: &[Vec<u8>]) -> Option<&[u8]> {
    sent.iter().rev().find_map(|f| {
        let udp = f.get(34..)?;
        (f.get(12..14)? == [8, 0] && udp.get(2..4)? == [0, 67]).then(|| udp.get(8..))?
    })
}

/// A reply for the client's own frame: a BOOTP header with its transaction id
/// and hardware address, the magic cookie, and `options` from the input.
fn dhcp_reply(request: &[u8], options: &[u8]) -> Vec<u8> {
    let mut p = vec![0u8; 240];
    p[0] = 2;
    p[1] = 1;
    p[2] = 6;
    if let (Some(xid), Some(chaddr)) = (request.get(4..8), request.get(28..34)) {
        p[4..8].copy_from_slice(xid);
        p[28..34].copy_from_slice(chaddr);
    }
    p[16..20].copy_from_slice(&[10, 0, 2, 15]);
    p[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
    // The message type comes from the input's first byte: offer, ack, nak...
    let kind = 1 + options.first().copied().unwrap_or(2) % 8;
    p.extend_from_slice(&[53, 1, kind]);
    p.extend_from_slice(&options[options.len().min(1)..]);
    p.push(255);
    p
}

/// An ICMP echo reply from 10.0.2.2 to the stack, with the given identifier
/// and `payload` after the ICMP header.
fn echo_reply(ident: u16, payload: &[u8]) -> Vec<u8> {
    let payload = &payload[..payload.len().min(200)];
    let icmp_len = 8 + payload.len();
    let ip_len = 20 + icmp_len;
    let mut f = vec![
        0x52, 0x54, 0, 0x12, 0x34, 0x56, 0x52, 0x55, 0x0a, 0, 2, 2, 8, 0,
    ];
    let mut ip = vec![
        0x45,
        0,
        (ip_len >> 8) as u8,
        ip_len as u8,
        0,
        0,
        0,
        0,
        64,
        1,
        0,
        0,
    ];
    ip.extend_from_slice(&[10, 0, 2, 2, 10, 0, 2, 15]);
    let sum = checksum(&ip);
    ip[10..12].copy_from_slice(&sum.to_be_bytes());
    f.extend_from_slice(&ip);
    let mut icmp = vec![
        0,
        0,
        0,
        0,
        (ident >> 8) as u8,
        ident as u8,
        0,
        payload.first().copied().unwrap_or(0) % 4,
    ];
    icmp.extend_from_slice(payload);
    let sum = checksum(&icmp);
    icmp[2..4].copy_from_slice(&sum.to_be_bytes());
    f.extend_from_slice(&icmp);
    f
}

fn static_config() -> Config {
    Config {
        mode: Mode::Static {
            address: [10, 0, 2, 15],
            prefix: 24,
            gateway: [10, 0, 2, 2],
        },
        ..Config::default()
    }
}

/// Frames cut out of `data`: a length byte, then that many bytes.
fn frames(mut data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Some((&n, rest)) = data.split_first() {
        let n = (usize::from(n) * 7).min(rest.len());
        out.push(rest[..n].to_vec());
        data = &rest[n..];
        if out.len() >= 256 {
            break;
        }
    }
    out
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in bytes.chunks(2) {
        let word = u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]);
        sum += u32::from(word);
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Ethernet, IPv4 and UDP (checksum 0: none) around `payload`, from the DHCP
/// server's port to the client's.
fn dhcp_frame(payload: &[u8]) -> Vec<u8> {
    let payload = &payload[..payload.len().min(1400)];
    let udp_len = 8 + payload.len();
    let ip_len = 20 + udp_len;
    let mut f = vec![0xff; 6];
    f.extend_from_slice(&[0x52, 0x55, 0x0a, 0x00, 0x02, 0x02, 0x08, 0x00]);
    let mut ip = vec![
        0x45,
        0,
        (ip_len >> 8) as u8,
        ip_len as u8,
        0,
        0,
        0,
        0,
        64,
        17,
        0,
        0,
    ];
    ip.extend_from_slice(&[10, 0, 2, 2, 255, 255, 255, 255]);
    let sum = checksum(&ip);
    ip[10..12].copy_from_slice(&sum.to_be_bytes());
    f.extend_from_slice(&ip);
    f.extend_from_slice(&[0, 67, 0, 68, (udp_len >> 8) as u8, udp_len as u8, 0, 0]);
    f.extend_from_slice(payload);
    f
}

struct Messages(VecDeque<Vec<u8>>, usize);

impl Wire for Messages {
    fn read(&mut self, buf: &mut [u8]) -> Read {
        match self.0.pop_front() {
            Some(m) => {
                let n = m.len().min(buf.len());
                buf[..n].copy_from_slice(&m[..n]);
                Read::Message(n)
            }
            None => Read::Empty,
        }
    }

    fn write(&mut self, _: &[u8]) -> Write {
        if self.1 == 0 {
            return Write::Full;
        }
        self.1 -= 1;
        Write::Done
    }
}

struct Memory(Vec<u8>);

impl SlotMemory for Memory {
    fn read_slot(&self, slot: u32, out: &mut [u8]) {
        let at = slot as usize * 1536;
        out.copy_from_slice(&self.0[at..at + out.len()]);
    }

    fn write_slot(&mut self, slot: u32, data: &[u8]) {
        let at = slot as usize * 1536;
        self.0[at..at + data.len()].copy_from_slice(data);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    match mode % 7 {
        0 => {
            if let Some(scope) = Scope::decode(rest) {
                let reach: Vec<ReachRule<'_>> = scope.reach_rules().collect();
                let listen: Vec<ListenRule> = scope.listen_rules().collect();
                let mut out = vec![0u8; rest.len() + 8];
                let n = encode(&reach, &listen, &mut out).expect("a decoded scope encodes");
                assert_eq!(&out[..n], rest);
                assert!(scope.within(&scope));
            }
        }
        1 => with_rig(|rig| {
            for f in frames(rest) {
                rig.fixed.link().incoming.push_back(f.clone());
                rig.dhcp.link().incoming.push_back(f);
                rig.now += 1_000;
                rig.fixed.poll(rig.now);
                rig.dhcp.poll(rig.now);
            }
            let _ = rig.fixed.status(rig.now);
            let _ = rig.dhcp.status(rig.now);
        }),
        2 => with_rig(|rig| {
            // Raw: a DHCP reply that is not an answer to anything.
            rig.dhcp.link().incoming.push_back(dhcp_frame(rest));
            // And an answer to the client's own last frame, with the
            // input's options: this is what reaches the lease handling.
            rig.now += 2_000_000;
            rig.dhcp.poll(rig.now);
            let request = last_dhcp(&rig.dhcp.link().sent).map(<[u8]>::to_vec);
            rig.dhcp.link().sent.clear();
            if let Some(request) = request {
                let reply = dhcp_reply(&request, rest);
                rig.dhcp.link().incoming.push_back(dhcp_frame(&reply));
            }
            rig.now += 1_000;
            rig.dhcp.poll(rig.now);
            let _ = rig.dhcp.status(rig.now);
            let _ = rig.dhcp.take_events();
        }),
        3 => {
            let memory = Memory(vec![0; 128 * 1536]);
            let wire = Messages(frames(rest).into(), 3);
            let mut nic = Nic::new(wire, memory, true);
            nic.pump();
            let mut buf = [0u8; 1514];
            while nic.recv(&mut buf).is_some() {}
            let _ = nic.send(&buf[..60]);
            nic.flush();
        }
        4 => with_rig(|rig| {
            let mut service = Service::new();
            let root = service.add(Connection::root()).unwrap();
            let selector = rest.first().copied().unwrap_or(0);
            let handles = usize::from(selector % 3);
            let mut ids = vec![root];
            // A connection with an account of its own, as `zosh` makes for a
            // POSIX program: the scope is one wildcard (so `resolve` and
            // `connect` get past the scope), the rights and the budget come
            // from the input (a budget over 16 does not decode).
            let wild = [ReachRule {
                target: Target::Wildcard("zoraya"),
                ports: Ports::ANY,
                protocols: Protocols::BOTH,
            }];
            let mut scope = [0u8; 128];
            if let Some(n) = encode(&wild, &[], &mut scope) {
                let carve = zoraya_proto_net::NarrowAccount {
                    rights: u32::from(selector >> 2 & 0xf),
                    budget: zoraya_protocol::Below(u32::from(selector >> 6) + 1),
                    scope: zoraya_protocol::Data(&scope[..n]),
                };
                let mut out = [0u8; 256];
                if let Ok(len) = zoraya_protocol::encode(&carve, 1, &mut out) {
                    if let Action::Narrowed { id, .. } =
                        service.handle(root, &out[..len], 1, &mut rig.fixed, rig.now)
                    {
                        ids.push(id);
                    }
                }
            }
            for (i, message) in frames(rest.get(1..).unwrap_or(&[])).iter().enumerate() {
                let id = ids[i % ids.len()];
                if let Action::Narrowed { id, .. } =
                    service.handle(id, message, handles, &mut rig.fixed, rig.now)
                    && ids.len() < 8
                {
                    ids.push(id);
                }
            }
            // Answers of the resolver with fuzzed tokens: unknown ones, ones
            // in the range of the lookups, and ones that wrap.
            for (i, c) in rest.chunks_exact(4).enumerate() {
                let raw = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                let token = if i % 2 == 0 {
                    raw
                } else {
                    LOOKUP_BASE + (raw & 0xffff)
                };
                let result = if raw & 1 == 0 {
                    Ok(Found {
                        addresses: vec![c.try_into().map(|a: [u8; 4]| a).unwrap_or([0; 4])],
                        from_hosts: raw & 2 != 0,
                    })
                } else {
                    Err(zoraya_net::dns::ResolveFail::Failure)
                };
                let _ = service.resolve_done(&rig.fixed, Resolved { token, result }, rig.now);
            }
            for id in ids {
                service.remove(id);
            }
        }),
        5 => with_rig(|rig| {
            // A ping, answered by fuzzed echo replies with the run's identifier.
            rig.now += 5_000_000;
            rig.fixed.poll(rig.now);
            let _ = rig.fixed.ping([10, 0, 2, 2], 4, 1_000_000, rig.now);
            rig.fixed.link().sent.clear();
            for _ in 0..3 {
                rig.now += 200_000;
                rig.fixed.poll(rig.now);
                let ident = rig
                    .fixed
                    .link()
                    .sent
                    .iter()
                    .rev()
                    .find_map(|f| (f.get(23) == Some(&1)).then(|| f.get(38..40))?)
                    .map_or(0x5a51, |i| u16::from_be_bytes([i[0], i[1]]));
                rig.fixed.link().incoming.push_back(echo_reply(ident, rest));
                rig.now += 1_000;
                rig.fixed.poll(rig.now);
            }
            rig.now += 5_000_000;
            rig.fixed.poll(rig.now);
            let _ = rig.fixed.take_events();
        }),
        _ => {
            let _ = config::read(rest);
        }
    }
});

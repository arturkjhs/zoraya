//! `netd`'s side of everything that comes from outside the machine
//! (ADR-0221, INV-0221-2, INV-0220-3, INV-0229-1), on arbitrary bytes. The
//! first byte picks what the rest is:
//!
//! 0. a scope for `Scope::decode` (a decoded one must re-encode to the same
//!    bytes and lie within itself);
//! 1. frames for the stack, length-prefixed, in static and DHCP mode;
//! 2. the payload of a DHCP reply wrapped in a well-formed Ethernet, IPv4 and
//!    UDP frame (port 67 to 68), so that smoltcp's DHCP parser sees it;
//! 3. messages from the driver for `Nic::pump`, then whatever it queued;
//! 4. requests for `Service::handle` on a root connection;
//! 5. a settings file for `config::read`.
//!
//! Nothing may panic.

#![no_main]

use std::collections::VecDeque;

use libfuzzer_sys::fuzz_target;
use zoraya_abi::net::{ListenRule, ReachRule, Scope, encode};
use zoraya_net::config;
use zoraya_net::nic::{Nic, Read, SlotMemory, Wire, Write};
use zoraya_net::service::Service;
use zoraya_net::{Config, Connection, Link, Mode, Stack};

const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

#[derive(Default)]
struct Frames {
    incoming: VecDeque<Vec<u8>>,
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

    fn send(&mut self, _: &[u8]) -> bool {
        true
    }

    fn is_up(&self) -> bool {
        true
    }
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
    let mut ip = vec![0x45, 0, (ip_len >> 8) as u8, ip_len as u8, 0, 0, 0, 0, 64, 17, 0, 0];
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
    match mode % 6 {
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
        1 | 2 => {
            for config in [static_config(), Config::default()] {
                let mut stack = Stack::new(Frames::default(), MAC, config, 0, 1).unwrap();
                let list = if mode % 6 == 1 {
                    frames(rest)
                } else {
                    vec![dhcp_frame(rest)]
                };
                let mut now = 0;
                for f in list {
                    stack.link().incoming.push_back(f);
                    now += 1_000;
                    stack.poll(now);
                }
                stack.poll(now + 10_000_000);
                let _ = stack.status(now);
                let _ = stack.take_events();
            }
        }
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
        4 => {
            let mut stack =
                Stack::new(Frames::default(), MAC, static_config(), 0, 1).unwrap();
            let mut service = Service::new();
            let root = service.add(Connection::root()).unwrap();
            let handles = usize::from(rest.first().copied().unwrap_or(0) % 3);
            for message in frames(rest.get(1..).unwrap_or(&[])) {
                let _ = service.handle(root, &message, handles, &mut stack, 0);
            }
        }
        _ => {
            let _ = config::read(rest);
        }
    }
});

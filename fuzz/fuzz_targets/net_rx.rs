//! The receive path of the NIC driver (ADR-0221, INV-0221-2) on arbitrary
//! bytes: the bytes are a frame for the ARP parser, and also a script for a
//! hostile device that completes used-ring entries (any queue, any head,
//! any length) while the service releases and transmits arbitrary slots.
//! The client half is driven too: the bytes are also messages for
//! `zoraya_virtio_net::serve::Server` over a channel that is sometimes full.
//! Nothing may panic; a broken device is an `Err`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zoraya_proto_nic::arp;
use zoraya_virtio::DEVICE_NET;
use zoraya_virtio::fake::Device;
use zoraya_virtio_net::serve::{Flow, Host, Read, Server, Write};
use zoraya_virtio_net::{Net, OBJECT_SIZE, SHARED_MEMORY};
use std::collections::VecDeque;

const BASE: u64 = 0x4000_0000;

/// A channel that takes `room` more messages, and a queue of the client's.
#[derive(Default)]
struct Wire {
    room: usize,
    written: usize,
    incoming: VecDeque<(Vec<u8>, Option<u32>)>,
}

impl Host for Wire {
    type Handle = u32;

    fn write(&mut self, _: &[u8]) -> Write {
        if self.room == 0 {
            return Write::Full;
        }
        self.room -= 1;
        self.written += 1;
        Write::Done
    }

    fn read(&mut self, buffer: &mut [u8]) -> Read<u32> {
        match self.incoming.pop_front() {
            Some((bytes, handle)) => {
                let n = bytes.len().min(buffer.len());
                buffer[..n].copy_from_slice(&bytes[..n]);
                Read::Message { length: n, handle }
            }
            None => Read::Empty,
        }
    }

    fn close(&mut self, _: u32) {}

    fn pin(&mut self, _: u32) -> Option<u64> {
        Some(BASE + SHARED_MEMORY as u64)
    }

    fn unpin(&mut self) {}

    fn now(&mut self) -> u64 {
        0
    }
}

fuzz_target!(|data: &[u8]| {
    let mut out = [0u8; 64];
    let _ = arp::parse(data);
    let _ = arp::answer(data, [0x52, 0x54, 0, 1, 2, 3], [10, 0, 2, 15], &mut out);

    let d = Device::new(DEVICE_NET, BASE, SHARED_MEMORY + OBJECT_SIZE as usize);
    {
        let mut f = d.state.borrow_mut();
        f.device_features = [(1 << 5) | (1 << 16), 1];
        f.config[..6].copy_from_slice(&[0x52, 0x54, 0, 1, 2, 3]);
    }
    let Ok(net) = Net::new(d.registers(), d.shared(), BASE) else {
        return;
    };
    let mut server = Server::new(net, Wire::default());
    // The client attaches a frames object first, then the script runs.
    server
        .host
        .incoming
        .push_back((attach_message(), Some(1)));
    server.host.room = 8;
    if server.serve() != Flow::Go {
        return;
    }
    // Five bytes a step: an action, and two numbers.
    for step in data.chunks_exact(5) {
        let a = u32::from(u16::from_le_bytes([step[1], step[2]]));
        let b = u32::from(u16::from_le_bytes([step[3], step[4]]));
        match step[0] % 11 {
            0 => {
                let _ = server.net.release(a);
            }
            1 => {
                let _ = server.net.transmit(a, b);
            }
            2 => {
                let mut f = d.state.borrow_mut();
                if let Some(head) = f.next_available(0) {
                    f.complete(0, u32::from(head), b);
                }
            }
            3 => {
                let mut f = d.state.borrow_mut();
                f.complete(usize::from(step[1] & 1), a, b);
            }
            4 => d.state.borrow_mut().set_used_index(usize::from(step[1] & 1), a as u16),
            5 => {
                if server.net.poll(&mut |_, _| b & 1 == 0, &mut |_| {}).is_err() {
                    return;
                }
            }
            6 => {
                // A message of the client: the rest of the input, cut at b.
                let at = (b as usize).min(data.len());
                let cut = data[at..].len().min(96);
                server
                    .host
                    .incoming
                    .push_back((data[at..at + cut].to_vec(), (a & 1 == 1).then_some(2)));
            }
            7 => server.host.room = (a % 4) as usize * 100,
            8 => {
                if server.serve() == Flow::Stop {
                    return;
                }
            }
            9 => {
                if server.device_event() != Flow::Go {
                    return;
                }
            }
            _ => {
                d.state.borrow_mut().config[6] = (a & 1) as u8;
            }
        }
    }
});

/// An `Attach` message with its handle count, as the wire has it.
fn attach_message() -> Vec<u8> {
    use zoraya_proto_nic::Attach;
    let mut out = [0u8; 64];
    let n = zoraya_protocol::encode(&Attach {}, 1, &mut out).expect("fits");
    out[..n].to_vec()
}

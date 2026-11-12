#![no_std]
#![no_main]

//! The random service (ADR-0227 item 2, proposed ADR-0288). Slot 5 is the
//! seed (a memory object holding a length and `/chosen/rng-seed`, or
//! nothing), slot 6 the first connection, which `init` holds. Every
//! connection speaks `zoraya-rng`'s protocol; the logic, including the
//! generator, is that crate's, under host test. This process only moves
//! channel messages. It keeps no seed after the generator has absorbed it.
//!
//! Not yet: reseeding from a virtio-rng device (the generator's `mix` is
//! the entry for it; recorded in the task report and ADR-0288).

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::{DEADLINE_INFINITE, Handle, MapFlags, Mode, Packet, Status, channel_signals};
use zoraya_rng::{Action, Generator, OUT_SIZE, Service};
use zoraya_user::{
    channel_read, channel_write, handle_close, handle_type, memory_map, memory_unmap, port_create,
    port_subscribe, port_unsubscribe, port_wait, println, process_exit,
};

const SEED: u64 = 5;
const ROOT: u64 = 6;
/// Connections at once, the first included.
const MAX_CONNECTIONS: usize = 256;
/// Requests served per connection per turn, so that one cannot starve the rest.
const PER_TURN: usize = 8;

fn slot(n: u64) -> Handle {
    Handle::new(n).expect("slot is not zero")
}

/// The seed bytes, once, or nothing.
fn read_seed() -> Vec<u8> {
    let object = slot(SEED);
    if handle_type(object) != Ok("memory") {
        return Vec::new();
    }
    let Ok(base) = memory_map(object, 0, 4096, MapFlags::READ) else {
        return Vec::new();
    };
    // SAFETY: a read mapping of one page of this process; the kernel wrote a
    // length and that many bytes into it, and the length is checked here.
    let page = unsafe { core::slice::from_raw_parts(base as *const u8, 4096) };
    let length = u32::from_le_bytes([page[0], page[1], page[2], page[3]]) as usize;
    let seed = page
        .get(4..4 + length)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    let _ = memory_unmap(base, 4096);
    let _ = handle_close(object);
    seed
}

struct Rngd {
    port: Handle,
    service: Service,
    /// Connection `i` is key `i`.
    ends: Vec<Option<Handle>>,
    request: Vec<u8>,
    out: Vec<u8>,
}

impl Rngd {
    fn attach(&mut self, end: Handle) -> bool {
        let id = match self.ends.iter().position(Option::is_none) {
            Some(id) => id,
            None => {
                self.ends.push(None);
                self.ends.len() - 1
            }
        };
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        if port_subscribe(self.port, end, wanted, id as u64, Mode::Level).is_err() {
            return false;
        }
        self.ends[id] = Some(end);
        true
    }

    fn drop_connection(&mut self, id: usize) {
        if let Some(end) = self.ends.get_mut(id).and_then(Option::take) {
            let _ = port_unsubscribe(self.port, id as u64);
            let _ = handle_close(end);
            self.service.detached();
        }
    }

    fn serve(&mut self, id: usize) {
        for _ in 0..PER_TURN {
            let Some(end) = self.ends.get(id).copied().flatten() else {
                return;
            };
            let mut handles = [end; 2];
            let sizes = match channel_read(end, &mut self.request, &mut handles) {
                Ok(s) => s,
                Err((Status::WouldBlock, _)) => return,
                Err(_) => {
                    self.drop_connection(id);
                    return;
                }
            };
            let count = (sizes.handles as usize).min(handles.len());
            let action = self.service.handle(
                &self.request[..sizes.bytes as usize],
                sizes.handles as usize,
                &mut self.out,
            );
            match action {
                Action::Reply(n) => {
                    for h in &handles[..count] {
                        let _ = handle_close(*h);
                    }
                    if channel_write(end, &self.out[..n], &[]).is_err() {
                        self.drop_connection(id);
                        return;
                    }
                }
                Action::Share { reply, attach } => {
                    let given = handles[0];
                    let ok = count == 1
                        && handle_type(given) == Ok("channel")
                        && attach
                        && self.attach(given);
                    if !ok {
                        let _ = handle_close(given);
                        if attach {
                            self.service.detached();
                        }
                    }
                    if channel_write(end, &self.out[..reply], &[]).is_err() {
                        self.drop_connection(id);
                        return;
                    }
                }
                Action::Close => {
                    for h in &handles[..count] {
                        let _ = handle_close(*h);
                    }
                    self.drop_connection(id);
                    return;
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let seed = read_seed();
    if seed.is_empty() {
        println!("rngd: no seed from the device tree");
    }
    let Ok(port) = port_create() else {
        return 1;
    };
    let mut rngd = Rngd {
        port,
        service: Service::new(Generator::new(&seed), MAX_CONNECTIONS),
        ends: Vec::new(),
        request: vec![0u8; 256],
        out: vec![0u8; OUT_SIZE],
    };
    let root = slot(ROOT);
    if handle_type(root) == Ok("channel") && rngd.service.attached() && !rngd.attach(root) {
        return 1;
    }
    println!("rngd: serving");
    let mut packets = [Packet::default(); 8];
    loop {
        let count = match port_wait(port, DEADLINE_INFINITE, &mut packets) {
            Ok(n) => n,
            Err(_) => process_exit(2),
        };
        for p in &packets[..count] {
            rngd.serve(p.key as usize);
        }
    }
}

//! The compositor's protocol, as a program in an untrusted room can speak it
//! (ADR-0268 point 3's "fuzzing", ADR-0093, ADR-0094, ADR-0298): windows,
//! sheets, popups, buffers, drops, copy and paste, from clients the manager
//! added (one with a room name), with memory objects of any size, interleaved
//! with the input drivers' frames. The compositor runs on a fake platform and
//! must never panic, whatever order the messages come in.

#![no_main]

mod wire;

use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};

use libfuzzer_sys::fuzz_target;
use zoraya_compositor::{Compositor, Incoming, Kind, Outgoing, Platform};
use zoraya_proto_display as display;
use zoraya_proto_gui as gui;
use zoraya_protocol::{Below, encode};

const W: u32 = 640;
const H: u32 = 480;
const FRAMEBUFFER: u32 = 999;

#[derive(Default)]
struct Peer {
    inbox: VecDeque<(Vec<u8>, Vec<u32>)>,
    gone: bool,
    sent: usize,
}

struct Memory {
    size: u64,
}

#[derive(Default)]
struct Fake {
    peers: BTreeMap<u32, Peer>,
    memory: BTreeMap<u32, Memory>,
    watched: BTreeMap<u64, u32>,
    mapped: BTreeMap<u64, u32>,
    frame: Vec<u32>,
    display: u32,
    clock: u64,
    next: u32,
    reads: Cell<u64>,
}

impl Fake {
    fn channel(&mut self) -> u32 {
        self.next += 1;
        self.peers.insert(self.next, Peer::default());
        self.next
    }
    fn memory(&mut self, size: u64) -> u32 {
        self.next += 1;
        self.memory.insert(self.next, Memory { size });
        self.next
    }
    fn send(&mut self, end: u32, bytes: Vec<u8>, handles: Vec<u32>) {
        if let Some(p) = self.peers.get_mut(&end) {
            p.inbox.push_back((bytes, handles));
        }
    }
}

impl Platform for Fake {
    type End = u32;
    fn read(&mut self, end: u32, bytes: &mut [u8], handles: &mut Vec<u32>) -> Incoming {
        let Some(peer) = self.peers.get_mut(&end) else { return Incoming::Closed };
        match peer.inbox.pop_front() {
            Some((m, h)) if m.len() > bytes.len() || h.len() > 64 => Incoming::Refused,
            Some((m, h)) => {
                bytes[..m.len()].copy_from_slice(&m);
                handles.extend(h);
                Incoming::Message(m.len())
            }
            None if peer.gone => Incoming::Closed,
            None => Incoming::Empty,
        }
    }
    fn write(&mut self, end: u32, bytes: &[u8], _: &[u32]) -> Outgoing {
        let Some(peer) = self.peers.get_mut(&end) else { return Outgoing::Closed };
        if peer.gone {
            return Outgoing::Closed;
        }
        if end == self.display {
            // A flush is answered at once.
            let mut reply = vec![0; 64];
            if let Ok((h, _)) = zoraya_protocol::Header::decode(bytes)
                && let Ok(n) = encode(&display::Done { status: Below(0) }, h.transaction, &mut reply)
            {
                reply.truncate(n);
                peer.inbox.push_back((reply, vec![]));
            }
            return Outgoing::Sent;
        }
        // A client's queue holds 64 messages, then it is full.
        if peer.sent >= 64 {
            return Outgoing::Full;
        }
        peer.sent += 1;
        Outgoing::Sent
    }
    fn close(&mut self, end: u32) {
        self.watched.retain(|_, e| *e != end);
    }
    fn kind(&mut self, end: u32) -> Kind {
        if self.peers.contains_key(&end) {
            Kind::Channel
        } else if self.memory.contains_key(&end) {
            Kind::Memory
        } else {
            Kind::Other
        }
    }
    fn watch(&mut self, end: u32, key: u64) -> bool {
        self.watched.insert(key, end);
        true
    }
    fn map(&mut self, memory: u32, length: u64) -> Option<u64> {
        if length > self.memory.get(&memory)?.size {
            return None;
        }
        let address = u64::from(memory) << 32;
        self.mapped.insert(address, memory);
        Some(address)
    }
    fn unmap(&mut self, address: u64, _: u64) {
        self.mapped.remove(&address);
    }
    fn copy_from(&mut self, address: u64, out: &mut [u32]) -> bool {
        let id = (address >> 32) as u32;
        if !self.mapped.contains_key(&(u64::from(id) << 32)) {
            return false;
        }
        self.reads.set(self.reads.get() + 1);
        for (i, o) in out.iter_mut().enumerate() {
            *o = (self.reads.get() as u32).wrapping_add(i as u32);
        }
        true
    }
    fn framebuffer(&mut self, width: u32, height: u32) -> Option<u32> {
        let n = (width as usize).checked_mul(height as usize)?;
        if self.frame.len() != n {
            self.frame = vec![0; n];
        }
        Some(FRAMEBUFFER)
    }
    fn pixels(&mut self) -> &mut [u32] {
        &mut self.frame
    }
    fn call(&mut self, _: u32, message: &[u8], _: &[u32], reply: &mut [u8]) -> Option<usize> {
        let (header, _) = zoraya_protocol::Header::decode(message).ok()?;
        match header.ordinal {
            1 => encode(
                &display::InfoReply {
                    status: Below(0),
                    width: W,
                    height: H,
                    format: display::FORMAT_XRGB,
                    width_mm: 0,
                    height_mm: 0,
                },
                header.transaction,
                reply,
            )
            .ok(),
            2 => encode(&display::Done { status: Below(0) }, header.transaction, reply).ok(),
            _ => None,
        }
    }
    fn wait(&mut self, ready: &mut Vec<u64>, deadline: Option<u64>) {
        for (key, end) in &self.watched {
            if let Some(p) = self.peers.get(end)
                && (!p.inbox.is_empty() || p.gone)
            {
                ready.push(*key);
            }
        }
        if ready.is_empty()
            && let Some(d) = deadline
        {
            self.clock = self.clock.max(d);
        }
    }
    fn now(&mut self) -> u64 {
        self.clock += 1000;
        self.clock
    }
}

fn message<'a, M: zoraya_protocol::Message<'a>>(m: &M) -> Vec<u8> {
    let mut out = vec![0; zoraya_protocol::MAX_WIRE_MESSAGE];
    let n = encode(m, 9, &mut out).unwrap();
    out.truncate(n);
    out
}

struct Rig {
    c: Compositor<Fake>,
    manager: u32,
    keyboard: u32,
    tablet: u32,
}

fn rig() -> Rig {
    let mut f = Fake::default();
    let manager = f.channel();
    let display = f.channel();
    let keyboard = f.channel();
    let tablet = f.channel();
    f.display = display;
    let c = Compositor::new(f, manager, Some(display), &[keyboard, tablet]);
    Rig { c, manager, keyboard, tablet }
}

thread_local! {
    // Making a compositor parses the fonts and paints the desktop: once per
    // process, so state carries over between inputs (as in net_stack).
    static RIG: std::cell::RefCell<Option<Rig>> = const { std::cell::RefCell::new(None) };
}

fuzz_target!(|data: &[u8]| {
    RIG.with(|cell| {
        let mut slot = cell.borrow_mut();
        let r = slot.get_or_insert_with(rig);
        run(r, data);
    });
});

fn run(r: &mut Rig, data: &[u8]) {
    let (manager, keyboard, tablet) = (r.manager, r.keyboard, r.tablet);
    let c = &mut r.c;
    // Three clients, as the manager adds them; the last input's are gone.
    let mut clients = Vec::new();
    for _ in 0..3 {
        let end = c.platform().channel();
        let bytes = message(&gui::AddClient {});
        c.platform().send(manager, bytes, vec![end]);
        c.step();
        clients.push(end);
    }
    for m in wire::messages(data, 64) {
        match m.sel % 8 {
            0 => {
                // Input: the bytes are an input frame's.
                let to = if m.sel & 8 == 0 { keyboard } else { tablet };
                c.platform().send(to, m.bytes, vec![]);
            }
            _ => {
                let to = clients[usize::from(m.sel >> 3) % clients.len()];
                let handles = (0..m.handles)
                    .map(|i| {
                        if (m.sel >> i) & 1 == 0 {
                            c.platform().memory(4096 * (1 + u64::from(m.sel & 15) * 16))
                        } else {
                            c.platform().channel()
                        }
                    })
                    .collect();
                c.platform().send(to, m.bytes, handles);
            }
        }
        c.step();
        if m.sel % 16 == 5 {
            let to = clients[usize::from(m.sel >> 4) % clients.len()];
            if let Some(p) = c.platform().peers.get_mut(&to) {
                p.sent = 0;
            }
        }
    }
    for end in clients {
        if let Some(p) = c.platform().peers.get_mut(&end) {
            p.gone = true;
        }
    }
    c.step();
    c.platform().peers.retain(|_, p| !p.gone);
    c.platform().memory.clear();
}

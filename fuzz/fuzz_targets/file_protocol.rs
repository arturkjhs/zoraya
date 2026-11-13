//! The file service's requests (ADR-0268 point 3's "fuzzing", ADR-0298):
//! every request a program in an untrusted room can send `fsd` over its
//! connection, and the handles it can put in one (channels, tokens, memory
//! objects of any size), on a small volume. The service must answer or close
//! and never panic, and the volume it leaves must check (`fsck`).

#![no_main]

mod wire;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use libfuzzer_sys::fuzz_target;
use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::memory::MemoryDevice;
use zoraya_fs_service::{HandleKind, Incoming, Interest, Outgoing, Service, Transport, Wake};
use zoraya_fs_testkit::TestClock;

type End = u32;

#[derive(Default)]
struct Net {
    next: End,
    queues: BTreeMap<End, VecDeque<(Vec<u8>, Vec<End>)>>,
    closed: BTreeSet<End>,
    kinds: BTreeMap<End, HandleKind>,
    interests: BTreeMap<u64, (End, Interest)>,
    memory: BTreeMap<End, Vec<u8>>,
    mapped: BTreeMap<u64, End>,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Net>>);

impl Fake {
    fn end(&self, kind: HandleKind) -> End {
        let mut n = self.0.borrow_mut();
        n.next += 1;
        let e = n.next;
        n.kinds.insert(e, kind);
        if kind == HandleKind::Channel {
            n.queues.insert(e, VecDeque::new());
        }
        e
    }
}

impl Transport for Fake {
    type End = End;
    fn read(&mut self, end: End, bytes: &mut [u8], handles: &mut Vec<End>) -> Incoming {
        let mut n = self.0.borrow_mut();
        match n.queues.get_mut(&end).and_then(VecDeque::pop_front) {
            Some((m, h)) if m.len() <= bytes.len() => {
                bytes[..m.len()].copy_from_slice(&m);
                handles.extend(h);
                Incoming::Message(m.len())
            }
            Some(_) => Incoming::Refused,
            None if n.closed.contains(&end) => Incoming::Closed,
            None => Incoming::Empty,
        }
    }
    fn write(&mut self, end: End, _: &[u8], _: &[End]) -> Outgoing {
        if self.0.borrow().closed.contains(&end) { Outgoing::Closed } else { Outgoing::Sent }
    }
    fn interest(&mut self, end: End, key: u64, interest: Interest) -> bool {
        self.0.borrow_mut().interests.insert(key, (end, interest));
        true
    }
    fn close(&mut self, end: End) {
        let mut n = self.0.borrow_mut();
        n.closed.insert(end);
        n.interests.retain(|_, (e, _)| *e != end);
    }
    fn kind(&mut self, end: End) -> HandleKind {
        self.0.borrow().kinds.get(&end).copied().unwrap_or(HandleKind::Other)
    }
    fn duplicate(&mut self, end: End) -> Option<End> {
        let kind = self.kind(end);
        Some(self.end(kind))
    }
    fn new_token(&mut self) -> Option<End> {
        Some(self.end(HandleKind::Token))
    }
    fn same(&mut self, a: End, b: End) -> bool {
        a == b
    }
    fn map(&mut self, memory: End, length: u64) -> Option<u64> {
        let mut n = self.0.borrow_mut();
        if n.memory.get(&memory)?.len() as u64 != length {
            return None;
        }
        let address = u64::from(memory) << 32;
        n.mapped.insert(address, memory);
        Some(address)
    }
    fn unmap(&mut self, address: u64, _: u64) {
        self.0.borrow_mut().mapped.remove(&address);
    }
    fn copy_from(&mut self, address: u64, out: &mut [u8]) -> bool {
        let n = self.0.borrow();
        let Some(mem) = n.mapped.get(&(address & !0xffff_ffff)).and_then(|m| n.memory.get(m)) else {
            return false;
        };
        let at = (address & 0xffff_ffff) as usize;
        match mem.get(at..at + out.len()) {
            Some(src) => {
                out.copy_from_slice(src);
                true
            }
            None => false,
        }
    }
    fn copy_to(&mut self, address: u64, bytes: &[u8]) -> bool {
        let mut n = self.0.borrow_mut();
        let Some(key) = n.mapped.get(&(address & !0xffff_ffff)).copied() else { return false };
        let at = (address & 0xffff_ffff) as usize;
        match n.memory.get_mut(&key).and_then(|m| m.get_mut(at..at + bytes.len())) {
            Some(dst) => {
                dst.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
    fn report(&mut self, _: u32) {}
    fn wait(&mut self, _: Option<u64>) -> Wake {
        let n = self.0.borrow();
        for (key, (end, interest)) in &n.interests {
            let closed = n.closed.contains(end);
            let ready = match interest {
                Interest::Requests => closed || n.queues.get(end).is_some_and(|q| !q.is_empty()),
                Interest::Reply => true,
                Interest::Closed => closed,
            };
            if ready {
                return Wake::Ready(*key);
            }
        }
        Wake::Stop
    }
    fn now(&mut self) -> u64 {
        0
    }
}

fuzz_target!(|data: &[u8]| {
    let now = Timestamp { secs: 1_700_000_000, nanos: 0 };
    let mut device = MemoryDevice::new(vec![0u8; 512 * 4096]);
    mkfs(&mut device, MkfsOptions { volume_id: 7, now, root_mode: 0o755 }).unwrap();
    let volume = Volume::mount(device, TestClock::new(now), MountOptions::default()).unwrap();
    let net = Fake::default();
    let mut service = Service::new(volume, net.clone());
    let root = net.end(HandleKind::Channel);
    assert!(service.add_root(root));
    let mut channels = vec![root];
    for m in wire::messages(data, 48) {
        let to = channels[usize::from(m.sel >> 3) % channels.len()];
        let mut handles = Vec::new();
        for i in 0..m.handles {
            let h = match (m.sel >> i) & 3 {
                0 => {
                    let c = net.end(HandleKind::Channel);
                    if channels.len() < 32 {
                        channels.push(c);
                    }
                    c
                }
                1 => net.end(HandleKind::Token),
                2 => {
                    let e = net.end(HandleKind::Memory);
                    let size = 4096 * (1 + usize::from(m.sel & 7));
                    net.0.borrow_mut().memory.insert(e, vec![0xa5; size]);
                    e
                }
                _ => channels[usize::from(m.sel) % channels.len()],
            };
            handles.push(h);
        }
        net.0.borrow_mut().queues.get_mut(&to).unwrap().push_back((m.bytes, handles));
        for _ in 0..64 {
            if !service.step() {
                break;
            }
        }
    }
    let _ = (service.open_count(), service.descriptions());
    // What the service committed must check (`zoraya-fsck`'s own check, on
    // the live device); a volume that refused the commit (full, read-only,
    // failed) is a state of its own and is not checked.
    if service.volume().sync().is_ok() {
        let report = check(service.volume().device_mut(), CheckOptions::default());
        assert!(report.is_clean(), "fsck: {:?}", report.problems);
    }
});

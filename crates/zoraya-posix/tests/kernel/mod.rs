//! A fake kernel for the library's host tests: channels, streams, memory
//! objects and tokens in one table, with the real filesystem service on the
//! other end of the channels. A wait that would block runs the service's
//! loop until something is ready, and fails if nothing ever would.

#![allow(dead_code)] // Reason: each test binary uses part of the rig.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use zoraya_abi::Status;
use zoraya_abi::posix::EntryKind;
use zoraya_fs::{BlockDevice, MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::{
    Device, HandleKind, Incoming, Interest, Outgoing, Service, Transport, Wake,
};
use zoraya_fs_testkit::clock::TestClock;
use zoraya_fs_testkit::devices::MemDevice;
use zoraya_posix::path::Form;
use zoraya_posix::process::Posix;
use zoraya_posix::sys::{Sys, User, signals};

pub type H = u32;

/// The ring of a stream (ADR-0101's rules, simply).
#[derive(Default)]
pub struct Ring {
    bytes: VecDeque<u8>,
    readers: u32,
    writers: u32,
}

pub const STREAM_CAPACITY: usize = 64 * 1024;
const STREAM_ATOMIC: usize = 4096;

enum Obj {
    /// A channel end: its queue and its peer object.
    End {
        queue: VecDeque<(Vec<u8>, Vec<H>)>,
        peer: u32,
    },
    StreamRead(Rc<RefCell<Ring>>),
    StreamWrite(Rc<RefCell<Ring>>),
    Memory(Vec<u8>),
    Token,
}

#[derive(Default)]
pub struct Net {
    next: u32,
    /// Handle to object.
    handles: BTreeMap<H, u32>,
    objects: BTreeMap<u32, Obj>,
    /// Handles held to each object.
    holders: BTreeMap<u32, u32>,
    /// Each service's subscriptions, by its id and its keys.
    interests: BTreeMap<(u32, u64), (H, Interest)>,
    services: u32,
    pub reports: Vec<u32>,
    pub now: u64,
}

impl Net {
    fn new_object(&mut self, o: Obj) -> u32 {
        self.next += 1;
        self.objects.insert(self.next, o);
        self.next
    }

    fn new_handle(&mut self, object: u32) -> H {
        self.next += 1;
        self.handles.insert(self.next, object);
        *self.holders.entry(object).or_default() += 1;
        self.next
    }

    fn object(&self, h: H) -> Option<u32> {
        self.handles.get(&h).copied()
    }

    /// Takes up to `n` bytes out of the stream whose read end is `h`, as a
    /// reader in another process would.
    pub fn drain(&mut self, h: H, n: usize) -> usize {
        let Some(Obj::StreamRead(r)) = self.object(h).and_then(|o| self.objects.get(&o)) else {
            return 0;
        };
        let mut r = r.borrow_mut();
        let k = n.min(r.bytes.len());
        r.bytes.drain(..k);
        k
    }

    pub fn close(&mut self, h: H) {
        let Some(object) = self.handles.remove(&h) else {
            return;
        };
        let held = self.holders.get_mut(&object).unwrap();
        *held -= 1;
        if *held > 0 {
            return;
        }
        self.holders.remove(&object);
        match self.objects.remove(&object) {
            Some(Obj::StreamRead(r)) => r.borrow_mut().readers -= 1,
            Some(Obj::StreamWrite(r)) => r.borrow_mut().writers -= 1,
            // Handles queued in a closed end go with it.
            Some(Obj::End { queue, .. }) => {
                for (_, list) in queue {
                    for h in list {
                        self.close(h);
                    }
                }
            }
            _ => {}
        }
    }

    fn peer_alive(&self, object: u32) -> bool {
        match self.objects.get(&object) {
            Some(Obj::End { peer, .. }) => self.objects.contains_key(peer),
            _ => false,
        }
    }

    fn signals(&self, h: H) -> u32 {
        let Some(object) = self.object(h) else {
            return 0;
        };
        match self.objects.get(&object) {
            Some(Obj::End { queue, .. }) => {
                let mut s = 0;
                if !queue.is_empty() {
                    s |= signals::READABLE;
                }
                if !self.peer_alive(object) {
                    s |= signals::PEER_CLOSED;
                }
                s
            }
            Some(Obj::StreamRead(r)) => {
                let r = r.borrow();
                let mut s = 0;
                if !r.bytes.is_empty() || r.writers == 0 {
                    s |= signals::READABLE;
                }
                if r.writers == 0 {
                    s |= signals::PEER_CLOSED;
                }
                s
            }
            Some(Obj::StreamWrite(r)) => {
                let r = r.borrow();
                let mut s = 0;
                if STREAM_CAPACITY - r.bytes.len() >= STREAM_ATOMIC && r.readers > 0 {
                    s |= signals::WRITABLE;
                }
                if r.readers == 0 {
                    s |= signals::PEER_CLOSED;
                }
                s
            }
            _ => 0,
        }
    }

    /// Bytes at `address`: the high half names the memory object.
    fn memory(&mut self, address: u64, length: usize) -> &mut [u8] {
        let object = (address >> 32) as u32;
        let at = (address & 0xffff_ffff) as usize;
        match self.objects.get_mut(&object) {
            Some(Obj::Memory(bytes)) => &mut bytes[at..at + length],
            _ => panic!("no memory at {address:#x}"),
        }
    }
}

/// The service's side: the net and the service's id, which keeps its
/// subscription keys apart from another service's.
#[derive(Clone)]
pub struct Wire(pub Rc<RefCell<Net>>, pub u32);

impl Transport for Wire {
    type End = H;

    fn read(&mut self, end: H, bytes: &mut [u8], handles: &mut Vec<H>) -> Incoming {
        let mut n = self.0.borrow_mut();
        let Some(object) = n.object(end) else {
            return Incoming::Closed;
        };
        let alive = n.peer_alive(object);
        match n.objects.get_mut(&object) {
            Some(Obj::End { queue, .. }) => match queue.pop_front() {
                Some((message, list)) => {
                    bytes[..message.len()].copy_from_slice(&message);
                    handles.extend(list);
                    Incoming::Message(message.len())
                }
                None if !alive => Incoming::Closed,
                None => Incoming::Empty,
            },
            _ => Incoming::Refused,
        }
    }

    fn write(&mut self, end: H, bytes: &[u8], handles: &[H]) -> Outgoing {
        let mut n = self.0.borrow_mut();
        let Some(object) = n.object(end) else {
            return Outgoing::Closed;
        };
        let peer = match n.objects.get(&object) {
            Some(Obj::End { peer, .. }) => *peer,
            _ => return Outgoing::Closed,
        };
        match n.objects.get_mut(&peer) {
            Some(Obj::End { queue, .. }) => {
                queue.push_back((bytes.to_vec(), handles.to_vec()));
                Outgoing::Sent
            }
            _ => Outgoing::Closed,
        }
    }

    fn interest(&mut self, end: H, key: u64, interest: Interest) -> bool {
        let id = self.1;
        self.0
            .borrow_mut()
            .interests
            .insert((id, key), (end, interest));
        true
    }

    fn close(&mut self, end: H) {
        let mut n = self.0.borrow_mut();
        n.close(end);
        n.interests.retain(|_, (e, _)| *e != end);
    }

    fn kind(&mut self, end: H) -> HandleKind {
        let n = self.0.borrow();
        match n.object(end).and_then(|o| n.objects.get(&o)) {
            Some(Obj::End { .. }) => HandleKind::Channel,
            Some(Obj::Memory(_)) => HandleKind::Memory,
            Some(Obj::Token) => HandleKind::Token,
            _ => HandleKind::Other,
        }
    }

    fn duplicate(&mut self, end: H) -> Option<H> {
        let mut n = self.0.borrow_mut();
        let object = n.object(end)?;
        Some(n.new_handle(object))
    }

    fn new_token(&mut self) -> Option<H> {
        let mut n = self.0.borrow_mut();
        let object = n.new_object(Obj::Token);
        Some(n.new_handle(object))
    }

    fn same(&mut self, a: H, b: H) -> bool {
        let n = self.0.borrow();
        n.object(a).is_some() && n.object(a) == n.object(b)
    }

    fn map(&mut self, memory: H, length: u64) -> Option<u64> {
        let n = self.0.borrow();
        let object = n.object(memory)?;
        match n.objects.get(&object) {
            Some(Obj::Memory(bytes)) if bytes.len() as u64 == length => {
                Some(u64::from(object) << 32)
            }
            _ => None,
        }
    }

    fn unmap(&mut self, _address: u64, _length: u64) {}

    fn copy_from(&mut self, address: u64, out: &mut [u8]) -> bool {
        out.copy_from_slice(self.0.borrow_mut().memory(address, out.len()));
        true
    }

    fn copy_to(&mut self, address: u64, bytes: &[u8]) -> bool {
        self.0
            .borrow_mut()
            .memory(address, bytes.len())
            .copy_from_slice(bytes);
        true
    }

    fn report(&mut self, reason: u32) {
        self.0.borrow_mut().reports.push(reason);
    }

    fn wait(&mut self, _deadline: Option<u64>) -> Wake {
        let n = self.0.borrow();
        for ((id, key), (end, interest)) in &n.interests {
            if *id != self.1 {
                continue;
            }
            let s = n.signals(*end);
            let ready = match interest {
                Interest::Requests => s & (signals::READABLE | signals::PEER_CLOSED) != 0,
                Interest::Reply => true,
                Interest::Closed => s & signals::PEER_CLOSED != 0,
            };
            if ready {
                return Wake::Ready(*key);
            }
        }
        Wake::Stop
    }

    fn now(&mut self) -> u64 {
        self.0.borrow().now
    }
}

pub struct TestDevice(MemDevice);

impl BlockDevice for TestDevice {
    fn sector_count(&self) -> u64 {
        self.0.sector_count()
    }
    fn flush_supported(&self) -> bool {
        true
    }
    fn read(&mut self, b: u64, buf: &mut zoraya_fs::Block) -> Result<(), zoraya_fs::DevError> {
        self.0.read(b, buf)
    }
    fn write(&mut self, w: &[(u64, &zoraya_fs::Block)]) -> Result<(), zoraya_fs::DevError> {
        self.0.write(w)
    }
    fn flush(&mut self) -> Result<(), zoraya_fs::DevError> {
        self.0.flush()
    }
}

impl Device for TestDevice {
    fn is_lost(&self) -> bool {
        false
    }
    fn mark_lost(&mut self) {}
}

pub type Svc = Service<TestDevice, TestClock, Wire>;

/// The library's side.
/// A reply to give instead of the next one, from its transaction.
pub type Craft = Box<dyn FnOnce(u32) -> Vec<u8>>;
/// What another process does while a wait waits.
pub type Meanwhile = Box<dyn FnMut(&mut Net)>;

pub struct Fake {
    pub net: Rc<RefCell<Net>>,
    pub services: Vec<Rc<RefCell<Svc>>>,
    pub waits: u32,
    /// Replaces the next reply read: given the reply's transaction, the
    /// bytes to give instead (a reply the service would never send).
    pub craft: Option<Craft>,
    /// Replies to let through before `craft` takes one.
    pub craft_skip: u32,
    /// Run when a wait finds nothing yet, before the services: what another
    /// process would do meanwhile (drain a stream, close its reader).
    pub meanwhile: Option<Meanwhile>,
}

impl Fake {
    pub fn new(net: Rc<RefCell<Net>>, services: Vec<Rc<RefCell<Svc>>>) -> Fake {
        Fake {
            net,
            services,
            waits: 0,
            craft: None,
            craft_skip: 0,
            meanwhile: None,
        }
    }
}

impl Fake {
    /// Runs every service until `h` asserts one of `wanted`.
    fn run_until(&mut self, h: H, wanted: u32) -> Result<u32, Status> {
        loop {
            let s = self.net.borrow().signals(h);
            if s & wanted != 0 {
                return Ok(s);
            }
            if let Some(f) = self.meanwhile.as_mut() {
                f(&mut self.net.borrow_mut());
                let s = self.net.borrow().signals(h);
                if s & wanted != 0 {
                    return Ok(s);
                }
            }
            let mut stepped = false;
            for service in &self.services {
                stepped |= service.borrow_mut().step();
            }
            if !stepped {
                // Nothing would ever wake this wait: a test bug or a hang.
                return Err(Status::BadState);
            }
        }
    }
}

impl Sys for Fake {
    type H = H;

    fn close(&mut self, h: H) {
        self.net.borrow_mut().close(h);
    }

    fn duplicate(&mut self, h: H) -> Result<H, Status> {
        let mut n = self.net.borrow_mut();
        let object = n.object(h).ok_or(Status::BadHandle)?;
        Ok(n.new_handle(object))
    }

    fn channel_create(&mut self) -> Result<(H, H), Status> {
        let mut n = self.net.borrow_mut();
        let a = n.new_object(Obj::End {
            queue: VecDeque::new(),
            peer: 0,
        });
        let b = n.new_object(Obj::End {
            queue: VecDeque::new(),
            peer: a,
        });
        if let Some(Obj::End { peer, .. }) = n.objects.get_mut(&a) {
            *peer = b;
        }
        Ok((n.new_handle(a), n.new_handle(b)))
    }

    fn channel_write(&mut self, end: H, bytes: &[u8], moved: &[H]) -> Result<(), Status> {
        let r = Wire(self.net.clone(), 0).write(end, bytes, moved);
        match r {
            Outgoing::Sent => Ok(()),
            _ => {
                for &h in moved {
                    self.close(h);
                }
                Err(Status::PeerClosed)
            }
        }
    }

    fn channel_read(
        &mut self,
        end: H,
        bytes: &mut [u8],
        handles: &mut Vec<H>,
    ) -> Result<usize, Status> {
        match Wire(self.net.clone(), 0).read(end, bytes, handles) {
            Incoming::Message(n) if self.craft.is_some() && self.craft_skip > 0 => {
                self.craft_skip -= 1;
                Ok(n)
            }
            Incoming::Message(n) => match self.craft.take() {
                Some(craft) => {
                    let (header, _) = zoraya_protocol::Header::decode(&bytes[..n]).unwrap();
                    let t = header.transaction;
                    let crafted = craft(t);
                    bytes[..crafted.len()].copy_from_slice(&crafted);
                    Ok(crafted.len())
                }
                None => Ok(n),
            },
            Incoming::Empty => Err(Status::WouldBlock),
            Incoming::Closed => Err(Status::PeerClosed),
            Incoming::Refused => Err(Status::BadHandle),
        }
    }

    fn wait(&mut self, h: H, wanted: u32) -> Result<u32, Status> {
        self.waits += 1;
        self.run_until(h, wanted)
    }

    fn stream_create(&mut self) -> Result<(H, H), Status> {
        let ring = Rc::new(RefCell::new(Ring {
            readers: 1,
            writers: 1,
            ..Ring::default()
        }));
        let mut n = self.net.borrow_mut();
        let r = n.new_object(Obj::StreamRead(ring.clone()));
        let w = n.new_object(Obj::StreamWrite(ring));
        Ok((n.new_handle(r), n.new_handle(w)))
    }

    fn stream_read(&mut self, end: H, into: User) -> Result<usize, Status> {
        let mut n = self.net.borrow_mut();
        let object = n.object(end).ok_or(Status::BadHandle)?;
        let Some(Obj::StreamRead(ring)) = n.objects.get(&object) else {
            return Err(Status::BadHandle);
        };
        let ring = ring.clone();
        let mut ring = ring.borrow_mut();
        if ring.bytes.is_empty() {
            return if ring.writers == 0 {
                Ok(0)
            } else {
                Err(Status::WouldBlock)
            };
        }
        let count = into.length.min(ring.bytes.len());
        let taken: Vec<u8> = ring.bytes.drain(..count).collect();
        n.memory(into.address, count).copy_from_slice(&taken);
        Ok(count)
    }

    fn stream_write(&mut self, end: H, from: User) -> Result<usize, Status> {
        let mut n = self.net.borrow_mut();
        let object = n.object(end).ok_or(Status::BadHandle)?;
        let Some(Obj::StreamWrite(ring)) = n.objects.get(&object) else {
            return Err(Status::BadHandle);
        };
        let ring = ring.clone();
        let mut ring = ring.borrow_mut();
        if ring.readers == 0 {
            return Err(Status::PeerClosed);
        }
        let room = STREAM_CAPACITY - ring.bytes.len();
        if (from.length <= STREAM_ATOMIC && room < from.length.max(1)) || room == 0 {
            return Err(Status::WouldBlock);
        }
        let count = from.length.min(room);
        let bytes = n.memory(from.address, count).to_vec();
        ring.bytes.extend(bytes);
        Ok(count)
    }

    fn copy_in(&mut self, from: User, out: &mut [u8]) {
        out.copy_from_slice(self.net.borrow_mut().memory(from.address, out.len()));
    }

    fn copy_out(&mut self, to: User, bytes: &[u8]) {
        self.net
            .borrow_mut()
            .memory(to.address, bytes.len())
            .copy_from_slice(bytes);
    }

    fn copy(&mut self, to: User, from: User) {
        let bytes = self
            .net
            .borrow_mut()
            .memory(from.address, from.length)
            .to_vec();
        self.copy_out(to, &bytes);
    }

    fn memory_create(&mut self, size: u64) -> Result<H, Status> {
        let mut n = self.net.borrow_mut();
        let object = n.new_object(Obj::Memory(vec![0; size as usize]));
        Ok(n.new_handle(object))
    }

    fn memory_map(&mut self, object: H, length: u64) -> Result<u64, Status> {
        Wire(self.net.clone(), 0)
            .map(object, length)
            .ok_or(Status::InvalidArgument)
    }

    fn memory_unmap(&mut self, _address: u64, _length: u64) -> Result<(), Status> {
        Ok(())
    }

    fn now(&mut self) -> u64 {
        let mut n = self.net.borrow_mut();
        n.now += 1;
        n.now
    }
}

pub const NOW: Timestamp = Timestamp {
    secs: 1000,
    nanos: 0,
};

/// A volume with its own service, and a client end with every right to its
/// root.
/// `blocks` of 4 KiB: every test of a binary holds its rigs at once, under
/// one 512 MiB cap.
pub fn volume(net: &Rc<RefCell<Net>>, blocks: u64) -> (Rc<RefCell<Svc>>, H) {
    let mut device = MemDevice::new(blocks);
    mkfs(
        &mut device,
        MkfsOptions {
            volume_id: 7,
            now: NOW,
            root_mode: 0o755,
        },
    )
    .unwrap();
    let volume = Volume::mount(
        TestDevice(device),
        TestClock::new(NOW),
        MountOptions::default(),
    )
    .unwrap();
    let id = {
        let mut n = net.borrow_mut();
        n.services += 1;
        n.services
    };
    let service = Rc::new(RefCell::new(Service::new(volume, Wire(net.clone(), id))));
    let mut fake = Fake::new(net.clone(), vec![]);
    let (client, server) = fake.channel_create().unwrap();
    assert!(service.borrow_mut().add_root(server));
    (service, client)
}

/// The program's memory: one object, addressed from `base`.
pub struct Arena {
    pub base: u64,
}

impl Arena {
    pub fn user(&self, offset: usize, length: usize) -> User {
        User::new(self.base + offset as u64, length)
    }
}

pub struct Rig {
    pub p: Posix<Fake>,
    pub net: Rc<RefCell<Net>>,
    pub arena: Arena,
    pub root: H,
    /// The far end of the report channel.
    pub reports: H,
}

pub const ARENA: usize = 4 * 1024 * 1024;

/// A process with `/` on a fresh volume, `/dev/null`, a `/tmp` that is a
/// directory of the same volume, and cwd `/`.
pub fn rig() -> Rig {
    rig_sized(2048)
}

/// `rig` with a root volume of `blocks` blocks of 4 KiB.
pub fn rig_sized(blocks: u64) -> Rig {
    let net = Rc::new(RefCell::new(Net::default()));
    let (service, root) = volume(&net, blocks);
    let mut fake = Fake::new(net.clone(), vec![service]);
    let arena = fake.memory_create(ARENA as u64).unwrap();
    let base = fake.memory_map(arena, ARENA as u64).unwrap();
    let (report, reports) = fake.channel_create().unwrap();
    let mut p = Posix::new(
        fake,
        zoraya_abi::posix::Facts {
            uid: 1000,
            gid: 100,
            umask: 0o022,
            ..Default::default()
        },
    );
    let slash = p.add_entry("/", EntryKind::Directory, Some(root));
    p.add_entry("/dev/null", EntryKind::Null, None);
    p.set_cwd(slash, "", Form::EntryRelative);
    p.set_report(Some(report));
    Rig {
        p,
        net,
        arena: Arena { base },
        root,
        reports,
    }
}

impl Rig {
    /// Puts `bytes` in the program's memory at `offset`.
    pub fn poke(&mut self, offset: usize, bytes: &[u8]) -> User {
        let u = self.arena.user(offset, bytes.len());
        self.p.sys.copy_out(u, bytes);
        u
    }

    pub fn peek(&mut self, offset: usize, length: usize) -> Vec<u8> {
        let mut out = vec![0; length];
        self.p
            .sys
            .copy_in(self.arena.user(offset, length), &mut out);
        out
    }

    pub fn write(&mut self, fd: i32, bytes: &[u8]) -> Result<usize, i32> {
        let u = self.poke(0, bytes);
        self.p.write(fd, u)
    }

    pub fn read(&mut self, fd: i32, length: usize) -> Result<Vec<u8>, i32> {
        let u = self.arena.user(ARENA / 2, length);
        let n = self.p.read(fd, u)?;
        Ok(self.peek(ARENA / 2, n))
    }

    /// The reports sent so far, decoded.
    pub fn reports(&mut self) -> Vec<(zoraya_abi::posix::Access, u32, String)> {
        let mut out = vec![];
        let mut buffer = vec![0; 8192];
        let mut handles = vec![];
        while let Ok(n) = self
            .p
            .sys
            .channel_read(self.reports, &mut buffer, &mut handles)
        {
            let r = zoraya_abi::posix::Report::decode(&buffer[..n]).expect("a report");
            out.push((r.access, r.dropped, r.path.to_string()));
        }
        out
    }

    /// Runs every service until none has anything to do: every close seen.
    pub fn settle(&mut self) {
        loop {
            let mut stepped = false;
            for service in &self.p.sys.services {
                stepped |= service.borrow_mut().step();
            }
            if !stepped {
                return;
            }
        }
    }

    /// Handles the process holds, outside the arena and the report channel.
    pub fn handles(&self) -> usize {
        self.net.borrow().handles.len()
    }
}

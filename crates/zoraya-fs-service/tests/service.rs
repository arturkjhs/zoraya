//! The service's loop over a fake transport, on a fault device.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use zoraya_fs::{Block, BlockDevice, DevError, MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::block::{ChannelDevice, Link, LinkError};
use zoraya_fs_service::{
    Device, HandleKind, Incoming, Interest, MAX_BUFFERS, MAX_OPEN, Outgoing, Service, Transport,
    Wake,
};
use zoraya_fs_testkit::clock::TestClock;
use zoraya_fs_testkit::devices::{FaultDevice, FaultPlan, MemDevice};
use zoraya_proto_block as blk;
use zoraya_proto_file::{self as proto, open_flags, rights, status, write_flags};
use zoraya_protocol::{
    Below, Data, ErrorReply, Flags, HEADER_SIZE, Header, Message, Name, Path, Text, decode, encode,
};

type End = u32;
type Queue = VecDeque<(Vec<u8>, Vec<End>)>;

#[derive(Default)]
struct Net {
    next: End,
    queues: BTreeMap<End, Queue>,
    replies: BTreeMap<End, Queue>,
    closed: BTreeSet<End>,
    kinds: BTreeMap<End, HandleKind>,
    interests: BTreeMap<u64, (End, Interest)>,
    /// Replies a client's queue holds before `Full`.
    room: BTreeMap<End, usize>,
    no_memory: bool,
    reports: Vec<u32>,
    copies_from: u32,
    memory: BTreeMap<u64, Vec<u8>>,
    wake_deadline: bool,
    driver_closed: bool,
    now: u64,
    tokens: BTreeMap<End, End>,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Net>>);

impl Fake {
    fn end(&self, kind: HandleKind) -> End {
        let mut n = self.0.borrow_mut();
        n.next += 1;
        let end = n.next;
        n.kinds.insert(end, kind);
        end
    }

    fn channel(&self) -> End {
        let end = self.end(HandleKind::Channel);
        self.0.borrow_mut().queues.insert(end, VecDeque::new());
        end
    }

    fn send(&self, end: End, bytes: Vec<u8>, handles: Vec<End>) {
        self.0
            .borrow_mut()
            .queues
            .get_mut(&end)
            .unwrap()
            .push_back((bytes, handles));
    }

    fn take_reply(&self, end: End) -> Option<(Vec<u8>, Vec<End>)> {
        self.0.borrow_mut().replies.get_mut(&end)?.pop_front()
    }
}

impl Transport for Fake {
    type End = End;

    fn read(&mut self, end: End, bytes: &mut [u8], handles: &mut Vec<End>) -> Incoming {
        let mut n = self.0.borrow_mut();
        match n.queues.get_mut(&end).and_then(VecDeque::pop_front) {
            Some((message, list)) => {
                bytes[..message.len()].copy_from_slice(&message);
                handles.extend(list);
                Incoming::Message(message.len())
            }
            None if n.closed.contains(&end) => Incoming::Closed,
            None => Incoming::Empty,
        }
    }

    fn write(&mut self, end: End, bytes: &[u8], handles: &[End]) -> Outgoing {
        let mut n = self.0.borrow_mut();
        if n.closed.contains(&end) {
            return Outgoing::Closed;
        }
        if n.no_memory {
            return Outgoing::NoMemory;
        }
        let room = n.room.get(&end).copied().unwrap_or(usize::MAX);
        let queue = n.replies.entry(end).or_default();
        if queue.len() >= room {
            return Outgoing::Full;
        }
        queue.push_back((bytes.to_vec(), handles.to_vec()));
        Outgoing::Sent
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
        self.0
            .borrow()
            .kinds
            .get(&end)
            .copied()
            .unwrap_or(HandleKind::Other)
    }

    fn duplicate(&mut self, end: End) -> Option<End> {
        let copy = self.end(HandleKind::Token);
        let mut n = self.0.borrow_mut();
        let root = n.tokens.get(&end).copied().unwrap_or(end);
        n.tokens.insert(copy, root);
        Some(copy)
    }

    fn new_token(&mut self) -> Option<End> {
        Some(self.end(HandleKind::Token))
    }

    fn same(&mut self, a: End, b: End) -> bool {
        let n = self.0.borrow();
        n.tokens.get(&a).unwrap_or(&a) == n.tokens.get(&b).unwrap_or(&b)
    }

    fn map(&mut self, memory: End, length: u64) -> Option<u64> {
        let mut n = self.0.borrow_mut();
        let address = u64::from(memory) << 32;
        if n.memory.get(&u64::from(memory))?.len() as u64 != length {
            return None;
        }
        let bytes = n.memory.remove(&u64::from(memory))?;
        n.memory.insert(address, bytes);
        Some(address)
    }

    fn unmap(&mut self, _address: u64, _length: u64) {}

    fn copy_from(&mut self, address: u64, out: &mut [u8]) -> bool {
        let mut n = self.0.borrow_mut();
        n.copies_from += 1;
        let (base, memory) = n.memory.range(..=address).next_back().unwrap();
        let at = (address - base) as usize;
        out.copy_from_slice(&memory[at..at + out.len()]);
        true
    }

    fn copy_to(&mut self, address: u64, bytes: &[u8]) -> bool {
        let mut n = self.0.borrow_mut();
        let (base, memory) = n.memory.range_mut(..=address).next_back().unwrap();
        let at = (address - *base) as usize;
        memory[at..at + bytes.len()].copy_from_slice(bytes);
        true
    }

    fn report(&mut self, reason: u32) {
        self.0.borrow_mut().reports.push(reason);
    }

    fn wait(&mut self, _deadline: Option<u64>) -> Wake {
        let mut n = self.0.borrow_mut();
        if std::mem::take(&mut n.driver_closed) {
            return Wake::DriverClosed;
        }
        if std::mem::take(&mut n.wake_deadline) {
            return Wake::Deadline;
        }
        for (key, (end, interest)) in &n.interests {
            let closed = n.closed.contains(end);
            let ready = match interest {
                Interest::Requests => closed || n.queues.get(end).is_some_and(|q| !q.is_empty()),
                Interest::Reply => {
                    let held = n.replies.get(end).map_or(0, VecDeque::len);
                    closed || held < n.room.get(end).copied().unwrap_or(usize::MAX)
                }
                Interest::Closed => closed,
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

/// `FaultDevice` with a lost flag and a flush count.
struct TestDevice {
    inner: FaultDevice<MemDevice>,
    lost: bool,
}

impl BlockDevice for TestDevice {
    fn sector_count(&self) -> u64 {
        self.inner.sector_count()
    }
    fn flush_supported(&self) -> bool {
        self.inner.flush_supported()
    }
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        if self.lost {
            return Err(DevError::IoError);
        }
        self.inner.read(block, buf)
    }
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        if self.lost {
            return Err(DevError::IoError);
        }
        self.inner.write(writes)
    }
    fn flush(&mut self) -> Result<(), DevError> {
        if self.lost {
            return Err(DevError::IoError);
        }
        self.inner.flush()
    }
}

impl Device for TestDevice {
    fn is_lost(&self) -> bool {
        self.lost
    }
    fn mark_lost(&mut self) {
        self.lost = true;
    }
}

type Svc = Service<TestDevice, TestClock, Fake>;

struct Rig {
    net: Fake,
    service: Svc,
    root: End,
    clock: TestClock,
    transaction: u32,
}

const NOW: Timestamp = Timestamp {
    secs: 1000,
    nanos: 0,
};

fn rig_with(plan: FaultPlan) -> Rig {
    let mut device = MemDevice::new(4096);
    mkfs(
        &mut device,
        MkfsOptions {
            volume_id: 1,
            now: NOW,
            root_mode: 0o755,
        },
    )
    .unwrap();
    let clock = TestClock::new(NOW);
    let device = TestDevice {
        inner: FaultDevice::new(device, plan),
        lost: false,
    };
    let volume = Volume::mount(device, clock.clone(), MountOptions::default()).unwrap();
    let net = Fake::default();
    let mut service = Service::new(volume, net.clone());
    let root = net.channel();
    assert!(service.add_root(root));
    Rig {
        net,
        service,
        root,
        clock,
        transaction: 0,
    }
}

fn rig() -> Rig {
    rig_with(FaultPlan::default())
}

const ANY: u32 = rights::ALL;

impl Rig {
    fn run(&mut self) {
        while self.service.step() {}
    }

    /// Sends a message with `handles`, and gives the reply's bytes.
    fn call<'a, M: Message<'a>>(&mut self, end: End, m: &M, handles: Vec<End>) -> Vec<u8> {
        self.post(end, m, handles);
        self.run();
        self.net.take_reply(end).expect("a reply").0
    }

    fn post<'a, M: Message<'a>>(&mut self, end: End, m: &M, handles: Vec<End>) {
        self.transaction += 1;
        let mut out = vec![0; 70_000];
        let length = encode(m, self.transaction, &mut out).unwrap();
        out.truncate(length);
        self.net.send(end, out, handles);
    }

    fn done<'a, M: Message<'a>>(&mut self, end: End, m: &M, handles: Vec<End>) -> (u32, u64) {
        let reply = self.call(end, m, handles);
        let (_, d) = decode::<proto::Done>(&reply, 0).expect("a Done");
        (d.status.0, d.count)
    }

    fn try_open(&mut self, dir: End, path: &str, flags: u32, wanted: u32) -> (u32, End) {
        let end = self.net.channel();
        let m = proto::Open {
            flags: Flags(flags),
            rights: Flags(wanted),
            path: Path(path),
        };
        let reply = self.call(dir, &m, vec![end]);
        let (_, r) = decode::<proto::OpenReply<'_>>(&reply, 0).unwrap();
        (r.status.0, end)
    }

    fn open(&mut self, dir: End, path: &str, flags: u32) -> End {
        let (s, end) = self.try_open(dir, path, flags, ANY);
        assert_eq!(s, status::OK, "open {path}");
        end
    }

    fn write_text(&mut self, end: End, offset: u64, flags: u32, text: &[u8]) -> (u32, u64) {
        let m = proto::Write {
            offset,
            flags: Flags(flags),
            data: Data(text),
        };
        let reply = self.call(end, &m, vec![]);
        let (_, d) = decode::<proto::Done>(&reply, 0).unwrap();
        (d.status.0, d.count)
    }

    fn read_text(&mut self, end: End, offset: u64, length: u32) -> (u32, Vec<u8>) {
        let reply = self.call(end, &proto::Read { offset, length }, vec![]);
        let (_, r) = decode::<proto::ReadReply<'_>>(&reply, 0).unwrap();
        (r.status.0, r.data.0.to_vec())
    }

    fn flushes(&mut self) -> u64 {
        self.service.volume().device().inner.flushes
    }
}

#[test]
fn a_file_is_created_written_and_read() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "a.txt", open_flags::CREATE);
    assert_eq!(r.write_text(f, 0, 0, b"hello"), (status::OK, 5));
    assert_eq!(r.read_text(f, 1, 100), (status::OK, b"ello".to_vec()));
    let reply = r.call(f, &proto::GetAttributes {}, vec![]);
    let (_, a) = decode::<proto::Attributes>(&reply, 0).unwrap();
    assert_eq!((a.size, a.kind.0), (5, proto::kind::FILE));
}

/// INV-0055-1: each reply follows its effect, in order, whichever connection.
#[test]
fn requests_take_effect_one_at_a_time_in_order() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let g = r.open(root, "f", 0);
    for i in 0..20u8 {
        let end = if i.is_multiple_of(2) { f } else { g };
        r.post(
            end,
            &proto::Write {
                offset: u64::from(i),
                flags: Flags(0),
                data: Data(&[i]),
            },
            vec![],
        );
    }
    r.run();
    for end in [f, g] {
        let mut count = 0;
        let mut last = 0;
        while let Some((reply, _)) = r.net.take_reply(end) {
            let (header, _) = Header::decode(&reply).unwrap();
            assert!(header.transaction > last);
            last = header.transaction;
            count += 1;
        }
        assert_eq!(count, 10);
    }
    let (_, data) = r.read_text(f, 0, 100);
    assert_eq!(data, (0..20).collect::<Vec<u8>>());
}

/// INV-0055-2.
#[test]
fn sync_replies_after_the_commit() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"x");
    let before = r.flushes();
    assert_eq!(r.done(root, &proto::Sync {}, vec![]).0, status::OK);
    assert!(r.flushes() > before, "the commit flushed");
}

#[test]
fn a_durable_write_replies_after_its_commit() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let before = r.flushes();
    assert_eq!(r.write_text(f, 0, write_flags::DURABLE, b"x").0, status::OK);
    assert!(r.flushes() > before);
}

/// INV-0055-3, over every request a right guards.
#[test]
fn no_request_succeeds_without_its_right() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let denied = status::ACCESS_DENIED;
    let no_rights = {
        let end = r.net.channel();
        let m = proto::Clone { rights: Flags(0) };
        assert_eq!(r.done(root, &m, vec![end]).0, status::OK);
        end
    };
    let (s, _) = r.try_open(no_rights, "f", 0, rights::READ);
    assert_eq!(s, denied, "open asks for more than the connection has");
    assert_eq!(r.read_text(no_rights, 0, 1).0, denied);
    assert_eq!(r.write_text(no_rights, 0, 0, b"x").0, denied);
    assert_eq!(r.done(no_rights, &proto::Sync {}, vec![]).0, denied);
    assert_eq!(
        r.done(no_rights, &proto::SetSize { size: 1 }, vec![]).0,
        denied
    );
    assert_eq!(
        r.done(no_rights, &proto::Remove { name: Name("f") }, vec![])
            .0,
        denied
    );
    let m = proto::MakeDirectory {
        mode: 0o755,
        name: Name("d"),
    };
    assert_eq!(r.done(no_rights, &m, vec![]).0, denied);
    let token = r.net.end(HandleKind::Token);
    let m = proto::Rename {
        mode: Below(0),
        old: Name("f"),
        new: Name("g"),
    };
    assert_eq!(r.done(no_rights, &m, vec![token]).0, denied);
    let m = proto::Publish {
        mode: Below(0),
        name: Name("g"),
    };
    assert_eq!(r.done(no_rights, &m, vec![token]).0, denied);
    let m = proto::Clone {
        rights: Flags(rights::READ),
    };
    let end = r.net.channel();
    assert_eq!(
        r.done(no_rights, &m, vec![end]).0,
        denied,
        "clone adds none"
    );
    let _ = f;
}

#[test]
fn a_read_only_connection_cannot_write_but_can_read() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"text");
    let (s, ro) = r.try_open(root, "f", 0, rights::READ);
    assert_eq!(s, status::OK);
    assert_eq!(r.read_text(ro, 0, 4).1, b"text");
    assert_eq!(r.write_text(ro, 0, 0, b"x").0, status::ACCESS_DENIED);
}

/// INV-0055-4: a failed timed commit on an idle volume.
#[test]
fn a_failed_timed_commit_reaches_the_supervisor_once() {
    let mut r = rig_with(FaultPlan::default());
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"x");
    r.service.volume().device_mut().inner.plan.fail_flush =
        Some(r.service.volume().device().inner.flushes);
    r.clock.advance_mono(60_000_000_000);
    r.net.0.borrow_mut().wake_deadline = true;
    r.run();
    assert_eq!(r.net.0.borrow().reports, [proto::failure::DEVICE_FLUSH]);
    assert_eq!(r.write_text(f, 1, 0, b"y").0, status::STORAGE_FAILED);
    assert_eq!(
        r.done(root, &proto::Sync {}, vec![]).0,
        status::STORAGE_FAILED
    );
    assert_eq!(
        r.done(root, &proto::Status {}, vec![]).0,
        status::STORAGE_FAILED
    );
    assert_eq!(r.net.0.borrow().reports.len(), 1, "once");
}

/// INV-0054-5: lost when idle and clean, heard once, with reads refused.
#[test]
fn a_lost_driver_fails_an_idle_clean_volume() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"x");
    assert_eq!(r.done(root, &proto::Sync {}, vec![]).0, status::OK);
    r.service.volume().drop_caches();
    r.net.0.borrow_mut().driver_closed = true;
    r.run();
    assert_eq!(r.net.0.borrow().reports, [proto::failure::DEVICE_LOST]);
    assert_eq!(
        r.done(root, &proto::Status {}, vec![]).0,
        status::STORAGE_FAILED
    );
    assert_eq!(r.write_text(f, 0, 0, b"y").0, status::STORAGE_FAILED);
    let (s, _) = r.read_text(f, 0, 1);
    assert_eq!(s, status::STORAGE_FAILED, "an outage, not corruption");
    let (s, count) = r.done(root, &proto::CorruptReads {}, vec![]);
    assert_eq!((s, count), (status::OK, 0), "and not counted as corruption");
}

/// INV-0055-5: the buffer is read once, before anything uses it.
#[test]
fn a_buffer_is_copied_once() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let memory = r.net.end(HandleKind::Memory);
    r.net
        .0
        .borrow_mut()
        .memory
        .insert(u64::from(memory), vec![7; 1 << 20]);
    let (s, size) = r.done(f, &proto::Attach { size: 1 << 20 }, vec![memory]);
    assert_eq!((s, size), (status::OK, 1 << 20));
    let m = proto::WriteBuffer {
        offset: 0,
        buffer_offset: 4096,
        length: 1 << 19,
        flags: Flags(0),
    };
    assert_eq!(r.done(f, &m, vec![]), (status::OK, 1 << 19));
    assert_eq!(r.net.0.borrow().copies_from, 1);
    let m = proto::ReadBuffer {
        offset: 0,
        buffer_offset: 0,
        length: 1 << 19,
    };
    assert_eq!(r.done(f, &m, vec![]), (status::OK, 1 << 19));
    assert_eq!(r.done(f, &proto::Detach {}, vec![]).0, status::OK);
    assert_eq!(
        r.done(f, &proto::Detach {}, vec![]).0,
        status::INVALID_ARGUMENT
    );
}

#[test]
fn a_bad_buffer_range_is_refused() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let memory = r.net.end(HandleKind::Memory);
    r.net
        .0
        .borrow_mut()
        .memory
        .insert(u64::from(memory), vec![0; 4096]);
    r.done(f, &proto::Attach { size: 4096 }, vec![memory]);
    for (buffer_offset, length) in [(4097, 0), (0, 4097), (u64::MAX, 2)] {
        let m = proto::WriteBuffer {
            offset: 0,
            buffer_offset,
            length,
            flags: Flags(0),
        };
        assert_eq!(r.done(f, &m, vec![]).0, status::INVALID_ARGUMENT);
    }
    let second = r.net.end(HandleKind::Memory);
    assert_eq!(
        r.done(f, &proto::Attach { size: 4096 }, vec![second]).0,
        status::INVALID_ARGUMENT,
        "one per connection"
    );
    let channel = r.net.channel();
    let g = r.open(root, "g", open_flags::CREATE);
    assert_eq!(
        r.done(g, &proto::Attach { size: 4096 }, vec![channel]).0,
        status::INVALID_ARGUMENT,
        "not memory"
    );
}

/// INV-0055-6: a client that never reads stalls only itself.
#[test]
fn a_client_that_never_reads_stalls_only_itself() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.net.0.borrow_mut().room.insert(f, 1);
    for _ in 0..3 {
        r.post(f, &proto::Status {}, vec![]);
    }
    r.run();
    assert_eq!(
        r.net.0.borrow().queues[&f].len(),
        1,
        "one replied, one held, and the third is not read"
    );
    assert_eq!(r.done(root, &proto::Status {}, vec![]).0, status::OK);
    // When it reads, it gets the rest.
    r.net.take_reply(f).unwrap();
    r.run();
    r.net.take_reply(f).unwrap();
    r.run();
    r.net.take_reply(f).unwrap();
    r.run();
    assert!(r.net.0.borrow().queues[&f].is_empty());
}

#[test]
fn a_reply_that_meets_no_memory_is_held_and_retried() {
    let mut r = rig();
    let root = r.root;
    r.post(root, &proto::Status {}, vec![]);
    r.net.0.borrow_mut().no_memory = true;
    r.run();
    assert!(r.net.take_reply(root).is_none());
    r.net.0.borrow_mut().no_memory = false;
    r.net.0.borrow_mut().wake_deadline = true;
    r.run();
    assert!(r.net.take_reply(root).is_some());
}

/// INV-0055-7, INV-0055-8.
#[test]
fn the_table_has_a_limit_and_the_connection_survives_it() {
    let mut r = rig();
    let root = r.root;
    let mut last = 0;
    for _ in 1..MAX_OPEN {
        let end = r.net.channel();
        let m = proto::Clone { rights: Flags(ANY) };
        assert_eq!(r.done(root, &m, vec![end]).0, status::OK);
        last = end;
        assert!(r.service.open_count() <= MAX_OPEN);
    }
    assert_eq!(r.service.open_count(), MAX_OPEN);
    let end = r.net.channel();
    let m = proto::Clone { rights: Flags(ANY) };
    assert_eq!(r.done(root, &m, vec![end]).0, status::TOO_MANY_OPEN);
    assert!(r.net.0.borrow().closed.contains(&end), "its handle closed");
    let (s, end) = r.try_open(root, "x", open_flags::CREATE, ANY);
    assert_eq!(s, status::TOO_MANY_OPEN);
    assert!(r.net.0.borrow().closed.contains(&end));
    assert_eq!(r.done(root, &proto::Status {}, vec![]).0, status::OK);
    // Closing one makes room.
    r.net.0.borrow_mut().closed.insert(last);
    r.run();
    assert_eq!(r.service.open_count(), MAX_OPEN - 1);
    let (s, _) = r.try_open(root, "x", open_flags::CREATE, ANY);
    assert_eq!(s, status::OK);
}

#[test]
fn buffers_have_a_limit() {
    let mut r = rig();
    let root = r.root;
    for i in 0..MAX_BUFFERS {
        let f = r.open(root, &format!("f{i}"), open_flags::CREATE);
        let memory = r.net.end(HandleKind::Memory);
        r.net
            .0
            .borrow_mut()
            .memory
            .insert(u64::from(memory), vec![0; 4096]);
        assert_eq!(
            r.done(f, &proto::Attach { size: 4096 }, vec![memory]).0,
            status::OK
        );
    }
    let f = r.open(root, "last", open_flags::CREATE);
    let memory = r.net.end(HandleKind::Memory);
    r.net
        .0
        .borrow_mut()
        .memory
        .insert(u64::from(memory), vec![0; 4096]);
    assert_eq!(
        r.done(f, &proto::Attach { size: 4096 }, vec![memory]).0,
        status::LIMIT_REACHED
    );
    assert_eq!(r.write_text(f, 0, 0, b"inline still works").0, status::OK);
}

/// INV-0055-9.
#[test]
fn a_record_is_not_its_connection() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"shared");
    let index = r
        .net
        .0
        .borrow()
        .interests
        .iter()
        .find(|(_, (e, _))| *e == f)
        .map(|(k, _)| *k as usize)
        .unwrap();
    let second = r.net.channel();
    let other = r.service.attach_second(index, second).unwrap();
    assert_eq!(r.service.record_of(index), r.service.record_of(other));
    r.net.0.borrow_mut().closed.insert(f);
    r.run();
    assert_eq!(r.read_text(second, 0, 6).1, b"shared", "the record stayed");
}

/// A nameless file is freed with its last connection (ADR-0011 rule 7).
#[test]
fn closing_ends_open_state() {
    let mut r = rig();
    let root = r.root;
    let before = r.service.open_count();
    let f = r.open(root, "f", open_flags::CREATE);
    assert_eq!(r.service.open_count(), before + 1);
    r.net.0.borrow_mut().closed.insert(f);
    r.run();
    assert_eq!(r.service.open_count(), before);
}

#[test]
fn an_unknown_ordinal_gets_the_generic_reply_and_the_connection_survives() {
    #[derive(Debug)]
    struct Unknown;
    impl<'a> Message<'a> for Unknown {
        const ORDINAL: u32 = 999;
        const HANDLES: usize = 0;
        fn body(
            &self,
            _: &mut zoraya_protocol::Writer<'_>,
        ) -> Result<(), zoraya_protocol::WireError> {
            Ok(())
        }
        fn parse(_: &mut zoraya_protocol::Reader<'a>) -> Result<Self, zoraya_protocol::WireError> {
            Ok(Unknown)
        }
    }
    let mut r = rig();
    let root = r.root;
    let end = r.net.channel();
    let reply = r.call(root, &Unknown, vec![end]);
    assert_eq!(
        decode::<ErrorReply>(&reply, 0).unwrap().1.status,
        status::UNSUPPORTED
    );
    assert!(r.net.0.borrow().closed.contains(&end), "handles are closed");
    assert_eq!(r.done(root, &proto::Status {}, vec![]).0, status::OK);
}

#[test]
fn a_malformed_message_is_refused_and_a_garbled_one_ends_the_connection() {
    let mut r = rig();
    let root = r.root;
    // A valid header on a body that does not decode.
    let m = proto::Read {
        offset: 0,
        length: 0,
    };
    let mut bytes = vec![0; 128];
    let length = encode(&m, 7, &mut bytes).unwrap();
    bytes.truncate(length - 1);
    bytes[12] -= 1;
    r.net.send(root, bytes, vec![]);
    r.run();
    let reply = r.net.take_reply(root).unwrap().0;
    assert_eq!(
        decode::<ErrorReply>(&reply, 0).unwrap().1.status,
        status::INVALID_ARGUMENT
    );
    // Extra handles.
    let end = r.net.channel();
    let reply = r.call(root, &proto::Status {}, vec![end]);
    assert!(decode::<ErrorReply>(&reply, 0).is_ok());
    assert!(r.net.0.borrow().closed.contains(&end));
    // No header at all.
    r.net.send(root, vec![1, 2, 3], vec![]);
    r.run();
    assert!(r.net.0.borrow().closed.contains(&root));
}

#[test]
fn a_handle_of_the_wrong_kind_is_refused_before_any_effect() {
    let mut r = rig();
    let root = r.root;
    let process = r.net.end(HandleKind::Other);
    let m = proto::Open {
        flags: Flags(open_flags::CREATE),
        rights: Flags(ANY),
        path: Path("never"),
    };
    let reply = r.call(root, &m, vec![process]);
    let (_, o) = decode::<proto::OpenReply<'_>>(&reply, 0).unwrap();
    assert_eq!(o.status.0, status::INVALID_ARGUMENT);
    let (s, _) = r.try_open(root, "never", 0, ANY);
    assert_eq!(s, status::NOT_FOUND, "no file was made");
}

#[test]
fn a_listing_pages_with_cookies() {
    let mut r = rig();
    let root = r.root;
    for i in 0..200 {
        r.open(root, &format!("file{i:03}"), open_flags::CREATE);
    }
    let dir = r.net.channel();
    let m = proto::Clone { rights: Flags(ANY) };
    assert_eq!(r.done(root, &m, vec![dir]).0, status::OK);
    let mut names = Vec::new();
    let mut cookie = 0;
    loop {
        let reply = r.call(dir, &proto::ReadDirectory { cookie }, vec![]);
        let (_, m) = decode::<proto::ReadDirectoryReply<'_>>(&reply, 0).unwrap();
        assert_eq!(m.status.0, status::OK);
        let mut next = 0;
        for e in m.entries.iter() {
            names.push(e.name.0.to_string());
            next = e.next;
        }
        if next == 0 {
            break;
        }
        cookie = next;
    }
    assert_eq!(names.len(), 200);
    assert!(names.windows(2).all(|w| w[0] < w[1]));
    let reply = r.call(dir, &proto::ReadDirectory { cookie: 12345 }, vec![]);
    let (_, m) = decode::<proto::ReadDirectoryReply<'_>>(&reply, 0).unwrap();
    assert_eq!(m.status.0, status::INVALID_ARGUMENT);
}

fn token_of(r: &mut Rig, end: End) -> End {
    r.post(end, &proto::GetToken {}, vec![]);
    r.run();
    let (reply, handles) = r.net.take_reply(end).unwrap();
    decode::<proto::TokenReply>(&reply, 1).unwrap();
    handles[0]
}

#[test]
fn rename_across_directories_uses_a_token() {
    let mut r = rig();
    let root = r.root;
    let m = proto::MakeDirectory {
        mode: 0o755,
        name: Name("d"),
    };
    assert_eq!(r.done(root, &m, vec![]).0, status::OK);
    let d = r.open(root, "d", open_flags::DIRECTORY);
    r.open(root, "f", open_flags::CREATE);
    let token = token_of(&mut r, d);
    let m = proto::Rename {
        mode: Below(proto::rename_mode::NO_REPLACE),
        old: Name("f"),
        new: Name("g"),
    };
    assert_eq!(r.done(root, &m, vec![token]).0, status::OK);
    assert_eq!(r.try_open(root, "f", 0, ANY).0, status::NOT_FOUND);
    assert_eq!(r.try_open(d, "g", 0, ANY).0, status::OK);
    // A token from nowhere.
    let forged = r.net.end(HandleKind::Token);
    let m = proto::Rename {
        mode: Below(0),
        old: Name("x"),
        new: Name("y"),
    };
    assert_eq!(r.done(root, &m, vec![forged]).0, status::INVALID_ARGUMENT);
    let channel = r.net.channel();
    assert_eq!(r.done(root, &m, vec![channel]).0, status::INVALID_ARGUMENT);
}

#[test]
fn an_unnamed_file_is_published() {
    let mut r = rig();
    let root = r.root;
    let token = token_of(&mut r, root);
    let f = r.open(root, "ignored", open_flags::UNNAMED);
    r.write_text(f, 0, 0, b"new");
    let m = proto::Publish {
        mode: Below(proto::publish_mode::NO_REPLACE),
        name: Name("named"),
    };
    assert_eq!(r.done(f, &m, vec![token]).0, status::OK);
    let g = r.open(root, "named", 0);
    assert_eq!(r.read_text(g, 0, 8).1, b"new");
}

#[test]
fn appends_go_to_the_end() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let g = r.open(root, "f", 0);
    r.write_text(f, 0, write_flags::APPEND, b"ab");
    r.write_text(g, 0, write_flags::APPEND, b"cd");
    assert_eq!(r.read_text(f, 0, 8).1, b"abcd");
}

#[test]
fn a_link_is_reported_not_followed() {
    let mut r = rig();
    let root = r.root;
    let m = proto::MakeLink {
        name: Name("l"),
        target: Text("elsewhere"),
    };
    assert_eq!(r.done(root, &m, vec![]).0, status::OK);
    let end = r.net.channel();
    let open = proto::Open {
        flags: Flags(0),
        rights: Flags(ANY),
        path: Path("l"),
    };
    let reply = r.call(root, &open, vec![end]);
    let (_, o) = decode::<proto::OpenReply<'_>>(&reply, 0).unwrap();
    assert_eq!((o.status.0, o.target.0), (status::IS_LINK, "elsewhere"));
    let link = r.open(root, "l", open_flags::NO_FOLLOW);
    let reply = r.call(link, &proto::ReadLink {}, vec![]);
    let (_, l) = decode::<proto::ReadLinkReply<'_>>(&reply, 0).unwrap();
    assert_eq!(l.target.0, "elsewhere");
}

#[test]
fn remove_takes_files_and_empty_directories() {
    let mut r = rig();
    let root = r.root;
    r.open(root, "f", open_flags::CREATE);
    let m = proto::MakeDirectory {
        mode: 0o755,
        name: Name("d"),
    };
    r.done(root, &m, vec![]);
    let d = r.open(root, "d", open_flags::DIRECTORY);
    r.open(d, "inner", open_flags::CREATE);
    let rm = |name| proto::Remove { name: Name(name) };
    assert_eq!(r.done(root, &rm("d"), vec![]).0, status::NOT_EMPTY);
    assert_eq!(r.done(d, &rm("inner"), vec![]).0, status::OK);
    assert_eq!(r.done(root, &rm("d"), vec![]).0, status::OK);
    assert_eq!(r.done(root, &rm("f"), vec![]).0, status::OK);
    assert_eq!(r.done(root, &rm("f"), vec![]).0, status::NOT_FOUND);
}

#[test]
fn an_open_of_a_file_as_a_directory_is_refused() {
    let mut r = rig();
    let root = r.root;
    r.open(root, "f", open_flags::CREATE);
    assert_eq!(
        r.try_open(root, "f", open_flags::DIRECTORY, ANY).0,
        status::NOT_DIRECTORY
    );
}

/// Reads after a corrupt data block stay this read's failure (ADR-0012 rule 3),
/// counted, with the volume healthy.
#[test]
fn a_corrupt_data_block_is_that_read_only() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, &[1; 8192]);
    assert_eq!(r.done(root, &proto::Sync {}, vec![]).0, status::OK);
    r.service.volume().drop_caches();
    // The first read brings the metadata back, so the next read is data alone.
    assert_eq!(r.read_text(f, 0, 4096).0, status::OK);
    let reads = r.service.volume().device().inner.reads;
    r.service.volume().device_mut().inner.plan.fail_read = Some(reads);
    assert_eq!(r.read_text(f, 4096, 4096).0, status::CORRUPT);
    assert_eq!(
        r.done(root, &proto::CorruptReads {}, vec![]),
        (status::OK, 1)
    );
    assert_eq!(r.done(root, &proto::Status {}, vec![]).0, status::OK);
}

// The block adapter.

/// A driver in the test: answers from a memory device.
struct FakeLink {
    device: MemDevice,
    data: Vec<u8>,
    flushes: u32,
    writes: Vec<u32>,
    lost_after: Option<u32>,
    wrong_transaction: bool,
    fail: bool,
}

impl FakeLink {
    fn new() -> FakeLink {
        FakeLink {
            device: MemDevice::new(64),
            data: vec![0; 8 * 4096],
            flushes: 0,
            writes: Vec::new(),
            lost_after: None,
            wrong_transaction: false,
            fail: false,
        }
    }
}

impl Link for FakeLink {
    fn call(&mut self, request: &[u8], reply: &mut [u8]) -> Result<usize, LinkError> {
        if let Some(left) = self.lost_after.as_mut() {
            if *left == 0 {
                return Err(LinkError::Lost);
            }
            *left -= 1;
        }
        let (header, _) = Header::decode(request).unwrap();
        let t = header.transaction + u32::from(self.wrong_transaction);
        let s = u32::from(self.fail);
        let done = |reply: &mut [u8]| encode(&blk::Done { status: Below(s) }, t, reply).unwrap();
        Ok(match header.ordinal {
            1 => encode(
                &blk::InfoReply {
                    status: Below(0),
                    sectors: 64 * 8,
                    flush: true,
                },
                t,
                reply,
            )
            .unwrap(),
            3 => {
                let (_, m) = decode::<blk::Read>(request, 0).unwrap();
                let mut block = [0; 4096];
                self.device.read(m.range.sector / 8, &mut block).unwrap();
                self.data[..4096].copy_from_slice(&block);
                done(reply)
            }
            4 => {
                let (_, m) = decode::<blk::Write<'_>>(request, 0).unwrap();
                self.writes.push(m.ranges.len() as u32);
                for range in m.ranges.iter() {
                    let at = range.offset as usize;
                    let block: &Block = self.data[at..at + 4096].try_into().unwrap();
                    self.device.write(&[(range.sector / 8, block)]).unwrap();
                }
                done(reply)
            }
            5 => {
                self.flushes += 1;
                done(reply)
            }
            other => panic!("ordinal {other}"),
        })
    }

    fn data(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

#[test]
fn the_block_adapter_reads_writes_and_flushes() {
    let mut d = ChannelDevice::new(FakeLink::new()).unwrap();
    assert_eq!(d.sector_count(), 64 * 8);
    assert!(d.flush_supported());
    let block = [9; 4096];
    d.write(&[(3, &block)]).unwrap();
    d.flush().unwrap();
    let mut back = [0; 4096];
    d.read(3, &mut back).unwrap();
    assert_eq!(back, block);
}

#[test]
fn a_large_batch_is_split_to_fit_the_buffer() {
    let mut d = ChannelDevice::new(FakeLink::new()).unwrap();
    let block = [1; 4096];
    let writes: Vec<(u64, &Block)> = (0..20).map(|b| (b, &block)).collect();
    d.write(&writes).unwrap();
    let mut back = [0; 4096];
    d.read(19, &mut back).unwrap();
    assert_eq!(back, block);
}

#[test]
fn a_device_error_is_an_error_and_not_a_loss() {
    let mut link = FakeLink::new();
    link.fail = true;
    let mut d = ChannelDevice::new(link).unwrap();
    assert_eq!(d.flush(), Err(DevError::IoError));
    assert!(!d.is_lost());
}

#[test]
fn a_lost_driver_stays_lost() {
    let mut link = FakeLink::new();
    link.lost_after = Some(1);
    let mut d = ChannelDevice::new(link).unwrap();
    let mut back = [0; 4096];
    assert_eq!(d.read(0, &mut back), Err(DevError::IoError));
    assert!(d.is_lost());
    assert_eq!(d.flush(), Err(DevError::IoError));
}

#[test]
fn a_reply_to_something_else_loses_the_driver() {
    let mut link = FakeLink::new();
    link.wrong_transaction = true;
    let d = ChannelDevice::new(link);
    assert!(d.is_err());
}

/// INV-0080-2, the count.
#[test]
fn restarts_stop_at_three() {
    let mut restarts = zoraya_fs_service::Restarts::new();
    assert!(restarts.take() && restarts.take() && restarts.take());
    assert!(!restarts.take());
    assert!(!restarts.take());
}

#[test]
fn header_size_is_what_the_tests_assume() {
    assert_eq!(HEADER_SIZE, 16);
}

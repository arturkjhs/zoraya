//! The service's loop over a fake transport, on a fault device.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use zoraya_fs::{Block, BlockDevice, DevError, MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::block::{ChannelDevice, Link, LinkError};
use zoraya_fs_service::{
    Device, HandleKind, Incoming, Interest, MAX_BUFFERS, MAX_HELD, MAX_OPEN,
    MAX_UNTRUSTED_ACCOUNTS, Outgoing, Service, Transport, UNTRUSTED_OPEN, Wake, buffers_for,
    held_for, worst_case,
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
    /// Handles the service holds: received, made or copied, and not yet
    /// closed or sent on (INV-0055-7 counts these, not the service's count).
    owned: BTreeSet<End>,
    /// What the service asked each wait for.
    deadlines: Vec<Option<u64>>,
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
                n.owned.extend(list.iter().copied());
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
        for handle in handles {
            n.owned.remove(handle);
        }
        Outgoing::Sent
    }

    fn interest(&mut self, end: End, key: u64, interest: Interest) -> bool {
        self.0.borrow_mut().interests.insert(key, (end, interest));
        true
    }

    fn close(&mut self, end: End) {
        let mut n = self.0.borrow_mut();
        n.owned.remove(&end);
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
        n.owned.insert(copy);
        Some(copy)
    }

    fn new_token(&mut self) -> Option<End> {
        let token = self.end(HandleKind::Token);
        self.0.borrow_mut().owned.insert(token);
        Some(token)
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

    fn wait(&mut self, deadline: Option<u64>) -> Wake {
        let mut n = self.0.borrow_mut();
        n.deadlines.push(deadline);
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
    /// `open_with_mode`'s last reply: created, kind, mode, id and size.
    last_open: (u32, u32, u32, u64, u64),
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
    net.0.borrow_mut().owned.insert(root);
    assert!(service.add_root(root));
    Rig {
        net,
        service,
        root,
        clock,
        transaction: 0,
        last_open: (0, 0, 0, 0, 0),
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

/// Gives connection `c` a page of buffer.
fn attach_page(r: &mut Rig, c: End) {
    let memory = r.net.end(HandleKind::Memory);
    let bytes = vec![0; 4096];
    r.net.0.borrow_mut().memory.insert(u64::from(memory), bytes);
    let m = proto::Attach { size: 4096 };
    assert_eq!(r.done(c, &m, vec![memory]).0, status::OK);
}

/// R-15: a token copy in a held reply is counted until it goes, whether the
/// client reads it or closes first.
#[test]
fn a_held_token_copy_is_counted_until_it_goes() {
    for closes in [false, true] {
        let mut r = rig();
        let root = r.root;
        let f = r.open(root, "f", open_flags::CREATE);
        let held = |r: &Rig| (r.net.0.borrow().owned.len(), r.service.open_count());
        r.net.0.borrow_mut().room.insert(f, 0);
        r.post(f, &proto::GetToken {}, vec![]);
        r.run();
        let (owned, open) = held(&r);
        assert_eq!(owned, open, "held with its copy");
        if closes {
            r.net.0.borrow_mut().closed.insert(f);
        } else {
            r.net.0.borrow_mut().room.insert(f, 1);
        }
        r.run();
        let (after, open) = held(&r);
        assert_eq!(after, open, "after it went");
        // The copy, and if it closed, its connection and token too.
        let gone = if closes { 3 } else { 1 };
        assert_eq!(after, owned - gone, "closes {closes}");
    }
}

/// A reply's status: every reply has it first, after the header.
fn status_of(reply: &[u8]) -> u32 {
    u32::from_le_bytes(reply[16..20].try_into().unwrap())
}

/// INV-0055-3, whole: every request a right guards, against a connection
/// lacking each right it needs, and `clone` never adding one.
#[test]
fn every_guarded_request_needs_each_of_its_rights() {
    use rights::*;
    type Ask = fn(&mut Rig, End) -> u32;
    let cases: &[(&str, bool, u32, Ask)] = &[
        ("open", true, READ, |r, c| r.try_open(c, "f", 0, READ).0),
        ("open create", true, CREATE, |r, c| {
            r.try_open(c, "new", open_flags::CREATE, 0).0
        }),
        ("open truncate", true, WRITE, |r, c| {
            r.try_open(c, "f", open_flags::TRUNCATE, 0).0
        }),
        ("clone", false, CLONE | READ, |r, c| {
            let end = r.net.channel();
            let m = proto::Clone {
                rights: Flags(READ),
            };
            r.done(c, &m, vec![end]).0
        }),
        ("share", false, CLONE | READ, |r, c| {
            let end = r.net.channel();
            let m = proto::Share {
                rights: Flags(READ),
            };
            r.done(c, &m, vec![end]).0
        }),
        ("open with mode", true, READ, |r, c| {
            r.try_open_mode(c, "f", 0, READ, 0, 0o600).0
        }),
        ("open with mode, create", true, CREATE, |r, c| {
            r.try_open_mode(c, "new", open_flags::CREATE, 0, 0, 0o600).0
        }),
        ("open with mode, truncate", true, WRITE, |r, c| {
            r.try_open_mode(c, "f", open_flags::TRUNCATE, 0, 0, 0o600).0
        }),
        ("read current", false, READ, |r, c| r.read_current(c, 1).0),
        ("write current", false, WRITE, |r, c| {
            r.write_current(c, 0, b"x").0
        }),
        ("durable write current", false, WRITE | SYNC, |r, c| {
            r.write_current(c, write_flags::DURABLE, b"x").0
        }),
        ("read buffer current", false, READ, |r, c| {
            attach_page(r, c);
            let m = proto::ReadBufferCurrent {
                buffer_offset: 0,
                length: 16,
            };
            r.done(c, &m, vec![]).0
        }),
        ("write buffer current", false, WRITE, |r, c| {
            attach_page(r, c);
            let m = proto::WriteBufferCurrent {
                buffer_offset: 0,
                length: 16,
                flags: Flags(0),
            };
            r.done(c, &m, vec![]).0
        }),
        (
            "durable write buffer current",
            false,
            WRITE | SYNC,
            |r, c| {
                attach_page(r, c);
                let m = proto::WriteBufferCurrent {
                    buffer_offset: 0,
                    length: 16,
                    flags: Flags(write_flags::DURABLE),
                };
                r.done(c, &m, vec![]).0
            },
        ),
        ("read", false, READ, |r, c| r.read_text(c, 0, 1).0),
        ("write", false, WRITE, |r, c| r.write_text(c, 0, 0, b"x").0),
        ("durable write", false, WRITE | SYNC, |r, c| {
            r.write_text(c, 0, write_flags::DURABLE, b"x").0
        }),
        ("set size", false, WRITE, |r, c| {
            r.done(c, &proto::SetSize { size: 1 }, vec![]).0
        }),
        ("set attributes", false, WRITE, |r, c| {
            let m = proto::SetAttributes {
                which: Flags(1),
                mode: 0o600,
                seconds: 0,
                nanos: 0,
            };
            r.done(c, &m, vec![]).0
        }),
        ("read link", false, READ, |r, c| {
            status_of(&r.call(c, &proto::ReadLink {}, vec![]))
        }),
        ("sync", false, SYNC, |r, c| {
            r.done(c, &proto::Sync {}, vec![]).0
        }),
        ("read buffer", false, READ, |r, c| {
            attach_page(r, c);
            let m = proto::ReadBuffer {
                offset: 0,
                buffer_offset: 0,
                length: 16,
            };
            r.done(c, &m, vec![]).0
        }),
        ("write buffer", false, WRITE, |r, c| {
            attach_page(r, c);
            let m = proto::WriteBuffer {
                offset: 0,
                buffer_offset: 0,
                length: 16,
                flags: Flags(0),
            };
            r.done(c, &m, vec![]).0
        }),
        ("durable write buffer", false, WRITE | SYNC, |r, c| {
            attach_page(r, c);
            let m = proto::WriteBuffer {
                offset: 0,
                buffer_offset: 0,
                length: 16,
                flags: Flags(write_flags::DURABLE),
            };
            r.done(c, &m, vec![]).0
        }),
        ("publish", false, PUBLISH, |r, c| {
            let root = r.root;
            let token = token_of(r, root);
            let m = proto::Publish {
                mode: Below(proto::publish_mode::NO_REPLACE),
                name: Name("named"),
            };
            r.done(c, &m, vec![token]).0
        }),
        // The token's directory needs the right too, not only the asker.
        ("rename's target", true, RENAME, |r, c| {
            let root = r.root;
            let token = token_of(r, c);
            let m = proto::Rename {
                mode: Below(0),
                old: Name("moved"),
                new: Name("moved2"),
            };
            r.done(root, &m, vec![token]).0
        }),
        ("publish's target", true, PUBLISH, |r, c| {
            let root = r.root;
            let token = token_of(r, c);
            let (s, unnamed) = r.try_open(root, "ignored", open_flags::UNNAMED, ANY);
            assert_eq!(s, status::OK);
            let m = proto::Publish {
                mode: Below(proto::publish_mode::NO_REPLACE),
                name: Name("named"),
            };
            r.done(unnamed, &m, vec![token]).0
        }),
        ("read directory", true, READ, |r, c| {
            status_of(&r.call(c, &proto::ReadDirectory { cookie: 0 }, vec![]))
        }),
        ("remove", true, REMOVE, |r, c| {
            r.done(c, &proto::Remove { name: Name("gone") }, vec![]).0
        }),
        ("make directory", true, CREATE, |r, c| {
            let m = proto::MakeDirectory {
                mode: 0o755,
                name: Name("d"),
            };
            r.done(c, &m, vec![]).0
        }),
        ("make link", true, CREATE, |r, c| {
            let m = proto::MakeLink {
                name: Name("l"),
                target: Text("f"),
            };
            r.done(c, &m, vec![]).0
        }),
        ("rename", true, RENAME, |r, c| {
            let root = r.root;
            let token = token_of(r, root);
            let m = proto::Rename {
                mode: Below(0),
                old: Name("moved"),
                new: Name("moved2"),
            };
            r.done(c, &m, vec![token]).0
        }),
    ];
    let denied = status::ACCESS_DENIED;
    for &(name, directory, needed, ask) in cases {
        let mut bits = needed;
        while bits != 0 {
            let right = bits.isolate_lowest_one();
            bits &= !right;
            let mut r = rig();
            let root = r.root;
            let f = r.open(root, "f", open_flags::CREATE);
            r.open(root, "gone", open_flags::CREATE);
            r.open(root, "moved", open_flags::CREATE);
            let base = if directory { root } else { f };
            let end = r.net.channel();
            let m = proto::Clone {
                rights: Flags(ANY & !right),
            };
            assert_eq!(r.done(base, &m, vec![end]).0, status::OK);
            let before = r.flushes();
            assert_eq!(ask(&mut r, end), denied, "{name} without {right:#x}");
            assert_eq!(r.flushes(), before, "{name}: a refusal commits nothing");
            assert_eq!(r.read_text(f, 0, 8).1, b"", "{name}: f is untouched");
            // With every right, it is not refused for want of one.
            let mut r = rig();
            let root = r.root;
            let f = r.open(root, "f", open_flags::CREATE);
            r.open(root, "gone", open_flags::CREATE);
            r.open(root, "moved", open_flags::CREATE);
            let base = if directory { root } else { f };
            assert_ne!(ask(&mut r, base), denied, "{name} with every right");
        }
    }
    // A connection never gets more than it has, by `clone` or `open`.
    let mut r = rig();
    let root = r.root;
    let narrow = r.net.channel();
    let m = proto::Clone {
        rights: Flags(CLONE | READ),
    };
    assert_eq!(r.done(root, &m, vec![narrow]).0, status::OK);
    for wanted in [ANY, CLONE | READ | WRITE, WRITE] {
        let end = r.net.channel();
        let m = proto::Clone {
            rights: Flags(wanted),
        };
        assert_eq!(r.done(narrow, &m, vec![end]).0, denied, "{wanted:#x}");
    }
    let (s, _) = r.try_open(narrow, "f", open_flags::CREATE, READ);
    assert_eq!(s, denied, "create through a connection without it");
}

impl Rig {
    #[allow(clippy::too_many_arguments)] // Reason: one per field of the message.
    fn try_open_mode(
        &mut self,
        dir: End,
        path: &str,
        flags: u32,
        wanted: u32,
        description: u32,
        mode: u32,
    ) -> (u32, End) {
        let end = self.net.channel();
        let m = proto::OpenWithMode {
            flags: Flags(flags),
            rights: Flags(wanted),
            description: Flags(description),
            mode,
            path: Path(path),
        };
        let reply = self.call(dir, &m, vec![end]);
        let (_, r) = decode::<proto::OpenWithModeReply<'_>>(&reply, 0).unwrap();
        self.last_open = (r.created.0, r.kind.0, r.mode, r.id, r.size);
        (r.status.0, end)
    }

    fn share(&mut self, end: End, wanted: u32) -> (u32, End) {
        let new = self.net.channel();
        let m = proto::Share {
            rights: Flags(wanted),
        };
        (self.done(end, &m, vec![new]).0, new)
    }

    fn read_current(&mut self, end: End, length: u32) -> (u32, Vec<u8>) {
        let reply = self.call(end, &proto::ReadCurrent { length }, vec![]);
        let (_, r) = decode::<proto::ReadReply<'_>>(&reply, 0).unwrap();
        (r.status.0, r.data.0.to_vec())
    }

    fn write_current(&mut self, end: End, flags: u32, text: &[u8]) -> (u32, u64) {
        let m = proto::WriteCurrent {
            flags: Flags(flags),
            data: Data(text),
        };
        self.done(end, &m, vec![])
    }

    fn seek(&mut self, end: End, whence: u32, offset: i64) -> (u32, u64) {
        let m = proto::Seek {
            whence: Below(whence),
            offset: offset as u64,
        };
        self.done(end, &m, vec![])
    }

    fn mode_of(&mut self, end: End) -> u32 {
        let reply = self.call(end, &proto::GetAttributes {}, vec![]);
        decode::<proto::Attributes>(&reply, 0).unwrap().1.mode
    }
}

/// INV-0103-5 with INV-0055-3: `share` gives exactly the rights asked, and
/// never one its connection lacked, for every right a connection can hold.
#[test]
fn share_never_yields_a_right_its_connection_lacked() {
    use rights::*;
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    for held in 0..=ANY {
        let (s, narrow) = {
            let end = r.net.channel();
            let m = proto::Clone {
                rights: Flags(held),
            };
            (r.done(f, &m, vec![end]).0, end)
        };
        assert_eq!(s, status::OK);
        for wanted in [0, READ, WRITE, READ | WRITE | CLONE, ANY, held] {
            let (s, shared) = r.share(narrow, wanted);
            let allowed = held & CLONE != 0 && wanted & !held == 0;
            if !allowed {
                assert_eq!(
                    s,
                    status::ACCESS_DENIED,
                    "held {held:#x} wanted {wanted:#x}"
                );
                continue;
            }
            assert_eq!(s, status::OK, "held {held:#x} wanted {wanted:#x}");
            // The new connection has what it asked and nothing more.
            let read = r.read_text(shared, 0, 1).0;
            assert_eq!(read == status::ACCESS_DENIED, wanted & READ == 0);
            let wrote = r.write_current(shared, 0, b"").0;
            assert_eq!(wrote == status::ACCESS_DENIED, wanted & WRITE == 0);
            let again = r.share(shared, 0).0;
            assert_eq!(again == status::ACCESS_DENIED, wanted & CLONE == 0);
            r.net.0.borrow_mut().closed.insert(shared);
        }
        r.net.0.borrow_mut().closed.insert(narrow);
        r.run();
    }
}

/// INV-0103-1: connections of one description see one position and one
/// append flag; `clone` and a second `open` make descriptions of their own.
#[test]
fn shared_connections_see_one_position() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let (s, g) = r.share(f, ANY);
    assert_eq!(s, status::OK);
    assert_eq!(r.write_current(f, 0, b"abc"), (status::OK, 3));
    assert_eq!(r.write_current(g, 0, b"def"), (status::OK, 3));
    assert_eq!(r.read_text(f, 0, 100).1, b"abcdef");
    assert_eq!(r.seek(g, proto::whence::CURRENT, 0), (status::OK, 6));
    assert_eq!(r.seek(f, proto::whence::START, 1), (status::OK, 1));
    assert_eq!(r.read_current(g, 2), (status::OK, b"bc".to_vec()));
    assert_eq!(r.read_current(f, 100), (status::OK, b"def".to_vec()));
    assert_eq!(r.read_current(g, 100), (status::OK, vec![]), "at the end");
    // A clone is a description of its own, with its own position.
    let clone = {
        let end = r.net.channel();
        let m = proto::Clone { rights: Flags(ANY) };
        assert_eq!(r.done(f, &m, vec![end]).0, status::OK);
        end
    };
    assert_eq!(r.seek(clone, proto::whence::CURRENT, 0), (status::OK, 0));
    assert_eq!(r.read_current(clone, 1), (status::OK, b"a".to_vec()));
    assert_eq!(r.seek(f, proto::whence::CURRENT, 0), (status::OK, 6));
    // The append flag is the description's, for both connections.
    let m = proto::SetFlags {
        flags: Flags(proto::description_flags::APPEND),
    };
    assert_eq!(r.done(g, &m, vec![]).0, status::OK);
    assert_eq!(r.seek(f, proto::whence::START, 0).0, status::OK);
    assert_eq!(r.write_current(f, 0, b"!"), (status::OK, 1));
    assert_eq!(r.read_text(f, 0, 100).1, b"abcdef!");
    assert_eq!(r.seek(g, proto::whence::CURRENT, 0), (status::OK, 7));
    let flags = |r: &mut Rig, end| r.done(end, &proto::GetFlags {}, vec![]).1;
    assert_eq!(
        flags(&mut r, f),
        u64::from(proto::description_flags::APPEND)
    );
    assert_eq!(flags(&mut r, clone), 0, "the clone's description");
    // An explicit offset moves nothing, appending or not.
    assert_eq!(r.write_text(f, 0, 0, b"A").0, status::OK);
    assert_eq!(r.seek(g, proto::whence::CURRENT, 0), (status::OK, 7));
}

/// INV-0103-1's model: random requests at the position over three
/// connections of one description and one of another, against a model of
/// both positions and the file; then the description ends with its last
/// connection, whichever order they close in.
#[test]
fn interleaved_requests_at_the_position_lose_no_update() {
    for seed in 1..=40u64 {
        let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = |n: u64| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng % n
        };
        let mut r = rig();
        let root = r.root;
        let first = r.open(root, "f", open_flags::CREATE);
        let mut ends = vec![first];
        for _ in 0..2 {
            let (s, end) = r.share(first, ANY);
            assert_eq!(s, status::OK);
            ends.push(end);
        }
        let other = r.open(root, "f", 0);
        let descriptions = r.service.descriptions();
        let mut file: Vec<u8> = Vec::new();
        let mut shared = 0u64;
        let mut own = 0u64;
        let mut append = false;
        for step in 0..120 {
            let which = next(4) as usize;
            let (end, position) = if which == 3 {
                (other, &mut own)
            } else {
                (ends[which], &mut shared)
            };
            match next(5) {
                0 | 1 => {
                    let n = 1 + next(5) as usize;
                    let data: Vec<u8> = (0..n).map(|k| (step * 7 + k) as u8).collect();
                    let at = if append && which != 3 {
                        file.len()
                    } else {
                        *position as usize
                    };
                    assert_eq!(r.write_current(end, 0, &data), (status::OK, n as u64));
                    if file.len() < at + n {
                        file.resize(at + n, 0);
                    }
                    file[at..at + n].copy_from_slice(&data);
                    *position = (at + n) as u64;
                }
                2 => {
                    let n = 1 + next(6) as u32;
                    let (s, got) = r.read_current(end, n);
                    assert_eq!(s, status::OK);
                    let at = (*position as usize).min(file.len());
                    let want = &file[at..(at + n as usize).min(file.len())];
                    assert_eq!(got, want, "seed {seed} step {step}");
                    *position += got.len() as u64;
                }
                3 => {
                    let to = next(file.len() as u64 + 2);
                    assert_eq!(
                        r.seek(end, proto::whence::START, to as i64),
                        (status::OK, to)
                    );
                    *position = to;
                }
                _ if which != 3 => {
                    append = !append;
                    let flags = if append {
                        proto::description_flags::APPEND
                    } else {
                        0
                    };
                    let m = proto::SetFlags {
                        flags: Flags(flags),
                    };
                    assert_eq!(r.done(end, &m, vec![]).0, status::OK);
                }
                _ => {
                    let (s, at) = r.seek(end, proto::whence::END, 0);
                    assert_eq!((s, at), (status::OK, file.len() as u64));
                    *position = at;
                }
            }
            for &end in &ends {
                assert_eq!(r.seek(end, proto::whence::CURRENT, 0).1, shared);
            }
        }
        assert_eq!(r.read_text(other, 0, 4096).1, file);
        // Closes in a seeded order: the description lives while one remains.
        let mut order = ends.clone();
        for k in (1..order.len()).rev() {
            order.swap(k, next(k as u64 + 1) as usize);
        }
        for (k, end) in order.iter().enumerate() {
            r.net.0.borrow_mut().closed.insert(*end);
            r.run();
            let left = order.len() - k - 1;
            let expected = if left == 0 {
                descriptions - 1
            } else {
                descriptions
            };
            assert_eq!(r.service.descriptions(), expected, "seed {seed}");
            if let Some(&live) = order.get(k + 1) {
                assert_eq!(r.seek(live, proto::whence::CURRENT, 0).1, shared);
            }
        }
    }
}

/// `seek` from each origin, and a seek to a place no file has is refused
/// and moves nothing.
#[test]
fn seek_moves_the_position_or_nothing() {
    use proto::whence::*;
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    assert_eq!(r.write_current(f, 0, b"0123456789"), (status::OK, 10));
    assert_eq!(r.seek(f, START, 3), (status::OK, 3));
    assert_eq!(r.seek(f, CURRENT, 2), (status::OK, 5));
    assert_eq!(r.seek(f, CURRENT, -5), (status::OK, 0));
    assert_eq!(r.seek(f, END, -1), (status::OK, 9));
    assert_eq!(r.seek(f, END, 5), (status::OK, 15), "past the end");
    let bad = status::INVALID_ARGUMENT;
    assert_eq!(r.seek(f, CURRENT, -16).0, bad);
    assert_eq!(r.seek(f, START, -1).0, bad);
    assert_eq!(r.seek(f, END, i64::MAX).0, bad);
    assert_eq!(r.seek(f, CURRENT, i64::MIN).0, bad);
    assert_eq!(r.seek(f, START, i64::MAX), (status::OK, i64::MAX as u64));
    assert_eq!(r.seek(f, CURRENT, 1).0, bad, "one past the largest");
    assert_eq!(
        r.write_current(f, 0, b"x").0,
        bad,
        "no write past the largest"
    );
    assert_eq!(r.seek(f, CURRENT, 0), (status::OK, i64::MAX as u64));
    // A write may end exactly at the largest, as the engine allows.
    assert_eq!(
        r.seek(f, START, i64::MAX - 1),
        (status::OK, i64::MAX as u64 - 1)
    );
    assert_eq!(
        r.write_current(f, 0, b"z"),
        (status::OK, 1),
        "ends at i64::MAX"
    );
    assert_eq!(r.seek(f, CURRENT, 0), (status::OK, i64::MAX as u64));
    assert_eq!(r.seek(f, START, 15), (status::OK, 15));
    assert_eq!(r.write_current(f, 0, b"x"), (status::OK, 1));
    assert_eq!(r.read_text(f, 9, 7).1, b"9\0\0\0\0\0x");
}

/// ADR-0014 rule 6 through the protocol: the mode of a create is the one
/// asked, an existing file keeps its own, and the description starts with
/// the flags asked.
#[test]
fn open_with_mode_creates_with_that_mode() {
    let mut r = rig();
    let root = r.root;
    let (s, f) = r.try_open_mode(root, "run", open_flags::CREATE, ANY, 0, 0o755);
    assert_eq!(s, status::OK);
    assert_eq!(r.mode_of(f), 0o755);
    let (created, kind, mode, id, size) = r.last_open;
    assert_eq!(
        (created, kind, mode, size),
        (1, proto::kind::FILE, 0o755, 0)
    );
    assert_eq!(r.write_text(f, 0, 0, b"abc").0, status::OK);
    let (s, g) = r.try_open_mode(root, "run", open_flags::CREATE, ANY, 0, 0o600);
    assert_eq!(s, status::OK);
    assert_eq!(r.mode_of(g), 0o755, "an existing file keeps its mode");
    assert_eq!(
        r.last_open,
        (0, proto::kind::FILE, 0o755, id, 3),
        "not made by this open"
    );
    let m = proto::MakeDirectory {
        mode: 0o750,
        name: Name("d"),
    };
    assert_eq!(r.done(root, &m, vec![]).0, status::OK);
    let (s, _) = r.try_open_mode(root, "d", open_flags::DIRECTORY, rights::READ, 0, 0);
    assert_eq!(s, status::OK);
    let (created, kind, mode, _, _) = r.last_open;
    assert_eq!((created, kind, mode), (0, proto::kind::DIRECTORY, 0o750));
    let (s, _) = r.try_open_mode(root, "missing", 0, 0, 0, 0);
    assert_eq!((s, r.last_open), (status::NOT_FOUND, (0, 0, 0, 0, 0)));
    let native = r.open(root, "native", open_flags::CREATE);
    assert_eq!(r.mode_of(native), 0o644, "a native create");
    let (s, u) = r.try_open_mode(root, "x", open_flags::UNNAMED, ANY, 0, 0o700);
    assert_eq!(s, status::OK);
    assert_eq!(r.mode_of(u), 0o700, "an unnamed create");
    let (s, _) = r.try_open_mode(root, "big", open_flags::CREATE, ANY, 0, 0x1_0000);
    assert_eq!(
        s,
        status::INVALID_ARGUMENT,
        "a mode wider than the stored word"
    );
    let (s, _) = r.try_open(root, "big", 0, ANY);
    assert_eq!(s, status::NOT_FOUND, "and the refusal made nothing");
    let append = proto::description_flags::APPEND;
    let (s, a) = r.try_open_mode(root, "run", 0, ANY, append, 0);
    assert_eq!(s, status::OK);
    assert_eq!(r.write_current(a, 0, b"ab"), (status::OK, 2));
    assert_eq!(r.write_current(a, 0, b"cd"), (status::OK, 2));
    assert_eq!(r.seek(a, proto::whence::START, 0).0, status::OK);
    assert_eq!(r.write_current(a, 0, b"e"), (status::OK, 1));
    assert_eq!(
        r.read_text(a, 0, 10).1,
        b"abcabcde",
        "after the earlier abc"
    );
}

/// The buffer requests at the position move it as the inline ones do.
#[test]
fn buffer_requests_at_the_position_move_it() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    attach_page(&mut r, f);
    let address = r.net.0.borrow().memory.keys().copied().max().unwrap();
    r.net.0.borrow_mut().memory.get_mut(&address).unwrap()[..8].copy_from_slice(b"ABCDEFGH");
    let w = proto::WriteBufferCurrent {
        buffer_offset: 0,
        length: 8,
        flags: Flags(0),
    };
    assert_eq!(r.done(f, &w, vec![]), (status::OK, 8));
    assert_eq!(r.done(f, &w, vec![]), (status::OK, 8));
    assert_eq!(r.seek(f, proto::whence::CURRENT, 0), (status::OK, 16));
    assert_eq!(r.seek(f, proto::whence::START, 4).0, status::OK);
    let m = proto::ReadBufferCurrent {
        buffer_offset: 100,
        length: 6,
    };
    assert_eq!(r.done(f, &m, vec![]), (status::OK, 6));
    assert_eq!(r.seek(f, proto::whence::CURRENT, 0), (status::OK, 10));
    let page = r.net.0.borrow().memory.get(&address).unwrap().clone();
    assert_eq!(&page[100..106], b"EFGHAB");
    assert_eq!(r.done(f, &m, vec![]), (status::OK, 6));
    assert_eq!(r.done(f, &m, vec![]), (status::OK, 0), "at the end");
    assert_eq!(r.seek(f, proto::whence::CURRENT, 0), (status::OK, 16));
}

/// `share` is held to `MAX_OPEN` as every request that brings a handle is
/// (INV-0055-8), and the connection it came on goes on working.
#[test]
fn share_has_the_table_limit() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let mut refused = None;
    for _ in 0..MAX_OPEN {
        let (s, end) = r.share(f, ANY);
        if s != status::OK {
            refused = Some((s, end));
            break;
        }
    }
    let (s, end) = refused.expect("a limit");
    assert_eq!(s, status::TOO_MANY_OPEN);
    assert!(r.net.0.borrow().closed.contains(&end), "its handle closed");
    assert_eq!(r.service.open_count(), MAX_OPEN);
    assert_eq!(r.write_current(f, 0, b"x"), (status::OK, 1));
}

/// ADR-0010 rule 2: a refused open changes nothing.
#[test]
fn an_open_as_a_directory_never_creates_or_truncates() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"precious");
    for flags in [
        open_flags::CREATE | open_flags::DIRECTORY,
        open_flags::TRUNCATE | open_flags::DIRECTORY,
        open_flags::UNNAMED | open_flags::DIRECTORY,
    ] {
        let (s, _) = r.try_open(root, "f", flags, ANY);
        assert_eq!(s, status::INVALID_ARGUMENT);
        let (s, _) = r.try_open(root, "new", flags, ANY);
        assert_eq!(s, status::INVALID_ARGUMENT);
    }
    assert_eq!(r.read_text(f, 0, 16).1, b"precious");
    assert_eq!(r.try_open(root, "new", 0, ANY).0, status::NOT_FOUND);
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

/// R-14: a failed volume has no commit to wait for, so the loop sleeps.
#[test]
fn a_failed_volume_does_not_wake_the_loop() {
    let mut r = rig_with(FaultPlan::default());
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"x");
    r.service.volume().device_mut().inner.plan.fail_flush =
        Some(r.service.volume().device().inner.flushes);
    r.clock.advance_mono(60_000_000_000);
    r.net.0.borrow_mut().wake_deadline = true;
    r.run();
    assert!(r.service.volume().failure().is_some());
    r.net.0.borrow_mut().deadlines.clear();
    r.done(root, &proto::Status {}, vec![]);
    assert!(r.net.0.borrow().deadlines.iter().all(Option::is_none));
}

/// ADR-0010 rule 5 under load: requests always ready still let the timed
/// commit run.
#[test]
fn a_busy_loop_still_commits_in_time() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"x");
    let before = r.flushes();
    r.clock.advance_mono(6_000_000_000);
    // Never a `Deadline` wake: there is always a request.
    r.done(root, &proto::Status {}, vec![]);
    assert!(r.flushes() > before, "committed between requests");
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
    // The root's token, kept by the service: one more handle in the table.
    let reply = r.call(root, &proto::GetToken {}, vec![]);
    assert_eq!(status_of(&reply), status::OK);
    let mut last = 0;
    for _ in 2..MAX_OPEN {
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
    // A token's copy is a handle too: none at the limit, though the token is
    // already made.
    let reply = r.call(root, &proto::GetToken {}, vec![]);
    assert_eq!(status_of(&reply), status::TOO_MANY_OPEN);
    // Closing one makes room.
    r.net.0.borrow_mut().closed.insert(last);
    r.run();
    assert_eq!(r.service.open_count(), MAX_OPEN - 1);
    let (s, _) = r.try_open(root, "x", open_flags::CREATE, ANY);
    assert_eq!(s, status::OK);
}

/// INV-0055-7: a model of random `open`, `clone`, `attach` and closes.
#[test]
fn the_table_never_passes_its_limit_under_random_use() {
    let mut r = rig();
    let root = r.root;
    r.done(root, &proto::Sync {}, vec![]);
    let (s, first) = r.try_open(root, "shared", open_flags::CREATE, ANY);
    assert_eq!(s, status::OK);
    let mut live = vec![first];
    let mut peak = 0;
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..12_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let at = (state >> 8) as usize % live.len().max(1);
        match state % 8 {
            // Mostly growth, so that it reaches the limit, and stays there.
            0..=2 => {
                let (s, end) = r.try_open(root, "shared", 0, ANY);
                match s {
                    status::OK => live.push(end),
                    status::TOO_MANY_OPEN => assert!(r.net.0.borrow().closed.contains(&end)),
                    other => panic!("{other}"),
                }
            }
            // A token, to a client that sometimes closes before its reply.
            3 if !live.is_empty() && state & 0x100 != 0 => {
                let end = live[at];
                r.post(end, &proto::GetToken {}, vec![]);
                if state & 0x200 != 0 {
                    live.swap_remove(at);
                    r.net.0.borrow_mut().closed.insert(end);
                }
                r.run();
                // The client takes the copy, which is then its own.
                let _ = r.net.take_reply(end);
            }
            3 | 4 if !live.is_empty() => {
                let end = r.net.channel();
                let m = proto::Clone { rights: Flags(ANY) };
                match r.done(live[at], &m, vec![end]).0 {
                    status::OK => live.push(end),
                    status::TOO_MANY_OPEN => {}
                    other => panic!("{other}"),
                }
            }
            5 if !live.is_empty() => {
                let memory = r.net.end(HandleKind::Memory);
                let bytes = vec![0; 4096];
                r.net.0.borrow_mut().memory.insert(u64::from(memory), bytes);
                r.done(live[at], &proto::Attach { size: 4096 }, vec![memory]);
            }
            6 | 7 if !live.is_empty() => {
                let end = live.swap_remove(at);
                r.net.0.borrow_mut().closed.insert(end);
                r.run();
            }
            _ => {}
        }
        let owned = r.net.0.borrow().owned.len();
        assert_eq!(owned, r.service.open_count(), "the count is what is held");
        assert!(owned <= MAX_OPEN);
        peak = peak.max(owned);
    }
    assert_eq!(peak, MAX_OPEN, "the run must reach the limit");
    // Attached buffers count too, and they go with the last connection.
    r.net.0.borrow_mut().closed.extend(live);
    r.run();
    assert_eq!(r.service.open_count(), 1);
    assert_eq!(r.net.0.borrow().owned.len(), 1, "only the root is held");
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

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
zoraya_test_alloc::cap_memory!();

/// `stat` gives the object's id, which no other object has, the volume's id
/// and the times as the engine holds them.
#[test]
fn stat_names_the_object_and_its_volume() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let g = r.open(root, "g", open_flags::CREATE);
    assert_eq!(r.write_text(f, 0, 0, b"12345").0, status::OK);
    let stat = |r: &mut Rig, end| {
        let reply = r.call(end, &proto::Stat {}, vec![]);
        decode::<proto::StatReply>(&reply, 0).unwrap().1
    };
    let (a, b, d) = (stat(&mut r, f), stat(&mut r, g), stat(&mut r, root));
    assert_eq!(a.status.0, status::OK);
    assert_eq!(
        (a.volume, b.volume, d.volume),
        (1, 1, 1),
        "mkfs's volume id"
    );
    assert_ne!(a.id, b.id);
    assert_ne!(a.id, d.id);
    assert_eq!((a.size, a.kind.0, a.mode), (5, proto::kind::FILE, 0o644));
    assert_eq!(d.kind.0, proto::kind::DIRECTORY);
    assert!(a.modified_seconds as i64 >= NOW.secs);
    // The same object through another connection is the same id.
    let again = r.open(root, "f", 0);
    assert_eq!(stat(&mut r, again).id, a.id);
    // A time set explicitly comes back exactly; set_attributes takes no
    // negative seconds (a stage 4 limit, unchanged).
    let m = proto::SetAttributes {
        which: Flags(2),
        mode: 0,
        seconds: (-2i64) as u64,
        nanos: 7,
    };
    assert_eq!(
        r.done(f, &m, vec![]).0,
        status::INVALID_ARGUMENT,
        "a u64 past i64"
    );
    let m = proto::SetAttributes {
        which: Flags(2),
        mode: 0,
        seconds: 86_400,
        nanos: 7,
    };
    assert_eq!(r.done(f, &m, vec![]).0, status::OK);
    let a = stat(&mut r, f);
    assert_eq!((a.modified_seconds, a.modified_nanos), (86_400, 7));
}

/// A durable write at the position replies after its commit, as `write`'s
/// does (ADR-0010 rule 4).
#[test]
fn a_durable_write_at_the_position_replies_after_its_commit() {
    let mut r = rig();
    let root = r.root;
    let f = r.open(root, "f", open_flags::CREATE);
    let before = r.flushes();
    assert_eq!(r.write_current(f, 0, b"x"), (status::OK, 1));
    assert_eq!(r.flushes(), before, "a plain write commits nothing");
    assert_eq!(
        r.write_current(f, write_flags::DURABLE, b"y"),
        (status::OK, 1)
    );
    assert!(r.flushes() > before, "the durable one did");
}

/// `read_directory_ids` gives each entry's object id, the one `stat` gives,
/// pages with the same cookies as `read_directory`, and needs `READ`.
#[test]
fn a_listing_with_ids_names_each_object() {
    let mut r = rig();
    let root = r.root;
    let mut want = std::collections::BTreeMap::new();
    for i in 0..100 {
        let name = format!("f{i:03}");
        let f = r.open(root, &name, open_flags::CREATE);
        let reply = r.call(f, &proto::Stat {}, vec![]);
        want.insert(name, decode::<proto::StatReply>(&reply, 0).unwrap().1.id);
    }
    let mut got = std::collections::BTreeMap::new();
    let mut cookie = 0;
    loop {
        let reply = r.call(root, &proto::ReadDirectoryIds { cookie }, vec![]);
        let (_, m) = decode::<proto::ReadDirectoryIdsReply<'_>>(&reply, 0).unwrap();
        assert_eq!(m.status.0, status::OK);
        let mut next = 0;
        for e in m.entries.iter() {
            got.insert(e.name.0.to_string(), e.id);
            next = e.next;
        }
        if next == 0 {
            break;
        }
        cookie = next;
    }
    assert_eq!(got, want);
    let narrow = r.net.channel();
    let m = proto::Clone {
        rights: Flags(rights::CLONE),
    };
    assert_eq!(r.done(root, &m, vec![narrow]).0, status::OK);
    let reply = r.call(narrow, &proto::ReadDirectoryIds { cookie: 0 }, vec![]);
    let (_, m) = decode::<proto::ReadDirectoryIdsReply<'_>>(&reply, 0).unwrap();
    assert_eq!(m.status.0, status::ACCESS_DENIED);
}

/// A page holds MAX_ENTRIES names however long they are: 64 names of 255
/// bytes are about 18 KiB, inside the 32 KiB a page allows.
#[test]
fn a_page_of_the_longest_names_is_full() {
    let mut r = rig();
    let root = r.root;
    for i in 0..proto::MAX_ENTRIES {
        let name = format!("{i:03}{}", "n".repeat(252));
        r.open(root, &name, open_flags::CREATE);
    }
    let reply = r.call(root, &proto::ReadDirectoryIds { cookie: 0 }, vec![]);
    let (_, m) = decode::<proto::ReadDirectoryIdsReply<'_>>(&reply, 0).unwrap();
    assert_eq!(m.entries.iter().count(), proto::MAX_ENTRIES);
    assert!(m.entries.iter().all(|e| e.name.0.len() == 255));
}

fn narrow(r: &mut Rig, dir: End, name: &str, wanted: u32) -> (u32, End) {
    let end = r.net.channel();
    let m = proto::Narrow {
        rights: Flags(wanted),
        name: Name(name),
    };
    (r.done(dir, &m, vec![end]).0, end)
}

/// ADR-0055's note (ADR-0153 rule 2): a narrowed connection names its one
/// name and nothing else, cannot list, cannot widen, and its clones and
/// shares stay narrowed; "create or replace" works through it.
#[test]
fn a_narrowed_connection_names_only_its_name() {
    let mut r = rig();
    let root = r.root;
    let other = r.open(root, "other.txt", open_flags::CREATE);
    r.write_text(other, 0, 0, b"keep");
    let create = rights::READ | rights::WRITE | rights::CREATE | rights::SYNC | rights::CLONE;
    let (s, n) = narrow(&mut r, root, "new.txt", create);
    assert_eq!(s, status::OK);
    // Create, then replace by truncation.
    let flags = open_flags::CREATE | open_flags::TRUNCATE;
    let rw = rights::READ | rights::WRITE;
    let (s, f) = r.try_open(n, "new.txt", flags, rw);
    assert_eq!(s, status::OK);
    assert_eq!(r.write_text(f, 0, 0, b"first").0, status::OK);
    let (s, f) = r.try_open(n, "new.txt", flags, rw);
    assert_eq!(s, status::OK);
    assert_eq!(r.write_text(f, 0, 0, b"2").0, status::OK);
    assert_eq!(r.read_text(f, 0, 10), (status::OK, b"2".to_vec()));
    // Nothing else, nor the directory itself, nor below the name.
    let denied = status::ACCESS_DENIED;
    // (`..` and the empty path do not pass the wire: `Path` refuses them.)
    for path in ["other.txt", "new.txt/x"] {
        assert_eq!(r.try_open(n, path, 0, rights::READ).0, denied, "{path:?}");
    }
    let mk = proto::MakeDirectory {
        mode: 0o755,
        name: Name("d"),
    };
    assert_eq!(r.done(n, &mk, vec![]).0, denied);
    // No listing.
    let reply = r.call(n, &proto::ReadDirectory { cookie: 0 }, vec![]);
    assert_eq!(status_of(&reply), denied);
    // Without remove it cannot delete, even its own name.
    let rm = proto::Remove {
        name: Name("new.txt"),
    };
    assert_eq!(r.done(n, &rm, vec![]).0, denied);
    // Clones and shares keep the one name; a narrower name is refused.
    let c = r.net.channel();
    let m = proto::Clone {
        rights: Flags(rights::READ | rights::CLONE),
    };
    assert_eq!(r.done(n, &m, vec![c]).0, status::OK);
    assert_eq!(r.try_open(c, "other.txt", 0, rights::READ).0, denied);
    assert_eq!(r.try_open(c, "new.txt", 0, rights::READ).0, status::OK);
    let (s, shared) = r.share(n, rights::READ);
    assert_eq!(s, status::OK);
    assert_eq!(r.try_open(shared, "other.txt", 0, rights::READ).0, denied);
    assert_eq!(narrow(&mut r, n, "other.txt", rights::READ).0, denied);
    assert_eq!(narrow(&mut r, n, "new.txt", rights::READ).0, status::OK);
    // No right it lacks. (A name of more than one component does not pass
    // the wire: `Name` refuses it.)
    assert_eq!(narrow(&mut r, n, "new.txt", rights::REMOVE).0, denied);
    // Remove narrowed to one name removes it and no other.
    let (_, rmn) = narrow(&mut r, root, "new.txt", rights::READ | rights::REMOVE);
    let other_rm = proto::Remove {
        name: Name("other.txt"),
    };
    assert_eq!(r.done(rmn, &other_rm, vec![]).0, denied);
    assert_eq!(r.done(rmn, &rm, vec![]).0, status::OK);
    assert_eq!(r.read_text(other, 0, 10), (status::OK, b"keep".to_vec()));
    // Rename: each side names only its own name.
    r.open(root, "a.txt", open_flags::CREATE);
    let rename = rights::READ | rights::RENAME | rights::CREATE | rights::REMOVE | rights::CLONE;
    let (_, from) = narrow(&mut r, root, "a.txt", rename);
    let (_, to) = narrow(&mut r, root, "b.txt", rename);
    let token = token_of(&mut r, to);
    let wrong = proto::Rename {
        mode: Below(0),
        old: Name("a.txt"),
        new: Name("c.txt"),
    };
    assert_eq!(r.done(from, &wrong, vec![token]).0, denied);
    let token = token_of(&mut r, to);
    let right = proto::Rename {
        mode: Below(0),
        old: Name("a.txt"),
        new: Name("b.txt"),
    };
    assert_eq!(r.done(from, &right, vec![token]).0, status::OK);
    assert_eq!(r.try_open(root, "b.txt", 0, rights::READ).0, status::OK);
}

/// ADR-0211: `status`'s count is the peak of the space in use since
/// the mount, in bytes: it grows with what is written and stays after it is
/// removed (ADR-0110 item 7's peak `/tmp`).
#[test]
fn status_counts_the_peak_of_the_space_in_use() {
    let mut r = rig();
    let root = r.root;
    let (s, empty) = r.done(root, &proto::Status {}, vec![]);
    assert_eq!(s, status::OK);
    assert!(empty > 0, "the volume's own blocks are in use");
    assert_eq!(empty % 4096, 0, "whole blocks");
    let f = r.open(root, "big", open_flags::CREATE);
    let chunk = [7u8; 2048];
    for i in 0..64 {
        assert_eq!(r.write_text(f, i * 2048, 0, &chunk).0, status::OK);
    }
    r.done(root, &proto::Sync {}, vec![]);
    let (_, full) = r.done(root, &proto::Status {}, vec![]);
    assert!(
        full >= empty + 128 * 1024,
        "128 KiB written: {empty} then {full}"
    );
    r.net.0.borrow_mut().closed.insert(f);
    r.run();
    assert_eq!(
        r.done(root, &proto::Remove { name: Name("big") }, vec![]).0,
        status::OK
    );
    r.done(root, &proto::Sync {}, vec![]);
    let (_, after) = r.done(root, &proto::Status {}, vec![]);
    assert_eq!(after, full.max(after), "a peak does not fall");
    assert!(after >= full);
}

/// Review S2: a narrowed connection's one name, when it is a directory,
/// opens to read only, so nothing inside it can be made, changed or
/// removed through the narrowing.
#[test]
fn a_narrowed_name_that_is_a_directory_opens_to_read_only() {
    let mut r = rig();
    let root = r.root;
    let mk = proto::MakeDirectory {
        mode: 0o755,
        name: Name("d"),
    };
    assert_eq!(r.done(root, &mk, vec![]).0, status::OK);
    for wanted in [
        rights::READ | rights::WRITE | rights::CREATE | rights::SYNC | rights::CLONE,
        rights::READ | rights::REMOVE | rights::CLONE,
    ] {
        let (s, n) = narrow(&mut r, root, "d", wanted);
        assert_eq!(s, status::OK);
        let wide = rights::READ | rights::CREATE | rights::REMOVE | rights::CLONE;
        let (s, _) = r.try_open(n, "d", open_flags::DIRECTORY, wide);
        assert_eq!(s, status::ACCESS_DENIED, "{wanted:#x}");
        let (s, _) = r.try_open(n, "d", open_flags::DIRECTORY, rights::READ);
        assert_eq!(s, status::OK, "{wanted:#x}");
    }
}

/// ADR-0248: `FreeSpace` gives the volume's free bytes on any
/// connection, and a write that takes blocks makes it smaller.
#[test]
fn free_space_counts_the_volumes_free_blocks() {
    let mut r = rig();
    let root = r.root;
    let (s, before) = r.done(root, &proto::FreeSpace {}, vec![]);
    assert_eq!(s, status::OK);
    assert!(before > 0);
    assert_eq!(before % 4096, 0, "whole blocks");
    let f = r.open(root, "big", open_flags::CREATE);
    let data = vec![7u8; 4096];
    for k in 0..16u64 {
        r.write_text(f, k * data.len() as u64, 0, &data);
    }
    // Blocks are taken at the commit.
    assert_eq!(r.done(root, &proto::Sync {}, vec![]).0, status::OK);
    let (_, after) = r.done(root, &proto::FreeSpace {}, vec![]);
    assert!(after < before, "{after} < {before}");
    // A file connection asks as well.
    assert_eq!(r.done(f, &proto::FreeSpace {}, vec![]).0, status::OK);
}

// ---------------------------------------------------------------------------
// Accounts (ADR-0262 item 2, INV-0262-1 to -3) and the folded `sync`
// (ADR-0265 item 9).

impl Rig {
    /// `CloneAccount` of connection `from`: the status and the new
    /// connection's end.
    fn carve(&mut self, from: End, budget: u32) -> (u32, End) {
        let end = self.net.channel();
        let m = proto::CloneAccount {
            rights: Flags(ANY),
            budget,
        };
        let (s, _) = self.done(from, &m, vec![end]);
        (s, end)
    }

    /// Opens files through `via` until a refusal: the connections made.
    fn open_until_refused(&mut self, via: End, tag: &str) -> Vec<End> {
        let mut ends = Vec::new();
        for n in 0..2000 {
            let (s, end) = self.try_open(via, &format!("{tag}{n}"), open_flags::CREATE, ANY);
            if s != status::OK {
                assert_eq!(s, status::TOO_MANY_OPEN);
                return ends;
            }
            ends.push(end);
        }
        panic!("never refused");
    }

    fn peer_closes(&mut self, end: End) {
        self.net.0.borrow_mut().closed.insert(end);
        self.run();
    }
}

/// INV-0262-1: one account opening without end is refused at its bound
/// (`TooManyOpen`, ADR-0055) while a second opens to its full bound and the
/// session still opens.
#[test]
fn an_account_at_its_open_bound_is_refused_and_a_second_is_not() {
    let mut r = rig();
    let root = r.root;
    let (s, a) = r.carve(root, UNTRUSTED_OPEN as u32);
    assert_eq!(s, status::OK);
    let (s, b) = r.carve(root, UNTRUSTED_OPEN as u32);
    assert_eq!(s, status::OK);
    let ea = r.open_until_refused(a, "a");
    // The account's first connection is the clone itself.
    assert_eq!(ea.len(), UNTRUSTED_OPEN - 1);
    let eb = r.open_until_refused(b, "b");
    assert_eq!(
        eb.len(),
        UNTRUSTED_OPEN - 1,
        "the second has its full bound"
    );
    let (s, _) = r.try_open(root, "session", open_flags::CREATE, ANY);
    assert_eq!(s, status::OK, "and the session is not refused");
    let t = r.service.account_totals();
    assert!(t.open <= t.open_limit);
    assert_eq!(t.open, r.service.open_count());
}

/// INV-0262-1 for attached buffers: the account's bound is its budget's
/// eighth (4 for 32); another account attaches to its own.
#[test]
fn an_account_at_its_buffer_bound_is_refused_and_a_second_is_not() {
    let mut r = rig();
    let root = r.root;
    let (_, a) = r.carve(root, UNTRUSTED_OPEN as u32);
    let (_, b) = r.carve(root, UNTRUSTED_OPEN as u32);
    let want = buffers_for(UNTRUSTED_OPEN);
    assert_eq!(want, 4);
    for via in [a, b] {
        let mut attached = 0;
        for n in 0..8 {
            let c = r.open(via, &format!("f{n}-{via}"), open_flags::CREATE);
            let memory = r.net.end(HandleKind::Memory);
            r.net
                .0
                .borrow_mut()
                .memory
                .insert(u64::from(memory), vec![0; 4096]);
            let (s, _) = r.done(c, &proto::Attach { size: 4096 }, vec![memory]);
            if s == status::OK {
                attached += 1;
            } else {
                assert_eq!(s, status::LIMIT_REACHED);
            }
        }
        assert_eq!(attached, want, "account of {via}");
    }
    assert_eq!(r.service.account_totals().buffers, 2 * want);
}

/// INV-0262-1 for held replies: the account holds one; another connection of
/// it whose client does not read loses its connection, and a second account
/// still holds its own.
#[test]
fn an_account_holds_one_reply_and_a_second_holds_its_own() {
    assert_eq!(held_for(UNTRUSTED_OPEN), 1);
    let mut r = rig();
    let root = r.root;
    let (_, a) = r.carve(root, UNTRUSTED_OPEN as u32);
    let (_, b) = r.carve(root, UNTRUSTED_OPEN as u32);
    let a2 = r.open(a, "x", open_flags::CREATE);
    let b2 = r.open(b, "y", open_flags::CREATE);
    for end in [a, a2, b2, root] {
        r.net.0.borrow_mut().room.insert(end, 0);
    }
    r.post(a, &proto::Status {}, vec![]);
    r.run();
    assert_eq!(r.service.account_totals().held, 1);
    // `a2` is in the same account: its reply cannot be held, so it is closed.
    r.post(a2, &proto::Status {}, vec![]);
    r.run();
    assert_eq!(r.service.account_totals().held, 1);
    assert!(
        r.net.0.borrow().closed.contains(&a2),
        "the service closed it"
    );
    // `b2` holds its own, and so does the session.
    r.post(b2, &proto::Status {}, vec![]);
    r.run();
    assert_eq!(r.service.account_totals().held, 2);
    assert!(!r.net.0.borrow().closed.contains(&b2));
    r.post(root, &proto::Status {}, vec![]);
    r.run();
    assert_eq!(r.service.account_totals().held, 3);
}

/// A budget is carved from the caller's account, refused when the caller
/// cannot spare it, and the untrusted column is four accounts of at most 32.
#[test]
fn budgets_are_carved_and_the_column_is_bounded() {
    let mut r = rig();
    let root = r.root;
    assert_eq!(r.carve(root, 0).0, status::INVALID_ARGUMENT);
    assert_eq!(
        r.carve(root, UNTRUSTED_OPEN as u32 + 1).0,
        status::INVALID_ARGUMENT
    );
    let mut ends = Vec::new();
    for _ in 0..MAX_UNTRUSTED_ACCOUNTS {
        let (s, e) = r.carve(root, UNTRUSTED_OPEN as u32);
        assert_eq!(s, status::OK);
        ends.push(e);
    }
    assert_eq!(r.carve(root, 1).0, status::TOO_MANY_OPEN, "a fifth");
    let t = r.service.account_totals();
    assert_eq!(t.top_accounts, 4);
    assert_eq!(t.top_open_limit, 4 * UNTRUSTED_OPEN);
    assert_eq!(t.open_limit, MAX_OPEN);
    assert_eq!(t.held_limit, MAX_HELD);
    assert_eq!(t.buffers_limit, MAX_BUFFERS);
    // The session keeps the rest: 768 open, 224 buffers (ADR-0262's table).
    assert_eq!(t.open_limit - t.top_open_limit, 768);
    assert_eq!(t.buffers_limit - t.top_buffers_limit, 224);
    // An untrusted account carves within itself and no further.
    let a = ends[0];
    assert_eq!(r.carve(a, 40).0, status::TOO_MANY_OPEN);
    let (s, sub) = r.carve(a, 8);
    assert_eq!(s, status::OK);
    assert_eq!(
        r.service.account_totals().top_accounts,
        4,
        "not in the column"
    );
    assert_eq!(r.service.account_totals().accounts, 6);
    // The last connection of an account frees it, and a place is free again.
    r.peer_closes(sub);
    r.peer_closes(ends[1]);
    let t = r.service.account_totals();
    assert_eq!((t.accounts, t.top_accounts), (4, 3));
    assert_eq!(t.open_limit, MAX_OPEN);
    assert_eq!(r.carve(root, UNTRUSTED_OPEN as u32).0, status::OK);
}

/// INV-0262-2: an account's state goes with its last connection, whichever
/// order the connections close in. An account with no connection whose
/// sub-accounts live stays with the limits it has left, and goes with the
/// last of them.
#[test]
fn an_account_is_freed_with_its_last_connection() {
    let mut r = rig();
    let root = r.root;
    let (_, a) = r.carve(root, 16);
    let a2 = r.open(a, "f", open_flags::CREATE);
    let (_, sub) = r.carve(a, 4);
    assert_eq!(r.service.account_totals().accounts, 3);
    r.peer_closes(a);
    assert_eq!(r.service.account_totals().accounts, 3, "a2 holds it");
    r.peer_closes(a2);
    let t = r.service.account_totals();
    assert_eq!(t.accounts, 3, "the parent waits for its sub-account");
    assert_eq!(t.open, r.service.open_count());
    assert_eq!(t.open_limit, MAX_OPEN, "and nothing was returned or lost");
    // The sub-account's end frees it and then its parent.
    r.peer_closes(sub);
    let t = r.service.account_totals();
    assert_eq!((t.accounts, t.open_limit, t.open), (1, MAX_OPEN, 1));
}

/// Review Q3, finding 1: a room carves 31 of its 32 and closes its own
/// connections. The column still holds the four accounts' 128, so a fifth
/// top account is refused until the sub-account goes.
#[test]
fn a_dead_top_account_keeps_its_column_slot_while_a_sub_account_lives() {
    let mut r = rig();
    let root = r.root;
    let mut tops = Vec::new();
    for _ in 0..MAX_UNTRUSTED_ACCOUNTS {
        tops.push(r.carve(root, UNTRUSTED_OPEN as u32).1);
    }
    let (s, sub) = r.carve(tops[0], 31);
    assert_eq!(s, status::OK);
    r.peer_closes(tops[0]);
    assert_eq!(
        r.carve(root, UNTRUSTED_OPEN as u32).0,
        status::TOO_MANY_OPEN
    );
    let t = r.service.account_totals();
    assert!(t.untrusted_open_limit <= MAX_UNTRUSTED_ACCOUNTS * UNTRUSTED_OPEN);
    assert_eq!(
        t.open_limit - t.untrusted_open_limit,
        768,
        "the session keeps 768"
    );
    r.peer_closes(sub);
    assert_eq!(
        r.carve(root, UNTRUSTED_OPEN as u32).0,
        status::OK,
        "now a place is free"
    );
}

/// INV-0262-2, -3 and INV-0291-1: random clones with budgets, opens, clones,
/// shares, narrows, tokens, buffers, held replies and closes, the session's
/// own connection included. After each one the limits of all accounts add up
/// to the global bounds, the untrusted accounts together (sub-accounts too)
/// stay inside the column, nothing is over its limit, every live connection
/// has a live account, and the accounts' counts equal the service's. At the
/// end every connection is closed and no account is left.
#[test]
fn random_accounts_keep_the_sum_of_bounds_and_free_their_state() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let mut r = rig();
    let root = r.root;
    let mut live: Vec<End> = vec![root];
    for step in 0..3000 {
        if live.is_empty() {
            break;
        }
        let from = live[next(live.len())];
        match next(16) {
            0..=1 => {
                let budget = [1, 2, 8, 16, 32, 33, 100][next(7)];
                let (s, e) = r.carve(from, budget);
                if s == status::OK {
                    live.push(e);
                }
            }
            2..=4 => {
                let (s, e) = r.try_open(from, &format!("f{step}"), open_flags::CREATE, ANY);
                if s == status::OK {
                    live.push(e);
                }
            }
            5 => {
                let e = r.net.channel();
                let (s, _) = r.done(from, &proto::Clone { rights: Flags(ANY) }, vec![e]);
                if s == status::OK {
                    live.push(e);
                }
            }
            6 => {
                let e = r.net.channel();
                let (s, _) = r.done(from, &proto::Share { rights: Flags(ANY) }, vec![e]);
                if s == status::OK {
                    live.push(e);
                }
            }
            7 => {
                let e = r.net.channel();
                let m = proto::Narrow {
                    rights: Flags(ANY),
                    name: Name("n"),
                };
                let (s, _) = r.done(from, &m, vec![e]);
                if s == status::OK {
                    live.push(e);
                }
            }
            8 => {
                r.post(from, &proto::GetToken {}, vec![]);
                r.run();
                // The client takes the reply and its token copy.
                while let Some((_, handles)) = r.net.take_reply(from) {
                    for h in handles {
                        r.net.0.borrow_mut().owned.remove(&h);
                    }
                }
            }
            9 => {
                let memory = r.net.end(HandleKind::Memory);
                r.net
                    .0
                    .borrow_mut()
                    .memory
                    .insert(u64::from(memory), vec![0; 4096]);
                let _ = r.done(from, &proto::Attach { size: 4096 }, vec![memory]);
            }
            10 => {
                let _ = r.done(from, &proto::Detach {}, vec![]);
            }
            11 => {
                // A held reply: the client's queue is full for a request.
                r.net.0.borrow_mut().room.insert(from, 0);
                r.post(from, &proto::Status {}, vec![]);
                r.run();
                if next(2) == 0 {
                    r.net.0.borrow_mut().no_memory = true;
                    let other = live[next(live.len())];
                    r.post(other, &proto::Status {}, vec![]);
                    r.run();
                    r.net.0.borrow_mut().no_memory = false;
                }
                r.net.0.borrow_mut().room.remove(&from);
                r.net.0.borrow_mut().wake_deadline = true;
                r.run();
                let others = live.clone();
                for e in others {
                    while r.net.take_reply(e).is_some() {}
                }
            }
            _ => {
                // Any connection may close, the session's included.
                live.retain(|e| *e != from);
                r.peer_closes(from);
            }
        }
        // Connections the service closed itself are not live.
        live.retain(|e| !r.net.0.borrow().closed.contains(e));
        let t = r.service.account_totals();
        if t.accounts > 0 {
            assert_eq!(t.open_limit, MAX_OPEN, "step {step}");
            assert_eq!(t.buffers_limit, MAX_BUFFERS, "step {step}");
            assert_eq!(t.held_limit, MAX_HELD, "step {step}");
        }
        assert!(t.top_accounts <= MAX_UNTRUSTED_ACCOUNTS, "step {step}");
        assert!(
            t.untrusted_open_limit <= MAX_UNTRUSTED_ACCOUNTS * UNTRUSTED_OPEN,
            "step {step}: {t:?}"
        );
        assert!(
            t.untrusted_buffers_limit <= MAX_UNTRUSTED_ACCOUNTS * buffers_for(UNTRUSTED_OPEN),
            "step {step}"
        );
        assert!(
            t.untrusted_held_limit <= MAX_UNTRUSTED_ACCOUNTS * held_for(UNTRUSTED_OPEN),
            "step {step}"
        );
        assert!(
            t.open <= t.open_limit && t.buffers <= t.buffers_limit,
            "step {step}"
        );
        assert_eq!(t.open, r.service.open_count(), "step {step}");
        for c in 0..4000 {
            if let Some(a) = r.service.account_index(c) {
                assert!(r.service.account_live(a), "step {step}: connection {c}");
            }
        }
    }
    for e in live.clone() {
        r.peer_closes(e);
    }
    let t = r.service.account_totals();
    assert_eq!((t.accounts, t.open), (0, 0), "no account is left");
    assert_eq!(r.service.open_count(), 0);
}

/// Review Q3, finding 2: the driver goes while an untrusted `sync` waits.
/// The `sync` is answered `StorageFailed` without another client's request.
#[test]
fn a_lost_driver_answers_a_waiting_sync() {
    let mut r = rig();
    let root = r.root;
    let (_, u) = r.carve(root, 8);
    let f = r.open(u, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"x");
    r.post(f, &proto::Sync {}, vec![]);
    r.run();
    assert!(r.net.take_reply(f).is_none());
    r.net.0.borrow_mut().driver_closed = true;
    r.run();
    r.run();
    let reply = r.net.take_reply(f).expect("answered").0;
    assert_eq!(status_of(&reply), status::STORAGE_FAILED);
}

/// Review Q3, finding 3: `NoMemory` is not the client's doing and does not
/// count against its account's bound of held replies.
#[test]
fn no_memory_does_not_close_a_connection_of_an_account_holding_a_reply() {
    let mut r = rig();
    let root = r.root;
    let (_, a) = r.carve(root, UNTRUSTED_OPEN as u32);
    let a2 = r.open(a, "x", open_flags::CREATE);
    r.net.0.borrow_mut().room.insert(a, 0);
    r.post(a, &proto::Status {}, vec![]);
    r.run();
    assert_eq!(r.service.account_totals().held, 1);
    r.net.0.borrow_mut().no_memory = true;
    r.post(a2, &proto::Status {}, vec![]);
    r.run();
    assert!(!r.net.0.borrow().closed.contains(&a2), "held, not closed");
    // Retried when the memory is back.
    r.net.0.borrow_mut().no_memory = false;
    r.clock.advance_mono(1_000_000_000);
    r.net.0.borrow_mut().wake_deadline = true;
    r.run();
    assert!(r.net.take_reply(a2).is_some());
}

/// Review Q3, finding 4: a durable write from an untrusted account waits
/// for the next commit like a `sync`, and replies with its count.
#[test]
fn an_untrusted_durable_write_waits_for_the_next_commit() {
    let mut r = rig();
    let root = r.root;
    let (_, u) = r.carve(root, 8);
    let f = r.open(u, "f", open_flags::CREATE);
    let m = proto::Write {
        offset: 0,
        flags: Flags(write_flags::DURABLE),
        data: Data(b"abc"),
    };
    for round in 0..5 {
        let before = r.flushes();
        r.post(f, &m, vec![]);
        r.run();
        assert!(
            r.net.take_reply(f).is_none(),
            "round {round}: no answer yet"
        );
        assert_eq!(r.flushes(), before, "and no commit of its own");
        r.clock.advance_mono(6_000_000_000);
        r.net.0.borrow_mut().wake_deadline = true;
        r.run();
        let reply = r.net.take_reply(f).expect("answered by the timed commit").0;
        let (_, d) = decode::<proto::Done>(&reply, 0).unwrap();
        assert_eq!((d.status.0, d.count), (status::OK, 3));
    }
    // The session's durable write commits at once.
    let g = r.open(root, "g", open_flags::CREATE);
    assert_eq!(
        r.write_text(g, 0, write_flags::DURABLE, b"z"),
        (status::OK, 1)
    );
}

/// Review Q3, finding 5: each refusal at an account's bound is counted for
/// its launcher.
#[test]
fn refusals_are_counted_per_account() {
    let mut r = rig();
    let root = r.root;
    let (_, a) = r.carve(root, 2);
    let (_, b) = r.carve(root, 2);
    let ia = r.service.account_index(1).unwrap();
    let ib = r.service.account_index(2).unwrap();
    let _ = (a, b);
    assert_eq!(r.service.account_refusals(ia), Some(0));
    let _ = r.open_until_refused(a, "a");
    let _ = r.try_open(a, "again", open_flags::CREATE, ANY);
    assert_eq!(r.service.account_refusals(ia), Some(2));
    assert_eq!(r.service.account_refusals(ib), Some(0), "b is not charged");
}

/// INV-0262-3's size test: the service's worst case from its bounds, and an
/// untrusted account's share of it.
#[test]
fn the_service_size_follows_from_the_bounds() {
    let w = worst_case::<End>();
    // An untrusted account, and a sub-account for each of its handles.
    let untrusted = w.of_account(UNTRUSTED_OPEN, held_for(UNTRUSTED_OPEN))
        + w.per_account * (UNTRUSTED_OPEN - 1);
    let session = w.of_account(
        MAX_OPEN - MAX_UNTRUSTED_ACCOUNTS * UNTRUSTED_OPEN,
        MAX_HELD - MAX_UNTRUSTED_ACCOUNTS * held_for(UNTRUSTED_OPEN),
    );
    // The total counts an account for every handle (sub-accounts); the
    // session's and the four untrusted ones above already count 5 + 4 * 31.
    assert_eq!(
        w.fixed
            + session
            + MAX_UNTRUSTED_ACCOUNTS * untrusted
            + w.per_account * (MAX_OPEN - 5 - 4 * 31),
        w.total(),
        "the session and four untrusted accounts are the whole"
    );
    println!(
        "worst case {w:?}: untrusted account {untrusted} B, session {session} B, total {} B",
        w.total()
    );
    assert!(untrusted < 512 * 1024, "{untrusted}");
    assert!(MAX_UNTRUSTED_ACCOUNTS * untrusted < w.total() / 8);
}

/// ADR-0265 item 9, INV-0265-5's host half: a `sync` from an untrusted
/// account waits for the next commit and causes none of its own; another
/// client's `sync` (or the timed commit) answers it.
#[test]
fn an_untrusted_sync_waits_for_the_next_commit() {
    let mut r = rig();
    let root = r.root;
    let (_, u) = r.carve(root, 8);
    let f = r.open(u, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"x");
    let before = r.flushes();
    r.post(f, &proto::Sync {}, vec![]);
    r.run();
    assert!(r.net.take_reply(f).is_none(), "no answer yet");
    assert_eq!(r.flushes(), before, "and no commit");
    // The connection is not read meanwhile.
    r.post(f, &proto::Status {}, vec![]);
    r.run();
    assert!(r.net.take_reply(f).is_none());
    // A session client's `sync` commits, and answers both.
    assert_eq!(r.done(root, &proto::Sync {}, vec![]).0, status::OK);
    assert!(r.flushes() > before);
    let reply = r.net.take_reply(f).expect("answered by that commit").0;
    assert_eq!(status_of(&reply), status::OK);
    let again = r.net.take_reply(f).expect("then the next request").0;
    assert_eq!(status_of(&again), status::OK);
    // With nothing to commit it answers at once.
    assert_eq!(r.done(f, &proto::Sync {}, vec![]).0, status::OK);
}

/// The timed commit answers an untrusted `sync` within its interval, and a
/// loop of them adds no commit of its own.
#[test]
fn a_sync_loop_in_an_untrusted_account_adds_no_commit() {
    let mut r = rig();
    let root = r.root;
    let (_, u) = r.carve(root, 8);
    let f = r.open(u, "f", open_flags::CREATE);
    for round in 0..5 {
        r.write_text(f, 0, 0, format!("{round}").as_bytes());
        r.post(f, &proto::Sync {}, vec![]);
        r.run();
        assert!(r.net.take_reply(f).is_none());
        let flushes = r.flushes();
        // Nothing commits before the timer, however often the loop runs.
        r.run();
        assert_eq!(r.flushes(), flushes);
        r.clock.advance_mono(6_000_000_000);
        r.net.0.borrow_mut().wake_deadline = true;
        r.run();
        let reply = r.net.take_reply(f).expect("answered by the timed commit").0;
        assert_eq!(status_of(&reply), status::OK);
        assert!(r.flushes() > flushes, "the timed commit, round {round}");
    }
}

/// A client that closes while its `sync` waits leaves nothing behind.
#[test]
fn a_closed_connection_waiting_for_a_sync_is_dropped() {
    let mut r = rig();
    let root = r.root;
    let (_, u) = r.carve(root, 8);
    let f = r.open(u, "f", open_flags::CREATE);
    r.write_text(f, 0, 0, b"x");
    r.post(f, &proto::Sync {}, vec![]);
    r.run();
    r.peer_closes(f);
    r.peer_closes(u);
    assert_eq!(r.service.account_totals().accounts, 1);
    assert_eq!(r.done(root, &proto::Sync {}, vec![]).0, status::OK);
    assert_eq!(r.service.account_totals().open, r.service.open_count());
}

/// ADR-0289 item 8: a sealed entry of the root, and all below it, is
/// read-only on every connection; no clone, narrowing, token or open can add
/// a write to it, and the entry itself cannot be removed, renamed or replaced.
#[test]
fn a_sealed_entry_is_read_only_for_every_connection() {
    let mut r = rig();
    let root = r.root;
    let denied = status::ACCESS_DENIED;
    let mkdir = |r: &mut Rig, dir: End, name: &'static str| {
        let m = proto::MakeDirectory {
            mode: 0o755,
            name: Name(name),
        };
        assert_eq!(r.done(dir, &m, vec![]).0, status::OK);
    };
    mkdir(&mut r, root, "system");
    let sys = r.open(root, "system", open_flags::DIRECTORY);
    mkdir(&mut r, sys, "tls");
    let roots = r.open(root, "system/tls/roots", open_flags::CREATE);
    assert_eq!(r.write_text(roots, 0, 0, b"anchor").0, status::OK);
    r.open(root, "other", open_flags::CREATE);
    r.service.seal("system");

    // Opening: reading is allowed, anything that changes is not, by any
    // spelling of the path.
    let read = rights::READ | rights::CLONE;
    for path in ["system/tls/roots"] {
        for wanted in [rights::WRITE, rights::CREATE, rights::REMOVE, ANY] {
            assert_eq!(r.try_open(root, path, 0, wanted).0, denied, "{path}");
        }
        assert_eq!(
            r.try_open(root, path, open_flags::TRUNCATE, rights::READ).0,
            denied,
            "{path} truncate"
        );
        let (s, ro) = r.try_open(root, path, 0, read);
        assert_eq!(s, status::OK, "{path} for reading");
        assert_eq!(r.read_text(ro, 0, 6).1, b"anchor");
        assert_eq!(r.write_text(ro, 0, 0, b"x").0, denied);
        let m = proto::Clone {
            rights: Flags(rights::WRITE),
        };
        let end = r.net.channel();
        assert_eq!(r.done(ro, &m, vec![end]).0, denied, "clone adds no write");
    }
    assert_eq!(
        r.try_open(root, "system/tls/new", open_flags::CREATE, rights::READ)
            .0,
        denied
    );
    assert_eq!(
        r.try_open(root, "system/x", open_flags::UNNAMED, rights::READ)
            .0,
        denied
    );

    // A directory connection below the seal.
    let (s, tls) = r.try_open(root, "system/tls", open_flags::DIRECTORY, read);
    assert_eq!(s, status::OK);
    assert_eq!(r.try_open(tls, "roots", 0, rights::WRITE).0, denied);
    assert_eq!(r.try_open(tls, "new", open_flags::CREATE, ANY).0, denied);
    let m = proto::MakeDirectory {
        mode: 0o755,
        name: Name("d"),
    };
    assert_eq!(r.done(tls, &m, vec![]).0, denied);
    assert_eq!(
        r.done(
            tls,
            &proto::Remove {
                name: Name("roots")
            },
            vec![]
        )
        .0,
        denied
    );
    let m = proto::MakeLink {
        name: Name("l"),
        target: Text("roots"),
    };
    assert_eq!(r.done(tls, &m, vec![]).0, denied);
    let m = proto::SetLimit {
        remove: Below(0),
        limit: 1 << 20,
        name: Name("roots"),
    };
    assert_eq!(r.done(tls, &m, vec![]).0, denied);
    let token = token_of(&mut r, tls);
    let m = proto::Publish {
        mode: Below(proto::publish_mode::REPLACE),
        name: Name("roots"),
    };
    let (s, unnamed) = r.try_open(root, "tmp-file", open_flags::UNNAMED, ANY);
    assert_eq!(s, status::OK);
    assert_eq!(r.done(unnamed, &m, vec![token]).0, denied, "no publish in");
    let token = token_of(&mut r, tls);
    let m = proto::Rename {
        mode: Below(0),
        old: Name("roots"),
        new: Name("moved"),
    };
    assert_eq!(r.done(tls, &m, vec![token]).0, denied, "no rename out");
    let token = token_of(&mut r, tls);
    let m = proto::Rename {
        mode: Below(0),
        old: Name("other"),
        new: Name("roots"),
    };
    assert_eq!(r.done(root, &m, vec![token]).0, denied, "no rename in");

    // The root itself: the entry is kept, its siblings are not affected.
    let remove = proto::Remove {
        name: Name("system"),
    };
    assert_eq!(r.done(root, &remove, vec![]).0, denied);
    let token = token_of(&mut r, root);
    let m = proto::Rename {
        mode: Below(0),
        old: Name("system"),
        new: Name("away"),
    };
    assert_eq!(r.done(root, &m, vec![token]).0, denied);
    let token = token_of(&mut r, root);
    let m = proto::Rename {
        mode: Below(proto::rename_mode::EXCHANGE),
        old: Name("other"),
        new: Name("system"),
    };
    assert_eq!(r.done(root, &m, vec![token]).0, denied);
    let token = token_of(&mut r, root);
    let m = proto::Publish {
        mode: Below(proto::publish_mode::REPLACE),
        name: Name("system"),
    };
    let (_, unnamed) = r.try_open(root, "tmp-file2", open_flags::UNNAMED, ANY);
    assert_eq!(r.done(unnamed, &m, vec![token]).0, denied);
    let m = proto::SetLimit {
        remove: Below(0),
        limit: 1 << 20,
        name: Name("system"),
    };
    assert_eq!(r.done(root, &m, vec![]).0, denied);

    // A clone or a narrowing of the root with write opens "system" to read
    // only, too.
    let end = r.net.channel();
    let m = proto::Narrow {
        rights: Flags(ANY),
        name: Name("system"),
    };
    assert_eq!(r.done(root, &m, vec![end]).0, status::OK);
    r.net.0.borrow_mut().queues.entry(end).or_default();
    assert_eq!(r.try_open(end, "system", 0, rights::WRITE).0, denied);
    let end2 = r.net.channel();
    let m = proto::Clone { rights: Flags(ANY) };
    assert_eq!(r.done(root, &m, vec![end2]).0, status::OK);
    assert_eq!(
        r.try_open(end2, "system/tls/roots", 0, rights::WRITE).0,
        denied
    );

    // The rest of the volume is still writable, and the file is unchanged.
    let m = proto::Remove {
        name: Name("other"),
    };
    assert_eq!(r.done(root, &m, vec![]).0, status::OK);
    let (_, ro) = r.try_open(root, "system/tls/roots", 0, read);
    assert_eq!(r.read_text(ro, 0, 16).1, b"anchor");
}

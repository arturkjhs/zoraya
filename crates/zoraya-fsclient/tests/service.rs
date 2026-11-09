//! The client against the file service itself, in-process: every request
//! the chooser and the file manager make, and the rights a narrowed or
//! cloned connection keeps (INV-0167-3's service half).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use zoraya_fs::{Block, BlockDevice, DevError, MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::{
    Device, HandleKind, Incoming, Interest, Outgoing, Service, Transport, Wake,
};
use zoraya_fs_testkit::clock::TestClock;
use zoraya_fs_testkit::devices::MemDevice;
use zoraya_fsclient::{Client, Link, Listed};
use zoraya_proto_file::{kind, open_flags, rights, status};

zoraya_test_alloc::cap_memory!();

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
    tokens: BTreeMap<End, End>,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Net>>);

impl Fake {
    fn end(&self, kind: HandleKind) -> End {
        let mut n = self.0.borrow_mut();
        n.next += 1;
        let e = n.next;
        n.kinds.insert(e, kind);
        e
    }

    fn channel(&self) -> End {
        let e = self.end(HandleKind::Channel);
        self.0.borrow_mut().queues.insert(e, VecDeque::new());
        e
    }
}

impl Transport for Fake {
    type End = End;

    fn read(&mut self, end: End, bytes: &mut [u8], handles: &mut Vec<End>) -> Incoming {
        let mut n = self.0.borrow_mut();
        match n.queues.get_mut(&end).and_then(VecDeque::pop_front) {
            Some((m, list)) => {
                bytes[..m.len()].copy_from_slice(&m);
                handles.extend(list);
                Incoming::Message(m.len())
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
        n.replies
            .entry(end)
            .or_default()
            .push_back((bytes.to_vec(), handles.to_vec()));
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

    fn map(&mut self, _memory: End, _length: u64) -> Option<u64> {
        None
    }

    fn unmap(&mut self, _address: u64, _length: u64) {}

    fn copy_from(&mut self, _address: u64, _out: &mut [u8]) -> bool {
        false
    }

    fn copy_to(&mut self, _address: u64, _bytes: &[u8]) -> bool {
        false
    }

    fn report(&mut self, _reason: u32) {}

    fn wait(&mut self, _deadline: Option<u64>) -> Wake {
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

struct Disk(MemDevice);

impl BlockDevice for Disk {
    fn sector_count(&self) -> u64 {
        self.0.sector_count()
    }
    fn flush_supported(&self) -> bool {
        self.0.flush_supported()
    }
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        self.0.read(block, buf)
    }
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        self.0.write(writes)
    }
    fn flush(&mut self) -> Result<(), DevError> {
        self.0.flush()
    }
}

impl Device for Disk {
    fn is_lost(&self) -> bool {
        false
    }
    fn mark_lost(&mut self) {}
}

type Svc = Service<Disk, TestClock, Fake>;

/// The client's side: requests go into the service's queue, and the
/// service runs until it has nothing left to do.
struct Wire {
    net: Fake,
    service: Rc<RefCell<Svc>>,
}

impl Link for Wire {
    type Handle = End;

    fn call(
        &mut self,
        on: End,
        request: &[u8],
        handles: &[End],
        reply: &mut [u8],
        got: &mut Vec<End>,
    ) -> Option<usize> {
        {
            let mut n = self.net.0.borrow_mut();
            if n.closed.contains(&on) {
                return None;
            }
            n.queues
                .get_mut(&on)?
                .push_back((request.to_vec(), handles.to_vec()));
        }
        while self.service.borrow_mut().step() {}
        let (bytes, list) = self
            .net
            .0
            .borrow_mut()
            .replies
            .get_mut(&on)
            .and_then(VecDeque::pop_front)?;
        reply[..bytes.len()].copy_from_slice(&bytes);
        got.extend(list);
        Some(bytes.len())
    }

    fn channel(&mut self) -> Option<(End, End)> {
        let e = self.net.channel();
        Some((e, e))
    }

    fn close(&mut self, h: End) {
        self.net.0.borrow_mut().closed.insert(h);
        while self.service.borrow_mut().step() {}
    }
}

const NOW: Timestamp = Timestamp {
    secs: 1000,
    nanos: 0,
};

fn rig() -> (Client<Wire>, End) {
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
    let volume = Volume::mount(Disk(device), TestClock::new(NOW), MountOptions::default()).unwrap();
    let net = Fake::default();
    let mut service = Service::new(volume, net.clone());
    let root = net.channel();
    assert!(service.add_root(root));
    let wire = Wire {
        net,
        service: Rc::new(RefCell::new(service)),
    };
    (Client::new(wire), root)
}

fn names(list: &[Listed]) -> Vec<(&str, bool)> {
    let mut v: Vec<(&str, bool)> = list.iter().map(|l| (l.name.as_str(), l.is_dir())).collect();
    v.sort();
    v
}

#[test]
fn a_folder_its_files_and_their_changes() {
    let (mut c, root) = rig();
    c.make_directory(root, "home").unwrap();
    let home = c
        .open(root, "home", open_flags::DIRECTORY, rights::ALL)
        .unwrap();
    c.make_directory(home, "Документи").unwrap();
    assert_eq!(c.make_directory(home, "Документи"), Err(status::EXISTS));
    let f = c
        .open(home, "plán.txt", open_flags::CREATE, rights::ALL)
        .unwrap();
    let text = "Poznámky k etapě 7\n".repeat(5000);
    c.write_all(f, text.as_bytes()).unwrap();
    assert_eq!(c.attributes(f).unwrap(), (text.len() as u64, kind::FILE));
    assert_eq!(c.read_all(f).unwrap(), text.as_bytes());
    // Shorter contents replace the longer.
    c.write_all(f, b"short").unwrap();
    assert_eq!(c.read_all(f).unwrap(), b"short");
    c.write_all(f, b"").unwrap();
    assert_eq!(c.attributes(f).unwrap().0, 0);
    c.close(f);
    assert_eq!(
        names(&c.list(home).unwrap()),
        [("plán.txt", false), ("Документи", true)]
    );
    // Rename, which never replaces, and remove.
    c.rename(home, "plán.txt", "звіт-2026.txt").unwrap();
    c.open(home, "x", open_flags::CREATE, rights::ALL)
        .map(|h| c.close(h))
        .unwrap();
    assert_eq!(c.rename(home, "x", "звіт-2026.txt"), Err(status::EXISTS));
    assert_eq!(c.rename(home, "x", "a/b"), Err(status::INVALID_NAME));
    c.remove(home, "x").unwrap();
    assert_eq!(c.remove(home, "x"), Err(status::NOT_FOUND));
    c.open(home, "Документи/a", open_flags::CREATE, rights::ALL)
        .map(|h| c.close(h))
        .unwrap();
    assert_eq!(c.remove(home, "Документи"), Err(status::NOT_EMPTY));
    assert_eq!(
        c.open(home, "nothing", 0, rights::READ),
        Err(status::NOT_FOUND)
    );
    // Many names: the listing follows its cookies past one reply.
    for i in 0..150 {
        let h = c
            .open(home, &format!("f{i:03}"), open_flags::CREATE, rights::ALL)
            .unwrap();
        c.close(h);
    }
    assert_eq!(c.list(home).unwrap().len(), 152);
}

/// What the chooser makes: a read-only file, a directory narrowed to one
/// name, a cloned directory with fewer rights; none can do more.
#[test]
fn narrowed_and_cloned_connections_keep_only_their_rights() {
    let (mut c, root) = rig();
    let f = c
        .open(root, "notes.txt", open_flags::CREATE, rights::ALL)
        .unwrap();
    c.write_all(f, b"mine").unwrap();
    // Open: the file with READ | CLONE only.
    let read = c
        .open(root, "notes.txt", 0, rights::READ | rights::CLONE)
        .unwrap();
    assert_eq!(c.read_all(read).unwrap(), b"mine");
    assert_eq!(c.write_all(read, b"theirs"), Err(status::ACCESS_DENIED));
    // Its clone cannot gain what it lacks.
    let more = c.clone_with(read, rights::ALL);
    if let Ok(h) = more {
        assert_eq!(c.write_all(h, b"theirs"), Err(status::ACCESS_DENIED));
    }
    // Save: the parent narrowed to one name, with create's rights.
    let create = rights::READ | rights::WRITE | rights::CREATE | rights::SYNC | rights::CLONE;
    let one = c.narrow(root, "new.txt", create).unwrap();
    let new = c
        .open(
            one,
            "new.txt",
            open_flags::CREATE,
            rights::READ | rights::WRITE,
        )
        .unwrap();
    c.write_all(new, b"saved").unwrap();
    assert_eq!(
        c.open(one, "notes.txt", 0, rights::READ),
        Err(status::ACCESS_DENIED)
    );
    assert!(
        c.list(one).is_err(),
        "a narrowed directory cannot be listed"
    );
    assert_eq!(c.make_directory(one, "d"), Err(status::ACCESS_DENIED));
    // PickDirectory without write: read only.
    let dir = c.clone_with(root, rights::READ | rights::CLONE).unwrap();
    assert!(c.list(dir).is_ok());
    assert_eq!(c.make_directory(dir, "d"), Err(status::ACCESS_DENIED));
    assert_eq!(c.remove(dir, "notes.txt"), Err(status::ACCESS_DENIED));
    assert_eq!(c.narrow(root, "a/b", create), Err(status::INVALID_NAME));
}

/// A service that answers each request with what `answer` makes of it:
/// replies the real service never sends, and records what was closed.
struct Canned<F: FnMut(u32, &[u8]) -> (Vec<u8>, Vec<End>)> {
    answer: F,
    closed: Vec<End>,
}

impl<F: FnMut(u32, &[u8]) -> (Vec<u8>, Vec<End>)> Link for Canned<F> {
    type Handle = End;

    fn call(
        &mut self,
        _on: End,
        request: &[u8],
        _handles: &[End],
        reply: &mut [u8],
        got: &mut Vec<End>,
    ) -> Option<usize> {
        let (h, _) = zoraya_protocol::Header::decode(request).ok()?;
        let (bytes, list) = (self.answer)(h.ordinal, request);
        reply[..bytes.len()].copy_from_slice(&bytes);
        got.extend(list);
        Some(bytes.len())
    }

    fn channel(&mut self) -> Option<(End, End)> {
        Some((90, 91))
    }

    fn close(&mut self, h: End) {
        self.closed.push(h);
    }
}

fn canned<F: FnMut(u32, &[u8]) -> (Vec<u8>, Vec<End>)>(answer: F) -> Client<Canned<F>> {
    Client::new(Canned {
        answer,
        closed: Vec::new(),
    })
}

fn wire<'a, M: zoraya_protocol::Message<'a>>(m: &M) -> Vec<u8> {
    let mut out = vec![0; 64 * 1024];
    let n = zoraya_protocol::encode(m, 1, &mut out).unwrap();
    out.truncate(n);
    out
}

fn done(status: u32, count: u64) -> Vec<u8> {
    wire(&zoraya_proto_file::Done {
        status: zoraya_protocol::Below(status),
        count,
    })
}

/// A token is taken only from an OK reply with its one handle; an error
/// reply's handle is closed and its status returned.
#[test]
fn a_token_is_taken_only_from_an_ok_reply_with_one_handle() {
    let mut c = canned(|ordinal, _| match ordinal {
        18 => (
            wire(&zoraya_proto_file::TokenReply {
                status: zoraya_protocol::Below(status::NOT_FOUND),
            }),
            vec![77],
        ),
        _ => (done(status::OK, 0), vec![]),
    });
    assert_eq!(c.rename(5, "a", "b"), Err(status::NOT_FOUND));
    assert_eq!(c.link.closed, [77]);
    // OK, without its handle: refused.
    let mut c = canned(|ordinal, _| match ordinal {
        18 => (
            wire(&zoraya_proto_file::TokenReply {
                status: zoraya_protocol::Below(status::OK),
            }),
            vec![],
        ),
        _ => (done(status::OK, 0), vec![]),
    });
    assert_eq!(c.rename(5, "a", "b"), Err(status::INVALID_ARGUMENT));
}

/// Attributes give the service's error status.
#[test]
fn attributes_return_the_services_error_status() {
    let mut c = canned(|_, _| {
        let a = zoraya_proto_file::Attributes {
            status: zoraya_protocol::Below(status::NOT_FOUND),
            size: 1,
            modified: 0,
            changed: 0,
            mode: 0,
            kind: zoraya_protocol::Below(kind::FILE),
        };
        (wire(&a), vec![])
    });
    assert_eq!(c.attributes(5), Err(status::NOT_FOUND));
}

/// A file of exactly sixteen mebibytes reads back; one byte more is
/// refused.
#[test]
fn read_all_takes_exactly_sixteen_mebibytes_and_refuses_more() {
    let chunk = vec![7u8; zoraya_proto_file::MAX_INLINE];
    for (size, want) in [
        (zoraya_fsclient::MAX_READ, Ok(())),
        (zoraya_fsclient::MAX_READ + 1, Err(status::LIMIT_REACHED)),
    ] {
        let mut c = canned(|_, request| {
            let (_, r) = zoraya_protocol::decode::<zoraya_proto_file::Read>(request, 0).unwrap();
            let left = size.saturating_sub(r.offset).min(u64::from(r.length)) as usize;
            let m = zoraya_proto_file::ReadReply {
                status: zoraya_protocol::Below(status::OK),
                data: zoraya_protocol::Data(&chunk[..left]),
            };
            (wire(&m), vec![])
        });
        let got = c.read_all(5).map(|v| assert_eq!(v.len() as u64, size));
        assert_eq!(got, want, "{size}");
    }
}

/// A write the service takes none of is out of space.
#[test]
fn write_all_reports_no_space_when_nothing_is_written() {
    let mut c = canned(|_, _| (done(status::OK, 0), vec![]));
    assert_eq!(c.write_all(5, b"hello"), Err(status::NO_SPACE));
}

/// A handle in a reply that asked for none is closed.
#[test]
fn a_handle_no_request_asked_for_is_closed() {
    let mut c = canned(|_, _| (done(status::OK, 0), vec![66]));
    assert_eq!(c.remove(5, "a"), Ok(()));
    assert_eq!(c.link.closed, [66]);
}

/// Closing a connection closes it on the link.
#[test]
fn close_tells_the_link() {
    let mut c = canned(|_, _| (done(status::OK, 0), vec![]));
    c.close(5);
    assert_eq!(c.link.closed, [5]);
}

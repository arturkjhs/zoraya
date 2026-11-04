//! The temporary directory service's FIFO nodes (ADR-0107 items 13 to 16)
//! through the service loop, on a `MemoryDevice`: GNU make's open sequence
//! (INV-0107-4), the three open behaviours (INV-0006-9), the end-of-file
//! rule, and unlink while open (INV-0107-5).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use zoraya_fs::{MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::memory::MemoryDevice;
use zoraya_fs_service::{HandleKind, Incoming, Interest, Outgoing, Service, Transport, Wake};
use zoraya_fs_testkit::clock::TestClock;
use zoraya_proto_file::{self as proto, S_IFIFO, fifo_side, open_flags, rights, status};
use zoraya_protocol::{
    Below, ERROR_ORDINAL, ErrorReply, Flags, Header, Message, Name, Path, decode, encode,
};

type End = u32;
type Queue = VecDeque<(Vec<u8>, Vec<End>)>;

/// One side of a stream pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Read(u32),
    Write(u32),
}

#[derive(Default)]
struct Net {
    next: End,
    queues: BTreeMap<End, Queue>,
    replies: BTreeMap<End, Queue>,
    closed: BTreeSet<End>,
    kinds: BTreeMap<End, HandleKind>,
    interests: BTreeMap<u64, (End, Interest)>,
    /// Stream ends: which pair and side each handle is.
    streams: BTreeMap<End, Side>,
    pairs: u32,
    /// Fail the next `stream` call.
    no_streams: bool,
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

    /// The client closes its end of a connection or a stream.
    fn client_close(&self, end: End) {
        let mut n = self.0.borrow_mut();
        n.closed.insert(end);
        n.streams.remove(&end);
    }

    /// Open handles to a side of a pair, anywhere.
    fn holders(&self, side: Side) -> usize {
        self.0
            .borrow()
            .streams
            .values()
            .filter(|s| **s == side)
            .count()
    }

    fn side(&self, end: End) -> Side {
        self.0.borrow().streams[&end]
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
        n.streams.remove(&end);
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
        let side = self.0.borrow().streams.get(&end).copied()?;
        let copy = self.end(HandleKind::Other);
        self.0.borrow_mut().streams.insert(copy, side);
        Some(copy)
    }

    fn new_token(&mut self) -> Option<End> {
        Some(self.end(HandleKind::Token))
    }

    fn same(&mut self, a: End, b: End) -> bool {
        a == b
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
                Interest::Reply | Interest::Closed => closed,
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

    fn stream(&mut self) -> Option<(End, End)> {
        if std::mem::take(&mut self.0.borrow_mut().no_streams) {
            return None;
        }
        let r = self.end(HandleKind::Other);
        let w = self.end(HandleKind::Other);
        let mut n = self.0.borrow_mut();
        n.pairs += 1;
        let p = n.pairs;
        n.streams.insert(r, Side::Read(p));
        n.streams.insert(w, Side::Write(p));
        Some((r, w))
    }
}

type Svc = Service<MemoryDevice<Vec<u8>>, TestClock, Fake>;

struct Rig {
    net: Fake,
    service: Svc,
    root: End,
    t: u32,
}

/// A fresh volume in memory, as `fsd --memory` makes it.
fn rig() -> Rig {
    let mut device = MemoryDevice::new(vec![0u8; 4 << 20]);
    let now = Timestamp {
        secs: 1000,
        nanos: 0,
    };
    mkfs(
        &mut device,
        MkfsOptions {
            volume_id: 7,
            now,
            root_mode: 0o1777,
        },
    )
    .unwrap();
    let volume = Volume::mount(device, TestClock::new(now), MountOptions::default()).unwrap();
    let net = Fake::default();
    let mut service = Service::new(volume, net.clone());
    service.enable_fifos();
    let root = net.channel();
    assert!(service.add_root(root));
    Rig {
        net,
        service,
        root,
        t: 0,
    }
}

/// What a `fifo_open` came to.
#[derive(Debug, PartialEq, Eq)]
enum Opened {
    End(End),
    Refused(u32),
    Held,
}

impl Rig {
    fn run(&mut self) {
        while self.service.step() {}
    }

    fn post<'a, M: Message<'a>>(&mut self, end: End, m: &M, handles: Vec<End>) -> u32 {
        self.t += 1;
        let mut out = vec![0; 8192];
        let n = encode(m, self.t, &mut out).unwrap();
        out.truncate(n);
        self.net
            .0
            .borrow_mut()
            .queues
            .get_mut(&end)
            .unwrap()
            .push_back((out, handles));
        self.t
    }

    fn reply(&mut self, end: End) -> Option<(Vec<u8>, Vec<End>)> {
        self.net.0.borrow_mut().replies.get_mut(&end)?.pop_front()
    }

    /// `open_with_mode`: the new connection and the mode the reply gave.
    fn open(&mut self, path: &str, flags: u32, mode: u32, wanted: u32) -> Result<(End, u32), u32> {
        let end = self.net.channel();
        let root = self.root;
        self.post(
            root,
            &proto::OpenWithMode {
                flags: Flags(flags),
                rights: Flags(wanted),
                description: Flags(0),
                mode,
                path: Path(path),
            },
            vec![end],
        );
        self.run();
        let (bytes, _) = self.reply(root).expect("a reply");
        let (_, r) = decode::<proto::OpenWithModeReply<'_>>(&bytes, 0).unwrap();
        if r.status.0 == status::OK {
            Ok((end, r.mode))
        } else {
            Err(r.status.0)
        }
    }

    /// `mkfifo`: an exclusive create with `S_IFIFO`, then close.
    fn mkfifo(&mut self, path: &str) {
        let flags = open_flags::CREATE | open_flags::EXCLUSIVE;
        let (end, mode) = self
            .open(path, flags, S_IFIFO | 0o600, rights::READ)
            .unwrap();
        assert_eq!(mode & proto::S_IFMT, S_IFIFO);
        self.net.client_close(end);
        self.run();
    }

    fn fifo_open_on(&mut self, conn: End, side: u32, nonblock: bool) -> Opened {
        self.post(
            conn,
            &proto::FifoOpen {
                side: Below(side),
                nonblock: Below(u32::from(nonblock)),
            },
            vec![],
        );
        self.run();
        self.answer(conn)
    }

    fn answer(&mut self, conn: End) -> Opened {
        match self.reply(conn) {
            None => Opened::Held,
            Some((bytes, handles)) => {
                let (h, _) = Header::decode(&bytes).unwrap();
                if h.ordinal == ERROR_ORDINAL {
                    Opened::Refused(decode::<ErrorReply>(&bytes, 0).unwrap().1.status)
                } else {
                    let (_, m) = decode::<proto::FifoOpenReply>(&bytes, 1).unwrap();
                    assert_eq!(m.status.0, status::OK);
                    Opened::End(handles[0])
                }
            }
        }
    }

    /// `open(path, O_RDONLY or O_WRONLY [| O_NONBLOCK])` of a FIFO, as
    /// `libposix` does it: the connection, then `fifo_open` on it.
    fn fifo(&mut self, path: &str, side: u32, nonblock: bool) -> (End, Opened) {
        let wanted = if side == fifo_side::READ {
            rights::READ
        } else {
            rights::WRITE
        };
        let (conn, mode) = self.open(path, 0, 0, wanted).unwrap();
        assert_eq!(mode & proto::S_IFMT, S_IFIFO);
        (conn, self.fifo_open_on(conn, side, nonblock))
    }

    fn end(o: &Opened) -> End {
        match o {
            Opened::End(e) => *e,
            o => panic!("not an end: {o:?}"),
        }
    }

    /// The client closes an fd: its stream end and its connection.
    fn close(&mut self, conn: End, end: Option<End>) {
        if let Some(e) = end {
            self.net.client_close(e);
        }
        self.net.client_close(conn);
        self.run();
    }
}

#[test]
fn gnu_makes_open_sequence_works() {
    // INV-0107-4: the top make's `O_RDONLY|O_NONBLOCK` with no writer, then
    // its `O_WRONLY` with that reader there; then a sub-make's blocking
    // `O_RDONLY`, which a writer answers at once and which reads the same
    // stream (its later `F_SETFL O_NONBLOCK` is the library's own flag).
    let mut r = rig();
    r.mkfifo("GMfifo1");
    let (rc, read) = r.fifo("GMfifo1", fifo_side::READ, true);
    let read = Rig::end(&read);
    let (wc, write) = r.fifo("GMfifo1", fifo_side::WRITE, false);
    let write = Rig::end(&write);
    let pair = match r.net.side(read) {
        Side::Read(p) => p,
        s => panic!("{s:?}"),
    };
    assert_eq!(r.net.side(write), Side::Write(pair), "one stream");
    let (sc, sub) = r.fifo("GMfifo1", fifo_side::READ, false);
    let sub = Rig::end(&sub);
    assert_eq!(
        r.net.side(sub),
        Side::Read(pair),
        "the sub-make reads the same tokens"
    );
    // The sub-make ends; the top make's ends still work.
    r.close(sc, Some(sub));
    assert_eq!(
        r.net.holders(Side::Write(pair)),
        2,
        "the writer's and the service's"
    );
    r.close(rc, Some(read));
    r.close(wc, Some(write));
    assert_eq!(
        r.service.open_fifos(),
        0,
        "nothing kept once every side is gone"
    );
    assert_eq!(
        r.net.holders(Side::Read(pair)) + r.net.holders(Side::Write(pair)),
        0
    );
}

#[test]
fn a_nonblocking_write_open_with_no_reader_is_enxio() {
    // INV-0006-9, the second behaviour.
    let mut r = rig();
    r.mkfifo("f");
    let (_, o) = r.fifo("f", fifo_side::WRITE, true);
    assert_eq!(o, Opened::Refused(status::NOT_FOUND));
    assert_eq!(r.service.open_fifos(), 0);
    // With a reader it opens.
    let (_, read) = r.fifo("f", fifo_side::READ, true);
    assert!(matches!(read, Opened::End(_)));
    let (_, o) = r.fifo("f", fifo_side::WRITE, true);
    assert!(matches!(o, Opened::End(_)));
}

#[test]
fn a_blocking_open_waits_for_the_other_side() {
    // INV-0006-9, the third behaviour, both ways.
    let mut r = rig();
    r.mkfifo("f");
    let (rc, read) = r.fifo("f", fifo_side::READ, false);
    assert_eq!(read, Opened::Held, "no writer yet");
    let (_, write) = r.fifo("f", fifo_side::WRITE, false);
    assert!(matches!(write, Opened::End(_)), "the reader was there");
    let read = r.answer(rc);
    assert!(
        matches!(read, Opened::End(_)),
        "answered when the writer came"
    );
    r.mkfifo("g");
    let (wc, write) = r.fifo("g", fifo_side::WRITE, false);
    assert_eq!(write, Opened::Held);
    let (_, read) = r.fifo("g", fifo_side::READ, true);
    assert!(matches!(read, Opened::End(_)));
    assert!(matches!(r.answer(wc), Opened::End(_)));
}

#[test]
fn a_held_open_goes_with_its_connection() {
    let mut r = rig();
    r.mkfifo("f");
    let (rc, read) = r.fifo("f", fifo_side::READ, false);
    assert_eq!(read, Opened::Held);
    r.close(rc, None);
    assert_eq!(r.service.open_fifos(), 0);
    // The reader that gave up is not counted: a non-blocking writer is
    // refused.
    let (_, o) = r.fifo("f", fifo_side::WRITE, true);
    assert_eq!(o, Opened::Refused(status::NOT_FOUND));
}

#[test]
fn the_last_writer_gone_is_end_of_file_and_the_last_reader_gone_is_peer_closed() {
    // ADR-0107 item 15, the FIFO end-of-file risk of plan.md.
    let mut r = rig();
    r.mkfifo("f");
    let (rc, read) = r.fifo("f", fifo_side::READ, true);
    let read = Rig::end(&read);
    let Side::Read(pair) = r.net.side(read) else {
        panic!()
    };
    // Before any writer the service holds the write side: no end of file.
    assert_eq!(r.net.holders(Side::Write(pair)), 1);
    let (w1c, w1) = r.fifo("f", fifo_side::WRITE, false);
    let (w2c, w2) = r.fifo("f", fifo_side::WRITE, false);
    r.close(w1c, Some(Rig::end(&w1)));
    assert_eq!(r.net.holders(Side::Write(pair)), 2, "one writer left");
    r.close(w2c, Some(Rig::end(&w2)));
    assert_eq!(
        r.net.holders(Side::Write(pair)),
        0,
        "no write end anywhere: the reader reads end of file"
    );
    assert_eq!(
        r.net.holders(Side::Read(pair)),
        2,
        "the reader's and the service's"
    );
    // A writer after that gets a fresh pair (the stated deviation).
    let (wc, w) = r.fifo("f", fifo_side::WRITE, false);
    let w = Rig::end(&w);
    assert_ne!(r.net.side(w), Side::Write(pair));
    // The last reader goes: no read end of the new pair is left either, so
    // the writer's next write is `PeerClosed`.
    r.close(rc, Some(read));
    let Side::Write(fresh) = r.net.side(w) else {
        panic!()
    };
    assert_eq!(r.net.holders(Side::Read(fresh)), 0);
    r.close(wc, Some(w));
    assert_eq!(r.service.open_fifos(), 0);
}

#[test]
fn unlinking_a_fifo_or_a_file_while_open_keeps_the_connections_working() {
    // INV-0107-5.
    let mut r = rig();
    r.mkfifo("f");
    let (rc, read) = r.fifo("f", fifo_side::READ, true);
    let root = r.root;
    r.post(root, &proto::Remove { name: Name("f") }, vec![]);
    r.run();
    let (bytes, _) = r.reply(root).unwrap();
    assert_eq!(
        decode::<proto::Done>(&bytes, 0).unwrap().1.status.0,
        status::OK
    );
    assert_eq!(
        r.open("f", 0, 0, rights::READ),
        Err(status::NOT_FOUND),
        "the name is gone"
    );
    // The connection still opens the other side by itself... no: a new
    // writer cannot find it, but the open reader still works and a second
    // side on the same connection is refused.
    assert_eq!(
        r.fifo_open_on(rc, fifo_side::WRITE, true),
        Opened::Refused(status::INVALID_ARGUMENT)
    );
    r.close(rc, Some(Rig::end(&read)));
    assert_eq!(r.service.open_fifos(), 0);
    // A file: written after its unlink, read back through the same
    // connection, freed with the last.
    let (f, _) = r
        .open(
            "data",
            open_flags::CREATE,
            0o644,
            rights::READ | rights::WRITE,
        )
        .unwrap();
    r.post(root, &proto::Remove { name: Name("data") }, vec![]);
    r.run();
    r.reply(root).unwrap();
    r.post(
        f,
        &proto::Write {
            offset: 0,
            flags: Flags(0),
            data: zoraya_protocol::Data(b"kept"),
        },
        vec![],
    );
    r.run();
    let (bytes, _) = r.reply(f).unwrap();
    assert_eq!(decode::<proto::Done>(&bytes, 0).unwrap().1.count, 4);
    r.post(
        f,
        &proto::Read {
            offset: 0,
            length: 10,
        },
        vec![],
    );
    r.run();
    let (bytes, _) = r.reply(f).unwrap();
    assert_eq!(
        decode::<proto::ReadReply<'_>>(&bytes, 0).unwrap().1.data.0,
        b"kept"
    );
    let before = r.service.open_count();
    r.close(f, None);
    assert_eq!(r.service.open_count(), before - 1);
}

#[test]
fn fifo_open_is_refused_where_it_has_no_meaning() {
    let mut r = rig();
    // A plain file.
    let (f, _) = r
        .open("plain", open_flags::CREATE, 0o644, rights::READ)
        .unwrap();
    assert_eq!(
        r.fifo_open_on(f, fifo_side::READ, true),
        Opened::Refused(status::INVALID_ARGUMENT)
    );
    // A side the connection's rights do not allow.
    r.mkfifo("p");
    let (c, _) = r.open("p", 0, 0, rights::READ).unwrap();
    assert_eq!(
        r.fifo_open_on(c, fifo_side::WRITE, false),
        Opened::Refused(status::ACCESS_DENIED)
    );
    // No stream to be had.
    r.net.0.borrow_mut().no_streams = true;
    assert_eq!(
        r.fifo_open_on(c, fifo_side::READ, true),
        Opened::Refused(status::LIMIT_REACHED)
    );
    assert_eq!(r.service.open_fifos(), 0);
    // A service whose type bits are verbatim (the root volume's).
    let mut v = rig();
    let mut device = MemoryDevice::new(vec![0u8; 1 << 20]);
    let now = Timestamp { secs: 1, nanos: 0 };
    mkfs(
        &mut device,
        MkfsOptions {
            volume_id: 1,
            now,
            root_mode: 0o755,
        },
    )
    .unwrap();
    let volume = Volume::mount(device, TestClock::new(now), MountOptions::default()).unwrap();
    let net = Fake::default();
    let mut plain = Service::new(volume, net.clone());
    let root = net.channel();
    assert!(plain.add_root(root));
    v.net = net;
    v.service = plain;
    v.root = root;
    // Its volume keeps 12 mode bits (ADR-0014): a FIFO cannot be made...
    let flags = open_flags::CREATE | open_flags::EXCLUSIVE;
    assert_eq!(
        v.open("q", flags, S_IFIFO | 0o600, rights::READ),
        Err(status::INVALID_ARGUMENT)
    );
    // ...and `fifo_open` means nothing there.
    let (c, mode) = v.open("q", flags, 0o600, rights::READ).unwrap();
    assert_eq!(mode & proto::S_IFMT, 0);
    assert_eq!(
        v.fifo_open_on(c, fifo_side::READ, true),
        Opened::Refused(status::UNSUPPORTED)
    );
}

#[test]
fn a_memory_device_keeps_what_it_was_given_and_refuses_past_its_end() {
    use zoraya_fs::BlockDevice;
    let new_block = || Box::new([0u8; zoraya_fs::BLOCK_SIZE]);
    let mut d = MemoryDevice::new(vec![0u8; 3 * 4096 + 100]);
    assert_eq!(d.block_count(), 3);
    let mut b = new_block();
    b[0] = 9;
    b[4095] = 8;
    d.write(&[(2, &b)]).unwrap();
    let mut back = new_block();
    d.read(2, &mut back).unwrap();
    assert_eq!((back[0], back[4095]), (9, 8));
    assert!(d.read(3, &mut back).is_err());
    // A batch with one bad block writes nothing.
    let zero = new_block();
    assert!(d.write(&[(2, &zero), (3, &zero)]).is_err());
    d.read(2, &mut back).unwrap();
    assert_eq!(back[0], 9);
    assert!(d.flush_supported());
    assert!(d.flush().is_ok());
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
zoraya_test_alloc::cap_memory!();

//! The temporary directory service's socket nodes (proposed ADR-0439,
//! ADR-0412 item 5) through the service loop, on a `MemoryDevice`: `bind` is
//! an exclusive create with `S_IFSOCK`, `socket_listen` gives the node a
//! listener, `socket_connect` forwards the connector's far end to it. Who
//! may listen and connect (`WRITE`), what a node shows, the bounds, and that
//! every handle the service takes is closed again.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use zoraya_fs::{MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::memory::MemoryDevice;
use zoraya_fs_service::{
    HandleKind, Incoming, Interest, Outgoing, Service, Transport, Wake, outgoing_for,
};
use zoraya_fs_testkit::clock::TestClock;
use zoraya_proto_file::{self as proto, S_IFSOCK, open_flags, rights, status};
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
    /// Ends whose queue is full: a write answers `Full`.
    full: BTreeSet<End>,
    /// Ends that refuse what they are sent (the kernel would not move a
    /// handle): a write answers `Refused`.
    refuse: BTreeSet<End>,
    /// A token's copies: copy to original.
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

    /// The client closes its end of a connection or a stream.
    fn client_close(&self, end: End) {
        let mut n = self.0.borrow_mut();
        n.closed.insert(end);
        n.streams.remove(&end);
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
        if n.full.contains(&end) {
            return Outgoing::Full;
        }
        if n.refuse.contains(&end) {
            return Outgoing::Refused;
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
        let side = self.0.borrow().streams.get(&end).copied();
        let Some(side) = side else {
            let copy = self.end(HandleKind::Token);
            let mut n = self.0.borrow_mut();
            let root = n.tokens.get(&end).copied().unwrap_or(end);
            n.tokens.insert(copy, root);
            return Some(copy);
        };
        let copy = self.end(HandleKind::Other);
        self.0.borrow_mut().streams.insert(copy, side);
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

fn rig_with(sockets: bool) -> Rig {
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
    if sockets {
        service.enable_sockets();
    }
    let root = net.channel();
    assert!(service.add_root(root));
    Rig {
        net,
        service,
        root,
        t: 0,
    }
}

fn rig() -> Rig {
    rig_with(true)
}

const BIND: u32 = open_flags::CREATE | open_flags::EXCLUSIVE;

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

    /// `bind`: the exclusive create with `S_IFSOCK`; the connection stays.
    fn bind(&mut self, path: &str) -> Result<End, u32> {
        let (end, mode) = self.open(path, BIND, S_IFSOCK | 0o600, rights::READ | rights::WRITE)?;
        assert_eq!(mode & proto::S_IFMT, S_IFSOCK);
        Ok(end)
    }

    /// A new connection to `path`, as `connect` and `listen` make one.
    fn connection(&mut self, path: &str, wanted: u32) -> Result<End, u32> {
        self.open(path, 0, 0, wanted).map(|(end, _)| end)
    }

    fn status_of(&mut self, conn: End) -> u32 {
        let (bytes, _) = self.reply(conn).expect("a reply");
        let (h, _) = Header::decode(&bytes).unwrap();
        if h.ordinal == ERROR_ORDINAL {
            decode::<ErrorReply>(&bytes, 0).unwrap().1.status
        } else {
            decode::<proto::Done>(&bytes, 0).unwrap().1.status.0
        }
    }

    /// `socket_listen` on `conn` with a fresh channel end; the status and
    /// that end.
    fn listen(&mut self, conn: End, sock_type: u32) -> (u32, End) {
        let end = self.net.channel();
        self.post(
            conn,
            &proto::SocketListen {
                sock_type: Below(sock_type),
            },
            vec![end],
        );
        self.run();
        (self.status_of(conn), end)
    }

    /// A packet connect on `conn`; the status and the handle it carried.
    fn connect(&mut self, conn: End, sock_type: u32, record: &[u8]) -> (u32, End) {
        let far = self.net.channel();
        self.post(
            conn,
            &proto::SocketConnect {
                sock_type: Below(sock_type),
                record: zoraya_protocol::Data(record),
            },
            vec![far],
        );
        self.run();
        (self.status_of(conn), far)
    }

    fn connect_stream(&mut self, conn: End, record: &[u8]) -> (u32, [End; 3]) {
        let far = [self.net.channel(), self.net.channel(), self.net.channel()];
        self.post(
            conn,
            &proto::SocketConnectStream {
                sock_type: Below(1),
                record: zoraya_protocol::Data(record),
            },
            far.to_vec(),
        );
        self.run();
        (self.status_of(conn), far)
    }

    fn is_closed(&self, end: End) -> bool {
        self.net.0.borrow().closed.contains(&end)
    }

    fn remove(&mut self, name: &str) -> u32 {
        let root = self.root;
        self.post(root, &proto::Remove { name: Name(name) }, vec![]);
        self.run();
        self.status_of(root)
    }
}

#[test]
fn bind_makes_a_socket_node_that_shows_its_type_once() {
    let mut r = rig();
    let conn = r.bind("sock").unwrap();
    assert_eq!(r.service.socket_nodes(), (1, 0));
    // A second bind to the name: the node exists.
    assert_eq!(r.bind("sock"), Err(status::EXISTS));
    assert_eq!(r.service.socket_nodes(), (1, 0));
    // Opened again, it reports the type bits.
    let (_, mode) = r.open("sock", 0, 0, rights::READ).unwrap();
    assert_eq!(mode & proto::S_IFMT, S_IFSOCK);
    r.net.client_close(conn);
    r.run();
}

#[test]
fn a_service_without_socket_nodes_refuses_them_and_the_requests() {
    let mut r = rig_with(false);
    // `S_IFSOCK` is one of the type bits the service never stored.
    let made = r.open("sock", BIND, S_IFSOCK | 0o600, rights::READ);
    assert!(made.is_err() || made.is_ok_and(|(_, m)| m & proto::S_IFMT != S_IFSOCK));
    let (conn, _) = r
        .open("file", BIND, 0o600, rights::READ | rights::WRITE)
        .unwrap();
    let (s, end) = r.listen(conn, 1);
    assert_eq!(s, status::UNSUPPORTED);
    let (s, far) = r.connect(conn, 1, &[0; 24]);
    assert_eq!(s, status::UNSUPPORTED);
    // `ready` closed what it was given.
    assert!(r.is_closed(end) && r.is_closed(far));
}

#[test]
fn a_connect_is_forwarded_to_the_listener_with_its_handles() {
    let mut r = rig();
    let conn = r.bind("sock").unwrap();
    let base = r.service.open_count();
    let (s, listener) = r.listen(conn, 5);
    assert_eq!(s, status::OK);
    assert_eq!(r.service.socket_nodes(), (1, 1));
    assert_eq!(
        r.service.open_count(),
        base + 1,
        "the listener end is charged"
    );
    // A second program connects through its own connection.
    let c = r.connection("sock", rights::CONNECT).unwrap();
    let (s, far) = r.connect(c, 5, &[7; 24]);
    assert_eq!(s, status::OK);
    assert!(!r.is_closed(far), "the far end went on, it was not closed");
    // The service wrote it to its end of the listener channel, which is the
    // handle `listen` moved to it.
    let (bytes, handles) = r.reply(listener).expect("forwarded");
    assert_eq!(bytes, vec![7; 24]);
    assert_eq!(handles, vec![far]);
    // A stream connect forwards three.
    let c2 = r.connection("sock", rights::CONNECT).unwrap();
    let (s, _) = r.connect_stream(c2, &[1; 24]);
    assert_eq!(
        s,
        status::INVALID_ARGUMENT,
        "a stream into a packet listener"
    );
}

#[test]
fn a_stream_listener_takes_the_three_ends_in_one_message() {
    let mut r = rig();
    let conn = r.bind("sock").unwrap();
    let (s, listener) = r.listen(conn, 1);
    assert_eq!(s, status::OK);
    let c = r.connection("sock", rights::CONNECT).unwrap();
    let (s, far) = r.connect_stream(c, &[9; 24]);
    assert_eq!(s, status::OK);
    let (bytes, handles) = r.reply(listener).unwrap();
    assert_eq!(bytes, vec![9; 24]);
    assert_eq!(handles, far.to_vec());
}

#[test]
fn nothing_listens_until_the_node_has_a_listener_and_nothing_after() {
    let mut r = rig();
    let conn = r.bind("sock").unwrap();
    let c = r.connection("sock", rights::CONNECT).unwrap();
    // Bound, not listening: refused, and the far end is not kept.
    let (s, far) = r.connect(c, 1, &[0; 24]);
    assert_eq!(s, status::NOT_FOUND);
    assert!(r.is_closed(far));
    // A regular file is no socket: the same answer.
    let (file, _) = r
        .open(
            "plain",
            BIND,
            0o600,
            rights::READ | rights::WRITE | rights::CONNECT,
        )
        .unwrap();
    let (s, _) = r.connect(file, 1, &[0; 24]);
    assert_eq!(s, status::NOT_FOUND);
    let (s, end) = r.listen(file, 1);
    assert_eq!(s, status::INVALID_ARGUMENT);
    assert!(r.is_closed(end));
    // Listening; then the listening connection goes: the listener with it.
    let base = r.service.open_count();
    let (s, listener) = r.listen(conn, 1);
    assert_eq!(s, status::OK);
    r.net.client_close(conn);
    r.run();
    assert_eq!(r.service.socket_nodes(), (1, 0), "the node stays, inert");
    assert!(r.is_closed(listener));
    assert_eq!(
        r.service.open_count(),
        base - 1,
        "its end is released, and so is the connection"
    );
    let (s, far) = r.connect(c, 1, &[0; 24]);
    assert_eq!(s, status::NOT_FOUND);
    assert!(r.is_closed(far));
    // The node is still there to bind to again: no, the name is taken.
    assert_eq!(r.bind("sock"), Err(status::EXISTS));
}

#[test]
fn listening_and_connecting_need_write() {
    let mut r = rig();
    let conn = r.bind("sock").unwrap();
    let (s, _) = r.listen(conn, 1);
    assert_eq!(s, status::OK);
    let read_only = r.connection("sock", rights::READ).unwrap();
    let (s, far) = r.connect(read_only, 1, &[0; 24]);
    assert_eq!(s, status::ACCESS_DENIED);
    assert!(r.is_closed(far));
    // WRITE alone does not connect: the right is CONNECT (ADR-0465).
    let write_only = r.connection("sock", rights::WRITE).unwrap();
    let (s, far) = r.connect(write_only, 1, &[0; 24]);
    assert_eq!(s, status::ACCESS_DENIED);
    assert!(r.is_closed(far));
    let (s, end) = r.listen(read_only, 1);
    assert_eq!(s, status::ACCESS_DENIED);
    assert!(r.is_closed(end));
}

#[test]
fn one_listener_a_node_and_one_node_a_connection_and_only_two_types() {
    let mut r = rig();
    let a = r.bind("a").unwrap();
    let b = r.connection("a", rights::WRITE | rights::CONNECT).unwrap();
    assert_eq!(r.listen(a, 1).0, status::OK);
    // Another connection to the same node, with WRITE: not the binder's, refused
    // (it could not take the name over), and taken anyway.
    let (s, end) = r.listen(b, 1);
    assert_eq!(s, status::ACCESS_DENIED);
    assert!(r.is_closed(end));
    // The same connection again: refused.
    let (s, end) = r.listen(a, 1);
    assert_eq!(s, status::INVALID_ARGUMENT);
    assert!(r.is_closed(end));
    // A datagram socket has no listener.
    let c = r.bind("c").unwrap();
    let (s, end) = r.listen(c, 2);
    assert_eq!(s, status::INVALID_ARGUMENT);
    assert!(r.is_closed(end));
    // A listener whose type is not the connector's.
    let (s, far) = r.connect(b, 5, &[0; 24]);
    assert_eq!(s, status::INVALID_ARGUMENT);
    assert!(r.is_closed(far));
    // The handle must be a channel.
    let d = r.bind("d").unwrap();
    let stream_end = r.net.end(HandleKind::Other);
    r.post(
        d,
        &proto::SocketListen {
            sock_type: Below(1),
        },
        vec![stream_end],
    );
    r.run();
    assert_eq!(r.status_of(d), status::INVALID_ARGUMENT);
    assert!(r.is_closed(stream_end));
}

#[test]
fn a_full_queue_refuses_the_connect_and_keeps_nothing() {
    let mut r = rig();
    let conn = r.bind("sock").unwrap();
    let (_, listener) = r.listen(conn, 1);
    r.net.0.borrow_mut().full.insert(listener);
    let c = r.connection("sock", rights::CONNECT).unwrap();
    let (s, far) = r.connect(c, 1, &[0; 24]);
    assert_eq!(s, status::LIMIT_REACHED);
    assert!(
        r.is_closed(far),
        "the refused far end is closed, not queued"
    );
    // The listener lives: when the queue has room again, it is served.
    r.net.0.borrow_mut().full.clear();
    let (s, _) = r.connect(c, 1, &[0; 24]);
    assert_eq!(s, status::OK);
}

/// A connector whose handles the kernel will not move (rights narrower than
/// the entry names) must not be able to end the listener by it.
#[test]
fn a_far_end_the_kernel_refuses_is_the_connectors_fault_and_not_the_listeners_death() {
    let mut r = rig();
    let conn = r.bind("sock").unwrap();
    let (_, listener) = r.listen(conn, 1);
    r.net.0.borrow_mut().refuse.insert(listener);
    let c = r.connection("sock", rights::CONNECT).unwrap();
    let (s, far) = r.connect(c, 1, &[0; 24]);
    assert_eq!(s, status::INVALID_ARGUMENT);
    assert!(r.is_closed(far));
    assert_eq!(r.service.socket_nodes(), (1, 1), "the listener lives");
    assert!(!r.is_closed(listener));
    r.net.0.borrow_mut().refuse.clear();
    let (s, _) = r.connect(c, 1, &[0; 24]);
    assert_eq!(s, status::OK, "the next connector is served");
}

#[test]
fn a_listener_channel_that_closed_is_a_dead_listener() {
    let mut r = rig();
    let conn = r.bind("sock").unwrap();
    let (_, listener) = r.listen(conn, 1);
    let base = r.service.open_count();
    // The client end's closing shows as `Closed` on the service's write.
    r.net.client_close(listener);
    let c = r.connection("sock", rights::CONNECT).unwrap();
    let (s, far) = r.connect(c, 1, &[0; 24]);
    assert_eq!(s, status::NOT_FOUND);
    assert!(r.is_closed(far));
    assert_eq!(r.service.socket_nodes(), (1, 0));
    assert_eq!(
        r.service.open_count(),
        base,
        "the end was released: the connection `c` is one more"
    );
    // The connection that listened is free to listen again: the dead listener
    // is not still its own.
    let (s, _) = r.listen(conn, 1);
    assert_eq!(s, status::OK);
}

#[test]
fn unlink_frees_a_node_and_a_listening_node_stays_until_its_listener_goes() {
    let mut r = rig();
    // No listener: the entry goes with the name.
    let a = r.bind("a").unwrap();
    assert_eq!(r.remove("a"), status::OK);
    assert_eq!(r.service.socket_nodes(), (0, 0));
    r.net.client_close(a);
    // With a listener: it outlives its name, as a listening socket does.
    let b = r.bind("b").unwrap();
    let (s, _) = r.listen(b, 1);
    assert_eq!(s, status::OK);
    assert_eq!(r.remove("b"), status::OK);
    assert_eq!(r.service.socket_nodes(), (1, 1));
    // The name can be bound again, and is a new node.
    let b2 = r.bind("b").unwrap();
    assert_eq!(r.service.socket_nodes(), (2, 1));
    // No one can connect to the old node by name any more: `connection`
    // finds the new one.
    let c = r.connection("b", rights::CONNECT).unwrap();
    let (s, _) = r.connect(c, 1, &[0; 24]);
    assert_eq!(s, status::NOT_FOUND);
    r.net.client_close(b);
    r.run();
    assert_eq!(
        r.service.socket_nodes(),
        (1, 0),
        "the old node went with its listener"
    );
    r.net.client_close(b2);
}

/// `rename` of a directory token from the root: a token copy and the request.
fn rename(r: &mut Rig, old: &str, new: &str) -> u32 {
    let root = r.root;
    r.post(root, &proto::GetToken {}, vec![]);
    r.run();
    let (_, handles) = r.reply(root).unwrap();
    r.post(
        root,
        &proto::Rename {
            old: Name(old),
            new: Name(new),
            mode: Below(proto::rename_mode::REPLACE),
        },
        vec![handles[0]],
    );
    r.run();
    r.status_of(root)
}

#[test]
fn a_rename_of_a_name_onto_itself_leaves_the_node_its_name() {
    let mut r = rig();
    let a = r.bind("a").unwrap();
    assert_eq!(r.listen(a, 1).0, status::OK);
    assert_eq!(rename(&mut r, "a", "a"), status::OK);
    assert_eq!(r.service.socket_nodes(), (1, 1));
    // The node still has its name: when its listener goes it stays, inert. (A
    // node taken as unnamed would go with its listener.)
    r.net.client_close(a);
    r.run();
    assert_eq!(r.service.socket_nodes(), (1, 0));
}

#[test]
fn a_rename_over_a_socket_node_takes_its_name_and_the_listener_outlives_it() {
    let mut r = rig();
    let a = r.bind("a").unwrap();
    assert_eq!(r.listen(a, 1).0, status::OK);
    let f = r
        .open("f", BIND, 0o600, rights::READ | rights::WRITE)
        .unwrap()
        .0;
    r.net.client_close(f);
    r.run();
    assert_eq!(rename(&mut r, "f", "a"), status::OK);
    // The listener stays (its node is unnamed); a new bind to `a` is `EXISTS`
    // as `a` is the plain file now.
    assert_eq!(r.service.socket_nodes(), (1, 1));
    assert_eq!(r.bind("a"), Err(status::EXISTS));
    r.net.client_close(a);
    r.run();
    assert_eq!(r.service.socket_nodes(), (0, 0));
}

#[test]
fn publishing_over_a_socket_node_takes_its_name() {
    let mut r = rig();
    let a = r.bind("s").unwrap();
    r.net.client_close(a);
    r.run();
    assert_eq!(r.service.socket_nodes(), (1, 0));
    let (file, _) = r
        .open("ignored", open_flags::UNNAMED, 0o600, rights::ALL)
        .unwrap();
    let root = r.root;
    r.post(root, &proto::GetToken {}, vec![]);
    r.run();
    let (_, handles) = r.reply(root).unwrap();
    r.post(
        file,
        &proto::Publish {
            mode: Below(proto::publish_mode::REPLACE),
            name: Name("s"),
        },
        vec![handles[0]],
    );
    r.run();
    assert_eq!(r.status_of(file), status::OK);
    assert_eq!(
        r.service.socket_nodes(),
        (0, 0),
        "the name went to the file"
    );
}

#[test]
fn the_table_of_nodes_and_of_listeners_is_bounded() {
    let mut r = rig();
    let mut conns = Vec::new();
    for n in 0..zoraya_fs_service::MAX_SOCKET_NODES {
        conns.push(r.bind(&format!("s{n}")).unwrap());
    }
    assert_eq!(
        r.bind("one-more"),
        Err(status::LIMIT_REACHED),
        "past the table's bound"
    );
    // Unlinking frees an entry.
    assert_eq!(r.remove("s0"), status::OK);
    let extra = r.bind("one-more").unwrap();
    // Listeners: a bound of their own, by the binders' connections.
    for (n, c) in conns
        .iter()
        .enumerate()
        .skip(1)
        .take(zoraya_fs_service::MAX_LISTENERS)
    {
        assert_eq!(r.listen(*c, 1).0, status::OK, "listener {n}");
    }
    let (s, end) = r.listen(extra, 1);
    assert_eq!(s, status::TOO_MANY_OPEN);
    assert!(r.is_closed(end));
    // Past the bound a program that is not the binder is refused for that, and
    // counts nothing against anyone.
    let thief = r.connection("s100", rights::READ | rights::WRITE).unwrap();
    let open = r.service.open_count();
    let (s, end) = r.listen(thief, 1);
    assert_eq!(s, status::ACCESS_DENIED);
    assert!(r.is_closed(end));
    assert_eq!(
        r.service.open_count(),
        open,
        "nothing counted against the account"
    );
    assert_eq!(
        r.service.socket_nodes().1,
        zoraya_fs_service::MAX_LISTENERS,
        "no listener added"
    );
}

/// What `fsd` makes of a kernel error on a write (`fsd.rs` calls this): only a closed peer kills a listener.
#[test]
fn only_a_closed_peer_is_a_closed_write() {
    use zoraya_abi::Status;
    assert_eq!(outgoing_for(Status::WouldBlock), Outgoing::Full);
    assert_eq!(outgoing_for(Status::NoMemory), Outgoing::NoMemory);
    assert_eq!(outgoing_for(Status::OverQuota), Outgoing::NoMemory);
    for s in [Status::AccessDenied, Status::BadState, Status::BadHandle] {
        assert_eq!(outgoing_for(s), Outgoing::Refused, "{s:?}");
    }
    assert_eq!(outgoing_for(Status::PeerClosed), Outgoing::Closed);
    // Every other status leaves the peer's state alone only if it says so: they are all `Closed`.
    for s in Status::ALL {
        let o = outgoing_for(*s);
        let known = matches!(
            s,
            Status::WouldBlock
                | Status::NoMemory
                | Status::OverQuota
                | Status::AccessDenied
                | Status::BadState
                | Status::BadHandle
        );
        assert_eq!(o == Outgoing::Closed, !known, "{s:?}");
    }
}

/// Only the connection that made a node makes it listen: another program with `WRITE` on the file cannot
/// win the race between `bind` and `listen`, nor take an inert node over when its server has gone.
#[test]
fn only_the_binder_may_listen_and_a_node_is_inert_when_it_goes() {
    let mut r = rig();
    let a = r.bind("a").unwrap();
    let thief = r.connection("a", rights::READ | rights::WRITE).unwrap();
    let (s, end) = r.listen(thief, 1);
    assert_eq!(s, status::ACCESS_DENIED);
    assert!(r.is_closed(end));
    assert_eq!(r.service.socket_nodes(), (1, 0));
    // The binder goes without listening: the node stays, and nobody takes it.
    r.net.client_close(a);
    r.run();
    let (s, _) = r.listen(thief, 1);
    assert_eq!(s, status::ACCESS_DENIED);
    // A new connection that happens to get the freed index is not the owner either.
    let late = r.connection("a", rights::READ | rights::WRITE).unwrap();
    assert_eq!(r.listen(late, 1).0, status::ACCESS_DENIED);
    assert_eq!(r.service.socket_nodes(), (1, 0));
}

#[test]
fn a_service_with_no_requests_leaves_no_handle_of_a_dead_connection() {
    let mut r = rig();
    let base = r.service.open_count();
    for n in 0..5 {
        let c = r.bind(&format!("x{n}")).unwrap();
        let (s, l) = r.listen(c, 1);
        assert_eq!(s, status::OK);
        r.net.client_close(c);
        r.run();
        assert!(r.is_closed(l));
        assert_eq!(r.remove(&format!("x{n}")), status::OK);
    }
    assert_eq!(r.service.socket_nodes(), (0, 0));
    assert_eq!(r.service.open_count(), base);
}

zoraya_test_alloc::cap_memory!();

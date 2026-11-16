//! Socket files over the fake kernel and the real temporary directory
//! service (ADR-0412 item 5, plan part F7, proposed ADR-0439): `bind`,
//! `listen`, `connect` and `accept` for `AF_UNIX` sockets between two
//! processes. INV-0412-3 (a socket file is made only in the line's `/tmp`),
//! the refusals of each call, what a program that cannot see the directory
//! cannot do, the backlog bound and that every handle is closed again.

mod kernel;

use std::cell::RefCell;
use std::rc::Rc;

use kernel::*;
use zoraya_abi::posix::{EntryKind, Facts};
use zoraya_posix::errno::*;
use zoraya_posix::poll::*;
use zoraya_posix::process::{AT_FDCWD, Posix, o};
use zoraya_posix::socket::{SO_ACCEPTCONN, SOCK_NONBLOCK, SOL_SOCKET};
use zoraya_posix::sys::Sys;
use zoraya_posix::unix::{AF_UNIX, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM};

zoraya_test_alloc::cap_memory!();

/// `struct sockaddr_un` for `path`.
fn un(path: &str) -> Vec<u8> {
    let mut a = (AF_UNIX as u16).to_ne_bytes().to_vec();
    a.extend_from_slice(path.as_bytes());
    a.push(0);
    a
}

/// A process whose table has `/`, `/dev/null`, `/tmp` (the temporary
/// directory service) and `/work` (a plain volume that keeps no socket nodes).
fn tmp_rig() -> (Rig, Rc<RefCell<Svc>>) {
    let mut r = rig();
    let net = r.net.clone();
    let (service, tmp) = tmp_volume(&net, 1024);
    r.p.sys.services.push(service.clone());
    r.p.add_entry("/tmp", EntryKind::Tmp, Some(tmp));
    let (work, wc) = volume(&net, 256);
    r.p.sys.services.push(work);
    r.p.add_entry("/work", EntryKind::Directory, Some(wc));
    (r, service)
}

/// A second process over the same kernel and services, with its own
/// connection to `/tmp` if `with_tmp`, and nothing else.
fn peer(r: &Rig, service: &Rc<RefCell<Svc>>, with_tmp: bool) -> Rig {
    let net = r.net.clone();
    let mut fake = Fake::new(net.clone(), r.p.sys.services.clone());
    let arena = fake.memory_create(ARENA as u64).unwrap();
    let base = fake.memory_map(arena, ARENA as u64).unwrap();
    let mut p = Posix::new(
        fake,
        Facts {
            uid: 1000,
            gid: 100,
            umask: 0o022,
            ..Default::default()
        },
    );
    let (report, reports) = p.sys.channel_create().unwrap();
    p.set_report(Some(report));
    if with_tmp {
        let h = another_root(&net, service);
        let e = p.add_entry("/tmp", EntryKind::Tmp, Some(h));
        p.set_cwd(e, "", zoraya_posix::path::Form::EntryRelative);
    }
    Rig {
        p,
        net,
        arena: Arena { base },
        root: r.root,
        reports,
    }
}

fn sock(r: &mut Rig, ty: i32) -> i32 {
    r.p.socket(AF_UNIX, ty, 0).unwrap()
}

/// A listening socket of `ty` bound at `path`.
fn server(r: &mut Rig, ty: i32, path: &str) -> i32 {
    let s = sock(r, ty);
    r.p.bind(s, &un(path)).unwrap();
    r.p.listen(s, 8).unwrap();
    s
}

/// The same, non-blocking: `accept` with nothing waiting is `EAGAIN`.
fn server_nb(r: &mut Rig, ty: i32, path: &str) -> i32 {
    let s = r.p.socket(AF_UNIX, ty | SOCK_NONBLOCK, 0).unwrap();
    r.p.bind(s, &un(path)).unwrap();
    r.p.listen(s, 8).unwrap();
    s
}

fn revents(r: &mut Rig, fd: i32, events: i16) -> i16 {
    let mut f = [PollFd {
        fd,
        events,
        revents: 0,
    }];
    loop {
        if r.p.poll_once(&mut f, Some(0)).unwrap().is_some() {
            return f[0].revents;
        }
    }
}

// ------------------------------------------------------------ the whole way

#[test]
fn a_stream_connects_by_path_and_both_sides_talk() {
    let (mut a, svc) = tmp_rig();
    let mut b = peer(&a, &svc, true);
    let s = server_nb(&mut a, SOCK_STREAM, "/tmp/sock");
    // The listener names itself, and nothing is accepted yet.
    let name = a.p.unix_sockaddr(s, false).unwrap();
    assert_eq!(name, un("/tmp/sock"));
    assert_eq!(
        a.p.accept(s, true, false),
        Err(EAGAIN),
        "nothing waits to be accepted"
    );
    assert_eq!(revents(&mut a, s, POLLIN), 0);
    let c = sock(&mut b, SOCK_STREAM);
    assert_eq!(b.p.connect(c, &un("/tmp/sock")), Ok(()));
    assert_eq!(b.p.unix_sockaddr(c, true).unwrap(), un("/tmp/sock"));
    // The pending connection makes the listener readable.
    assert_eq!(revents(&mut a, s, POLLIN) & POLLIN, POLLIN);
    let d = a.p.accept(s, false, false).unwrap();
    assert_eq!(a.p.getsockopt(s, SOL_SOCKET, SO_ACCEPTCONN), Ok(1));
    assert_eq!(a.p.getsockopt(d, SOL_SOCKET, SO_ACCEPTCONN), Ok(0));
    // The accepted socket has the listener's name and no peer name.
    assert_eq!(a.p.unix_sockaddr(d, false).unwrap(), un("/tmp/sock"));
    assert_eq!(
        a.p.unix_sockaddr(d, true).unwrap(),
        (AF_UNIX as u16).to_ne_bytes().to_vec()
    );
    // The two processes exchange bytes through streams the connector made.
    b.write(c, b"hello server").unwrap();
    assert_eq!(a.read(d, 100).unwrap(), b"hello server");
    a.write(d, b"hello client").unwrap();
    assert_eq!(b.read(c, 100).unwrap(), b"hello client");
    // A close is an end, and a second connection is a second pair.
    let c2 = sock(&mut b, SOCK_STREAM);
    b.p.connect(c2, &un("/tmp/sock")).unwrap();
    let d2 = a.p.accept(s, false, true).unwrap();
    b.write(c2, b"two").unwrap();
    assert_eq!(a.read(d2, 100).unwrap(), b"two");
    b.p.close(c).unwrap();
    assert_eq!(a.read(d, 100).unwrap(), b"");
    a.p.close(d).unwrap();
    a.p.close(d2).unwrap();
}

#[test]
fn a_seqpacket_socket_keeps_its_message_boundaries_through_a_socket_file() {
    let (mut a, svc) = tmp_rig();
    let mut b = peer(&a, &svc, true);
    let s = server(&mut a, SOCK_SEQPACKET, "/tmp/pk");
    let c = sock(&mut b, SOCK_SEQPACKET);
    b.p.connect(c, &un("/tmp/pk")).unwrap();
    let d = a.p.accept(s, false, false).unwrap();
    b.write(c, b"one").unwrap();
    b.write(c, b"second").unwrap();
    assert_eq!(a.read(d, 100).unwrap(), b"one");
    assert_eq!(a.read(d, 100).unwrap(), b"second");
    a.write(d, b"reply").unwrap();
    assert_eq!(b.read(c, 100).unwrap(), b"reply");
}

#[test]
fn accept4_sets_the_flags_of_the_new_socket_alone() {
    let (mut a, svc) = tmp_rig();
    let mut b = peer(&a, &svc, true);
    let s = server_nb(&mut a, SOCK_STREAM, "/tmp/f");
    let c = sock(&mut b, SOCK_STREAM);
    b.p.connect(c, &un("/tmp/f")).unwrap();
    let d = a.p.accept(s, true, true).unwrap();
    // Non-blocking: a read with nothing is EAGAIN, not a wait.
    assert_eq!(a.read(d, 10), Err(EAGAIN));
    assert_eq!(a.p.fds.cloexec(d), Ok(true), "SOCK_CLOEXEC");
    // The listener is neither, and a second accept with nothing queued on a
    // non-blocking accept is EAGAIN.
    assert_eq!(a.p.fds.cloexec(s), Ok(false));
    assert_eq!(a.p.accept(s, true, false), Err(EAGAIN));
}

// ------------------------------------------------------------ INV-0412-3

/// INV-0412-3: `bind` succeeds only in the line's `/tmp`.
#[test]
fn bind_makes_a_socket_file_only_in_the_lines_tmp() {
    let (mut r, _svc) = tmp_rig();
    let s = sock(&mut r, SOCK_STREAM);
    // Inside `/tmp`: yes, and the file is an `S_IFSOCK` object.
    r.p.bind(s, &un("/tmp/ok")).unwrap();
    let st = r.p.stat_at(AT_FDCWD, b"/tmp/ok", 0).unwrap();
    let mode = u32::from_le_bytes(st[16..20].try_into().unwrap());
    assert_eq!(mode & 0o170000, 0o140000, "S_IFSOCK");
    assert_eq!(mode & 0o777, 0o755, "0777 and the umask");
    // In a directory of another service, or of the root volume: refused.
    // The two refusals that matter by name.
    let s3 = sock(&mut r, SOCK_STREAM);
    assert_eq!(
        r.p.bind(s3, &un("/work/no")),
        Err(EACCES),
        "another service's directory"
    );
    assert_eq!(
        r.p.bind(s3, &un("/no")),
        Err(EACCES),
        "the root volume's directory"
    );
    assert_eq!(r.p.bind(s3, &un("/tmp")), Err(EINVAL), "the entry itself");
    assert_eq!(
        r.p.bind(s3, &un("/tmp/dir/")),
        Err(EINVAL),
        "a name that is a directory"
    );
    // Nothing was made by the refusals.
    assert_eq!(r.p.stat_at(AT_FDCWD, b"/work/no", 0), Err(ENOENT));
    assert_eq!(r.p.stat_at(AT_FDCWD, b"/no", 0), Err(ENOENT));
    // A path no entry covers is the resolver's refusal, with its report.
    let mut bare = peer(&r, &_svc, false);
    let s4 = sock(&mut bare, SOCK_STREAM);
    assert_eq!(bare.p.bind(s4, &un("/home/x")), Err(ENOENT));
    assert_eq!(bare.reports().len(), 1, "reported once");
}

#[test]
fn bind_refuses_what_it_cannot_name() {
    let (mut r, _svc) = tmp_rig();
    let s = sock(&mut r, SOCK_STREAM);
    // Abstract names (a path that starts with NUL, or none) are not offered.
    let mut abstract_name = (AF_UNIX as u16).to_ne_bytes().to_vec();
    abstract_name.extend_from_slice(b"\0abc");
    assert_eq!(r.p.bind(s, &abstract_name), Err(EAFNOSUPPORT));
    assert_eq!(
        r.p.bind(s, &(AF_UNIX as u16).to_ne_bytes()),
        Err(EAFNOSUPPORT)
    );
    // A short structure, another family, one too long.
    assert_eq!(r.p.bind(s, &[1]), Err(EINVAL));
    let mut inet = un("/tmp/x");
    inet[0] = 2;
    assert_eq!(r.p.bind(s, &inet), Err(EAFNOSUPPORT));
    assert_eq!(r.p.bind(s, &un(&"a".repeat(120))), Err(EINVAL));
    // A datagram socket needs every sender to reach it: not built.
    let d = sock(&mut r, SOCK_DGRAM);
    assert_eq!(r.p.bind(d, &un("/tmp/d")), Err(EOPNOTSUPP));
    assert_eq!(r.p.stat_at(AT_FDCWD, b"/tmp/d", 0), Err(ENOENT));
    // Not a socket, not a descriptor.
    let (p, _) = r.p.pipe(0).unwrap();
    assert_eq!(r.p.bind(p, &un("/tmp/p")), Err(ENOTSOCK));
    assert_eq!(r.p.bind(99, &un("/tmp/p")), Err(EBADF));
}

#[test]
fn a_name_is_taken_once_and_a_socket_binds_once() {
    let (mut r, _svc) = tmp_rig();
    let a = sock(&mut r, SOCK_STREAM);
    r.p.bind(a, &un("/tmp/one")).unwrap();
    assert_eq!(r.p.bind(a, &un("/tmp/two")), Err(EINVAL), "already bound");
    let b = sock(&mut r, SOCK_STREAM);
    assert_eq!(r.p.bind(b, &un("/tmp/one")), Err(EADDRINUSE));
    // A plain file's name is taken too.
    let fd =
        r.p.open(AT_FDCWD, b"/tmp/plain", o::CREAT | o::RDWR, 0o644)
            .unwrap();
    r.p.close(fd).unwrap();
    assert_eq!(r.p.bind(b, &un("/tmp/plain")), Err(EADDRINUSE));
    // Only the unlink frees the name, even after the socket is closed.
    r.p.close(a).unwrap();
    assert_eq!(r.p.bind(b, &un("/tmp/one")), Err(EADDRINUSE));
    r.p.remove(b"/tmp/one").unwrap();
    assert_eq!(r.p.bind(b, &un("/tmp/one")), Ok(()));
}

// ------------------------------------------------------------ refusals

#[test]
fn listen_and_accept_need_a_bound_listening_socket() {
    let (mut r, _svc) = tmp_rig();
    let s = sock(&mut r, SOCK_STREAM);
    assert_eq!(r.p.listen(s, 4), Err(EINVAL), "unbound");
    assert_eq!(r.p.accept(s, true, false), Err(EINVAL), "not listening");
    r.p.bind(s, &un("/tmp/l")).unwrap();
    assert_eq!(
        r.p.accept(s, true, false),
        Err(EINVAL),
        "bound, not listening"
    );
    r.p.listen(s, 4).unwrap();
    assert_eq!(
        r.p.listen(s, 9),
        Ok(()),
        "again: the hint changes, nothing else"
    );
    // A connected socket does not listen or accept.
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_STREAM, 0).unwrap();
    assert_eq!(r.p.listen(a, 1), Err(EINVAL));
    assert_eq!(r.p.accept(b, true, false), Err(EINVAL));
    // A datagram socket neither binds nor listens.
    let d = sock(&mut r, SOCK_DGRAM);
    assert_eq!(r.p.listen(d, 1), Err(EINVAL));
    // Not sockets.
    assert_eq!(r.p.listen(99, 1), Err(EBADF));
    let (p, _) = r.p.pipe(0).unwrap();
    assert_eq!(r.p.accept(p, true, false), Err(ENOTSOCK));
    // A TCP socket has no listener yet: honest, not silent.
    let t = r.p.socket(2, SOCK_STREAM, 0).unwrap();
    assert_eq!(r.p.listen(t, 1), Err(EOPNOTSUPP));
    assert_eq!(
        r.p.bind(t, &[2, 0, 0, 80, 127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(EOPNOTSUPP)
    );
}

#[test]
fn connect_is_refused_where_nothing_listens_and_for_the_wrong_kind() {
    let (mut a, svc) = tmp_rig();
    let mut b = peer(&a, &svc, true);
    let c = sock(&mut b, SOCK_STREAM);
    assert_eq!(b.p.connect(c, &un("/tmp/none")), Err(ENOENT));
    // Bound, not listening: refused.
    let s = a.p.socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0).unwrap();
    a.p.bind(s, &un("/tmp/b")).unwrap();
    assert_eq!(b.p.connect(c, &un("/tmp/b")), Err(ECONNREFUSED));
    // A regular file is not a socket.
    let fd =
        a.p.open(AT_FDCWD, b"/tmp/file", o::CREAT | o::RDWR, 0o666)
            .unwrap();
    a.p.close(fd).unwrap();
    assert_eq!(b.p.connect(c, &un("/tmp/file")), Err(ECONNREFUSED));
    // A directory.
    a.p.mkdir_at(AT_FDCWD, b"/tmp/d", 0o777).unwrap();
    assert_eq!(b.p.connect(c, &un("/tmp/d")), Err(ECONNREFUSED));
    // A socket of the other type: EPROTOTYPE, and the listener is untouched.
    a.p.listen(s, 4).unwrap();
    let w = sock(&mut b, SOCK_SEQPACKET);
    assert_eq!(b.p.connect(w, &un("/tmp/b")), Err(EPROTOTYPE));
    assert_eq!(
        a.p.accept(s, true, false),
        Err(EAGAIN),
        "nothing was queued"
    );
    // A datagram socket has no connection to make.
    let d = sock(&mut b, SOCK_DGRAM);
    assert_eq!(b.p.connect(d, &un("/tmp/b")), Err(EOPNOTSUPP));
    // Abstract and malformed addresses.
    let mut abs = (AF_UNIX as u16).to_ne_bytes().to_vec();
    abs.extend_from_slice(b"\0x");
    assert_eq!(b.p.connect(c, &abs), Err(EAFNOSUPPORT));
    // One connection a socket.
    b.p.connect(c, &un("/tmp/b")).unwrap();
    assert_eq!(b.p.connect(c, &un("/tmp/b")), Err(EISCONN));
    // A listening socket does not connect.
    assert_eq!(a.p.connect(s, &un("/tmp/b")), Err(EINVAL));
    // Nothing leaked from the refusals: the connector is whole.
    b.p.close(c).unwrap();
}

/// No ambient authority: a socket file is reachable only through a table
/// that has its directory, and with `WRITE` on it.
#[test]
fn a_program_that_cannot_see_the_directory_cannot_connect() {
    let (mut a, svc) = tmp_rig();
    let _s = server(&mut a, SOCK_STREAM, "/tmp/private");
    // A process with no `/tmp` entry: the path is outside its table.
    let mut blind = peer(&a, &svc, false);
    let c = sock(&mut blind, SOCK_STREAM);
    assert_eq!(blind.p.connect(c, &un("/tmp/private")), Err(ENOENT));
    assert_eq!(
        blind.reports().len(),
        1,
        "refused: reported as a path outside the table"
    );
    // A process whose own `/tmp` is another service's: the name is not there.
    let net = a.net.clone();
    let (other, oc) = tmp_volume(&net, 256);
    a.p.sys.services.push(other.clone());
    let mut other_line = peer(&a, &svc, false);
    other_line.p.sys.services.push(other);
    other_line.p.add_entry("/tmp", EntryKind::Tmp, Some(oc));
    let c2 = sock(&mut other_line, SOCK_STREAM);
    assert_eq!(other_line.p.connect(c2, &un("/tmp/private")), Err(ENOENT));
}

#[test]
fn the_backlog_is_the_channels_and_a_full_queue_is_eagain() {
    let (mut a, svc) = tmp_rig();
    let mut b = peer(&a, &svc, true);
    let s = server(&mut a, SOCK_STREAM, "/tmp/q");
    let mut clients = vec![];
    for _ in 0..zoraya_posix::unix::LISTEN_QUEUE {
        let c = sock(&mut b, SOCK_STREAM);
        b.p.connect(c, &un("/tmp/q")).unwrap();
        clients.push(c);
    }
    let over = sock(&mut b, SOCK_STREAM);
    assert_eq!(b.p.connect(over, &un("/tmp/q")), Err(EAGAIN));
    // The refused attempt left the socket usable: one accept makes room.
    let d = a.p.accept(s, false, false).unwrap();
    assert_eq!(b.p.connect(over, &un("/tmp/q")), Ok(()));
    a.p.close(d).unwrap();
}

#[test]
fn a_listener_that_goes_leaves_an_inert_file_and_every_handle_is_closed() {
    let (mut a, svc) = tmp_rig();
    let mut b = peer(&a, &svc, true);
    // A first round, so that what the services keep for good (a token, a
    // scratch buffer) is there before the count is taken.
    let s0 = server(&mut a, SOCK_STREAM, "/tmp/warm");
    a.p.close(s0).unwrap();
    a.p.remove(b"/tmp/warm").unwrap();
    a.settle();
    let base = a.handles();
    let s = server(&mut a, SOCK_STREAM, "/tmp/gone");
    let c = sock(&mut b, SOCK_STREAM);
    b.p.connect(c, &un("/tmp/gone")).unwrap();
    // The pending connection is dropped with the listener, and its connector
    // sees the end.
    a.p.close(s).unwrap();
    a.settle();
    assert_eq!(b.read(c, 10).unwrap(), b"");
    assert_eq!(b.write(c, b"x"), Err(EPIPE));
    b.p.close(c).unwrap();
    // The name stays, inert: a connect is refused, a bind is `EADDRINUSE`.
    let c2 = sock(&mut b, SOCK_STREAM);
    assert_eq!(b.p.connect(c2, &un("/tmp/gone")), Err(ECONNREFUSED));
    let s2 = sock(&mut a, SOCK_STREAM);
    assert_eq!(a.p.bind(s2, &un("/tmp/gone")), Err(EADDRINUSE));
    // Unlinked, it can be made again and served.
    a.p.remove(b"/tmp/gone").unwrap();
    a.p.bind(s2, &un("/tmp/gone")).unwrap();
    a.p.listen(s2, 1).unwrap();
    b.p.connect(c2, &un("/tmp/gone")).unwrap();
    let d = a.p.accept(s2, false, false).unwrap();
    a.p.close(d).unwrap();
    a.p.close(s2).unwrap();
    b.p.close(c2).unwrap();
    a.p.remove(b"/tmp/gone").unwrap();
    a.settle();
    assert_eq!(
        a.handles(),
        base,
        "every handle of both processes and the service is back"
    );
}

#[test]
fn a_listening_socket_is_not_sent_and_a_node_is_not_opened() {
    let (mut a, svc) = tmp_rig();
    let s = server(&mut a, SOCK_STREAM, "/tmp/n");
    let (x, _y) = a.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let u = a.poke(0, b"m");
    assert_eq!(a.p.sendmsg(x, &[u], &[s], 0), Err(EINVAL));
    // `open` of a socket file is `ENXIO`, `stat` says what it is.
    assert_eq!(a.p.open(AT_FDCWD, b"/tmp/n", o::RDWR, 0), Err(ENXIO));
    let st = a.p.stat_at(AT_FDCWD, b"/tmp/n", 0).unwrap();
    assert_eq!(
        u32::from_le_bytes(st[16..20].try_into().unwrap()) & 0o170000,
        0o140000
    );
    let _ = svc;
}

#[test]
fn accept_with_the_table_full_is_emfile_and_keeps_the_connection() {
    let (mut a, svc) = tmp_rig();
    let mut b = peer(&a, &svc, true);
    let s = server(&mut a, SOCK_STREAM, "/tmp/full");
    let c = sock(&mut b, SOCK_STREAM);
    b.p.connect(c, &un("/tmp/full")).unwrap();
    let mut extra = vec![];
    while let Ok(fd) = a.p.fds.dup(s, 0, false) {
        extra.push(fd as i32);
    }
    assert_eq!(a.p.accept(s, false, false), Err(EMFILE));
    a.p.close(extra.pop().unwrap()).unwrap();
    let d = a.p.accept(s, false, false).unwrap();
    b.write(c, b"still there").unwrap();
    assert_eq!(a.read(d, 100).unwrap(), b"still there");
}

#[test]
fn a_hostile_connection_message_is_dropped_by_the_listener() {
    use zoraya_posix::unix::decode_connect;
    // Whoever holds the file with `WRITE` can send the service any record;
    // the listener decodes it by the rules of its own type.
    let good_stream =
        zoraya_posix::unix::encode_connect(&zoraya_posix::unix::connect_record(SOCK_STREAM));
    assert!(decode_connect(&good_stream, 3, SOCK_STREAM).is_some());
    assert!(
        decode_connect(&good_stream, 2, SOCK_STREAM).is_none(),
        "a handle short"
    );
    assert!(
        decode_connect(&good_stream, 4, SOCK_STREAM).is_none(),
        "a handle over"
    );
    assert!(
        decode_connect(&good_stream, 3, SOCK_SEQPACKET).is_none(),
        "a stream for a packet listener"
    );
    let good_packet =
        zoraya_posix::unix::encode_connect(&zoraya_posix::unix::connect_record(SOCK_SEQPACKET));
    assert!(decode_connect(&good_packet, 1, SOCK_SEQPACKET).is_some());
    assert!(decode_connect(&good_packet, 1, SOCK_STREAM).is_none());
    assert!(decode_connect(&good_packet, 3, SOCK_SEQPACKET).is_none());
    // Any bytes: never a panic, and only the two forms decode.
    for len in 0..64usize {
        for seed in 0u8..=255 {
            let bytes: Vec<u8> = (0..len)
                .map(|i| seed.wrapping_mul(31).wrapping_add(i as u8))
                .collect();
            for ty in [SOCK_STREAM, SOCK_SEQPACKET] {
                for h in 0..5 {
                    if let Some(r) = decode_connect(&bytes, h, ty) {
                        assert_eq!(r.handles(), h);
                        assert_eq!(bytes.len(), 24);
                    }
                }
            }
        }
    }
}

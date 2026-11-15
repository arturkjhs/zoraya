//! `AF_UNIX` sockets over the fake kernel (ADR-0412, plan part F2):
//! `socketpair`, `sendmsg` and `recvmsg` with `SCM_RIGHTS`, for each kind of
//! descriptor and of socket. INV-0412-1 (a descriptor arrives as the same
//! kind with the same rights or fewer, a refused send changes nothing) and
//! INV-0412-2 (boundaries of packets, the order of a stream, descriptors with
//! the byte they were sent with, and the shared case after `fork`).

mod kernel;

use kernel::*;
use zoraya_abi::posix::Facts;
use zoraya_posix::errno::*;
use zoraya_posix::poll::*;
use zoraya_posix::process::{AT_FDCWD, Desc, Posix, o};
use zoraya_posix::socket::{
    MSG_DONTWAIT, SHUT_RD, SHUT_RDWR, SHUT_WR, SO_DOMAIN, SO_TYPE, SOCK_CLOEXEC, SOCK_NONBLOCK,
    SOL_SOCKET, Socket, State,
};
use zoraya_posix::sys::Sys;
use zoraya_posix::unix::{
    AF_UNIX, ETOOMANYREFS, MSG_CMSG_CLOEXEC, MSG_CTRUNC, MSG_TRUNC, SOCK_DGRAM, SOCK_SEQPACKET,
    SOCK_STREAM,
};

zoraya_test_alloc::cap_memory!();

const NONE: &[i32] = &[];

fn pair(r: &mut Rig, ty: i32) -> (i32, i32) {
    r.p.socketpair(AF_UNIX, ty, 0).unwrap()
}

fn send(r: &mut Rig, fd: i32, bytes: &[u8], rights: &[i32]) -> Result<usize, i32> {
    let u = r.poke(0, bytes);
    r.p.sendmsg(fd, &[u], rights, 0)
}

/// What `recvmsg` gave: the bytes, the new fds, the flags.
fn recv(
    r: &mut Rig,
    fd: i32,
    n: usize,
    max_fds: usize,
    flags: i32,
) -> Result<(Vec<u8>, Vec<i32>, i32), i32> {
    let u = r.arena.user(ARENA / 2, n);
    let got = r.p.recvmsg(fd, &[u], max_fds, flags)?;
    Ok((r.peek(ARENA / 2, got.bytes), got.fds, got.flags))
}

/// A second process over the same kernel and services.
fn second(r: &Rig) -> Rig {
    let net = r.net.clone();
    let mut fake = Fake::new(net.clone(), r.p.sys.services.clone());
    let arena = fake.memory_create(ARENA as u64).unwrap();
    let base = fake.memory_map(arena, ARENA as u64).unwrap();
    let p = Posix::new(
        fake,
        Facts {
            uid: 1000,
            gid: 100,
            umask: 0o022,
            ..Default::default()
        },
    );
    Rig {
        p,
        net,
        arena: Arena { base },
        root: r.root,
        reports: r.reports,
    }
}

/// Hands the socket at `fd` of `a` to `b`, as a spawner would: the handles
/// move, the fd closes.
fn give(a: &mut Rig, fd: i32, b: &mut Rig) -> i32 {
    let Some(Desc::Socket(s)) = a.p.fds.get_mut(fd) else {
        panic!("fd {fd} is no socket");
    };
    let mut t = Socket::fresh_unix(s.kind, s.nonblock);
    t.state = State::Connected;
    t.read = s.read.take();
    t.write = s.write.take();
    t.ctl = s.ctl.take();
    s.state = State::Dead;
    a.p.close(fd).unwrap();
    match b.p.fds.install(0, Desc::Socket(Box::new(t)), false) {
        Ok(fd) => fd as i32,
        Err(_) => panic!("table full"),
    }
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

fn open(r: &mut Rig, path: &[u8], flags: u32) -> i32 {
    r.p.open(AT_FDCWD, path, flags, 0o644).unwrap()
}

// ------------------------------------------------------------ the pairs

#[test]
fn a_stream_pair_carries_bytes_both_ways_and_ends() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_STREAM);
    assert_eq!(r.write(a, b"hello").unwrap(), 5);
    assert_eq!(r.write(b, b"back").unwrap(), 4);
    assert_eq!(r.read(b, 100).unwrap(), b"hello");
    assert_eq!(r.read(a, 100).unwrap(), b"back");
    // Bytes are not messages: two writes, one read.
    r.write(a, b"ab").unwrap();
    r.write(a, b"cd").unwrap();
    assert_eq!(r.read(b, 100).unwrap(), b"abcd");
    // The peer closes: its bytes first, then the end; a write is `EPIPE`.
    r.write(a, b"last").unwrap();
    r.p.close(a).unwrap();
    assert_eq!(r.read(b, 100).unwrap(), b"last");
    assert_eq!(r.read(b, 100).unwrap(), b"");
    assert_eq!(r.write(b, b"x"), Err(EPIPE));
    r.p.close(b).unwrap();
}

#[test]
fn a_pair_is_a_socket_of_its_type_and_domain() {
    let mut r = rig();
    for ty in [SOCK_STREAM, SOCK_DGRAM, SOCK_SEQPACKET] {
        let (a, b) = pair(&mut r, ty);
        assert_eq!(r.p.getsockopt(a, SOL_SOCKET, SO_TYPE), Ok(ty));
        assert_eq!(r.p.getsockopt(b, SOL_SOCKET, SO_DOMAIN), Ok(AF_UNIX));
        assert!(r.p.is_unix(a));
        let st = r.p.fstat(a).unwrap();
        assert_eq!(st.len(), zoraya_posix::stat::SIZE);
        r.p.close(a).unwrap();
        r.p.close(b).unwrap();
    }
    // Flags, and what is refused.
    let (a, b) =
        r.p.socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0)
            .unwrap();
    assert_eq!(r.p.fds.cloexec(a), Ok(true));
    assert_eq!(r.read(a, 1), Err(EAGAIN));
    assert_eq!(r.p.fds.cloexec(b), Ok(true));
    assert_eq!(r.p.socketpair(2, SOCK_STREAM, 0), Err(EAFNOSUPPORT));
    assert_eq!(r.p.socketpair(AF_UNIX, 3, 0), Err(EPROTONOSUPPORT));
    assert_eq!(
        r.p.socketpair(AF_UNIX, SOCK_STREAM, 6),
        Err(EPROTONOSUPPORT)
    );
    // `socket(AF_UNIX)` is a socket with no peer.
    let s = r.p.socket(AF_UNIX, SOCK_STREAM, 0).unwrap();
    assert!(r.p.is_unix(s));
    assert_eq!(r.read(s, 1), Err(ENOTCONN));
    assert_eq!(r.write(s, b"x"), Err(ENOTCONN));
    assert_eq!(r.p.connect(s, &[1, 0, b'/', b'x']), Err(ENOENT));
    assert_eq!(r.p.connect(a, &[1, 0, b'/', b'x']), Err(EISCONN));
}

#[test]
fn a_full_table_leaves_no_half_pair() {
    let mut r = rig();
    let before = r.handles();
    let (a, _b) = pair(&mut r, SOCK_STREAM);
    while r.p.fds.dup(a, 0, false).is_ok() {}
    let held = r.handles();
    assert_eq!(r.p.socketpair(AF_UNIX, SOCK_STREAM, 0), Err(EMFILE));
    assert_eq!(r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0), Err(EMFILE));
    assert_eq!(r.handles(), held, "nothing left over");
    assert!(held > before);
}

#[test]
fn a_packet_pair_keeps_boundaries() {
    for ty in [SOCK_SEQPACKET, SOCK_DGRAM] {
        let mut r = rig();
        let (a, b) = pair(&mut r, ty);
        send(&mut r, a, b"one", NONE).unwrap();
        send(&mut r, a, b"", NONE).unwrap();
        send(&mut r, a, b"three!", NONE).unwrap();
        assert_eq!(recv(&mut r, b, 100, 0, 0).unwrap().0, b"one");
        let (empty, ..) = recv(&mut r, b, 100, 0, 0).unwrap();
        assert!(empty.is_empty(), "an empty message is a message");
        // A short buffer truncates and says so; the rest of the message is gone.
        let (got, _, flags) = recv(&mut r, b, 3, 0, 0).unwrap();
        assert_eq!((got.as_slice(), flags), (&b"thr"[..], MSG_TRUNC));
        assert_eq!(recv(&mut r, b, 100, 0, MSG_DONTWAIT), Err(EAGAIN));
        // The most a message holds, and one more.
        let big = vec![7u8; 64 * 1024 - 1];
        assert_eq!(r.p.sys.copy_out(r.arena.user(0, big.len()), &big), ());
        assert_eq!(
            r.p.sendmsg(a, &[r.arena.user(0, big.len())], NONE, 0),
            Ok(big.len())
        );
        let (got, ..) = recv(&mut r, b, 70_000, 0, 0).unwrap();
        assert_eq!(got.len(), big.len());
        assert_eq!(
            r.p.sendmsg(a, &[r.arena.user(0, 64 * 1024)], NONE, 0),
            Err(EMSGSIZE)
        );
        // Gather and scatter.
        let x = r.poke(100, b"ab");
        let y = r.poke(200, b"cde");
        let none = r.arena.user(300, 0);
        assert_eq!(r.p.sendmsg(a, &[x, none, y], NONE, 0), Ok(5));
        let (p, q) = (r.arena.user(ARENA / 2, 2), r.arena.user(ARENA / 2 + 10, 8));
        let got = r.p.recvmsg(b, &[p, q], 0, 0).unwrap();
        assert_eq!(got.bytes, 5);
        assert_eq!(r.peek(ARENA / 2, 2), b"ab");
        assert_eq!(r.peek(ARENA / 2 + 10, 3), b"cde");
    }
}

#[test]
fn a_packet_socket_blocks_its_writer_at_64_messages() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    let u = r.poke(0, b"m");
    for _ in 0..64 {
        r.p.sendmsg(a, &[u], NONE, MSG_DONTWAIT).unwrap();
    }
    assert_eq!(r.p.sendmsg(a, &[u], NONE, MSG_DONTWAIT), Err(EAGAIN));
    assert_eq!(
        revents(&mut r, a, POLLOUT),
        0,
        "a full queue is not writable"
    );
    assert_eq!(recv(&mut r, b, 10, 0, 0).unwrap().0, b"m");
    assert_eq!(revents(&mut r, a, POLLOUT), POLLOUT);
    r.p.sendmsg(a, &[u], NONE, MSG_DONTWAIT).unwrap();
    // The peer goes: its messages are read, then the end; a send is `EPIPE`.
    r.p.close(b).unwrap();
    assert_eq!(r.p.sendmsg(a, &[u], NONE, MSG_DONTWAIT), Err(EPIPE));
}

#[test]
fn a_packet_pair_ends_when_the_peer_closes() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    send(&mut r, a, b"bye", NONE).unwrap();
    r.p.close(a).unwrap();
    assert_eq!(recv(&mut r, b, 10, 0, 0).unwrap().0, b"bye");
    assert_eq!(recv(&mut r, b, 10, 0, 0).unwrap().0, b"");
    assert_eq!(revents(&mut r, b, POLLIN), POLLIN | POLLHUP);
}

#[test]
fn shutdown_ends_one_direction() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_STREAM);
    r.write(a, b"x").unwrap();
    r.p.shutdown(a, SHUT_WR).unwrap();
    assert_eq!(r.read(b, 10).unwrap(), b"x");
    assert_eq!(r.read(b, 10).unwrap(), b"", "the end");
    assert_eq!(r.write(a, b"y"), Err(EPIPE));
    r.write(b, b"z").unwrap();
    assert_eq!(r.read(a, 10).unwrap(), b"z", "the other way still goes");
    r.p.shutdown(a, SHUT_RD).unwrap();
    assert_eq!(r.read(a, 10).unwrap(), b"");
    assert_eq!(r.p.shutdown(a, 7), Err(EINVAL));
    let s = r.p.socket(AF_UNIX, SOCK_STREAM, 0).unwrap();
    assert_eq!(r.p.shutdown(s, SHUT_RDWR), Err(ENOTCONN));
    // A packet pair keeps a flag for each half and ends with the second.
    let (c, d) = pair(&mut r, SOCK_SEQPACKET);
    r.p.shutdown(c, SHUT_WR).unwrap();
    assert_eq!(send(&mut r, c, b"n", NONE), Err(EPIPE));
    send(&mut r, d, b"m", NONE).unwrap();
    assert_eq!(recv(&mut r, c, 5, 0, 0).unwrap().0, b"m");
    r.p.shutdown(c, SHUT_RD).unwrap();
    assert_eq!(recv(&mut r, c, 5, 0, 0).unwrap().0, b"");
    assert_eq!(
        revents(&mut r, d, POLLIN),
        POLLIN | POLLHUP,
        "its peer closed"
    );
}

// ------------------------------------------------------------ INV-0412-1

/// Each kind of descriptor arrives as the same kind, and works.
#[test]
fn every_kind_of_descriptor_crosses_a_packet_pair() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    // A file read and written, a read-only one that stays read-only.
    let f = open(&mut r, b"/f", o::CREAT | o::RDWR);
    r.write(f, b"data").unwrap();
    let ro = open(&mut r, b"/f", o::RDONLY);
    let dir = open(&mut r, b"/", o::RDONLY | o::DIRECTORY);
    let (pr, pw) = r.p.pipe(0).unwrap();
    let (sa, sb) = pair(&mut r, SOCK_STREAM);
    let (pa, pb) = pair(&mut r, SOCK_SEQPACKET);
    let rights = [f, ro, dir, pr, pw, sa, pa];
    assert_eq!(send(&mut r, a, b"all", &rights), Ok(3));
    // The two sockets moved; the rest are still here.
    assert!(r.p.fds.get(sa).is_none(), "a moved socket's fd is closed");
    assert!(r.p.fds.get(pa).is_none());
    for fd in [f, ro, dir, pr, pw] {
        assert!(r.p.fds.get(fd).is_some());
    }
    let (got, fds, flags) = recv(&mut r, b, 10, 7, MSG_CMSG_CLOEXEC).unwrap();
    assert_eq!((got.as_slice(), flags), (&b"all"[..], 0));
    assert_eq!(fds.len(), 7);
    for &fd in &fds {
        assert_eq!(r.p.fds.cloexec(fd), Ok(true), "MSG_CMSG_CLOEXEC");
    }
    let [rf, rro, rdir, rpr, rpw, rsa, rpa] = fds[..] else {
        panic!()
    };
    // The file: the same description's object, read and written.
    r.p.lseek(rf, 0, 0).unwrap();
    assert_eq!(r.read(rf, 10).unwrap(), b"data");
    r.write(rf, b"!").unwrap();
    // Read-only stays read-only (rights only decrease).
    r.p.lseek(rro, 0, 0).unwrap();
    assert_eq!(r.read(rro, 10).unwrap(), b"data!");
    assert_eq!(r.write(rro, b"no"), Err(EBADF));
    // The directory lists.
    assert!(r.p.fstat(rdir).is_ok());
    // The pipe ends are the same pipe.
    r.write(rpw, b"pipe").unwrap();
    assert_eq!(r.read(pr, 10).unwrap(), b"pipe");
    r.write(pw, b"back").unwrap();
    assert_eq!(r.read(rpr, 10).unwrap(), b"back");
    // The stream socket and the packet socket talk to their peers.
    r.write(sb, b"stream").unwrap();
    assert_eq!(r.read(rsa, 10).unwrap(), b"stream");
    r.write(rsa, b"reply").unwrap();
    assert_eq!(r.read(sb, 10).unwrap(), b"reply");
    send(&mut r, pb, b"packet", NONE).unwrap();
    assert_eq!(recv(&mut r, rpa, 10, 0, 0).unwrap().0, b"packet");
    // The moved socket is the same socket: its end is not here twice.
    assert_eq!(r.p.getsockopt(rsa, SOL_SOCKET, SO_TYPE), Ok(SOCK_STREAM));
    assert_eq!(r.p.getsockopt(rpa, SOL_SOCKET, SO_TYPE), Ok(SOCK_SEQPACKET));
}

#[test]
fn a_sent_pipe_end_outlives_the_sender_and_ends_with_the_last_holder() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    let (pr, pw) = r.p.pipe(0).unwrap();
    send(&mut r, a, b"x", &[pw]).unwrap();
    r.p.close(pw).unwrap();
    let (_, fds, _) = recv(&mut r, b, 5, 1, 0).unwrap();
    r.write(fds[0], b"still").unwrap();
    assert_eq!(r.read(pr, 10).unwrap(), b"still");
    r.p.close(fds[0]).unwrap();
    assert_eq!(r.read(pr, 10).unwrap(), b"", "the last writer went");
}

#[test]
fn a_refused_send_changes_nothing() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    let (pr, pw) = r.p.pipe(0).unwrap();
    let f = open(&mut r, b"/f", o::CREAT | o::RDWR);
    let (sa, _sb) = pair(&mut r, SOCK_STREAM);
    let tcp = r.p.socket(2, SOCK_STREAM, 0).unwrap();
    let null = open(&mut r, b"/dev/null", o::RDWR);
    r.settle();
    let handles = r.handles();
    let fds = r.p.fds.count();
    // Every refusal is before anything is made, with good descriptors ahead
    // of the bad one.
    for (bad, e) in [(999, EBADF), (tcp, EINVAL), (null, EINVAL), (-1, EBADF)] {
        assert_eq!(send(&mut r, a, b"x", &[f, pr, bad]), Err(e), "fd {bad}");
        r.settle();
        assert_eq!(r.handles(), handles, "handles");
        assert_eq!(r.p.fds.count(), fds, "fds");
    }
    // The same moving socket twice, or through itself.
    assert_eq!(send(&mut r, a, b"x", &[sa, sa]), Err(EINVAL));
    let sa2 = r.p.dup(sa).unwrap();
    assert_eq!(send(&mut r, a, b"x", &[sa, sa2]), Err(EINVAL));
    assert_eq!(
        send(&mut r, a, b"x", &[f, a]),
        Err(EBUSY),
        "its own channel"
    );
    assert_eq!(send(&mut r, a, b"x", &[b]), Err(EBUSY), "its peer's");
    r.p.close(sa2).unwrap();
    // Too many.
    let many = vec![pw; 65];
    assert_eq!(send(&mut r, a, b"x", &many), Err(ETOOMANYREFS));
    // A message too long with descriptors.
    let big = r.arena.user(0, 64 * 1024 - 1);
    assert_eq!(r.p.sendmsg(a, &[big], &[pw], 0), Err(EMSGSIZE));
    assert_eq!(
        r.handles(),
        handles + 1,
        "only the dup of sa2 left (closed)"
    );
    // The peer goes: `EPIPE`, and the socket that would have moved is still ours.
    let (pa, pb) = pair(&mut r, SOCK_SEQPACKET);
    let handles = r.handles();
    r.p.close(b).unwrap();
    assert_eq!(send(&mut r, a, b"x", &[pa, f]), Err(EPIPE));
    assert!(r.p.fds.get(pa).is_some());
    assert_eq!(r.handles(), handles - 1, "b's own handle only");
    send(&mut r, pa, b"alive", NONE).unwrap();
    assert_eq!(recv(&mut r, pb, 10, 0, 0).unwrap().0, b"alive");
}

#[test]
fn a_channel_end_with_handles_queued_cannot_move() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    let (x, y) = pair(&mut r, SOCK_SEQPACKET);
    let (pr, _pw) = r.p.pipe(0).unwrap();
    // `x`'s end now has a pipe end queued for it to read: ADR-0050.
    send(&mut r, y, b"m", &[pr]).unwrap();
    let handles = r.handles();
    assert_eq!(send(&mut r, a, b"n", &[x]), Err(EBUSY));
    assert_eq!(r.handles(), handles);
    assert!(r.p.fds.get(x).is_some());
    // Read it and it can go.
    let (_, fds, _) = recv(&mut r, x, 5, 1, 0).unwrap();
    assert_eq!(fds.len(), 1);
    send(&mut r, a, b"n", &[x]).unwrap();
    let (_, got, _) = recv(&mut r, b, 5, 1, 0).unwrap();
    send(&mut r, y, b"hi", NONE).unwrap();
    assert_eq!(recv(&mut r, got[0], 5, 0, 0).unwrap().0, b"hi");
}

#[test]
fn a_full_table_keeps_the_message_queued() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    let (pr, pw) = r.p.pipe(0).unwrap();
    send(&mut r, a, b"p", &[pw, pr]).unwrap();
    let mut extra = vec![];
    while let Ok(fd) = r.p.fds.dup(a, 0, false) {
        extra.push(fd as i32);
    }
    assert_eq!(recv(&mut r, b, 5, 2, 0), Err(EMFILE));
    // Still queued: polls readable, and with room it comes whole.
    assert_eq!(revents(&mut r, b, POLLIN), POLLIN);
    r.p.close(extra.pop().unwrap()).unwrap();
    assert_eq!(
        recv(&mut r, b, 5, 2, 0),
        Err(EMFILE),
        "one is too few for two"
    );
    r.p.close(extra.pop().unwrap()).unwrap();
    let (got, fds, flags) = recv(&mut r, b, 5, 2, 0).unwrap();
    assert_eq!((got.as_slice(), fds.len(), flags), (&b"p"[..], 2, 0));
    // And a program that asked for none gets no error for the full table.
    send(&mut r, a, b"q", &[fds[0]]).unwrap();
    let (got, fds2, flags) = recv(&mut r, b, 5, 0, 0).unwrap();
    assert_eq!(
        (got.as_slice(), fds2.len(), flags),
        (&b"q"[..], 0, MSG_CTRUNC)
    );
}

#[test]
fn a_short_control_buffer_closes_the_rest_and_says_so() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    let (pr, pw) = r.p.pipe(0).unwrap();
    let before = r.handles();
    send(&mut r, a, b"p", &[pr, pw, pr]).unwrap();
    let (_, fds, flags) = recv(&mut r, b, 5, 1, 0).unwrap();
    assert_eq!((fds.len(), flags), (1, MSG_CTRUNC));
    // The two that did not fit are closed: only one new handle is left.
    assert_eq!(r.handles(), before + 1);
}

#[test]
fn a_hostile_message_is_dropped_and_its_handles_closed() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    let (pr, _pw) = r.p.pipe(0).unwrap();
    // Raw messages a peer of this library would never send.
    let Some(Desc::Socket(s)) = r.p.fds.get(a) else {
        panic!()
    };
    let end = s.ctl.unwrap();
    let junk: Vec<Vec<u8>> = vec![vec![], vec![9], vec![1, 0xff, 0, 0], vec![0; 3]];
    let handle_of_pr = match r.p.fds.get(pr) {
        Some(Desc::Stream { read: Some(h), .. }) => *h,
        _ => panic!(),
    };
    for m in junk {
        let h = r.p.sys.duplicate(handle_of_pr).unwrap();
        let before = r.handles();
        assert!(r.net.borrow_mut().send_message(end, &m, &[h]));
        assert_eq!(recv(&mut r, b, 10, 4, MSG_DONTWAIT), Err(EAGAIN));
        assert_eq!(r.handles(), before - 1, "the handle it brought is closed");
    }
}

// ------------------------------------------------------------ across processes

#[test]
fn descriptors_cross_to_another_process_and_work_there() {
    let mut p1 = rig();
    let mut p2 = second(&p1);
    let (a, b) = pair(&mut p1, SOCK_SEQPACKET);
    let b2 = give(&mut p1, b, &mut p2);
    let f = open(&mut p1, b"/shared", o::CREAT | o::RDWR);
    p1.write(f, b"from one").unwrap();
    let (pr, pw) = p1.p.pipe(0).unwrap();
    let (sa, sb) = pair(&mut p1, SOCK_STREAM);
    send(&mut p1, a, b"go", &[f, pw, sa]).unwrap();
    let (_, fds, _) = recv(&mut p2, b2, 5, 3, 0).unwrap();
    let [f2, pw2, sa2] = fds[..] else { panic!() };
    p2.p.lseek(f2, 0, 0).unwrap();
    assert_eq!(p2.read(f2, 20).unwrap(), b"from one");
    p2.write(pw2, b"hello").unwrap();
    assert_eq!(p1.read(pr, 10).unwrap(), b"hello");
    p2.write(sa2, b"socket").unwrap();
    assert_eq!(p1.read(sb, 10).unwrap(), b"socket");
    // Closing in the sender does not end what the receiver holds.
    p1.p.close(f).unwrap();
    p1.p.close(pw).unwrap();
    p2.write(f2, b"!").unwrap();
    p2.write(pw2, b"more").unwrap();
    assert_eq!(p1.read(pr, 10).unwrap(), b"more");
    // And back.
    send(&mut p2, b2, b"back", &[sa2]).unwrap();
    let (_, fds, _) = recv(&mut p1, a, 5, 1, 0).unwrap();
    p1.write(sb, b"again").unwrap();
    assert_eq!(p1.read(fds[0], 10).unwrap(), b"again");
}

// ------------------------------------------------------------ INV-0412-2

/// A seeded generator.
struct Rng(u64);
impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// A pipe whose read end holds one byte `id`: the descriptor to send, which
/// says which it is when it arrives.
fn token(r: &mut Rig, id: u8) -> i32 {
    let (pr, pw) = r.p.pipe(0).unwrap();
    r.write(pw, &[id]).unwrap();
    r.p.close(pw).unwrap();
    pr
}

fn which(r: &mut Rig, fd: i32) -> u8 {
    let id = r.read(fd, 1).unwrap()[0];
    r.p.close(fd).unwrap();
    id
}

/// For any split of sends and receives, a stream gives its bytes in order,
/// none lost or repeated, and a descriptor comes with the `recvmsg` that returns
/// the byte it was sent with.
#[test]
fn a_stream_keeps_order_and_each_descriptor_with_its_byte() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for seed in 0..6 {
        let mut r = rig();
        let (a, b) = pair(&mut r, SOCK_STREAM);
        let mut sent: Vec<u8> = Vec::new();
        let mut tags: Vec<(usize, u8)> = Vec::new();
        let mut at = 0usize;
        let mut next_id = 1u8;
        let mut checked = 0;
        for _ in 0..400 {
            if rng.next(2) == 0 && sent.len() - at < 20_000 {
                let n = 1 + rng.next(300) as usize;
                let bytes: Vec<u8> = (0..n).map(|_| rng.next(256) as u8).collect();
                let carry = rng.next(3) == 0;
                let t = if carry {
                    let id = next_id;
                    next_id = next_id.wrapping_add(1).max(1);
                    Some((id, token(&mut r, id)))
                } else {
                    None
                };
                let u = r.poke(0, &bytes);
                let rights: Vec<i32> = t.iter().map(|t| t.1).collect();
                let got = r.p.sendmsg(a, &[u], &rights, MSG_DONTWAIT);
                assert_eq!(got, Ok(n), "seed {seed}");
                if let Some((id, fd)) = t {
                    tags.push((sent.len(), id));
                    r.p.close(fd).unwrap();
                }
                sent.extend(bytes);
            } else {
                let want = 1 + rng.next(400) as usize;
                match recv(&mut r, b, want, 4, MSG_DONTWAIT) {
                    Ok((bytes, fds, flags)) => {
                        assert_eq!(flags, 0);
                        assert!(!bytes.is_empty() && bytes.len() <= want);
                        assert_eq!(bytes, sent[at..at + bytes.len()], "seed {seed} at {at}");
                        let mut ids: Vec<u8> = fds.iter().map(|&fd| which(&mut r, fd)).collect();
                        ids.sort();
                        let mut expect: Vec<u8> = tags
                            .iter()
                            .filter(|t| t.0 >= at && t.0 < at + bytes.len())
                            .map(|t| t.1)
                            .collect();
                        expect.sort();
                        assert_eq!(
                            ids,
                            expect,
                            "seed {seed}: descriptors of [{at}, +{})",
                            bytes.len()
                        );
                        checked += expect.len();
                        at += bytes.len();
                    }
                    Err(EAGAIN) => assert_eq!(at, sent.len(), "seed {seed}: bytes are waiting"),
                    Err(e) => panic!("seed {seed}: {e}"),
                }
            }
        }
        // Drain.
        while at < sent.len() {
            let (bytes, fds, _) = recv(&mut r, b, 1000, 4, MSG_DONTWAIT).unwrap();
            assert_eq!(bytes, sent[at..at + bytes.len()]);
            for fd in fds {
                which(&mut r, fd);
                checked += 1;
            }
            at += bytes.len();
        }
        assert_eq!(checked, tags.len(), "every descriptor arrived once");
    }
}

/// The race the offsets exist for: a reader looks for tags, finds none, takes
/// the bytes, and the sender's tag for the second half arrives in between.
/// The reader gives the bytes before the tag, keeps the rest in the library,
/// and the descriptor comes with its own byte.
#[test]
fn bytes_read_past_a_late_tag_are_kept_for_the_next_call() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_STREAM);
    let id = token(&mut r, 42);
    // The sender's two sends, as the reader sees them: the bytes of both are
    // in the stream, and the tag of the second (offset 4) comes only after
    // the reader has looked.
    r.write(a, b"AAAA").unwrap();
    r.write(a, b"BBBB").unwrap();
    let Some(Desc::Socket(sock)) = r.p.fds.get(a) else {
        panic!()
    };
    let anc = sock.ctl.unwrap();
    let h = match r.p.fds.get(id) {
        Some(Desc::Stream { read: Some(h), .. }) => *h,
        _ => panic!(),
    };
    let copy = r.p.sys.duplicate(h).unwrap();
    let message = zoraya_posix::unix::encode_anc(
        4,
        &[zoraya_posix::unix::Record::one(
            zoraya_posix::unix::Kind::PipeRead,
            0,
            false,
        )],
    );
    r.p.sys.after_stream_read = Some(Box::new(move |net: &mut Net| {
        assert!(net.send_message(anc, &message, &[copy]));
    }));
    let (got, fds, _) = recv(&mut r, b, 100, 2, 0).unwrap();
    assert_eq!(
        (got.as_slice(), fds.len()),
        (&b"AAAA"[..], 0),
        "cut at the tag"
    );
    assert_eq!(revents(&mut r, b, POLLIN), POLLIN, "the library holds BBBB");
    let (got, fds, _) = recv(&mut r, b, 100, 2, 0).unwrap();
    assert_eq!((got.as_slice(), fds.len()), (&b"BBBB"[..], 1));
    assert_eq!(which(&mut r, fds[0]), 42);
    assert_eq!(recv(&mut r, b, 100, 2, MSG_DONTWAIT), Err(EAGAIN));
}

/// For sequenced packets and datagrams: one message, its descriptors and
/// nothing of another.
#[test]
fn packets_keep_their_descriptors_with_their_message() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for ty in [SOCK_SEQPACKET, SOCK_DGRAM] {
        let mut r = rig();
        let (a, b) = pair(&mut r, ty);
        let mut queue: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut next_id = 1u8;
        for _ in 0..500 {
            if rng.next(2) == 0 && queue.len() < 60 {
                let n = rng.next(200) as usize;
                let bytes: Vec<u8> = (0..n).map(|_| rng.next(256) as u8).collect();
                let k = rng.next(4) as usize;
                let mut ids = vec![];
                let mut fds = vec![];
                for _ in 0..k {
                    ids.push(next_id);
                    fds.push(token(&mut r, next_id));
                    next_id = next_id.wrapping_add(1).max(1);
                }
                let u = r.poke(0, &bytes);
                assert_eq!(r.p.sendmsg(a, &[u], &fds, MSG_DONTWAIT), Ok(n));
                for fd in fds {
                    r.p.close(fd).unwrap();
                }
                queue.push((bytes, ids));
            } else {
                let room = rng.next(250) as usize;
                match recv(&mut r, b, room, 4, MSG_DONTWAIT) {
                    Ok((bytes, fds, flags)) => {
                        let (sent, ids) = queue.remove(0);
                        let cut = sent.len().min(room);
                        assert_eq!(bytes, sent[..cut]);
                        assert_eq!(flags & MSG_TRUNC != 0, sent.len() > room);
                        let got: Vec<u8> = fds.iter().map(|&fd| which(&mut r, fd)).collect();
                        assert_eq!(got, ids);
                    }
                    Err(EAGAIN) => assert!(queue.is_empty()),
                    Err(e) => panic!("{e}"),
                }
            }
        }
    }
}

/// After a `fork` the byte streams are in two processes: rights are refused
/// in both and the bytes still arrive in order.
#[test]
fn after_a_fork_rights_are_refused_and_bytes_still_flow() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_STREAM);
    let (pr, _pw) = r.p.pipe(0).unwrap();
    send(&mut r, a, b"before", &[pr]).unwrap();
    let (got, fds, _) = recv(&mut r, b, 100, 1, 0).unwrap();
    assert_eq!((got.as_slice(), fds.len()), (&b"before"[..], 1));
    // The parent's side.
    r.p.after_fork_in_parent();
    assert_eq!(send(&mut r, a, b"x", &[pr]), Err(EOPNOTSUPP));
    assert_eq!(send(&mut r, b, b"x", &[pr]), Err(EOPNOTSUPP));
    r.write(a, b"one").unwrap();
    r.write(a, b"two").unwrap();
    assert_eq!(r.read(b, 100).unwrap(), b"onetwo");
    // A peer that still sends rights to a shared holder: the descriptors are
    // dropped (closed), the bytes arrive, nothing piles up.
    let mut other = second(&r);
    let c = give(&mut r, b, &mut other);
    let (pr2, _pw2) = other.p.pipe(0).unwrap();
    let handles = r.handles();
    send(&mut other, c, b"late", &[pr2]).unwrap();
    assert_eq!(r.read(a, 100).unwrap(), b"late");
    assert_eq!(r.handles(), handles, "the descriptor that came was closed");
}

#[test]
fn a_fork_child_has_no_ancillary_channel() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_STREAM);
    let (pr, _pw) = r.p.pipe(0).unwrap();
    send(&mut r, a, b"x", &[pr]).unwrap();
    assert!(r.p.fds.get(b).is_some());
    // The tag is the parent's: the child neither holds nor closes it.
    r.p.after_fork_in_child(77, 77);
    let Some(Desc::Socket(s)) = r.p.fds.get(b) else {
        panic!()
    };
    assert!(s.ctl.is_none(), "not the child's handle");
    assert_eq!(send(&mut r, b, b"y", &[pr]), Err(EOPNOTSUPP));
    r.write(b, b"y").unwrap();
    assert_eq!(r.read(a, 10).unwrap(), b"y");
    // A packet socket does not cross a fork at all (`fork_moves`).
}

// ------------------------------------------------------------ readiness (INV-0414-1)

#[test]
fn poll_says_what_a_read_would_do() {
    for ty in [SOCK_STREAM, SOCK_SEQPACKET] {
        let mut r = rig();
        let (a, b) = pair(&mut r, ty);
        assert_eq!(revents(&mut r, b, POLLIN), 0);
        assert_eq!(recv(&mut r, b, 5, 0, MSG_DONTWAIT), Err(EAGAIN));
        assert_eq!(revents(&mut r, a, POLLOUT), POLLOUT);
        send(&mut r, a, b"x", NONE).unwrap();
        assert_eq!(revents(&mut r, b, POLLIN), POLLIN);
        assert_eq!(recv(&mut r, b, 5, 0, 0).unwrap().0, b"x");
        assert_eq!(revents(&mut r, b, POLLIN), 0);
        r.p.close(a).unwrap();
        assert_eq!(revents(&mut r, b, POLLIN) & POLLHUP, POLLHUP);
        assert_eq!(recv(&mut r, b, 5, 0, MSG_DONTWAIT).unwrap().0, b"");
    }
    // Bytes the library holds past a tag are readable at once.
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_STREAM);
    let (pr, _pw) = r.p.pipe(0).unwrap();
    r.write(a, b"12").unwrap();
    send(&mut r, a, b"34", &[pr]).unwrap();
    // `rest` is empty here (the tag was seen before the bytes), so the
    // second byte pair waits behind the tag.
    assert_eq!(recv(&mut r, b, 100, 1, 0).unwrap().0, b"12");
    assert_eq!(revents(&mut r, b, POLLIN), POLLIN);
}

#[test]
fn a_socket_with_no_peer_is_ready_at_once_as_select_has_it() {
    let mut r = rig();
    let s = r.p.socket(AF_UNIX, SOCK_STREAM, 0).unwrap();
    assert_eq!(revents(&mut r, s, POLLIN | POLLOUT), POLLIN | POLLOUT);
}

#[test]
fn closing_a_socket_closes_what_it_held() {
    let mut r = rig();
    let before = r.handles();
    let (a, b) = pair(&mut r, SOCK_STREAM);
    let (c, d) = pair(&mut r, SOCK_SEQPACKET);
    let (pr, pw) = r.p.pipe(0).unwrap();
    // A descriptor that came and was never taken is closed with the socket.
    send(&mut r, a, b"x", &[pr, pw]).unwrap();
    send(&mut r, c, b"m", &[pr]).unwrap();
    for fd in [pr, pw, a, b, c, d] {
        r.p.close(fd).unwrap();
    }
    assert_eq!(r.handles(), before, "no handle is left");
}

#[test]
fn a_dup_shares_the_description_and_a_moved_one_is_dead() {
    let mut r = rig();
    let (a, b) = pair(&mut r, SOCK_SEQPACKET);
    let (x, y) = pair(&mut r, SOCK_SEQPACKET);
    let x2 = r.p.dup(x).unwrap();
    send(&mut r, y, b"hi", NONE).unwrap();
    assert_eq!(recv(&mut r, x2, 5, 0, 0).unwrap().0, b"hi");
    // Sending the description through one fd leaves the other dead, not
    // pointing at handles that went.
    send(&mut r, a, b"s", &[x]).unwrap();
    assert!(r.p.fds.get(x).is_none());
    assert_eq!(recv(&mut r, x2, 5, 0, 0).unwrap().0, b"", "ended");
    assert_eq!(send(&mut r, x2, b"z", NONE), Err(EPIPE));
    let (_, got, _) = recv(&mut r, b, 5, 1, 0).unwrap();
    send(&mut r, y, b"yo", NONE).unwrap();
    assert_eq!(recv(&mut r, got[0], 5, 0, 0).unwrap().0, b"yo");
}

#[test]
fn recv_on_a_file_or_a_pipe_is_not_a_socket() {
    let mut r = rig();
    let (pr, _pw) = r.p.pipe(0).unwrap();
    let u = r.arena.user(0, 4);
    assert_eq!(r.p.sendmsg(pr, &[u], NONE, 0), Err(ENOTSOCK));
    assert_eq!(r.p.recvmsg(pr, &[u], 0, 0).map(|_| ()), Err(ENOTSOCK));
    assert_eq!(r.p.sendmsg(55, &[u], NONE, 0), Err(EBADF));
}

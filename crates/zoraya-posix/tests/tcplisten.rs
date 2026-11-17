//! TCP `bind`, `listen` and `accept` against a fake `netd` through the fake
//! kernel (plan part F7b, proposed ADR-0441): the fake answers `Listen` as a
//! script says and does what `netd`'s loopback does for a `connect` to
//! 127.0.0.0/8: the client's two stream ends go to the listener as an
//! `Accepted`, the client is told `Connected`.

mod kernel;

use std::cell::RefCell;
use std::rc::Rc;

use kernel::*;
use zoraya_posix::errno::*;
use zoraya_posix::poll::{POLLIN, POLLOUT, PollFd};
use zoraya_posix::process::{F_GETFL, F_SETFL, o};
use zoraya_posix::socket::{self as sock};
use zoraya_posix::sys::{Sys, signals};
use zoraya_proto_net::{
    Accepted, Connect, Connected, Connecting, Failed, Listen, Listening, reason, status,
};
use zoraya_protocol::{Below, Header, decode, encode};

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
zoraya_test_alloc::cap_memory!();

#[derive(Clone)]
struct Script {
    /// The status of `Listening`.
    listening: u32,
    /// The port a `Listen` of port 0 gets.
    picked: u32,
}

impl Default for Script {
    fn default() -> Script {
        Script {
            listening: status::OK,
            picked: 50000,
        }
    }
}

#[derive(Default)]
struct Log {
    /// The port of each `Listen`.
    listens: Vec<u32>,
    /// The listener channel ends `netd` holds, with their ports.
    listeners: Vec<(u32, H)>,
    connects: usize,
}

struct Rigged {
    r: Rig,
    log: Rc<RefCell<Log>>,
}

fn msg<'a, M: zoraya_protocol::Message<'a>>(m: &M, t: u32) -> Vec<u8> {
    let mut out = vec![0u8; 256];
    let n = encode(m, t, &mut out).unwrap();
    out.truncate(n);
    out
}

fn rigged(script: Script) -> Rigged {
    let mut r = rig();
    let (client, server) = r.p.sys.channel_create().unwrap();
    r.p.net = Some(client);
    let log = Rc::new(RefCell::new(Log::default()));
    let (l, sc) = (log.clone(), script);
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| {
        let mut log = l.borrow_mut();
        while let Some((bytes, handles)) = n.take_message(server) {
            let (h, _) = Header::decode(&bytes).unwrap();
            match h.ordinal {
                5 => {
                    let (_, q) = decode::<Listen>(&bytes, 1).unwrap();
                    log.listens.push(q.port.0);
                    let port = if q.port.0 == 0 { sc.picked } else { q.port.0 };
                    let answer = Listening {
                        status: Below(sc.listening),
                        port: Below(if sc.listening == status::OK { port } else { 0 }),
                    };
                    n.send_message(server, &msg(&answer, h.transaction), &[]);
                    if sc.listening == status::OK {
                        log.listeners.push((port, handles[0]));
                    } else {
                        n.close(handles[0]);
                    }
                }
                3 => {
                    let (_, c) = decode::<Connect>(&bytes, 3).unwrap();
                    log.connects += 1;
                    n.send_message(
                        server,
                        &msg(
                            &Connecting {
                                status: Below(status::OK),
                            },
                            h.transaction,
                        ),
                        &[],
                    );
                    let (ctl, in_write, out_read) = (handles[0], handles[1], handles[2]);
                    let to = log
                        .listeners
                        .iter()
                        .find(|(p, _)| *p == c.port.0)
                        .map(|(_, e)| *e);
                    match to {
                        Some(end) if n.signals_of(end) & signals::PEER_CLOSED == 0 => {
                            // The listener reads what the client writes.
                            n.send_message(
                                end,
                                &msg(
                                    &Accepted {
                                        address: 0x7f00_0001,
                                        port: Below(49153),
                                    },
                                    0,
                                ),
                                &[out_read, in_write],
                            );
                            n.send_message(
                                ctl,
                                &msg(
                                    &Connected {
                                        address: c.address,
                                        port: c.port,
                                    },
                                    0,
                                ),
                                &[],
                            );
                            n.close(ctl);
                        }
                        _ => {
                            n.send_message(
                                ctl,
                                &msg(
                                    &Failed {
                                        reason: Below(reason::REFUSED),
                                    },
                                    0,
                                ),
                                &[],
                            );
                            n.close(in_write);
                            n.close(out_read);
                            n.close(ctl);
                        }
                    }
                }
                _ => {}
            }
        }
    }));
    Rigged { r, log }
}

fn addr(a: [u8; 4], port: u16) -> Vec<u8> {
    sock::sockaddr_in(a, port).to_vec()
}

fn stream(r: &mut Rig) -> i32 {
    r.p.socket(sock::AF_INET, sock::SOCK_STREAM, 0).unwrap()
}

/// A listener on `port`.
fn listener(t: &mut Rigged, port: u16) -> i32 {
    let fd = stream(&mut t.r);
    t.r.p.bind(fd, &addr([0; 4], port)).unwrap();
    t.r.p.listen(fd, 8).unwrap();
    fd
}

#[test]
fn bind_records_listen_asks_netd_and_getsockname_has_the_port() {
    let mut t = rigged(Script::default());
    let fd = stream(&mut t.r);
    assert_eq!(t.r.p.getsockname(fd), Ok(([0; 4], 0)));
    t.r.p.bind(fd, &addr([0; 4], 8000)).unwrap();
    // `bind` alone asks nobody (ADR-0225 item 5): netd hears at `listen`.
    assert!(t.log.borrow().listens.is_empty());
    assert_eq!(t.r.p.getsockname(fd), Ok(([0; 4], 8000)));
    assert_eq!(
        t.r.p.getsockopt(fd, sock::SOL_SOCKET, sock::SO_ACCEPTCONN),
        Ok(0)
    );
    t.r.p.listen(fd, 5).unwrap();
    assert_eq!(t.log.borrow().listens, vec![8000]);
    assert_eq!(
        t.r.p.getsockopt(fd, sock::SOL_SOCKET, sock::SO_ACCEPTCONN),
        Ok(1)
    );
    assert_eq!(t.r.p.getsockname(fd), Ok(([0; 4], 8000)));
    // Listening again is `Ok` and asks nobody.
    t.r.p.listen(fd, 5).unwrap();
    assert_eq!(t.log.borrow().listens, vec![8000]);
    // A listener neither reads, writes, shuts down, connects nor binds again.
    assert_eq!(t.r.read(fd, 4), Err(ENOTCONN));
    assert_eq!(t.r.write(fd, b"x"), Err(ENOTCONN));
    assert_eq!(t.r.p.shutdown(fd, 2), Err(ENOTCONN));
    assert_eq!(t.r.p.connect(fd, &addr([127, 0, 0, 1], 8000)), Err(EISCONN));
    assert_eq!(t.r.p.bind(fd, &addr([0; 4], 8001)), Err(EINVAL));
    assert_eq!(t.r.p.getpeername(fd), Err(ENOTCONN));
    // `SO_REUSEADDR` is accepted and read back.
    t.r.p
        .setsockopt(fd, sock::SOL_SOCKET, sock::SO_REUSEADDR, 1)
        .unwrap();
    assert_eq!(
        t.r.p.getsockopt(fd, sock::SOL_SOCKET, sock::SO_REUSEADDR),
        Ok(1)
    );
}

#[test]
fn listen_without_bind_or_with_port_zero_gets_the_port_netd_picked() {
    let mut t = rigged(Script::default());
    let a = stream(&mut t.r);
    t.r.p.listen(a, 1).unwrap();
    assert_eq!(t.r.p.getsockname(a), Ok(([0; 4], 50000)));
    let b = stream(&mut t.r);
    t.r.p.bind(b, &addr([127, 0, 0, 1], 0)).unwrap();
    t.r.p.listen(b, 1).unwrap();
    assert_eq!(t.r.p.getsockname(b), Ok(([127, 0, 0, 1], 50000)));
    assert_eq!(t.log.borrow().listens, vec![0, 0]);
}

#[test]
fn bind_takes_the_wildcard_and_the_loopback_and_nothing_else() {
    let mut t = rigged(Script::default());
    for ok in [[0, 0, 0, 0], [127, 0, 0, 1], [127, 1, 2, 3]] {
        let fd = stream(&mut t.r);
        t.r.p.bind(fd, &addr(ok, 9000)).unwrap();
    }
    for other in [[10, 0, 2, 15], [8, 8, 8, 8], [255, 255, 255, 255]] {
        let fd = stream(&mut t.r);
        assert_eq!(t.r.p.bind(fd, &addr(other, 9000)), Err(EADDRNOTAVAIL));
        assert_eq!(t.r.p.getsockname(fd), Ok(([0; 4], 0)), "nothing recorded");
    }
    // Short, foreign family (AF_INET6 is a clean EAFNOSUPPORT), not a socket.
    let fd = stream(&mut t.r);
    assert_eq!(t.r.p.bind(fd, &[2, 0, 0]), Err(EINVAL));
    let mut v6 = addr([0; 4], 80);
    v6[..2].copy_from_slice(&(sock::AF_INET6 as u16).to_ne_bytes());
    assert_eq!(t.r.p.bind(fd, &v6), Err(EAFNOSUPPORT));
    assert_eq!(
        t.r.p.socket(sock::AF_INET6, sock::SOCK_STREAM, 0),
        Err(EAFNOSUPPORT)
    );
    assert_eq!(t.r.p.bind(99, &addr([0; 4], 80)), Err(EBADF));
    // Twice is `EINVAL`, as Linux has it.
    t.r.p.bind(fd, &addr([0; 4], 80)).unwrap();
    assert_eq!(t.r.p.bind(fd, &addr([0; 4], 81)), Err(EINVAL));
    // A socket that connected cannot be bound.
    let l = listener(&mut t, 7000);
    let c = stream(&mut t.r);
    t.r.p.connect(c, &addr([127, 0, 0, 1], 7000)).unwrap();
    assert_eq!(t.r.p.bind(c, &addr([0; 4], 82)), Err(EINVAL));
    let _ = l;
}

#[test]
fn a_refused_listen_says_why_and_holds_nothing() {
    for (code, errno) in [
        (status::NOT_GRANTED, EACCES),
        (status::BUSY, EADDRINUSE),
        (status::OVER_QUOTA, ENOBUFS),
        (status::INVALID, EINVAL),
        (status::UNAVAILABLE, ENETDOWN),
    ] {
        let mut t = rigged(Script {
            listening: code,
            ..Script::default()
        });
        let fd = stream(&mut t.r);
        t.r.p.bind(fd, &addr([0; 4], 8000)).unwrap();
        let before = t.r.handles();
        assert_eq!(t.r.p.listen(fd, 1), Err(errno), "status {code}");
        assert_eq!(t.r.handles(), before, "status {code}: a handle is held");
        // The socket is as it was: it can listen again (and be refused again).
        assert_eq!(
            t.r.p.getsockopt(fd, sock::SOL_SOCKET, sock::SO_ACCEPTCONN),
            Ok(0)
        );
        assert_eq!(t.r.p.listen(fd, 1), Err(errno));
        assert_eq!(t.r.handles(), before);
        t.r.p.close(fd).unwrap();
    }
}

#[test]
fn listen_without_the_network_is_refused_at_listen() {
    let mut r = rig();
    let fd = stream(&mut r);
    // `bind` needs no grant (ADR-0225 item 2); `listen` names a port.
    r.p.bind(fd, &addr([0; 4], 8000)).unwrap();
    assert_eq!(r.p.listen(fd, 1), Err(EACCES));
    assert_eq!(r.p.accept(fd, false, false), Err(EINVAL));
}

#[test]
fn a_connection_is_accepted_and_bytes_go_both_ways() {
    let mut t = rigged(Script::default());
    let l = listener(&mut t, 8000);
    let c = stream(&mut t.r);
    t.r.p.connect(c, &addr([127, 0, 0, 1], 8000)).unwrap();
    assert_eq!(t.r.p.getpeername(c), Ok(([127, 0, 0, 1], 8000)));
    let a = t.r.p.accept(l, false, false).unwrap();
    assert!(a > l && a != c);
    // The accepted socket: connected, its peer is the client's (made up)
    // address, its own name the listener's port on the loopback.
    assert_eq!(t.r.p.getpeername(a), Ok(([127, 0, 0, 1], 49153)));
    assert_eq!(t.r.p.getsockname(a), Ok(([127, 0, 0, 1], 8000)));
    assert_eq!(
        t.r.p.getsockopt(a, sock::SOL_SOCKET, sock::SO_ACCEPTCONN),
        Ok(0)
    );
    // The client writes, the server reads; then the other way.
    assert_eq!(t.r.write(c, b"GET / HTTP/1.0\r\n\r\n"), Ok(18));
    assert_eq!(t.r.read(a, 100).unwrap(), b"GET / HTTP/1.0\r\n\r\n");
    assert_eq!(t.r.write(a, b"HTTP/1.0 200 OK\r\n"), Ok(17));
    assert_eq!(t.r.read(c, 100).unwrap(), b"HTTP/1.0 200 OK\r\n");
    // The server closes: the client reads the end of the stream.
    t.r.p.close(a).unwrap();
    assert_eq!(t.r.read(c, 10).unwrap(), b"");
    // `shutdown(SHUT_WR)` of the client, on the next connection.
    let c2 = stream(&mut t.r);
    t.r.p.connect(c2, &addr([127, 0, 0, 1], 8000)).unwrap();
    let a2 = t.r.p.accept(l, false, true).unwrap();
    t.r.p.shutdown(c2, sock::SHUT_WR).unwrap();
    assert_eq!(t.r.read(a2, 10).unwrap(), b"");
}

#[test]
fn accept_is_eagain_when_non_blocking_and_the_flags_are_the_new_sockets() {
    let mut t = rigged(Script::default());
    let l = listener(&mut t, 8000);
    t.r.p.fcntl(l, F_SETFL, i64::from(o::NONBLOCK)).unwrap();
    assert_eq!(t.r.p.accept(l, false, false), Err(EAGAIN));
    let c = stream(&mut t.r);
    t.r.p.connect(c, &addr([127, 0, 0, 1], 8000)).unwrap();
    // The new socket is blocking whatever the listener is (Linux), unless
    // `accept4` says `SOCK_NONBLOCK`.
    let a = t.r.p.accept(l, false, false).unwrap();
    assert_eq!(t.r.p.fcntl(a, F_GETFL, 0).unwrap() & o::NONBLOCK as i32, 0);
    let c2 = stream(&mut t.r);
    t.r.p.connect(c2, &addr([127, 0, 0, 1], 8000)).unwrap();
    let b = t.r.p.accept(l, true, true).unwrap();
    assert_ne!(t.r.p.fcntl(b, F_GETFL, 0).unwrap() & o::NONBLOCK as i32, 0);
    // Empty again.
    assert_eq!(t.r.p.accept(l, true, false), Err(EAGAIN));
    // `accept` on what does not listen, and on what is not a socket.
    assert_eq!(t.r.p.accept(a, false, false), Err(EINVAL));
    assert_eq!(t.r.p.accept(99, false, false), Err(EBADF));
}

#[test]
fn poll_says_pollin_on_a_listener_exactly_when_a_connection_waits() {
    let mut t = rigged(Script::default());
    let l = listener(&mut t, 8000);
    let ready = |t: &mut Rigged| {
        let mut f = [PollFd {
            fd: l,
            events: POLLIN | POLLOUT,
            revents: 0,
        }];
        t.r.p.poll_once(&mut f, Some(0)).unwrap();
        f[0].revents & POLLIN != 0
    };
    assert!(!ready(&mut t));
    let c = stream(&mut t.r);
    t.r.p.connect(c, &addr([127, 0, 0, 1], 8000)).unwrap();
    assert!(ready(&mut t));
    t.r.p.fcntl(l, F_SETFL, i64::from(o::NONBLOCK)).unwrap();
    t.r.p.accept(l, false, false).unwrap();
    assert!(!ready(&mut t), "nothing waits after the accept");
    // A listener is never writable: POLLOUT alone reports nothing, with
    // or without a waiting connection.
    let out = |t: &mut Rigged| {
        let mut f = [PollFd {
            fd: l,
            events: POLLOUT,
            revents: 0,
        }];
        t.r.p.poll_once(&mut f, Some(0)).unwrap();
        f[0].revents
    };
    assert_eq!(out(&mut t), 0);
    let c2 = stream(&mut t.r);
    t.r.p.connect(c2, &addr([127, 0, 0, 1], 8000)).unwrap();
    assert_eq!(out(&mut t), 0);
    t.r.p.accept(l, false, false).unwrap();
    assert_eq!(t.r.p.accept(l, false, false), Err(EAGAIN));
}

#[test]
fn a_connect_where_nothing_listens_is_refused_and_a_closed_listener_frees_its_end() {
    let mut t = rigged(Script::default());
    let c = stream(&mut t.r);
    assert_eq!(
        t.r.p.connect(c, &addr([127, 0, 0, 1], 8000)),
        Err(ECONNREFUSED)
    );
    let l = listener(&mut t, 8000);
    let (_, end) = t.log.borrow().listeners[0];
    assert_eq!(
        t.r.net.borrow().signals_of(end) & signals::PEER_CLOSED,
        0,
        "open while the program holds the listener"
    );
    t.r.p.close(l).unwrap();
    // Closing the descriptor closes the channel: `netd` sees it end.
    assert_ne!(t.r.net.borrow().signals_of(end) & signals::PEER_CLOSED, 0);
    let c2 = stream(&mut t.r);
    assert_eq!(
        t.r.p.connect(c2, &addr([127, 0, 0, 1], 8000)),
        Err(ECONNREFUSED)
    );
}

#[test]
fn a_listener_whose_netd_went_is_econnaborted_once() {
    let mut t = rigged(Script::default());
    let l = listener(&mut t, 8000);
    let (_, end) = t.log.borrow().listeners[0];
    t.r.net.borrow_mut().close(end);
    t.r.p.fcntl(l, F_SETFL, i64::from(o::NONBLOCK)).unwrap();
    assert_eq!(t.r.p.accept(l, false, false), Err(ECONNABORTED));
    assert_eq!(t.r.p.accept(l, false, false), Err(EINVAL));
    assert_eq!(
        t.r.p.getsockopt(l, sock::SOL_SOCKET, sock::SO_ACCEPTCONN),
        Ok(0)
    );
}

#[test]
fn what_the_listener_channel_carries_that_is_not_a_connection_is_closed_and_skipped() {
    let mut t = rigged(Script::default());
    let l = listener(&mut t, 8000);
    let (_, end) = t.log.borrow().listeners[0];
    t.r.p.fcntl(l, F_SETFL, i64::from(o::NONBLOCK)).unwrap();
    let (s1, s2) = t.r.p.sys.stream_create().unwrap();
    let before = t.r.handles();
    {
        let mut n = t.r.net.borrow_mut();
        // The wrong number of handles, an ordinal that is not `Accepted`,
        // and bytes that are not a message at all.
        n.send_message(
            end,
            &msg(
                &Accepted {
                    address: 1,
                    port: Below(2),
                },
                0,
            ),
            &[s1],
        );
        n.send_message(
            end,
            &msg(
                &Connected {
                    address: 1,
                    port: Below(2),
                },
                0,
            ),
            &[s2],
        );
        n.send_message(end, &[1, 2, 3], &[]);
    }
    assert_eq!(t.r.p.accept(l, false, false), Err(EAGAIN));
    assert!(t.r.handles() < before, "the handles that came were closed");
}

#[test]
fn a_full_descriptor_table_does_not_take_a_connection() {
    let mut t = rigged(Script::default());
    let l = listener(&mut t, 8000);
    let c = stream(&mut t.r);
    t.r.p.connect(c, &addr([127, 0, 0, 1], 8000)).unwrap();
    // Fill the table with sockets; the accept says `EMFILE` and the
    // connection stays queued for a later accept.
    let mut fds = Vec::new();
    while let Ok(fd) = t.r.p.socket(sock::AF_INET, sock::SOCK_STREAM, 0) {
        fds.push(fd);
    }
    assert_eq!(t.r.p.accept(l, false, false), Err(EMFILE));
    t.r.p.close(fds.pop().unwrap()).unwrap();
    let a = t.r.p.accept(l, false, false).unwrap();
    assert_eq!(t.r.write(c, b"x"), Ok(1));
    assert_eq!(t.r.read(a, 1).unwrap(), b"x");
}

/// Review of F7b, note 6: a request that `netd` cannot take (its end of the
/// connection is gone) leaves no handle behind, for `listen` (one moved) and
/// `connect` (three). The `Sys` closes what it could not send, as
/// `ffi.rs`'s does, and the library keeps its own.
#[test]
fn a_request_to_a_gone_netd_leaks_no_handle() {
    let mut r = rig();
    let (client, server) = r.p.sys.channel_create().unwrap();
    r.p.net = Some(client);
    r.net.borrow_mut().close(server);
    let l = r.p.socket(sock::AF_INET, sock::SOCK_STREAM, 0).unwrap();
    let c = r.p.socket(sock::AF_INET, sock::SOCK_STREAM, 0).unwrap();
    let before = r.handles();
    assert_eq!(r.p.listen(l, 1), Err(ENETDOWN));
    assert_eq!(r.p.connect(c, &addr([127, 0, 0, 1], 8000)), Err(ENETDOWN));
    assert_eq!(r.handles(), before, "a handle of a refused request is held");
}

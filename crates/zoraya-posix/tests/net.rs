//! Sockets against a fake `netd` through the fake kernel (ADR-0225; N4): the
//! fake records the kind of every request it gets (INV-0225-2, INV-0225-3),
//! answers as a script says, and holds the ends of each connection so that a
//! test can feed the program bytes, read what it wrote, send `Reset`, and
//! close its ends.

mod kernel;

use std::cell::RefCell;
use std::rc::Rc;

use kernel::*;
use zoraya_posix::errno::*;
use zoraya_posix::process::{AT_FDCWD, F_GETFL, F_SETFL, o};
use zoraya_posix::socket::{self as sock, Hints};
use zoraya_posix::stat;
use zoraya_posix::sys::{Sys, signals};
use zoraya_proto_net::{
    Address, Connect, Connected, Connecting, Failed, Reset, Resolve, Resolved, reason, status,
};
use zoraya_protocol::{Below, Header, Seq, decode, encode};

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
zoraya_test_alloc::cap_memory!();

/// What the fake `netd` does.
#[derive(Clone)]
struct Script {
    /// The status of `Connecting`.
    connecting: u32,
    /// What follows an accepted connect: `Connected`, or `Failed(reason)`.
    failed: Option<u32>,
    /// `Connected` is held back until `go` is set (a connect in progress).
    hold: bool,
    /// Bytes put in the inbound stream when the connection opens.
    page: Vec<u8>,
    /// Close the inbound stream after `page` (the peer's FIN).
    fin: bool,
    /// The answer to `Resolve`: a status and addresses.
    resolve: (u32, Vec<u32>),
}

impl Default for Script {
    fn default() -> Script {
        Script {
            connecting: status::OK,
            failed: None,
            hold: false,
            page: Vec::new(),
            fin: true,
            resolve: (status::OK, vec![0x5db8_d822]),
        }
    }
}

/// One connection as `netd` holds it.
#[derive(Clone, Copy)]
struct Held {
    ctl: H,
    inbound: H,
    outbound: H,
}

#[derive(Default)]
struct Log {
    /// Request kinds, in order: `connect`, `resolve`, `other`.
    kinds: Vec<&'static str>,
    /// `(name, address, port)` of each connect.
    connects: Vec<(String, u32, u32)>,
    resolves: Vec<String>,
    held: Vec<Held>,
    /// Messages to send on a connection's channel at the next step.
    pending: Vec<(usize, Vec<u8>)>,
}

struct Rigged {
    r: Rig,
    log: Rc<RefCell<Log>>,
    /// The `netd` end of the connection at `NET`.
    server: H,
}

fn reply<'a, M: zoraya_protocol::Message<'a>>(m: &M, t: u32) -> Vec<u8> {
    let mut out = vec![0u8; 4096];
    let n = encode(m, t, &mut out).unwrap();
    out.truncate(n);
    out
}

fn rigged(script: Script) -> Rigged {
    let mut r = rig();
    let (client, server) = r.p.sys.channel_create().unwrap();
    r.p.net = Some(client);
    let log = Rc::new(RefCell::new(Log::default()));
    let script = Rc::new(script);
    let (l, s) = (log.clone(), script);
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| {
        let mut log = l.borrow_mut();
        let sc = s.clone();
        while let Some((bytes, handles)) = n.take_message(server) {
            let (h, _) = Header::decode(&bytes).unwrap();
            match h.ordinal {
                3 => {
                    let (_, c) = decode::<Connect>(&bytes, 3).unwrap();
                    log.kinds.push("connect");
                    log.connects.push((
                        String::from_utf8(c.name.0.to_vec()).unwrap(),
                        c.address,
                        c.port.0,
                    ));
                    let held = Held {
                        ctl: handles[0],
                        inbound: handles[1],
                        outbound: handles[2],
                    };
                    log.held.push(held);
                    n.send_message(
                        server,
                        &reply(
                            &Connecting {
                                status: Below(sc.connecting),
                            },
                            h.transaction,
                        ),
                        &[],
                    );
                    if sc.connecting != status::OK {
                        for x in handles {
                            n.close(x);
                        }
                        log.held.pop();
                        continue;
                    }
                    if let Some(why) = sc.failed {
                        n.send_message(held.ctl, &reply(&Failed { reason: Below(why) }, 0), &[]);
                        n.close(held.inbound);
                        n.close(held.outbound);
                    } else if !sc.hold {
                        n.send_message(
                            held.ctl,
                            &reply(
                                &Connected {
                                    address: c.address,
                                    port: c.port,
                                },
                                0,
                            ),
                            &[],
                        );
                        n.push(held.inbound, &sc.page);
                        if sc.fin {
                            n.close(held.inbound);
                        }
                    }
                }
                4 => {
                    let (_, q) = decode::<Resolve>(&bytes, 0).unwrap();
                    log.kinds.push("resolve");
                    log.resolves
                        .push(String::from_utf8(q.name.0.to_vec()).unwrap());
                    let list: Vec<Address> = sc
                        .resolve
                        .1
                        .iter()
                        .map(|&a| Address { address: a })
                        .collect();
                    n.send_message(
                        server,
                        &reply(
                            &Resolved {
                                status: Below(sc.resolve.0),
                                addresses: Seq::Of(&list),
                            },
                            h.transaction,
                        ),
                        &[],
                    );
                }
                _ => log.kinds.push("other"),
            }
        }
        for (i, m) in std::mem::take(&mut log.pending) {
            let ctl = log.held[i].ctl;
            n.send_message(ctl, &m, &[]);
        }
    }));
    Rigged { r, log, server }
}

const EXAMPLE: [u8; 4] = [93, 184, 216, 34];

fn addr(a: [u8; 4], port: u16) -> Vec<u8> {
    sock::sockaddr_in(a, port).to_vec()
}

fn stream(r: &mut Rig) -> i32 {
    r.p.socket(sock::AF_INET, sock::SOCK_STREAM, 0).unwrap()
}

fn recv(r: &mut Rig, fd: i32, n: usize) -> Result<Vec<u8>, i32> {
    let u = r.arena.user(kernel::ARENA / 2, n);
    let got = r.p.sock_read(fd, u, false)?;
    Ok(r.peek(kernel::ARENA / 2, got))
}

#[test]
fn a_socket_without_the_network_is_made_and_refused_at_connect() {
    let mut r = rig();
    let fd = stream(&mut r);
    assert_eq!(r.p.connect(fd, &addr(EXAMPLE, 80)), Err(EACCES));
    let (rest, port) = (r.p.getsockname(fd).unwrap(), 0);
    assert_eq!(rest, ([0; 4], port));
    let err =
        r.p.lookup(Some(b"example.org"), Some(b"80"), &Hints::default())
            .unwrap_err();
    assert_eq!((err.eai, err.errno), (sock::EAI_SYSTEM, EACCES));
    // A literal address needs no network to resolve.
    let l =
        r.p.lookup(Some(b"10.0.2.2"), Some(b"8080"), &Hints::default())
            .unwrap();
    assert_eq!((l.addresses, l.port), (vec![[10, 0, 2, 2]], 8080));
    r.p.close(fd).unwrap();
}

#[test]
fn a_page_is_fetched_by_address() {
    let page = b"HTTP/1.1 200 OK\r\n\r\nhello".to_vec();
    let mut t = rigged(Script {
        page: page.clone(),
        ..Script::default()
    });
    let fd = stream(&mut t.r);
    t.r.p.connect(fd, &addr([10, 0, 2, 2], 8080)).unwrap();
    // The request goes to the outbound stream, not through a request.
    let n = t.r.write(fd, b"GET / HTTP/1.1\r\n\r\n").unwrap();
    assert_eq!(n, 18);
    let held = t.log.borrow().held[0];
    assert_eq!(
        t.r.net.borrow_mut().pulled(held.outbound),
        b"GET / HTTP/1.1\r\n\r\n"
    );
    // The whole page, then the end of the stream.
    let mut got = Vec::new();
    loop {
        let part = t.r.read(fd, 7).unwrap();
        if part.is_empty() {
            break;
        }
        got.extend(part);
    }
    assert_eq!(got, page);
    assert_eq!(t.r.p.getpeername(fd).unwrap(), ([10, 0, 2, 2], 8080));
    let log = t.log.borrow();
    // An address from nowhere is a connect by address, and no resolve ran.
    assert_eq!(log.kinds, vec!["connect"]);
    assert_eq!(log.connects, vec![(String::new(), 0x0a00_0202, 8080)]);
}

#[test]
fn a_name_resolved_by_getaddrinfo_is_connected_by_name() {
    // INV-0225-2 and INV-0225-3: one resolve per getaddrinfo, and the
    // connect that follows carries the name.
    let mut t = rigged(Script {
        page: b"ok".to_vec(),
        ..Script::default()
    });
    let l =
        t.r.p
            .lookup(
                Some(b"test.zoraya"),
                Some(b"http"),
                &Hints {
                    flags: sock::AI_CANONNAME,
                    ..Hints::default()
                },
            )
            .unwrap();
    let (found, port) = (l.addresses.clone(), l.port);
    assert_eq!(
        (l.addresses, l.port, l.canonical.as_deref()),
        (vec![EXAMPLE], 80, Some("test.zoraya"))
    );
    let fd = stream(&mut t.r);
    t.r.p.connect(fd, &addr(found[0], port)).unwrap();
    assert_eq!(t.r.read(fd, 10).unwrap(), b"ok");
    let log = t.log.borrow();
    assert_eq!(log.kinds, vec!["resolve", "connect"]);
    assert_eq!(log.resolves, vec!["test.zoraya"]);
    assert_eq!(log.connects, vec![("test.zoraya".to_string(), 0, 80)]);
}

#[test]
fn a_fork_child_has_no_connection_to_netd() {
    // A channel end is not duplicable (ADR-0102 item 3), so the child holds
    // no connection (ADR-0287 item 3): it must not keep the parent's slot
    // value, which a handle of its own could take, nor the parent's names.
    let mut t = rigged(Script::default());
    let l =
        t.r.p
            .lookup(Some(b"test.zoraya"), Some(b"http"), &Hints::default())
            .unwrap();
    assert!(!t.r.p.addrs.is_empty());
    let found = l.addresses[0];
    t.r.p.after_fork_in_child(77, 77);
    assert!(t.r.p.net.is_none());
    assert!(t.r.p.addrs.is_empty());
    let fd = stream(&mut t.r);
    assert_eq!(t.r.p.connect(fd, &addr(found, 80)), Err(EACCES));
    let err =
        t.r.p
            .lookup(Some(b"test.zoraya"), Some(b"80"), &Hints::default())
            .unwrap_err();
    assert_eq!((err.eai, err.errno), (sock::EAI_SYSTEM, EACCES));
    // Nothing but the parent's one resolve reached netd.
    assert_eq!(t.log.borrow().kinds, vec!["resolve"]);
}

#[test]
fn a_literal_or_a_numeric_service_asks_nobody() {
    let mut t = rigged(Script::default());
    let l =
        t.r.p
            .lookup(Some(b"1.2.3.4"), Some(b"443"), &Hints::default())
            .unwrap();
    assert_eq!(l.addresses, vec![[1, 2, 3, 4]]);
    // An address spelled any other way is not one: `127.1` ends in digits,
    // so it is no name either, and nobody is asked.
    let e =
        t.r.p
            .lookup(Some(b"127.1"), None, &Hints::default())
            .unwrap_err();
    assert_eq!(e.eai, sock::EAI_NONAME);
    let e =
        t.r.p
            .lookup(Some(b"a b"), None, &Hints::default())
            .unwrap_err();
    assert_eq!(e.eai, sock::EAI_NONAME);
    assert!(t.log.borrow().kinds.is_empty());
}

#[test]
fn resolve_statuses_become_getaddrinfo_codes() {
    for (st, eai, errno) in [
        (status::NOT_GRANTED, sock::EAI_SYSTEM, EACCES),
        (status::UNRESOLVABLE, sock::EAI_NONAME, 0),
        (status::LOCAL, sock::EAI_NONAME, 0),
        (status::UNAVAILABLE, sock::EAI_AGAIN, 0),
        (status::TIMED_OUT, sock::EAI_AGAIN, 0),
    ] {
        let mut t = rigged(Script {
            resolve: (st, vec![]),
            ..Script::default()
        });
        let e =
            t.r.p
                .lookup(Some(b"example.org"), None, &Hints::default())
                .unwrap_err();
        assert_eq!((e.eai, e.errno), (eai, errno), "status {st}");
    }
}

#[test]
fn a_refused_connect_is_the_errno_of_its_status_or_reason() {
    for (st, errno) in [
        (status::NOT_GRANTED, EACCES),
        (status::UNAVAILABLE, ENETUNREACH),
        (status::OVER_QUOTA, ENOBUFS),
    ] {
        let mut t = rigged(Script {
            connecting: st,
            ..Script::default()
        });
        let fd = stream(&mut t.r);
        assert_eq!(
            t.r.p.connect(fd, &addr(EXAMPLE, 80)),
            Err(errno),
            "status {st}"
        );
        // Nothing is left for the socket: the handles were closed.
        assert_eq!(t.r.write(fd, b"x"), Err(ENOTCONN));
    }
    for (why, errno) in [
        (reason::REFUSED, ECONNREFUSED),
        (reason::UNREACHABLE, ENETUNREACH),
        (reason::TIMED_OUT, ETIMEDOUT),
        (reason::LOOKUP_TIMED_OUT, ETIMEDOUT),
        (reason::UNRESOLVABLE, EHOSTUNREACH),
        (reason::LOCAL, EHOSTUNREACH),
        (reason::OVER_QUOTA, ENOBUFS),
    ] {
        let mut t = rigged(Script {
            failed: Some(why),
            ..Script::default()
        });
        let fd = stream(&mut t.r);
        assert_eq!(
            t.r.p.connect(fd, &addr(EXAMPLE, 80)),
            Err(errno),
            "reason {why}"
        );
        // The socket is dead: a second connect is `ECONNABORTED`.
        assert_eq!(t.r.p.connect(fd, &addr(EXAMPLE, 80)), Err(ECONNABORTED));
        t.r.p.close(fd).unwrap();
    }
}

#[test]
fn a_nonblocking_connect_is_in_progress_until_the_channel_says_so() {
    let mut t = rigged(Script {
        hold: true,
        page: b"late".to_vec(),
        ..Script::default()
    });
    let fd =
        t.r.p
            .socket(sock::AF_INET, sock::SOCK_STREAM | sock::SOCK_NONBLOCK, 0)
            .unwrap();
    assert_eq!(t.r.p.connect(fd, &addr(EXAMPLE, 80)), Err(EINPROGRESS));
    assert_eq!(t.r.p.connect(fd, &addr(EXAMPLE, 80)), Err(EALREADY));
    assert_eq!(
        t.r.p.getsockopt(fd, sock::SOL_SOCKET, sock::SO_ERROR),
        Ok(0)
    );
    assert_eq!(recv(&mut t.r, fd, 10), Err(EAGAIN));
    // A connecting socket is waited on through its channel.
    let held = t.log.borrow().held[0];
    let sk = match t.r.p.fds.get(fd) {
        Some(zoraya_posix::process::Desc::Socket(sk)) => sk.wait_source(false),
        _ => None,
    };
    assert_eq!(
        sk.map(|(_, w)| w),
        Some(signals::READABLE | signals::PEER_CLOSED)
    );
    t.log.borrow_mut().pending.push((
        0,
        reply(
            &Connected {
                address: 0x5db8_d822,
                port: Below(80),
            },
            0,
        ),
    ));
    t.r.net.borrow_mut().push(held.inbound, b"late");
    t.r.p.finish_connect(fd, true).unwrap();
    assert_eq!(
        t.r.p.getsockopt(fd, sock::SOL_SOCKET, sock::SO_ERROR),
        Ok(0)
    );
    assert_eq!(t.r.p.connect(fd, &addr(EXAMPLE, 80)), Err(EISCONN));
    assert_eq!(recv(&mut t.r, fd, 10).unwrap(), b"late");
}

#[test]
fn a_reset_arrives_after_the_data_before_it() {
    let mut t = rigged(Script {
        page: b"abc".to_vec(),
        fin: true,
        ..Script::default()
    });
    let fd = stream(&mut t.r);
    t.r.p.connect(fd, &addr(EXAMPLE, 80)).unwrap();
    let ctl = t.log.borrow().held[0].ctl;
    t.r.net
        .borrow_mut()
        .send_message(ctl, &reply(&Reset { why: Below(0) }, 0), &[]);
    assert_eq!(recv(&mut t.r, fd, 10).unwrap(), b"abc");
    assert_eq!(recv(&mut t.r, fd, 10), Err(ECONNRESET));
    // Said once: the next read is the end of the stream.
    assert_eq!(recv(&mut t.r, fd, 10).unwrap(), b"");
}

#[test]
fn shutdown_write_ends_our_stream_and_reads_go_on() {
    let mut t = rigged(Script {
        page: b"reply".to_vec(),
        ..Script::default()
    });
    let fd = stream(&mut t.r);
    assert_eq!(t.r.p.shutdown(fd, sock::SHUT_WR), Err(ENOTCONN));
    t.r.p.connect(fd, &addr(EXAMPLE, 80)).unwrap();
    t.r.write(fd, b"q").unwrap();
    t.r.p.shutdown(fd, sock::SHUT_WR).unwrap();
    let held = t.log.borrow().held[0];
    // `netd` sees the bytes, then the end.
    assert_eq!(t.r.net.borrow_mut().pulled(held.outbound), b"q");
    assert_ne!(
        t.r.net.borrow().signals_of(held.outbound) & signals::PEER_CLOSED,
        0
    );
    assert_eq!(t.r.write(fd, b"x"), Err(EPIPE));
    assert_eq!(t.r.read(fd, 10).unwrap(), b"reply");
    assert_eq!(t.r.p.shutdown(fd, 7), Err(EINVAL));
}

#[test]
fn closing_a_socket_gives_back_every_handle() {
    let mut t = rigged(Script {
        fin: false,
        ..Script::default()
    });
    let before = t.r.handles();
    let fd = stream(&mut t.r);
    t.r.p.connect(fd, &addr(EXAMPLE, 80)).unwrap();
    t.r.p.close(fd).unwrap();
    // `netd`'s ends are still the fake's; close them as it would.
    let held = t.log.borrow().held[0];
    for h in [held.ctl, held.inbound, held.outbound] {
        t.r.net.borrow_mut().close(h);
    }
    assert_eq!(t.r.handles(), before);
    // And a socket never connected.
    let fd = stream(&mut t.r);
    t.r.p.close(fd).unwrap();
    assert_eq!(t.r.handles(), before);
}

#[test]
fn the_descriptor_calls_know_a_socket() {
    let mut t = rigged(Script::default());
    let fd = stream(&mut t.r);
    // `fstat` says socket; a seek is a seek on a pipe.
    let st = t.r.p.fstat(fd).unwrap();
    let mode = u32::from_le_bytes(st[16..20].try_into().unwrap());
    assert_eq!(mode & stat::S_IFMT, stat::S_IFSOCK);
    assert_eq!(t.r.p.lseek(fd, 0, 0), Err(ESPIPE));
    assert_eq!(t.r.p.fsync(fd), Err(EINVAL));
    // Not a socket: `ENOTSOCK`; not a descriptor: `EBADF`.
    let f = t.r.p.open(AT_FDCWD, b"/dev/null", 0, 0).unwrap();
    assert_eq!(t.r.p.shutdown(f, 0), Err(ENOTSOCK));
    assert_eq!(t.r.p.shutdown(99, 0), Err(EBADF));
    // `O_NONBLOCK` through `fcntl`, as for a pipe.
    assert_eq!(t.r.p.fcntl(fd, F_GETFL, 0).unwrap() & o::NONBLOCK as i32, 0);
    t.r.p.fcntl(fd, F_SETFL, i64::from(o::NONBLOCK)).unwrap();
    assert_ne!(t.r.p.fcntl(fd, F_GETFL, 0).unwrap() & o::NONBLOCK as i32, 0);
    // An unconnected socket has no peer and reads and writes fail.
    assert_eq!(t.r.p.getpeername(fd), Err(ENOTCONN));
    assert_eq!(recv(&mut t.r, fd, 4), Err(ENOTCONN));
    assert_eq!(t.r.write(fd, b"x"), Err(ENOTCONN));
}

#[test]
fn what_is_not_offered_is_refused() {
    let mut r = rig();
    assert_eq!(
        r.p.socket(sock::AF_INET6, sock::SOCK_STREAM, 0),
        Err(EAFNOSUPPORT)
    );
    assert_eq!(
        r.p.socket(sock::AF_INET, sock::SOCK_RAW, 0),
        Err(EPROTONOSUPPORT)
    );
    assert_eq!(
        r.p.socket(sock::AF_INET, sock::SOCK_STREAM, 17),
        Err(EPROTONOSUPPORT)
    );
    let fd = stream(&mut r);
    assert_eq!(r.p.connect(fd, &[0; 4]), Err(EINVAL));
    let mut v6 = addr(EXAMPLE, 80);
    v6[0] = 10;
    assert_eq!(r.p.connect(fd, &v6), Err(EAFNOSUPPORT));
    assert_eq!(r.p.connect(fd, &addr(EXAMPLE, 0)), Err(EADDRNOTAVAIL));
    assert_eq!(
        r.p.setsockopt(fd, sock::SOL_SOCKET, 1234, 1),
        Err(ENOPROTOOPT)
    );
    r.p.setsockopt(fd, sock::IPPROTO_TCP, sock::TCP_NODELAY, 1)
        .unwrap();
    assert_eq!(
        r.p.getsockopt(fd, sock::IPPROTO_TCP, sock::TCP_NODELAY),
        Ok(1)
    );
    assert_eq!(
        r.p.getsockopt(fd, sock::SOL_SOCKET, sock::SO_TYPE),
        Ok(sock::SOCK_STREAM)
    );
    assert_eq!(
        zoraya_posix::process::Posix::<Fake>::check_msg_flags(sock::MSG_PEEK),
        Err(EOPNOTSUPP)
    );
}

#[test]
fn a_dead_netd_is_enetdown() {
    let mut t = rigged(Script::default());
    t.r.net.borrow_mut().close(t.server);
    let fd = stream(&mut t.r);
    assert_eq!(t.r.p.connect(fd, &addr(EXAMPLE, 80)), Err(ENETDOWN));
    let e =
        t.r.p
            .lookup(Some(b"example.org"), None, &Hints::default())
            .unwrap_err();
    assert_eq!((e.eai, e.errno), (sock::EAI_SYSTEM, ENETDOWN));
}

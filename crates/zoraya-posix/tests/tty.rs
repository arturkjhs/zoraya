//! The terminal calls of libposix over the fake kernel and a scripted
//! terminal service (ADR-0331 items 2 and 5, ADR-0342 item 4, ADR-0389):
//! `tcsetattr`'s three actions, `tcflush`, `tcdrain`, the `IO` status as
//! `EIO`, and `open("/dev/tty")`.

mod kernel;

use kernel::*;
use std::cell::RefCell;
use std::rc::Rc;
use zoraya_abi::posix::EntryKind;
use zoraya_posix::errno::*;
use zoraya_posix::process::{self, AT_FDCWD, o};
use zoraya_posix::sys::Sys;
use zoraya_posix::termios;
use zoraya_proto_tty as tp;
use zoraya_protocol::{Below, Header, Message, encode};

zoraya_test_alloc::cap_memory!();

/// What the scripted service was asked, in order: `(ordinal, handles)`.
type Log = Rc<RefCell<Vec<(u32, usize)>>>;

struct Tty {
    fd: i32,
    server: H,
    log: Log,
}

/// A terminal descriptor whose service answers each request at once with
/// `status` for the three that have one (`OK` unless `status` says).
fn tty_with(r: &mut Rig, status: u32) -> Tty {
    let (conn, server) = r.p.sys.channel_create().unwrap();
    let (_out, wr) = r.p.sys.stream_create().unwrap();
    let fd =
        r.p.fds
            .install(
                0,
                process::Desc::Stream {
                    read: None,
                    write: Some(wr),
                    nonblock: false,
                    tty: true,
                    conn: Some(conn),
                },
                false,
            )
            .unwrap() as i32;
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| {
        while let Some((bytes, handles)) = net.take_message(server) {
            let (h, _) = Header::decode(&bytes).unwrap();
            seen.borrow_mut().push((h.ordinal, handles.len()));
            for hd in handles {
                net.close(hd);
            }
            let mut buf = [0u8; 128];
            let n = if status != tp::status::OK {
                zoraya_protocol::encode(
                    &zoraya_protocol::ErrorReply { status },
                    h.transaction,
                    &mut buf,
                )
                .unwrap()
            } else if h.ordinal == tp::SetAttr::ORDINAL {
                let m = tp::SetAttrReply {
                    status: Below(tp::status::OK),
                };
                encode(&m, h.transaction, &mut buf).unwrap()
            } else if h.ordinal == tp::Flush::ORDINAL {
                let m = tp::FlushReply {
                    status: Below(tp::status::OK),
                };
                encode(&m, h.transaction, &mut buf).unwrap()
            } else if h.ordinal == tp::Drain::ORDINAL {
                let m = tp::DrainReply {
                    status: Below(tp::status::OK),
                };
                encode(&m, h.transaction, &mut buf).unwrap()
            } else if h.ordinal == tp::Clone::ORDINAL {
                let m = tp::CloneReply {
                    status: Below(tp::status::OK),
                };
                encode(&m, h.transaction, &mut buf).unwrap()
            } else {
                panic!("unexpected ordinal {}", h.ordinal)
            };
            net.send_message(server, &buf[..n], &[]);
        }
    }));
    Tty { fd, server, log }
}

fn ordinals(t: &Tty) -> Vec<u32> {
    t.log.borrow().iter().map(|e| e.0).collect()
}

const SET: u32 = tp::SetAttr::ORDINAL;
const FLUSH: u32 = tp::Flush::ORDINAL;
const DRAIN: u32 = tp::Drain::ORDINAL;

#[test]
fn tcsetattr_has_three_actions_that_differ_as_posix_says() {
    // INV-0331-3's `tcsetattr` half.
    let mut r = rig();
    let t = tty_with(&mut r, tp::status::OK);
    let attrs = [0u8; termios::SIZE];
    assert_eq!(r.p.tty_set_attr(t.fd, termios::TCSANOW, &attrs), Ok(()));
    assert_eq!(ordinals(&t), [SET]);
    t.log.borrow_mut().clear();
    assert_eq!(r.p.tty_set_attr(t.fd, termios::TCSADRAIN, &attrs), Ok(()));
    assert_eq!(
        ordinals(&t),
        [DRAIN, SET],
        "wait for the output, then change"
    );
    t.log.borrow_mut().clear();
    assert_eq!(r.p.tty_set_attr(t.fd, termios::TCSAFLUSH, &attrs), Ok(()));
    assert_eq!(
        ordinals(&t),
        [DRAIN, FLUSH, SET],
        "wait, drop the unread input, then change"
    );
    t.log.borrow_mut().clear();
    for bad in [3, -1, 99] {
        assert_eq!(r.p.tty_set_attr(t.fd, bad, &attrs), Err(EINVAL));
    }
    assert!(ordinals(&t).is_empty(), "a bad action sends nothing");
}

#[test]
fn a_connection_of_a_line_that_is_not_in_front_gets_eio() {
    // The service's IO status (ADR-0331 item 4) is `EIO`, not `EINVAL`.
    let mut r = rig();
    let t = tty_with(&mut r, tp::status::IO);
    let attrs = [0u8; termios::SIZE];
    assert_eq!(r.p.tty_set_attr(t.fd, termios::TCSANOW, &attrs), Err(EIO));
    assert_eq!(r.p.tty_flush(t.fd, termios::TCIFLUSH), Err(EIO));
    assert_eq!(r.p.tty_drain(t.fd), Err(EIO));
    // A refusal that is not `IO` stays `EINVAL`, a control request `EPERM`.
    let u = tty_with(&mut r, tp::status::INVALID);
    assert_eq!(
        r.p.tty_set_attr(u.fd, termios::TCSANOW, &attrs),
        Err(EINVAL)
    );
    let v = tty_with(&mut r, tp::status::NOT_ALLOWED);
    assert_eq!(r.p.tty_drain(v.fd), Err(EPERM));
}

#[test]
fn tcflush_drops_unread_input_and_what_the_library_kept() {
    let mut r = rig();
    let t = tty_with(&mut r, tp::status::OK);
    let conn = r.p.tty_of(t.fd).unwrap();
    r.p.tty_rest.push((conn, b"kept".to_vec()));
    assert_eq!(r.p.tty_flush(t.fd, termios::TCOFLUSH), Ok(()));
    assert!(
        ordinals(&t).is_empty(),
        "output is a stream: nothing to ask"
    );
    assert_eq!(r.p.tty_rest.len(), 1, "TCOFLUSH keeps the input");
    assert_eq!(r.p.tty_flush(t.fd, termios::TCIFLUSH), Ok(()));
    assert_eq!(ordinals(&t), [FLUSH]);
    assert!(r.p.tty_rest.is_empty());
    r.p.tty_rest.push((conn, b"more".to_vec()));
    assert_eq!(r.p.tty_flush(t.fd, termios::TCIOFLUSH), Ok(()));
    assert_eq!(ordinals(&t), [FLUSH, FLUSH]);
    assert!(r.p.tty_rest.is_empty());
    assert_eq!(r.p.tty_flush(t.fd, 7), Err(EINVAL));
    assert_eq!(r.p.tty_flush(99, 0), Err(EBADF));
    let (rd, _wr) = r.p.pipe(0).unwrap();
    assert_eq!(r.p.tty_flush(rd, 0), Err(ENOTTY));
    assert_eq!(r.p.tty_drain(rd), Err(ENOTTY));
}

#[test]
fn tcdrain_waits_for_the_service() {
    let mut r = rig();
    let t = tty_with(&mut r, tp::status::OK);
    assert_eq!(r.p.tty_drain(t.fd), Ok(()));
    assert_eq!(ordinals(&t), [DRAIN]);
}

#[test]
fn open_dev_tty_clones_the_entrys_connection_for_the_callers_group() {
    // INV-0331-1: `open("/dev/tty")` succeeds exactly when there is a terminal.
    let mut r = rig();
    // No terminal entry: no /dev/tty (the table has none), `ENOENT`.
    assert!(r.p.open(AT_FDCWD, b"/dev/tty", o::RDWR, 0).is_err());
    let t = tty_with(&mut r, tp::status::OK);
    let entry_conn = r.p.tty_of(t.fd).unwrap();
    // The entry's own connection is its own channel to the same service.
    let (base, base_server) = r.p.sys.channel_create().unwrap();
    let _ = (entry_conn, t.server);
    r.p.add_entry("/dev/tty", EntryKind::Terminal, Some(base));
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    let group = Rc::new(std::cell::Cell::new(0));
    let group_seen = group.clone();
    r.p.facts.pgid = 4242;
    r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| {
        while let Some((bytes, handles)) = net.take_message(base_server) {
            let (h, _) = Header::decode(&bytes).unwrap();
            let (_, c) = zoraya_protocol::decode::<tp::Clone>(&bytes, handles.len()).unwrap();
            group_seen.set(c.group);
            seen.borrow_mut().push((h.ordinal, handles.len()));
            for hd in handles {
                net.close(hd);
            }
            let mut buf = [0u8; 64];
            let m = tp::CloneReply {
                status: Below(tp::status::OK),
            };
            let n = encode(&m, h.transaction, &mut buf).unwrap();
            net.send_message(base_server, &buf[..n], &[]);
        }
    }));
    let fd = r.p.open(AT_FDCWD, b"/dev/tty", o::RDWR, 0).unwrap();
    assert_eq!(log.borrow().as_slice(), [(tp::Clone::ORDINAL, 1)]);
    assert_eq!(group.get(), 4242, "tagged with the caller's own group");
    assert_eq!(r.p.isatty(fd), Ok(true));
    assert_ne!(
        r.p.tty_of(fd).unwrap(),
        entry_conn,
        "a connection of its own"
    );
    // It writes where the terminal's output goes.
    match r.p.fds.get(fd) {
        Some(process::Desc::Stream {
            write: Some(_),
            read: None,
            ..
        }) => {}
        other => panic!("not a terminal description: {other:?}"),
    }
}

#[test]
fn open_dev_tty_without_a_connection_in_the_entry_is_enxio() {
    let mut r = rig();
    r.p.add_entry("/dev/tty", EntryKind::Terminal, None);
    assert_eq!(r.p.open(AT_FDCWD, b"/dev/tty", o::RDWR, 0), Err(ENXIO));
}

#[test]
fn a_terminal_refuses_input_injection_and_a_size_change() {
    // INV-0338-1: TIOCSTI is ENOTTY on a terminal, TIOCSWINSZ is EPERM; a pipe
    // and a closed fd say what any ioctl says.
    let mut r = rig();
    let t = tty_with(&mut r, tp::status::OK);
    assert_eq!(r.p.tty_ioctl_refusal(t.fd, termios::TIOCSTI), Err(ENOTTY));
    assert_eq!(r.p.tty_ioctl_refusal(t.fd, termios::TIOCSWINSZ), Err(EPERM));
    assert_eq!(r.p.tty_ioctl_refusal(t.fd, 0x5413), Ok(()));
    let (rd, _wr) = r.p.pipe(0).unwrap();
    assert_eq!(r.p.tty_ioctl_refusal(rd, termios::TIOCSTI), Err(ENOTTY));
    assert_eq!(r.p.tty_ioctl_refusal(99, termios::TIOCSTI), Err(EBADF));
}

#[test]
fn ioctl_free_names_of_the_terminal() {
    assert_eq!(termios::TTY_NAME, b"/dev/tty\0");
}

use super::*;
use std::vec;
use std::vec::Vec;
use zoraya_protocol::{ERROR_ORDINAL, NO_REPLY};

type H = u32;

fn message<'m, M: Message<'m>>(m: &M, t: u32) -> Vec<u8> {
    let mut out = vec![0; 8192];
    let n = encode(m, t, &mut out).unwrap();
    out.truncate(n);
    out
}

/// The bytes of a `Read` reply, or the error status.
fn read_reply(bytes: &[u8]) -> Result<Vec<u8>, u32> {
    let (h, _) = Header::decode(bytes).unwrap();
    if h.ordinal == ERROR_ORDINAL {
        return Err(reply_status(bytes));
    }
    Ok(decode::<ReadReply<'_>>(bytes, 0)
        .unwrap()
        .1
        .bytes
        .0
        .to_vec())
}

fn replies(actions: &[Action<H>]) -> Vec<(Id, Vec<u8>)> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Reply { to, bytes } => Some((*to, bytes.clone())),
            _ => None,
        })
        .collect()
}

fn shown(actions: &[Action<H>]) -> Vec<u8> {
    let mut out = Vec::new();
    for a in actions {
        if let Action::Show(b) = a {
            out.extend_from_slice(b);
        }
    }
    out
}

/// A terminal with the broker's control connection (id 0) and the shell's
/// connection in group 2 (id 1), group 2 in the foreground with job 100.
fn session() -> Terminal<H> {
    let mut t = Terminal::new();
    assert_eq!(t.add_control(), Some(0));
    assert_eq!(t.add_terminal(2), Some(1));
    let a = t.request(0, &message(&Foreground { group: 2 }, 1), vec![100]);
    assert_eq!(replies(&a).len(), 1);
    assert_eq!(reply_status(&replies(&a)[0].1), status::OK);
    t
}

#[test]
fn a_line_is_echoed_and_read_whole_after_its_newline() {
    let mut t = session();
    let a = t.request(1, &message(&Read { max: 100 }, 5), vec![]);
    assert!(replies(&a).is_empty(), "held until the line completes");
    let a = t.keys(b"echo hi");
    assert_eq!(shown(&a), b"echo hi");
    assert!(replies(&a).is_empty());
    let a = t.keys(b"\r");
    assert_eq!(shown(&a), b"\r\n", "ICRNL in, ONLCR on the echo");
    let r = replies(&a);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].0, 1);
    assert_eq!(Header::decode(&r[0].1).unwrap().0.transaction, 5);
    assert_eq!(read_reply(&r[0].1).unwrap(), b"echo hi\n");
}

#[test]
fn erase_kill_and_eof_edit_the_line() {
    let mut d = Discipline::new();
    let mut echo = Vec::new();
    for &b in b"abx\x7f" {
        d.input(b, &mut echo);
    }
    assert_eq!(echo, b"abx\x08 \x08");
    d.input(0x08, &mut echo);
    d.input(b'c', &mut echo);
    d.input(b'\n', &mut echo);
    assert_eq!(d.read(100).unwrap(), b"ac\n");
    // KILL erases the line; then EOF at the start of a line is end of file.
    for &b in b"junk\x15" {
        d.input(b, &mut echo);
    }
    d.input(0x04, &mut echo);
    assert_eq!(d.read(100).unwrap(), b"");
    // EOF in the middle of a line gives the line without a newline.
    for &b in b"ab\x04" {
        d.input(b, &mut echo);
    }
    assert_eq!(d.read(100).unwrap(), b"ab");
    assert!(d.read(100).is_none());
    // A UTF-8 character is erased whole.
    for &b in "é\x7f\n".as_bytes() {
        d.input(b, &mut echo);
    }
    assert_eq!(d.read(100).unwrap(), b"\n");
}

#[test]
fn a_short_read_leaves_the_rest_of_the_line() {
    let mut d = Discipline::new();
    let mut echo = Vec::new();
    for &b in b"abcdef\n" {
        d.input(b, &mut echo);
    }
    assert_eq!(d.read(4).unwrap(), b"abcd");
    assert_eq!(d.read(4).unwrap(), b"ef\n");
    assert!(d.read(4).is_none());
}

#[test]
fn interrupt_flushes_and_signals_the_foreground_job_once() {
    let mut t = session();
    let a = t.keys(b"sleep 30\x03");
    let signals: Vec<_> = a
        .iter()
        .filter(|a| matches!(a, Action::Signal { .. }))
        .collect();
    assert_eq!(
        signals,
        [&Action::Signal {
            job: 100,
            kind: RequestKind::Interrupt,
            detail: 2
        }]
    );
    assert!(shown(&a).ends_with(b"^C"));
    assert!(!t.discipline.readable(), "the line was flushed");
    let a = t.keys(b"\x1c");
    assert!(a.contains(&Action::Signal {
        job: 100,
        kind: RequestKind::Quit,
        detail: 3
    }));
    assert_eq!(t.counts.interrupts, 2);
}

#[test]
fn suspend_goes_to_the_broker_not_to_the_job() {
    let mut t = session();
    let a = t.keys(b"\x1a");
    assert!(a.contains(&Action::Suspend {
        group: 2,
        signal: 20
    }));
    assert!(!a.iter().any(|a| matches!(a, Action::Signal { .. })));
}

#[test]
fn signal_characters_with_no_foreground_send_nothing() {
    let mut t: Terminal<H> = Terminal::new();
    t.add_terminal(2);
    let a = t.keys(b"\x03\x1a\x1c");
    assert!(
        a.iter()
            .all(|a| matches!(a, Action::Show(_) | Action::Reply { .. }))
    );
}

#[test]
fn a_background_read_is_refused_and_consumes_nothing() {
    // INV-0105-3, the service's half.
    let mut t = session();
    let background = t.add_terminal(7).unwrap();
    t.keys(b"line\n");
    let a = t.request(background, &message(&Read { max: 100 }, 9), vec![]);
    let r = replies(&a);
    assert_eq!(read_reply(&r[0].1), Err(status::NOT_FOREGROUND));
    assert_eq!(t.counts.not_foreground, 1);
    let a = t.request(1, &message(&Read { max: 100 }, 10), vec![]);
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"line\n");
}

#[test]
fn only_the_control_connection_sets_the_foreground() {
    // INV-0105-2, the service's half.
    let mut t = session();
    let a = t.request(1, &message(&Foreground { group: 1 }, 3), vec![55]);
    assert!(a.contains(&Action::Close(55)), "the job handle is not kept");
    assert_eq!(reply_status(&replies(&a)[0].1), status::NOT_ALLOWED);
    assert_eq!(t.foreground(), Some(2));
    let a = t.request(1, &message(&Session {}, 3), vec![56]);
    assert!(a.contains(&Action::Close(56)));
    let a = t.request(1, &message(&ClearForeground {}, 3), vec![]);
    assert_eq!(reply_status(&replies(&a)[0].1), status::NOT_ALLOWED);
    // From the broker, the old job handle is closed.
    let a = t.request(0, &message(&Foreground { group: 4 }, 3), vec![101]);
    assert!(a.contains(&Action::Close(100)));
    assert_eq!(t.foreground(), Some(4));
}

#[test]
fn a_held_read_of_a_group_moved_to_the_background_is_answered_not_foreground() {
    let mut t = session();
    let a = t.request(1, &message(&Read { max: 10 }, 4), vec![]);
    assert!(replies(&a).is_empty());
    let a = t.request(0, &message(&Foreground { group: 9 }, 5), vec![101]);
    let r = replies(&a);
    assert_eq!(r.len(), 2);
    assert_eq!(r[1].0, 1);
    assert_eq!(read_reply(&r[1].1), Err(status::NOT_FOREGROUND));
}

#[test]
fn a_second_read_while_one_is_held_is_busy() {
    let mut t = session();
    t.request(1, &message(&Read { max: 10 }, 4), vec![]);
    let a = t.request(1, &message(&Read { max: 10 }, 5), vec![]);
    assert_eq!(read_reply(&replies(&a)[0].1), Err(status::BUSY));
    // The first is still answered.
    let a = t.keys(b"x\n");
    let r = replies(&a);
    assert_eq!(Header::decode(&r[0].1).unwrap().0.transaction, 4);
}

#[test]
fn clone_makes_a_connection_with_the_declared_group() {
    let mut t = session();
    let a = t.request(1, &message(&Clone { group: 6 }, 2), vec![77]);
    let id = a
        .iter()
        .find_map(|a| match a {
            Action::Watch { id, end: 77 } => Some(*id),
            _ => None,
        })
        .unwrap();
    assert_eq!(t.group_of(id), Some(6));
    assert_eq!(t.connections(), 3);
    t.closed(id);
    assert_eq!(t.connections(), 2);
    // A message on a closed connection closes its handles and says nothing.
    let a = t.request(id, &message(&Clone { group: 6 }, 2), vec![78]);
    assert_eq!(a, [Action::Close(78)]);
}

#[test]
fn attributes_round_trip_and_raw_mode_reads_what_there_is() {
    let mut t = session();
    let a = t.request(1, &message(&GetAttr {}, 2), vec![]);
    let bytes = &replies(&a)[0].1;
    let (_, attr) = decode::<Attr<'_>>(bytes, 0).unwrap();
    assert_ne!(attr.lflag & discipline::bits::ICANON, 0);
    let mut cc = [0u8; NCCS];
    cc.copy_from_slice(attr.cc.0);
    let raw = SetAttr {
        iflag: attr.iflag,
        oflag: attr.oflag,
        cflag: attr.cflag,
        lflag: attr.lflag & !(discipline::bits::ICANON | discipline::bits::ECHO),
        cc: Data(&cc),
    };
    let a = t.request(1, &message(&raw, 3), vec![]);
    assert_eq!(reply_status(&replies(&a)[0].1), status::OK);
    let a = t.keys(b"q");
    assert!(shown(&a).is_empty(), "no echo");
    let a = t.request(1, &message(&Read { max: 10 }, 4), vec![]);
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"q");
}

#[test]
fn poll_is_answered_when_a_line_is_there() {
    let mut t = session();
    let a = t.request(1, &message(&PollReadable {}, 2), vec![]);
    assert!(replies(&a).is_empty());
    let a = t.keys(b"x\n");
    let r = replies(&a);
    assert_eq!(r.len(), 1);
    assert_eq!(reply_status(&r[0].1), status::OK);
}

#[test]
fn window_size_hangup_and_output() {
    let mut t = session();
    let a = t.resize(30, 100);
    assert!(a.contains(&Action::Signal {
        job: 100,
        kind: RequestKind::Custom(28),
        detail: 28
    }));
    let a = t.request(1, &message(&WindowSize {}, 2), vec![]);
    let (_, s) = decode::<WindowSizeReply>(&replies(&a)[0].1, 0).unwrap();
    assert_eq!((s.rows, s.columns), (30, 100));
    assert!(t.hangup().is_empty(), "no session job yet");
    t.request(0, &message(&Session {}, 2), vec![200]);
    assert_eq!(
        t.hangup(),
        [Action::Signal {
            job: 200,
            kind: RequestKind::Hangup,
            detail: 1
        }]
    );
    let mut out = Vec::new();
    t.output(b"a\nb", &mut out);
    assert_eq!(out, b"a\r\nb");
}

#[test]
fn malformed_and_unknown_messages_are_refused_and_never_panic() {
    let mut t = session();
    // No header: nothing to answer, the handles closed.
    assert_eq!(t.request(1, b"xx", vec![9]), [Action::Close(9)]);
    // The wrong number of handles.
    let a = t.request(1, &message(&Read { max: 1 }, 2), vec![9]);
    assert!(a.contains(&Action::Close(9)));
    assert_eq!(read_reply(&replies(&a)[0].1), Err(status::INVALID));
    let a = t.request(0, &message(&Foreground { group: 1 }, 2), vec![]);
    assert_eq!(reply_status(&replies(&a)[0].1), status::INVALID);
    // An ordinal nobody knows.
    let a = t.request(
        1,
        &message(&proto_unknown::Unknown { x: 1 }, NO_REPLY.wrapping_add(3)),
        vec![],
    );
    assert_eq!(reply_status(&replies(&a)[0].1), status::UNSUPPORTED);
    // Every damaged form of every request, on both kinds of connection.
    let valid = [
        message(&Read { max: 5 }, 1),
        message(&Foreground { group: 1 }, 1),
        message(&Clone { group: 1 }, 1),
        message(
            &SetAttr {
                iflag: 0,
                oflag: 0,
                cflag: 0,
                lflag: 0,
                cc: Data(&[0; NCCS]),
            },
            1,
        ),
    ];
    let t = core::cell::RefCell::new(t);
    for v in &valid {
        zoraya_protocol::testing::exercise(v, |b| {
            for id in [0, 1] {
                for handles in [vec![], vec![1], vec![1, 2]] {
                    let _ = t.borrow_mut().request(id, b, handles);
                }
            }
        });
    }
}

mod proto_unknown {
    zoraya_protocol::protocol! {
        message Unknown = 99, handles 0 { x: u32 }
    }
}

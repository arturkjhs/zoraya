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

// ---------------------------------------------------------------- the
// tests the mutation run asked for (TASK-21-review.md, "Mutation testing").

use discipline::bits::*;

/// Feeds `bytes`, and gives the echo and what the last byte returned.
fn feed(d: &mut Discipline, bytes: &[u8]) -> (Vec<u8>, Option<Signal>) {
    let mut echo = Vec::new();
    let mut last = None;
    for &b in bytes {
        last = d.input(b, &mut echo);
    }
    (echo, last)
}

fn with_lflag(lflag: u32) -> Discipline {
    let mut d = Discipline::new();
    d.attr.lflag = lflag;
    d
}

#[test]
fn the_counts_are_kept() {
    let mut t = session();
    t.request(1, &message(&Read { max: 100 }, 5), vec![]);
    t.keys(b"x\n");
    assert_eq!(t.counts.lines, 1);
    t.keys(b"\x1a");
    assert_eq!(t.counts.suspends, 1);
    t.request(1, &message(&Foreground { group: 1 }, 3), vec![55]);
    assert_eq!(t.counts.refused, 1);
    t.request(1, b"xx", vec![]);
    assert_eq!(t.counts.refused, 2);
    t.request(1, &message(&Read { max: 10 }, 6), vec![]);
    t.request(0, &message(&Foreground { group: 9 }, 7), vec![101]);
    assert_eq!(t.counts.not_foreground, 1);
    let a = t.request(0, &message(&Statistics {}, 8), vec![]);
    let bytes = &replies(&a)[0].1;
    assert_eq!(reply_status(bytes), status::OK);
    let (_, s) = decode::<StatisticsReply>(bytes, 0).unwrap();
    assert_eq!((s.lines, s.suspends, s.connections), (1, 1, 2));
}

#[test]
fn a_signal_is_listed_after_the_echo_of_its_character() {
    let mut t = session();
    assert_eq!(
        t.keys(b"\x03"),
        [
            Action::Show(b"^C".to_vec()),
            Action::Signal {
                job: 100,
                kind: RequestKind::Interrupt,
                detail: 2
            }
        ]
    );
    t.discipline.attr.lflag &= !ECHO;
    assert_eq!(
        t.keys(b"\x03"),
        [Action::Signal {
            job: 100,
            kind: RequestKind::Interrupt,
            detail: 2
        }]
    );
}

#[test]
fn the_pending_queue_holds_16_kib_and_drops_the_rest() {
    let mut d = with_lflag(0);
    feed(&mut d, &vec![b'a'; discipline::MAX_PENDING]);
    assert_eq!(d.dropped, 0);
    feed(&mut d, b"a");
    assert_eq!(d.dropped, 1);
    assert_eq!(d.read(20_000).unwrap().len(), discipline::MAX_PENDING);
}

#[test]
fn a_line_past_the_limit_is_dropped_and_counted() {
    let mut d = Discipline::new();
    feed(&mut d, &vec![b'a'; discipline::MAX_LINE]);
    feed(&mut d, b"b\n");
    assert_eq!(d.dropped, 1);
    let mut want = vec![b'a'; discipline::MAX_LINE];
    want.push(b'\n');
    assert_eq!(d.read(10_000).unwrap(), want);
}

#[test]
fn carriage_returns_follow_igncr_icrnl_and_inlcr() {
    let mut d = Discipline::new();
    d.attr.iflag = IGNCR;
    feed(&mut d, b"a\rb\n");
    assert_eq!(d.read(100).unwrap(), b"ab\n");
    let mut d = Discipline::new();
    d.attr.iflag = 0;
    feed(&mut d, b"a\rb\n");
    assert_eq!(d.read(100).unwrap(), b"a\rb\n");
    let mut d = Discipline::new();
    d.attr.iflag = INLCR;
    feed(&mut d, b"a\n");
    assert!(
        d.read(100).is_none(),
        "the newline became a carriage return"
    );
}

#[test]
fn signal_characters_are_ordinary_without_isig() {
    let mut d = with_lflag(ICANON | ECHO);
    assert_eq!(feed(&mut d, b"\x03").1, None);
    feed(&mut d, b"\n");
    assert_eq!(d.read(100).unwrap(), b"\x03\n");
}

#[test]
fn a_signal_flushes_completed_lines_and_the_partial_one_unless_noflsh() {
    let mut d = Discipline::new();
    feed(&mut d, b"done\nab\x03");
    assert!(!d.readable());
    feed(&mut d, b"\n");
    assert_eq!(d.read(100).unwrap(), b"\n");
    let mut d = Discipline::new();
    d.attr.lflag |= NOFLSH;
    feed(&mut d, b"done\n\x03");
    assert!(d.readable());
}

#[test]
fn kill_echoes_by_echoke_and_echok() {
    // Default: ECHOKE, each character erased.
    let mut d = Discipline::new();
    assert_eq!(feed(&mut d, b"ab\x15").0, b"ab\x08 \x08\x08 \x08");
    // ECHOK without ECHOKE: a newline.
    let mut d = with_lflag(ICANON | ECHO | ECHOE | ECHOK);
    assert_eq!(feed(&mut d, b"ab\x15").0, b"ab\r\n");
    // Neither: nothing after the characters.
    let mut d = with_lflag(ICANON | ECHO | ECHOE);
    assert_eq!(feed(&mut d, b"ab\x15").0, b"ab");
}

#[test]
fn a_nul_is_an_ordinary_byte() {
    let mut d = Discipline::new();
    feed(&mut d, b"a\0b\n");
    assert_eq!(d.read(100).unwrap(), b"a\0b\n");
}

#[test]
fn echonl_echoes_a_newline_without_echo() {
    let mut d = with_lflag(ICANON | ECHONL);
    assert_eq!(feed(&mut d, b"a\n").0, b"\r\n");
    let mut d = with_lflag(ICANON | ISIG);
    assert!(feed(&mut d, b"a\n").0.is_empty());
}

#[test]
fn erase_echoes_by_echo_echoe_and_echoctl() {
    assert_eq!(
        feed(&mut with_lflag(ICANON | ECHO | ISIG), b"ab\x7f").0,
        b"ab"
    );
    assert!(
        feed(&mut with_lflag(ICANON | ECHOE | ISIG), b"ab\x7f")
            .0
            .is_empty()
    );
    let mut d = Discipline::new();
    assert_eq!(feed(&mut d, b"\x01\x7f").0, b"^A\x08 \x08\x08 \x08");
    let mut d = Discipline::new();
    assert_eq!(feed(&mut d, b" \x7f").0, b" \x08 \x08");
    let mut d = with_lflag(ICANON | ECHO | ECHOE);
    assert_eq!(feed(&mut d, b"\x01\x7f").0, b"\x01\x08 \x08");
}

#[test]
fn raw_echo_shows_control_bytes_in_caret_form_except_tab_and_newline() {
    let mut d = with_lflag(ISIG | ECHO | ECHOCTL);
    assert_eq!(feed(&mut d, b"\x01\t\n\x7f").0, b"^A\t\r\n\x7f");
}

#[test]
fn signal_characters_echo_by_echoctl() {
    let mut d = with_lflag(ISIG | ICANON | ECHO);
    assert_eq!(
        feed(&mut d, b"\x03"),
        (b"\x03".to_vec(), Some(Signal::Interrupt))
    );
    for (c, lflag, want) in [
        (b' ', Termios::default().lflag, &b" "[..]),
        (b'a', Termios::default().lflag, &b"a"[..]),
        (0x7f, ISIG | ICANON | ECHO, &b"\x7f"[..]),
        (0x7f, Termios::default().lflag, &b"^?"[..]),
    ] {
        let mut d = Discipline::new();
        d.attr.lflag = lflag;
        d.attr.cc[VINTR] = c;
        assert_eq!(
            feed(&mut d, &[c]),
            (want.to_vec(), Some(Signal::Interrupt)),
            "{c:#x}"
        );
    }
}

#[test]
fn output_applies_onlcr_only_under_opost() {
    for (oflag, want) in [
        (0, &b"a\n"[..]),
        (OPOST, b"a\n"),
        (ONLCR, b"a\n"),
        (OPOST | ONLCR, b"a\r\n"),
    ] {
        let mut d = Discipline::new();
        d.attr.oflag = oflag;
        let mut out = Vec::new();
        d.output(b"a\n", &mut out);
        assert_eq!(out, want, "oflag {oflag:#o}");
    }
}

#[test]
fn attributes_change_the_mode_without_losing_input() {
    // Leaving canonical mode makes the partial line readable.
    let mut d = Discipline::new();
    feed(&mut d, b"ab");
    let mut raw = d.attr;
    raw.lflag &= !ICANON;
    d.set(raw);
    assert_eq!(d.read(100).unwrap(), b"ab");
    // Canonical to canonical keeps the completed lines.
    let mut d = Discipline::new();
    feed(&mut d, b"x\nab");
    let mut quiet = d.attr;
    quiet.lflag &= !ECHO;
    d.set(quiet);
    assert!(d.readable());
    assert_eq!(d.read(100).unwrap(), b"x\n");
    // Raw to raw keeps what is queued.
    let mut d = with_lflag(0);
    feed(&mut d, b"q");
    let mut a = d.attr;
    a.lflag = ISIG;
    d.set(a);
    assert_eq!(d.read(100).unwrap(), b"q");
    // Raw to canonical makes what is queued the line.
    let mut d = with_lflag(0);
    feed(&mut d, b"q");
    d.set(Termios::default());
    feed(&mut d, b"\n");
    assert_eq!(d.read(100).unwrap(), b"q\n");
}

// ---- Part 10c: VMIN and VTIME, flush, drain, lines (ADR-0331 items 4 and 5).

use discipline::bits::{ECHO, ICANON, VMIN, VTIME};

/// The session with `ICANON` and `ECHO` off and the given `VMIN` and
/// `VTIME` (tenths of a second).
fn raw(vmin: u8, vtime: u8) -> Terminal<H> {
    let mut t = session();
    let mut a = t.discipline.attr;
    a.lflag &= !(ICANON | ECHO);
    a.cc[VMIN] = vmin;
    a.cc[VTIME] = vtime;
    let m = SetAttr {
        iflag: a.iflag,
        oflag: a.oflag,
        cflag: a.cflag,
        lflag: a.lflag,
        cc: Data(&a.cc),
    };
    let r = t.request(1, &message(&m, 2), vec![]);
    assert_eq!(reply_status(&replies(&r)[0].1), status::OK);
    t
}

fn read(t: &mut Terminal<H>, max: u32, tr: u32) -> Vec<Action<H>> {
    t.request(1, &message(&Read { max }, tr), vec![])
}

#[test]
fn vmin_0_vtime_0_returns_at_once_with_what_there_is() {
    // POSIX case 3 (INV-0331-3).
    let mut t = raw(0, 0);
    let a = read(&mut t, 10, 3);
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"");
    t.keys(b"ab");
    let a = read(&mut t, 10, 4);
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"ab");
    assert_eq!(t.deadline(), None);
}

#[test]
fn vmin_n_vtime_0_waits_for_n_bytes() {
    // POSIX case 1.
    let mut t = raw(3, 0);
    t.set_time(0);
    assert!(replies(&read(&mut t, 10, 3)).is_empty());
    assert!(replies(&t.keys(b"ab")).is_empty());
    assert_eq!(t.deadline(), None, "no timer without VTIME");
    assert!(replies(&t.tick(1_000_000)).is_empty());
    let a = t.keys(b"c");
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"abc");
    // A read of fewer bytes than VMIN returns when that many are there.
    t.keys(b"xy");
    let a = read(&mut t, 2, 4);
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"xy");
}

#[test]
fn vmin_n_vtime_t_is_an_interbyte_timer_started_by_the_first_byte() {
    // POSIX case 2: no timer before a byte; each byte restarts it.
    let mut t = raw(3, 2); // 200 ms
    t.set_time(1_000);
    assert!(replies(&read(&mut t, 10, 3)).is_empty());
    assert_eq!(t.deadline(), None, "blocks for ever with no byte");
    assert!(replies(&t.tick(50_000)).is_empty());
    t.set_time(60_000);
    assert!(replies(&t.keys(b"a")).is_empty());
    assert_eq!(t.deadline(), Some(60_200));
    t.set_time(60_150);
    assert!(replies(&t.keys(b"b")).is_empty());
    assert_eq!(t.deadline(), Some(60_350), "the second byte restarted it");
    assert!(replies(&t.tick(60_349)).is_empty());
    let a = t.tick(60_350);
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"ab");
    // VMIN bytes before the timer ends: at once.
    t.set_time(70_000);
    assert!(replies(&read(&mut t, 10, 4)).is_empty());
    let a = t.keys(b"xyz");
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"xyz");
}

#[test]
fn vmin_0_vtime_t_is_a_timeout_from_the_start_of_the_read() {
    // POSIX case 4: the first byte or the time, whichever comes first.
    let mut t = raw(0, 5); // 500 ms
    t.set_time(1_000);
    assert!(replies(&read(&mut t, 10, 3)).is_empty());
    assert_eq!(t.deadline(), Some(1_500));
    assert!(replies(&t.tick(1_499)).is_empty());
    let a = t.tick(1_500);
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"", "0 bytes");
    assert_eq!(t.deadline(), None);
    t.set_time(2_000);
    assert!(replies(&read(&mut t, 10, 4)).is_empty());
    t.set_time(2_100);
    let a = t.keys(b"k");
    assert_eq!(
        read_reply(&replies(&a)[0].1).unwrap(),
        b"k",
        "one byte ends it"
    );
}

#[test]
fn poll_is_ready_only_when_a_read_would_not_block() {
    // INV-0331-3: VMIN 2 with one byte is not ready; the poll is idempotent
    // (a second poll after an answer is held afresh).
    let mut t = raw(2, 0);
    assert!(replies(&t.request(1, &message(&PollReadable {}, 3), vec![])).is_empty());
    assert!(replies(&t.keys(b"a")).is_empty(), "one byte is below VMIN");
    let a = t.keys(b"b");
    assert_eq!(replies(&a).len(), 1);
    let a = t.request(1, &message(&PollReadable {}, 4), vec![]);
    assert_eq!(replies(&a).len(), 1, "still readable: answered at once");
    // VMIN 0 and VTIME 0: always ready.
    let mut t = raw(0, 0);
    let a = t.request(1, &message(&PollReadable {}, 3), vec![]);
    assert_eq!(replies(&a).len(), 1);
}

#[test]
fn flush_drops_unread_input_and_drain_waits_for_the_output() {
    let mut t = raw(1, 0);
    t.keys(b"abc");
    let a = t.request(1, &message(&Flush { queue: 0 }, 3), vec![]);
    assert_eq!(reply_status(&replies(&a)[0].1), status::OK);
    assert!(replies(&read(&mut t, 10, 4)).is_empty(), "nothing left");
    let a = t.request(1, &message(&Flush { queue: 1 }, 5), vec![]);
    assert_eq!(reply_status(&replies(&a)[0].1), status::INVALID);
    // Drain: at once with nothing on its way, held with output pending.
    let a = t.request(1, &message(&Drain {}, 6), vec![]);
    assert_eq!(replies(&a).len(), 1);
    t.set_output_pending(true);
    let a = t.request(1, &message(&Drain {}, 7), vec![]);
    assert!(replies(&a).is_empty());
    let a = t.request(1, &message(&Drain {}, 8), vec![]);
    assert_eq!(reply_status(&replies(&a)[0].1), status::BUSY);
    let a = t.set_output_pending(false);
    let r = replies(&a);
    assert_eq!(r.len(), 1);
    assert_eq!(Header::decode(&r[0].1).unwrap().0.transaction, 7);
}

/// Two lines: line 1 in front with connection 1 (group 2), line 2's program
/// on connection 2 declaring the front line's group.
fn two_lines() -> (Terminal<H>, Id) {
    let mut t = Terminal::new();
    t.add_control();
    let first = t.add_terminal_in(2, 1).unwrap();
    assert_eq!(first, 1);
    let other = t.add_terminal_in(2, 2).unwrap();
    t.set_foreground(2, 100);
    t.set_front_line(Some(1));
    (t, other)
}

#[test]
fn a_connection_of_another_line_cannot_read_poll_or_change_modes() {
    // INV-0331-2, the discipline's half: the declared group is the front
    // line's, and every request is still refused and changes nothing.
    let (mut t, other) = two_lines();
    t.keys(b"secret\n");
    let a = t.request(other, &message(&Read { max: 100 }, 3), vec![]);
    assert_eq!(read_reply(&replies(&a)[0].1), Err(status::NOT_FOREGROUND));
    let a = t.request(other, &message(&PollReadable {}, 4), vec![]);
    assert_eq!(reply_status(&replies(&a)[0].1), status::IO);
    let before = t.discipline.attr;
    let mut a = before;
    a.lflag &= !(ICANON | ECHO);
    let m = SetAttr {
        iflag: a.iflag,
        oflag: a.oflag,
        cflag: a.cflag,
        lflag: a.lflag,
        cc: Data(&a.cc),
    };
    let r = t.request(other, &message(&m, 5), vec![]);
    assert_eq!(reply_status(&replies(&r)[0].1), status::IO);
    assert_eq!(t.discipline.attr, before, "the modes are unchanged");
    let r = t.request(other, &message(&Flush { queue: 0 }, 6), vec![]);
    assert_eq!(reply_status(&replies(&r)[0].1), status::IO);
    // Nothing was consumed: the front line reads its line.
    let a = t.request(1, &message(&Read { max: 100 }, 7), vec![]);
    assert_eq!(read_reply(&replies(&a)[0].1).unwrap(), b"secret\n");
    // GetAttr and the size are answered for any connection.
    let r = t.request(other, &message(&GetAttr {}, 8), vec![]);
    assert!(decode::<Attr<'_>>(&replies(&r)[0].1, 0).is_ok());
    let r = t.request(other, &message(&WindowSize {}, 9), vec![]);
    assert!(decode::<WindowSizeReply>(&replies(&r)[0].1, 0).is_ok());
}

#[test]
fn a_clone_carries_its_parents_line_whatever_group_it_declares() {
    let (mut t, other) = two_lines();
    let a = t.request(other, &message(&Clone { group: 2 }, 3), vec![77]);
    let id = a
        .iter()
        .find_map(|a| match a {
            Action::Watch { id, end: 77 } => Some(*id),
            _ => None,
        })
        .unwrap();
    let r = t.request(id, &message(&Read { max: 10 }, 4), vec![]);
    assert_eq!(read_reply(&replies(&r)[0].1), Err(status::NOT_FOREGROUND));
    // A clone of the front line's connection is of the front line.
    let a = t.request(1, &message(&Clone { group: 2 }, 5), vec![78]);
    let mine = a
        .iter()
        .find_map(|a| match a {
            Action::Watch { id, end: 78 } => Some(*id),
            _ => None,
        })
        .unwrap();
    t.keys(b"x\n");
    let r = t.request(mine, &message(&Read { max: 10 }, 6), vec![]);
    assert_eq!(read_reply(&replies(&r)[0].1).unwrap(), b"x\n");
}

#[test]
fn a_held_read_goes_when_its_line_goes_to_the_back() {
    let (mut t, _) = two_lines();
    assert!(replies(&t.request(1, &message(&Read { max: 10 }, 3), vec![])).is_empty());
    let a = t.set_front_line(Some(2));
    let r = replies(&a);
    assert_eq!(r.len(), 1);
    assert_eq!(read_reply(&r[0].1), Err(status::NOT_FOREGROUND));
    // With no line in front nobody reads.
    let a = t.set_front_line(None);
    assert!(replies(&a).is_empty());
    t.keys(b"k\n");
    let r = t.request(1, &message(&Read { max: 10 }, 4), vec![]);
    assert_eq!(read_reply(&replies(&r)[0].1), Err(status::NOT_FOREGROUND));
}

#[test]
fn the_end_of_a_line_restores_the_defaults_and_fg_gives_the_attributes_back() {
    // INV-0331-4, the discipline's half.
    let mut t = raw(5, 3);
    t.keys(b"zz");
    let (was, _) = t.end_line();
    assert_eq!(t.discipline.attr, Termios::default());
    assert_ne!(was, Termios::default());
    assert!(replies(&read(&mut t, 10, 3)).is_empty(), "input dropped");
    t.continue_line(was);
    assert_eq!(t.discipline.attr, was);
}

// ---- Holes the mutation run of part 10c found.

#[test]
fn raw_is_cfmakeraw_and_nothing_else() {
    use discipline::bits::*;
    let d = Termios::default();
    let r = Termios::raw();
    assert_eq!(r.iflag, d.iflag & !(ICRNL | INLCR | IGNCR | IXON));
    assert_eq!(r.iflag & IUTF8, IUTF8, "UTF-8 input mode stays");
    assert_eq!(r.oflag, d.oflag & !OPOST, "ONLCR stays but OPOST is off");
    assert_eq!(r.lflag & (ISIG | ICANON | ECHO | ECHONL | IEXTEN), 0);
    assert_eq!(r.lflag, d.lflag & !(ISIG | ICANON | ECHO | ECHONL | IEXTEN));
    assert_ne!(r.lflag, 0, "the other local flags are kept");
    assert_eq!((r.cflag, r.cc), (d.cflag, d.cc));
    assert_ne!(r, d);
    // Each flag it clears is set in the default, so each clear matters.
    for f in [ISIG, ICANON, ECHO, IEXTEN] {
        assert_ne!(d.lflag & f, 0);
    }
    for f in [ICRNL, IXON] {
        assert_ne!(d.iflag & f, 0);
    }
    assert_ne!(d.oflag & OPOST, 0);
}

#[test]
fn set_attr_request_is_the_message_a_client_sends() {
    let mut t = session();
    let raw = Termios::raw();
    let bytes = set_attr_request(&raw, 77);
    assert!(!bytes.is_empty());
    let (h, _) = Header::decode(&bytes).unwrap();
    assert_eq!((h.ordinal, h.transaction), (SetAttr::ORDINAL, 77));
    let a = t.request(1, &bytes, vec![]);
    assert_eq!(reply_status(&replies(&a)[0].1), status::OK);
    assert_eq!(t.discipline.attr, raw);
}

#[test]
fn the_host_calls_move_the_front_line_and_the_foreground() {
    let mut t: Terminal<H> = Terminal::new();
    assert_eq!(t.front_line(), None);
    let c = t.add_terminal_in(5, 3).unwrap();
    t.set_front_line(Some(3));
    assert_eq!(t.front_line(), Some(3));
    t.set_front_line(Some(0));
    assert_eq!(t.front_line(), Some(0));
    t.set_front_line(Some(3));
    // A read held with no foreground group is answered when one is set, and
    // asking again after the group changed to another is refused.
    t.keys(b"a\n");
    assert!(!replies(&t.request(c, &message(&Read { max: 9 }, 1), vec![])).is_empty());
    t.request(c, &message(&Read { max: 9 }, 2), vec![]);
    let a = t.set_foreground(9, 100);
    let r = replies(&a);
    assert_eq!(r.len(), 1, "the held read of group 5 is told NotForeground");
    assert_eq!(read_reply(&r[0].1), Err(status::NOT_FOREGROUND));
    assert_eq!(t.foreground(), Some(9));
    t.clear_foreground_group();
    assert_eq!(t.foreground(), None);
    // With no foreground group the signal characters send nothing.
    assert!(
        !t.keys(&[0x03])
            .iter()
            .any(|a| matches!(a, Action::Signal { .. }))
    );
}

#[test]
fn read_ready_and_deadline_in_canonical_mode_follow_lines_only() {
    let mut d = Discipline::new();
    d.attr.cc[discipline::bits::VMIN] = 0;
    d.attr.cc[discipline::bits::VTIME] = 5;
    d.set_now(10);
    assert!(!d.read_ready(10, 0), "no line yet");
    assert_eq!(d.read_deadline(10, 0), None, "VTIME is for raw mode only");
    let mut echo = Vec::new();
    for &b in b"x\n" {
        d.input(b, &mut echo);
    }
    assert!(d.read_ready(10, 0));
    assert_eq!(d.read_deadline(10, 0), None);
}

#[test]
fn the_whole_line_in_front_reads_until_a_group_is_chosen() {
    // `zosh`: a pipeline's programs are in groups of their own; all of the
    // line in front read until a `tcsetpgrp` chooses one (ADR-0331 item 6).
    let mut t: Terminal<H> = Terminal::new();
    let a = t.add_terminal_in(5, 3).unwrap();
    let b = t.add_terminal_in(6, 3).unwrap();
    let other = t.add_terminal_in(5, 4).unwrap();
    t.set_front_line(Some(3));
    t.set_foreground_line(100);
    assert_eq!(t.foreground(), None, "no group is chosen");
    t.keys(b"x\ny\n");
    let r = replies(&t.request(a, &message(&Read { max: 9 }, 1), vec![]));
    assert_eq!(read_reply(&r[0].1).unwrap(), b"x\n");
    let r = replies(&t.request(b, &message(&Read { max: 9 }, 2), vec![]));
    assert_eq!(read_reply(&r[0].1).unwrap(), b"y\n");
    let r = replies(&t.request(other, &message(&Read { max: 9 }, 3), vec![]));
    assert_eq!(read_reply(&r[0].1), Err(status::NOT_FOREGROUND));
    // The signal characters go to the line's job.
    let s: Vec<_> = t
        .keys(&[0x03])
        .into_iter()
        .filter(|a| matches!(a, Action::Signal { .. }))
        .collect();
    assert_eq!(
        s,
        [Action::Signal {
            job: 100,
            kind: RequestKind::Interrupt,
            detail: 2
        }]
    );
    // A chosen group ends it: the other group of the line is refused.
    t.request(b, &message(&Read { max: 9 }, 4), vec![]);
    let r = replies(&t.set_foreground(5, 100));
    assert_eq!(
        read_reply(&r[0].1),
        Err(status::NOT_FOREGROUND),
        "b's held read"
    );
    assert_eq!(t.foreground(), Some(5));
    t.set_foreground_line(100);
    t.clear_foreground_group();
    t.keys(b"z\n");
    let r = replies(&t.request(b, &message(&Read { max: 9 }, 5), vec![]));
    assert_eq!(r.len(), 1, "no foreground group: anyone of the line reads");
    assert!(
        !t.keys(&[0x03])
            .iter()
            .any(|a| matches!(a, Action::Signal { .. })),
        "and no signal goes anywhere"
    );
}

#[test]
fn one_line_cannot_take_the_whole_table_of_connections() {
    // The review of 10c, finding 5: a background program that clones in a
    // loop is refused past its line's share; another line still connects.
    let (mut t, other) = two_lines();
    let mut made = 1; // `other` itself
    for i in 0..MAX_CONNECTIONS as u32 {
        let a = t.request(other, &message(&Clone { group: 2 }, 10 + i), vec![1000 + i]);
        if a.iter().any(|a| matches!(a, Action::Watch { .. })) {
            made += 1;
        } else {
            assert!(a.contains(&Action::Close(1000 + i)), "the end is closed");
            assert_eq!(
                crate::reply_status(&replies(&a)[0].1),
                status::NO_SPACE,
                "refused for want of room"
            );
        }
    }
    assert_eq!(made, MAX_LINE_CONNECTIONS);
    assert!(t.add_terminal_in(7, 3).is_some(), "line 3 still connects");
    let a = t.request(1, &message(&Clone { group: 2 }, 9), vec![99]);
    assert!(a.iter().any(|a| matches!(a, Action::Watch { end: 99, .. })));
    // A host that does not tell lines apart (ttyd) has the table's limit only.
    let mut s: Terminal<H> = Terminal::new();
    for _ in 0..MAX_LINE_CONNECTIONS + 1 {
        assert!(s.add_terminal_in(2, 0).is_some());
    }
}

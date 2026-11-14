//! `poll` and `ppoll` over the fake kernel (ADR-0342 item 5, ADR-0414 item 1;
//! INV-0342-2, INV-0414-1): the readiness `Posix::poll_once` reports for each
//! kind of descriptor, `POLLNVAL`, the wait with a deadline and the same wait
//! under ADR-0315's deferred protocol. Sockets are in `net.rs`.

mod kernel;

use kernel::*;
use std::cell::Cell;
use std::rc::Rc;
use zoraya_posix::errno::*;
use zoraya_posix::poll::*;
use zoraya_posix::process::{self, AT_FDCWD, o};
use zoraya_posix::sys::Sys;
use zoraya_protocol::Message;

zoraya_test_alloc::cap_memory!();

fn pf(fd: i32, events: i16) -> PollFd {
    PollFd {
        fd,
        events,
        revents: 0,
    }
}

/// One call as the C boundary makes it: look, and while the answer is "look
/// again" look again. `Some(n)` with `fds` filled in.
fn poll(r: &mut Rig, fds: &mut [PollFd], deadline: Option<u64>) -> Result<i32, i32> {
    loop {
        if let Some(n) = r.p.poll_once(fds, deadline)? {
            return Ok(n);
        }
    }
}

/// A look that never waits: a deadline that has passed.
fn look(r: &mut Rig, fds: &mut [PollFd]) -> i32 {
    poll(r, fds, Some(0)).unwrap()
}

fn read_end(r: &Rig, fd: i32) -> H {
    match r.p.fds.get(fd) {
        Some(process::Desc::Stream { read: Some(h), .. }) => *h,
        _ => panic!("fd {fd} is no read end"),
    }
}

fn revents(r: &mut Rig, fd: i32, events: i16) -> i16 {
    let mut f = [pf(fd, events)];
    look(r, &mut f);
    f[0].revents
}

#[test]
fn an_empty_pipe_is_writable_and_not_readable() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(0).unwrap();
    let mut f = [pf(rd, POLLIN), pf(wr, POLLOUT)];
    assert_eq!(look(&mut r, &mut f), 1);
    assert_eq!((f[0].revents, f[1].revents), (0, POLLOUT));
    // Only what was asked for: the "norm" bits are separate requests.
    assert_eq!(revents(&mut r, wr, POLLWRNORM), POLLWRNORM);
    assert_eq!(
        revents(&mut r, wr, POLLOUT | POLLWRNORM),
        POLLOUT | POLLWRNORM
    );
    // POLLPRI is never ready.
    assert_eq!(revents(&mut r, rd, POLLPRI), 0);
    // Nothing asked for: nothing to say while the pipe is whole.
    assert_eq!(revents(&mut r, rd, 0), 0);
    assert_eq!(revents(&mut r, wr, 0), 0);
}

#[test]
fn a_pipe_with_bytes_is_readable_until_they_are_read() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(0).unwrap();
    r.write(wr, b"x").unwrap();
    assert_eq!(revents(&mut r, rd, POLLIN), POLLIN);
    assert_eq!(revents(&mut r, rd, POLLRDNORM), POLLRDNORM);
    assert_eq!(r.read(rd, 10).unwrap(), b"x");
    assert_eq!(revents(&mut r, rd, POLLIN), 0);
}

#[test]
fn a_closed_writer_is_a_hang_up_that_reads_as_end_of_file() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(0).unwrap();
    r.write(wr, b"tail").unwrap();
    r.p.close(wr).unwrap();
    // The bytes first, and the hang-up with them.
    assert_eq!(revents(&mut r, rd, POLLIN), POLLIN | POLLHUP);
    assert_eq!(r.read(rd, 10).unwrap(), b"tail");
    // End of file: a read does not block, so POLLIN (INV-0414-1).
    assert_eq!(revents(&mut r, rd, POLLIN), POLLIN | POLLHUP);
    assert_eq!(r.read(rd, 10).unwrap(), b"");
    // A hang-up is reported when nothing is asked for.
    assert_eq!(revents(&mut r, rd, 0), POLLHUP);
    // No write end: ready at once, as select has it (a write is EBADF).
    assert_eq!(revents(&mut r, rd, POLLOUT), POLLOUT);
}

#[test]
fn a_closed_reader_is_an_error_on_the_write_end() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(0).unwrap();
    r.p.close(rd).unwrap();
    // A write fails with EPIPE at once, so it does not block: POLLOUT too.
    assert_eq!(revents(&mut r, wr, POLLOUT), POLLOUT | POLLERR);
    assert_eq!(revents(&mut r, wr, 0), POLLERR);
    assert_eq!(r.write(wr, b"x"), Err(EPIPE));
}

#[test]
fn a_full_pipe_is_not_writable() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(o::NONBLOCK).unwrap();
    let big = vec![1u8; STREAM_CAPACITY];
    assert_eq!(r.write(wr, &big), Ok(STREAM_CAPACITY));
    assert_eq!(revents(&mut r, wr, POLLOUT), 0);
    assert_eq!(r.write(wr, b"x"), Err(EAGAIN), "what POLLOUT promised");
    r.p.sys.net.borrow_mut().drain(read_end(&r, rd), 10_000);
    assert_eq!(revents(&mut r, wr, POLLOUT), POLLOUT);
}

#[test]
fn a_direction_with_no_end_is_ready_at_once_as_select_has_it() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(0).unwrap();
    // A read of a write end fails with EBADF, it does not block.
    assert_eq!(revents(&mut r, wr, POLLIN), POLLIN);
    assert_eq!(revents(&mut r, rd, POLLOUT), POLLOUT);
}

#[test]
fn files_directories_and_dev_null_are_ready_at_once() {
    let mut r = rig();
    let file = r.p.open(AT_FDCWD, b"f", o::RDWR | o::CREAT, 0o644).unwrap();
    let dir = r.p.open(AT_FDCWD, b"/", o::RDONLY, 0).unwrap();
    let null = r.p.open(AT_FDCWD, b"/dev/null", o::RDWR, 0).unwrap();
    for fd in [file, dir, null] {
        assert_eq!(revents(&mut r, fd, POLLIN | POLLOUT), POLLIN | POLLOUT);
        assert_eq!(revents(&mut r, fd, POLLIN), POLLIN);
        assert_eq!(revents(&mut r, fd, POLLOUT), POLLOUT);
        assert_eq!(revents(&mut r, fd, 0), 0);
        assert_eq!(revents(&mut r, fd, POLLPRI), 0);
    }
}

#[test]
fn a_descriptor_that_is_not_open_is_pollnval_and_a_negative_one_is_skipped() {
    let mut r = rig();
    let (rd, _wr) = r.p.pipe(0).unwrap();
    let mut f = [
        pf(77, POLLIN),
        pf(-1, POLLIN),
        pf(rd, POLLIN),
        pf(1 << 30, 0),
    ];
    f[1].revents = 0x55;
    // Both unopen entries count; the negative one has revents 0 whatever it
    // held, and the pipe nothing.
    assert_eq!(look(&mut r, &mut f), 2);
    assert_eq!(f.map(|x| x.revents), [POLLNVAL, 0, 0, POLLNVAL]);
    // A closed one too, and POLLNVAL is not a wait.
    let (a, b) = r.p.pipe(0).unwrap();
    r.p.close(a).unwrap();
    r.p.close(b).unwrap();
    let mut f = [pf(a, POLLIN)];
    assert_eq!(poll(&mut r, &mut f, None), Ok(1));
    assert_eq!(f[0].revents, POLLNVAL);
}

#[test]
fn nothing_ready_with_the_deadline_passed_is_zero() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(0).unwrap();
    r.p.sys.net.borrow_mut().now = 1000;
    let mut f = [pf(rd, POLLIN)];
    assert_eq!(poll(&mut r, &mut f, Some(1)), Ok(0));
    assert_eq!(f[0].revents, 0);
    // The deadline decides nothing when something is ready.
    r.write(wr, b"x").unwrap();
    assert_eq!(poll(&mut r, &mut f, Some(1)), Ok(1));
    assert_eq!(f[0].revents, POLLIN);
    // Stale revents are cleared by a look that finds nothing.
    r.read(rd, 1).unwrap();
    f[0].revents = POLLIN;
    assert_eq!(poll(&mut r, &mut f, Some(1)), Ok(0));
    assert_eq!(f[0].revents, 0);
}

#[test]
fn nothing_to_poll_waits_out_its_deadline() {
    let mut r = rig();
    // `poll(NULL, 0, ms)` as a sleep: no source but the signal queue's.
    let waits = r.p.sys.waits;
    assert_eq!(poll(&mut r, &mut [], Some(5_000_000)), Ok(0));
    assert!(r.p.sys.waits > waits, "it waited");
    let mut f = [pf(-3, POLLIN)];
    assert_eq!(poll(&mut r, &mut f, Some(5_000_000)), Ok(0));
}

#[test]
fn a_wait_ends_when_another_process_makes_a_descriptor_ready() {
    let mut r = rig();
    let (r1, _w1) = r.p.pipe(0).unwrap();
    let (r2, _w2) = r.p.pipe(0).unwrap();
    let (r3, _w3) = r.p.pipe(0).unwrap();
    let third = read_end(&r, r3);
    let rounds = Rc::new(Cell::new(0));
    let n = rounds.clone();
    r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| {
        n.set(n.get() + 1);
        if n.get() == 1 {
            net.feed(third, b"late");
        }
    }));
    let mut f = [pf(r1, POLLIN), pf(r2, POLLIN), pf(r3, POLLIN), pf(-1, 0)];
    assert_eq!(poll(&mut r, &mut f, None), Ok(1));
    assert_eq!(f.map(|x| x.revents), [0, 0, POLLIN, 0]);
    assert_eq!(rounds.get(), 1);
}

#[test]
fn a_wait_ends_when_the_writer_closes() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(0).unwrap();
    // The write end's descriptor closes while the wait runs: another process
    // holding it would; here the same table's entry, closed by the hook's
    // effect on the kernel object.
    let end = match r.p.fds.get(wr) {
        Some(process::Desc::Stream { write: Some(h), .. }) => *h,
        _ => panic!(),
    };
    r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| net.close(end)));
    let mut f = [pf(rd, POLLIN)];
    assert_eq!(poll(&mut r, &mut f, None), Ok(1));
    assert_eq!(f[0].revents, POLLIN | POLLHUP);
}

#[test]
fn a_thread_process_defers_the_wait_with_every_source_and_the_deadline() {
    let mut r = rig();
    r.p.defer_waits = true;
    let (r1, _w1) = r.p.pipe(0).unwrap();
    let (r2, w2) = r.p.pipe(0).unwrap();
    let (e1, e2) = (read_end(&r, r1), read_end(&r, r2));
    let mut f = [pf(r1, POLLIN), pf(r2, POLLIN), pf(w2, POLLOUT)];
    // A writable end is ready: no wait is asked for.
    r.p.set_resume(None);
    let out = r.p.poll_once(&mut f, Some(u64::MAX));
    assert_eq!(out, Ok(Some(1)));
    assert!(r.p.end_run(&out).is_none());
    assert_eq!(f.map(|x| x.revents), [0, 0, POLLOUT]);

    // Without the writable end the wait is deferred with both read ends.
    let mut f = [pf(r1, POLLIN), pf(r2, POLLIN)];
    r.p.set_resume(None);
    let out = r.p.poll_once(&mut f, Some(u64::MAX));
    assert_eq!(out, Err(zoraya_posix::errno::PENDING));
    let w = r.p.end_run(&out).expect("a wait");
    assert_eq!(w.own_len, 2);
    assert_eq!(w.deadline, Some(u64::MAX));
    assert_eq!(&w.sources[..2], &[(e1, 5), (e2, 5)]);
    // The outcome: the second source asserted READABLE; the next run reports it.
    r.p.sys.net.borrow_mut().feed(e2, b"z");
    let result = r.p.sys.wait_any(&w.sources, w.deadline);
    assert_eq!(result.map(|(i, _)| i), Ok(1));
    r.p.set_resume(Some(zoraya_posix::jobs::Resume {
        sources: w.sources,
        own_len: w.own_len,
        result,
        progress: 0,
    }));
    // The look that follows a wait finds it, without waiting again.
    let out = r.p.poll_once(&mut f, Some(u64::MAX));
    assert_eq!(out, Ok(Some(1)));
    assert!(r.p.end_run(&out).is_none());
    assert_eq!(f.map(|x| x.revents), [0, POLLIN]);
}

#[test]
fn the_arguments_of_a_hostile_caller_are_refused() {
    assert_eq!(array_len(0, true), Ok(0));
    assert_eq!(array_len(0, false), Ok(0));
    assert_eq!(array_len(1, false), Ok(1));
    assert_eq!(array_len(NFDS_MAX, false), Ok(NFDS_MAX as usize));
    assert_eq!(array_len(1, true), Err(EFAULT));
    assert_eq!(array_len(NFDS_MAX + 1, false), Err(EINVAL));
    assert_eq!(array_len(u64::MAX, false), Err(EINVAL));
    assert_eq!(array_len(u64::MAX, true), Err(EINVAL));
    assert_eq!(array_len(1 << 32, false), Err(EINVAL), "no truncation");

    assert_eq!(deadline_after_ms(100, -1), None);
    assert_eq!(deadline_after_ms(100, i32::MIN), None);
    assert_eq!(deadline_after_ms(100, 0), Some(100));
    assert_eq!(deadline_after_ms(100, 2), Some(2_000_100));
    assert_eq!(deadline_after_ms(u64::MAX - 5, i32::MAX), Some(u64::MAX));

    assert_eq!(deadline_after_timespec(10, 0, 0), Ok(10));
    assert_eq!(deadline_after_timespec(10, 1, 5), Ok(10 + 1_000_000_005));
    assert_eq!(
        deadline_after_timespec(10, 0, 999_999_999),
        Ok(10 + 999_999_999)
    );
    assert_eq!(deadline_after_timespec(10, -1, 0), Err(EINVAL));
    assert_eq!(deadline_after_timespec(10, 0, -1), Err(EINVAL));
    assert_eq!(deadline_after_timespec(10, 0, 1_000_000_000), Err(EINVAL));
    assert_eq!(deadline_after_timespec(10, i64::MAX, 0), Ok(u64::MAX));
    assert_eq!(deadline_after_timespec(u64::MAX, 1, 0), Ok(u64::MAX));
}

#[test]
fn the_layout_is_the_c_librarys() {
    assert_eq!(core::mem::size_of::<PollFd>(), 8);
    assert_eq!(core::mem::offset_of!(PollFd, fd), 0);
    assert_eq!(core::mem::offset_of!(PollFd, events), 4);
    assert_eq!(core::mem::offset_of!(PollFd, revents), 6);
    // musl's <poll.h> values on aarch64.
    assert_eq!(
        [
            POLLIN, POLLPRI, POLLOUT, POLLERR, POLLHUP, POLLNVAL, POLLRDNORM, POLLRDBAND
        ],
        [1, 2, 4, 8, 0x10, 0x20, 0x40, 0x80]
    );
    assert_eq!([POLLWRNORM, POLLWRBAND], [0x100, 0x200]);
}

#[test]
fn a_kernel_that_cannot_tell_signals_makes_every_requested_end_ready() {
    // As `select` falls back: ready, so that the call that follows decides.
    let mut r = rig();
    r.p.sys.signals_unknown = true;
    let (rd, wr) = r.p.pipe(0).unwrap();
    assert_eq!(revents(&mut r, rd, POLLIN), POLLIN);
    assert_eq!(revents(&mut r, wr, POLLOUT), POLLOUT);
    // No hang-up or error can be known.
    assert_eq!(revents(&mut r, rd, POLLIN | POLLPRI), POLLIN);
}

/// A terminal descriptor: its connection's server end and the stream's read
/// end (what the program writes).
struct Tty {
    fd: i32,
    server: H,
    _output: H,
}

fn tty(r: &mut Rig) -> Tty {
    let (conn, server) = r.p.sys.channel_create().unwrap();
    let (output, wr) = r.p.sys.stream_create().unwrap();
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
    Tty {
        fd,
        server,
        _output: output,
    }
}

/// What the service has been sent: `(ordinal, transaction)` of each message.
fn sent(r: &mut Rig, t: &Tty) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    while let Some((bytes, _)) = r.net.borrow_mut().take_message(t.server) {
        let (h, _) = zoraya_protocol::Header::decode(&bytes).unwrap();
        out.push((h.ordinal, h.transaction));
    }
    out
}

fn answer_poll(r: &mut Rig, t: &Tty, transaction: u32) {
    let mut buf = [0u8; 64];
    let m = zoraya_proto_tty::PollReadableReply {
        status: zoraya_protocol::Below(zoraya_proto_tty::status::OK),
    };
    let n = zoraya_protocol::encode(&m, transaction, &mut buf).unwrap();
    assert!(r.net.borrow_mut().send_message(t.server, &buf[..n], &[]));
}

#[test]
fn a_terminals_input_is_ready_when_the_service_says_a_read_would_not_block() {
    // ADR-0331 item 5, INV-0331-3 and INV-0342-2: not "at once" any more.
    let mut r = rig();
    let t = tty(&mut r);
    let ordinal = zoraya_proto_tty::PollReadable::ORDINAL;
    assert_eq!(revents(&mut r, t.fd, POLLIN), 0, "nothing typed");
    let first = sent(&mut r, &t);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, ordinal);
    // The same look again, as after a deferred wait, or a second call: the
    // request is outstanding, so no second one is sent (idempotent).
    for _ in 0..3 {
        assert_eq!(revents(&mut r, t.fd, POLLIN), 0);
    }
    assert!(sent(&mut r, &t).is_empty(), "no second PollReadable");
    answer_poll(&mut r, &t, first[0].1);
    assert_eq!(revents(&mut r, t.fd, POLLIN), POLLIN);
    // The answer was spent by that look: the next one asks afresh.
    assert_eq!(revents(&mut r, t.fd, POLLIN | POLLRDNORM), 0);
    assert_eq!(sent(&mut r, &t).len(), 1);
}

#[test]
fn a_terminals_answered_request_is_spent_by_the_look_that_took_it() {
    let mut r = rig();
    let t = tty(&mut r);
    assert_eq!(revents(&mut r, t.fd, POLLIN), 0);
    let first = sent(&mut r, &t);
    answer_poll(&mut r, &t, first[0].1);
    assert_eq!(revents(&mut r, t.fd, POLLIN), POLLIN);
    // Nothing more typed: the next look sends a new request and is not ready.
    assert_eq!(revents(&mut r, t.fd, POLLIN), 0);
    let second = sent(&mut r, &t);
    assert_eq!(second.len(), 1);
    assert_ne!(second[0].1, first[0].1);
}

#[test]
fn a_terminals_poll_waits_on_its_connection_and_a_timeout_ends_it() {
    let mut r = rig();
    let t = tty(&mut r);
    let mut f = [pf(t.fd, POLLIN)];
    assert_eq!(poll(&mut r, &mut f, Some(1_000_000)), Ok(0));
    assert_eq!(f[0].revents, 0);
    // The answer comes while the call waits: the wait ends on the
    // connection's READABLE, and the look after it reports the terminal.
    let first = sent(&mut r, &t);
    let server = t.server;
    let txn = first[0].1;
    let mut buf = [0u8; 64];
    let m = zoraya_proto_tty::PollReadableReply {
        status: zoraya_protocol::Below(zoraya_proto_tty::status::OK),
    };
    let n = zoraya_protocol::encode(&m, txn, &mut buf).unwrap();
    r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| {
        net.send_message(server, &buf[..n], &[]);
    }));
    assert_eq!(poll(&mut r, &mut f, None), Ok(1));
    assert_eq!(f[0].revents, POLLIN);
}

#[test]
fn a_terminal_that_is_gone_is_a_hang_up_and_its_output_is_writable() {
    let mut r = rig();
    let t = tty(&mut r);
    assert_eq!(revents(&mut r, t.fd, POLLOUT), POLLOUT);
    assert_eq!(revents(&mut r, t.fd, POLLIN), 0);
    r.p.sys.close(t.server);
    assert_eq!(revents(&mut r, t.fd, POLLIN), POLLIN | POLLHUP);
}

#[test]
fn a_terminals_unread_bytes_are_ready_without_a_request() {
    // A read that took 2 of the 5 bytes the service gave keeps 3: the
    // next look says ready and asks the service nothing.
    let mut r = rig();
    let t = tty(&mut r);
    let server = t.server;
    r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| {
        if let Some((bytes, _)) = net.take_message(server) {
            let (h, _) = zoraya_protocol::Header::decode(&bytes).unwrap();
            let mut buf = [0u8; 64];
            let m = zoraya_proto_tty::ReadReply {
                bytes: zoraya_protocol::Data(b"abcde"),
            };
            let n = zoraya_protocol::encode(&m, h.transaction, &mut buf).unwrap();
            net.send_message(server, &buf[..n], &[]);
        }
    }));
    let into = r.poke(0, &[0; 2]);
    assert_eq!(r.p.read(t.fd, into), Ok(2));
    r.p.sys.meanwhile = None;
    assert_eq!(r.peek(0, 2), b"ab");
    assert_eq!(revents(&mut r, t.fd, POLLIN), POLLIN);
    assert!(
        sent(&mut r, &t).is_empty(),
        "no PollReadable for kept bytes"
    );
    let into = r.poke(0, &[0; 8]);
    assert_eq!(r.p.read(t.fd, into), Ok(3));
}

#[test]
fn a_read_that_finds_the_polls_answer_ahead_of_its_own_takes_it_and_goes_on() {
    // The poll's answer is on the channel when a read asks: the read drops
    // it (its data is what the read takes), and no request stays outstanding.
    let mut r = rig();
    let t = tty(&mut r);
    assert_eq!(revents(&mut r, t.fd, POLLIN), 0);
    let first = sent(&mut r, &t);
    answer_poll(&mut r, &t, first[0].1);
    let server = t.server;
    r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| {
        if let Some((bytes, _)) = net.take_message(server) {
            let (h, _) = zoraya_protocol::Header::decode(&bytes).unwrap();
            let mut buf = [0u8; 64];
            let m = zoraya_proto_tty::ReadReply {
                bytes: zoraya_protocol::Data(b"hi\n"),
            };
            let n = zoraya_protocol::encode(&m, h.transaction, &mut buf).unwrap();
            net.send_message(server, &buf[..n], &[]);
        }
    }));
    let into = r.poke(0, &[0; 8]);
    assert_eq!(r.p.read(t.fd, into), Ok(3));
    r.p.sys.meanwhile = None;
    // Nothing is outstanding or kept: the next look sends afresh.
    assert_eq!(revents(&mut r, t.fd, POLLIN), 0);
    assert_eq!(sent(&mut r, &t).len(), 1);
}

#[test]
fn one_look_after_a_deadline_wait_is_the_answer() {
    let mut r = rig();
    assert_eq!(r.p.poll_once(&mut [], Some(5_000_000)), Ok(Some(0)));
    let (rd, _wr) = r.p.pipe(0).unwrap();
    assert_eq!(
        r.p.poll_once(&mut [pf(rd, POLLIN)], Some(5_000_000)),
        Ok(Some(0))
    );
}

#[test]
fn a_full_pipe_poll_ends_when_the_reader_goes() {
    // The write probe's PEER_CLOSED is the only thing that ends this wait on
    // the real kernel (zoraya-stream clears WRITABLE when the reader is gone).
    let mut r = rig();
    let (rd, wr) = r.p.pipe(o::NONBLOCK).unwrap();
    assert_eq!(
        r.write(wr, &vec![1u8; STREAM_CAPACITY]),
        Ok(STREAM_CAPACITY)
    );
    let end = read_end(&r, rd);
    r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| net.close(end)));
    let mut f = [pf(wr, POLLOUT)];
    assert_eq!(poll(&mut r, &mut f, None), Ok(1));
    assert_eq!(f[0].revents, POLLOUT | POLLERR);
}

#[test]
fn nothing_to_wait_for_and_no_timeout_blocks_instead_of_einval() {
    let mut r = rig();
    let waits = r.p.sys.waits;
    // The fake ends a wait nothing could end as a deadline.
    assert_eq!(r.p.poll_once(&mut [], None), Ok(Some(0)));
    assert!(r.p.sys.waits > waits, "it waited");
    let mut f = [pf(-1, POLLIN), pf(-2, 0)];
    assert_eq!(r.p.poll_once(&mut f, None), Ok(Some(0)));
}

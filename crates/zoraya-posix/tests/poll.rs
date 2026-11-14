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

//! `getrandom` against a fake random service through the fake kernel
//! (ADR-0227 item 4; proposed ADR-0310): the bytes in the buffer are the
//! service's, whole; a program without the connection gets `ENOSYS` and the
//! buffer untouched; a flag it does not know is `EINVAL`; a service that
//! answers wrongly is `EIO` (never a short or invented result).

mod kernel;

use std::cell::RefCell;
use std::rc::Rc;

use kernel::*;
use zoraya_posix::errno::*;
use zoraya_posix::random::{GRND_NONBLOCK, GRND_RANDOM};
use zoraya_posix::sys::Sys;
use zoraya_protocol::{Data, Header, decode, encode};
use zoraya_rng::{Bytes, BytesReply};

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
zoraya_test_alloc::cap_memory!();

/// What the fake service does with a `Bytes` request.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Answers with `count` bytes of a counter that never repeats.
    Honest,
    /// Answers one byte fewer than asked.
    Short,
    /// Never answers: the channel is closed instead.
    Gone,
}

fn rigged(mode: Mode) -> (Rig, Rc<RefCell<Vec<u32>>>) {
    let mut r = rig();
    let (client, server) = r.p.sys.channel_create().unwrap();
    r.p.random = Some(client);
    let asked = Rc::new(RefCell::new(Vec::new()));
    let (log, counter) = (asked.clone(), Rc::new(RefCell::new(0u8)));
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| {
        while let Some((bytes, _)) = n.take_message(server) {
            let (h, _) = Header::decode(&bytes).unwrap();
            let (_, q) = decode::<Bytes>(&bytes, 0).unwrap();
            log.borrow_mut().push(q.count);
            if mode == Mode::Gone {
                n.close(server);
                return;
            }
            let count = q.count as usize - usize::from(mode == Mode::Short);
            let mut c = counter.borrow_mut();
            let body: Vec<u8> = (0..count)
                .map(|_| {
                    *c = c.wrapping_add(1);
                    *c
                })
                .collect();
            let mut out = vec![0u8; 4096 + 64];
            let size = encode(&BytesReply { bytes: Data(&body) }, h.transaction, &mut out).unwrap();
            n.send_message(server, &out[..size], &[]);
        }
    }));
    (r, asked)
}

#[test]
fn the_buffer_is_the_services_bytes_whole() {
    let (mut r, asked) = rigged(Mode::Honest);
    let u = r.arena.user(0, 5000);
    assert_eq!(r.p.getrandom(u, 0), Ok(5000));
    let got = r.peek(0, 5000);
    // 2 KiB a request, three requests: the counter runs on across them.
    assert_eq!(*asked.borrow(), vec![2048, 2048, 904]);
    let want: Vec<u8> = (1..=5000u32).map(|i| (i % 256) as u8).collect();
    assert_eq!(got, want);
}

#[test]
fn a_zero_length_request_asks_nobody() {
    let (mut r, asked) = rigged(Mode::Honest);
    let u = r.arena.user(0, 0);
    assert_eq!(r.p.getrandom(u, 0), Ok(0));
    assert!(asked.borrow().is_empty());
}

#[test]
fn without_the_connection_it_is_enosys_and_the_buffer_is_untouched() {
    let mut r = rig();
    r.poke(0, &[0xAA; 32]);
    let u = r.arena.user(0, 32);
    assert_eq!(r.p.getrandom(u, 0), Err(ENOSYS));
    assert_eq!(r.peek(0, 32), vec![0xAA; 32]);
}

#[test]
fn unknown_flags_are_einval_and_the_known_ones_change_nothing() {
    let (mut r, asked) = rigged(Mode::Honest);
    let u = r.arena.user(0, 8);
    assert_eq!(r.p.getrandom(u, 4), Err(EINVAL));
    assert_eq!(r.p.getrandom(u, 0x8000_0000), Err(EINVAL));
    assert!(asked.borrow().is_empty());
    assert_eq!(r.p.getrandom(u, GRND_NONBLOCK | GRND_RANDOM), Ok(8));
}

#[test]
fn a_short_answer_is_eio_and_no_byte_of_it_reaches_the_program() {
    let (mut r, _) = rigged(Mode::Short);
    r.poke(0, &[0xAA; 16]);
    let u = r.arena.user(0, 16);
    assert_eq!(r.p.getrandom(u, 0), Err(EIO));
    assert_eq!(r.peek(0, 16), vec![0xAA; 16]);
}

#[test]
fn a_service_that_is_gone_is_eio() {
    let (mut r, _) = rigged(Mode::Gone);
    let u = r.arena.user(0, 16);
    assert_eq!(r.p.getrandom(u, 0), Err(EIO));
}

#[test]
fn a_fork_child_has_no_connection() {
    let (mut r, _) = rigged(Mode::Honest);
    r.p.random = None;
    let u = r.arena.user(0, 4);
    assert_eq!(r.p.getrandom(u, 0), Err(ENOSYS));
}

#[test]
fn a_signal_after_the_first_chunk_returns_the_bytes_written_and_before_it_eintr() {
    use zoraya_posix::signals::Action;
    // Before any byte: EINTR and the buffer untouched.
    let (mut r, _) = rigged(Mode::Honest);
    let me = r.p.sys.channel_create().unwrap().0;
    r.p.self_handle = Some(me);
    r.p.signals
        .set(
            10,
            Some(Action {
                handler: 0x4000,
                mask: 0,
                flags: 0,
            }),
        )
        .unwrap();
    r.poke(0, &[0xAA; 5000]);
    r.p.sys.signal_after_waits = Some(0);
    let u = r.arena.user(0, 5000);
    assert_eq!(r.p.getrandom(u, 0), Err(EINTR));
    assert_eq!(r.peek(0, 5000), vec![0xAA; 5000]);
    // After the first 2 KiB chunk: the count of what was written, and only
    // that much of the buffer changed.
    r.p.sys.signal_after_waits = Some(1);
    let u = r.arena.user(0, 5000);
    assert_eq!(r.p.getrandom(u, 0), Ok(2048));
    let got = r.peek(0, 5000);
    assert!(got[..2048].iter().any(|&b| b != 0xAA));
    assert_eq!(got[2048..], vec![0xAA; 5000 - 2048]);
}

/// What the C boundary's `state` does (ADR-0302 item 9): runs `f`; a wait it
/// asks for (`Err(PENDING)`) is made between two runs, and `f` runs again
/// with the outcome. Gives the result and the number of runs.
fn deferred<R>(
    r: &mut Rig,
    mut f: impl FnMut(&mut Rig) -> Result<R, i32>,
) -> (Result<R, i32>, usize) {
    use zoraya_posix::jobs::Resume;
    let mut resume = None;
    let mut runs = 0;
    loop {
        runs += 1;
        assert!(runs < 100, "a deferred call that never ends");
        r.p.set_resume(resume.take());
        let out = f(r);
        let Some(w) = r.p.end_run(&out) else {
            return (out, runs);
        };
        let result = match w.sources.first() {
            Some(&(h, wanted)) => r.p.sys.wait(h, wanted).map(|s| (0, s)),
            None => Err(zoraya_abi::Status::DeadlineExceeded),
        };
        resume = Some(Resume {
            sources: w.sources,
            own_len: w.own_len,
            result,
            progress: w.progress,
        });
    }
}

/// A process with threads (`defer_waits`, every python3: patch 0004 runs the
/// interpreter on a thread of its own) whose request finds no reply at the
/// first read: the fake service answers only inside a wait, the timing in which
/// `rngd` has not run between the request's write and its read. The wait for
/// the reply is a service's, made inside the lock (ADR-0302 item 9), so the call
/// is one run with every byte the service's and no request asked twice. Before
/// the fix the deferral (`Err(PENDING)`) became `ENETDOWN` and then `EIO`, and
/// OpenSSL, unable to seed, refused `SSL_CTX_new` with `ERR_R_RAND_LIB`
/// (0xa080024, `make test-python` on main, run 43565853069).
#[test]
fn a_process_with_threads_gets_the_bytes_when_the_reply_comes_late() {
    let (mut r, asked) = rigged(Mode::Honest);
    r.p.defer_waits = true;
    r.poke(0, &[0xAA; 5000]);
    let u = r.arena.user(0, 5000);
    let (got, runs) = deferred(&mut r, |r| r.p.getrandom(u, 0));
    assert_eq!(got, Ok(5000));
    assert_eq!(runs, 1);
    assert_eq!(*asked.borrow(), vec![2048, 2048, 904]);
    let want: Vec<u8> = (1..=5000u32).map(|i| (i % 256) as u8).collect();
    assert_eq!(r.peek(0, 5000), want);
    // A service that is gone is still EIO, not a deferred wait.
    let (mut r, _) = rigged(Mode::Gone);
    r.p.defer_waits = true;
    let u = r.arena.user(0, 16);
    assert_eq!(deferred(&mut r, |r| r.p.getrandom(u, 0)), (Err(EIO), 1));
}

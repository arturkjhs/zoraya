//! What libposix answers for Rust's `std` that the host can show (ADR-0451):
//! `ioctl(FIONBIO)` as `fcntl(F_SETFL)` does it, and the report of an
//! unanswered `syscall()` number. The numbers and the futex forms are
//! `rust_std`'s own unit tests; the C boundary is `make test-libposix`'s and
//! `make test-rust-std`'s.

mod kernel;

use kernel::*;
use zoraya_abi::posix::SyscallReport;
use zoraya_posix::errno::*;
use zoraya_posix::process::{AT_FDCWD, F_GETFL, F_SETFL, o};
use zoraya_posix::sys::Sys;

zoraya_test_alloc::cap_memory!();

fn flags(r: &mut Rig, fd: i32) -> u32 {
    r.p.fcntl(fd, F_GETFL, 0).unwrap() as u32
}

#[test]
fn fionbio_sets_and_clears_nonblock_on_a_file_a_pipe_and_a_socket() {
    let mut r = rig();
    let file =
        r.p.open(AT_FDCWD, b"/f", o::CREAT | o::RDWR, 0o644)
            .unwrap();
    let (rd, wr) = r.p.pipe(0).unwrap();
    for fd in [file, rd, wr] {
        assert_eq!(flags(&mut r, fd) & o::NONBLOCK, 0, "{fd}");
        r.p.set_nonblocking(fd, true).unwrap();
        assert_ne!(flags(&mut r, fd) & o::NONBLOCK, 0, "{fd}");
        // As F_SETFL does: the other bits are kept.
        r.p.set_nonblocking(fd, true).unwrap();
        r.p.set_nonblocking(fd, false).unwrap();
        assert_eq!(flags(&mut r, fd) & o::NONBLOCK, 0, "{fd}");
    }
    // The access mode survives.
    assert_eq!(flags(&mut r, file) & 3, o::RDWR);
    assert_eq!(flags(&mut r, rd) & 3, o::RDONLY);
    assert_eq!(flags(&mut r, wr) & 3, o::WRONLY);
}

#[test]
fn fionbio_keeps_append_and_fails_on_a_bad_descriptor() {
    let mut r = rig();
    let fd =
        r.p.open(AT_FDCWD, b"/g", o::CREAT | o::RDWR | o::APPEND, 0o644)
            .unwrap();
    r.p.set_nonblocking(fd, true).unwrap();
    assert_ne!(flags(&mut r, fd) & o::APPEND, 0);
    r.p.set_nonblocking(fd, false).unwrap();
    assert_ne!(flags(&mut r, fd) & o::APPEND, 0);
    assert_eq!(r.p.set_nonblocking(77, true), Err(EBADF));
    assert_eq!(r.p.set_nonblocking(-1, false), Err(EBADF));
    // F_SETFL itself still works as before.
    r.p.fcntl(fd, F_SETFL, i64::from(o::NONBLOCK | o::APPEND))
        .unwrap();
    assert_ne!(flags(&mut r, fd) & o::NONBLOCK, 0);
}

fn numbers(r: &mut Rig) -> Vec<u32> {
    let mut out = vec![];
    let mut buffer = vec![0; 64];
    let mut handles = vec![];
    while let Ok(n) = r.p.sys.channel_read(r.reports, &mut buffer, &mut handles) {
        out.push(
            SyscallReport::decode(&buffer[..n])
                .expect("a syscall report")
                .number,
        );
    }
    out
}

#[test]
fn an_unanswered_number_is_reported_on_the_report_channel() {
    let mut r = rig();
    r.p.report_syscall(279);
    r.p.report_syscall(-1);
    assert_eq!(numbers(&mut r), [279, u32::MAX]);
    assert!(numbers(&mut r).is_empty());
}

#[test]
fn syscall_reports_count_within_the_sixteen_of_a_process() {
    let mut r = rig();
    for n in 0..40 {
        r.p.report_syscall(1000 + n);
    }
    assert_eq!(
        numbers(&mut r).len(),
        zoraya_abi::posix::MAX_REPORTS as usize
    );
}

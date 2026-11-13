//! The library against the real filesystem service, through a fake kernel:
//! files, directories, descriptors, pipes and the resolver's reports.

mod kernel;

use kernel::*;
use std::rc::Rc;
use zoraya_abi::posix::{Access, EntryKind};
use zoraya_posix::errno::*;
use zoraya_posix::process::{
    self, AT_FDCWD, AT_REMOVEDIR, AT_SYMLINK_NOFOLLOW, F_DUPFD, F_GETFD, F_GETFL, F_SETFD, F_SETFL,
    FD_CLOEXEC, R_OK, Target, W_OK, X_OK, o,
};
use zoraya_posix::stat;
use zoraya_posix::sys::Sys;

const RW_CREATE: u32 = o::RDWR | o::CREAT;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn open(r: &mut Rig, path: &str, flags: u32, mode: u32) -> Result<i32, i32> {
    r.p.open(AT_FDCWD, path.as_bytes(), flags, mode)
}

#[test]
fn a_file_is_created_written_read_renamed_stated_and_removed() {
    let mut r = rig();
    for name in ["notes.txt", "Київ.txt", "Plzeň — žluťoučký kůň.txt"] {
        let fd = open(&mut r, name, RW_CREATE | o::EXCL, 0o644).unwrap();
        assert_eq!(r.write(fd, name.as_bytes()), Ok(name.len()));
        assert_eq!(r.p.lseek(fd, 0, 0), Ok(0));
        assert_eq!(r.read(fd, 4096).unwrap(), name.as_bytes());
        assert_eq!(r.read(fd, 4096).unwrap(), b"", "end of file");
        let st = r.p.fstat(fd).unwrap();
        assert_eq!(u64_at(&st, 48), name.len() as u64);
        assert_eq!(u32_at(&st, 16), stat::S_IFREG | 0o644);
        assert_eq!(
            (u32_at(&st, 24), u32_at(&st, 28)),
            (1000, 100),
            "the session's ids"
        );
        assert_eq!(u64_at(&st, 0), 7, "the volume");
        r.p.close(fd).unwrap();
        let new = format!("moved-{name}");
        r.p.rename(name.as_bytes(), new.as_bytes()).unwrap();
        assert_eq!(open(&mut r, name, o::RDONLY, 0), Err(ENOENT));
        let st = r.p.stat_at(AT_FDCWD, new.as_bytes(), 0).unwrap();
        assert_eq!(u64_at(&st, 48), name.len() as u64);
        r.p.unlink_at(AT_FDCWD, new.as_bytes(), 0).unwrap();
        assert_eq!(r.p.stat_at(AT_FDCWD, new.as_bytes(), 0), Err(ENOENT));
    }
    assert!(
        r.reports().is_empty(),
        "a missing file inside an entry is never reported"
    );
}

#[test]
fn the_mode_of_a_create_is_masked_and_ids_differ() {
    let mut r = rig();
    let a = open(&mut r, "a", RW_CREATE, 0o777).unwrap();
    let b = open(&mut r, "b", RW_CREATE, 0o640).unwrap();
    let sa = r.p.fstat(a).unwrap();
    let sb = r.p.fstat(b).unwrap();
    assert_eq!(u32_at(&sa, 16) & 0o7777, 0o755, "0o777 & ~0o022");
    assert_eq!(u32_at(&sb, 16) & 0o7777, 0o640);
    assert_ne!(u64_at(&sa, 8), u64_at(&sb, 8), "st_ino is the object's id");
    assert_eq!(r.p.umask(0o077), 0o022);
    let c = open(&mut r, "c", RW_CREATE, 0o666).unwrap();
    assert_eq!(u32_at(&r.p.fstat(c).unwrap(), 16) & 0o777, 0o600);
    // stat by path is one object: the same id as through the fd.
    let by_path = r.p.stat_at(AT_FDCWD, b"/a", 0).unwrap();
    assert_eq!(u64_at(&by_path, 8), u64_at(&sa, 8));
}

/// ADR-0014 rule 6: the owner bits, as for a non-root owner; a file this open
/// made is writable whatever its mode, and O_TRUNC never cuts a protected
/// file.
#[test]
fn owner_bits_are_applied_as_posix_would() {
    let mut r = rig();
    let fd = open(&mut r, "ro", o::WRONLY | o::CREAT, 0o444).unwrap();
    assert_eq!(r.write(fd, b"keep"), Ok(4), "the creator writes");
    r.p.close(fd).unwrap();
    assert_eq!(open(&mut r, "ro", o::WRONLY, 0), Err(EACCES));
    assert_eq!(open(&mut r, "ro", o::WRONLY | o::TRUNC, 0), Err(EACCES));
    let fd = open(&mut r, "ro", o::RDONLY, 0).unwrap();
    assert_eq!(r.read(fd, 10).unwrap(), b"keep", "not truncated");
    assert_eq!(r.p.access_at(AT_FDCWD, b"ro", R_OK, 0), Ok(()));
    assert_eq!(r.p.access_at(AT_FDCWD, b"ro", W_OK, 0), Err(EACCES));
    assert_eq!(r.p.access_at(AT_FDCWD, b"ro", X_OK, 0), Err(EACCES));
    assert_eq!(r.p.access_at(AT_FDCWD, b"missing", 0, 0), Err(ENOENT));
    assert_eq!(r.p.access_at(AT_FDCWD, b"ro", 8, 0), Err(EINVAL));
    r.p.fchmod_at(AT_FDCWD, b"ro", 0o755, 0).unwrap();
    assert_eq!(r.p.access_at(AT_FDCWD, b"ro", X_OK | W_OK, 0), Ok(()));
    // fchmod through an fd opened to read: the owner may.
    r.p.fchmod(fd, 0o600).unwrap();
    assert_eq!(r.p.access_at(AT_FDCWD, b"ro", X_OK, 0), Err(EACCES));
    let wo = open(&mut r, "wo", o::WRONLY | o::CREAT, 0o200).unwrap();
    r.p.close(wo).unwrap();
    assert_eq!(open(&mut r, "wo", o::RDONLY, 0), Err(EACCES));
    assert_eq!(open(&mut r, "wo", o::RDWR, 0), Err(EACCES));
}

#[test]
fn truncate_append_and_exclusive_create() {
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    r.write(fd, b"0123456789").unwrap();
    r.p.close(fd).unwrap();
    assert_eq!(open(&mut r, "f", RW_CREATE | o::EXCL, 0o644), Err(EEXIST));
    let fd = open(&mut r, "f", o::WRONLY | o::TRUNC, 0).unwrap();
    assert_eq!(u64_at(&r.p.fstat(fd).unwrap(), 48), 0);
    r.write(fd, b"ab").unwrap();
    let app = open(&mut r, "f", o::WRONLY | o::APPEND, 0).unwrap();
    assert_eq!(r.p.lseek(app, 0, 0), Ok(0));
    r.write(app, b"cd").unwrap();
    assert_eq!(r.p.lseek(app, 0, 1), Ok(4), "an append moves to the end");
    assert_eq!(
        r.p.fcntl(app, F_GETFL, 0),
        Ok((o::WRONLY | o::APPEND) as i32)
    );
    r.p.fcntl(app, F_SETFL, 0).unwrap();
    assert_eq!(r.p.fcntl(app, F_GETFL, 0), Ok(o::WRONLY as i32));
    r.p.lseek(app, 0, 0).unwrap();
    r.write(app, b"X").unwrap();
    let rd = open(&mut r, "f", o::RDONLY, 0).unwrap();
    assert_eq!(r.read(rd, 10).unwrap(), b"Xbcd");
    r.p.ftruncate(app, 2).unwrap();
    assert_eq!(u64_at(&r.p.fstat(rd).unwrap(), 48), 2);
    assert_eq!(r.p.ftruncate(rd, 0), Err(EINVAL), "read-only");
    assert_eq!(r.p.ftruncate(app, -1), Err(EINVAL));
    assert_eq!(r.write(rd, b"x"), Err(EBADF));
    assert_eq!(r.read(app, 1), Err(EBADF));
    assert_eq!(
        open(&mut r, "f", 3, 0),
        Err(EINVAL),
        "no fourth access mode"
    );
}

/// ADR-0103 item 5: `dup` and `dup2` share one description, so one
/// position; two opens have two; `FD_CLOEXEC` is per fd.
#[test]
fn dup_and_dup2_share_one_position() {
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE | o::CLOEXEC, 0o644).unwrap();
    let copy = r.p.dup(fd).unwrap();
    assert_eq!(r.p.fcntl(fd, F_GETFD, 0), Ok(FD_CLOEXEC));
    assert_eq!(r.p.fcntl(copy, F_GETFD, 0), Ok(0));
    r.write(fd, b"abc").unwrap();
    assert_eq!(r.p.lseek(copy, 0, 1), Ok(3));
    r.write(copy, b"def").unwrap();
    assert_eq!(r.p.lseek(fd, 0, 1), Ok(6));
    assert_eq!(r.p.dup3(fd, 9, 0, false), Ok(9));
    r.p.lseek(9, 1, 0).unwrap();
    assert_eq!(r.read(fd, 2).unwrap(), b"bc");
    assert_eq!(r.read(copy, 10).unwrap(), b"def");
    let other = open(&mut r, "f", o::RDONLY, 0).unwrap();
    assert_eq!(
        r.read(other, 3).unwrap(),
        b"abc",
        "a second open has its own position"
    );
    assert_eq!(r.p.fcntl(fd, F_DUPFD, 20), Ok(20));
    assert_eq!(r.p.fcntl(20, F_SETFD, i64::from(FD_CLOEXEC)), Ok(0));
    assert_eq!(r.p.fcntl(20, F_GETFD, 0), Ok(FD_CLOEXEC));
    // dup2 over an open fd closes it; equal fds change nothing.
    assert_eq!(r.p.dup3(other, copy, 0, false), Ok(copy));
    assert_eq!(r.read(copy, 10).unwrap(), b"def", "copy is now other's");
    assert_eq!(r.p.dup3(copy, copy, 0, false), Ok(copy));
    assert_eq!(
        r.p.dup3(copy, copy, 0, true),
        Err(EINVAL),
        "dup3 refuses equal fds"
    );
    assert_eq!(r.p.dup3(77, 3, 0, false), Err(EBADF));
    assert_eq!(r.p.fcntl(fd, 99, 0), Err(EINVAL));
    for f in [fd, copy, 9, other, 20] {
        r.p.close(f).unwrap();
    }
    assert_eq!(r.p.close(fd), Err(EBADF));
    assert_eq!(r.p.fds.entries(), 0);
}

#[test]
fn directories_are_made_listed_and_removed() {
    let mut r = rig();
    r.p.mkdir_at(AT_FDCWD, b"d", 0o777).unwrap();
    assert_eq!(r.p.mkdir_at(AT_FDCWD, b"d", 0o777), Err(EEXIST));
    r.p.mkdir_at(AT_FDCWD, b"d/sub", 0o700).unwrap();
    let st = r.p.stat_at(AT_FDCWD, b"d", 0).unwrap();
    assert_eq!(u32_at(&st, 16), stat::S_IFDIR | 0o755);
    for name in ["d/x", "d/Žár", "d/ї"] {
        let fd = open(&mut r, name, RW_CREATE, 0o644).unwrap();
        r.p.close(fd).unwrap();
    }
    let dir = open(&mut r, "d", o::RDONLY | o::DIRECTORY, 0).unwrap();
    r.p.is_directory(dir).unwrap();
    let mut names = vec![];
    let mut numbers = vec![];
    while let Some((name, kind, ino)) = r.p.read_dir(dir).unwrap() {
        names.push((name, kind));
        numbers.push(ino);
    }
    names.sort();
    assert_eq!(
        names,
        vec![
            ("sub".to_string(), 4),
            ("x".to_string(), 8),
            ("Žár".to_string(), 8),
            ("ї".to_string(), 8)
        ]
    );
    assert!(numbers.iter().all(|&n| n != 0), "d_ino is never 0");
    assert_eq!(r.p.read_dir(dir), Ok(None), "the end stays the end");
    assert_eq!(r.p.unlink_at(AT_FDCWD, b"d/sub", 0), Err(EISDIR));
    assert_eq!(r.p.unlink_at(AT_FDCWD, b"d/x", AT_REMOVEDIR), Err(ENOTDIR));
    assert_eq!(r.p.unlink_at(AT_FDCWD, b"d", AT_REMOVEDIR), Err(ENOTEMPTY));
    r.p.unlink_at(AT_FDCWD, b"d/sub", AT_REMOVEDIR).unwrap();
    r.p.remove(b"d/x").unwrap();
    r.p.remove("d/Žár".as_bytes()).unwrap();
    r.p.remove("d/ї".as_bytes()).unwrap();
    // openat relative to a directory fd.
    let fd = r.p.open(dir, b"rel", RW_CREATE, 0o644).unwrap();
    r.p.close(fd).unwrap();
    assert!(r.p.stat_at(AT_FDCWD, b"d/rel", 0).is_ok());
    r.p.remove(b"d/rel").unwrap();
    r.p.close(dir).unwrap();
    r.p.remove(b"d").unwrap();
    assert_eq!(r.p.stat_at(AT_FDCWD, b"d", 0), Err(ENOENT));
    let file = open(&mut r, "plain", RW_CREATE, 0o644).unwrap();
    assert_eq!(r.p.read_dir(file), Err(ENOTDIR));
    assert_eq!(r.p.is_directory(file), Err(ENOTDIR));
    assert_eq!(r.p.open(file, b"x", o::RDONLY, 0), Err(ENOTDIR));
    assert_eq!(open(&mut r, "plain/", o::RDONLY, 0), Err(ENOTDIR));
    assert_eq!(open(&mut r, "/", o::WRONLY, 0), Err(EISDIR));
}

#[test]
fn a_directory_listing_pages_and_a_dup_shares_it() {
    let mut r = rig();
    r.p.mkdir_at(AT_FDCWD, b"many", 0o755).unwrap();
    for i in 0..150 {
        let fd = open(&mut r, &format!("many/file-{i:03}"), RW_CREATE, 0o644).unwrap();
        r.p.close(fd).unwrap();
    }
    let dir = open(&mut r, "many", o::RDONLY, 0).unwrap();
    let copy = r.p.dup(dir).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let mut turn = 0;
    loop {
        let fd = if turn % 2 == 0 { dir } else { copy };
        turn += 1;
        match r.p.read_dir(fd).unwrap() {
            Some((name, _, _)) => assert!(seen.insert(name), "a name twice"),
            None => break,
        }
    }
    assert_eq!(seen.len(), 150, "one stream through both fds, across pages");
}

#[test]
fn cwd_and_getcwd() {
    let mut r = rig();
    r.p.mkdir_at(AT_FDCWD, b"/a", 0o755).unwrap();
    r.p.mkdir_at(AT_FDCWD, b"/a/b", 0o755).unwrap();
    r.p.chdir(b"/a/b").unwrap();
    assert_eq!(r.p.getcwd(), "/a/b");
    let fd = open(&mut r, "here", RW_CREATE, 0o644).unwrap();
    r.p.close(fd).unwrap();
    assert!(r.p.stat_at(AT_FDCWD, b"/a/b/here", 0).is_ok());
    r.p.chdir(b"..").unwrap();
    assert_eq!(r.p.getcwd(), "/a");
    assert!(r.p.stat_at(AT_FDCWD, b"b/here", 0).is_ok());
    r.p.chdir(b"../../..").unwrap();
    assert_eq!(r.p.getcwd(), "/");
    assert_eq!(r.p.chdir(b"/a/b/here"), Err(ENOTDIR));
    assert_eq!(r.p.chdir(b"/nope"), Err(ENOENT));
    assert_eq!(r.p.chdir(b"/dev/null"), Err(ENOTDIR));
    assert_eq!(r.p.getcwd(), "/");
}

#[test]
fn links_are_followed_and_read() {
    let mut r = rig();
    let fd = open(&mut r, "target", RW_CREATE, 0o644).unwrap();
    r.write(fd, b"through the link").unwrap();
    r.p.close(fd).unwrap();
    r.p.mkdir_at(AT_FDCWD, b"dir", 0o755).unwrap();
    r.p.symlink_at(b"../target", AT_FDCWD, b"dir/up").unwrap();
    r.p.symlink_at(b"/dir", AT_FDCWD, b"abs").unwrap();
    assert_eq!(r.p.readlink_at(AT_FDCWD, b"dir/up").unwrap(), "../target");
    let fd = open(&mut r, "dir/up", o::RDONLY, 0).unwrap();
    assert_eq!(r.read(fd, 100).unwrap(), b"through the link");
    let fd = open(&mut r, "abs/up", o::RDONLY, 0).unwrap();
    assert_eq!(
        r.read(fd, 100).unwrap(),
        b"through the link",
        "a link in the middle"
    );
    let st =
        r.p.stat_at(AT_FDCWD, b"dir/up", AT_SYMLINK_NOFOLLOW)
            .unwrap();
    assert_eq!(u32_at(&st, 16) & stat::S_IFMT, stat::S_IFLNK);
    let st = r.p.stat_at(AT_FDCWD, b"dir/up", 0).unwrap();
    assert_eq!(u32_at(&st, 16) & stat::S_IFMT, stat::S_IFREG);
    assert_eq!(
        open(&mut r, "dir/up", o::RDONLY | o::NOFOLLOW, 0),
        Err(ELOOP)
    );
    assert_eq!(r.p.readlink_at(AT_FDCWD, b"target"), Err(EINVAL));
    r.p.symlink_at(b"loop", AT_FDCWD, b"loop").unwrap();
    assert_eq!(open(&mut r, "loop", o::RDONLY, 0), Err(ELOOP));
    r.p.symlink_at(b"/etc/shadow", AT_FDCWD, b"out").unwrap();
    // The link's absolute target is read from the table, which has `/`.
    assert_eq!(open(&mut r, "out", o::RDONLY, 0), Err(ENOENT));
    r.p.unlink_at(AT_FDCWD, b"dir/up", 0).unwrap();
    assert!(
        r.p.stat_at(AT_FDCWD, b"target", 0).is_ok(),
        "unlink takes the link, not its target"
    );
}

/// INV-0153-7's library half: a path no entry covers is reported once per
/// path and process, a missing path inside an entry never, and both are
/// `ENOENT` to the program.
#[test]
fn refused_paths_are_reported_once_and_missing_ones_never() {
    let mut r = rig_without_root();
    assert_eq!(open(&mut r, "/etc/passwd", o::RDONLY, 0), Err(ENOENT));
    assert_eq!(open(&mut r, "/etc/passwd", o::RDONLY, 0), Err(ENOENT));
    assert_eq!(
        open(&mut r, "/etc/../etc/./passwd", o::RDONLY, 0),
        Err(ENOENT)
    );
    assert_eq!(open(&mut r, "/work/missing", o::RDONLY, 0), Err(ENOENT));
    assert_eq!(r.p.mkdir_at(AT_FDCWD, b"/elsewhere/d", 0o755), Err(ENOENT));
    assert_eq!(r.p.stat_at(AT_FDCWD, b"/workshop", 0), Err(ENOENT));
    // Entry-relative: `..` from cwd stays in /work, so this is a missing
    // file there, not a refusal.
    assert_eq!(open(&mut r, "../../etc/passwd", o::RDONLY, 0), Err(ENOENT));
    let got = r.reports();
    assert_eq!(
        got,
        vec![
            (Access::Read, 0, "/etc/passwd".to_string()),
            (Access::Write, 0, "/elsewhere/d".to_string()),
            (Access::Read, 0, "/workshop".to_string()),
        ]
    );
}

/// At most 16 reports a process; past that they are dropped and counted.
#[test]
fn reports_stop_at_sixteen() {
    let mut r = rig_without_root();
    for i in 0..20 {
        let path = format!("/outside/{i}");
        assert_eq!(open(&mut r, &path, o::RDONLY, 0), Err(ENOENT));
    }
    let got = r.reports();
    assert_eq!(got.len(), 16);
    assert!(got.iter().all(|(_, dropped, _)| *dropped == 0));
    assert_eq!(r.p.stats.reports, 16);
}

/// A table without `/`: `/work` (cwd) on the volume and `/dev/null`.
fn rig_without_root() -> Rig {
    let mut r = rig();
    r.p.mkdir_at(AT_FDCWD, b"/work", 0o755).unwrap();
    let net = r.net.clone();
    let (service, root) = volume(&net, 1024);
    r.p.sys.services.push(service);
    let mut fresh = zoraya_posix::process::Posix::new(
        std::mem::replace(&mut r.p.sys, Fake::new(net.clone(), vec![])),
        Default::default(),
    );
    let work = fresh.add_entry("/work", EntryKind::Directory, Some(root));
    fresh.add_entry("/dev/null", EntryKind::Null, None);
    fresh.set_cwd(work, "", zoraya_posix::path::Form::EntryRelative);
    let reports = r.reports;
    let (end, far) = fresh.sys.channel_create().unwrap();
    fresh.set_report(Some(end));
    r.p = fresh;
    r.reports = far;
    let _ = reports;
    r
}

#[test]
fn dev_null_reads_nothing_and_takes_everything() {
    let mut r = rig();
    let fd = open(&mut r, "/dev/null", o::RDWR, 0).unwrap();
    assert_eq!(r.write(fd, b"gone"), Ok(4));
    assert_eq!(r.read(fd, 10).unwrap(), b"");
    assert_eq!(r.p.lseek(fd, 100, 0), Ok(0));
    let st = r.p.fstat(fd).unwrap();
    assert_eq!(u32_at(&st, 16) & stat::S_IFMT, stat::S_IFCHR);
    let st = r.p.stat_at(AT_FDCWD, b"/dev/null", 0).unwrap();
    assert_eq!(u32_at(&st, 16) & stat::S_IFMT, stat::S_IFCHR);
    assert_eq!(open(&mut r, "/dev/null/x", o::RDONLY, 0), Err(ENOTDIR));
    assert_eq!(r.p.isatty(fd), Ok(false));
    let ro = open(&mut r, "/dev/null", o::RDONLY, 0).unwrap();
    assert_eq!(r.write(ro, b"x"), Err(EBADF));
    assert_eq!(r.p.unlink_at(AT_FDCWD, b"/dev/null", 0), Err(EBUSY));
}

/// ADR-0101: a pipe within one process, read end at the lower fd, with end
/// of file after the last writer and EPIPE after the last reader.
#[test]
fn a_pipe_between_two_descriptors() {
    let mut r = rig();
    let (rd, wr) = r.p.pipe(0).unwrap();
    assert!(rd < wr);
    assert_eq!(r.write(wr, b"through"), Ok(7));
    assert_eq!(r.read(rd, 100).unwrap(), b"through");
    assert_eq!(r.p.lseek(rd, 0, 0), Err(ESPIPE));
    assert_eq!(
        u32_at(&r.p.fstat(rd).unwrap(), 16) & stat::S_IFMT,
        stat::S_IFIFO
    );
    assert_eq!(r.read(wr, 1), Err(EBADF));
    assert_eq!(r.write(rd, b"x"), Err(EBADF));
    // More than the ring holds, with a reader that drains it: the blocking
    // write waits, here for a wait that nothing will end.
    let big = vec![7u8; STREAM_CAPACITY];
    assert_eq!(r.write(wr, &big), Ok(STREAM_CAPACITY));
    // The fake kernel ends a wait that nothing could end with BadState
    // (EBUSY); the real one would block.
    assert_eq!(
        r.p.write(wr, r.arena.user(0, 1)),
        Err(EBUSY),
        "a wait nothing ends"
    );
    let mut got = 0;
    while got < STREAM_CAPACITY {
        got += r.read(rd, 8192).unwrap().len();
    }
    // Non-blocking: EAGAIN instead of a wait.
    let (nrd, nwr) = r.p.pipe(o::NONBLOCK).unwrap();
    assert_eq!(r.read(nrd, 10), Err(EAGAIN));
    assert_eq!(
        r.p.fcntl(nrd, F_GETFL, 0),
        Ok((o::RDONLY | o::NONBLOCK) as i32)
    );
    r.write(nwr, &big).unwrap();
    assert_eq!(r.write(nwr, b"x"), Err(EAGAIN));
    // The last writer closes: end of file.
    let copy = r.p.dup(wr).unwrap();
    r.p.close(wr).unwrap();
    r.write(copy, b"last").unwrap();
    r.p.close(copy).unwrap();
    assert_eq!(r.read(rd, 100).unwrap(), b"last");
    assert_eq!(r.read(rd, 100).unwrap(), b"", "end of file");
    // The last reader closes: EPIPE (SIGPIPE is the caller's).
    r.p.close(nrd).unwrap();
    assert_eq!(r.write(nwr, b"x"), Err(EPIPE));
    assert_eq!(r.p.pipe(0x1), Err(EINVAL));
}

/// Transfers past MAX_INLINE go through the attached buffer, a MiB at most
/// a request, and come back whole.
#[test]
fn large_transfers_go_through_the_buffer() {
    let mut r = rig();
    let fd = open(&mut r, "big", RW_CREATE, 0o644).unwrap();
    let data: Vec<u8> = (0..(1024 * 1024 + 70_000))
        .map(|i| (i * 7 + i / 251) as u8)
        .collect();
    let requests = r.p.stats.requests;
    assert_eq!(r.write(fd, &data), Ok(data.len()));
    assert!(
        r.p.stats.requests - requests <= 4,
        "attach and two buffer writes, not 18 inline"
    );
    assert_eq!(r.p.lseek(fd, 0, 0), Ok(0));
    let mut back = vec![];
    loop {
        let chunk = r.read(fd, 1024 * 1024 + 1).unwrap();
        if chunk.is_empty() {
            break;
        }
        back.extend(chunk);
    }
    assert_eq!(back.len(), data.len());
    assert!(back == data, "the bytes come back");
    // A 4 KiB read is inline.
    r.p.lseek(fd, 4096, 0).unwrap();
    assert_eq!(r.read(fd, 4096).unwrap(), &data[4096..8192]);
}

#[test]
fn bad_descriptors_and_paths_are_refused() {
    let mut r = rig();
    for fd in [-1, 3, 511, 512, i32::MAX] {
        assert_eq!(r.read(fd, 1), Err(EBADF), "{fd}");
        assert_eq!(r.write(fd, b"x"), Err(EBADF));
        assert_eq!(r.p.close(fd), Err(EBADF));
        assert_eq!(r.p.lseek(fd, 0, 0), Err(EBADF));
        assert_eq!(r.p.fstat(fd), Err(EBADF));
        assert_eq!(r.p.dup(fd), Err(EBADF));
        assert_eq!(r.p.fcntl(fd, F_GETFL, 0), Err(EBADF));
        assert_eq!(r.p.open(fd, b"x", o::RDONLY, 0), Err(EBADF));
        assert_eq!(r.p.isatty(fd), Err(EBADF));
    }
    assert_eq!(open(&mut r, "", o::RDONLY, 0), Err(ENOENT));
    assert_eq!(
        r.p.open(AT_FDCWD, b"bad\xff", o::RDONLY | o::CREAT, 0),
        Err(EILSEQ)
    );
    assert_eq!(
        r.p.open(AT_FDCWD, &[b'a'; 5000], o::RDONLY, 0),
        Err(ENAMETOOLONG)
    );
    assert_eq!(
        open(&mut r, "ctl\u{1}x", RW_CREATE, 0o644).map(|_| ()),
        Err(EINVAL),
        "a control character is not a name"
    );
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    assert_eq!(r.p.lseek(fd, 0, 3), Err(EINVAL));
    assert_eq!(r.p.lseek(fd, -1, 0), Err(EINVAL));
    assert_eq!(
        r.p.open(AT_FDCWD, b"d", o::CREAT | o::DIRECTORY, 0),
        Err(EINVAL)
    );
}

#[test]
fn times_are_set_and_tmp_and_publish_work() {
    let mut r = rig();
    let fd = open(&mut r, "t", RW_CREATE, 0o644).unwrap();
    r.p.set_mtime(Target::Fd(fd), Some((86_400, 5)), 0).unwrap();
    let st = r.p.fstat(fd).unwrap();
    assert_eq!((u64_at(&st, 88), u64_at(&st, 96)), (86_400, 5));
    assert_eq!(&st[72..88], &st[88..104], "atime is mtime");
    r.p.set_mtime(Target::Path(AT_FDCWD, b"t"), Some((10, 0)), 0)
        .unwrap();
    assert_eq!(u64_at(&r.p.fstat(fd).unwrap(), 88), 10);
    assert_eq!(
        r.p.set_mtime(Target::Fd(fd), Some((1, 1_000_000_000)), 0),
        Err(EINVAL)
    );
    assert_eq!(r.p.set_mtime(Target::Fd(99), None, 0), Err(EBADF));
    assert_eq!(r.p.set_mtime(Target::Fd(fd), None, 0), Ok(()));
    // No Tmp entry yet: nowhere for an unnamed file.
    assert_eq!(r.p.unnamed(false), Err(ENOENT));
    r.p.mkdir_at(AT_FDCWD, b"/tmp", 0o777).unwrap();
    let net = r.net.clone();
    let (service, tmp) = volume(&net, 1024);
    r.p.sys.services.push(service);
    r.p.add_entry("/tmp", EntryKind::Tmp, Some(tmp));
    let u = r.p.unnamed(false).unwrap();
    r.write(u, b"anonymous").unwrap();
    r.p.lseek(u, 0, 0).unwrap();
    assert_eq!(r.read(u, 100).unwrap(), b"anonymous");
    // O_TMPFILE in a directory, then linkat's publish.
    let t = open(&mut r, "/tmp", o::TMPFILE | o::RDWR, 0o600).unwrap();
    r.write(t, b"published").unwrap();
    r.p.publish(t, AT_FDCWD, b"/tmp/named").unwrap();
    let named = open(&mut r, "/tmp/named", o::RDONLY, 0).unwrap();
    assert_eq!(r.read(named, 100).unwrap(), b"published");
    assert_eq!(
        r.p.publish(named, AT_FDCWD, b"/tmp/again"),
        Err(EPERM),
        "no hard links"
    );
    assert_eq!(
        r.p.rename(b"/tmp/named", b"/elsewhere"),
        Err(EXDEV),
        "two services"
    );
}

#[test]
fn closing_frees_every_handle() {
    let mut r = rig();
    let before = r.handles();
    for _ in 0..3 {
        let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
        let d = r.p.dup(fd).unwrap();
        r.write(d, &vec![1u8; 100_000]).unwrap();
        let (a, b) = r.p.pipe(0).unwrap();
        r.p.mkdir_at(AT_FDCWD, b"x", 0o755).unwrap();
        r.p.stat_at(AT_FDCWD, b"x", 0).unwrap();
        r.p.rename(b"x", b"y").unwrap();
        r.p.remove(b"y").unwrap();
        for f in [fd, d, a, b] {
            r.p.close(f).unwrap();
        }
        r.p.remove(b"f").unwrap();
    }
    r.settle();
    // What stays: the scratch buffer, for the process's life, and the token
    // the service keeps for the root connection since the first rename.
    assert_eq!(r.handles(), before + 2, "{} then {}", before, r.handles());
}

#[test]
fn process_constants_are_musls() {
    assert_eq!(
        (o::CREAT, o::EXCL, o::TRUNC, o::APPEND),
        (0o100, 0o200, 0o1000, 0o2000)
    );
    assert_eq!(
        (o::DIRECTORY, o::NOFOLLOW, o::CLOEXEC),
        (0o40000, 0o100000, 0o2000000)
    );
    assert_eq!((process::AT_FDCWD, process::F_DUPFD_CLOEXEC), (-100, 1030));
}

/// A last component of `.` or `..` names no entry of a directory: the calls
/// that take a name refuse it instead of acting on the parent (review R1).
#[test]
fn dot_as_the_last_name_is_refused_by_the_naming_calls() {
    let mut r = rig();
    r.p.mkdir_at(AT_FDCWD, b"d", 0o755).unwrap();
    assert_eq!(r.p.mkdir_at(AT_FDCWD, b"new/.", 0o755), Err(EEXIST));
    assert_eq!(
        r.p.stat_at(AT_FDCWD, b"new", 0),
        Err(ENOENT),
        "nothing made"
    );
    assert_eq!(r.p.unlink_at(AT_FDCWD, b"d/.", AT_REMOVEDIR), Err(EINVAL));
    assert_eq!(r.p.unlink_at(AT_FDCWD, b"d/..", AT_REMOVEDIR), Err(EINVAL));
    assert_eq!(r.p.rename(b"d/.", b"e"), Err(EINVAL));
    assert_eq!(r.p.rename(b"d", b"d/.."), Err(EINVAL));
    assert_eq!(r.p.symlink_at(b"x", AT_FDCWD, b"d/."), Err(EEXIST));
    assert!(r.p.stat_at(AT_FDCWD, b"d", 0).is_ok(), "d untouched");
    let fd = open(&mut r, "d/.", o::RDONLY, 0).unwrap();
    r.p.is_directory(fd).unwrap();
}

/// `fchmod` through an fd opened to read changes the fd's own object, and
/// never what its old name holds after a rename and a new create (R2).
#[test]
fn fchmod_by_place_acts_only_on_the_fds_object() {
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    r.p.close(fd).unwrap();
    let ro = open(&mut r, "f", o::RDONLY, 0).unwrap();
    r.p.fchmod(ro, 0o600).unwrap();
    assert_eq!(u32_at(&r.p.fstat(ro).unwrap(), 16) & 0o777, 0o600);
    r.p.rename(b"f", b"g").unwrap();
    let new = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    assert_eq!(
        r.p.fchmod(ro, 0o400),
        Err(EACCES),
        "f is another object now"
    );
    assert_eq!(u32_at(&r.p.fstat(new).unwrap(), 16) & 0o777, 0o644);
    assert_eq!(u32_at(&r.p.fstat(ro).unwrap(), 16) & 0o777, 0o600);
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md):
// the library's cap does not reach an integration test binary.
zoraya_test_alloc::cap_memory!();

// ---- the holes the mutation run of zoraya-posix found (TASK-20 review) ----

use zoraya_posix::process::{F_DUPFD_CLOEXEC, F_SETFD as SETFD};
use zoraya_posix::sys::User;
use zoraya_proto_file as proto;
use zoraya_protocol::{Below, Data, Text, encode};

/// The kernel handle of a pipe fd's read end.
fn read_end(r: &Rig, fd: i32) -> H {
    match r.p.fds.get(fd) {
        Some(process::Desc::Stream { read: Some(h), .. }) => *h,
        _ => panic!("fd {fd} is no read end"),
    }
}

fn fd_flag(r: &mut Rig, fd: i32) -> i32 {
    r.p.fcntl(fd, F_GETFD, 0).unwrap()
}

/// A Tmp entry: a second volume standing in for the temporary service.
fn with_tmp(r: &mut Rig) {
    r.p.mkdir_at(AT_FDCWD, b"/tmp", 0o777).unwrap();
    let net = r.net.clone();
    let (service, tmp) = volume(&net, 1024);
    r.p.sys.services.push(service);
    r.p.add_entry("/tmp", EntryKind::Tmp, Some(tmp));
}

/// A message with any ordinal and body, as no service would send.
fn raw_reply(ordinal: u32, transaction: u32, body: &[u8]) -> Vec<u8> {
    let mut magic = [0u8; 64];
    encode(&zoraya_protocol::ErrorReply { status: 0 }, 0, &mut magic).unwrap();
    let mut m = magic[..4].to_vec();
    m.extend_from_slice(&ordinal.to_le_bytes());
    m.extend_from_slice(&transaction.to_le_bytes());
    m.extend_from_slice(&(body.len() as u32).to_le_bytes());
    m.extend_from_slice(body);
    m
}

fn reply<'a, M: zoraya_protocol::Message<'a>>(m: M) -> impl FnOnce(u32) -> Vec<u8> {
    let mut out = vec![0u8; 70_000];
    let mut keep = Some(move |t: u32| {
        let n = encode(&m, t, &mut out).unwrap();
        out.truncate(n);
        out
    });
    move |t| (keep.take().unwrap())(t)
}

#[test]
fn fd_flags_are_set_only_as_asked() {
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    assert_eq!(fd_flag(&mut r, fd), 0);
    let n = open(&mut r, "/dev/null", o::RDWR | o::CLOEXEC, 0).unwrap();
    assert_eq!(fd_flag(&mut r, n), FD_CLOEXEC);
    let n = open(&mut r, "/dev/null", o::RDWR, 0).unwrap();
    assert_eq!(fd_flag(&mut r, n), 0);
    let (a, b) = r.p.pipe(0).unwrap();
    assert_eq!((fd_flag(&mut r, a), fd_flag(&mut r, b)), (0, 0));
    let (a, b) = r.p.pipe(o::CLOEXEC).unwrap();
    assert_eq!(
        (fd_flag(&mut r, a), fd_flag(&mut r, b)),
        (FD_CLOEXEC, FD_CLOEXEC)
    );
    assert_eq!(r.p.dup3(fd, 9, 0, false), Ok(9));
    assert_eq!(fd_flag(&mut r, 9), 0);
    assert_eq!(r.p.fcntl(fd, F_DUPFD, 20), Ok(20));
    assert_eq!(fd_flag(&mut r, 20), 0);
    assert_eq!(r.p.fcntl(fd, F_DUPFD_CLOEXEC, 30), Ok(30));
    assert_eq!(fd_flag(&mut r, 30), FD_CLOEXEC);
    r.p.fcntl(30, SETFD, 0).unwrap();
    assert_eq!(fd_flag(&mut r, 30), 0);
}

#[test]
fn dup3_takes_its_flags_and_refuses_the_others() {
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    assert_eq!(r.p.dup3(fd, 8, 0, true), Ok(8));
    assert_eq!(r.p.dup3(fd, 9, o::CLOEXEC, true), Ok(9));
    assert_eq!(fd_flag(&mut r, 9), FD_CLOEXEC);
    assert_eq!(r.p.dup3(fd, 10, 1, true), Err(EINVAL));
}

#[test]
fn fcntl_setfl_sets_nonblock_on_a_pipe() {
    let mut r = rig();
    let (rd, _) = r.p.pipe(0).unwrap();
    r.p.fcntl(rd, F_SETFL, i64::from(o::NONBLOCK)).unwrap();
    assert_eq!(
        r.p.fcntl(rd, F_GETFL, 0),
        Ok((o::RDONLY | o::NONBLOCK) as i32)
    );
    assert_eq!(r.read(rd, 1), Err(EAGAIN));
    r.p.fcntl(rd, F_SETFL, 0).unwrap();
    assert_eq!(r.p.fcntl(rd, F_GETFL, 0), Ok(o::RDONLY as i32));
}

#[test]
fn open_flags_reach_the_service_as_asked() {
    let mut r = rig();
    let fd = open(&mut r, "f", o::RDWR | o::CREAT, 0o644).unwrap();
    r.write(fd, b"abc").unwrap();
    r.p.lseek(fd, 0, 0).unwrap();
    r.write(fd, b"X").unwrap();
    r.p.lseek(fd, 0, 0).unwrap();
    assert_eq!(
        r.read(fd, 10).unwrap(),
        b"Xbc",
        "no append without O_APPEND"
    );
    assert!(
        open(&mut r, "f", RW_CREATE, 0o644).is_ok(),
        "O_CREAT alone opens what exists"
    );
    assert_eq!(open(&mut r, "f", o::RDONLY | o::DIRECTORY, 0), Err(ENOTDIR));
    assert!(
        open(&mut r, "f", o::RDONLY | o::NOFOLLOW, 0).is_ok(),
        "a file is no link"
    );
    assert_eq!(r.p.lseek(fd, 0, 2), Ok(3), "SEEK_END");
}

#[test]
fn an_entry_root_named_without_a_slash_opens_as_a_directory() {
    let mut r = rig_without_root();
    let fd = open(&mut r, "/work", o::RDONLY, 0).unwrap();
    r.p.is_directory(fd).unwrap();
    assert_eq!(
        open(&mut r, "/work", RW_CREATE | o::EXCL, 0o644),
        Err(EEXIST)
    );
}

#[test]
fn o_tmpfile_takes_the_mode_and_cloexec_it_asks_for() {
    let mut r = rig();
    with_tmp(&mut r);
    let t = open(&mut r, "/tmp", o::TMPFILE | o::CREAT | o::RDWR, 0o640).unwrap();
    assert_eq!(fd_flag(&mut r, t), 0);
    r.p.publish(t, AT_FDCWD, b"/tmp/named").unwrap();
    let st = r.p.stat_at(AT_FDCWD, b"/tmp/named", 0).unwrap();
    assert_eq!(u32_at(&st, 16) & 0o7777, 0o640);
    let c = open(&mut r, "/tmp", o::TMPFILE | o::RDWR | o::CLOEXEC, 0o600).unwrap();
    assert_eq!(fd_flag(&mut r, c), FD_CLOEXEC);
    assert_eq!(
        open(&mut r, "/tmp", o::TMPFILE | o::RDONLY, 0o600),
        Err(EINVAL)
    );
}

#[test]
fn a_refused_create_is_reported_as_a_write() {
    let mut r = rig_without_root();
    assert_eq!(
        open(&mut r, "/elsewhere/c", o::RDONLY | o::CREAT, 0o644),
        Err(ENOENT)
    );
    assert_eq!(
        r.reports(),
        vec![(Access::Write, 0, "/elsewhere/c".to_string())]
    );
}

#[test]
fn pipes_wait_and_count_as_posix_says() {
    let mut r = rig();
    let (rd, _wr) = r.p.pipe(0).unwrap();
    // The fake ends a wait nothing could end with EBUSY: the read waited.
    assert_eq!(r.read(rd, 1), Err(EBUSY));
    let (_nrd, nwr) = r.p.pipe(o::NONBLOCK).unwrap();
    let big = vec![1u8; 2 * STREAM_CAPACITY];
    assert_eq!(r.write(nwr, &big), Ok(STREAM_CAPACITY), "what fit");
    // A blocking write of twice the ring, with a reader draining it meanwhile.
    let (brd, bwr) = r.p.pipe(0).unwrap();
    let brd = read_end(&r, brd);
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| {
        n.drain(brd, usize::MAX);
    }));
    assert_eq!(r.write(bwr, &big), Ok(2 * STREAM_CAPACITY));
    // A reader that closes while the writer waits: the count written.
    let (crd, cwr) = r.p.pipe(0).unwrap();
    let crd = read_end(&r, crd);
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| n.close(crd)));
    assert_eq!(r.write(cwr, &big), Ok(STREAM_CAPACITY));
    r.p.sys.meanwhile = None;
    assert_eq!(
        r.write(cwr, b"x"),
        Err(EPIPE),
        "and the next write meets EPIPE"
    );
}

#[test]
fn a_transfer_up_to_max_inline_is_one_request() {
    let mut r = rig();
    let requests = |r: &mut Rig, f: &mut dyn FnMut(&mut Rig)| {
        let before = r.p.stats.requests;
        f(r);
        r.p.stats.requests - before
    };
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    assert_eq!(
        requests(&mut r, &mut |r| assert_eq!(
            r.write(fd, b"0123456789"),
            Ok(10)
        )),
        1
    );
    let fd = open(&mut r, "g", RW_CREATE, 0o644).unwrap();
    let block = vec![7u8; 61_440];
    assert_eq!(
        requests(&mut r, &mut |r| assert_eq!(r.write(fd, &block), Ok(61_440))),
        1
    );
    let fd = open(&mut r, "g", o::RDONLY, 0).unwrap();
    assert_eq!(
        requests(&mut r, &mut |r| assert_eq!(
            r.read(fd, 61_440).unwrap().len(),
            61_440
        )),
        1
    );
    let fd = open(&mut r, "g", o::RDONLY, 0).unwrap();
    assert_eq!(
        requests(&mut r, &mut |r| assert_eq!(
            r.read(fd, 4096).unwrap().len(),
            4096
        )),
        1
    );
    let fd = open(&mut r, "h", RW_CREATE, 0o644).unwrap();
    r.write(fd, &vec![3u8; 102_400]).unwrap();
    r.p.lseek(fd, 0, 0).unwrap();
    assert_eq!(
        r.read(fd, 102_400).unwrap().len(),
        102_400,
        "past MAX_INLINE, whole"
    );
    let e = open(&mut r, "empty", RW_CREATE, 0o644).unwrap();
    r.p.close(e).unwrap();
    let n = requests(&mut r, &mut |r| {
        open(r, "empty", o::WRONLY | o::TRUNC, 0).unwrap();
    });
    assert_eq!(n, 1, "an empty file needs no set_size");
    assert_eq!(r.p.write(fd, User::new(0, usize::MAX)), Err(EINVAL));
}

#[test]
fn a_write_the_volume_cannot_take_is_its_count_then_enospc() {
    let mut r = rig_sized(400);
    let fd = open(&mut r, "big", RW_CREATE, 0o644).unwrap();
    let data = vec![9u8; 2 << 20];
    assert_eq!(
        r.write(fd, &data),
        Ok(1 << 20),
        "the first MiB fits, the second not"
    );
    assert_eq!(r.write(fd, &data[..1 << 20]), Err(ENOSPC));
}

#[test]
fn special_files_report_their_modes_and_errors() {
    let mut r = rig();
    let (rd, _) = r.p.pipe(0).unwrap();
    assert_eq!(u32_at(&r.p.fstat(rd).unwrap(), 16) & 0o777, 0o600);
    let n = open(&mut r, "/dev/null", o::RDWR, 0).unwrap();
    assert_eq!(u32_at(&r.p.fstat(n).unwrap(), 16) & 0o777, 0o666);
    assert_eq!(
        u32_at(&r.p.stat_at(AT_FDCWD, b"/dev/null", 0).unwrap(), 16) & 0o777,
        0o666
    );
    r.p.add_entry("/proc/self/exe", EntryKind::Image, None);
    assert_eq!(
        u32_at(&r.p.stat_at(AT_FDCWD, b"/proc/self/exe", 0).unwrap(), 16) & 0o777,
        0o555
    );
    r.p.add_entry("/dev/tty", EntryKind::Terminal, None);
    assert_eq!(open(&mut r, "/dev/tty", o::RDWR, 0), Err(ENXIO));
    assert_eq!(open(&mut r, "/proc/self/exe", o::RDONLY, 0), Err(EACCES));
    // A name under a special entry is ENOTDIR, whatever the call.
    let t = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    assert_eq!(r.p.mkdir_at(AT_FDCWD, b"/dev/null/x", 0o755), Err(ENOTDIR));
    assert_eq!(r.p.symlink_at(b"x", AT_FDCWD, b"/dev/null/x"), Err(ENOTDIR));
    assert_eq!(
        r.p.publish(t, AT_FDCWD, b"/dev/null/x"),
        Err(EPERM),
        "a named file first"
    );
    assert_eq!(r.p.unlink_at(AT_FDCWD, b"/dev/null/x", 0), Err(ENOTDIR));
    assert_eq!(r.p.mkdir_at(AT_FDCWD, b"/dev/null", 0o755), Err(EEXIST));
    assert_eq!(r.p.unlink_at(AT_FDCWD, b"/dev/null", 0), Err(EBUSY));
    assert_eq!(r.p.rename(b"f", b"/dev/null"), Err(EXDEV));
    assert_eq!(r.p.rename(b"missing/f", b"/"), Err(EBUSY));
}

#[test]
fn stat_at_takes_an_empty_path_only_with_at_empty_path() {
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    assert_eq!(r.p.stat_at(AT_FDCWD, b"", 0), Err(ENOENT));
    assert_eq!(
        r.p.stat_at(fd, b"", zoraya_posix::process::AT_EMPTY_PATH),
        r.p.fstat(fd)
    );
}

#[test]
fn access_and_utimensat_follow_a_link_unless_told_not_to() {
    let mut r = rig();
    r.p.symlink_at(b"nowhere", AT_FDCWD, b"dangle").unwrap();
    assert_eq!(r.p.access_at(AT_FDCWD, b"dangle", 0, 0), Err(ENOENT));
    assert_eq!(
        r.p.access_at(AT_FDCWD, b"dangle", 0, AT_SYMLINK_NOFOLLOW),
        Ok(())
    );
    let at = Target::Path(AT_FDCWD, b"dangle");
    assert_eq!(r.p.set_mtime(at, Some((10, 0)), 0), Err(ENOENT));
    let at = Target::Path(AT_FDCWD, b"dangle");
    assert_eq!(
        r.p.set_mtime(at, Some((10, 0)), AT_SYMLINK_NOFOLLOW),
        Ok(())
    );
    let ro = open(&mut r, "ro", o::WRONLY | o::CREAT, 0o444).unwrap();
    r.p.close(ro).unwrap();
    assert_eq!(
        r.p.access_at(AT_FDCWD, b"ro", 0, AT_SYMLINK_NOFOLLOW),
        Ok(())
    );
    assert_eq!(r.p.access_at(AT_FDCWD, b"ro", 0, 0x1), Err(EINVAL));
    let wo = open(&mut r, "wo", o::WRONLY | o::CREAT, 0o200).unwrap();
    r.p.close(wo).unwrap();
    assert_eq!(r.p.access_at(AT_FDCWD, b"wo", R_OK, 0), Err(EACCES));
    assert_eq!(r.p.access_at(AT_FDCWD, b"wo", 0, 0), Ok(()));
    assert_eq!(r.p.fchmod_at(AT_FDCWD, b"wo", 0o600, 0x1), Err(EINVAL));
    assert_eq!(
        r.p.fchmod_at(AT_FDCWD, b"wo", 0o600, AT_SYMLINK_NOFOLLOW),
        Err(ENOTSUP)
    );
}

#[test]
fn naming_calls_refuse_the_root_and_dot_names_and_close_what_they_open() {
    let mut r = rig();
    with_tmp(&mut r);
    let t = open(&mut r, "/tmp", o::TMPFILE | o::RDWR, 0o600).unwrap();
    assert_eq!(r.p.mkdir_at(AT_FDCWD, b"/", 0o755), Err(EEXIST));
    assert_eq!(r.p.symlink_at(b"x", AT_FDCWD, b"/"), Err(EEXIST));
    assert_eq!(r.p.publish(t, AT_FDCWD, b"/"), Err(EEXIST));
    assert_eq!(r.p.symlink_at(b"x", AT_FDCWD, b"new/."), Err(EEXIST));
    assert_eq!(r.p.publish(t, AT_FDCWD, b"/tmp/new/."), Err(EEXIST));
    r.settle();
    let before = r.handles();
    r.p.mkdir_at(AT_FDCWD, b"d", 0o755).unwrap();
    r.p.mkdir_at(AT_FDCWD, b"d/e", 0o755).unwrap();
    r.p.remove(b"d/e").unwrap();
    r.p.remove(b"d").unwrap();
    r.settle();
    assert_eq!(r.handles(), before, "a nested name's parent is closed");
}

#[test]
fn share_gives_a_connection_to_the_same_description() {
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    r.write(fd, b"shared").unwrap();
    r.p.lseek(fd, 2, 0).unwrap();
    let h = r.p.share(fd).unwrap();
    assert_eq!(r.p.stats.shares, 1);
    let mut out = vec![0u8; 256];
    let n = encode(&proto::ReadCurrent { length: 4 }, 77, &mut out).unwrap();
    r.p.sys.channel_write(h, &out[..n], &[]).unwrap();
    r.p.sys
        .wait(h, zoraya_posix::sys::signals::READABLE)
        .unwrap();
    let mut back = vec![0u8; 70_000];
    let mut handles = vec![];
    let n = r.p.sys.channel_read(h, &mut back, &mut handles).unwrap();
    let (_, m) = zoraya_protocol::decode::<proto::ReadReply<'_>>(&back[..n], 0).unwrap();
    assert_eq!(
        (m.status.0, m.data.0),
        (proto::status::OK, &b"ared"[..]),
        "one position"
    );
    // It may write and clone too, as the fd's own connection may.
    let w = proto::WriteCurrent {
        flags: zoraya_protocol::Flags(0),
        data: Data(b"!"),
    };
    let n = encode(&w, 78, &mut out).unwrap();
    r.p.sys.channel_write(h, &out[..n], &[]).unwrap();
    r.p.sys
        .wait(h, zoraya_posix::sys::signals::READABLE)
        .unwrap();
    let n = r.p.sys.channel_read(h, &mut back, &mut handles).unwrap();
    let (_, d) = zoraya_protocol::decode::<proto::Done>(&back[..n], 0).unwrap();
    assert_eq!(d.status.0, proto::status::OK, "write");
    let (_theirs, mine) = r.p.sys.channel_create().unwrap();
    let c = proto::Clone {
        rights: zoraya_protocol::Flags(proto::rights::READ),
    };
    let n = encode(&c, 79, &mut out).unwrap();
    r.p.sys.channel_write(h, &out[..n], &[mine]).unwrap();
    r.p.sys
        .wait(h, zoraya_posix::sys::signals::READABLE)
        .unwrap();
    let n = r.p.sys.channel_read(h, &mut back, &mut handles).unwrap();
    let (_, d) = zoraya_protocol::decode::<proto::Done>(&back[..n], 0).unwrap();
    assert_eq!(d.status.0, proto::status::OK, "clone");
    assert_eq!(
        r.p.lseek(fd, 0, 1),
        Ok(7),
        "the write through it moved the fd's position"
    );
}

#[test]
fn the_counters_and_the_table_setters_do_what_they_say() {
    let mut r = rig();
    let before = r.p.stats;
    r.p.stat_at(AT_FDCWD, b"/", 0).unwrap();
    assert_eq!(r.p.stats.resolutions, before.resolutions + 1);
    assert!(r.p.stats.resolve_ns > before.resolve_ns);
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    let before = r.p.stats;
    r.p.lseek(fd, 0, 0).unwrap();
    assert_eq!(
        (r.p.stats.requests, r.p.stats.seeks),
        (before.requests + 1, before.seeks + 1)
    );
    let root = r.root;
    assert_eq!(r.p.add_entry("/sub", EntryKind::Directory, Some(root)), 2);
    assert_eq!(r.p.add_entry("/sub2", EntryKind::Directory, Some(root)), 3);
    r.p.set_cwd(2, "a", zoraya_posix::path::Form::Absolute);
    assert_eq!(r.p.getcwd(), "/sub/a");
}

#[test]
fn from_bootstrap_takes_the_form_fds_and_kinds_from_the_record() {
    use zoraya_abi::posix::{ABSOLUTE, Bootstrap, Entry, Fd, FdKind, NO_HANDLE, encode};
    for flags in [0, ABSOLUTE] {
        let mut r = rig();
        let root = r.root;
        let entries = [Entry {
            prefix: "/work",
            kind: EntryKind::Directory,
            handle: 0,
        }];
        let fds = [
            Fd {
                fd: 3,
                kind: FdKind::Dir,
                flags: zoraya_abi::posix::FD_NONBLOCK,
                tty: NO_HANDLE,
            },
            Fd {
                fd: 4,
                kind: FdKind::File,
                flags: 0,
                tty: NO_HANDLE,
            },
        ];
        let mut bytes = [0u8; 512];
        let facts = Default::default();
        let n = encode(&facts, flags, "/x", 0, "", &fds, &entries, 1, &mut bytes).unwrap();
        let record = Bootstrap::decode(&bytes[..n], 1).unwrap();
        let sys = std::mem::replace(&mut r.p.sys, Fake::new(r.net.clone(), vec![]));
        let mut p =
            zoraya_posix::process::Posix::from_bootstrap(sys, &record, &[root], |_| root, None);
        let up = p.resolve(AT_FDCWD, b"../../x", Access::Read);
        if flags == ABSOLUTE {
            assert_eq!(up.map(|r| r.absolute), Err(ENOENT), "climbs out of /work");
        } else {
            assert_eq!(up.unwrap().absolute, "/work/x", "stays in /work");
        }
        assert_eq!(p.is_directory(3), Ok(()));
        assert_eq!(p.is_directory(4), Err(ENOTDIR));
        assert_eq!(
            p.fcntl(3, F_GETFL, 0).unwrap() as u32 & o::NONBLOCK,
            o::NONBLOCK
        );
        assert_eq!(p.fcntl(4, F_GETFL, 0).unwrap() as u32 & o::NONBLOCK, 0);
    }
}

/// Replies with a status that is not OK are the status's errno, whatever
/// else they carry; a reply that does not decode is judged by its status
/// word. The service sends such replies only when its engine fails, so the
/// fake kernel crafts them.
#[test]
fn a_reply_that_is_not_ok_is_its_errno_whatever_else_it_carries() {
    use proto::status::*;
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    r.write(fd, b"data").unwrap();
    let stat = proto::StatReply {
        status: Below(ACCESS_DENIED),
        volume: 1,
        id: 2,
        size: 3,
        modified_seconds: 4,
        modified_nanos: 5,
        changed_seconds: 6,
        changed_nanos: 7,
        mode: 0o644,
        kind: Below(0),
    };
    r.p.sys.craft = Some(Box::new(reply(stat)));
    assert_eq!(r.p.fstat(fd), Err(EACCES));
    r.p.lseek(fd, 0, 0).unwrap();
    let read = proto::ReadReply {
        status: Below(CORRUPT),
        data: Data(b""),
    };
    r.p.sys.craft = Some(Box::new(reply(read)));
    assert_eq!(r.read(fd, 10), Err(EIO));
    r.p.symlink_at(b"t", AT_FDCWD, b"l").unwrap();
    let link = proto::ReadLinkReply {
        status: Below(CORRUPT),
        target: Text(""),
    };
    r.p.sys.craft = Some(Box::new(reply(link)));
    r.p.sys.craft_skip = 1;
    assert_eq!(r.p.readlink_at(AT_FDCWD, b"l"), Err(EIO));
    let dir = open(&mut r, "/", o::RDONLY, 0).unwrap();
    let listing = proto::ReadDirectoryIdsReply {
        status: Below(CORRUPT),
        entries: zoraya_protocol::Seq::Of(&[]),
    };
    r.p.sys.craft = Some(Box::new(reply(listing)));
    assert_eq!(r.p.read_dir(dir), Err(EIO));
    let token = proto::TokenReply {
        status: Below(ACCESS_DENIED),
    };
    r.p.sys.craft = Some(Box::new(reply(token)));
    assert_eq!(r.p.rename(b"f", b"g"), Err(EACCES));
    with_tmp(&mut r);
    let t = open(&mut r, "/tmp", o::TMPFILE | o::RDWR, 0o600).unwrap();
    let token = proto::TokenReply {
        status: Below(ACCESS_DENIED),
    };
    r.p.sys.craft = Some(Box::new(reply(token)));
    assert_eq!(r.p.publish(t, AT_FDCWD, b"/tmp/p"), Err(EACCES));
    // Ordinal 99, a status word ACCESS_DENIED: judged by the word.
    r.p.sys.craft = Some(Box::new(|t| raw_reply(99, t, &ACCESS_DENIED.to_le_bytes())));
    assert_eq!(r.p.fstat(fd), Err(EACCES));
    // The generic reply with a word too many does not decode: EINVAL.
    let mut body = ACCESS_DENIED.to_le_bytes().to_vec();
    body.extend_from_slice(&[0; 4]);
    r.p.sys.craft = Some(Box::new(move |t| raw_reply(0, t, &body)));
    assert_eq!(r.p.fstat(fd), Err(EINVAL));
}

/// An O_DSYNC write is durable: its connection holds SYNC as well as WRITE.
#[test]
fn a_dsync_write_is_durable() {
    let mut r = rig();
    let fd = open(&mut r, "f", o::WRONLY | o::CREAT | o::DSYNC, 0o644).unwrap();
    assert_eq!(r.write(fd, b"abcd"), Ok(4));
}

/// Refused paths are remembered up to a bound, whether their reports were
/// sent, dropped past 16, or could not be sent at all.
#[test]
fn refused_paths_are_remembered_up_to_a_bound() {
    for sendable in [true, false] {
        let mut r = rig_without_root();
        if !sendable {
            let far = r.reports;
            r.p.sys.close(far);
        }
        for i in 0..400 {
            let path = format!("/outside/{i}");
            assert_eq!(open(&mut r, &path, o::RDONLY, 0), Err(ENOENT));
        }
        assert_eq!(r.p.remembered(), process::REMEMBERED, "sendable {sendable}");
    }
}

/// ADR-0014 rule 7: `d_ino` is the object's id, as `st_ino` is: for every
/// entry, after a rename, and with names deleted and made again (a new
/// object, a new id).
#[test]
fn d_ino_is_st_ino_for_every_entry() {
    let mut r = rig();
    r.p.mkdir_at(AT_FDCWD, b"d", 0o755).unwrap();
    for name in ["a", "b", "Київ", "Plzeň"] {
        let fd = open(&mut r, &format!("d/{name}"), RW_CREATE, 0o644).unwrap();
        r.p.close(fd).unwrap();
    }
    r.p.mkdir_at(AT_FDCWD, b"d/sub", 0o755).unwrap();
    let listing = |r: &mut Rig| {
        let dir = open(r, "d", o::RDONLY, 0).unwrap();
        let mut out = vec![];
        while let Some((name, _, ino)) = r.p.read_dir(dir).unwrap() {
            out.push((name, ino));
        }
        r.p.close(dir).unwrap();
        out.sort();
        out
    };
    let check = |r: &mut Rig| {
        let names = listing(r);
        for (name, ino) in &names {
            let st =
                r.p.stat_at(
                    AT_FDCWD,
                    format!("d/{name}").as_bytes(),
                    AT_SYMLINK_NOFOLLOW,
                )
                .unwrap();
            assert_eq!(*ino, u64_at(&st, 8), "{name}");
        }
        names
    };
    let first = check(&mut r);
    assert_eq!(first.len(), 5);
    let id_of = |names: &[(String, u64)], n: &str| names.iter().find(|(x, _)| x == n).unwrap().1;
    r.p.rename(b"d/a", b"d/renamed").unwrap();
    let second = check(&mut r);
    assert_eq!(
        id_of(&second, "renamed"),
        id_of(&first, "a"),
        "a rename keeps the id"
    );
    r.p.remove(b"d/b").unwrap();
    let fd = open(&mut r, "d/b", RW_CREATE, 0o644).unwrap();
    r.p.close(fd).unwrap();
    let third = check(&mut r);
    assert_ne!(
        id_of(&third, "b"),
        id_of(&first, "b"),
        "a name made again is a new object"
    );
    let ids: std::collections::BTreeSet<u64> = third.iter().map(|(_, i)| *i).collect();
    assert_eq!(ids.len(), third.len(), "no two entries share an id");
}

/// An O_TRUNC whose truncation fails undoes the open: no fd, the
/// description closed, the errno returned. (The fake replaces the reply the
/// library reads; the service's truncation itself happened.)
#[test]
fn a_failed_truncation_undoes_the_open() {
    use proto::status::STORAGE_FAILED;
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    r.write(fd, b"keep").unwrap();
    r.p.close(fd).unwrap();
    r.settle();
    let (fds, handles) = (r.p.fds.count(), r.handles());
    let failed = proto::Done {
        status: Below(STORAGE_FAILED),
        count: 0,
    };
    r.p.sys.craft = Some(Box::new(reply(failed)));
    r.p.sys.craft_skip = 1;
    assert_eq!(open(&mut r, "f", o::WRONLY | o::TRUNC, 0), Err(EIO));
    r.settle();
    assert_eq!(
        (r.p.fds.count(), r.handles()),
        (fds, handles),
        "nothing left open"
    );
}

// ---- the survivors the main model rechecked (TASK-20 review, point 4) ----

/// The kernel handle of a pipe fd's write end.
fn write_end(r: &Rig, fd: i32) -> H {
    match r.p.fds.get(fd) {
        Some(process::Desc::Stream { write: Some(h), .. }) => *h,
        _ => panic!("fd {fd} is no write end"),
    }
}

/// A connection to the root with only `rights`, through `clone`.
fn narrowed(r: &mut Rig, rights: u32) -> H {
    let root = r.root;
    let (mine, theirs) = r.p.sys.channel_create().unwrap();
    let mut out = vec![0u8; 256];
    let m = proto::Clone {
        rights: zoraya_protocol::Flags(rights),
    };
    let n = encode(&m, 90, &mut out).unwrap();
    r.p.sys.channel_write(root, &out[..n], &[theirs]).unwrap();
    r.p.sys
        .wait(root, zoraya_posix::sys::signals::READABLE)
        .unwrap();
    let mut back = vec![0u8; 256];
    let mut handles = vec![];
    let n = r.p.sys.channel_read(root, &mut back, &mut handles).unwrap();
    let (_, d) = zoraya_protocol::decode::<proto::Done>(&back[..n], 0).unwrap();
    assert_eq!(d.status.0, proto::status::OK);
    mine
}

/// An open asks the rights its flags need and no more: a table entry that
/// gives only reading (posixrun's `:ro`) opens files to read, and one
/// without SYNC opens them to write; chdir asks READ.
#[test]
fn an_open_asks_only_the_rights_its_flags_need() {
    use proto::rights::*;
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    r.p.close(fd).unwrap();
    r.p.mkdir_at(AT_FDCWD, b"d", 0o755).unwrap();
    let ro = narrowed(&mut r, READ | CLONE);
    r.p.add_entry("/ro", EntryKind::Directory, Some(ro));
    assert!(open(&mut r, "/ro/f", o::RDONLY, 0).is_ok(), "no SYNC asked");
    assert_eq!(open(&mut r, "/ro/f", o::WRONLY, 0), Err(EACCES));
    let rw = narrowed(&mut r, READ | WRITE | CLONE);
    r.p.add_entry("/rw", EntryKind::Directory, Some(rw));
    assert!(open(&mut r, "/rw/f", o::RDWR, 0).is_ok(), "no SYNC asked");
    assert_eq!(open(&mut r, "/rw/f", o::RDWR | o::DSYNC, 0), Err(EACCES));
    let blind = narrowed(&mut r, CLONE);
    r.p.add_entry("/blind", EntryKind::Directory, Some(blind));
    assert_eq!(r.p.chdir(b"/blind/d"), Err(EACCES), "chdir asks READ");
    assert_eq!(r.p.chdir(b"/ro/d"), Ok(()));
}

/// A blocking read waits for data or an end: here a writer elsewhere puts
/// bytes in while it waits.
#[test]
fn a_blocking_read_takes_what_arrives_while_it_waits() {
    let mut r = rig();
    let (rd, _wr) = r.p.pipe(0).unwrap();
    let h = read_end(&r, rd);
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| n.feed(h, b"late")));
    assert_eq!(r.read(rd, 10).unwrap(), b"late");
}

/// A write that must wait before its first byte, whose reader then goes,
/// is EPIPE; a stream error before any byte is its errno, and after some
/// bytes the count.
#[test]
fn a_stream_write_ends_as_posix_says_whatever_stops_it() {
    let mut r = rig();
    let full = vec![1u8; STREAM_CAPACITY];
    let (rd, wr) = r.p.pipe(0).unwrap();
    assert_eq!(r.write(wr, &full), Ok(STREAM_CAPACITY));
    let h = read_end(&r, rd);
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| n.close(h)));
    assert_eq!(
        r.write(wr, b"x"),
        Err(EPIPE),
        "nothing written before the reader went"
    );
    r.p.sys.meanwhile = None;
    // The write end's handle gone from under the library: EBADF at once.
    let (_rd2, wr2) = r.p.pipe(0).unwrap();
    let w = write_end(&r, wr2);
    r.p.sys.close(w);
    assert_eq!(r.write(wr2, b"x"), Err(EBADF));
    // Gone after the first ring: the count written.
    let (rd3, wr3) = r.p.pipe(0).unwrap();
    let _keep = rd3;
    let w = write_end(&r, wr3);
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| n.close(w)));
    let big = vec![2u8; 2 * STREAM_CAPACITY];
    assert_eq!(r.write(wr3, &big), Ok(STREAM_CAPACITY));
}

#[test]
fn only_files_and_directories_are_objects() {
    let mut r = rig();
    let fd = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
    let dir = open(&mut r, "/", o::RDONLY, 0).unwrap();
    let (rd, wr) = r.p.pipe(0).unwrap();
    let null = open(&mut r, "/dev/null", o::RDWR, 0).unwrap();
    assert!(r.p.is_object(fd) && r.p.is_object(dir));
    assert!(!r.p.is_object(rd) && !r.p.is_object(wr) && !r.p.is_object(null));
    assert!(!r.p.is_object(99));
}

/// INV-0106-2's host half: the file paths take no request, and a library
/// entry polls only once a handler is installed or a pending signal is let
/// through.
#[test]
fn no_request_is_taken_on_the_file_paths_and_polls_wait_for_a_handler() {
    use zoraya_posix::signals::{Action, SIG_BLOCK, SIG_UNBLOCK, bit};
    let mut r = rig();
    assert!(!r.p.should_poll());
    for _ in 0..5 {
        let fd = open(&mut r, "/f.txt", o::RDWR | o::CREAT, 0o644).unwrap();
        assert_eq!(r.write(fd, b"data"), Ok(4));
        assert_eq!(r.p.lseek(fd, 0, 0), Ok(0));
        assert_eq!(r.read(fd, 4).unwrap(), b"data");
        r.p.fstat(fd).unwrap();
        r.p.close(fd).unwrap();
        let (a, b) = r.p.pipe(0).unwrap();
        r.p.close(a).unwrap();
        r.p.close(b).unwrap();
    }
    assert_eq!(r.p.stats.takes, 0);
    assert_eq!(r.p.stats.broker, 0, "INV-0104-5: no broker message either");
    assert!(!r.p.should_poll());
    // An ignored action is no reason to poll; a handler is.
    r.p.signals.set(10, Some(Action::IGNORE)).unwrap();
    assert!(!r.p.should_poll());
    let handler = Action {
        handler: 0x4000,
        mask: 0,
        flags: 0,
    };
    r.p.signals.set(10, Some(handler)).unwrap();
    assert!(r.p.should_poll());
    r.p.signals.set(10, Some(Action::DEFAULT)).unwrap();
    // A pending signal polls once its block is lifted, not before.
    r.p.signals.mask(SIG_BLOCK, Some(bit(15))).unwrap();
    r.p.signals.pending |= bit(15);
    assert!(!r.p.should_poll());
    r.p.signals.mask(SIG_UNBLOCK, Some(bit(15))).unwrap();
    assert!(r.p.should_poll());
}

/// `fsync` (ADR-0210 item 3): a file's fd succeeds, written or not;
/// a pipe is `EINVAL`, a closed fd `EBADF`.
#[test]
fn fsync_takes_a_file_and_refuses_a_pipe() {
    let mut r = rig();
    let fd = open(&mut r, "synced", RW_CREATE, 0o644).unwrap();
    let before = r.p.stats.requests;
    assert_eq!(r.p.fsync(fd), Ok(()), "nothing written yet");
    assert_eq!(
        r.p.stats.requests, before,
        "no request for an fd never written"
    );
    assert_eq!(r.write(fd, b"data"), Ok(4));
    let before = r.p.stats.requests;
    assert_eq!(
        r.p.fsync(fd),
        Ok(()),
        "written through: the status is asked"
    );
    assert_eq!(r.p.stats.requests, before + 1, "one status request");
    let (rd, wr) = r.p.pipe(0).unwrap();
    assert_eq!(r.p.fsync(rd), Err(EINVAL));
    assert_eq!(r.p.fsync(wr), Err(EINVAL));
    r.p.close(fd).unwrap();
    assert_eq!(r.p.fsync(fd), Err(EBADF));
}

/// ADR-0110 item 7: every counter the broker names comes from a field of
/// its own, in the protocol's order; the call log packs to bits.
#[test]
fn every_reported_counter_has_its_own_source() {
    use zoraya_posix::process::{Stats, pack_calls};
    use zoraya_proto_posix::stats::NAMES;
    let s = Stats {
        resolutions: 1,
        resolve_ns: 2,
        shares: 3,
        seeks: 4,
        requests: 5,
        reports: 6,
        broker: 7,
        takes: 8,
        prepares: 9,
        prepare_ns: 10,
        commits: 11,
        commit_ns: 12,
        reaps: 13,
        reap_ns: 14,
        forks: 15,
        fork_ns: 16,
        snapshot_ns: 17,
        waits: 18,
        execs: 19,
        exec_ns: 20,
        spawns: 21,
        spawn_ns: 22,
        max_spawn_buffer: 23,
        calls_list: 24,
        inherit_failed_at: 0,
    };
    let values: Vec<u64> = NAMES.iter().map(|n| s.counter(n, 100, 200)).collect();
    let mut seen = values.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), NAMES.len(), "no two names share a source");
    assert!(!values.contains(&0), "every name is known: {values:?}");
    assert_eq!(s.counter("syscalls", 100, 200), 100);
    assert_eq!(s.counter("max_fds", 100, 200), 200);
    assert_eq!(s.counter("unknown", 100, 200), 0);
    let bytes = s.encode(NAMES, 100, 200);
    assert_eq!(bytes.len(), NAMES.len() * 8);
    assert_eq!(u64_at(&bytes, 8), s.takes, "the second name's value");
    assert_eq!(pack_calls(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 7]), [0x01, 0x02]);
    assert_eq!(pack_calls(&[]), Vec::<u8>::new());
    assert_eq!(pack_calls(&[0, 1, 1]), [0x06]);
}

/// What the C boundary's `state` does (ADR-0302 item 9): runs `f`; a wait it
/// asks for is made here, between two runs, with nothing borrowed from the
/// process; `f` runs again with the outcome.
fn run<R>(
    r: &mut Rig,
    resume: Option<zoraya_posix::jobs::Resume<H>>,
    f: &mut impl FnMut(&mut Rig) -> Result<R, i32>,
) -> (Result<R, i32>, Option<zoraya_posix::jobs::PendingWait<H>>) {
    r.p.set_resume(resume);
    let out = f(r);
    let w = r.p.end_run(&out);
    (out, w)
}

fn deferred<R>(
    r: &mut Rig,
    mut f: impl FnMut(&mut Rig) -> Result<R, i32>,
    mut waited: impl FnMut(&mut Rig),
) -> Result<R, i32> {
    use zoraya_posix::jobs::Resume;
    let mut resume = None;
    loop {
        let (out, w) = run(r, resume.take(), &mut f);
        let Some(w) = w else { return out };
        waited(r);
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

#[test]
fn a_deferred_write_resumes_where_it_stopped() {
    let mut r = rig();
    r.p.defer_waits = true;
    let big = vec![5u8; 2 * STREAM_CAPACITY];
    let (brd, bwr) = r.p.pipe(0).unwrap();
    let brd = read_end(&r, brd);
    let drained = Rc::new(std::cell::Cell::new(0usize));
    let d = drained.clone();
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| {
        d.set(d.get() + n.drain(brd, usize::MAX));
    }));
    let u = r.poke(0, &big);
    let n = deferred(&mut r, |r| r.p.write(bwr, u), |_| {});
    assert_eq!(n, Ok(2 * STREAM_CAPACITY));
    // Nothing written twice: what the reader took is what the writer did,
    // less what still sits in the ring.
    r.p.sys.meanwhile = None;
    let rest = r.p.read(
        {
            // The read end's descriptor is 3 less than the write end's.
            bwr - 1
        },
        r.arena.user(ARENA / 2, 2 * STREAM_CAPACITY),
    );
    let left = rest.unwrap_or(0);
    assert_eq!(drained.get() + left, 2 * STREAM_CAPACITY);
}

#[test]
fn a_deferred_read_and_sleep_get_their_outcomes() {
    let mut r = rig();
    r.p.defer_waits = true;
    let (rd, _wr) = r.p.pipe(0).unwrap();
    let h = read_end(&r, rd);
    // A reader waits; the data comes while it is out of the library.
    let got = deferred(
        &mut r,
        |r| r.read(rd, 4),
        |r| r.net.borrow_mut().feed(h, b"abcd"),
    );
    assert_eq!(got, Ok(b"abcd".to_vec()));
    // A sleep: no sources, a deadline: the second run sees the deadline pass.
    let slept = deferred(&mut r, |r| r.p.wait_for(&[], Some(1)), |_| {});
    assert_eq!(slept, Ok((None, None)));
}

// ------------------------------------------------------- record locks (ADR-0319)

mod record_locks {
    use super::*;
    use zoraya_posix::records::{
        F_GETLK, F_OFD_GETLK, F_OFD_SETLK, F_OFD_SETLKW, F_RDLCK, F_SETLK, F_SETLKW, F_UNLCK,
        F_WRLCK, FLOCK_SIZE,
    };

    fn flock(kind: i16, whence: i16, start: i64, len: i64) -> [u8; FLOCK_SIZE] {
        let mut b = [0u8; FLOCK_SIZE];
        b[0..2].copy_from_slice(&kind.to_le_bytes());
        b[2..4].copy_from_slice(&whence.to_le_bytes());
        b[8..16].copy_from_slice(&start.to_le_bytes());
        b[16..24].copy_from_slice(&len.to_le_bytes());
        b
    }

    fn ty(b: &[u8; FLOCK_SIZE]) -> i16 {
        i16::from_le_bytes([b[0], b[1]])
    }

    fn lk(
        r: &mut Rig,
        fd: i32,
        cmd: i32,
        mut b: [u8; FLOCK_SIZE],
    ) -> Result<[u8; FLOCK_SIZE], i32> {
        r.p.fcntl_lock(fd, cmd, &mut b).map(|()| b)
    }

    /// Two opens of one file: the locks of one process do not conflict with
    /// each other, those of two descriptions (OFD) do.
    #[test]
    fn ofd_locks_conflict_by_description_and_posix_locks_by_process() {
        let mut r = rig();
        let a = open(&mut r, "db", RW_CREATE, 0o644).unwrap();
        let b = open(&mut r, "db", o::RDWR, 0).unwrap();
        // POSIX locks: one process, never in its own way.
        lk(&mut r, a, F_SETLK, flock(F_WRLCK, 0, 0, 10)).unwrap();
        lk(&mut r, b, F_SETLK, flock(F_WRLCK, 0, 5, 10)).unwrap();
        let g = lk(&mut r, b, F_GETLK, flock(F_WRLCK, 0, 0, 100)).unwrap();
        assert_eq!(ty(&g), F_UNLCK, "its own locks are not reported");
        lk(&mut r, a, F_SETLK, flock(F_UNLCK, 0, 0, 0)).unwrap();
        // OFD locks: a description conflicts with another's, and with the
        // process's POSIX lock.
        lk(&mut r, a, F_OFD_SETLK, flock(F_WRLCK, 0, 0, 10)).unwrap();
        assert_eq!(
            lk(&mut r, b, F_OFD_SETLK, flock(F_RDLCK, 0, 9, 1)),
            Err(EAGAIN)
        );
        lk(&mut r, b, F_OFD_SETLK, flock(F_RDLCK, 0, 10, 1)).unwrap();
        let g = lk(&mut r, b, F_OFD_GETLK, flock(F_WRLCK, 0, 0, 100)).unwrap();
        assert_eq!(ty(&g), F_WRLCK, "the OFD write lock of `a`");
        assert_eq!(i64::from_le_bytes(g[8..16].try_into().unwrap()), 0);
        assert_eq!(i64::from_le_bytes(g[16..24].try_into().unwrap()), 10);
        assert_eq!(i32::from_le_bytes(g[24..28].try_into().unwrap()), -1);
        // A pid with an OFD command is refused.
        let mut bad = flock(F_RDLCK, 0, 0, 1);
        bad[24] = 1;
        assert_eq!(lk(&mut r, a, F_OFD_SETLK, bad), Err(EINVAL));
        // A POSIX lock over what `b` holds: refused with EAGAIN; the wait
        // command is the same call (the caller sleeps).
        assert_eq!(
            lk(&mut r, a, F_SETLK, flock(F_WRLCK, 0, 10, 1)),
            Err(EAGAIN)
        );
        assert_eq!(
            lk(&mut r, a, F_SETLKW, flock(F_WRLCK, 0, 10, 1)),
            Err(EAGAIN)
        );
        // F_GETLK shows a POSIX lock's holder: this process.
        r.p.close(a).unwrap();
        lk(&mut r, b, F_SETLK, flock(F_WRLCK, 0, 50, 0)).unwrap();
        let c = open(&mut r, "db", o::RDWR, 0).unwrap();
        let g = lk(&mut r, c, F_OFD_GETLK, flock(F_RDLCK, 0, 60, 1)).unwrap();
        assert_eq!(ty(&g), F_WRLCK);
        assert_eq!(
            i64::from_le_bytes(g[16..24].try_into().unwrap()),
            0,
            "to EOF"
        );
        assert_eq!(
            i32::from_le_bytes(g[24..28].try_into().unwrap()),
            r.p.facts.pid as i32
        );
    }

    /// POSIX: closing any fd of a file drops all the process's locks on it;
    /// closing the last fd of a description drops its OFD locks, and a `dup`
    /// keeps them.
    #[test]
    fn close_releases() {
        let mut r = rig();
        let a = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
        let b = open(&mut r, "f", o::RDWR, 0).unwrap();
        let other = open(&mut r, "other", RW_CREATE, 0o644).unwrap();
        lk(&mut r, a, F_SETLK, flock(F_WRLCK, 0, 0, 0)).unwrap();
        lk(&mut r, other, F_SETLK, flock(F_WRLCK, 0, 0, 0)).unwrap();
        let probe = open(&mut r, "f", o::RDWR, 0).unwrap();
        let held = |r: &mut Rig| lk(r, probe, F_OFD_SETLK, flock(F_RDLCK, 0, 0, 1));
        assert_eq!(held(&mut r), Err(EAGAIN));
        r.p.close(b).unwrap(); // not the fd that locked
        assert!(held(&mut r).is_ok(), "the lock is on the file, not the fd");
        r.p.close(probe).unwrap();
        // The lock of `other` is untouched by that.
        let p2 = open(&mut r, "other", o::RDWR, 0).unwrap();
        assert_eq!(
            lk(&mut r, p2, F_OFD_SETLK, flock(F_RDLCK, 0, 0, 1)),
            Err(EAGAIN)
        );
        // OFD: a dup keeps the lock while one fd is open.
        let d = open(&mut r, "g", RW_CREATE, 0o644).unwrap();
        let d2 = r.p.dup(d).unwrap();
        lk(&mut r, d, F_OFD_SETLK, flock(F_WRLCK, 0, 0, 0)).unwrap();
        let q = open(&mut r, "g", o::RDWR, 0).unwrap();
        r.p.close(d).unwrap();
        assert_eq!(
            lk(&mut r, q, F_OFD_SETLK, flock(F_RDLCK, 0, 0, 1)),
            Err(EAGAIN)
        );
        r.p.close(d2).unwrap();
        lk(&mut r, q, F_OFD_SETLK, flock(F_RDLCK, 0, 0, 1)).unwrap();
        // dup2 over an fd that locked ends the process's locks on its file.
        let e = open(&mut r, "h", RW_CREATE, 0o644).unwrap();
        lk(&mut r, e, F_SETLK, flock(F_WRLCK, 0, 0, 0)).unwrap();
        let hp = open(&mut r, "h", o::RDWR, 0).unwrap();
        assert_eq!(
            lk(&mut r, hp, F_OFD_SETLK, flock(F_RDLCK, 0, 0, 1)),
            Err(EAGAIN)
        );
        r.p.dup3(q, e, 0, false).unwrap();
        lk(&mut r, hp, F_OFD_SETLK, flock(F_RDLCK, 0, 0, 1)).unwrap();
    }

    #[test]
    fn arguments_and_whence() {
        let mut r = rig();
        let ro = open(&mut r, "w", RW_CREATE, 0o644).unwrap();
        r.write(ro, b"0123456789").unwrap();
        let rd = open(&mut r, "w", o::RDONLY, 0).unwrap();
        let wr = open(&mut r, "w", o::WRONLY, 0).unwrap();
        assert_eq!(lk(&mut r, rd, F_SETLK, flock(F_WRLCK, 0, 0, 1)), Err(EBADF));
        assert_eq!(lk(&mut r, wr, F_SETLK, flock(F_RDLCK, 0, 0, 1)), Err(EBADF));
        lk(&mut r, rd, F_SETLK, flock(F_RDLCK, 0, 0, 1)).unwrap();
        assert_eq!(lk(&mut r, rd, F_SETLK, flock(5, 0, 0, 1)), Err(EINVAL));
        assert_eq!(
            lk(&mut r, rd, F_SETLK, flock(F_RDLCK, 3, 0, 1)),
            Err(EINVAL)
        );
        assert_eq!(
            lk(&mut r, rd, F_SETLK, flock(F_RDLCK, 0, -1, 1)),
            Err(EINVAL)
        );
        assert_eq!(lk(&mut r, 99, F_SETLK, flock(F_RDLCK, 0, 0, 1)), Err(EBADF));
        // SEEK_END counts from the size, SEEK_CUR from the position.
        let probe = open(&mut r, "w", o::RDWR, 0).unwrap();
        lk(&mut r, ro, F_OFD_SETLK, flock(F_WRLCK, 2, -2, 2)).unwrap(); // bytes 8..10
        assert_eq!(
            lk(&mut r, probe, F_OFD_SETLK, flock(F_RDLCK, 0, 9, 1)),
            Err(EAGAIN)
        );
        lk(&mut r, probe, F_OFD_SETLK, flock(F_RDLCK, 0, 7, 1)).unwrap();
        r.p.lseek(ro, 4, 0).unwrap();
        lk(&mut r, ro, F_OFD_SETLK, flock(F_UNLCK, 1, 4, 0)).unwrap(); // from 8 on
        lk(&mut r, probe, F_OFD_SETLK, flock(F_RDLCK, 0, 9, 1)).unwrap();
        // Not a file: a pipe is EINVAL.
        let (p0, _p1) = r.p.pipe(0).unwrap();
        assert_eq!(
            lk(&mut r, p0, F_SETLK, flock(F_RDLCK, 0, 0, 1)),
            Err(EINVAL)
        );
        // The ordinary fcntl does not take the lock commands.
        assert_eq!(r.p.fcntl(rd, F_SETLK, 0), Err(EINVAL));
        let _ = F_OFD_SETLKW;
    }

    /// A file's locks are those of the process that holds them: a table of a
    /// new `Posix` (a `fork` child's) is empty.
    #[test]
    fn a_new_process_starts_with_no_locks() {
        let mut r = rig();
        let a = open(&mut r, "f", RW_CREATE, 0o644).unwrap();
        lk(&mut r, a, F_OFD_SETLK, flock(F_WRLCK, 0, 0, 0)).unwrap();
        assert!(!r.p.locks_is_empty());
        let r2 = rig();
        assert!(r2.p.locks_is_empty());
    }
}

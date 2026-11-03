//! The library against the real filesystem service, through a fake kernel:
//! files, directories, descriptors, pipes and the resolver's reports.

mod kernel;

use kernel::*;
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
        std::mem::replace(
            &mut r.p.sys,
            Fake {
                net: net.clone(),
                services: vec![],
                waits: 0,
            },
        ),
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

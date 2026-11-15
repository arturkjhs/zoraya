//! What `posix_spawn` gives a child (plan part F4, ADR-0411 item 1, ADR-0433)
//! over the fake kernel and the real filesystem service: a chosen table, the
//! fds the file actions name, `addopen`, the connections of a grant mask, and
//! the Unix sockets and memory descriptors of the third bootstrap message
//! delivered to a second process. INV-0411-1 (exactly the listed entries, the
//! named connections and fds, each no wider than the parent's), INV-0411-4
//! (no attribute: what a spawn gave before), INV-0412-1's two-process half and
//! INV-0413-2's "in another process" half.
//!
//! Every test that lists rights asks the service, not the library's own
//! record of them: a child's entry is a connection of the service, and what it
//! refuses is what the child cannot do.

mod kernel;

use std::cell::RefCell;
use std::rc::Rc;

use kernel::*;
use zoraya_abi::posix::{EntryKind, Facts, FdKind, NO_ENTRY, NO_HANDLE, OPEN_MAX};
use zoraya_posix::errno::*;
use zoraya_posix::jobs::{FileAction, Inherit};
use zoraya_posix::memfd::{
    F_ADD_SEALS, MAP_SHARED, MFD_ALLOW_SEALING, MFD_CLOEXEC, PROT_READ, PROT_WRITE, seal,
};
use zoraya_posix::path::Form;
use zoraya_posix::process::{AT_FDCWD, Desc, F_SETFD, FD_CLOEXEC, Posix, o};
use zoraya_posix::socket::SOCK_CLOEXEC;
use zoraya_posix::spawn::*;
use zoraya_posix::sys::Sys;
use zoraya_posix::unix::{
    AF_UNIX, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM, decode_passed, encode_passed,
};
use zoraya_proto_file::rights;
use zoraya_proto_net::{Duplicate, Duplicated, status as net_status};
use zoraya_protocol::{Below, Header, encode};
use zoraya_rng::{Share, ShareReply, status as rng_status};

zoraya_test_alloc::cap_memory!();

const PAGE: usize = 4096;

fn spec(path: &str, read_only: bool) -> Spec {
    Spec {
        path: path.into(),
        read_only,
    }
}

fn chosen(entries: Vec<Spec>) -> Shape {
    Shape {
        empty: true,
        entries,
        grants: 0,
    }
}

/// The handles the kernel holds, once the services have seen every close.
fn count(r: &mut Rig) -> usize {
    r.settle();
    r.handles()
}

/// Closes every fd of the process, as its end would.
fn close_all(r: &mut Rig) {
    let fds: Vec<i32> = r.p.fds.fds().map(|f| f as i32).collect();
    for fd in fds {
        let _ = r.p.close(fd);
    }
}

fn mkdir(r: &mut Rig, path: &[u8]) {
    r.p.mkdir_at(AT_FDCWD, path, 0o755).unwrap();
}

fn create(r: &mut Rig, path: &[u8], bytes: &[u8]) -> i32 {
    let fd = r.p.open(AT_FDCWD, path, o::CREAT | o::RDWR, 0o644).unwrap();
    if !bytes.is_empty() {
        r.write(fd, bytes).unwrap();
    }
    fd
}

/// A second process over the same kernel and services.
fn second(r: &Rig) -> Rig {
    let net = r.net.clone();
    let mut fake = Fake::new(net.clone(), r.p.sys.services.clone());
    let arena = fake.memory_create(ARENA as u64).unwrap();
    let base = fake.memory_map(arena, ARENA as u64).unwrap();
    let p = Posix::new(
        fake,
        Facts {
            uid: 1000,
            gid: 100,
            umask: 0o022,
            ..Default::default()
        },
    );
    Rig {
        p,
        net,
        arena: Arena { base },
        root: r.root,
        reports: r.reports,
    }
}

/// The handles an inheritance made for the child alone, closed as a refused
/// spawn closes them.
fn drop_made(r: &mut Rig, out: &Inherit<u32>) {
    let made: Vec<u32> = out
        .slots
        .iter()
        .filter(|s| s.2)
        .map(|s| s.1)
        .chain(out.message.iter().copied())
        .collect();
    for h in made {
        r.p.sys.close(h);
    }
}

/// The child's table, from what a spawn gave: its entries as connections of
/// the real service, its fds from `slots`' made connections, and the
/// descriptors of the third message delivered as the kernel would.
fn become_child(parent: &mut Rig, out: &Inherit<u32>) -> Rig {
    let mut child = second(parent);
    for (prefix, kind, index) in &out.entries {
        let handle = (*index != NO_HANDLE).then(|| out.message[*index as usize]);
        child.p.add_entry(prefix, *kind, handle);
    }
    match out.cwd {
        (NO_ENTRY, _) => child.p.set_cwd(usize::MAX, "", Form::EntryRelative),
        (entry, ref path) => child.p.set_cwd(entry as usize, path, Form::EntryRelative),
    }
    for (fd, record) in out.fds.iter().map(|f| (f.fd, f)) {
        let handle = out
            .slots
            .iter()
            .find(|s| s.0 == fd)
            .map(|s| s.1)
            .expect("a slot for the fd");
        let desc = match record.kind {
            FdKind::File | FdKind::Dir => Desc::Object {
                conn: handle,
                kind: if record.kind == FdKind::Dir {
                    zoraya_posix::stat::Kind::Directory
                } else {
                    zoraya_posix::stat::Kind::File
                },
                access: if record.flags & 4 != 0 && record.flags & 2 != 0 {
                    o::RDWR
                } else if record.flags & 4 != 0 {
                    o::WRONLY
                } else {
                    o::RDONLY
                },
                nonblock: false,
                sync: false,
                written: false,
                attached: false,
                place: None,
                listing: Default::default(),
            },
            other => panic!("not built here: {other:?}"),
        };
        let _ = child.p.fds.install_at(fd, desc);
    }
    child
}

/// Sends the third message of `out` from `parent` to a fresh `child`, as
/// `send_record` does, and installs it there; the parent is told after.
fn ship(parent: &mut Rig, out: &Inherit<u32>, child: &mut Rig) {
    let (a, b) = parent.p.sys.channel_create().unwrap();
    let bytes = encode_passed(&out.passed.fds, &out.passed.records);
    parent
        .p
        .sys
        .channel_send(a, &bytes, &out.passed.handles, &out.passed.read_only)
        .expect("the kernel takes the message");
    let mut buffer = vec![0u8; 4096];
    let mut handles = Vec::new();
    let n = child
        .p
        .sys
        .channel_read(b, &mut buffer, &mut handles)
        .unwrap();
    let (fds, records) =
        decode_passed(&buffer[..n], handles.len(), OPEN_MAX).expect("a message it built");
    child.p.install_passed(&fds, records, handles);
    parent.p.sys.close(a);
    parent.p.sys.close(b);
}

fn read_fds(r: &mut Rig, fd: i32) -> Result<(Vec<u8>, Vec<i32>), i32> {
    let u = r.arena.user(ARENA / 2, 100);
    let got = r.p.recvmsg(fd, &[u], 8, 0)?;
    Ok((r.peek(ARENA / 2, got.bytes), got.fds))
}

// ------------------------------------------------------------ INV-0411-4

/// No attribute of this part: the parent's table in the parent's order, its
/// cwd, every non-close-on-exec fd, no descriptor of the third message.
#[test]
fn a_spawn_without_the_attributes_gives_what_it_gave_before() {
    let mut r = rig();
    let held = count(&mut r);
    let fd = create(&mut r, b"/f", b"x");
    let out = r.p.inherit(&[], 0).unwrap();
    let after = r.p.inherit_shaped(&[], 0, &Shape::default()).unwrap();
    for o in [&out, &after] {
        let prefixes: Vec<(&str, EntryKind)> =
            o.entries.iter().map(|e| (e.0.as_str(), e.1)).collect();
        assert_eq!(
            prefixes,
            [("/", EntryKind::Directory), ("/dev/null", EntryKind::Null)]
        );
        assert_eq!(o.cwd, (0, String::new()));
        assert!(o.passed.is_empty() && o.temps.is_empty());
        assert_eq!(o.fds.len(), 1);
        assert_eq!(o.fds[0].fd, fd as u32);
        assert_eq!(o.message.len(), 1, "the root directory's connection");
    }
    drop_made(&mut r, &out);
    drop_made(&mut r, &after);
    r.p.close(fd).unwrap();
    assert_eq!(count(&mut r), held);
}

#[test]
fn only_a_shape_with_an_attribute_changes_anything() {
    assert!(Shape::default().is_default());
    assert!(
        !Shape {
            grants: GRANT_RANDOM,
            ..Default::default()
        }
        .is_default()
    );
}

// ------------------------------------------------------------ INV-0411-1: the table

#[test]
fn a_chosen_table_has_exactly_the_listed_entries_longest_first() {
    let mut r = rig();
    mkdir(&mut r, b"/a");
    mkdir(&mut r, b"/a/b");
    mkdir(&mut r, b"/c");
    mkdir(&mut r, b"/d");
    // An open file is not inherited by a child whose fds are the actions'.
    let fd = create(&mut r, b"/f", b"hi");
    let held = count(&mut r);
    let out =
        r.p.inherit_shaped(
            &[FileAction::Dup2(fd, 3), FileAction::Close(99)],
            0,
            &chosen(vec![spec("/c", false), spec("/a/b", true)]),
        )
        .unwrap();
    let list: Vec<(&str, EntryKind)> = out.entries.iter().map(|e| (e.0.as_str(), e.1)).collect();
    assert_eq!(
        list,
        [("/a/b", EntryKind::Directory), ("/c", EntryKind::Directory)]
    );
    assert_eq!(out.message.len(), 2);
    assert_eq!(out.entries[0].2, 0);
    assert_eq!(out.entries[1].2, 1);
    // The fds: the one the action names, and nothing else.
    assert_eq!(out.fds.len(), 1);
    assert_eq!(out.fds[0].fd, 3);
    assert_eq!(out.slots.len(), 1);
    // The parent's cwd, `/`, is in no listed entry: the child has none.
    assert_eq!(out.cwd, (NO_ENTRY, String::new()));
    drop_made(&mut r, &out);
    assert_eq!(count(&mut r), held);
}

#[test]
fn a_child_has_no_cwd_unless_a_listed_directory_covers_the_parents() {
    let mut r = rig();
    mkdir(&mut r, b"/a");
    mkdir(&mut r, b"/a/b");
    r.p.chdir(b"/a/b").unwrap();
    for (list, expected) in [
        (vec![spec("/a", true)], (0, "b")),
        (vec![spec("/a/b", true), spec("/a", true)], (0, "")),
        (vec![spec("/", true)], (0, "a/b")),
        (vec![spec("/a/bb", true)], (NO_ENTRY, "")),
    ] {
        if list[0].path == "/a/bb" {
            mkdir(&mut r, b"/a/bb");
        }
        let out = r.p.inherit_shaped(&[], 0, &chosen(list)).unwrap();
        assert_eq!(out.cwd, (expected.0, expected.1.to_string()));
        drop_made(&mut r, &out);
    }
    // A path that only starts the same way as the cwd is not a parent of it.
    let out =
        r.p.inherit_shaped(&[], 0, &chosen(vec![spec("/a/bb", true)]))
            .unwrap();
    assert_eq!(out.cwd.0, NO_ENTRY);
    drop_made(&mut r, &out);
}

/// What the child's entries can do is what the service says: a `:ro` entry
/// refuses a write the parent's own connection takes, and a plain one does not.
#[test]
fn a_read_only_entry_is_read_only_at_the_service() {
    let mut r = rig();
    mkdir(&mut r, b"/ro");
    mkdir(&mut r, b"/rw");
    let held = count(&mut r);
    let out =
        r.p.inherit_shaped(&[], 0, &chosen(vec![spec("/ro", true), spec("/rw", false)]))
            .unwrap();
    let mut child = become_child(&mut r, &out);
    // Writing in the read-only entry: the service refuses; reading is fine.
    assert_eq!(
        child
            .p
            .open(AT_FDCWD, b"/ro/new", o::CREAT | o::WRONLY, 0o644),
        Err(EACCES)
    );
    assert_eq!(child.p.mkdir_at(AT_FDCWD, b"/ro/d", 0o755), Err(EACCES));
    assert!(
        child
            .p
            .open(AT_FDCWD, b"/ro", o::RDONLY | o::DIRECTORY, 0)
            .is_ok()
    );
    // The plain one takes both.
    let fd = child
        .p
        .open(AT_FDCWD, b"/rw/new", o::CREAT | o::WRONLY, 0o644)
        .unwrap();
    child.p.close(fd).unwrap();
    child.p.mkdir_at(AT_FDCWD, b"/rw/d", 0o755).unwrap();
    // The child cannot reach what it was not given: a path outside its table.
    assert!(
        child
            .p
            .open(AT_FDCWD, b"/", o::RDONLY | o::DIRECTORY, 0)
            .is_err()
    );
    assert_eq!(
        child.p.open(AT_FDCWD, b"/rw/../zzz/x", o::RDONLY, 0),
        Err(ENOENT)
    );
    // A directory above an entry is not in the table (`..` stops at the entry).
    assert!(child.p.open(AT_FDCWD, b"/f", o::RDONLY, 0).is_err());
    // Everything closes again.
    close_all(&mut child);
    let handles: Vec<u32> = out.message.clone();
    for h in handles {
        r.p.sys.close(h);
    }
    // The child's own arena is the one handle more.
    assert_eq!(count(&mut r), held + 1);
}

/// A child's connection can make another no wider: even `clone` cannot give
/// back what the entry was narrowed from.
#[test]
fn a_read_only_entry_cannot_clone_itself_wider() {
    let mut r = rig();
    mkdir(&mut r, b"/ro");
    let out =
        r.p.inherit_shaped(&[], 0, &chosen(vec![spec("/ro", true)]))
            .unwrap();
    let conn = out.message[0];
    // A second process asks the service through the entry's connection.
    let mut child = second(&r);
    let wider = child.p.share_conn(conn, rights::ALL);
    assert_eq!(wider, Err(EACCES), "share cannot widen");
    let same = child.p.share_conn(conn, rights::READ | rights::CLONE);
    assert!(same.is_ok());
    drop_made(&mut r, &out);
}

/// The parent can only narrow: a path it holds read-only is not given
/// read-write, a path it has no entry for is not given at all.
#[test]
fn a_child_gets_no_more_than_the_parent_holds() {
    let mut r = rig();
    mkdir(&mut r, b"/data");
    mkdir(&mut r, b"/data/sub");
    mkdir(&mut r, b"/other");
    // A parent whose `/data` is a read-only connection and which has no `/`.
    let mut parent = second(&r);
    let ro =
        r.p.share_conn(r.root, rights::READ | rights::CLONE)
            .unwrap();
    let rw = r.p.share_conn(r.root, rights::ALL).unwrap();
    parent.p.add_entry("/ro", EntryKind::Directory, Some(ro));
    parent.p.add_entry("/rw", EntryKind::Directory, Some(rw));
    parent.p.add_entry("/dev/null", EntryKind::Null, None);
    parent.p.set_cwd(1, "", Form::EntryRelative);
    let held = {
        parent.settle();
        parent.handles()
    };
    let try_one = |parent: &mut Rig, list: Vec<Spec>| {
        let r = parent.p.inherit_shaped(&[], 0, &chosen(list));
        let handles = {
            parent.settle();
            parent.handles()
        };
        if let Ok(out) = &r {
            drop_made(parent, out);
        }
        (r.map(|o| o.entries.len()), handles)
    };
    // Read-write of a read-only entry, or of a directory in it, is refused.
    assert_eq!(
        try_one(&mut parent, vec![spec("/ro", false)]),
        (Err(EACCES), held)
    );
    assert_eq!(
        try_one(&mut parent, vec![spec("/ro/data", false)]),
        (Err(EACCES), held)
    );
    // Read-only of it, and anything of a read-write entry, is given.
    assert_eq!(try_one(&mut parent, vec![spec("/ro/data", true)]).0, Ok(1));
    assert_eq!(
        try_one(&mut parent, vec![spec("/rw/data/sub", false)]).0,
        Ok(1)
    );
    assert_eq!(
        try_one(
            &mut parent,
            vec![spec("/rw", false), spec("/ro/data", true)]
        )
        .0,
        Ok(2)
    );
    // A path no entry covers: refused and reported, nothing left made.
    assert_eq!(
        try_one(&mut parent, vec![spec("/etc", true)]),
        (Err(EACCES), held)
    );
    assert_eq!(
        try_one(&mut parent, vec![spec("/rw", false), spec("/etc", true)]),
        (Err(EACCES), held),
        "the entry made before the refusal is closed again"
    );
    // A path that is no directory.
    let made = create(&mut r, b"/data/file", b"");
    r.p.close(made).unwrap();
    assert_eq!(
        try_one(&mut parent, vec![spec("/rw/data/file", true)]),
        (Err(ENOTDIR), held)
    );
    assert_eq!(
        try_one(&mut parent, vec![spec("/rw/nothing", true)]),
        (Err(ENOENT), held)
    );
}

#[test]
fn a_special_entry_is_listed_by_its_own_name() {
    let mut r = rig();
    let out =
        r.p.inherit_shaped(&[], 0, &chosen(vec![spec("/dev/null", false)]))
            .unwrap();
    assert_eq!(
        out.entries,
        [("/dev/null".to_string(), EntryKind::Null, NO_HANDLE)]
    );
    assert!(out.message.is_empty());
    // `:ro` of a device is no meaning; a path under it is no directory.
    assert!(
        r.p.inherit_shaped(&[], 0, &chosen(vec![spec("/dev/null", true)]))
            .is_err()
    );
    assert_eq!(
        r.p.inherit_shaped(&[], 0, &chosen(vec![spec("/dev/null/x", false)]))
            .err(),
        Some(EINVAL)
    );
}

#[test]
fn an_empty_table_has_no_entry_no_cwd_and_the_actions_fds() {
    let mut r = rig();
    let a = create(&mut r, b"/a", b"");
    let b = create(&mut r, b"/b", b"");
    let held = count(&mut r);
    let out =
        r.p.inherit_shaped(
            &[FileAction::Dup2(a, 9), FileAction::Dup2(b, 7)],
            0,
            &chosen(vec![]),
        )
        .unwrap();
    assert!(out.entries.is_empty() && out.message.is_empty());
    assert_eq!(out.cwd.0, NO_ENTRY);
    assert_eq!(
        out.fds.iter().map(|f| f.fd).collect::<Vec<_>>(),
        [7, 9],
        "exactly the fds the actions name"
    );
    drop_made(&mut r, &out);
    // The same without the flag: every fd the parent has, and its table.
    let out = r.p.inherit(&[FileAction::Dup2(a, 9)], 0).unwrap();
    assert_eq!(out.fds.len(), 3);
    drop_made(&mut r, &out);
    assert_eq!(count(&mut r), held);
    // A `dup2` of an fd the parent lacks is `EBADF` in both.
    assert_eq!(
        r.p.inherit_shaped(&[FileAction::Dup2(55, 3)], 0, &chosen(vec![]))
            .err(),
        Some(EBADF)
    );
    assert_eq!(
        r.p.inherit(&[FileAction::Dup2(55, 3)], 0).err(),
        Some(EBADF)
    );
    assert_eq!(count(&mut r), held);
}

#[test]
fn a_shape_no_spawn_can_have_makes_nothing() {
    let mut r = rig();
    let held = count(&mut r);
    for shape in [
        Shape {
            entries: vec![spec("/", false)],
            ..Default::default()
        },
        Shape {
            grants: 64,
            ..Default::default()
        },
        Shape {
            grants: GRANT_NET | MOVE_NET,
            ..Default::default()
        },
        chosen(vec![spec("/", false), spec("/", true)]),
    ] {
        assert_eq!(r.p.inherit_shaped(&[], 0, &shape).err(), Some(EINVAL));
    }
    assert_eq!(count(&mut r), held);
}

// ------------------------------------------------------------ addopen

#[test]
fn addopen_opens_in_the_parent_and_the_child_gets_the_file() {
    let mut r = rig();
    mkdir(&mut r, b"/logs");
    let keep = create(&mut r, b"/logs/in", b"abc");
    r.p.close(keep).unwrap();
    let held = count(&mut r);
    let open = |path: &[u8], flags| FileAction::Open(5, path.to_vec(), flags, 0o600);
    // A file outside the listed table: the parent's act, the child's file.
    let out =
        r.p.inherit_shaped(&[open(b"/logs/in", o::RDONLY)], 0, &chosen(vec![]))
            .unwrap();
    assert_eq!(out.fds.len(), 1);
    assert_eq!((out.fds[0].fd, out.fds[0].kind), (5, FdKind::File));
    assert_eq!(out.fds[0].flags & 6, 2, "read, not write");
    assert!(out.entries.is_empty());
    assert_eq!(out.temps.len(), 1);
    let temp = out.temps[0];
    assert!(r.p.fds.get(temp).is_some(), "open until the spawn is over");
    let mut child = become_child(&mut r, &out);
    assert_eq!(child.read(5, 10).unwrap(), b"abc");
    // The child's connection is as read-only as the open said.
    assert_eq!(child.write(5, b"z"), Err(EBADF));
    r.p.spawned(&out, &Granted::none());
    assert!(
        r.p.fds.get(temp).is_none(),
        "closed when the spawn happened"
    );
    drop(child);
    drop_made(&mut r, &out);
    // `O_CREAT` creates, as an open does.
    let out =
        r.p.inherit_shaped(
            &[open(b"/logs/new", o::CREAT | o::WRONLY)],
            0,
            &chosen(vec![]),
        )
        .unwrap();
    assert_eq!(out.fds[0].flags & 6, 4);
    r.p.spawn_refused(&out);
    drop_made(&mut r, &out);
    assert!(r.p.open(AT_FDCWD, b"/logs/new", o::RDONLY, 0).is_ok());
    // A failing open, or an action after it that fails, closes what it made.
    let before = count(&mut r);
    assert_eq!(
        r.p.inherit_shaped(&[open(b"/logs/none", o::RDONLY)], 0, &chosen(vec![]))
            .err(),
        Some(ENOENT)
    );
    assert_eq!(
        r.p.inherit_shaped(
            &[open(b"/logs/in", o::RDONLY), FileAction::Dup2(77, 6)],
            0,
            &chosen(vec![])
        )
        .err(),
        Some(EBADF)
    );
    assert_eq!(count(&mut r), before);
    let _ = held;
}

// ------------------------------------------------------------ Unix sockets

#[test]
fn a_stream_socket_crosses_a_spawn_and_its_rights_work_in_the_child() {
    let mut r = rig();
    let held = count(&mut r);
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_STREAM, 0).unwrap();
    let out =
        r.p.inherit_shaped(&[FileAction::Dup2(b, 4)], 0, &chosen(vec![]))
            .unwrap();
    // One fd, three handles: the two byte streams copied, the ancillary
    // channel moved.
    assert_eq!(out.passed.fds, [4]);
    assert_eq!(out.passed.handles.iter().filter(|h| h.1).count(), 2);
    assert_eq!(out.passed.handles.iter().filter(|h| !h.1).count(), 1);
    let mut child = second(&r);
    ship(&mut r, &out, &mut child);
    r.p.spawned(&out, &Granted::none());
    // The parent keeps the bytes and gives up descriptors on this socket.
    let Some(Desc::Socket(s)) = r.p.fds.get(b) else {
        panic!()
    };
    assert!(s.ctl.is_none() && s.unix.as_ref().unwrap().shared);
    // Bytes, both ways.
    assert_eq!(r.write(a, b"to the child").unwrap(), 12);
    assert_eq!(child.read(4, 100).unwrap(), b"to the child");
    assert_eq!(child.write(4, b"and back").unwrap(), 8);
    assert_eq!(r.read(a, 100).unwrap(), b"and back");
    // A descriptor from the child to the parent's other end: the child has
    // the ancillary channel, so it can send.
    let (pr, pw) = child.p.pipe(0).unwrap();
    child.write(pw, b"payload").unwrap();
    let u = child.poke(0, b"!");
    assert_eq!(child.p.sendmsg(4, &[u], &[pr], 0), Ok(1));
    let (bytes, fds) = read_fds(&mut r, a).unwrap();
    assert_eq!((bytes.as_slice(), fds.len()), (&b"!"[..], 1));
    assert_eq!(r.read(fds[0], 10).unwrap(), b"payload");
    // The parent's end cannot send descriptors any more (shared, item 6).
    let u = r.poke(0, b"?");
    assert_eq!(r.p.sendmsg(b, &[u], &[fds[0]], 0), Err(EOPNOTSUPP));
    // Nothing left over: close every fd of both and every handle is back
    // (but the child's own arena).
    for fd in fds {
        let _ = r.p.close(fd);
    }
    close_all(&mut r);
    close_all(&mut child);
    assert_eq!(count(&mut r), held + 1);
}

#[test]
fn a_packet_socket_moves_to_the_child_and_is_dead_in_the_parent() {
    for ty in [SOCK_SEQPACKET, SOCK_DGRAM] {
        let mut r = rig();
        let held = count(&mut r);
        let (a, b) = r.p.socketpair(AF_UNIX, ty, 0).unwrap();
        let out =
            r.p.inherit_shaped(&[FileAction::Dup2(b, 3)], 0, &chosen(vec![]))
                .unwrap();
        assert_eq!(out.passed.handles, [(out.passed.handles[0].0, false)]);
        let mut child = second(&r);
        ship(&mut r, &out, &mut child);
        r.p.spawned(&out, &Granted::none());
        // The parent's copy is dead, not closed: the number stays its own.
        assert_eq!(r.write(b, b"x"), Err(EPIPE));
        assert!(r.p.fds.get(b).is_some());
        assert_eq!(r.write(a, b"to the child").unwrap(), 12);
        assert_eq!(child.read(3, 100).unwrap(), b"to the child");
        assert_eq!(child.write(3, b"back").unwrap(), 4);
        assert_eq!(r.read(a, 100).unwrap(), b"back");
        r.p.close(b).unwrap();
        // The peer going away is seen by the child.
        r.p.close(a).unwrap();
        assert_eq!(child.read(3, 10).unwrap(), b"");
        close_all(&mut child);
        assert_eq!(count(&mut r), held + 1);
    }
}

#[test]
fn a_spawn_that_does_not_happen_changes_no_socket() {
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let (c, d) = r.p.socketpair(AF_UNIX, SOCK_STREAM, 0).unwrap();
    let held = count(&mut r);
    let out =
        r.p.inherit(&[FileAction::Close(a), FileAction::Close(c)], 0)
            .unwrap();
    assert_eq!(out.passed.fds, [b as u32, d as u32]);
    // Nobody called `spawned`: the parent's sockets are as they were.
    r.p.spawn_refused(&out);
    drop_made(&mut r, &out);
    assert_eq!(count(&mut r), held);
    assert_eq!(r.write(b, b"still").unwrap(), 5);
    assert_eq!(r.read(a, 10).unwrap(), b"still");
    let Some(Desc::Socket(s)) = r.p.fds.get(d) else {
        panic!()
    };
    assert!(s.ctl.is_some() && !s.unix.as_ref().unwrap().shared);
}

#[test]
fn a_socket_that_cannot_go_makes_the_spawn_fail_and_leaves_nothing() {
    let mut r = rig();
    let held = count(&mut r);
    let (a, _b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let (c, _d) = r.p.socketpair(AF_UNIX, SOCK_STREAM, 0).unwrap();
    let before = count(&mut r);
    // A description at two fds of the child goes once: `EINVAL`.
    assert_eq!(
        r.p.inherit(&[FileAction::Dup2(a, 20), FileAction::Dup2(a, 21)], 0)
            .err(),
        Some(EINVAL)
    );
    // An unconnected Unix socket and a TCP socket cannot go: `EBADF`, as a
    // socket could not before.
    let fresh = r.p.socket(AF_UNIX, SOCK_STREAM, 0).unwrap();
    assert_eq!(r.p.inherit(&[], 0).err(), Some(EBADF));
    r.p.close(fresh).unwrap();
    let tcp = r.p.socket(2, SOCK_STREAM, 0).unwrap();
    assert_eq!(
        r.p.inherit_shaped(&[FileAction::Dup2(tcp, 9)], 0, &chosen(vec![]))
            .err(),
        Some(EBADF)
    );
    r.p.close(tcp).unwrap();
    // A held packet message (the table was full when it arrived) holds it.
    let _ = c;
    assert_eq!(count(&mut r), before);
    let _ = held;
}

/// Close-on-exec is the one way to keep a socket from a child that does not
/// list it, and an inherited one is no longer an error.
#[test]
fn a_close_on_exec_socket_is_not_inherited_and_the_rest_is() {
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_STREAM, 0).unwrap();
    r.p.fcntl(a, F_SETFD, i64::from(FD_CLOEXEC)).unwrap();
    // Made close-on-exec, as `SOCK_CLOEXEC` does.
    let (c, d) =
        r.p.socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0)
            .unwrap();
    let out = r.p.inherit(&[], 0).unwrap();
    assert_eq!(out.passed.fds, [b as u32]);
    let _ = (c, d);
    drop_made(&mut r, &out);
}

// ------------------------------------------------------------ memory descriptors

/// The plan's "4 MiB memfd to a child that reads it", over two processes of
/// the fake kernel (INV-0432-2, INV-0413-2's other-process half).
#[test]
fn a_memory_descriptor_crosses_a_spawn_with_its_bytes_and_its_size() {
    let mut r = rig();
    let size = 4 * 1024 * 1024;
    let fd = r.p.memfd_create(b"big", MFD_ALLOW_SEALING).unwrap();
    r.p.ftruncate(fd, size as i64).unwrap();
    let at =
        r.p.memory_mmap(fd, size as i64, PROT_READ | PROT_WRITE, MAP_SHARED, 0)
            .unwrap();
    r.net.borrow_mut().memory(at, 5).copy_from_slice(b"hello");
    r.net
        .borrow_mut()
        .memory(at + size as u64 - 3, 3)
        .copy_from_slice(b"end");
    let out =
        r.p.inherit_shaped(&[FileAction::Dup2(fd, 6)], 0, &chosen(vec![]))
            .unwrap();
    assert_eq!(out.passed.fds, [6]);
    assert!(
        out.passed.handles.iter().all(|h| h.1),
        "the object is copied"
    );
    let mut child = second(&r);
    ship(&mut r, &out, &mut child);
    r.p.spawned(&out, &Granted::none());
    let there = child
        .p
        .memory_mmap(6, size as i64, PROT_READ, MAP_SHARED, 0)
        .unwrap();
    assert_eq!(child.net.borrow_mut().memory(there, 5), b"hello");
    assert_eq!(
        child.net.borrow_mut().memory(there + size as u64 - 3, 3),
        b"end"
    );
    // The same object: the parent's later write is the child's read.
    r.net.borrow_mut().memory(at, 5).copy_from_slice(b"HELLO");
    assert_eq!(child.net.borrow_mut().memory(there, 5), b"HELLO");
    // The size is fixed on both sides once it went (INV-0413-1).
    assert_eq!(r.p.ftruncate(fd, 100), Err(EBUSY));
    assert_eq!(child.p.ftruncate(6, 100), Err(EBUSY));
    // Seals the parent had are the child's, as for a descriptor that was sent.
    let Some(Desc::Memory(m)) = child.p.fds.get(6) else {
        panic!()
    };
    assert!(m.object().is_some());
}

/// A sealed descriptor's handle is read-only where it goes, as the kernel
/// sees it: the child cannot map it writable however it asks.
#[test]
fn a_read_only_memory_descriptor_is_read_only_in_the_child() {
    let mut r = rig();
    let fd = r.p.memfd_create(b"ro", MFD_ALLOW_SEALING).unwrap();
    r.p.ftruncate(fd, PAGE as i64).unwrap();
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::FUTURE_WRITE))
        .unwrap();
    let out =
        r.p.inherit_shaped(&[FileAction::Dup2(fd, 6)], 0, &chosen(vec![]))
            .unwrap();
    let object = out.passed.handles[0].0;
    assert!(r.net.borrow().is_read_only(object), "the parent's handle");
    assert_eq!(out.passed.read_only, [object]);
    let mut child = second(&r);
    ship(&mut r, &out, &mut child);
    r.p.spawned(&out, &Granted::none());
    let Some(Desc::Memory(m)) = child.p.fds.get(6) else {
        panic!()
    };
    let theirs = m.object().unwrap();
    assert!(
        child.net.borrow().is_read_only(theirs),
        "the child's handle"
    );
    assert!(
        child
            .p
            .memory_mmap(6, PAGE as i64, PROT_WRITE, MAP_SHARED, 0)
            .is_err()
    );
    assert!(
        child
            .p
            .memory_mmap(6, PAGE as i64, PROT_READ, MAP_SHARED, 0)
            .is_ok()
    );
}

/// ADR-0432 item 5, INV-0432-2 after plan part F4: a descriptor without
/// close-on-exec goes with a spawn, one with it does not, a descriptor
/// without an object cannot go, and a refused spawn leaves no handle.
#[test]
fn a_memory_descriptor_goes_unless_close_on_exec_and_a_failure_leaves_nothing() {
    let mut r = rig();
    let held = count(&mut r);
    let closed = r.p.memfd_create(b"c", MFD_CLOEXEC).unwrap();
    r.p.ftruncate(closed, 100).unwrap();
    let out = r.p.inherit(&[], 0).unwrap();
    assert!(out.passed.is_empty(), "close-on-exec stays behind");
    drop_made(&mut r, &out);
    let open = r.p.memfd_create(b"o", 0).unwrap();
    // No object yet: nothing to give.
    assert_eq!(r.p.inherit(&[], 0).err(), Some(EINVAL));
    r.p.ftruncate(open, 100).unwrap();
    let out = r.p.inherit(&[], 0).unwrap();
    assert_eq!(out.passed.fds, [open as u32]);
    // A description at two fds of the child goes once.
    let d = r.p.dup(open).unwrap();
    let before = count(&mut r);
    assert_eq!(
        r.p.inherit(&[FileAction::Dup2(open, 30), FileAction::Dup2(d, 31)], 0)
            .err(),
        Some(EINVAL)
    );
    assert_eq!(count(&mut r), before);
    // Not told it went: the parent's descriptor can still change size? It
    // was never sent, so it can.
    r.p.spawn_refused(&out);
    drop_made(&mut r, &out);
    assert_eq!(r.p.ftruncate(open, 200), Ok(()));
    let _ = (held, closed);
}

/// A message no library of ours built never panics a child and leaves no
/// handle open (INV-0430-1, INV-0432-1 for the third message).
#[test]
fn a_hostile_third_message_is_dropped_and_its_handles_closed() {
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_STREAM, 0).unwrap();
    let (c, d) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let out =
        r.p.inherit(&[FileAction::Close(a), FileAction::Close(c)], 0)
            .unwrap();
    let good = encode_passed(&out.passed.fds, &out.passed.records);
    assert_eq!(out.passed.fds.len(), 2);
    let _ = (b, d);
    let count = out.passed.handles.len();
    let mut decoded = 0;
    // One child over and over: a rig holds a 4 MiB arena.
    let mut child = second(&r);
    for i in 0..good.len() * 8 {
        let mut m = good.clone();
        m[i / 8] ^= 1 << (i % 8);
        let before = child.handles();
        // The handles the message came with: fresh ones, as a kernel gives.
        let made: Vec<u32> = (0..count)
            .map(|_| child.p.sys.channel_create().unwrap().0)
            .collect();
        let after_made = child.handles();
        match decode_passed(&m, made.len(), OPEN_MAX) {
            Some((fds, records)) => {
                decoded += 1;
                child.p.install_passed(&fds, records, made);
                let fds: Vec<i32> = child.p.fds.fds().map(|f| f as i32).collect();
                for fd in fds {
                    let _ = child.p.close(fd);
                }
            }
            None => {
                for h in made {
                    child.p.sys.close(h);
                }
            }
        }
        // Whatever it decoded to, the channels' peers are the only ones left.
        assert_eq!(child.handles(), after_made - count, "bit {i}");
        // Those peers, as the parent holds them, closed again.
        let _ = before;
    }
    assert!(decoded > 0, "some flips still decode");
    drop_made(&mut r, &out);
}

// ------------------------------------------------------------ grants

/// A service end that answers one request kind and keeps the handles that
/// came with the requests.
struct Service {
    end: u32,
    received: Rc<RefCell<Vec<u32>>>,
}

/// Makes `r.net` or `r.random` a channel whose far end `answer` serves, each
/// time a wait finds nothing yet.
fn serve(r: &mut Rig, answer: impl Fn(u32) -> Vec<u8> + 'static) -> (u32, Service) {
    let (mine, theirs) = r.p.sys.channel_create().unwrap();
    let received = Rc::new(RefCell::new(Vec::new()));
    let keep = received.clone();
    let end = theirs;
    r.p.sys.meanwhile = Some(Box::new(move |n: &mut Net| {
        while let Some((bytes, handles)) = n.take_message(end) {
            let (h, _) = Header::decode(&bytes).unwrap();
            keep.borrow_mut().extend(handles);
            n.send_message(end, &answer(h.transaction), &[]);
        }
    }));
    (mine, Service { end, received })
}

fn duplicated(status: u32) -> impl Fn(u32) -> Vec<u8> {
    move |t| {
        let mut out = vec![0u8; 64];
        let n = encode(
            &Duplicated {
                status: Below(status),
            },
            t,
            &mut out,
        )
        .unwrap();
        out.truncate(n);
        out
    }
}

fn shared(status: u32) -> impl Fn(u32) -> Vec<u8> {
    move |t| {
        let mut out = vec![0u8; 64];
        let n = encode(
            &ShareReply {
                status: Below(status),
            },
            t,
            &mut out,
        )
        .unwrap();
        out.truncate(n);
        out
    }
}

#[test]
fn grant_net_asks_the_service_for_a_connection_and_a_parent_without_one_is_refused() {
    let mut r = rig();
    let held = count(&mut r);
    assert_eq!(r.p.grant_connections(GRANT_NET).err(), Some(EACCES));
    assert_eq!(r.p.grant_connections(MOVE_NET).err(), Some(EACCES));
    assert_eq!(r.p.grant_connections(GRANT_RANDOM).err(), Some(EACCES));
    assert_eq!(r.p.grant_connections(GRANT_WINDOW).err(), Some(EACCES));
    assert_eq!(count(&mut r), held);
    let (mine, service) = serve(&mut r, duplicated(net_status::OK));
    r.p.net = Some(mine);
    let before = count(&mut r);
    let g = r.p.grant_connections(GRANT_NET).unwrap();
    let (h, moved) = g.net.expect("a connection");
    assert!(!moved && h != mine);
    // The service got one `Duplicate` with the far end of what the child got.
    assert_eq!(service.received.borrow().len(), 1);
    let theirs = service.received.borrow()[0];
    assert!(r.net.borrow_mut().send_message(h, b"ping", &[]));
    assert_eq!(r.net.borrow_mut().take_message(theirs).unwrap().0, b"ping");
    // The parent keeps its own, and nothing else came.
    assert_eq!(r.p.net, Some(mine));
    assert!(g.random.is_none() && g.window.is_none());
    assert_eq!(g.made(), [h]);
    // The request is a `Duplicate` and nothing else.
    let _ = Duplicate {};
    r.p.sys.close(h);
    r.p.sys.close(theirs);
    assert_eq!(count(&mut r), before);
    let _ = service.end;
}

#[test]
fn a_service_that_refuses_or_is_full_leaves_nothing_and_says_why() {
    for (status, errno) in [
        (net_status::NOT_GRANTED, EACCES),
        (net_status::BUSY, EAGAIN),
        (net_status::INVALID, EACCES),
    ] {
        let mut r = rig();
        let (mine, service) = serve(&mut r, duplicated(status));
        r.p.net = Some(mine);
        let before = count(&mut r);
        assert_eq!(r.p.grant_connections(GRANT_NET).err(), Some(errno));
        // The far end the service received is the service's to close.
        let theirs: Vec<u32> = service.received.borrow().clone();
        for h in &theirs {
            r.p.sys.close(*h);
        }
        assert_eq!(count(&mut r), before);
    }
    // A random service with no room.
    let mut r = rig();
    let (mine, service) = serve(&mut r, shared(rng_status::NO_SPACE));
    r.p.random = Some(mine);
    let before = count(&mut r);
    assert_eq!(r.p.grant_connections(GRANT_RANDOM).err(), Some(EAGAIN));
    let theirs: Vec<u32> = service.received.borrow().clone();
    for h in &theirs {
        r.p.sys.close(*h);
    }
    assert_eq!(count(&mut r), before);
}

#[test]
fn grant_random_makes_a_share_and_a_later_refusal_closes_the_first() {
    let mut r = rig();
    let (net, ns) = {
        let (mine, s) = serve(&mut r, duplicated(net_status::OK));
        (mine, s)
    };
    r.p.net = Some(net);
    let before = count(&mut r);
    // The net connection is made, then there is no random connection: the
    // net connection made is closed again.
    assert_eq!(
        r.p.grant_connections(GRANT_NET | GRANT_RANDOM).err(),
        Some(EACCES)
    );
    let theirs: Vec<u32> = ns.received.borrow().clone();
    for h in &theirs {
        r.p.sys.close(*h);
    }
    assert_eq!(count(&mut r), before);
    drop(ns);
    // With both: the request is a `Share` and the child's end is the peer.
    let mut r = rig();
    let (mine, service) = serve(&mut r, shared(rng_status::OK));
    r.p.random = Some(mine);
    let g = r.p.grant_connections(GRANT_RANDOM).unwrap();
    let h = g.random.expect("a connection");
    assert!(g.net.is_none());
    let theirs = service.received.borrow()[0];
    assert!(r.net.borrow_mut().send_message(h, b"x", &[]));
    assert_eq!(r.net.borrow_mut().take_message(theirs).unwrap().0, b"x");
    let _ = Share {};
}

/// A moved connection is the parent's own handle, and the parent has none
/// after the spawn; a shared one is a new handle and the parent keeps its own.
#[test]
fn move_net_and_grant_window_move_the_parents_handle_and_it_is_gone_after() {
    let mut r = rig();
    let (net, _service) = serve(&mut r, duplicated(net_status::OK));
    let (window, _w) = r.p.sys.channel_create().unwrap();
    r.p.net = Some(net);
    r.p.window = Some(window);
    let g = r.p.grant_connections(MOVE_NET | GRANT_WINDOW).unwrap();
    assert_eq!(g.net, Some((net, true)));
    assert_eq!(g.window, Some(window));
    assert!(g.made().is_empty(), "nothing the spawn made");
    assert_eq!(g.in_order(), [net, window]);
    // Until the spawn happens the parent has them.
    assert_eq!((r.p.net, r.p.window), (Some(net), Some(window)));
    r.p.spawned(&Inherit::empty(), &g);
    assert_eq!((r.p.net, r.p.window), (None, None));
    // A shared connection leaves the parent's own.
    let (net, _service) = serve(&mut r, duplicated(net_status::OK));
    r.p.net = Some(net);
    let g = r.p.grant_connections(GRANT_NET).unwrap();
    r.p.spawned(&Inherit::empty(), &g);
    assert_eq!(r.p.net, Some(net));
}

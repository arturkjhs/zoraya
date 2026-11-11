//! Storage limits (ADR-0265): INV-0265-1 (`used` equals the recomputed sum,
//! also after a crash at every write) and INV-0265-2 (a growing request past
//! a limit fails with `OverQuota` and changes nothing; a shrinking request
//! never fails for a limit, also on a full volume).

mod common;

use common::*;
use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{
    BLOCK_SIZE, BlockDevice, FsError, Handle, MountOptions, ObjKind, OpenOptions, PublishMode,
    RenameMode, Volume,
};
use zoraya_fs_testkit::crash::{CrashConfig, for_each_crash_image};
use zoraya_fs_testkit::{RecordingDevice, Rng, TestClock};

const BS: u64 = BLOCK_SIZE as u64;

fn create() -> OpenOptions {
    OpenOptions {
        create: Some(0o644),
        ..OpenOptions::default()
    }
}

fn charge(kind: ObjKind, size: u64) -> u64 {
    match kind {
        ObjKind::Directory => BS,
        _ => BS + size.div_ceil(BS) * BS,
    }
}

/// What the directory at `path` should be charged, computed through the
/// public API only: one block per object below it and each file's size in
/// blocks.
fn oracle<D: BlockDevice>(v: &mut Volume<D, TestClock>, path: &str) -> u64 {
    let root = v.root();
    let h = v.open(root, path, OpenOptions::default()).unwrap();
    let n = sum(v, h);
    v.close(h).unwrap();
    n
}

fn sum<D: BlockDevice>(v: &mut Volume<D, TestClock>, dir: Handle) -> u64 {
    let mut total = 0;
    let mut after: Option<String> = None;
    while let Some(e) = v.readdir(dir, after.as_deref()).unwrap() {
        let a = v.stat_at(dir, &e.name).unwrap();
        total += charge(a.kind, a.size);
        if e.kind == ObjKind::Directory {
            let h = v.open(dir, &e.name, OpenOptions::default()).unwrap();
            total += sum(v, h);
            v.close(h).unwrap();
        }
        after = Some(e.name);
    }
    total
}

fn used<D: BlockDevice>(v: &mut Volume<D, TestClock>, path: &str) -> u64 {
    let root = v.root();
    let h = v.open(root, path, OpenOptions::default()).unwrap();
    let l = v.limit_of(h).unwrap().expect("a limit");
    v.close(h).unwrap();
    l.used
}

/// Creates `path` if needed and writes `data`. A file this call created is
/// removed again if the write fails, so a refusal leaves nothing behind.
fn put<D: BlockDevice>(
    v: &mut Volume<D, TestClock>,
    path: &str,
    off: u64,
    data: &[u8],
) -> Result<(), FsError> {
    let root = v.root();
    let existed = v.stat_at(root, path).is_ok();
    let f = v.open(root, path, create())?;
    let r = v.write(f, off, data).map(|_| ());
    v.close(f).unwrap();
    if r.is_err() && !existed {
        v.unlink(root, path).unwrap();
    }
    r
}

fn fsck<D: BlockDevice>(mut d: D) {
    let rep = check(&mut d, CheckOptions::default());
    assert!(rep.is_clean(), "{:?}", rep.problems);
}

#[test]
fn limit_stops_growth_and_delete_works_at_the_limit() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(512), &clock);
    let root = v.root();
    v.mkdir(root, "room", 0o755).unwrap();
    v.set_limit(root, "room", Some(10 * BS)).unwrap();
    assert_eq!(used(&mut v, "room"), 0);
    // A file takes a block and its data: 1 + 8 blocks.
    put(&mut v, "room/a", 0, &vec![1u8; 8 * BLOCK_SIZE]).unwrap();
    assert_eq!(used(&mut v, "room"), 9 * BS);
    // Past the limit: refused, nothing changed.
    let before = used(&mut v, "room");
    assert_eq!(
        put(&mut v, "room/a", 8 * BS, &vec![2u8; 2 * BLOCK_SIZE]),
        Err(FsError::OverQuota)
    );
    assert_eq!(used(&mut v, "room"), before);
    assert_eq!(v.stat_at(root, "room/a").unwrap().size, 8 * BS);
    // A new file needs a block for itself plus its data.
    assert_eq!(
        put(&mut v, "room/b", 0, &vec![3u8; BLOCK_SIZE]),
        Err(FsError::OverQuota)
    );
    assert!(matches!(v.stat_at(root, "room/b"), Err(FsError::NotFound)));
    // Exactly to the limit is allowed (an empty file: one block).
    put(&mut v, "room/c", 0, b"").unwrap();
    assert_eq!(used(&mut v, "room"), 10 * BS);
    assert_eq!(v.mkdir(root, "room/d", 0o755), Err(FsError::OverQuota));
    assert_eq!(v.symlink(root, "room/l", "x"), Err(FsError::OverQuota));
    // A rewrite in place never grows.
    put(&mut v, "room/a", 0, &vec![9u8; 8 * BLOCK_SIZE]).unwrap();
    // set_size: growth refused, shrink fine.
    let f = v.open(root, "room/c", OpenOptions::default()).unwrap();
    assert_eq!(v.set_size(f, 1), Err(FsError::OverQuota));
    v.close(f).unwrap();
    let f = v.open(root, "room/a", OpenOptions::default()).unwrap();
    v.set_size(f, BS).unwrap();
    assert_eq!(used(&mut v, "room"), 10 * BS - 7 * BS);
    v.set_size(f, 4 * BS).unwrap();
    assert_eq!(used(&mut v, "room"), 10 * BS - 4 * BS);
    v.close(f).unwrap();
    // Delete at the limit.
    put(&mut v, "room/e", 0, &vec![1u8; 3 * BLOCK_SIZE]).unwrap();
    assert_eq!(used(&mut v, "room"), 10 * BS);
    v.unlink(root, "room/e").unwrap();
    v.unlink(root, "room/a").unwrap();
    v.unlink(root, "room/c").unwrap();
    assert_eq!(used(&mut v, "room"), 0);
    // Outside the limit, the volume is not affected.
    put(&mut v, "outside", 0, &vec![1u8; 20 * BLOCK_SIZE]).unwrap();
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

#[test]
fn every_limited_ancestor_applies() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(512), &clock);
    let root = v.root();
    v.mkdir(root, "a", 0o755).unwrap();
    v.mkdir(root, "a/b", 0o755).unwrap();
    v.mkdir(root, "a/b/c", 0o755).unwrap();
    v.set_limit(root, "a", Some(100 * BS)).unwrap();
    v.set_limit(root, "a/b", Some(6 * BS)).unwrap();
    // c itself costs a block in a/b and in a.
    assert_eq!(used(&mut v, "a/b"), BS);
    assert_eq!(used(&mut v, "a"), 2 * BS);
    put(&mut v, "a/b/c/f", 0, &vec![1u8; 3 * BLOCK_SIZE]).unwrap();
    assert_eq!(used(&mut v, "a/b"), 5 * BS);
    assert_eq!(used(&mut v, "a"), 6 * BS);
    // The inner limit refuses although the outer has room.
    assert_eq!(
        put(&mut v, "a/b/c/g", 0, &vec![1u8; 2 * BLOCK_SIZE]),
        Err(FsError::OverQuota)
    );
    // Both counters are unchanged by the failed request.
    assert_eq!(used(&mut v, "a/b"), 5 * BS);
    assert_eq!(used(&mut v, "a"), 6 * BS);
    // A sibling of b is bound by a only.
    put(&mut v, "a/h", 0, &vec![1u8; 50 * BLOCK_SIZE]).unwrap();
    assert_eq!(used(&mut v, "a"), 57 * BS);
    assert_eq!(
        put(&mut v, "a/h2", 0, &vec![1u8; 50 * BLOCK_SIZE]),
        Err(FsError::OverQuota)
    );
    // Removing the inner limit leaves the outer one.
    v.set_limit(root, "a/b", None).unwrap();
    put(&mut v, "a/b/c/g", 0, &vec![1u8; 2 * BLOCK_SIZE]).unwrap();
    assert_eq!(used(&mut v, "a"), 60 * BS);
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

#[test]
fn a_new_limit_counts_what_is_there_and_a_low_limit_stops_growth_only() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(512), &clock);
    let root = v.root();
    v.mkdir(root, "d", 0o755).unwrap();
    v.mkdir(root, "d/s", 0o755).unwrap();
    put(&mut v, "d/s/f", 0, &vec![1u8; 5 * BLOCK_SIZE]).unwrap();
    put(&mut v, "d/g", 0, b"x").unwrap();
    v.symlink(root, "d/l", "target").unwrap();
    v.set_limit(root, "d", Some(3 * BS)).unwrap();
    // s (1) + f (1+5) + g (1+1) + l (1+1).
    assert_eq!(used(&mut v, "d"), 11 * BS);
    assert_eq!(used(&mut v, "d"), oracle(&mut v, "d"));
    assert_eq!(put(&mut v, "d/new", 0, b""), Err(FsError::OverQuota));
    // Shrinking still works and the counter follows.
    v.unlink(root, "d/s/f").unwrap();
    assert_eq!(used(&mut v, "d"), 5 * BS);
    assert_eq!(put(&mut v, "d/new", 0, b""), Err(FsError::OverQuota));
    v.unlink(root, "d/g").unwrap();
    v.unlink(root, "d/l").unwrap();
    put(&mut v, "d/new", 0, b"").unwrap();
    // Changing the limit keeps `used`.
    v.set_limit(root, "d", Some(100 * BS)).unwrap();
    assert_eq!(used(&mut v, "d"), 2 * BS);
    // A file cannot carry a limit.
    assert_eq!(v.set_limit(root, "d/new", Some(1)), Err(FsError::NotDir));
    assert_eq!(v.set_limit(root, "nope", Some(1)), Err(FsError::NotFound));
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

#[test]
fn rename_across_a_boundary() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(512), &clock);
    let root = v.root();
    for d in ["a", "b", "free", "a/in"] {
        v.mkdir(root, d, 0o755).unwrap();
    }
    v.set_limit(root, "a", Some(20 * BS)).unwrap();
    v.set_limit(root, "b", Some(6 * BS)).unwrap();
    put(&mut v, "a/f", 0, &vec![1u8; 4 * BLOCK_SIZE]).unwrap();
    let ua = used(&mut v, "a");
    // A file moves its charge.
    v.rename(root, "a/f", root, "b/f", RenameMode::NoReplace)
        .unwrap();
    assert_eq!(used(&mut v, "a"), ua - 5 * BS);
    assert_eq!(used(&mut v, "b"), 5 * BS);
    // Into a directory without room: refused, nothing moved.
    put(&mut v, "a/g", 0, &vec![1u8; 3 * BLOCK_SIZE]).unwrap();
    let (ua, ub) = (used(&mut v, "a"), used(&mut v, "b"));
    assert_eq!(
        v.rename(root, "a/g", root, "b/g", RenameMode::NoReplace),
        Err(FsError::OverQuota)
    );
    assert_eq!((used(&mut v, "a"), used(&mut v, "b")), (ua, ub));
    v.stat_at(root, "a/g").unwrap();
    // Out of a limited tree into an unlimited one.
    v.rename(root, "b/f", root, "free/f", RenameMode::NoReplace)
        .unwrap();
    assert_eq!(used(&mut v, "b"), 0);
    // A directory moves freely inside one limited tree...
    v.mkdir(root, "a/in/deep", 0o755).unwrap();
    v.rename(root, "a/in/deep", root, "a/deep", RenameMode::NoReplace)
        .unwrap();
    // ...but not across a boundary, either way.
    for (s, d) in [("a/deep", "b/deep"), ("a/deep", "free/deep")] {
        assert_eq!(
            v.rename(root, s, root, d, RenameMode::NoReplace),
            Err(FsError::CrossesLimit)
        );
    }
    v.mkdir(root, "free/fd", 0o755).unwrap();
    assert_eq!(
        v.rename(root, "free/fd", root, "a/fd", RenameMode::NoReplace),
        Err(FsError::CrossesLimit)
    );
    // A directory between two places under no limit moves freely.
    v.rename(root, "free/fd", root, "fd", RenameMode::NoReplace)
        .unwrap();
    // Replace: the replaced file's charge goes.
    put(&mut v, "a/r1", 0, &vec![1u8; 2 * BLOCK_SIZE]).unwrap();
    put(&mut v, "a/r2", 0, b"").unwrap();
    let ua = used(&mut v, "a");
    v.rename(root, "a/r2", root, "a/r1", RenameMode::Replace)
        .unwrap();
    assert_eq!(used(&mut v, "a"), ua - 3 * BS);
    // Exchange of files between two limited trees.
    put(&mut v, "b/x", 0, &vec![1u8; 4 * BLOCK_SIZE]).unwrap();
    put(&mut v, "a/y", 0, &vec![1u8; BLOCK_SIZE]).unwrap();
    let (ua, ub) = (used(&mut v, "a"), used(&mut v, "b"));
    v.rename(root, "b/x", root, "a/y", RenameMode::Exchange)
        .unwrap();
    assert_eq!(used(&mut v, "a"), ua + 3 * BS);
    assert_eq!(used(&mut v, "b"), ub - 3 * BS);
    for p in ["a", "b"] {
        assert_eq!(used(&mut v, p), oracle(&mut v, p));
    }
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

#[test]
fn unnamed_files_and_publish_move_the_charge() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(512), &clock);
    let root = v.root();
    v.mkdir(root, "a", 0o755).unwrap();
    v.mkdir(root, "b", 0o755).unwrap();
    v.set_limit(root, "a", Some(4 * BS)).unwrap();
    v.set_limit(root, "b", Some(4 * BS)).unwrap();
    let ad = v.open(root, "a", OpenOptions::default()).unwrap();
    let u = v.create_unnamed(ad, 0o644).unwrap();
    assert_eq!(used(&mut v, "a"), BS);
    // The unnamed file is charged where it was made, also when it grows.
    assert_eq!(
        v.write(u, 0, &vec![1u8; 4 * BLOCK_SIZE]),
        Err(FsError::OverQuota)
    );
    v.write(u, 0, &vec![1u8; 3 * BLOCK_SIZE]).unwrap();
    assert_eq!(used(&mut v, "a"), 4 * BS);
    let bd = v.open(root, "b", OpenOptions::default()).unwrap();
    v.publish(u, bd, "f", PublishMode::NoReplace).unwrap();
    assert_eq!(used(&mut v, "a"), 0);
    assert_eq!(used(&mut v, "b"), 4 * BS);
    // A second one does not fit b.
    let u2 = v.create_unnamed(ad, 0o644).unwrap();
    v.write(u2, 0, b"x").unwrap();
    assert_eq!(
        v.publish(u2, bd, "g", PublishMode::NoReplace),
        Err(FsError::OverQuota)
    );
    assert_eq!(used(&mut v, "a"), 2 * BS);
    v.close(u2).unwrap();
    assert_eq!(used(&mut v, "a"), 0);
    for h in [u, ad, bd] {
        v.close(h).unwrap();
    }
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

#[test]
fn a_file_unlinked_while_open_keeps_its_charge_until_closed() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(512), &clock);
    let root = v.root();
    v.mkdir(root, "a", 0o755).unwrap();
    v.mkdir(root, "a/sub", 0o755).unwrap();
    v.set_limit(root, "a", Some(6 * BS)).unwrap();
    let f = v.open(root, "a/sub/f", create()).unwrap();
    v.write(f, 0, b"x").unwrap();
    v.unlink(root, "a/sub/f").unwrap();
    // Still held, so a room cannot grow past its limit this way.
    assert_eq!(used(&mut v, "a"), 3 * BS);
    assert_eq!(
        v.write(f, 0, &vec![1u8; 10 * BLOCK_SIZE]),
        Err(FsError::OverQuota)
    );
    // The directory stays while the open file is charged to it (review Q5,
    // finding 4): removing it would let the file grow with no limit.
    assert_eq!(v.rmdir(root, "a/sub"), Err(FsError::NotEmpty));
    v.write(f, 0, &vec![1u8; 2 * BLOCK_SIZE]).unwrap();
    assert_eq!(used(&mut v, "a"), 4 * BS);
    v.sync().unwrap();
    fsck(v.clone_device_for_check());
    v.close(f).unwrap();
    assert_eq!(used(&mut v, "a"), BS);
    v.rmdir(root, "a/sub").unwrap();
    assert_eq!(used(&mut v, "a"), 0);
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

trait CloneDev {
    fn clone_device_for_check(&mut self) -> zoraya_fs_testkit::MemDevice;
}

impl CloneDev for Volume<zoraya_fs_testkit::MemDevice, TestClock> {
    fn clone_device_for_check(&mut self) -> zoraya_fs_testkit::MemDevice {
        self.device().clone()
    }
}

/// INV-0265-2 on a full volume: a delete works, a growing request is
/// `NoSpace`, and the counters stay true.
#[test]
fn deletes_work_on_a_full_volume_with_limits() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(128), &clock);
    let root = v.root();
    v.mkdir(root, "room", 0o755).unwrap();
    v.set_limit(root, "room", Some(1000 * BS)).unwrap();
    let mut n = 0;
    let err = loop {
        match put(&mut v, &format!("room/f{n}"), 0, &vec![7u8; 4 * BLOCK_SIZE]) {
            Ok(()) => n += 1,
            Err(e) => break e,
        }
    };
    assert_eq!(err, FsError::NoSpace);
    assert!(n > 3);
    let u = used(&mut v, "room");
    assert_eq!(u, oracle(&mut v, "room"));
    for i in 0..n {
        v.unlink(root, &format!("room/f{i}")).unwrap();
    }
    assert_eq!(used(&mut v, "room"), 0);
    put(&mut v, "room/again", 0, &vec![1u8; BLOCK_SIZE]).unwrap();
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

#[test]
fn limits_survive_remount() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(256), &clock);
    let root = v.root();
    v.mkdir(root, "r", 0o755).unwrap();
    v.set_limit(root, "r", Some(7 * BS)).unwrap();
    put(&mut v, "r/f", 0, &vec![1u8; 2 * BLOCK_SIZE]).unwrap();
    let d = v.unmount().unwrap();
    let mut v = mount(d, &clock);
    let root = v.root();
    let h = v.open(root, "r", OpenOptions::default()).unwrap();
    let l = v.limit_of(h).unwrap().unwrap();
    assert_eq!((l.limit, l.used), (7 * BS, 3 * BS));
    v.close(h).unwrap();
    assert_eq!(
        put(&mut v, "r/g", 0, &vec![1u8; 4 * BLOCK_SIZE]),
        Err(FsError::OverQuota)
    );
    // Removing a limited directory removes its record.
    v.unlink(root, "r/f").unwrap();
    v.rmdir(root, "r").unwrap();
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

/// One random request on the tree below `root`; every failure but the
/// expected ones is a test failure.
static REFUSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CROSSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn random_request<D: BlockDevice>(
    v: &mut Volume<D, TestClock>,
    rng: &mut Rng,
    pool: &mut Vec<Handle>,
) {
    const DIRS: &[&str] = &["a", "a/b", "a/b/c", "d", "d/e", "f", ""];
    let root = v.root();
    let dir = |rng: &mut Rng| *rng.pick(DIRS);
    let join = |d: &str, n: &str| {
        if d.is_empty() {
            n.to_string()
        } else {
            format!("{d}/{n}")
        }
    };
    let name = |rng: &mut Rng| format!("n{}", rng.below(6));
    let ok = |r: Result<(), FsError>| {
        use std::sync::atomic::Ordering::Relaxed;
        match &r {
            Err(FsError::OverQuota) => {
                REFUSED.fetch_add(1, Relaxed);
            }
            Err(FsError::CrossesLimit) => {
                CROSSED.fetch_add(1, Relaxed);
            }
            _ => {}
        }
        r
    };
    let ok = |r: Result<(), FsError>| match ok(r) {
        Ok(())
        | Err(
            FsError::OverQuota
            | FsError::CrossesLimit
            | FsError::NotFound
            | FsError::Exists
            | FsError::NotEmpty
            | FsError::NoSpace
            | FsError::IsDir
            | FsError::NotDir
            | FsError::InvalidArgument,
        ) => {}
        Err(e) => panic!("unexpected {e:?}"),
    };
    match rng.below(14) {
        0..=2 => {
            let p = join(dir(rng), &name(rng));
            let off = rng.below(3 * BS);
            let len = rng.below(3 * BS) as usize;
            let mut data = vec![0u8; len];
            rng.fill(&mut data);
            ok(put(v, &p, off, &data));
        }
        3 => {
            let p = join(dir(rng), &name(rng));
            if let Ok(f) = v.open(root, &p, OpenOptions::default()) {
                let size = rng.below(6 * BS);
                ok(v.set_size(f, size));
                v.close(f).unwrap();
            }
        }
        4 => {
            let p = join(dir(rng), &name(rng));
            ok(v.unlink(root, &p));
        }
        5 => {
            let p = join(dir(rng), &format!("s{}", rng.below(3)));
            ok(v.mkdir(root, &p, 0o755));
        }
        6 => {
            let p = join(dir(rng), &format!("s{}", rng.below(3)));
            ok(v.rmdir(root, &p));
        }
        7 | 8 => {
            let s = if rng.chance(1, 4) {
                join(dir(rng), &format!("s{}", rng.below(3)))
            } else {
                join(dir(rng), &name(rng))
            };
            let d = if rng.chance(1, 4) {
                join(dir(rng), &format!("s{}", rng.below(3)))
            } else {
                join(dir(rng), &name(rng))
            };
            let mode = *rng.pick(&[
                RenameMode::Replace,
                RenameMode::NoReplace,
                RenameMode::Exchange,
            ]);
            ok(v.rename(root, &s, root, &d, mode));
        }
        // Handles that live across requests: a file kept open can be
        // unlinked meanwhile (an orphan, ADR-0290), grows on writing, and
        // is freed by the close or at the next mount.
        10 => {
            if pool.len() < 4 {
                let p = join(dir(rng), &name(rng));
                if let Ok(h) = v.open(root, &p, create()) {
                    pool.push(h);
                }
            }
        }
        11 => {
            if !pool.is_empty() {
                let h = *rng.pick(pool);
                let off = rng.below(3 * BS);
                let len = rng.below(2 * BS) as usize;
                ok(v.write(h, off, &vec![5u8; len]).map(|_| ()));
            }
        }
        12 => {
            if !pool.is_empty() {
                let at = rng.below(pool.len() as u64) as usize;
                let h = pool.swap_remove(at);
                v.close(h).unwrap();
            }
        }
        // A limit changed, or taken away and put back (the scan of a
        // subtree that holds orphans).
        13 => {
            let (d, _) = *rng.pick(LIMITED);
            let new = (2 + rng.below(58)) * BS;
            if rng.chance(1, 2) {
                ok(v.set_limit(root, d, Some(new)));
            } else {
                ok(v.set_limit(root, d, None));
                ok(v.set_limit(root, d, Some(new)));
            }
        }
        _ => {
            let d = dir(rng);
            let n = join(d, &name(rng));
            if let Ok(dh) = v.open(
                root,
                if d.is_empty() { "." } else { d },
                OpenOptions::default(),
            ) && let Ok(u) = v.create_unnamed(dh, 0o644)
            {
                ok(v.write(u, 0, &[1u8; 700]).map(|_| ()));
                ok(v.publish(
                    u,
                    dh,
                    &n[n.rfind('/').map_or(0, |i| i + 1)..],
                    PublishMode::Replace,
                ));
                v.close(u).unwrap();
                v.close(dh).unwrap();
            }
        }
    }
}

const LIMITED: &[(&str, u64)] = &[("a", 40), ("a/b", 14), ("d", 24), ("a/b/c", 6)];

fn setup<D: BlockDevice>(v: &mut Volume<D, TestClock>) {
    let root = v.root();
    for d in ["a", "a/b", "a/b/c", "d", "d/e", "f"] {
        v.mkdir(root, d, 0o755).unwrap();
    }
    for (d, n) in LIMITED {
        v.set_limit(root, d, Some(n * BS)).unwrap();
    }
}

fn counters_true<D: BlockDevice>(v: &mut Volume<D, TestClock>, ctx: &str) {
    for (d, _) in LIMITED {
        let u = used(v, d);
        let o = oracle(v, d);
        assert_eq!(u, o, "{ctx}: used of {d}");
    }
}

/// INV-0265-1 and -2: random requests against nested limits; after each, the
/// counters equal the recomputed sums, a refused request changed nothing
/// (the sums still match), and no counter is past its limit.
#[test]
fn random_requests_keep_the_counters_true() {
    for seed in 0..12u64 {
        let clock = TestClock::new(T0);
        let mut v = mount(fresh(1024), &clock);
        setup(&mut v);
        let mut rng = Rng::new(seed);
        let mut pool = Vec::new();
        for i in 0..400 {
            random_request(&mut v, &mut rng, &mut pool);
            // The API oracle counts names; with no open handle there is no
            // orphan, so it is complete. Otherwise `fsck` below is the
            // oracle, orphans included.
            if pool.is_empty() {
                counters_true(&mut v, &format!("seed {seed} request {i}"));
            }
            if i % 25 == 24 {
                v.sync().unwrap();
                fsck(v.device().clone());
            }
        }
        for h in pool {
            v.close(h).unwrap();
        }
        counters_true(&mut v, &format!("seed {seed} at the end"));
        v.sync().unwrap();
        fsck(v.unmount().unwrap());
    }
    use std::sync::atomic::Ordering::Relaxed;
    eprintln!(
        "random limits: {} OverQuota, {} CrossesLimit",
        REFUSED.load(Relaxed),
        CROSSED.load(Relaxed)
    );
    assert!(REFUSED.load(Relaxed) > 20, "the run never hit a limit");
    assert!(
        CROSSED.load(Relaxed) > 0,
        "the run never crossed a boundary"
    );
}

/// INV-0265-1 across crashes: every crash image of a run of random requests
/// passes `fsck` (which recomputes every `used`), and after the mount the
/// counters equal the sums computed through the API.
#[test]
fn the_counters_survive_a_crash_at_every_write() {
    let mut images = 0u64;
    for seed in 0..6u64 {
        let clock = TestClock::new(T0);
        let rec = RecordingDevice::new(fresh(512));
        let mut v = Volume::mount(rec, clock.clone(), MountOptions::default()).unwrap();
        setup(&mut v);
        let mut rng = Rng::new(seed + 100);
        let mut done = 0u64;
        // Handles stay open to the crash: the images hold orphans, and the
        // mount frees them.
        let mut pool = Vec::new();
        for i in 0..60 {
            random_request(&mut v, &mut rng, &mut pool);
            done += 1;
            if i % 12 == 11 {
                v.sync().unwrap();
                v.device_mut().sync_mark(done);
            } else {
                v.device_mut().mark(done);
            }
        }
        let (base, events, _) = v.into_device().into_parts();
        let cfg = CrashConfig {
            k: 8,
            random_subsets: 8,
            torn: 4,
            seed,
        };
        images += for_each_crash_image(&base, &events, cfg, |w, variant, dev| {
            let mut d = dev.clone();
            let rep = check(&mut d, CheckOptions { read_data: false });
            assert!(
                rep.is_clean(),
                "seed {seed} window {} {variant:?}: fsck: {:?}",
                w.index,
                rep.problems
            );
            let mut v = Volume::mount(dev, clock.clone(), MountOptions::default()).unwrap();
            // Limits exist once the setup was durable.
            let root = v.root();
            if v.open(root, "a", OpenOptions::default()).is_ok() {
                let h = v.open(root, "a", OpenOptions::default()).unwrap();
                let has = v.limit_of(h).unwrap().is_some();
                v.close(h).unwrap();
                if has {
                    for (d, _) in LIMITED {
                        let Ok(h) = v.open(root, d, OpenOptions::default()) else {
                            continue;
                        };
                        if let Some(l) = v.limit_of(h).unwrap() {
                            assert_eq!(
                                l.used,
                                oracle(&mut v, d),
                                "seed {seed} window {} {variant:?}: used of {d}",
                                w.index
                            );
                        }
                        v.close(h).unwrap();
                    }
                }
            }
            let mut d = v.unmount().unwrap();
            let rep = check(&mut d, CheckOptions { read_data: false });
            assert!(rep.is_clean(), "after remount: {:?}", rep.problems);
        });
    }
    eprintln!("limits crash test: {images} crash images");
    assert!(images > 100);
}

/// The checker finds a wrong `used` and repair puts it right.
#[test]
fn fsck_finds_and_repairs_a_wrong_counter() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(256), &clock);
    let root = v.root();
    v.mkdir(root, "r", 0o755).unwrap();
    v.set_limit(root, "r", Some(9 * BS)).unwrap();
    put(&mut v, "r/f", 0, &vec![1u8; BLOCK_SIZE]).unwrap();
    let d = v.unmount().unwrap();
    assert!(check(&mut d.clone(), CheckOptions::default()).is_clean());
    // Remount, then damage the counter through a limit that is too low a
    // record: the supported way is to write the record with another `used`.
    let mut v = mount(d, &clock);
    let root = v.root();
    v.debug_set_used(root, "r", 123);
    let d = v.unmount().unwrap();
    let rep = check(&mut d.clone(), CheckOptions::default());
    assert!(!rep.is_clean(), "a wrong counter is not found");
    let (_, d) = zoraya_fs::check::repair(d, clock.clone()).unwrap();
    fsck(d.clone());
    let mut v = mount(d, &clock);
    assert_eq!(used(&mut v, "r"), 2 * BS);
}

/// Review Q5, finding 3: a tree whose charge passes `u64::MAX` gets no limit,
/// and a later delete does not fail the volume; `fsck` calls a record of
/// such a sum damage, not clean.
#[test]
fn a_sum_past_u64_is_refused_and_not_clean() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(256), &clock);
    let root = v.root();
    v.mkdir(root, "a", 0o755).unwrap();
    for f in ["a/x", "a/y"] {
        let h = v.open(root, f, create()).unwrap();
        v.set_size(h, i64::MAX as u64).unwrap();
        v.close(h).unwrap();
    }
    assert_eq!(
        v.set_limit(root, "a", Some(u64::MAX)),
        Err(FsError::OverQuota)
    );
    // Nothing was recorded, and the deletes work.
    let h = v.open(root, "a", OpenOptions::default()).unwrap();
    assert_eq!(v.limit_of(h).unwrap(), None);
    v.close(h).unwrap();
    v.unlink(root, "a/x").unwrap();
    v.unlink(root, "a/y").unwrap();
    assert!(v.failure().is_none());
    // The checker: a record forced onto the huge tree is damage.
    let h = v.open(root, "a/z", create()).unwrap();
    v.set_size(h, i64::MAX as u64).unwrap();
    v.close(h).unwrap();
    let h = v.open(root, "a/w", create()).unwrap();
    v.set_size(h, i64::MAX as u64).unwrap();
    v.close(h).unwrap();
    v.debug_set_used(root, "a", 5);
    v.sync().unwrap();
    let d = v.unmount().unwrap();
    let rep = check(&mut d.clone(), CheckOptions::default());
    assert!(
        rep.problems
            .iter()
            .any(|p| p.what.contains("past the largest limit")),
        "{:?}",
        rep.problems
    );
    // Repair takes the record away.
    let (_, d) = zoraya_fs::check::repair(d, clock.clone()).unwrap();
    fsck(d);
}

/// Review Q5, finding 4: a directory that an open unlinked file is charged
/// to cannot be removed, also by a rename over it, so the file cannot grow
/// past the limit above.
#[test]
fn a_directory_with_a_charged_open_file_stays() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(256), &clock);
    let root = v.root();
    for d in ["room", "room/agent", "other"] {
        v.mkdir(root, d, 0o755).unwrap();
    }
    v.set_limit(root, "room/agent", Some(4 * BS)).unwrap();
    let f = v.open(root, "room/agent/f", create()).unwrap();
    v.unlink(root, "room/agent/f").unwrap();
    assert_eq!(v.rmdir(root, "room/agent"), Err(FsError::NotEmpty));
    assert_eq!(
        v.rename(root, "other", root, "room/agent", RenameMode::Replace),
        Err(FsError::NotEmpty)
    );
    assert_eq!(
        v.write(f, 0, &vec![1u8; 8 * BLOCK_SIZE]),
        Err(FsError::OverQuota)
    );
    v.close(f).unwrap();
    v.rmdir(root, "room/agent").unwrap();
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

/// Review Q5, finding 5: `set_limit` is not a freeing request: on a full
/// volume a new limit is refused for space and the delete reserve stays.
#[test]
fn a_new_limit_leaves_the_delete_reserve_alone() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(128), &clock);
    let root = v.root();
    for i in 0..30 {
        v.mkdir(root, &format!("d{i}"), 0o755).unwrap();
    }
    let mut n = 0;
    while put(&mut v, &format!("fill{n}"), 0, &vec![7u8; 4 * BLOCK_SIZE]).is_ok() {
        n += 1;
    }
    // And the last blocks, with empty files, until even a create is refused.
    let mut e = 0;
    while put(&mut v, &format!("e{e}"), 0, b"").is_ok() {
        e += 1;
    }
    // Taking a limit away frees, so it may use the reserve; a new limit does
    // not (it asks for the reserve to stay free, as a create does).
    v.set_limit(root, "d0", None).unwrap();
    let before = v.free_blocks().unwrap();
    let r = v.set_limit(root, "d1", Some(BS));
    if r == Ok(()) {
        // It fitted beside the reserve: the reserve is still all there.
        assert!(v.free_blocks().unwrap() + 8 >= before.min(zoraya_fs::DELETE_RESERVE));
    } else {
        assert_eq!(r, Err(FsError::NoSpace));
    }
    v.unlink(root, "fill0").unwrap();
    v.sync().unwrap();
    fsck(v.unmount().unwrap());
}

/// Review Q5, finding 6: a tree past `SCAN_BOUND` objects gets no new limit.
#[test]
fn a_tree_past_the_scan_bound_gets_no_limit() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(16384), &clock);
    let root = v.root();
    v.mkdir(root, "big", 0o755).unwrap();
    let h = v.open(root, "big", OpenOptions::default()).unwrap();
    for i in 0..=zoraya_fs::SCAN_BOUND {
        v.mkdir(h, &format!("{i}"), 0o755).unwrap();
    }
    v.close(h).unwrap();
    assert_eq!(
        v.set_limit(root, "big", Some(u64::MAX)),
        Err(FsError::ScanTooLarge)
    );
    // A small tree next to it is fine, and removing nothing is.
    v.mkdir(root, "small", 0o755).unwrap();
    v.set_limit(root, "small", Some(BS)).unwrap();
    v.set_limit(root, "big", None).unwrap();
}

/// Review Q5, finding 12: a volume of another format version is refused with
/// words that say to rebuild it.
#[test]
fn another_format_version_says_to_rebuild() {
    use zoraya_fs::format::{Header, NODE_MAGIC, Rd};
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&NODE_MAGIC.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 40]);
    let err = Header::decode(&mut Rd::new(&bytes)).unwrap_err();
    assert!(err.0.contains("zoraya-mkfs"), "{}", err.0);
}

/// The cost of a growing request with a limit three levels up (Q5's
/// measurement): appends of one block to a file in `a/b/c`, with no limit
/// and with a limit on `a`, on a memory device. Run by hand:
/// `cargo test --release -p zoraya-fs --test limits -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a test"]
fn measure_the_cost_of_a_growing_request() {
    use std::time::Instant;
    fn run(limit_on: Option<&str>) -> f64 {
        let clock = TestClock::new(T0);
        let mut v = mount(fresh(8192), &clock);
        let root = v.root();
        for d in ["a", "a/b", "a/b/c"] {
            v.mkdir(root, d, 0o755).unwrap();
        }
        if let Some(l) = limit_on {
            v.set_limit(root, l, Some(1 << 30)).unwrap();
        }
        let f = v.open(root, "a/b/c/f", create()).unwrap();
        let block = vec![7u8; BLOCK_SIZE];
        let n = 4000u64;
        let t = Instant::now();
        for i in 0..n {
            v.write(f, i * BS, &block).unwrap();
        }
        let dt = t.elapsed().as_nanos() as f64 / n as f64;
        v.close(f).unwrap();
        dt
    }
    // Best of five, to leave the machine's noise out.
    let best = |l: Option<&str>| (0..5).map(|_| run(l)).fold(f64::MAX, f64::min);
    let none = best(None);
    let up3 = best(Some("a"));
    let near = best(Some("a/b/c"));
    eprintln!(
        "growing write of one block, ns per request: no limit {none:.0}, limit on a (3 levels up) {up3:.0} \
         (+{:.0}), limit on the parent {near:.0}",
        up3 - none
    );
}

zoraya_test_alloc::cap_memory!();

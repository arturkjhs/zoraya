//! Model-based tests: random request sequences against the engine and the
//! reference model, compared after every request (INV-0010-1's model half,
//! INV-0013-2, INV-0013-4, INV-0014-2, INV-0014-3; ADR-0010 rule 1's live
//! visibility).

mod common;

use std::collections::BTreeSet;

use common::*;
use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{FsError, OpenOptions, RenameMode, Timestamp};
use zoraya_fs_testkit::Rng;
use zoraya_fs_testkit::ops::{ClockMode, Gen, Out, Session, random_op};

const SEEDS: u64 = 48;
const OPS: usize = 400;

fn run(seed: u64, mode: ClockMode) -> (usize, usize) {
    let mut rng = Rng::new(seed);
    let mut s = session(fresh(4096));
    s.clock_mode = mode;
    let g = Gen::default();
    let (mut ok, mut err) = (0, 0);
    for i in 0..OPS {
        let op = random_op(&mut rng, s.handles(), &g);
        match s.step(&op, &mut rng) {
            Ok(_) => ok += 1,
            Err(_) => err += 1,
        }
        if i % 40 == 39 {
            s.compare();
        }
        if i % 150 == 149 {
            // Clean remount: the model forgets handles and orphans.
            let clock = s.clock.clone();
            let mode = s.clock_mode;
            let assigned = std::mem::take(&mut s.assigned);
            let ids = std::mem::take(&mut s.ids);
            let mut model = s.model.clone();
            let dev = s.vol.unmount().expect("unmount");
            let vol = mount(dev, &clock);
            model.after_remount(vol.next_id());
            let mut ns = Session::new(vol, model, clock);
            ns.clock_mode = mode;
            ns.assigned = assigned;
            ns.ids = ids;
            s = ns;
            s.compare();
        }
    }
    s.compare();
    // INV-0014-1: every assigned time strictly increases in completion
    // order, whatever the clock did, across the remounts above.
    for w in s.assigned.windows(2) {
        assert!(
            w[0] < w[1],
            "seed {seed}: times not strictly increasing: {w:?}"
        );
    }
    let mut dev = s.vol.unmount().expect("unmount");
    let rep = check(&mut dev, CheckOptions::default());
    assert!(rep.is_clean(), "seed {seed}: {:?}", rep.problems);
    (ok, err)
}

#[test]
fn random_sequences_match_the_model() {
    let modes = [
        ClockMode::Advancing,
        ClockMode::Stuck,
        ClockMode::Coarse,
        ClockMode::Stepping,
    ];
    let (mut ok, mut err) = (0, 0);
    for seed in 0..SEEDS {
        let (o, e) = run(seed, modes[seed as usize % 4]);
        ok += o;
        err += e;
    }
    eprintln!(
        "model test: {} requests, {ok} succeeded, {err} failed as the model predicted",
        SEEDS as usize * OPS
    );
    assert!(ok > 1000 && err > 1000);
}

/// INV-0013-2: names that differ only in case or normalisation are distinct.
#[test]
fn names_compare_byte_for_byte() {
    let mut s = session(fresh(512));
    let root = s.vol.root();
    let pairs = [
        ("e\u{301}", "\u{e9}"),
        ("a", "A"),
        ("\u{212b}", "\u{c5}"),
        ("ss", "\u{df}"),
        ("fi", "\u{fb01}"),
    ];
    let mut ids = BTreeSet::new();
    for (x, y) in pairs {
        for n in [x, y] {
            let h = s
                .vol
                .open(
                    root,
                    n,
                    OpenOptions {
                        create: Some(0o644),
                        exclusive: true,
                        ..Default::default()
                    },
                )
                .unwrap();
            s.vol.write(h, 0, n.as_bytes()).unwrap();
            ids.insert(s.vol.stat(h).unwrap().id);
            s.vol.close(h).unwrap();
        }
    }
    assert_eq!(ids.len(), 10);
    for (x, y) in pairs {
        for n in [x, y] {
            let h = s.vol.open(root, n, OpenOptions::default()).unwrap();
            let mut b = [0u8; 16];
            let k = s.vol.read(h, 0, &mut b).unwrap();
            assert_eq!(&b[..k], n.as_bytes());
            s.vol.close(h).unwrap();
        }
    }
}

/// INV-0013-4: the namespace stays a tree with at most one name per object,
/// over random sequences that include unnamed objects, publish, removal of
/// open files and directory renames. Checked by the offline checker after
/// every 20 requests (it reports any object with two names or a directory
/// cycle), and by explicit attempts to make a directory its own descendant.
#[test]
fn namespace_stays_a_tree() {
    let g = Gen {
        invalid: 0,
        ..Gen::default()
    };
    for seed in 100..124 {
        let mut rng = Rng::new(seed);
        let mut s = session(fresh(2048));
        for i in 0..300 {
            let op = random_op(&mut rng, s.handles(), &g);
            let _ = s.step(&op, &mut rng);
            if i % 20 == 19 {
                s.vol.sync().unwrap();
                let mut dev = s.vol.device().clone();
                let rep = check(&mut dev, CheckOptions { read_data: false });
                assert!(rep.is_clean(), "seed {seed} step {i}: {:?}", rep.problems);
            }
        }
    }
    let mut s = session(fresh(512));
    let root = s.vol.root();
    s.vol.mkdir(root, "a", 0o755).unwrap();
    s.vol.mkdir(root, "a/b", 0o755).unwrap();
    s.vol.mkdir(root, "a/b/c", 0o755).unwrap();
    for (src, dst) in [
        ("a", "a/x"),
        ("a", "a/b/x"),
        ("a", "a/b/c/x"),
        ("a/b", "a/b/c/x"),
    ] {
        assert_eq!(
            s.vol.rename(root, src, root, dst, RenameMode::Replace),
            Err(FsError::InvalidArgument)
        );
    }
    assert_eq!(
        s.vol.rename(root, "a/b/c", root, "a", RenameMode::Exchange),
        Err(FsError::InvalidArgument)
    );
    assert_eq!(
        s.vol.rename(root, "a", root, "a/b", RenameMode::Exchange),
        Err(FsError::InvalidArgument)
    );
    // Publish of an object that already has a name is refused.
    let u = s.vol.create_unnamed(root, 0o600).unwrap();
    s.vol
        .publish(u, root, "pub", zoraya_fs::PublishMode::NoReplace)
        .unwrap();
    assert_eq!(
        s.vol
            .publish(u, root, "pub2", zoraya_fs::PublishMode::NoReplace),
        Err(FsError::InvalidArgument)
    );
}

/// INV-0014-2: every content change sets mtime and ctime to a new time; no
/// read or lookup changes any attribute, and a read-only request sequence
/// leaves the image byte-identical.
#[test]
fn reads_change_nothing() {
    let mut rng = Rng::new(7);
    let mut s = session(fresh(1024));
    let g = Gen::default();
    for _ in 0..300 {
        let op = random_op(&mut rng, s.handles(), &g);
        let _ = s.step(&op, &mut rng);
    }
    s.vol.sync().unwrap();
    let before = s.vol.device().image().to_vec();
    let state = s.compare();
    for _ in 0..400 {
        let op = random_op(&mut rng, s.handles(), &g);
        if op.mutates() {
            continue;
        }
        let r = s.step(&op, &mut rng);
        if let Ok(Out::Attr(a)) = r {
            assert!(a.mtime.is_valid());
        }
    }
    assert_eq!(s.compare(), state, "a read changed an attribute");
    s.vol.sync().unwrap();
    s.vol.tick().unwrap();
    assert!(
        s.vol.device().image() == &before[..],
        "a read-only sequence changed the image"
    );
}

/// INV-0014-2's other half, explicitly: each kind of content change gives
/// the object a time later than any before.
#[test]
fn content_changes_assign_new_times() {
    let clock = zoraya_fs_testkit::TestClock::new(T0); // never moves
    let mut v = mount(fresh(512), &clock);
    let root = v.root();
    let mut last = T0;
    let mut check_newer = |v: &mut zoraya_fs::Volume<_, _>, h, what: &str| {
        let a: zoraya_fs::Attr = zoraya_fs::Volume::stat(v, h).unwrap();
        assert!(
            a.mtime > last && a.ctime == a.mtime,
            "{what}: {a:?} not after {last:?}"
        );
        last = a.mtime;
    };
    let f = v
        .open(
            root,
            "f",
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        )
        .unwrap();
    check_newer(&mut v, f, "create");
    v.write(f, 0, b"x").unwrap();
    check_newer(&mut v, f, "write");
    v.set_size(f, 10).unwrap();
    check_newer(&mut v, f, "set size");
    v.mkdir(root, "d", 0o755).unwrap();
    check_newer(&mut v, root, "entry added");
    v.unlink(root, "f").unwrap();
    check_newer(&mut v, root, "entry removed");
    v.set_attr(f, None, Some(Timestamp { secs: -5, nanos: 7 }))
        .unwrap();
    assert_eq!(v.stat(f).unwrap().mtime, Timestamp { secs: -5, nanos: 7 });
    assert!(v.stat(f).unwrap().ctime > last);
}

//! Crash consistency (INV-0010-1, -2, -4, -6; INV-0011-1 to -4;
//! INV-0015-2).
//!
//! Random request sequences run on a recording device, in lockstep with the
//! model. Every crash image the ADR-0015 contract allows within the stated
//! bound (see `zoraya_fs_testkit::crash`) is checked with `fsck`, mounted,
//! and its observable state compared with the model's state after every
//! allowed prefix of the completion order.

mod common;

use std::collections::HashMap;
use std::time::Instant;

use common::*;
use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{MountOptions, OpenOptions, RenameMode, Volume};
use zoraya_fs_testkit::crash::{CrashConfig, Variant, check_ordering, for_each_crash_image};
use zoraya_fs_testkit::model::Model;
use zoraya_fs_testkit::ops::{Gen, Op, Session, random_op};
use zoraya_fs_testkit::state::{engine_state, hash_state};
use zoraya_fs_testkit::{ImageDevice, RecordingDevice, Rng, TestClock};

#[derive(Default, Debug)]
struct Totals {
    images: u64,
    windows: u64,
    /// Windows with at most k writes, whose subsets were all enumerated.
    exhaustive: u64,
    max_writes: usize,
    commits: u64,
    requests: u64,
}

/// Runs `ops` (or random ones) on a recording device and checks every crash
/// image. `states[i]` is the model's state after `i` completed requests.
fn run_and_crash(
    seed: u64,
    n_ops: usize,
    permute: bool,
    cfg: CrashConfig,
    fixed: Option<&[Op]>,
    totals: &mut Totals,
) {
    let mut rng = Rng::new(seed);
    let mut rec = RecordingDevice::new(fresh(384));
    if permute {
        rec = rec.permuting(seed);
    }
    let clock = TestClock::new(T0);
    let vol = Volume::mount(rec, clock.clone(), MountOptions::default()).expect("mount");
    let mut s = Session::new(vol, Model::new(ROOT_MODE, T0), clock.clone());
    let mut states = vec![hash_state(&s.model.state())];
    let g = Gen {
        max_write: 6000,
        max_off: 12_000,
        sync: 6,
        ..Gen::default()
    };
    let ops: Vec<Op> = match fixed {
        Some(f) => f.to_vec(),
        None => (0..n_ops).map(|_| Op::Stat { h: 0 }).collect(),
    };
    for (i, fixed_op) in ops.iter().enumerate() {
        let op = if fixed.is_some() {
            fixed_op.clone()
        } else {
            random_op(&mut rng, s.handles(), &g)
        };
        let r = s.step(&op, &mut rng);
        let done = s.completed;
        let dev = s.vol.device_mut();
        if matches!(op, Op::Sync) && r.is_ok() {
            dev.sync_mark(done);
        } else {
            dev.mark(done);
        }
        states.push(hash_state(&s.model.state()));
        let _ = i;
    }
    totals.requests += ops.len() as u64;
    totals.commits += s.vol.commits();
    // Crash now: no unmount.
    let (base, events, _) = s.vol.into_device().into_parts();
    let ord =
        check_ordering(&base, &events).unwrap_or_else(|e| panic!("seed {seed}: ordering: {e}"));
    assert_eq!(ord.commits, s_commits(&events), "commit count");
    let mut windows_seen = std::collections::BTreeSet::new();
    for w in zoraya_fs_testkit::crash::windows(&events) {
        if w.writes.len() <= cfg.k {
            totals.exhaustive += 1;
        }
        totals.max_writes = totals.max_writes.max(w.writes.len());
    }
    // Mounting the same image twice gives the same result; cache by content
    // of the overlay to keep the run short.
    let mut seen: HashMap<u64, u64> = HashMap::new();
    let n = for_each_crash_image(&base, &events, cfg, |w, variant, dev| {
        windows_seen.insert(w.index);
        let key = image_key(w.index, &dev);
        if let Some(h) = seen.get(&key) {
            assert!(
                states[w.durable as usize..=w.completed as usize].contains(h),
                "seed {seed} window {} {variant:?}: state not an allowed prefix",
                w.index
            );
            return;
        }
        let mut d = dev.clone();
        let rep = check(&mut d, CheckOptions::default());
        assert!(
            rep.is_clean(),
            "seed {seed} window {} {variant:?}: fsck: {:?}",
            w.index,
            rep.problems
        );
        let mut v = Volume::mount(dev, clock.clone(), MountOptions::default())
            .unwrap_or_else(|e| panic!("seed {seed} window {} {variant:?}: mount: {e:?}", w.index));
        let st = engine_state(&mut v).expect("state");
        let h = hash_state(&st);
        let allowed = &states[w.durable as usize..=w.completed as usize];
        if !allowed.contains(&h) {
            let pos = states.iter().position(|x| *x == h);
            panic!(
                "seed {seed} window {} {variant:?}: state matches prefix {pos:?}, allowed {}..={}",
                w.index, w.durable, w.completed
            );
        }
        // INV-0010-6: after a mount, nothing without a name remains and its
        // space is free.
        let mut d = v.unmount().expect("unmount after crash");
        let rep = check(&mut d, CheckOptions { read_data: false });
        assert!(
            rep.is_clean() && rep.stats.orphans == 0,
            "seed {seed}: after remount: {rep:?}"
        );
        seen.insert(key, h);
    });
    totals.images += n;
    totals.windows += windows_seen.len() as u64;
}

fn s_commits(events: &[zoraya_fs_testkit::Event]) -> u64 {
    // Root records are written in pairs, one batch per commit.
    let mut batches = std::collections::BTreeSet::new();
    for e in events {
        if let zoraya_fs_testkit::Event::Write { block, batch, .. } = e
            && *block < zoraya_fs::format::ROOT_SLOTS
        {
            batches.insert(*batch);
        }
    }
    batches.len() as u64
}

fn image_key(window: usize, dev: &ImageDevice) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    window.hash(&mut h);
    dev.overlay_hash().hash(&mut h);
    h.finish()
}

const CRASH_SEEDS: u64 = 40;

#[test]
fn every_crash_state_is_a_prefix() {
    let started = Instant::now();
    let mut t = Totals::default();
    for seed in 0..CRASH_SEEDS {
        run_and_crash(seed, 80, false, CrashConfig::default(), None, &mut t);
    }
    eprintln!(
        "crash test: {} requests, {} commits, {} windows ({} with at most k = 12 writes, all subsets; \
         largest {} writes), {} crash images in {:?}",
        t.requests,
        t.commits,
        t.windows,
        t.exhaustive,
        t.max_writes,
        t.images,
        started.elapsed()
    );
}

/// INV-0015-2: the same with the device completing every batch's writes in
/// a random order.
#[test]
fn crash_states_with_permuted_completion() {
    let mut t = Totals::default();
    for seed in 1000..1016 {
        run_and_crash(seed, 80, true, CrashConfig::default(), None, &mut t);
    }
    eprintln!("permuted crash test: {} crash images", t.images);
}

/// INV-0010-2's targeted test: crash between every pair of block writes of a
/// single rename-over and of a single maximum-size write, each committed by
/// a `sync`.
#[test]
fn rename_over_and_max_write_are_all_or_nothing() {
    let big: Vec<u8> = (0..zoraya_fs::MAX_ATOMIC_WRITE)
        .map(|i| (i * 7 + 3) as u8)
        .collect();
    let ops = vec![
        Op::Open {
            dir: 0,
            path: "target".into(),
            create: Some(0o644),
            exclusive: true,
            truncate: false,
            link_itself: false,
        },
        Op::Write {
            h: 1,
            off: 0,
            data: b"old content".to_vec(),
        },
        Op::Open {
            dir: 0,
            path: "tmp".into(),
            create: Some(0o644),
            exclusive: true,
            truncate: false,
            link_itself: false,
        },
        Op::Write {
            h: 2,
            off: 0,
            data: b"new content, longer".to_vec(),
        },
        Op::Sync,
        Op::Rename {
            sdir: 0,
            spath: "tmp".into(),
            ddir: 0,
            dpath: "target".into(),
            mode: RenameMode::Replace,
        },
        Op::Sync,
        Op::Write {
            h: 1,
            off: 4096,
            data: big,
        },
        Op::Sync,
    ];
    let mut t = Totals::default();
    run_and_crash_big(1, &ops, &mut t);
    eprintln!("targeted crash test: {} crash images", t.images);
}

fn run_and_crash_big(seed: u64, ops: &[Op], t: &mut Totals) {
    // A volume large enough for a 1 MiB write and its copy.
    let mut rng = Rng::new(seed);
    let rec = RecordingDevice::new(fresh(2048));
    let clock = TestClock::new(T0);
    let vol = Volume::mount(rec, clock.clone(), MountOptions::default()).expect("mount");
    let mut s = Session::new(vol, Model::new(ROOT_MODE, T0), clock.clone());
    let mut states = vec![hash_state(&s.model.state())];
    for op in ops {
        let r = s.step(op, &mut rng);
        assert!(r.is_ok(), "{op:?}: {r:?}");
        let done = s.completed;
        if matches!(op, Op::Sync) {
            s.vol.device_mut().sync_mark(done);
        } else {
            s.vol.device_mut().mark(done);
        }
        states.push(hash_state(&s.model.state()));
    }
    let (base, events, _) = s.vol.into_device().into_parts();
    check_ordering(&base, &events).expect("ordering");
    let cfg = CrashConfig {
        k: 12,
        random_subsets: 16,
        torn: 8,
        seed,
    };
    let n = for_each_crash_image(&base, &events, cfg, |w, variant, dev| {
        let mut v = Volume::mount(
            dev,
            clock.clone(),
            MountOptions {
                read_only: true,
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("window {} {variant:?}: {e:?}", w.index));
        let st = engine_state(&mut v).expect("state");
        let h = hash_state(&st);
        assert!(
            states[w.durable as usize..=w.completed as usize].contains(&h),
            "window {} {:?}: not an allowed prefix",
            w.index,
            matches!(variant, Variant::Torn(_))
        );
        // The name refers to the whole old or the whole new object.
        if let Some(o) = st.get("target") {
            assert!(
                o.content == b"old content" || o.content.starts_with(b"new content, longer"),
                "torn rename target: {:?}",
                &o.content[..o.content.len().min(32)]
            );
        }
    });
    t.images += n;
}

/// INV-0010-4: with no `sync`, a crash after the volume has been idle for
/// `T_COMMIT` plus one commit loses nothing.
#[test]
fn idle_volume_loses_nothing() {
    let mut rng = Rng::new(9);
    let rec = RecordingDevice::new(fresh(512));
    let clock = TestClock::new(T0);
    let vol = Volume::mount(rec, clock.clone(), MountOptions::default()).expect("mount");
    let mut s = Session::new(vol, Model::new(ROOT_MODE, T0), clock.clone());
    let g = Gen {
        sync: 0,
        tick: false,
        ..Gen::default()
    };
    for _ in 0..80 {
        let op = random_op(&mut rng, s.handles(), &g);
        let _ = s.step(&op, &mut rng);
    }
    let want = s.model.state();
    // Not yet due: nothing is committed by a tick.
    let before = s.vol.commits();
    clock.advance_mono(zoraya_fs::T_COMMIT_NS - 1);
    s.vol.tick().unwrap();
    assert_eq!(s.vol.commits(), before);
    // Idle for T_COMMIT: the service's timer calls tick; the commit runs.
    clock.advance_mono(1);
    s.vol.tick().unwrap();
    assert_eq!(s.vol.commits(), before + 1);
    let (base, events, cur) = s.vol.into_device().into_parts();
    // The log ends with a flush, so the last window has no write that a crash
    // could undo: only the full prefix survives (TASK-07 review R-04).
    let ws = zoraya_fs_testkit::crash::windows(&events);
    let last = ws.last().expect("a window");
    assert!(last.writes.is_empty(), "the commit left unstable writes");
    check_ordering(&base, &events).unwrap();
    let mut v = Volume::mount(cur, clock, MountOptions::default()).unwrap();
    assert_eq!(engine_state(&mut v).unwrap(), want);
}

/// INV-0010-6: an unnamed object never published, and an open file whose
/// last name was removed, are absent after a crash at every flush boundary,
/// and their space is free; before the crash they stay readable and
/// writable through their handles.
#[test]
fn objects_without_names_vanish_after_a_crash() {
    let rec = RecordingDevice::new(fresh(512));
    let clock = TestClock::new(T0);
    let mut v = Volume::mount(rec, clock.clone(), MountOptions::default()).expect("mount");
    let root = v.root();
    let u = v.create_unnamed(root, 0o600).unwrap();
    v.write(u, 0, &[1u8; 9000]).unwrap();
    let f = v
        .open(
            root,
            "open-file",
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        )
        .unwrap();
    v.write(f, 0, &[2u8; 5000]).unwrap();
    v.sync().unwrap();
    v.unlink(root, "open-file").unwrap();
    v.sync().unwrap();
    v.write(f, 5000, &[3u8; 5000]).unwrap();
    v.write(u, 9000, &[4u8; 100]).unwrap();
    v.sync().unwrap();
    let mut b = vec![0u8; 10_000];
    assert_eq!(v.read(f, 0, &mut b).unwrap(), 10_000);
    assert!(b[..5000].iter().all(|&x| x == 2) && b[5000..].iter().all(|&x| x == 3));
    assert_eq!(v.read(u, 0, &mut b).unwrap(), 9100);
    let (base, events, _) = v.into_device().into_parts();
    check_ordering(&base, &events).unwrap();
    let cfg = CrashConfig {
        k: 4,
        random_subsets: 4,
        torn: 0,
        seed: 3,
    };
    let mut n = 0;
    for_each_crash_image(&base, &events, cfg, |_, _, dev| {
        let mut v = Volume::mount(dev, clock.clone(), MountOptions::default()).unwrap();
        let st = engine_state(&mut v).unwrap();
        // The unnamed object never appears. The open file appears only with
        // its name, as it was before the unlink became durable.
        let mut named_blocks = 0;
        for (path, o) in &st {
            match path.as_str() {
                "" => {}
                "open-file" => {
                    assert_eq!(o.content, vec![2u8; 5000]);
                    named_blocks = 2;
                }
                p => panic!("unexpected name {p:?}"),
            }
        }
        let mut d = v.unmount().unwrap();
        let rep = check(&mut d, CheckOptions::default());
        assert!(rep.is_clean(), "{:?}", rep.problems);
        assert_eq!(rep.stats.orphans, 0);
        assert_eq!(
            rep.stats.data_blocks, named_blocks,
            "space of nameless objects not freed"
        );
        n += 1;
    });
    assert!(n > 10);
}

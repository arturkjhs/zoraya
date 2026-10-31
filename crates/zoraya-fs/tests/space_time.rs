//! Space reservation (INV-0010-5), and times and ids across clean and crash
//! remounts (INV-0014-1, INV-0014-3).

mod common;

use std::collections::BTreeSet;

use common::*;
use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{FsError, MountOptions, OpenOptions, Timestamp, Volume};
use zoraya_fs_testkit::crash::{CrashConfig, for_each_crash_image, windows};
use zoraya_fs_testkit::model::Model;
use zoraya_fs_testkit::ops::{ClockMode, Gen, Op, Session, random_op};
use zoraya_fs_testkit::state::{engine_state, hash_state};
use zoraya_fs_testkit::{MemDevice, RecordingDevice, Rng, TestClock};

/// INV-0010-5: filling a small volume with writes, overwrites and renames:
/// every commit succeeds, every refusal is `NoSpace` and changes nothing
/// (the session compares engine and model after each request, and the
/// model skips refused requests), and removing files works on a full
/// volume.
#[test]
fn full_volume_refuses_cleanly() {
    let mut refusals = 0;
    for seed in 0..12 {
        let mut rng = Rng::new(seed);
        let mut s = session(fresh(120));
        let g = Gen {
            max_write: 40_000,
            max_off: 60_000,
            invalid: 0,
            sync: 4,
            tick: true,
        };
        for i in 0..400 {
            let op = fill_op(&mut rng, s.handles(), &g);
            match s.step(&op, &mut rng) {
                Err(FsError::NoSpace) => refusals += 1,
                Err(FsError::StorageFailed) => panic!("seed {seed}: a commit failed: {op:?}"),
                _ => {}
            }
            assert_eq!(s.vol.failure(), None, "seed {seed}");
            if i % 25 == 0 {
                s.compare();
                assert!(s.vol.sync().is_ok(), "seed {seed}: sync failed");
            }
        }
        s.compare();
        // On the full volume, every name can still be removed.
        let st = s.model.state();
        let root = s.vol.root();
        let mut paths: Vec<_> = st.iter().filter(|(p, _)| !p.is_empty()).collect();
        paths.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        for (p, o) in paths {
            let r = if o.kind == zoraya_fs::ObjKind::Directory {
                s.vol.rmdir(root, p)
            } else {
                s.vol.unlink(root, p)
            };
            assert_eq!(r, Ok(()), "seed {seed}: removing {p}");
        }
        s.vol.close(root).unwrap();
        let mut d = s.vol.unmount().unwrap();
        let rep = check(&mut d, CheckOptions::default());
        assert!(rep.is_clean(), "{:?}", rep.problems);
    }
    eprintln!("full volume: {refusals} NoSpace refusals, no commit failed");
    assert!(refusals > 100);
}

/// Mostly large writes and overwrites to a few names, with renames over,
/// removals and syncs.
fn fill_op(rng: &mut Rng, handles: usize, g: &Gen) -> Op {
    let name = |rng: &mut Rng| ["f0", "f1", "f2", "f3", "d/f4"][rng.below(5) as usize].to_string();
    match rng.below(20) {
        0..=2 => Op::Open {
            dir: 0,
            path: name(rng),
            create: Some(0o644),
            exclusive: false,
            truncate: rng.chance(1, 4),
            link_itself: false,
        },
        3..=10 => {
            let mut data = vec![0u8; rng.below(g.max_write as u64) as usize];
            rng.fill(&mut data);
            Op::Write {
                h: rng.below(handles as u64) as usize,
                off: rng.below(g.max_off),
                data,
            }
        }
        11 => Op::Rename {
            sdir: 0,
            spath: name(rng),
            ddir: 0,
            dpath: name(rng),
            mode: zoraya_fs::RenameMode::Replace,
        },
        12 => Op::Unlink {
            dir: 0,
            path: name(rng),
        },
        13 => Op::Mkdir {
            dir: 0,
            path: "d".into(),
            mode: 0o755,
        },
        14 => Op::Sync,
        15 => Op::Close {
            h: rng.below(handles as u64) as usize,
        },
        _ => random_op(rng, handles, g),
    }
}

/// INV-0014-1 and INV-0014-3 across crash-remounts: random requests with a
/// scripted clock (stuck, coarse, stepping back, and behind the newest
/// stored time after each remount), interleaved with crashes at random
/// points of the device log. Times assigned in the surviving history are
/// strictly increasing; no id is ever handed out twice, including ids of
/// objects lost in a crash.
#[test]
fn times_and_ids_across_crash_remounts() {
    for (seed, mode) in [
        (1, ClockMode::Stuck),
        (2, ClockMode::Coarse),
        (3, ClockMode::Stepping),
        (4, ClockMode::Advancing),
    ] {
        let mut rng = Rng::new(seed);
        let clock = TestClock::new(T0);
        let mut dev: MemDevice = fresh(1024);
        let mut model = Model::new(ROOT_MODE, T0);
        // The surviving history of assigned times, and every id ever created.
        let mut history: Vec<Timestamp> = Vec::new();
        let mut ids: BTreeSet<u64> = BTreeSet::new();
        for round in 0..12 {
            let vol = Volume::mount(
                RecordingDevice::new(dev),
                clock.clone(),
                MountOptions::default(),
            )
            .unwrap();
            let next = vol.next_id();
            let last = vol.durable_root().last_assigned;
            // ADR-0014 rule 3: the restored value is at least every time
            // stored in the selected tree.
            assert!(
                history.last().is_none_or(|t| *t <= last),
                "round {round}: restored time went back"
            );
            model.after_remount(next);
            model.last = last;
            let mut s = Session::new(vol, model.clone(), clock.clone());
            s.clock_mode = mode;
            // The wall clock behind the newest stored time.
            clock.set_wall(Timestamp {
                secs: last.secs - 1000,
                nanos: 0,
            });
            let mut states = vec![hash_state(&s.model.state())];
            // (request index, id) of every object created in this round.
            let mut round_ids: Vec<(u64, u64)> = Vec::new();
            for _ in 0..100 {
                let op = random_op(
                    &mut rng,
                    s.handles(),
                    &Gen {
                        invalid: 0,
                        sync: 5,
                        ..Gen::default()
                    },
                );
                let before = s.created.len();
                let index = s.completed;
                let _ = s.step(&op, &mut rng);
                for id in &s.created[before..] {
                    assert!(
                        !ids.contains(id),
                        "round {round}: id {id} handed out before, in this timeline"
                    );
                    round_ids.push((index, *id));
                }
                let n = s.completed;
                if matches!(op, Op::Sync) {
                    s.vol.device_mut().sync_mark(n);
                } else {
                    s.vol.device_mut().mark(n);
                }
                states.push(hash_state(&s.model.state()));
            }
            let assigned = s.assigned_at.clone();
            let (base, events, _) = s.vol.into_device().into_parts();
            // Crash at a random window, as a random allowed image.
            let ws = windows(&events);
            // Mostly late in the log, so that a useful history survives.
            let pick = ws.len() / 2 + rng.below(ws.len() as u64 - ws.len() as u64 / 2) as usize;
            let mut chosen = None;
            let cfg = CrashConfig {
                k: 6,
                random_subsets: 4,
                torn: 2,
                seed,
            };
            let mut k = 0;
            let target = rng.below(4);
            for_each_crash_image(&base, &events, cfg, |w, _, d| {
                if w.index == pick && (chosen.is_none() || k == target) {
                    chosen = Some(d.to_image());
                }
                if w.index == pick {
                    k += 1;
                }
            });
            dev = MemDevice::from_image(chosen.unwrap());
            // Ids handed out before the crash (requests that completed by the
            // end of the chosen window) belong to this timeline, whether or
            // not their objects survived.
            let crash_completed = ws[pick].completed;
            ids.extend(
                round_ids
                    .iter()
                    .filter(|(i, _)| *i < crash_completed)
                    .map(|(_, id)| *id),
            );
            let mut v = Volume::mount(
                dev.clone(),
                clock.clone(),
                MountOptions {
                    read_only: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let st = engine_state(&mut v).unwrap();
            // The earliest prefix with this state: later requests with the
            // same visible state may have changed only nameless objects.
            let p = states
                .iter()
                .position(|h| *h == hash_state(&st))
                .expect("crash state is a prefix");
            // The surviving history: times assigned by the first p requests.
            history.extend(
                assigned
                    .iter()
                    .filter(|(i, _)| (*i as usize) < p)
                    .map(|(_, t)| *t),
            );
            for w in history.windows(2) {
                assert!(w[0] < w[1], "seed {seed} round {round}: {w:?}");
            }
            let last = v.durable_root().last_assigned;
            model = Model::from_state(&st, v.next_id(), last);
        }
        eprintln!(
            "seed {seed} {mode:?}: {} surviving assigned times, {} ids",
            history.len(),
            ids.len()
        );
        assert!(history.len() > 40, "seed {seed}: {}", history.len());
    }
}

/// The id reservation of proposed ADR-0016: after a crash that loses the
/// creation of an object, the next object gets a new id.
#[test]
fn lost_ids_are_not_reused() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(256), &clock);
    let root = v.root();
    let mk = |v: &mut Volume<MemDevice, TestClock>, n: &str| {
        let h = v
            .open(
                root,
                n,
                OpenOptions {
                    create: Some(0o644),
                    ..Default::default()
                },
            )
            .unwrap();
        v.stat(h).unwrap().id
    };
    let a = mk(&mut v, "a");
    v.sync().unwrap();
    let b = mk(&mut v, "b"); // not durable
    let dev = v.into_device(); // crash
    let mut v = mount(dev, &clock);
    let root2 = v.root();
    assert_eq!(v.stat_at(root2, "b"), Err(FsError::NotFound));
    let c = {
        let h = v
            .open(
                root2,
                "c",
                OpenOptions {
                    create: Some(0o644),
                    ..Default::default()
                },
            )
            .unwrap();
        v.stat(h).unwrap().id
    };
    assert!(a < b && b < c, "{a} {b} {c}");
}

//! Error propagation (INV-0010-3, INV-0012-5, INV-0015-3, INV-0015-4).
//!
//! A fault-injecting device fails the n-th write, flush or read, for every
//! n over a fixed request sequence, or corrupts what is read from metadata
//! blocks. After every request: an injected fault is never silently
//! dropped (the request that met it fails); once the volume is `Failed`,
//! every mutating request and every `sync` fails with `StorageFailed`, no
//! device write is issued, and the failure is reported exactly once.

mod common;

use std::collections::BTreeSet;

use common::*;
use zoraya_fs::check::{BlockKind, reachable_from};
use zoraya_fs::{BlockDevice, FailReason, FsError, MountOptions, Volume, select_root};
use zoraya_fs_testkit::model::Model;
use zoraya_fs_testkit::ops::{Gen, Op, Session, random_op};
use zoraya_fs_testkit::{FaultDevice, FaultPlan, MemDevice, Rng, TestClock};

const SEQ_LEN: usize = 120;

fn sequence(seed: u64) -> Vec<Op> {
    // Generated against a live session so that handles exist; the same seed
    // gives the same sequence each run.
    let mut rng = Rng::new(seed);
    let g = Gen {
        invalid: 0,
        sync: 8,
        ..Gen::default()
    };
    let mut s = session(fresh(1024));
    let mut ops = Vec::new();
    for _ in 0..SEQ_LEN {
        let op = random_op(&mut rng, s.handles(), &g);
        let _ = s.step(&op, &mut rng);
        ops.push(op);
    }
    ops
}

#[derive(Default, Debug)]
struct Seen {
    runs: u64,
    failed_runs: u64,
    reasons: BTreeSet<String>,
}

/// Runs `ops` on a device with `plan`; checks the invariants after every
/// request. Returns the device counters of a fault-free run when `plan` is
/// empty.
fn run(ops: &[Op], plan: FaultPlan, corrupt_at: Option<usize>, seen: &mut Seen) -> (u64, u64, u64) {
    let clock = TestClock::new(T0);
    let dev = FaultDevice::new(fresh(1024), plan);
    let vol = match Volume::mount(dev, clock.clone(), MountOptions::default()) {
        Ok(v) => v,
        Err(e) => {
            // A fault during mount fails the mount: reported, not dropped.
            assert!(
                matches!(e, FsError::Corrupt | FsError::StorageFailed),
                "mount: {e:?}"
            );
            return (0, 0, 0);
        }
    };
    let mut s = Session::new(vol, Model::new(ROOT_MODE, T0), clock);
    let mut rng = Rng::new(77);
    let mut failed_writes: Option<u64> = None;
    let mut reported = 0;
    seen.runs += 1;
    if let Some(reason) = s.vol.failure() {
        // A metadata read failed during mount: the volume mounted `Failed`.
        seen.failed_runs += 1;
        seen.reasons.insert(format!("{reason:?} at mount"));
        failed_writes = Some(s.vol.device().writes);
    }
    for (i, op) in ops.iter().enumerate() {
        if corrupt_at == Some(i) {
            // Corrupt every tree node of the durable generation, as read
            // from now on.
            let rec = s.vol.durable_root();
            let mut inner = s.vol.device().inner.clone();
            let blocks = reachable_from(&mut inner, &rec).expect("reachable");
            s.vol.device_mut().plan.corrupt_reads_of = blocks
                .iter()
                .filter(|(_, (k, _))| matches!(k, BlockKind::Node(_)))
                .map(|(b, _)| *b)
                .collect();
            // Make the engine read them again (as memory pressure would).
            s.vol.drop_caches();
        }
        if i % 7 == 6 {
            // Memory pressure: clean nodes are read again, so read faults
            // also hit metadata during requests.
            s.vol.drop_caches();
        }
        let injected = s.vol.device().injected;
        let was_failed = s.vol.failure().is_some();
        let r = s.step_engine(op, &mut rng);
        if s.vol.device().injected > injected {
            assert!(
                r.is_err(),
                "request {i} {op:?} met an injected fault and returned {r:?}"
            );
        }
        if let Some(reason) = s.vol.failure() {
            if !was_failed {
                seen.failed_runs += 1;
                seen.reasons.insert(format!("{reason:?}"));
                failed_writes = Some(s.vol.device().writes);
                assert!(
                    r.is_err(),
                    "request {i} {op:?} moved the volume to Failed but returned {r:?}"
                );
            } else if op.mutates()
                && !matches!(op, Op::Tick { .. })
                && !matches!(op, Op::Close { h } if h % s.handles() == 0)
            {
                assert_eq!(
                    r,
                    Err(FsError::StorageFailed),
                    "request {i} {op:?} after failure"
                );
            }
            assert_eq!(s.vol.sync(), Err(FsError::StorageFailed));
            assert_eq!(
                Some(s.vol.device().writes),
                failed_writes,
                "a write after the volume failed"
            );
            if s.vol.take_failure_report().is_some() {
                reported += 1;
            }
        }
        // A `Corrupt` with the volume still healthy is a data block's
        // failure: that request only (ADR-0012 rule 3), including a link
        // object's target read while resolving a path.
    }
    if s.vol.failure().is_some() {
        assert_eq!(reported, 1, "failure reported {reported} times");
        // Handles opened after the failure fail too.
        let root = s.vol.root();
        assert_eq!(
            s.vol.mkdir(root, "late", 0o755),
            Err(FsError::StorageFailed)
        );
        assert_eq!(s.vol.sync(), Err(FsError::StorageFailed));
    }
    let d = s.vol.device();
    (d.writes, d.flushes, d.reads)
}

#[test]
fn every_injected_fault_is_reported_and_sticky() {
    let mut seen = Seen::default();
    let mut points = 0;
    for seed in [1, 2, 3, 4, 5, 6] {
        let ops = sequence(seed);
        let (w, f, r) = run(&ops, FaultPlan::default(), None, &mut seen);
        assert!(
            w > 20 && f > 5 && r > 5,
            "too few device operations: {w} {f} {r}"
        );
        for n in 0..w {
            run(
                &ops,
                FaultPlan {
                    fail_write: Some(n),
                    ..Default::default()
                },
                None,
                &mut seen,
            );
        }
        for n in 0..f {
            run(
                &ops,
                FaultPlan {
                    fail_flush: Some(n),
                    ..Default::default()
                },
                None,
                &mut seen,
            );
        }
        for n in 0..r {
            run(
                &ops,
                FaultPlan {
                    fail_read: Some(n),
                    ..Default::default()
                },
                None,
                &mut seen,
            );
        }
        points += w + f + r;
    }
    eprintln!(
        "fault injection: {points} fault points, {} runs, {} reached Failed, reasons {:?}",
        seen.runs, seen.failed_runs, seen.reasons
    );
    for r in ["DeviceWrite", "DeviceFlush", "MetadataRead"] {
        assert!(seen.reasons.contains(r), "no run failed with {r}");
    }
}

/// INV-0012-5: metadata that fails validation moves the volume to `Failed`.
#[test]
fn corrupt_metadata_is_fail_stop() {
    let mut seen = Seen::default();
    for seed in [4, 5] {
        let ops = sequence(seed);
        for at in (0..SEQ_LEN).step_by(3) {
            run(&ops, FaultPlan::default(), Some(at), &mut seen);
        }
    }
    eprintln!(
        "metadata corruption: {} runs, {} reached Failed",
        seen.runs, seen.failed_runs
    );
    assert!(seen.reasons.contains("MetadataCorrupt"));
    assert!(seen.failed_runs * 4 > seen.runs);
}

/// INV-0015-3: no read-write mount without flush, unless told it is unsafe.
#[test]
fn no_flush_means_read_only() {
    let clock = TestClock::new(T0);
    let dev = fresh(256).without_flush();
    assert!(!dev.flush_supported());
    assert_eq!(
        Volume::mount(dev.clone(), clock.clone(), MountOptions::default()).err(),
        Some(FsError::ReadOnly)
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
    let root = v.root();
    assert_eq!(v.mkdir(root, "x", 0o755), Err(FsError::ReadOnly));
    assert!(v.readdir(root, None).unwrap().is_none());
    let plan = FaultPlan {
        no_flush: true,
        ..Default::default()
    };
    let fd = FaultDevice::new(fresh(256), plan);
    assert!(Volume::mount(fd.clone(), clock.clone(), MountOptions::default()).is_err());
    // The explicit unsafe flag, for tests that never crash.
    let v = Volume::mount(
        fd,
        clock,
        MountOptions {
            unsafe_no_flush: true,
            ..Default::default()
        },
    );
    assert!(v.is_ok());
}

/// INV-0015-4: every device error reaches the engine as a typed result, and
/// write and flush errors move the volume to `Failed` with the matching
/// reason, also when the commit is started by a time tick.
#[test]
fn device_errors_are_typed() {
    for (fail_write, want) in [
        (true, FailReason::DeviceWrite),
        (false, FailReason::DeviceFlush),
    ] {
        let clock = TestClock::new(T0);
        let mut v = Volume::mount(
            FaultDevice::new(fresh(256), FaultPlan::default()),
            clock.clone(),
            MountOptions::default(),
        )
        .unwrap();
        let root = v.root();
        // Reserve ids first, so that the next commit is the timed one.
        v.mkdir(root, "warm", 0o755).unwrap();
        v.sync().unwrap();
        let (w, f) = (v.device().writes, v.device().flushes);
        if fail_write {
            v.device_mut().plan.fail_write = Some(w);
        } else {
            v.device_mut().plan.fail_flush = Some(f);
        }
        v.mkdir(root, "d", 0o755).unwrap();
        clock.advance_mono(zoraya_fs::T_COMMIT_NS);
        assert_eq!(v.tick(), Err(FsError::StorageFailed));
        assert_eq!(v.failure(), Some(want));
        assert_eq!(v.take_failure_report(), Some(want));
        assert_eq!(v.take_failure_report(), None);
        // Reads go on from memory.
        assert_eq!(
            v.stat_at(root, "d").unwrap().kind,
            zoraya_fs::ObjKind::Directory
        );
        // A remount reverts to the durable prefix and clears the state.
        let dev = v.into_device().inner;
        let mut v = Volume::mount(dev, clock, MountOptions::default()).unwrap();
        let root = v.root();
        assert_eq!(v.stat_at(root, "d"), Err(FsError::NotFound));
        v.mkdir(root, "d", 0o755).unwrap();
        v.sync().unwrap();
    }
    // A read error on a data block is that read's `Corrupt`; on metadata it
    // fails the volume.
    let clock = TestClock::new(T0);
    let mut dev = fresh(256);
    {
        let mut v = mount(&mut dev, &clock);
        let root = v.root();
        let f = v
            .open(
                root,
                "f",
                zoraya_fs::OpenOptions {
                    create: Some(0o644),
                    ..Default::default()
                },
            )
            .unwrap();
        v.write(f, 0, &[7u8; 100]).unwrap();
        v.close(f).unwrap();
        v.unmount().unwrap();
    }
    let rec = select_root(&mut dev).unwrap().record;
    let kinds = reachable_from(&mut dev, &rec).unwrap();
    let data = kinds
        .iter()
        .find(|(_, (k, _))| *k == BlockKind::Data)
        .map(|(b, _)| *b)
        .unwrap();
    let plan = FaultPlan {
        corrupt_reads_of: [data].into(),
        ..Default::default()
    };
    let mut v = Volume::mount(
        FaultDevice::new(dev.clone(), plan),
        clock.clone(),
        MountOptions::default(),
    )
    .unwrap();
    let root = v.root();
    let f = v
        .open(root, "f", zoraya_fs::OpenOptions::default())
        .unwrap();
    let mut b = [0u8; 100];
    assert_eq!(v.read(f, 0, &mut b), Err(FsError::Corrupt));
    assert_eq!(v.failure(), None);
    assert_eq!(v.corrupt_reads(), 1);
    v.mkdir(root, "still-writable", 0o755).unwrap();
    let _ = MemDevice::new(1);
}

/// INV-0010-3 after a read-only request: a one-time read fault leaves no
/// persistent error behind, so only the volume's state can show it was met
/// (TASK-07 review R-05).
#[test]
fn read_only_request_that_meets_a_read_fault_fails_the_volume() {
    let clock = TestClock::new(T0);
    let mut base = fresh(128);
    {
        let mut v = mount(&mut base, &clock);
        let r = v.root();
        v.mkdir(r, "d", 0o755).unwrap();
        v.close(r).unwrap();
        v.unmount().unwrap();
    }
    let image = base.into_image();
    let mut met = 0;
    for n in 0..40 {
        let plan = FaultPlan {
            fail_read: Some(n),
            ..FaultPlan::default()
        };
        let dev = FaultDevice::new(MemDevice::from_image(image.clone()), plan);
        let Ok(mut v) = Volume::mount(dev, clock.clone(), MountOptions::default()) else {
            continue;
        };
        if v.device().injected > 0 {
            continue;
        }
        v.drop_caches();
        let r = v.root();
        let got = v.stat_at(r, "d");
        if v.device().injected == 0 {
            continue;
        }
        met += 1;
        assert!(got.is_err());
        assert!(
            v.failure().is_some(),
            "read fault {n} left the volume healthy"
        );
        assert_eq!(v.sync(), Err(FsError::StorageFailed));
    }
    assert!(met > 0);
}

/// INV-0016-4: the checker runs on what fault runs leave behind too. A device
/// whose writes failed still holds a volume that mounts clean.
#[test]
fn volumes_left_by_faults_check_clean() {
    use zoraya_fs::check::{CheckOptions, check};
    let clock = TestClock::new(T0);
    let mut d = fresh(128);
    for fail_write in 0..12 {
        let plan = FaultPlan {
            fail_write: Some(fail_write),
            ..FaultPlan::default()
        };
        let dev = FaultDevice::new(MemDevice::from_image(d.image().to_vec()), plan);
        let mut v = Volume::mount(dev, clock.clone(), MountOptions::default()).unwrap();
        let r = v.root();
        let _ = v.mkdir(r, "d", 0o755);
        let _ = v.sync();
        let mut inner = v.into_device().inner;
        let rep = check(&mut inner, CheckOptions::default());
        assert!(rep.is_clean(), "fault {fail_write}: {:?}", rep.problems);
    }
    let _ = &mut d;
}

//! Integrity and root selection (INV-0012-1, INV-0012-2, INV-0011-5,
//! INV-0011-6).

mod common;

use std::collections::BTreeMap;
use std::rc::Rc;

use common::*;
use zoraya_fs::check::reachable_from;
use zoraya_fs::format::{N_PAIRS, RootRecord};
use zoraya_fs::{
    BLOCK_SIZE, BlockDevice, FailReason, FsError, MountOptions, OpenOptions, SECTOR_SIZE, Volume,
    select_root,
};
use zoraya_fs_testkit::fields::field_map;
use zoraya_fs_testkit::ops::{Gen, Op, random_op};
use zoraya_fs_testkit::state::{FsState, engine_state};
use zoraya_fs_testkit::{ImageDevice, MemDevice, RecordingDevice, Rng, TestClock};

/// Files with known content: path -> bytes (a link's target for "ln").
type Expected = BTreeMap<String, Vec<u8>>;

fn put(v: &mut Volume<&mut MemDevice, TestClock>, exp: &mut Expected, path: &str, data: Vec<u8>) {
    let root = v.root();
    let f = v
        .open(
            root,
            path,
            OpenOptions {
                create: Some(0o644),
                truncate: true,
                ..Default::default()
            },
        )
        .unwrap();
    v.write(f, 0, &data).unwrap();
    v.close(f).unwrap();
    v.close(root).unwrap();
    exp.insert(path.to_string(), data);
}

/// An image with every structure kind: a two-level tree, directories, files
/// of 0 to 20,000 bytes, a link object.
fn known_image(dev: &mut MemDevice) -> Expected {
    let clock = TestClock::new(T0);
    let mut v = mount(dev, &clock);
    let mut exp = Expected::new();
    let root = v.root();
    v.mkdir(root, "dir", 0o755).unwrap();
    v.mkdir(root, "dir/sub", 0o700).unwrap();
    v.symlink(root, "ln", "dir/sub/../f").unwrap();
    for (i, size) in [0usize, 1, 100, 4095, 4096, 5000, 20_000]
        .iter()
        .enumerate()
    {
        let data: Vec<u8> = (0..*size).map(|j| (j * 31 + i * 7) as u8).collect();
        put(&mut v, &mut exp, &format!("dir/sub/f{i}"), data);
    }
    for i in 0..110 {
        put(
            &mut v,
            &mut exp,
            &format!("dir/n{i:03}"),
            format!("small file {i}").into_bytes(),
        );
    }
    v.close(root).unwrap();
    v.unmount().unwrap();
    exp
}

/// Mounts read-write (which also reads and validates the allocator) and
/// reads everything: each read returns the expected bytes or `Corrupt`,
/// never other bytes. Returns (reads that succeeded, reads or mounts that
/// reported corruption).
fn verify<D: BlockDevice>(dev: D, exp: &Expected) -> (u64, u64) {
    let (mut good, mut corrupt) = (0, 0);
    let clock = TestClock::new(T0);
    let mut v = match Volume::mount(dev, clock, MountOptions::default()) {
        Ok(v) => v,
        Err(FsError::Corrupt) => return (0, exp.len() as u64),
        Err(e) => panic!("mount: {e:?}"),
    };
    if v.take_failure_report().is_some() {
        // Metadata (allocator, root ring) failed validation at mount.
        corrupt += 1;
    }
    let root = v.root();
    for (path, want) in exp {
        let r = v.open(root, path, OpenOptions::default()).and_then(|f| {
            let mut buf = vec![0u8; want.len() + 10];
            let n = v.read(f, 0, &mut buf)?;
            buf.truncate(n);
            Ok(buf)
        });
        match r {
            Ok(b) => {
                assert_eq!(&b, want, "{path}: wrong bytes returned");
                good += 1;
            }
            Err(FsError::Corrupt) => corrupt += 1,
            Err(e) => panic!("{path}: {e:?} (expected the content or Corrupt)"),
        }
    }
    match v
        .open(
            root,
            "ln",
            OpenOptions {
                link_itself: true,
                ..Default::default()
            },
        )
        .and_then(|h| v.readlink(h))
    {
        Ok(t) => assert_eq!(t, "dir/sub/../f"),
        Err(FsError::Corrupt) => {}
        Err(e) => panic!("ln: {e:?}"),
    }
    (good, corrupt)
}

/// INV-0012-1: a flipped bit anywhere gives the original bytes or Corrupt.
#[test]
fn bit_flips_never_return_wrong_data() {
    let mut dev = fresh(512);
    let exp = known_image(&mut dev);
    let base = Rc::new(dev.image().to_vec());
    let rec = select_root(&mut dev).unwrap().record;
    let used = reachable_from(&mut dev, &rec).unwrap();
    let mut rng = Rng::new(5);
    let (mut flips, mut hit) = (0, 0);
    for b in 0..dev.block_count() {
        for _ in 0..3 {
            let mut d = ImageDevice::new(base.clone());
            let mut blk = d.get_block(b);
            let bit = rng.below((BLOCK_SIZE * 8) as u64) as usize;
            blk[bit / 8] ^= 1 << (bit % 8);
            d.set_block(b, blk);
            let (_, corrupt) = verify(d, &exp);
            flips += 1;
            if corrupt > 0 {
                hit += 1;
            }
        }
    }
    eprintln!(
        "bit flips: {flips} images, {hit} with Corrupt results, {} blocks in use",
        used.len() + 8
    );
    assert!(hit > used.len() as u64);
}

/// INV-0012-2: a lost write (an older version of a block) and a misdirected
/// write (two blocks swapped) are detected.
#[test]
fn lost_and_misdirected_writes_are_detected() {
    let mut dev = fresh(512);
    let exp_a = known_image(&mut dev);
    let old = dev.image().to_vec();
    let clock = TestClock::new(T0);
    let mut exp = exp_a.clone();
    {
        let mut v = mount(&mut dev, &clock);
        for (i, path) in exp_a.keys().enumerate() {
            if i % 3 == 0 {
                put(
                    &mut v,
                    &mut exp,
                    path,
                    format!("rewritten {i} {}", "x".repeat(i * 50)).into_bytes(),
                );
            }
        }
        v.unmount().unwrap();
    }
    let new = Rc::new(dev.image().to_vec());
    let rec = select_root(&mut dev).unwrap().record;
    let used: Vec<u64> = reachable_from(&mut dev, &rec)
        .unwrap()
        .into_keys()
        .collect();
    let mut lost = 0;
    for &b in &used {
        let s = b as usize * BLOCK_SIZE;
        if old[s..s + BLOCK_SIZE] == new[s..s + BLOCK_SIZE] {
            continue;
        }
        let mut d = ImageDevice::new(new.clone());
        d.set_block(b, old[s..s + BLOCK_SIZE].try_into().unwrap());
        let (_, corrupt) = verify(d, &exp);
        assert!(corrupt > 0, "lost write of block {b} went unnoticed");
        lost += 1;
    }
    let mut rng = Rng::new(11);
    let mut swaps = 0;
    for _ in 0..300 {
        let (a, b) = (*rng.pick(&used), *rng.pick(&used));
        if a == b {
            continue;
        }
        let mut d = ImageDevice::new(new.clone());
        let (ba, bb) = (d.get_block(a), d.get_block(b));
        if ba == bb {
            continue;
        }
        d.set_block(a, bb);
        d.set_block(b, ba);
        let (_, corrupt) = verify(d, &exp);
        assert!(corrupt > 0, "swap of blocks {a} and {b} went unnoticed");
        swaps += 1;
    }
    eprintln!("lost writes: {lost} blocks; misdirected: {swaps} swaps; all detected");
    assert!(lost > 20 && swaps > 200);
}

fn state_of<D: BlockDevice>(dev: D) -> (FsState, u64, Option<FailReason>, bool) {
    let mut v = Volume::mount(
        dev,
        TestClock::new(T0),
        MountOptions {
            read_only: true,
            ..Default::default()
        },
    )
    .expect("mount");
    // A volume found corrupt mounts read-only and says so.
    let f = v.take_failure_report();
    let st = engine_state(&mut v).unwrap();
    (st, v.durable_generation(), f, v.is_read_only())
}

/// INV-0011-5: torn root records, single-copy corruption, and an older pair
/// invalid while a newer one is valid.
#[test]
fn root_record_selection() {
    let clock = TestClock::new(T0);
    let mut dev = fresh(256);
    {
        let mut v = mount(&mut dev, &clock);
        let root = v.root();
        for i in 0..6 {
            v.mkdir(root, &format!("d{i}"), 0o755).unwrap();
            v.sync().unwrap();
        }
        v.close(root).unwrap();
    }
    let durable = Rc::new(dev.image().to_vec());
    let (state_g, g, _, _) = state_of(ImageDevice::new(durable.clone()));
    // The next commit, recorded, to get its root record and blocks.
    let mut rec = RecordingDevice::new(MemDevice::from_image((*durable).clone()));
    {
        let mut v = mount(&mut rec, &clock);
        let root = v.root();
        // Not a create: a create after mount first reserves ids durably
        // (one more commit, ADR-0016).
        v.set_attr(root, Some(0o700), None).unwrap();
        v.sync().unwrap();
    }
    let committed = Rc::new(rec.current().image().to_vec());
    let (state_next, g_next, _, _) = state_of(ImageDevice::new(committed.clone()));
    assert_eq!(g_next, g + 1);
    let pair = g_next % N_PAIRS;
    let [s0, s1] = RootRecord::slots(pair);
    let get = |img: &[u8], b: u64| -> [u8; BLOCK_SIZE] {
        img[b as usize * BLOCK_SIZE..(b as usize + 1) * BLOCK_SIZE]
            .try_into()
            .unwrap()
    };
    let (old0, old1, new) = (get(&durable, s0), get(&durable, s1), get(&committed, s0));
    let sectors = BLOCK_SIZE / SECTOR_SIZE;
    let torn = |old: &[u8; BLOCK_SIZE], k: usize| {
        let mut b = *old;
        b[..k * SECTOR_SIZE].copy_from_slice(&new[..k * SECTOR_SIZE]);
        b
    };
    let mut cases = 0;
    for k0 in 0..=sectors {
        for k1 in 0..=sectors {
            let mut d = ImageDevice::new(committed.clone());
            d.set_block(s0, torn(&old0, k0));
            d.set_block(s1, torn(&old1, k1));
            let (st, sel_gen, fail, _) = state_of(d);
            if k0 == sectors || k1 == sectors {
                assert_eq!((sel_gen, &st), (g_next, &state_next), "tear {k0}/{k1}");
            } else {
                assert_eq!((sel_gen, &st), (g, &state_g), "tear {k0}/{k1}");
            }
            assert_eq!(fail, None, "tear {k0}/{k1} reported as corruption");
            cases += 1;
        }
    }
    // Each field of each copy of the newest durable generation, corrupted in
    // turn: the other copy is used and nothing is reported.
    let mut probe = ImageDevice::new(durable.clone());
    let map = field_map(&mut probe).unwrap();
    let root_fields: Vec<_> = map
        .fields
        .iter()
        .filter(|f| f.block == map.slots[0])
        .collect();
    for f in &root_fields {
        for copy in map.slots {
            let mut d = ImageDevice::new(durable.clone());
            let mut b = d.get_block(copy);
            b[f.off] ^= 0x01;
            d.set_block(copy, b);
            let (st, sel_gen, fail, ro) = state_of(d);
            assert_eq!(
                (sel_gen, &st, fail, ro),
                (g, &state_g, None, true),
                "field {} of copy {copy}",
                f.what
            );
            cases += 1;
        }
    }
    // An older pair invalid while a newer one is valid: corrupt, read-only.
    let older = RootRecord::slots((g - 1) % N_PAIRS);
    let mut d = ImageDevice::new(durable.clone());
    for s in older {
        let mut b = d.get_block(s);
        b[100] ^= 0xFF;
        d.set_block(s, b);
    }
    let mut v = Volume::mount(d, clock.clone(), MountOptions::default()).unwrap();
    assert!(v.is_read_only());
    assert_eq!(v.take_failure_report(), Some(FailReason::MetadataCorrupt));
    assert_eq!(engine_state(&mut v).unwrap(), state_g);
    let root = v.root();
    assert_eq!(v.mkdir(root, "x", 0o755), Err(FsError::StorageFailed));
    eprintln!("root selection: {cases} cases");
}

/// INV-0011-6: read-only requests, syncs and ticks across several
/// `T_COMMIT` periods write nothing.
#[test]
fn idle_and_read_only_sequences_write_nothing() {
    let mut s = session(fresh(1024));
    let mut rng = Rng::new(3);
    let g = Gen::default();
    for _ in 0..200 {
        let op = random_op(&mut rng, s.handles(), &g);
        let _ = s.step(&op, &mut rng);
    }
    let dev = s.vol.unmount().unwrap();
    let clock = TestClock::new(T0);
    // Objects left on the orphan list by handles open at unmount are freed
    // by the next read-write mount, which writes; do that first.
    let dev = Volume::mount(dev, clock.clone(), MountOptions::default())
        .unwrap()
        .unmount()
        .unwrap();
    let mut v = Volume::mount(
        RecordingDevice::new(dev),
        clock.clone(),
        MountOptions::default(),
    )
    .unwrap();
    let mut sess = zoraya_fs_testkit::ops::Session::new(v, s.model.clone(), clock.clone());
    sess.model.after_remount(sess.vol.next_id());
    let mut n = 0;
    for i in 0..600 {
        let op = random_op(&mut rng, sess.handles(), &g);
        let op = match op {
            Op::Sync | Op::Tick { .. } | Op::Close { .. } => op,
            o if o.mutates() => continue,
            o => o,
        };
        sess.step(&op, &mut rng).ok();
        if i % 50 == 0 {
            clock.advance_mono(zoraya_fs::T_COMMIT_NS * 2);
            sess.vol.tick().unwrap();
            sess.vol.sync().unwrap();
        }
        n += 1;
    }
    v = sess.vol;
    assert_eq!(v.device().write_count(), 0, "a read-only sequence wrote");
    assert_eq!(v.commits(), 0);
    eprintln!("idle: {n} read-only requests, 0 writes");
}

/// Proposed ADR-0016 item 2: when the newest generation's root record is
/// valid but its tree root block is not, mount fails with `Corrupt` rather
/// than fall back to an older generation (ADR-0011 rule 4: never select a
/// generation lower than the highest valid one).
#[test]
fn invalid_tree_root_of_the_newest_generation_fails_the_mount() {
    let clock = TestClock::new(T0);
    let mut dev = fresh(256);
    {
        let mut v = mount(&mut dev, &clock);
        let root = v.root();
        for i in 0..3 {
            v.mkdir(root, &format!("d{i}"), 0o755).unwrap();
            v.sync().unwrap();
        }
    }
    let rec = select_root(&mut dev).unwrap().record;
    let s = rec.tree_root.addr as usize * BLOCK_SIZE;
    dev.block_mut(rec.tree_root.addr)[100] ^= 1;
    let _ = s;
    let r = Volume::mount(
        dev.clone(),
        clock.clone(),
        MountOptions {
            read_only: true,
            ..Default::default()
        },
    );
    assert_eq!(r.err(), Some(FsError::Corrupt));
    assert_eq!(
        Volume::mount(dev, clock, MountOptions::default()).err(),
        Some(FsError::Corrupt)
    );
}

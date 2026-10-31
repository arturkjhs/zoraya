//! Inputs that once crashed or hung the engine, kept fixed (see
//! `fuzz/README.md`).

mod common;

use std::rc::Rc;

use common::*;
use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{
    BLOCK_SIZE, BlockDevice, FsError, MountOptions, OpenOptions, RenameMode, Volume, select_root,
};
use zoraya_fs_testkit::crash::windows;
use zoraya_fs_testkit::fields::{field_map, fix_checksums, reseal_root};
use zoraya_fs_testkit::{Event, ImageDevice, MemDevice, RecordingDevice, TestClock};

/// Found by `decoder_fuzz.rs` (field-level, "root generation", all ones):
/// root selection computed `generation + N_PAIRS - k` and overflowed. A
/// generation at the top of the range must neither panic at mount nor at
/// the next commit.
#[test]
fn generation_at_the_top_of_the_range() {
    let clock = TestClock::new(T0);
    let mut dev = fresh(128);
    {
        let mut v = mount(&mut dev, &clock);
        let root = v.root();
        v.mkdir(root, "d", 0o755).unwrap();
        v.unmount().unwrap();
    }
    let base = Rc::new(dev.into_image());
    let mut d = ImageDevice::new(base.clone());
    let map = field_map(&mut d).unwrap();
    // u64::MAX belongs to pair 3; put the record there.
    let mut rec = d.get_block(map.slots[0]);
    rec[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
    for s in [3u64, 7] {
        d.set_block(s, rec);
    }
    let mut m = map;
    m.slots = [3, 7];
    reseal_root(&mut d, &m);
    let mut v = Volume::mount(d, clock, MountOptions::default()).unwrap();
    let root = v.root();
    assert_eq!(
        v.stat_at(root, "d").unwrap().kind,
        zoraya_fs::ObjKind::Directory
    );
    // The next commit cannot have a generation: the volume fails cleanly.
    assert_eq!(v.mkdir(root, "e", 0o755), Err(FsError::StorageFailed));
    assert!(v.failure().is_some());
}

/// Found in review: the ancestor walk of a directory rename was bounded by
/// the on-disk id limit, which a crafted image can set to `u64::MAX`; with a
/// parent cycle it would not end. It is now bounded by the volume size.
#[test]
fn parent_cycle_with_huge_id_limit_terminates() {
    let clock = TestClock::new(T0);
    let mut dev = fresh(128);
    {
        let mut v = mount(&mut dev, &clock);
        let root = v.root();
        v.mkdir(root, "a", 0o755).unwrap();
        v.mkdir(root, "a/b", 0o755).unwrap();
        v.mkdir(root, "x", 0o755).unwrap();
        v.unmount().unwrap();
    }
    let base = Rc::new(dev.into_image());
    let mut d = ImageDevice::new(base.clone());
    let map = field_map(&mut d).unwrap();
    let (a, b) = {
        let mut v = Volume::mount(
            ImageDevice::new(base.clone()),
            clock.clone(),
            MountOptions {
                read_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        let root = v.root();
        (
            v.stat_at(root, "a").unwrap().id,
            v.stat_at(root, "a/b").unwrap().id,
        )
    };
    // PARENT(a) := b, making a and b each other's parent.
    let leaf = map
        .fields
        .iter()
        .find(|f| f.what == "parent id")
        .unwrap()
        .block;
    let old = d.get_block(leaf);
    let mut blk = old;
    // The whole key (id, K_PARENT, offset 0, no name) and the value length 8:
    // the id followed by 2 alone also matches a directory's attribute record.
    let mut key = Vec::new();
    key.extend_from_slice(&a.to_le_bytes());
    key.push(2);
    key.extend_from_slice(&0u64.to_le_bytes());
    key.push(0);
    key.extend_from_slice(&8u16.to_le_bytes());
    let at = blk.windows(key.len()).position(|w| w == key).unwrap();
    let val = at + 18 + 2;
    blk[val..val + 8].copy_from_slice(&b.to_le_bytes());
    d.set_block(leaf, blk);
    fix_checksums(&mut d, &map, leaf, &old);
    // And the id limit at its maximum.
    let mut rec = d.get_block(map.slots[0]);
    rec[112..120].copy_from_slice(&u64::MAX.to_le_bytes());
    d.set_block(map.slots[0], rec);
    reseal_root(&mut d, &map);
    let mut v = Volume::mount(d, clock, MountOptions::default()).unwrap();
    let root = v.root();
    let r = v.rename(root, "x", root, "a/b/x", RenameMode::NoReplace);
    assert_eq!(r, Err(FsError::Corrupt));
    assert!(v.failure().is_some());
    let _ = v.open(root, "a", OpenOptions::default());
}

fn create(
    v: &mut Volume<impl BlockDevice, TestClock>,
    root: zoraya_fs::Handle,
    n: &str,
    data: &[u8],
) {
    let f = v
        .open(
            root,
            n,
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        )
        .unwrap();
    v.write(f, 0, data).unwrap();
    v.close(f).unwrap();
}

/// 2-3). A device that shows it after reboot has not made it stable: no flush
/// followed. Mount trusts g+1, its next commit reuses blocks that g+1 freed,
/// which g still needs. A second crash before that commit's first flush may
/// drop the unstable root: g is back, with its blocks overwritten.
#[test]
fn a_double_crash_keeps_synced_data() {
    let clock = TestClock::new(T0);
    let mut d = fresh(256);
    {
        let mut v = mount(&mut d, &clock);
        let root = v.root();
        create(&mut v, root, "keep", &[0xAB; 3000]);
        v.mkdir(root, "dir", 0o755).unwrap();
        v.sync().unwrap(); // generation g: "keep" is durable by a sync that returned
        v.close(root).unwrap();
        v.unmount().unwrap();
    }
    let gsel = select_root(&mut d).unwrap().record;
    // Commit g+1, recorded: it frees blocks of g's tree.
    let rec = RecordingDevice::new(d);
    let mut v = Volume::mount(rec, clock.clone(), MountOptions::default()).unwrap();
    let root = v.root();
    v.set_attr(root, Some(0o700), None).unwrap();
    v.sync().unwrap();
    let (base, events, _) = v.into_device().into_parts();
    let ws = windows(&events);
    // The window of the root write: the crash image with both copies new.
    let w = ws
        .iter()
        .find(|w| {
            w.writes
                .iter()
                .any(|&i| matches!(&events[i], Event::Write{block,..} if *block < 8))
        })
        .unwrap();
    let mut stable = (*base).clone();
    for i in 0..w.start {
        if let Event::Write { block, data, .. } = &events[i] {
            let s = *block as usize * BLOCK_SIZE;
            stable[s..s + BLOCK_SIZE].copy_from_slice(&data[..]);
        }
    }
    let mut roots: Vec<(u64, [u8; BLOCK_SIZE])> = Vec::new();
    let mut seen = stable.clone();
    for &i in &w.writes {
        if let Event::Write { block, data, .. } = &events[i] {
            roots.push((*block, **data));
            let s = *block as usize * BLOCK_SIZE;
            seen[s..s + BLOCK_SIZE].copy_from_slice(&data[..]);
        }
    }
    // First reboot: the device shows g+1 (one allowed crash image).
    let mut rec2 = RecordingDevice::new(MemDevice::from_image(seen));
    {
        let v = Volume::mount(&mut rec2, clock.clone(), MountOptions::default()).unwrap();
        let mut v = v;
        let r = v.root();
        // A full device-flush count check: does mount flush first?
        v.set_attr(r, Some(0o711), None).unwrap();
        v.sync().unwrap();
        let _ = v.close(r);
    }
    let ev2 = rec2.events();
    // TASK-07 review R-01: the flush comes before anything the mount writes.
    assert!(matches!(ev2.first(), Some(Event::Flush)));
    // Second crash before the second boot's first flush: the unflushed g+1
    // root writes fall back to their old content, the new writes landed.
    let mut img = stable.clone();
    for e in ev2 {
        match e {
            Event::Write { block, data, .. } => {
                let s = *block as usize * BLOCK_SIZE;
                img[s..s + BLOCK_SIZE].copy_from_slice(&data[..]);
            }
            Event::Flush => break,
            _ => {}
        }
    }
    let mut dev = MemDevice::from_image(img);
    let sel = select_root(&mut dev).unwrap().record;
    assert_eq!(sel.generation, gsel.generation);
    let rep = check(&mut dev, CheckOptions::default());
    let m = Volume::mount(
        dev.clone(),
        clock.clone(),
        MountOptions {
            read_only: true,
            ..Default::default()
        },
    );
    assert!(
        rep.is_clean() && m.is_ok(),
        "generation {} was durable and its tree is destroyed",
        gsel.generation
    );
}

fn base_for_repair() -> (Rc<Vec<u8>>, u64) {
    let clock = TestClock::new(T0);
    let mut d = fresh(256);
    let dir;
    {
        let mut v = mount(&mut d, &clock);
        let r = v.root();
        v.mkdir(r, "d", 0o755).unwrap();
        dir = v.stat_at(r, "d").unwrap().id;
        create(&mut v, r, "d/precious", b"hello hello");
        v.close(r).unwrap();
        v.unmount().unwrap();
    }
    (Rc::new(d.into_image()), dir)
}

/// TASK-07 review R-11: one wrong kind byte in an entry must not lose the
/// directory: the object's own record says what it is.
#[test]
fn repair_corrects_an_entry_kind_and_keeps_the_subtree() {
    use zoraya_fs::check::{BlockKind, repair};
    let (base, dir) = base_for_repair();
    let mut d = ImageDevice::new(base);
    let map = field_map(&mut d).unwrap();
    let leaf = map
        .kinds
        .iter()
        .find(|(_, (k, _))| *k == BlockKind::Node(0))
        .map(|(b, _)| *b)
        .unwrap();
    let old = d.get_block(leaf);
    let mut b = old;
    let mut key = 1u64.to_le_bytes().to_vec();
    key.push(3);
    key.extend_from_slice(&0u64.to_le_bytes());
    key.extend_from_slice(&[1, b'd']);
    let at = b.windows(key.len()).position(|w| w == key).unwrap();
    let value = at + key.len() + 2;
    assert_eq!(
        u64::from_le_bytes(b[value..value + 8].try_into().unwrap()),
        dir
    );
    b[value + 8] = 1; // File
    d.set_block(leaf, b);
    fix_checksums(&mut d, &map, leaf, &old);
    assert!(!check(&mut d, CheckOptions::default()).is_clean());

    let (_, mut d) = repair(d, TestClock::new(T0)).unwrap();
    let rep = check(&mut d, CheckOptions::default());
    assert!(rep.is_clean(), "{:?}", rep.problems);
    let mut v = Volume::mount(d, TestClock::new(T0), MountOptions::default()).unwrap();
    let r = v.root();
    assert_eq!(v.stat_at(r, "d/precious").unwrap().size, 11);
}

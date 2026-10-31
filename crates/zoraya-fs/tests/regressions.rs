//! Inputs that once crashed or hung the engine, kept fixed (see
//! `fuzz/README.md`).

mod common;

use std::rc::Rc;

use common::*;
use zoraya_fs::{FsError, MountOptions, OpenOptions, RenameMode, Volume};
use zoraya_fs_testkit::fields::{field_map, fix_checksums, reseal_root};
use zoraya_fs_testkit::{ImageDevice, TestClock};

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

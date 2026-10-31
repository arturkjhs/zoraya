//! zoraya-fsck: accepts a good image, reports damage without modifying the
//! image, and repairs what is repairable only when asked.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use zoraya_fs::check::{BlockKind, reachable_from};
use zoraya_fs::{
    BLOCK_SIZE, MkfsOptions, MountOptions, OpenOptions, Timestamp, Volume, mkfs, select_root,
};
use zoraya_fs_testkit::ImageDevice;
use zoraya_fs_testkit::fields::{field_map, fix_checksums};
use zoraya_mkfs::{FileDevice, SystemClock};

fn image(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fsck");
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    let mut dev = FileDevice::create(&p, 1 << 20).unwrap();
    mkfs(
        &mut dev,
        MkfsOptions {
            volume_id: 7,
            now: Timestamp { secs: 1, nanos: 0 },
            root_mode: 0o755,
        },
    )
    .unwrap();
    let mut v = Volume::mount(dev, SystemClock::default(), MountOptions::default()).unwrap();
    let root = v.root();
    v.mkdir(root, "d", 0o755).unwrap();
    for i in 0..5 {
        let f = v
            .open(
                root,
                &format!("d/f{i}"),
                OpenOptions {
                    create: Some(0o644),
                    ..Default::default()
                },
            )
            .unwrap();
        v.write(f, 0, &vec![i as u8; 5000]).unwrap();
        v.close(f).unwrap();
    }
    v.unmount().unwrap();
    p
}

fn fsck(args: &[&str], p: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_zoraya-fsck"))
        .args(args)
        .arg(p)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn edit(p: &Path, f: impl FnOnce(&mut ImageDevice)) {
    let mut d = ImageDevice::new(Rc::new(fs::read(p).unwrap()));
    f(&mut d);
    fs::write(p, d.to_image()).unwrap();
}

#[test]
fn clean_image_passes() {
    let p = image("clean.img");
    let (code, out) = fsck(&[], &p);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("6 files") || out.contains("5 files"), "{out}");
}

#[test]
fn damage_is_reported_and_not_modified() {
    let p = image("data.img");
    let mut dev = FileDevice::open(&p, false).unwrap();
    let rec = select_root(&mut dev).unwrap().record;
    let data = reachable_from(&mut dev, &rec)
        .unwrap()
        .into_iter()
        .find(|(_, (k, _))| *k == BlockKind::Data)
        .unwrap()
        .0;
    edit(&p, |d| {
        let mut b = d.get_block(data);
        b[10] ^= 1;
        d.set_block(data, b);
    });
    let before = fs::read(&p).unwrap();
    let (code, out) = fsck(&[], &p);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("checksum mismatch"), "{out}");
    assert_eq!(
        fs::read(&p).unwrap(),
        before,
        "fsck without --repair modified the image"
    );
    // A damaged data block has no second copy: repair refuses.
    let (code, _) = fsck(&["--repair"], &p);
    assert_eq!(code, 1);
}

#[test]
fn allocator_damage_is_repaired() {
    let p = image("alloc.img");
    let mut dev = FileDevice::open(&p, false).unwrap();
    let rec = select_root(&mut dev).unwrap().record;
    let bitmap = reachable_from(&mut dev, &rec)
        .unwrap()
        .into_iter()
        .find(|(_, (k, _))| *k == BlockKind::Bitmap)
        .unwrap()
        .0;
    edit(&p, |d| {
        let mut b = d.get_block(bitmap);
        b[BLOCK_SIZE - 100] ^= 0xFF;
        d.set_block(bitmap, b);
    });
    assert_eq!(fsck(&[], &p).0, 1);
    let (code, out) = fsck(&["--repair"], &p);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("clean after repair"), "{out}");
    assert_eq!(fsck(&[], &p).0, 0);
}

#[test]
fn namespace_damage_is_repaired() {
    let p = image("ns.img");
    // Make directory "d" claim a wrong size, with every checksum recomputed
    // (only the checker's namespace rules can see it).
    edit(&p, |d| {
        let map = field_map(d).unwrap();
        let leaf = map
            .kinds
            .iter()
            .find(|(_, (k, _))| *k == BlockKind::Node(0))
            .map(|(b, _)| *b)
            .unwrap();
        let old = d.get_block(leaf);
        let mut b = old;
        // The size field of the second attribute record (directory "d",
        // id 2): find "id = 2, kind = 1 (inode)" and its value.
        let key = [2u8, 0, 0, 0, 0, 0, 0, 0, 1];
        let at = b.windows(key.len()).position(|w| w == key).unwrap();
        let attr = at + 18 + 2; // key, then the value length
        b[attr + 9] = b[attr + 9].wrapping_add(3);
        d.set_block(leaf, b);
        fix_checksums(d, &map, leaf, &old);
    });
    let (code, out) = fsck(&[], &p);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("has size"), "{out}");
    let (code, out) = fsck(&["--repair"], &p);
    assert_eq!(code, 0, "{out}");
    let dev = FileDevice::open(&p, false).unwrap();
    let mut v = Volume::mount(
        dev,
        SystemClock::default(),
        MountOptions {
            read_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    let root = v.root();
    assert_eq!(v.stat_at(root, "d").unwrap().size, 5);
}

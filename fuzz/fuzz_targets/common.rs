//! Shared by the fuzz targets: a valid seed image with every structure kind,
//! and the walk every target ends with.

use std::collections::HashSet;
use std::rc::Rc;

use zoraya_fs::{MkfsOptions, MountOptions, OpenOptions, Timestamp, Volume, mkfs};
use zoraya_fs_testkit::walk::{fsck, walk};
use zoraya_fs_testkit::{ImageDevice, MemDevice, TestClock};

pub const BLOCKS: u64 = 256;

/// The seed image: a two-level tree, directories, files, a link object and
/// an object on the orphan list.
pub fn seed_image() -> Vec<u8> {
    let now = Timestamp { secs: 1_700_000_000, nanos: 0 };
    let mut dev = MemDevice::new(BLOCKS);
    mkfs(&mut dev, MkfsOptions { volume_id: 0xF022, now, root_mode: 0o755 }).unwrap();
    let mut v = Volume::mount(&mut dev, TestClock::new(now), MountOptions::default()).unwrap();
    let root = v.root();
    v.mkdir(root, "d", 0o755).unwrap();
    v.mkdir(root, "d/e", 0o700).unwrap();
    v.symlink(root, "d/ln", "../d/e/f0").unwrap();
    for i in 0..80 {
        let f = v.open(root, &format!("d/e/f{i}"), OpenOptions { create: Some(0o644), ..Default::default() }).unwrap();
        v.write(f, 0, &vec![i as u8; [0usize, 10, 300, 4100][i % 4]]).unwrap();
        v.close(f).unwrap();
    }
    let u = v.create_unnamed(root, 0o600).unwrap();
    v.write(u, 0, b"orphan").unwrap();
    v.unmount().unwrap();
    dev.into_image()
}

/// Mount read-only and read-write, walk everything, run the checker.
pub fn exercise(img: Rc<Vec<u8>>, dev: ImageDevice) {
    let mut visited = HashSet::new();
    let mut buf = Vec::new();
    let _ = walk(dev.clone(), false, &mut visited, &mut buf);
    let _ = walk(dev.clone(), true, &mut visited, &mut buf);
    let mut d = dev;
    fsck(&mut d);
    let _ = img;
}

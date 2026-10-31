//! Basic requests end to end, and remount.

mod common;

use common::*;
use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{FsError, ObjKind, OpenOptions, RenameMode};
use zoraya_fs_testkit::TestClock;

fn create() -> OpenOptions {
    OpenOptions {
        create: Some(0o644),
        ..OpenOptions::default()
    }
}

#[test]
fn write_read_remount() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(256), &clock);
    let root = v.root();
    v.mkdir(root, "dir", 0o755).unwrap();
    let f = v.open(root, "dir/file", create()).unwrap();
    let data: Vec<u8> = (0..10_000u32).map(|i| i as u8).collect();
    assert_eq!(v.write(f, 100, &data).unwrap(), data.len());
    let mut buf = vec![0u8; 20_000];
    assert_eq!(v.read(f, 0, &mut buf).unwrap(), 10_100);
    assert!(buf[..100].iter().all(|&b| b == 0));
    assert_eq!(&buf[100..10_100], &data[..]);
    v.symlink(root, "ln", "dir/file").unwrap();
    assert!(matches!(
        v.open(root, "ln", OpenOptions::default()),
        Err(FsError::IsLink { .. })
    ));
    v.close(f).unwrap();
    v.close(root).unwrap();
    let d = v.unmount().unwrap();
    let mut d2 = d.clone();
    let rep = check(&mut d2, CheckOptions::default());
    assert!(rep.is_clean(), "{:?}", rep.problems);
    let mut v = mount(d, &clock);
    let root = v.root();
    let f = v.open(root, "dir/file", OpenOptions::default()).unwrap();
    assert_eq!(v.stat(f).unwrap().size, 10_100);
    let mut buf = vec![0u8; 10_000];
    v.read(f, 100, &mut buf).unwrap();
    assert_eq!(buf, data);
    v.rename(root, "dir/file", root, "moved", RenameMode::NoReplace)
        .unwrap();
    let l = v.readdir(root, None).unwrap().unwrap();
    assert_eq!(l.name, "dir");
    assert_eq!(l.kind, ObjKind::Directory);
}

#[test]
fn many_files_split_tree() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(2048), &clock);
    let root = v.root();
    for i in 0..600 {
        let f = v.open(root, &format!("file-{i:04}"), create()).unwrap();
        v.write(f, 0, format!("content {i}").as_bytes()).unwrap();
        v.close(f).unwrap();
        if i % 97 == 0 {
            v.sync().unwrap();
        }
    }
    for i in (0..600).step_by(3) {
        v.unlink(root, &format!("file-{i:04}")).unwrap();
    }
    let d = v.unmount().unwrap();
    let mut d2 = d.clone();
    let rep = check(&mut d2, CheckOptions::default());
    assert!(rep.is_clean(), "{:?}", rep.problems);
    assert_eq!(rep.stats.files, 400);
    assert!(rep.stats.tree_height >= 2);
    let mut v = mount(d, &clock);
    let root = v.root();
    let mut n = 0;
    let mut after: Option<String> = None;
    while let Some(e) = v.readdir(root, after.as_deref()).unwrap() {
        n += 1;
        after = Some(e.name);
    }
    assert_eq!(n, 400);
    for i in 0..600 {
        let r = v.open(root, &format!("file-{i:04}"), OpenOptions::default());
        if i % 3 == 0 {
            assert_eq!(r.unwrap_err(), FsError::NotFound);
        } else {
            let f = r.unwrap();
            let mut b = vec![0u8; 64];
            let k = v.read(f, 0, &mut b).unwrap();
            assert_eq!(&b[..k], format!("content {i}").as_bytes());
            v.close(f).unwrap();
        }
    }
    // Remove everything: the tree shrinks back.
    for i in 0..600 {
        if i % 3 != 0 {
            v.unlink(root, &format!("file-{i:04}")).unwrap();
        }
    }
    let mut d = v.unmount().unwrap();
    let rep = check(&mut d, CheckOptions::default());
    assert!(rep.is_clean(), "{:?}", rep.problems);
    assert_eq!(rep.stats.files, 0);
}

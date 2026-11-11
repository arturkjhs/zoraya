//! zoraya-mkfs builds an image from a host directory, and the checker
//! accepts it.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};

use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{MountOptions, ObjKind, OpenOptions, Volume};
use zoraya_mkfs::{FileDevice, SystemClock, system_time};

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn image_from_a_host_directory() {
    let dir = tmp("mkfs-src");
    let src = dir.join("tree");
    fs::create_dir_all(src.join("bin")).unwrap();
    fs::create_dir_all(src.join("src/deep/er")).unwrap();
    fs::write(src.join("bin/tool"), b"#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(src.join("bin/tool"), fs::Permissions::from_mode(0o755)).unwrap();
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    fs::write(src.join("src/big.bin"), &big).unwrap();
    fs::write(src.join("src/deep/er/empty"), b"").unwrap();
    fs::write(src.join("src/\u{e9}t\u{e9}.c"), b"int main(){}\n").unwrap();
    symlink("../bin/tool", src.join("src/link")).unwrap();
    let old = SystemTime::UNIX_EPOCH + Duration::new(1_000_000_000, 123_456_789);
    fs::File::options()
        .write(true)
        .open(src.join("src/big.bin"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    let image = dir.join("disk.img");
    let out = Command::new(env!("CARGO_BIN_EXE_zoraya-mkfs"))
        .args(["--size", "16M", "--from"])
        .arg(&src)
        .arg(&image)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut dev = FileDevice::open(&image, false).unwrap();
    let rep = check(&mut dev, CheckOptions::default());
    assert!(rep.is_clean(), "{:?}", rep.problems);
    assert_eq!(
        (rep.stats.files, rep.stats.directories, rep.stats.links),
        (4, 5, 1)
    );
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
    let a = v.stat_at(root, "bin/tool").unwrap();
    assert_eq!(a.posix_mode, 0o755);
    let h = v.open(root, "src/big.bin", OpenOptions::default()).unwrap();
    let st = v.stat(h).unwrap();
    assert_eq!(st.size, big.len() as u64);
    assert_eq!(st.mtime, system_time(old));
    let mut buf = vec![0u8; big.len()];
    assert_eq!(v.read(h, 0, &mut buf).unwrap(), big.len());
    assert!(buf == big);
    let l = v
        .open(
            root,
            "src/link",
            OpenOptions {
                link_itself: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(v.readlink(l).unwrap(), "../bin/tool");
    assert_eq!(
        v.stat_at(root, "src/deep/er").unwrap().kind,
        ObjKind::Directory
    );
    assert_eq!(v.stat_at(root, "src/\u{e9}t\u{e9}.c").unwrap().size, 13);
}

#[test]
fn names_the_volume_refuses_are_errors() {
    let dir = tmp("mkfs-bad");
    let src = dir.join("tree");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("bad\u{7}name"), b"x").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_zoraya-mkfs"))
        .args(["--size", "1M", "--from"])
        .arg(&src)
        .arg(dir.join("disk.img"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("bad"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn empty_image() {
    let dir = tmp("mkfs-empty");
    let image = dir.join("e.img");
    let out = Command::new(env!("CARGO_BIN_EXE_zoraya-mkfs"))
        .args(["--size", "512K"])
        .arg(&image)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(fs::metadata(&image).unwrap().len(), 512 * 1024);
    let mut dev = FileDevice::open(&image, false).unwrap();
    assert!(check(&mut dev, CheckOptions::default()).is_clean());
}

/// An empty `.keep` only keeps an empty directory in git (as rootfs does for home): the
/// directory is made, the file is not. A `.keep` with content is copied.
#[test]
fn an_empty_keep_file_is_not_copied() {
    let dir = tmp("mkfs-keep");
    let src = dir.join("tree");
    fs::create_dir_all(src.join("home")).unwrap();
    fs::write(src.join("home/.keep"), b"").unwrap();
    fs::create_dir_all(src.join("other")).unwrap();
    fs::write(src.join("other/.keep"), b"kept").unwrap();
    let image = dir.join("disk.img");
    let out = Command::new(env!("CARGO_BIN_EXE_zoraya-mkfs"))
        .args(["--size", "16M", "--from"])
        .arg(&src)
        .arg(&image)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut dev = FileDevice::open(&image, false).unwrap();
    let rep = check(&mut dev, CheckOptions::default());
    assert!(rep.is_clean(), "{:?}", rep.problems);
    // The root, home and other; only other/.keep.
    assert_eq!((rep.stats.files, rep.stats.directories), (1, 3));
}

/// `--limit PATH=SIZE` puts a storage limit (ADR-0265) on a directory of
/// the image, counting what the copy put there.
#[test]
fn limits_on_directories_of_the_image() {
    let dir = tmp("mkfs-limit");
    let src = dir.join("tree");
    fs::create_dir_all(src.join("room/inner")).unwrap();
    fs::create_dir_all(src.join("other")).unwrap();
    fs::write(src.join("room/inner/f"), vec![1u8; 5000]).unwrap();
    let image = dir.join("disk.img");
    let out = Command::new(env!("CARGO_BIN_EXE_zoraya-mkfs"))
        .args([
            "--size",
            "16M",
            "--limit",
            "room=64K",
            "--limit",
            "room/inner=1M",
            "--from",
        ])
        .arg(&src)
        .arg(&image)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dev = FileDevice::open(&image, false).unwrap();
    let opts = MountOptions {
        read_only: true,
        ..MountOptions::default()
    };
    let mut v = Volume::mount(dev, SystemClock::default(), opts).unwrap();
    let root = v.root();
    let room = v.open(root, "room", OpenOptions::default()).unwrap();
    // inner (1 block) + f (1 + 2 blocks).
    let l = v.limit_of(room).unwrap().unwrap();
    assert_eq!((l.limit, l.used), (65536, 4 * 4096));
    let inner = v.open(root, "room/inner", OpenOptions::default()).unwrap();
    assert_eq!(v.limit_of(inner).unwrap().unwrap().used, 3 * 4096);
    let other = v.open(root, "other", OpenOptions::default()).unwrap();
    assert_eq!(v.limit_of(other).unwrap(), None);
    let bad = Command::new(env!("CARGO_BIN_EXE_zoraya-mkfs"))
        .args(["--size", "16M", "--limit", "nowhere=1K"])
        .arg(dir.join("bad.img"))
        .output()
        .unwrap();
    assert!(!bad.status.success());
}

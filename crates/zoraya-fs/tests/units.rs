//! Unit-level invariant checks: CRC32C, names and paths (INV-0013-1,
//! INV-0013-5), timestamps and the attribute record (INV-0014-4,
//! INV-0014-5), link objects (INV-0013-3) and the test device's crash model
//! (INV-0015-1).

mod common;

use std::collections::BTreeSet;
use std::rc::Rc;

use common::*;
use zoraya_fs::crc32c::crc32c;
use zoraya_fs::format::{ATTR_FIELDS, ATTR_LEN, Attr};
use zoraya_fs::name::{MAX_NAME, NameError, validate_name};
use zoraya_fs::{
    BLOCK_SIZE, FsError, Name, ObjKind, OpenOptions, PublishMode, RelPath, RenameMode, SECTOR_SIZE,
    Timestamp,
};
use zoraya_fs_testkit::crash::{
    CrashConfig, for_each_crash_image, random_sector, sector_choices, windows,
};
use zoraya_fs_testkit::{Event, Rng, TestClock};

#[test]
fn crc32c_test_vectors() {
    // RFC 3720 appendix B.4 and the common check value.
    assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA);
    assert_eq!(crc32c(&[0xFFu8; 32]), 0x62A8_AB43);
    let inc: Vec<u8> = (0..32).collect();
    assert_eq!(crc32c(&inc), 0x46DD_794E);
    let dec: Vec<u8> = (0..32).rev().collect();
    assert_eq!(crc32c(&dec), 0x113F_DB5C);
    assert_eq!(crc32c(b""), 0);
}

/// An independent reference for ADR-0013 rules 1, 2 and 4: a byte-level
/// UTF-8 decoder written from RFC 3629's table, not `core::str`.
fn reference_ok(b: &[u8]) -> bool {
    if b.is_empty() || b.len() > 255 || b == b"." || b == b".." {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        let c0 = b[i];
        let (len, min, init) = match c0 {
            0x00..=0x7F => (1, 0, u32::from(c0)),
            0xC2..=0xDF => (2, 0x80, u32::from(c0 & 0x1F)),
            0xE0..=0xEF => (3, 0x800, u32::from(c0 & 0x0F)),
            0xF0..=0xF4 => (4, 0x10000, u32::from(c0 & 0x07)),
            _ => return false,
        };
        if i + len > b.len() {
            return false;
        }
        let mut cp = init;
        for k in 1..len {
            let c = b[i + k];
            if c & 0xC0 != 0x80 {
                return false;
            }
            cp = (cp << 6) | u32::from(c & 0x3F);
        }
        if cp < min || (0xD800..=0xDFFF).contains(&cp) || cp > 0x10FFFF {
            return false;
        }
        if cp == u32::from(b'/') || cp <= 0x1F || (0x7F..=0x9F).contains(&cp) {
            return false;
        }
        i += len;
    }
    true
}

#[test]
fn name_boundaries() {
    let ok = |s: &[u8]| validate_name(s).is_ok();
    assert!(ok(&[b'a'; 255]));
    assert_eq!(validate_name(&[b'a'; 256]), Err(NameError::TooLong));
    assert_eq!(validate_name(b""), Err(NameError::Empty));
    assert_eq!(validate_name(b"."), Err(NameError::Dot));
    assert_eq!(validate_name(b".."), Err(NameError::Dot));
    assert!(ok(b"..."));
    assert!(ok(b".hidden"));
    assert_eq!(validate_name(b"a/b"), Err(NameError::Slash));
    for c in (0u32..=0x1F).chain([0x7F]).chain(0x80..=0x9F) {
        let s = format!("x{}y", char::from_u32(c).unwrap());
        assert_eq!(
            validate_name(s.as_bytes()),
            Err(NameError::Control),
            "U+{c:04X}"
        );
    }
    assert!(ok("\u{A0}".as_bytes()));
    // Overlong forms, encoded surrogates, above U+10FFFF, truncated.
    for bad in [
        &[0xC0, 0xAF][..],
        &[0xE0, 0x80, 0xAF],
        &[0xED, 0xA0, 0x80],
        &[0xF4, 0x90, 0x80, 0x80],
        &[0xF8, 0x88, 0x80, 0x80, 0x80],
        &[0xE2, 0x82],
    ] {
        assert_eq!(validate_name(bad), Err(NameError::NotUtf8), "{bad:x?}");
    }
    assert!(ok("\u{10FFFF}".as_bytes()));
    // 255 bytes made of multi-byte characters.
    let s = "\u{e9}".repeat(127) + "a";
    assert_eq!(s.len(), 255);
    assert!(ok(s.as_bytes()));
    assert!(!ok(("\u{e9}".repeat(128)).as_bytes()));
    assert_eq!(MAX_NAME, 255);
}

/// INV-0013-1's property test: the validator agrees with the independent
/// reference on random byte strings biased towards UTF-8 and the forbidden
/// characters.
#[test]
fn name_validator_matches_reference() {
    let mut rng = Rng::new(42);
    let pieces: &[&[u8]] = &[
        b"a",
        b"Z",
        b".",
        b"/",
        b"\x00",
        b"\x1f",
        b"\x7f",
        "\u{80}".as_bytes(),
        "\u{9f}".as_bytes(),
        "\u{a0}".as_bytes(),
        "\u{e9}".as_bytes(),
        "\u{fffd}".as_bytes(),
        "\u{10ffff}".as_bytes(),
        "\u{1f600}".as_bytes(),
        &[0xC0],
        &[0x80],
        &[0xED, 0xA0, 0x80],
        &[0xF4, 0x90, 0x80, 0x80],
        &[0xE0, 0x80],
        &[0xFF],
    ];
    let mut accepted = 0;
    for _ in 0..200_000 {
        let mut b = Vec::new();
        let n = rng.below(12);
        for _ in 0..n {
            if rng.chance(1, 8) {
                b.push(rng.below(256) as u8);
            } else {
                b.extend_from_slice(rng.pick(pieces));
            }
        }
        if rng.chance(1, 50) {
            b = vec![b'x'; 250 + rng.below(10) as usize];
        }
        let got = validate_name(&b).is_ok();
        assert_eq!(got, reference_ok(&b), "{b:x?}");
        if got {
            accepted += 1;
            assert!(Name::new(std::str::from_utf8(&b).unwrap()).is_ok());
        }
    }
    assert!(accepted > 10_000);
}

/// INV-0013-5: no request accepts `.`, `..`, an empty component or an
/// absolute path.
#[test]
fn requests_reject_dot_and_absolute_paths() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(256), &clock);
    let root = v.root();
    v.mkdir(root, "d", 0o755).unwrap();
    let f = v
        .open(
            root,
            "d/f",
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        )
        .unwrap();
    let bad = [
        "/d", ".", "..", "d/.", "d/..", "./d", "../d", "d//f", "d/", "",
    ];
    for p in bad {
        let is_bad = |r: Result<(), FsError>| matches!(r, Err(FsError::InvalidName(_)));
        assert!(RelPath::new(p).is_err(), "{p:?}");
        assert!(
            is_bad(v.open(root, p, OpenOptions::default()).map(|_| ())),
            "open {p:?}"
        );
        assert!(is_bad(
            v.open(
                root,
                p,
                OpenOptions {
                    create: Some(0o644),
                    ..Default::default()
                }
            )
            .map(|_| ())
        ));
        assert!(is_bad(v.mkdir(root, p, 0o755)), "mkdir {p:?}");
        assert!(is_bad(v.symlink(root, p, "t")), "symlink {p:?}");
        assert!(is_bad(v.unlink(root, p)), "unlink {p:?}");
        assert!(is_bad(v.rmdir(root, p)), "rmdir {p:?}");
        assert!(
            is_bad(v.rename(root, p, root, "x", RenameMode::Replace)),
            "rename from {p:?}"
        );
        assert!(
            is_bad(v.rename(root, "d", root, p, RenameMode::Replace)),
            "rename to {p:?}"
        );
        assert!(is_bad(v.stat_at(root, p).map(|_| ())), "stat {p:?}");
        let u = v.create_unnamed(root, 0o600).unwrap();
        assert!(
            is_bad(v.publish(u, root, p, PublishMode::NoReplace)),
            "publish {p:?}"
        );
        v.close(u).unwrap();
    }
    v.close(f).unwrap();
    // Exactly the 4,096-byte limit.
    let long = vec!["a"; 2048].join("/");
    assert_eq!(long.len(), 4095);
    assert!(RelPath::new(&long).is_ok());
    assert!(RelPath::new(&(long.clone() + "/b")).is_err());
}

/// INV-0014-4: times round-trip exactly through the encoder and the
/// service, across a remount; out-of-range nanoseconds are refused.
#[test]
fn timestamps_round_trip() {
    let mut rng = Rng::new(8);
    let mut values = vec![
        Timestamp {
            secs: i64::MIN,
            nanos: 0,
        },
        Timestamp {
            secs: i64::MAX,
            nanos: 999_999_999,
        },
        Timestamp { secs: 0, nanos: 0 },
        Timestamp {
            secs: -1,
            nanos: 999_999_999,
        },
        Timestamp {
            secs: 2_147_483_648,
            nanos: 1,
        },
    ];
    for _ in 0..200 {
        values.push(Timestamp {
            secs: rng.next_u64() as i64,
            nanos: rng.below(1_000_000_000) as u32,
        });
    }
    for &t in &values {
        let a = Attr {
            id: 9,
            kind: ObjKind::File,
            size: 1,
            mtime: t,
            ctime: t,
            posix_mode: 0o7777,
        };
        let mut buf = [0u8; ATTR_LEN];
        a.encode(&mut buf).unwrap();
        assert_eq!(Attr::decode(&buf).unwrap(), a);
    }
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(512), &clock);
    let root = v.root();
    let f = v
        .open(
            root,
            "f",
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        )
        .unwrap();
    for bad in [1_000_000_000u32, u32::MAX] {
        assert_eq!(
            v.set_attr(
                f,
                None,
                Some(Timestamp {
                    secs: 0,
                    nanos: bad
                })
            ),
            Err(FsError::InvalidArgument)
        );
        assert_eq!(Timestamp::new(0, bad), None);
        let mut buf = [0u8; ATTR_LEN];
        let a = Attr {
            id: 9,
            kind: ObjKind::File,
            size: 1,
            mtime: T0,
            ctime: T0,
            posix_mode: 0,
        };
        a.encode(&mut buf).unwrap();
        buf[17 + 8..17 + 12].copy_from_slice(&bad.to_le_bytes());
        assert!(Attr::decode(&buf).is_err());
    }
    for (i, &t) in values.iter().enumerate() {
        let name = format!("t{i}");
        let h = v
            .open(
                root,
                &name,
                OpenOptions {
                    create: Some(0o644),
                    ..Default::default()
                },
            )
            .unwrap();
        v.set_attr(h, None, Some(t)).unwrap();
        assert_eq!(v.stat(h).unwrap().mtime, t);
        v.close(h).unwrap();
    }
    v.close(f).unwrap();
    v.close(root).unwrap();
    let d = v.unmount().unwrap();
    let mut v = mount(d, &clock);
    let root = v.root();
    for (i, &t) in values.iter().enumerate() {
        assert_eq!(v.stat_at(root, &format!("t{i}")).unwrap().mtime, t);
    }
}

/// INV-0014-5: the attribute record has exactly ADR-0014 rule 1's fields.
#[test]
fn attribute_record_fields() {
    let names: Vec<&str> = ATTR_FIELDS.iter().map(|f| f.0).collect();
    assert_eq!(
        names,
        ["id", "kind", "size", "mtime", "ctime", "posix_mode"]
    );
    for forbidden in [
        "owner", "uid", "gid", "group", "atime", "crtime", "btime", "xattr",
    ] {
        assert!(!names.contains(&forbidden));
    }
    // The fields tile the record with no gap and no overlap.
    let mut at = 0;
    for &(_, off, len) in ATTR_FIELDS {
        assert_eq!(off, at);
        at += len;
    }
    assert_eq!(at, ATTR_LEN);
}

/// INV-0013-3: the server never follows a link object, wherever it is in
/// the path, and touches no object beyond it.
#[test]
fn link_objects_are_never_followed() {
    let clock = TestClock::new(T0);
    let mut v = mount(fresh(256), &clock);
    let root = v.root();
    v.mkdir(root, "real", 0o755).unwrap();
    v.mkdir(root, "real/b", 0o755).unwrap();
    let t = v
        .open(
            root,
            "real/b/c",
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        )
        .unwrap();
    let target_id = v.stat(t).unwrap().id;
    v.close(t).unwrap();
    v.symlink(root, "a", "real").unwrap();
    v.mkdir(root, "x", 0o755).unwrap();
    v.symlink(root, "x/b", "../real/b").unwrap();
    v.mkdir(root, "y", 0o755).unwrap();
    v.mkdir(root, "y/b", 0o755).unwrap();
    v.symlink(root, "y/b/c", "../../real/b/c").unwrap();
    let cases = [
        ("a/b/c", "b/c", "real"),
        ("x/b/c", "c", "../real/b"),
        ("y/b/c", "", "../../real/b/c"),
    ];
    for (path, remaining, target) in cases {
        let _ = v.take_touched();
        let r = v.open(root, path, OpenOptions::default());
        assert_eq!(
            r,
            Err(FsError::IsLink {
                remaining: remaining.into(),
                target: target.into()
            }),
            "{path}"
        );
        assert!(
            !v.take_touched().contains(&target_id),
            "{path}: the link's target was touched"
        );
        let r = v.stat_at(root, path);
        if remaining.is_empty() {
            // The last component: stat_at does not follow; it describes the link.
            assert_eq!(r.unwrap().kind, ObjKind::Link);
        } else {
            assert!(matches!(r, Err(FsError::IsLink { .. })));
        }
        assert!(!v.take_touched().contains(&target_id));
        // Creating through a link fails the same way.
        let r = v.open(
            root,
            &format!("{path}/new"),
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        );
        assert!(
            matches!(r, Err(FsError::IsLink { .. })),
            "{path}/new: {r:?}"
        );
    }
    // The link itself can be opened, read, renamed and removed.
    let l = v
        .open(
            root,
            "a",
            OpenOptions {
                link_itself: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(v.readlink(l).unwrap(), "real");
    v.close(l).unwrap();
    v.rename(root, "a", root, "a2", RenameMode::NoReplace)
        .unwrap();
    v.unlink(root, "a2").unwrap();
    assert_eq!(v.stat_at(root, "real/b/c").unwrap().id, target_id);
}

/// INV-0015-1: the test device's crash images are exactly what rules 2-4
/// allow. An independent brute-force model computes, for every sector, the
/// set of contents a crash may leave, from the history alone; the
/// enumerator's per-sector sets must equal it, every enumerated image must
/// lie inside it, and on small cases the enumerated images must cover the
/// whole product.
#[test]
fn test_device_crash_model() {
    let blocks = 3usize;
    let spb = BLOCK_SIZE / SECTOR_SIZE;
    let mut rng = Rng::new(15);
    let mut cases = 0;
    for _ in 0..300 {
        // A random history: writes in batches, flushes, on 3 blocks.
        let base: Vec<u8> = {
            let mut b = vec![0u8; blocks * BLOCK_SIZE];
            rng.fill(&mut b);
            b
        };
        let mut events = Vec::new();
        let mut batch = 0;
        for _ in 0..rng.below(7) {
            if rng.chance(1, 3) {
                events.push(Event::Flush);
            } else {
                batch += 1;
                let mut d = [0u8; BLOCK_SIZE];
                rng.fill(&mut d);
                events.push(Event::Write {
                    block: rng.below(blocks as u64),
                    data: Rc::new(d),
                    batch,
                });
            }
        }
        let seed = rng.next_u64();
        for w in windows(&events) {
            // Brute force from the history: replay; a flush makes every write
            // before it stable; the writes after the last flush before the
            // window's end are the unstable ones.
            let mut stable: Vec<Vec<u8>> = base.chunks(SECTOR_SIZE).map(<[u8]>::to_vec).collect();
            let mut unstable: Vec<Vec<BTreeSet<Vec<u8>>>> = Vec::new();
            let mut pending: Vec<(usize, &[u8; BLOCK_SIZE], u64)> = Vec::new();
            for (i, e) in events.iter().enumerate().take(w.end) {
                match e {
                    Event::Write { block, data, .. } => pending.push((i, data, *block)),
                    Event::Flush => {
                        for (_, d, b) in pending.drain(..) {
                            for s in 0..spb {
                                stable[b as usize * spb + s] =
                                    d[s * SECTOR_SIZE..(s + 1) * SECTOR_SIZE].to_vec();
                            }
                        }
                    }
                    _ => {}
                }
            }
            let mut model: Vec<BTreeSet<Vec<u8>>> =
                stable.iter().map(|s| BTreeSet::from([s.clone()])).collect();
            for (i, d, b) in &pending {
                for s in 0..spb {
                    let set = &mut model[*b as usize * spb + s];
                    set.insert(d[s * SECTOR_SIZE..(s + 1) * SECTOR_SIZE].to_vec());
                    set.insert(vec![0; SECTOR_SIZE]);
                    set.insert(random_sector(seed, *i, s).to_vec());
                }
            }
            let _ = &mut unstable;
            assert_eq!(
                sector_choices(&base, &events, &w, seed),
                model,
                "window {}",
                w.index
            );
            cases += 1;
        }
        // Every image the enumerator produces lies inside the model's sets.
        let base_rc = Rc::new(base.clone());
        let cfg = CrashConfig {
            k: 12,
            random_subsets: 8,
            torn: 24,
            seed,
        };
        for_each_crash_image(&base_rc, &events, cfg, |w, _, dev| {
            let choices = sector_choices(&base, &events, w, seed);
            let img = dev.to_image();
            for (s, set) in choices.iter().enumerate() {
                assert!(
                    set.contains(&img[s * SECTOR_SIZE..(s + 1) * SECTOR_SIZE]),
                    "sector {s} outside the model"
                );
            }
        });
    }
    assert!(cases > 300);
    // Small case: one unstable write; the torn variants together with the
    // subsets reach all four choices of every sector.
    let base = vec![1u8; BLOCK_SIZE];
    let events = vec![Event::Write {
        block: 0,
        data: Rc::new([2u8; BLOCK_SIZE]),
        batch: 1,
    }];
    let mut seen: Vec<BTreeSet<Vec<u8>>> = vec![BTreeSet::new(); spb];
    let cfg = CrashConfig {
        k: 12,
        random_subsets: 0,
        torn: 200,
        seed: 1,
    };
    for_each_crash_image(&Rc::new(base.clone()), &events, cfg, |_, _, dev| {
        let img = dev.to_image();
        for s in 0..spb {
            seen[s].insert(img[s * SECTOR_SIZE..(s + 1) * SECTOR_SIZE].to_vec());
        }
    });
    let w = &windows(&events)[0];
    assert_eq!(seen, sector_choices(&base, &events, w, 1));
}

/// INV-0015-1: sectors of one write choose independently (TASK-07 review
/// R-10). One choice per write would give every sector the same value.
#[test]
fn sectors_of_one_write_tear_independently() {
    use std::rc::Rc;
    use zoraya_fs_testkit::Event;
    use zoraya_fs_testkit::crash::{torn_image, windows};
    let base = Rc::new(vec![1u8; zoraya_fs::BLOCK_SIZE]);
    let events = vec![Event::Write {
        block: 0,
        data: Rc::new([2u8; zoraya_fs::BLOCK_SIZE]),
        batch: 1,
    }];
    let w = &windows(&events)[0];
    for seed in 0..8 {
        let image = torn_image(&base, &events, w, seed, 1).to_image();
        let mut kinds: Vec<&[u8]> = image.chunks(512).collect();
        kinds.sort();
        kinds.dedup();
        assert!(
            kinds.len() > 1,
            "seed {seed}: one choice for the whole write"
        );
    }
}

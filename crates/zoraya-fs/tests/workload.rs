//! Toolchain workload replay (TASK-07; `docs/research/fs/requirements.md`
//! sections 2 and 4.1).
//!
//! Replays the filesystem operation pattern of a build, as measured in
//! TASK-02: `configure`-style probes with exclusive temporary files
//! (`mkstemp`, section 2.3), `sed -i`/`mv` writing a temporary next to the
//! target and renaming it over (section 2.2), tcc unlinking and rewriting
//! its output in place (section 2.2), `>>` appends to a log (section 2.6),
//! and make's `stat` of targets and prerequisites (section 4.1). Checks the
//! make-relevant behaviour: a target written after its prerequisite has a
//! strictly later modification time, also when the wall clock stands still
//! or steps backwards, times keep nanosecond resolution, and every rename
//! over a target leaves the complete old or new content (here: new, since
//! nothing crashes).

mod common;

use common::*;
use zoraya_fs::{FsError, OpenOptions, RenameMode, Timestamp, Volume};
use zoraya_fs_testkit::ops::add_ns;
use zoraya_fs_testkit::{MemDevice, Rng, TestClock};

type V = Volume<MemDevice, TestClock>;

fn create(v: &mut V, path: &str, excl: bool) -> Result<zoraya_fs::Handle, FsError> {
    let root = v.root();
    let r = v.open(
        root,
        path,
        OpenOptions {
            create: Some(0o644),
            exclusive: excl,
            truncate: !excl,
            ..Default::default()
        },
    );
    v.close(root).unwrap();
    r
}

fn mtime(v: &mut V, path: &str) -> Timestamp {
    let root = v.root();
    let t = v.stat_at(root, path).unwrap().mtime;
    v.close(root).unwrap();
    t
}

fn write_file(v: &mut V, path: &str, data: &[u8]) {
    let h = create(v, path, false).unwrap();
    v.write(h, 0, data).unwrap();
    v.close(h).unwrap();
}

/// `sed -i`: a temporary next to the target, then rename over it.
fn sed_i(v: &mut V, rng: &mut Rng, path: &str, data: &[u8]) {
    let root = v.root();
    loop {
        let tmp = format!("{}.tmp{:06}", path, rng.below(1_000_000));
        match create(v, &tmp, true) {
            Ok(h) => {
                v.write(h, 0, data).unwrap();
                v.close(h).unwrap();
                v.rename(root, &tmp, root, path, RenameMode::Replace)
                    .unwrap();
                break;
            }
            Err(FsError::Exists) => continue,
            Err(e) => panic!("{e:?}"),
        }
    }
    v.close(root).unwrap();
}

/// tcc: unlink the output, then create it and write it in pieces.
fn tcc(v: &mut V, out: &str, size: usize) {
    let root = v.root();
    match v.unlink(root, out) {
        Ok(()) | Err(FsError::NotFound) => {}
        Err(e) => panic!("{e:?}"),
    }
    v.close(root).unwrap();
    let h = create(v, out, false).unwrap();
    let mut off = 0;
    while off < size {
        let n = (size - off).min(1500);
        v.write(h, off as u64, &vec![0xCC; n]).unwrap();
        off += n;
    }
    v.close(h).unwrap();
}

fn append(v: &mut V, path: &str, line: &str) {
    let root = v.root();
    let h = v
        .open(
            root,
            path,
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        )
        .unwrap();
    let size = v.stat(h).unwrap().size;
    v.write(h, size, line.as_bytes()).unwrap();
    v.close(h).unwrap();
    v.close(root).unwrap();
}

#[test]
fn build_replay_keeps_make_correct() {
    for (label, step) in [("advancing", 1), ("stuck", 0), ("backwards", -1)] {
        let clock = TestClock::new(T0);
        let mut v = mount(fresh(4096), &clock);
        let mut rng = Rng::new(17);
        let root = v.root();
        v.mkdir(root, "src", 0o755).unwrap();
        v.mkdir(root, "obj", 0o755).unwrap();
        v.close(root).unwrap();
        let tick = |clock: &TestClock| {
            let t = clock.wall_now();
            match step {
                1 => clock.set_wall(add_ns(t, 1_000)),
                -1 => clock.set_wall(Timestamp {
                    secs: t.secs - 1,
                    nanos: t.nanos,
                }),
                _ => {}
            }
        };
        // configure: probes with mkstemp-style temporaries, appends to a log.
        for i in 0..40 {
            tick(&clock);
            let h = create(&mut v, &format!("conftest{i}.c"), true).unwrap();
            v.write(h, 0, b"int main(void){return 0;}\n").unwrap();
            v.close(h).unwrap();
            append(
                &mut v,
                "config.log",
                &format!("checking probe {i}... yes\n"),
            );
            let root = v.root();
            v.unlink(root, &format!("conftest{i}.c")).unwrap();
            v.close(root).unwrap();
        }
        // Generated headers via sed -i, sources, then compile each source.
        for i in 0..30 {
            tick(&clock);
            write_file(
                &mut v,
                &format!("src/m{i}.c"),
                format!("#include \"t{i}.h\"\n").as_bytes(),
            );
            write_file(&mut v, &format!("src/t{i}.h"), b"#define X 1\n");
            sed_i(&mut v, &mut rng, &format!("src/t{i}.h"), b"#define X 2\n");
        }
        for i in 0..30 {
            tick(&clock);
            tcc(&mut v, &format!("obj/m{i}.o"), 3000 + i * 100);
        }
        // make's rule: a target is up to date iff no prerequisite is newer.
        for i in 0..30 {
            let (src, hdr, obj) = (
                format!("src/m{i}.c"),
                format!("src/t{i}.h"),
                format!("obj/m{i}.o"),
            );
            let (ts, th, to) = (
                mtime(&mut v, &src),
                mtime(&mut v, &hdr),
                mtime(&mut v, &obj),
            );
            assert!(
                to > ts && to > th,
                "{label}: obj/m{i}.o not newer than its prerequisites"
            );
        }
        // Touch one header: exactly its object is out of date.
        tick(&clock);
        sed_i(&mut v, &mut rng, "src/t7.h", b"#define X 3\n");
        for i in 0..30 {
            let stale =
                mtime(&mut v, &format!("src/t{i}.h")) > mtime(&mut v, &format!("obj/m{i}.o"));
            assert_eq!(stale, i == 7, "{label}: m{i}");
        }
        // Rebuild it; it is newer again, with a distinct time.
        tcc(&mut v, "obj/m7.o", 4000);
        assert!(mtime(&mut v, "obj/m7.o") > mtime(&mut v, "src/t7.h"));
        // Every rename over a target left the complete new content.
        let root = v.root();
        let h = v.open(root, "src/t7.h", OpenOptions::default()).unwrap();
        let mut b = [0u8; 32];
        let n = v.read(h, 0, &mut b).unwrap();
        assert_eq!(&b[..n], b"#define X 3\n");
        // Two writes within one wall-clock nanosecond get distinct times.
        let a = mtime(&mut v, "config.log");
        append(&mut v, "config.log", "x\n");
        let b2 = mtime(&mut v, "config.log");
        assert!(b2 > a);
        // Resolution: the nanoseconds of an advancing clock are kept.
        if step == 1 {
            // Clearly ahead of every time assigned so far (assigned times
            // may run ahead of a slow clock, ADR-0014 rule 3).
            write_file(&mut v, "obj/precise", b"x");
            let t = clock.wall_now();
            let t = add_ns(t, 1_000_000_123);
            clock.set_wall(t);
            // One request: its time is exactly the clock's.
            let h = create(&mut v, "obj/precise", false).unwrap();
            assert_eq!(mtime(&mut v, "obj/precise"), t);
            v.close(h).unwrap();
        }
        // No temporaries left behind.
        let mut names = Vec::new();
        let mut after: Option<String> = None;
        while let Some(e) = v.readdir(root, after.as_deref()).unwrap() {
            after = Some(e.name.clone());
            names.push(e.name);
        }
        assert!(
            !names
                .iter()
                .any(|n| n.contains(".tmp") || n.starts_with("conftest")),
            "{names:?}"
        );
        // And the whole history survives a remount with its times.
        let before = mtime(&mut v, "obj/m7.o");
        v.close(h).unwrap();
        v.close(root).unwrap();
        let d = v.unmount().unwrap();
        let mut v = mount(d, &clock);
        assert_eq!(mtime(&mut v, "obj/m7.o"), before);
        tcc(&mut v, "obj/m7.o", 10);
        assert!(
            mtime(&mut v, "obj/m7.o") > before,
            "{label}: time went back after remount"
        );
    }
}

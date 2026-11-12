//! One time base for every volume (ADR-0191): the root volume's service
//! takes it from its volume, a temporary volume is made on it, and a file
//! written on the temporary volume after one on the root volume is later,
//! whatever the root volume had stored.

use std::cell::Cell;
use std::rc::Rc;

use zoraya_fs::{Clock, MkfsOptions, MountOptions, ObjKind, OpenOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::memory::MemoryDevice;
use zoraya_fs_service::{time_base, wall_time};

/// fsd's clock: the base plus a monotonic clock the test moves.
#[derive(Clone)]
struct BaseClock {
    base: Rc<Cell<u64>>,
    mono: Rc<Cell<u64>>,
}

impl Clock for BaseClock {
    fn wall(&mut self) -> Timestamp {
        wall_time(self.base.get(), self.mono.get())
    }
    fn monotonic_ns(&mut self) -> u64 {
        self.mono.get()
    }
}

fn ns(t: Timestamp) -> u64 {
    t.secs as u64 * 1_000_000_000 + u64::from(t.nanos)
}

#[test]
fn the_base_goes_on_from_the_latest_stored_time() {
    let latest = Timestamp {
        secs: 1_800_000_000,
        nanos: 5,
    };
    let base = time_base(latest, 3_000_000_000, 0);
    assert_eq!(base + 3_000_000_000, ns(latest) + 1);
    assert!(ns(wall_time(base, 3_000_000_000)) > ns(latest));
    // Nothing stored, or a clock ahead of the stored time: no negative base.
    assert_eq!(time_base(Timestamp { secs: 0, nanos: 0 }, 99, 0), 0);
    assert_eq!(time_base(Timestamp { secs: -5, nanos: 0 }, 0, 0), 1);
    let t = wall_time(1_500_000_000, 2_600_000_000);
    assert_eq!((t.secs, t.nanos), (4, 100_000_000));
}

/// INV-0226-1: the base is not below the floor and not below the latest
/// stored time plus one, whichever is higher.
#[test]
fn the_base_is_not_below_the_floor_or_the_stored_time() {
    let stored = Timestamp {
        secs: 1_800_000_000,
        nanos: 0,
    };
    let mono = 7_000_000_000;
    // A floor above the stored time raises the clock to it.
    let floor = ns(stored) + 5_000_000_000;
    assert_eq!(time_base(stored, mono, floor) + mono, floor);
    // A floor equal to the stored time: the stored time plus one wins.
    assert_eq!(time_base(stored, mono, ns(stored)) + mono, ns(stored) + 1);
    // A floor below it changes nothing (a host clock that went back).
    assert_eq!(time_base(stored, mono, 1), time_base(stored, mono, 0));
    // A floor with nothing stored is the clock itself.
    let none = Timestamp { secs: 0, nanos: 0 };
    assert_eq!(time_base(none, mono, floor) + mono, floor);
    // The monotonic clock ahead of the floor gives 0, never a wrap.
    assert_eq!(time_base(none, floor + 1, floor), 0);
}

#[test]
fn a_file_in_tmp_written_after_one_in_home_is_later() {
    // INV-0191-1's host half. The root volume was made and written on a
    // previous boot that ran for an hour: its latest time is far above this
    // boot's monotonic clock.
    let earlier = Timestamp {
        secs: 1_790_000_000,
        nanos: 0,
    };
    let mut root_device = MemoryDevice::new(vec![0u8; 4 << 20]);
    mkfs(
        &mut root_device,
        MkfsOptions {
            volume_id: 1,
            now: earlier,
            root_mode: 0o755,
        },
    )
    .unwrap();
    let mono = Rc::new(Cell::new(0));
    let previous = BaseClock {
        base: Rc::new(Cell::new(ns(earlier))),
        mono: mono.clone(),
    };
    mono.set(3_600_000_000_000);
    let mut v = Volume::mount(root_device, previous, MountOptions::default()).unwrap();
    let root = v.root();
    let h = v
        .open(
            root,
            "old",
            OpenOptions {
                create: Some(0o644),
                ..Default::default()
            },
        )
        .unwrap();
    v.write(h, 0, b"x").unwrap();
    v.close(h).unwrap();
    v.commit().unwrap();
    let stored = v.last_assigned();
    let device = v.into_device();
    // This boot: two seconds in when the root volume is mounted.
    mono.set(2_000_000_000);
    let base = Rc::new(Cell::new(0));
    let clock = BaseClock {
        base: base.clone(),
        mono: mono.clone(),
    };
    let mut home = Volume::mount(device, clock.clone(), MountOptions::default()).unwrap();
    base.set(time_base(home.last_assigned(), mono.get(), 0));
    assert_eq!(home.last_assigned(), stored);
    // The temporary volume, made on the same base a little later.
    mono.set(2_500_000_000);
    let mut tmp_device = MemoryDevice::new(vec![0u8; 4 << 20]);
    mkfs(
        &mut tmp_device,
        MkfsOptions {
            volume_id: 2,
            now: wall_time(base.get(), mono.get()),
            root_mode: 0o1777,
        },
    )
    .unwrap();
    let mut tmp = Volume::mount(tmp_device, clock.clone(), MountOptions::default()).unwrap();
    let make = |v: &mut Volume<MemoryDevice<Vec<u8>>, BaseClock>, name: &str| {
        let root = v.root();
        let h = v
            .open(
                root,
                name,
                OpenOptions {
                    create: Some(0o644),
                    ..Default::default()
                },
            )
            .unwrap();
        v.write(h, 0, b"data").unwrap();
        let a = v.stat(h).unwrap();
        assert_eq!(a.kind, ObjKind::File);
        v.close(h).unwrap();
        a.mtime
    };
    mono.set(3_000_000_000);
    let in_home = make(&mut home, "prerequisite");
    mono.set(3_000_100_000);
    let in_tmp = make(&mut tmp, "target");
    assert!(
        in_home > stored,
        "the root volume's time goes on from what it had"
    );
    assert!(in_tmp > in_home, "{in_tmp:?} after {in_home:?}");
    // CLOCK_REALTIME, read after both: never below either.
    let realtime = wall_time(base.get(), mono.get() + 1);
    assert!(realtime >= in_tmp && realtime >= in_home);
    // And the other way round: home after tmp.
    mono.set(3_000_200_000);
    let later_home = make(&mut home, "again");
    assert!(later_home > in_tmp);
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
zoraya_test_alloc::cap_memory!();

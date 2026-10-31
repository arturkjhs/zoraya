//! Helpers shared by the integration tests.
#![allow(dead_code)] // each test binary uses a different subset

use zoraya_fs::{BlockDevice, MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_testkit::model::Model;
use zoraya_fs_testkit::ops::Session;
use zoraya_fs_testkit::{MemDevice, TestClock};

pub const T0: Timestamp = Timestamp {
    secs: 1_700_000_000,
    nanos: 0,
};
pub const ROOT_MODE: u16 = 0o755;

/// A freshly made volume of `blocks` blocks.
pub fn fresh(blocks: u64) -> MemDevice {
    let mut d = MemDevice::new(blocks);
    mkfs(
        &mut d,
        MkfsOptions {
            volume_id: 0x5EED_0001,
            now: T0,
            root_mode: ROOT_MODE,
        },
    )
    .expect("mkfs");
    d
}

pub fn mount<D: BlockDevice>(d: D, clock: &TestClock) -> Volume<D, TestClock> {
    Volume::mount(d, clock.clone(), MountOptions::default()).expect("mount")
}

/// A session on a fresh volume.
pub fn session<D: BlockDevice>(d: D) -> Session<D> {
    let clock = TestClock::new(T0);
    let vol = mount(d, &clock);
    Session::new(vol, Model::new(ROOT_MODE, T0), clock)
}

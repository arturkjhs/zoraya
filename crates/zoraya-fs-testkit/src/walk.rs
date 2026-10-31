//! Mount and traverse an arbitrary image through the engine's request API,
//! the way a client would: every directory, every name, every file's first
//! and last bytes, every link target. Used by the decoder fuzz test and the
//! cargo-fuzz target. Never panics on a bad image; returns what it saw.

use std::collections::HashSet;

use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{
    BlockDevice, FsError, Handle, MountOptions, ObjKind, OpenOptions, Timestamp, Volume,
};

use crate::clock::TestClock;

/// What a walk did.
#[derive(Debug, Clone, Default)]
pub struct WalkStats {
    pub mounted: bool,
    pub objects: u64,
    pub errors: u64,
    /// Engine requests issued.
    pub requests: u64,
    /// Device reads the engine issued.
    pub device_reads: u64,
}

/// Bytes read from the start and from the end of each file.
pub const READ_SPAN: usize = 16 * 1024;

/// Walks the volume on `dev`. `visited` and `buf` are provided by the caller
/// so that the walk itself allocates little (for INV-0012-4's measurement).
pub fn walk<D: BlockDevice>(
    dev: D,
    read_write: bool,
    visited: &mut HashSet<u64>,
    buf: &mut Vec<u8>,
) -> (WalkStats, Option<D>) {
    let mut st = WalkStats::default();
    let opts = MountOptions {
        read_only: !read_write,
        ..Default::default()
    };
    let mut v = match Volume::mount(dev, TestClock::new(Timestamp::default()), opts) {
        Ok(v) => v,
        Err(_) => return (st, None),
    };
    st.mounted = true;
    let root = v.root();
    visited.clear();
    visited.insert(v.stat(root).map(|a| a.id).unwrap_or(0));
    dir(&mut v, root, visited, buf, &mut st, 0);
    let _ = v.close(root);
    st.device_reads = v.device_reads();
    (st, Some(v.into_device()))
}

fn dir<D: BlockDevice>(
    v: &mut Volume<D, TestClock>,
    h: Handle,
    visited: &mut HashSet<u64>,
    buf: &mut Vec<u8>,
    st: &mut WalkStats,
    depth: usize,
) {
    // Depth is bounded by the visited set (each directory once); this is a
    // second guard for the walker's own stack.
    if depth > 2048 {
        return;
    }
    let mut after: Option<String> = None;
    loop {
        st.requests += 1;
        let e = match v.readdir(h, after.as_deref()) {
            Ok(Some(e)) => e,
            Ok(None) => return,
            Err(_) => {
                st.errors += 1;
                return;
            }
        };
        after = Some(e.name.clone());
        if !visited.insert(e.id) {
            continue;
        }
        st.objects += 1;
        st.requests += 1;
        let c = match v.open(
            h,
            &e.name,
            OpenOptions {
                link_itself: true,
                ..Default::default()
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                st.errors += 1;
                continue;
            }
        };
        st.requests += 1;
        match v.stat(c) {
            Ok(a) => match a.kind {
                ObjKind::Directory => dir(v, c, visited, buf, st, depth + 1),
                ObjKind::Link => {
                    st.requests += 1;
                    if v.readlink(c).is_err() {
                        st.errors += 1;
                    }
                }
                ObjKind::File => {
                    for off in [0, a.size.saturating_sub(READ_SPAN as u64)] {
                        st.requests += 1;
                        buf.resize(READ_SPAN, 0);
                        match v.read(c, off, buf) {
                            Ok(_) | Err(FsError::Corrupt) => {}
                            Err(_) => st.errors += 1,
                        }
                    }
                }
            },
            Err(_) => st.errors += 1,
        }
        let _ = v.close(c);
    }
}

/// The checker on the same image: it must terminate and not panic.
pub fn fsck<D: BlockDevice>(dev: &mut D) -> usize {
    check(dev, CheckOptions::default()).problems.len()
}

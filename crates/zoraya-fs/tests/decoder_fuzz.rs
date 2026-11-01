//! The decoder fuzz test (INV-0012-3, INV-0012-4), deterministic: fixed
//! seeds and counts, like `zoraya-dtb`'s mutation test.
//!
//! - **Field-level**: every field of every structure of two images (root
//!   records, metadata headers, keys, values, pointers, allocator and bitmap
//!   blocks), each with XFS's eight transformations, with checksums
//!   recomputed up to the root record so that only structural validation
//!   can catch the change.
//! - **Random**: 10,000 images with 1 to 8 random bytes changed (three in
//!   four in reachable blocks), checksums recomputed for half of them.
//!
//! For each image: mount (read-only, and read-write for a share) and walk
//! everything; run the checker. Asserts no panic (a panic fails the test),
//! a device-read count linear in the volume size, and a peak of decoder
//! state within `MAX_DEPTH × BLOCK_SIZE + CACHE_CAP` (measured with a
//! counting allocator, per thread). `#![forbid(unsafe_code)]` in the crate
//! is the compile-time half of INV-0012-3.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use common::*;
use zoraya_fs::format::MAX_DEPTH;
use zoraya_fs::{BLOCK_SIZE, BlockDevice, CACHE_CAP, OpenOptions};
use zoraya_fs_testkit::fields::{TRANSFORMS, field_map, fix_checksums, reseal_root, transform};
use zoraya_fs_testkit::walk::{fsck, walk};
use zoraya_fs_testkit::{ImageDevice, MemDevice, Rng, TestClock};

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: forwards to the system allocator unchanged; only adds counting
// in thread-local cells, which never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = LIVE.try_with(|c| {
            let n = c.get() + l.size() as isize;
            c.set(n);
            let _ = PEAK.try_with(|p| p.set(p.get().max(n)));
        });
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let _ = LIVE.try_with(|c| c.set(c.get() - l.size() as isize));
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Allowance for the walker's own transient data (names, link targets in
/// errors), on top of the engine's bound.
const WALKER_SLACK: isize = 64 * 1024;

struct Bench {
    images: u64,
    mounted: u64,
    peak: isize,
    max_reads_per_block: f64,
    visited: HashSet<u64>,
    buf: Vec<u8>,
}

impl Bench {
    fn new() -> Bench {
        Bench {
            images: 0,
            mounted: 0,
            peak: 0,
            max_reads_per_block: 0.0,
            visited: HashSet::with_capacity(1 << 16),
            buf: Vec::with_capacity(zoraya_fs_testkit::walk::READ_SPAN),
        }
    }

    fn run(&mut self, dev: ImageDevice, read_write: bool) {
        let blocks = dev.block_count() as f64;
        let mut copy = dev.clone();
        let base = LIVE.with(Cell::get);
        PEAK.with(|p| p.set(base));
        let (st, _) = walk(dev, read_write, &mut self.visited, &mut self.buf);
        let peak = PEAK.with(Cell::get) - base;
        if !read_write {
            // INV-0012-4: decoder state bound (the read-only path holds no
            // allocator bitmap).
            let bound = (MAX_DEPTH as usize * BLOCK_SIZE + CACHE_CAP) as isize + WALKER_SLACK;
            assert!(peak <= bound, "peak {peak} bytes over the bound {bound}");
            self.peak = self.peak.max(peak);
        }
        let ratio = st.device_reads as f64 / blocks;
        // A bound linear in the number of blocks (generous constant).
        assert!(
            ratio <= 512.0,
            "{} device reads on {blocks} blocks",
            st.device_reads
        );
        self.max_reads_per_block = self.max_reads_per_block.max(ratio);
        fsck(&mut copy);
        self.images += 1;
        self.mounted += u64::from(st.mounted);
    }
}

fn image(variant: u64) -> Rc<Vec<u8>> {
    let clock = TestClock::new(T0);
    let mut dev: MemDevice = fresh(if variant == 0 { 96 } else { 1000 });
    {
        let mut v = mount(&mut dev, &clock);
        let root = v.root();
        v.mkdir(root, "d", 0o755).unwrap();
        v.mkdir(root, "d/e", 0o700).unwrap();
        v.symlink(root, "d/ln", "../d/e/f0").unwrap();
        let n = if variant == 0 { 12 } else { 260 };
        for i in 0..n {
            let f = v
                .open(
                    root,
                    &format!("d/e/f{i}"),
                    OpenOptions {
                        create: Some(0o644),
                        ..Default::default()
                    },
                )
                .unwrap();
            let size = [0usize, 10, 4096, 9000][i % 4];
            v.write(f, 0, &vec![i as u8; size]).unwrap();
            v.close(f).unwrap();
        }
        // An object on the orphan list: an unnamed file kept open at unmount.
        let u = v.create_unnamed(root, 0o600).unwrap();
        v.write(u, 0, b"orphan").unwrap();
        v.unmount().unwrap();
    }
    Rc::new(dev.into_image())
}

/// Every field of every structure kind, and of repeated instances (the items
/// of a node) the first two and the last of each field name per block.
fn sample(fields: &[zoraya_fs_testkit::fields::Field]) -> Vec<zoraya_fs_testkit::fields::Field> {
    use std::collections::HashMap;
    let mut by: HashMap<(u64, &str), Vec<usize>> = HashMap::new();
    for (i, f) in fields.iter().enumerate() {
        by.entry((f.block, f.what)).or_default().push(i);
    }
    let mut keep: Vec<usize> = Vec::new();
    for v in by.values() {
        keep.extend(v.iter().take(2));
        if v.len() > 2 {
            keep.push(*v.last().unwrap());
        }
    }
    keep.sort_unstable();
    keep.into_iter().map(|i| fields[i]).collect()
}

#[test]
fn field_level_mutation() {
    let mut b = Bench::new();
    let mut rng = Rng::new(1);
    let mut fields = 0;
    for variant in 0..2 {
        let base = image(variant);
        let mut probe = ImageDevice::new(base.clone());
        let map = field_map(&mut probe).unwrap();
        if variant == 1 {
            assert!(
                map.rec.tree_height >= 2,
                "the larger image must have interior nodes"
            );
        }
        for f in &sample(&map.fields) {
            for t in 0..TRANSFORMS {
                let mut d = ImageDevice::new(base.clone());
                let old = d.get_block(f.block);
                let mut blk = old;
                transform(&mut blk[f.off..f.off + f.len], t, rng.next_u64());
                if blk == old {
                    continue;
                }
                d.set_block(f.block, blk);
                if map.slots.contains(&f.block) {
                    reseal_root(&mut d, &map);
                } else {
                    fix_checksums(&mut d, &map, f.block, &old);
                }
                b.run(d, t % 3 == 0);
            }
            fields += 1;
        }
    }
    eprintln!(
        "field-level fuzz: {fields} fields x {TRANSFORMS} transformations, {} images, {} mounted, peak decoder \
         state {} bytes, at most {:.1} device reads per block",
        b.images, b.mounted, b.peak, b.max_reads_per_block
    );
    assert!(fields > 1000);
}

const RANDOM_IMAGES: u64 = 10_000;

#[test]
fn random_mutation() {
    let mut b = Bench::new();
    let mut rng = Rng::new(2);
    let bases = [image(0), image(1)];
    for i in 0..RANDOM_IMAGES {
        let base = &bases[(i % 2) as usize];
        let mut probe = ImageDevice::new(base.clone());
        let map = if i % 2 == 0 {
            field_map(&mut probe).ok()
        } else {
            None
        };
        let mut d = ImageDevice::new(base.clone());
        let blocks = (base.len() / BLOCK_SIZE) as u64;
        for _ in 0..1 + rng.below(8) {
            let blk_no = match &map {
                Some(m) if rng.chance(3, 4) => *m
                    .kinds
                    .keys()
                    .nth(rng.below(m.kinds.len() as u64) as usize)
                    .unwrap(),
                _ => rng.below(blocks),
            };
            let old = d.get_block(blk_no);
            let mut blk = old;
            let pos = rng.below(BLOCK_SIZE as u64) as usize;
            blk[pos] = rng.next_u64() as u8;
            d.set_block(blk_no, blk);
            if let Some(m) = &map
                && rng.chance(1, 2)
            {
                if m.slots.contains(&blk_no) {
                    reseal_root(&mut d, m);
                } else if m.kinds.contains_key(&blk_no) {
                    fix_checksums(&mut d, m, blk_no, &old);
                }
            }
        }
        b.run(d, i % 5 == 0);
    }
    eprintln!(
        "random fuzz: {} images, {} mounted, peak decoder state {} bytes, at most {:.1} device reads per block",
        b.images, b.mounted, b.peak, b.max_reads_per_block
    );
}

/// INV-0012-4 on a tree far larger than the node cache: the small fuzz images
/// could not reach the bound, whatever the cache did (TASK-07 review R-03).
/// The read-write path also holds one bit per block of allocator bitmap, which
/// ADR-0012's bound does not count, so only the read-only path is held to it.
#[test]
fn large_tree_stays_within_the_decoder_bound() {
    use zoraya_fs::{CACHE_ENTRIES, MkfsOptions, MountOptions, OpenOptions, Timestamp, mkfs};
    use zoraya_fs_testkit::{MemDevice, TestClock};
    let t0 = Timestamp {
        secs: 1_700_000_000,
        nanos: 0,
    };
    let clock = TestClock::new(t0);
    let mut dev = MemDevice::new(12_000);
    mkfs(
        &mut dev,
        MkfsOptions {
            volume_id: 1,
            now: t0,
            root_mode: 0o755,
        },
    )
    .unwrap();
    {
        let mut v =
            zoraya_fs::Volume::mount(&mut dev, clock.clone(), MountOptions::default()).unwrap();
        let r = v.root();
        for i in 0..1500 {
            let f = v
                .open(
                    r,
                    &format!("file-{i:05}"),
                    OpenOptions {
                        create: Some(0o644),
                        ..Default::default()
                    },
                )
                .unwrap();
            v.write(f, 0, &[7u8; 5000]).unwrap();
            v.close(f).unwrap();
        }
        v.close(r).unwrap();
        v.unmount().unwrap();
    }
    let image = std::rc::Rc::new(dev.into_image());

    let read_only = MountOptions {
        read_only: true,
        ..Default::default()
    };
    let d = ImageDevice::new(image.clone());
    let mut v = zoraya_fs::Volume::mount(d, clock, read_only).unwrap();
    let r = v.root();
    for i in 0..1500 {
        v.stat_at(r, &format!("file-{i:05}")).unwrap();
        assert!(v.cached_nodes() <= CACHE_ENTRIES, "{}", v.cached_nodes());
    }

    let mut bench = Bench::new();
    bench.run(ImageDevice::new(image), false);
    assert!(bench.peak > 0);
}

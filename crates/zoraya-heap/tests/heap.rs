//! ADR-0022's guarantees against hand-written cases and a model.

use std::alloc::{Layout, alloc, dealloc};
use std::collections::BTreeMap;

use zoraya_heap::{Backing, CHUNK_SIZE, GROW_SIZE, Heap, MAX_SMALL, PAGE_SIZE};

/// Page-aligned host memory, handed out upward until `limit`, optionally
/// leaving a hole after each range so that nothing is contiguous.
struct Arena {
    base: usize,
    used: usize,
    limit: usize,
    gap: usize,
}

impl Arena {
    fn layout(limit: usize) -> Layout {
        Layout::from_size_align(limit, PAGE_SIZE).unwrap()
    }

    fn new(limit: usize, gap: usize) -> Arena {
        // SAFETY: the layout has a non-zero size.
        let base = unsafe { alloc(Self::layout(limit)) } as usize;
        assert_ne!(base, 0);
        Arena {
            base,
            used: 0,
            limit,
            gap,
        }
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with this layout.
        unsafe { dealloc(self.base as *mut u8, Self::layout(self.limit)) };
    }
}

// SAFETY: ranges are page-aligned, disjoint parts of an allocation that
// outlives the heap, because the heap owns the arena.
unsafe impl Backing for Arena {
    fn grow(&mut self, pages: usize) -> Option<usize> {
        let bytes = pages * PAGE_SIZE;
        if self.used + bytes > self.limit {
            return None;
        }
        let at = self.base + self.used;
        self.used += bytes + self.gap;
        Some(at)
    }
}

fn layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align).unwrap()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Fills a block with its own address and checks it later, so that an
/// overlap between live blocks shows up as a changed byte.
fn stamp(p: *mut u8, size: usize) {
    for i in 0..size {
        // SAFETY: the block has `size` bytes.
        unsafe { p.add(i).write(((p as usize + i) >> 3) as u8 ^ i as u8) };
    }
}

fn check(p: *mut u8, size: usize) {
    for i in 0..size {
        // SAFETY: the block has `size` bytes.
        let got = unsafe { p.add(i).read() };
        assert_eq!(
            got,
            ((p as usize + i) >> 3) as u8 ^ i as u8,
            "byte {i} of {p:?}"
        );
    }
}

#[test]
fn every_size_and_alignment_is_honoured() {
    let mut heap = Heap::new(Arena::new(64 << 20, 0));
    let mut live = Vec::new();
    for align_shift in 0..=16 {
        let align = 1usize << align_shift;
        for size in [1, 7, 8, 16, 17, 100, 1024, 2048, 2049, 4096, 5000, 70000] {
            let l = layout(size, align);
            let p = heap.alloc(l);
            assert!(!p.is_null());
            assert!(
                (p as usize).is_multiple_of(align),
                "{size} aligned {align}: {p:?}"
            );
            stamp(p, size);
            live.push((p, l));
        }
    }
    for (p, l) in live {
        check(p, l.size());
        // SAFETY: from `alloc` with this layout, freed once.
        unsafe { heap.dealloc(p, l) };
    }
    assert_eq!(heap.stats().live, 0);
}

#[test]
fn freed_memory_is_reused_without_growing() {
    let mut heap = Heap::new(Arena::new(16 << 20, 0));
    let small = layout(64, 8);
    let large = layout(3 * PAGE_SIZE, 8);
    let s = heap.alloc(small);
    let l = heap.alloc(large);
    let grows = heap.stats().grows;
    for _ in 0..10_000 {
        // SAFETY: each block was just allocated with this layout.
        unsafe {
            heap.dealloc(s, small);
            heap.dealloc(l, large);
        }
        assert_eq!(heap.alloc(small), s);
        assert_eq!(heap.alloc(large), l);
    }
    assert_eq!(heap.stats().grows, grows);
}

#[test]
fn spans_coalesce_in_any_free_order() {
    let mut heap = Heap::new(Arena::new(16 << 20, 0));
    let l = layout(16 * PAGE_SIZE, PAGE_SIZE);
    for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2], [0, 2, 1]] {
        let blocks = [heap.alloc(l), heap.alloc(l), heap.alloc(l)];
        let grows = heap.stats().grows;
        for i in order {
            // SAFETY: from `alloc` with this layout, freed once.
            unsafe { heap.dealloc(blocks[i], l) };
        }
        let big = layout(48 * PAGE_SIZE, PAGE_SIZE);
        let p = heap.alloc(big);
        assert_eq!(p, blocks[0], "the three spans merged");
        assert_eq!(heap.stats().grows, grows);
        // SAFETY: as above.
        unsafe { heap.dealloc(p, big) };
    }
}

#[test]
fn growth_is_bounded_and_works_without_contiguous_backing() {
    let mut heap = Heap::new(Arena::new(32 << 20, PAGE_SIZE));
    let l = layout(GROW_SIZE / 2, 8);
    let live: Vec<_> = (0..8).map(|_| heap.alloc(l)).collect();
    assert!(live.iter().all(|p| !p.is_null()));
    let st = heap.stats();
    assert!(st.grows >= 4);
    assert!(st.backing <= 8 * GROW_SIZE, "{st:?}");
    let huge = layout(4 * GROW_SIZE, 8);
    let p = heap.alloc(huge);
    assert!(!p.is_null(), "one request larger than the growth step");
    stamp(p, huge.size());
    check(p, huge.size());
}

#[test]
fn exhausted_backing_returns_null_and_stays_usable() {
    let mut heap = Heap::new(Arena::new(GROW_SIZE, 0));
    let l = layout(128 * 1024, PAGE_SIZE);
    let mut live = Vec::new();
    loop {
        let p = heap.alloc(l);
        if p.is_null() {
            break;
        }
        live.push(p);
    }
    assert_eq!(live.len(), 8, "1 MiB of 128 KiB blocks");
    assert!(
        heap.alloc(layout(16, 8)).is_null(),
        "no span left for a class"
    );
    assert!(heap.alloc(l).is_null(), "still null");

    let p = live.pop().unwrap();
    // SAFETY: from `alloc` with this layout, freed once.
    unsafe { heap.dealloc(p, l) };
    assert_eq!(heap.alloc(l), p, "usable again after a free");
    // SAFETY: as above.
    unsafe { heap.dealloc(p, l) };
    assert!(!heap.alloc(layout(16, 8)).is_null());
}

#[test]
fn impossible_requests_fail_instead_of_wrapping() {
    let mut heap = Heap::new(Arena::new(GROW_SIZE, 0));
    let size = isize::MAX as usize - (2 << 30);
    assert!(heap.alloc(layout(size, 8)).is_null());
    assert!(heap.alloc(layout(size, 1 << 30)).is_null());
    assert_eq!(heap.stats().live, 0);
    assert!(!heap.alloc(layout(8, 8)).is_null());
}

/// Random allocate and free against a model of the live blocks.
#[test]
fn random_sequences_match_a_model() {
    for seed in 1..=12u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let limit = (1 + rng.below(16) as usize) << 20;
        let gap = rng.below(2) as usize * PAGE_SIZE;
        let mut heap = Heap::new(Arena::new(limit, gap));
        let mut live: BTreeMap<usize, Layout> = BTreeMap::new();
        for _ in 0..5000 {
            if rng.below(5) < 3 {
                let size = match rng.below(10) {
                    0..=6 => 1 + rng.below(MAX_SMALL as u64),
                    7 | 8 => 1 + rng.below(32 * PAGE_SIZE as u64),
                    _ => 1 + rng.below(64 * PAGE_SIZE as u64),
                } as usize;
                let l = layout(size, 1 << rng.below(14));
                let p = heap.alloc(l);
                if p.is_null() {
                    continue;
                }
                let addr = p as usize;
                assert!(addr.is_multiple_of(l.align()), "seed {seed}");
                if let Some((&below, b)) = live.range(..addr).next_back() {
                    assert!(below + b.size() <= addr, "seed {seed}: overlap below");
                }
                if let Some((&above, _)) = live.range(addr..).next() {
                    assert!(addr + size <= above, "seed {seed}: overlap above");
                }
                stamp(p, size);
                live.insert(addr, l);
            } else if !live.is_empty() {
                let i = rng.below(live.len() as u64) as usize;
                let (&addr, &l) = live.iter().nth(i).unwrap();
                live.remove(&addr);
                check(addr as *mut u8, l.size());
                // SAFETY: tracked as live with this layout, now removed.
                unsafe { heap.dealloc(addr as *mut u8, l) };
            }
        }
        for (addr, l) in live {
            check(addr as *mut u8, l.size());
            // SAFETY: as above.
            unsafe { heap.dealloc(addr as *mut u8, l) };
        }
        assert_eq!(heap.stats().live, 0, "seed {seed}");
    }
}

/// The stated workload of ADR-0022: repeated churn of a mix of sizes from 16
/// bytes to 256 KiB, holding up to 8 MiB live. The bound: memory taken from
/// the backing stays within 3 times the peak of live bytes, plus 16 chunks.
#[test]
fn fragmentation_stays_bounded_under_the_stated_workload() {
    const LIVE_TARGET: usize = 8 << 20;
    let mut heap = Heap::new(Arena::new(256 << 20, 0));
    let mut rng = Rng(0xfeed);
    let mut live = Vec::new();
    let mut live_bytes = 0;
    for _ in 0..200_000 {
        if live_bytes < LIVE_TARGET && rng.below(5) < 3 {
            let size = match rng.below(100) {
                0..=79 => 16 << rng.below(7),
                80..=97 => PAGE_SIZE << rng.below(3),
                _ => 1 + rng.below(256 * 1024) as usize,
            };
            let l = layout(size, 8);
            let p = heap.alloc(l);
            assert!(!p.is_null());
            live.push((p, l));
            live_bytes += size;
        } else if !live.is_empty() {
            let (p, l) = live.swap_remove(rng.below(live.len() as u64) as usize);
            live_bytes -= l.size();
            // SAFETY: from `alloc` with this layout, freed once.
            unsafe { heap.dealloc(p, l) };
        }
    }
    let st = heap.stats();
    let bound = 3 * st.peak_live + 16 * CHUNK_SIZE;
    assert!(st.backing <= bound, "{st:?}, bound {bound}");
}

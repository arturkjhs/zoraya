//! ADR-0021's guarantees, against hand-written cases, a model and the region
//! sets of the committed device tree fixtures.

use std::collections::HashSet;

use zoraya_dtb::Fdt;
use zoraya_dtb::platform::PlatformInfo;
use zoraya_pmm::{
    FRAME_SIZE, Frame, FrameAllocator, FrameState, FreeError, InitError, OutOfFrames, Range,
    RangeSet, place_metadata, storage_words,
};

const RAM: u64 = 0x4000_0000;

/// xorshift64*: deterministic, so a failure reproduces.
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

fn storage(memory: &[Range]) -> Vec<u64> {
    vec![!0; storage_words(memory).unwrap()]
}

fn r(start: u64, end: u64) -> Range {
    Range { start, end }
}

fn frame(addr: u64) -> Frame {
    Frame::from_addr(addr).unwrap()
}

#[test]
fn stats_count_ram_and_reservations() {
    let mem = [Range::at(RAM, 128 * FRAME_SIZE)];
    let mut s = storage(&mem);
    let res = [Range::at(RAM + 3 * FRAME_SIZE, 2 * FRAME_SIZE)];
    let a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    let st = a.stats();
    assert_eq!((st.total, st.reserved, st.free), (128, 2, 126));
}

#[test]
fn drain_never_returns_reserved_or_outside_ram_and_exhausts_cleanly() {
    let mem = [
        Range::at(RAM, 300 * FRAME_SIZE),
        Range::at(0x1_0000_0000, 70 * FRAME_SIZE),
    ];
    let res = [
        Range::at(RAM + 0x123, 1),
        Range::at(RAM + 10 * FRAME_SIZE, 64 * FRAME_SIZE),
        Range::at(0x1_0000_0000 - FRAME_SIZE, 2 * FRAME_SIZE),
    ];
    let mut s = storage(&mem);
    let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    let expected = a.stats().free;
    let mut seen = HashSet::new();
    while let Ok(f) = a.alloc() {
        assert!(seen.insert(f), "frame {f:?} returned twice");
        let frame_range = Range::at(f.addr(), FRAME_SIZE);
        assert!(!res.iter().any(|r| r.overlaps(&frame_range)));
        assert!(mem.iter().any(|m| m.start <= f.addr() && f.addr() < m.end));
    }
    assert_eq!(seen.len() as u64, expected);
    assert_eq!(a.stats().free, 0);
    assert_eq!(a.alloc(), Err(OutOfFrames));
    assert_eq!(a.alloc_contig(1, 1), Err(OutOfFrames));

    // Exhaustion leaves the allocator usable.
    let back = *seen.iter().next().unwrap();
    a.free(back).unwrap();
    assert_eq!(a.alloc(), Ok(back));
}

#[test]
fn bad_frees_are_detected_and_change_nothing() {
    let mem = [Range::at(RAM, 16 * FRAME_SIZE)];
    let res = [Range::at(RAM, FRAME_SIZE)];
    let mut s = storage(&mem);
    let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    let f = a.alloc().unwrap();
    let st = a.stats();

    let free_frame = frame(RAM + 5 * FRAME_SIZE);
    assert_eq!(a.free(free_frame), Err(FreeError::NotAllocated));
    assert_eq!(a.free(frame(RAM)), Err(FreeError::Reserved));
    assert_eq!(a.free(frame(0x1000)), Err(FreeError::OutOfRange));
    assert_eq!(
        a.free(frame(RAM + 16 * FRAME_SIZE)),
        Err(FreeError::OutOfRange)
    );
    assert_eq!(a.stats(), st);

    a.free(f).unwrap();
    assert_eq!(a.free(f), Err(FreeError::NotAllocated), "double free");
    assert_eq!(a.state(f), FrameState::Free);
}

#[test]
fn free_contig_is_all_or_nothing() {
    let mem = [Range::at(RAM, 32 * FRAME_SIZE)];
    let mut s = storage(&mem);
    let mut a = FrameAllocator::new(&mem, &[], &mut s).unwrap();
    let f = a.alloc_contig(4, 1).unwrap();
    a.free(frame(f.addr() + FRAME_SIZE)).unwrap();
    assert_eq!(a.free_contig(f, 4), Err(FreeError::NotAllocated));
    assert_eq!(a.state(f), FrameState::Allocated, "first frame kept");
    assert_eq!(
        a.state(frame(f.addr() + 3 * FRAME_SIZE)),
        FrameState::Allocated
    );
    assert_eq!(a.free_contig(f, u64::MAX), Err(FreeError::OutOfRange));
}

#[test]
fn contig_respects_alignment_reservations_and_holes() {
    let mem = [Range::at(RAM, 64 * FRAME_SIZE)];
    let res = [Range::at(RAM + 16 * FRAME_SIZE, FRAME_SIZE)];
    let mut s = storage(&mem);
    let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    let f = a.alloc_contig(20, 8).unwrap();
    assert_eq!(f.addr() % (8 * FRAME_SIZE), 0);
    assert!(
        f.addr() > RAM + 16 * FRAME_SIZE,
        "run skips the reservation"
    );
    let g = a.alloc_contig(16, 16).unwrap();
    assert_eq!(g.addr(), RAM, "the run below the reservation");
    assert_eq!(a.alloc_contig(1000, 1), Err(OutOfFrames));
    assert_eq!(a.alloc_contig(0, 1), Err(OutOfFrames));
    assert_eq!(a.alloc_contig(1, 3), Err(OutOfFrames));
}

#[test]
fn frame_zero_is_never_returned() {
    let mem = [Range::at(0, 8 * FRAME_SIZE)];
    let mut s = storage(&mem);
    let mut a = FrameAllocator::new(&mem, &[], &mut s).unwrap();
    assert_eq!(a.stats().reserved, 1);
    while let Ok(f) = a.alloc() {
        assert_ne!(f.addr(), 0);
    }
}

#[test]
fn partial_pages_round_the_safe_way() {
    // RAM rounds in; reservations round out.
    let mem = [r(RAM + 1, RAM + 4 * FRAME_SIZE - 1)];
    let res = [r(RAM + FRAME_SIZE + 1, RAM + FRAME_SIZE + 2)];
    let mut s = storage(&mem);
    let a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    assert_eq!(a.stats().total, 2);
    assert_eq!(a.stats().free, 1);
    assert_eq!(a.state(frame(RAM)), FrameState::Outside);
    assert_eq!(a.state(frame(RAM + FRAME_SIZE)), FrameState::Reserved);
    assert_eq!(a.state(frame(RAM + 2 * FRAME_SIZE)), FrameState::Free);
    assert_eq!(a.state(frame(RAM + 3 * FRAME_SIZE)), FrameState::Outside);
}

#[test]
fn reservation_at_the_top_of_the_address_space_is_harmless() {
    let top = u64::MAX - FRAME_SIZE + 1;
    let mem = [r(top - 8 * FRAME_SIZE, top)];
    let res = [r(top, u64::MAX), Range::at(u64::MAX, 100)];
    let mut s = storage(&mem);
    let a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    assert_eq!(a.stats().free, 8);
}

#[test]
fn init_errors() {
    let mem = [Range::at(RAM, 128 * FRAME_SIZE)];
    let mut small = vec![0; storage_words(&mem).unwrap() - 1];
    assert_eq!(
        FrameAllocator::new(&mem, &[], &mut small).unwrap_err(),
        InitError::StorageTooSmall
    );
    assert_eq!(
        storage_words(&[r(RAM + 1, RAM + 2)]),
        Err(InitError::NoMemory)
    );
    let many: Vec<Range> = (0..20)
        .map(|i| Range::at(i * 2 * FRAME_SIZE, FRAME_SIZE))
        .collect();
    assert_eq!(storage_words(&many), Err(InitError::TooManyRegions));
}

#[test]
fn overlapping_memory_ranges_are_merged() {
    let mem = [
        Range::at(RAM, 8 * FRAME_SIZE),
        Range::at(RAM + 4 * FRAME_SIZE, 8 * FRAME_SIZE),
    ];
    let mut s = storage(&mem);
    let a = FrameAllocator::new(&mem, &[], &mut s).unwrap();
    assert_eq!(a.stats().total, 12);
    assert_eq!(a.regions().count(), 1);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Model {
    Free,
    Allocated,
    Reserved,
}

/// Random operations against a plain array, for several memory shapes.
#[test]
fn random_sequences_match_a_model() {
    for seed in 1..=40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let regions = 1 + rng.below(4);
        let mut mem = Vec::new();
        let mut at = RAM;
        for _ in 0..regions {
            let frames = 1 + rng.below(300);
            mem.push(Range::at(at, frames * FRAME_SIZE));
            at += (frames + 1 + rng.below(16)) * FRAME_SIZE;
        }
        let top = at;
        let mut res = Vec::new();
        for _ in 0..rng.below(8) {
            let start = RAM + rng.below(top - RAM);
            res.push(Range::at(start, 1 + rng.below(40 * FRAME_SIZE)));
        }

        let index = |addr: u64| ((addr - RAM) / FRAME_SIZE) as usize;
        let mut model = vec![None; ((top - RAM) / FRAME_SIZE) as usize];
        for m in &mem {
            for a in (m.start..m.end).step_by(FRAME_SIZE as usize) {
                model[index(a)] = Some(Model::Free);
            }
        }
        for x in &res {
            let around = x.frames_around();
            for a in (around.start..around.end.min(top)).step_by(FRAME_SIZE as usize) {
                if model[index(a)].is_some() {
                    model[index(a)] = Some(Model::Reserved);
                }
            }
        }

        let mut s = storage(&mem);
        let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
        let mut live: Vec<(Frame, u64)> = Vec::new();
        for _ in 0..3000 {
            match rng.below(10) {
                0..=4 => match a.alloc() {
                    Ok(f) => {
                        assert_eq!(model[index(f.addr())], Some(Model::Free), "seed {seed}");
                        model[index(f.addr())] = Some(Model::Allocated);
                        live.push((f, 1));
                    }
                    Err(OutOfFrames) => assert!(!model.contains(&Some(Model::Free))),
                },
                5 => {
                    let count = 1 + rng.below(9);
                    if let Ok(f) = a.alloc_contig(count, 1 << rng.below(4)) {
                        for i in 0..count {
                            let m = &mut model[index(f.addr()) + i as usize];
                            assert_eq!(*m, Some(Model::Free), "seed {seed}");
                            *m = Some(Model::Allocated);
                        }
                        live.push((f, count));
                    }
                }
                6..=8 if !live.is_empty() => {
                    let (f, count) = live.swap_remove(rng.below(live.len() as u64) as usize);
                    a.free_contig(f, count).unwrap();
                    for i in 0..count {
                        model[index(f.addr()) + i as usize] = Some(Model::Free);
                    }
                }
                _ => {
                    // A random free: only an allocated single frame may succeed.
                    let addr = RAM + rng.below(top - RAM) / FRAME_SIZE * FRAME_SIZE;
                    let f = frame(addr);
                    let got = a.free(f);
                    match model[index(addr)] {
                        None => assert_eq!(got, Err(FreeError::OutOfRange)),
                        Some(Model::Free) => assert_eq!(got, Err(FreeError::NotAllocated)),
                        Some(Model::Reserved) => assert_eq!(got, Err(FreeError::Reserved)),
                        Some(Model::Allocated) => {
                            got.unwrap();
                            model[index(addr)] = Some(Model::Free);
                            // Split the live run this frame came from.
                            let i = live
                                .iter()
                                .position(|(g, c)| {
                                    g.addr() <= addr && addr < g.addr() + c * FRAME_SIZE
                                })
                                .unwrap();
                            let (g, c) = live.swap_remove(i);
                            let before = (addr - g.addr()) / FRAME_SIZE;
                            if before > 0 {
                                live.push((g, before));
                            }
                            if c - before > 1 {
                                live.push((frame(addr + FRAME_SIZE), c - before - 1));
                            }
                        }
                    }
                }
            }
            let free = model.iter().filter(|m| **m == Some(Model::Free)).count();
            assert_eq!(a.stats().free, free as u64, "seed {seed}");
        }
    }
}

#[test]
fn works_with_the_regions_of_the_device_tree_fixtures() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../zoraya-dtb/tests/fixtures");
    let names = [
        "virt-default",
        "virt-el2",
        "virt-gicv3",
        "virt-hvf-default",
        "virt-smp4-2g",
    ];
    for name in names {
        let blob = std::fs::read(format!("{dir}/{name}.dtb")).unwrap();
        let fdt = Fdt::new(&blob).unwrap();
        let info = PlatformInfo::from_fdt(&fdt).unwrap();
        let mem: Vec<Range> = info
            .memory
            .as_slice()
            .iter()
            .map(|r| Range::at(r.address, r.size))
            .collect();
        // The image and blob as the kernel would pass them, as the banner
        // shows them.
        let mut res: Vec<Range> = info
            .reserved
            .as_slice()
            .iter()
            .map(|r| Range::at(r.address, r.size))
            .collect();
        res.push(Range::at(0x4000_0000, blob.len() as u64));
        res.push(Range::at(0x4020_0000, 0x40_0000));

        let words = storage_words(&mem).unwrap();
        let meta = place_metadata(&mem, &res, words).expect("room for metadata");
        assert!(!res.iter().any(|r| r.overlaps(&meta)), "{name}");
        res.push(meta);

        let mut s = vec![0; words];
        let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
        let st = a.stats();
        assert_eq!(
            st.total * FRAME_SIZE,
            mem.iter().map(Range::len).sum::<u64>()
        );
        assert!(st.free > 0, "{name}");
        let mut n = 0;
        while let Ok(f) = a.alloc() {
            let frame_range = Range::at(f.addr(), FRAME_SIZE);
            assert!(!res.iter().any(|r| r.overlaps(&frame_range)), "{name}");
            n += 1;
        }
        assert_eq!(n, st.free, "{name}");
    }
}

#[test]
fn place_metadata_uses_the_largest_free_range() {
    let mem = [
        Range::at(RAM, 16 * FRAME_SIZE),
        Range::at(0x8000_0000, 64 * FRAME_SIZE),
    ];
    let res = [Range::at(0x8000_0000, FRAME_SIZE)];
    let got = place_metadata(&mem, &res, 1000).unwrap();
    assert_eq!(got, Range::at(0x8000_0000 + FRAME_SIZE, 2 * FRAME_SIZE));
    assert_eq!(place_metadata(&mem, &res, 1 << 20), None);
}

#[test]
fn range_set_add_and_remove_match_a_model() {
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let mut set = RangeSet::<64>::new();
        let mut model = [false; 256];
        for _ in 0..40 {
            let start = rng.below(256);
            let end = (start + rng.below(24)).min(256);
            let add = rng.below(2) == 0;
            if add {
                set.add(r(start, end)).unwrap();
            } else {
                set.remove(r(start, end)).unwrap();
            }
            model[start as usize..end as usize].fill(add);
            let slice = set.as_slice();
            for w in slice.windows(2) {
                assert!(w[0].end < w[1].start, "sorted, disjoint, not touching");
            }
            for (i, want) in model.iter().enumerate() {
                let got = slice
                    .iter()
                    .any(|x| x.start <= i as u64 && (i as u64) < x.end);
                assert_eq!(got, *want, "seed {seed} byte {i}");
            }
        }
    }
}

#[test]
fn range_set_reports_full_and_stays_unchanged() {
    let mut set = RangeSet::<2>::new();
    set.add(r(0, 10)).unwrap();
    set.add(r(20, 30)).unwrap();
    let before = set;
    assert!(set.add(r(40, 50)).is_err());
    assert_eq!(set, before);
    assert!(set.remove(r(2, 3)).is_err());
    assert_eq!(set, before);
}

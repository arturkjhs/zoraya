//! The frame allocator at the size of the browser's guest (ADR-0415 item 5:
//! 2 GiB), and at the sizes around it: the counts, a full drain, the most a
//! single allocation reads (which the kernel does with the lock held and
//! interrupts masked, INV-0270-1) and the steps of a run search. The
//! allocator's own tests (`allocator.rs`) use a few hundred frames.

use zoraya_pmm::{FRAME_SIZE, Frame, FrameAllocator, FrameState, FreeError, Range, RunSearch};
use zoraya_pmm::{place_metadata, storage_words};

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
/// Where the device tree of QEMU's `virt` puts RAM.
const RAM: u64 = 0x4000_0000;
/// The kernel's step of a run search, in words of the frame map
/// (`kernel/src/memory/frames.rs`, `SCAN_WORDS`).
const SCAN_WORDS: u64 = 48;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
zoraya_test_alloc::cap_memory!();

fn storage(memory: &[Range]) -> Vec<u64> {
    vec![!0; storage_words(memory).unwrap()]
}

/// What the kernel reserves in a guest of `memory`: the image at the start
/// of RAM, the device tree blob after it and the frame map, placed where
/// `place_metadata` puts it.
fn reservations(memory: &[Range]) -> Vec<Range> {
    let mut reserved = vec![
        Range::at(memory[0].start, 2 * MIB),
        Range::at(memory[0].start + 2 * MIB, 64 * 1024),
    ];
    let words = storage_words(memory).unwrap();
    reserved.push(place_metadata(memory, &reserved, words).unwrap());
    reserved
}

fn number(frame: Frame) -> u64 {
    frame.addr() / FRAME_SIZE
}

/// Takes every free frame and says which were taken, as a bitmap over the
/// frame numbers from `base`.
fn drain(a: &mut FrameAllocator<'_>, base: u64, frames: u64) -> (Vec<u64>, u64) {
    let mut seen = vec![0u64; frames.div_ceil(64) as usize];
    let mut count = 0;
    while let Ok(frame) = a.alloc() {
        let index = number(frame) - base;
        assert!(index < frames, "frame {:#x} is outside RAM", frame.addr());
        let (word, mask) = ((index / 64) as usize, 1 << (index % 64));
        assert_eq!(seen[word] & mask, 0, "frame {:#x} twice", frame.addr());
        seen[word] |= mask;
        count += 1;
    }
    (seen, count)
}

#[test]
fn two_gib_counts_and_a_full_drain() {
    let mem = [Range::at(RAM, 2 * GIB)];
    let res = reservations(&mem);
    let mut s = storage(&mem);
    // 2 GiB is 524288 frames, 8192 words of each of the two maps.
    assert_eq!(s.len(), 2 * 8192);
    let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    let st = a.stats();
    let meta = res[2].len() / FRAME_SIZE;
    // The image, the blob, the map, and frame 0 (not in this RAM: no count).
    assert_eq!(st.total, 524_288);
    assert_eq!(st.reserved, 512 + 16 + meta);
    assert_eq!(st.free, st.total - st.reserved);
    assert_eq!(a.alloc_scan_bound(), 8192);

    let (seen, count) = drain(&mut a, RAM / FRAME_SIZE, st.total);
    assert_eq!(count, st.free);
    assert_eq!(a.stats().free, 0);
    assert_eq!(a.alloc(), Err(zoraya_pmm::OutOfFrames));
    // Reserved frames were not handed out; everything else was.
    for r in &res {
        for addr in (r.start..r.end).step_by(FRAME_SIZE as usize) {
            let index = addr / FRAME_SIZE - RAM / FRAME_SIZE;
            assert_eq!(seen[(index / 64) as usize] >> (index % 64) & 1, 0);
            assert_eq!(
                a.state(Frame::from_addr(addr).unwrap()),
                FrameState::Reserved
            );
        }
    }
    let taken: u64 = seen.iter().map(|w| u64::from(w.count_ones())).sum();
    assert_eq!(taken, st.free);

    // The first and the last frame of the 2 GiB, and the one past it.
    let last = Frame::from_addr(RAM + 2 * GIB - FRAME_SIZE).unwrap();
    assert_eq!(a.state(last), FrameState::Allocated);
    assert_eq!(a.free(last), Ok(()));
    assert_eq!(a.free(last), Err(FreeError::NotAllocated));
    let past = Frame::from_addr(RAM + 2 * GIB).unwrap();
    assert_eq!(a.state(past), FrameState::Outside);
    assert_eq!(a.free(past), Err(FreeError::OutOfRange));
    assert_eq!(a.alloc(), Ok(last));
}

/// The same for the sizes of the tests' machines and beyond the browser's:
/// 128 MiB to 4 GiB, in one region, and in two apart (the second above
/// 4 GiB, where a 32-bit count of bytes would have wrapped).
#[test]
fn the_sizes_around_two_gib_agree_with_their_arithmetic() {
    let high = 0x80_0000_0000;
    let layouts: Vec<Vec<Range>> = vec![
        vec![Range::at(RAM, 128 * MIB)],
        vec![Range::at(RAM, 512 * MIB)],
        vec![Range::at(RAM, GIB)],
        vec![Range::at(RAM, 2 * GIB - 3 * FRAME_SIZE)],
        vec![Range::at(RAM, 4 * GIB)],
        vec![Range::at(RAM, GIB), Range::at(high, GIB)],
        vec![Range::at(RAM, 2 * GIB), Range::at(high, 2 * GIB)],
    ];
    for mem in layouts {
        let res = reservations(&mem);
        let mut s = storage(&mem);
        let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
        let total: u64 = mem.iter().map(|r| r.len() / FRAME_SIZE).sum();
        assert_eq!(a.stats().total, total);
        let worst = mem
            .iter()
            .map(|r| (r.len() / FRAME_SIZE).div_ceil(64))
            .sum::<u64>();
        assert_eq!(a.alloc_scan_bound(), worst);
        let free = a.stats().free;
        let mut count = 0;
        let mut last = 0;
        while let Ok(f) = a.alloc() {
            // Next-fit from an empty map goes up: any repeat or a frame
            // out of order is a bug in the map's arithmetic.
            assert!(f.addr() > last, "{:#x} after {:#x}", f.addr(), last);
            assert!(mem.iter().any(|r| r.start <= f.addr() && f.addr() < r.end));
            last = f.addr();
            count += 1;
        }
        assert_eq!(count, free);
    }
}

/// The most one `alloc` reads, with the map as full as it can be and the one
/// free frame as far behind the search as it can be: every word of the map
/// once. Measured, not assumed, so that the
/// bound the kernel relies on (ADR-0270) is the real worst case, and not
/// larger than the 2 GiB map.
#[test]
fn the_worst_single_allocation_reads_the_whole_map_and_no_more() {
    for (mem, expected, behind) in [
        (
            vec![Range::at(RAM, 2 * GIB)],
            8192,
            RAM + 2 * GIB - 512 * 1024,
        ),
        (
            vec![Range::at(RAM, GIB), Range::at(0x1_0000_0000, GIB)],
            8192,
            0x1_0000_0000 + GIB - 512 * 1024,
        ),
    ] {
        let res = reservations(&mem);
        let mut s = storage(&mem);
        let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
        let base = RAM / FRAME_SIZE;
        let (_, taken) = drain(&mut a, base, 1 << 21);
        assert!(taken > 500_000);
        // The search stands in the last word of the last region. Free a frame
        // in the word before it (512 KiB below the top): everything else is
        // read first, going round through the first region.
        let behind = Frame::from_addr(behind).unwrap();
        a.free(behind).unwrap();
        let (got, read) = a.alloc_counted();
        assert_eq!(got, Ok(behind));
        assert_eq!(read, expected, "words read by the worst allocation");
        assert!(read <= a.alloc_scan_bound());
        // Nothing is free now: no read at all.
        assert_eq!(a.alloc_counted(), (Err(zoraya_pmm::OutOfFrames), 0));
    }
}

/// Sequential allocation reads about one word per frame (next-fit): the
/// common case does not scan the map from its start.
#[test]
fn next_fit_reads_about_one_word_per_allocation() {
    let mem = [Range::at(RAM, 2 * GIB)];
    let res = reservations(&mem);
    let mut s = storage(&mem);
    let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    let mut words = 0;
    for _ in 0..64 * 1024 {
        words += a.alloc_counted().1;
    }
    // Each allocation reads the word it takes from, and the one after a
    // word is filled reads that full word again before the next (one more
    // read in 64); the image's words are read once.
    assert!(
        words <= 65_536 + 1024 + 16,
        "{words} words for 65536 frames"
    );
    // Nothing read is far from the search: a free in the middle does not
    // make the next allocation walk the map.
    let early = Frame::from_addr(RAM + 100 * MIB).unwrap();
    a.free(early).unwrap();
    let (got, read) = a.alloc_counted();
    assert!(got.is_ok());
    assert_eq!(read, 1, "next-fit goes on after the last frame");
}

/// A run of 1024 frames (the kernel's largest, 4 MiB) in a map of 2 GiB, in
/// the steps the kernel makes: each step reads at most its budget and one
/// candidate run, the search ends, and a run found is all free.
#[test]
fn a_run_search_at_two_gib_is_made_in_bounded_steps() {
    let mem = [Range::at(RAM, 2 * GIB)];
    let res = reservations(&mem);
    let mut s = storage(&mem);
    let mut a = FrameAllocator::new(&mem, &res, &mut s).unwrap();
    // Take everything, then free a run low in the map.
    let (_, _) = drain(&mut a, RAM / FRAME_SIZE, 1 << 19);
    // Every other frame of the top gigabyte is free again: a map full of
    // free frames and with no run of two.
    for addr in (RAM + GIB..RAM + 2 * GIB).step_by(2 * FRAME_SIZE as usize) {
        a.free(Frame::from_addr(addr).unwrap()).unwrap();
    }
    let run_at = RAM + 64 * MIB;
    for i in 0..1024 {
        a.free(Frame::from_addr(run_at + i * FRAME_SIZE).unwrap())
            .unwrap();
    }
    let mut search = RunSearch::default();
    let (mut steps, mut most) = (0, 0);
    let found = loop {
        steps += 1;
        let got = a
            .alloc_contig_step(1024, 1, &mut search, SCAN_WORDS)
            .unwrap();
        most = most.max(search.words_read());
        if let Some(first) = got {
            break first;
        }
        assert!(steps < 10_000, "the search does not end");
    };
    assert_eq!(found.addr(), run_at);
    // The run is at 64 MiB of RAM: 256 words of map past the image, so the
    // the image, so the search takes more than one step but few.
    assert!(steps >= 4, "{steps} steps");
    // A step reads its budget and, at most, the candidate run's 1024 / 64
    // words and two more (`alloc_contig_step`'s documentation).
    assert!(most <= SCAN_WORDS + 1024 / 64 + 2, "{most} words in a step");
    // The whole run was taken, and none of it is free now.
    for i in 0..1024 {
        let f = Frame::from_addr(run_at + i * FRAME_SIZE).unwrap();
        assert_eq!(a.state(f), FrameState::Allocated);
    }
    // No run at all: the search ends with `OutOfFrames` in steps that stay
    // within their budget, over the whole 2 GiB map.
    let mut search = RunSearch::default();
    let mut steps = 0u64;
    loop {
        steps += 1;
        match a.alloc_contig_step(1024, 1, &mut search, SCAN_WORDS) {
            Ok(None) => assert!(search.words_read() <= SCAN_WORDS + 1024 / 64 + 2),
            Ok(Some(_)) => panic!("a run in a map that has none"),
            Err(_) => break,
        }
    }
    assert!(
        steps * (SCAN_WORDS + 2) >= 8192,
        "{steps} steps for 8192 words"
    );
}

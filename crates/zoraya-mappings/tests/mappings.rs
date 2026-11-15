//! The list against a plain sorted vector, with the hand-written cases at the
//! limits that matter: 4096 records, a gap at each end, a full chunk.

use zoraya_mappings::{CHUNK, Mappings};

const PAGE: u64 = 4096;
const LOW: u64 = 1 << 46;
const HIGH: u64 = 3 << 45;

/// The kernel's limit (ADR-0415 item 1): the tests go to it and past it.
const LIMIT: usize = 4096;

/// The model: sorted `(va, length)` pairs, with the first gap found by a
/// plain scan.
struct Model(Vec<(u64, u64)>);

impl Model {
    fn gap(&self, length: u64) -> Option<u64> {
        let mut at = LOW;
        for &(va, len) in &self.0 {
            if at.checked_add(length)? < va {
                return Some(at);
            }
            at = va + len + PAGE;
        }
        (at.checked_add(length)? <= HIGH).then_some(at)
    }
}

/// Splitmix64, so that a failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

fn add(list: &mut Mappings<u32>, model: &mut Model, length: u64, tag: u32) -> Option<u64> {
    let (va, _) = list.find_gap(length, LOW, HIGH, PAGE);
    assert_eq!(va, model.gap(length), "gap for {length}");
    let va = va?;
    list.reserve_for(va).expect("room");
    list.insert(va, length, tag).expect("insert after reserve");
    let at = model.0.partition_point(|&(v, _)| v < va);
    model.0.insert(at, (va, length));
    Some(va)
}

fn same(list: &Mappings<u32>, model: &Model) {
    list.check();
    assert_eq!(list.len(), model.0.len());
    let got: Vec<(u64, u64)> = list.iter().map(|e| (e.va, e.length)).collect();
    assert_eq!(got, model.0);
}

#[test]
fn fills_to_the_limit_and_empties() {
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for n in 0..LIMIT {
        add(&mut list, &mut model, PAGE * (1 + n as u64 % 3), n as u32).expect("a gap");
    }
    same(&list, &model);
    // Added at the top, the chunks are full: the charge is the records and
    // at most one chunk's room.
    let entry = core::mem::size_of::<zoraya_mappings::Entry<u32>>() as u64;
    let bound = (LIMIT + CHUNK) as u64 * entry + (LIMIT / CHUNK + 1) as u64 * 24;
    assert!(
        list.bytes_held() <= bound,
        "{} > {bound}",
        list.bytes_held()
    );
    // Pop from the top until empty: every chunk goes back but one.
    let mut last = u64::MAX;
    while let Some(e) = list.pop_last() {
        assert!(e.va < last);
        last = e.va;
        model.0.pop();
    }
    same(&list, &model);
    assert!(list.is_empty());
    // Every chunk went back, the last of them too.
    assert_eq!(list.bytes_held(), 0);
}

#[test]
fn random_adds_and_removals_match_the_model() {
    let mut rng = Rng(7);
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for round in 0..20_000u32 {
        let r = rng.next();
        if r % 5 < 3 && model.0.len() < LIMIT {
            let length = PAGE * (1 + (r >> 8) % 4);
            add(&mut list, &mut model, length, round);
        } else if !model.0.is_empty() {
            let i = (r >> 8) as usize % model.0.len();
            let (va, length) = model.0.remove(i);
            // A wrong length is no mapping: nothing is removed.
            assert!(list.remove_exact(va, length + PAGE).is_none());
            assert!(!list.contains_exact(va, length + PAGE));
            assert!(list.contains_exact(va, length));
            let e = list.remove_exact(va, length).expect("exact");
            assert_eq!((e.va, e.length), (va, length));
            assert!(list.remove_exact(va, length).is_none());
        }
        if round % 997 == 0 {
            same(&list, &model);
        }
    }
    same(&list, &model);
}

#[test]
fn a_full_chunk_splits_in_two_and_keeps_the_order() {
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for n in 0..=CHUNK {
        add(&mut list, &mut model, PAGE, n as u32).expect("a gap");
    }
    same(&list, &model);
    // Insert in the middle of the lower half after the split.
    let (va, len) = model.0[3];
    assert!(list.remove_exact(va, len).is_some());
    model.0.remove(3);
    add(&mut list, &mut model, PAGE, 999).expect("the freed gap");
    same(&list, &model);
}

#[test]
fn insert_without_room_gives_the_record_back() {
    let mut list: Mappings<u32> = Mappings::new();
    let e = list
        .insert(LOW, PAGE, 5)
        .expect_err("an empty list has no chunk");
    assert_eq!((e.va, e.length, e.value), (LOW, PAGE, 5));
    list.reserve_for(LOW).expect("room");
    list.insert(LOW, PAGE, 5).expect("room");
    // A full chunk takes none without `reserve_for`.
    let mut va = LOW + 2 * PAGE;
    for n in 1..CHUNK as u32 {
        list.reserve_for(va).expect("room");
        list.insert(va, PAGE, n).expect("room");
        va += 2 * PAGE;
    }
    assert_eq!(list.len(), CHUNK);
    assert!(list.insert(va, PAGE, 0).is_err(), "the chunk is full");
    list.reserve_for(va).expect("split");
    list.insert(va, PAGE, 0).expect("room after the split");
    list.check();
}

#[test]
fn find_gap_reports_what_it_visited() {
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for n in 0..300u32 {
        add(&mut list, &mut model, PAGE, n);
    }
    // No gap before the end: every record is visited.
    let (va, visited) = list.find_gap(PAGE, LOW, HIGH, PAGE);
    assert!(va.is_some());
    assert_eq!(visited, 300);
    // A gap at the front is found at the first record.
    let (va, len) = model.0.remove(0);
    list.remove_exact(va, len).unwrap();
    let (found, visited) = list.find_gap(PAGE, LOW, HIGH, PAGE);
    assert_eq!(found, Some(LOW));
    assert_eq!(visited, 1);
    // No room at all: nothing fits above the top; an overflow is no gap.
    assert_eq!(list.find_gap(HIGH, LOW, HIGH, PAGE).0, None);
    assert_eq!(list.find_gap(u64::MAX, LOW, HIGH, PAGE).0, None);
}

#[test]
fn a_record_added_and_removed_leaves_the_charge_as_it_was() {
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for n in 0..=(4 * CHUNK) as u32 {
        let before = list.bytes_held();
        let va = add(&mut list, &mut model, PAGE, n).expect("a gap");
        let (_, len) = model.0.pop().expect("one");
        let removed = list.remove_exact(va, len).expect("exact");
        assert_eq!(removed.value, n);
        assert_eq!(list.bytes_held(), before, "after {n} records");
    }
}

#[test]
fn room_made_for_a_record_that_does_not_come_is_given_back() {
    let mut list: Mappings<u32> = Mappings::new();
    list.reserve_for(LOW).unwrap();
    assert!(list.bytes_held() > 0);
    list.trim();
    assert_eq!(list.bytes_held(), 0);
    // At the top of a full chunk too: the new chunk goes, the full one stays.
    let mut model = Model(Vec::new());
    for n in 0..CHUNK as u32 {
        add(&mut list, &mut model, PAGE, n).unwrap();
    }
    let held = list.bytes_held();
    let va = list.find_gap(PAGE, LOW, HIGH, PAGE).0.unwrap();
    list.reserve_for(va).unwrap();
    assert!(list.bytes_held() > held);
    list.trim();
    assert_eq!(list.bytes_held(), held);
    same(&list, &model);
}

#[test]
fn the_chunk_is_charged_at_its_full_size_and_lookups_agree_with_the_model() {
    let mut list: Mappings<u32> = Mappings::new();
    assert!(list.is_empty());
    assert_eq!(
        Mappings::<u32>::CHUNK_BYTES,
        (CHUNK * core::mem::size_of::<zoraya_mappings::Entry<u32>>() + 24) as u64
    );
    assert_eq!(list.growth_for(LOW), Mappings::<u32>::CHUNK_BYTES);
    let mut model = Model(Vec::new());
    for n in 0..200u32 {
        add(&mut list, &mut model, PAGE * (1 + u64::from(n % 3)), n).expect("a gap");
    }
    assert!(!list.is_empty());
    for &(va, length) in &model.0 {
        let e = list.get_exact(va, length).expect("exact");
        assert_eq!((e.va, e.length), (va, length));
        assert!(list.get_exact(va, length + PAGE).is_none());
        assert!(list.get_exact(va + PAGE, length).is_none());
    }
    // `iter_from` starts at the first record at or above the address, from
    // anywhere in the list, between records and past the end too.
    let top = model.0.last().expect("records").0 + 10 * PAGE;
    for from in [
        0,
        LOW,
        model.0[0].0 + 1,
        model.0[99].0,
        model.0[99].0 + 1,
        model.0[199].0,
        top,
    ] {
        let got: Vec<u64> = list.iter_from(from).map(|e| e.va).collect();
        let want: Vec<u64> = model.0.iter().map(|m| m.0).filter(|&v| v >= from).collect();
        assert_eq!(got, want, "from {from:#x}");
    }
}

/// The kernel's own pattern (lowest place first, a guard page): fill the list
/// with big mappings, unmap all but every second, fill the holes with half
/// the size, again and again. Before chunks merged it reached 485 chunks for
/// 4096 records.
#[test]
fn unmapping_and_mapping_again_keeps_the_chunks_few() {
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    let mut size = 8u64;
    while model.0.len() < LIMIT {
        add(&mut list, &mut model, PAGE * size, 0).expect("a gap");
    }
    for round in 0..12 {
        // Unmap all but every second record, in address order.
        let keep: Vec<(u64, u64)> = model.0.iter().copied().step_by(2).collect();
        for &(va, length) in model.0.clone().iter().filter(|r| !keep.contains(r)) {
            list.remove_exact(va, length).expect("exact");
        }
        model.0 = keep;
        same(&list, &model);
        size = (size / 2).max(1);
        while model.0.len() < LIMIT {
            if add(&mut list, &mut model, PAGE * size, round).is_none() {
                break;
            }
        }
        same(&list, &model);
        let bound = 2 * list.len() / CHUNK + 2;
        assert!(
            list.chunk_count() <= bound,
            "round {round}: {} chunks for {}",
            list.chunk_count(),
            list.len()
        );
        assert!(list.bytes_held() <= bound as u64 * Mappings::<u32>::CHUNK_BYTES);
    }
}

/// Two full chunks, then room in one of them: a record for the other one goes
/// to its neighbour, no chunk is made, and the order holds.
#[test]
fn a_full_chunk_gives_a_record_to_a_neighbour_with_room() {
    let fill = |list: &mut Mappings<u32>, model: &mut Model| {
        // 2 full chunks, with a gap after each record for a record between.
        for n in 0..(2 * CHUNK) as u32 {
            add(list, model, PAGE, n).expect("a gap");
        }
        assert_eq!(list.chunk_count(), 2);
    };
    // Room in the second: a record inside the first spills its last record.
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    fill(&mut list, &mut model);
    for _ in 0..3 {
        let (va, length) = model.0.remove(2 * CHUNK - 1 - 3);
        list.remove_exact(va, length).expect("exact");
    }
    let held = list.bytes_held();
    // A place inside the first chunk: free one record there and take its place.
    let (hole, length) = model.0.remove(5);
    list.remove_exact(hole, length).expect("exact");
    assert_eq!(list.growth_for(hole), 0);
    list.reserve_for(hole).expect("room");
    list.insert(hole, length, 99).expect("room");
    model.0.insert(5, (hole, length));
    assert_eq!(list.chunk_count(), 2);
    assert_eq!(list.bytes_held(), held);
    same(&list, &model);
    // Room in the first: a record inside the second spills its first one.
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    fill(&mut list, &mut model);
    for _ in 0..3 {
        let (va, length) = model.0.remove(7);
        list.remove_exact(va, length).expect("exact");
    }
    let held = list.bytes_held();
    let (hole, length) = model.0.remove(CHUNK);
    list.remove_exact(hole, length).expect("exact");
    assert_eq!(list.growth_for(hole), 0);
    list.reserve_for(hole).expect("room");
    list.insert(hole, length, 99).expect("room");
    model.0.insert(CHUNK, (hole, length));
    assert_eq!(list.chunk_count(), 2);
    assert_eq!(list.bytes_held(), held);
    same(&list, &model);
}

/// Two small neighbours become one, and a chunk's last record leaving merges
/// it with the one before or after.
#[test]
fn small_neighbours_merge_and_the_charge_follows() {
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for n in 0..(3 * CHUNK) as u32 {
        add(&mut list, &mut model, PAGE, n).expect("a gap");
    }
    assert_eq!(list.chunk_count(), 3);
    // Empty the middle chunk but 10 records, and the last but 10: the three
    // chunks (32, 10, 10) become two (32, 20).
    for _ in 0..22 {
        let (va, length) = model.0.remove(CHUNK);
        list.remove_exact(va, length).expect("exact");
    }
    same(&list, &model);
    assert_eq!(list.chunk_count(), 3, "32 + 10 does not fit one chunk");
    for _ in 0..12 {
        let (va, length) = model.0.pop().expect("one");
        list.remove_exact(va, length).expect("exact");
    }
    same(&list, &model);
    assert_eq!(list.chunk_count(), 2, "10 and 10 are one");
    assert_eq!(list.bytes_held(), 2 * Mappings::<u32>::CHUNK_BYTES);
}

/// Chunks of 32, 32 and 1 records, then two records leave the middle one: it
/// holds 30 and the next 1, which are less than a chunk and become one (the
/// merge with the chunk after); and the mirror, 1, 32 and 32 (the merge with
/// the chunk before). Without either the neighbours hold less than a chunk's
/// worth and the bound on the chunks fails (`check`).
#[test]
fn a_chunk_that_falls_under_half_merges_with_the_next_and_with_the_one_before() {
    for forward in [true, false] {
        let mut list = Mappings::new();
        let mut model = Model(Vec::new());
        for n in 0..(3 * CHUNK) as u32 {
            add(&mut list, &mut model, PAGE, n).expect("a gap");
        }
        assert_eq!(list.chunk_count(), 3);
        // Cut the end that is to stay small to one record.
        let doomed: Vec<(u64, u64)> = if forward {
            model.0[2 * CHUNK + 1..].to_vec()
        } else {
            model.0[..CHUNK - 1].to_vec()
        };
        for &(va, length) in &doomed {
            list.remove_exact(va, length).expect("exact");
        }
        model.0.retain(|r| !doomed.contains(r));
        same(&list, &model);
        // Two from the middle chunk.
        let middle = if forward { CHUNK } else { 1 };
        for _ in 0..2 {
            let (va, length) = model.0.remove(middle + 8);
            list.remove_exact(va, length).expect("exact");
        }
        same(&list, &model);
        assert_eq!(list.chunk_count(), 2, "forward {forward}");
        assert_eq!(list.bytes_held(), 2 * Mappings::<u32>::CHUNK_BYTES);
    }
}

/// The merge is for neighbours that hold less than a chunk together, not
/// for exactly a chunk: 24 and 8 stay two, 22 and 8 become one; both ways.
#[test]
fn neighbours_merge_when_they_hold_less_than_a_chunk_and_not_at_exactly_one() {
    for forward in [true, false] {
        let mut list = Mappings::new();
        let mut model = Model(Vec::new());
        for n in 0..(3 * CHUNK) as u32 {
            add(&mut list, &mut model, PAGE, n).expect("a gap");
        }
        let doomed: Vec<(u64, u64)> = if forward {
            model.0[2 * CHUNK + 8..].to_vec()
        } else {
            model.0[..CHUNK - 8].to_vec()
        };
        for &(va, length) in &doomed {
            list.remove_exact(va, length).expect("exact");
        }
        model.0.retain(|r| !doomed.contains(r));
        assert_eq!(list.chunk_count(), 3);
        let middle = if forward { CHUNK } else { 8 };
        for n in 0..10 {
            let (va, length) = model.0.remove(middle + 3);
            list.remove_exact(va, length).expect("exact");
            same(&list, &model);
            // 32 - n - 1 and 8: 32 records together at n = 7, 31 at n = 8.
            let want = if n < 8 { 3 } else { 2 };
            assert_eq!(
                list.chunk_count(),
                want,
                "forward {forward}, after {}",
                n + 1
            );
        }
    }
}

/// Random maps and unmaps of random sizes: what `growth_for` says is what
/// `reserve_for` adds, and the list stays valid and few.
#[test]
fn random_use_keeps_the_prediction_and_the_bounds() {
    let mut rng = Rng(11);
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for round in 0..30_000u32 {
        let r = rng.next();
        if r % 7 < 4 && model.0.len() < LIMIT {
            let length = PAGE * (1 + (r >> 8) % 6);
            let Some(va) = list.find_gap(length, LOW, HIGH, PAGE).0 else {
                continue;
            };
            let (before, said) = (list.bytes_held(), list.growth_for(va));
            list.reserve_for(va).expect("room");
            assert_eq!(list.bytes_held() - before, said, "round {round}");
            list.insert(va, length, round).expect("room after reserve");
            let at = model.0.partition_point(|&(v, _)| v < va);
            model.0.insert(at, (va, length));
        } else if !model.0.is_empty() {
            // Bursts: now and then a run of neighbours leaves together.
            let n = if r.is_multiple_of(31) { 40 } else { 1 };
            for _ in 0..n.min(model.0.len()) {
                let i = (r >> 8) as usize % model.0.len();
                let (va, length) = model.0.remove(i);
                list.remove_exact(va, length).expect("exact");
            }
        }
        assert_eq!(list.len(), model.0.len());
        assert!(list.chunk_count() <= 2 * list.len() / CHUNK + 2);
        if round.is_multiple_of(211) {
            same(&list, &model);
        }
    }
    same(&list, &model);
}

#[test]
fn growth_is_bounded_by_what_max_growth_says() {
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for n in 0..LIMIT as u32 {
        let before = list.bytes_held();
        let worst = list.growth_for(list.find_gap(PAGE, LOW, HIGH, PAGE).0.unwrap());
        let va = list.find_gap(PAGE, LOW, HIGH, PAGE).0.unwrap();
        list.reserve_for(va).unwrap();
        let grown = list.bytes_held() - before;
        assert_eq!(grown, worst, "grew {grown}, said {worst}");
        list.insert(va, PAGE, n).unwrap();
        model.0.push((va, PAGE));
    }
    same(&list, &model);
}

#[test]
fn a_record_is_dropped_with_the_list() {
    use std::rc::Rc;
    let held = Rc::new(());
    let mut list = Mappings::new();
    let mut va = LOW;
    for _ in 0..200 {
        list.reserve_for(va).unwrap();
        list.insert(va, PAGE, held.clone()).ok().unwrap();
        va += 2 * PAGE;
    }
    assert_eq!(Rc::strong_count(&held), 201);
    drop(list);
    assert_eq!(Rc::strong_count(&held), 1);
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
zoraya_test_alloc::cap_memory!();

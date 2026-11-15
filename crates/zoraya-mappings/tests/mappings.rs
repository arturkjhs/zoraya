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
    assert_eq!(list.max_growth(), Mappings::<u32>::CHUNK_BYTES);
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

#[test]
fn growth_is_bounded_by_what_max_growth_says() {
    let mut list = Mappings::new();
    let mut model = Model(Vec::new());
    for n in 0..LIMIT as u32 {
        let before = list.bytes_held();
        let worst = list.max_growth();
        let va = list.find_gap(PAGE, LOW, HIGH, PAGE).0.unwrap();
        list.reserve_for(va).unwrap();
        let grown = list.bytes_held() - before;
        assert!(grown <= worst, "grew {grown}, said at most {worst}");
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

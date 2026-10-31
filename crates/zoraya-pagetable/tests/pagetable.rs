//! Tables are built in a buffer. Descriptor tests read each field by hand,
//! with the bit positions of the Arm ARM's "VMSAv8-64 descriptor formats",
//! rather than through the crate's own decoder.

use std::collections::BTreeMap;

use zoraya_pagetable::{
    Attributes, ENTRIES, Half, MAIR_VALUE, MapError, Mapping, MemoryType, PAGE_SIZE, PageTable,
    TableMemory, level_size,
};

const GIB: u64 = 1 << 30;
const MIB2: u64 = 2 << 20;
const HIGH: u64 = 0xffff_0000_0000_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Write(u64, usize, u64),
    Invalidate(u64, u64),
    Sync,
}

#[derive(Default)]
struct Buf {
    tables: BTreeMap<u64, Box<[u64; ENTRIES]>>,
    next: u64,
    /// Tables left before `alloc_table` fails.
    budget: Option<usize>,
    log: Vec<Event>,
}

impl Buf {
    fn new() -> Buf {
        Buf {
            next: 0x1000,
            ..Buf::default()
        }
    }
}

impl TableMemory for Buf {
    fn alloc_table(&mut self) -> Option<u64> {
        if let Some(b) = &mut self.budget {
            *b = b.checked_sub(1)?;
        }
        let pa = self.next;
        self.next += PAGE_SIZE;
        self.tables.insert(pa, Box::new([0; ENTRIES]));
        Some(pa)
    }

    fn read(&self, table: u64, index: usize) -> u64 {
        self.tables[&table][index]
    }

    fn write(&mut self, table: u64, index: usize, value: u64) {
        self.tables.get_mut(&table).unwrap()[index] = value;
        self.log.push(Event::Write(table, index, value));
    }

    fn invalidate(&mut self, va: u64, size: u64) {
        self.log.push(Event::Invalidate(va, size));
    }

    fn sync(&mut self) {
        self.log.push(Event::Sync);
    }
}

fn setup() -> (Buf, PageTable) {
    let mut m = Buf::new();
    let t = PageTable::new(&mut m, Half::Upper).unwrap();
    (m, t)
}

/// The descriptor mapping `va`, by a walk that shares no code with the crate.
fn raw_walk(m: &Buf, root: u64, va: u64) -> (usize, u64) {
    let mut table = root;
    for level in 0..4 {
        let shift = 39 - 9 * level;
        let d = m.read(table, ((va >> shift) & 0x1ff) as usize);
        assert_eq!(d & 1, 1, "invalid at level {level}");
        let table_or_page = d & 0b10 != 0;
        if level == 3 || !table_or_page {
            return (level, d);
        }
        assert_eq!(d & !0x0000_ffff_ffff_f003, 0, "table descriptor attributes");
        table = d & 0x0000_ffff_ffff_f000;
    }
    unreachable!()
}

fn mappings(m: &Buf, t: &PageTable) -> Vec<Mapping> {
    let mut v = Vec::new();
    t.for_each_mapping(m, &mut |x| v.push(x));
    v
}

#[test]
fn mair_has_device_at_0_and_write_back_at_1() {
    assert_eq!(MAIR_VALUE & 0xff, 0x00);
    assert_eq!((MAIR_VALUE >> 8) & 0xff, 0xff);
}

#[test]
fn page_descriptor_bits_for_kernel_text() {
    let (mut m, mut t) = setup();
    t.map(
        &mut m,
        HIGH + 0x5000,
        0x4000_3000,
        PAGE_SIZE,
        Attributes::KERNEL_TEXT,
    )
    .unwrap();
    let (level, d) = raw_walk(&m, t.root(), HIGH + 0x5000);
    assert_eq!(level, 3);
    assert_eq!(d & 0b11, 0b11, "page descriptor");
    assert_eq!((d >> 2) & 0b111, 1, "AttrIndx: Normal");
    assert_eq!((d >> 6) & 0b11, 0b10, "AP: EL1 read-only, no EL0 access");
    assert_eq!((d >> 8) & 0b11, 0b11, "SH: inner shareable");
    assert_eq!((d >> 10) & 1, 1, "AF");
    assert_eq!((d >> 11) & 1, 0, "nG: global");
    assert_eq!(d & 0x0000_ffff_ffff_f000, 0x4000_3000, "output address");
    assert_eq!((d >> 53) & 1, 0, "PXN clear: EL1 may execute");
    assert_eq!((d >> 54) & 1, 1, "UXN set");
}

#[test]
fn page_descriptor_bits_for_kernel_rodata_data_and_device() {
    let (mut m, mut t) = setup();
    let cases = [
        (Attributes::KERNEL_RODATA, 0b10, 1, 0b11),
        (Attributes::KERNEL_DATA, 0b00, 1, 0b11),
        (Attributes::DEVICE, 0b00, 0, 0b00),
    ];
    for (i, (attrs, ap, attr_index, sh)) in cases.into_iter().enumerate() {
        let va = HIGH + i as u64 * PAGE_SIZE;
        t.map(&mut m, va, 0x4000_0000, PAGE_SIZE, attrs).unwrap();
        let (_, d) = raw_walk(&m, t.root(), va);
        assert_eq!((d >> 6) & 0b11, ap, "AP {attrs:?}");
        assert_eq!((d >> 2) & 0b111, attr_index, "AttrIndx {attrs:?}");
        assert_eq!((d >> 8) & 0b11, sh, "SH {attrs:?}");
        assert_eq!((d >> 53) & 1, 1, "PXN {attrs:?}");
        assert_eq!((d >> 54) & 1, 1, "UXN {attrs:?}");
    }
}

#[test]
fn user_mappings_are_never_executable_at_el1() {
    let (mut m, mut t) = setup();
    let text = Attributes {
        memory: MemoryType::Normal,
        write: false,
        execute: true,
        user: true,
    };
    let data = Attributes {
        write: true,
        execute: false,
        ..text
    };
    t.map(&mut m, HIGH, 0x4000_0000, PAGE_SIZE, text).unwrap();
    t.map(&mut m, HIGH + PAGE_SIZE, 0x4000_0000, PAGE_SIZE, data)
        .unwrap();

    let (_, d) = raw_walk(&m, t.root(), HIGH);
    assert_eq!((d >> 6) & 0b11, 0b11, "AP: read-only at EL1 and EL0");
    assert_eq!((d >> 11) & 1, 1, "nG");
    assert_eq!((d >> 53) & 1, 1, "PXN");
    assert_eq!((d >> 54) & 1, 0, "UXN clear: EL0 may execute");

    let (_, d) = raw_walk(&m, t.root(), HIGH + PAGE_SIZE);
    assert_eq!((d >> 6) & 0b11, 0b01, "AP: read-write at EL1 and EL0");
    assert_eq!((d >> 53) & 1, 1, "PXN");
    assert_eq!((d >> 54) & 1, 1, "UXN");
}

#[test]
fn blocks_are_used_when_alignment_allows() {
    let (mut m, mut t) = setup();
    let va = HIGH + GIB - MIB2;
    let pa = GIB - MIB2;
    // 2 MiB up to a 1 GiB boundary, one 1 GiB block, 2 MiB, one page.
    let size = MIB2 + GIB + MIB2 + PAGE_SIZE;
    t.map(&mut m, va, pa, size, Attributes::KERNEL_DATA)
        .unwrap();

    let (level, d) = raw_walk(&m, t.root(), va);
    assert_eq!(level, 2);
    assert_eq!(d & 0b11, 0b01, "block descriptor");
    let (level, d) = raw_walk(&m, t.root(), va + MIB2);
    assert_eq!(level, 1);
    assert_eq!(d & 0b11, 0b01, "block descriptor");
    assert_eq!(d & 0x0000_ffff_c000_0000, GIB);
    assert_eq!(raw_walk(&m, t.root(), va + MIB2 + GIB).0, 2);
    assert_eq!(raw_walk(&m, t.root(), va + size - 1).0, 3);

    let sizes: Vec<u64> = mappings(&m, &t).iter().map(|x| x.size).collect();
    assert_eq!(sizes, [MIB2, GIB, MIB2, PAGE_SIZE]);
}

#[test]
fn misaligned_pa_falls_back_to_pages() {
    let (mut m, mut t) = setup();
    t.map(&mut m, HIGH, PAGE_SIZE, MIB2, Attributes::KERNEL_DATA)
        .unwrap();
    assert_eq!(mappings(&m, &t).len(), 512);
    let (mut m, mut t) = setup();
    t.map_pages(&mut m, HIGH, 0, MIB2, Attributes::KERNEL_DATA)
        .unwrap();
    assert_eq!(mappings(&m, &t).len(), 512, "pages asked for");
}

#[test]
fn walking_gives_back_the_mapping_set() {
    let (mut m, mut t) = setup();
    let requests = [
        (HIGH, 0x4000_0000, 3 * PAGE_SIZE, Attributes::KERNEL_TEXT),
        (HIGH + GIB, 0x8000_0000, GIB, Attributes::KERNEL_DATA),
        (HIGH + 5 * GIB, 0x900_0000, PAGE_SIZE, Attributes::DEVICE),
        (
            u64::MAX - PAGE_SIZE + 1,
            0,
            PAGE_SIZE,
            Attributes::KERNEL_RODATA,
        ),
    ];
    for (va, pa, size, attrs) in requests {
        t.map(&mut m, va, pa, size, attrs).unwrap();
    }
    let walked = mappings(&m, &t);
    assert_eq!(walked.len(), 6);
    for (va, pa, size, attrs) in requests {
        let mut at = 0;
        while at < size {
            let x = walked.iter().find(|x| x.va == va + at).unwrap();
            assert_eq!((x.pa, x.attributes), (pa + at, attrs));
            at += x.size;
        }
        let last = va + (size - 1);
        let tr = t.translate(&m, last).unwrap();
        assert_eq!(tr.pa + (last - tr.va), pa + size - 1);
    }
    assert_eq!(t.translate(&m, HIGH + 3 * PAGE_SIZE), None);
    assert_eq!(t.translate(&m, 0x4000), None, "other half");
}

#[test]
fn forbidden_attributes_are_refused() {
    let (mut m, mut t) = setup();
    let wx = Attributes {
        memory: MemoryType::Normal,
        write: true,
        execute: true,
        user: false,
    };
    let dx = Attributes {
        memory: MemoryType::Device,
        write: false,
        execute: true,
        user: false,
    };
    let user_wx = Attributes { user: true, ..wx };
    for a in [wx, dx, user_wx] {
        assert_eq!(
            t.map(&mut m, HIGH, 0, PAGE_SIZE, a),
            Err(MapError::Forbidden)
        );
    }
    assert!(mappings(&m, &t).is_empty());
}

#[test]
fn every_attribute_combination_round_trips_and_stays_wx_free() {
    let (mut m, mut t) = setup();
    let mut va = HIGH;
    for bits in 0..16u32 {
        let a = Attributes {
            memory: if bits & 1 != 0 {
                MemoryType::Device
            } else {
                MemoryType::Normal
            },
            write: bits & 2 != 0,
            execute: bits & 4 != 0,
            user: bits & 8 != 0,
        };
        let r = t.map(&mut m, va, 0, PAGE_SIZE, a);
        if a.is_forbidden() {
            assert_eq!(r, Err(MapError::Forbidden));
        } else {
            r.unwrap();
            assert_eq!(t.translate(&m, va).unwrap().attributes, a);
            let (_, d) = raw_walk(&m, t.root(), va);
            let (pxn, uxn, ro) = ((d >> 53) & 1 == 1, (d >> 54) & 1 == 1, (d >> 7) & 1 == 1);
            assert!(pxn || ro, "EL1-executable means read-only: {a:?}");
            assert!(uxn || ro, "EL0-executable means read-only: {a:?}");
        }
        va += PAGE_SIZE;
    }
}

#[test]
fn bad_ranges_are_refused() {
    let (mut m, mut t) = setup();
    let a = Attributes::KERNEL_DATA;
    assert_eq!(
        t.map(&mut m, HIGH + 1, 0, PAGE_SIZE, a),
        Err(MapError::Unaligned)
    );
    assert_eq!(
        t.map(&mut m, HIGH, 1, PAGE_SIZE, a),
        Err(MapError::Unaligned)
    );
    assert_eq!(t.map(&mut m, HIGH, 0, 100, a), Err(MapError::Unaligned));
    assert_eq!(t.map(&mut m, HIGH, 0, 0, a), Err(MapError::Unaligned));
    assert_eq!(
        t.map(&mut m, 0x1000, 0, PAGE_SIZE, a),
        Err(MapError::OutOfRange)
    );
    assert_eq!(
        t.map(&mut m, u64::MAX - PAGE_SIZE + 1, 0, 2 * PAGE_SIZE, a),
        Err(MapError::OutOfRange)
    );
    assert_eq!(
        t.map(&mut m, HIGH, 1 << 48, PAGE_SIZE, a),
        Err(MapError::OutOfRange)
    );
    let mut low = PageTable::new(&mut m, Half::Lower).unwrap();
    assert_eq!(
        low.map(&mut m, HIGH, 0, PAGE_SIZE, a),
        Err(MapError::OutOfRange)
    );
    low.map(&mut m, 0x1000, 0, PAGE_SIZE, a).unwrap();
    assert_eq!(low.translate(&m, 0x1000).unwrap().pa, 0);
}

#[test]
fn remapping_is_refused_and_changes_nothing() {
    let (mut m, mut t) = setup();
    let a = Attributes::KERNEL_DATA;
    t.map(&mut m, HIGH + MIB2, 0, MIB2, a).unwrap();
    let before = mappings(&m, &t);
    // A range that starts unmapped and runs into the block.
    assert_eq!(
        t.map(&mut m, HIGH, 0, 2 * MIB2, a),
        Err(MapError::AlreadyMapped)
    );
    assert_eq!(
        t.map(&mut m, HIGH + MIB2 + PAGE_SIZE, 0, PAGE_SIZE, a),
        Err(MapError::AlreadyMapped)
    );
    assert_eq!(mappings(&m, &t), before);
}

#[test]
fn running_out_of_tables_rolls_the_map_back() {
    let (mut m, mut t) = setup();
    // Two 2 MiB blocks under different level 1 entries need three tables.
    m.budget = Some(2);
    let r = t.map(
        &mut m,
        HIGH + GIB - MIB2,
        0,
        2 * MIB2,
        Attributes::KERNEL_DATA,
    );
    assert_eq!(r, Err(MapError::OutOfTables));
    assert!(mappings(&m, &t).is_empty());
    m.budget = None;
    t.map(&mut m, HIGH, 0, PAGE_SIZE, Attributes::KERNEL_DATA)
        .unwrap();
}

#[test]
fn unmap_removes_exactly_the_range() {
    let (mut m, mut t) = setup();
    t.map_pages(&mut m, HIGH, 0, 8 * PAGE_SIZE, Attributes::KERNEL_DATA)
        .unwrap();
    t.unmap(&mut m, HIGH + 2 * PAGE_SIZE, 3 * PAGE_SIZE)
        .unwrap();
    let left: Vec<u64> = mappings(&m, &t)
        .iter()
        .map(|x| (x.va - HIGH) / PAGE_SIZE)
        .collect();
    assert_eq!(left, [0, 1, 5, 6, 7]);
    assert_eq!(
        m.log.last(),
        Some(&Event::Invalidate(HIGH + 2 * PAGE_SIZE, 3 * PAGE_SIZE))
    );
    assert_eq!(
        t.unmap(&mut m, HIGH, 3 * PAGE_SIZE),
        Err(MapError::NotMapped),
        "a hole in the range"
    );
    assert_eq!(mappings(&m, &t).len(), 5, "nothing changed");
}

#[test]
fn unmapping_inside_a_block_splits_it_with_break_before_make() {
    let (mut m, mut t) = setup();
    t.map(&mut m, HIGH, 0, GIB, Attributes::KERNEL_DATA)
        .unwrap();
    assert_eq!(mappings(&m, &t).len(), 1);
    m.log.clear();
    let hole = HIGH + 3 * MIB2 + PAGE_SIZE;
    t.unmap(&mut m, hole, PAGE_SIZE).unwrap();

    assert_eq!(t.translate(&m, hole), None);
    // Everything else still maps to the same place with the same attributes.
    let mut covered = 0;
    for x in mappings(&m, &t) {
        assert_eq!(x.pa, x.va - HIGH);
        assert_eq!(x.attributes, Attributes::KERNEL_DATA);
        covered += x.size;
    }
    assert_eq!(covered, GIB - PAGE_SIZE);

    // Each entry that went from one valid descriptor to another went through
    // an invalid one, followed by a TLB invalidation of the whole old block.
    let mut splits = 0;
    let events = m.log.clone();
    for w in events.windows(3) {
        if let [
            Event::Write(t1, i1, 0),
            Event::Invalidate(va, size),
            Event::Write(t2, i2, new),
        ] = *w
        {
            assert_eq!((t1, i1), (t2, i2));
            assert_eq!(new & 0b11, 0b11, "new table descriptor");
            assert!(size == GIB || size == MIB2);
            assert!(va <= hole && hole < va + size);
            splits += 1;
        }
    }
    assert_eq!(splits, 2, "1 GiB to 2 MiB, 2 MiB to pages");
}

#[test]
fn a_failed_split_leaves_every_translation_in_place() {
    let (mut m, mut t) = setup();
    t.map(&mut m, HIGH, 0, 2 * MIB2, Attributes::KERNEL_DATA)
        .unwrap();
    let before = mappings(&m, &t);
    m.budget = Some(0);
    assert_eq!(
        t.unmap(&mut m, HIGH + PAGE_SIZE, MIB2),
        Err(MapError::OutOfTables)
    );
    assert_eq!(mappings(&m, &t), before);
}

#[test]
fn tables_of_an_unmapped_range_are_reused() {
    let (mut m, mut t) = setup();
    t.map(&mut m, HIGH, 0, PAGE_SIZE, Attributes::KERNEL_DATA)
        .unwrap();
    let tables = m.tables.len();
    t.unmap(&mut m, HIGH, PAGE_SIZE).unwrap();
    t.map(&mut m, HIGH, 0, PAGE_SIZE, Attributes::KERNEL_DATA)
        .unwrap();
    assert_eq!(m.tables.len(), tables);
}

#[test]
fn a_block_never_replaces_an_empty_table() {
    let (mut m, mut t) = setup();
    t.map_pages(&mut m, HIGH, 0, PAGE_SIZE, Attributes::KERNEL_DATA)
        .unwrap();
    t.unmap(&mut m, HIGH, PAGE_SIZE).unwrap();
    let tables = m.tables.len();
    m.log.clear();

    t.map(&mut m, HIGH, 0, MIB2, Attributes::KERNEL_DATA)
        .unwrap();
    assert_eq!(raw_walk(&m, t.root(), HIGH).0, 3, "pages in the kept table");
    assert_eq!(mappings(&m, &t).len(), 512);
    assert_eq!(m.tables.len(), tables, "no table lost or added");
    assert!(
        !m.log.iter().any(|e| matches!(e, Event::Invalidate(..))),
        "from invalid to valid needs no invalidation"
    );
}

#[test]
fn a_new_table_is_visible_before_its_parent_links_it() {
    let (mut m, mut t) = setup();
    t.map(&mut m, HIGH, 0, PAGE_SIZE, Attributes::KERNEL_DATA)
        .unwrap();
    let links = m.log.iter().enumerate().filter(|(_, e)| {
        matches!(e, Event::Write(_, _, v) if v & 0b11 == 0b11 && m.tables.contains_key(&(v & !0xfff)))
    });
    let mut count = 0;
    for (i, _) in links {
        assert_eq!(m.log[i - 1], Event::Sync, "barrier before link {i}");
        count += 1;
    }
    assert_eq!(count, 3, "levels 0, 1 and 2 link a new table");
}

#[test]
fn level_sizes() {
    assert_eq!(level_size(3), 4096);
    assert_eq!(level_size(2), MIB2);
    assert_eq!(level_size(1), GIB);
    assert_eq!(level_size(0), 512 * GIB);
}

/// Random map and unmap against a page-granular model.
#[test]
fn random_sequences_match_a_model() {
    let mut seed = 0x1234_5678_9abc_def1u64;
    let mut next = move |n: u64| {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        seed.wrapping_mul(0x2545_f491_4f6c_dd1d) % n
    };
    const PAGES: u64 = 4 * 512 + 40;
    let (mut m, mut t) = setup();
    let mut model = vec![None; PAGES as usize];
    for _ in 0..4000 {
        let start = next(PAGES);
        let longest = if next(4) == 0 { 700 } else { 20 };
        let len = (1 + next(longest)).min(PAGES - start);
        let va = HIGH + GIB - MIB2 + start * PAGE_SIZE;
        let size = len * PAGE_SIZE;
        let range = start as usize..(start + len) as usize;
        if next(2) == 0 {
            let pa = next(1 << 20) * PAGE_SIZE * if next(2) == 0 { 512 } else { 1 };
            let attrs = [
                Attributes::KERNEL_DATA,
                Attributes::KERNEL_RODATA,
                Attributes::KERNEL_TEXT,
                Attributes::DEVICE,
            ][next(4) as usize];
            let free = model[range.clone()].iter().all(Option::is_none);
            let got = t.map(&mut m, va, pa, size, attrs);
            if free {
                got.unwrap();
                for (i, slot) in model[range].iter_mut().enumerate() {
                    *slot = Some((pa + i as u64 * PAGE_SIZE, attrs));
                }
            } else {
                assert_eq!(got, Err(MapError::AlreadyMapped));
            }
        } else {
            let all = model[range.clone()].iter().all(Option::is_some);
            let got = t.unmap(&mut m, va, size);
            if all {
                got.unwrap();
                model[range].fill(None);
            } else {
                assert_eq!(got, Err(MapError::NotMapped));
            }
        }
        for _ in 0..16 {
            let page = next(PAGES);
            let va = HIGH + GIB - MIB2 + page * PAGE_SIZE;
            let got = t
                .translate(&m, va)
                .map(|x| (x.pa + (va - x.va), x.attributes));
            assert_eq!(got, model[page as usize]);
        }
    }
}

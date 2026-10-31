//! Crash states and ordering checks over a [`RecordingDevice`] log.
//!
//! # The crash model (ADR-0015 rules 2–4, INV-0015-1)
//!
//! A write is stable once a flush that completed after it exists in the log.
//! The log is cut into *windows*: the events between two consecutive flushes
//! (and before the first, and after the last). A crash anywhere inside window
//! `w` leaves:
//! - every write before the window's starting flush as written (stable);
//! - every 512-byte sector covered by a write inside the window holding its
//!   old (stable) content, the content of a write in the window, all zeros,
//!   or a seeded pseudo-random pattern, independently per sector.
//!
//! A crash earlier inside the window is included: a write not yet issued is
//! the "old" choice. So each window, cut at its end, covers every crash point
//! in it.
//!
//! # The bound (TASK-07, INV-0010-1)
//!
//! For each window with `u` writes:
//! - **whole-write subsets**: all `2^u` subsets when `u <= k` (each write in
//!   the subset fully new, the others fully old), otherwise the empty set, the
//!   full set and `random_subsets` seeded random subsets;
//! - **torn variants**: `torn` seeded images where every sector of every
//!   write independently takes one of the four values.
//!
//! [`RecordingDevice`]: crate::devices::RecordingDevice

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use zoraya_fs::check::{BlockKind, reachable_from};
use zoraya_fs::format::{N_PAIRS, ROOT_SLOTS, RootRecord};
use zoraya_fs::{BLOCK_SIZE, Block, SECTOR_SIZE};

use crate::devices::{Event, ImageDevice};
use crate::rng::Rng;

/// A stretch of the log between flushes.
#[derive(Debug, Clone)]
pub struct Window {
    /// Index of the window (0 = before the first flush).
    pub index: usize,
    /// Events `[start, end)` belong to it; `end` is the closing flush or the
    /// end of the log.
    pub start: usize,
    pub end: usize,
    /// Event indices of its writes.
    pub writes: Vec<usize>,
    /// Requests guaranteed durable at any crash in this window: the last
    /// `SyncMark` before `start`.
    pub durable: u64,
    /// Requests that may have completed by a crash in this window: the last
    /// `Mark` before `end`.
    pub completed: u64,
}

/// Splits a log into windows.
///
/// `durable` at a window's start is the larger of: the requests completed
/// before the last `sync` that returned (ADR-0010 rule 3's lower bound), and
/// the requests completed before the last commit whose root record a flush
/// has made stable (a commit group holds every request completed before it
/// started, ADR-0011 rule 2; the engine commits only between requests).
pub fn windows(events: &[Event]) -> Vec<Window> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut durable = 0;
    let mut completed = 0;
    let mut writes = Vec::new();
    let mut durable_at_start = 0;
    let mut root_in_window = false;
    for (i, e) in events.iter().enumerate() {
        match e {
            Event::Write { block, .. } => {
                if *block < ROOT_SLOTS {
                    root_in_window = true;
                }
                writes.push(i);
            }
            Event::Mark { completed: c } => completed = *c,
            Event::SyncMark { completed: c } => {
                durable = durable.max(*c);
                completed = completed.max(*c);
            }
            Event::Flush => {
                out.push(Window {
                    index: out.len(),
                    start,
                    end: i,
                    writes: std::mem::take(&mut writes),
                    durable: durable_at_start,
                    completed,
                });
                if root_in_window {
                    durable = durable.max(completed);
                }
                root_in_window = false;
                start = i + 1;
                durable_at_start = durable;
            }
        }
    }
    out.push(Window {
        index: out.len(),
        start,
        end: events.len(),
        writes,
        durable: durable_at_start,
        completed,
    });
    out
}

/// Bounds of the enumeration.
#[derive(Debug, Clone, Copy)]
pub struct CrashConfig {
    /// All subsets when a window has at most this many writes.
    pub k: usize,
    /// Random subsets per window beyond `k`.
    pub random_subsets: usize,
    /// Torn (per-sector) variants per window.
    pub torn: usize,
    pub seed: u64,
}

impl Default for CrashConfig {
    fn default() -> Self {
        CrashConfig {
            k: 12,
            random_subsets: 64,
            torn: 16,
            seed: 1,
        }
    }
}

/// How a crash image was made.
#[derive(Debug, Clone)]
pub enum Variant {
    /// The writes marked `true` are fully new, the others fully old.
    Subset(Vec<bool>),
    /// Per-sector choices from a seed.
    Torn(u64),
}

/// The seeded "arbitrary" pattern for a sector of a write (INV-0015-1's
/// finite representation).
pub fn random_sector(seed: u64, event: usize, sector: usize) -> [u8; SECTOR_SIZE] {
    let mut r = Rng::new(seed ^ ((event as u64) << 20) ^ sector as u64);
    let mut s = [0u8; SECTOR_SIZE];
    r.fill(&mut s);
    s
}

fn write_of(events: &[Event], i: usize) -> (u64, &Block) {
    match &events[i] {
        Event::Write { block, data, .. } => (*block, data),
        _ => unreachable!("not a write"),
    }
}

/// Calls `f` with every crash image of every window, within `cfg`'s bound.
/// Returns the number of images.
pub fn for_each_crash_image(
    base: &Rc<Vec<u8>>,
    events: &[Event],
    cfg: CrashConfig,
    mut f: impl FnMut(&Window, &Variant, ImageDevice),
) -> u64 {
    let mut stable = (**base).clone();
    let mut count = 0;
    let mut rng = Rng::new(cfg.seed);
    for w in windows(events) {
        let stable_rc = Rc::new(stable.clone());
        let u = w.writes.len();
        let mut masks: Vec<Vec<bool>> = Vec::new();
        if u <= cfg.k {
            for m in 0..(1u64 << u) {
                masks.push((0..u).map(|j| m >> j & 1 == 1).collect());
            }
        } else {
            for p in 0..=u {
                masks.push((0..u).map(|j| j < p).collect());
            }
            for _ in 0..cfg.random_subsets {
                masks.push((0..u).map(|_| rng.chance(1, 2)).collect());
            }
        }
        for m in masks {
            let mut dev = ImageDevice::new(stable_rc.clone());
            for (j, &ei) in w.writes.iter().enumerate() {
                if m[j] {
                    let (b, d) = write_of(events, ei);
                    dev.set_block(b, *d);
                }
            }
            count += 1;
            f(&w, &Variant::Subset(m), dev);
        }
        if u > 0 {
            for _ in 0..cfg.torn {
                let seed = rng.next_u64();
                let dev = torn_image(&stable_rc, events, &w, seed, cfg.seed);
                count += 1;
                f(&w, &Variant::Torn(seed), dev);
            }
        }
        // Advance the stable image past this window's writes.
        for &ei in &w.writes {
            let (b, d) = write_of(events, ei);
            let s = b as usize * BLOCK_SIZE;
            stable[s..s + BLOCK_SIZE].copy_from_slice(&d[..]);
        }
    }
    count
}

/// One torn image of window `w`: each sector of each write takes old, new,
/// zeros or the random pattern.
pub fn torn_image(
    stable: &Rc<Vec<u8>>,
    events: &[Event],
    w: &Window,
    seed: u64,
    pattern_seed: u64,
) -> ImageDevice {
    let mut r = Rng::new(seed);
    let mut dev = ImageDevice::new(stable.clone());
    for &ei in &w.writes {
        let (b, d) = write_of(events, ei);
        let mut blk = dev.get_block(b);
        for s in 0..BLOCK_SIZE / SECTOR_SIZE {
            let range = s * SECTOR_SIZE..(s + 1) * SECTOR_SIZE;
            match r.below(4) {
                0 => {}
                1 => blk[range.clone()].copy_from_slice(&d[range]),
                2 => blk[range].fill(0),
                _ => blk[range].copy_from_slice(&random_sector(pattern_seed, ei, s)),
            }
        }
        dev.set_block(b, blk);
    }
    dev
}

/// The possible contents of every sector after a crash at the end of window
/// `w`, as the enumerator represents them: the stable content, the content of
/// each write in the window covering the sector, zeros and the random
/// pattern of each such write. For the INV-0015-1 self-test.
pub fn sector_choices(
    base: &[u8],
    events: &[Event],
    w: &Window,
    pattern_seed: u64,
) -> Vec<BTreeSet<Vec<u8>>> {
    let mut stable = base.to_vec();
    for (i, e) in events.iter().enumerate().take(w.start) {
        if let Event::Write { block, data, .. } = e {
            let s = *block as usize * BLOCK_SIZE;
            stable[s..s + BLOCK_SIZE].copy_from_slice(&data[..]);
        }
        let _ = i;
    }
    let mut out: Vec<BTreeSet<Vec<u8>>> = stable
        .chunks(SECTOR_SIZE)
        .map(|c| BTreeSet::from([c.to_vec()]))
        .collect();
    for &ei in &w.writes {
        let (b, d) = write_of(events, ei);
        for s in 0..BLOCK_SIZE / SECTOR_SIZE {
            let set = &mut out[b as usize * (BLOCK_SIZE / SECTOR_SIZE) + s];
            set.insert(d[s * SECTOR_SIZE..(s + 1) * SECTOR_SIZE].to_vec());
            set.insert(vec![0; SECTOR_SIZE]);
            set.insert(random_sector(pattern_seed, ei, s).to_vec());
        }
    }
    out
}

/// What [`check_ordering`] saw.
#[derive(Debug, Clone, Default)]
pub struct OrderingStats {
    pub commits: u64,
    pub writes_checked: u64,
    pub syncs_checked: u64,
}

/// Checks a log against INV-0011-1, -2 and -3:
///
/// - no write targets a block reachable from the root record of a
///   generation mount could select (the newest durable one and every one
///   written after it);
/// - a block freed by commit `g` is not written before `g`'s root record is
///   stable;
/// - a root record is written only after every new block of its commit was
///   written and a flush followed, both copies of its pair in one batch, and
///   no other write in that batch;
/// - a `SyncMark` is preceded by a flush after the last root record write.
pub fn check_ordering(base: &[u8], events: &[Event]) -> Result<OrderingStats, String> {
    let mut cur = base.to_vec();
    let device_blocks = (cur.len() / BLOCK_SIZE) as u64;
    let reach = |img: &[u8], rec: &RootRecord| -> Result<BTreeSet<u64>, String> {
        let mut dev = ImageDevice::new(Rc::new(img.to_vec()));
        let m = reachable_from(&mut dev, rec)?;
        Ok(m.into_iter()
            .filter(|(_, (k, _))| *k != BlockKind::RootSlot)
            .map(|(b, _)| b)
            .collect())
    };
    let first = {
        let mut dev = ImageDevice::new(Rc::new(cur.clone()));
        zoraya_fs::select_root(&mut dev)
            .map_err(|e| e.0.to_string())?
            .record
    };
    // generation -> reachable set, for the durable generation and later.
    let mut gens: BTreeMap<u64, BTreeSet<u64>> =
        BTreeMap::from([(first.generation, reach(&cur, &first)?)]);
    let mut durable = first.generation;
    // Root writes not yet followed by a flush.
    let mut unflushed_roots: Vec<u64> = Vec::new();
    // Blocks written since the last root record write, and whether a flush
    // has completed since the last non-root write.
    let mut written_since_root: BTreeSet<u64> = BTreeSet::new();
    let mut flushed_since_data = true;
    let mut root_flushed = true;
    let mut freed_pending: BTreeMap<u64, u64> = BTreeMap::new(); // block -> freeing generation
    let mut stats = OrderingStats::default();
    let mut batches: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for e in events {
        if let Event::Write { block, batch, .. } = e {
            batches.entry(*batch).or_default().push(*block);
        }
    }
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Write { block, data, batch } if *block < ROOT_SLOTS => {
                let pair = block % N_PAIRS;
                let rec = RootRecord::decode(data, device_blocks, pair).map_err(|e| {
                    format!(
                        "event {i}: root slot {block} written with an invalid record: {}",
                        e.0
                    )
                })?;
                let members = &batches[batch];
                let mut want = RootRecord::slots(pair).to_vec();
                let mut got = members.clone();
                want.sort();
                got.sort();
                if got != want {
                    return Err(format!(
                        "event {i}: root record batch {got:?} is not exactly the pair {want:?}"
                    ));
                }
                // Apply the whole batch, then check generation g.
                let n = members.len();
                for e in &events[i..i + n] {
                    if let Event::Write { block, data, .. } = e {
                        let s = *block as usize * BLOCK_SIZE;
                        cur[s..s + BLOCK_SIZE].copy_from_slice(&data[..]);
                    }
                }
                if !flushed_since_data {
                    return Err(format!(
                        "event {i}: root record of generation {} written before a flush",
                        rec.generation
                    ));
                }
                let g = rec.generation;
                let prev = gens.get(&(g - 1)).ok_or(format!(
                    "event {i}: generation {g} does not follow a known one"
                ))?;
                let r = reach(&cur, &rec).map_err(|e| format!("event {i}: generation {g}: {e}"))?;
                for b in r.difference(prev) {
                    if !written_since_root.contains(b) {
                        return Err(format!(
                            "event {i}: block {b} of generation {g} was not written by its commit"
                        ));
                    }
                }
                for b in prev.difference(&r) {
                    freed_pending.insert(*b, g);
                }
                gens.insert(g, r);
                unflushed_roots.push(g);
                written_since_root.clear();
                root_flushed = false;
                stats.commits += 1;
                i += n;
                continue;
            }
            Event::Write { block, data, .. } => {
                for (g, set) in gens.range(durable..) {
                    if set.contains(block) {
                        return Err(format!(
                            "event {i}: write to block {block}, reachable from generation {g} (durable is {durable})"
                        ));
                    }
                }
                if let Some(g) = freed_pending.get(block) {
                    return Err(format!(
                        "event {i}: block {block} reused before its freeing commit {g} is durable"
                    ));
                }
                let s = *block as usize * BLOCK_SIZE;
                cur[s..s + BLOCK_SIZE].copy_from_slice(&data[..]);
                written_since_root.insert(*block);
                flushed_since_data = false;
                stats.writes_checked += 1;
            }
            Event::Flush => {
                flushed_since_data = true;
                if let Some(&g) = unflushed_roots.iter().max() {
                    durable = g;
                    freed_pending.retain(|_, fg| *fg > g);
                    let keep: BTreeMap<u64, BTreeSet<u64>> = gens.split_off(&g);
                    gens = keep;
                }
                unflushed_roots.clear();
                root_flushed = true;
            }
            Event::SyncMark { .. } => {
                if !root_flushed {
                    return Err(format!(
                        "event {i}: sync returned before the root record was flushed"
                    ));
                }
                stats.syncs_checked += 1;
            }
            Event::Mark { .. } => {}
        }
        i += 1;
    }
    Ok(stats)
}

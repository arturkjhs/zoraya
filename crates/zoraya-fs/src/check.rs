//! The offline checker (`fsck`), ADR-0011 INV-0011-4 and ADR-0012 rule 3.
//!
//! [`check`] reads a volume without writing to it and reports every
//! violation of the invariants the ADRs define:
//!
//! - the root slots: a valid newest generation, consecutive in ring order
//!   (ADR-0011 rule 4);
//! - every reachable block's checksum, header and structure (ADR-0012 rules 1
//!   and 4), every data block's checksum if asked;
//! - no block reachable twice; the allocator's used set equal to the
//!   reachable set (ADR-0011, INV-0011-4);
//! - the namespace a tree with at most one name per object, every entry
//!   pointing at an object of the kind it says, directory sizes equal to
//!   their entry counts, parent links matching (ADR-0013 rule 5);
//! - attributes in range, `ctime` not after the root record's
//!   `last_assigned`, ids below `id_limit` (ADR-0014 rules 1 and 3);
//! - link targets valid UTF-8 of at most 4,096 bytes (ADR-0013 rule 7).
//!
//! Objects on the orphan list are reported in the statistics, not as
//! problems: a read-write mount frees them (ADR-0011 rule 7).
//!
//! [`repair`] fixes what can be fixed without a second copy of anything: the
//! allocator (rebuilt from the reachable set) and namespace damage (dangling
//! entries removed, unreferenced objects put on the orphan list, directory
//! sizes and parent links corrected). It refuses if any tree block is
//! unreadable or invalid. Every repair is itself a copy-on-write commit.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::device::{BLOCK_SIZE, Block, BlockDevice};
use crate::format::{
    Attr, BITS_PER_BITMAP, BlockPtr, Body, Child, DataRef, K_DIRENT, K_EXTENT, K_INODE, K_ORPHAN,
    K_PARENT, Key, MAX_LINK_TARGET, Node, ObjKind, ROOT_ID, ROOT_SLOTS, RootRecord, Value,
    decode_alloc_root, decode_bitmap,
};
use crate::root::{RootSelection, select_root};

/// Options for [`check`].
#[derive(Debug, Clone, Copy)]
pub struct CheckOptions {
    /// Read every data block and verify its checksum.
    pub read_data: bool,
}

impl Default for CheckOptions {
    fn default() -> Self {
        CheckOptions { read_data: true }
    }
}

/// What the volume holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub generation: u64,
    pub block_count: u64,
    pub tree_height: u8,
    pub tree_nodes: u64,
    pub data_blocks: u64,
    pub allocator_blocks: u64,
    pub used_blocks: u64,
    pub files: u64,
    pub directories: u64,
    pub links: u64,
    pub orphans: u64,
}

/// What kind of problem was found, for repair decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    /// Root slots: no valid record, or generations not consecutive.
    Roots,
    /// A tree node or data block is unreadable or invalid. Not repairable.
    Tree,
    /// The allocator blocks or the used set. Repairable.
    Allocator,
    /// The namespace or attributes. Repairable.
    Namespace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub kind: ProblemKind,
    pub what: String,
}

/// The result of [`check`].
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub problems: Vec<Problem>,
    pub stats: Stats,
    pub selection: Option<RootSelection>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }

    fn add(&mut self, kind: ProblemKind, what: String) {
        // Bounded report: a badly damaged volume could produce millions.
        if self.problems.len() < 1000 {
            self.problems.push(Problem { kind, what });
        }
    }

    pub fn has(&self, kind: ProblemKind) -> bool {
        self.problems.iter().any(|p| p.kind == kind)
    }
}

/// Everything the walk collects, for the namespace checks and repair.
#[derive(Default)]
struct Collected {
    inodes: BTreeMap<u64, Attr>,
    parents: BTreeMap<u64, u64>,
    /// (directory, name, child, kind)
    dirents: Vec<(u64, Vec<u8>, u64, ObjKind)>,
    /// (object, index, pointer)
    extents: Vec<(u64, u64, BlockPtr)>,
    orphans: BTreeSet<u64>,
}

struct Walker<'d, D: ?Sized> {
    dev: &'d mut D,
    rec: RootRecord,
    reachable: Vec<u64>,
    /// Blocks of the allocator itself, marked in `reachable` too.
    alloc_blocks: BTreeSet<u64>,
    report: Report,
    col: Collected,
    opts: CheckOptions,
}

impl<D: BlockDevice + ?Sized> Walker<'_, D> {
    fn mark(&mut self, addr: u64, what: &str) {
        let (w, bit) = ((addr / 64) as usize, addr % 64);
        match self.reachable.get_mut(w) {
            Some(word) if *word >> bit & 1 != 0 => {
                self.report.add(
                    ProblemKind::Tree,
                    format!("block {addr} ({what}) is reachable twice"),
                );
            }
            Some(word) => *word |= 1 << bit,
            None => self.report.add(
                ProblemKind::Tree,
                format!("block {addr} ({what}) is outside the volume"),
            ),
        }
    }

    fn is_reachable(&self, addr: u64) -> bool {
        self.reachable
            .get((addr / 64) as usize)
            .is_some_and(|w| w >> (addr % 64) & 1 != 0)
    }

    fn read(&mut self, addr: u64) -> Option<Box<Block>> {
        let mut b = Box::new([0u8; BLOCK_SIZE]);
        match self.dev.read(addr, &mut b) {
            Ok(()) => Some(b),
            Err(e) => {
                self.report.add(
                    ProblemKind::Tree,
                    format!("block {addr}: device error {e:?}"),
                );
                None
            }
        }
    }

    fn walk(&mut self, ptr: BlockPtr, level: u8, lower: Option<&Key>, upper: Option<&Key>) {
        self.mark(ptr.addr, "tree node");
        let Some(b) = self.read(ptr.addr) else { return };
        let node = match Node::decode(
            &b,
            &ptr,
            self.rec.volume_id,
            self.rec.block_count,
            level,
            lower,
            upper,
        ) {
            Ok(n) => n,
            Err(e) => {
                self.report.add(
                    ProblemKind::Tree,
                    format!("tree node at block {}: {}", ptr.addr, e.0),
                );
                return;
            }
        };
        self.report.stats.tree_nodes += 1;
        match node.body {
            Body::Leaf(items) => {
                for (k, v) in items {
                    self.item(k, v);
                }
            }
            Body::Interior(es) => {
                for i in 0..es.len() {
                    let lo = if i > 0 {
                        es.get(i).map(|e| e.0.clone())
                    } else {
                        lower.cloned()
                    };
                    let up = match es.get(i + 1) {
                        Some(e) => Some(e.0.clone()),
                        None => upper.cloned(),
                    };
                    if let Some((_, Child::Clean(p))) = es.get(i) {
                        self.walk(*p, level - 1, lo.as_ref(), up.as_ref());
                    }
                }
            }
        }
    }

    fn item(&mut self, k: Key, v: Value) {
        match (k.kind, v) {
            (K_INODE, Value::Inode(a)) => {
                self.col.inodes.insert(k.id, a);
            }
            (K_PARENT, Value::Parent(p)) => {
                self.col.parents.insert(k.id, p);
            }
            (K_DIRENT, Value::Dirent { child, kind }) => {
                self.col.dirents.push((k.id, k.name, child, kind))
            }
            (K_EXTENT, Value::Extent(DataRef::Clean(p))) => {
                self.mark(p.addr, "data block");
                self.report.stats.data_blocks += 1;
                if self.opts.read_data
                    && let Some(b) = self.read(p.addr)
                    && !p.verifies(&b)
                {
                    self.report.add(
                        ProblemKind::Tree,
                        format!(
                            "data block {} of object {} index {}: checksum mismatch",
                            p.addr, k.id, k.off
                        ),
                    );
                }
                self.col.extents.push((k.id, k.off, p));
            }
            (K_ORPHAN, Value::Orphan) => {
                self.col.orphans.insert(k.off);
            }
            _ => self.report.add(
                ProblemKind::Tree,
                format!("item {k:?} has a value of another kind"),
            ),
        }
    }

    /// The allocator: its blocks valid, its used set equal to the reachable
    /// set. Returns the bitmap bits if readable.
    fn allocator(&mut self) {
        let rec = self.rec;
        self.mark(rec.alloc_root.addr, "allocator root");
        self.alloc_blocks.insert(rec.alloc_root.addr);
        let Some(ab) = self.read(rec.alloc_root.addr) else {
            return;
        };
        let bitmaps = match decode_alloc_root(&ab, &rec.alloc_root, rec.volume_id, rec.block_count)
        {
            Ok(v) => v,
            Err(e) => {
                self.report
                    .add(ProblemKind::Allocator, format!("allocator root: {}", e.0));
                return;
            }
        };
        self.report.stats.allocator_blocks = 1 + bitmaps.len() as u64;
        let mut bits: Vec<u8> = Vec::new();
        for (i, p) in bitmaps.iter().enumerate() {
            self.mark(p.addr, "bitmap");
            self.alloc_blocks.insert(p.addr);
            let Some(bb) = self.read(p.addr) else { return };
            match decode_bitmap(&bb, p, rec.volume_id, rec.block_count, i as u32) {
                Ok(b) => bits.extend_from_slice(b),
                Err(e) => {
                    self.report
                        .add(ProblemKind::Allocator, format!("bitmap block {i}: {}", e.0));
                    return;
                }
            }
        }
        let used = |b: u64| {
            bits.get((b / 8) as usize)
                .is_some_and(|x| x >> (b % 8) & 1 != 0)
        };
        let (mut leaked, mut missing) = (0u64, 0u64);
        let mut first = None;
        for b in 0..bitmaps.len() as u64 * BITS_PER_BITMAP {
            let expect = b < ROOT_SLOTS || b >= rec.block_count || self.is_reachable(b);
            if used(b) != expect {
                if expect {
                    missing += 1;
                } else {
                    leaked += 1;
                }
                first.get_or_insert(b);
            }
        }
        if leaked + missing > 0 {
            self.report.add(
                ProblemKind::Allocator,
                format!(
                    "allocator disagrees with the reachable set: {leaked} blocks marked used but unreachable, \
                     {missing} reachable or reserved but marked free (first at block {})",
                    first.unwrap_or(0)
                ),
            );
        }
    }

    fn namespace(&mut self) {
        let rec = self.rec;
        let col = &self.col;
        let mut problems: Vec<String> = Vec::new();
        let mut stats = (0u64, 0u64, 0u64);
        match col.inodes.get(&ROOT_ID) {
            Some(a) if a.kind == ObjKind::Directory => {}
            Some(_) => problems.push("the root is not a directory".into()),
            None => problems.push("the root directory has no attribute record".into()),
        }
        let mut refs: BTreeMap<u64, u32> = BTreeMap::new();
        let mut entries: BTreeMap<u64, u64> = BTreeMap::new();
        let mut children: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for (dir, name, child, kind) in &col.dirents {
            let shown = String::from_utf8_lossy(name);
            match col.inodes.get(dir) {
                Some(a) if a.kind == ObjKind::Directory => {}
                _ => problems.push(format!(
                    "entry {shown:?} is in {dir}, which is not a directory"
                )),
            }
            match col.inodes.get(child) {
                Some(a) if a.kind == *kind => {}
                Some(a) => problems.push(format!(
                    "entry {shown:?} in {dir} says {kind:?} but object {child} is {:?}",
                    a.kind
                )),
                None => problems.push(format!(
                    "entry {shown:?} in {dir} points to missing object {child}"
                )),
            }
            *refs.entry(*child).or_insert(0) += 1;
            *entries.entry(*dir).or_insert(0) += 1;
            children.entry(*dir).or_default().push(*child);
            if *kind == ObjKind::Directory && col.parents.get(child) != Some(dir) {
                problems.push(format!(
                    "directory {child} is in {dir} but its parent link says {:?}",
                    col.parents.get(child)
                ));
            }
        }
        for o in &col.orphans {
            if !col.inodes.contains_key(o) {
                problems.push(format!("orphan entry for missing object {o}"));
            }
            *refs.entry(*o).or_insert(0) += 1;
        }
        for (id, a) in &col.inodes {
            let r = refs.get(id).copied().unwrap_or(0);
            if *id == ROOT_ID {
                if r != 0 {
                    problems.push("the root directory has a name".into());
                }
            } else if r != 1 {
                problems.push(format!(
                    "object {id} has {r} names (expected exactly one, or the orphan list)"
                ));
            }
            if *id >= rec.id_limit {
                problems.push(format!(
                    "object {id} is at or above the id limit {}",
                    rec.id_limit
                ));
            }
            if a.ctime > rec.last_assigned {
                problems.push(format!(
                    "object {id} has a ctime after the last assigned time"
                ));
            }
            match a.kind {
                ObjKind::Directory => {
                    stats.1 += 1;
                    let n = entries.get(id).copied().unwrap_or(0);
                    if a.size != n {
                        problems.push(format!(
                            "directory {id} has size {} but {n} entries",
                            a.size
                        ));
                    }
                    let named = *id != ROOT_ID && !col.orphans.contains(id);
                    if named != col.parents.contains_key(id) {
                        problems.push(format!(
                            "directory {id} has a parent link but no name, or a name but no link"
                        ));
                    }
                }
                ObjKind::File => stats.0 += 1,
                ObjKind::Link => stats.2 += 1,
            }
        }
        for id in col.parents.keys() {
            if col
                .inodes
                .get(id)
                .is_none_or(|a| a.kind != ObjKind::Directory)
            {
                problems.push(format!("parent link for {id}, which is not a directory"));
            }
        }
        for (id, index, _) in &col.extents {
            match col.inodes.get(id) {
                None => problems.push(format!("data for missing object {id}")),
                Some(a) if a.kind == ObjKind::Directory => {
                    problems.push(format!("data for directory {id}"))
                }
                Some(a) => {
                    if *index >= a.size.div_ceil(BLOCK_SIZE as u64) {
                        problems.push(format!(
                            "object {id} has data at block {index}, past its size {}",
                            a.size
                        ));
                    }
                }
            }
        }
        // Every named object is reachable from the root by names: no
        // directory cycle detached from the root.
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        let mut stack = vec![ROOT_ID];
        while let Some(d) = stack.pop() {
            if !seen.insert(d) {
                continue;
            }
            if let Some(cs) = children.get(&d) {
                stack.extend(cs.iter().copied());
            }
        }
        for id in col.inodes.keys() {
            if !seen.contains(id)
                && !col.orphans.contains(id)
                && refs.get(id).copied().unwrap_or(0) > 0
            {
                problems.push(format!(
                    "object {id} is named but not reachable from the root"
                ));
            }
        }
        let links: Vec<(u64, u64)> = col
            .inodes
            .iter()
            .filter(|(_, a)| a.kind == ObjKind::Link)
            .map(|(id, a)| (*id, a.size))
            .collect();
        let link_ptrs: BTreeMap<u64, BlockPtr> = col
            .extents
            .iter()
            .filter(|(_, i, _)| *i == 0)
            .map(|(id, _, p)| (*id, *p))
            .collect();
        for p in problems {
            self.report.add(ProblemKind::Namespace, p);
        }
        for (id, size) in links {
            if size as usize > MAX_LINK_TARGET {
                self.report
                    .add(ProblemKind::Namespace, format!("link {id} target too long"));
                continue;
            }
            let ok = match link_ptrs.get(&id) {
                None => size == 0,
                Some(p) => self.read(p.addr).is_some_and(|b| {
                    p.verifies(&b)
                        && b.get(..size as usize)
                            .is_some_and(|s| core::str::from_utf8(s).is_ok())
                }),
            };
            if !ok {
                self.report.add(
                    ProblemKind::Namespace,
                    format!("link {id} target unreadable or not UTF-8"),
                );
            }
        }
        self.report.stats.files = stats.0;
        self.report.stats.directories = stats.1;
        self.report.stats.links = stats.2;
        self.report.stats.orphans = self.col.orphans.len() as u64;
    }
}

/// Checks the volume on `dev` without writing to it.
pub fn check<D: BlockDevice + ?Sized>(dev: &mut D, opts: CheckOptions) -> Report {
    check_collect(dev, opts).0
}

/// What a check found, for repair.
struct Found {
    col: Collected,
    reachable: Vec<u64>,
    alloc_blocks: BTreeSet<u64>,
}

fn check_collect<D: BlockDevice + ?Sized>(dev: &mut D, opts: CheckOptions) -> (Report, Found) {
    let mut report = Report::default();
    let sel = match select_root(dev) {
        Ok(s) => s,
        Err(e) => {
            report.add(ProblemKind::Roots, format!("root records: {}", e.0));
            let found = Found {
                col: Collected::default(),
                reachable: Vec::new(),
                alloc_blocks: BTreeSet::new(),
            };
            return (report, found);
        }
    };
    if !sel.ring_consistent {
        report.add(
            ProblemKind::Roots,
            format!(
                "root record generations are not consecutive: {:?}",
                sel.pairs
            ),
        );
    }
    let rec = sel.record;
    report.stats.generation = rec.generation;
    report.stats.block_count = rec.block_count;
    report.stats.tree_height = rec.tree_height;
    report.selection = Some(sel);
    let mut w = Walker {
        dev,
        rec,
        reachable: vec![0u64; rec.block_count.div_ceil(64) as usize],
        alloc_blocks: BTreeSet::new(),
        report,
        col: Collected::default(),
        opts,
    };
    w.walk(rec.tree_root, rec.tree_height - 1, None, None);
    w.allocator();
    w.namespace();
    w.report.stats.used_blocks = w
        .reachable
        .iter()
        .map(|x| u64::from(x.count_ones()))
        .sum::<u64>();
    (
        w.report,
        Found {
            col: w.col,
            reachable: w.reachable,
            alloc_blocks: w.alloc_blocks,
        },
    )
}

/// What [`repair`] did.
#[derive(Debug, Clone, Default)]
pub struct RepairReport {
    pub allocator_rebuilt: bool,
    pub entries_removed: u64,
    pub objects_orphaned: u64,
    pub sizes_fixed: u64,
    pub parents_fixed: u64,
}

/// Repairs what [`check`] reports as repairable. Writes only through
/// copy-on-write commits, so a crash during repair leaves the volume as it
/// was or repaired. Refuses (`Err`) if the root records or the tree are
/// damaged.
pub fn repair<D: BlockDevice, C: crate::Clock>(
    mut dev: D,
    clock: C,
) -> Result<(RepairReport, D), String> {
    let (report, found) = check_collect(&mut dev, CheckOptions { read_data: false });
    if report.has(ProblemKind::Roots) || report.has(ProblemKind::Tree) {
        return Err(
            "the root records or the tree are damaged; there is no second copy to repair from"
                .into(),
        );
    }
    let mut out = RepairReport::default();
    if report.has(ProblemKind::Allocator) {
        let rec = report.selection.as_ref().ok_or("no root record")?.record;
        // The old allocator's blocks are marked reachable, so the new one
        // never overwrites them while the old root can still be chosen.
        rebuild_allocator(&mut dev, rec, &found.reachable, &found.alloc_blocks)
            .map_err(|e| format!("rebuilding the allocator: {e:?}"))?;
        out.allocator_rebuilt = true;
    }
    if report.has(ProblemKind::Namespace) {
        let fixes = plan_namespace_fixes(&found.col);
        let mut v = crate::Volume::mount(dev, clock, crate::MountOptions::default())
            .map_err(|e| format!("mounting for repair: {e:?}"))?;
        v.apply_fixes(&fixes, &mut out)
            .map_err(|e| format!("repairing the namespace: {e:?}"))?;
        dev = v
            .unmount()
            .map_err(|e| format!("committing the repair: {e:?}"))?;
        // A second mount frees what was put on the orphan list.
        let v = crate::Volume::mount(dev, NoClock, crate::MountOptions::default())
            .map_err(|e| format!("freeing orphans: {e:?}"))?;
        dev = v.unmount().map_err(|e| format!("freeing orphans: {e:?}"))?;
    }
    Ok((out, dev))
}

/// A clock for a mount that assigns no times.
struct NoClock;

impl crate::Clock for NoClock {
    fn wall(&mut self) -> crate::Timestamp {
        crate::Timestamp::MIN
    }
    fn monotonic_ns(&mut self) -> u64 {
        0
    }
}

/// One namespace repair.
#[derive(Debug, Clone)]
pub(crate) enum Fix {
    RemoveEntry { dir: u64, name: Vec<u8> },
    Orphan(u64),
    RemoveOrphan(u64),
    RemoveExtent { id: u64, index: u64 },
    SetDirSize { dir: u64, size: u64 },
    SetParent { dir: u64, parent: Option<u64> },
}

/// Keeps, for every object, the first valid entry that reaches it from the
/// root (breadth first, in key order); every other entry goes, every object
/// left without a name goes on the orphan list, and sizes and parent links
/// follow what is kept.
fn plan_namespace_fixes(col: &Collected) -> Vec<Fix> {
    let mut by_dir: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, (dir, _, _, _)) in col.dirents.iter().enumerate() {
        by_dir.entry(*dir).or_default().push(i);
    }
    let valid = |i: usize| {
        col.dirents.get(i).is_some_and(|(dir, _, child, kind)| {
            col.inodes
                .get(dir)
                .is_some_and(|a| a.kind == ObjKind::Directory)
                && col.inodes.get(child).is_some_and(|a| a.kind == *kind)
                && *child != ROOT_ID
                && !col.orphans.contains(child)
        })
    };
    let mut keep = vec![false; col.dirents.len()];
    let mut parent_of: BTreeMap<u64, u64> = BTreeMap::new();
    let mut queue = alloc::collections::VecDeque::from([ROOT_ID]);
    let mut seen: BTreeSet<u64> = BTreeSet::from([ROOT_ID]);
    while let Some(d) = queue.pop_front() {
        for &i in by_dir.get(&d).map(Vec::as_slice).unwrap_or(&[]) {
            let Some((_, _, child, kind)) = col.dirents.get(i) else {
                continue;
            };
            if valid(i) && seen.insert(*child) {
                if let Some(k) = keep.get_mut(i) {
                    *k = true;
                }
                parent_of.insert(*child, d);
                if *kind == ObjKind::Directory {
                    queue.push_back(*child);
                }
            }
        }
    }
    let mut fixes = Vec::new();
    let mut entries: BTreeMap<u64, u64> = BTreeMap::new();
    for (i, (dir, name, _, _)) in col.dirents.iter().enumerate() {
        if keep.get(i).copied().unwrap_or(false) {
            *entries.entry(*dir).or_insert(0) += 1;
        } else {
            fixes.push(Fix::RemoveEntry {
                dir: *dir,
                name: name.clone(),
            });
        }
    }
    for o in &col.orphans {
        if !col.inodes.contains_key(o) {
            fixes.push(Fix::RemoveOrphan(*o));
        }
    }
    for (id, a) in &col.inodes {
        if *id != ROOT_ID && !parent_of.contains_key(id) && !col.orphans.contains(id) {
            fixes.push(Fix::Orphan(*id));
        }
        if a.kind == ObjKind::Directory {
            let n = entries.get(id).copied().unwrap_or(0);
            if a.size != n {
                fixes.push(Fix::SetDirSize { dir: *id, size: n });
            }
            let want = parent_of.get(id).copied();
            if col.parents.get(id).copied() != want {
                fixes.push(Fix::SetParent {
                    dir: *id,
                    parent: want,
                });
            }
        }
    }
    for id in col.parents.keys() {
        if col
            .inodes
            .get(id)
            .is_none_or(|a| a.kind != ObjKind::Directory)
        {
            fixes.push(Fix::SetParent {
                dir: *id,
                parent: None,
            });
        }
    }
    for (id, index, _) in &col.extents {
        let ok = col.inodes.get(id).is_some_and(|a| {
            a.kind != ObjKind::Directory && *index < a.size.div_ceil(BLOCK_SIZE as u64)
        });
        if !ok {
            fixes.push(Fix::RemoveExtent {
                id: *id,
                index: *index,
            });
        }
    }
    fixes
}

/// Writes a new allocator built from the reachable set, then a root record
/// of the next generation pointing to it, with the usual flush order.
fn rebuild_allocator<D: BlockDevice>(
    dev: &mut D,
    rec: RootRecord,
    reachable: &[u64],
    old_alloc: &BTreeSet<u64>,
) -> Result<(), crate::DevError> {
    use crate::format::{N_PAIRS, bitmap_count, encode_alloc_root, encode_bitmap};
    let g = rec
        .generation
        .checked_add(1)
        .ok_or(crate::DevError::Unsupported)?;
    let n = bitmap_count(rec.block_count);
    let is_used = |b: u64| {
        b < ROOT_SLOTS
            || reachable
                .get((b / 64) as usize)
                .is_some_and(|w| w >> (b % 64) & 1 != 0)
    };
    // New blocks for the bitmaps and the root, from blocks no generation
    // that mount could choose references (unreachable now, and not the old
    // allocator's blocks, which are marked reachable).
    let mut fresh = Vec::new();
    let mut b = ROOT_SLOTS;
    while (fresh.len() as u64) < n + 1 && b < rec.block_count {
        if !is_used(b) {
            fresh.push(b);
        }
        b += 1;
    }
    if (fresh.len() as u64) < n + 1 {
        return Err(crate::DevError::IoError);
    }
    let alloc_addr = *fresh.last().ok_or(crate::DevError::IoError)?;
    let fresh_set: BTreeSet<u64> = fresh.iter().copied().collect();
    let used_now = |b: u64| {
        (is_used(b) && !old_alloc.contains(&b)) || fresh_set.contains(&b) || b >= rec.block_count
    };
    let mut writes: Vec<(u64, Box<Block>)> = Vec::new();
    let mut ptrs = Vec::new();
    for i in 0..n {
        let mut bits = vec![0u8; (BITS_PER_BITMAP / 8) as usize];
        for (j, byte) in bits.iter_mut().enumerate() {
            for k in 0..8u64 {
                let blk = i * BITS_PER_BITMAP + j as u64 * 8 + k;
                if used_now(blk) {
                    *byte |= 1 << k;
                }
            }
        }
        let addr = *fresh.get(i as usize).ok_or(crate::DevError::IoError)?;
        let blk = encode_bitmap(rec.volume_id, addr, g, i as u32, &bits)
            .map_err(|_| crate::DevError::IoError)?;
        ptrs.push(BlockPtr {
            addr,
            csum: crate::crc32c::checksum64(&blk),
            generation: g,
        });
        writes.push((addr, Box::new(blk)));
    }
    let ab = encode_alloc_root(rec.volume_id, alloc_addr, g, &ptrs)
        .map_err(|_| crate::DevError::IoError)?;
    let alloc_root = BlockPtr {
        addr: alloc_addr,
        csum: crate::crc32c::checksum64(&ab),
        generation: g,
    };
    writes.push((alloc_addr, Box::new(ab)));
    let refs: Vec<(u64, &Block)> = writes.iter().map(|(a, b)| (*a, &**b)).collect();
    dev.write(&refs)?;
    dev.flush()?;
    let new = RootRecord {
        generation: g,
        alloc_root,
        ..rec
    };
    let rb = new.encode().map_err(|_| crate::DevError::IoError)?;
    let [s0, s1] = RootRecord::slots(g % N_PAIRS);
    dev.write(&[(s0, &rb), (s1, &rb)])?;
    dev.flush()
}

/// What a block of a volume is, from a walk of the selected generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockKind {
    RootSlot,
    /// A tree node at this level (0 = leaf).
    Node(u8),
    Data,
    AllocRoot,
    Bitmap,
}

/// Every block reachable from root record `rec` (tree nodes, data blocks,
/// allocator blocks), with its kind and the block whose pointer leads to it
/// (`None` for blocks the root record points to). Fails on the first block
/// that does not validate. For tests and tools.
pub fn reachable_from<D: BlockDevice + ?Sized>(
    dev: &mut D,
    rec: &RootRecord,
) -> Result<BTreeMap<u64, (BlockKind, Option<u64>)>, String> {
    let mut out = BTreeMap::new();
    let add = |out: &mut BTreeMap<u64, (BlockKind, Option<u64>)>,
               a: u64,
               k: BlockKind,
               p: Option<u64>| {
        if out.insert(a, (k, p)).is_some() {
            Err(format!("block {a} reachable twice"))
        } else {
            Ok(())
        }
    };
    // Explicit stack: (pointer, level, lower, upper, parent).
    type Pending = (BlockPtr, u8, Option<Key>, Option<Key>, Option<u64>);
    let mut stack: Vec<Pending> = vec![(
        rec.tree_root,
        rec.tree_height.saturating_sub(1),
        None,
        None,
        None,
    )];
    let mut buf = [0u8; BLOCK_SIZE];
    while let Some((p, level, lo, up, parent)) = stack.pop() {
        add(&mut out, p.addr, BlockKind::Node(level), parent)?;
        dev.read(p.addr, &mut buf)
            .map_err(|e| format!("block {}: {e:?}", p.addr))?;
        let n = Node::decode(
            &buf,
            &p,
            rec.volume_id,
            rec.block_count,
            level,
            lo.as_ref(),
            up.as_ref(),
        )
        .map_err(|e| format!("node {}: {}", p.addr, e.0))?;
        match n.body {
            Body::Leaf(items) => {
                for (_, v) in items {
                    if let Value::Extent(DataRef::Clean(d)) = v {
                        add(&mut out, d.addr, BlockKind::Data, Some(p.addr))?;
                    }
                }
            }
            Body::Interior(es) => {
                for i in 0..es.len() {
                    let clo = if i > 0 {
                        es.get(i).map(|e| e.0.clone())
                    } else {
                        lo.clone()
                    };
                    let cup = match es.get(i + 1) {
                        Some(e) => Some(e.0.clone()),
                        None => up.clone(),
                    };
                    if let Some((_, Child::Clean(c))) = es.get(i) {
                        stack.push((*c, level.saturating_sub(1), clo, cup, Some(p.addr)));
                    }
                }
            }
        }
    }
    add(&mut out, rec.alloc_root.addr, BlockKind::AllocRoot, None)?;
    dev.read(rec.alloc_root.addr, &mut buf)
        .map_err(|e| format!("allocator root: {e:?}"))?;
    let bitmaps = decode_alloc_root(&buf, &rec.alloc_root, rec.volume_id, rec.block_count)
        .map_err(|e| format!("allocator root: {}", e.0))?;
    for p in bitmaps {
        add(
            &mut out,
            p.addr,
            BlockKind::Bitmap,
            Some(rec.alloc_root.addr),
        )?;
    }
    Ok(out)
}

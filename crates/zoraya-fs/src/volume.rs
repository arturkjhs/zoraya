//! The engine: mount, the request API of ADR-0010, commit groups and the
//! commit protocol of ADR-0011, and the fail-stop state.
//!
//! The engine is single-threaded: the service that wraps it (stage 4)
//! serialises requests, so the order of calls is the completion order of
//! ADR-0010 rule 1. Every request either succeeds completely or fails and
//! changes nothing: the engine snapshots its in-memory state before each
//! mutating request and restores it on any error, including `NoSpace` found
//! after the request was applied in memory.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::device::{BLOCK_SIZE, Block, BlockDevice};
use crate::error::FsError;
use crate::format::{
    Attr, BITS_PER_BITMAP, BlockPtr, Body, Child, DataRef, K_DIRENT, K_EXTENT, K_ORPHAN, Key,
    MAX_FILE_SIZE, MAX_LINK_TARGET, MODE_MASK, Node, ObjKind, ROOT_ID, RootRecord, Value,
    bitmap_count, decode_alloc_root, decode_bitmap, encode_alloc_root, encode_bitmap,
};
use crate::io::{FailReason, Io};
use crate::name::{RelPath, validate_name};
use crate::root::select_root;
use crate::space::Space;
use crate::time::{Clock, Timestamp};
use crate::tree::{Acct, Loader, Tree};

/// Largest write request; larger writes are split by the client library
/// (ADR-0010 rule 2, an assumption there: 1 MiB).
pub const MAX_ATOMIC_WRITE: usize = 1 << 20;
/// Completed requests are committed within this long (ADR-0010 rule 5, an
/// assumption there: 5 s).
pub const T_COMMIT_NS: u64 = 5_000_000_000;
/// A commit group is cut when it would need more blocks than this
/// (ADR-0011 rule 2's dirty-state threshold; 4 MiB).
pub const COMMIT_THRESHOLD_BLOCKS: u64 = 1024;
/// Blocks kept free for requests that only free space, so that removing a
/// file works on a full volume (proposed ADR-0016, item 3).
pub const DELETE_RESERVE: u64 = 64;
/// Object ids are reserved durably in batches of this many (proposed
/// ADR-0016, item 1).
pub const ID_BATCH: u64 = 1024;
/// Default modes for native clients that give none (ADR-0014 rule 6).
pub const DEFAULT_FILE_MODE: u16 = 0o644;
pub const DEFAULT_DIR_MODE: u16 = 0o755;

/// A handle to an open object. Handles are per volume; the service maps
/// them to kernel handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle {
    slot: u32,
    generation: u32,
}

#[derive(Debug, Clone, Copy)]
struct HandleEntry {
    id: u64,
    kind: ObjKind,
    generation: u32,
}

/// How [`Volume::open`] treats the path.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenOptions {
    /// Create a file with this mode if the name does not exist.
    pub create: Option<u16>,
    /// With `create`: fail with `Exists` if the name exists (exclusive
    /// create, ADR-0010 rule 2).
    pub exclusive: bool,
    /// Set the size of an existing file to 0, in the same request.
    pub truncate: bool,
    /// Open a link object itself rather than failing with `IsLink`.
    pub link_itself: bool,
}

/// Modes of [`Volume::rename`] (ADR-0010 rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameMode {
    Replace,
    NoReplace,
    Exchange,
}

/// Modes of [`Volume::publish`] (ADR-0010 rule 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishMode {
    Replace,
    NoReplace,
}

/// A directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub id: u64,
    pub kind: ObjKind,
}

/// Options for [`Volume::mount`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MountOptions {
    pub read_only: bool,
    /// Mount read-write on a device without flush (ADR-0015 rule 5). Only
    /// for tests that never crash.
    pub unsafe_no_flush: bool,
}

/// Options for [`mkfs`].
#[derive(Debug, Clone, Copy)]
pub struct MkfsOptions {
    pub volume_id: u64,
    /// Time of the root directory and the volume's first assigned time.
    pub now: Timestamp,
    pub root_mode: u16,
}

/// The in-memory state a request may change; cloned before each request.
#[derive(Debug, Clone)]
struct State {
    tree: Tree,
    next_id: u64,
    id_limit: u64,
    last_assigned: Timestamp,
    /// The time assigned to the current request, once assigned.
    req_time: Option<Timestamp>,
    /// Whether the open group has changes to commit.
    changed: bool,
}

struct Snapshot {
    st: State,
    dirty_nodes: u64,
    dirty_data: u64,
    freed: usize,
}

/// A mounted volume.
pub struct Volume<D: BlockDevice, C: Clock> {
    io: Io<D>,
    clock: C,
    st: State,
    acct: Acct,
    space: Option<Space>,
    durable: RootRecord,
    read_only: bool,
    failed: Option<FailReason>,
    failure_unreported: bool,
    handles: Vec<Option<HandleEntry>>,
    handle_gen: u32,
    open: BTreeMap<u64, u32>,
    oldest_uncommitted: Option<u64>,
    corrupt_reads: u64,
    commits: u64,
    #[cfg(feature = "instrument")]
    touched: Vec<u64>,
}

/// Creates an empty volume on `dev`: generation 1 with an empty root
/// directory. Writes the tree and allocator, flushes, then writes the root
/// record into both copies of pair 1 and zeros into every other slot, and
/// flushes again.
pub fn mkfs<D: BlockDevice>(dev: &mut D, opts: MkfsOptions) -> Result<(), FsError> {
    use crate::format::{MAX_BLOCKS, ROOT_SLOTS};
    let block_count = dev.block_count().min(MAX_BLOCKS);
    let nbitmaps = bitmap_count(block_count);
    // Root slots, one leaf, the bitmaps and the allocator root.
    if block_count < ROOT_SLOTS + 2 + nbitmaps + DELETE_RESERVE {
        return Err(FsError::NoSpace);
    }
    if opts.root_mode & !MODE_MASK != 0 || !opts.now.is_valid() {
        return Err(FsError::InvalidArgument);
    }
    let generation = 1;
    let vid = opts.volume_id;
    let leaf_addr = ROOT_SLOTS;
    let root_attr = Attr {
        id: ROOT_ID,
        kind: ObjKind::Directory,
        size: 0,
        mtime: opts.now,
        ctime: opts.now,
        posix_mode: opts.root_mode,
    };
    let leaf = Node {
        level: 0,
        body: Body::Leaf(vec![(Key::inode(ROOT_ID), Value::Inode(root_attr))]),
    };
    let leaf_block = leaf
        .encode(vid, leaf_addr, generation)
        .map_err(|_| FsError::Internal)?;
    let tree_root = BlockPtr {
        addr: leaf_addr,
        csum: crate::crc32c::checksum64(&leaf_block),
        generation,
    };
    let mut writes: Vec<(u64, Box<Block>)> = vec![(leaf_addr, Box::new(leaf_block))];
    // Used: root slots, the leaf, the bitmaps, the allocator root.
    let first_bitmap = leaf_addr + 1;
    let alloc_addr = first_bitmap + nbitmaps;
    let mut used = vec![0u8; (nbitmaps * BITS_PER_BITMAP / 8) as usize];
    for b in 0..nbitmaps * BITS_PER_BITMAP {
        if (b <= alloc_addr || b >= block_count)
            && let Some(byte) = used.get_mut((b / 8) as usize)
        {
            *byte |= 1 << (b % 8);
        }
    }
    let mut bitmaps = Vec::new();
    let per = (BITS_PER_BITMAP / 8) as usize;
    for i in 0..nbitmaps {
        let addr = first_bitmap + i;
        let bits = used
            .get(i as usize * per..(i as usize + 1) * per)
            .ok_or(FsError::Internal)?;
        let blk =
            encode_bitmap(vid, addr, generation, i as u32, bits).map_err(|_| FsError::Internal)?;
        bitmaps.push(BlockPtr {
            addr,
            csum: crate::crc32c::checksum64(&blk),
            generation,
        });
        writes.push((addr, Box::new(blk)));
    }
    let ablk =
        encode_alloc_root(vid, alloc_addr, generation, &bitmaps).map_err(|_| FsError::Internal)?;
    let alloc_root = BlockPtr {
        addr: alloc_addr,
        csum: crate::crc32c::checksum64(&ablk),
        generation,
    };
    writes.push((alloc_addr, Box::new(ablk)));
    let refs: Vec<(u64, &Block)> = writes.iter().map(|(a, b)| (*a, &**b)).collect();
    dev.write(&refs).map_err(|_| FsError::StorageFailed)?;
    dev.flush().map_err(|_| FsError::StorageFailed)?;
    let rec = RootRecord {
        volume_id: vid,
        block_count,
        generation,
        tree_root,
        tree_height: 1,
        alloc_root,
        last_assigned: opts.now,
        id_limit: crate::format::FIRST_FREE_ID,
    };
    let rb = rec.encode().map_err(|_| FsError::Internal)?;
    let zero = [0u8; BLOCK_SIZE];
    let mut slots: Vec<(u64, &Block)> = Vec::new();
    for s in 0..ROOT_SLOTS {
        slots.push((
            s,
            if RootRecord::slots(generation % crate::format::N_PAIRS).contains(&s) {
                &rb
            } else {
                &zero
            },
        ));
    }
    dev.write(&slots).map_err(|_| FsError::StorageFailed)?;
    dev.flush().map_err(|_| FsError::StorageFailed)?;
    Ok(())
}

impl<D: BlockDevice, C: Clock> Volume<D, C> {
    /// Mounts the volume on `dev` (ADR-0011 rule 4): selects the newest valid
    /// root record, with no replay. A read-write mount needs a device with
    /// flush (ADR-0015 rule 5) and frees every object on the orphan list
    /// (ADR-0011 rule 7), committing if there were any.
    pub fn mount(mut dev: D, clock: C, opts: MountOptions) -> Result<Self, FsError> {
        if !opts.read_only && !dev.flush_supported() && !opts.unsafe_no_flush {
            return Err(FsError::ReadOnly);
        }
        let sel = select_root(&mut dev).map_err(|_| FsError::Corrupt)?;
        let rec = sel.record;
        let mut io = Io::new(dev, rec.volume_id, rec.block_count);
        // The selected generation's tree root must validate; mount never
        // falls back to an older generation (ADR-0011 rule 4).
        io.load_node(&rec.tree_root, rec.tree_height - 1, None, None)?;
        io.meta_error = None;
        let mut v = Volume {
            io,
            clock,
            st: State {
                tree: Tree {
                    root: Child::Clean(rec.tree_root),
                    height: rec.tree_height,
                },
                next_id: rec.id_limit,
                id_limit: rec.id_limit,
                last_assigned: rec.last_assigned,
                req_time: None,
                changed: false,
            },
            acct: Acct::default(),
            space: None,
            durable: rec,
            read_only: opts.read_only,
            failed: None,
            failure_unreported: false,
            handles: Vec::new(),
            handle_gen: 0,
            open: BTreeMap::new(),
            oldest_uncommitted: None,
            corrupt_reads: 0,
            commits: 0,
            #[cfg(feature = "instrument")]
            touched: Vec::new(),
        };
        if !sel.ring_consistent {
            // An older pair invalid while a newer one is valid: corrupt;
            // read-only and reported (ADR-0011 rule 4).
            v.read_only = true;
            v.fail(FailReason::MetadataCorrupt);
        }
        if !v.read_only {
            match v.load_space() {
                Ok(s) => v.space = Some(s),
                Err(_) => {
                    let r =
                        v.io.meta_error
                            .take()
                            .unwrap_or(FailReason::MetadataCorrupt);
                    v.fail(r);
                }
            }
        }
        if !v.read_only && v.failed.is_none() {
            v.free_orphans()?;
        }
        Ok(v)
    }

    fn load_space(&mut self) -> Result<Space, FsError> {
        let rec = self.durable;
        let ab = self
            .io
            .read_block(rec.alloc_root.addr)
            .map_err(|_| self.io.meta_fail(FailReason::MetadataRead))?;
        let bitmaps = decode_alloc_root(&ab, &rec.alloc_root, rec.volume_id, rec.block_count)
            .map_err(|_| self.io.meta_fail(FailReason::MetadataCorrupt))?;
        let mut bits = Vec::with_capacity((bitmaps.len() as u64 * BITS_PER_BITMAP / 8) as usize);
        for (i, p) in bitmaps.iter().enumerate() {
            let bb = self
                .io
                .read_block(p.addr)
                .map_err(|_| self.io.meta_fail(FailReason::MetadataRead))?;
            let b = decode_bitmap(&bb, p, rec.volume_id, rec.block_count, i as u32)
                .map_err(|_| self.io.meta_fail(FailReason::MetadataCorrupt))?;
            bits.extend_from_slice(b);
        }
        Ok(Space::from_bits(
            rec.block_count,
            &bits,
            bitmaps,
            rec.alloc_root,
        ))
    }

    /// Frees every object on the orphan list and commits (INV-0010-6).
    fn free_orphans(&mut self) -> Result<(), FsError> {
        let mut any = false;
        loop {
            let first = self.mutate(true, false, &mut |v| {
                let start = Key {
                    id: 0,
                    kind: K_ORPHAN,
                    off: 0,
                    name: Vec::new(),
                };
                match v.tnext(&start, true)? {
                    Some((k, _)) if k.id == 0 && k.kind == K_ORPHAN => {
                        v.delete_object(k.off)?;
                        v.st.changed = true;
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            });
            match first {
                Ok(true) => any = true,
                Ok(false) => break,
                Err(FsError::NoSpace) => break, // left for a later mount
                Err(e) => return Err(e),
            }
        }
        if any { self.commit() } else { Ok(()) }
    }

    // -----------------------------------------------------------------------
    // State, failure and accounting.

    /// `None` while the volume is healthy; the reason once it is `Failed`.
    pub fn failure(&self) -> Option<FailReason> {
        self.failed
    }

    /// Returns the failure once, the first time it is called after the
    /// volume entered `Failed`: the report to the supervisor of ADR-0010
    /// rule 6. The service forwards it on its supervisor channel.
    pub fn take_failure_report(&mut self) -> Option<FailReason> {
        if self.failure_unreported {
            self.failure_unreported = false;
            self.failed
        } else {
            None
        }
    }

    /// Data reads that failed their checksum (ADR-0012 rule 3's counter).
    pub fn corrupt_reads(&self) -> u64 {
        self.corrupt_reads
    }

    /// Whether the volume is mounted read-only.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Commits completed so far in this mount.
    pub fn commits(&self) -> u64 {
        self.commits
    }

    /// Device reads issued so far.
    pub fn device_reads(&self) -> u64 {
        self.io.reads
    }

    /// The id the next created object will get (for tests that check ids).
    pub fn next_id(&self) -> u64 {
        self.st.next_id
    }

    /// Empties the node cache, so that clean nodes are read again (memory
    /// pressure; also used by tests).
    pub fn drop_caches(&mut self) {
        self.io.cache_clear();
    }

    /// The generation of the newest durable root record.
    pub fn durable_generation(&self) -> u64 {
        self.durable.generation
    }

    /// The newest durable root record.
    pub fn durable_root(&self) -> RootRecord {
        self.durable
    }

    /// The device, for tests that inspect it.
    pub fn device(&self) -> &D {
        &self.io.dev
    }

    pub fn device_mut(&mut self) -> &mut D {
        &mut self.io.dev
    }

    pub fn clock_mut(&mut self) -> &mut C {
        &mut self.clock
    }

    /// Object ids whose attribute records were read since the last call.
    #[cfg(feature = "instrument")]
    pub fn take_touched(&mut self) -> Vec<u64> {
        core::mem::take(&mut self.touched)
    }

    /// Blocks the open group needs at commit (tree nodes and data) and the
    /// count found by walking the dirty tree, which must be equal.
    pub fn debug_dirty_counts(&self) -> (u64, u64) {
        let mut walked = 0;
        count_dirty(&self.st.tree.root, &mut walked);
        (self.acct.dirty_nodes + self.acct.dirty_data, walked)
    }

    fn fail(&mut self, r: FailReason) {
        if self.failed.is_none() {
            self.failed = Some(r);
            self.failure_unreported = true;
        }
    }

    fn writable(&self) -> Result<(), FsError> {
        if self.failed.is_some() {
            Err(FsError::StorageFailed)
        } else if self.read_only {
            Err(FsError::ReadOnly)
        } else {
            Ok(())
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            st: self.st.clone(),
            dirty_nodes: self.acct.dirty_nodes,
            dirty_data: self.acct.dirty_data,
            freed: self.acct.freed.len(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.st = s.st;
        self.acct.dirty_nodes = s.dirty_nodes;
        self.acct.dirty_data = s.dirty_data;
        self.acct.freed.truncate(s.freed);
    }

    /// Whether the open group's commit would fit in free space.
    fn space_ok(&self, freeing: bool) -> bool {
        let Some(sp) = &self.space else { return false };
        let need = self.acct.dirty_nodes + self.acct.dirty_data + sp.overhead();
        let reserve = if freeing { 0 } else { DELETE_RESERVE };
        need + reserve <= sp.free()
    }

    /// Runs a mutating request atomically. `freeing` requests may use the
    /// delete reserve. `creates` requests need one fresh object id.
    fn mutate<R>(
        &mut self,
        freeing: bool,
        creates: bool,
        f: &mut dyn FnMut(&mut Self) -> Result<R, FsError>,
    ) -> Result<R, FsError> {
        self.writable()?;
        if self.acct.dirty_nodes + self.acct.dirty_data >= COMMIT_THRESHOLD_BLOCKS {
            self.commit()?;
        }
        if creates && self.st.next_id >= self.st.id_limit {
            // Reserve the next batch of ids durably before handing any out.
            self.st.id_limit = self.st.next_id.saturating_add(ID_BATCH);
            self.commit()?;
        }
        for attempt in 0..2 {
            let snap = self.snapshot();
            self.st.req_time = None;
            let r = f(self).and_then(|v| {
                if self.space_ok(freeing) {
                    Ok(v)
                } else {
                    Err(FsError::NoSpace)
                }
            });
            match r {
                Ok(v) => {
                    self.st.req_time = None;
                    if self.st.changed && self.oldest_uncommitted.is_none() {
                        self.oldest_uncommitted = Some(self.clock.monotonic_ns());
                    }
                    return Ok(v);
                }
                Err(e) => {
                    self.restore(snap);
                    if let Some(reason) = self.io.meta_error.take() {
                        self.fail(reason);
                        return Err(e);
                    }
                    if e == FsError::Internal {
                        self.fail(FailReason::Internal);
                        return Err(e);
                    }
                    // Space freed by the open group becomes reusable once it
                    // is committed; try once more after a commit.
                    if e == FsError::NoSpace
                        && attempt == 0
                        && (self.st.changed || !self.acct.freed.is_empty())
                    {
                        self.commit()?;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        Err(FsError::NoSpace)
    }

    /// The time for the current request: `max(wall clock, last + 1 ns)`,
    /// assigned once per request (ADR-0014 rule 3).
    fn now(&mut self) -> Timestamp {
        if let Some(t) = self.st.req_time {
            return t;
        }
        let w = self.clock.wall();
        let t = if w > self.st.last_assigned {
            w
        } else {
            self.st.last_assigned.succ()
        };
        self.st.last_assigned = t;
        self.st.req_time = Some(t);
        self.st.changed = true;
        t
    }

    // -----------------------------------------------------------------------
    // Tree access.

    fn tget(&mut self, k: &Key) -> Result<Option<Value>, FsError> {
        self.st.tree.get(&mut self.io, k)
    }

    fn tins(&mut self, k: Key, v: Value) -> Result<Option<Value>, FsError> {
        self.st.tree.insert(&mut self.io, &mut self.acct, k, v)
    }

    fn tdel(&mut self, k: &Key) -> Result<Option<Value>, FsError> {
        self.st.tree.remove(&mut self.io, &mut self.acct, k)
    }

    fn tnext(&mut self, k: &Key, inclusive: bool) -> Result<Option<(Key, Value)>, FsError> {
        self.st.tree.next(&mut self.io, k, inclusive)
    }

    /// A metadata inconsistency found by the engine itself.
    fn inconsistent(&mut self) -> FsError {
        self.io.meta_fail(FailReason::MetadataCorrupt)
    }

    fn inode(&mut self, id: u64) -> Result<Attr, FsError> {
        #[cfg(feature = "instrument")]
        self.touched.push(id);
        match self.tget(&Key::inode(id))? {
            Some(Value::Inode(a)) => Ok(a),
            _ => Err(self.inconsistent()),
        }
    }

    fn put_inode(&mut self, a: Attr) -> Result<(), FsError> {
        self.tins(Key::inode(a.id), Value::Inode(a)).map(|_| ())
    }

    fn entry(&mut self, dir: u64, name: &[u8]) -> Result<Option<(u64, ObjKind)>, FsError> {
        match self.tget(&Key::dirent(dir, name))? {
            None => Ok(None),
            Some(Value::Dirent { child, kind }) => Ok(Some((child, kind))),
            Some(_) => Err(self.inconsistent()),
        }
    }

    fn is_orphan(&mut self, id: u64) -> Result<bool, FsError> {
        Ok(self.tget(&Key::orphan(id))?.is_some())
    }

    /// Releases a data reference that the tree no longer holds.
    fn drop_data(&mut self, v: Option<Value>) {
        match v {
            Some(Value::Extent(DataRef::Clean(p))) => self.acct.freed.push(p.addr),
            Some(Value::Extent(DataRef::Dirty(_))) => self.acct.dirty_data -= 1,
            _ => {}
        }
    }

    fn set_extent(&mut self, id: u64, index: u64, data: Block) -> Result<(), FsError> {
        self.acct.dirty_data += 1;
        let old = self.tins(
            Key::extent(id, index),
            Value::Extent(DataRef::Dirty(Rc::new(data))),
        )?;
        self.drop_data(old);
        Ok(())
    }

    /// Block `index` of object `id`: its content, or `None` for a hole.
    fn get_block(&mut self, id: u64, index: u64) -> Result<Option<Block>, FsError> {
        match self.tget(&Key::extent(id, index))? {
            None => Ok(None),
            Some(Value::Extent(DataRef::Dirty(b))) => Ok(Some(*b)),
            Some(Value::Extent(DataRef::Clean(p))) => match self.io.read_data(&p) {
                Ok(b) => Ok(Some(*b)),
                Err(e) => {
                    self.corrupt_reads += 1;
                    Err(e)
                }
            },
            Some(_) => Err(self.inconsistent()),
        }
    }

    /// Removes every extent of `id` with index `>= from`.
    fn remove_extents_from(&mut self, id: u64, from: u64) -> Result<(), FsError> {
        loop {
            match self.tnext(&Key::extent(id, from), true)? {
                Some((k, _)) if k.id == id && k.kind == K_EXTENT => {
                    let old = self.tdel(&k)?;
                    self.drop_data(old);
                }
                _ => return Ok(()),
            }
        }
    }

    /// Removes an object entirely: attributes, parent link, data, orphan
    /// entry.
    fn delete_object(&mut self, id: u64) -> Result<(), FsError> {
        self.tdel(&Key::inode(id))?;
        self.tdel(&Key::parent(id))?;
        self.remove_extents_from(id, 0)?;
        self.tdel(&Key::orphan(id))?;
        Ok(())
    }

    /// The object lost its last name: free it, or keep it on the orphan
    /// list while handles are open (ADR-0011 rule 7).
    fn unname(&mut self, id: u64, kind: ObjKind, t: Timestamp) -> Result<(), FsError> {
        if kind == ObjKind::Directory {
            self.tdel(&Key::parent(id))?;
        }
        if self.open.get(&id).copied().unwrap_or(0) > 0 {
            let mut a = self.inode(id)?;
            a.ctime = t;
            self.put_inode(a)?;
            self.tins(Key::orphan(id), Value::Orphan)?;
            Ok(())
        } else {
            self.delete_object(id)
        }
    }

    fn adjust_dir(&mut self, dir: u64, delta: i64, t: Timestamp) -> Result<(), FsError> {
        let mut a = self.inode(dir)?;
        a.size = a
            .size
            .checked_add_signed(delta)
            .ok_or_else(|| self.inconsistent())?;
        a.mtime = t;
        a.ctime = t;
        self.put_inode(a)
    }

    fn link_target(&mut self, id: u64) -> Result<String, FsError> {
        let a = self.inode(id)?;
        let len = a.size as usize;
        if len > MAX_LINK_TARGET {
            return Err(self.inconsistent());
        }
        let b = self.get_block(id, 0)?.unwrap_or([0; BLOCK_SIZE]);
        let s = b.get(..len).ok_or(FsError::Internal)?;
        core::str::from_utf8(s)
            .map(ToString::to_string)
            .map_err(|_| FsError::Corrupt)
    }

    // -----------------------------------------------------------------------
    // Path resolution (ADR-0013 rules 6 and 7).

    fn handle(&self, h: Handle) -> Result<HandleEntry, FsError> {
        match self.handles.get(h.slot as usize) {
            Some(Some(e)) if e.generation == h.generation => Ok(*e),
            _ => Err(FsError::BadHandle),
        }
    }

    fn dir_handle(&self, h: Handle) -> Result<u64, FsError> {
        let e = self.handle(h)?;
        if e.kind != ObjKind::Directory {
            return Err(FsError::NotDir);
        }
        Ok(e.id)
    }

    /// Walks the first `n` components of `path` from directory `start`. Each
    /// must be a directory; a link object stops the walk with `IsLink`.
    fn walk(&mut self, start: u64, path: &str, n: usize) -> Result<u64, FsError> {
        let mut cur = start;
        let mut pos = 0usize;
        for comp in path.split('/').take(n) {
            pos += comp.len() + 1;
            let (child, kind) = self.entry(cur, comp.as_bytes())?.ok_or(FsError::NotFound)?;
            match kind {
                ObjKind::Directory => cur = child,
                ObjKind::Link => {
                    let target = self.link_target(child)?;
                    let remaining = path.get(pos..).unwrap_or("").to_string();
                    return Err(FsError::IsLink { remaining, target });
                }
                ObjKind::File => return Err(FsError::NotDir),
            }
        }
        Ok(cur)
    }

    /// The directory holding the last component of `path`, and that name.
    fn parent_of<'p>(&mut self, dir: Handle, path: &'p str) -> Result<(u64, &'p str), FsError> {
        let start = self.dir_handle(dir)?;
        let rp = RelPath::new(path)?;
        let n = rp.components().count();
        let parent = self.walk(start, path, n - 1)?;
        let (_, name) = rp.split_last();
        Ok((parent, name.as_str()))
    }

    /// The object `path` names, without following a final link object.
    fn lookup(&mut self, dir: Handle, path: &str) -> Result<(u64, ObjKind), FsError> {
        let (parent, name) = self.parent_of(dir, path)?;
        self.entry(parent, name.as_bytes())?
            .ok_or(FsError::NotFound)
    }

    fn new_handle(&mut self, id: u64, kind: ObjKind) -> Handle {
        self.handle_gen = self.handle_gen.wrapping_add(1);
        let generation = self.handle_gen;
        let e = Some(HandleEntry {
            id,
            kind,
            generation,
        });
        *self.open.entry(id).or_insert(0) += 1;
        if let Some(i) = self.handles.iter().position(Option::is_none) {
            if let Some(slot) = self.handles.get_mut(i) {
                *slot = e;
            }
            Handle {
                slot: i as u32,
                generation,
            }
        } else {
            self.handles.push(e);
            Handle {
                slot: (self.handles.len() - 1) as u32,
                generation,
            }
        }
    }

    // -----------------------------------------------------------------------
    // Requests (ADR-0010 rule 2).

    /// A handle to the volume's root directory.
    pub fn root(&mut self) -> Handle {
        self.new_handle(ROOT_ID, ObjKind::Directory)
    }

    /// Opens, and with `create` creates, the object `path` names.
    pub fn open(&mut self, dir: Handle, path: &str, o: OpenOptions) -> Result<Handle, FsError> {
        if o.create.is_some() || o.truncate {
            self.writable()?;
        }
        let (id, kind) = if o.create.is_some() || o.truncate {
            let mut f = |v: &mut Self| v.open_mut(dir, path, o);
            self.mutate(false, o.create.is_some(), &mut f)?
        } else {
            let (id, kind) = self
                .lookup(dir, path)
                .inspect_err(|_| self.note_meta_error())?;
            if kind == ObjKind::Link && !o.link_itself {
                let target = self
                    .link_target(id)
                    .inspect_err(|_| self.note_meta_error())?;
                return Err(FsError::IsLink {
                    remaining: String::new(),
                    target,
                });
            }
            (id, kind)
        };
        Ok(self.new_handle(id, kind))
    }

    /// A read-only request hit a metadata failure: enter `Failed`.
    fn note_meta_error(&mut self) {
        if let Some(r) = self.io.meta_error.take() {
            self.fail(r);
        }
    }

    fn open_mut(
        &mut self,
        dir: Handle,
        path: &str,
        o: OpenOptions,
    ) -> Result<(u64, ObjKind), FsError> {
        let (parent, name) = self.parent_of(dir, path)?;
        match self.entry(parent, name.as_bytes())? {
            Some((id, kind)) => {
                if o.create.is_some() && o.exclusive {
                    return Err(FsError::Exists);
                }
                if kind == ObjKind::Link && !o.link_itself {
                    let target = self.link_target(id)?;
                    return Err(FsError::IsLink {
                        remaining: String::new(),
                        target,
                    });
                }
                if o.truncate {
                    match kind {
                        ObjKind::Directory => return Err(FsError::IsDir),
                        ObjKind::Link => return Err(FsError::InvalidArgument),
                        ObjKind::File => self.resize(id, 0)?,
                    }
                }
                Ok((id, kind))
            }
            None => {
                let Some(mode) = o.create else {
                    return Err(FsError::NotFound);
                };
                let id = self.create_obj(parent, name, ObjKind::File, mode, None)?;
                Ok((id, ObjKind::File))
            }
        }
    }

    fn check_mode(mode: u16) -> Result<(), FsError> {
        if mode & !MODE_MASK != 0 {
            Err(FsError::InvalidArgument)
        } else {
            Ok(())
        }
    }

    fn create_obj(
        &mut self,
        parent: u64,
        name: &str,
        kind: ObjKind,
        mode: u16,
        target: Option<&str>,
    ) -> Result<u64, FsError> {
        Self::check_mode(mode)?;
        if self.is_orphan(parent)? {
            return Err(FsError::NotFound);
        }
        if self.entry(parent, name.as_bytes())?.is_some() {
            return Err(FsError::Exists);
        }
        let id = self.st.next_id;
        if id >= self.st.id_limit {
            return Err(FsError::Internal);
        }
        self.st.next_id += 1;
        let t = self.now();
        let size = target.map_or(0, |s| s.len() as u64);
        self.put_inode(Attr {
            id,
            kind,
            size,
            mtime: t,
            ctime: t,
            posix_mode: mode,
        })?;
        if kind == ObjKind::Directory {
            self.tins(Key::parent(id), Value::Parent(parent))?;
        }
        if let Some(s) = target
            && !s.is_empty()
        {
            let mut b = [0u8; BLOCK_SIZE];
            b.get_mut(..s.len())
                .ok_or(FsError::InvalidArgument)?
                .copy_from_slice(s.as_bytes());
            self.set_extent(id, 0, b)?;
        }
        self.tins(
            Key::dirent(parent, name.as_bytes()),
            Value::Dirent { child: id, kind },
        )?;
        self.adjust_dir(parent, 1, t)?;
        Ok(id)
    }

    /// Creates a directory.
    pub fn mkdir(&mut self, dir: Handle, path: &str, mode: u16) -> Result<(), FsError> {
        self.writable()?;
        self.mutate(false, true, &mut |v| {
            let (parent, name) = v.parent_of(dir, path)?;
            v.create_obj(parent, name, ObjKind::Directory, mode, None)
                .map(|_| ())
        })
    }

    /// Creates a link object holding `target` (ADR-0013 rule 7).
    pub fn symlink(&mut self, dir: Handle, path: &str, target: &str) -> Result<(), FsError> {
        self.writable()?;
        if target.len() > MAX_LINK_TARGET {
            return Err(FsError::InvalidArgument);
        }
        self.mutate(false, true, &mut |v| {
            let (parent, name) = v.parent_of(dir, path)?;
            v.create_obj(parent, name, ObjKind::Link, 0o777, Some(target))
                .map(|_| ())
        })
    }

    /// Removes a name of a file or link object.
    pub fn unlink(&mut self, dir: Handle, path: &str) -> Result<(), FsError> {
        self.writable()?;
        self.mutate(true, false, &mut |v| {
            let (parent, name) = v.parent_of(dir, path)?;
            let (id, kind) = v.entry(parent, name.as_bytes())?.ok_or(FsError::NotFound)?;
            if kind == ObjKind::Directory {
                return Err(FsError::IsDir);
            }
            let t = v.now();
            v.tdel(&Key::dirent(parent, name.as_bytes()))?;
            v.adjust_dir(parent, -1, t)?;
            v.unname(id, kind, t)
        })
    }

    /// Removes an empty directory.
    pub fn rmdir(&mut self, dir: Handle, path: &str) -> Result<(), FsError> {
        self.writable()?;
        self.mutate(true, false, &mut |v| {
            let (parent, name) = v.parent_of(dir, path)?;
            let (id, kind) = v.entry(parent, name.as_bytes())?.ok_or(FsError::NotFound)?;
            if kind != ObjKind::Directory {
                return Err(FsError::NotDir);
            }
            if v.inode(id)?.size != 0 {
                return Err(FsError::NotEmpty);
            }
            let t = v.now();
            v.tdel(&Key::dirent(parent, name.as_bytes()))?;
            v.adjust_dir(parent, -1, t)?;
            v.unname(id, kind, t)
        })
    }

    /// Whether directory `dir` is `anc` or below it.
    fn is_within(&mut self, mut dir: u64, anc: u64) -> Result<bool, FsError> {
        // Bounded: each step goes to a distinct directory on a sane volume,
        // and a volume holds fewer directories than this (each needs an item
        // of at least 20 bytes in some block); a crafted parent cycle is
        // caught by the limit.
        let limit = self.durable.block_count.saturating_mul(256);
        for _ in 0..=limit {
            if dir == anc {
                return Ok(true);
            }
            if dir == ROOT_ID {
                return Ok(false);
            }
            dir = match self.tget(&Key::parent(dir))? {
                Some(Value::Parent(p)) => p,
                _ => return Err(self.inconsistent()),
            };
        }
        Err(self.inconsistent())
    }

    /// Renames within the volume (ADR-0010 rule 2, ADR-0013 rule 5).
    pub fn rename(
        &mut self,
        sdir: Handle,
        spath: &str,
        ddir: Handle,
        dpath: &str,
        mode: RenameMode,
    ) -> Result<(), FsError> {
        self.writable()?;
        self.mutate(true, false, &mut |v| {
            let (sp, sn) = v.parent_of(sdir, spath)?;
            let (dp, dn) = v.parent_of(ddir, dpath)?;
            let (sid, skind) = v.entry(sp, sn.as_bytes())?.ok_or(FsError::NotFound)?;
            if sp == dp && sn == dn {
                return Ok(());
            }
            let dst = v.entry(dp, dn.as_bytes())?;
            match (mode, dst) {
                (RenameMode::NoReplace, Some(_)) => return Err(FsError::Exists),
                (RenameMode::Exchange, None) => return Err(FsError::NotFound),
                (RenameMode::Replace, Some((did, dkind))) => {
                    if skind == ObjKind::Directory && dkind != ObjKind::Directory {
                        return Err(FsError::NotDir);
                    }
                    if skind != ObjKind::Directory && dkind == ObjKind::Directory {
                        return Err(FsError::IsDir);
                    }
                    if dkind == ObjKind::Directory && v.inode(did)?.size != 0 {
                        return Err(FsError::NotEmpty);
                    }
                }
                _ => {}
            }
            if v.is_orphan(dp)? {
                return Err(FsError::NotFound);
            }
            if skind == ObjKind::Directory && v.is_within(dp, sid)? {
                return Err(FsError::InvalidArgument);
            }
            if mode == RenameMode::Exchange
                && let Some((did, ObjKind::Directory)) = dst
                && v.is_within(sp, did)?
            {
                return Err(FsError::InvalidArgument);
            }
            let t = v.now();
            match (mode, dst) {
                (RenameMode::Exchange, Some((did, dkind))) => {
                    v.tins(
                        Key::dirent(sp, sn.as_bytes()),
                        Value::Dirent {
                            child: did,
                            kind: dkind,
                        },
                    )?;
                    v.tins(
                        Key::dirent(dp, dn.as_bytes()),
                        Value::Dirent {
                            child: sid,
                            kind: skind,
                        },
                    )?;
                    for (id, kind, parent) in [(sid, skind, dp), (did, dkind, sp)] {
                        if kind == ObjKind::Directory {
                            v.tins(Key::parent(id), Value::Parent(parent))?;
                        }
                        let mut a = v.inode(id)?;
                        a.ctime = t;
                        v.put_inode(a)?;
                    }
                    v.adjust_dir(sp, 0, t)?;
                    v.adjust_dir(dp, 0, t)?;
                }
                _ => {
                    if let Some((did, dkind)) = dst {
                        v.unname(did, dkind, t)?;
                    }
                    v.tdel(&Key::dirent(sp, sn.as_bytes()))?;
                    v.tins(
                        Key::dirent(dp, dn.as_bytes()),
                        Value::Dirent {
                            child: sid,
                            kind: skind,
                        },
                    )?;
                    if skind == ObjKind::Directory {
                        v.tins(Key::parent(sid), Value::Parent(dp))?;
                    }
                    let mut a = v.inode(sid)?;
                    a.ctime = t;
                    v.put_inode(a)?;
                    v.adjust_dir(sp, -1, t)?;
                    v.adjust_dir(dp, if dst.is_some() { 0 } else { 1 }, t)?;
                }
            }
            Ok(())
        })
    }

    /// Sets the advisory mode and/or an explicit modification time
    /// (ADR-0014 rules 3 and 6). `ctime` gets a newly assigned time.
    pub fn set_attr(
        &mut self,
        h: Handle,
        mode: Option<u16>,
        mtime: Option<Timestamp>,
    ) -> Result<(), FsError> {
        self.writable()?;
        let e = self.handle(h)?;
        if let Some(m) = mode {
            Self::check_mode(m)?;
        }
        if mtime.is_some_and(|t| !t.is_valid()) {
            return Err(FsError::InvalidArgument);
        }
        self.mutate(false, false, &mut |v| {
            let mut a = v.inode(e.id)?;
            a.ctime = v.now();
            if let Some(m) = mode {
                a.posix_mode = m;
            }
            if let Some(t) = mtime {
                a.mtime = t;
            }
            v.put_inode(a)
        })
    }

    fn resize(&mut self, id: u64, size: u64) -> Result<(), FsError> {
        let mut a = self.inode(id)?;
        let bs = BLOCK_SIZE as u64;
        if size < a.size {
            self.remove_extents_from(id, size.div_ceil(bs))?;
            let tail = (size % bs) as usize;
            if tail != 0
                && let Some(mut b) = self.get_block(id, size / bs)?
            {
                b.get_mut(tail..).ok_or(FsError::Internal)?.fill(0);
                self.set_extent(id, size / bs, b)?;
            }
        }
        let t = self.now();
        a.size = size;
        a.mtime = t;
        a.ctime = t;
        self.put_inode(a)
    }

    /// Sets a file's size; new space reads as zeros.
    pub fn set_size(&mut self, h: Handle, size: u64) -> Result<(), FsError> {
        self.writable()?;
        let e = self.handle(h)?;
        match e.kind {
            ObjKind::Directory => return Err(FsError::IsDir),
            ObjKind::Link => return Err(FsError::InvalidArgument),
            ObjKind::File => {}
        }
        if size > MAX_FILE_SIZE {
            return Err(FsError::InvalidArgument);
        }
        self.mutate(true, false, &mut |v| v.resize(e.id, size))
    }

    /// Writes `data` at `offset`; atomic up to [`MAX_ATOMIC_WRITE`] bytes.
    /// An empty write changes nothing.
    pub fn write(&mut self, h: Handle, offset: u64, data: &[u8]) -> Result<usize, FsError> {
        self.writable()?;
        let e = self.handle(h)?;
        match e.kind {
            ObjKind::Directory => return Err(FsError::IsDir),
            ObjKind::Link => return Err(FsError::InvalidArgument),
            ObjKind::File => {}
        }
        if data.len() > MAX_ATOMIC_WRITE {
            return Err(FsError::InvalidArgument);
        }
        let end = offset
            .checked_add(data.len() as u64)
            .filter(|&e| e <= MAX_FILE_SIZE)
            .ok_or(FsError::InvalidArgument)?;
        if data.is_empty() {
            self.writable()?;
            return Ok(0);
        }
        self.mutate(false, false, &mut |v| {
            let bs = BLOCK_SIZE as u64;
            let mut a = v.inode(e.id)?;
            let mut pos = offset;
            while pos < end {
                let index = pos / bs;
                let within = (pos % bs) as usize;
                let n = core::cmp::min(BLOCK_SIZE - within, (end - pos) as usize);
                let mut b = if within == 0 && n == BLOCK_SIZE {
                    [0u8; BLOCK_SIZE]
                } else {
                    v.get_block(e.id, index)?.unwrap_or([0; BLOCK_SIZE])
                };
                let src_off = (pos - offset) as usize;
                let src = data.get(src_off..src_off + n).ok_or(FsError::Internal)?;
                b.get_mut(within..within + n)
                    .ok_or(FsError::Internal)?
                    .copy_from_slice(src);
                v.set_extent(e.id, index, b)?;
                pos += n as u64;
            }
            let t = v.now();
            a.size = a.size.max(end);
            a.mtime = t;
            a.ctime = t;
            v.put_inode(a)?;
            Ok(data.len())
        })
    }

    /// Reads from `offset` into `buf`; returns the bytes read (0 at or past
    /// the end). A data block that fails its checksum makes this read fail
    /// with `Corrupt` and changes nothing else (ADR-0012 rule 3).
    pub fn read(&mut self, h: Handle, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let e = self.handle(h)?;
        if e.kind == ObjKind::Directory {
            return Err(FsError::IsDir);
        }
        let r = self.read_inner(e.id, offset, buf);
        self.note_meta_error();
        r
    }

    fn read_inner(&mut self, id: u64, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let a = self.inode(id)?;
        if offset >= a.size {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len() as u64, a.size - offset) as usize;
        let bs = BLOCK_SIZE as u64;
        let mut done = 0usize;
        while done < n {
            let pos = offset + done as u64;
            let within = (pos % bs) as usize;
            let k = core::cmp::min(BLOCK_SIZE - within, n - done);
            let dst = buf.get_mut(done..done + k).ok_or(FsError::Internal)?;
            match self.get_block(id, pos / bs)? {
                Some(b) => dst.copy_from_slice(b.get(within..within + k).ok_or(FsError::Internal)?),
                None => dst.fill(0),
            }
            done += k;
        }
        Ok(n)
    }

    /// The object's attributes.
    pub fn stat(&mut self, h: Handle) -> Result<Attr, FsError> {
        let e = self.handle(h)?;
        let r = self.inode(e.id);
        self.note_meta_error();
        r
    }

    /// The attributes of the object `path` names, without following a final
    /// link object.
    pub fn stat_at(&mut self, dir: Handle, path: &str) -> Result<Attr, FsError> {
        let r = self.lookup(dir, path).and_then(|(id, _)| self.inode(id));
        self.note_meta_error();
        r
    }

    /// The target of a link object opened with `link_itself`.
    pub fn readlink(&mut self, h: Handle) -> Result<String, FsError> {
        let e = self.handle(h)?;
        if e.kind != ObjKind::Link {
            return Err(FsError::InvalidArgument);
        }
        let r = self.link_target(e.id);
        self.note_meta_error();
        r
    }

    /// The first entry of directory `h` whose name sorts after `after`
    /// (byte order), or the first entry if `after` is `None`.
    pub fn readdir(&mut self, h: Handle, after: Option<&str>) -> Result<Option<DirEntry>, FsError> {
        let id = self.dir_handle(h)?;
        let from = Key::dirent(id, after.unwrap_or("").as_bytes());
        let r = match self.tnext(&from, after.is_none()) {
            Ok(Some((k, Value::Dirent { child, kind }))) if k.id == id && k.kind == K_DIRENT => {
                match validate_name(&k.name) {
                    Ok(n) => Ok(Some(DirEntry {
                        name: n.to_string(),
                        id: child,
                        kind,
                    })),
                    Err(_) => Err(self.inconsistent()),
                }
            }
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        };
        self.note_meta_error();
        r
    }

    /// Creates a file with no name (ADR-0010 rule 7). It is on the orphan
    /// list until published; a crash or the last close removes it.
    pub fn create_unnamed(&mut self, dir: Handle, mode: u16) -> Result<Handle, FsError> {
        self.writable()?;
        let d = self.dir_handle(dir)?;
        Self::check_mode(mode)?;
        let id = self.mutate(false, true, &mut |v| {
            if v.is_orphan(d)? {
                return Err(FsError::NotFound);
            }
            let id = v.st.next_id;
            if id >= v.st.id_limit {
                return Err(FsError::Internal);
            }
            v.st.next_id += 1;
            let t = v.now();
            v.put_inode(Attr {
                id,
                kind: ObjKind::File,
                size: 0,
                mtime: t,
                ctime: t,
                posix_mode: mode,
            })?;
            v.tins(Key::orphan(id), Value::Orphan)?;
            Ok(id)
        })?;
        Ok(self.new_handle(id, ObjKind::File))
    }

    /// Gives the unnamed file `h` the name `path`, atomically
    /// (ADR-0010 rule 7).
    pub fn publish(
        &mut self,
        h: Handle,
        dir: Handle,
        path: &str,
        mode: PublishMode,
    ) -> Result<(), FsError> {
        self.writable()?;
        let e = self.handle(h)?;
        if e.kind != ObjKind::File {
            return Err(FsError::InvalidArgument);
        }
        self.mutate(false, false, &mut |v| {
            let (parent, name) = v.parent_of(dir, path)?;
            if !v.is_orphan(e.id)? {
                // It has a name already: no second name (ADR-0013 rule 5).
                return Err(FsError::InvalidArgument);
            }
            if v.is_orphan(parent)? {
                return Err(FsError::NotFound);
            }
            let dst = v.entry(parent, name.as_bytes())?;
            match (mode, dst) {
                (PublishMode::NoReplace, Some(_)) => return Err(FsError::Exists),
                (PublishMode::Replace, Some((_, ObjKind::Directory))) => {
                    return Err(FsError::IsDir);
                }
                _ => {}
            }
            let t = v.now();
            if let Some((did, dkind)) = dst {
                v.unname(did, dkind, t)?;
            }
            v.tdel(&Key::orphan(e.id))?;
            v.tins(
                Key::dirent(parent, name.as_bytes()),
                Value::Dirent {
                    child: e.id,
                    kind: ObjKind::File,
                },
            )?;
            let mut a = v.inode(e.id)?;
            a.ctime = t;
            v.put_inode(a)?;
            v.adjust_dir(parent, if dst.is_some() { 0 } else { 1 }, t)
        })
    }

    /// Closes a handle. Closing the last handle of an object with no name
    /// frees it. On a `Failed` volume the handle is released and the call
    /// reports `StorageFailed`.
    pub fn close(&mut self, h: Handle) -> Result<(), FsError> {
        let e = self.handle(h)?;
        if let Some(slot) = self.handles.get_mut(h.slot as usize) {
            *slot = None;
        }
        let left = match self.open.get_mut(&e.id) {
            Some(n) => {
                *n -= 1;
                *n
            }
            None => 0,
        };
        if left == 0 {
            self.open.remove(&e.id);
        }
        if self.failed.is_some() {
            return Err(FsError::StorageFailed);
        }
        if left > 0 || self.read_only {
            return Ok(());
        }
        let id = e.id;
        let r = self.mutate(true, false, &mut |v| {
            if v.is_orphan(id)? {
                v.delete_object(id)?;
                v.st.changed = true;
            }
            Ok(())
        });
        match r {
            // A full volume keeps the orphan; the next mount frees it.
            Err(FsError::NoSpace) => Ok(()),
            r => r,
        }
    }

    // -----------------------------------------------------------------------
    // Durability (ADR-0010 rules 4 and 5, ADR-0011 rules 2 and 3).

    /// Returns when every request completed before the call is durable.
    pub fn sync(&mut self) -> Result<(), FsError> {
        if self.failed.is_some() {
            return Err(FsError::StorageFailed);
        }
        self.commit()
    }

    /// The monotonic time by which the open group must be committed, if it
    /// has changes.
    pub fn commit_deadline(&self) -> Option<u64> {
        self.oldest_uncommitted
            .map(|t| t.saturating_add(T_COMMIT_NS))
    }

    /// Commits if `T_COMMIT` has passed since the oldest uncommitted request
    /// completed. The service calls this when [`Volume::commit_deadline`]
    /// passes.
    pub fn tick(&mut self) -> Result<(), FsError> {
        if self.failed.is_some() {
            return Err(FsError::StorageFailed);
        }
        let now = self.clock.monotonic_ns();
        match self.commit_deadline() {
            Some(d) if now >= d => self.commit(),
            _ => Ok(()),
        }
    }

    /// Syncs and returns the device.
    pub fn unmount(mut self) -> Result<D, FsError> {
        if self.failed.is_some() {
            return Err(FsError::StorageFailed);
        }
        self.commit()?;
        Ok(self.io.dev)
    }

    /// Gives back the device without committing (a simulated crash).
    pub fn into_device(self) -> D {
        self.io.dev
    }

    /// Commits the open group. A group with no changes writes nothing
    /// (INV-0011-6).
    pub fn commit(&mut self) -> Result<(), FsError> {
        if self.failed.is_some() {
            return Err(FsError::StorageFailed);
        }
        if self.read_only || (!self.st.changed && self.st.id_limit == self.durable.id_limit) {
            return Ok(());
        }
        match self.commit_inner() {
            Ok(()) => {
                self.commits += 1;
                Ok(())
            }
            Err(r) => {
                self.fail(r);
                Err(FsError::StorageFailed)
            }
        }
    }

    fn commit_inner(&mut self) -> Result<(), FailReason> {
        let g = self
            .durable
            .generation
            .checked_add(1)
            .ok_or(FailReason::Internal)?;
        let vid = self.io.volume_id;
        let mut walked = 0;
        count_dirty(&self.st.tree.root, &mut walked);
        if walked != self.acct.dirty_nodes + self.acct.dirty_data {
            return Err(FailReason::Internal);
        }
        let space = self.space.as_mut().ok_or(FailReason::Internal)?;
        // (a) Allocate and encode every new block of the group, children
        // first so that parents record their children's checksums.
        let mut addrs = Vec::with_capacity(walked as usize);
        for _ in 0..walked {
            addrs.push(space.alloc().ok_or(FailReason::Internal)?);
        }
        let mut writes: Vec<(u64, Box<Block>)> = Vec::with_capacity(walked as usize + 4);
        let mut written: Vec<(BlockPtr, Rc<Node>)> = Vec::new();
        let mut next = addrs.iter().copied();
        let root = self.st.tree.root.clone();
        let root_ptr = write_child(&root, vid, g, &mut next, &mut writes, &mut written)?;
        // Allocator: every bitmap block with a changed bit is rewritten, and
        // so is the allocator root; repeat until no new bitmap is touched.
        let mut freed: BTreeSet<u64> = self.acct.freed.iter().copied().collect();
        let mut new_bitmaps = space.bitmaps.clone();
        let mut renewed: BTreeSet<u64> = BTreeSet::new();
        let mut touched: BTreeSet<u64> = addrs
            .iter()
            .chain(freed.iter())
            .map(|a| a / BITS_PER_BITMAP)
            .collect();
        let mut alloc_root_addr = None;
        loop {
            let mut fresh = Vec::new();
            for &i in touched.iter() {
                if renewed.insert(i) {
                    let a = space.alloc().ok_or(FailReason::Internal)?;
                    let old = space
                        .bitmaps
                        .get(i as usize)
                        .ok_or(FailReason::Internal)?
                        .addr;
                    if let Some(p) = new_bitmaps.get_mut(i as usize) {
                        p.addr = a;
                    }
                    freed.insert(old);
                    fresh.push(a);
                    fresh.push(old);
                }
            }
            if alloc_root_addr.is_none() {
                let a = space.alloc().ok_or(FailReason::Internal)?;
                alloc_root_addr = Some(a);
                freed.insert(space.alloc_root.addr);
                fresh.push(a);
                fresh.push(space.alloc_root.addr);
            }
            let before = touched.len();
            touched.extend(fresh.iter().map(|a| a / BITS_PER_BITMAP));
            if touched.len() == before && touched.iter().all(|i| renewed.contains(i)) {
                break;
            }
        }
        for &i in &renewed {
            let bytes = space.bitmap_bytes(i, &freed);
            let p = new_bitmaps
                .get_mut(i as usize)
                .ok_or(FailReason::Internal)?;
            let blk = encode_bitmap(vid, p.addr, g, i as u32, &bytes)
                .map_err(|_| FailReason::Internal)?;
            p.csum = crate::crc32c::checksum64(&blk);
            p.generation = g;
            writes.push((p.addr, Box::new(blk)));
        }
        let aaddr = alloc_root_addr.ok_or(FailReason::Internal)?;
        let ablk =
            encode_alloc_root(vid, aaddr, g, &new_bitmaps).map_err(|_| FailReason::Internal)?;
        let alloc_root = BlockPtr {
            addr: aaddr,
            csum: crate::crc32c::checksum64(&ablk),
            generation: g,
        };
        writes.push((aaddr, Box::new(ablk)));
        // (a)–(c) Write every new block, wait, flush.
        let refs: Vec<(u64, &Block)> = writes.iter().map(|(a, b)| (*a, &**b)).collect();
        self.io
            .dev
            .write(&refs)
            .map_err(|_| FailReason::DeviceWrite)?;
        self.io.dev.flush().map_err(|_| FailReason::DeviceFlush)?;
        // (d)–(e) The root record into both copies of pair g mod N_PAIRS,
        // then flush.
        let rec = RootRecord {
            volume_id: vid,
            block_count: self.durable.block_count,
            generation: g,
            tree_root: root_ptr,
            tree_height: self.st.tree.height,
            alloc_root,
            last_assigned: self.st.last_assigned,
            id_limit: self.st.id_limit,
        };
        let rb = rec.encode().map_err(|_| FailReason::Internal)?;
        let [s0, s1] = RootRecord::slots(g % crate::format::N_PAIRS);
        self.io
            .dev
            .write(&[(s0, &rb), (s1, &rb)])
            .map_err(|_| FailReason::DeviceWrite)?;
        self.io.dev.flush().map_err(|_| FailReason::DeviceFlush)?;
        // (f) Durable: freed blocks become reusable (ADR-0011 rule 5).
        let space = self.space.as_mut().ok_or(FailReason::Internal)?;
        space.bitmaps = new_bitmaps;
        space.alloc_root = alloc_root;
        let freed: Vec<u64> = freed.into_iter().collect();
        space.release(&freed);
        self.durable = rec;
        self.st.tree.root = Child::Clean(root_ptr);
        self.st.changed = false;
        self.acct = Acct::default();
        self.oldest_uncommitted = None;
        for (p, n) in written {
            self.io.cache_insert(p, n);
        }
        Ok(())
    }
}

impl<D: BlockDevice, C: Clock> Volume<D, C> {
    /// Applies namespace repairs planned by the checker, then commits.
    pub(crate) fn apply_fixes(
        &mut self,
        fixes: &[crate::check::Fix],
        out: &mut crate::check::RepairReport,
    ) -> Result<(), FsError> {
        use crate::check::Fix;
        for f in fixes {
            self.mutate(true, false, &mut |v| {
                v.st.changed = true;
                match f {
                    Fix::RemoveEntry { dir, name } => {
                        v.tdel(&Key::dirent(*dir, name))?;
                    }
                    Fix::Orphan(id) => {
                        v.tins(Key::orphan(*id), Value::Orphan)?;
                    }
                    Fix::RemoveOrphan(id) => {
                        v.tdel(&Key::orphan(*id))?;
                    }
                    Fix::RemoveExtent { id, index } => {
                        let old = v.tdel(&Key::extent(*id, *index))?;
                        v.drop_data(old);
                    }
                    Fix::SetDirSize { dir, size } => {
                        if let Some(Value::Inode(mut a)) = v.tget(&Key::inode(*dir))? {
                            a.size = *size;
                            v.put_inode(a)?;
                        }
                    }
                    Fix::SetParent { dir, parent } => match parent {
                        Some(p) => {
                            v.tins(Key::parent(*dir), Value::Parent(*p))?;
                        }
                        None => {
                            v.tdel(&Key::parent(*dir))?;
                        }
                    },
                }
                Ok(())
            })?;
            match f {
                Fix::RemoveEntry { .. } | Fix::RemoveOrphan(_) | Fix::RemoveExtent { .. } => {
                    out.entries_removed += 1
                }
                Fix::Orphan(_) => out.objects_orphaned += 1,
                Fix::SetDirSize { .. } => out.sizes_fixed += 1,
                Fix::SetParent { .. } => out.parents_fixed += 1,
            }
        }
        self.commit()
    }
}

fn count_dirty(c: &Child, n: &mut u64) {
    if let Child::Dirty(node) = c {
        *n += 1;
        match &node.body {
            Body::Leaf(items) => {
                for (_, v) in items {
                    if let Value::Extent(DataRef::Dirty(_)) = v {
                        *n += 1;
                    }
                }
            }
            Body::Interior(es) => {
                for (_, c) in es {
                    count_dirty(c, n);
                }
            }
        }
    }
}

/// Writes a dirty subtree: data and children first, then the node. Returns
/// the pointer to it.
fn write_child(
    c: &Child,
    vid: u64,
    g: u64,
    addrs: &mut impl Iterator<Item = u64>,
    writes: &mut Vec<(u64, Box<Block>)>,
    written: &mut Vec<(BlockPtr, Rc<Node>)>,
) -> Result<BlockPtr, FailReason> {
    let node = match c {
        Child::Clean(p) => return Ok(*p),
        Child::Dirty(n) => n,
    };
    let body = match &node.body {
        Body::Leaf(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (k, v) in items {
                let v = match v {
                    Value::Extent(DataRef::Dirty(b)) => {
                        let addr = addrs.next().ok_or(FailReason::Internal)?;
                        let p = BlockPtr {
                            addr,
                            csum: crate::crc32c::checksum64(&b[..]),
                            generation: g,
                        };
                        writes.push((addr, Box::new(**b)));
                        Value::Extent(DataRef::Clean(p))
                    }
                    v => v.clone(),
                };
                out.push((k.clone(), v));
            }
            Body::Leaf(out)
        }
        Body::Interior(es) => {
            let mut out = Vec::with_capacity(es.len());
            for (k, c) in es {
                let p = write_child(c, vid, g, addrs, writes, written)?;
                out.push((k.clone(), Child::Clean(p)));
            }
            Body::Interior(out)
        }
    };
    let clean = Node {
        level: node.level,
        body,
    };
    let addr = addrs.next().ok_or(FailReason::Internal)?;
    let blk = clean
        .encode(vid, addr, g)
        .map_err(|_| FailReason::Internal)?;
    let p = BlockPtr {
        addr,
        csum: crate::crc32c::checksum64(&blk),
        generation: g,
    };
    writes.push((addr, Box::new(blk)));
    written.push((p, Rc::new(clean)));
    Ok(p)
}

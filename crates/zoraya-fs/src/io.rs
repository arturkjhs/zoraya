//! Reading blocks: the bounded node cache, validation on every load, and the
//! record of metadata failures.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::device::{BLOCK_SIZE, Block, BlockDevice, DevError};
use crate::error::FsError;
use crate::format::{BlockPtr, Body, Key, Node};
use crate::tree::Loader;

/// Decoded nodes kept in memory (ADR-0012 rule 4's fixed cache).
pub const CACHE_ENTRIES: usize = 16;

/// Why the volume entered `Failed` (ADR-0010 rule 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    /// The device failed a write.
    DeviceWrite,
    /// The device failed a flush.
    DeviceFlush,
    /// The device failed a metadata read.
    MetadataRead,
    /// A metadata block failed its checksum or validation (ADR-0012 rule 3).
    MetadataCorrupt,
    /// An internal check of the engine failed.
    Internal,
}

pub(crate) struct Io<D> {
    pub dev: D,
    pub volume_id: u64,
    pub block_count: u64,
    cache: Vec<(BlockPtr, Rc<Node>, u64)>,
    tick: u64,
    /// Set by a metadata read that failed; the engine moves to `Failed`.
    pub meta_error: Option<FailReason>,
    /// Device reads issued, for the step bound of INV-0012-4.
    pub reads: u64,
}

impl<D: BlockDevice> Io<D> {
    pub fn new(dev: D, volume_id: u64, block_count: u64) -> Self {
        Io {
            dev,
            volume_id,
            block_count,
            cache: Vec::new(),
            tick: 0,
            meta_error: None,
            reads: 0,
        }
    }

    pub fn read_block(&mut self, addr: u64) -> Result<Box<Block>, DevError> {
        self.reads += 1;
        let mut b = Box::new([0u8; BLOCK_SIZE]);
        self.dev.read(addr, &mut b)?;
        Ok(b)
    }

    /// A data block, checked against its pointer. A failure is `Corrupt`
    /// for this read only (ADR-0012 rule 3, ADR-0015 rule 6).
    pub fn read_data(&mut self, ptr: &BlockPtr) -> Result<Box<Block>, FsError> {
        let b = self.read_block(ptr.addr).map_err(|_| FsError::Corrupt)?;
        if !ptr.verifies(&b) {
            return Err(FsError::Corrupt);
        }
        Ok(b)
    }

    /// A metadata failure: recorded so the engine enters `Failed`.
    pub fn meta_fail(&mut self, r: FailReason) -> FsError {
        self.meta_error = Some(r);
        match r {
            FailReason::MetadataRead => FsError::StorageFailed,
            _ => FsError::Corrupt,
        }
    }

    pub fn cache_clear(&mut self) {
        self.cache.clear();
    }

    /// Decoded nodes held now.
    pub fn cached_nodes(&self) -> usize {
        self.cache.len()
    }

    /// Adds a node the engine just wrote, so it need not be read back.
    pub fn cache_insert(&mut self, ptr: BlockPtr, node: Rc<Node>) {
        self.tick += 1;
        if let Some(e) = self.cache.iter_mut().find(|e| e.0 == ptr) {
            e.1 = node;
            e.2 = self.tick;
            return;
        }
        if self.cache.len() >= CACHE_ENTRIES
            && let Some(i) = self
                .cache
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.2)
                .map(|(i, _)| i)
        {
            self.cache.swap_remove(i);
        }
        self.cache.push((ptr, node, self.tick));
    }
}

fn within(node: &Node, lower: Option<&Key>, upper: Option<&Key>) -> bool {
    let ok = |k: &Key| lower.is_none_or(|l| k >= l) && upper.is_none_or(|u| k < u);
    match &node.body {
        Body::Leaf(items) => {
            items.first().is_none_or(|(k, _)| ok(k)) && items.last().is_none_or(|(k, _)| ok(k))
        }
        Body::Interior(es) => {
            es.get(1).is_none_or(|(k, _)| ok(k))
                && es.last().is_none_or(|(k, _)| es.len() < 2 || ok(k))
        }
    }
}

impl<D: BlockDevice> Loader for Io<D> {
    fn load_node(
        &mut self,
        ptr: &BlockPtr,
        level: u8,
        lower: Option<&Key>,
        upper: Option<&Key>,
    ) -> Result<Rc<Node>, FsError> {
        self.tick += 1;
        let tick = self.tick;
        if let Some(e) = self.cache.iter_mut().find(|e| e.0 == *ptr) {
            e.2 = tick;
            let n = e.1.clone();
            // The same block reached through another path must still fit
            // where it is now (a crafted image can point twice at a block).
            if n.level != level || !within(&n, lower, upper) {
                return Err(self.meta_fail(FailReason::MetadataCorrupt));
            }
            return Ok(n);
        }
        let b = match self.read_block(ptr.addr) {
            Ok(b) => b,
            Err(_) => return Err(self.meta_fail(FailReason::MetadataRead)),
        };
        let node = match Node::decode(
            &b,
            ptr,
            self.volume_id,
            self.block_count,
            level,
            lower,
            upper,
        ) {
            Ok(n) => Rc::new(n),
            Err(_) => return Err(self.meta_fail(FailReason::MetadataCorrupt)),
        };
        self.cache_insert(*ptr, node.clone());
        Ok(node)
    }
}

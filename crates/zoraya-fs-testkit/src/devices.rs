//! Block devices for tests (ADR-0015).
//!
//! - [`MemDevice`]: RAM, every write immediately visible; flush optional.
//! - [`RecordingDevice`]: a [`MemDevice`] that logs every write (with its
//!   batch), flush and test mark, keeps the image it started from, and can
//!   permute the completion order inside each batch (INV-0015-2). The crash
//!   images the log allows are produced by [`crate::crash`].
//! - [`FaultDevice`]: wraps any device and fails the n-th write, flush or
//!   read, or corrupts reads of chosen blocks.
//! - [`ImageDevice`]: a shared base image plus a private overlay; how crash
//!   images and mutated images are mounted without copying.

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use zoraya_fs::{BLOCK_SIZE, Block, BlockDevice, DevError, SECTOR_SIZE};

use crate::rng::Rng;

/// A RAM block device.
#[derive(Debug, Clone)]
pub struct MemDevice {
    data: Vec<u8>,
    flush: bool,
}

impl MemDevice {
    pub fn new(blocks: u64) -> MemDevice {
        MemDevice {
            data: vec![0; blocks as usize * BLOCK_SIZE],
            flush: true,
        }
    }
    pub fn from_image(data: Vec<u8>) -> MemDevice {
        MemDevice { data, flush: true }
    }
    /// A device that does not offer flush (ADR-0015 rule 5, INV-0015-3).
    pub fn without_flush(mut self) -> MemDevice {
        self.flush = false;
        self
    }
    pub fn image(&self) -> &[u8] {
        &self.data
    }
    pub fn into_image(self) -> Vec<u8> {
        self.data
    }
    pub fn block(&self, b: u64) -> &[u8] {
        let s = b as usize * BLOCK_SIZE;
        &self.data[s..s + BLOCK_SIZE]
    }
    pub fn block_mut(&mut self, b: u64) -> &mut [u8] {
        let s = b as usize * BLOCK_SIZE;
        &mut self.data[s..s + BLOCK_SIZE]
    }
}

impl BlockDevice for MemDevice {
    fn sector_count(&self) -> u64 {
        (self.data.len() / SECTOR_SIZE) as u64
    }
    fn flush_supported(&self) -> bool {
        self.flush
    }
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        let s = (block as usize)
            .checked_mul(BLOCK_SIZE)
            .ok_or(DevError::IoError)?;
        buf.copy_from_slice(self.data.get(s..s + BLOCK_SIZE).ok_or(DevError::IoError)?);
        Ok(())
    }
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        for (block, data) in writes {
            let s = (*block as usize)
                .checked_mul(BLOCK_SIZE)
                .ok_or(DevError::IoError)?;
            self.data
                .get_mut(s..s + BLOCK_SIZE)
                .ok_or(DevError::IoError)?
                .copy_from_slice(&data[..]);
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), DevError> {
        if self.flush {
            Ok(())
        } else {
            Err(DevError::Unsupported)
        }
    }
}

/// One entry of a [`RecordingDevice`]'s log.
#[derive(Debug, Clone)]
pub enum Event {
    /// A write; writes with the same `batch` were in flight together.
    Write {
        block: u64,
        data: Rc<Block>,
        batch: u64,
    },
    /// A flush that completed.
    Flush,
    /// Test mark: `completed` requests had replied at this point.
    Mark { completed: u64 },
    /// Test mark: a `sync` returned success; `completed` requests had
    /// replied before it was called.
    SyncMark { completed: u64 },
}

/// A RAM device that records its history.
#[derive(Debug, Clone)]
pub struct RecordingDevice {
    dev: MemDevice,
    base: Rc<Vec<u8>>,
    events: Vec<Event>,
    batch: u64,
    permute: Option<Rng>,
    pub reads: u64,
}

impl RecordingDevice {
    /// Starts recording on top of `dev`'s current content.
    pub fn new(dev: MemDevice) -> RecordingDevice {
        let base = Rc::new(dev.image().to_vec());
        RecordingDevice {
            dev,
            base,
            events: Vec::new(),
            batch: 0,
            permute: None,
            reads: 0,
        }
    }
    /// Completes the writes of each batch in a seeded random order.
    pub fn permuting(mut self, seed: u64) -> RecordingDevice {
        self.permute = Some(Rng::new(seed));
        self
    }
    pub fn mark(&mut self, completed: u64) {
        self.events.push(Event::Mark { completed });
    }
    pub fn sync_mark(&mut self, completed: u64) {
        self.events.push(Event::SyncMark { completed });
    }
    pub fn base(&self) -> &Rc<Vec<u8>> {
        &self.base
    }
    pub fn events(&self) -> &[Event] {
        &self.events
    }
    pub fn current(&self) -> &MemDevice {
        &self.dev
    }
    pub fn into_parts(self) -> (Rc<Vec<u8>>, Vec<Event>, MemDevice) {
        (self.base, self.events, self.dev)
    }
    pub fn write_count(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, Event::Write { .. }))
            .count()
    }
}

impl BlockDevice for RecordingDevice {
    fn sector_count(&self) -> u64 {
        self.dev.sector_count()
    }
    fn flush_supported(&self) -> bool {
        self.dev.flush_supported()
    }
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        self.reads += 1;
        self.dev.read(block, buf)
    }
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        self.batch += 1;
        let mut order: Vec<usize> = (0..writes.len()).collect();
        if let Some(rng) = &mut self.permute {
            for i in (1..order.len()).rev() {
                let j = rng.below(i as u64 + 1) as usize;
                order.swap(i, j);
            }
        }
        for i in order {
            let (block, data) = writes[i];
            self.dev.write(&[(block, data)])?;
            self.events.push(Event::Write {
                block,
                data: Rc::new(*data),
                batch: self.batch,
            });
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), DevError> {
        self.dev.flush()?;
        self.events.push(Event::Flush);
        Ok(())
    }
}

/// Which operations a [`FaultDevice`] fails.
#[derive(Debug, Clone, Default)]
pub struct FaultPlan {
    /// Fail the batch containing the n-th block write (0-based).
    pub fail_write: Option<u64>,
    /// Fail the n-th flush (0-based).
    pub fail_flush: Option<u64>,
    /// Fail the n-th read (0-based).
    pub fail_read: Option<u64>,
    /// Flip a bit in what is read from these blocks.
    pub corrupt_reads_of: BTreeSet<u64>,
    /// Report no flush support.
    pub no_flush: bool,
}

/// Wraps a device and injects faults.
#[derive(Debug, Clone)]
pub struct FaultDevice<D> {
    pub inner: D,
    pub plan: FaultPlan,
    pub writes: u64,
    pub flushes: u64,
    pub reads: u64,
    /// Faults injected so far.
    pub injected: u64,
    /// Every block written, in order, including failed batches.
    pub write_log: Vec<u64>,
}

impl<D: BlockDevice> FaultDevice<D> {
    pub fn new(inner: D, plan: FaultPlan) -> Self {
        FaultDevice {
            inner,
            plan,
            writes: 0,
            flushes: 0,
            reads: 0,
            injected: 0,
            write_log: Vec::new(),
        }
    }
}

impl<D: BlockDevice> BlockDevice for FaultDevice<D> {
    fn sector_count(&self) -> u64 {
        self.inner.sector_count()
    }
    fn flush_supported(&self) -> bool {
        !self.plan.no_flush && self.inner.flush_supported()
    }
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        let n = self.reads;
        self.reads += 1;
        if self.plan.fail_read == Some(n) {
            self.injected += 1;
            return Err(DevError::IoError);
        }
        self.inner.read(block, buf)?;
        if self.plan.corrupt_reads_of.contains(&block) {
            buf[(block as usize * 7919) % BLOCK_SIZE] ^= 0x10;
        }
        Ok(())
    }
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        let first = self.writes;
        self.writes += writes.len() as u64;
        self.write_log.extend(writes.iter().map(|w| w.0));
        if let Some(n) = self.plan.fail_write
            && (first..self.writes).contains(&n)
        {
            self.injected += 1;
            return Err(DevError::IoError);
        }
        self.inner.write(writes)
    }
    fn flush(&mut self) -> Result<(), DevError> {
        let n = self.flushes;
        self.flushes += 1;
        if self.plan.fail_flush == Some(n) {
            self.injected += 1;
            return Err(DevError::IoError);
        }
        self.inner.flush()
    }
}

/// A shared base image plus a private overlay of changed blocks.
#[derive(Debug, Clone)]
pub struct ImageDevice {
    base: Rc<Vec<u8>>,
    over: HashMap<u64, Box<Block>>,
    pub reads: u64,
    pub writes: u64,
}

impl ImageDevice {
    pub fn new(base: Rc<Vec<u8>>) -> ImageDevice {
        ImageDevice {
            base,
            over: HashMap::new(),
            reads: 0,
            writes: 0,
        }
    }
    pub fn set_block(&mut self, b: u64, data: Block) {
        self.over.insert(b, Box::new(data));
    }
    pub fn get_block(&self, b: u64) -> Block {
        if let Some(x) = self.over.get(&b) {
            return **x;
        }
        let s = b as usize * BLOCK_SIZE;
        let mut out = [0u8; BLOCK_SIZE];
        if let Some(src) = self.base.get(s..s + BLOCK_SIZE) {
            out.copy_from_slice(src);
        }
        out
    }
    /// A hash of the overlay (block numbers and contents), independent of
    /// insertion order.
    pub fn overlay_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut blocks: Vec<_> = self.over.iter().collect();
        blocks.sort_by_key(|(b, _)| **b);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (b, d) in blocks {
            b.hash(&mut h);
            d[..].hash(&mut h);
        }
        h.finish()
    }

    pub fn to_image(&self) -> Vec<u8> {
        let mut v = (*self.base).clone();
        for (b, d) in &self.over {
            let s = *b as usize * BLOCK_SIZE;
            v[s..s + BLOCK_SIZE].copy_from_slice(&d[..]);
        }
        v
    }
}

impl BlockDevice for ImageDevice {
    fn sector_count(&self) -> u64 {
        (self.base.len() / SECTOR_SIZE) as u64
    }
    fn flush_supported(&self) -> bool {
        true
    }
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        self.reads += 1;
        if block as usize >= self.base.len() / BLOCK_SIZE {
            return Err(DevError::IoError);
        }
        *buf = self.get_block(block);
        Ok(())
    }
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        for (b, d) in writes {
            if *b as usize >= self.base.len() / BLOCK_SIZE {
                return Err(DevError::IoError);
            }
            self.writes += 1;
            self.over.insert(*b, Box::new(**d));
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), DevError> {
        Ok(())
    }
}

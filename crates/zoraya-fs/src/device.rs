//! The block device contract (ADR-0015).
//!
//! The filesystem uses 4,096-byte blocks and issues only whole-block reads
//! and writes (rule 1). The device reports its size in 512-byte sectors.
//!
//! What the engine assumes, and what the test devices implement exactly:
//! - **Writes are volatile until stable** (rule 2). After a crash, every
//!   512-byte sector of every write that was not stable may hold its old
//!   content, its new content or arbitrary bytes, independently. No write of
//!   any size is atomic.
//! - **Stability** (rule 3). A write is stable once it has completed and a
//!   flush issued after its completion has completed. FUA is not used: QEMU's
//!   virtio-blk does not offer it.
//! - **No order among requests in flight together.** [`BlockDevice::write`]
//!   submits a batch of writes that are all in flight at once; the device may
//!   complete them in any order. The engine never writes the same block
//!   twice in one batch and never depends on the order inside a batch
//!   (INV-0015-2).
//! - **Errors are typed** (rule 6) and never retried by the engine.

/// Bytes in a filesystem block (ADR-0015 rule 1, an assumption there).
pub const BLOCK_SIZE: usize = 4096;
/// Bytes in a device sector (virtio units).
pub const SECTOR_SIZE: usize = 512;
/// Sectors per block.
pub const SECTORS_PER_BLOCK: u64 = (BLOCK_SIZE / SECTOR_SIZE) as u64;

/// One block of data.
pub type Block = [u8; BLOCK_SIZE];

/// How a device request failed (ADR-0015 rule 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevError {
    /// The device or driver reported an error. A failed write leaves its
    /// sectors undefined; a failed flush means nothing since the last
    /// successful flush is known to be stable.
    IoError,
    /// The device does not support the request.
    Unsupported,
}

/// A block device as ADR-0015 describes it. Block numbers are in
/// [`BLOCK_SIZE`] units; the driver converts to sectors.
pub trait BlockDevice {
    /// Size in 512-byte sectors. Space past the last whole block is unused.
    fn sector_count(&self) -> u64;

    /// Whether the device offers flush (`VIRTIO_BLK_F_FLUSH`). Without it the
    /// engine mounts read-only unless told otherwise (rule 5).
    fn flush_supported(&self) -> bool;

    /// Reads block `block` into `buf`.
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError>;

    /// Submits every write in `writes` at once and returns when all have
    /// completed, in whatever order the device chose. If any fails, the call
    /// fails and every write in the batch is undefined.
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError>;

    /// Returns when every write that completed before this call is stable
    /// (rule 4).
    fn flush(&mut self) -> Result<(), DevError>;

    /// Number of whole blocks.
    fn block_count(&self) -> u64 {
        self.sector_count() / SECTORS_PER_BLOCK
    }
}

impl<D: BlockDevice + ?Sized> BlockDevice for &mut D {
    fn sector_count(&self) -> u64 {
        (**self).sector_count()
    }
    fn flush_supported(&self) -> bool {
        (**self).flush_supported()
    }
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        (**self).read(block, buf)
    }
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        (**self).write(writes)
    }
    fn flush(&mut self) -> Result<(), DevError> {
        (**self).flush()
    }
}

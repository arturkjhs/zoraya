//! A block device in memory, for the temporary directory service (ADR-0107
//! item 12: `fsd --memory` over a fresh volume). Writes are stable when they
//! return, so flush has nothing to do.

use zoraya_fs::{BLOCK_SIZE, Block, BlockDevice, DevError, SECTOR_SIZE};

use crate::Device;

/// A device over `B`'s bytes, whole blocks of them.
pub struct MemoryDevice<B> {
    bytes: B,
    lost: bool,
}

impl<B: AsRef<[u8]> + AsMut<[u8]>> MemoryDevice<B> {
    pub fn new(bytes: B) -> MemoryDevice<B> {
        MemoryDevice { bytes, lost: false }
    }

    fn range(&self, block: u64) -> Option<core::ops::Range<usize>> {
        let start = usize::try_from(block).ok()?.checked_mul(BLOCK_SIZE)?;
        let end = start.checked_add(BLOCK_SIZE)?;
        (end <= self.bytes.as_ref().len()).then_some(start..end)
    }
}

impl<B: AsRef<[u8]> + AsMut<[u8]>> BlockDevice for MemoryDevice<B> {
    fn sector_count(&self) -> u64 {
        (self.bytes.as_ref().len() / BLOCK_SIZE * (BLOCK_SIZE / SECTOR_SIZE)) as u64
    }

    fn flush_supported(&self) -> bool {
        true
    }

    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        let r = self.range(block).ok_or(DevError::IoError)?;
        buf.copy_from_slice(&self.bytes.as_ref()[r]);
        Ok(())
    }

    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        // Every block is checked first, so a refused batch writes nothing.
        for (block, _) in writes {
            self.range(*block).ok_or(DevError::IoError)?;
        }
        for (block, data) in writes {
            if let Some(r) = self.range(*block) {
                self.bytes.as_mut()[r].copy_from_slice(&data[..]);
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), DevError> {
        Ok(())
    }
}

impl<B: AsRef<[u8]> + AsMut<[u8]>> Device for MemoryDevice<B> {
    fn is_lost(&self) -> bool {
        self.lost
    }

    fn mark_lost(&mut self) {
        self.lost = true;
    }
}

//! The block allocator (ADR-0011 rules 5 and 6).
//!
//! On disk: an allocator root pointing to bitmap blocks, one bit per block,
//! written copy-on-write by every commit that changes them. The bitmap of
//! generation `g` marks exactly the blocks reachable from `g`'s root record
//! (plus the root slots and the bitmap padding).
//!
//! In memory, a block freed by the open commit group stays marked used until
//! that commit is durable, so it cannot be reused before (deferred reuse,
//! INV-0011-2). Its bit is cleared in the bitmap the commit writes.

use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;

use crate::format::{BITS_PER_BITMAP, BlockPtr, ROOT_SLOTS};

#[derive(Debug, Clone)]
pub(crate) struct Space {
    /// One bit per block, set = used or pending deferred reuse.
    used: Vec<u64>,
    free: u64,
    cursor: u64,
    block_count: u64,
    /// The bitmap blocks of the durable generation.
    pub bitmaps: Vec<BlockPtr>,
    /// The allocator root of the durable generation.
    pub alloc_root: BlockPtr,
}

impl Space {
    /// From bitmap bytes as stored (bit set = used). `bits` holds at least
    /// `block_count` bits.
    pub fn from_bits(
        block_count: u64,
        bits: &[u8],
        bitmaps: Vec<BlockPtr>,
        alloc_root: BlockPtr,
    ) -> Space {
        let words = block_count.div_ceil(64) as usize;
        let mut used = vec![0u64; words];
        let mut free = 0;
        for b in 0..block_count {
            let set = bits
                .get((b / 8) as usize)
                .is_none_or(|byte| byte >> (b % 8) & 1 != 0)
                || b < ROOT_SLOTS;
            if set {
                if let Some(w) = used.get_mut((b / 64) as usize) {
                    *w |= 1 << (b % 64);
                }
            } else {
                free += 1;
            }
        }
        for b in block_count..words as u64 * 64 {
            if let Some(w) = used.get_mut((b / 64) as usize) {
                *w |= 1 << (b % 64);
            }
        }
        Space {
            used,
            free,
            cursor: ROOT_SLOTS,
            block_count,
            bitmaps,
            alloc_root,
        }
    }

    pub fn free(&self) -> u64 {
        self.free
    }

    pub fn is_used(&self, b: u64) -> bool {
        self.used
            .get((b / 64) as usize)
            .is_none_or(|w| w >> (b % 64) & 1 != 0)
    }

    fn set(&mut self, b: u64, v: bool) {
        if let Some(w) = self.used.get_mut((b / 64) as usize) {
            if v {
                *w |= 1 << (b % 64);
            } else {
                *w &= !(1 << (b % 64));
            }
        }
    }

    /// Blocks of metadata a commit may need beyond its tree and data blocks:
    /// every bitmap block and the allocator root.
    pub fn overhead(&self) -> u64 {
        self.bitmaps.len() as u64 + 1
    }

    /// Takes one free block (next fit), or `None` if none is free.
    pub fn alloc(&mut self) -> Option<u64> {
        if self.free == 0 {
            return None;
        }
        // Word at a time from the cursor; padding bits past the end and the
        // root slots are always set, so any clear bit is a free block.
        let words = self.used.len();
        let start = (self.cursor / 64) as usize;
        for i in 0..=words {
            let wi = (start + i) % words;
            let mut m = !self.used.get(wi).copied().unwrap_or(u64::MAX);
            if i == 0 {
                m &= u64::MAX << (self.cursor % 64);
            }
            if m == 0 {
                continue;
            }
            let b = wi as u64 * 64 + u64::from(m.trailing_zeros());
            if b < ROOT_SLOTS || b >= self.block_count {
                return None;
            }
            self.set(b, true);
            self.free -= 1;
            self.cursor = if b + 1 >= self.block_count {
                ROOT_SLOTS
            } else {
                b + 1
            };
            return Some(b);
        }
        None
    }

    /// After the commit that freed them is durable: makes `blocks`
    /// reusable.
    pub fn release(&mut self, blocks: &[u64]) {
        for &b in blocks {
            if b >= ROOT_SLOTS && b < self.block_count && self.is_used(b) {
                self.set(b, false);
                self.free += 1;
            }
        }
    }

    /// The bits of bitmap block `index` as the commit writes them: the
    /// in-memory bits with `freed` cleared; bits past the volume's end set.
    pub fn bitmap_bytes(&self, index: u64, freed: &BTreeSet<u64>) -> Vec<u8> {
        let nbytes = (BITS_PER_BITMAP / 8) as usize;
        let mut out = vec![0u8; nbytes];
        let first = index * BITS_PER_BITMAP;
        for (i, byte) in out.iter_mut().enumerate() {
            for bit in 0..8u64 {
                let b = first + i as u64 * 8 + bit;
                let used = b >= self.block_count || (self.is_used(b) && !freed.contains(&b));
                if used {
                    *byte |= 1 << bit;
                }
            }
        }
        out
    }
}

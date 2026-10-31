//! Choosing the root record at mount (ADR-0011 rule 4). Shared by the engine
//! and the checker.

use crate::device::BlockDevice;
use crate::format::{Corrupt, N_PAIRS, ROOT_SLOTS, RootRecord};

/// What the root slots hold.
#[derive(Debug, Clone)]
pub struct RootSelection {
    /// The record mount uses: the highest generation with a valid copy.
    pub record: RootRecord,
    /// Whether the valid generations are consecutive in ring order, ending
    /// at the selected one. If not, the volume is corrupt and mounts
    /// read-only.
    pub ring_consistent: bool,
    /// The newest valid generation in each pair, if any.
    pub pairs: [Option<u64>; N_PAIRS as usize],
    /// Valid copies found, out of [`ROOT_SLOTS`].
    pub valid_copies: u32,
}

/// Reads every root slot and selects per ADR-0011 rule 4. Fails if no copy
/// is valid or a slot cannot be read.
pub fn select_root<D: BlockDevice + ?Sized>(dev: &mut D) -> Result<RootSelection, Corrupt> {
    let device_blocks = dev.block_count();
    if device_blocks <= ROOT_SLOTS {
        return Err(Corrupt("device too small"));
    }
    let mut found: [Option<RootRecord>; ROOT_SLOTS as usize] = [None; ROOT_SLOTS as usize];
    let mut buf = [0u8; crate::device::BLOCK_SIZE];
    for slot in 0..ROOT_SLOTS {
        dev.read(slot, &mut buf)
            .map_err(|_| Corrupt("root slot unreadable"))?;
        if let Ok(r) = RootRecord::decode(&buf, device_blocks, slot % N_PAIRS)
            && let Some(f) = found.get_mut(slot as usize)
        {
            *f = Some(r);
        }
    }
    let best = found
        .iter()
        .flatten()
        .max_by_key(|r| r.generation)
        .copied()
        .ok_or(Corrupt("no valid root record"))?;
    // Records of another volume or geometry do not count.
    let mut pairs: [Option<u64>; N_PAIRS as usize] = [None; N_PAIRS as usize];
    let mut valid_copies = 0;
    let mut consistent = true;
    for (slot, r) in found.iter().enumerate() {
        let Some(r) = r else { continue };
        if r.volume_id != best.volume_id || r.block_count != best.block_count {
            continue;
        }
        valid_copies += 1;
        if r.generation == best.generation && *r != best {
            // Two valid copies of one generation that differ.
            consistent = false;
        }
        if let Some(p) = pairs.get_mut(slot % N_PAIRS as usize) {
            *p = Some(p.map_or(r.generation, |g| g.max(r.generation)));
        }
    }
    // Going back from the selected generation, each pair must hold the
    // expected generation until the first pair that holds none; after that
    // gap no older pair may be valid.
    let mut gap = false;
    for k in 0..N_PAIRS {
        let pair = ((best.generation + N_PAIRS - k) % N_PAIRS) as usize;
        let held = pairs.get(pair).copied().flatten();
        let expected = best.generation.checked_sub(k).filter(|&g| g >= 1);
        match (held, expected) {
            (None, _) => gap = true,
            (Some(h), Some(e)) if h == e && !gap => {}
            _ => consistent = false,
        }
    }
    Ok(RootSelection {
        record: best,
        ring_consistent: consistent,
        pairs,
        valid_copies,
    })
}

//! The flat buffer `snapshot_process` takes (ADR-0102, in ADR-0042's form).
//! Integers are little-endian.
//!
//! ```text
//! magic "ZSN1"    4 bytes
//! header size     u32
//! flags           u32
//! reserved        u32, zero
//! job             u64
//! group           u64, 0 for none
//! self slot       u32, 0 for none
//! move count      u32
//! reserved        u64, zero
//! move entries    ENTRY_SIZE bytes each: slot u32, reserved u32 zero, handle u64
//! ```
//!
//! Nothing may follow the entries. The decoder is total, and whatever it
//! accepts iterates without failure.

use crate::spawn::{MAX_SPAWN_HANDLES, START_SUSPENDED};
use crate::{Handle, MAX_HANDLES, Status};

const MAGIC: &[u8; 4] = b"ZSN1";
/// The size of this version's header.
pub const HEADER_SIZE: usize = 48;
const ENTRY_SIZE: usize = 16;
/// Most move entries: `spawn`'s bound (ADR-0102 item 4).
pub const MAX_MOVES: usize = MAX_SPAWN_HANDLES;
/// Longest buffer.
pub const MAX_SNAPSHOT_SIZE: usize = HEADER_SIZE + MAX_MOVES * ENTRY_SIZE;

/// A handle the call moves out of the caller into the child at `slot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub slot: u32,
    pub handle: Handle,
}

/// A checked buffer.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotRequest<'a> {
    pub suspended: bool,
    pub job: Handle,
    pub group: Option<Handle>,
    /// Where the child finds a handle to itself.
    pub self_slot: Option<u32>,
    entries: &'a [u8],
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    let b = bytes.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes(b.try_into().ok()?))
}

fn entry(bytes: &[u8]) -> Result<Move, Status> {
    let bad = Status::InvalidArgument;
    let slot = u32_at(bytes, 0).ok_or(bad)?;
    if slot == 0 || slot > MAX_HANDLES || u32_at(bytes, 4) != Some(0) {
        return Err(bad);
    }
    Ok(Move {
        slot,
        handle: u64_at(bytes, 8)
            .and_then(Handle::new)
            .ok_or(Status::BadHandle)?,
    })
}

impl<'a> SnapshotRequest<'a> {
    /// Checks all of `bytes`. Trailing bytes, unknown flags, nonzero reserved
    /// fields, a slot or handle named twice, and the self slot among the moves
    /// are refused (CP-11).
    pub fn decode(bytes: &'a [u8]) -> Result<SnapshotRequest<'a>, Status> {
        let bad = Status::InvalidArgument;
        if bytes.len() > MAX_SNAPSHOT_SIZE || bytes.len() < HEADER_SIZE || &bytes[..4] != MAGIC {
            return Err(bad);
        }
        let field = |at: usize| u32_at(bytes, at).ok_or(bad);
        let wide = |at: usize| u64_at(bytes, at).ok_or(bad);
        if field(4)? as usize != HEADER_SIZE {
            return Err(bad);
        }
        let flags = field(8)?;
        if flags & !START_SUSPENDED != 0 || field(12)? != 0 || wide(40)? != 0 {
            return Err(bad);
        }
        let self_slot = field(32)?;
        if self_slot > MAX_HANDLES {
            return Err(bad);
        }
        let count = field(36)? as usize;
        if count > MAX_MOVES || bytes.len() != HEADER_SIZE + count * ENTRY_SIZE {
            return Err(bad);
        }
        let entries = &bytes[HEADER_SIZE..];
        let chunks = entries.as_chunks::<ENTRY_SIZE>().0;
        for (i, chunk) in chunks.iter().enumerate() {
            let a = entry(chunk)?;
            if a.slot == self_slot {
                return Err(bad);
            }
            for other in chunks.iter().skip(i + 1) {
                let b = entry(other)?;
                if a.slot == b.slot || a.handle == b.handle {
                    return Err(bad);
                }
            }
        }
        Ok(SnapshotRequest {
            suspended: flags & START_SUSPENDED != 0,
            job: Handle::new(wide(16)?).ok_or(Status::BadHandle)?,
            group: Handle::new(wide(24)?),
            self_slot: (self_slot != 0).then_some(self_slot),
            entries,
        })
    }

    pub fn moves(&self) -> impl Iterator<Item = Move> + 'a {
        // `decode` checked every entry.
        self.entries
            .as_chunks::<ENTRY_SIZE>()
            .0
            .iter()
            .filter_map(|chunk| entry(chunk).ok())
    }

    pub fn move_count(&self) -> usize {
        self.entries.len() / ENTRY_SIZE
    }
}

/// Builds a buffer. The first failure sticks, so a caller can write
/// everything and check once.
pub struct Encoder<'a> {
    bytes: &'a mut [u8],
    length: usize,
    moves: usize,
    overflow: bool,
}

impl<'a> Encoder<'a> {
    pub fn new(bytes: &'a mut [u8], job: Handle) -> Encoder<'a> {
        let mut e = Encoder {
            bytes,
            length: HEADER_SIZE,
            moves: 0,
            overflow: false,
        };
        if e.bytes.len() < HEADER_SIZE {
            e.overflow = true;
            return e;
        }
        e.bytes[..HEADER_SIZE].fill(0);
        e.bytes[..4].copy_from_slice(MAGIC);
        e.set(4, HEADER_SIZE as u32);
        e.set_wide(16, job.raw());
        e
    }

    fn set(&mut self, at: usize, value: u32) {
        if !self.overflow {
            self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn set_wide(&mut self, at: usize, value: u64) {
        if !self.overflow {
            self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }

    pub fn suspended(&mut self) -> &mut Self {
        self.set(8, START_SUSPENDED);
        self
    }

    pub fn group(&mut self, group: Handle) -> &mut Self {
        self.set_wide(24, group.raw());
        self
    }

    pub fn self_slot(&mut self, slot: u32) -> &mut Self {
        self.set(32, slot);
        self
    }

    pub fn moved(&mut self, entry: Move) -> &mut Self {
        match self.bytes.get_mut(self.length..self.length + ENTRY_SIZE) {
            Some(target) if !self.overflow => {
                target[..4].copy_from_slice(&entry.slot.to_le_bytes());
                target[4..8].fill(0);
                target[8..].copy_from_slice(&entry.handle.raw().to_le_bytes());
                self.length += ENTRY_SIZE;
                self.moves += 1;
            }
            _ => self.overflow = true,
        }
        self
    }

    /// The length of the finished buffer, or `None` if anything did not fit.
    pub fn finish(&mut self) -> Option<usize> {
        self.set(36, self.moves as u32);
        (!self.overflow).then_some(self.length)
    }
}

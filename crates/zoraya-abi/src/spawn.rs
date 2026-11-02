//! The flat buffer `spawn` takes, and the block a child reads its vectors
//! from (ADR-0042). Integers are little-endian. There are no pointers, so the
//! kernel copies a buffer once and everything after that is a pure function
//! of those bytes.
//!
//! ```text
//! magic "ZSP2"    4 bytes
//! header size     u32
//! flags           u32
//! reserved        u32, zero
//! job, image      u64 each
//! group           u64, 0 for none
//! argc, envc      u32 each
//! handle count    u32
//! reserved        u32, zero
//! dispositions    REQUEST_KINDS bytes
//! reserved        4 bytes, zero
//! handle entries  ENTRY_SIZE bytes each: slot, handle, rights, mode
//! strings         argc + envc of: u32 length, bytes
//! ```
//!
//! The decoder is total: any input gives a result, never a panic. Whatever it
//! accepts, iteration over it cannot fail.

use crate::{Disposition, Handle, MAX_HANDLES, REQUEST_KINDS, Rights, Status};

const MAGIC: &[u8; 4] = b"ZSP2";
/// The size of this version's header.
pub const HEADER_SIZE: usize = 60 + REQUEST_KINDS;
const DISPOSITIONS: usize = 56;
const ENTRY_SIZE: usize = 24;

/// Longest `spawn` buffer.
pub const MAX_SPAWN_SIZE: usize = 64 * 1024;
/// Longest handle list.
pub const MAX_SPAWN_HANDLES: usize = 64;
/// Start with the thread suspended, until `process_resume`.
pub const START_SUSPENDED: u32 = 1;

/// End of the user range (ADR-0031).
const USER_END: u64 = 1 << 47;
const PAGE: u64 = 4096;
/// Top of the user stack, one page below the end of the user range.
pub const USER_STACK_TOP: u64 = USER_END - PAGE;
/// Size of a process's stack. The page below it stays unmapped.
pub const USER_STACK_SIZE: u64 = 64 * 1024;
/// Room for the startup block.
pub const STARTUP_BLOCK_MAX: u64 = MAX_SPAWN_SIZE as u64 + PAGE;
/// Where a process finds its vectors, read-only, below its stack's guard page.
pub const STARTUP_BLOCK: u64 = USER_STACK_TOP - USER_STACK_SIZE - PAGE - STARTUP_BLOCK_MAX;

/// How a listed handle reaches the child.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Move = 0,
    Duplicate = 1,
}

/// One entry of the handle list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleEntry {
    pub slot: u32,
    pub handle: Handle,
    pub rights: Rights,
    pub transfer: Transfer,
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    let b = bytes.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes(b.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

/// `count` strings from `bytes`, which must end exactly at the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strings<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> Strings<'a> {
    /// Splits `count` strings off the front of `bytes`.
    fn split(bytes: &'a [u8], count: usize) -> Option<(Strings<'a>, &'a [u8])> {
        let mut at = 0usize;
        for _ in 0..count {
            let length = u32_at(bytes, at)? as usize;
            at = at.checked_add(4)?.checked_add(length)?;
            if at > bytes.len() {
                return None;
            }
        }
        let (taken, rest) = bytes.split_at(at);
        Some((
            Strings {
                bytes: taken,
                count,
            },
            rest,
        ))
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The bytes these strings occupy.
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

impl<'a> Iterator for Strings<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.count == 0 {
            return None;
        }
        let length = u32_at(self.bytes, 0)? as usize;
        let (string, rest) = self.bytes.get(4..)?.split_at_checked(length)?;
        self.bytes = rest;
        self.count -= 1;
        Some(string)
    }
}

/// The argument vector: byte strings, never one string (INV-0003-3).
pub type ArgVec<'a> = Strings<'a>;
/// The environment vector: byte strings, never one string (INV-0003-3).
pub type EnvVec<'a> = Strings<'a>;

/// A checked `spawn` buffer.
#[derive(Debug, Clone, Copy)]
pub struct SpawnRequest<'a> {
    pub suspended: bool,
    pub job: Handle,
    pub image: Handle,
    pub group: Option<Handle>,
    pub dispositions: [Disposition; REQUEST_KINDS],
    entries: &'a [u8],
    pub argv: ArgVec<'a>,
    pub envp: EnvVec<'a>,
}

fn entry(bytes: &[u8]) -> Result<HandleEntry, Status> {
    let bad = Status::InvalidArgument;
    let slot = u32_at(bytes, 0).ok_or(bad)?;
    if slot == 0 || slot > MAX_HANDLES || u32_at(bytes, 4) != Some(0) {
        return Err(bad);
    }
    Ok(HandleEntry {
        slot,
        handle: u64_at(bytes, 8)
            .and_then(Handle::new)
            .ok_or(Status::BadHandle)?,
        rights: u32_at(bytes, 16).and_then(Rights::from_raw).ok_or(bad)?,
        transfer: match u32_at(bytes, 20).ok_or(bad)? {
            0 => Transfer::Move,
            1 => Transfer::Duplicate,
            _ => return Err(bad),
        },
    })
}

impl<'a> SpawnRequest<'a> {
    /// Checks all of `bytes`. Trailing bytes, unknown flags and nonzero
    /// reserved fields are refused (CP-11).
    pub fn decode(bytes: &'a [u8]) -> Result<SpawnRequest<'a>, Status> {
        let bad = Status::InvalidArgument;
        if bytes.len() > MAX_SPAWN_SIZE || bytes.len() < HEADER_SIZE || &bytes[..4] != MAGIC {
            return Err(bad);
        }
        let field = |at: usize| u32_at(bytes, at).ok_or(bad);
        let wide = |at: usize| u64_at(bytes, at).ok_or(bad);
        if field(4)? as usize != HEADER_SIZE {
            return Err(bad);
        }
        let flags = field(8)?;
        let reserved = field(12)? | field(HEADER_SIZE - 4)? | field(52)?;
        if flags & !START_SUSPENDED != 0 || reserved != 0 {
            return Err(bad);
        }
        let mut dispositions = [Disposition::Terminate; REQUEST_KINDS];
        for (i, d) in dispositions.iter_mut().enumerate() {
            *d = Disposition::from_raw(u32::from(bytes[DISPOSITIONS + i])).ok_or(bad)?;
        }
        let count = field(48)? as usize;
        if count > MAX_SPAWN_HANDLES {
            return Err(bad);
        }
        let (entries, rest) = bytes[HEADER_SIZE..]
            .split_at_checked(count * ENTRY_SIZE)
            .ok_or(bad)?;
        let (argv, rest) = Strings::split(rest, field(40)? as usize).ok_or(bad)?;
        let (envp, rest) = Strings::split(rest, field(44)? as usize).ok_or(bad)?;
        if !rest.is_empty() {
            return Err(bad);
        }
        let request = SpawnRequest {
            suspended: flags & START_SUSPENDED != 0,
            job: Handle::new(wide(16)?).ok_or(Status::BadHandle)?,
            image: Handle::new(wide(24)?).ok_or(Status::BadHandle)?,
            group: Handle::new(wide(32)?),
            dispositions,
            entries,
            argv,
            envp,
        };
        // Each slot once; a moved handle in no other entry.
        for (i, chunk) in entries.as_chunks::<ENTRY_SIZE>().0.iter().enumerate() {
            let a = entry(chunk)?;
            for other in entries.as_chunks::<ENTRY_SIZE>().0.iter().skip(i + 1) {
                let b = entry(other)?;
                let moved = a.transfer == Transfer::Move || b.transfer == Transfer::Move;
                if a.slot == b.slot || (moved && a.handle == b.handle) {
                    return Err(bad);
                }
            }
        }
        Ok(request)
    }

    pub fn handles(&self) -> impl Iterator<Item = HandleEntry> + 'a {
        // `decode` checked every entry.
        self.entries
            .as_chunks::<ENTRY_SIZE>()
            .0
            .iter()
            .filter_map(|chunk| entry(chunk).ok())
    }

    pub fn handle_count(&self) -> usize {
        self.entries.len() / ENTRY_SIZE
    }
}

/// The startup block: `argc`, `envc`, then the strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupBlock<'a> {
    pub argv: ArgVec<'a>,
    pub envp: EnvVec<'a>,
}

impl<'a> StartupBlock<'a> {
    pub fn decode(bytes: &'a [u8]) -> Option<StartupBlock<'a>> {
        let argc = u32_at(bytes, 0)? as usize;
        let envc = u32_at(bytes, 4)? as usize;
        let (argv, rest) = Strings::split(bytes.get(8..)?, argc)?;
        let (envp, _) = Strings::split(rest, envc)?;
        Some(StartupBlock { argv, envp })
    }
}

/// The first 8 bytes of a startup block; the strings follow as they came.
pub fn startup_header(argc: usize, envc: usize) -> [u8; 8] {
    let mut h = [0; 8];
    h[0..4].copy_from_slice(&(argc as u32).to_le_bytes());
    h[4..8].copy_from_slice(&(envc as u32).to_le_bytes());
    h
}

/// Builds a `spawn` buffer. The first failure sticks, so a caller can write
/// everything and check once.
pub struct Encoder<'a> {
    bytes: &'a mut [u8],
    length: usize,
    entries: usize,
    args: usize,
    envs: usize,
    overflow: bool,
}

impl<'a> Encoder<'a> {
    pub fn new(bytes: &'a mut [u8], job: Handle, image: Handle) -> Encoder<'a> {
        let mut e = Encoder {
            bytes,
            length: HEADER_SIZE,
            entries: 0,
            args: 0,
            envs: 0,
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
        e.set_wide(24, image.raw());
        e
    }

    fn set_wide(&mut self, at: usize, value: u64) {
        if !self.overflow {
            self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn set(&mut self, at: usize, value: u32) {
        if !self.overflow {
            self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        match self.bytes.get_mut(self.length..self.length + bytes.len()) {
            Some(target) if !self.overflow => {
                target.copy_from_slice(bytes);
                self.length += bytes.len();
            }
            _ => self.overflow = true,
        }
    }

    pub fn suspended(&mut self) -> &mut Self {
        self.set(8, START_SUSPENDED);
        self
    }

    pub fn group(&mut self, group: Handle) -> &mut Self {
        self.set_wide(32, group.raw());
        self
    }

    pub fn disposition(&mut self, kind: crate::RequestKind, d: Disposition) -> &mut Self {
        if !self.overflow {
            self.bytes[DISPOSITIONS + kind.raw() as usize] = d as u8;
        }
        self
    }

    /// Handles come first in the buffer, so all of them before any string.
    pub fn handle(&mut self, entry: HandleEntry) -> &mut Self {
        if self.args + self.envs != 0 {
            self.overflow = true;
        }
        self.push(&entry.slot.to_le_bytes());
        self.push(&[0; 4]);
        self.push(&entry.handle.raw().to_le_bytes());
        self.push(&entry.rights.raw().to_le_bytes());
        self.push(&(entry.transfer as u32).to_le_bytes());
        self.entries += 1;
        self
    }

    /// Arguments come before environment strings.
    pub fn arg(&mut self, arg: &[u8]) -> &mut Self {
        if self.envs != 0 {
            self.overflow = true;
        }
        self.push(&(arg.len() as u32).to_le_bytes());
        self.push(arg);
        self.args += 1;
        self
    }

    pub fn env(&mut self, env: &[u8]) -> &mut Self {
        self.push(&(env.len() as u32).to_le_bytes());
        self.push(env);
        self.envs += 1;
        self
    }

    /// The length of the finished buffer, or `None` if anything did not fit.
    pub fn finish(&mut self) -> Option<usize> {
        self.set(40, self.args as u32);
        self.set(44, self.envs as u32);
        self.set(48, self.entries as u32);
        (!self.overflow).then_some(self.length)
    }
}

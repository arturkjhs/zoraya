//! Zoraya's filesystem storage engine.
//!
//! A library that knows nothing about IPC: the filesystem service (stage 4)
//! wraps it. It implements the accepted filesystem ADRs:
//!
//! - ADR-0010, guarantees to clients: every request atomic, a crash leaves a
//!   prefix of the completion order, `sync` for durability, sticky fail-stop
//!   on storage errors, unnamed objects with atomic publish ([`Volume`]);
//! - ADR-0011, a copy-on-write B+tree with dual root records in a ring and
//!   no replay ([`format`], `tree`, [`Volume::commit`]);
//! - ADR-0012, CRC32C of every block in its parent pointer and a total
//!   decoder ([`crc32c`], [`format`]);
//! - ADR-0013, names ([`name`]); ADR-0014, metadata ([`format::Attr`],
//!   [`Timestamp`]); ADR-0015, the block device ([`BlockDevice`]);
//! - and an offline checker ([`check`]) used by `zoraya-fsck` and the tests.
//!
//! Choices the ADRs leave open are listed in proposed ADR-0016.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod check;
pub mod crc32c;
pub mod device;
mod error;
pub mod format;
mod io;
pub mod name;
mod root;
mod space;
pub mod time;
mod tree;
mod volume;

pub use device::{BLOCK_SIZE, Block, BlockDevice, DevError, SECTOR_SIZE};
pub use error::FsError;
pub use format::{Attr, ObjKind};
pub use io::{CACHE_ENTRIES, FailReason};
pub use name::{Name, NameError, RelPath};
pub use root::{RootSelection, select_root};
pub use time::{Clock, Timestamp};
pub use volume::{
    COMMIT_THRESHOLD_BLOCKS, DEFAULT_DIR_MODE, DEFAULT_FILE_MODE, DELETE_RESERVE, DirEntry, Handle,
    ID_BATCH, MAX_ATOMIC_WRITE, MkfsOptions, MountOptions, OpenOptions, PublishMode, RenameMode,
    T_COMMIT_NS, Volume, mkfs,
};

/// Upper bound on decoded node bytes held by the cache and by one root-to-
/// leaf path, beyond `MAX_DEPTH × BLOCK_SIZE` (ADR-0012 rule 4, INV-0012-4).
/// A decoded node can be larger than its block (keys and values are held as
/// Rust values), so the bound is generous: each cached or on-path node is
/// counted at 64 KiB.
pub const CACHE_CAP: usize = (CACHE_ENTRIES + format::MAX_DEPTH as usize) * 64 * 1024;

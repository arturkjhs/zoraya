//! Request errors (ADR-0010 rule 6, ADR-0013 rule 7).

use alloc::string::String;

use crate::name::NameError;

/// Why a request failed. Every request either fails with one of these and
/// changes nothing, or succeeds completely (ADR-0010 rule 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    Exists,
    /// The space the request's commit needs cannot be reserved
    /// (ADR-0010 rule 6, INV-0010-5).
    NoSpace,
    NotEmpty,
    InvalidName(NameError),
    NotDir,
    IsDir,
    /// A path component, or the final component of an open that did not ask
    /// for the link itself, is a link object. The server does not follow it
    /// (ADR-0013 rule 7). `remaining` is the path after the link, empty if
    /// the link was the last component.
    IsLink {
        remaining: String,
        target: String,
    },
    /// The request is malformed for its object: a write to a directory, a
    /// rename of a directory into its own subtree, an out-of-range time or
    /// mode, a write larger than `MAX_ATOMIC_WRITE`.
    InvalidArgument,
    /// A data block failed its checksum or could not be read; that read
    /// only (ADR-0012 rule 3). For metadata, the volume is now `Failed`.
    Corrupt,
    /// The volume is `Failed` (ADR-0010 rule 6). Sticky until remount.
    StorageFailed,
    /// The volume is mounted read-only.
    ReadOnly,
    /// The handle is not open.
    BadHandle,
    /// An internal consistency check of the engine failed. The volume is
    /// now `Failed`. Never expected; reported rather than panicking.
    Internal,
}

impl From<NameError> for FsError {
    fn from(e: NameError) -> Self {
        FsError::InvalidName(e)
    }
}

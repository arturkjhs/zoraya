//! What a native program starts with when `zosh` runs it (ADR-0153 rule 8):
//! the start-up record, which `zoraya-rt` reads once before `main` from a
//! channel at [`slot::STARTUP`]. It has none of the POSIX facts of ADR-0107's
//! `Bootstrap` (pid, uid, umask, ignored signals): only where the program
//! is, what it was given, and its streams. Integers are little-endian; the
//! decoder is total (INV-0153-5).
//!
//! ```text
//! magic "ZNS1"     4 bytes
//! header size      u32
//! flags            u32, zero (reserved)
//! entry count      u32
//! in, out, err     u8 each: handle indices, or NO_HANDLE
//! job, report      u8 each: the program's own job, the REPORT channel
//! reserved         3 bytes, zero
//! cwd              u32 length, bytes: the current directory, absolute
//! entries          kind u8, handle u8, two zero bytes, u32 length, path;
//!                  longest path first
//! ```
//!
//! A handle index names a handle of the message the record came in, in the
//! order of its handle list, each at most once; `NO_HANDLE` is none. Paths
//! are absolute and normal (ADR-0153 rule 6); resolving a program's path
//! against them is the one resolver's (TASK-18 plan step 7, with 6d).
//! Refused paths are reported on `report` with [`crate::posix::Report`]'s
//! encoding (rule 11).

use crate::posix::{MAX_MESSAGE_HANDLES, MAX_PATH, NO_HANDLE, is_absolute_normal};

const MAGIC: &[u8; 4] = b"ZNS1";
/// The size of this version's header.
pub const HEADER_SIZE: usize = 24;
const ENTRY_HEADER: usize = 8;
/// The most entries one record holds.
pub const MAX_ENTRIES: usize = 64;

/// Fixed slots of a native program run by `zosh`.
pub mod slot {
    use crate::Handle;
    /// The start-up channel's end: read before `main` and closed.
    pub const STARTUP: Handle = Handle(1000);
    /// The command's three streams (ADR-0152 rule 1): the read end of `in`,
    /// the write ends of `out` and `err`. A program without them (a test
    /// program the kernel started, a service) prints to the console.
    pub const IN: Handle = Handle(1001);
    pub const OUT: Handle = Handle(1002);
    pub const ERR: Handle = Handle(1003);
}

/// What an entry is (ADR-0153 rule 2).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A directory connection: the path and everything below it, as its
    /// rights allow; or, narrowed, one name in it.
    Directory = 0,
    /// A file connection: the path alone.
    File = 1,
    /// A designated path that did not exist when the line started: no
    /// handle, and the program finds nothing there (`NotFound`).
    Missing = 2,
}

impl EntryKind {
    pub const fn from_raw(raw: u8) -> Option<EntryKind> {
        Some(match raw {
            0 => EntryKind::Directory,
            1 => EntryKind::File,
            2 => EntryKind::Missing,
            _ => return None,
        })
    }

    pub const fn has_handle(self) -> bool {
        !matches!(self, EntryKind::Missing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub path: &'a str,
    pub kind: EntryKind,
    /// An index into the message's handles, or `NO_HANDLE`.
    pub handle: u8,
}

/// The handle indices of the fixed parts, each `NO_HANDLE` if absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slots {
    pub input: u8,
    pub output: u8,
    pub error: u8,
    pub job: u8,
    pub report: u8,
}

impl Slots {
    pub const NONE: Slots = Slots {
        input: NO_HANDLE,
        output: NO_HANDLE,
        error: NO_HANDLE,
        job: NO_HANDLE,
        report: NO_HANDLE,
    };

    fn all(&self) -> [u8; 5] {
        [self.input, self.output, self.error, self.job, self.report]
    }
}

/// A decoded record. The entries are kept as the bytes they were checked
/// in, and iterate without failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Startup<'a> {
    pub cwd: &'a str,
    pub slots: Slots,
    entries: &'a [u8],
    entry_count: usize,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

fn string_at(bytes: &[u8], at: usize) -> Option<(&str, usize)> {
    let length = u32_at(bytes, at)? as usize;
    if length > MAX_PATH {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(length)?;
    let text = core::str::from_utf8(bytes.get(start..end)?).ok()?;
    Some((text, end))
}

impl<'a> Startup<'a> {
    /// Decodes a record that came with `handles` handles. Any input gives a
    /// result, never a panic.
    pub fn decode(bytes: &'a [u8], handles: usize) -> Option<Startup<'a>> {
        if bytes.get(..4)? != MAGIC || u32_at(bytes, 4)? as usize != HEADER_SIZE {
            return None;
        }
        let flags = u32_at(bytes, 8)?;
        let entry_count = u32_at(bytes, 12)? as usize;
        let s = bytes.get(16..HEADER_SIZE)?;
        let slots = Slots {
            input: s[0],
            output: s[1],
            error: s[2],
            job: s[3],
            report: s[4],
        };
        if flags != 0 || s[5..8] != [0, 0, 0] || entry_count > MAX_ENTRIES {
            return None;
        }
        let limit = handles.min(MAX_MESSAGE_HANDLES);
        let mut used = 0u64;
        let mut take = |index: u8| -> bool {
            if index == NO_HANDLE {
                return true;
            }
            if (index as usize) >= limit || used & (1 << index) != 0 {
                return false;
            }
            used |= 1 << index;
            true
        };
        if !slots.all().into_iter().all(&mut take) {
            return None;
        }
        let (cwd, mut at) = string_at(bytes, HEADER_SIZE)?;
        if !is_absolute_normal(cwd) {
            return None;
        }
        let start = at;
        let mut previous: Option<&str> = None;
        for _ in 0..entry_count {
            let head = bytes.get(at..at.checked_add(ENTRY_HEADER)?)?;
            let kind = EntryKind::from_raw(head[0])?;
            let handle = head[1];
            let (path, end) = string_at(bytes, at + 4)?;
            let handle_ok = if kind.has_handle() {
                handle != NO_HANDLE && take(handle)
            } else {
                handle == NO_HANDLE
            };
            if head[2..4] != [0, 0]
                || !handle_ok
                || !is_absolute_normal(path)
                || previous.is_some_and(|p| p.len() < path.len() || p == path)
            {
                return None;
            }
            previous = Some(path);
            at = end;
        }
        if at != bytes.len() {
            return None;
        }
        Some(Startup {
            cwd,
            slots,
            entries: &bytes[start..at],
            entry_count,
        })
    }

    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    /// The entries, longest path first.
    pub fn entries(&self) -> impl Iterator<Item = Entry<'a>> + 'a {
        let bytes = self.entries;
        let mut at = 0;
        (0..self.entry_count).map_while(move |_| {
            let head = bytes.get(at..at + ENTRY_HEADER)?;
            let (path, end) = string_at(bytes, at + 4)?;
            let entry = Entry {
                path,
                kind: EntryKind::from_raw(head[0])?,
                handle: head[1],
            };
            at = end;
            Some(entry)
        })
    }
}

/// Writes a record into `out`, and gives its size; `None` if it does not fit
/// or `decode` would refuse it (with `handles` handles in its message). The
/// entries are sorted longest path first here.
pub fn encode(
    cwd: &str,
    slots: &Slots,
    entries: &[Entry<'_>],
    handles: usize,
    out: &mut [u8],
) -> Option<usize> {
    if entries.len() > MAX_ENTRIES {
        return None;
    }
    let mut sorted = [None; MAX_ENTRIES];
    for (i, e) in entries.iter().enumerate() {
        sorted[i] = Some(*e);
    }
    let sorted = &mut sorted[..entries.len()];
    sorted.sort_unstable_by(|a, b| {
        let (a, b) = (a.map_or(0, |e| e.path.len()), b.map_or(0, |e| e.path.len()));
        b.cmp(&a)
    });
    let mut at = 0usize;
    let mut put = |bytes: &[u8]| -> Option<()> {
        let end = at.checked_add(bytes.len())?;
        out.get_mut(at..end)?.copy_from_slice(bytes);
        at = end;
        Some(())
    };
    put(MAGIC)?;
    for word in [HEADER_SIZE as u32, 0, u32::try_from(entries.len()).ok()?] {
        put(&word.to_le_bytes())?;
    }
    let s = slots.all();
    put(&[s[0], s[1], s[2], s[3], s[4], 0, 0, 0])?;
    put(&u32::try_from(cwd.len()).ok()?.to_le_bytes())?;
    put(cwd.as_bytes())?;
    for e in sorted.iter().flatten() {
        put(&[e.kind as u8, e.handle, 0, 0])?;
        put(&u32::try_from(e.path.len()).ok()?.to_le_bytes())?;
        put(e.path.as_bytes())?;
    }
    let size = at;
    Startup::decode(&out[..size], handles).map(|_| size)
}

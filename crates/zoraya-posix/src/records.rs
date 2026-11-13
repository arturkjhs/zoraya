//! Advisory record locks (ADR-0317): the table of `fcntl`'s `F_SETLK`,
//! `F_SETLKW`, `F_GETLK` and the open-file-description forms `F_OFD_*`, of
//! one process.
//!
//! A lock is a byte range of a file, read or write, with an owner. The owner
//! of a POSIX lock is the process (all its threads, any of its fds); the
//! owner of an OFD lock is one open file description. Locks of one owner never
//! conflict with each other; a read lock conflicts with another owner's write
//! lock, a write lock with any other owner's lock. Setting a lock over a
//! range an owner already has replaces its part there (split, merge, change
//! of kind), as POSIX has it.
//!
//! The table lives in the process: another process does not see these locks
//! (ADR-0317 item 2). The pure logic is here and host-tested; `process`
//! decodes `struct flock` and finds the file's identity.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

/// The most records at once: `ENOLCK` past it (a split makes one more).
pub const MAX_RECORDS: usize = 4096;

/// A file: its volume and its id, as `st_dev` and `st_ino`.
pub type File = (u64, u64);

/// "To the end of the file, and beyond".
pub const EOF: u64 = u64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner<O> {
    /// The process (`F_SETLK`, `F_GETLK`).
    Process,
    /// An open file description (`F_OFD_SETLK`, `F_OFD_GETLK`).
    Description(O),
}

/// A lock: bytes `start..end` (`end` exclusive; [`EOF`] is unbounded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record<O> {
    pub file: File,
    pub owner: Owner<O>,
    pub write: bool,
    pub start: u64,
    pub end: u64,
}

impl<O> Record<O> {
    fn overlaps(&self, start: u64, end: u64) -> bool {
        self.start < end && start < self.end
    }
}

#[derive(Debug)]
pub struct Locks<O> {
    records: Vec<Record<O>>,
}

impl<O> Default for Locks<O> {
    fn default() -> Self {
        Locks {
            records: Vec::new(),
        }
    }
}

impl<O: Copy + PartialEq> Locks<O> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// The first lock of another owner that would stop `owner` from taking
    /// `start..end` for reading (`write` false) or writing.
    pub fn conflict(
        &self,
        file: File,
        owner: Owner<O>,
        write: bool,
        start: u64,
        end: u64,
    ) -> Option<Record<O>> {
        self.records
            .iter()
            .find(|r| {
                r.file == file && r.owner != owner && (r.write || write) && r.overlaps(start, end)
            })
            .copied()
    }

    /// Takes `start..end` for `owner`, replacing what it had there.
    /// [`Full::Conflict`] means another owner's lock is in the way
    /// ([`Locks::conflict`] says which).
    pub fn lock(
        &mut self,
        file: File,
        owner: Owner<O>,
        write: bool,
        start: u64,
        end: u64,
    ) -> Result<(), Full> {
        if self.conflict(file, owner, write, start, end).is_some() {
            return Err(Full::Conflict);
        }
        // Worst case: the range splits one record in two, and adds one.
        if self.records.len() + 2 > MAX_RECORDS {
            return Err(Full::Table);
        }
        self.subtract(file, owner, start, end);
        let (mut start, mut end) = (start, end);
        // Merge with the owner's neighbours of the same kind.
        self.records.retain(|r| {
            let same = r.file == file && r.owner == owner && r.write == write;
            if same && r.end == start {
                start = r.start;
                false
            } else if same && r.start == end {
                end = r.end;
                false
            } else {
                true
            }
        });
        self.records.push(Record {
            file,
            owner,
            write,
            start,
            end,
        });
        Ok(())
    }

    /// Gives up `start..end` of `owner`'s locks on `file`.
    pub fn unlock(&mut self, file: File, owner: Owner<O>, start: u64, end: u64) {
        self.subtract(file, owner, start, end);
    }

    fn subtract(&mut self, file: File, owner: Owner<O>, start: u64, end: u64) {
        let mut rest = Vec::new();
        self.records.retain(|r| {
            if r.file != file || r.owner != owner || !r.overlaps(start, end) {
                return true;
            }
            if r.start < start {
                rest.push(Record { end: start, ..*r });
            }
            if end < r.end {
                rest.push(Record { start: end, ..*r });
            }
            false
        });
        self.records.extend(rest);
    }

    /// The process closed an fd of `file`: POSIX releases all its locks on
    /// the file, whichever fd took them (OFD locks stay with their
    /// description).
    pub fn release_process(&mut self, file: File) {
        self.records
            .retain(|r| !(r.file == file && r.owner == Owner::Process));
    }

    /// The last fd of the description `owner` was closed.
    pub fn release_description(&mut self, owner: O) {
        self.records
            .retain(|r| r.owner != Owner::Description(owner));
    }
}

/// Why [`Locks::lock`] did not take the range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Full {
    Conflict,
    Table,
}

/// The bytes of a `struct flock` (musl, aarch64): `short l_type, short
/// l_whence, off_t l_start, off_t l_len, pid_t l_pid`.
pub const FLOCK_SIZE: usize = 32;

pub const F_RDLCK: i16 = 0;
pub const F_WRLCK: i16 = 1;
pub const F_UNLCK: i16 = 2;

pub const F_GETLK: i32 = 5;
pub const F_SETLK: i32 = 6;
pub const F_SETLKW: i32 = 7;
pub const F_OFD_GETLK: i32 = 36;
pub const F_OFD_SETLK: i32 = 37;
pub const F_OFD_SETLKW: i32 = 38;

/// Whether `command` is one of the record-lock commands.
pub fn is_lock_command(command: i32) -> bool {
    matches!(
        command,
        F_GETLK | F_SETLK | F_SETLKW | F_OFD_GETLK | F_OFD_SETLK | F_OFD_SETLKW
    )
}

/// Whether `command` waits for a conflicting lock to go.
pub fn waits(command: i32) -> bool {
    matches!(command, F_SETLKW | F_OFD_SETLKW)
}

/// The decoded range of a `struct flock`, with `base` the offset its
/// `l_whence` counts from (0, the current position or the size): bytes
/// `start..end`, or `None` for an empty or negative-start one that `fcntl`
/// refuses with `EINVAL`.
pub fn range(base: i64, start: i64, len: i64) -> Option<(u64, u64)> {
    let at = base.checked_add(start)?;
    if len >= 0 {
        let from = u64::try_from(at).ok()?;
        if len == 0 {
            return Some((from, EOF));
        }
        Some((from, from.saturating_add(len as u64).min(EOF - 1)))
    } else {
        // A negative length covers the bytes before `at`.
        let from = u64::try_from(at.checked_add(len)?).ok()?;
        Some((from, at as u64))
    }
}

pub fn flock_type(b: &[u8; FLOCK_SIZE]) -> i16 {
    i16::from_le_bytes([b[0], b[1]])
}

pub fn flock_whence(b: &[u8; FLOCK_SIZE]) -> i16 {
    i16::from_le_bytes([b[2], b[3]])
}

pub fn flock_start(b: &[u8; FLOCK_SIZE]) -> i64 {
    i64::from_le_bytes(b[8..16].try_into().unwrap_or([0; 8]))
}

pub fn flock_len(b: &[u8; FLOCK_SIZE]) -> i64 {
    i64::from_le_bytes(b[16..24].try_into().unwrap_or([0; 8]))
}

pub fn flock_pid(b: &[u8; FLOCK_SIZE]) -> i32 {
    i32::from_le_bytes(b[24..28].try_into().unwrap_or([0; 4]))
}

/// What `F_GETLK` answers: the lock in the way, as `SEEK_SET` bytes.
pub fn put_conflict(b: &mut [u8; FLOCK_SIZE], write: bool, start: u64, end: u64, pid: i32) {
    let len = if end == EOF { 0 } else { (end - start) as i64 };
    b[0..2].copy_from_slice(&(if write { F_WRLCK } else { F_RDLCK }).to_le_bytes());
    b[2..4].copy_from_slice(&0i16.to_le_bytes());
    b[8..16].copy_from_slice(&(start as i64).to_le_bytes());
    b[16..24].copy_from_slice(&len.to_le_bytes());
    b[24..28].copy_from_slice(&pid.to_le_bytes());
}

/// What `F_GETLK` answers when nothing is in the way.
pub fn put_free(b: &mut [u8; FLOCK_SIZE]) {
    b[0..2].copy_from_slice(&F_UNLCK.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: File = (1, 7);
    const G: File = (1, 8);
    const P: Owner<u32> = Owner::Process;
    const A: Owner<u32> = Owner::Description(1);
    const B: Owner<u32> = Owner::Description(2);

    #[test]
    fn readers_share_and_a_writer_excludes() {
        let mut t = Locks::new();
        t.lock(F, A, false, 0, 10).unwrap();
        t.lock(F, B, false, 5, 15).unwrap();
        assert_eq!(t.lock(F, P, true, 8, 9), Err(Full::Conflict));
        assert_eq!(t.lock(F, P, true, 15, 20), Ok(()), "past both readers");
        assert_eq!(t.lock(F, A, true, 12, 18), Err(Full::Conflict));
        assert_eq!(t.lock(G, A, true, 0, 30), Ok(()), "another file");
    }

    #[test]
    fn one_owner_never_conflicts_with_itself_and_may_change_kind() {
        let mut t = Locks::new();
        t.lock(F, P, false, 0, 100).unwrap();
        t.lock(F, P, true, 40, 60).unwrap();
        assert_eq!(t.len(), 3, "read, write, read");
        assert!(t.conflict(F, A, false, 50, 51).is_some());
        assert!(t.conflict(F, A, false, 0, 40).is_none());
        // Downgrade the middle again: the three merge into one.
        t.lock(F, P, false, 40, 60).unwrap();
        assert_eq!(t.len(), 1);
        assert!(t.conflict(F, A, true, 99, 100).is_some());
    }

    #[test]
    fn unlock_splits_and_get_reports_the_first_conflict() {
        let mut t = Locks::new();
        t.lock(F, A, true, 0, EOF).unwrap();
        t.unlock(F, A, 10, 20);
        assert_eq!(t.len(), 2);
        assert!(t.conflict(F, B, false, 10, 20).is_none());
        let c = t.conflict(F, B, false, 15, 25).unwrap();
        assert_eq!((c.start, c.end, c.write, c.owner), (20, EOF, true, A));
        t.unlock(F, A, 0, EOF);
        assert!(t.is_empty());
    }

    #[test]
    fn a_process_lock_conflicts_with_a_description_of_its_own_process() {
        let mut t = Locks::new();
        t.lock(F, P, true, 0, 1).unwrap();
        assert_eq!(t.lock(F, A, false, 0, 1), Err(Full::Conflict));
        t.release_process(F);
        assert_eq!(t.lock(F, A, false, 0, 1), Ok(()));
        // An OFD lock stays when the process closes some fd of the file.
        t.release_process(F);
        assert_eq!(t.len(), 1);
        t.release_description(1);
        assert!(t.is_empty());
    }

    #[test]
    fn adjacent_locks_of_one_kind_merge_and_the_table_is_bounded() {
        let mut t = Locks::new();
        t.lock(F, P, true, 0, 10).unwrap();
        t.lock(F, P, true, 10, 20).unwrap();
        t.lock(F, P, true, 30, 40).unwrap();
        t.lock(F, P, true, 20, 30).unwrap();
        assert_eq!(t.len(), 1);
        let mut t = Locks::new();
        let mut n = 0;
        loop {
            match t.lock(F, P, true, n * 2, n * 2 + 1) {
                Ok(()) => n += 1,
                Err(e) => {
                    assert_eq!(e, Full::Table);
                    break;
                }
            }
        }
        assert!(n >= 4000 && t.len() <= MAX_RECORDS);
    }

    #[test]
    fn ranges_follow_flock() {
        assert_eq!(range(0, 10, 5), Some((10, 15)));
        assert_eq!(range(0, 10, 0), Some((10, EOF)));
        assert_eq!(range(100, -10, 5), Some((90, 95)));
        assert_eq!(range(0, 10, -4), Some((6, 10)), "bytes before 10");
        assert_eq!(range(0, -1, 1), None);
        assert_eq!(range(0, 2, -3), None);
        assert_eq!(range(0, 0, i64::MAX), Some((0, i64::MAX as u64)));
    }
}

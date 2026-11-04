//! A native program's namespace when `zosh` runs it (ADR-0153 rules 6, 8 and
//! 11): the start-up record at [`slot::STARTUP`], read once, and paths
//! resolved against it by the one resolver (`zoraya-resolve`). A path is
//! joined to the current directory and served by the entry with the longest
//! path that covers it; a path no entry covers is refused as not granted and
//! reported to the shell on `REPORT`, once per path and at most
//! [`MAX_REPORTS`] times.
//!
//! A program started some other way (by a test, or by a shell of TASK-11's
//! kind) has no record: [`get`] gives `None`, and it has no files.

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use zoraya_abi::Handle;
use zoraya_abi::native::{EntryKind, Startup, slot};
use zoraya_abi::posix::{Access, MAX_REPORTS, NO_HANDLE, Report};
use zoraya_proto_file::{open_flags, rights, status};

use crate::files::{File, FsResult};
use crate::{channel_read, channel_write, handle_close, handle_type};

/// One entry of the table, with its connection.
pub struct Entry {
    pub path: String,
    pub kind: EntryKind,
    pub file: Option<File>,
}

pub struct Namespace {
    pub cwd: String,
    pub entries: Vec<Entry>,
    report: Option<Handle>,
    reported: RefCell<Vec<String>>,
    dropped: RefCell<u32>,
}

struct Once(RefCell<Option<Option<Namespace>>>);

// SAFETY: a native program has one thread (ADR-0080's heap says so too).
unsafe impl Sync for Once {}

static NAMESPACE: Once = Once(RefCell::new(None));

/// The handle the record's index names, among those the message brought.
fn nth(handles: &[Handle], index: u8) -> Option<Handle> {
    (index != NO_HANDLE).then(|| handles.get(usize::from(index)).copied())?
}

fn read_record() -> Option<Namespace> {
    if handle_type(slot::STARTUP) != Ok("channel") {
        return None;
    }
    let mut bytes = alloc::vec![0u8; 64 * 1024];
    let mut handles = [slot::STARTUP; 64];
    let sizes = channel_read(slot::STARTUP, &mut bytes, &mut handles).ok()?;
    let _ = handle_close(slot::STARTUP);
    let got = &handles[..sizes.handles as usize];
    let record = Startup::decode(&bytes[..sizes.bytes as usize], got.len())?;
    let mut entries = Vec::new();
    for e in record.entries() {
        entries.push(Entry {
            path: e.path.into(),
            kind: e.kind,
            file: nth(got, e.handle).map(|end| File { end }),
        });
    }
    Some(Namespace {
        cwd: record.cwd.into(),
        entries,
        report: nth(got, record.slots.report),
        reported: RefCell::new(Vec::new()),
        dropped: RefCell::new(0),
    })
}

/// The namespace, read on first use.
pub fn get() -> Option<&'static Namespace> {
    let mut slot = NAMESPACE.0.borrow_mut();
    if slot.is_none() {
        *slot = Some(read_record());
    }
    let ns = slot.as_ref()?.as_ref()?;
    // SAFETY: the value is written once above and never moved or dropped:
    // the static lives as long as the program, and nothing borrows it
    // mutably again once it is `Some`.
    Some(unsafe { &*(ns as *const Namespace) })
}

impl Namespace {
    /// Absolute and normal (ADR-0153 rule 6).
    pub fn absolute(&self, path: &str) -> String {
        zoraya_resolve::join(&self.cwd, 0, path)
    }

    fn report(&self, abs: &str, access: Access) {
        let Some(end) = self.report else { return };
        let mut seen = self.reported.borrow_mut();
        if seen.iter().any(|p| p == abs) {
            return;
        }
        if seen.len() as u32 >= MAX_REPORTS {
            *self.dropped.borrow_mut() += 1;
            return;
        }
        let r = Report {
            access,
            dropped: *self.dropped.borrow(),
            path: abs,
        };
        let mut out = alloc::vec![0u8; Report::HEADER_SIZE + abs.len()];
        if let Some(n) = r.encode(&mut out) {
            // Never waits: a report that does not fit is dropped and counted.
            match channel_write(end, &out[..n], &[]) {
                Ok(()) => {
                    seen.push(abs.into());
                    *self.dropped.borrow_mut() = 0;
                }
                Err(_) => *self.dropped.borrow_mut() += 1,
            }
        }
    }

    /// The entry that serves `abs`, and the path below it.
    fn serving(&self, abs: &str, access: Access) -> FsResult<(&Entry, String)> {
        let found = zoraya_resolve::longest(self.entries.iter().map(|e| e.path.as_str()), abs);
        let Some((index, rest)) = found else {
            self.report(abs, access);
            return Err(status::ACCESS_DENIED);
        };
        let entry = &self.entries[index];
        match entry.kind {
            // A designated path that was missing when the line started.
            EntryKind::Missing => Err(status::NOT_FOUND),
            // A file entry serves its own path only.
            EntryKind::File if !rest.is_empty() => {
                self.report(abs, access);
                Err(status::ACCESS_DENIED)
            }
            _ => Ok((entry, rest.into())),
        }
    }

    /// Opens `path` with `flags` and `wanted` rights.
    pub fn open(&self, path: &str, flags: u32, wanted: u32) -> FsResult<File> {
        let abs = self.absolute(path);
        let writes =
            flags & (open_flags::CREATE | open_flags::TRUNCATE) != 0 || wanted & rights::WRITE != 0;
        let access = if writes { Access::Write } else { Access::Read };
        let (entry, rest) = self.serving(&abs, access)?;
        let file = entry.file.as_ref().ok_or(status::NOT_FOUND)?;
        if rest.is_empty() {
            file.clone_as(wanted)
        } else {
            file.open(&rest, flags, wanted)
        }
    }

    /// Runs `f` on the directory holding `path` and its last name, for the
    /// requests that name one component (make, remove, rename). When the
    /// parent is an entry itself, as a narrowed entry is, `f` gets that
    /// entry's own connection, with exactly the rights it was given; a
    /// directory below an entry is opened with `wanted` and `CLONE`.
    pub fn with_parent<T>(
        &self,
        path: &str,
        wanted: u32,
        f: impl FnOnce(&File, &str) -> FsResult<T>,
    ) -> FsResult<T> {
        let abs = self.absolute(path);
        let (dir, name) = match abs.rfind('/') {
            Some(0) => ("/", &abs[1..]),
            Some(cut) => (&abs[..cut], &abs[cut + 1..]),
            None => return Err(status::INVALID_NAME),
        };
        if name.is_empty() {
            return Err(status::INVALID_NAME);
        }
        let (entry, rest) = self.serving(dir, Access::Write)?;
        let file = entry.file.as_ref().ok_or(status::NOT_FOUND)?;
        if rest.is_empty() {
            f(file, name)
        } else {
            let wanted = wanted | rights::READ | rights::CLONE;
            let opened = file.open(&rest, open_flags::DIRECTORY, wanted)?;
            f(&opened, name)
        }
    }
}

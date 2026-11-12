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

use crate::files::{File, FsResult, Opened};
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
    net: Option<Handle>,
    /// The connection to the random service (ADR-0227 item 3), if any.
    random: Option<Handle>,
    /// The wall clock's base from the record (ADR-0226 item 4), or 0.
    realtime_base: u64,
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
        net: nth(got, record.slots.net),
        random: nth(got, record.slots.random),
        realtime_base: record.realtime_base,
        reported: RefCell::new(Vec::new()),
        dropped: RefCell::new(0),
    })
}

/// The start-up record with its connections, for a program that keeps
/// them itself (the shell in a terminal, ADR-0242's condition); `get` then
/// finds none.
pub fn take() -> Option<Namespace> {
    let mut slot = NAMESPACE.0.borrow_mut();
    let ns = read_record();
    *slot = Some(None);
    ns
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

/// The wall clock in nanoseconds since the epoch: the record's base plus the
/// monotonic clock (ADR-0226 item 4). `None` for a program started without a
/// record or with a base of 0 (ADR-0191 rule 5: it keeps the monotonic clock).
pub fn realtime() -> Option<u64> {
    let base = get()?.realtime_base;
    (base != 0).then(|| base.saturating_add(crate::clock_monotonic()))
}

impl Namespace {
    /// The connection to the random service the launcher gave (ADR-0227).
    pub fn random(&self) -> Option<Handle> {
        self.random
    }

    /// The network connection the launcher gave (ADR-0220 item 7), if any.
    pub fn net(&self) -> Option<Handle> {
        self.net
    }

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
        self.open_absolute(abs, flags, wanted, access)
    }

    /// Opens the absolute normal `abs`, following link objects as the
    /// resolver does (ADR-0153 rules 5 and 6, ADR-0107 item 7): a relative
    /// target from the link's directory, an absolute one from `/`, `..`
    /// lexically, and the result served by the entry that covers it, so
    /// that no link leads outside the table. At most `MAX_LINKS` links.
    fn open_absolute(
        &self,
        mut abs: String,
        flags: u32,
        wanted: u32,
        access: Access,
    ) -> FsResult<File> {
        for _ in 0..=zoraya_resolve::MAX_LINKS {
            let (entry, rest) = self.serving(&abs, access)?;
            let file = entry.file.as_ref().ok_or(status::NOT_FOUND)?;
            if rest.is_empty() {
                return file.clone_as(wanted);
            }
            match file.open_link(&rest, flags, wanted)? {
                Opened::File(f) => return Ok(f),
                Opened::Link { remaining, target } => {
                    let consumed = rest
                        .get(..rest.len().saturating_sub(remaining.len()))
                        .unwrap_or("")
                        .trim_end_matches('/');
                    let link = zoraya_resolve::join(&entry.path, 0, consumed);
                    let dir = match link.rfind('/') {
                        Some(0) | None => "/",
                        Some(cut) => &link[..cut],
                    };
                    let next = zoraya_resolve::join(dir, 0, &target);
                    abs = zoraya_resolve::join(&next, 0, remaining.trim_start_matches('/'));
                }
            }
        }
        Err(status::IS_LINK)
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
            // The directory through any links; the name itself is not
            // followed: removing or renaming a link acts on the link.
            let wanted = wanted | rights::READ | rights::CLONE;
            let opened =
                self.open_absolute(dir.into(), open_flags::DIRECTORY, wanted, Access::Write)?;
            f(&opened, name)
        }
    }
}

/// The shell's description, for `shell.home`'s default.
const ZOSH: &str = include_str!("../describe/system/zosh.cmd");

/// `shell.home` as the shell reads it from `/config/settings`.
pub fn shell_home(root: &File) -> String {
    let declared = zoraya_describe::parse(ZOSH.as_bytes())
        .map(|d| d.settings)
        .unwrap_or_default();
    let bytes = root
        .open("config/settings", 0, rights::READ)
        .ok()
        .and_then(|f| crate::files::read_all(&f).ok())
        .unwrap_or_default();
    let values = zoraya_settings::read(&bytes, &declared);
    String::from(values.get("shell.home").unwrap_or("/home"))
}

/// `~` and `~/...` as the home directory.
pub fn expand(path: &str, home: &str) -> String {
    if path == "~" {
        String::from(home)
    } else if let Some(rest) = path.strip_prefix("~/") {
        alloc::format!("{}/{rest}", home.trim_end_matches('/'))
    } else {
        String::from(path)
    }
}

/// A start-up record (ADR-0153 rule 8) of `holds`, each `(path, write)`
/// opened from `root` with ADR-0153's rights for `tree` (write) or `read`,
/// starting in `cwd`; gives the end to put at its slot. What a terminal's
/// shell holds instead of the volume's root (ADR-0242's condition).
pub fn record(root: &File, home: &str, cwd: &str, holds: &[(String, bool)]) -> Option<Handle> {
    use zoraya_abi::native::{self, Slots};
    use zoraya_abi::spawn::Transfer;
    use zoraya_abi::{ChannelEntry, ChannelObject, ObjectType};
    let mut paths = Vec::new();
    let mut handles: Vec<ChannelEntry> = Vec::new();
    let mut kinds = Vec::new();
    let close = |handles: &[ChannelEntry]| {
        for h in handles {
            let _ = handle_close(h.handle);
        }
    };
    for (path, write) in holds {
        let abs = expand(path, home);
        let wanted = if *write {
            rights::ALL & !rights::PUBLISH
        } else {
            rights::READ | rights::CLONE
        };
        let open = |rel: &str| match rel {
            "" => root.clone_as(wanted),
            rel => root.open(rel, 0, wanted),
        };
        let rel = abs.trim_start_matches('/');
        // A folder the shell may change that is not there yet (`/config`
        // on a new volume) is made, one level, so that the grant holds.
        let opened = match open(rel) {
            Err(status::NOT_FOUND) if *write && !rel.is_empty() => {
                let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
                let made = match parent {
                    "" => root.make_directory(name),
                    p => root
                        .open(p, open_flags::DIRECTORY, rights::ALL)
                        .and_then(|d| d.make_directory(name)),
                };
                made.and_then(|()| open(rel))
            }
            other => other,
        };
        match opened {
            Ok(file) => {
                let kind = match file.stat() {
                    Ok(s) if s.kind == zoraya_proto_file::kind::DIRECTORY => EntryKind::Directory,
                    _ => EntryKind::File,
                };
                handles.push(ChannelEntry {
                    handle: file.end,
                    rights: ChannelObject::RIGHTS,
                    transfer: Transfer::Move,
                });
                core::mem::forget(file);
                kinds.push((kind, (handles.len() - 1) as u8));
            }
            Err(_) => kinds.push((EntryKind::Missing, NO_HANDLE)),
        }
        paths.push(abs);
    }
    let entries: Vec<native::Entry<'_>> = paths
        .iter()
        .zip(&kinds)
        .map(|(p, (kind, handle))| native::Entry {
            path: p,
            kind: *kind,
            handle: *handle,
        })
        .collect();
    let mut bytes = alloc::vec![0u8; 16 * 1024];
    let base = root.time_base().unwrap_or(0);
    let Some(n) = native::encode(cwd, &Slots::NONE, &entries, handles.len(), base, &mut bytes)
    else {
        close(&handles);
        return None;
    };
    let Ok((ours, theirs)) = crate::channel_create() else {
        close(&handles);
        return None;
    };
    let sent = channel_write(ours, &bytes[..n], &handles);
    let _ = handle_close(ours);
    if sent.is_err() {
        close(&handles);
        let _ = handle_close(theirs);
        return None;
    }
    Some(theirs)
}

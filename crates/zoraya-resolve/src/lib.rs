//! The resolver (ADR-0107 items 6 and 7, ADR-0153 rule 6): a path to an
//! entry of a namespace table and the relative path within it, lexically.
//! Nothing here talks to a service, so all of it is tested on the host.
//!
//! The one resolver of the system (TASK-18 plan step 7): `libposix`
//! (`zoraya-posix`'s `path`, which maps [`Error`] to `errno` values) and the
//! native runtime and `zosh` use it. Moved here from `zoraya-posix` as it
//! was, with its errors as a type of its own.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// The longest path, with its NUL (POSIX `PATH_MAX`, musl's 4096).
pub const PATH_MAX: usize = 4096;
/// Links followed in one resolution before `ELOOP` (ADR-0107 item 7).
pub const MAX_LINKS: u32 = 40;

/// How `..` and relative paths behave (`zoraya_abi::posix::ABSOLUTE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// ADR-0107 item 6, INV-0002-4: a relative path is resolved in cwd's
    /// entry, and `..` at that entry's root stays there.
    EntryRelative,
    /// ADR-0153 rule 6, INV-0153-8: a relative path is joined to cwd's
    /// absolute path, and `..` stops only at `/`.
    Absolute,
}

/// A place in the table: an entry and a normal relative path in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub entry: usize,
    pub path: String,
}

/// Where a path led.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub place: Place,
    /// The absolute normal path, for `getcwd`, reports and links.
    pub absolute: String,
    /// The path ended in `/`: it must name a directory.
    pub directory: bool,
    /// The path's last component was `.` or `..`: it names no entry of a
    /// directory, so the calls that take a name refuse it (`rmdir("d/.")`).
    pub dot: bool,
}

/// What went wrong with a path, apart from its lying outside the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Empty, or an empty link target: `ENOENT`.
    NotFound,
    /// Longer than [`PATH_MAX`]: `ENAMETOOLONG`.
    NameTooLong,
    /// Not UTF-8 (ADR-0013 resolves X-8 so): `EILSEQ`.
    NotUtf8,
    /// More than [`MAX_LINKS`] links in one resolution: `ELOOP`.
    TooManyLinks,
}

/// Why a path led nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No entry covers the absolute path: a missing file to the program,
    /// and a report (ADR-0153 rule 11).
    Outside(String),
    Error(Error),
}

/// A path as a program names it: bytes, without the NUL.
pub fn check(path: &[u8]) -> Result<&str, Error> {
    if path.is_empty() {
        return Err(Error::NotFound);
    }
    if path.len() >= PATH_MAX {
        return Err(Error::NameTooLong);
    }
    // ADR-0013 resolves X-8 so: a name that is not UTF-8 is `EILSEQ`.
    core::str::from_utf8(path).map_err(|_| Error::NotUtf8)
}

/// The components of an absolute normal path.
fn components(absolute: &str) -> impl Iterator<Item = &str> {
    absolute.split('/').filter(|c| !c.is_empty())
}

/// `path` applied to the absolute normal `base`: `.` and empty components
/// dropped, `..` removing the component before it, but none of the first
/// `floor` components of `base`.
pub fn join(base: &str, floor: usize, path: &str) -> String {
    let mut parts: Vec<&str> = if path.starts_with('/') {
        Vec::new()
    } else {
        components(base).collect()
    };
    let floor = if path.starts_with('/') {
        0
    } else {
        floor.min(parts.len())
    };
    for c in path.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if parts.len() > floor {
                    parts.pop();
                }
            }
            name => parts.push(name),
        }
    }
    let mut out = String::with_capacity(path.len() + base.len() + 1);
    if parts.is_empty() {
        out.push('/');
    }
    for p in parts {
        out.push('/');
        out.push_str(p);
    }
    out
}

/// The absolute path of `place`, under its entry's `prefix`.
pub fn absolute_of(prefix: &str, path: &str) -> String {
    let mut out = String::from(prefix);
    if !path.is_empty() {
        if !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(path);
    }
    out
}

/// True if `prefix` covers the absolute normal `path`, on a component
/// boundary.
pub fn covers(prefix: &str, path: &str) -> bool {
    prefix == "/"
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The entry with the longest prefix that covers the absolute normal `path`,
/// and the relative path after it.
pub fn longest<'p, 'a>(
    prefixes: impl Iterator<Item = &'p str>,
    path: &'a str,
) -> Option<(usize, &'a str)> {
    let mut best: Option<(usize, usize)> = None;
    for (i, prefix) in prefixes.enumerate() {
        if covers(prefix, path) && best.is_none_or(|(_, length)| prefix.len() > length) {
            best = Some((i, prefix.len()));
        }
    }
    let (i, length) = best?;
    let rest = path[length..].trim_start_matches('/');
    Some((i, rest))
}

/// Resolves the absolute normal `path` against the table.
pub fn place<'p>(
    prefixes: impl Iterator<Item = &'p str>,
    path: String,
    directory: bool,
) -> Result<Resolved, Refusal> {
    // A joined path can outgrow what a request carries (4096 bytes) though
    // each part did not.
    if path.len() > PATH_MAX {
        return Err(Refusal::Error(Error::NameTooLong));
    }
    match longest(prefixes, &path) {
        Some((entry, rest)) => Ok(Resolved {
            place: Place {
                entry,
                path: String::from(rest),
            },
            absolute: path,
            directory,
            dot: false,
        }),
        None => Err(Refusal::Outside(path)),
    }
}

/// Resolves `path`, named from `from` (cwd or a directory fd): `prefixes` are
/// the table's, and `from.entry` indexes them.
pub fn resolve(
    prefixes: &[&str],
    form: Form,
    from: &Place,
    path: &[u8],
) -> Result<Resolved, Refusal> {
    let text = check(path).map_err(Refusal::Error)?;
    let prefix = prefixes.get(from.entry).copied().unwrap_or("/");
    let base = absolute_of(prefix, &from.path);
    let floor = match form {
        Form::EntryRelative => components(prefix).count(),
        Form::Absolute => 0,
    };
    let joined = join(&base, floor, text);
    let directory = text.ends_with('/') || text.ends_with("/.") || text.ends_with("/..");
    let last = text.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let dot = last == "." || last == "..";
    place(prefixes.iter().copied(), joined, directory || dot).map(|r| Resolved { dot, ..r })
}

/// Where resolution goes on after the service met a link (ADR-0107 item 7).
/// `at` is the place that was opened, `remaining` the part of its path after
/// the link and `target` the link's contents. A relative target is read from
/// the link's directory, with `..` held as `form` says; an absolute one from
/// the table.
pub fn follow(
    prefixes: &[&str],
    form: Form,
    at: &Place,
    remaining: &str,
    target: &str,
    directory: bool,
) -> Result<Resolved, Refusal> {
    if target.is_empty() {
        return Err(Refusal::Error(Error::NotFound));
    }
    // The link is the component before `remaining`.
    let consumed = at
        .path
        .strip_suffix(remaining)
        .unwrap_or(&at.path)
        .trim_end_matches('/');
    let link_directory = match consumed.rfind('/') {
        Some(cut) => &consumed[..cut],
        None => "",
    };
    let prefix = prefixes.get(at.entry).copied().unwrap_or("/");
    let base = absolute_of(prefix, link_directory);
    let floor = match form {
        Form::EntryRelative => components(prefix).count(),
        Form::Absolute => 0,
    };
    let mut path = String::from(target);
    if !remaining.is_empty() {
        path.push('/');
        path.push_str(remaining);
    }
    if path.len() >= PATH_MAX {
        return Err(Refusal::Error(Error::NameTooLong));
    }
    let joined = join(&base, floor, &path);
    place(prefixes.iter().copied(), joined, directory)
}

/// Counts links in one resolution: [`Error::TooManyLinks`] past `MAX_LINKS`.
#[derive(Debug, Default)]
pub struct Links(u32);

impl Links {
    pub fn take(&mut self) -> Result<(), Error> {
        self.0 += 1;
        if self.0 > MAX_LINKS {
            Err(Error::TooManyLinks)
        } else {
            Ok(())
        }
    }
}

/// The parent of a place and the last name in it, for the requests that
/// name one component (`make_directory`, `remove`, `rename`, `make_link`).
/// `None` for an entry's root, which has no name in it.
pub fn split(place: &Place) -> Option<(Place, &str)> {
    if place.path.is_empty() {
        return None;
    }
    let (parent, name) = match place.path.rfind('/') {
        Some(cut) => (&place.path[..cut], &place.path[cut + 1..]),
        None => ("", place.path.as_str()),
    };
    Some((
        Place {
            entry: place.entry,
            path: String::from(parent),
        },
        name,
    ))
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

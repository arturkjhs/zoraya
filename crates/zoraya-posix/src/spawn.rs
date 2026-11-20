//! What `posix_spawn` gives a child beyond its fds (ADR-0411 item 1, plan part
//! F4, ADR-0433): the shape of its table and which of the parent's service
//! connections it receives. The parent holds all authority and can only narrow
//! it: an entry is opened through the parent's own table (a service refuses
//! rights the parent lacks), a connection is made by the service from the
//! parent's own, a descriptor the child gets is the parent's. The library
//! half is here, over the `Sys` trait, so a fake kernel runs it; the C
//! boundary (`ffi/procs.rs`) reads the attributes and makes the spawn.

#![forbid(unsafe_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_abi::posix::{
    Access, EntryKind, NO_ENTRY, NO_HANDLE, is_absolute_normal, is_relative_normal,
};
use zoraya_proto_file::{open_flags, rights};
use zoraya_proto_net::{Duplicate, Duplicated, status as net_status};
use zoraya_protocol::decode;
use zoraya_rng::{Share, ShareReply, status as rng_status};

use crate::errno::*;
use crate::jobs::{FileAction, Inherit, Passed};
use crate::process::{AT_FDCWD, Posix};
use crate::sys::Sys;

/// `POSIX_SPAWN_GRANT_NET`: the child gets a connection to the network with
/// the parent's own rights and scope.
pub const GRANT_NET: u32 = 1;
/// `POSIX_SPAWN_GRANT_RANDOM`: the child gets a connection to the random
/// service.
pub const GRANT_RANDOM: u32 = 2;
/// `POSIX_SPAWN_GRANT_WINDOW`: the child gets the parent's window channel,
/// which the parent no longer has (a channel end cannot be shared).
pub const GRANT_WINDOW: u32 = 4;
/// `POSIX_SPAWN_MOVE_NET`: the child gets the parent's own network
/// connection, which the parent no longer has.
pub const MOVE_NET: u32 = 8;
/// Every bit of a grant mask.
pub const GRANTS_KNOWN: u32 = GRANT_NET | GRANT_RANDOM | GRANT_WINDOW | MOVE_NET;

/// One namespace entry a spawn lists: `PATH` or `PATH:ro`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub path: String,
    pub read_only: bool,
}

/// Reads `PATH` or `PATH:ro`. The path is absolute and in normal form
/// (`EINVAL` otherwise, and for a `:` suffix other than `:ro`).
pub fn parse_spec(raw: &[u8]) -> Result<Spec, i32> {
    let text = core::str::from_utf8(raw).map_err(|_| EINVAL)?;
    let (path, read_only) = match text.strip_suffix(":ro") {
        Some(p) => (p, true),
        None => (text, false),
    };
    if !is_absolute_normal(path) {
        return Err(EINVAL);
    }
    if path.len() > zoraya_abi::posix::MAX_PATH {
        return Err(EINVAL);
    }
    Ok(Spec {
        path: path.to_string(),
        read_only,
    })
}

/// How a child starts, beyond its fds and its signals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shape {
    /// `POSIX_SPAWN_EMPTY_NAMESPACE`: the child's table is `entries` and
    /// nothing else, and its fds are the file actions' alone.
    pub empty: bool,
    pub entries: Vec<Spec>,
    /// A mask of [`GRANT_NET`] and the others.
    pub grants: u32,
}

impl Shape {
    /// `EINVAL` for a shape no spawn can have: a list without the flag, a
    /// mask with a bit that is none of ours, `GRANT_NET` and `MOVE_NET`
    /// together, a path listed twice.
    pub fn check(&self) -> Result<(), i32> {
        if !self.entries.is_empty() && !self.empty
            || self.grants & !GRANTS_KNOWN != 0
            || self.grants & GRANT_NET != 0 && self.grants & MOVE_NET != 0
        {
            return Err(EINVAL);
        }
        for (i, e) in self.entries.iter().enumerate() {
            if self.entries[..i].iter().any(|o| o.path == e.path) {
                return Err(EINVAL);
            }
        }
        Ok(())
    }

    /// True for a spawn with no attribute of this part: the child gets what
    /// a spawn gave before it (INV-0411-4).
    pub fn is_default(&self) -> bool {
        !self.empty && self.entries.is_empty() && self.grants == 0
    }
}

/// The service connections made or chosen for a child.
#[derive(Debug)]
pub struct Granted<H> {
    /// The network connection and whether it is the parent's own, moved.
    pub net: Option<(H, bool)>,
    /// A new connection to the random service.
    pub random: Option<H>,
    /// The parent's window channel, moved.
    pub window: Option<H>,
}

impl<H: Copy> Granted<H> {
    pub const fn none() -> Granted<H> {
        Granted {
            net: None,
            random: None,
            window: None,
        }
    }

    /// The handles made for this spawn alone: closed if it does not happen.
    pub fn made(&self) -> Vec<H> {
        let net = self.net.filter(|n| !n.1).map(|n| n.0);
        net.into_iter().chain(self.random).collect()
    }

    /// Every handle of the child's second message, in the order `ZPBX` says
    /// (net, random, window).
    pub fn in_order(&self) -> Vec<H> {
        self.net
            .map(|n| n.0)
            .into_iter()
            .chain(self.random)
            .chain(self.window)
            .collect()
    }
}

/// The place of the absolute, normal path `cwd` in a child's `entries`: the
/// first (longest) directory entry that covers it, else none.
fn cwd_in(cwd: &str, entries: &[(String, EntryKind, u8)]) -> (u32, String) {
    for (i, (prefix, kind, _)) in entries.iter().enumerate() {
        if !kind.is_directory() {
            continue;
        }
        let rest = if prefix == "/" {
            cwd.strip_prefix('/')
        } else if cwd == *prefix {
            Some("")
        } else {
            cwd.strip_prefix(prefix.as_str())
                .and_then(|r| r.strip_prefix('/'))
        };
        if let Some(rest) = rest
            && is_relative_normal(rest)
        {
            return (i as u32, rest.to_string());
        }
    }
    (NO_ENTRY, String::new())
}

impl<S: Sys> Posix<S> {
    /// What a child spawned with `shape` inherits (ADR-0103 item 7, ADR-0107,
    /// ADR-0433): what [`Posix::inherit`] gives for a default shape. On an
    /// error every handle made for it is closed and every file opened for an
    /// `addopen` action is closed again.
    pub fn inherit_shaped(
        &mut self,
        actions: &[FileAction],
        group: u32,
        shape: &Shape,
    ) -> Result<Inherit<S::H>, i32> {
        shape.check()?;
        let mut out = Inherit {
            slots: Vec::new(),
            fds: Vec::new(),
            message: Vec::new(),
            entries: Vec::new(),
            cwd: (self.cwd.entry as u32, self.cwd.path.clone()),
            passed: Passed::default(),
            temps: Vec::new(),
        };
        // The entries are checked before anything is opened: a spawn refused for
        // a bad entry has created no file.
        if shape.empty
            && let Err(e) = self.chosen_entries(group, shape, &mut out)
        {
            self.abandon(&out, &[]);
            return Err(e);
        }
        // `addopen`: opened here, in the parent's table, as the parent's own
        // act (the child gets the descriptor, not the path).
        let mut temps: Vec<i32> = Vec::new();
        let mut resolved: Vec<FileAction> = Vec::with_capacity(actions.len());
        for a in actions {
            match a {
                FileAction::Open(fd, path, flags, mode) => {
                    match self.open(AT_FDCWD, path, *flags, *mode) {
                        Ok(t) => {
                            let _ = self.fds.set_cloexec(t, true);
                            temps.push(t);
                            // `O_CLOEXEC` closes it at the exec: the child has
                            // no such fd, and the open still reported its errors.
                            resolved.push(if flags & crate::process::o::CLOEXEC == 0 {
                                FileAction::Opened(t, *fd)
                            } else {
                                FileAction::Close(*fd)
                            });
                        }
                        Err(e) => {
                            self.abandon(&out, &temps);
                            return Err(e);
                        }
                    }
                }
                other => resolved.push(other.clone()),
            }
        }
        let map = match self.child_fds_with(&resolved, shape.empty) {
            Ok(m) => m,
            Err(e) => {
                self.abandon(&out, &temps);
                return Err(e);
            }
        };
        let listed: Vec<u32> = resolved
            .iter()
            .filter_map(|a| match *a {
                FileAction::Dup2(_, to) | FileAction::Opened(_, to) => u32::try_from(to).ok(),
                _ => None,
            })
            .collect();
        let r = self
            .inherit_into(&map, group, shape, &listed, &mut out)
            .and_then(|()| self.apply_chdirs(actions, &mut out));
        if let Err(e) = r {
            self.abandon(&out, &temps);
            return Err(e);
        }
        out.temps = temps;
        Ok(out)
    }

    /// The `addchdir` actions, in order, each resolved in the parent's table
    /// as `chdir` does (a relative path from the directory before it): the
    /// child's working directory is the last, found again among the entries
    /// the child has. A path that is no directory the parent may enter, or
    /// that the parent's table does not cover, fails the spawn with that
    /// error (`ENOENT`, `ENOTDIR`, `EACCES`).
    fn apply_chdirs(&mut self, actions: &[FileAction], out: &mut Inherit<S::H>) -> Result<(), i32> {
        let mut at: Option<String> = None;
        for action in actions {
            let FileAction::Chdir(path) = action else {
                continue;
            };
            if path.is_empty() {
                return Err(ENOENT);
            }
            let base = at.clone().unwrap_or_else(|| self.getcwd());
            let mut full = Vec::new();
            if path[0] != b'/' {
                full.extend_from_slice(base.trim_end_matches('/').as_bytes());
                full.push(b'/');
            }
            full.extend_from_slice(path);
            let r = self.resolve(AT_FDCWD, &full, Access::Read)?;
            if self.special(&r).is_some() {
                return Err(ENOTDIR);
            }
            let prefix = self
                .entries
                .get(r.place.entry)
                .map_or_else(|| "/".to_string(), |e| e.prefix.clone());
            let place = r.place.path.clone();
            let (_, opened) = self.open_resolved(
                r,
                open_flags::DIRECTORY,
                rights::READ | rights::CLONE,
                0,
                0,
                Access::Read,
            )?;
            self.sys.close(opened.conn);
            if opened.kind != crate::stat::Kind::Directory {
                return Err(ENOTDIR);
            }
            if !crate::stat::owner_allows(opened.meta().mode, false, false, true) {
                return Err(EACCES);
            }
            at = Some(crate::path::absolute_of(&prefix, &place));
        }
        if let Some(cwd) = at {
            out.cwd = cwd_in(&cwd, &out.entries);
        }
        Ok(())
    }

    /// A refusal: every handle made for the child (the slots' new ends, the
    /// entries) and the files `addopen` opened are closed again, on whichever
    /// path it came (INV-0433-2).
    fn abandon(&mut self, out: &Inherit<S::H>, temps: &[i32]) {
        let made: Vec<S::H> = out
            .slots
            .iter()
            .filter(|s| s.2)
            .map(|s| s.1)
            .chain(out.message.iter().copied())
            .collect();
        for h in made {
            self.sys.close(h);
        }
        self.close_temps(temps);
    }

    /// Closes the files `addopen` opened for the spawn.
    pub fn close_temps(&mut self, temps: &[i32]) {
        for &t in temps {
            let _ = self.close(t);
        }
    }

    /// The entries of a chosen table: each listed path opened through the
    /// parent's table as a directory with the rights asked, `PATH:ro` with
    /// reading alone. A path no entry covers is `EACCES` (and reported as any
    /// refused path is); one the parent holds with fewer rights is `EACCES`
    /// from the service. `/dev/null`, the terminal and `/proc/self/exe` are
    /// listed by their own names.
    pub(crate) fn chosen_entries(
        &mut self,
        group: u32,
        shape: &Shape,
        out: &mut Inherit<S::H>,
    ) -> Result<(), i32> {
        // Longest first, as the record has them; equal lengths in byte order.
        let mut specs: Vec<&crate::spawn::Spec> = shape.entries.iter().collect();
        specs.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then_with(|| a.path.cmp(&b.path))
        });
        for spec in specs {
            if out.message.len() >= zoraya_abi::posix::MAX_MESSAGE_HANDLES {
                return Err(E2BIG);
            }
            let access = if spec.read_only {
                Access::Read
            } else {
                Access::Write
            };
            // A path outside the parent's table is `EACCES` here (ADR-0411
            // item 1), where an open of it is `ENOENT` and a report.
            let r = match self.resolve(AT_FDCWD, spec.path.as_bytes(), access) {
                Ok(r) => r,
                Err(ENOENT) => return Err(EACCES),
                Err(e) => return Err(e),
            };
            // A path that is exactly a special entry: that entry.
            if let Some(kind) = self.special(&r) {
                if spec.read_only || !r.place.path.is_empty() {
                    return Err(EINVAL);
                }
                let handle = match kind {
                    EntryKind::Terminal => {
                        let Some(h) = self.entries[r.place.entry].handle else {
                            return Err(EACCES);
                        };
                        let mine = self.clone_tty(h, group)?;
                        out.message.push(mine);
                        (out.message.len() - 1) as u8
                    }
                    _ => NO_HANDLE,
                };
                out.entries.push((spec.path.clone(), kind, handle));
                continue;
            }
            let wanted = if spec.read_only {
                rights::READ | rights::CLONE
            } else {
                rights::ALL
            };
            let (_, opened) = self.open_resolved(r, open_flags::DIRECTORY, wanted, 0, 0, access)?;
            if opened.kind != crate::stat::Kind::Directory {
                self.sys.close(opened.conn);
                return Err(ENOTDIR);
            }
            out.message.push(opened.conn);
            out.entries.push((
                spec.path.clone(),
                EntryKind::Directory,
                (out.message.len() - 1) as u8,
            ));
        }
        // The child's working directory: the parent's, if a listed
        // directory covers it, else none (ADR-0433 item 3).
        out.cwd = cwd_in(&self.getcwd(), &out.entries);
        Ok(())
    }

    /// The connections a spawn with `grants` gives its child. A connection the
    /// parent lacks is `EACCES`; a service with no room `EAGAIN`. On an error
    /// nothing made is left.
    pub fn grant_connections(&mut self, grants: u32) -> Result<Granted<S::H>, i32> {
        let mut g = Granted::none();
        let r = self.grant_into(grants, &mut g);
        if let Err(e) = r {
            for h in g.made() {
                self.sys.close(h);
            }
            return Err(e);
        }
        // Every POSIX process the broker's session starts has a connection of
        // its own to the random service, whoever started it and with no grant
        // word (ADR-0453 item 3, ADR-0227 item 3): the broker makes it. A
        // process the parent named `GRANT_RANDOM` for has the parent's
        // (ADR-0435); a session with no broker, or a broker with none to give,
        // starts the child as it always was, without one.
        if g.random.is_none()
            && let Ok(h) = self.broker_random()
        {
            g.random = Some(h);
        }
        Ok(g)
    }

    fn grant_into(&mut self, grants: u32, g: &mut Granted<S::H>) -> Result<(), i32> {
        if grants & MOVE_NET != 0 {
            g.net = Some((self.net.ok_or(EACCES)?, true));
        }
        if grants & GRANT_NET != 0 {
            let conn = self.net.ok_or(EACCES)?;
            let (mine, theirs) = self.sys.channel_create().map_err(|_| EMFILE)?;
            g.net = Some((mine, false));
            let size = self.net_call_with(conn, &Duplicate {}, &[theirs], false)?;
            match decode::<Duplicated>(&self.input[..size], 0) {
                Ok((_, r)) if r.status.0 == net_status::OK => {}
                Ok((_, r)) if r.status.0 == net_status::BUSY => return Err(EAGAIN),
                Ok(_) => return Err(EACCES),
                Err(_) => return Err(EIO),
            }
        }
        if grants & GRANT_RANDOM != 0 {
            let conn = self.random.ok_or(EACCES)?;
            let (mine, theirs) = self.sys.channel_create().map_err(|_| EMFILE)?;
            g.random = Some(mine);
            let size = self.net_call_with(conn, &Share {}, &[theirs], false)?;
            match decode::<ShareReply>(&self.input[..size], 0) {
                Ok((_, r)) if r.status.0 == rng_status::OK => {}
                Ok(_) => return Err(EAGAIN),
                Err(_) => return Err(EIO),
            }
        }
        if grants & GRANT_WINDOW != 0 {
            g.window = Some(self.window.ok_or(EACCES)?);
        }
        Ok(())
    }

    /// The spawn happened (the child holds the handles): what the parent
    /// gave away is no longer its own. A moved connection, the window, the
    /// ancillary channel of a stream socket and a packet socket leave the
    /// library's table; a memory descriptor's size stays (INV-0413-1); the
    /// files `addopen` opened are closed.
    pub fn spawned(&mut self, inherited: &Inherit<S::H>, granted: &Granted<S::H>) {
        if granted.net.is_some_and(|n| n.1) {
            self.net = None;
            self.addrs = Default::default();
        }
        if granted.window.is_some() {
            self.window = None;
        }
        self.finish_passed(&inherited.passed);
        self.close_temps(&inherited.temps);
    }

    /// The spawn did not happen: only what `addopen` opened is closed. The
    /// handles made for the child are the caller's to close, as they are on
    /// the paths that never reach `spawn`.
    pub fn spawn_refused(&mut self, inherited: &Inherit<S::H>) {
        self.close_temps(&inherited.temps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn an_entry_is_a_normal_absolute_path_with_an_optional_ro() {
        let ok = |s: &str| parse_spec(s.as_bytes());
        assert_eq!(
            ok("/usr/share/wpe"),
            Ok(Spec {
                path: "/usr/share/wpe".into(),
                read_only: false
            })
        );
        assert_eq!(
            ok("/system/fonts:ro"),
            Ok(Spec {
                path: "/system/fonts".into(),
                read_only: true
            })
        );
        assert_eq!(ok("/").map(|s| s.read_only), Ok(false));
        assert_eq!(ok("/:ro").map(|s| s.read_only), Ok(true));
        for bad in [
            "", "x", "a/b", "/a/", "/a//b", "/a/./b", "/a/../b", ":ro", "a:ro",
        ] {
            assert_eq!(ok(bad), Err(EINVAL), "{bad:?}");
        }
        // A colon is part of a name; only a final `:ro` is the suffix.
        assert_eq!(
            ok("/a:rw").map(|s| (s.path, s.read_only)),
            Ok(("/a:rw".into(), false))
        );
        assert_eq!(
            ok("/a:ro:ro").map(|s| (s.path, s.read_only)),
            Ok(("/a:ro".into(), true))
        );
        assert_eq!(parse_spec(b"/a\xff"), Err(EINVAL));
        let long = alloc::format!("/{}", "a".repeat(4096));
        assert_eq!(ok(&long), Err(EINVAL));
    }

    #[test]
    fn a_shape_is_checked_before_anything_is_made() {
        let spec = |p: &str| Spec {
            path: p.into(),
            read_only: false,
        };
        let shape = |empty, entries: Vec<Spec>, grants| Shape {
            empty,
            entries,
            grants,
        };
        assert_eq!(Shape::default().check(), Ok(()));
        assert!(Shape::default().is_default());
        assert_eq!(shape(true, vec![], 0).check(), Ok(()));
        assert!(!shape(true, vec![], 0).is_default());
        assert_eq!(shape(true, vec![spec("/a"), spec("/b")], 0).check(), Ok(()));
        // A list without the flag, a path twice, an unknown grant bit, both
        // ways of giving the network.
        assert_eq!(shape(false, vec![spec("/a")], 0).check(), Err(EINVAL));
        assert_eq!(
            shape(true, vec![spec("/a"), spec("/a")], 0).check(),
            Err(EINVAL)
        );
        assert_eq!(shape(false, vec![], 16).check(), Err(EINVAL));
        assert_eq!(
            shape(false, vec![], GRANT_NET | MOVE_NET).check(),
            Err(EINVAL)
        );
        assert_eq!(
            shape(false, vec![], GRANT_NET | GRANT_RANDOM | GRANT_WINDOW).check(),
            Ok(())
        );
        // Each alone, `MOVE_NET` too (the mutants of the check's operators).
        for one in [
            GRANT_NET,
            GRANT_RANDOM,
            GRANT_WINDOW,
            MOVE_NET,
            MOVE_NET | GRANT_RANDOM,
        ] {
            assert_eq!(shape(false, vec![], one).check(), Ok(()), "{one}");
        }
        // `:ro` and plain on one path are one path.
        let ro = Spec {
            path: "/a".into(),
            read_only: true,
        };
        assert_eq!(shape(true, vec![spec("/a"), ro], 0).check(), Err(EINVAL));
    }
}

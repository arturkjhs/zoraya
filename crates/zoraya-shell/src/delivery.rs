//! What the shell sends a running POSIX program when the person gives it a
//! file or a directory (ADR-0337 items 6 to 8, ADR-0395): the entries of its
//! table after the grant, made only from what the program holds and what was
//! asked, never wider. Pure, so every rule is tested on the host; `zosh`
//! opens the connections the specs name and writes them to the program's
//! `GRANTS` channel, and keeps the [`Held`] record for the job and nowhere
//! else (INV-0337-4).
//!
//! A file is given as its directory narrowed to a list of names, each with
//! its own rights, so a second file of a directory the program holds a name
//! of (`vi notes.txt`, then `~/other.txt`) is the list of both. A directory
//! is given whole. A grant never shadows what the program holds: a place
//! below a directory it holds whole is refused (the person starts it again
//! with the wider grant), and a system directory is never replaced.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_proto_file::rights;
use zoraya_resolve::covers;

/// Rights of a `+w DIR` (ADR-0153's `tree`) and of a `+r`.
pub const TREE: u32 = rights::ALL & !rights::PUBLISH;
pub const READ: u32 = rights::READ | rights::CLONE;
/// A name given to be changed (ADR-0153's `write` operand).
pub const CHANGE: u32 = rights::READ | rights::WRITE | rights::SYNC | rights::CLONE;

/// What a program's table holds: whole directories with their rights, and
/// names narrowed in directories (`prefix`, `name`, rights).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    pub wholes: Vec<(String, u32)>,
    pub names: Vec<(String, String, u32)>,
    /// Entries of the table the shell did not make from the line (a program's
    /// own directory, ADR-0341 item 3) or whose rights it cannot know: never
    /// replaced, widened or shadowed, and nothing is given at, above or below
    /// them.
    pub fixed: Vec<String>,
}

/// One thing the person gave: an absolute normal path, whether the shell
/// found a directory there, and whether it is `+w`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub path: String,
    pub dir: bool,
    pub write: bool,
}

/// An entry to send: its prefix, the connection to open, and its rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    /// The directory itself, with these rights.
    Whole { prefix: String, rights: u32 },
    /// The directory narrowed to these names, `rights` overall.
    Names {
        prefix: String,
        names: Vec<(String, u32)>,
        rights: u32,
    },
}

fn parent_and_name(path: &str) -> Option<(&str, &str)> {
    let cut = path.rfind('/')?;
    let name = &path[cut + 1..];
    (!name.is_empty()).then(|| (if cut == 0 { "/" } else { &path[..cut] }, name))
}

/// What kind of entry a program's table has, for [`Held::from_table`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    /// A directory connection.
    Directory,
    /// The temporary directory.
    Tmp,
    /// A program's own directory (ADR-0341 item 3): never touched.
    Own,
    /// Anything else (`/dev/null`, the image).
    Other,
}

/// The system directories every POSIX program is given read only.
const SYSTEM: [&str; 3] = ["/bin", "/usr", "/etc"];

impl Held {
    /// What a table holds: `entries` are its prefix, kind and whether it has a
    /// connection; `merged` is the plan the shell made them from. An entry with
    /// no connection holds nothing; a directory the plan does not list and that
    /// is not a system directory is **fixed** (a program's own directory, ADR-0341
    /// item 3, or any entry whose rights the shell cannot know): it is never
    /// assumed to be read only, and nothing is given at, above or below it.
    pub fn from_table<'a>(
        entries: impl Iterator<Item = (&'a str, TableKind, bool)>,
        merged: &[crate::grants::Merged],
    ) -> Held {
        let mut held = Held::default();
        for (prefix, kind, connected) in entries {
            if !connected {
                continue;
            }
            let found = merged.iter().find(|m| m.path == prefix);
            match (kind, found) {
                (TableKind::Directory, Some(m)) => match &m.name {
                    Some(name) => held.names.push((m.path.clone(), name.clone(), m.rights)),
                    None => held.wholes.push((m.path.clone(), m.rights)),
                },
                (TableKind::Directory, None) if SYSTEM.contains(&prefix) => {
                    held.wholes.push((prefix.into(), READ));
                }
                (TableKind::Directory, None) | (TableKind::Own, _) => {
                    held.fixed.push(prefix.into())
                }
                (TableKind::Tmp, _) => held.wholes.push((prefix.into(), TREE)),
                (TableKind::Other, _) => {}
            }
        }
        held
    }

    /// The grant of `wanted` to a program holding `self`: the entries to
    /// send, and `self` as it is then. All or nothing: on `Err` nothing is
    /// changed, and the text says why (and what to do).
    pub fn give(&mut self, wanted: &[Wanted]) -> Result<Vec<Spec>, String> {
        let mut next = self.clone();
        let mut specs: Vec<Spec> = Vec::new();
        let push = |s: Spec, specs: &mut Vec<Spec>| {
            let prefix = match &s {
                Spec::Whole { prefix, .. } | Spec::Names { prefix, .. } => prefix.clone(),
            };
            specs.retain(|o| match o {
                Spec::Whole { prefix: p, .. } | Spec::Names { prefix: p, .. } => *p != prefix,
            });
            specs.push(s);
        };
        for w in wanted {
            let place = if w.dir {
                Some(w.path.as_str())
            } else {
                parent_and_name(&w.path).map(|(p, _)| p)
            };
            if let Some(place) = place
                && let Some(f) = next
                    .fixed
                    .iter()
                    .find(|f| covers(f, place) || covers(place, f))
            {
                return Err(format!(
                    "{}: the program has its own place at {f}; start it again with the grant",
                    w.path
                ));
            }
            if w.dir && w.path == "/" {
                return Err(String::from(
                    "/: the whole volume cannot be given to a running program",
                ));
            }
            if w.dir {
                let want = if w.write { TREE } else { READ };
                if let Some((q, _)) = next
                    .wholes
                    .iter()
                    .find(|(q, _)| covers(q, &w.path) && *q != w.path)
                {
                    return Err(format!(
                        "{}: the program holds {q} already; start it again with the grant",
                        w.path
                    ));
                }
                // A deeper entry decides every path below it: giving this
                // directory would leave it as it was, not as the row says.
                if let Some(q) = next
                    .wholes
                    .iter()
                    .map(|(q, _)| q)
                    .chain(next.names.iter().map(|(p, _, _)| p))
                    .find(|q| covers(&w.path, q) && **q != w.path)
                {
                    return Err(format!(
                        "{}: the program holds {q} already, a narrower place; start it again with the grant",
                        w.path
                    ));
                }
                let held = next.wholes.iter().position(|(q, _)| *q == w.path);
                let mut all = want;
                // Names the program holds here with more rights than the
                // typed word gives would be widened to the whole directory:
                // never more than was typed.
                if next
                    .names
                    .iter()
                    .any(|(p, _, r)| *p == w.path && r & !want != 0)
                {
                    return Err(format!(
                        "{}: the program holds files there with more rights than that; start it again with the grant",
                        w.path
                    ));
                }
                match held {
                    Some(i) => {
                        let now = next.wholes[i].1;
                        if now | all == now {
                            return Err(format!("{}: the program holds it already", w.path));
                        }
                        all |= now;
                        next.wholes[i].1 = all;
                    }
                    None => next.wholes.push((w.path.clone(), all)),
                }
                next.names.retain(|(p, _, _)| *p != w.path);
                push(
                    Spec::Whole {
                        prefix: w.path.clone(),
                        rights: all,
                    },
                    &mut specs,
                );
            } else {
                let Some((prefix, name)) = parent_and_name(&w.path) else {
                    return Err(format!("{}: not a file that can be given", w.path));
                };
                if let Some((q, _)) = next.wholes.iter().find(|(q, _)| covers(q, prefix)) {
                    return Err(format!(
                        "{}: the program holds {q} already; start it again with the grant",
                        w.path
                    ));
                }
                let want = if w.write { CHANGE } else { READ };
                match next
                    .names
                    .iter_mut()
                    .find(|(p, n, _)| p == prefix && n == name)
                {
                    Some((_, _, r)) => {
                        if *r | want == *r {
                            return Err(format!("{}: the program holds it already", w.path));
                        }
                        *r |= want;
                    }
                    None => {
                        if next.names.iter().filter(|(p, _, _)| p == prefix).count()
                            >= zoraya_proto_file::narrow_names::MAX_NAMES
                        {
                            return Err(format!("{prefix}: too many files given there"));
                        }
                        next.names.push((prefix.into(), name.into(), want));
                    }
                }
                let names: Vec<(String, u32)> = next
                    .names
                    .iter()
                    .filter(|(p, _, _)| p == prefix)
                    .map(|(_, n, r)| (n.clone(), *r))
                    .collect();
                let all = names.iter().fold(0, |a, (_, r)| a | r);
                push(
                    Spec::Names {
                        prefix: prefix.into(),
                        names,
                        rights: all,
                    },
                    &mut specs,
                );
            }
        }
        *self = next;
        Ok(specs)
    }
}

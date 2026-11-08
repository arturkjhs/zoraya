//! What a command line gives a native program (ADR-0153 rules 1 to 4, 6 and
//! 7): a plan of namespace entries made from the command's description, its
//! parsed arguments, the current directory and the line's `+r` and `+w`.
//! The plan is pure, so every rule is tested on the host; `zosh` opens what
//! it says and nothing else, and `grants` prints it (INV-0153-4).
//!
//! Paths are absolute and normal (rule 6): a relative one is joined to the
//! current directory, and `..` stops only at `/`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_command::Parsed;
use zoraya_describe::{Cwd, Description, Kind};
use zoraya_proto_file::rights;
use zoraya_resolve::{covers, join};

use crate::eval::Value;

/// What an entry allows (rule 2's table, and the grants of rule 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// `read`: the file or directory, to read.
    Read,
    /// `write`: the existing file, to change.
    Change,
    /// `create`: the parent directory, narrowed to the name: create or
    /// replace it.
    Create(String),
    /// `remove`: the parent, narrowed to the name.
    Remove(String),
    /// `rename`: the parent, narrowed to the name.
    Rename(String),
    /// `tree` or `+w`: the directory and everything below it.
    Tree,
}

impl Access {
    /// The connection's rights (ADR-0055).
    pub fn rights(&self) -> u32 {
        match self {
            Access::Read => rights::READ | rights::CLONE,
            Access::Change => rights::READ | rights::WRITE | rights::SYNC | rights::CLONE,
            Access::Create(_) => {
                rights::READ | rights::WRITE | rights::CREATE | rights::SYNC | rights::CLONE
            }
            Access::Remove(_) => rights::READ | rights::REMOVE | rights::CLONE,
            Access::Rename(_) => {
                rights::READ | rights::RENAME | rights::CREATE | rights::REMOVE | rights::CLONE
            }
            Access::Tree => rights::ALL & !rights::PUBLISH,
        }
    }

    /// The one name a narrowed entry may use.
    pub fn name(&self) -> Option<&str> {
        match self {
            Access::Create(n) | Access::Remove(n) | Access::Rename(n) => Some(n),
            _ => None,
        }
    }

    /// How `grants` says it (rule 2's table, as decided: "create or
    /// replace").
    pub fn words(&self) -> String {
        match self {
            Access::Read => "read".into(),
            Access::Change => "change".into(),
            Access::Create(n) => format!("create or replace {n}"),
            Access::Remove(n) => format!("remove {n}"),
            Access::Rename(n) => format!("rename {n}"),
            Access::Tree => "change anything in".into(),
        }
    }
}

/// One entry of the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// Absolute and normal: the path itself, or for a narrowed entry its
    /// parent directory.
    pub path: String,
    pub access: Access,
    /// Why: `current directory`, `operand FROM`, `option -o`, `+r`, `+w`.
    pub reason: String,
}

/// What `plan` needs.
pub struct Line<'a> {
    pub description: &'a Description,
    pub parsed: &'a Parsed<'a>,
    /// The arguments as values, for their marks (rule 4).
    pub args: &'a [Value],
    /// The current directory, absolute.
    pub cwd: &'a str,
    /// `+r` (false) and `+w` (true) with their paths, as written.
    pub grants: &'a [(bool, Value)],
    /// The description is the system image's own (ADR-0153's Decision):
    /// only then are kinds other than `read` designated.
    pub base_system: bool,
}

/// Why a line may not run (rule 3, rule 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub subject: String,
    pub problem: String,
    pub hint: Option<String>,
}

fn parent_and_name(abs: &str) -> Option<(String, String)> {
    let cut = abs.rfind('/')?;
    let name = &abs[cut + 1..];
    if name.is_empty() {
        return None;
    }
    let parent = if cut == 0 { "/" } else { &abs[..cut] };
    Some((parent.into(), name.into()))
}

/// The entries a kind gives for absolute path `abs` (rule 2).
fn entries_for(kind: Kind, abs: &str, reason: &str, out: &mut Vec<Planned>) {
    let push = |out: &mut Vec<Planned>, path: String, access: Access| {
        out.push(Planned {
            path,
            access,
            reason: reason.into(),
        });
    };
    // `/` itself has no name in a parent: nothing to narrow to.
    let narrowed = |out: &mut Vec<Planned>, make: fn(String) -> Access| {
        if let Some((parent, name)) = parent_and_name(abs) {
            push(out, parent, make(name));
        }
    };
    match kind {
        Kind::Text => {}
        Kind::Read => push(out, abs.into(), Access::Read),
        Kind::Write => push(out, abs.into(), Access::Change),
        Kind::Create => narrowed(out, Access::Create),
        Kind::Remove => narrowed(out, Access::Remove),
        Kind::RemoveTree => {
            narrowed(out, Access::Remove);
            push(out, abs.into(), Access::Tree);
        }
        Kind::Rename => narrowed(out, Access::Rename),
        Kind::Tree => push(out, abs.into(), Access::Tree),
    }
}

/// The value an operand's text came from: the argument whose text holds
/// it, the whole word or a piece of it (`-C$d`, `--directory=$d`, which
/// the parser gives as part of a word; review S1).
fn value_of<'v>(args: &'v [Value], text: &str) -> Option<&'v Value> {
    let start = text.as_ptr() as usize;
    let end = start + text.len();
    args.iter().find(|a| {
        let a_start = a.text.as_ptr() as usize;
        a_start <= start && end <= a_start + a.text.len()
    })
}

/// Plans the entries of a line (rules 1 to 4).
pub fn plan(line: &Line<'_>) -> Result<Vec<Planned>, Refused> {
    let d = line.description;
    let p = line.parsed;
    let mut out = Vec::new();
    if d.cwd == Cwd::Read {
        out.push(Planned {
            path: line.cwd.into(),
            access: Access::Read,
            reason: "current directory".into(),
        });
    }
    // Grants are known first, so that operands from values can be checked
    // against them (rule 4), and listed last, as the line reads.
    let mut granted: Vec<String> = Vec::new();
    let mut grant_entries = Vec::new();
    for (write, value) in line.grants {
        let abs = join(line.cwd, 0, &value.text);
        granted.push(abs.clone());
        grant_entries.push(Planned {
            path: abs,
            access: if *write { Access::Tree } else { Access::Read },
            reason: if *write { "+w".into() } else { "+r".into() },
        });
    }
    let designate =
        |kind: Kind, text: &str, reason: String, out: &mut Vec<Planned>| -> Result<(), Refused> {
            if !kind.is_path() {
                return Ok(());
            }
            let abs = join(line.cwd, 0, text);
            // A text that is no argument's is taken as from a value: the
            // check fails closed.
            let literal = value_of(line.args, text).is_some_and(|v| v.literal);
            // A kind that narrows the parent needs the path strictly below:
            // a value naming the current directory itself would give its
            // parent (review S16).
            let narrows = matches!(
                kind,
                Kind::Create | Kind::Remove | Kind::RemoveTree | Kind::Rename
            );
            let within = |base: &str| covers(base, &abs) && !(narrows && abs == base);
            let inside = within(line.cwd) || granted.iter().any(|g| within(g));
            if !literal && !inside {
                let parent = parent_and_name(&abs).map_or_else(|| "/".to_string(), |(p, _)| p);
                // The least grant that would cover it (review S10).
                let (marker, dir) = if kind == Kind::Read {
                    ("+r", abs.clone())
                } else {
                    ("+w", parent)
                };
                return Err(Refused {
                    subject: text.into(),
                    problem: "from a value, outside the current directory".into(),
                    hint: Some(format!(
                        "if that is meant, write {marker} {dir} before the command"
                    )),
                });
            }
            // ADR-0153's Decision: only base-system commands get more than
            // `read` without a grant.
            if kind != Kind::Read && !line.base_system {
                return Ok(());
            }
            entries_for(kind, &abs, &reason, out);
            Ok(())
        };
    for (index, text) in &p.operands {
        let kind = p.kind_of(d, *index);
        let reason = format!("operand {}", d.operands[*index].name);
        designate(kind, text, reason, &mut out)?;
    }
    for (index, value) in &p.options {
        let o = &d.options[*index];
        if let (Some(arg), Some(text)) = (&o.argument, value) {
            let reason = match o.short {
                Some(c) => format!("option -{c}"),
                None => format!("option --{}", o.long),
            };
            designate(arg.kind, text, reason, &mut out)?;
        }
    }
    out.extend(grant_entries);
    Ok(out)
}

/// One entry as it is given: the plan's entries for one path, merged
/// (rule 2: "two entries with the same prefix are merged into one with the
/// union of their rights").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub path: String,
    pub rights: u32,
    /// The one name it is narrowed to, if all its parts named the same one.
    pub name: Option<String>,
    /// `(words, reason)` for each part, as `grants` shows it.
    pub lines: Vec<(String, String)>,
}

/// Rule 2's fallback words for a narrowed kind given as the whole directory.
fn fallback(access: &Access) -> String {
    match access {
        Access::Create(_) => "create and change anything in".into(),
        Access::Remove(_) => "remove anything in".into(),
        Access::Rename(_) => "rename, create and remove anything in".into(),
        other => other.words(),
    }
}

/// Rule 5: a designated path that is or passes through a link object
/// becomes the link's target when the target lies inside the current
/// directory or a grant of the line; otherwise the line is refused, naming
/// the path and the target. `target(abs)` gives where `abs` leads through
/// its links (ADR-0107 item 7's rules), or `None` when it passes through
/// none or does not exist. Grants are followed too: what the person named
/// is what they looked at. The current directory's own entry is kept.
pub fn through_links(
    plan: Vec<Planned>,
    cwd: &str,
    home: &str,
    mut target: impl FnMut(&str) -> Option<String>,
) -> Result<Vec<Planned>, Refused> {
    let is_grant = |e: &Planned| e.reason == "+r" || e.reason == "+w";
    let mut roots: Vec<String> = alloc::vec![cwd.into()];
    let mut out = plan;
    for e in out.iter_mut().filter(|e| is_grant(e)) {
        if let Some(t) = target(&e.path) {
            e.path = t;
        }
        roots.push(e.path.clone());
    }
    for e in out.iter_mut() {
        if is_grant(e) || e.reason == "current directory" {
            continue;
        }
        let Some(t) = target(&e.path).filter(|t| *t != e.path) else {
            continue;
        };
        if roots.iter().any(|r| zoraya_resolve::covers(r, &t)) {
            e.reason = format!("{}, through a link at {}", e.reason, shown(&e.path, home));
            e.path = t;
            continue;
        }
        let marker = if e.access == Access::Read { "+r" } else { "+w" };
        return Err(Refused {
            subject: shown(&e.path, home),
            problem: format!(
                "leads through a link to {}, outside the current directory and this line's grants",
                shown(&t, home)
            ),
            hint: Some(format!(
                "to give it, put {marker} {} before the command",
                shown(&t, home)
            )),
        });
    }
    Ok(out)
}

/// Merges a plan's entries by path, in the order each path first appears.
/// Parts that all narrow to one name stay narrowed to it; otherwise the
/// directory is given whole with the union of the rights, and a narrowed
/// part is shown in rule 2's fallback words, so `grants` says what is given.
pub fn merge(plan: &[Planned]) -> Vec<Merged> {
    covered(by_path(plan))
}

/// The entries of a plan merged by exact path, in the order each first
/// appears.
fn by_path(plan: &[Planned]) -> Vec<Merged> {
    let mut paths: Vec<&str> = Vec::new();
    for e in plan {
        if !paths.contains(&e.path.as_str()) {
            paths.push(&e.path);
        }
    }
    paths
        .into_iter()
        .map(|path| {
            let parts: Vec<&Planned> = plan.iter().filter(|e| e.path == path).collect();
            let first = parts[0].access.name();
            let name = first
                .filter(|n| parts.iter().all(|e| e.access.name() == Some(n)))
                .map(String::from);
            let rights = parts.iter().fold(0, |r, e| r | e.access.rights());
            let lines = parts
                .iter()
                .map(|e| {
                    let words = if name.is_some() {
                        e.access.words()
                    } else {
                        fallback(&e.access)
                    };
                    (words, e.reason.clone())
                })
                .collect();
            Merged {
                path: path.into(),
                rights,
                name,
                lines,
            }
        })
        .collect()
}

/// Entries inside others (review S3, S9): a path is served by the entry
/// with the longest prefix alone, so an entry inside an undivided one gets
/// that one's rights as well (a narrowed one is undivided then too, since
/// the covering entry already allows more), and an entry that adds nothing
/// to one that covers it is left out: `+w ~` reaches into a current
/// directory below it, and `cat *.txt` takes one entry, not one a file.
fn covered(mut merged: Vec<Merged>) -> Vec<Merged> {
    let n = merged.len();
    for i in 0..n {
        for j in 0..n {
            if i == j || merged[j].name.is_some() {
                continue;
            }
            let (outer, inner) = (merged[j].path.clone(), merged[i].path.clone());
            if outer != inner && resolve_covers(&outer, &inner) {
                let more = merged[j].rights;
                let m = &mut merged[i];
                if m.name.is_some() && more & m.rights == m.rights {
                    m.name = None;
                }
                if m.name.is_none() {
                    m.rights |= more;
                }
            }
        }
    }
    let keep: Vec<bool> = (0..n)
        .map(|i| {
            let m = &merged[i];
            !(0..n).any(|j| {
                let o = &merged[j];
                j != i
                    && o.name.is_none()
                    && m.name.is_none()
                    && o.path != m.path
                    && resolve_covers(&o.path, &m.path)
                    && o.rights & m.rights == m.rights
            })
        })
        .collect();
    merged
        .into_iter()
        .zip(keep)
        .filter_map(|(m, k)| k.then_some(m))
        .collect()
}

fn resolve_covers(outer: &str, inner: &str) -> bool {
    zoraya_resolve::covers(outer, inner)
}

/// A path as `grants` shows it: `~` for home.
pub fn shown(path: &str, home: &str) -> String {
    if path == home {
        return "~".into();
    }
    match path.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') && home != "/" => format!("~{rest}"),
        _ => path.into(),
    }
}

/// What a job was given, as `jobs` shows it (ADR-0158 rule 5): each
/// entry as `PATH: WORDS`, in the words of [`report`], joined by `, `.
pub fn summary(plan: &[Planned], home: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for m in merge(plan) {
        for (words, _) in &m.lines {
            let part = format!("{}: {words}", shown(&m.path, home));
            if !parts.contains(&part) {
                parts.push(part);
            }
        }
    }
    parts.join(", ")
}

/// `grants`' report (rule 7, ADR-0153's examples, the tour's 14 and 17):
/// the streams, then each entry of the plan in the order of the line, its
/// rights in words (rule 2's fallback words where a directory is given
/// whole). A POSIX program's also shows its toolchain set (rule 10).
pub fn report(name: &str, plan: &[Planned], home: &str, streams: &str, posix: bool) -> String {
    let merged = merge(plan);
    let mut rows: Vec<(String, String)> = Vec::new();
    rows.push(("in, out, err".into(), streams.into()));
    for e in plan {
        let narrowed = merged
            .iter()
            .find(|m| m.path == e.path)
            .is_some_and(|m| m.name.is_some());
        let words = if narrowed {
            e.access.words()
        } else {
            fallback(&e.access)
        };
        rows.push((shown(&e.path, home), format!("{words} ({})", e.reason)));
    }
    if posix {
        for (path, right) in [
            ("/bin /usr /etc", "read (system)"),
            ("/proc/self/exe", "read (system)"),
            ("/tmp", "change anything in (this line's own)"),
            ("/dev/null", ""),
        ] {
            rows.push((path.into(), right.into()));
        }
    }
    let width = rows
        .iter()
        .map(|(p, _)| p.chars().count())
        .max()
        .unwrap_or(0)
        .max(19)
        + 2;
    let mut s = if posix {
        format!("{name} gets (a POSIX program):\n")
    } else {
        format!("{name} gets:\n")
    };
    for (path, right) in rows {
        let mut row = format!("  {path}");
        if !right.is_empty() {
            for _ in path.chars().count()..width {
                row.push(' ');
            }
            row.push_str(&right);
        }
        s.push_str(&row);
        s.push('\n');
    }
    s
}

/// The description that applies to a POSIX program found at the absolute
/// `path` (ADR-0153 rule 10, INV-0153-6): one of the system image's own,
/// `trusted`, and only the one that names that path. A program reached any
/// other way (`./tcc`, a copy, one the person built) gets none, and nothing
/// on a volume is ever read for one.
pub fn trusted_description<'a>(trusted: &'a [Description], path: &str) -> Option<&'a Description> {
    trusted.iter().find(|d| d.program.as_deref() == Some(path))
}

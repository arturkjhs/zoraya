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

/// The value an operand's text came from, by identity of its text.
fn value_of<'v>(args: &'v [Value], text: &str) -> Option<&'v Value> {
    args.iter().find(|a| core::ptr::eq(a.text.as_str(), text))
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
    // Grants first, so that operands from values can be checked against
    // them (rule 4).
    let mut granted: Vec<String> = Vec::new();
    for (write, value) in line.grants {
        let abs = join(line.cwd, 0, &value.text);
        granted.push(abs.clone());
        out.push(Planned {
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
            let literal = value_of(line.args, text).is_none_or(|v| v.literal);
            let inside = covers(line.cwd, &abs) || granted.iter().any(|g| covers(g, &abs));
            if !literal && !inside {
                let parent = parent_and_name(&abs).map_or_else(|| "/".to_string(), |(p, _)| p);
                return Err(Refused {
                    subject: text.into(),
                    problem: "from a value, outside the current directory".into(),
                    hint: Some(format!(
                        "if that is meant, write +w {parent} before the command"
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

/// Merges a plan's entries by path, in the order each path first appears.
/// Parts that all narrow to one name stay narrowed to it; otherwise the
/// directory is given whole with the union of the rights, and a narrowed
/// part is shown in rule 2's fallback words, so `grants` says what is given.
pub fn merge(plan: &[Planned]) -> Vec<Merged> {
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

/// `grants`' report (rule 7, ADR-0153's examples): the streams, then each
/// entry as it is given, one line per reason, with its rights in words.
pub fn report(name: &str, plan: &[Planned], home: &str, streams: &str) -> String {
    let mut rows: Vec<(String, String)> = Vec::new();
    rows.push(("in, out, err".into(), streams.into()));
    for m in merge(plan) {
        for (words, reason) in &m.lines {
            rows.push((shown(&m.path, home), format!("{words} ({reason})")));
        }
    }
    let width = rows
        .iter()
        .map(|(p, _)| p.chars().count())
        .max()
        .unwrap_or(0)
        .max(19)
        + 2;
    let mut s = format!("{name} gets:\n");
    for (path, right) in rows {
        s.push_str("  ");
        s.push_str(&path);
        for _ in path.chars().count()..width {
            s.push(' ');
        }
        s.push_str(&right);
        s.push('\n');
    }
    s
}

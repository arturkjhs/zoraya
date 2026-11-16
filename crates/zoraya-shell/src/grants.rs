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
    /// `read` operand of a `cwd none` description (ADR-0336 item 6): the
    /// parent directory, narrowed to the name, to read it.
    ReadName(String),
    /// `write` operand of a `cwd none` description: the parent, narrowed to
    /// the name, to change the existing file (no create, no remove).
    ChangeName(String),
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
            Access::ReadName(_) => rights::READ | rights::CLONE,
            Access::ChangeName(_) => rights::READ | rights::WRITE | rights::SYNC | rights::CLONE,
        }
    }

    /// The one name a narrowed entry may use.
    pub fn name(&self) -> Option<&str> {
        match self {
            Access::Create(n)
            | Access::Remove(n)
            | Access::Rename(n)
            | Access::ReadName(n)
            | Access::ChangeName(n) => Some(n),
            _ => None,
        }
    }

    /// The same access narrowed to `name` instead (an entry followed
    /// through a link, rule 5). A whole access stays as it is.
    pub fn renamed(&self, name: String) -> Access {
        match self {
            Access::Create(_) => Access::Create(name),
            Access::Remove(_) => Access::Remove(name),
            Access::Rename(_) => Access::Rename(name),
            Access::ReadName(_) => Access::ReadName(name),
            Access::ChangeName(_) => Access::ChangeName(name),
            other => other.clone(),
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
            Access::ReadName(n) => format!("read {n}"),
            Access::ChangeName(n) => format!("change {n}"),
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
    /// `+net` words with their arguments, as written (ADR-0224 item 1).
    pub nets: &'a [Value],
    /// The description is the system image's own (ADR-0153's Decision):
    /// only then are kinds other than `read` designated.
    pub base_system: bool,
    /// The room the line runs in, with its `+net` words (ADR-0224 item 2,
    /// ADR-0462): they bound what the line may be given and let a value
    /// designate inside them; they give a line nothing by themselves.
    pub room: Option<RoomBound<'a>>,
}

/// A room's name and `+net` words, as the bound of a line's network.
#[derive(Debug, Clone, Copy)]
pub struct RoomBound<'a> {
    pub name: &'a str,
    pub words: &'a [String],
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
        Kind::Text | Kind::Url | Kind::Host | Kind::Rest => {}
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
pub(crate) fn value_of<'v>(args: &'v [Value], text: &str) -> Option<&'v Value> {
    args.iter().find(|a| within(&a.text, text))
}

/// Whether `inner` is a slice of `outer`'s bytes (by address: the same text
/// elsewhere is not).
pub(crate) fn within(outer: &str, inner: &str) -> bool {
    let (o, i) = (outer.as_ptr() as usize, inner.as_ptr() as usize);
    o <= i && i + inner.len() <= o + outer.len()
}

/// The descriptions whose files are given one narrowed name each (ADR-0335 item 3).
const NARROW_ONE: [&str; 4] = ["vi", "ex", "view", "less"];

/// Plans the entries of a line (rules 1 to 4).
pub fn plan(line: &Line<'_>) -> Result<Vec<Planned>, Refused> {
    let d = line.description;
    let p = line.parsed;
    let mut out = Vec::new();
    // ADR-0341 item 8: a current directory at `/`, `/data`, a room's
    // directory or inside a program's own directory gives nothing, as if the
    // description said `cwd none`; relative operands still resolve.
    let into_data = cwd_reaches_data(line.cwd);
    if d.cwd == Cwd::Read && !into_data {
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
            // `-` after a `create` option of a `cwd none` description is the
            // standard output (`wget -O -`), not a file named `-`: nothing
            // to create (review of 9a, N5).
            if kind == Kind::Create && text == "-" && d.cwd == Cwd::None {
                return Ok(());
            }
            let abs = join(line.cwd, 0, text);
            // A text that is no argument's is taken as from a value: the
            // check fails closed.
            let literal = value_of(line.args, text).is_some_and(|v| v.literal);
            // A kind that narrows the parent needs the path strictly below:
            // a value naming the current directory itself would give its
            // parent (review S16). A file operand of a `cwd none`
            // description narrows the parent too (below).
            let narrows = matches!(
                kind,
                Kind::Create | Kind::Remove | Kind::RemoveTree | Kind::Rename
            ) || (d.cwd == Cwd::None && matches!(kind, Kind::Read | Kind::Write));
            let within = |base: &str| covers(base, &abs) && !(narrows && abs == base);
            // A value inside a current directory that gives nothing is not
            // inside anything (ADR-0341 item 8).
            let inside = (!into_data && within(line.cwd)) || granted.iter().any(|g| within(g));
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
            // ADR-0336 item 6: a file operand of a `cwd none` description
            // arrives as its directory narrowed to the name, so the one
            // file is enough and its siblings are not given.
            if d.cwd == Cwd::None
                && matches!(kind, Kind::Read | Kind::Write)
                && let Some((parent, name)) = parent_and_name(&abs)
            {
                let access = if kind == Kind::Read {
                    Access::ReadName(name)
                } else {
                    Access::ChangeName(name)
                };
                out.push(Planned {
                    path: parent,
                    access,
                    reason,
                });
                return Ok(());
            }
            entries_for(kind, &abs, &reason, out);
            Ok(())
        };
    for (index, text) in &p.operands {
        let kind = p.kind_of(d, *index);
        let reason = format!("operand {}", d.operands[*index].name);
        // A `+` word is an option of the programs that are given one name and no
        // directory (`vi +10 f`, `less +G f`): taken as a second file it would
        // give the directory. It is refused, never planned (review of 10d, 1).
        if d.cwd == Cwd::None && kind.is_path() && text.starts_with('+') {
            return Err(Refused {
                subject: (*text).into(),
                problem: "a word that starts with + is not taken as a file".into(),
                hint: Some(format!(
                    "for a file of that name write ./{text}; {} takes a command with -c COMMAND",
                    d.name
                )),
            });
        }
        designate(kind, text, reason, &mut out)?;
    }
    // ADR-0335 item 3: the editor and the pager are given the names the line writes. A narrowed
    // connection takes one name until `narrow` takes a list, so two names of one directory would
    // become the directory: the line is refused instead, and the person says it (review of 10d, 2).
    if NARROW_ONE.contains(&d.name.as_str()) {
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for e in &out {
            let (Access::Create(n) | Access::ReadName(n) | Access::ChangeName(n)) = &e.access
            else {
                continue;
            };
            if let Some((_, first)) = seen.iter().find(|(p, _)| *p == e.path) {
                let marker = if d.name == "vi" || d.name == "ex" {
                    "+w"
                } else {
                    "+r"
                };
                return Err(Refused {
                    subject: format!("{first} {n}"),
                    problem: "two files of one directory need the whole directory".into(),
                    hint: Some(format!(
                        "if that is meant, write {marker} {} before the command; or name one file",
                        e.path
                    )),
                });
            }
            seen.push((&e.path, n));
        }
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
    // `names OPTION OPERAND` (proposed ADR-0311): the option makes the program
    // create a file named by the URL's last segment in the current
    // directory. The shell works the name out itself and designates that one
    // file to create; a name it cannot be sure of refuses the line.
    for n in &d.names {
        if !p.options.iter().any(|(o, _)| *o == n.option) {
            continue;
        }
        let o = &d.options[n.option];
        let reason = match o.short {
            Some(c) => format!("option -{c}"),
            None => format!("option --{}", o.long),
        };
        for (index, text) in &p.operands {
            if *index != n.operand {
                continue;
            }
            let Some(name) = zoraya_describe::url_file_name(text) else {
                return Err(Refused {
                    subject: (*text).into(),
                    problem: format!(
                        "{} would save under a name the shell cannot be sure of",
                        o.label()
                    ),
                    hint: Some("to say where it goes, write -o FILE".into()),
                });
            };
            designate(Kind::Create, name, reason.clone(), &mut out)?;
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
        Access::ReadName(_) => "read anything in".into(),
        Access::ChangeName(_) => "read and change anything in".into(),
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
    let mut links: Vec<Planned> = Vec::new();
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
        // A narrowed read or change name (a `cwd none` file operand) is
        // followed at that name, not only at its directory: the name itself
        // may be the link, and the program's open follows it (review of
        // 10e, finding 7). Its target must lie strictly below a root, since
        // the entry becomes the target's parent narrowed to the target's
        // name. The link's own name stays given with the same rights, so
        // that the program's open (for writing too: the service checks the
        // rights before it answers that the name is a link) meets the link
        // and libposix follows it to the target's entry (check of the fix
        // pass, N1). Without CREATE, REMOVE or RENAME the link itself cannot
        // be replaced through it. `create`, `remove` and
        // `rename` act on the link object itself (`ln -sf`, `mv` onto a
        // link), so only their directory is followed (below; N2).
        let opens = matches!(e.access, Access::ReadName(_) | Access::ChangeName(_));
        let named = e.access.name().filter(|_| opens).map(String::from);
        if let Some(name) = named
            && let full = join(&e.path, 0, &name)
            && let Some(t) = target(&full).filter(|t| *t != full)
        {
            let below = |r: &String| zoraya_resolve::covers(r, &t) && *r != t;
            if roots.iter().any(below)
                && let Some((parent, tname)) = parent_and_name(&t)
            {
                links.push(Planned {
                    path: e.path.clone(),
                    access: e.access.clone(),
                    reason: format!("{}, the link itself", e.reason),
                });
                e.reason = format!("{}, through a link at {}", e.reason, shown(&full, home));
                e.path = parent;
                e.access = e.access.renamed(tname);
                continue;
            }
            let marker = if matches!(e.access, Access::ReadName(_)) {
                "+r"
            } else {
                "+w"
            };
            return Err(link_refused(&full, &t, marker, home));
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
        return Err(link_refused(&e.path, &t, marker, home));
    }
    out.extend(links);
    Ok(out)
}

/// Rule 5's refusal: `path` leads through a link to `t`, which no root
/// covers.
fn link_refused(path: &str, t: &str, marker: &str, home: &str) -> Refused {
    Refused {
        subject: shown(path, home),
        problem: format!(
            "leads through a link to {}, outside the current directory and this line's grants",
            shown(t, home)
        ),
        hint: Some(format!(
            "to give it, put {marker} {} before the command",
            shown(t, home)
        )),
    }
}

/// Merges a plan's entries by path, in the order each path first appears.
/// Parts that all narrow to one name stay narrowed to it; otherwise the
/// directory is given whole with the union of the rights, and a narrowed
/// part is shown in rule 2's fallback words, so `grants` says what is given.
pub fn merge(plan: &[Planned]) -> Vec<Merged> {
    covered(by_path(plan))
}

/// The directory of the system's fonts (ADR-0162, ADR-0326 item 4).
pub const SYSTEM_FONTS: &str = "/system/fonts";

/// What a POSIX program of the image is given with its window (ADR-0326
/// item 4, INV-0326-1): `/system/fonts` read only, "with the window", when
/// its description has `window`; nothing for one without. This is the only
/// place that gives a POSIX program that directory.
pub fn with_window(description: &Description) -> Option<Planned> {
    description.window.as_ref().map(|_| Planned {
        path: String::from(SYSTEM_FONTS),
        access: Access::Read,
        reason: String::from("with the window"),
    })
}

/// The most pills a window shows (`zoraya_panel::window_record` accepts as
/// many lines).
pub const MAX_WINDOW_PILLS: usize = 16;

/// The pills of a POSIX program's window (ADR-0317 item 3, INV-0317-1): the
/// network's first, `net_pills` as `netgrant::pills` writes them (`+net
/// TARGET`, or `web` for the web rule, ADR-0324), then `+r PATH`
/// or `+w PATH` for each merged entry of the plan. The window must show every
/// grant, so a line with more than [`MAX_WINDOW_PILLS`] pills is refused
/// (`Err` with how many it would have), never cut short.
pub fn window_pills(plan: &[Planned], net_pills: &[String]) -> Result<Vec<String>, usize> {
    let mut pills: Vec<String> = Vec::new();
    let mut add = |pill: String| {
        if !pills.contains(&pill) {
            pills.push(pill);
        }
    };
    for p in net_pills {
        add(p.clone());
    }
    for m in merge(plan) {
        let word = if m.rights & rights::WRITE != 0 {
            "+w"
        } else {
            "+r"
        };
        add(alloc::format!("{word} {}", m.path));
    }
    if pills.len() > MAX_WINDOW_PILLS {
        return Err(pills.len());
    }
    Ok(pills)
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
pub fn report(
    name: &str,
    plan: &[Planned],
    home: &str,
    streams: &str,
    posix: bool,
    keys: bool,
) -> String {
    report_with(name, plan, home, streams, posix, keys, None)
}

/// [`report`] with the program's own directory, shown with the real path and
/// the path the program sees (ADR-0341 item 4, INV-0341-3).
pub fn report_with(
    name: &str,
    plan: &[Planned],
    home: &str,
    streams: &str,
    posix: bool,
    keys: bool,
    own: Option<&Own>,
) -> String {
    let merged = merge(plan);
    let mut rows: Vec<(String, String)> = Vec::new();
    rows.push(("in, out, err".into(), streams.into()));
    // ADR-0331 item 3: a program with the terminal on any of its three
    // fds reads keys while its line is in front.
    if keys {
        rows.push(("keys".into(), crate::terminal::KEYS_WORDS.into()));
    }
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
    if let Some(own) = own {
        rows.push((own.real.clone(), own_words(own)));
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

/// The home directory of the image's user database, which `~` in a `data`
/// line means (ADR-0341 item 3): the program's view of its own directory.
pub const USER_HOME: &str = "/home";

/// ADR-0341 item 5: every `NAME` of a `data` line with the programs that
/// carry it, one `NAME program...` per line. The lint of INV-0341-4 keeps
/// it and the descriptions to each other.
pub const DATA_NAMES: &str = include_str!("../../../user/describe/data-names.txt");

/// The names of the list.
pub fn data_names() -> impl Iterator<Item = &'static str> {
    DATA_NAMES
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_whitespace().next())
}

/// The program's own directory a line gives (ADR-0341 items 2 and 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Own {
    /// The `NAME` of the description's `data` line.
    pub name: String,
    /// `/data/ROOM/NAME`, or `/data/home/NAME` outside a room: the shell's
    /// path to it, which it makes when the program first runs.
    pub real: String,
    /// Where the program sees it: `~` of the user database, `/home`, and the
    /// path of the line.
    pub view: String,
    /// `read`: to read only.
    pub read: bool,
}

/// What a program's own-directory entry is, if it has one. Only a system
/// image's description with a `data` line gives one (INV-0341-1), and only
/// for a name the list gives this program (INV-0341-4).
pub fn own_directory(d: &Description, base_system: bool, room: Option<&str>) -> Option<Own> {
    if !base_system {
        return None;
    }
    let data = d.data.as_ref()?;
    let listed = DATA_NAMES.lines().any(|l| {
        let mut words = l.split_whitespace();
        words.next() == Some(data.name.as_str()) && words.any(|p| p == d.name)
    });
    if !listed {
        return None;
    }
    Some(Own {
        name: data.name.clone(),
        real: format!("/data/{}/{}", room.unwrap_or("home"), data.name),
        view: format!("{USER_HOME}/{}", data.path.strip_prefix("~/")?),
        read: data.read,
    })
}

/// ADR-0341 item 8: whether a current directory is `/`, `/data`, the
/// directory of a room (or `home`) or inside a program's own directory, so
/// that the line gives no current-directory entry. A read connection on any
/// of them reaches every program's files below, and nothing can cut a
/// subtree out of one.
pub fn cwd_reaches_data(cwd: &str) -> bool {
    if cwd == "/" || cwd == "/data" {
        return true;
    }
    let Some(rest) = cwd.strip_prefix("/data/") else {
        return false;
    };
    let mut parts = rest.split('/');
    let _room = parts.next();
    match parts.next() {
        None => true,
        Some(name) => data_names().any(|n| n == name),
    }
}

/// ADR-0341 item 3: the own-directory entry is never merged with a grant of
/// the line. A `+r` or `+w` at or inside the path the program sees it at is
/// refused.
pub fn own_conflict(
    own: &Own,
    cwd: &str,
    grants: &[(bool, Value)],
    room_paths: &[(bool, String)],
) -> Result<(), Refused> {
    // A room's grants too (absolute): a room entry on the view path would
    // sit beside the own entry with the same prefix.
    let room = room_paths.iter().map(|(w, p)| (*w, p.clone(), p.clone()));
    let line = grants
        .iter()
        .map(|(w, v)| (*w, join(cwd, 0, &v.text), v.text.clone()));
    for (write, abs, shown_as) in line.chain(room) {
        if covers(&own.view, &abs) {
            return Err(Refused {
                subject: shown_as,
                problem: format!(
                    "is {}'s own directory, which its description gives and a grant cannot name",
                    own.name
                ),
                hint: Some(format!(
                    "to read the files yourself, name them: cat {}/FILE",
                    own.real
                )),
            });
        }
        // A `+w` that covers the real directory (`/data`, `/data/home`, the room's own
        // `/data/ROOM`) would be a writable entry a child inherits beside the read-only
        // one of the own entry (ADR-0341 item 7, review of 10g-2, finding 5).
        if write && covers(&abs, &own.real) {
            return Err(Refused {
                subject: shown_as,
                problem: format!(
                    "would give a write handle on {}, {}'s own directory, to its children",
                    own.real, own.name
                ),
                hint: Some(String::from(
                    "give a narrower directory, or +r: the program is given its own files already",
                )),
            });
        }
    }
    Ok(())
}

/// How `grants` words the entry (ADR-0341 item 4).
pub fn own_words(own: &Own) -> String {
    let seen = shown(&own.view, USER_HOME);
    if own.read {
        format!("its own files, to read (it sees them as {seen})")
    } else {
        format!("its own files (it sees them as {seen})")
    }
}

/// ADR-0340 item 4: for a description with a `rest` operand, where the shell
/// writes `--` into the program's arguments: before the operand that comes
/// first, the host, so that a program that reads options again after the host
/// (OpenSSH does) reads none. `None` for a description without one, a line
/// that does not parse, a line with no operand, or one that already has the
/// `--` there. The index is into `args` (the words after the program's name).
pub fn end_of_options_at(d: &Description, args: &[&str]) -> Option<usize> {
    d.operands.iter().find(|o| o.kind == Kind::Rest)?;
    let Ok(zoraya_command::Outcome::Run(parsed)) = zoraya_command::parse(d, args) else {
        return None;
    };
    let (_, first) = parsed.operands.first()?;
    // The operand's text is a slice of the argument it came from.
    let at = args
        .iter()
        .position(|a| core::ptr::eq(a.as_ptr(), first.as_ptr()) && a.len() == first.len())?;
    (at == 0 || args[at - 1] != "--").then_some(at)
}

/// ADR-0340 item 4 and ADR-0341: a description with a `data` or `reach` line
/// is held to its options as the shell reads them, but the program parses
/// short options only (OpenSSH's `getopt`): a long form of a listed option
/// would be granted by the shell and be a usage error of the program. The word
/// and the short form to write instead, for the first long option among the
/// words the shell reads (those before the host, or before `--`).
pub fn long_option_used(d: &Description, args: &[&str]) -> Option<(String, String)> {
    let end = args.iter().position(|a| *a == "--").unwrap_or(args.len());
    let end = end_of_options_at(d, args).map_or(end, |host| host.min(end));
    let mut i = 0;
    while i < end {
        let word = args[i];
        i += 1;
        let Some(long) = word.strip_prefix("--").filter(|l| !l.is_empty()) else {
            continue;
        };
        let name = long.split_once('=').map_or(long, |(n, _)| n);
        if name == "help" {
            continue;
        }
        let short = d
            .option_by_long(name)
            .and_then(|o| d.options[o].short)
            .map(|c| alloc::format!("-{c}"))
            .unwrap_or_default();
        return Some((word.into(), short));
    }
    None
}

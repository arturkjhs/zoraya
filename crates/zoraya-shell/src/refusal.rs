//! A refusal a running program reported, as the person is shown it (ADR-0337
//! items 1 to 3, INV-0337-5 and INV-0337-6). Pure, so every rule is tested
//! on the host; `zosh` reads the reports and sends what this module words.
//!
//! A report is whatever the program wrote: it is hostile data. A path is
//! shown only if, after the shell normalises it itself, the whole of it is
//! made of the characters ADR-0311 allows in a name and `/`, and it fits the
//! row; anything else is shown as [`CANNOT_SHOW`]. What is shown is what the
//! program *reported*: it can report a path it never opened, and cannot make
//! the line say anything but a path.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::grants::shown;

/// What takes the place of a path the filter refuses (ADR-0337 item 2).
pub const CANNOT_SHOW: &str = "a path that cannot be shown";

/// The longest path shown, in bytes: it must fit the row whole (a row of 80
/// columns keeps room for the program's name and the key's words).
pub const MAX_SHOWN: usize = 60;

/// The most distinct refusals a program's line remembers for its count; the
/// ones past it are only counted.
pub const MAX_PENDING: usize = 64;

/// Whether `c` may be in a shown path: ASCII letters and digits and
/// `. - _ + ~ @ , =` (ADR-0311), and `/`.
const fn allowed(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | '~' | '@' | ',' | '=' | '/')
}

/// The path as the shell normalises it (`.` and empty components dropped,
/// `..` removing the one before it, never above `/`), or `None` when it is
/// not absolute.
fn normalised(path: &str) -> Option<String> {
    let rest = path.strip_prefix('/')?;
    let mut parts: Vec<&str> = Vec::new();
    for c in rest.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    let mut out = String::from("/");
    out.push_str(&parts.join("/"));
    Some(out)
}

/// A host and port as shown (ADR-0224 item 5): the name or dotted address as
/// the program gave it, if it is made of ASCII letters, digits, `.`, `-` and
/// `_` and fits the row, and `:PORT` unless the port is 0. `None` otherwise.
pub fn shown_target(target: &str, port: u16) -> Option<String> {
    let host_ok = !target.is_empty()
        && target.len() <= MAX_SHOWN - 6
        && target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    host_ok.then(|| {
        if port == 0 {
            String::from(target)
        } else {
            format!("{target}:{port}")
        }
    })
}

/// The path to show, or `None` when it may not be shown (ADR-0337 item 2).
/// Total: any text gives a path or `None`.
pub fn shown_path(path: &str) -> Option<String> {
    let n = normalised(path)?;
    if n.len() > MAX_SHOWN || !n.chars().all(allowed) {
        return None;
    }
    // A name made of dots only is not a name a person reads as one.
    if n.split('/')
        .any(|c| !c.is_empty() && c.chars().all(|c| c == '.'))
    {
        return None;
    }
    Some(n)
}

/// The path as a hint or the status row prints it: with `~` for the home
/// directory, or [`CANNOT_SHOW`]. Every character that survives is one a
/// shell word needs no quoting for (the leading `/` or `~/` keeps `~` from
/// being read as a user's home), so the hint can be pasted back as a word.
pub fn hint_path(path: &str, home: &str) -> String {
    match shown_path(path) {
        Some(p) => shown(&p, home),
        None => String::from(CANNOT_SHOW),
    }
}

/// A program's name for the row: its characters other than printable ASCII
/// become `?`, and at most 32 are kept. The name is the person's own text;
/// the row still shows only what it can show.
pub fn program_name(name: &str) -> String {
    name.chars()
        .take(32)
        .map(|c| if (' '..='~').contains(&c) { c } else { '?' })
        .collect()
}

/// Which of ADR-0337 item 3's three things is true of a refused path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// The shell holds it and the program does not: `was not given`.
    NotGiven,
    /// The shell holds the place and nothing is there: a typing mistake.
    DoesNotExist,
    /// The shell does not hold it: nothing can be given from here.
    NotHeld,
}

/// The shell's own table, looked up without opening anything and without
/// following a link object (INV-0337-6).
pub trait View {
    /// Whether the shell's table covers `path`.
    fn holds(&self, path: &str) -> bool;
    /// Whether the shell finds something at `path` by the table's own entries
    /// (a `stat` that follows no link object).
    fn exists(&self, path: &str) -> bool;
}

/// The case of `path` (already shown, so absolute and normal).
pub fn classify(view: &dyn View, path: &str) -> Case {
    if !view.holds(path) {
        Case::NotHeld
    } else if !view.exists(path) {
        Case::DoesNotExist
    } else {
        Case::NotGiven
    }
}

/// One refusal as reported: the path that passed the filter (`None` for one
/// that did not), the access, and the element of the line it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub path: Option<String>,
    pub write: bool,
    pub element: usize,
    /// A refused network request: the host and port as shown (`Some(None)`:
    /// they may not be shown). `path` is `None` then.
    pub net: Option<Option<String>>,
    /// The reported text itself, kept only to tell refusals apart.
    key: String,
}

/// The refusals of the line in front since its program last received a
/// grant (ADR-0337 item 1: the limit restarts after a grant).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Pending {
    seen: Vec<Refusal>,
    latest: Option<usize>,
    overflow: u32,
}

impl Pending {
    pub fn new() -> Pending {
        Pending::default()
    }

    /// A report arrived: `path` is the reported text, `write` the access,
    /// `element` the line's element it came from. Returns whether it is new
    /// (a path and access seen already change nothing).
    pub fn note(&mut self, path: &str, write: bool, element: usize) -> bool {
        if self
            .seen
            .iter()
            .any(|r| r.key == path && r.write == write && r.element == element)
        {
            return false;
        }
        if self.seen.len() >= MAX_PENDING {
            self.overflow = self.overflow.saturating_add(1);
            return false;
        }
        self.seen.push(Refusal {
            path: shown_path(path),
            write,
            element,
            net: None,
            key: String::from(path),
        });
        self.latest = Some(self.seen.len() - 1);
        true
    }

    /// A report of a refused network request (ADR-0224 item 5): `target` as
    /// the program wrote it, `port` 0 for a lookup.
    pub fn note_net(&mut self, target: &str, port: u16, element: usize) -> bool {
        let key = format!("net:{target}:{port}");
        if self
            .seen
            .iter()
            .any(|r| r.key == key && r.element == element)
        {
            return false;
        }
        if self.seen.len() >= MAX_PENDING {
            self.overflow = self.overflow.saturating_add(1);
            return false;
        }
        self.seen.push(Refusal {
            path: None,
            write: false,
            element,
            net: Some(shown_target(target, port)),
            key,
        });
        self.latest = Some(self.seen.len() - 1);
        true
    }

    /// The latest refusal.
    pub fn latest(&self) -> Option<&Refusal> {
        self.latest.and_then(|i| self.seen.get(i))
    }

    /// How many other refusals wait.
    pub fn more(&self) -> usize {
        (self.seen.len() + self.overflow as usize).saturating_sub(1)
    }

    /// A grant was given: the count starts again.
    pub fn clear(&mut self) {
        *self = Pending::default();
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

/// The words of the status row for the latest refusal (ADR-0337 item 3):
///
/// ```text
/// vi was not given ~/other.txt (to read)  [2 more]
/// vi asked for ~/othr.txt, which does not exist
/// vi asked for /x, which this shell does not hold
/// vi was refused a path that cannot be shown
/// ```
///
/// The words `System+G: give` are the terminal's, at the right end.
pub fn notice(program: &str, r: &Refusal, case: Case, home: &str, more: usize) -> String {
    let name = program_name(program);
    let mut text = match &r.path {
        None if r.net.is_some() => match &r.net {
            Some(Some(t)) => format!("{name} was not given {t} (to reach)"),
            _ => format!("{name} was refused a host that cannot be shown"),
        },
        None => format!("{name} was refused {CANNOT_SHOW}"),
        Some(p) => {
            let p = shown(p, home);
            match case {
                Case::NotGiven => format!(
                    "{name} was not given {p} (to {})",
                    if r.write { "change" } else { "read" }
                ),
                Case::DoesNotExist => format!("{name} asked for {p}, which does not exist"),
                Case::NotHeld => format!("{name} asked for {p}, which this shell does not hold"),
            }
        }
    };
    if more > 0 {
        text.push_str(&format!("  [{more} more]"));
    }
    text
}

/// What the row says after a grant (ADR-0337's "What the person sees").
pub fn given_notice(program: &str, words: &str) -> String {
    format!("given until {} ends: {words}", program_name(program))
}

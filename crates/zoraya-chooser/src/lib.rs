//! The chooser's logic (ADR-0167, proposed ADR-0245): what a sheet lists,
//! what the person may pick in it, and the grant each pick gives, in
//! ADR-0153's kinds and rights (the shell's own [`Access`] table, row for
//! row, INV-0167-3) and in the words the sheet, the frame and the shell
//! use. No I/O here: the program lists directories and makes connections
//! through a [`Volume`], and only after [`Browser::confirm`], which only a
//! person's press or key reaches (INV-0167-2).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_proto_chooser::{kind, sheet};
use zoraya_protocol::Name;
pub use zoraya_shell::grants::Access;

/// One name in a listed directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    /// In bytes; 0 for a directory.
    pub size: u64,
}

/// What the application asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// A file to read; `filters` are extensions without the dot.
    Open { filters: Vec<String> },
    /// A name to create or replace, starting from `suggested`.
    Save {
        suggested: String,
        filters: Vec<String>,
    },
    /// A directory, to read or (`write`) to change anything in.
    Pick { write: bool },
}

impl Ask {
    /// Parses a filter list (`txt md`), dropping what cannot be an
    /// extension.
    pub fn filters(text: &str) -> Vec<String> {
        text.split(' ')
            .filter(|f| !f.is_empty() && Name::is_valid(f) && !f.contains('.'))
            .map(|f| f.to_lowercase())
            .collect()
    }

    /// The heading kind the compositor draws (`zoraya-proto-chooser`'s
    /// `sheet`).
    pub fn sheet(&self) -> u32 {
        match self {
            Ask::Open { .. } => sheet::OPEN,
            Ask::Save { .. } => sheet::SAVE,
            Ask::Pick { .. } => sheet::PICK,
        }
    }
}

/// The connection to make for a confirmed pick, and how to say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// The absolute path of the directory the pick is in (`Open`, `Save`)
    /// or of the picked directory (`Pick`).
    pub dir: String,
    /// The file to open (`Open`) or the one name to narrow to (`Save`).
    pub name: Option<String>,
    pub access: Access,
    /// `zoraya-proto-chooser`'s `kind`.
    pub kind: u32,
    /// The display name the application gets.
    pub shown: String,
    /// The line under the list: the grant as the shell would make it
    /// (`+r ~/project/plán.txt edit`), for the person only.
    pub line: String,
    /// `Chosen`'s words: the kind and the display name, never the path
    /// (ADR-0167: an application never learns where what it got is).
    pub words: String,
    /// The frame's pill (`+r ~/project/plán.txt`).
    pub pill: String,
}

impl Grant {
    /// The connection's rights: ADR-0153's table, by the shell's own
    /// [`Access`].
    pub fn rights(&self) -> u32 {
        self.access.rights()
    }
}

/// The sheet's state: where it is, what it lists, what is picked.
#[derive(Debug, Clone)]
pub struct Browser {
    pub ask: Ask,
    /// The application's name, from the panel, never from the request.
    pub app: String,
    /// The shell's home (`shell.home`), for `~`.
    pub home: String,
    /// The listed directory's components from the root.
    pub path: Vec<String>,
    /// What the directory shows: folders first, then the files the
    /// filters let through (none for `Pick`), each group by name.
    pub entries: Vec<Entry>,
    pub selected: Option<usize>,
    /// `Save`'s name field.
    pub name: String,
}

/// What a press or a key on a row asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing to do.
    None,
    /// List the new `path` (the program calls [`Browser::show`]).
    List,
    /// The person confirmed: make this grant.
    Confirm(Grant),
}

impl Browser {
    /// A sheet for `ask` from `app`, starting at `home`.
    pub fn new(ask: Ask, app: &str, home: &str) -> Browser {
        let name = match &ask {
            Ask::Save { suggested, .. } if Name::is_valid(suggested) => suggested.clone(),
            _ => String::new(),
        };
        Browser {
            ask,
            app: app.to_string(),
            home: home.to_string(),
            path: components(home),
            entries: Vec::new(),
            selected: None,
            name,
        }
    }

    /// The listed directory's absolute path.
    pub fn dir(&self) -> String {
        let mut s = String::new();
        for c in &self.path {
            s.push('/');
            s.push_str(c);
        }
        if s.is_empty() {
            s.push('/');
        }
        s
    }

    /// The listing to show: `raw` filtered by the request and sorted.
    pub fn show(&mut self, raw: Vec<Entry>) {
        let filters: &[String] = match &self.ask {
            Ask::Open { filters } | Ask::Save { filters, .. } => filters,
            Ask::Pick { .. } => &[],
        };
        let files = !matches!(self.ask, Ask::Pick { .. });
        let mut kept: Vec<Entry> = raw
            .into_iter()
            .filter(|e| Name::is_valid(&e.name))
            .filter(|e| e.dir || (files && passes(&e.name, filters)))
            .collect();
        kept.sort_by(|a, b| {
            b.dir
                .cmp(&a.dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        self.entries = kept;
        self.selected = None;
    }

    /// The breadcrumbs: `~ › project`, or from `/` outside home.
    pub fn crumbs(&self) -> String {
        let shown = zoraya_shell::grants::shown(&self.dir(), &self.home);
        let mut parts: Vec<&str> = Vec::new();
        let rest = if let Some(r) = shown.strip_prefix('~') {
            parts.push("~");
            r
        } else {
            parts.push("/");
            &shown[..]
        };
        parts.extend(rest.split('/').filter(|p| !p.is_empty()));
        parts.join(" › ")
    }

    /// The person moved the selection to row `i`.
    pub fn select(&mut self, i: usize) {
        if i < self.entries.len() {
            self.selected = Some(i);
            if matches!(self.ask, Ask::Save { .. }) && !self.entries[i].dir {
                self.name = self.entries[i].name.clone();
            }
        }
    }

    /// The person activated row `i` (a double press or Enter): a folder
    /// is entered; a file is confirmed for `Open` and `Save`.
    pub fn activate(&mut self, i: usize) -> Step {
        let Some(e) = self.entries.get(i).cloned() else {
            return Step::None;
        };
        if e.dir {
            self.path.push(e.name);
            self.entries.clear();
            self.selected = None;
            return Step::List;
        }
        self.select(i);
        self.confirm()
    }

    /// Up one level (the crumbs' parent); nothing at the root.
    pub fn up(&mut self) -> Step {
        if self.path.pop().is_none() {
            return Step::None;
        }
        self.entries.clear();
        self.selected = None;
        Step::List
    }

    /// What confirming now would give, for the sentence under the list and
    /// to enable the button; `None` while nothing can be confirmed.
    pub fn pending(&self) -> Option<Grant> {
        let dir = self.dir();
        match &self.ask {
            Ask::Open { .. } => {
                let e = self.entries.get(self.selected?)?;
                if e.dir {
                    return None;
                }
                let path = join(&dir, &e.name);
                Some(self.grant(dir, Some(e.name.clone()), Access::Read, &path, "+r"))
            }
            Ask::Save { .. } => {
                if !Name::is_valid(&self.name) {
                    return None;
                }
                let path = join(&dir, &self.name);
                let access = Access::Create(self.name.clone());
                Some(self.grant(dir, Some(self.name.clone()), access, &path, "+c"))
            }
            Ask::Pick { write } => {
                let picked = match self.selected.and_then(|i| self.entries.get(i)) {
                    Some(e) if e.dir => join(&dir, &e.name),
                    _ => dir,
                };
                let (access, flag) = if *write {
                    (Access::Tree, "+w")
                } else {
                    (Access::Read, "+r")
                };
                Some(self.grant(picked.clone(), None, access, &picked, flag))
            }
        }
    }

    fn grant(
        &self,
        dir: String,
        name: Option<String>,
        access: Access,
        path: &str,
        flag: &str,
    ) -> Grant {
        let shown = zoraya_shell::grants::shown(path, &self.home);
        let kind = match access {
            Access::Create(_) => kind::CREATE,
            Access::Tree => kind::TREE,
            _ => kind::READ,
        };
        let line = match access {
            // The shell has no `+` form for one name to create: the words
            // `grants` would show.
            Access::Create(_) => format!("create or replace {shown}"),
            _ => format!("{flag} {shown} {}", self.app),
        };
        let display = match path.rsplit('/').next() {
            Some("") | None => String::from("/"),
            Some(n) => n.to_string(),
        };
        Grant {
            dir,
            name,
            access,
            kind,
            words: format!("{flag} {display}"),
            shown: display,
            line,
            pill: format!("{flag} {shown}"),
        }
    }

    /// The sentence over the grant's line: "edit will be given this file,
    /// to read:".
    pub fn sentence(&self) -> String {
        let what = match &self.ask {
            Ask::Open { .. } => "this file, to read",
            Ask::Save { .. } => "this name, to create or replace",
            Ask::Pick { write: false } => "this folder, to read",
            Ask::Pick { write: true } => "this folder, to change anything in",
        };
        format!("{} will be given {what}:", self.app)
    }

    /// The person pressed Open, Save or Choose (or Enter on the field):
    /// the grant, if one can be made.
    pub fn confirm(&self) -> Step {
        match self.pending() {
            Some(g) => Step::Confirm(g),
            None => Step::None,
        }
    }
}

fn components(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .map(String::from)
        .collect()
}

fn join(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Whether a file name passes the filters (none: every file).
fn passes(name: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => filters.iter().any(|f| *f == ext.to_lowercase()),
        _ => false,
    }
}

/// A size as the sheet and the file manager show it: `412 B`, `1.9 kB`,
/// `2.1 MB` (powers of 1000, one decimal under 10).
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut unit = 0;
    let mut div: u128 = 1000;
    while u128::from(bytes) >= div * 1000 && unit + 1 < UNITS.len() {
        div *= 1000;
        unit += 1;
    }
    // Tenths of the unit, rounded half up.
    let tenths = (u128::from(bytes) * 10 + div / 2) / div;
    if tenths < 100 {
        format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
    } else {
        format!("{} {}", (tenths + 5) / 10, UNITS[unit])
    }
}

pub mod session;
pub use session::{Outcome, Sheet};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_session;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

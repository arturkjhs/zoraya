//! The file manager's logic (plan 7e): the folder shown, its rows, and
//! what each press asks the program to do. No I/O here: the program lists
//! folders and carries out each [`Act`] through `zoraya-fsclient`, and
//! opens a file only through the panel (`OpenWith`, ADR-0168 point 4),
//! which launches the file's application with one read-only connection.
//!
//! Delete asks twice: the first press names what would go, the second, on
//! the same row with nothing else between, removes it.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_protocol::Name;
use zoraya_shell::grants;

/// One name in the folder shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

/// What a press asks the program to do. Paths are absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// List this folder and show it.
    Show(String),
    /// Open `path` read-only and hand it to the panel; `words` are the
    /// grant as the record and the pills say it.
    OpenWith {
        path: String,
        words: String,
    },
    MakeDirectory {
        dir: String,
        name: String,
    },
    Rename {
        dir: String,
        old: String,
        new: String,
    },
    Remove {
        dir: String,
        name: String,
    },
    /// Nothing to do; the status line says why.
    Say(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manager {
    /// The folder shown, absolute.
    pub dir: String,
    pub home: String,
    /// Folders first, then files, each by name.
    pub entries: Vec<Entry>,
    /// The row a first Delete press named.
    delete_armed: Option<usize>,
}

/// `name` in `dir`.
pub fn join(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The folder above `dir`; `/` has none.
pub fn parent(dir: &str) -> Option<String> {
    if dir == "/" {
        return None;
    }
    Some(match dir.rsplit_once('/') {
        Some(("", _)) | None => String::from("/"),
        Some((p, _)) => String::from(p),
    })
}

impl Manager {
    pub fn new(dir: &str, home: &str) -> Manager {
        Manager {
            dir: String::from(dir),
            home: String::from(home),
            entries: Vec::new(),
            delete_armed: None,
        }
    }

    /// The folder `dir` with what it holds.
    pub fn show(&mut self, dir: &str, mut entries: Vec<Entry>) {
        entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
        self.dir = String::from(dir);
        self.entries = entries;
        self.delete_armed = None;
    }

    /// The folder as the shell writes it (`~/project`).
    pub fn shown(&self) -> String {
        grants::shown(&self.dir, &self.home)
    }

    /// The list's rows: a folder ends in `/`.
    pub fn rows(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|e| {
                if e.dir {
                    format!("{}/", e.name)
                } else {
                    e.name.clone()
                }
            })
            .collect()
    }

    fn entry(&self, row: Option<usize>) -> Result<&Entry, Act> {
        row.and_then(|r| self.entries.get(r))
            .ok_or_else(|| Act::Say(String::from("Nothing is selected.")))
    }

    /// Enter or Open on `row`: a folder is shown, a file is opened with
    /// its application, read-only.
    pub fn activate(&mut self, row: Option<usize>) -> Act {
        self.delete_armed = None;
        match self.entry(row) {
            Ok(e) if e.dir => Act::Show(join(&self.dir, &e.name)),
            Ok(e) => {
                let path = join(&self.dir, &e.name);
                let words = format!("+r {}", grants::shown(&path, &self.home));
                Act::OpenWith { path, words }
            }
            Err(a) => a,
        }
    }

    pub fn up(&mut self) -> Act {
        self.delete_armed = None;
        match parent(&self.dir) {
            Some(p) => Act::Show(p),
            None => Act::Say(String::from("This is the top of the volume.")),
        }
    }

    fn valid(&self, name: &str) -> Result<(), Act> {
        let name_ok = Name::is_valid(name) && name != "." && name != "..";
        if !name_ok {
            return Err(Act::Say(format!("\u{201c}{name}\u{201d} is not a name.")));
        }
        if self.entries.iter().any(|e| e.name == name) {
            return Err(Act::Say(format!("{name} is already here.")));
        }
        Ok(())
    }

    pub fn new_folder(&mut self, name: &str) -> Act {
        self.delete_armed = None;
        let name = name.trim();
        match self.valid(name) {
            Ok(()) => Act::MakeDirectory {
                dir: self.dir.clone(),
                name: String::from(name),
            },
            Err(a) => a,
        }
    }

    pub fn rename(&mut self, row: Option<usize>, name: &str) -> Act {
        self.delete_armed = None;
        let name = name.trim();
        let old = match self.entry(row) {
            Ok(e) => e.name.clone(),
            Err(a) => return a,
        };
        if old == name {
            return Act::Say(format!("{name} already has that name."));
        }
        match self.valid(name) {
            Ok(()) => Act::Rename {
                dir: self.dir.clone(),
                old,
                new: String::from(name),
            },
            Err(a) => a,
        }
    }

    /// The first press on a row names it; the second removes it.
    pub fn delete(&mut self, row: Option<usize>) -> Act {
        let name = match self.entry(row) {
            Ok(e) => e.name.clone(),
            Err(a) => {
                self.delete_armed = None;
                return a;
            }
        };
        if self.delete_armed.is_some() && self.delete_armed == row {
            self.delete_armed = None;
            return Act::Remove {
                dir: self.dir.clone(),
                name,
            };
        }
        self.delete_armed = row;
        Act::Say(format!("Delete {name}? Press Delete again."))
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

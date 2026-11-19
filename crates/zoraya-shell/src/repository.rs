//! The repository of a line (ADR-0339 items 2 and 3): the `repository` key of
//! a description, and the `create-tree` kind's two commands.
//!
//! The search is pure: it asks the table what it holds and what a directory
//! holds, and gives back at most one entry (the nearest directory that has a
//! `.git` directory, inside what the shell holds), the ceiling for git's own
//! search, or the line's refusal. It never looks above what the shell holds
//! and never makes a grant from what a repository says (INV-0339-1, CP-28).

use alloc::format;
use alloc::string::{String, ToString};

use crate::grants::{Access, Planned, Refused};

/// What a directory holds as `.git`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// A directory named `.git`.
    Directory,
    /// A file or link named `.git` (a work tree or submodule whose repository
    /// is elsewhere).
    File,
    /// No `.git`.
    Nothing,
}

/// What the shell holds at a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Held {
    /// Nothing: the search stops (the edge of the table).
    No,
    /// To read.
    Read,
    /// To change anything in.
    Write,
}

/// The answer of the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The entry for the repository's root, if one was found.
    pub entry: Option<Planned>,
    /// `GIT_CEILING_DIRECTORIES`: the parent of what was found, or of the
    /// starting directory. git never goes up into a ceiling directory
    /// (measured: the ceiling itself is not searched), so the parent is the
    /// directory the search must not reach; it need not exist in the
    /// program's table.
    pub ceiling: String,
}

/// The parent of an absolute path (`/` is its own).
fn parent(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(cut) => path[..cut].to_string(),
    }
}

/// Looks for the repository of `start` (the current directory, or `-C DIR`),
/// absolute and normal, walking up while `held` says the shell holds the
/// directory. `home` is the user's home directory. A `.git` that is a file,
/// and a repository whose root is `home` or `/`, refuse the line.
pub fn find(
    start: &str,
    home: &str,
    probe: &dyn Fn(&str) -> Probe,
    held: &dyn Fn(&str) -> Held,
) -> Result<Found, Refused> {
    let mut dir = start.to_string();
    loop {
        let rights = held(&dir);
        if rights == Held::No {
            break;
        }
        match probe(&dir) {
            Probe::Nothing => {}
            Probe::File => {
                return Err(Refused {
                    subject: format!("{dir}/.git"),
                    problem: "a .git that is a file: the repository is elsewhere".into(),
                    hint: Some(
                        "write the grant for the directory it is in: +w DIRECTORY git ...".into(),
                    ),
                });
            }
            Probe::Directory => {
                if dir == "/" || dir == home {
                    let shown = if dir == "/" { "/" } else { "~" };
                    return Err(Refused {
                        subject: shown.to_string(),
                        problem: format!(
                            "this repository is {}",
                            if dir == "/" {
                                "the whole volume"
                            } else {
                                "your whole home directory"
                            }
                        ),
                        hint: Some(format!("to allow it: +w {shown} git ...")),
                    });
                }
                return Ok(Found {
                    ceiling: parent(&dir),
                    entry: Some(Planned {
                        access: if rights == Held::Write {
                            Access::Tree
                        } else {
                            Access::Read
                        },
                        path: dir,
                        reason: "the repository of the current directory".into(),
                    }),
                });
            }
        }
        if dir == "/" {
            break;
        }
        dir = parent(&dir);
    }
    Ok(Found {
        entry: None,
        ceiling: parent(start),
    })
}

/// git applies every `-C` in turn and the shell searches from one directory: a second is refused
/// (review of 10h, finding 4: `git -C ~ -C sub` would start where the shell did not look).
pub fn one_directory(count: usize) -> Result<(), Refused> {
    if count <= 1 {
        return Ok(());
    }
    Err(Refused {
        subject: "-C".into(),
        problem: "git gets one -C: the shell looks for the repository from one directory".into(),
        hint: Some("write the directory once".into()),
    })
}

/// A `create-tree` entry gives everything in the directory it makes, and on one that exists, everything in it: the home
/// directory, `/` and the directories above the home are not made that way (`git init ~` would give the whole
/// home without `+w`, which the repository rule refuses, review of 10h, finding 7).
pub fn create_tree_target(plan: &[Planned], home: &str) -> Result<(), Refused> {
    for e in plan {
        let Access::CreateTree(name) = &e.access else {
            continue;
        };
        let target = zoraya_resolve::join(&e.path, 0, name);
        if zoraya_resolve::covers(&target, home) {
            return Err(Refused {
                subject: name.clone(),
                problem: "a directory that holds your home directory, or is it, is not made or entered this way".into(),
                hint: Some("make a directory below your home: git init ~/NAME".into()),
            });
        }
    }
    Ok(())
}

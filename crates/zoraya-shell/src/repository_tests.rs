//! Host tests of the repository search (ADR-0339 items 2 and 3; INV-0339-1).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_resolve::covers;

use crate::grants::Access;
use crate::repository::{Held, Probe, find};

/// A table: the directories that hold a `.git` of some kind, and the entries
/// the shell holds (prefix, writable).
struct Table {
    git: Vec<(&'static str, Probe)>,
    held: Vec<(&'static str, bool)>,
}

impl Table {
    fn find(&self, start: &str) -> Result<crate::repository::Found, crate::grants::Refused> {
        let probe = |dir: &str| {
            self.git
                .iter()
                .find(|(d, _)| *d == dir)
                .map_or(Probe::Nothing, |(_, p)| *p)
        };
        let held = |dir: &str| {
            let mut best = Held::No;
            for (prefix, write) in &self.held {
                if covers(prefix, dir) {
                    let h = if *write { Held::Write } else { Held::Read };
                    if h > best {
                        best = h;
                    }
                }
            }
            best
        };
        find(start, "/home/user", &probe, &held)
    }
}

fn path(found: &crate::repository::Found) -> Option<(String, Access)> {
    found
        .entry
        .as_ref()
        .map(|e| (e.path.clone(), e.access.clone()))
}

#[test]
fn the_nearest_repository_is_given_with_what_the_shell_holds() {
    let t = Table {
        git: Vec::from([
            ("/home/user/proj", Probe::Directory),
            // A repository inside the work tree of another (a nested one).
            ("/home/user/proj/vendor/lib", Probe::Directory),
        ]),
        held: Vec::from([("/home/user/proj", true)]),
    };
    let f = t.find("/home/user/proj/src").unwrap();
    assert_eq!(
        path(&f),
        Some(("/home/user/proj".to_string(), Access::Tree))
    );
    assert_eq!(f.ceiling, "/home/user");
    assert_eq!(
        f.entry.as_ref().unwrap().reason,
        "the repository of the current directory"
    );
    // The nested one is the nearest from inside it, and nothing above it is given.
    let f = t.find("/home/user/proj/vendor/lib/src").unwrap();
    assert_eq!(
        path(&f),
        Some(("/home/user/proj/vendor/lib".to_string(), Access::Tree))
    );
    assert_eq!(f.ceiling, "/home/user/proj/vendor");
}

#[test]
fn a_read_only_table_gives_read() {
    let t = Table {
        git: Vec::from([("/home/user/proj", Probe::Directory)]),
        held: Vec::from([("/home/user/proj", false)]),
    };
    let f = t.find("/home/user/proj/src").unwrap();
    assert_eq!(
        path(&f),
        Some(("/home/user/proj".to_string(), Access::Read))
    );
}

#[test]
fn a_repository_above_the_edge_of_the_table_is_not_looked_for() {
    // The room holds `proj/src` only; the repository is at `proj`, outside it.
    let t = Table {
        git: Vec::from([("/home/user/proj", Probe::Directory)]),
        held: Vec::from([("/home/user/proj/src", true)]),
    };
    let f = t.find("/home/user/proj/src").unwrap();
    assert_eq!(f.entry, None);
    // Git's own search ends at the parent of the starting directory.
    assert_eq!(f.ceiling, "/home/user/proj");
}

#[test]
fn no_repository_gives_no_entry() {
    let t = Table {
        git: Vec::new(),
        held: Vec::from([("/home/user", true)]),
    };
    let f = t.find("/home/user/notes").unwrap();
    assert_eq!(f.entry, None);
    assert_eq!(f.ceiling, "/home/user");
}

#[test]
fn a_git_file_refuses_the_line() {
    let t = Table {
        git: Vec::from([("/home/user/proj/wt", Probe::File)]),
        held: Vec::from([("/home/user/proj", true)]),
    };
    let r = t.find("/home/user/proj/wt/src").unwrap_err();
    assert_eq!(r.subject, "/home/user/proj/wt/.git");
    assert!(r.problem.contains("file"), "{}", r.problem);
}

#[test]
fn a_repository_that_is_the_home_directory_or_the_root_refuses_the_line() {
    let t = Table {
        git: Vec::from([("/home/user", Probe::Directory)]),
        held: Vec::from([("/home/user", true)]),
    };
    let r = t.find("/home/user/a").unwrap_err();
    assert!(r.problem.contains("home directory"), "{}", r.problem);
    assert_eq!(r.hint.as_deref(), Some("to allow it: +w ~ git ..."));
    let t = Table {
        git: Vec::from([("/", Probe::Directory)]),
        held: Vec::from([("/", true)]),
    };
    let r = t.find("/x").unwrap_err();
    assert!(r.problem.contains("volume"), "{}", r.problem);
}

#[test]
fn the_search_from_c_starts_there() {
    // `-C other` starts at `other`: a repository above the current directory is not found from it.
    let t = Table {
        git: Vec::from([("/home/user/proj", Probe::Directory)]),
        held: Vec::from([("/home/user", true)]),
    };
    let f = t.find("/home/user/other").unwrap();
    assert_eq!(f.entry, None);
    assert_eq!(f.ceiling, "/home/user");
    let f = t.find("/home/user/proj").unwrap();
    assert_eq!(
        path(&f),
        Some(("/home/user/proj".to_string(), Access::Tree))
    );
}

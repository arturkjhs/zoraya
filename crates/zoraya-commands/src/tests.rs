use super::*;

use alloc::collections::BTreeMap;
use alloc::string::ToString;

/// A volume in memory: paths relative to the root, directories ending in
/// `/` as keys.
#[derive(Default)]
struct Mem {
    files: BTreeMap<String, Vec<u8>>,
    dirs: Vec<String>,
    synced: bool,
}

impl Mem {
    fn new(files: &[(&str, &str)], dirs: &[&str]) -> Mem {
        let mut m = Mem::default();
        m.dirs.push(String::new());
        for d in dirs {
            m.dirs.push((*d).into());
        }
        for (p, t) in files {
            m.files.insert((*p).into(), t.as_bytes().to_vec());
        }
        m
    }

    fn norm(path: &str) -> String {
        let p = path.trim_end_matches('/');
        if p == "." {
            String::new()
        } else {
            p.trim_start_matches("./").to_string()
        }
    }

    fn parent_exists(&self, p: &str) -> bool {
        match p.rsplit_once('/') {
            Some((dir, _)) => self.dirs.iter().any(|d| d == dir),
            None => true,
        }
    }
}

impl Files for Mem {
    fn list(&mut self, dir: &str) -> Result<Vec<(String, Kind)>, Status> {
        let dir = Mem::norm(dir);
        if !self.dirs.contains(&dir) {
            return Err(if self.files.contains_key(&dir) {
                status::NOT_DIRECTORY
            } else {
                status::NOT_FOUND
            });
        }
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut out = Vec::new();
        for f in self.files.keys() {
            if let Some(rest) = f.strip_prefix(&prefix)
                && !rest.contains('/')
            {
                out.push((rest.to_string(), Kind::File));
            }
        }
        for d in &self.dirs {
            if let Some(rest) = d.strip_prefix(&prefix)
                && !rest.is_empty()
                && !rest.contains('/')
            {
                out.push((rest.to_string(), Kind::Directory));
            }
        }
        out.reverse(); // any order
        Ok(out)
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, Status> {
        let p = Mem::norm(path);
        if self.dirs.contains(&p) {
            return Err(status::IS_DIRECTORY);
        }
        self.files.get(&p).cloned().ok_or(status::NOT_FOUND)
    }

    fn create(&mut self, path: &str, bytes: &[u8], replace: bool) -> Result<(), Status> {
        let p = Mem::norm(path);
        if !self.parent_exists(&p) {
            return Err(status::NOT_FOUND);
        }
        if self.dirs.contains(&p) {
            return Err(status::IS_DIRECTORY);
        }
        if self.files.contains_key(&p) && !replace {
            return Err(status::EXISTS);
        }
        self.files.insert(p, bytes.to_vec());
        Ok(())
    }

    fn make_directory(&mut self, path: &str) -> Result<(), Status> {
        let p = Mem::norm(path);
        if self.dirs.contains(&p) || self.files.contains_key(&p) {
            return Err(status::EXISTS);
        }
        if !self.parent_exists(&p) {
            return Err(status::NOT_FOUND);
        }
        self.dirs.push(p);
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), Status> {
        let p = Mem::norm(path);
        if self.files.remove(&p).is_some() {
            return Ok(());
        }
        if self.dirs.contains(&p) {
            let prefix = format!("{p}/");
            let busy = self.files.keys().any(|f| f.starts_with(&prefix))
                || self.dirs.iter().any(|d| d.starts_with(&prefix));
            if busy {
                return Err(status::NOT_EMPTY);
            }
            self.dirs.retain(|d| *d != p);
            return Ok(());
        }
        Err(status::NOT_FOUND)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), Status> {
        let (f, t) = (Mem::norm(from), Mem::norm(to));
        let bytes = self.files.remove(&f).ok_or(status::NOT_FOUND)?;
        self.files.insert(t, bytes);
        Ok(())
    }

    fn stat(&mut self, path: &str) -> Result<Stat, Status> {
        let p = Mem::norm(path);
        let (kind, size) = if self.dirs.contains(&p) {
            (Kind::Directory, 0)
        } else {
            (
                Kind::File,
                self.files.get(&p).ok_or(status::NOT_FOUND)?.len() as u64,
            )
        };
        Ok(Stat {
            kind,
            size,
            modified: 3_000_000_000,
            changed: 4_000_000_000,
            mode: 0o644,
        })
    }

    fn sync(&mut self) -> Result<(), Status> {
        self.synced = true;
        Ok(())
    }
}

fn description(name: &str) -> Description {
    let text = match name {
        "ls" => include_str!("../../../user/describe/commands/ls.cmd"),
        "cat" => include_str!("../../../user/describe/commands/cat.cmd"),
        "cp" => include_str!("../../../user/describe/commands/cp.cmd"),
        "mv" => include_str!("../../../user/describe/commands/mv.cmd"),
        "rm" => include_str!("../../../user/describe/commands/rm.cmd"),
        "mkdir" => include_str!("../../../user/describe/commands/mkdir.cmd"),
        "stat" => include_str!("../../../user/describe/commands/stat.cmd"),
        "sync" => include_str!("../../../user/describe/commands/sync.cmd"),
        "head" => include_str!("../../../user/describe/commands/head.cmd"),
        "find" => include_str!("../../../user/describe/commands/find.cmd"),
        "search" => include_str!("../../../user/describe/commands/search.cmd"),
        _ => panic!("{name}"),
    };
    zoraya_describe::parse(text.as_bytes()).unwrap()
}

/// Runs `line` (a command and its arguments) and gives code, out, err.
fn go(m: &mut Mem, line: &[&str]) -> (u8, String, String) {
    let d = description(line[0]);
    let p = match zoraya_command::parse(&d, &line[1..]) {
        Ok(zoraya_command::Outcome::Run(p)) => p,
        other => panic!("{line:?}: {other:?}"),
    };
    let (code, o) = run(line[0], &d, &p, m);
    (code, o.out, o.err)
}

fn tour() -> Mem {
    Mem::new(
        &[
            ("notes.txt", "buy milk\ncall Ada\n"),
            ("project/meeting notes.txt", "agenda\n"),
            ("project/minutes.txt", "agenda\n"),
            (".hidden.txt", "x\n"),
        ],
        &["project"],
    )
}

#[test]
fn listings_are_sorted_with_directories_marked() {
    let mut m = tour();
    assert_eq!(go(&mut m, &["ls"]).1, ".hidden.txt\nnotes.txt\nproject/\n");
    assert_eq!(
        go(&mut m, &["ls", "project"]).1,
        "meeting notes.txt\nminutes.txt\n"
    );
    assert_eq!(
        go(&mut m, &["ls", "nowhere"]),
        (1, "".into(), "ls: nowhere: not found\n".into())
    );
}

#[test]
fn the_tours_find_and_search() {
    let mut m = tour();
    // Tour example 9.
    assert_eq!(
        go(&mut m, &["find", "*.txt"]).1,
        "notes.txt\nproject/meeting notes.txt\nproject/minutes.txt\n"
    );
    assert_eq!(go(&mut m, &["find", ".*"]).1, ".hidden.txt\n");
    assert_eq!(go(&mut m, &["find", "proj*"]).1, "project/\n");
    assert_eq!(go(&mut m, &["find", "a/b"]).0, 2);
    // Tour example 10: found, and a "no" that is not a failure.
    assert_eq!(
        go(&mut m, &["search", "milk", "notes.txt"]),
        (0, "notes.txt:1: buy milk\n".into(), "".into())
    );
    assert_eq!(
        go(&mut m, &["search", "cheese", "notes.txt"]),
        (1, "".into(), "".into())
    );
    assert_eq!(
        go(&mut m, &["search", "agenda", "project"]).1,
        "project/meeting notes.txt:1: agenda\nproject/minutes.txt:1: agenda\n"
    );
    // A question's failure is 3 (ADR-0155 rule 6).
    assert_eq!(go(&mut m, &["search", "x", "nowhere"]).0, 3);
    assert_eq!(go(&mut m, &["search", "x"]).0, 3);
}

#[test]
fn copy_and_its_hint() {
    let mut m = tour();
    assert_eq!(
        go(&mut m, &["cp", "notes.txt", "project/notes.txt"]),
        (0, "".into(), "".into())
    );
    assert_eq!(m.files["project/notes.txt"], b"buy milk\ncall Ada\n");
    // Tour example 26 and ADR-0155's example.
    assert_eq!(
        go(&mut m, &["cp", "notes.txt", "project/notes.txt"]),
        (
            1,
            "".into(),
            "cp: project/notes.txt: already exists\n  hint: cp --force notes.txt project/notes.txt replaces it\n".into()
        )
    );
    assert_eq!(
        go(&mut m, &["cp", "--force", "notes.txt", "project/notes.txt"]).0,
        0
    );
    assert_eq!(
        go(&mut m, &["cp", "missing", "x"]).2,
        "cp: missing: not found\n"
    );
}

#[test]
fn remove_and_its_hint() {
    let mut m = tour();
    assert_eq!(
        go(&mut m, &["rm", "project"]).2,
        "rm: project: not empty\n  hint: rm --recursive project removes it and everything in it\n"
    );
    assert_eq!(go(&mut m, &["rm", "--recursive", "project"]).0, 0);
    assert!(!m.dirs.contains(&"project".to_string()));
    assert_eq!(m.files.len(), 2);
    assert_eq!(
        go(&mut m, &["rm", "notes.txt", "nowhere"]).0,
        1,
        "one failed, the other went"
    );
    assert!(!m.files.contains_key("notes.txt"));
}

#[test]
fn the_rest() {
    let mut m = tour();
    assert_eq!(go(&mut m, &["cat", "notes.txt"]).1, "buy milk\ncall Ada\n");
    assert_eq!(
        go(&mut m, &["cat", "project"]).2,
        "cat: project: is a directory\n"
    );
    assert_eq!(
        go(&mut m, &["head", "--lines", "1", "notes.txt"]).1,
        "buy milk\n"
    );
    assert_eq!(
        go(
            &mut m,
            &["head", "-n", "1", "notes.txt", "project/minutes.txt"]
        )
        .1,
        "notes.txt:\nbuy milk\n\nproject/minutes.txt:\nagenda\n"
    );
    assert_eq!(go(&mut m, &["head", "--lines", "x", "notes.txt"]).0, 2);
    assert_eq!(go(&mut m, &["mkdir", "build"]).0, 0);
    assert_eq!(
        go(&mut m, &["mkdir", "build"]).2,
        "mkdir: build: already exists\n"
    );
    assert_eq!(
        go(&mut m, &["mkdir", "no/such"]).2,
        "mkdir: no/such: not found\n"
    );
    assert_eq!(go(&mut m, &["mv", "notes.txt", "build/notes.txt"]).0, 0);
    assert_eq!(
        go(&mut m, &["stat", "build/notes.txt"]).1,
        "file size 18 mtime 3 ctime 4 mode 644\n"
    );
    assert_eq!(go(&mut m, &["sync"]).0, 0);
    assert!(m.synced);
}

/// INV-0155-3 over this crate's errors: every line is NAME: SUBJECT: PROBLEM
/// with no status name.
#[test]
fn every_error_line_is_well_formed() {
    let mut m = tour();
    let cases: [&[&str]; 9] = [
        &["ls", "x"],
        &["cat", "x"],
        &["cp", "x", "y"],
        &["mv", "x", "y"],
        &["rm", "x"],
        &["mkdir", "a/b"],
        &["stat", "x"],
        &["head", "x"],
        &["search", "t", "x"],
    ];
    for line in cases {
        let (_, _, err) = go(&mut m, line);
        let first = err.lines().next().unwrap_or_else(|| panic!("{line:?}"));
        let parts: Vec<&str> = first.split(": ").collect();
        assert_eq!(parts.len(), 3, "{first}");
        assert_eq!(parts[0], line[0]);
        assert!(!parts[2].contains("NotFound") && !parts[2].ends_with('.'));
    }
}

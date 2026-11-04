//! The file commands (ADR-0155 rules 1, 4, 5, 8): `ls`, `cat`, `cp`, `mv`,
//! `rm`, `mkdir`, `stat`, `sync`, `head`, `find` and `search`, over a
//! [`Files`] trait, so that one implementation serves the shell now (its
//! builtins, with the shell's own access) and the programs later (TASK-18
//! step 7, each over what its start-up record designates). Arguments come
//! parsed by `zoraya-command` from each command's description; output goes
//! to strings the caller writes.
//!
//! A successful command prints only its result: `cp`, `mv`, `rm`, `mkdir`
//! and `sync` print nothing. A listing is one item per line, directories
//! ending in `/`, sorted byte by byte (ADR-0013 rule 3). An error is one
//! line `NAME: SUBJECT: PROBLEM`, with a hint when one is known.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_command::{Parsed, error, failure_code, file_problem};
use zoraya_describe::Description;
use zoraya_proto_file::status;

/// What a path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub kind: Kind,
    pub size: u64,
    /// Nanoseconds.
    pub modified: u64,
    pub changed: u64,
    pub mode: u32,
}

/// A file-protocol status (`zoraya_proto_file::status`).
pub type Status = u32;

/// The file system as a command sees it. Paths are as the person wrote
/// them; the implementation resolves them.
pub trait Files {
    /// The names in a directory, with their kinds, in any order.
    fn list(&mut self, dir: &str) -> Result<Vec<(String, Kind)>, Status>;
    fn read(&mut self, path: &str) -> Result<Vec<u8>, Status>;
    /// Creates `path` with `bytes`; with `replace`, an existing file is
    /// replaced, else it is `EXISTS`.
    fn create(&mut self, path: &str, bytes: &[u8], replace: bool) -> Result<(), Status>;
    fn make_directory(&mut self, path: &str) -> Result<(), Status>;
    /// Removes a file or an empty directory.
    fn remove(&mut self, path: &str) -> Result<(), Status>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), Status>;
    fn stat(&mut self, path: &str) -> Result<Stat, Status>;
    fn sync(&mut self) -> Result<(), Status>;
}

/// What a command wrote.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub out: String,
    pub err: String,
}

/// The commands this crate runs.
pub const NAMES: [&str; 11] = [
    "ls", "cat", "cp", "mv", "rm", "mkdir", "stat", "sync", "head", "find", "search",
];

/// Most entries `find` and a recursive `search` or `rm` visit: past it they
/// stop with `LIMIT_REACHED` rather than run without bound.
pub const MAX_VISITS: usize = 100_000;
/// Deepest directory they descend.
pub const MAX_DEPTH: usize = 64;

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "." {
        String::from(name)
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

struct Run<'a, F: Files> {
    name: &'a str,
    files: &'a mut F,
    o: Output,
    failed: bool,
}

impl<F: Files> Run<'_, F> {
    fn fail(&mut self, subject: &str, s: Status, hints: &[&str]) {
        self.o
            .err
            .push_str(&error(self.name, Some(subject), &file_problem(s), hints));
        self.failed = true;
    }

    fn sorted(&mut self, dir: &str) -> Result<Vec<(String, Kind)>, Status> {
        let mut list = self.files.list(dir)?;
        list.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        Ok(list)
    }

    /// Every path below `dir` (not `dir` itself), depth first in sorted
    /// order, with its kind; paths relative to `dir` as written.
    fn walk(
        &mut self,
        dir: &str,
        out: &mut Vec<(String, Kind)>,
        depth: usize,
    ) -> Result<(), Status> {
        if depth > MAX_DEPTH {
            return Err(status::LIMIT_REACHED);
        }
        for (name, kind) in self.sorted(dir)? {
            if out.len() >= MAX_VISITS {
                return Err(status::LIMIT_REACHED);
            }
            let path = join(dir, &name);
            out.push((path.clone(), kind));
            if kind == Kind::Directory {
                self.walk(&path, out, depth + 1)?;
            }
        }
        Ok(())
    }

    fn remove_tree(&mut self, path: &str, depth: usize) -> Result<(), Status> {
        if depth > MAX_DEPTH {
            return Err(status::LIMIT_REACHED);
        }
        if self.files.stat(path)?.kind == Kind::Directory {
            for (name, _) in self.sorted(path)? {
                self.remove_tree(&join(path, &name), depth + 1)?;
            }
        }
        self.files.remove(path)
    }
}

/// Runs command `name` with its parsed arguments; gives its exit code and
/// what it wrote.
pub fn run<F: Files>(name: &str, d: &Description, p: &Parsed<'_>, files: &mut F) -> (u8, Output) {
    let mut r = Run {
        name,
        files,
        o: Output::default(),
        failed: false,
    };
    let mut answer_no = false;
    match name {
        "ls" => {
            let dir = p.operand(d, "DIR").unwrap_or(".");
            match r.sorted(dir) {
                Ok(list) => {
                    for (n, kind) in list {
                        r.o.out.push_str(&n);
                        r.o.out
                            .push_str(if kind == Kind::Directory { "/\n" } else { "\n" });
                    }
                }
                Err(s) => r.fail(dir, s, &[]),
            }
        }
        "cat" => {
            for file in p.operand_values(d, "FILE") {
                match r.files.read(file) {
                    Ok(bytes) => match core::str::from_utf8(&bytes) {
                        Ok(text) => {
                            r.o.out.push_str(text);
                            if !text.is_empty() && !text.ends_with('\n') {
                                r.o.out.push('\n');
                            }
                        }
                        Err(_) => {
                            r.o.out
                                .push_str(&format!("{} bytes of binary\n", bytes.len()))
                        }
                    },
                    Err(s) => r.fail(file, s, &[]),
                }
            }
        }
        "cp" => {
            let (from, to) = (
                p.operand(d, "FROM").unwrap_or(""),
                p.operand(d, "TO").unwrap_or(""),
            );
            let force = p.flag(d, "force");
            match r.files.read(from) {
                Ok(bytes) => {
                    if let Err(s) = r.files.create(to, &bytes, force) {
                        let hint = format!("cp --force {from} {to} replaces it");
                        let hints: &[&str] = if s == status::EXISTS { &[&hint] } else { &[] };
                        r.fail(to, s, hints);
                    }
                }
                Err(s) => r.fail(from, s, &[]),
            }
        }
        "mv" => {
            let (from, to) = (
                p.operand(d, "FROM").unwrap_or(""),
                p.operand(d, "TO").unwrap_or(""),
            );
            if let Err(s) = r.files.rename(from, to) {
                r.fail(from, s, &[]);
            }
        }
        "rm" => {
            let recursive = p.flag(d, "recursive");
            for path in p.operand_values(d, "PATH") {
                let result = if recursive {
                    r.remove_tree(path, 0)
                } else {
                    r.files.remove(path)
                };
                if let Err(s) = result {
                    let hint = format!("rm --recursive {path} removes it and everything in it");
                    let hints: &[&str] = if s == status::NOT_EMPTY {
                        &[&hint]
                    } else {
                        &[]
                    };
                    r.fail(path, s, hints);
                }
            }
        }
        "mkdir" => {
            for dir in p.operand_values(d, "DIR") {
                if let Err(s) = r.files.make_directory(dir) {
                    r.fail(dir, s, &[]);
                }
            }
        }
        "stat" => {
            for path in p.operand_values(d, "PATH") {
                match r.files.stat(path) {
                    Ok(s) => {
                        let kind = match s.kind {
                            Kind::Directory => "directory",
                            Kind::Link => "link",
                            Kind::File => "file",
                        };
                        r.o.out.push_str(&format!(
                            "{kind} size {} mtime {} ctime {} mode {:o}\n",
                            s.size,
                            s.modified / 1_000_000_000,
                            s.changed / 1_000_000_000,
                            s.mode
                        ));
                    }
                    Err(s) => r.fail(path, s, &[]),
                }
            }
        }
        "sync" => {
            if let Err(s) = r.files.sync() {
                r.o.err
                    .push_str(&error("sync", None, &file_problem(s), &[]));
                r.failed = true;
            }
        }
        "head" => {
            let lines = match p.value(d, "lines") {
                None => 10,
                Some(v) => match zoraya_describe::parse_number(v) {
                    Some(n) if n >= 0 => n as usize,
                    _ => {
                        let e = zoraya_command::UsageError(format!("--lines: {v} is not a count"));
                        r.o.err.push_str(&zoraya_command::usage_error(d, &e));
                        return (zoraya_command::exit::USAGE, r.o);
                    }
                },
            };
            let files = p.operand_values(d, "FILE");
            let several = files.len() > 1;
            for (i, file) in files.iter().enumerate() {
                match r.files.read(file) {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        if several {
                            if i > 0 {
                                r.o.out.push('\n');
                            }
                            r.o.out.push_str(&format!("{file}:\n"));
                        }
                        for line in text.lines().take(lines) {
                            r.o.out.push_str(line);
                            r.o.out.push('\n');
                        }
                    }
                    Err(s) => r.fail(file, s, &[]),
                }
            }
        }
        "find" => {
            let pattern = p.operand(d, "PATTERN").unwrap_or("");
            let dir = p.operand(d, "DIR").unwrap_or(".");
            let chars: Vec<(char, bool)> = pattern.chars().map(|c| (c, true)).collect();
            let tokens = match zoraya_shell::glob::pattern(&chars) {
                Some(p) => match p.components.as_slice() {
                    [zoraya_shell::glob::Component::Tokens(t)] => t.clone(),
                    _ => Vec::new(),
                },
                None => chars
                    .iter()
                    .map(|(c, _)| zoraya_shell::glob::Token::Char(*c))
                    .collect(),
            };
            if tokens.is_empty() || pattern.contains('/') {
                let e = zoraya_command::UsageError(format!("{pattern}: a pattern is one name"));
                r.o.err.push_str(&zoraya_command::usage_error(d, &e));
                return (zoraya_command::exit::USAGE, r.o);
            }
            let mut found = Vec::new();
            match r.walk(dir, &mut found, 0) {
                Ok(()) => {
                    let mut paths: Vec<String> = found
                        .into_iter()
                        .filter(|(path, _)| {
                            let last = path.rsplit('/').next().unwrap_or(path);
                            zoraya_shell::glob::matches(&tokens, last)
                        })
                        .map(|(path, kind)| {
                            if kind == Kind::Directory {
                                format!("{path}/")
                            } else {
                                path
                            }
                        })
                        .collect();
                    paths.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
                    for path in paths {
                        r.o.out.push_str(&path);
                        r.o.out.push('\n');
                    }
                }
                Err(s) => r.fail(dir, s, &[]),
            }
        }
        "search" => {
            let text = p.operand(d, "TEXT").unwrap_or("");
            let paths = p.operand_values(d, "PATH");
            if paths.is_empty() {
                r.o.err.push_str(&error(
                    "search",
                    None,
                    "reading input needs byte streams, which zosh does not use yet",
                    &["name the files to search: search TEXT FILE..."],
                ));
                return (failure_code(d), r.o);
            }
            let mut any = false;
            for path in paths {
                let mut files = Vec::new();
                match r.files.stat(path) {
                    Ok(s) if s.kind == Kind::Directory => {
                        let mut below = Vec::new();
                        match r.walk(path, &mut below, 0) {
                            Ok(()) => files.extend(
                                below
                                    .into_iter()
                                    .filter(|(_, k)| *k == Kind::File)
                                    .map(|(p, _)| p),
                            ),
                            Err(s) => {
                                r.fail(path, s, &[]);
                                continue;
                            }
                        }
                    }
                    Ok(_) => files.push(String::from(path)),
                    Err(s) => {
                        r.fail(path, s, &[]);
                        continue;
                    }
                }
                for file in files {
                    match r.files.read(&file) {
                        Ok(bytes) => {
                            let content = String::from_utf8_lossy(&bytes);
                            for (n, line) in content.lines().enumerate() {
                                if line.contains(text) {
                                    any = true;
                                    r.o.out.push_str(&format!("{file}:{}: {line}\n", n + 1));
                                }
                            }
                        }
                        Err(s) => r.fail(&file, s, &[]),
                    }
                }
            }
            answer_no = !any;
        }
        _ => {
            r.o.err
                .push_str(&error(name, None, "not a file command", &[]));
            return (zoraya_command::exit::USAGE, r.o);
        }
    }
    let code = if r.failed {
        failure_code(d)
    } else if answer_no {
        zoraya_command::exit::NO
    } else {
        zoraya_command::exit::SUCCESS
    };
    (code, r.o)
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

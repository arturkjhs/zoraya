use super::*;

use alloc::collections::BTreeMap;
use alloc::string::ToString;

/// A volume in memory: paths relative to the root, directories ending in
/// `/` as keys.
#[derive(Default)]
struct Mem {
    files: BTreeMap<String, Vec<u8>>,
    dirs: Vec<String>,
    /// Link objects: path to the path they lead to; followed by everything
    /// but a listing and `remove`, as the file service does.
    links: BTreeMap<String, String>,
    synced: bool,
    /// The input, and how much of it each read gives.
    input: Vec<u8>,
    chunk: usize,
    /// Bytes of input read.
    taken: usize,
    /// Output taken by `print` and `print_bytes`, when `printing`.
    printing: bool,
    printed: String,
    printed_bytes: Vec<u8>,
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

    /// `p` with any link at its start followed.
    fn follow(&self, p: &str) -> String {
        for (link, target) in &self.links {
            if p == link {
                return target.clone();
            }
            if let Some(rest) = p.strip_prefix(&format!("{link}/")) {
                return format!("{target}/{rest}");
            }
        }
        p.to_string()
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
        let dir = self.follow(&Mem::norm(dir));
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
        for l in self.links.keys() {
            if let Some(rest) = l.strip_prefix(&prefix)
                && !rest.is_empty()
                && !rest.contains('/')
            {
                out.push((rest.to_string(), Kind::Link));
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
        if self.links.remove(&p).is_some() || self.files.remove(&p).is_some() {
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
        let p = self.follow(&Mem::norm(path));
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

    fn read_input(&mut self, buffer: &mut [u8]) -> Result<usize, Status> {
        let n = (self.input.len() - self.taken)
            .min(buffer.len())
            .min(self.chunk.max(1));
        buffer[..n].copy_from_slice(&self.input[self.taken..self.taken + n]);
        self.taken += n;
        Ok(n)
    }

    fn print(&mut self, text: &str) -> bool {
        if self.printing {
            self.printed.push_str(text);
        }
        self.printing
    }

    fn print_bytes(&mut self, bytes: &[u8]) -> bool {
        if self.printing {
            self.printed_bytes.extend_from_slice(bytes);
        }
        self.printing
    }
}

fn with_input(text: &[u8], chunk: usize) -> Mem {
    let mut m = Mem::new(&[("a.txt", "one\n")], &[]);
    m.input = text.to_vec();
    m.chunk = chunk;
    m
}

/// ADR-0152 rule 1: with no file, `cat`, `head` and `search` read their
/// input; a UTF-8 sequence cut between reads stays whole.
#[test]
fn commands_read_their_input_when_given_no_file() {
    let text = "Київ milk\nPlzeň\nmilk again\nlast, no newline";
    for chunk in [1, 2, 3, 7, 4096] {
        let mut m = with_input(text.as_bytes(), chunk);
        assert_eq!(
            go(&mut m, &["cat"]),
            (0, text.into(), String::new()),
            "{chunk}"
        );
        let mut m = with_input(text.as_bytes(), chunk);
        let (code, out, _) = go(&mut m, &["search", "milk"]);
        assert_eq!(
            (code, out.as_str()),
            (0, "Київ milk\nmilk again\n"),
            "{chunk}"
        );
        let mut m = with_input(text.as_bytes(), chunk);
        assert_eq!(go(&mut m, &["search", "nope"]).0, 1, "no is an answer");
        let mut m = with_input(text.as_bytes(), chunk);
        let (_, out, _) = go(&mut m, &["head", "--lines", "2"]);
        assert_eq!(out, "Київ milk\nPlzeň\n", "{chunk}");
        let mut m = with_input(text.as_bytes(), chunk);
        let (_, out, _) = go(&mut m, &["head", "--lines", "9"]);
        assert_eq!(out, "Київ milk\nPlzeň\nmilk again\nlast, no newline\n");
    }
    // A sequence cut between reads stays whole after a byte that is not
    // UTF-8 in the same read, and a lone lead byte at the end is replaced:
    // in the lines `search` reads.
    let bytes = [b"\xff".as_slice(), "€€".as_bytes(), b"\xe2"].concat();
    for chunk in [2, 3, 4, 5] {
        let mut m = with_input(&bytes, chunk);
        assert_eq!(
            go(&mut m, &["search", "€"]).1,
            "\u{fffd}€€\u{fffd}\n",
            "{chunk}"
        );
    }
    // A file operand is read instead of the input.
    let mut m = with_input(b"input\n", 4096);
    assert_eq!(go(&mut m, &["cat", "a.txt"]).1, "one\n");
    assert_eq!(m.taken, 0);
}

/// `head` stops reading once it has its lines, so that a writer before it
/// finds its reader gone (INV-0155-4's host half).
#[test]
fn head_reads_no_more_than_it_needs() {
    let mut big = Vec::new();
    for i in 0..10_000 {
        big.extend_from_slice(format!("line {i}\n").as_bytes());
    }
    let mut m = with_input(&big, 512);
    let (_, out, _) = go(&mut m, &["head", "--lines", "1"]);
    assert_eq!(out, "line 0\n");
    assert!(m.taken <= 512, "read {} bytes", m.taken);
}

/// Long output goes out as it is made when the caller takes it.
#[test]
fn long_output_is_printed_as_it_comes() {
    let big = "x\n".repeat(3 * PRINT_AT);
    let mut m = with_input(big.as_bytes(), 1000);
    m.printing = true;
    let (code, out, _) = go(&mut m, &["search", "x"]);
    assert_eq!(code, 0);
    assert!(m.printed.len() >= PRINT_AT, "printed {}", m.printed.len());
    assert_eq!(m.printed + &out, big);
}

/// R9 (ADR-0155's note): `cat` passes bytes unchanged, from its input or a
/// file: nothing replaced, no line end added, nothing summarised; a caller
/// that takes bytes gets them as they come.
#[test]
fn cat_passes_bytes_unchanged() {
    let odd = b"bad \xff byte \xe2\x82 and no line end".to_vec();
    for chunk in [1, 3, 4096] {
        let mut m = with_input(&odd, chunk);
        assert_eq!(go_raw(&mut m, &["cat"]), (0, odd.clone()), "{chunk}");
    }
    let mut m = Mem::new(&[], &[]);
    m.files.insert("odd.bin".into(), odd.clone());
    m.files.insert("plain".into(), b"no end".to_vec());
    assert_eq!(
        go_raw(&mut m, &["cat", "odd.bin", "plain"]),
        (0, [odd.clone(), b"no end".to_vec()].concat())
    );
    let mut m = with_input(&odd, 5);
    m.printing = true;
    assert_eq!(go_raw(&mut m, &["cat"]), (0, Vec::new()));
    assert_eq!(m.printed_bytes, odd, "taken as they came");
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
    let mut out = o.out;
    out.push_str(&String::from_utf8_lossy(&o.raw));
    (code, out, o.err)
}

/// [`go`], giving the bytes of out (text, then raw) and the code.
fn go_raw(m: &mut Mem, line: &[&str]) -> (u8, Vec<u8>) {
    let d = description(line[0]);
    let p = match zoraya_command::parse(&d, &line[1..]) {
        Ok(zoraya_command::Outcome::Run(p)) => p,
        other => panic!("{line:?}: {other:?}"),
    };
    let (code, o) = run(line[0], &d, &p, m);
    let mut out = o.out.into_bytes();
    out.extend_from_slice(&o.raw);
    (code, out)
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
    // A file is named as it was given (the tour's `ls *.bak`).
    assert_eq!(
        go(&mut m, &["ls", "notes.txt"]),
        (0, "notes.txt\n".into(), String::new())
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
    // No PATH: the input, empty here, so the answer is no.
    assert_eq!(go(&mut m, &["search", "x"]).0, 1);
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

/// `Files` over a `Mem`, forwarding what the trait requires, and then
/// whatever else is given.
macro_rules! over_mem {
    ($name:ident { $($more:item)* }) => {
        struct $name<'a>(&'a mut Mem);

        impl Files for $name<'_> {
            fn list(&mut self, dir: &str) -> Result<Vec<(String, Kind)>, Status> {
                self.0.list(dir)
            }
            fn read(&mut self, path: &str) -> Result<Vec<u8>, Status> {
                self.0.read(path)
            }
            fn create(&mut self, path: &str, bytes: &[u8], replace: bool) -> Result<(), Status> {
                self.0.create(path, bytes, replace)
            }
            fn make_directory(&mut self, path: &str) -> Result<(), Status> {
                self.0.make_directory(path)
            }
            fn remove(&mut self, path: &str) -> Result<(), Status> {
                self.0.remove(path)
            }
            fn rename(&mut self, from: &str, to: &str) -> Result<(), Status> {
                self.0.rename(from, to)
            }
            fn stat(&mut self, path: &str) -> Result<Stat, Status> {
                self.0.stat(path)
            }
            fn sync(&mut self) -> Result<(), Status> {
                self.0.sync()
            }
            $($more)*
        }
    };
}

// Both defaults, as the shell's own `Files` has them.
over_mem!(Plain {});
// Input, and the default `print`.
over_mem!(Fed {
    fn read_input(&mut self, buffer: &mut [u8]) -> Result<usize, Status> {
        self.0.read_input(buffer)
    }
});

/// The defaults: no input at all, and every byte of output kept in
/// `Output`, however long (mutants of `read_input` and `print`).
#[test]
fn a_caller_with_the_defaults_has_no_input_and_gets_all_output() {
    let mut m = Mem::new(&[], &[]);
    assert_eq!(Input::new().next(&mut Plain(&mut m)), Ok(None));
    let big = "x".repeat(3 * PRINT_AT) + "\n";
    let mut m = with_input(big.as_bytes(), 1000);
    let d = description("cat");
    let p = zoraya_command::Parsed::default();
    let (code, o) = run("cat", &d, &p, &mut Fed(&mut m));
    assert_eq!(
        (code, o.raw),
        (0, big.into_bytes()),
        "bytes are held, not lost"
    );
    let text = "x\n".repeat(3 * PRINT_AT);
    let mut m = with_input(text.as_bytes(), 1000);
    let d = description("search");
    let p = match zoraya_command::parse(&d, &["x"]) {
        Ok(zoraya_command::Outcome::Run(p)) => p,
        other => panic!("{other:?}"),
    };
    let (code, o) = run("search", &d, &p, &mut Fed(&mut m));
    assert_eq!((code, o.out), (0, text), "text is held, not lost");
}

/// `Input` on its own: what one read gives, then the end, once.
#[test]
fn input_gives_each_read_and_then_its_end() {
    let mut m = with_input(b"ab", 4096);
    let mut input = Input::new();
    assert_eq!(input.next(&mut m), Ok(Some("ab".into())));
    assert_eq!(input.next(&mut m), Ok(None));
    assert_eq!(input.next(&mut m), Ok(None));
    let mut m = with_input(b"", 4096);
    assert_eq!(Input::new().next(&mut m), Ok(None));
    // A four-byte character cut anywhere stays whole.
    for chunk in [1, 2, 3] {
        let mut m = with_input("😀\n".as_bytes(), chunk);
        assert_eq!(go(&mut m, &["head", "--lines", "1"]).1, "😀\n", "{chunk}");
    }
}

/// Output is offered to `print` only once it reaches `PRINT_AT`, and a
/// caller that does not take it keeps it all; a character complete at the
/// end of a read goes out with that read.
#[test]
fn output_is_offered_only_past_print_at() {
    let mut m = with_input(b"small\n", 4096);
    m.printing = true;
    assert_eq!(go(&mut m, &["head", "--lines", "9"]).1, "small\n");
    assert_eq!(m.printed, "");
    let big = "x".repeat(3 * PRINT_AT) + "\n";
    let mut m = with_input(big.as_bytes(), 1000);
    assert_eq!(
        go(&mut m, &["head", "--lines", "9"]).1,
        big,
        "not printing: all kept"
    );
}

/// `head` reads its input only when no file is named, and `--lines 0`
/// reads none.
#[test]
fn head_reads_its_input_only_when_it_needs_it() {
    let mut m = with_input(b"input\n", 4096);
    assert_eq!(go(&mut m, &["head", "--lines", "1", "a.txt"]).1, "one\n");
    assert_eq!(m.taken, 0);
    let mut m = with_input(b"one\n", 4096);
    assert_eq!(go(&mut m, &["head", "--lines", "0"]).1, "");
    assert_eq!(m.taken, 0);
}

/// Review S4: `rm --recursive` removes a link object as itself and never
/// goes into it, so what the link leads to stays.
#[test]
fn removing_a_tree_never_follows_a_link() {
    let mut m = Mem::new(
        &[("tree/a.txt", "a"), ("kept/b.txt", "b")],
        &["tree", "kept"],
    );
    m.links.insert("tree/lnk".into(), "kept".into());
    m.links.insert("top-link".into(), "kept".into());
    assert_eq!(go(&mut m, &["rm", "--recursive", "tree"]).0, 0);
    assert!(!m.dirs.contains(&"tree".to_string()));
    assert!(
        m.files.contains_key("kept/b.txt"),
        "the link's target stays"
    );
    assert_eq!(go(&mut m, &["rm", "--recursive", "top-link"]).0, 0);
    assert!(!m.links.contains_key("top-link"));
    assert!(m.files.contains_key("kept/b.txt"), "the link alone goes");
}

/// A chain of `n` directories below `t`: `t/d/d/...`.
fn chain(n: usize) -> Mem {
    let mut dirs = vec![String::from("t")];
    for _ in 0..n {
        let next = format!("{}/d", dirs.last().expect("one at least"));
        dirs.push(next);
    }
    let names: Vec<&str> = dirs.iter().map(String::as_str).collect();
    Mem::new(&[], &names)
}

/// The depth bound of a walk and of a removal: `MAX_DEPTH` levels below
/// the named directory are walked, one more is refused (mutation testing).
#[test]
fn a_tree_is_walked_and_removed_to_its_depth_bound() {
    let mut m = chain(MAX_DEPTH);
    let (code, out, _) = go(&mut m, &["find", "d", "t"]);
    assert_eq!(
        (code, out.lines().count()),
        (0, MAX_DEPTH),
        "every level found"
    );
    assert_eq!(go(&mut m, &["rm", "--recursive", "t"]).0, 0);
    assert!(!m.dirs.contains(&"t".to_string()));
    let mut m = chain(MAX_DEPTH + 1);
    let (code, _, err) = go(&mut m, &["find", "d", "t"]);
    assert!(
        code != 0 && !err.is_empty(),
        "too deep is said: {code} {err:?}"
    );
    assert_ne!(go(&mut m, &["rm", "--recursive", "t"]).0, 0);
    assert!(
        m.dirs.contains(&"t".to_string()),
        "a refused removal keeps the top"
    );
}

/// `incomplete_tail` holds back only a sequence that is cut: a whole one
/// at the end is complete (mutation testing).
#[test]
fn a_whole_sequence_at_the_end_is_not_held() {
    for c in ["é", "€", "𝄞"] {
        let b = c.as_bytes();
        assert_eq!(incomplete_tail(b), 0, "{c}");
        assert_eq!(incomplete_tail(&b[..b.len() - 1]), b.len() - 1, "{c} cut");
    }
}

/// `head --lines` takes a count: a negative one is a usage error (mutation
/// testing).
#[test]
fn heads_count_is_not_negative() {
    let mut m = Mem::new(&[("a.txt", "1\n2\n")], &[]);
    let (code, out, err) = go(&mut m, &["head", "--lines", "-1", "a.txt"]);
    assert_eq!(code, zoraya_command::exit::USAGE, "{out:?} {err:?}");
    assert!(err.contains("is not a count"), "{err:?}");
}

/// Review S20: `rm` refuses a path that ends in `.` or `..`, or is the
/// root, with or without `--recursive`, and removes nothing.
#[test]
fn rm_refuses_dot_dot_dot_and_the_root() {
    for path in [".", "..", "/", "t/.", "t/..", "t/./", "./"] {
        for line in [vec!["rm", "--recursive", path], vec!["rm", path]] {
            let mut m = Mem::new(&[("t/a.txt", "a"), ("b.txt", "b")], &["t"]);
            let (code, _, err) = go(&mut m, &line);
            assert_eq!(code, 1, "{line:?}");
            assert!(
                err.starts_with(&format!(
                    "rm: {path}: a path that is /, or whose last part is . or .., is not removed\n"
                )) && err.contains("hint: name the directory itself"),
                "{line:?}: {err:?}"
            );
            assert!(
                m.files.contains_key("t/a.txt") && m.files.contains_key("b.txt"),
                "{line:?} removed nothing"
            );
        }
    }
    // A name that only starts with a dot is a name.
    let mut m = Mem::new(&[(".hidden", "h"), ("t/..x", "x")], &["t"]);
    assert_eq!(go(&mut m, &["rm", ".hidden", "t/..x"]).0, 0);
    assert!(m.files.is_empty());
}

/// Review S20: a trailing slash, as `ls` prints a directory and completion
/// writes one, is not refused: `rm --recursive project/` removes `project`,
/// and `rm empty/` an empty directory.
#[test]
fn rm_takes_a_directory_with_a_trailing_slash() {
    let mut m = Mem::new(
        &[
            ("project/a.txt", "a"),
            ("project/sub/b.txt", "b"),
            ("kept.txt", "k"),
        ],
        &["project", "project/sub", "empty"],
    );
    let (code, _, err) = go(&mut m, &["rm", "--recursive", "project/"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(!m.dirs.iter().any(|d| d.starts_with("project")));
    assert!(!m.files.keys().any(|f| f.starts_with("project")));
    let (code, _, err) = go(&mut m, &["rm", "empty//"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(!m.dirs.contains(&"empty".to_string()));
    assert!(m.files.contains_key("kept.txt"));
}

/// Review S20: an empty operand is not the root: it is not found (as for
/// POSIX), and it removes nothing, though joined to the working directory
/// it would name that.
#[test]
fn rm_of_an_empty_path_is_not_the_roots_refusal() {
    for line in [vec!["rm", "--recursive", ""], vec!["rm", ""]] {
        let mut m = Mem::new(&[("t/a.txt", "a"), ("b.txt", "b")], &["t"]);
        let (code, _, err) = go(&mut m, &line);
        assert_eq!(code, 1, "{line:?}: {err:?}");
        assert!(!err.contains("is not removed"), "{line:?}: {err:?}");
        assert!(err.starts_with("rm: : not found\n"), "{line:?}: {err:?}");
        assert!(m.files.contains_key("t/a.txt") && m.files.contains_key("b.txt"));
        assert!(m.dirs.contains(&"t".to_string()));
    }
}

//! zoraya-mutants: applies each mutation of `tools/mutants/*.toml` to the
//! committed tree, runs only the tests the list names for it, and writes the
//! mutants that survived to `build/mutants/report.md`.
//!
//! ```text
//! zoraya-mutants [--jobs N] [--only ID[,ID...]] [LIST.toml...]
//! ```
//!
//! Run from the repository root (`make mutants`). Each of N workers has a git
//! worktree of `HEAD` in `build/mutants/w<i>`, kept between runs so that its
//! `target/` builds incrementally: uncommitted changes are not mutated. Before
//! any mutant, every set of tests runs once unmutated, and a set that fails
//! stops the run, since every mutant would then look caught. A test command
//! runs under `sh -c` in its own process group, which is killed with all of
//! its QEMU children when the mutant's timeout passes.
//!
//! A mutant is *caught* when a test fails, *timed out* when one ran past the
//! timeout, *unviable* when it did not compile, and *survived* when every test
//! passed. A mutation whose `find` is not where the list says is an error in
//! the list, reported as such. Logs are in `build/mutants/logs/<id>.log`.
//! Exit status: 0 if nothing survived and every mutation applied, 1 if any
//! survived or did not apply, 2 on a usage or setup error.

mod list;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use list::Mutant;

const ROOT: &str = "build/mutants";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    Survived,
    NotApplied,
    Caught,
    TimedOut,
    Unviable,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Survived => "survived",
            Outcome::NotApplied => "not applied",
            Outcome::Caught => "caught",
            Outcome::TimedOut => "timed out",
            Outcome::Unviable => "unviable",
        }
    }
}

struct Tried {
    mutant: Mutant,
    list: String,
    outcome: Outcome,
    /// For a survivor: the line of the change. For a list error: why.
    detail: String,
    seconds: u64,
}

/// One test command: whether it passed, timed out, and what it printed.
struct Run {
    passed: bool,
    timed_out: bool,
    output: String,
}

fn run(dir: &Path, command: &str, timeout: Duration, jobs: usize) -> Run {
    let log = dir.join(".mutant-output");
    let file = match fs::File::create(&log) {
        Ok(file) => file,
        Err(e) => {
            return Run {
                passed: false,
                timed_out: false,
                output: format!("cannot create {}: {e}", log.display()),
            };
        }
    };
    let err = file.try_clone();
    let mut child = match err.and_then(|err| {
        Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(dir)
            .env("CARGO_BUILD_JOBS", jobs.to_string())
            .stdin(Stdio::null())
            .stdout(file)
            .stderr(err)
            .process_group(0)
            .spawn()
    }) {
        Ok(child) => child,
        Err(e) => {
            return Run {
                passed: false,
                timed_out: false,
                output: format!("cannot start sh: {e}"),
            };
        }
    };
    let start = Instant::now();
    let (passed, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.success(), false),
            Ok(None) if start.elapsed() >= timeout => {
                // The whole group: make, cargo and every QEMU under them.
                let _ = Command::new("kill")
                    .arg("-KILL")
                    .arg(format!("-{}", child.id()))
                    .status();
                let _ = child.wait();
                break (false, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => break (false, false),
        }
    };
    let output = fs::read_to_string(&log).unwrap_or_default();
    Run {
        passed,
        timed_out,
        output,
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A worktree of `commit` at `build/mutants/w<i>`, made or brought back to it.
fn worktree(i: usize, commit: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(format!("{ROOT}/w{i}"));
    if dir.join(".git").exists() {
        git(&dir, &["checkout", "-q", "-f", "--detach", commit])?;
    } else {
        git(Path::new("."), &["worktree", "prune"])?;
        let path = dir.to_string_lossy().into_owned();
        git(
            Path::new("."),
            &["worktree", "add", "-q", "-f", "--detach", &path, commit],
        )?;
    }
    Ok(dir)
}

/// The work every worker shares: the queue, and what came of each.
struct Shared {
    queue: Vec<(String, Mutant)>,
    results: Vec<Tried>,
}

fn mutate(dir: &Path, list: &str, m: &Mutant, jobs: usize) -> Tried {
    let start = Instant::now();
    let done = |outcome, detail: String| Tried {
        mutant: m.clone(),
        list: list.to_string(),
        outcome,
        detail,
        seconds: start.elapsed().as_secs(),
    };
    let path = dir.join(&m.file);
    let original = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => return done(Outcome::NotApplied, format!("{}: {e}", m.file)),
    };
    let (changed, line) = match list::apply(m, &original) {
        Ok(changed) => {
            let at = list::locate(m, &original).unwrap_or(0);
            (changed, original[..at].matches('\n').count() + 1)
        }
        Err(e) => return done(Outcome::NotApplied, e),
    };
    if let Err(e) = fs::write(&path, changed) {
        return done(Outcome::NotApplied, format!("{}: {e}", m.file));
    }
    let mut log = String::new();
    let mut outcome = Outcome::Survived;
    for command in &m.tests {
        let r = run(dir, command, Duration::from_secs(m.timeout), jobs);
        let _ = writeln!(log, "$ {command}\n{}", r.output);
        if r.timed_out {
            outcome = Outcome::TimedOut;
            break;
        }
        if !r.passed {
            let unbuilt =
                r.output.contains("error: could not compile") || r.output.contains("error[E");
            outcome = if unbuilt {
                Outcome::Unviable
            } else {
                Outcome::Caught
            };
            break;
        }
    }
    let restored = fs::write(&path, &original);
    let _ = fs::write(format!("{ROOT}/logs/{}.log", m.id), &log);
    if let Err(e) = restored {
        // The worktree is wrong from here on: stop rather than mislead.
        eprintln!("zoraya-mutants: cannot restore {}: {e}", path.display());
        std::process::exit(2);
    }
    done(outcome, format!("{}:{line}", m.file))
}

fn worker(i: usize, dir: PathBuf, shared: Arc<Mutex<Shared>>, jobs: usize, total: usize) {
    loop {
        let next = shared.lock().map(|mut s| s.queue.pop()).ok().flatten();
        let Some((list, m)) = next else {
            return;
        };
        let r = mutate(&dir, &list, &m, jobs);
        if let Ok(mut s) = shared.lock() {
            let n = s.results.len() + 1;
            println!(
                "[{n}/{total}] {} {} ({} s, worker {i})",
                m.id,
                r.outcome.name(),
                r.seconds
            );
            s.results.push(r);
        }
    }
}

/// Every set of tests, unmutated: a failing one would make every mutant
/// that uses it look caught.
fn baseline(dirs: &[PathBuf], mutants: &[(String, Mutant)], jobs: usize) -> Result<(), String> {
    let mut sets: BTreeMap<Vec<String>, u64> = BTreeMap::new();
    for (_, m) in mutants {
        let timeout = sets.entry(m.tests.clone()).or_insert(m.timeout);
        *timeout = (*timeout).max(m.timeout);
    }
    let sets: Vec<(Vec<String>, u64)> = sets.into_iter().collect();
    let failures = Arc::new(Mutex::new(Vec::new()));
    std::thread::scope(|scope| {
        for (i, dir) in dirs.iter().enumerate() {
            let mine: Vec<&(Vec<String>, u64)> = sets.iter().skip(i).step_by(dirs.len()).collect();
            let failures = Arc::clone(&failures);
            scope.spawn(move || {
                for (tests, timeout) in mine {
                    for command in tests {
                        let r = run(dir, command, Duration::from_secs(*timeout), jobs);
                        if !r.passed {
                            let log = format!("{ROOT}/logs/baseline-w{i}.log");
                            let _ = fs::write(&log, &r.output);
                            if let Ok(mut f) = failures.lock() {
                                f.push(format!("`{command}` fails unmutated (see {log})"));
                            }
                        }
                    }
                }
            });
        }
    });
    let failures = failures.lock().map(|f| f.clone()).unwrap_or_default();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn report(results: &[Tried], commit: &str, seconds: u64) -> String {
    let mut counts: BTreeMap<Outcome, usize> = BTreeMap::new();
    for r in results {
        *counts.entry(r.outcome).or_default() += 1;
    }
    let count = |o| counts.get(&o).copied().unwrap_or(0);
    let mut out = String::new();
    let _ = writeln!(out, "# Surviving mutants\n");
    let _ = writeln!(
        out,
        "Commit `{commit}`, {} mutants in {} min: {} caught, {} timed out, {} unviable, **{} survived**, {} not applied.\n",
        results.len(),
        seconds.div_ceil(60),
        count(Outcome::Caught),
        count(Outcome::TimedOut),
        count(Outcome::Unviable),
        count(Outcome::Survived),
        count(Outcome::NotApplied),
    );
    let _ = writeln!(
        out,
        "A survivor is a change that every listed test passed: either it changes nothing observable \
         (an equivalent mutant), or a test is missing. Each has its log in `build/mutants/logs/<id>.log`.\n"
    );
    let mut sorted: Vec<&Tried> = results.iter().collect();
    sorted.sort_by(|a, b| a.mutant.id.cmp(&b.mutant.id));
    let survivors: Vec<&&Tried> = sorted
        .iter()
        .filter(|r| r.outcome == Outcome::Survived)
        .collect();
    if survivors.is_empty() {
        let _ = writeln!(out, "No mutant survived.");
    }
    for r in survivors {
        let m = &r.mutant;
        let _ = writeln!(out, "## {} — `{}`\n", m.id, r.detail);
        let _ = writeln!(out, "List: `{}`.", r.list);
        if !m.note.is_empty() {
            let _ = writeln!(out, "Note: {}", m.note);
        }
        let _ = writeln!(out, "\nFrom:\n\n```rust\n{}\n```\n", m.find);
        let _ = writeln!(out, "To:\n\n```rust\n{}\n```\n", m.replace);
        let _ = writeln!(out, "Tests run, all passed:\n");
        for t in &m.tests {
            let _ = writeln!(out, "- `{t}`");
        }
        let _ = writeln!(out);
    }
    let bad: Vec<&&Tried> = sorted
        .iter()
        .filter(|r| r.outcome == Outcome::NotApplied)
        .collect();
    if !bad.is_empty() {
        let _ = writeln!(out, "# Mutations that did not apply\n");
        let _ = writeln!(
            out,
            "The list is out of date with the code: fix the `find` text.\n"
        );
        for r in bad {
            let _ = writeln!(out, "- {} (`{}`): {}", r.mutant.id, r.mutant.file, r.detail);
        }
    }
    out
}

/// Most mutants run at once, whatever `--jobs` says: two runs of the test
/// suite at a time is what the host's memory takes (docs/backlog.md).
const MAX_WORKERS: usize = 2;

struct Args {
    jobs: usize,
    only: BTreeSet<String>,
    lists: Vec<PathBuf>,
    /// `--check`: only that every mutant still applies to the working tree
    /// (its `find` is where the list says), for `make check-mutant-lists`.
    check: bool,
}

fn parse_args() -> Result<Args, String> {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut a = Args {
        jobs: (cores / 2).clamp(1, MAX_WORKERS),
        only: BTreeSet::new(),
        lists: Vec::new(),
        check: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--jobs" => {
                let n = args.next().ok_or("--jobs needs a number")?;
                a.jobs = n.parse().map_err(|_| format!("bad --jobs {n}"))?;
                if a.jobs == 0 {
                    return Err("--jobs must be at least 1".into());
                }
            }
            "--only" => {
                let ids = args.next().ok_or("--only needs ids")?;
                a.only.extend(ids.split(',').map(str::to_string));
            }
            "--check" => a.check = true,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ => a.lists.push(PathBuf::from(arg)),
        }
    }
    if a.lists.is_empty() {
        let mut found: Vec<PathBuf> = fs::read_dir("tools/mutants")
            .map_err(|e| format!("tools/mutants: {e}"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            // The tool's own manifest lives here too and is not a list.
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .filter(|p| p.file_name().is_some_and(|n| n != "Cargo.toml"))
            .collect();
        found.sort();
        a.lists = found;
    }
    Ok(a)
}

fn main_() -> Result<bool, String> {
    let args = parse_args()?;
    if !Path::new("Makefile").exists() || !Path::new("tools/mutants").exists() {
        return Err("run from the repository root".into());
    }
    let mut mutants: Vec<(String, Mutant)> = Vec::new();
    let mut ids = BTreeSet::new();
    for path in &args.lists {
        let name = path.to_string_lossy().into_owned();
        let text = fs::read_to_string(path).map_err(|e| format!("{name}: {e}"))?;
        for m in list::parse(&name, &text)? {
            if !ids.insert(m.id.clone()) {
                return Err(format!("{name}: id {} is used twice", m.id));
            }
            if args.only.is_empty() || args.only.contains(&m.id) {
                mutants.push((name.clone(), m));
            }
        }
    }
    if let Some(missing) = args.only.iter().find(|id| !ids.contains(*id)) {
        return Err(format!("--only: no mutant {missing}"));
    }
    if mutants.is_empty() {
        return Err("no mutants".into());
    }
    if args.check {
        return check_lists(&mutants);
    }
    let commit = git(Path::new("."), &["rev-parse", "--short", "HEAD"])?;
    let dirty = git(
        Path::new("."),
        &["status", "--porcelain", "--untracked-files=no"],
    )?;
    if !dirty.is_empty() {
        println!(
            "zoraya-mutants: uncommitted changes are not mutated: the workers check out {commit}"
        );
    }
    if args.jobs > MAX_WORKERS {
        println!(
            "zoraya-mutants: --jobs {} is more than {MAX_WORKERS}; running {MAX_WORKERS}",
            args.jobs
        );
    }
    let workers = args.jobs.min(MAX_WORKERS).min(mutants.len());
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let jobs = (cores / workers).max(1);
    fs::create_dir_all(format!("{ROOT}/logs")).map_err(|e| format!("{ROOT}: {e}"))?;
    for entry in fs::read_dir(format!("{ROOT}/logs"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let _ = fs::remove_file(entry.path());
    }
    let mut dirs = Vec::new();
    for i in 0..workers {
        dirs.push(worktree(i, &commit)?);
    }
    let start = Instant::now();
    println!(
        "zoraya-mutants: {} mutants at {commit}, {workers} workers; first the tests unmutated",
        mutants.len()
    );
    baseline(&dirs, &mutants, jobs)
        .map_err(|e| format!("the tests fail without a mutant:\n{e}"))?;
    let total = mutants.len();
    // Popped from the end: reversed, so that they run in list order.
    mutants.reverse();
    let shared = Arc::new(Mutex::new(Shared {
        queue: mutants,
        results: Vec::new(),
    }));
    std::thread::scope(|scope| {
        for (i, dir) in dirs.into_iter().enumerate() {
            let shared = Arc::clone(&shared);
            scope.spawn(move || worker(i, dir, shared, jobs, total));
        }
    });
    let results = match Arc::try_unwrap(shared) {
        Ok(shared) => shared.into_inner().map(|s| s.results).unwrap_or_default(),
        Err(_) => return Err("a worker is still running".into()),
    };
    let text = report(&results, &commit, start.elapsed().as_secs());
    let path = format!("{ROOT}/report.md");
    fs::write(&path, &text).map_err(|e| format!("{path}: {e}"))?;
    let bad = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Survived | Outcome::NotApplied))
        .count();
    let survived = results
        .iter()
        .filter(|r| r.outcome == Outcome::Survived)
        .count();
    println!(
        "zoraya-mutants: {} mutants, {survived} survived, {} not applied: {path}",
        results.len(),
        bad - survived
    );
    Ok(bad == 0)
}

/// Every mutant applies to the working tree. A `find` that is gone means the
/// code moved (update the list) or the mutated text itself was committed
/// (b447edd committed T-exec-exit-outside-state's `replace`).
fn check_lists(mutants: &[(String, Mutant)]) -> Result<bool, String> {
    let mut bad = 0;
    for (name, m) in mutants {
        let applies = fs::read_to_string(&m.file)
            .map_err(|e| format!("{}: {e}", m.file))
            .and_then(|text| list::apply(m, &text));
        if let Err(e) = applies {
            println!("{name}: {} ({}): {e}", m.id, m.file);
            bad += 1;
        }
    }
    println!(
        "zoraya-mutants: {} mutants, {bad} do not apply",
        mutants.len()
    );
    Ok(bad == 0)
}

fn main() -> ExitCode {
    match main_() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("zoraya-mutants: {e}");
            ExitCode::from(2)
        }
    }
}

//! `zoraya-ar`: the archiver of the Zoraya toolchain (ADR-0109 item 3).
//!
//! `zoraya-ar [-]OPERATION[MODIFIERS] ARCHIVE [FILE...]`, the subset of `ar`
//! that build systems use:
//!
//! - `r` inserts the files, replacing members of the same name, `q` appends
//!   them, `d` deletes the named members, `t` lists the members, `x` extracts
//!   them (all, or the named ones), `s` alone only rewrites the index;
//! - modifiers `c` (create quietly), `s` (index), `u` (only newer: every file
//!   is taken), `v` (verbose) and `D` (deterministic) are accepted; the index
//!   is always written and every archive is deterministic.
//!
//! Invoked under a name ending in `ranlib`, it rewrites the index of each
//! archive named. Member names are the files' base names.

use std::path::Path;
use std::process::ExitCode;

use zoraya_ar::{Member, read, replace, write};

fn load(path: &str) -> Result<Vec<Member>, String> {
    match std::fs::read(path) {
        Ok(bytes) => read(&bytes).map_err(|e| format!("{path}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

fn store(path: &str, members: &[Member]) -> Result<(), String> {
    let bytes = write(members).map_err(|e| format!("{path}: {e}"))?;
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{tmp}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{path}: {e}"))
}

fn base_name(path: &str) -> Result<String, String> {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or(format!("{path}: no usable file name"))
}

fn run(program: &str, args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) == Some("--version") {
        println!("zoraya-ar {} (GNU ar format)", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if program.ends_with("ranlib") {
        if args.is_empty() {
            return Err("usage: ranlib ARCHIVE...".into());
        }
        for path in args {
            if !Path::new(path).exists() {
                return Err(format!("{path}: no such archive"));
            }
            store(path, &load(path)?)?;
        }
        return Ok(());
    }
    let [ops, archive, files @ ..] = args else {
        return Err("usage: zoraya-ar [-]{r|q|d|t|x|s}[csuvD] ARCHIVE [FILE...]".into());
    };
    let ops = ops.strip_prefix('-').unwrap_or(ops);
    let mut operation = None;
    let mut verbose = false;
    let mut quiet = false;
    let mut index = false;
    for c in ops.chars() {
        match c {
            'r' | 'q' | 'd' | 't' | 'x' => {
                if operation.replace(c).is_some() {
                    return Err(format!("more than one operation in '{ops}'"));
                }
            }
            'c' => quiet = true,
            's' => index = true,
            'v' => verbose = true,
            'u' | 'D' => {}
            other => return Err(format!("unsupported operation or modifier '{other}'")),
        }
    }
    let Some(operation) = operation.or(index.then_some('s')) else {
        return Err(format!("no operation in '{ops}'"));
    };
    let exists = Path::new(archive).exists();
    let mut members = load(archive)?;
    match operation {
        'r' | 'q' => {
            if !exists && !quiet {
                eprintln!("zoraya-ar: creating {archive}");
            }
            for f in files {
                let data = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
                let member = Member {
                    name: base_name(f)?,
                    data,
                };
                if verbose {
                    println!(
                        "{} - {}",
                        if operation == 'q' { 'q' } else { 'r' },
                        member.name
                    );
                }
                if operation == 'q' {
                    members.push(member);
                } else {
                    replace(&mut members, member);
                }
            }
            store(archive, &members)
        }
        'd' => {
            if !exists {
                return Err(format!("{archive}: no such archive"));
            }
            for f in files {
                let name = base_name(f)?;
                let before = members.len();
                members.retain(|m| m.name != name);
                if verbose && members.len() != before {
                    println!("d - {name}");
                }
            }
            store(archive, &members)
        }
        't' => {
            if !exists {
                return Err(format!("{archive}: no such archive"));
            }
            for m in &members {
                if verbose {
                    println!("rw-r--r-- 0/0 {:>8} {}", m.data.len(), m.name);
                } else {
                    println!("{}", m.name);
                }
            }
            Ok(())
        }
        'x' => {
            if !exists {
                return Err(format!("{archive}: no such archive"));
            }
            for m in &members {
                if !files.is_empty() && !files.contains(&m.name) {
                    continue;
                }
                if m.name.contains('/') || m.name == ".." || m.name == "." {
                    return Err(format!("{archive}: refusing to extract '{}'", m.name));
                }
                if verbose {
                    println!("x - {}", m.name);
                }
                std::fs::write(&m.name, &m.data).map_err(|e| format!("{}: {e}", m.name))?;
            }
            Ok(())
        }
        _ => {
            if !exists {
                return Err(format!("{archive}: no such archive"));
            }
            store(archive, &members)
        }
    }
}

/// The tool's memory ceiling (capped.rs).
#[global_allocator]
static ALLOCATOR: zoraya_ar::capped::Capped =
    zoraya_ar::capped::Capped::new(zoraya_ar::capped::LIMIT);

fn main() -> ExitCode {
    let mut args = std::env::args();
    let program = args.next().unwrap_or_default();
    let program = Path::new(&program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("zoraya-ar")
        .to_string();
    let args: Vec<String> = args.collect();
    match run(&program, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{program}: {e}");
            ExitCode::FAILURE
        }
    }
}

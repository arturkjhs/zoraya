//! INV-0336-2: every name in the image's `/bin`, links included, has a trusted
//! description in `user/describe/posix/` that names its path, and every
//! option of a description that takes an argument gives the argument a kind
//! (the parser requires one; this test also requires that an option or
//! operand whose text says it takes a file name has a kind that is a path).
//!
//! The names are the commands of `ports/toybox-applets.txt`, the links `sh`
//! and `cc`, and the ported programs of `ports/Makefile`'s rootfs rule
//! (`BIN_PROGRAMS` below). When `build/rootfs-posix/bin` exists (after
//! `make ports`), every name found there is checked too.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use zoraya_describe::{Kind, parse};

zoraya_test_alloc::cap_memory!();

/// The programs `ports/Makefile` puts in `/bin` besides the toybox commands.
const BIN_PROGRAMS: &[&str] = &[
    "dash",
    "sh",
    "make",
    "pdpmake",
    "tcc",
    "cc",
    "awk",
    "ssh",
    "ssh-keygen",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn applets() -> Vec<String> {
    let text = fs::read_to_string(root().join("ports/toybox-applets.txt")).expect("applet list");
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

fn names_in_bin() -> BTreeSet<String> {
    let mut names: BTreeSet<String> = applets().into_iter().collect();
    names.extend(BIN_PROGRAMS.iter().map(|s| s.to_string()));
    if let Ok(dir) = fs::read_dir(root().join("build/rootfs-posix/bin")) {
        for e in dir.flatten() {
            names.insert(e.file_name().to_string_lossy().into_owned());
        }
    }
    names
}

#[test]
fn every_name_in_bin_has_a_description_naming_its_path() {
    let dir = root().join("user/describe/posix");
    let mut problems = Vec::new();
    for name in names_in_bin() {
        // `toybox` itself moved out of /bin (ADR-0336 item 8); a program the
        // ports build for tests only is not in the list.
        if name == "toybox" || name == "busybox" {
            continue;
        }
        let file = dir.join(format!("{name}.cmd"));
        let Ok(bytes) = fs::read(&file) else {
            problems.push(format!("{name}: no user/describe/posix/{name}.cmd"));
            continue;
        };
        match parse(&bytes) {
            Err(e) => problems.push(format!("{name}: {e}")),
            Ok(d) => {
                if d.program.as_deref() != Some(&format!("/bin/{name}")) {
                    problems.push(format!(
                        "{name}: program is {:?}, not /bin/{name}",
                        d.program
                    ));
                }
                if d.name != name {
                    problems.push(format!("{name}: the description is named {}", d.name));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_path_taking_option_or_operand_has_a_path_kind() {
    let dir = root().join("user/describe/posix");
    let mut problems = Vec::new();
    for name in names_in_bin() {
        let Ok(bytes) = fs::read(dir.join(format!("{name}.cmd"))) else {
            continue;
        };
        let Ok(d) = parse(&bytes) else { continue };
        for o in &d.options {
            if let Some(a) = &o.argument
                && a.kind == Kind::Text
                && names_a_path(&a.name)
            {
                problems.push(format!(
                    "{name}: option {} takes {} as text",
                    o.label(),
                    a.name
                ));
            }
        }
        for op in &d.operands {
            if op.kind == Kind::Text && names_a_path(&op.name) {
                problems.push(format!("{name}: operand {} is text", op.name));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Whether an argument's name says it is a file name: `FILE`, `DIR`,
/// `ARCHIVE`, `PATH`, or a name ending in one (`PROGFILE`, `REFFILE`,
/// `PATCHFILE`; review of 10e, finding 18).
fn names_a_path(name: &str) -> bool {
    let name = name.trim_end_matches("...");
    ["FILE", "DIR", "ARCHIVE", "PATH"]
        .iter()
        .any(|p| name == *p || name.ends_with(p))
}

#[test]
fn the_path_rule_knows_the_longer_names() {
    for n in ["FILE", "PROGFILE", "REFFILE", "PATCHFILE", "DIR", "PATH..."] {
        assert!(names_a_path(n), "{n}");
    }
    for n in ["PATTERN", "N", "MODE", "FS"] {
        assert!(!names_a_path(n), "{n}");
    }
}

/// ADR-0336 item 5 and CP-05: a command that copies or reads its sources is
/// not given them to change. No option widens a `read` operand to a kind
/// that writes, except where the command changes that operand itself
/// (`chmod -R`: the modes throughout the tree; `sed -i`: the file). Review of 10e, finding 2:
/// `cp -r` had made its source a tree to change.
#[test]
fn no_option_widens_a_read_operand_to_write_unless_it_changes_it() {
    const CHANGES_ITS_OPERAND: &[&str] = &["chmod", "sed"];
    let dir = root().join("user/describe/posix");
    let mut problems = Vec::new();
    for name in names_in_bin() {
        let Ok(bytes) = fs::read(dir.join(format!("{name}.cmd"))) else {
            continue;
        };
        let Ok(d) = parse(&bytes) else { continue };
        for w in &d.whens {
            let op = &d.operands[w.operand];
            let writes = !matches!(w.kind, Kind::Read | Kind::Text | Kind::Url | Kind::Host);
            if op.kind == Kind::Read && writes && !CHANGES_ITS_OPERAND.contains(&name.as_str()) {
                problems.push(format!(
                    "{name}: {} makes {} {:?}",
                    d.options[w.option].label(),
                    op.name,
                    w.kind
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// ADR-0153 rule 10: only a description that says `cwd none` gets file operands
/// as narrowed names, so a command that can write through an operand does not
/// leave the current directory read-only by accident; this test lists what
/// each description lets through for the architect (ADR-0336 item 5): run
/// with `--nocapture`, it prints one line per command.
#[test]
fn list_the_options_each_description_lets_through() {
    let dir = root().join("user/describe/posix");
    for name in names_in_bin() {
        let Ok(bytes) = fs::read(dir.join(format!("{name}.cmd"))) else {
            continue;
        };
        let Ok(d) = parse(&bytes) else { continue };
        let opts: Vec<String> = d.options.iter().map(|o| o.label()).collect();
        println!("lets-through {name}: {}", opts.join(" "));
    }
}

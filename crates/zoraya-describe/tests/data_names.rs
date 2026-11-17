//! INV-0341-4: every `NAME` of a `data` line in `user/describe/` is in
//! `user/describe/data-names.txt` with exactly the programs that carry it, and
//! no application name of the panel's dock equals one.
//!
//! INV-0340-2 (the lint half): `ssh.cmd` offers no option that names another
//! host, runs a program, listens or reads a file outside ssh's directory
//! (ADR-0340's deny list), no option of it takes a path, and its `reach` is
//! the one `HOST` operand.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use zoraya_describe::{Description, Kind, parse};

zoraya_test_alloc::cap_memory!();

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every description under `user/describe/GROUP/`, with its group.
fn descriptions() -> Vec<(String, Description)> {
    let mut out = Vec::new();
    for group in fs::read_dir(root().join("user/describe"))
        .unwrap()
        .flatten()
    {
        if !group.path().is_dir() {
            continue;
        }
        for file in fs::read_dir(group.path()).unwrap().flatten() {
            let path = file.path();
            if path.extension().is_none_or(|e| e != "cmd") {
                continue;
            }
            let d = parse(&fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            out.push((group.file_name().to_string_lossy().into_owned(), d));
        }
    }
    out
}

/// The list: NAME to its programs.
fn list() -> BTreeMap<String, BTreeSet<String>> {
    let text = fs::read_to_string(root().join("user/describe/data-names.txt")).unwrap();
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let name = words.next().unwrap().to_string();
        let programs: BTreeSet<String> = words.map(String::from).collect();
        assert!(!programs.is_empty(), "{name} has no program");
        assert!(out.insert(name.clone(), programs).is_none(), "{name} twice");
    }
    out
}

#[test]
fn the_data_lines_and_the_list_agree() {
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (group, d) in descriptions() {
        let Some(data) = &d.data else { continue };
        // Only the system image's POSIX programs carry one (INV-0341-1); a
        // native command's directory is a different thing.
        assert_eq!(group, "posix", "{}: a data line outside posix/", d.name);
        assert!(
            d.program.is_some(),
            "{}: a data line needs a program",
            d.name
        );
        found
            .entry(data.name.clone())
            .or_default()
            .insert(d.name.clone());
    }
    assert_eq!(
        found,
        list(),
        "user/describe/data-names.txt and the data lines differ"
    );
}

#[test]
fn no_panel_application_has_a_data_name() {
    let text = fs::read_to_string(root().join("crates/zoraya-panel/src/lib.rs")).unwrap();
    let start = text.find("pub const APPS").expect("the panel's APPS");
    let table = &text[start..start + text[start..].find("];").unwrap()];
    let apps: Vec<&str> = table
        .split('(')
        .skip(1)
        .filter_map(|t| t.split('"').nth(1))
        .collect();
    assert!(
        apps.contains(&"term") && apps.contains(&"files"),
        "{apps:?}"
    );
    for name in list().keys() {
        assert!(
            !apps.contains(&name.as_str()),
            "{name} is a panel application"
        );
    }
}

/// ADR-0340 item 4: what `ssh.cmd` must not offer.
#[test]
fn ssh_offers_nothing_that_changes_the_host_or_runs_a_program() {
    let all = descriptions();
    let (_, ssh) = all.iter().find(|(_, d)| d.name == "ssh").expect("ssh.cmd");
    for o in &ssh.options {
        if let Some(c) = o.short {
            assert!(
                !"JWoFLRDAESwIi".contains(c),
                "ssh offers -{c}, which ADR-0340 item 4 refuses"
            );
        }
        for banned in [
            "config", "proxy", "jump", "identity", "forward", "agent", "option",
        ] {
            assert!(!o.long.contains(banned), "--{} offered", o.long);
        }
        // No option designates a path: ssh's files are its directory's.
        assert!(
            o.argument.as_ref().is_none_or(|a| !a.kind.is_path()),
            "-{:?} takes a path",
            o.short
        );
    }
    assert_eq!(ssh.operands[0].kind, Kind::Host);
    assert_eq!(ssh.operands.last().unwrap().kind, Kind::Rest);
    let reach = ssh.reach.expect("ssh has a reach line");
    assert_eq!((reach.operand, reach.port), (0, 22));
    assert_eq!(ssh.options[reach.option.unwrap()].short, Some('p'));
    assert!(!ssh.needs_net_status && !ssh.net_web);
    let data = ssh.data.as_ref().unwrap();
    assert_eq!(
        (data.name.as_str(), data.path.as_str(), data.read),
        ("ssh", "~/.ssh", false)
    );
    // ssh-keygen has no network and shares the directory.
    let (_, keygen) = all.iter().find(|(_, d)| d.name == "ssh-keygen").unwrap();
    assert!(keygen.reach.is_none());
    assert_eq!(keygen.data.as_ref().unwrap().name, "ssh");
    // The planner's `--` before HOST is only sound if the description gives
    // HOST as the first operand and no option follows the host in the usage.
    assert_eq!(ssh.operands[0].name, "HOST");
}

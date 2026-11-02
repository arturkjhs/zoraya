//! `posix-surface-check`
//!
//! - `list CSV OUT` writes the tier list generated from the CSV;
//!   `list --check CSV LIST` fails if LIST is not what the CSV gives.
//! - `check LIST ARCHIVE [--allow-svc PREFIX]...` checks an archive's exports
//!   against the list (INV-0100-1, INV-0005-1), the exclusion table
//!   (INV-0005-2), that it defines every name of the list and every name its
//!   members use, and that no member outside the allowed ones contains `svc`
//!   (INV-0100-2).
//! - `hide LIST OBJECT...` prints the renames that hide every defined name
//!   outside the list, one `old new` pair per line, for `objcopy
//!   --redefine-syms` (ADR-0141).

use std::collections::BTreeSet;
use std::process::ExitCode;

use posix_surface_check::{ar, elf, surface};

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn read_text(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn list(args: &[String]) -> Result<(), String> {
    match args {
        [flag, csv, file] if flag == "--check" => {
            let want = surface::render_list(&surface::tier_list(&read_text(csv)?)?);
            if read_text(file)? != want {
                return Err(format!(
                    "{file} is not what {csv} gives: regenerate it with `posix-surface-check list {csv} {file}`"
                ));
            }
            println!("{file}: up to date");
            Ok(())
        }
        [csv, out] => {
            let names = surface::tier_list(&read_text(csv)?)?;
            std::fs::write(out, surface::render_list(&names)).map_err(|e| format!("{out}: {e}"))?;
            println!("{out}: {} names", names.len());
            Ok(())
        }
        _ => Err("usage: list [--check] CSV FILE".into()),
    }
}

fn check(args: &[String]) -> Result<bool, String> {
    let [list, archive, rest @ ..] = args else {
        return Err("usage: check LIST ARCHIVE [--allow-svc PREFIX]...".into());
    };
    let mut allowed = Vec::new();
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--allow-svc", Some(p)) => allowed.push(p.clone()),
            _ => return Err(format!("unexpected argument '{flag}'")),
        }
    }
    let names = surface::parse_list(&read_text(list)?);
    let bytes = read(archive)?;
    let mut objects = Vec::new();
    for m in ar::members(&bytes).map_err(|e| format!("{archive}: {e}"))? {
        let object = elf::read(m.data).map_err(|e| format!("{archive}({}): {e}", m.name))?;
        objects.push((m.name, object));
    }
    let report = surface::check(&names, &objects, &allowed);
    let exports: BTreeSet<&str> = objects
        .iter()
        .flat_map(|(_, o)| o.symbols.iter())
        .filter(|s| s.is_defined_global() && !surface::is_internal(&s.name))
        .map(|s| s.name.as_str())
        .collect();
    for line in report.lines() {
        println!("{archive}: {line}");
    }
    println!(
        "{archive}: {} members, {} exports, tier list of {} names: {}",
        objects.len(),
        exports.len(),
        names.len(),
        if report.passed() { "passed" } else { "FAILED" }
    );
    Ok(report.passed())
}

fn hide(args: &[String]) -> Result<(), String> {
    let [list, paths @ ..] = args else {
        return Err("usage: hide LIST OBJECT...".into());
    };
    let names = surface::parse_list(&read_text(list)?);
    let files: Vec<(String, Vec<u8>)> = paths
        .iter()
        .map(|p| Ok((p.clone(), read(p)?)))
        .collect::<Result<_, String>>()?;
    let mut objects = Vec::new();
    for (path, bytes) in &files {
        objects.push(elf::read(bytes).map_err(|e| format!("{path}: {e}"))?);
    }
    for (old, new) in surface::hide(&names, &objects)? {
        println!("{old} {new}");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, rest)) if cmd == "list" => list(rest).map(|()| true),
        Some((cmd, rest)) if cmd == "check" => check(rest),
        Some((cmd, rest)) if cmd == "hide" => hide(rest).map(|()| true),
        _ => Err("usage: posix-surface-check {list|check|hide} ...".into()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("posix-surface-check: {e}");
            ExitCode::FAILURE
        }
    }
}

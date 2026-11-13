//! `posix-surface-check`
//!
//! - `list CSV OUT` writes the tier list generated from the CSV;
//!   `list --check CSV LIST` fails if LIST is not what the CSV gives.
//! - `check LIST ARCHIVE [--allow-svc PREFIX]...` checks an archive's exports
//!   against the list (INV-0100-1, INV-0005-1), the exclusion table
//!   (INV-0005-2), that it defines every name of the list and every name its
//!   members use, and that no member outside the allowed ones contains `svc`
//!   (INV-0100-2).
//!
//! LIST may be followed by `+ADR` parts: ADR files whose `posix-surface-added`
//! blocks add names to the list (ADR-0142, ADR-0144).
//! - `hide LIST OUTDIR OBJECT...` hides every defined name outside the list
//!   (ADR-0141): it writes each object, renamed, into OUTDIR under its base
//!   name and prints the renames, one `old new` pair per line.
//! - `calllog LIST OUTDIR ASMDIR OBJECT...` is the call log of ADR-0110 item 7
//!   (INV-0005-3): every exported function's definition and the objects' own
//!   references are renamed to `__zoraya_real_NAME` (the objects written into
//!   OUTDIR), and ASMDIR gets one wrapper `NAME.S` a function and `zoraya-calls.S`,
//!   the bitmap; it prints the logged names, one a line, in the bitmap's
//!   order.

use std::collections::BTreeSet;
use std::process::ExitCode;

use posix_surface_check::{ar, elf, rename, surface};

/// The names an archive may export: `SPEC` is the tier list's file, then
/// `+`-separated ADR files whose `posix-surface-added` blocks add names, or
/// further list files (a name ending in `.txt`: the T9 list of ADR-0308). A
/// name from an ADR that is not accepted is admitted with a printed note. A
/// `.txt` part is a list file and is always admitted, with no note and no
/// status check: the ADR that governs it is not read, so adding a name there
/// is a change of the list, reviewed as one.
fn names(spec: &str) -> Result<BTreeSet<String>, String> {
    let mut parts = spec.split('+');
    let list = parts.next().unwrap_or_default();
    let mut names = surface::parse_list(&read_text(list)?);
    for adr in parts {
        if adr.ends_with(".txt") {
            names.extend(surface::parse_list(&read_text(adr)?));
            continue;
        }
        let (added, accepted) =
            surface::added_names(&read_text(adr)?).map_err(|e| format!("{adr}: {e}"))?;
        if !accepted {
            eprintln!(
                "posix-surface-check: note: {adr} is not accepted; admitting {}",
                added.join(", ")
            );
        }
        names.extend(added);
    }
    Ok(names)
}

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
    let names = names(list)?;
    let bytes = read(archive)?;
    let mut objects = Vec::new();
    for m in ar::members(&bytes).map_err(|e| format!("{archive}: {e}"))? {
        let object = elf::read(m.data).map_err(|e| format!("{archive}({}): {e}", m.name))?;
        objects.push((m.name, object));
    }
    let entries = ar::index_entries(&bytes).map_err(|e| format!("{archive}: {e}"))?;
    let index: Option<Vec<String>> = entries
        .as_ref()
        .map(|e| e.iter().map(|(_, n)| n.clone()).collect());
    let mut report = surface::check(&names, &objects, &allowed, index.as_deref());
    if let Some(entries) = &entries {
        let at: Vec<(usize, String, elf::Object<'_>)> = ar::members(&bytes)
            .map_err(|e| format!("{archive}: {e}"))?
            .into_iter()
            .zip(objects.iter())
            .map(|(m, (_, o))| (m.offset, m.name, o.clone()))
            .collect();
        report
            .index
            .extend(surface::check_index_offsets(entries, &at));
    }
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
    let [list, outdir, paths @ ..] = args else {
        return Err("usage: hide LIST OUTDIR OBJECT...".into());
    };
    let names = names(list)?;
    let files: Vec<(String, Vec<u8>)> = paths
        .iter()
        .map(|p| Ok((p.clone(), read(p)?)))
        .collect::<Result<_, String>>()?;
    let mut objects = Vec::new();
    for (path, bytes) in &files {
        objects.push(elf::read(bytes).map_err(|e| format!("{path}: {e}"))?);
    }
    let renames = surface::hide(&names, &objects)?;
    for (old, new) in &renames {
        println!("{old} {new}");
    }
    let map: std::collections::BTreeMap<String, String> = renames.into_iter().collect();
    for (path, bytes) in &files {
        let name = std::path::Path::new(path)
            .file_name()
            .ok_or(format!("{path}: no file name"))?;
        let out = std::path::Path::new(outdir).join(name);
        if out.exists() {
            return Err(format!("{}: two objects with this name", out.display()));
        }
        let renamed = rename::rename(bytes, &map).map_err(|e| format!("{path}: {e}"))?;
        std::fs::write(&out, renamed).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(())
}

fn calllog(args: &[String]) -> Result<(), String> {
    let [list, outdir, asmdir, paths @ ..] = args else {
        return Err("usage: calllog LIST OUTDIR ASMDIR OBJECT...".into());
    };
    let names = names(list)?;
    let files: Vec<(String, Vec<u8>)> = paths
        .iter()
        .map(|p| Ok((p.clone(), read(p)?)))
        .collect::<Result<_, String>>()?;
    let mut objects = Vec::new();
    for (path, bytes) in &files {
        objects.push(elf::read(bytes).map_err(|e| format!("{path}: {e}"))?);
    }
    let (logged, renames) = surface::call_log(&names, &objects)?;
    let map: std::collections::BTreeMap<String, String> = renames.into_iter().collect();
    for (path, bytes) in &files {
        let name = std::path::Path::new(path)
            .file_name()
            .ok_or(format!("{path}: no file name"))?;
        let out = std::path::Path::new(outdir).join(name);
        if out.exists() {
            return Err(format!("{}: two objects with this name", out.display()));
        }
        let renamed = rename::rename(bytes, &map).map_err(|e| format!("{path}: {e}"))?;
        std::fs::write(&out, renamed).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    let write = |file: String, text: String| {
        let out = std::path::Path::new(asmdir).join(file);
        std::fs::write(&out, text).map_err(|e| format!("{}: {e}", out.display()))
    };
    for (i, n) in logged.iter().enumerate() {
        write(format!("{n}.S"), surface::call_log_wrapper(n, i))?;
        println!("{n}");
    }
    // A hyphen is in no function's name, so no wrapper's file is this one.
    write(
        "zoraya-calls.S".into(),
        surface::call_log_bitmap(logged.len(), surface::list_hash(&logged)),
    )
}

/// The tool's memory ceiling (capped.rs).
#[global_allocator]
static ALLOCATOR: posix_surface_check::capped::Capped =
    posix_surface_check::capped::Capped::new(posix_surface_check::capped::LIMIT);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, rest)) if cmd == "list" => list(rest).map(|()| true),
        Some((cmd, rest)) if cmd == "check" => check(rest),
        Some((cmd, rest)) if cmd == "hide" => hide(rest).map(|()| true),
        Some((cmd, rest)) if cmd == "calllog" => calllog(rest).map(|()| true),
        _ => Err("usage: posix-surface-check {list|check|hide|calllog} ...".into()),
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

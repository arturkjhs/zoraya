//! `zoraya-elf-check PATH...`: checks every file named, and every regular
//! file under a directory named, with `zoraya_elf_check::check`. Symbolic
//! links are not followed (a link's target is checked as its own file), and a
//! link to nothing fails the check (an applet link in `/bin`).
//! Fails if any file is refused or if no file was checked.
//!
//! `zoraya-elf-check --no-foreign-svc MAP [--libc ARCHIVE] PATH...` also fails on an `svc`
//! instruction in an executable segment that the linker's map (`-Wl,-Map=MAP`,
//! lld's format) does not attribute to a member of `libc.a` (INV-0450-2): the
//! check of a Rust program built by `tools/zcargo`, run before any stripping.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn walk(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        entries.sort();
        for e in entries {
            walk(&e, out)?;
        }
    } else if meta.is_file() {
        out.push(path.to_path_buf());
    } else if meta.file_type().is_symlink() && std::fs::metadata(path).is_err() {
        // An absolute target is the image's own path (ADR-0336 item 8: the
        // links of /bin point at /usr/libexec/toybox): it is found under
        // one of the link's ancestors, the image's root.
        let target = std::fs::read_link(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if target.is_absolute() {
            let inside = target.strip_prefix("/").unwrap_or(&target);
            if path.ancestors().skip(1).any(|a| a.join(inside).is_file()) {
                return Ok(());
            }
        }
        return Err(format!("{}: a symbolic link to nothing", path.display()));
    }
    Ok(())
}

/// The tool's memory ceiling (capped.rs).
#[global_allocator]
static ALLOCATOR: zoraya_elf_check::capped::Capped =
    zoraya_elf_check::capped::Capped::new(zoraya_elf_check::capped::LIMIT);

fn main() -> ExitCode {
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1).peekable();
    let mut allowed: Option<Vec<(u64, u64)>> = None;
    if args.peek().map(String::as_str) == Some("--no-foreign-svc") {
        args.next();
        let Some(map) = args.next() else {
            eprintln!("zoraya-elf-check: --no-foreign-svc needs a map file");
            return ExitCode::FAILURE;
        };
        let archive = if args.peek().map(String::as_str) == Some("--libc") {
            args.next();
            args.next()
        } else {
            None
        };
        match std::fs::read_to_string(&map) {
            Ok(text) => allowed = Some(zoraya_elf_check::libc_ranges(&text, archive.as_deref())),
            Err(e) => {
                eprintln!("zoraya-elf-check: {map}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    for a in args {
        if let Err(e) = walk(Path::new(&a), &mut files) {
            eprintln!("zoraya-elf-check: {e}");
            return ExitCode::FAILURE;
        }
    }
    if files.is_empty() {
        eprintln!("zoraya-elf-check: no file to check");
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    for f in &files {
        match std::fs::read(f) {
            Ok(bytes) => match zoraya_elf_check::check(&bytes) {
                Ok(p) => match allowed
                    .as_deref()
                    .map(|a| zoraya_elf_check::foreign_svc(&bytes, a))
                {
                    Some(Ok(found)) if !found.is_empty() => {
                        let at: Vec<String> =
                            found.iter().take(8).map(|a| format!("{a:#x}")).collect();
                        println!(
                            "{}: REFUSED: {} svc instruction(s) outside libc.a, at {}",
                            f.display(),
                            found.len(),
                            at.join(", ")
                        );
                        failed += 1;
                    }
                    _ => println!("{}: ok, {}", f.display(), zoraya_elf_check::describe(&p)),
                },
                Err(r) => {
                    println!("{}: REFUSED: {r}", f.display());
                    failed += 1;
                }
            },
            Err(e) => {
                println!("{}: {e}", f.display());
                failed += 1;
            }
        }
    }
    println!("zoraya-elf-check: {} files, {failed} refused", files.len());
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

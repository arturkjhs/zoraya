//! `zoraya-elf-check PATH...`: checks every file named, and every regular
//! file under a directory named, with `zoraya_elf_check::check`. Symbolic
//! links are not followed (a link's target is checked as its own file).
//! Fails if any file is refused or if no file was checked.

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
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
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
                Ok(p) => println!("{}: ok, {}", f.display(), zoraya_elf_check::describe(&p)),
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

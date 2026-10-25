//! Links with linker.ld and embeds the git commit as ZORAYA_GIT_COMMIT.

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let script = Path::new(&dir).join("linker.ld");
    println!("cargo:rustc-link-arg-bins=-T{}", script.display());
    println!("cargo:rerun-if-changed={}", script.display());

    let commit = match git(&["rev-parse", "--short=12", "HEAD"]) {
        Some(c) => {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|s| !s.is_empty());
            if dirty { format!("{c}-dirty") } else { c }
        }
        None => "unknown".to_string(),
    };
    println!("cargo:rustc-env=ZORAYA_GIT_COMMIT={commit}");

    // Rebuild when HEAD moves or the index changes.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let git_dir = Path::new(&git_dir);
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        println!("cargo:rerun-if-changed={}", git_dir.join("index").display());
        if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={}", git_dir.join(r).display());
        }
    }
}

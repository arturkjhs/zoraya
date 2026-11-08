//! Links programs at user addresses with user.ld, and checks every command
//! description in `describe/` (INV-0156-2): a description the parser refuses
//! fails the build. The checked descriptions are listed for `zosh`, which
//! has them compiled in (ADR-0156 item 2).

use std::fmt::Write as _;
use std::path::Path;

/// The groups under `describe/`, as `zosh` shows them.
/// `tests` holds the test programs' descriptions: they are used, but no
/// listing (`help`, completion) shows them.
const GROUPS: [&str; 7] = [
    "builtins", "commands", "demos", "posix", "topics", "system", "tests",
];

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let script = Path::new(&dir).join("user.ld");
    println!("cargo:rustc-link-arg-bins=-T{}", script.display());
    println!("cargo:rerun-if-changed={}", script.display());

    let mut out = String::new();
    let mut failed = Vec::new();
    for group in GROUPS {
        let path = Path::new(&dir).join("describe").join(group);
        println!("cargo:rerun-if-changed={}", path.display());
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.flatten() {
                let file = entry.path();
                if file.extension().and_then(|e| e.to_str()) != Some("cmd") {
                    continue;
                }
                println!("cargo:rerun-if-changed={}", file.display());
                let stem = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_owned();
                let bytes = std::fs::read(&file).unwrap();
                match zoraya_describe::parse(&bytes) {
                    Ok(d) if d.name == stem => names.push(stem),
                    Ok(d) => failed.push(format!("{}: names {}", file.display(), d.name)),
                    Err(e) => failed.push(format!("{}: {e}", file.display())),
                }
            }
        }
        names.sort();
        let upper = group.to_uppercase();
        writeln!(out, "pub const {upper}: &[(&str, &str)] = &[").unwrap();
        for name in names {
            writeln!(
                out,
                "    ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/describe/{group}/{name}.cmd\"))),"
            )
            .unwrap();
        }
        writeln!(out, "];").unwrap();
    }
    if !failed.is_empty() {
        for f in &failed {
            println!("cargo:warning=refused description {f}");
        }
        panic!("{} command descriptions refused (INV-0156-2)", failed.len());
    }
    let target = Path::new(&std::env::var("OUT_DIR").unwrap()).join("descriptions.rs");
    std::fs::write(target, out).unwrap();
}

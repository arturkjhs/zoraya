//! Packs files into the initial ramdisk (ADR-0033).
//!
//! Usage: `zoraya-mkramdisk [--describe DIR] OUTPUT FILE...`. Each file is
//! stored under its base name.
//!
//! With `--describe DIR`, a program `NAME` that has a description
//! `DIR/GROUP/NAME.cmd` (ADR-0156 item 2, for the groups in [`GROUPS`]) also
//! gets a ramdisk entry `NAME.cmd` holding it. The description must parse,
//! and the program's ELF section `.zoraya.command` must hold the same bytes,
//! so that the two copies cannot differ.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Groups of `DIR` whose descriptions belong to programs in the ramdisk.
/// Builtins and topics are compiled into `zosh` instead.
const GROUPS: [&str; 3] = ["commands", "demos", "system"];

/// The bytes of the ELF64 section named `name`, if there is one.
fn section<'a>(elf: &'a [u8], name: &str) -> Option<&'a [u8]> {
    zoraya_elf::section(elf, name)
}

fn description_of(dir: &Path, name: &str) -> Option<PathBuf> {
    GROUPS
        .iter()
        .map(|g| dir.join(g).join(format!("{name}.cmd")))
        .find(|p| p.is_file())
}

fn run(args: &[String]) -> Result<(), String> {
    let (describe, args) = match args {
        [flag, dir, rest @ ..] if flag == "--describe" => (Some(PathBuf::from(dir)), rest),
        _ => (None, args),
    };
    let (output, inputs) = args
        .split_first()
        .ok_or("usage: [--describe DIR] OUTPUT FILE...")?;
    let mut files = Vec::new();
    for path in inputs {
        let name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(format!("{path}: no usable file name"))?
            .to_owned();
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        // Every program of the ramdisk is native (ADR-0153 rule 9): this
        // checks zoraya-rt's mark against zoraya-elf's reader.
        if !zoraya_elf::is_native(&bytes) {
            return Err(format!(
                "{path}: it has no {} mark",
                zoraya_elf::NATIVE_SECTION
            ));
        }
        let found = describe.as_deref().and_then(|d| description_of(d, &name));
        let description = match found {
            Some(file) => {
                let text = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                zoraya_describe::parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
                if section(&bytes, ".zoraya.command") != Some(text.as_slice()) {
                    return Err(format!(
                        "{path}: its .zoraya.command section is not {}",
                        file.display()
                    ));
                }
                Some((format!("{name}.cmd"), text))
            }
            None => None,
        };
        files.push((name, bytes));
        files.extend(description);
    }
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let image = zoraya_ramdisk::pack(&refs).ok_or("a name or size does not fit the format")?;
    std::fs::write(output, image).map_err(|e| format!("{output}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zoraya-mkramdisk: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal ELF64 with a section-name table and one named section.
    fn elf_with(name: &str, body: &[u8]) -> Vec<u8> {
        let mut strings = vec![0u8];
        strings.extend_from_slice(b".shstrtab\0");
        let name_at = strings.len() as u32;
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
        let mut elf = vec![0u8; 64];
        elf[..5].copy_from_slice(b"\x7fELF\x02");
        let strings_at = elf.len();
        elf.extend_from_slice(&strings);
        let body_at = elf.len();
        elf.extend_from_slice(body);
        let shoff = elf.len();
        let mut header = |name: u32, offset: usize, size: usize| {
            let mut h = vec![0u8; 64];
            h[..4].copy_from_slice(&name.to_le_bytes());
            h[24..32].copy_from_slice(&(offset as u64).to_le_bytes());
            h[32..40].copy_from_slice(&(size as u64).to_le_bytes());
            elf.extend_from_slice(&h);
        };
        header(0, 0, 0);
        header(1, strings_at, strings.len());
        header(name_at, body_at, body.len());
        elf[0x28..0x30].copy_from_slice(&(shoff as u64).to_le_bytes());
        elf[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        elf[0x3c..0x3e].copy_from_slice(&3u16.to_le_bytes());
        elf[0x3e..0x40].copy_from_slice(&1u16.to_le_bytes());
        elf
    }

    #[test]
    fn finds_a_section_by_name() {
        let elf = elf_with(".zoraya.command", b"name x\n");
        assert_eq!(section(&elf, ".zoraya.command"), Some(&b"name x\n"[..]));
        assert_eq!(section(&elf, ".other"), None);
        assert_eq!(section(b"not an elf", ".zoraya.command"), None);
        // Truncated anywhere, it gives no section rather than a panic.
        for cut in 0..elf.len() {
            let _ = section(&elf[..cut], ".zoraya.command");
        }
    }
}

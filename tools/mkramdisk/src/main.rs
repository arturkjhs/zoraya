//! Packs files into the initial ramdisk (ADR-0033).
//!
//! Usage: `zoraya-mkramdisk OUTPUT FILE...`. Each file is stored under its
//! base name.

use std::path::Path;
use std::process::ExitCode;

fn run(args: &[String]) -> Result<(), String> {
    let (output, inputs) = args.split_first().ok_or("usage: OUTPUT FILE...")?;
    let mut files = Vec::new();
    for path in inputs {
        let name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(format!("{path}: no usable file name"))?;
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        files.push((name, bytes));
    }
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
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

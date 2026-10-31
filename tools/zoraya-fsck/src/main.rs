//! zoraya-fsck: checks a Zoraya filesystem image (see `zoraya_fs::check`
//! for the list of invariants) and reports every violation.
//!
//! ```text
//! zoraya-fsck [--repair] [--no-data] IMAGE
//! ```
//!
//! Without `--repair` the image is opened read-only and never modified.
//! `--repair` rebuilds the allocator and fixes namespace damage through
//! copy-on-write commits, then checks again; it refuses if the root records
//! or the tree itself are damaged (there is no second copy to repair from,
//! ADR-0012 rule 3). `--no-data` skips reading data blocks.
//!
//! Exit status: 0 clean (or repaired clean), 1 problems found (or left),
//! 2 usage or I/O errors.

use std::path::PathBuf;
use std::process::ExitCode;

use zoraya_fs::check::{CheckOptions, Report, check, repair};
use zoraya_mkfs::{FileDevice, SystemClock};

fn usage() -> ExitCode {
    eprintln!("usage: zoraya-fsck [--repair] [--no-data] IMAGE");
    ExitCode::from(2)
}

fn print(rep: &Report) {
    let s = &rep.stats;
    println!(
        "generation {}, {} blocks ({} used): {} files, {} directories, {} links, {} on the orphan list; tree height {}, \
         {} nodes, {} data blocks",
        s.generation,
        s.block_count,
        s.used_blocks + 8,
        s.files,
        s.directories,
        s.links,
        s.orphans,
        s.tree_height,
        s.tree_nodes,
        s.data_blocks
    );
    for p in &rep.problems {
        println!("problem ({:?}): {}", p.kind, p.what);
    }
}

fn main() -> ExitCode {
    let (mut fix, mut data, mut image) = (false, true, None);
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--repair" => fix = true,
            "--no-data" => data = false,
            "-h" | "--help" => return usage(),
            s if s.starts_with('-') => return usage(),
            _ => image = Some(PathBuf::from(a)),
        }
    }
    let Some(image) = image else { return usage() };
    let mut dev = match FileDevice::open(&image, fix) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("zoraya-fsck: {}: {e}", image.display());
            return ExitCode::from(2);
        }
    };
    let rep = check(&mut dev, CheckOptions { read_data: data });
    print(&rep);
    if rep.is_clean() {
        println!("clean");
        return ExitCode::SUCCESS;
    }
    if !fix {
        println!(
            "{} problems; not modified (use --repair)",
            rep.problems.len()
        );
        return ExitCode::from(1);
    }
    match repair(dev, SystemClock::default()) {
        Ok((r, mut dev)) => {
            println!("repaired: {r:?}");
            let rep = check(&mut dev, CheckOptions { read_data: data });
            print(&rep);
            if rep.is_clean() {
                println!("clean after repair");
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("zoraya-fsck: cannot repair: {e}");
            ExitCode::from(1)
        }
    }
}

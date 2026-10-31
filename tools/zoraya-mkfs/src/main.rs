//! zoraya-mkfs: creates a Zoraya filesystem image (ADR-0010 to ADR-0015),
//! optionally populated from a host directory.
//!
//! ```text
//! zoraya-mkfs --size BYTES [--from DIR] [--volume-id N] IMAGE
//! ```
//!
//! `BYTES` takes a `K`, `M` or `G` suffix. The image is created or
//! truncated. With `--from`, the tree under `DIR` is copied in (see
//! `zoraya_mkfs::populate`), committed, and checked with the same checker
//! as `zoraya-fsck`. Exit status: 0 on success, 1 on failure, 2 on usage
//! errors.

use std::path::PathBuf;
use std::process::ExitCode;

use zoraya_fs::check::{CheckOptions, check};
use zoraya_fs::{MkfsOptions, MountOptions, Volume, mkfs};
use zoraya_mkfs::{Copied, FileDevice, SystemClock, populate, system_time};

fn usage() -> ExitCode {
    eprintln!("usage: zoraya-mkfs --size BYTES[K|M|G] [--from DIR] [--volume-id N] IMAGE");
    ExitCode::from(2)
}

fn parse_size(s: &str) -> Option<u64> {
    let (num, mul) = match s.chars().last()? {
        'K' | 'k' => (&s[..s.len() - 1], 1u64 << 10),
        'M' | 'm' => (&s[..s.len() - 1], 1 << 20),
        'G' | 'g' => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    num.parse::<u64>().ok()?.checked_mul(mul)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (mut size, mut from, mut vid, mut image) = (None, None, None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--size" => size = args.next().and_then(|s| parse_size(&s)),
            "--from" => from = args.next().map(PathBuf::from),
            "--volume-id" => vid = args.next().and_then(|s| s.parse::<u64>().ok()),
            "-h" | "--help" => return usage(),
            s if s.starts_with('-') => return usage(),
            _ => image = Some(PathBuf::from(a)),
        }
    }
    let (Some(size), Some(image)) = (size, image) else {
        return usage();
    };
    match run(size, from, vid, &image) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zoraya-mkfs: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(
    size: u64,
    from: Option<PathBuf>,
    vid: Option<u64>,
    image: &std::path::Path,
) -> Result<(), String> {
    let now = system_time(std::time::SystemTime::now());
    let volume_id = vid.unwrap_or_else(|| {
        (now.secs as u64).rotate_left(20)
            ^ u64::from(now.nanos)
            ^ u64::from(std::process::id()) << 32
    });
    let mut dev =
        FileDevice::create(image, size).map_err(|e| format!("{}: {e}", image.display()))?;
    mkfs(
        &mut dev,
        MkfsOptions {
            volume_id,
            now,
            root_mode: 0o755,
        },
    )
    .map_err(|e| format!("mkfs: {e:?}"))?;
    let mut copied = Copied::default();
    if let Some(src) = &from {
        let mut v = Volume::mount(dev, SystemClock::default(), MountOptions::default())
            .map_err(|e| format!("mount: {e:?}"))?;
        let root = v.root();
        populate(&mut v, root, src, &mut copied)?;
        if let Ok(meta) = std::fs::metadata(src)
            && let Ok(t) = meta.modified()
        {
            v.set_attr(root, None, Some(system_time(t)))
                .map_err(|e| format!("root: {e:?}"))?;
        }
        v.close(root).map_err(|e| format!("{e:?}"))?;
        dev = v.unmount().map_err(|e| format!("commit: {e:?}"))?;
    }
    let rep = check(&mut dev, CheckOptions::default());
    if !rep.is_clean() {
        return Err(format!(
            "the new image does not pass the checker: {:?}",
            rep.problems
        ));
    }
    println!(
        "{}: {} blocks, volume id {volume_id:#x}; {} files ({} bytes), {} directories, {} links",
        image.display(),
        rep.stats.block_count,
        copied.files,
        copied.bytes,
        copied.directories,
        copied.links
    );
    Ok(())
}

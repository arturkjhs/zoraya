//! Host support for the filesystem tools: an image file as a block device,
//! the host clock, and populating a volume from a host directory.
//!
//! `std` only (CLAUDE.md, host-side tools).

use std::fs::{File, OpenOptions as FsOpen};
use std::io;
use std::os::unix::fs::{FileExt, PermissionsExt};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use zoraya_fs::{
    BLOCK_SIZE, Block, BlockDevice, Clock, DevError, FsError, Handle, MAX_ATOMIC_WRITE,
    OpenOptions, SECTOR_SIZE, Timestamp, Volume,
};

/// An image file. Opened read-only, every write fails.
pub struct FileDevice {
    file: File,
    sectors: u64,
    writable: bool,
}

impl FileDevice {
    pub fn open(path: &Path, writable: bool) -> io::Result<FileDevice> {
        let file = FsOpen::new().read(true).write(writable).open(path)?;
        let len = file.metadata()?.len();
        Ok(FileDevice {
            file,
            sectors: len / SECTOR_SIZE as u64,
            writable,
        })
    }

    /// Creates (or truncates) an image of `bytes` bytes, rounded down to
    /// whole blocks.
    pub fn create(path: &Path, bytes: u64) -> io::Result<FileDevice> {
        let file = FsOpen::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        let bytes = bytes / BLOCK_SIZE as u64 * BLOCK_SIZE as u64;
        file.set_len(bytes)?;
        Ok(FileDevice {
            file,
            sectors: bytes / SECTOR_SIZE as u64,
            writable: true,
        })
    }
}

impl BlockDevice for FileDevice {
    fn sector_count(&self) -> u64 {
        self.sectors
    }
    fn flush_supported(&self) -> bool {
        true
    }
    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        let off = block
            .checked_mul(BLOCK_SIZE as u64)
            .ok_or(DevError::IoError)?;
        self.file
            .read_exact_at(buf, off)
            .map_err(|_| DevError::IoError)
    }
    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        if !self.writable {
            return Err(DevError::Unsupported);
        }
        for (block, data) in writes {
            let off = block
                .checked_mul(BLOCK_SIZE as u64)
                .ok_or(DevError::IoError)?;
            self.file
                .write_all_at(&data[..], off)
                .map_err(|_| DevError::IoError)?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), DevError> {
        if !self.writable {
            return Err(DevError::Unsupported);
        }
        self.file.sync_data().map_err(|_| DevError::IoError)
    }
}

/// The host's wall clock and a monotonic clock.
pub struct SystemClock {
    start: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock {
            start: Instant::now(),
        }
    }
}

pub fn system_time(t: SystemTime) -> Timestamp {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => Timestamp {
            secs: d.as_secs() as i64,
            nanos: d.subsec_nanos(),
        },
        Err(e) => {
            let d = e.duration();
            let (s, n) = (d.as_secs() as i64, d.subsec_nanos());
            if n == 0 {
                Timestamp { secs: -s, nanos: 0 }
            } else {
                Timestamp {
                    secs: -s - 1,
                    nanos: 1_000_000_000 - n,
                }
            }
        }
    }
}

impl Clock for SystemClock {
    fn wall(&mut self) -> Timestamp {
        system_time(SystemTime::now())
    }
    fn monotonic_ns(&mut self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
}

/// What [`populate`] copied.
#[derive(Debug, Default)]
pub struct Copied {
    pub files: u64,
    pub directories: u64,
    pub links: u64,
    pub bytes: u64,
}

fn fs_err(path: &Path, e: FsError) -> String {
    format!("{}: {e:?}", path.display())
}

/// Copies the tree under `src` into directory `dir` of the volume: files,
/// directories and symbolic links (as link objects, ADR-0013 rule 7), with
/// their permission bits and modification times (ADR-0014 rule 1). A name
/// the volume does not accept (ADR-0013 rules 1 and 2) is an error naming
/// the path. Hard links become separate files (ADR-0013 rule 5). Other file
/// types are an error.
pub fn populate<D: BlockDevice, C: Clock>(
    v: &mut Volume<D, C>,
    dir: Handle,
    src: &Path,
    out: &mut Copied,
) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(src)
        .map_err(|e| format!("{}: {e}", src.display()))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{}: {e}", src.display()))?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| format!("{}: name is not UTF-8", path.display()))?;
        let meta =
            std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mode = (meta.permissions().mode() & 0o7777) as u16;
        let mtime = meta.modified().ok().map(system_time);
        let ft = meta.file_type();
        if ft.is_symlink() {
            let target =
                std::fs::read_link(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let target = target
                .into_os_string()
                .into_string()
                .map_err(|_| format!("{}: target not UTF-8", path.display()))?;
            v.symlink(dir, &name, &target)
                .map_err(|e| fs_err(&path, e))?;
            out.links += 1;
        } else if ft.is_dir() {
            v.mkdir(dir, &name, mode).map_err(|e| fs_err(&path, e))?;
            let h = v
                .open(dir, &name, OpenOptions::default())
                .map_err(|e| fs_err(&path, e))?;
            populate(v, h, &path, out)?;
            // After the children, so their creation does not move it.
            v.set_attr(h, None, mtime).map_err(|e| fs_err(&path, e))?;
            v.close(h).map_err(|e| fs_err(&path, e))?;
            out.directories += 1;
        } else if ft.is_file() {
            let opts = OpenOptions {
                create: Some(mode),
                exclusive: true,
                ..OpenOptions::default()
            };
            let h = v.open(dir, &name, opts).map_err(|e| fs_err(&path, e))?;
            let f = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut buf = vec![0u8; MAX_ATOMIC_WRITE];
            let mut off = 0u64;
            loop {
                let n = f
                    .read_at(&mut buf, off)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                if n == 0 {
                    break;
                }
                v.write(h, off, &buf[..n]).map_err(|e| fs_err(&path, e))?;
                off += n as u64;
            }
            v.set_attr(h, None, mtime).map_err(|e| fs_err(&path, e))?;
            v.close(h).map_err(|e| fs_err(&path, e))?;
            out.files += 1;
            out.bytes += off;
        } else {
            return Err(format!(
                "{}: not a file, directory or symbolic link",
                path.display()
            ));
        }
    }
    Ok(())
}

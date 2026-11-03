//! `struct stat` from what the service stores (ADR-0103 item 10, ADR-0014):
//! `st_ino` the object's id, `st_dev` its volume, `st_nlink` 1, `st_atime`
//! = `st_mtime`, the session's uid and gid, the mode from `posix_mode` and the
//! kind. The layout is musl's for AArch64 (`arch/aarch64/bits/stat.h`).

#![forbid(unsafe_code)]

use zoraya_proto_file::kind;

pub const S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFCHR: u32 = 0o020000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFIFO: u32 = 0o010000;
pub const S_IFLNK: u32 = 0o120000;
/// The bits `posix_mode` stores (ADR-0014 rule 1).
pub const MODE_BITS: u32 = 0o7777;

/// `sizeof(struct stat)` on AArch64.
pub const SIZE: usize = 128;
/// `st_blksize`: the filesystem's block.
pub const BLOCK: u64 = 4096;

/// What an object is, as `struct stat` tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    File,
    Directory,
    Link,
    /// A pipe or FIFO: a stream.
    Fifo,
    /// `/dev/null` or a terminal.
    Character,
}

impl Kind {
    /// The file protocol's kind; anything else is a file.
    pub fn of_wire(k: u32) -> Kind {
        match k {
            kind::DIRECTORY => Kind::Directory,
            kind::LINK => Kind::Link,
            _ => Kind::File,
        }
    }

    pub fn type_bits(self) -> u32 {
        match self {
            Kind::File => S_IFREG,
            Kind::Directory => S_IFDIR,
            Kind::Link => S_IFLNK,
            Kind::Fifo => S_IFIFO,
            Kind::Character => S_IFCHR,
        }
    }

    /// `dirent`'s `d_type` (`DT_REG`, `DT_DIR`, ...).
    pub fn dirent_type(self) -> u8 {
        (self.type_bits() >> 12) as u8
    }
}

/// One object's metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Meta {
    pub volume: u64,
    pub id: u64,
    pub size: u64,
    pub modified: (i64, u32),
    pub changed: (i64, u32),
    /// `posix_mode`: permission, set-id and sticky bits.
    pub mode: u32,
}

/// The `struct stat` bytes for an object of `kind`, owned by `uid`/`gid`.
pub fn encode(kind: Kind, m: &Meta, uid: u32, gid: u32) -> [u8; SIZE] {
    let mut out = [0u8; SIZE];
    let mut put = |at: usize, bytes: &[u8]| out[at..at + bytes.len()].copy_from_slice(bytes);
    put(0, &m.volume.to_le_bytes());
    put(8, &m.id.to_le_bytes());
    put(16, &(kind.type_bits() | (m.mode & MODE_BITS)).to_le_bytes());
    put(20, &1u32.to_le_bytes());
    put(24, &uid.to_le_bytes());
    put(28, &gid.to_le_bytes());
    // st_rdev and __pad stay zero.
    let size = m.size.min(i64::MAX as u64);
    put(48, &size.to_le_bytes());
    put(56, &(BLOCK as u32).to_le_bytes());
    // st_blocks in 512-byte units, from the size: ADR-0014 rule 9's blocks
    // the service does not report (a stated deviation for sparse files).
    put(
        64,
        &size
            .div_ceil(BLOCK)
            .saturating_mul(BLOCK / 512)
            .to_le_bytes(),
    );
    let time = |seconds: i64, nanos: u32| {
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&seconds.to_le_bytes());
        b[8..].copy_from_slice(&i64::from(nanos).to_le_bytes());
        b
    };
    // ADR-0014 rule 4: no access time, so st_atime is st_mtime.
    put(72, &time(m.modified.0, m.modified.1));
    put(88, &time(m.modified.0, m.modified.1));
    put(104, &time(m.changed.0, m.changed.1));
    out
}

/// ADR-0014 rule 6: the owner bit a POSIX program's access needs, which
/// `libposix` applies as for a file's owner who is not root.
pub fn owner_allows(mode: u32, read: bool, write: bool, execute: bool) -> bool {
    (!read || mode & 0o400 != 0) && (!write || mode & 0o200 != 0) && (!execute || mode & 0o100 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn u64_at(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }

    /// The fields at musl's offsets: dev 0, ino 8, mode 16, nlink 20, uid 24,
    /// gid 28, rdev 32, size 48, blksize 56, blocks 64, atim 72, mtim 88,
    /// ctim 104.
    #[test]
    fn the_fields_are_where_musl_reads_them() {
        let m = Meta {
            volume: 0x11,
            id: 0x22,
            size: 5000,
            modified: (-3, 999_999_999),
            changed: (1_700_000_000, 5),
            mode: 0o104755,
        };
        let b = encode(Kind::File, &m, 1000, 100);
        assert_eq!(u64_at(&b, 0), 0x11);
        assert_eq!(u64_at(&b, 8), 0x22);
        assert_eq!(
            u32_at(&b, 16),
            S_IFREG | 0o4755,
            "type bits are the kind's, not the store's"
        );
        assert_eq!(
            (u32_at(&b, 20), u32_at(&b, 24), u32_at(&b, 28)),
            (1, 1000, 100)
        );
        assert_eq!(u64_at(&b, 32), 0);
        assert_eq!(u64_at(&b, 48), 5000);
        assert_eq!(u32_at(&b, 56), 4096);
        assert_eq!(u64_at(&b, 64), 16, "two blocks of 4 KiB in 512-byte units");
        assert_eq!(u64_at(&b, 72) as i64, -3);
        assert_eq!(u64_at(&b, 80), 999_999_999);
        assert_eq!(&b[72..88], &b[88..104], "atime is mtime");
        assert_eq!(u64_at(&b, 104), 1_700_000_000);
        assert_eq!(u64_at(&b, 112), 5);
        assert_eq!(&b[120..], &[0; 8]);
        let empty = encode(Kind::Directory, &Meta::default(), 0, 0);
        assert_eq!(u32_at(&empty, 16), S_IFDIR);
        assert_eq!(u64_at(&empty, 64), 0);
        let huge = Meta {
            size: u64::MAX,
            ..Meta::default()
        };
        assert_eq!(
            u64_at(&encode(Kind::File, &huge, 0, 0), 48),
            i64::MAX as u64
        );
    }

    #[test]
    fn kinds_and_dirent_types() {
        assert_eq!(Kind::of_wire(kind::FILE), Kind::File);
        assert_eq!(Kind::of_wire(kind::DIRECTORY), Kind::Directory);
        assert_eq!(Kind::of_wire(kind::LINK), Kind::Link);
        assert_eq!(Kind::of_wire(7), Kind::File);
        let dt = [
            (Kind::File, 8),
            (Kind::Directory, 4),
            (Kind::Link, 10),
            (Kind::Fifo, 1),
            (Kind::Character, 2),
        ];
        for (k, t) in dt {
            assert_eq!(k.dirent_type(), t, "{k:?}");
            assert_eq!(k.type_bits() & S_IFMT, k.type_bits());
        }
    }

    #[test]
    fn owner_bits_decide_each_access_alone() {
        for mode in 0..0o1000u32 {
            for (r, w, x) in [
                (true, false, false),
                (false, true, false),
                (false, false, true),
            ] {
                let want = (r && mode & 0o400 != 0)
                    || (w && mode & 0o200 != 0)
                    || (x && mode & 0o100 != 0);
                assert_eq!(owner_allows(mode, r, w, x), want, "{mode:o}");
            }
            assert!(
                owner_allows(mode, false, false, false),
                "existence needs no bit"
            );
            assert_eq!(owner_allows(mode, true, true, false), mode & 0o600 == 0o600);
        }
    }
}

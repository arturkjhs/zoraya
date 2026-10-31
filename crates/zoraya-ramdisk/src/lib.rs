//! The initial ramdisk format (ADR-0033).
//!
//! ```text
//! magic "ZRD1"    4 bytes
//! count           u32
//! count entries of ENTRY_SIZE bytes:
//!   name          NAME_SIZE bytes of UTF-8, zero padded
//!   offset, size  u32 each, from the start of the ramdisk
//! file bytes
//! ```
//!
//! All integers are little-endian. The reader is total: any input gives a
//! result, never a panic. [`Ramdisk::new`] checks every entry.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

const MAGIC: &[u8; 4] = b"ZRD1";
const HEADER_SIZE: usize = 8;
/// Longest name in bytes.
pub const NAME_SIZE: usize = 32;
const ENTRY_SIZE: usize = NAME_SIZE + 8;

/// Why bytes are not a ramdisk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    BadMagic,
    Truncated,
    BadName,
    /// A file lies outside the ramdisk.
    BadRange,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

/// A validated ramdisk.
#[derive(Debug, Clone, Copy)]
pub struct Ramdisk<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> Ramdisk<'a> {
    /// Validates `bytes`. Empty input is an empty ramdisk, which is what a
    /// kernel built with no user programs has.
    pub fn new(bytes: &'a [u8]) -> Result<Ramdisk<'a>, Error> {
        if bytes.is_empty() {
            return Ok(Ramdisk { bytes, count: 0 });
        }
        if bytes.get(..4).is_none_or(|m| m != MAGIC) {
            return Err(Error::BadMagic);
        }
        let count = u32_at(bytes, 4).ok_or(Error::Truncated)? as usize;
        let table = count
            .checked_mul(ENTRY_SIZE)
            .and_then(|t| t.checked_add(HEADER_SIZE))
            .ok_or(Error::Truncated)?;
        if table > bytes.len() {
            return Err(Error::Truncated);
        }
        let disk = Ramdisk { bytes, count };
        for i in 0..count {
            disk.entry(i)?;
        }
        Ok(disk)
    }

    fn entry(&self, index: usize) -> Result<(&'a str, &'a [u8]), Error> {
        let at = HEADER_SIZE + index * ENTRY_SIZE;
        let raw = self
            .bytes
            .get(at..at + ENTRY_SIZE)
            .ok_or(Error::Truncated)?;
        let (name, numbers) = raw.split_at(NAME_SIZE);
        let len = name.iter().position(|b| *b == 0).unwrap_or(NAME_SIZE);
        if len == 0 || name[len..].iter().any(|b| *b != 0) {
            return Err(Error::BadName);
        }
        let name = core::str::from_utf8(&name[..len]).map_err(|_| Error::BadName)?;
        let offset = u32_at(numbers, 0).ok_or(Error::Truncated)? as usize;
        let size = u32_at(numbers, 4).ok_or(Error::Truncated)? as usize;
        let end = offset.checked_add(size).ok_or(Error::BadRange)?;
        let file = self.bytes.get(offset..end).ok_or(Error::BadRange)?;
        Ok((name, file))
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Every file, in order.
    pub fn files(&self) -> impl Iterator<Item = (&'a str, &'a [u8])> + '_ {
        (0..self.count).filter_map(|i| self.entry(i).ok())
    }

    /// The file called `name`.
    pub fn find(&self, name: &str) -> Option<&'a [u8]> {
        self.files().find(|(n, _)| *n == name).map(|(_, f)| f)
    }
}

/// Builds a ramdisk. `None` if a name is empty, too long or has a zero byte,
/// or the result would not fit in `u32` offsets.
#[cfg(feature = "std")]
pub fn pack(files: &[(&str, &[u8])]) -> Option<std::vec::Vec<u8>> {
    let mut out = std::vec::Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&u32::try_from(files.len()).ok()?.to_le_bytes());
    let mut offset = HEADER_SIZE + files.len() * ENTRY_SIZE;
    for (name, file) in files {
        if name.is_empty() || name.len() > NAME_SIZE || name.as_bytes().contains(&0) {
            return None;
        }
        let mut field = [0; NAME_SIZE];
        field[..name.len()].copy_from_slice(name.as_bytes());
        out.extend_from_slice(&field);
        out.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
        out.extend_from_slice(&u32::try_from(file.len()).ok()?.to_le_bytes());
        offset = offset.checked_add(file.len())?;
    }
    for (_, file) in files {
        out.extend_from_slice(file);
    }
    Some(out)
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    fn sample() -> std::vec::Vec<u8> {
        pack(&[("hello", b"abc"), ("empty", b""), ("spin", b"0123456789")]).unwrap()
    }

    #[test]
    fn round_trip() {
        let bytes = sample();
        let d = Ramdisk::new(&bytes).unwrap();
        assert_eq!(d.len(), 3);
        assert_eq!(d.find("hello"), Some(&b"abc"[..]));
        assert_eq!(d.find("empty"), Some(&b""[..]));
        assert_eq!(d.find("spin"), Some(&b"0123456789"[..]));
        assert_eq!(d.find("hell"), None);
        assert_eq!(d.files().count(), 3);
    }

    #[test]
    fn empty_input_is_an_empty_ramdisk() {
        assert!(Ramdisk::new(&[]).unwrap().is_empty());
        assert!(Ramdisk::new(&pack(&[]).unwrap()).unwrap().is_empty());
    }

    #[test]
    fn bad_names_are_not_packed() {
        assert_eq!(pack(&[("", b"")]), None);
        assert_eq!(pack(&[("a\0b", b"")]), None);
        assert!(pack(&[(&"x".repeat(32), b"")]).is_some());
        assert_eq!(pack(&[(&"x".repeat(33), b"")]), None);
    }

    #[test]
    fn every_truncation_is_handled() {
        let bytes = sample();
        for n in 1..bytes.len() {
            // Must not panic; a cut in the file bytes is a range error.
            assert!(Ramdisk::new(&bytes[..n]).is_err(), "cut at {n}");
        }
    }

    #[test]
    fn bad_headers() {
        let mut b = sample();
        b[0] = b'X';
        assert_eq!(Ramdisk::new(&b).unwrap_err(), Error::BadMagic);
        let mut b = sample();
        b[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Ramdisk::new(&b).unwrap_err(), Error::Truncated);
        let mut b = sample();
        b[HEADER_SIZE + NAME_SIZE + 4..HEADER_SIZE + NAME_SIZE + 8]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Ramdisk::new(&b).unwrap_err(), Error::BadRange);
        let mut b = sample();
        b[HEADER_SIZE] = 0xff;
        assert_eq!(Ramdisk::new(&b).unwrap_err(), Error::BadName);
        let mut b = sample();
        b[HEADER_SIZE + 10] = b'x'; // bytes after the terminating zero
        assert_eq!(Ramdisk::new(&b).unwrap_err(), Error::BadName);
    }

    /// INV-0033-1: random mutations, deterministically.
    #[test]
    fn mutations_never_panic() {
        let base = sample();
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            x.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        for _ in 0..100_000 {
            let mut b = base.clone();
            for _ in 0..1 + next() % 8 {
                let i = (next() % b.len() as u64) as usize;
                b[i] = next() as u8;
            }
            if let Ok(d) = Ramdisk::new(&b) {
                for (name, file) in d.files() {
                    assert_eq!(d.find(name).map(|_| ()), Some(()));
                    let _ = file.len();
                }
            }
        }
    }
}

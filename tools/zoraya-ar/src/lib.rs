//! The GNU `ar` format that `rust-lld` and tcc read (ADR-0109 item 3).
//!
//! An archive is `!<arch>\n` and then members, each a 60-byte header and its
//! bytes, padded to an even length. Two special members come first: `/`, the
//! symbol index (a big-endian count, that many big-endian offsets of member
//! headers, then the names, each ended by NUL), and `//`, the table of names
//! longer than 15 bytes, each ended by `/\n` and named in its member's header
//! as `/offset`. Everything written is deterministic: dates, owners and groups
//! are 0 and the mode is 644, so two builds of the same objects give the same
//! bytes.
//!
//! The index lists every global or weak symbol an ELF relocatable member
//! defines, in member order and then symbol table order, as GNU `ar` does.

#![forbid(unsafe_code)]

pub mod elf;

/// The magic string an archive starts with.
pub const MAGIC: &[u8] = b"!<arch>\n";
const HEADER: usize = 60;
const SHORT_NAME: usize = 15;

/// One member: its name and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub data: Vec<u8>,
}

/// Why an archive was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    BadMagic,
    Truncated,
    BadHeader(usize),
    BadName(usize),
}

impl core::fmt::Display for ReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReadError::BadMagic => write!(f, "not an archive"),
            ReadError::Truncated => write!(f, "truncated archive"),
            ReadError::BadHeader(at) => write!(f, "bad member header at offset {at}"),
            ReadError::BadName(at) => write!(f, "bad member name at offset {at}"),
        }
    }
}

fn field(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes)
        .unwrap_or("")
        .trim_end_matches(' ')
}

/// Reads the ordinary members of an archive, leaving out the index and the
/// name table.
pub fn read(archive: &[u8]) -> Result<Vec<Member>, ReadError> {
    if !archive.starts_with(MAGIC) {
        return Err(ReadError::BadMagic);
    }
    let mut at = MAGIC.len();
    let mut names: &[u8] = &[];
    let mut members = Vec::new();
    while at < archive.len() {
        let header = archive.get(at..at + HEADER).ok_or(ReadError::Truncated)?;
        if &header[58..60] != b"`\n" {
            return Err(ReadError::BadHeader(at));
        }
        let size: usize = field(&header[48..58])
            .parse()
            .map_err(|_| ReadError::BadHeader(at))?;
        let start = at + HEADER;
        let end = start.checked_add(size).ok_or(ReadError::Truncated)?;
        let data = archive.get(start..end).ok_or(ReadError::Truncated)?;
        let raw = field(&header[..16]);
        if raw == "/" || raw == "/SYM64/" {
            // The index: rebuilt on every write.
        } else if raw == "//" {
            names = data;
        } else {
            let name = if let Some(offset) = raw.strip_prefix('/') {
                let offset: usize = offset.parse().map_err(|_| ReadError::BadName(at))?;
                let rest = names.get(offset..).ok_or(ReadError::BadName(at))?;
                let len = rest
                    .windows(2)
                    .position(|w| w == b"/\n")
                    .ok_or(ReadError::BadName(at))?;
                core::str::from_utf8(&rest[..len]).map_err(|_| ReadError::BadName(at))?
            } else {
                raw.strip_suffix('/').unwrap_or(raw)
            };
            members.push(Member {
                name: name.to_string(),
                data: data.to_vec(),
            });
        }
        at = end + (size & 1);
    }
    Ok(members)
}

fn header(out: &mut Vec<u8>, name: &str, size: usize) {
    let start = out.len();
    out.extend_from_slice(
        format!("{name:<16}{:<12}{:<6}{:<6}{:<8}{size:<10}`\n", 0, 0, 0, 644).as_bytes(),
    );
    debug_assert_eq!(out.len() - start, HEADER);
}

fn pad(out: &mut Vec<u8>) {
    if out.len() % 2 == 1 {
        out.push(b'\n');
    }
}

/// Writes `members` as an archive with a symbol index. Fails only when a
/// member would start beyond the index's 32-bit offsets.
pub fn write(members: &[Member]) -> Result<Vec<u8>, String> {
    // The name table and each member's header name.
    let mut table = Vec::new();
    let mut header_names = Vec::with_capacity(members.len());
    for m in members {
        if m.name.len() > SHORT_NAME || m.name.contains(' ') || m.name.contains('/') {
            header_names.push(format!("/{}", table.len()));
            table.extend_from_slice(m.name.as_bytes());
            table.extend_from_slice(b"/\n");
        } else {
            header_names.push(format!("{}/", m.name));
        }
    }

    let symbols: Vec<Vec<String>> = members
        .iter()
        .map(|m| elf::defined_globals(&m.data))
        .collect();
    let count: usize = symbols.iter().map(Vec::len).sum();
    let mut out = MAGIC.to_vec();
    if members.is_empty() {
        return Ok(out);
    }

    // The index's size, so that member offsets are known before it is written.
    let names_len: usize = symbols.iter().flatten().map(|s| s.len() + 1).sum();
    let index_size = 4 + 4 * count + names_len;
    let mut offset = MAGIC.len() + HEADER + index_size + (index_size & 1);
    if !table.is_empty() {
        offset += HEADER + table.len() + (table.len() & 1);
    }
    let mut offsets = Vec::with_capacity(members.len());
    for m in members {
        offsets.push(offset);
        offset += HEADER + m.data.len() + (m.data.len() & 1);
    }

    header(&mut out, "/", index_size);
    let too_large = || "the archive is larger than its index can address (4 GiB)".to_string();
    out.extend_from_slice(&u32::try_from(count).map_err(|_| too_large())?.to_be_bytes());
    for (i, syms) in symbols.iter().enumerate() {
        let at = u32::try_from(offsets[i]).map_err(|_| too_large())?;
        for _ in syms {
            out.extend_from_slice(&at.to_be_bytes());
        }
    }
    for name in symbols.iter().flatten() {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
    }
    pad(&mut out);
    if !table.is_empty() {
        header(&mut out, "//", table.len());
        out.extend_from_slice(&table);
        pad(&mut out);
    }
    for (m, name) in members.iter().zip(&header_names) {
        header(&mut out, name, m.data.len());
        out.extend_from_slice(&m.data);
        pad(&mut out);
    }
    Ok(out)
}

/// Inserts `new` into `members`, replacing a member of the same name in place
/// and appending the others, as `ar r` does.
pub fn replace(members: &mut Vec<Member>, new: Member) {
    if let Some(m) = members.iter_mut().find(|m| m.name == new.name) {
        *m = new;
    } else {
        members.push(new);
    }
}

#[cfg(test)]
mod tests;

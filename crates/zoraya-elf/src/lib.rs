//! Parser for the programs the kernel loads (ADR-0033): little-endian, 64-bit,
//! AArch64, static `ET_EXEC`. Field offsets are from the ELF-64 specification.
//!
//! The parser is total: any input gives a result, never a panic, and
//! everything is checked before [`Program::parse`] returns. It refuses a
//! segment that is writable and executable, segments that share a page,
//! and anything outside the given address range.

#![no_std]
#![forbid(unsafe_code)]

/// Page size segments are laid out for.
pub const PAGE_SIZE: u64 = 4096;
/// Most `PT_LOAD` segments accepted.
pub const MAX_SEGMENTS: usize = 8;

const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;
const MAX_PHDRS: usize = 32;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_AARCH64: u16 = 183;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// Why a file was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Shorter than its headers say.
    Truncated,
    /// Not an ELF file.
    BadMagic,
    /// Not 64-bit little-endian AArch64 static `ET_EXEC`.
    Unsupported,
    /// Program headers of an unexpected size or number.
    BadHeaders,
    /// No `PT_LOAD` segment, or more than [`MAX_SEGMENTS`].
    BadSegmentCount,
    /// Not readable, `memsz < filesz`, unaligned, or file bytes outside the
    /// file. User pages are always readable, so a segment must say so.
    BadSegment,
    /// Writable and executable (CP-16).
    WritableExecutable,
    /// Outside the allowed address range.
    OutOfRange,
    /// Two segments touch the same page, or are out of order.
    Overlap,
    /// The entry point is not in an executable segment.
    BadEntry,
}

/// One `PT_LOAD` segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Segment {
    pub vaddr: u64,
    pub memsz: u64,
    pub offset: u64,
    pub filesz: u64,
    pub write: bool,
    pub execute: bool,
}

impl Segment {
    /// First and one-past-last page addresses.
    pub const fn pages(&self) -> (u64, u64) {
        let start = self.vaddr - self.vaddr % PAGE_SIZE;
        // Checked against the range in `parse`, so this cannot overflow.
        let end = (self.vaddr + self.memsz).next_multiple_of(PAGE_SIZE);
        (start, end)
    }
}

/// A checked program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Program {
    pub entry: u64,
    segments: [Segment; MAX_SEGMENTS],
    count: usize,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

impl Program {
    pub fn segments(&self) -> &[Segment] {
        &self.segments[..self.count]
    }

    /// Checks `file`, whose loaded addresses must lie in `[low, high)`.
    pub fn parse(file: &[u8], low: u64, high: u64) -> Result<Program, Error> {
        let e = file.get(..EHDR_SIZE).ok_or(Error::Truncated)?;
        if e[..4] != *b"\x7fELF" {
            return Err(Error::BadMagic);
        }
        let machine = u16_at(e, 18).ok_or(Error::Truncated)?;
        let kind = u16_at(e, 16).ok_or(Error::Truncated)?;
        if e[4] != ELFCLASS64
            || e[5] != ELFDATA2LSB
            || e[6] != EV_CURRENT
            || kind != ET_EXEC
            || machine != EM_AARCH64
        {
            return Err(Error::Unsupported);
        }
        let entry = u64_at(e, 24).ok_or(Error::Truncated)?;
        let phoff = u64_at(e, 32).ok_or(Error::Truncated)?;
        let phentsize = u16_at(e, 54).ok_or(Error::Truncated)? as usize;
        let phnum = u16_at(e, 56).ok_or(Error::Truncated)? as usize;
        if phentsize != PHDR_SIZE || phnum > MAX_PHDRS {
            return Err(Error::BadHeaders);
        }
        let phoff = usize::try_from(phoff).map_err(|_| Error::Truncated)?;
        let table_end = phoff
            .checked_add(phnum * PHDR_SIZE)
            .ok_or(Error::Truncated)?;
        let table = file.get(phoff..table_end).ok_or(Error::Truncated)?;

        let mut program = Program {
            entry,
            segments: [Segment::default(); MAX_SEGMENTS],
            count: 0,
        };
        let mut previous_end = low;
        for ph in table.as_chunks::<PHDR_SIZE>().0 {
            if u32_at(ph, 0) != Some(PT_LOAD) {
                continue;
            }
            let segment = read_segment(ph, file.len() as u64, low, high)?;
            let (start, end) = segment.pages();
            if start < previous_end {
                return Err(Error::Overlap);
            }
            previous_end = end;
            let slot = program
                .segments
                .get_mut(program.count)
                .ok_or(Error::BadSegmentCount)?;
            *slot = segment;
            program.count += 1;
        }
        if program.count == 0 {
            return Err(Error::BadSegmentCount);
        }
        let in_text = program.segments().iter().any(|s| {
            s.execute && entry >= s.vaddr && entry - s.vaddr < s.memsz && entry.is_multiple_of(4)
        });
        if !in_text {
            return Err(Error::BadEntry);
        }
        Ok(program)
    }
}

fn read_segment(ph: &[u8], file_len: u64, low: u64, high: u64) -> Result<Segment, Error> {
    let flags = u32_at(ph, 4).ok_or(Error::Truncated)?;
    let offset = u64_at(ph, 8).ok_or(Error::Truncated)?;
    let vaddr = u64_at(ph, 16).ok_or(Error::Truncated)?;
    let filesz = u64_at(ph, 32).ok_or(Error::Truncated)?;
    let memsz = u64_at(ph, 40).ok_or(Error::Truncated)?;
    let (write, execute) = (flags & PF_W != 0, flags & PF_X != 0);
    if write && execute {
        return Err(Error::WritableExecutable);
    }
    if flags & PF_R == 0 || memsz == 0 || memsz < filesz {
        return Err(Error::BadSegment);
    }
    if vaddr % PAGE_SIZE != offset % PAGE_SIZE {
        return Err(Error::BadSegment);
    }
    if offset.checked_add(filesz).is_none_or(|e| e > file_len) {
        return Err(Error::BadSegment);
    }
    let rounded_end = vaddr
        .checked_add(memsz)
        .and_then(|e| e.checked_next_multiple_of(PAGE_SIZE))
        .ok_or(Error::OutOfRange)?;
    if vaddr < low || rounded_end > high {
        return Err(Error::OutOfRange);
    }
    Ok(Segment {
        vaddr,
        memsz,
        offset,
        filesz,
        write,
        execute,
    })
}

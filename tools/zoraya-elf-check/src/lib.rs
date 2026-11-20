//! The host check of INV-0109-1: a program will load on Zoraya. It runs the
//! loader's own parser, `zoraya-elf`, with the range the kernel gives it
//! (`kernel/src/process.rs`: from `USER_LOW` to one page below the startup
//! block), so W^X, the ranges, overlaps and the entry point are checked by the
//! same code as at `spawn`; then it refuses what makes a program not static:
//! a `PT_INTERP` or `PT_DYNAMIC` program header (ADR-0033, ADR-0109).

// Only the capped allocator (capped.rs) has `unsafe`; everything else is
// refused by this lint.
#![deny(unsafe_code)]

use zoraya_elf::{Program, Segment};

/// `kernel/src/memory/user.rs`'s `USER_LOW` (ADR-0031): the kernel crate is
/// not a host crate, so the value is repeated here.
pub const USER_LOW: u64 = 0x40_0000;
/// The end of the range the kernel's loader accepts.
pub const USER_HIGH: u64 = zoraya_abi::spawn::STARTUP_BLOCK - zoraya_elf::PAGE_SIZE;

const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;

/// Why a program would not load, or is not static.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Loader(zoraya_elf::Error),
    Interpreter,
    Dynamic,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Refusal::Loader(e) => write!(f, "the loader refuses it: {e:?}"),
            Refusal::Interpreter => write!(f, "it has a PT_INTERP program header"),
            Refusal::Dynamic => write!(f, "it has a PT_DYNAMIC program header"),
        }
    }
}

/// The program header types, read after `zoraya-elf` has checked the table's
/// place and size.
fn header_types(file: &[u8]) -> impl Iterator<Item = u32> + '_ {
    let phoff = file
        .get(32..40)
        .map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])));
    let phnum = file
        .get(56..58)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    (0..usize::from(phnum)).filter_map(move |i| {
        let at = usize::try_from(phoff).ok()?.checked_add(i * 56)?;
        Some(u32::from_le_bytes(file.get(at..at + 4)?.try_into().ok()?))
    })
}

/// Checks one file.
pub fn check(file: &[u8]) -> Result<Program, Refusal> {
    let program = Program::parse(file, USER_LOW, USER_HIGH).map_err(Refusal::Loader)?;
    for t in header_types(file) {
        match t {
            PT_INTERP => return Err(Refusal::Interpreter),
            PT_DYNAMIC => return Err(Refusal::Dynamic),
            _ => {}
        }
    }
    Ok(program)
}

/// The mask and value of an AArch64 `svc #imm` (`0xd4000001 | imm << 5`).
const SVC_MASK: u32 = 0xffe0_001f;
const SVC_VALUE: u32 = 0xd400_0001;

/// The ranges a linker's map (`-Wl,-Map`, lld's format) attributes to input
/// sections of an archive named `libc.a`: a line of `VMA LMA Size Align`
/// followed by `.../libc.a(member.o):(.section)`. A section the linker
/// discarded is not in the map.
pub fn libc_ranges(map: &str) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for line in map.lines() {
        let mut words = line.split_whitespace();
        let (Some(vma), Some(_lma), Some(size), Some(_align), Some(input)) = (
            words.next().and_then(|w| u64::from_str_radix(w, 16).ok()),
            words.next().and_then(|w| u64::from_str_radix(w, 16).ok()),
            words.next().and_then(|w| u64::from_str_radix(w, 16).ok()),
            words.next().and_then(|w| w.parse::<u64>().ok()),
            words.next(),
        ) else {
            continue;
        };
        // The input section's own line has the member and the section in the
        // file column: `PATH/libc.a(member.o):(.text.name)`.
        let in_libc = input.split_once("libc.a(").is_some_and(|(before, after)| {
            (before.is_empty() || before.ends_with('/')) && after.contains("):(")
        });
        if in_libc && size > 0 {
            out.push((vma, vma.saturating_add(size)));
        }
    }
    out
}

/// The addresses of `svc` instructions in the executable segments of `file`
/// that lie outside `allowed` (INV-0450-2): a Rust program contains no
/// system-call instruction of its own, only libc.a's. Every aligned word of
/// an executable segment is read; a word that is `svc #imm` and not in a range
/// is reported. A constant that looks like one in code is a false positive
/// the linker's map would not clear: none was seen in a Rust program.
pub fn foreign_svc(file: &[u8], allowed: &[(u64, u64)]) -> Result<Vec<u64>, Refusal> {
    let program = check(file)?;
    let mut found = Vec::new();
    for s in program.segments().iter().filter(|s| s.execute) {
        let Some(bytes) = usize::try_from(s.offset)
            .ok()
            .zip(usize::try_from(s.filesz).ok())
            .and_then(|(at, n)| file.get(at..at.checked_add(n)?))
        else {
            continue;
        };
        for (i, word) in bytes.as_chunks::<4>().0.iter().enumerate() {
            let w = u32::from_le_bytes(*word);
            if w & SVC_MASK != SVC_VALUE {
                continue;
            }
            let at = s.vaddr + 4 * i as u64;
            if !allowed.iter().any(|&(lo, hi)| lo <= at && at < hi) {
                found.push(at);
            }
        }
    }
    Ok(found)
}

/// One line describing an accepted program's segments.
pub fn describe(program: &Program) -> String {
    let segs: Vec<String> = program
        .segments()
        .iter()
        .map(|s: &Segment| {
            format!(
                "r{}{} {:#x}+{:#x}",
                if s.write { 'w' } else { '-' },
                if s.execute { 'x' } else { '-' },
                s.vaddr,
                s.memsz
            )
        })
        .collect();
    format!("entry {:#x}, {}", program.entry, segs.join(", "))
}

#[allow(unsafe_code)]
pub mod capped;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: capped::Capped = capped::Capped::new(capped::LIMIT);

#[cfg(test)]
mod tests;

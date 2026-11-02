//! The host check of INV-0109-1: a program will load on Zoraya. It runs the
//! loader's own parser, `zoraya-elf`, with the range the kernel gives it
//! (`kernel/src/process.rs`: from `USER_LOW` to one page below the startup
//! block), so W^X, the ranges, overlaps and the entry point are checked by the
//! same code as at `spawn`; then it refuses what makes a program not static:
//! a `PT_INTERP` or `PT_DYNAMIC` program header (ADR-0033, ADR-0109).

#![forbid(unsafe_code)]

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

#[cfg(test)]
mod tests;

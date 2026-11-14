//! The symbols an ELF relocatable object defines, for the archive's index.
//! Little-endian 64-bit ELF only, which is all the Zoraya toolchain makes; any
//! other member, or a malformed one, contributes no symbol. Total: no input
//! panics.

const SHT_SYMTAB: u32 = 2;
const SHT_NOBITS: u32 = 8;
const SHN_UNDEF: u16 = 0;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;
const STT_SECTION: u8 = 3;
const STT_FILE: u8 = 4;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn u64_at(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u64::from_le_bytes(
        b.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
    .ok()
}

/// Section `i`'s file range.
fn section(elf: &[u8], i: usize) -> Option<(u32, usize, usize, usize)> {
    let shoff = u64_at(elf, 40)?;
    let shentsize = usize::from(u16_at(elf, 58)?);
    let shnum = usize::from(u16_at(elf, 60)?);
    if shentsize != 64 || i >= shnum {
        return None;
    }
    let h = shoff.checked_add(i.checked_mul(64)?)?;
    // The whole header is in the file, so no field offset below overflows.
    elf.get(h..h.checked_add(64)?)?;
    let kind = u32_at(elf, h + 4)?;
    let offset = u64_at(elf, h + 24)?;
    let size = u64_at(elf, h + 32)?;
    let link = u32_at(elf, h + 40)? as usize;
    // A `.bss` (SHT_NOBITS) takes no bytes of the file: its size is memory.
    // Checking it as a file range dropped every symbol of an object whose
    // `.bss` was larger than the file (the call log's `zoraya-calls.o` once
    // libc.a wrapped more than about 640 functions, 9f-text).
    if kind != SHT_NOBITS {
        elf.get(offset..offset.checked_add(size)?)?;
    }
    Some((kind, offset, size, link))
}

/// Names of the global and weak symbols `elf` defines, in table order.
pub fn defined_globals(elf: &[u8]) -> Vec<String> {
    defined(elf).unwrap_or_default()
}

fn defined(elf: &[u8]) -> Option<Vec<String>> {
    // ELF64, little-endian, ET_REL.
    if elf.get(..6)? != b"\x7fELF\x02\x01" || u16_at(elf, 16)? != 1 {
        return None;
    }
    let shnum = usize::from(u16_at(elf, 60)?);
    let mut names = Vec::new();
    for i in 0..shnum {
        let (kind, offset, size, link) = section(elf, i)?;
        if kind != SHT_SYMTAB {
            continue;
        }
        let (str_kind, str_off, str_size, _) = section(elf, link)?;
        // A NOBITS string table has no bytes in the file, and `section`
        // does not check its range: refuse it rather than index past the end.
        if str_kind == SHT_NOBITS {
            return None;
        }
        let strtab = elf.get(str_off..str_off.checked_add(str_size)?)?;
        // Entry 0 is the null symbol.
        for at in (offset..offset + size).step_by(24).skip(1) {
            let sym = elf.get(at..at + 24)?;
            let name = u32_at(sym, 0)? as usize;
            let info = sym[4];
            let shndx = u16_at(sym, 6)?;
            let (bind, kind) = (info >> 4, info & 0xf);
            if !(bind == STB_GLOBAL || bind == STB_WEAK)
                || shndx == SHN_UNDEF
                || kind == STT_SECTION
                || kind == STT_FILE
            {
                continue;
            }
            let rest = strtab.get(name..)?;
            let end = rest.iter().position(|&b| b == 0)?;
            names.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        }
    }
    Some(names)
}

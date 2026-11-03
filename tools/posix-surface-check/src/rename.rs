//! Renaming symbols in an ELF64 relocatable, for ADR-0141's hiding of the
//! names outside the tier list. The new names are appended to the symbol
//! string table, which is moved to the end of the file; each renamed symbol's
//! `st_name` then points at its new name. Nothing else moves, so sections,
//! relocations (which refer to symbols by index) and section names stay as
//! they were. The pinned toolchain's `rust-objcopy` would do the same, but it
//! needs the `llvm-tools` component, which the toolchain's minimal profile
//! does not install.

use std::collections::BTreeMap;

const SHT_SYMTAB: u32 = 2;

fn get<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N], String> {
    let end = at.checked_add(N).ok_or("overflow")?;
    b.get(at..end)
        .ok_or("truncated object".to_string())?
        .try_into()
        .map_err(|_| "?".to_string())
}

fn word(b: &[u8], at: usize) -> Result<usize, String> {
    usize::try_from(u64::from_le_bytes(get::<8>(b, at)?)).map_err(|_| "too large".to_string())
}

/// `elf` with every symbol named by a key of `renames` renamed to its value.
/// An object with no such symbol is returned unchanged.
pub fn rename(elf: &[u8], renames: &BTreeMap<String, String>) -> Result<Vec<u8>, String> {
    if elf.get(..6) != Some(b"\x7fELF\x02\x01") || u16::from_le_bytes(get::<2>(elf, 16)?) != 1 {
        return Err("not a 64-bit little-endian relocatable object".into());
    }
    let shoff = word(elf, 40)?;
    if u16::from_le_bytes(get::<2>(elf, 58)?) != 64 {
        return Err("unexpected section header size".into());
    }
    let shnum = usize::from(u16::from_le_bytes(get::<2>(elf, 60)?));
    let header = |i: usize| -> Result<usize, String> {
        if i >= shnum {
            return Err("bad section index".into());
        }
        let h = shoff.checked_add(i * 64).ok_or("overflow")?;
        // The whole header is in the file, so no field offset overflows.
        match h.checked_add(64) {
            Some(end) if end <= elf.len() => Ok(h),
            _ => Err("section header outside the file".into()),
        }
    };
    let mut out = elf.to_vec();
    for i in 0..shnum {
        let h = header(i)?;
        if u32::from_le_bytes(get::<4>(elf, h + 4)?) != SHT_SYMTAB {
            continue;
        }
        let (sym_off, sym_size) = (word(elf, h + 24)?, word(elf, h + 32)?);
        let link = u32::from_le_bytes(get::<4>(elf, h + 40)?) as usize;
        let sh = header(link)?;
        let (str_off, str_size) = (word(elf, sh + 24)?, word(elf, sh + 32)?);
        let strtab = elf
            .get(str_off..str_off.checked_add(str_size).ok_or("overflow")?)
            .ok_or("string table outside the file")?;
        let mut table = strtab.to_vec();
        let mut placed: BTreeMap<&str, u32> = BTreeMap::new();
        let mut changed = false;
        let end = sym_off.checked_add(sym_size).ok_or("overflow")?;
        if end > elf.len() {
            return Err("symbol table outside the file".into());
        }
        for at in (sym_off..end).step_by(24).skip(1) {
            let name_off = u32::from_le_bytes(get::<4>(elf, at)?) as usize;
            let rest = strtab.get(name_off..).ok_or("bad symbol name")?;
            let len = rest
                .iter()
                .position(|&b| b == 0)
                .ok_or("unterminated name")?;
            let name = core::str::from_utf8(&rest[..len]).map_err(|_| "symbol name is not text")?;
            let Some(new) = renames.get(name) else {
                continue;
            };
            let off = match placed.get(new.as_str()) {
                Some(&o) => o,
                None => {
                    let o = u32::try_from(table.len()).map_err(|_| "string table too large")?;
                    table.extend_from_slice(new.as_bytes());
                    table.push(0);
                    placed.insert(new, o);
                    o
                }
            };
            out[at..at + 4].copy_from_slice(&off.to_le_bytes());
            changed = true;
        }
        if changed {
            while !out.len().is_multiple_of(8) {
                out.push(0);
            }
            let new_off = out.len() as u64;
            out.extend_from_slice(&table);
            out[sh + 24..sh + 32].copy_from_slice(&new_off.to_le_bytes());
            out[sh + 32..sh + 40].copy_from_slice(&(table.len() as u64).to_le_bytes());
        }
    }
    Ok(out)
}

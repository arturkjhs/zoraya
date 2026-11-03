//! A reader of ELF64 little-endian relocatable objects (`ET_REL`): symbols
//! and executable sections, which is what the export check and the `svc` scan
//! need (ADR-0100 item 7). `zoraya-elf` reads only `ET_EXEC` program headers.
//! Total: any input gives a result.

const ET_REL: u16 = 1;
const EM_AARCH64: u16 = 183;
const SHT_SYMTAB: u32 = 2;
const SHF_EXECINSTR: u64 = 4;
const SHT_NOBITS: u32 = 8;
pub const SHN_UNDEF: u16 = 0;
pub const SHN_COMMON: u16 = 0xfff2;
pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;
const STT_SECTION: u8 = 3;
const STT_FILE: u8 = 4;

/// One symbol of the symbol table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub bind: u8,
    pub kind: u8,
    pub shndx: u16,
}

impl Symbol {
    /// A global or weak symbol this object defines.
    pub fn is_defined_global(&self) -> bool {
        (self.bind == STB_GLOBAL || self.bind == STB_WEAK)
            && self.shndx != SHN_UNDEF
            && self.kind != STT_SECTION
            && self.kind != STT_FILE
    }

    /// A strong definition: global (not weak) and in a section, not common.
    pub fn is_strong_definition(&self) -> bool {
        self.is_defined_global() && self.bind == STB_GLOBAL && self.shndx != SHN_COMMON
    }

    /// A reference that some other object must satisfy: undefined and not
    /// weak.
    pub fn is_strong_undefined(&self) -> bool {
        self.bind == STB_GLOBAL && self.shndx == SHN_UNDEF && !self.name.is_empty()
    }
}

/// What the check needs of one object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Object<'a> {
    pub symbols: Vec<Symbol>,
    /// The bytes of every executable section.
    pub code: Vec<&'a [u8]>,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    let end = at.checked_add(2).ok_or("overflow")?;
    Ok(u16::from_le_bytes(
        b.get(at..end)
            .ok_or("truncated")?
            .try_into()
            .map_err(|_| "?")?,
    ))
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    let end = at.checked_add(4).ok_or("overflow")?;
    Ok(u32::from_le_bytes(
        b.get(at..end)
            .ok_or("truncated")?
            .try_into()
            .map_err(|_| "?")?,
    ))
}
fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    let end = at.checked_add(8).ok_or("overflow")?;
    Ok(u64::from_le_bytes(
        b.get(at..end)
            .ok_or("truncated")?
            .try_into()
            .map_err(|_| "?")?,
    ))
}
fn usize_at(b: &[u8], at: usize) -> Result<usize, String> {
    usize::try_from(u64_at(b, at)?).map_err(|_| "too large".to_string())
}

struct Section {
    kind: u32,
    flags: u64,
    offset: usize,
    size: usize,
    link: usize,
}

fn sections(elf: &[u8]) -> Result<Vec<Section>, String> {
    let shoff = usize_at(elf, 40)?;
    if u16_at(elf, 58)? != 64 {
        return Err("unexpected section header size".into());
    }
    let shnum = usize::from(u16_at(elf, 60)?);
    let mut out = Vec::with_capacity(shnum);
    for i in 0..shnum {
        let h = shoff
            .checked_add(i.checked_mul(64).ok_or("overflow")?)
            .ok_or("overflow")?;
        out.push(Section {
            kind: u32_at(elf, h + 4)?,
            flags: u64_at(elf, h + 8)?,
            offset: usize_at(elf, h + 24)?,
            size: usize_at(elf, h + 32)?,
            link: u32_at(elf, h + 40)? as usize,
        });
    }
    Ok(out)
}

fn bytes<'a>(elf: &'a [u8], s: &Section) -> Result<&'a [u8], String> {
    let end = s.offset.checked_add(s.size).ok_or("overflow")?;
    elf.get(s.offset..end)
        .ok_or("section outside the file".to_string())
}

/// Reads an AArch64 `ET_REL` object. Anything else is an error, so that a
/// member the check cannot read is reported rather than skipped.
pub fn read(elf: &[u8]) -> Result<Object<'_>, String> {
    if elf.get(..6) != Some(b"\x7fELF\x02\x01") {
        return Err("not a 64-bit little-endian ELF file".into());
    }
    if u16_at(elf, 16)? != ET_REL || u16_at(elf, 18)? != EM_AARCH64 {
        return Err("not an AArch64 relocatable object".into());
    }
    let secs = sections(elf)?;
    let mut object = Object::default();
    for s in &secs {
        if s.flags & SHF_EXECINSTR != 0 && s.kind != SHT_NOBITS {
            object.code.push(bytes(elf, s)?);
        }
        if s.kind != SHT_SYMTAB {
            continue;
        }
        let strtab = bytes(elf, secs.get(s.link).ok_or("bad symbol string table")?)?;
        let table = bytes(elf, s)?;
        for sym in table.as_chunks::<24>().0.iter().skip(1) {
            let off = u32_at(sym, 0)? as usize;
            let rest = strtab.get(off..).ok_or("bad symbol name")?;
            let len = rest
                .iter()
                .position(|&b| b == 0)
                .ok_or("unterminated name")?;
            object.symbols.push(Symbol {
                name: String::from_utf8_lossy(&rest[..len]).into_owned(),
                bind: sym[4] >> 4,
                kind: sym[4] & 0xf,
                shndx: u16_at(sym, 6)?,
            });
        }
    }
    Ok(object)
}

/// The encoding of `svc #imm16`: `0xd4000001 | imm16 << 5`.
pub fn is_svc(word: u32) -> bool {
    word & 0xffe0_001f == 0xd400_0001
}

/// Offsets of `svc` instructions in the object's code. Instructions are
/// 4-byte aligned in their section.
pub fn svc_offsets(object: &Object<'_>) -> Vec<usize> {
    let mut out = Vec::new();
    for code in &object.code {
        for (i, w) in code.as_chunks::<4>().0.iter().enumerate() {
            if is_svc(u32::from_le_bytes(*w)) {
                out.push(i * 4);
            }
        }
    }
    out
}

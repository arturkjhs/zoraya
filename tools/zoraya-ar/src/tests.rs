use super::*;

/// A minimal ELF64 little-endian relocatable: a null section, `.text`,
/// `.symtab` and `.strtab`. Each symbol is (name, bind, shndx, type).
pub(crate) fn object(symbols: &[(&str, u8, u16, u8)]) -> Vec<u8> {
    let text = [0u8; 8];
    let mut strtab = vec![0u8];
    let mut symtab = vec![0u8; 24];
    for (name, bind, shndx, kind) in symbols {
        let off = strtab.len() as u32;
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
        let mut s = [0u8; 24];
        s[0..4].copy_from_slice(&off.to_le_bytes());
        s[4] = (bind << 4) | kind;
        s[6..8].copy_from_slice(&shndx.to_le_bytes());
        symtab.extend_from_slice(&s);
    }
    let mut elf = vec![0u8; 64];
    elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
    elf[6] = 1;
    elf[16..18].copy_from_slice(&1u16.to_le_bytes());
    elf[18..20].copy_from_slice(&183u16.to_le_bytes());
    let text_off = elf.len();
    elf.extend_from_slice(&text);
    let sym_off = elf.len();
    elf.extend_from_slice(&symtab);
    let str_off = elf.len();
    elf.extend_from_slice(&strtab);
    while !elf.len().is_multiple_of(8) {
        elf.push(0);
    }
    let shoff = elf.len();
    let sections: [(u32, usize, usize, u32); 4] = [
        (0, 0, 0, 0),
        (1, text_off, text.len(), 0),
        (SHT_SYMTAB_FOR_TESTS, sym_off, symtab.len(), 3),
        (3, str_off, strtab.len(), 0),
    ];
    for (kind, off, size, link) in sections {
        let mut h = [0u8; 64];
        h[4..8].copy_from_slice(&kind.to_le_bytes());
        h[24..32].copy_from_slice(&(off as u64).to_le_bytes());
        h[32..40].copy_from_slice(&(size as u64).to_le_bytes());
        h[40..44].copy_from_slice(&link.to_le_bytes());
        elf.extend_from_slice(&h);
    }
    elf[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    elf[58..60].copy_from_slice(&64u16.to_le_bytes());
    elf[60..62].copy_from_slice(&4u16.to_le_bytes());
    elf
}

const SHT_SYMTAB_FOR_TESTS: u32 = 2;
const GLOBAL: u8 = 1;
const WEAK: u8 = 2;
const LOCAL: u8 = 0;
const FUNC: u8 = 2;

fn member(name: &str, data: Vec<u8>) -> Member {
    Member {
        name: name.to_string(),
        data,
    }
}

/// The index of an archive as (offset, name) pairs, read independently.
fn index(archive: &[u8]) -> Vec<(u32, String)> {
    assert_eq!(&archive[8..24], b"/               ");
    let size: usize = field(&archive[56..66]).parse().unwrap();
    let body = &archive[68..68 + size];
    let count = u32::from_be_bytes(body[..4].try_into().unwrap()) as usize;
    let offsets: Vec<u32> = (0..count)
        .map(|i| u32::from_be_bytes(body[4 + 4 * i..8 + 4 * i].try_into().unwrap()))
        .collect();
    let names: Vec<String> = body[4 + 4 * count..]
        .split(|&b| b == 0)
        .take(count)
        .map(|n| String::from_utf8(n.to_vec()).unwrap())
        .collect();
    offsets.into_iter().zip(names).collect()
}

#[test]
fn defined_globals_takes_global_and_weak_definitions_only() {
    let o = object(&[
        ("g", GLOBAL, 1, FUNC),
        ("w", WEAK, 1, FUNC),
        ("l", LOCAL, 1, FUNC),
        ("u", GLOBAL, 0, 0),
        ("c", GLOBAL, 0xfff2, 1),
        ("sec", GLOBAL, 1, 3),
        ("file", GLOBAL, 0xfff1, 4),
    ]);
    assert_eq!(elf::defined_globals(&o), ["g", "w", "c"]);
}

#[test]
fn round_trip_keeps_names_bytes_and_order() {
    let members = vec![
        member("a.o", object(&[("alpha", GLOBAL, 1, FUNC)])),
        member("a_rather_long_member_name.o", b"odd".to_vec()),
        member("b.txt", b"even".to_vec()),
        member("exactly15chars.", b"x".to_vec()),
        member("sixteen_chars.o_", b"y".to_vec()),
    ];
    let bytes = write(&members).unwrap();
    assert_eq!(read(&bytes).unwrap(), members);
    assert_eq!(bytes.len() % 2, 0);
}

#[test]
fn index_points_at_the_defining_member_header() {
    let members = vec![
        member(
            "one.o",
            object(&[("f1", GLOBAL, 1, FUNC), ("f2", WEAK, 1, FUNC)]),
        ),
        member("text", b"no symbols here".to_vec()),
        member("a_long_name_for_two.o", object(&[("f3", GLOBAL, 1, FUNC)])),
    ];
    let bytes = write(&members).unwrap();
    let idx = index(&bytes);
    let names: Vec<&str> = idx.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, ["f1", "f2", "f3"]);
    for (offset, name) in &idx {
        let at = *offset as usize;
        let size: usize = field(&bytes[at + 48..at + 58]).parse().unwrap();
        let data = &bytes[at + 60..at + 60 + size];
        assert!(elf::defined_globals(data).contains(name), "{name} at {at}");
        assert_eq!(&bytes[at + 58..at + 60], b"`\n");
    }
    assert_eq!(idx[0].0, idx[1].0);
    assert_ne!(idx[0].0, idx[2].0);
}

#[test]
fn headers_are_deterministic() {
    let members = vec![member("m.o", object(&[("s", GLOBAL, 1, FUNC)]))];
    let a = write(&members).unwrap();
    assert_eq!(a, write(&members).unwrap());
    let at = a.len() - members[0].data.len() - 60;
    assert_eq!(
        &a[at..at + 48],
        format!("{:<16}{:<12}{:<6}{:<6}{:<8}", "m.o/", 0, 0, 0, 644).as_bytes()
    );
}

#[test]
fn an_empty_archive_is_the_magic_alone() {
    assert_eq!(write(&[]).unwrap(), MAGIC);
    assert_eq!(read(MAGIC).unwrap(), []);
}

#[test]
fn an_archive_without_symbols_has_an_empty_index() {
    let bytes = write(&[member("t", b"abc".to_vec())]).unwrap();
    assert_eq!(index(&bytes), []);
    assert_eq!(read(&bytes).unwrap()[0].data, b"abc");
}

#[test]
fn replace_keeps_position_and_appends_new_names() {
    let mut members = vec![member("a", vec![1]), member("b", vec![2])];
    replace(&mut members, member("a", vec![3]));
    replace(&mut members, member("c", vec![4]));
    assert_eq!(
        members,
        vec![
            member("a", vec![3]),
            member("b", vec![2]),
            member("c", vec![4])
        ]
    );
}

#[test]
fn read_refuses_malformed_archives() {
    assert_eq!(read(b"!<arch\n"), Err(ReadError::BadMagic));
    let good = write(&[member("a_long_member_name.o", b"data".to_vec())]).unwrap();
    assert_eq!(read(&good[..good.len() - 3]), Err(ReadError::Truncated));
    let mut bad = good.clone();
    let last = good.len() - 4 - 2;
    bad[last..last + 2].copy_from_slice(b"xx");
    assert!(matches!(read(&bad), Err(ReadError::BadHeader(_))));
    let mut bad = good.clone();
    let at = good.len() - 4 - 60;
    bad[at..at + 4].copy_from_slice(b"/999");
    assert!(matches!(read(&bad), Err(ReadError::BadName(_))));
}

/// INV-0033-1's discipline for the archiver's own parsers: every truncation
/// and many byte mutations give a result, never a panic.
#[test]
fn parsers_are_total() {
    let archive = write(&[
        member(
            "x.o",
            object(&[("sym", GLOBAL, 1, FUNC), ("w", WEAK, 1, FUNC)]),
        ),
        member("long_long_long_name.o", object(&[("t", GLOBAL, 1, FUNC)])),
    ])
    .unwrap();
    for n in 0..archive.len() {
        let _ = read(&archive[..n]);
    }
    let elf = object(&[("sym", GLOBAL, 1, FUNC)]);
    for n in 0..elf.len() {
        let _ = elf::defined_globals(&elf[..n]);
    }
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..20_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let mut a = archive.clone();
        let mut e = elf.clone();
        for k in 0..4 {
            let r = (seed >> (k * 16)) as usize;
            a[r % archive.len()] = (r >> 8) as u8;
            e[r % elf.len()] = (r >> 8) as u8;
        }
        let _ = read(&a);
        let _ = elf::defined_globals(&e);
    }
}

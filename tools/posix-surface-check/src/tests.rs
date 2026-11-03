use std::collections::BTreeSet;

use crate::ar;
use crate::elf::{self, Object, STB_GLOBAL, STB_LOCAL, STB_WEAK};
use crate::surface::{self, Report};

const FUNC: u8 = 2;
const TEXT: u16 = 1;
const UNDEF: u16 = 0;
const SVC0: u32 = 0xd400_0001;
const NOP: u32 = 0xd503_201f;

/// An ELF64 AArch64 relocatable with one executable `.text` holding `code`
/// and the given symbols (name, bind, shndx).
fn object(symbols: &[(&str, u8, u16)], code: &[u32]) -> Vec<u8> {
    let text: Vec<u8> = code.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut strtab = vec![0u8];
    let mut symtab = vec![0u8; 24];
    for (name, bind, shndx) in symbols {
        let off = strtab.len() as u32;
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
        let mut s = [0u8; 24];
        s[..4].copy_from_slice(&off.to_le_bytes());
        s[4] = (bind << 4) | if *shndx == UNDEF { 0 } else { FUNC };
        s[6..8].copy_from_slice(&shndx.to_le_bytes());
        symtab.extend_from_slice(&s);
    }
    let mut e = vec![0u8; 64];
    e[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    e[16..18].copy_from_slice(&1u16.to_le_bytes());
    e[18..20].copy_from_slice(&183u16.to_le_bytes());
    let place = |e: &mut Vec<u8>, b: &[u8]| {
        while !e.len().is_multiple_of(8) {
            e.push(0);
        }
        let at = e.len();
        e.extend_from_slice(b);
        at
    };
    let t = place(&mut e, &text);
    let s = place(&mut e, &symtab);
    let st = place(&mut e, &strtab);
    let shoff = place(&mut e, &[]);
    // null, .text (PROGBITS, ALLOC|EXECINSTR), .symtab, .strtab
    for (kind, flags, off, size, link) in [
        (0u32, 0u64, 0, 0, 0u32),
        (1, 6, t, text.len(), 0),
        (2, 0, s, symtab.len(), 3),
        (3, 0, st, strtab.len(), 0),
    ] {
        let mut h = [0u8; 64];
        h[4..8].copy_from_slice(&kind.to_le_bytes());
        h[8..16].copy_from_slice(&flags.to_le_bytes());
        h[24..32].copy_from_slice(&(off as u64).to_le_bytes());
        h[32..40].copy_from_slice(&(size as u64).to_le_bytes());
        h[40..44].copy_from_slice(&link.to_le_bytes());
        e.extend_from_slice(&h);
    }
    e[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
    e[58..60].copy_from_slice(&64u16.to_le_bytes());
    e[60..62].copy_from_slice(&4u16.to_le_bytes());
    e
}

fn archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let members: Vec<zoraya_ar::Member> = members
        .iter()
        .map(|(n, d)| zoraya_ar::Member {
            name: n.to_string(),
            data: d.clone(),
        })
        .collect();
    zoraya_ar::write(&members).unwrap()
}

fn list(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// Runs the check on an archive's bytes, through the tool's own readers.
fn check_archive(names: &[&str], bytes: &[u8], allowed: &[&str]) -> Report {
    let members = ar::members(bytes).unwrap();
    let objects: Vec<(String, Object<'_>)> = members
        .iter()
        .map(|m| (m.name.clone(), elf::read(m.data).unwrap()))
        .collect();
    let allowed: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
    let index = ar::index(bytes).unwrap();
    surface::check(&list(names), &objects, &allowed, index.as_deref())
}

/// A small archive that passes against the list `open`, `read`, `_exit`.
fn good() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "open.o",
            object(
                &[
                    ("open", STB_GLOBAL, TEXT),
                    ("__open_helper", STB_GLOBAL, TEXT),
                ],
                &[NOP],
            ),
        ),
        (
            "read.o",
            object(
                &[
                    ("read", STB_WEAK, TEXT),
                    ("open", STB_GLOBAL, UNDEF),
                    ("zoraya_posix_read", STB_GLOBAL, TEXT),
                ],
                &[NOP, NOP],
            ),
        ),
        (
            "exit.o",
            object(
                &[
                    ("_exit", STB_GLOBAL, TEXT),
                    ("local_thing", STB_LOCAL, TEXT),
                ],
                &[NOP],
            ),
        ),
        ("weakref.o", object(&[("optional", STB_WEAK, UNDEF)], &[])),
    ]
}
const NAMES: [&str; 3] = ["open", "read", "_exit"];

#[test]
fn a_good_archive_passes() {
    let r = check_archive(&NAMES, &archive(&good()), &[]);
    assert!(r.passed(), "{r:?}");
}

#[test]
fn an_extra_export_fails() {
    let mut m = good();
    m.push(("extra.o", object(&[("strlcpy", STB_GLOBAL, TEXT)], &[NOP])));
    let r = check_archive(&NAMES, &archive(&m), &[]);
    assert_eq!(r.extra, ["strlcpy"]);
    assert!(!r.passed());
}

#[test]
fn a_weak_extra_export_fails() {
    let mut m = good();
    m.push(("w.o", object(&[("frexpl", STB_WEAK, TEXT)], &[])));
    assert_eq!(check_archive(&NAMES, &archive(&m), &[]).extra, ["frexpl"]);
}

#[test]
fn internal_names_are_not_exports() {
    let mut m = good();
    m.push((
        "i.o",
        object(
            &[
                ("_x", STB_GLOBAL, TEXT),
                ("__y", STB_WEAK, TEXT),
                ("zoraya_posix_start", STB_GLOBAL, TEXT),
            ],
            &[],
        ),
    ));
    assert!(check_archive(&NAMES, &archive(&m), &[]).passed());
}

#[test]
fn a_missing_tier_name_fails() {
    let m: Vec<_> = good().into_iter().filter(|(n, _)| *n != "exit.o").collect();
    let r = check_archive(&NAMES, &archive(&m), &[]);
    assert_eq!(r.missing, ["_exit"]);
    assert!(!r.passed());
}

#[test]
fn a_denied_export_fails_even_when_listed() {
    let mut m = good();
    m.push((
        "d.o",
        object(
            &[
                ("alarm", STB_GLOBAL, TEXT),
                ("epoll_wait", STB_GLOBAL, TEXT),
            ],
            &[],
        ),
    ));
    let mut names = NAMES.to_vec();
    names.push("alarm");
    let r = check_archive(&names, &archive(&m), &[]);
    assert_eq!(r.denied, ["alarm", "epoll_wait"]);
    assert_eq!(r.extra, ["epoll_wait"]);
}

#[test]
fn a_stray_svc_fails() {
    let mut m = good();
    m.push((
        "stray.o",
        object(
            &[("__stray", STB_GLOBAL, TEXT)],
            &[NOP, SVC0, NOP, SVC0 | (0x42 << 5)],
        ),
    ));
    let r = check_archive(&NAMES, &archive(&m), &[]);
    assert_eq!(r.svc, [("stray.o".to_string(), vec![4, 12])]);
    assert!(!r.passed());
}

#[test]
fn svc_in_the_permitted_member_passes() {
    let mut m = good();
    m.push((
        "zoraya_rt-1234.o",
        object(&[("__rt_stub", STB_GLOBAL, TEXT)], &[SVC0]),
    ));
    assert!(check_archive(&NAMES, &archive(&m), &["zoraya_rt-"]).passed());
    assert!(!check_archive(&NAMES, &archive(&m), &["zoraya_posix-"]).passed());
}

#[test]
fn svc_encoding_is_exact() {
    assert!(elf::is_svc(SVC0));
    assert!(elf::is_svc(SVC0 | (0xffff << 5)));
    for not in [
        0xd400_0002,
        0xd400_0003,
        0xd420_0000,
        0xd440_0000,
        NOP,
        0xd500_0001,
        0x5400_0001,
        0xd400_0000,
    ] {
        assert!(!elf::is_svc(not), "{not:#x}");
    }
}

#[test]
fn an_unresolved_reference_fails() {
    let mut m = good();
    m.push(("u.o", object(&[("__lock", STB_GLOBAL, UNDEF)], &[])));
    let r = check_archive(&NAMES, &archive(&m), &[]);
    assert_eq!(r.unresolved, [("__lock".to_string(), "u.o".to_string())]);
}

#[test]
fn duplicate_member_names_fail() {
    let mut m = good();
    m.push(("open.o", object(&[], &[])));
    assert_eq!(
        check_archive(&NAMES, &archive(&m), &[]).duplicates,
        ["open.o"]
    );
}

#[test]
fn hide_renames_exactly_the_unlisted_defined_names() {
    let a = object(
        &[
            ("open", STB_GLOBAL, TEXT),
            ("frexpl", STB_GLOBAL, TEXT),
            ("_x", STB_GLOBAL, TEXT),
        ],
        &[],
    );
    let b = object(
        &[
            ("frexpl", STB_GLOBAL, UNDEF),
            ("sin", STB_WEAK, TEXT),
            ("rust_begin_unwind", STB_GLOBAL, TEXT),
        ],
        &[],
    );
    let c = object(
        &[
            ("zoraya_posix_read", STB_GLOBAL, TEXT),
            ("loc", STB_LOCAL, TEXT),
            ("ext", STB_GLOBAL, UNDEF),
        ],
        &[],
    );
    let objs = [
        elf::read(&a).unwrap(),
        elf::read(&b).unwrap(),
        elf::read(&c).unwrap(),
    ];
    let renames = surface::hide(&list(&NAMES), &objs).unwrap();
    let want: Vec<(String, String)> = ["frexpl", "rust_begin_unwind", "sin"]
        .iter()
        .map(|n| (n.to_string(), format!("__zoraya_{n}")))
        .collect();
    assert_eq!(renames, want);
}

#[test]
fn hide_refuses_a_taken_name() {
    let a = object(
        &[
            ("sin", STB_GLOBAL, TEXT),
            ("__zoraya_sin", STB_GLOBAL, UNDEF),
        ],
        &[],
    );
    assert!(surface::hide(&list(&NAMES), &[elf::read(&a).unwrap()]).is_err());
}

const CSV: &str = include_str!("../../../docs/research/task-02/libc-surface.csv");
const LIST: &str = include_str!("../../../docs/adr/posix-surface-T1.txt");

#[test]
fn the_tier_list_follows_adr_0100_item_7() {
    let names = surface::tier_list(CSV).unwrap();
    assert_eq!(names.len(), 248 - 2 - 4 + 1);
    for gone in surface::STRUCK {
        assert!(!names.iter().any(|n| n == gone), "{gone}");
    }
    for n in [
        "writev",
        "_exit",
        "__assert_fail",
        "__errno_location",
        "environ",
        "stdin",
        "optind",
        "tcc_never",
    ] {
        assert_eq!(names.iter().any(|x| x == n), n != "tcc_never", "{n}");
    }
}

#[test]
fn the_committed_tier_list_is_up_to_date() {
    assert_eq!(
        LIST,
        surface::render_list(&surface::tier_list(CSV).unwrap())
    );
}

#[test]
fn the_exclusion_table_and_the_list_are_disjoint() {
    for n in surface::parse_list(LIST) {
        assert!(!surface::is_denied(&n), "{n}");
    }
}

#[test]
fn csv_quotes_and_errors() {
    let rows = surface::parse_csv("a,b,c\n1,\"x,y\",\"say \"\"hi\"\"\"\n").unwrap();
    assert_eq!(rows[1], ["1", "x,y", "say \"hi\""]);
    assert!(surface::parse_csv("a,b\n1,\"x\n").is_err());
    assert!(surface::parse_csv("a,b\n1,2,3\n").is_err());
    assert!(
        surface::tier_list("symbol,tier\nopen,T1\n").is_err(),
        "struck rows must exist"
    );
}

#[test]
fn readers_are_total() {
    let bytes = archive(&good());
    for n in 0..bytes.len() {
        let _ = ar::members(&bytes[..n]);
    }
    let o = object(
        &[("open", STB_GLOBAL, TEXT), ("x", STB_GLOBAL, UNDEF)],
        &[SVC0, NOP],
    );
    for n in 0..o.len() {
        let _ = elf::read(&o[..n]);
    }
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..20_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let (mut a, mut e) = (bytes.clone(), o.clone());
        for k in 0..4 {
            let r = (seed >> (k * 16)) as usize;
            a[r % bytes.len()] = (r >> 8) as u8;
            e[r % o.len()] = (r >> 8) as u8;
        }
        if let Ok(ms) = ar::members(&a) {
            for m in ms {
                let _ = elf::read(m.data);
            }
        }
        if let Ok(obj) = elf::read(&e) {
            let _ = elf::svc_offsets(&obj);
        }
    }
}

#[test]
fn a_non_relocatable_member_is_an_error() {
    let mut exec = object(&[], &[]);
    exec[16] = 2;
    assert!(elf::read(&exec).is_err());
    assert!(elf::read(b"plain text").is_err());
}

#[test]
fn the_index_must_name_exactly_the_defined_symbols() {
    let bytes = archive(&good());
    let members = ar::members(&bytes).unwrap();
    let objects: Vec<(String, Object<'_>)> = members
        .iter()
        .map(|m| (m.name.clone(), elf::read(m.data).unwrap()))
        .collect();
    let mut index = ar::index(&bytes).unwrap().unwrap();
    assert!(surface::check(&list(&NAMES), &objects, &[], Some(&index)).passed());
    index.retain(|n| n != "read");
    index.push("ghost".into());
    let r = surface::check(&list(&NAMES), &objects, &[], Some(&index));
    assert_eq!(
        r.index,
        [
            "read is defined but not indexed",
            "ghost is indexed but not defined"
        ]
    );
    let r = surface::check(&list(&NAMES), &objects, &[], None);
    assert_eq!(r.index, ["the archive has no symbol index"]);
}

#[test]
fn rename_changes_only_the_named_symbols() {
    use std::collections::BTreeMap;
    let o = object(
        &[
            ("frexpl", STB_GLOBAL, TEXT),
            ("open", STB_GLOBAL, UNDEF),
            ("sin", STB_WEAK, UNDEF),
            ("frexpl2", STB_LOCAL, TEXT),
        ],
        &[NOP, SVC0],
    );
    let map: BTreeMap<String, String> = [("frexpl", "__zoraya_frexpl"), ("sin", "__zoraya_sin")]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let renamed = crate::rename::rename(&o, &map).unwrap();
    let before = elf::read(&o).unwrap();
    let after = elf::read(&renamed).unwrap();
    let names: Vec<&str> = after.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["__zoraya_frexpl", "open", "__zoraya_sin", "frexpl2"]
    );
    for (a, b) in before.symbols.iter().zip(&after.symbols) {
        assert_eq!((a.bind, a.kind, a.shndx), (b.bind, b.kind, b.shndx));
    }
    assert_eq!(before.code, after.code);
    assert_eq!(crate::rename::rename(&o, &BTreeMap::new()).unwrap(), o);
    assert!(crate::rename::rename(b"junk", &map).is_err());
    for n in 0..o.len() {
        let _ = crate::rename::rename(&o[..n], &map);
    }
}

// Holes found by mutation testing (cargo-mutants, sorted by mutant-triage).

#[test]
fn a_small_archive_still_has_its_index() {
    let bytes = archive(&[("t.o", b"abc".to_vec())]);
    assert!(bytes.len() < 488);
    assert_eq!(ar::index(&bytes).unwrap(), Some(vec![]));
    assert_eq!(ar::index(b"!<arch>\n").unwrap(), None);
}

#[test]
fn a_symbol_reads_its_type_apart_from_its_binding() {
    let o = object(
        &[("open", STB_GLOBAL, TEXT), ("ext", STB_WEAK, UNDEF)],
        &[NOP],
    );
    let syms = elf::read(&o).unwrap().symbols;
    assert_eq!((syms[0].bind, syms[0].kind), (STB_GLOBAL, FUNC));
    assert_eq!((syms[1].bind, syms[1].kind), (STB_WEAK, 0));
}

#[test]
fn rename_refuses_a_file_without_the_elf_magic() {
    use std::collections::BTreeMap;
    let mut o = object(&[("open", STB_GLOBAL, TEXT)], &[]);
    o[0] = 0x7e;
    assert!(crate::rename::rename(&o, &BTreeMap::new()).is_err());
}

/// The section header table's offset and the `.symtab` and `.strtab` headers.
fn headers(o: &[u8]) -> (usize, usize, usize) {
    let shoff = u64::from_le_bytes(o[40..48].try_into().unwrap()) as usize;
    (shoff, shoff + 2 * 64, shoff + 3 * 64)
}

#[test]
fn rename_keeps_every_name_of_a_string_table_longer_than_its_symbol_table() {
    use std::collections::BTreeMap;
    let a = "a_forty_character_name_for_a_long_tab_01";
    let b = "a_forty_character_name_for_a_long_tab_02";
    let o = object(&[(a, STB_GLOBAL, TEXT), (b, STB_GLOBAL, TEXT)], &[]);
    let map: BTreeMap<String, String> = [(b.to_string(), "__zoraya_b".to_string())]
        .into_iter()
        .collect();
    let renamed = crate::rename::rename(&o, &map).unwrap();
    let names: Vec<String> = elf::read(&renamed)
        .unwrap()
        .symbols
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, [a, "__zoraya_b"]);
    // The new string table starts on an 8-byte boundary.
    let (_, _, strtab) = headers(&renamed);
    let off = u64::from_le_bytes(renamed[strtab + 24..strtab + 32].try_into().unwrap());
    assert_eq!(off % 8, 0);
}

#[test]
fn a_symbol_table_at_the_end_of_the_file_is_renamed() {
    use std::collections::BTreeMap;
    let mut o = object(&[("frexpl", STB_GLOBAL, TEXT)], &[]);
    let (_, symtab, _) = headers(&o);
    let off = u64::from_le_bytes(o[symtab + 24..symtab + 32].try_into().unwrap()) as usize;
    let size = u64::from_le_bytes(o[symtab + 32..symtab + 40].try_into().unwrap()) as usize;
    let copy = o[off..off + size].to_vec();
    let at = o.len() as u64;
    o.extend_from_slice(&copy);
    o[symtab + 24..symtab + 32].copy_from_slice(&at.to_le_bytes());
    let map: BTreeMap<String, String> = [("frexpl".to_string(), "__zoraya_frexpl".to_string())]
        .into_iter()
        .collect();
    let renamed = crate::rename::rename(&o, &map).unwrap();
    assert_eq!(
        elf::read(&renamed).unwrap().symbols[0].name,
        "__zoraya_frexpl"
    );
}

#[test]
fn a_quote_inside_an_unquoted_field_is_a_character() {
    let rows = surface::parse_csv("a,b\nx\"y,z\n").unwrap();
    assert_eq!(rows[1], ["x\"y", "z"]);
}

#[test]
fn the_list_file_names_are_its_first_words_outside_comments() {
    assert_eq!(
        surface::parse_list("# c\nopen # n\n\n read \nwritev x\n"),
        list(&["open", "read", "writev"])
    );
}

#[test]
fn a_failed_report_says_what_failed() {
    let r = Report {
        extra: vec!["strlcpy".into()],
        denied: vec!["alarm".into()],
        missing: vec!["puts".into()],
        svc: vec![("s.o".into(), vec![4])],
        unresolved: vec![("__lock".into(), "u.o".into())],
        duplicates: vec!["d.o".into()],
        index: vec!["x is indexed but not defined".into()],
        doubly_defined: vec![("strlen".into(), vec!["a.o".into(), "b.o".into()])],
    };
    assert_eq!(
        r.lines(),
        [
            "export outside the tier list (INV-0100-1): strlcpy",
            "export on the exclusion table (INV-0005-2): alarm",
            "tier list name not defined: puts",
            "svc in s.o at code offsets [4] (INV-0100-2)",
            "undefined in the archive: __lock (referenced by u.o)",
            "member name occurs more than once: d.o",
            "defined strongly in more than one member: strlen (a.o, b.o)",
            "symbol index: x is indexed but not defined",
        ]
    );
    assert!(Report::default().lines().is_empty());
}

// Findings of the independent review (docs/tasks/TASK-17-review.md).

#[test]
fn a_name_defined_strongly_twice_fails_but_a_weak_twin_does_not() {
    let mut m = good();
    m.push(("weak_open.o", object(&[("open", STB_WEAK, TEXT)], &[])));
    assert!(check_archive(&NAMES, &archive(&m), &[]).passed());
    m.push(("strong_open.o", object(&[("open", STB_GLOBAL, TEXT)], &[])));
    let r = check_archive(&NAMES, &archive(&m), &[]);
    assert_eq!(
        r.doubly_defined,
        [(
            "open".to_string(),
            vec!["open.o".to_string(), "strong_open.o".to_string()]
        )]
    );
}

#[test]
fn the_exclusion_table_is_inv_0005_2_s() {
    for n in [
        "ptrace",
        "msgget",
        "semget",
        "shmget",
        "socket",
        "getaddrinfo",
        "pthread_create",
        "dlopen",
        "flock",
        "setuid",
        "setgid",
        "setresuid",
        "setresgid",
        "sigaltstack",
        "setitimer",
        "alarm",
        "epoll_create",
        "epoll_wait",
    ] {
        assert!(surface::is_denied(n), "{n}");
    }
    assert!(!surface::is_denied("open") && !surface::is_denied("epoll"));
}

#[test]
fn only_the_shim_s_prefix_is_internal_among_zoraya_names() {
    assert!(surface::is_internal("zoraya_posix_read") && surface::is_internal("_x"));
    assert!(!surface::is_internal("zoraya_fork_group"));
    let mut m = good();
    m.push((
        "z.o",
        object(&[("zoraya_fork_group", STB_GLOBAL, TEXT)], &[]),
    ));
    assert_eq!(
        check_archive(&NAMES, &archive(&m), &[]).extra,
        ["zoraya_fork_group"]
    );
}

#[test]
fn an_index_with_fewer_names_than_its_count_is_an_error() {
    let mut bytes = archive(&good());
    // The count is the index body's first four bytes, after the magic and header.
    let at = 8 + 60;
    let n = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    bytes[at..at + 4].copy_from_slice(&(n + 1000).to_be_bytes());
    assert!(ar::index(&bytes).is_err());
}

#[test]
fn a_section_header_offset_near_the_top_of_the_address_space_is_refused() {
    let mut o = object(&[("open", STB_GLOBAL, TEXT)], &[NOP]);
    o[40..48].copy_from_slice(&0xffff_ffff_ffff_fffeu64.to_le_bytes());
    o[60..62].copy_from_slice(&1u16.to_le_bytes());
    assert!(elf::read(&o).is_err());
    assert!(crate::rename::rename(&o, &std::collections::BTreeMap::new()).is_err());
}

#[test]
fn the_index_must_point_at_the_defining_member() {
    let bytes = archive(&good());
    let members = ar::members(&bytes).unwrap();
    let at: Vec<(usize, String, Object<'_>)> = members
        .iter()
        .map(|m| (m.offset, m.name.clone(), elf::read(m.data).unwrap()))
        .collect();
    let mut entries = ar::index_entries(&bytes).unwrap().unwrap();
    assert!(surface::check_index_offsets(&entries, &at).is_empty());
    let read_at = at.iter().find(|(_, n, _)| n == "read.o").unwrap().0;
    let open_at = at.iter().find(|(_, n, _)| n == "open.o").unwrap().0;
    let i = entries.iter().position(|(_, n)| n == "open").unwrap();
    entries[i].0 = read_at;
    assert_eq!(
        surface::check_index_offsets(&entries, &at),
        [
            "open points at read.o, which does not define it".to_string(),
            format!("open is indexed 0 times for the member at {open_at}, defined 1 times"),
        ]
    );
    entries[i].0 = 3;
    assert_eq!(
        surface::check_index_offsets(&entries, &at),
        [
            "open points at offset 3, which is no member's header".to_string(),
            format!("open is indexed 0 times for the member at {open_at}, defined 1 times"),
        ]
    );
    entries[i].0 = open_at;
    entries.push((open_at, "open".into()));
    assert_eq!(
        surface::check_index_offsets(&entries, &at),
        [format!(
            "open is indexed 2 times for the member at {open_at}, defined 1 times"
        )]
    );
}

#[test]
fn every_member_defining_an_indexed_name_needs_its_entry() {
    let mut m = good();
    m.push(("weak_open.o", object(&[("open", STB_WEAK, TEXT)], &[])));
    let bytes = archive(&m);
    let members = ar::members(&bytes).unwrap();
    let at: Vec<(usize, String, Object<'_>)> = members
        .iter()
        .map(|m| (m.offset, m.name.clone(), elf::read(m.data).unwrap()))
        .collect();
    let mut entries = ar::index_entries(&bytes).unwrap().unwrap();
    assert!(surface::check_index_offsets(&entries, &at).is_empty());
    let weak_at = at.iter().find(|(_, n, _)| n == "weak_open.o").unwrap().0;
    entries.retain(|(o, n)| !(n == "open" && *o == weak_at));
    assert_eq!(
        surface::check_index_offsets(&entries, &at),
        [format!(
            "open is indexed 0 times for the member at {weak_at}, defined 1 times"
        )]
    );
}

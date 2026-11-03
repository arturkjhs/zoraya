//! The command line of `posix-surface-check`: the paths `make check-ports`
//! does not take (writing the list, a stale list, `--allow-svc`, misuse).

use std::path::PathBuf;
use std::process::Command;

const CSV: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/research/task-02/libc-surface.csv"
);
const LIST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/adr/posix-surface-T1.txt"
);

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "posix-surface-check-cli-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_posix-surface-check"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

#[test]
fn list_writes_the_committed_list_and_check_refuses_a_stale_one() {
    let d = tmp("list");
    let out = d.join("T1.txt");
    let (ok, text) = run(&["list", CSV, out.to_str().unwrap()]);
    assert!(ok && text.contains("243 names"), "{text}");
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(LIST).unwrap());
    assert!(run(&["list", "--check", CSV, LIST]).0);
    let stale = std::fs::read_to_string(LIST)
        .unwrap()
        .replace("\nwritev\n", "\n");
    std::fs::write(&out, stale).unwrap();
    let (ok, text) = run(&["list", "--check", CSV, out.to_str().unwrap()]);
    assert!(!ok && text.contains("regenerate"), "{text}");
    assert!(!run(&["list", "--junk", CSV, LIST]).0);
}

#[test]
fn allow_svc_permits_the_named_member_only() {
    let d = tmp("svc");
    // An object with `svc #0` in .text and no symbols, for a list with no names.
    let mut e = vec![0u8; 64];
    e[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    e[16..18].copy_from_slice(&1u16.to_le_bytes());
    e[18..20].copy_from_slice(&183u16.to_le_bytes());
    let text_at = e.len();
    e.extend_from_slice(&0xd400_0001u32.to_le_bytes());
    let strtab_at = e.len();
    e.extend_from_slice(b"\0\0\0\0");
    let sym_at = e.len();
    e.extend_from_slice(&[0u8; 24]);
    let shoff = e.len();
    for (kind, flags, off, size, link) in [
        (0u32, 0u64, 0usize, 0usize, 0u32),
        (1, 6, text_at, 4, 0),
        (2, 0, sym_at, 24, 3),
        (3, 0, strtab_at, 4, 0),
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
    let members = [zoraya_ar::Member {
        name: "zoraya_rt-1.o".into(),
        data: e,
    }];
    let archive = d.join("lib.a");
    std::fs::write(&archive, zoraya_ar::write(&members).unwrap()).unwrap();
    let list = d.join("empty.txt");
    std::fs::write(&list, "# no names\n").unwrap();
    let (a, l) = (archive.to_str().unwrap(), list.to_str().unwrap());
    let (ok, text) = run(&["check", l, a]);
    assert!(!ok && text.contains("svc in zoraya_rt-1.o"), "{text}");
    let (ok, text) = run(&["check", l, a, "--allow-svc", "zoraya_rt-"]);
    assert!(ok && text.contains("1 members, 0 exports"), "{text}");
    assert!(!run(&["check", l, a, "--allow-svc"]).0);
    assert!(!run(&["check", l, a, "--bogus", "x"]).0);
}

#[test]
fn an_unknown_command_fails_and_writes_nothing() {
    let d = tmp("bogus");
    assert!(!run(&["bogus", LIST, d.to_str().unwrap()]).0);
    assert!(!run(&[]).0);
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    assert!(!run(&["hide", LIST]).0);
    assert!(!run(&["check", LIST]).0);
}

#[test]
fn hide_refuses_two_objects_with_one_name_and_prints_its_renames() {
    let d = tmp("hide");
    let out = d.join("out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(d.join("x")).unwrap();
    let mut e = vec![0u8; 64];
    e[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    e[16..18].copy_from_slice(&1u16.to_le_bytes());
    e[18..20].copy_from_slice(&183u16.to_le_bytes());
    e[40..48].copy_from_slice(&64u64.to_le_bytes());
    e[58..60].copy_from_slice(&64u16.to_le_bytes());
    std::fs::write(d.join("a.o"), &e).unwrap();
    std::fs::write(d.join("x/a.o"), &e).unwrap();
    let (a, b) = (d.join("a.o"), d.join("x/a.o"));
    let (ok, text) = run(&["hide", LIST, out.to_str().unwrap(), a.to_str().unwrap()]);
    assert!(ok && text.is_empty(), "{text}");
    assert!(out.join("a.o").exists());
    let (ok, text) = run(&["hide", LIST, out.to_str().unwrap(), b.to_str().unwrap()]);
    assert!(!ok && text.contains("two objects"), "{text}");
}

/// An AArch64 ET_REL with one `.text` and the given (name, bind, defined)
/// symbols.
fn object(symbols: &[(&str, u8, bool)]) -> Vec<u8> {
    let mut strtab = vec![0u8];
    let mut symtab = vec![0u8; 24];
    for (name, bind, defined) in symbols {
        let mut s = [0u8; 24];
        s[..4].copy_from_slice(&(strtab.len() as u32).to_le_bytes());
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
        s[4] = (bind << 4) | if *defined { 2 } else { 0 };
        s[6..8].copy_from_slice(&(*defined as u16).to_le_bytes());
        symtab.extend_from_slice(&s);
    }
    let mut e = vec![0u8; 64];
    e[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    e[16..18].copy_from_slice(&1u16.to_le_bytes());
    e[18..20].copy_from_slice(&183u16.to_le_bytes());
    let text = e.len();
    e.extend_from_slice(&0xd503_201fu32.to_le_bytes());
    let sym = e.len();
    e.extend_from_slice(&symtab);
    let st = e.len();
    e.extend_from_slice(&strtab);
    while !e.len().is_multiple_of(8) {
        e.push(0);
    }
    let shoff = e.len();
    for (kind, flags, off, size, link) in [
        (0u32, 0u64, 0, 0, 0u32),
        (1, 6, text, 4, 0),
        (2, 0, sym, symtab.len(), 3),
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

#[test]
fn the_summary_counts_defined_non_internal_names_only() {
    let d = tmp("count");
    let o = object(&[
        ("open", 1, true),
        ("read", 2, true),
        ("_x", 1, true),
        ("printf", 2, false),
    ]);
    let members = [zoraya_ar::Member {
        name: "o.o".into(),
        data: o,
    }];
    let archive = d.join("lib.a");
    std::fs::write(&archive, zoraya_ar::write(&members).unwrap()).unwrap();
    let list = d.join("list.txt");
    std::fs::write(&list, "open\nread\n").unwrap();
    let (ok, text) = run(&["check", list.to_str().unwrap(), archive.to_str().unwrap()]);
    assert!(
        ok && text.contains("1 members, 2 exports, tier list of 2 names: passed"),
        "{text}"
    );
}

#[test]
fn an_adr_s_block_extends_the_list_and_a_proposed_one_is_noted() {
    let d = tmp("added");
    let o = object(&[("open", 1, true), ("atoll", 1, true)]);
    let members = [zoraya_ar::Member {
        name: "o.o".into(),
        data: o,
    }];
    let archive = d.join("lib.a");
    std::fs::write(&archive, zoraya_ar::write(&members).unwrap()).unwrap();
    let list = d.join("list.txt");
    std::fs::write(&list, "open\n").unwrap();
    let adr = d.join("adr.md");
    std::fs::write(
        &adr,
        "- **Status:** proposed\n\n```posix-surface-added\natoll\n```\n",
    )
    .unwrap();
    let (l, a) = (list.to_str().unwrap(), archive.to_str().unwrap());
    assert!(!run(&["check", l, a]).0);
    let spec = format!("{l}+{}", adr.to_str().unwrap());
    let (ok, text) = run(&["check", &spec, a]);
    assert!(
        ok && text.contains("is not accepted; admitting atoll"),
        "{text}"
    );
    std::fs::write(
        &adr,
        "- **Status:** accepted\n\n```posix-surface-added\natoll\n```\n",
    )
    .unwrap();
    let (ok, text) = run(&["check", &spec, a]);
    assert!(ok && !text.contains("not accepted"), "{text}");
}

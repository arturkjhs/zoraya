use super::*;

const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// An ET_EXEC with the given program headers (type, flags, vaddr, size); each
/// PT_LOAD's bytes follow the headers, page-aligned.
fn exec(kind: u16, headers: &[(u32, u32, u64, u64)], entry: u64) -> Vec<u8> {
    let mut e = vec![0u8; 64];
    e[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    e[16..18].copy_from_slice(&kind.to_le_bytes());
    e[18..20].copy_from_slice(&183u16.to_le_bytes());
    e[24..32].copy_from_slice(&entry.to_le_bytes());
    e[32..40].copy_from_slice(&64u64.to_le_bytes());
    e[54..56].copy_from_slice(&56u16.to_le_bytes());
    e[56..58].copy_from_slice(&(headers.len() as u16).to_le_bytes());
    let mut offset = 0x1000u64;
    for (t, flags, vaddr, size) in headers {
        let mut h = [0u8; 56];
        h[..4].copy_from_slice(&t.to_le_bytes());
        h[4..8].copy_from_slice(&flags.to_le_bytes());
        h[8..16].copy_from_slice(&offset.to_le_bytes());
        h[16..24].copy_from_slice(&vaddr.to_le_bytes());
        h[32..40].copy_from_slice(&size.to_le_bytes());
        h[40..48].copy_from_slice(&size.to_le_bytes());
        h[48..56].copy_from_slice(&0x1000u64.to_le_bytes());
        e.extend_from_slice(&h);
        offset += size.next_multiple_of(0x1000);
    }
    e.resize(offset as usize, 0);
    e
}

const LOAD: u32 = 1;
const TEXT: (u32, u32, u64, u64) = (LOAD, PF_R | PF_X, 0x40_0000, 0x1000);
const DATA: (u32, u32, u64, u64) = (LOAD, PF_R | PF_W, 0x40_1000, 0x1000);

#[test]
fn a_static_program_at_4_mib_passes() {
    let p = check(&exec(
        2,
        &[TEXT, DATA, (0x6474_e551, PF_R | PF_W, 0, 0)],
        0x40_0000,
    ))
    .unwrap();
    assert_eq!(p.entry, 0x40_0000);
    assert_eq!(
        describe(&p),
        "entry 0x400000, r-x 0x400000+0x1000, rw- 0x401000+0x1000"
    );
}

#[test]
fn an_interpreter_or_dynamic_section_is_refused() {
    assert_eq!(
        check(&exec(
            2,
            &[(PT_INTERP, PF_R, 0x40_2000, 0x10), TEXT],
            0x40_0000
        ))
        .unwrap_err(),
        Refusal::Interpreter
    );
    assert_eq!(
        check(&exec(
            2,
            &[TEXT, DATA, (PT_DYNAMIC, PF_R | PF_W, 0x40_1000, 0x10)],
            0x40_0000
        ))
        .unwrap_err(),
        Refusal::Dynamic
    );
}

#[test]
fn the_loader_s_refusals_are_reported() {
    use zoraya_elf::Error;
    let low = (LOAD, PF_R | PF_X, 0x20_0000, 0x1000);
    assert_eq!(
        check(&exec(2, &[low], 0x20_0000)).unwrap_err(),
        Refusal::Loader(Error::OutOfRange)
    );
    let rwx = (LOAD, PF_R | PF_W | PF_X, 0x40_0000, 0x1000);
    assert_eq!(
        check(&exec(2, &[rwx], 0x40_0000)).unwrap_err(),
        Refusal::Loader(Error::WritableExecutable)
    );
    assert_eq!(
        check(&exec(3, &[TEXT], 0x40_0000)).unwrap_err(),
        Refusal::Loader(Error::Unsupported)
    );
    assert_eq!(
        check(&exec(2, &[TEXT, DATA], 0x40_1000)).unwrap_err(),
        Refusal::Loader(Error::BadEntry)
    );
    let high = (LOAD, PF_R | PF_X, USER_HIGH, 0x1000);
    assert_eq!(
        check(&exec(2, &[high], USER_HIGH)).unwrap_err(),
        Refusal::Loader(Error::OutOfRange)
    );
}

#[test]
fn the_range_ends_one_page_below_the_startup_block() {
    use zoraya_elf::Error;
    let last = zoraya_abi::spawn::STARTUP_BLOCK - 2 * 4096;
    assert!(check(&exec(2, &[(LOAD, PF_R | PF_X, last, 0x1000)], last)).is_ok());
    let over = last + 4096;
    assert_eq!(
        check(&exec(2, &[(LOAD, PF_R | PF_X, over, 0x1000)], over)).unwrap_err(),
        Refusal::Loader(Error::OutOfRange)
    );
    assert_eq!(
        Refusal::Interpreter.to_string(),
        "it has a PT_INTERP program header"
    );
    assert_eq!(
        Refusal::Dynamic.to_string(),
        "it has a PT_DYNAMIC program header"
    );
    assert!(
        Refusal::Loader(Error::BadEntry)
            .to_string()
            .contains("BadEntry")
    );
}

#[test]
fn the_range_is_the_kernel_s() {
    assert_eq!(USER_LOW, 4 << 20);
    const { assert!(USER_HIGH > USER_LOW && USER_HIGH < 1 << 47) };
}

// ------------------------------------------------------------ INV-0450-2

const MAP: &str = "\
     VMA      LMA     Size Align Out     In      Symbol
  400000   400000       58     1 .interp
  4000f0   4000f0     1000     4 .text
  4000f0   4000f0       20     4         /w/target/release/deps/rprobe-1.rcgu.o:(.text.main)
  400110   400110       20     4         /w/build/sysroot/usr/lib/libc.a(zoraya_rt-ab.o):(.text.raw_syscall)
  400110   400110       20     1                 raw_syscall
  400130   400130       10     4         /w/build/sysroot/usr/lib/libc.a(zoraya_posix-cd.o):(.text.exit)
  400140   400140       10     4         /w/x/notlibc.a(a.o):(.text.f)
  400150   400150       10     4         /w/x/libc.a:(.text.nomember)
";

#[test]
fn the_map_gives_the_input_sections_of_libc_a_only() {
    assert_eq!(
        libc_ranges(MAP, None),
        [(0x40_0110, 0x40_0130), (0x40_0130, 0x40_0140)]
    );
    assert!(libc_ranges("", None).is_empty());
    // With the archive's exact path, another libc.a is not it (a build script's, say).
    assert_eq!(
        libc_ranges(MAP, Some("/w/build/sysroot/usr/lib/libc.a")),
        [(0x40_0110, 0x40_0130), (0x40_0130, 0x40_0140)]
    );
    assert!(libc_ranges(MAP, Some("/elsewhere/libc.a")).is_empty());
    // A section or a path made to look like a member of libc.a is not one.
    let forged = "  400000   400000       10     4         /w/x.rcgu.o:(/w/libc.a(m):(.text))\n  400010   400010       10     4         /w/t/libc.a(x):(y)/prog.o:(.text)\n";
    assert!(libc_ranges(forged, Some("/w/libc.a")).is_empty());
    assert!(libc_ranges(forged, None).is_empty());
}

/// A text segment of `words` at 0x400000.
fn text(words: &[u32]) -> Vec<u8> {
    let mut e = exec(2, &[(LOAD, PF_R | PF_X, 0x40_0000, 0x1000)], 0x40_0000);
    for (i, w) in words.iter().enumerate() {
        e[0x1000 + 4 * i..0x1000 + 4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    e
}

#[test]
fn an_svc_outside_libc_a_is_found_and_one_inside_is_not() {
    const NOP: u32 = 0xd503_201f;
    const SVC0: u32 = 0xd400_0001;
    let file = text(&[NOP, SVC0, NOP, 0xd400_0021 /* svc #1 */, NOP]);
    // No range: both are foreign.
    assert_eq!(foreign_svc(&file, &[]).unwrap(), [0x40_0004, 0x40_000c]);
    // The first inside libc.a's range, the second not.
    assert_eq!(
        foreign_svc(&file, &[(0x40_0004, 0x40_0008)]).unwrap(),
        [0x40_000c]
    );
    // Both inside.
    assert!(
        foreign_svc(&file, &[(0x40_0000, 0x40_0010)])
            .unwrap()
            .is_empty()
    );
    // Neighbours of the encoding are not svc: hvc, smc, brk.
    let file = text(&[0xd400_0002, 0xd400_0003, 0xd420_0000, NOP]);
    assert!(foreign_svc(&file, &[]).unwrap().is_empty());
    // Data is not read as code: a word in a non-executable segment.
    let mut e = exec(2, &[TEXT, DATA], 0x40_0000);
    e[0x2000..0x2004].copy_from_slice(&SVC0.to_le_bytes());
    assert!(foreign_svc(&e, &[]).unwrap().is_empty());
    // A file the loader refuses is refused.
    assert!(foreign_svc(b"not an elf", &[]).is_err());
}

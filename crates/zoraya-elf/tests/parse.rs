//! Files are built by hand, so each field under test is explicit.

use zoraya_elf::{Error, PAGE_SIZE, Program};

const LOW: u64 = 0x40_0000;
const HIGH: u64 = 1 << 47;

const R: u32 = 4;
const W: u32 = 2;
const X: u32 = 1;

#[derive(Clone, Copy)]
struct Ph {
    kind: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
}

fn load(flags: u32, offset: u64, vaddr: u64, filesz: u64, memsz: u64) -> Ph {
    Ph {
        kind: 1,
        flags,
        offset,
        vaddr,
        filesz,
        memsz,
    }
}

fn elf(entry: u64, phs: &[Ph], total: usize) -> Vec<u8> {
    let mut f = vec![0u8; total];
    f[..4].copy_from_slice(b"\x7fELF");
    f[4] = 2;
    f[5] = 1;
    f[6] = 1;
    f[16..18].copy_from_slice(&2u16.to_le_bytes());
    f[18..20].copy_from_slice(&183u16.to_le_bytes());
    f[24..32].copy_from_slice(&entry.to_le_bytes());
    f[32..40].copy_from_slice(&64u64.to_le_bytes());
    f[54..56].copy_from_slice(&56u16.to_le_bytes());
    f[56..58].copy_from_slice(&(phs.len() as u16).to_le_bytes());
    for (i, p) in phs.iter().enumerate() {
        let at = 64 + i * 56;
        f[at..at + 4].copy_from_slice(&p.kind.to_le_bytes());
        f[at + 4..at + 8].copy_from_slice(&p.flags.to_le_bytes());
        f[at + 8..at + 16].copy_from_slice(&p.offset.to_le_bytes());
        f[at + 16..at + 24].copy_from_slice(&p.vaddr.to_le_bytes());
        f[at + 32..at + 40].copy_from_slice(&p.filesz.to_le_bytes());
        f[at + 40..at + 48].copy_from_slice(&p.memsz.to_le_bytes());
    }
    f
}

const TOTAL: usize = 3 * PAGE_SIZE as usize;

fn good() -> Vec<u8> {
    elf(
        LOW + 0x10,
        &[
            load(R | X, 0x1000, LOW, 0x800, 0x800),
            load(R | W, 0x2000, LOW + 0x1000, 0x100, 0x3000),
        ],
        TOTAL,
    )
}

fn parse(f: &[u8]) -> Result<Program, Error> {
    Program::parse(f, LOW, HIGH)
}

#[test]
fn a_good_file_parses() {
    let p = parse(&good()).unwrap();
    assert_eq!(p.entry, LOW + 0x10);
    assert_eq!(p.segments().len(), 2);
    let text = p.segments()[0];
    assert!(text.execute && !text.write);
    let data = p.segments()[1];
    assert!(data.write && !data.execute);
    assert_eq!(data.pages(), (LOW + 0x1000, LOW + 0x4000));
}

#[test]
fn writable_executable_is_refused() {
    let f = elf(LOW, &[load(R | W | X, 0x1000, LOW, 0x100, 0x100)], TOTAL);
    assert_eq!(parse(&f), Err(Error::WritableExecutable));
}

#[test]
fn segments_outside_the_range_are_refused() {
    for vaddr in [0, LOW - PAGE_SIZE, HIGH, HIGH - PAGE_SIZE, u64::MAX - 0xfff] {
        let f = elf(
            vaddr,
            &[load(R | X, 0x1000, vaddr, 0x10, 2 * PAGE_SIZE)],
            TOTAL,
        );
        assert_eq!(parse(&f), Err(Error::OutOfRange), "{vaddr:#x}");
    }
}

#[test]
fn memsz_wrapping_is_refused() {
    let f = elf(LOW, &[load(R | X, 0x1000, LOW, 0x10, u64::MAX)], TOTAL);
    assert_eq!(parse(&f), Err(Error::OutOfRange));
}

#[test]
fn segments_sharing_a_page_or_out_of_order_are_refused() {
    let f = elf(
        LOW,
        &[
            load(R | X, 0x1000, LOW, 0x100, 0x100),
            load(R | W, 0x2800, LOW + 0x800, 0x100, 0x100),
        ],
        TOTAL,
    );
    assert_eq!(parse(&f), Err(Error::Overlap));
    let f = elf(
        LOW,
        &[
            load(R | W, 0x1000, LOW + 0x1000, 0x100, 0x100),
            load(R | X, 0x2000, LOW, 0x100, 0x100),
        ],
        TOTAL,
    );
    assert_eq!(parse(&f), Err(Error::Overlap));
}

#[test]
fn bad_sizes_and_offsets_are_refused() {
    for ph in [
        load(R | X, 0x1000, LOW, 0x200, 0x100),   // memsz < filesz
        load(R | X, 0x1000, LOW, 0, 0),           // empty
        load(R | X, 0x1100, LOW, 0x10, 0x10),     // offset and vaddr disagree
        load(R | X, 0x2000, LOW, 0x2000, 0x2000), // past the end of the file
        load(R | X, 0xffff_ffff_ffff_f000, LOW, 0x2000, 0x2000),
    ] {
        assert_eq!(parse(&elf(LOW, &[ph], TOTAL)), Err(Error::BadSegment));
    }
}

#[test]
fn the_entry_must_be_in_aligned_executable_memory() {
    let phs = [
        load(R | X, 0x1000, LOW, 0x100, 0x100),
        load(R | W, 0x2000, LOW + 0x1000, 0x100, 0x100),
    ];
    for entry in [0, LOW + 0x100, LOW + 0x1000, LOW + 2] {
        assert_eq!(
            parse(&elf(entry, &phs, TOTAL)),
            Err(Error::BadEntry),
            "{entry:#x}"
        );
    }
}

#[test]
fn other_kinds_of_file_are_refused() {
    for (at, value) in [(4, 1), (5, 2), (6, 0), (16, 3), (18, 62)] {
        let mut f = good();
        f[at] = value;
        assert_eq!(parse(&f), Err(Error::Unsupported), "byte {at}");
    }
    let mut f = good();
    f[0] = 0;
    assert_eq!(parse(&f), Err(Error::BadMagic));
    let mut f = good();
    f[54] = 32;
    assert_eq!(parse(&f), Err(Error::BadHeaders));
    let mut f = good();
    f[32] = 0xff;
    f[39] = 0xff;
    assert_eq!(parse(&f), Err(Error::Truncated));
}

#[test]
fn counts_of_segments_are_limited() {
    assert_eq!(parse(&elf(LOW, &[], TOTAL)), Err(Error::BadSegmentCount));
    let note = Ph {
        kind: 4,
        ..load(R, 0, 0, 0, 0)
    };
    assert_eq!(
        parse(&elf(LOW, &[note], TOTAL)),
        Err(Error::BadSegmentCount)
    );
    let many: Vec<Ph> = (0..9)
        .map(|i| load(R | X, 0x1000, LOW + i * PAGE_SIZE, 0x10, 0x10))
        .collect();
    assert_eq!(parse(&elf(LOW, &many, TOTAL)), Err(Error::BadSegmentCount));
}

#[test]
fn every_truncation_is_handled() {
    let f = good();
    for n in 0..f.len() {
        let _ = parse(&f[..n]);
    }
    assert_eq!(parse(&f[..10]), Err(Error::Truncated));
}

/// INV-0033-1, deterministically.
#[test]
fn mutations_never_panic_and_never_accept_what_the_rules_refuse() {
    let base = good();
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    let mut accepted = 0;
    for _ in 0..200_000 {
        let mut f = base.clone();
        for _ in 0..1 + next() % 6 {
            // The headers are where the meaning is.
            let i = (next() % 200) as usize;
            f[i] = next() as u8;
        }
        if let Ok(p) = parse(&f) {
            accepted += 1;
            for s in p.segments() {
                assert!(!(s.write && s.execute));
                let (start, end) = s.pages();
                assert!(start >= LOW && end <= HIGH && start < end);
                assert!(s.filesz <= s.memsz);
                assert!(s.offset + s.filesz <= f.len() as u64);
            }
        }
    }
    assert!(accepted > 0);
}

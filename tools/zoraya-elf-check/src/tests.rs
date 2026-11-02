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
fn the_range_is_the_kernel_s() {
    assert_eq!(USER_LOW, 4 << 20);
    const { assert!(USER_HIGH > USER_LOW && USER_HIGH < 1 << 47) };
}

//! `zoraya-elf-check` fails when any file is refused, or when it checked none.

use std::path::PathBuf;
use std::process::Command;

fn exec(vaddr: u64, flags: u32) -> Vec<u8> {
    let mut e = vec![0u8; 0x2000];
    e[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    e[16..18].copy_from_slice(&2u16.to_le_bytes());
    e[18..20].copy_from_slice(&183u16.to_le_bytes());
    e[24..32].copy_from_slice(&vaddr.to_le_bytes());
    e[32..40].copy_from_slice(&64u64.to_le_bytes());
    e[54..56].copy_from_slice(&56u16.to_le_bytes());
    e[56..58].copy_from_slice(&1u16.to_le_bytes());
    let h = 64;
    e[h..h + 4].copy_from_slice(&1u32.to_le_bytes());
    e[h + 4..h + 8].copy_from_slice(&flags.to_le_bytes());
    e[h + 8..h + 16].copy_from_slice(&0x1000u64.to_le_bytes());
    e[h + 16..h + 24].copy_from_slice(&vaddr.to_le_bytes());
    e[h + 32..h + 40].copy_from_slice(&0x1000u64.to_le_bytes());
    e[h + 40..h + 48].copy_from_slice(&0x1000u64.to_le_bytes());
    e[h + 48..h + 56].copy_from_slice(&0x1000u64.to_le_bytes());
    e
}

fn run(args: &[PathBuf]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_zoraya-elf-check"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

#[test]
fn passes_good_files_and_fails_on_any_refusal() {
    let d = std::env::temp_dir().join(format!("zoraya-elf-check-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("bin")).unwrap();
    std::fs::create_dir_all(d.join("empty")).unwrap();
    std::fs::write(d.join("bin/good"), exec(0x40_0000, 5)).unwrap();
    std::fs::write(d.join("bad"), exec(0x40_0000, 7)).unwrap();
    std::os::unix::fs::symlink("good", d.join("bin/link")).unwrap();

    let (ok, text) = run(&[d.join("bin")]);
    assert!(ok, "{text}");
    assert!(text.contains("1 files, 0 refused"), "{text}");
    let (ok, text) = run(&[d.join("bin"), d.join("bad")]);
    assert!(!ok, "{text}");
    assert!(
        text.contains("2 files, 1 refused") && text.contains("REFUSED"),
        "{text}"
    );
    std::fs::write(d.join("bad2"), b"text").unwrap();
    let (ok, text) = run(&[d.join("bad"), d.join("bad2")]);
    assert!(!ok && text.contains("2 files, 2 refused"), "{text}");
    assert!(!run(&[d.join("empty")]).0);
    std::fs::write(d.join("locked"), exec(0x40_0000, 5)).unwrap();
    std::fs::set_permissions(
        d.join("locked"),
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();
    let (ok, text) = run(&[d.join("bin"), d.join("locked")]);
    assert!(!ok && text.contains("2 files, 1 refused"), "{text}");
    assert!(!run(&[d.join("nowhere")]).0);
}

//! The command line of `zoraya-ar`, run as build systems run it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zoraya-ar-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ar(cwd: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_zoraya-ar"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

#[test]
fn create_list_replace_delete_extract() {
    let d = dir("ops");
    std::fs::write(d.join("a.o"), b"first").unwrap();
    std::fs::write(d.join("b.o"), b"second").unwrap();
    assert!(ar(&d, &["rcs", "lib.a", "a.o", "b.o"]).0);
    assert_eq!(ar(&d, &["t", "lib.a"]), (true, "a.o\nb.o\n".to_string()));
    std::fs::write(d.join("a.o"), b"changed").unwrap();
    assert!(ar(&d, &["-ru", "lib.a", "a.o"]).0);
    assert_eq!(ar(&d, &["t", "lib.a"]).1, "a.o\nb.o\n");
    assert!(ar(&d, &["q", "lib.a", "b.o"]).0);
    assert_eq!(ar(&d, &["t", "lib.a"]).1, "a.o\nb.o\nb.o\n");
    assert!(ar(&d, &["dv", "lib.a", "b.o"]).0);
    assert_eq!(ar(&d, &["t", "lib.a"]).1, "a.o\n");
    std::fs::remove_file(d.join("a.o")).unwrap();
    assert!(ar(&d, &["x", "lib.a"]).0);
    assert_eq!(std::fs::read(d.join("a.o")).unwrap(), b"changed");
    assert!(ar(&d, &["s", "lib.a"]).0);
    assert!(ar(&d, &["--version"]).1.starts_with("zoraya-ar"));
}

#[test]
fn x_extracts_only_the_named_members() {
    let d = dir("x");
    std::fs::write(d.join("a.o"), b"a").unwrap();
    std::fs::write(d.join("b.o"), b"b").unwrap();
    assert!(ar(&d, &["rc", "lib.a", "a.o", "b.o"]).0);
    std::fs::remove_file(d.join("a.o")).unwrap();
    std::fs::remove_file(d.join("b.o")).unwrap();
    assert!(ar(&d, &["x", "lib.a", "b.o"]).0);
    assert!(!d.join("a.o").exists() && d.join("b.o").exists());
}

#[test]
fn a_member_name_with_a_slash_is_not_extracted() {
    let d = dir("slash");
    let members = [zoraya_ar::Member {
        name: "../evil".into(),
        data: b"x".to_vec(),
    }];
    std::fs::write(d.join("bad.a"), zoraya_ar::write(&members).unwrap()).unwrap();
    let (ok, text) = ar(&d, &["x", "bad.a"]);
    assert!(!ok && text.contains("refusing"), "{text}");
}

#[test]
fn errors() {
    let d = dir("errors");
    for args in [
        &["t", "none.a"][..],
        &["x", "none.a"],
        &["d", "none.a", "a.o"],
        &["s", "none.a"],
    ] {
        let (ok, text) = ar(&d, args);
        assert!(!ok && text.contains("no such archive"), "{args:?}: {text}");
    }
    assert!(!ar(&d, &["rz", "lib.a"]).0);
    assert!(!ar(&d, &["rt", "lib.a"]).0);
    assert!(!ar(&d, &["c", "lib.a"]).0);
    assert!(!ar(&d, &["r"]).0);
    assert!(!ar(&d, &["r", "lib.a", "missing.o"]).0);
    std::fs::write(d.join("junk.a"), b"not an archive").unwrap();
    assert!(!ar(&d, &["t", "junk.a"]).0);
    assert!(!d.join("lib.a").exists());
}

#[test]
fn as_ranlib_it_rewrites_the_index() {
    let d = dir("ranlib");
    std::fs::write(d.join("a.o"), b"a").unwrap();
    assert!(ar(&d, &["rc", "lib.a", "a.o"]).0);
    let link = d.join("aarch64-unknown-zoraya-ranlib");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_zoraya-ar"), &link).unwrap();
    let run = |args: &[&str]| {
        Command::new(&link)
            .current_dir(&d)
            .args(args)
            .status()
            .unwrap()
            .success()
    };
    assert!(run(&["lib.a"]));
    assert!(!run(&["none.a"]));
    assert!(!run(&[]));
}

#[test]
fn messages_and_odd_inputs() {
    let d = dir("odd");
    std::fs::write(d.join("a.o"), b"a").unwrap();
    let (ok, text) = ar(&d, &["r", "new.a", "a.o"]);
    assert!(ok && text.contains("creating new.a"), "{text}");
    let (ok, text) = ar(&d, &["rc", "quiet.a", "a.o"]);
    assert!(ok && !text.contains("creating"), "{text}");
    let (ok, text) = ar(&d, &["dv", "new.a", "a.o", "none.o"]);
    assert!(ok && text == "d - a.o\n", "{text}");
    std::fs::create_dir(d.join("dir.a")).unwrap();
    assert!(!ar(&d, &["t", "dir.a"]).0);
    for name in [".", ".."] {
        let members = [zoraya_ar::Member {
            name: name.into(),
            data: b"x".to_vec(),
        }];
        std::fs::write(d.join("dots.a"), zoraya_ar::write(&members).unwrap()).unwrap();
        assert!(!ar(&d, &["x", "dots.a"]).0, "{name}");
    }
}

//! ADR-0466: `BIND` is minted at a short, named list of places and nowhere else.
//! The rights word is the ABI; this test reads the sources of the repository for
//! the names that carry the bit (the identifiers `BIND`, `WITH_BIND` and `MASTER`,
//! however imported) and fails on a file not in the list, so
//! that a new place that asks for it is a change to this list, and to the ADR.

use std::fs;
use std::path::{Path, PathBuf};

use zoraya_proto_file::rights;

/// Every file that names the bit, with the reason (paths from the repository root).
const SITES: &[(&str, &str)] = &[
    ("crates/zoraya-proto-file/tests/bind_sites.rs", "this list"),
    (
        "crates/zoraya-proto-file/src/lib.rs",
        "the definition, and the `Rights` type's mask",
    ),
    (
        "crates/zoraya-fs-service/src/lib.rs",
        "the service: a root holds it, a socket node's create needs it",
    ),
    (
        "user/src/files.rs",
        "`MASTER`, the trusted hand-over of a root; `make_socket_node`'s doc",
    ),
    (
        "user/src/bin/init.rs",
        "`init` hands the shell its root with `MASTER`",
    ),
    (
        "user/src/gui.rs",
        "the panel's root, handed on with `MASTER`",
    ),
    (
        "user/src/bin/broker.rs",
        "the broker hands the first line's program its `/tmp` with `BIND`",
    ),
    (
        "user/src/namespace.rs",
        "a window shell's write entries ask for it, so that it can make an own directory",
    ),
    (
        "user/src/bin/zosh.rs",
        "the shell: the own directory and the line's `/tmp`; nothing else",
    ),
    ("user/src/bin/posixrun.rs", "a test mount of kind `Tmp`"),
    (
        "crates/zoraya-posix/src/jobs.rs",
        "a child of a line keeps the `/tmp` it shares (`clone_tmp`)",
    ),
    (
        "user/src/bin/t_storage.rs",
        "the raw-call test of the service's check",
    ),
    (
        "crates/zoraya-fs-service/tests/socket.rs",
        "host tests of the service's check",
    ),
];

/// Whether `word` occurs in `text` as a whole identifier.
fn has_word(text: &str, word: &str) -> bool {
    let id = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(id)
            && !text[at + word.len()..].chars().next().is_some_and(id)
    })
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if p.is_dir() {
            if matches!(
                name.as_ref(),
                "target" | "build" | ".git" | "docs" | "ports"
            ) {
                continue;
            }
            walk(&p, out);
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

#[test]
fn bind_is_named_only_where_the_adr_says() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for top in ["crates", "user", "kernel", "tools"] {
        walk(&root.join(top), &mut files);
    }
    let mut found: Vec<String> = Vec::new();
    for f in files {
        let text = fs::read_to_string(&f).unwrap_or_default();
        let names_it = text.lines().any(|l| {
            let code = l.split("//").next().unwrap_or("");
            has_word(code, "BIND") || has_word(code, "WITH_BIND") || has_word(code, "MASTER")
        });
        if names_it {
            let rel = f
                .strip_prefix(&root)
                .unwrap()
                .components()
                .collect::<PathBuf>();
            found.push(rel.to_string_lossy().to_string());
        }
    }
    found.sort();
    let allowed: Vec<&str> = SITES.iter().map(|(p, _)| *p).collect();
    for f in &found {
        let f = f.trim_start_matches("../").trim_start_matches("./");
        assert!(
            allowed.iter().any(|a| f.ends_with(a)),
            "{f} names BIND and is not in the list (ADR-0466): add it with a reason, and say it in the ADR"
        );
    }
    assert!(
        !found.is_empty(),
        "the walk found nothing: the test is blind"
    );
}

#[test]
fn ordinary_rights_do_not_contain_bind() {
    assert_eq!(rights::ALL & rights::BIND, 0);
    assert_eq!(rights::WITH_BIND, rights::ALL | rights::BIND);
    assert_eq!(rights::WITH_BIND.count_ones(), rights::ALL.count_ones() + 1);
    // What the shell asks for a grant: no `BIND` in it.
    assert_eq!((rights::ALL & !rights::PUBLISH) & rights::BIND, 0);
}

zoraya_test_alloc::cap_memory!();

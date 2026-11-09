use super::*;
use alloc::vec;
use zoraya_proto_file::rights;

fn e(name: &str, dir: bool, size: u64) -> Entry {
    Entry {
        name: name.into(),
        dir,
        size,
    }
}

/// The mockup's `~/project`, shuffled, with names no sheet may show.
fn project() -> Vec<Entry> {
    vec![
        e("звіт-2026.txt", false, 2100),
        e("Makefile", false, 412),
        e("plán.txt", false, 1200),
        e("include", true, 0),
        e("main.c", false, 1900),
        e("notes.txt", false, 2100),
        e("bad/name", false, 1),
        e("..", true, 0),
    ]
}

fn names(b: &Browser) -> Vec<&str> {
    b.entries.iter().map(|e| e.name.as_str()).collect()
}

fn open(filters: &str) -> Browser {
    let mut b = Browser::new(
        Ask::Open {
            filters: Ask::filters(filters),
        },
        "edit",
        "/home",
    );
    b.path.push("project".into());
    b.show(project());
    b
}

#[test]
fn a_listing_is_folders_first_then_names_and_filtered() {
    let b = open("");
    assert_eq!(
        names(&b),
        [
            "include",
            "main.c",
            "Makefile",
            "notes.txt",
            "plán.txt",
            "звіт-2026.txt"
        ]
    );
    let b = open("txt TXT .md x/y");
    assert_eq!(Ask::filters("txt TXT .md x/y"), ["txt", "txt"]);
    assert_eq!(
        names(&b),
        ["include", "notes.txt", "plán.txt", "звіт-2026.txt"]
    );
    let mut pick = Browser::new(Ask::Pick { write: false }, "edit", "/home");
    pick.show(project());
    assert_eq!(names(&pick), ["include"]);
    // A dotfile has no extension to filter by.
    let mut b = open("txt");
    b.show(vec![e(".txt", false, 1), e("a.TXT", false, 1)]);
    assert_eq!(names(&b), ["a.TXT"]);
}

/// INV-0167-3's host half: each kind's connection has exactly the rights
/// of ADR-0153's table, by the shell's own `Access`.
#[test]
fn each_pick_gives_the_kind_and_rights_of_its_request() {
    let mut b = open("");
    assert_eq!(b.pending(), None, "nothing picked yet");
    b.select(4);
    let g = b.pending().unwrap();
    assert_eq!(g.dir, "/home/project");
    assert_eq!(g.name.as_deref(), Some("plán.txt"));
    assert_eq!(g.access, Access::Read);
    assert_eq!(g.kind, kind::READ);
    assert_eq!(g.rights(), rights::READ | rights::CLONE);
    assert_eq!(g.rights() & rights::WRITE, 0);
    assert_eq!(g.shown, "plán.txt");
    assert_eq!(g.line, "+r ~/project/plán.txt edit");
    assert_eq!(g.words, "+r plán.txt", "Chosen carries no path (ADR-0167)");
    assert_eq!(g.pill, "+r ~/project/plán.txt");
    assert_eq!(b.sentence(), "edit will be given this file, to read:");
    // A folder cannot be opened as a file.
    b.select(0);
    assert_eq!(b.pending(), None);

    let mut s = Browser::new(
        Ask::Save {
            suggested: "new.txt".into(),
            filters: vec![],
        },
        "edit",
        "/home",
    );
    s.show(project());
    let g = s.pending().unwrap();
    assert_eq!(g.dir, "/home");
    assert_eq!(g.access, Access::Create("new.txt".into()));
    assert_eq!(g.kind, kind::CREATE);
    assert_eq!(
        g.rights(),
        rights::READ | rights::WRITE | rights::CREATE | rights::SYNC | rights::CLONE
    );
    assert_eq!(g.line, "create or replace ~/new.txt");
    assert_eq!(g.pill, "+c ~/new.txt");
    assert_eq!(g.words, "+c new.txt");
    // Picking a file takes its name; an invalid name gives nothing.
    s.select(2);
    assert_eq!(s.name, "Makefile");
    s.name = "a/b".into();
    assert_eq!(s.pending(), None);
    s.name = String::new();
    assert_eq!(s.confirm(), Step::None);
    let bad = Browser::new(
        Ask::Save {
            suggested: "..".into(),
            filters: vec![],
        },
        "edit",
        "/home",
    );
    assert_eq!(bad.name, "");

    for (write, access, flag, k) in [
        (false, Access::Read, "+r", kind::READ),
        (true, Access::Tree, "+w", kind::TREE),
    ] {
        let mut p = Browser::new(Ask::Pick { write }, "edit", "/home");
        p.show(project());
        let here = p.pending().unwrap();
        assert_eq!(here.dir, "/home");
        assert_eq!(here.line, std::format!("{flag} ~ edit"));
        p.select(0);
        let g = p.pending().unwrap();
        assert_eq!((g.dir.as_str(), g.name.as_deref()), ("/home/include", None));
        assert_eq!(g.access, access);
        assert_eq!(g.kind, k);
        assert_eq!(g.rights(), access.rights());
    }
    assert_eq!(
        Access::Tree.rights() & rights::PUBLISH,
        0,
        "a tree entry cannot publish"
    );
}

#[test]
fn rows_enter_folders_and_confirm_files() {
    let mut b = open("");
    assert_eq!(b.crumbs(), "~ › project");
    assert_eq!(b.activate(0), Step::List);
    assert_eq!(b.dir(), "/home/project/include");
    assert!(b.entries.is_empty());
    assert_eq!(b.up(), Step::List);
    assert_eq!(b.up(), Step::List);
    assert_eq!(b.crumbs(), "~");
    assert_eq!(b.up(), Step::List);
    assert_eq!((b.dir().as_str(), b.crumbs().as_str()), ("/", "/"));
    assert_eq!(b.up(), Step::None, "nothing above the root");
    b.path = vec!["system".into(), "fonts".into()];
    assert_eq!(b.crumbs(), "/ › system › fonts");
    let mut b = open("");
    match b.activate(5) {
        Step::Confirm(g) => assert_eq!(g.shown, "звіт-2026.txt"),
        s => panic!("{s:?}"),
    }
    assert_eq!(b.activate(99), Step::None);
    assert_eq!(b.selected, Some(5));
    b.select(99);
    assert_eq!(b.selected, Some(5), "an out-of-range row changes nothing");
}

#[test]
fn sizes_read_as_the_mockups_write_them() {
    for (bytes, text) in [
        (0, "0 B"),
        (412, "412 B"),
        (999, "999 B"),
        (1000, "1.0 kB"),
        (1200, "1.2 kB"),
        (1949, "1.9 kB"),
        (1950, "2.0 kB"),
        (15_000, "15 kB"),
        (999_999, "1000 kB"),
        (2_100_000, "2.1 MB"),
        (u64::MAX, "18446744 TB"),
    ] {
        assert_eq!(size_text(bytes), text, "{bytes}");
    }
}

/// INV-0167-1: what `Chosen` tells the application is a display name and
/// its kind, never a path: whatever the pick, deep in the volume or at its
/// top, the words are the flag and one name.
#[test]
fn chosen_names_are_never_paths() {
    let asks = [
        Ask::Open { filters: vec![] },
        Ask::Save {
            suggested: "new.txt".into(),
            filters: vec![],
        },
        Ask::Pick { write: false },
        Ask::Pick { write: true },
    ];
    for ask in asks {
        let mut b = Browser::new(ask.clone(), "edit", "/home");
        b.show(project());
        for row in 0..4 {
            b.select(row);
            if let Some(g) = b.pending() {
                let (flag, name) = g.words.split_once(' ').unwrap();
                assert!(["+r", "+w", "+c"].contains(&flag), "{}", g.words);
                assert!(
                    !name.contains('/') || name == "/",
                    "{} for {ask:?}",
                    g.words
                );
                assert_eq!(name, g.shown);
            }
        }
    }
    // The top of the volume is shown as `/`, which no one resolves.
    let mut top = Browser::new(Ask::Pick { write: false }, "edit", "/");
    top.show(vec![]);
    assert_eq!(top.pending().map(|g| g.words), Some(String::from("+r /")));
}

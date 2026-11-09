use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::rooms::*;

const MOCKUP: &str = "# the mockups' rooms
project --colour 1 +w ~/project +r ~/include
agent-1 --untrusted +w ~/sandbox/agent-1
";

#[test]
fn the_mockups_rooms_read_and_write_back() {
    let r = read(MOCKUP.as_bytes());
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.rooms.len(), 2);
    let p = r.get("project").unwrap();
    assert_eq!((p.untrusted, p.colour), (false, 1));
    let words: Vec<String> = p.grants.iter().map(Grant::words).collect();
    assert_eq!(words, ["+w ~/project", "+r ~/include"]);
    let a = r.get("agent-1").unwrap();
    assert_eq!((a.untrusted, a.colour), (true, 0));
    for room in &r.rooms {
        let again = read(room.line().as_bytes());
        assert_eq!(again.rooms, vec![room.clone()]);
    }
    assert_eq!(p.line(), "project --colour 1 +w ~/project +r ~/include");
    assert_eq!(a.line(), "agent-1 --untrusted +w ~/sandbox/agent-1");
}

#[test]
fn bad_lines_are_reported_once_and_skipped() {
    let text = "Bad +w ~/x\nhome +w ~/a\nok +w relative\nok2 +q ~/a\nok3 --colour 5\n\
                ok4 --untrusted --colour 2\nok5 +w ~/a/../b\nok6 +w\nok7 'open\n\
                fine +r /system\nfine +w ~/again\n\u{1}x\n";
    let r = read(text.as_bytes());
    assert_eq!(r.rooms.len(), 1);
    assert_eq!(r.rooms[0].name, "fine");
    let lines: Vec<usize> = r.problems.iter().map(|p| p.line).collect();
    assert_eq!(lines, [1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12]);
    assert!(r.problems[10].problem.contains("control"));
    // Any bytes: no panic.
    for n in 0..=255u8 {
        let _ = read(&[b'a', b' ', b'+', b'w', b' ', n, b'\n', n]);
    }
}

#[test]
fn colours_take_the_lowest_free_slot_and_untrusted_has_none() {
    let r = read(b"a --colour 2\nb\nc --untrusted\nd\ne --colour 1\n");
    let c: Vec<u8> = r.rooms.iter().map(|r| r.colour).collect();
    // b and d are given slots after every explicit one: 1 and 2 are taken.
    assert_eq!(c, [2, 3, 0, 4, 1]);
    assert_eq!(r.free_colour(), 1, "all four once: the lowest again");
    let r = read(b"a --colour 1\na2 --colour 1\nb --colour 2\nc --colour 3\nd --colour 4\n");
    assert_eq!(r.free_colour(), 2);
    assert_eq!(read(b"").free_colour(), 1);
}

#[test]
fn names_and_paths() {
    for good in ["project", "agent-1", "a", "x".repeat(MAX_NAME).as_str()] {
        assert!(is_room_name(good), "{good}");
    }
    for bad in [
        "",
        "home",
        "Project",
        "1st",
        "-a",
        "a b",
        "a_b",
        "ж",
        "x".repeat(MAX_NAME + 1).as_str(),
    ] {
        assert!(!is_room_name(bad), "{bad}");
    }
    for good in [
        "~",
        "~/project",
        "/",
        "/system/fonts",
        "~/Документи/звіт-2026.txt",
    ] {
        assert!(is_grant_path(good), "{good}");
    }
    for bad in [
        "", "project", "~x", "~/", "~/a//b", "/a/./b", "/a/..", "~/a/", "/a\u{7}",
    ] {
        assert!(!is_grant_path(bad), "{bad}");
    }
    let quoted = Grant {
        write: false,
        path: "~/my files".into(),
    };
    assert_eq!(quoted.words(), "+r '~/my files'");
    assert_eq!(read(b"r +r '~/my files'\n").rooms[0].grants, vec![quoted]);
}

#[test]
fn the_writer_keeps_every_other_byte() {
    let room = Room {
        name: "notes".into(),
        untrusted: false,
        colour: 2,
        grants: vec![Grant {
            write: true,
            path: "~/notes".into(),
        }],
    };
    let out = add(MOCKUP.as_bytes(), &room).unwrap();
    assert!(out.starts_with(MOCKUP.as_bytes()));
    assert_eq!(read(&out).get("notes"), Some(&room));
    assert_eq!(
        add(b"x", &room).unwrap(),
        b"x\nnotes --colour 2 +w ~/notes\n"
    );
    assert_eq!(add(&out, &room), Err(RoomRefusal::Exists));
    let bad = Room {
        name: "home".into(),
        ..room.clone()
    };
    assert_eq!(add(b"", &bad), Err(RoomRefusal::BadName));
    let bad = Room {
        grants: vec![Grant {
            write: true,
            path: "x".into(),
        }],
        ..room.clone()
    };
    assert_eq!(add(b"", &bad), Err(RoomRefusal::BadGrant));
    let back = remove(&out, "notes").unwrap();
    assert_eq!(back, MOCKUP.as_bytes());
    let first = remove(MOCKUP.as_bytes(), "project").unwrap();
    assert_eq!(
        first,
        b"# the mockups' rooms\nagent-1 --untrusted +w ~/sandbox/agent-1\n"
    );
    assert_eq!(
        remove(MOCKUP.as_bytes(), "nobody"),
        Err(RoomRefusal::NoSuchRoom)
    );
    // Every line of a name goes (review 7d, 14).
    let two = b"a --colour 1\nb --colour 2\na --colour 3\n";
    assert_eq!(remove(two, "a").unwrap(), b"b --colour 2\n");
    let mut many = Vec::new();
    for i in 0..MAX_ROOMS {
        many = add(
            &many,
            &Room {
                name: alloc::format!("r{i}"),
                ..room.clone()
            },
        )
        .unwrap();
    }
    assert_eq!(add(&many, &room), Err(RoomRefusal::TooMany));
}

/// Review 7d, 7: a file past MAX_SIZE is cut at a whole line, and said so.
#[test]
fn a_long_file_is_cut_at_a_line_and_reported() {
    let mut text = String::from("keep +r /system\n");
    while text.len() < crate::MAX_SIZE - 10 {
        text.push_str("# padding padding padding padding\n");
    }
    text.push_str("wide +r /srv/project/sub\n");
    let r = read(text.as_bytes());
    assert!(r.get("keep").is_some());
    assert!(r.get("wide").is_none(), "the cut line is not read");
    assert!(r.problems.iter().any(|p| p.problem.contains("256 KiB")));
}

/// Mutants 7d: the bounds of a grant's path, a line's words, a repeated
/// flag, the number of rooms and a writer's grants.
#[test]
fn every_bound_is_where_it_says() {
    let long = |n: usize| format!("/{}", "a".repeat(n - 1));
    assert!(is_grant_path(&long(MAX_PATH)));
    assert!(!is_grant_path(&long(MAX_PATH + 1)));
    assert!(!is_grant_path("/a\u{7}"));

    let grants = (0..MAX_GRANTS)
        .map(|i| format!(" +r /g{i}"))
        .collect::<String>();
    // 4 + 2 * MAX_GRANTS words is still a line; one more is not.
    let at = format!("full --colour 1{grants} x\n");
    let past = format!("full --colour 1{grants} x y\n");
    let p = |t: &str| read(t.as_bytes()).problems[0].problem.clone();
    assert!(p(&at).contains("not --untrusted"), "{}", p(&at));
    assert_eq!(p(&past), "too many grants");

    assert!(p("twice --untrusted --untrusted\n").contains("not --untrusted"));
    assert!(p("twice --colour 1 --colour 2\n").contains("not --untrusted"));

    let many: String = (0..=MAX_ROOMS).map(|i| format!("r{i}\n")).collect();
    let r = read(many.as_bytes());
    assert_eq!(r.rooms.len(), MAX_ROOMS);
    assert_eq!(r.problems.len(), 1);
    assert_eq!(r.problems[0].line, MAX_ROOMS + 1);
    assert!(r.problems[0].problem.contains("more than"));

    let room = |n: usize| Room {
        name: String::from("new"),
        untrusted: false,
        colour: 1,
        grants: (0..n)
            .map(|i| Grant {
                write: false,
                path: format!("/g{i}"),
            })
            .collect(),
    };
    assert!(add(b"", &room(MAX_GRANTS)).is_ok());
    assert_eq!(add(b"", &room(MAX_GRANTS + 1)), Err(RoomRefusal::TooMany));
}

/// Mutants 7d: the cut keeps every whole line, and only those, and names
/// the first line it drops; a file of exactly MAX_SIZE is not cut.
#[test]
fn the_cut_is_exact() {
    let last = "last +r /system/xyz\n";
    // Comments, then `last` ending 16 bytes before the limit.
    let pad = |end: usize| {
        let mut t = String::new();
        while t.len() < end - 100 {
            t.push_str("# padding padding padding padding\n");
        }
        t.push_str(&"#".repeat(end - t.len() - 1));
        t.push('\n');
        t
    };
    let mut text = pad(crate::MAX_SIZE - 16 - last.len());
    text.push_str(last);
    let kept_lines = text.matches('\n').count();
    // `wide +r /srv/pro` is the part before the limit: a wider grant.
    text.push_str("wide +r /srv/project/sub\n");
    let r = read(text.as_bytes());
    assert_eq!(r.get("last").unwrap().grants[0].path, "/system/xyz");
    assert!(r.get("wide").is_none(), "the cut line is not read");
    assert_eq!(r.problems.len(), 1);
    assert_eq!(r.problems[0].line, kept_lines + 1);

    let exact = pad(crate::MAX_SIZE);
    assert_eq!(exact.len(), crate::MAX_SIZE);
    assert!(read(exact.as_bytes()).problems.is_empty());
}

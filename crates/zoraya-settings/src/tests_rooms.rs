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
        limits: Limits::default(),
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
    // 4 + 2 * (MAX_GRANTS + the six limit words) words is still a line; one
    // more is not.
    let pad = " x".repeat(2 * LIMIT_WORDS.len());
    let at = format!("full --colour 1{grants}{pad} x\n");
    let past = format!("full --colour 1{grants}{pad} x y\n");
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
        limits: Limits::default(),
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

#[test]
fn sizes_parse_and_print_the_way_the_line_writes_them() {
    assert_eq!(parse_size("512M"), Some(512 << 20));
    assert_eq!(parse_size("2G"), Some(2 << 30));
    assert_eq!(parse_size("64K"), Some(64 << 10));
    for bad in [
        "",
        "M",
        "0M",
        "12",
        "1.5G",
        "-1M",
        "1m",
        "99999999999999999G",
        "1MB",
    ] {
        assert_eq!(parse_size(bad), None, "{bad}");
    }
    assert_eq!(format_size(89 << 20), "89M");
    assert_eq!(format_size(2 << 30), "2G");
    assert_eq!(format_size(5000 << 10), "5000K");
    for n in [1u64, 1023, 1024, 65536, 1 << 20, 3 << 29, 5 << 30] {
        let t = format_size(n);
        assert_eq!(parse_size(&t), Some(n.div_ceil(1024) * 1024), "{n} {t}");
    }
}

fn machine(session_mem: u64) -> Machine {
    Machine {
        session_mem,
        session_procs: 200,
        free_disk: 64 << 30,
    }
}

#[test]
fn defaults_follow_adr_0266_on_both_guest_sizes() {
    // ADR-0266 point 3's table: a quarter of the session, whole MiB.
    let mut l = Limits::default();
    l.fill_defaults(&machine(86 << 20));
    assert_eq!(
        l.words(),
        "--mem 21M --cpu 25% --procs 32 --disk 1G --conns 16 --tmp 5M"
    );
    let mut l = Limits::default();
    l.fill_defaults(&machine(356 << 20));
    assert_eq!(
        l.words(),
        "--mem 89M --cpu 25% --procs 32 --disk 1G --conns 16 --tmp 16M"
    );
    // A small volume: an eighth of its free space.
    let mut l = Limits::default();
    l.fill_defaults(&Machine {
        free_disk: 80 << 20,
        ..machine(356 << 20)
    });
    assert_eq!(l.disk, Some(10 << 20));
    // What the person gave stays.
    let mut l = Limits {
        mem: Some(64 << 20),
        cpu: Some(10),
        ..Limits::default()
    };
    l.fill_defaults(&machine(356 << 20));
    assert_eq!(
        (l.mem, l.cpu, l.tmp),
        (Some(64 << 20), Some(10), Some(16 << 20))
    );
    assert!(l.is_complete());
}

#[test]
fn the_limit_words_read_and_write_back_in_the_lines_order() {
    let text = "agent-1 --untrusted --mem 89M --cpu 25% --procs 32 --disk 1G --conns 16 --tmp 16M +w ~/sandbox/agent-1\n\
                build --colour 2 --cpu 50% --mem 2G +w ~/build\n";
    let r = read(text.as_bytes());
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let a = r.get("agent-1").unwrap();
    assert!(a.limits.is_complete());
    assert_eq!(a.limits.mem, Some(89 << 20));
    assert_eq!(
        a.line(),
        text.lines()
            .next()
            .unwrap()
            .replace("--untrusted --mem", "--untrusted --mem")
    );
    let b = r.get("build").unwrap();
    assert_eq!(
        (b.limits.mem, b.limits.cpu, b.limits.tmp),
        (Some(2 << 30), Some(50), None)
    );
    // Written in the order of the line, whatever order it was typed in.
    assert_eq!(b.line(), "build --colour 2 --mem 2G --cpu 50% +w ~/build");
    // The word may come before `--untrusted`.
    let r = read(b"x --tmp 1M --untrusted\n");
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    // Without the words the limits are empty.
    assert_eq!(
        read(MOCKUP.as_bytes()).get("project").unwrap().limits,
        Limits::default()
    );
}

#[test]
fn bad_limit_words_are_reported_and_the_line_skipped() {
    for bad in [
        "a --mem",
        "a --mem 12",
        "a --mem 0M",
        "a --mem 1M --mem 2M",
        "a --cpu 0%",
        "a --cpu 101%",
        "a --cpu 25",
        "a --procs 0",
        "a --procs x",
        "a --conns -1",
        "a --disk none",
        "a --mem none",
        "a --tmp 1M",
    ] {
        let r = read(bad.as_bytes());
        assert!(r.rooms.is_empty(), "{bad}");
        assert_eq!(r.problems.len(), 1, "{bad}");
    }
}

#[test]
fn set_refuses_none_for_untrusted_and_a_tmp_for_trusted() {
    let mut l = Limits::default();
    l.fill_defaults(&machine(356 << 20));
    assert!(l.set("--mem", "none", true).is_err());
    assert!(l.set("--tmp", "1M", false).is_err());
    assert!(l.set("--cpu", "10%", true).is_ok());
    assert_eq!(l.cpu, Some(10));
    let mut t = Limits::default();
    t.set("--mem", "1G", false).unwrap();
    t.set("--mem", "none", false).unwrap();
    assert_eq!(t, Limits::default());
    assert!(t.set("--mem", "512", false).is_err());
    assert!(t.set("--nonsense", "1", false).is_err());
}

#[test]
fn no_limit_is_above_its_parents() {
    let m = machine(356 << 20);
    let mut l = Limits::default();
    l.set("--mem", "512M", true).unwrap();
    assert_eq!(
        l.check_within(&m.untrusted()).unwrap_err(),
        "--mem 512M: above the untrusted rooms' 178M"
    );
    assert!(l.check_within(&m.session()).is_err());
    let mut l = Limits::default();
    l.set("--procs", "97", true).unwrap();
    assert!(l.check_within(&m.untrusted()).is_err());
    assert!(l.check_within(&m.session()).is_ok());
    l.set("--procs", "201", true).unwrap();
    assert!(l.check_within(&m.session()).is_err());
    let mut l = Limits::default();
    l.set("--cpu", "60%", true).unwrap();
    assert!(l.check_within(&m.untrusted()).is_err());
    assert!(l.check_within(&m.session()).is_ok());
    // The defaults themselves fit on the machines of ADR-0266.
    for session in [86u64 << 20, 356 << 20] {
        let m = machine(session);
        let mut d = Limits::default();
        d.fill_defaults(&m);
        assert_eq!(d.check_within(&m.untrusted()), Ok(()));
        // Two rooms at their default fill the untrusted job (point 4).
        assert!(d.mem.unwrap() * 2 <= m.untrusted().mem + (2 << 20));
    }
}

#[test]
fn replace_swaps_one_line_and_keeps_the_rest() {
    let r = read(MOCKUP.as_bytes());
    let mut a = r.get("agent-1").unwrap().clone();
    a.limits.fill_defaults(&machine(356 << 20));
    let out = replace(MOCKUP.as_bytes(), &a).unwrap();
    let text = String::from_utf8(out.clone()).unwrap();
    assert!(text.starts_with("# the mockups' rooms\nproject --colour 1 +w ~/project +r ~/include\nagent-1 --untrusted --mem 89M"));
    assert_eq!(read(&out).get("agent-1"), Some(&a));
    assert_eq!(read(&out).get("project"), r.get("project"));
    let ghost = Room {
        name: "ghost".into(),
        ..a.clone()
    };
    assert_eq!(
        replace(MOCKUP.as_bytes(), &ghost),
        Err(RoomRefusal::NoSuchRoom)
    );
}

#[test]
fn every_untrusted_line_is_complete_after_fill_and_survives_the_reader() {
    for word in LIMIT_WORDS {
        let mut l = Limits::default();
        l.fill_defaults(&machine(356 << 20));
        let value = match word {
            "--cpu" => "10%",
            "--procs" | "--conns" => "3",
            _ => "1M",
        };
        l.set(word, value, true).unwrap();
        assert!(l.is_complete());
        let room = Room {
            name: "z".into(),
            untrusted: true,
            colour: 0,
            limits: l,
            grants: vec![],
        };
        assert_eq!(read(room.line().as_bytes()).rooms, vec![room]);
    }
}

#[test]
fn a_line_with_many_refusals_gets_one_hint_naming_the_field_and_the_room() {
    // INV-0263-4: however often a line's commands were refused, one hint.
    assert_eq!(refused_field((3, 0), (3, 0)), None);
    assert_eq!(refused_field((3, 1), (900, 40)), Some(Refused::Memory));
    assert_eq!(refused_field((3, 1), (3, 40)), Some(Refused::Processes));
    let h = quota_hint("agent-1", Refused::Memory, 64 << 20, true);
    assert_eq!(
        h,
        "hint: room agent-1 is at its memory limit of 64M; room set agent-1 --mem 128M\n"
    );
    assert_eq!(h.matches("hint:").count(), 1);
    let h = quota_hint("agent-1", Refused::Processes, 32, false);
    assert_eq!(
        h,
        "hint: room agent-1 is at its processes limit of 32; from home: room set agent-1 --procs 64\n"
    );
}

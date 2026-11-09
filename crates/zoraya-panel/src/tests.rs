use super::*;

const ROOMS: &[u8] =
    b"project --colour 1 +w ~/project +r ~/include\nagent-1 --untrusted +w ~/sandbox/agent-1\n";

/// What a fake `spawn` records of one program: its image, its handles and
/// its arguments.
type Spawned = (&'static str, Vec<(u32, Give)>, Vec<String>);

/// A fake `spawn`: what each program would be given.
fn spawned(l: &Launch) -> Vec<Spawned> {
    l.programs
        .iter()
        .map(|p| {
            (
                p.image,
                p.handles.iter().map(|s| (s.slot, s.give)).collect(),
                p.args.clone(),
            )
        })
        .collect()
}

/// INV-0168-1 and INV-0172-2, the host half: a launch gives each program
/// exactly the handles its record lists, the record names its room, and
/// it holds the room's entries and no other room's.
/// The toolchain a trusted room's shell reads: added after the room's
/// grants, unless a grant covers it, and never with a write.
#[test]
fn a_rooms_shell_reads_the_toolchain() {
    let room = |grants: &str| {
        let mut p = Panel::default();
        assert!(p.reload(format!("r {grants}\n").as_bytes()));
        assert_eq!(p.pressed(ROOM, 1), Action::Entered);
        let Action::Launch(l) = p.pressed(LAUNCH, 0) else {
            panic!("{grants}")
        };
        l.entries
    };
    assert_eq!(
        room("+w ~/project"),
        ["+w ~/project", "+r /bin", "+r /usr", "+r /etc"]
    );
    assert_eq!(room("+w /usr"), ["+w /usr", "+r /bin", "+r /etc"]);
    assert_eq!(
        room("+r /usr/lib"),
        ["+r /usr/lib", "+r /bin", "+r /usr", "+r /etc"]
    );
    assert_eq!(
        room("+r /binaries"),
        ["+r /binaries", "+r /bin", "+r /usr", "+r /etc"]
    );
    assert_eq!(room("+r /"), ["+r /"]);
    assert_eq!(
        room("+w /etc/x +r /etc"),
        ["+w /etc/x", "+r /etc", "+r /bin", "+r /usr"]
    );
}

#[test]
fn a_launch_gives_exactly_its_record_in_its_room() {
    let mut p = Panel::default();
    assert!(p.reload(ROOMS));
    let home = terminal(None);
    assert_eq!(home.room, None);
    assert_eq!(home.entries, ["+r /", "+w ~", "+w /config", "+w /tmp"]);
    assert_eq!(home.holds.len(), HOME_GRANTS.len());
    assert_eq!(home.line, "term");
    let s = spawned(&home);
    assert_eq!(s[0].0, "term");
    assert_eq!(
        s[0].1,
        [
            (1, Give::Console),
            (5, Give::Compositor),
            (6, Give::Terminal)
        ]
    );
    assert_eq!(s[1].0, "zosh");
    assert_eq!(
        s[1].1,
        [
            (1, Give::Console),
            (2, Give::Job),
            (3, Give::Directory),
            (STARTUP_SLOT, Give::Startup),
            (6, Give::Terminal),
            (7, Give::Broker),
            (8, Give::Tmp)
        ]
    );
    assert!(s[1].2.is_empty());
    // Only the shell has a start-up record; nothing holds the volume's
    // root, and home gives no write to `/` (ADR-0242's condition).
    for (image, handles, _) in &s {
        let record = handles.iter().any(|h| h.1 == Give::Startup);
        assert_eq!(record, *image == "zosh", "{image}");
    }
    assert!(!home.entries.contains(&String::from("+w /")));
    // Every room: its own grants, its name, nothing of another room.
    for (i, room) in p.rooms.rooms.clone().iter().enumerate() {
        assert_eq!(p.pressed(ROOM, i as u32 + 1), Action::Entered);
        // Nothing runs in an untrusted room until quotas (ADR-0056).
        if room.untrusted {
            assert_eq!(p.pressed(LAUNCH, 0), Action::Nothing, "{}", room.name);
            continue;
        }
        let Action::Launch(l) = p.pressed(LAUNCH, 0) else {
            panic!()
        };
        assert_eq!(
            l.room.as_ref().map(|r| r.name.as_str()),
            Some(room.name.as_str())
        );
        assert_eq!(l.line, format!("room {} term", room.name));
        // INV-0172-2: exactly the room's grants and the toolchain read-only
        // where no grant covers it, shown and held.
        let mut held: Vec<(String, bool)> = room
            .grants
            .iter()
            .map(|g| (g.path.clone(), g.write))
            .collect();
        for t in TOOLCHAIN_GRANTS {
            if !room.grants.iter().any(|g| g.path == "/" || g.path == t) {
                held.push((String::from(t), false));
            }
        }
        let mine: Vec<String> = held.iter().map(|(p, w)| grant_words(p, *w)).collect();
        assert_eq!(l.entries, mine);
        assert_eq!(l.holds, held);
        for other in p.rooms.rooms.iter().filter(|o| o.name != room.name) {
            for g in &other.grants {
                assert!(
                    !l.entries.contains(&g.words()),
                    "{} in {}",
                    g.words(),
                    room.name
                );
            }
        }
        assert_eq!(spawned(&l)[1].2, ["--room", room.name.as_str()]);
        assert_eq!(
            spawned(&l)[1].1,
            spawned(&home)[1].1,
            "the same handles in every room"
        );
    }
}

#[test]
fn chips_enter_rooms_and_a_vanished_room_leaves_for_home() {
    let mut p = Panel::default();
    p.reload(ROOMS);
    assert_eq!(p.current_name(), "");
    assert_eq!(p.pressed(ROOM, 1), Action::Entered);
    assert_eq!(p.current_name(), "project");
    assert_eq!(p.pressed(ROOM, 9), Action::Nothing);
    assert_eq!(p.current_name(), "project");
    assert_eq!(p.pressed(NEW_ROOM, 0), Action::Nothing);
    assert_eq!(p.pressed(LAUNCH, 7), Action::Nothing);
    assert_eq!(p.pressed(99, 0), Action::Nothing);
    // The same table again: nothing changed.
    assert!(!p.reload(ROOMS));
    // project removed by `room remove` in a shell: home again.
    assert!(p.reload(b"agent-1 --untrusted +w ~/sandbox/agent-1\n"));
    assert_eq!(p.current, None);
    assert_eq!(p.pressed(ROOM, 0), Action::Entered);
    assert_eq!(p.room(), None);
}

#[test]
fn the_compositor_is_told_the_table_and_the_dock() {
    let mut p = Panel::default();
    p.reload(b"b\na --colour 3 +r /system\n");
    assert_eq!(p.rooms_text(), "b --colour 1\na --colour 3 +r /system\n");
    let again = zoraya_settings::rooms::read(p.rooms_text().as_bytes());
    assert_eq!(again.rooms, p.rooms.rooms);
    assert_eq!(dock_text(), "term\tterm\nfiles\tfiles\n");
}

#[test]
fn the_table_kept_is_the_table_told() {
    let mut text = String::new();
    for i in 0..30 {
        text.push_str(&format!("r{i} +r /{}\n", "a".repeat(150)));
    }
    let mut p = Panel::default();
    p.reload(text.as_bytes());
    assert!(p.rooms_text().len() <= MAX_ROOMS_TEXT);
    assert!(p.rooms.rooms.len() < 30);
    assert!(!p.rooms.problems.is_empty());
}

/// Mutants 7d: a table of exactly `MAX_ROOMS_TEXT` is kept whole.
#[test]
fn a_table_at_the_limit_is_kept() {
    let mut head = String::new();
    for i in 0..17 {
        head.push_str(&format!("r{i} +r /{}\n", "a".repeat(150)));
    }
    let at = (1..200)
        .map(|n| format!("{head}last +r /{}\n", "b".repeat(n)))
        .find(|t| {
            let mut p = Panel::default();
            p.reload(t.as_bytes());
            p.rooms_text().len() == MAX_ROOMS_TEXT
        })
        .expect("some length is exactly at the limit");
    let mut p = Panel::default();
    p.reload(at.as_bytes());
    assert_eq!(p.rooms.rooms.len(), 18);
    assert!(p.rooms.problems.is_empty());
}

/// INV-0168-2's host half: of the applications only `files` holds the
/// volume's root; every other holds its data directory, its chooser
/// channel (not in an untrusted room) and what the person opened, and the
/// record shows each.
#[test]
fn an_application_gets_its_data_its_chooser_and_what_was_opened() {
    let mut p = Panel::default();
    assert!(p.reload(ROOMS));
    let gives =
        |l: &Launch| -> Vec<Give> { l.programs[0].handles.iter().map(|s| s.give).collect() };
    let opened = application(None, "t_open", Some("+r ~/notes.txt"));
    assert_eq!(
        gives(&opened),
        [
            Give::Console,
            Give::Compositor,
            Give::Chooser,
            Give::Data,
            Give::Opened
        ]
    );
    assert_eq!(opened.entries, ["+w /data/home/t_open", "+r ~/notes.txt"]);
    assert_eq!(opened.line, "t_open ~/notes.txt");
    let files = application(None, "files", None);
    assert!(gives(&files).contains(&Give::Root));
    assert!(gives(&files).contains(&Give::Launcher));
    assert!(files.entries.contains(&String::from("+w /")));
    for app in ["t_open", "edit"] {
        let l = application(None, app, None);
        assert!(!gives(&l).contains(&Give::Root), "{app}");
        assert!(!l.entries.iter().any(|e| e == "+w /"), "{app}");
    }
    // In a room: its grants, its own data directory, its line.
    let project = p.rooms.get("project").cloned();
    let l = application(project.as_ref(), "t_open", None);
    assert_eq!(l.entries[0], "+w ~/project");
    assert!(l.entries.contains(&String::from("+w /data/project/t_open")));
    assert_eq!(l.line, "room project t_open");
    // An untrusted room's applications get no chooser.
    let agent = p.rooms.get("agent-1").cloned();
    let l = application(agent.as_ref(), "t_open", None);
    assert!(!gives(&l).contains(&Give::Chooser));
    // The dock's files icon launches files; openers by extension.
    p.current = None;
    let Action::Launch(l) = p.pressed(LAUNCH, 1) else {
        panic!()
    };
    assert_eq!(l.app, "files");
    assert_eq!(opener("plán.TXT"), Some("t_open"));
    assert_eq!(opener("a.md"), Some("t_open"));
    assert_eq!(opener(".txt"), None);
    assert_eq!(opener("main.c"), None);
    assert_eq!(data_dir(None, "files"), "/data/home/files");
}

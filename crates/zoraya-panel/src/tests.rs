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
    // Only a shell in a room holds a keeper channel (INV-0269-2); `home`'s
    // terminal holds none.
    assert!(
        s.iter()
            .all(|(_, h, _)| h.iter().all(|x| x.1 != Give::Keeper))
    );
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
        // The same handles in every room, and a `Room(R)` channel to the
        // keeper in a room's shell alone (INV-0269-2).
        let mut with_keeper = spawned(&home)[1].1.clone();
        with_keeper.push((KEEPER_SLOT, Give::Keeper));
        assert_eq!(spawned(&l)[1].1, with_keeper);
        assert!(spawned(&l)[0].1.iter().all(|h| h.1 != Give::Keeper));
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
    assert_eq!(
        dock_text(),
        "term\tterm\nfiles\tfiles\nedit\tedit\nsettings\tsettings\n"
    );
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
    // INV-0168-2 as changed on 2026-11-10: `files` holds the volume to
    // read and `~` to change, in a start-up record, never the root.
    let files = application(None, "files", None);
    assert!(gives(&files).contains(&Give::Startup));
    assert!(gives(&files).contains(&Give::Launcher));
    assert_eq!(
        files.holds,
        [(String::from("/"), false), (String::from("~"), true)]
    );
    assert_eq!(files.entries, ["+r /", "+w ~", "+w /data/home/files"]);
    // In a room: the volume to read and the room's grants.
    let project = p.rooms.get("project").cloned();
    let in_room = application(project.as_ref(), "files", None);
    assert_eq!(in_room.holds[0], (String::from("/"), false));
    assert!(in_room.holds.contains(&(String::from("~/project"), true)));
    assert!(!in_room.holds.iter().any(|(p, _)| p == "~"));
    assert_eq!(in_room.line, "room project files");
    assert_eq!(in_room.programs[0].args, ["--room", "project"]);
    assert!(files.programs[0].args.is_empty());
    for app in ["t_open", "edit", "files"] {
        let l = application(None, app, None);
        assert!(!l.holds.iter().any(|(p, w)| p == "/" && *w), "{app}");
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
    assert_eq!(opener("plán.TXT"), Some("edit"));
    assert_eq!(opener("a.md"), Some("edit"));
    assert_eq!(opener("notes.test"), Some("t_open"));
    assert_eq!(opener(".txt"), None);
    assert_eq!(opener("main.c"), None);
    assert_eq!(data_dir(None, "files"), "/data/home/files");
}

/// INV-0169-4's host half: no launch the panel makes carries /config with
/// a writing right but the settings window's, and its record says so.
#[test]
fn only_the_settings_window_gets_config() {
    let mut p = Panel::default();
    assert!(p.reload(ROOMS));
    for (app, _) in APPS {
        let l = match app {
            "term" => terminal(None),
            a => application(None, a, None),
        };
        let config = l
            .programs
            .iter()
            .any(|pr| pr.handles.iter().any(|h| h.give == Give::Config));
        assert_eq!(config, app == "settings", "{app}");
        assert_eq!(
            l.entries.iter().any(|e| e == "+w /config/settings"),
            app == "settings",
            "{app}"
        );
    }
    for (_, app) in OPENERS {
        let l = application(None, app, Some("+r ~/a.txt"));
        assert!(
            !l.programs
                .iter()
                .any(|pr| pr.handles.iter().any(|h| h.give == Give::Config))
        );
    }
    assert_eq!(
        CONFIG_RIGHTS & zoraya_proto_file::rights::PUBLISH,
        zoraya_proto_file::rights::PUBLISH
    );
}

#[test]
fn the_settings_window_holds_the_six_rights_of_adr_0169() {
    use zoraya_proto_file::rights::*;
    // Read, write, create, publish, sync and clone; not remove or rename.
    assert_eq!(CONFIG_RIGHTS, 0b1110_0111);
    assert_eq!(
        CONFIG_RIGHTS,
        READ | WRITE | CREATE | PUBLISH | SYNC | CLONE
    );
    assert_eq!(CONFIG_RIGHTS & (REMOVE | RENAME), 0);
}

/// INV-0169-4 over `files`: a room's grant that would reach `/config` is
/// given to read, and `+w /` adds nothing to the volume it reads already.
#[test]
fn files_never_gets_config_to_change() {
    let mut p = Panel::default();
    p.reload(b"wide +w / +w /config +w /config/rooms +w ~/project\n");
    let wide = p.rooms.get("wide").cloned();
    assert!(wide.is_some(), "{:?}", p.rooms.problems);
    for room in [None, wide.as_ref()] {
        let l = application(room, "files", None);
        assert!(
            !l.holds.iter().any(|(path, w)| *w && reaches_config(path)),
            "{:?}",
            l.holds
        );
        assert!(
            !l.entries
                .iter()
                .any(|e| e == "+w /" || e.starts_with("+w /config"))
        );
    }
    let l = application(wide.as_ref(), "files", None);
    assert_eq!(
        l.holds,
        [
            (String::from("/"), false),
            (String::from("/config"), false),
            (String::from("/config/rooms"), false),
            (String::from("~/project"), true),
        ]
    );
    assert!(reaches_config("/"));
    assert!(reaches_config("/config/settings"));
    assert!(!reaches_config("/configs"));
    assert!(!reaches_config("~"));
}

/// ADR-0248, INV-0248-4: Open terminal here launches a terminal
/// in the file manager's room at the folder, only where that room's holds
/// reach.
#[test]
fn a_terminal_opens_at_a_folder_only_inside_its_rooms_holds() {
    let mut p = Panel::default();
    assert!(p.reload(ROOMS));
    let project = p.rooms.get("project").cloned();
    let l = terminal_at(project.as_ref(), "/home/ann/project/src", "/home/ann").unwrap();
    assert_eq!(l.cwd.as_deref(), Some("/home/ann/project/src"));
    assert_eq!(l.room.as_ref().map(|r| r.name.as_str()), Some("project"));
    assert_eq!(l.line, "room project term");
    // `~` is the shell's home.
    let l = terminal_at(project.as_ref(), "~/include", "/home/ann").unwrap();
    assert_eq!(l.cwd.as_deref(), Some("/home/ann/include"));
    // The toolchain the room's shell reads is a hold too.
    assert!(terminal_at(project.as_ref(), "/usr/include", "/home/ann").is_some());
    // Outside the room: refused.
    for bad in [
        "/home/ann",
        "/home/ann/other",
        "/config",
        "~/project/../secret",
        "",
        "rel/path",
        "/home/ann/project\n",
    ] {
        assert!(
            terminal_at(project.as_ref(), bad, "/home/ann").is_none(),
            "{bad:?}"
        );
    }
    // In home the volume is read, so any folder; the home terminal's
    // record is home's.
    let l = terminal_at(None, "/system", "/home/ann").unwrap();
    assert_eq!(l.cwd.as_deref(), Some("/system"));
    assert_eq!(l.holds, terminal(None).holds);
    // An ordinary launch has no current directory of its own.
    assert!(terminal(None).cwd.is_none());
}

/// Open in edit (ADR-0248): `edit` for any name, nothing else by
/// name; an empty request is by extension.
#[test]
fn the_file_manager_may_ask_for_edit_alone() {
    assert_eq!(opener_for("main.c", "edit"), Some("edit"));
    assert_eq!(opener_for("main.c", ""), None);
    assert_eq!(opener_for("notes.test", ""), Some("t_open"));
    assert_eq!(opener_for("notes.txt", "term"), None);
    assert_eq!(opener_for("notes.txt", "settings"), None);
    assert_eq!(opener_for("", "edit"), None);
}

/// Mutation testing (7h): control characters anywhere, `~user` and `~x`
/// are refused; only `files` gets `--room`.
#[test]
fn terminal_at_refuses_odd_paths_and_only_files_gets_a_room_argument() {
    let mut p = Panel::default();
    assert!(p.reload(ROOMS));
    let project = p.rooms.get("project").cloned();
    for bad in [
        "/home/ann/project/a\nb",
        "~/project/a\tb",
        "~project",
        "~x/y",
    ] {
        assert!(
            terminal_at(project.as_ref(), bad, "/home/ann").is_none(),
            "{bad:?}"
        );
        assert!(
            terminal_at(None, bad, "/home/ann").is_none(),
            "{bad:?} in home"
        );
    }
    for app in ["edit", "settings", "t_open"] {
        assert!(
            application(project.as_ref(), app, None).programs[0]
                .args
                .is_empty(),
            "{app}"
        );
    }
}

/// Review of the fix branch, 2: with `shell.home` at `/` or under
/// `/config`, `files`' `~` is given to read only.
#[test]
fn files_gets_no_config_through_its_home() {
    for home in ["/", "/config", "/config/me"] {
        let mut l = application(None, "files", None);
        confine_files(&mut l, home);
        assert_eq!(
            l.holds,
            [(String::from("/"), false), (String::from("~"), false)],
            "{home}"
        );
        assert_eq!(l.entries, ["+r /", "+r ~", "+w /data/home/files"]);
    }
    let mut l = application(None, "files", None);
    confine_files(&mut l, "/home/ann");
    assert_eq!(l.holds[1], (String::from("~"), true));
    assert_eq!(l.entries, ["+r /", "+w ~", "+w /data/home/files"]);
    // Only `files`: a terminal in home keeps /config for `settings`.
    let mut t = terminal(None);
    confine_files(&mut t, "/");
    assert!(t.holds.contains(&(String::from("/config"), true)));
}

/// What `set_room_limit` touched, with a file and a keeper that record it.
struct LimitWorld {
    file: Vec<u8>,
    writes: usize,
    asked: Vec<String>,
    refuse: Option<KeeperFail>,
}

impl LimitWorld {
    fn new() -> LimitWorld {
        LimitWorld {
            file: ROOMS.to_vec(),
            writes: 0,
            asked: Vec::new(),
            refuse: None,
        }
    }

    fn set(
        &mut self,
        untrusted_origin: bool,
        room: &str,
        word: &str,
        value: &str,
    ) -> Result<(), String> {
        // The three closures share the world through a cell.
        let w = core::cell::RefCell::new(self);
        set_room_limit(
            untrusted_origin,
            room,
            word,
            value,
            || w.borrow().file.clone(),
            |b| {
                let mut w = w.borrow_mut();
                w.writes += 1;
                w.file = b.to_vec();
                Ok(())
            },
            Some(|name: &str| {
                let mut w = w.borrow_mut();
                w.asked.push(name.into());
                w.refuse.take().map_or(Ok(()), Err)
            }),
        )
    }
}

/// INV-0294-3: a settings window launched in an untrusted room cannot set a
/// limit: nothing is read back, written or asked of the keeper.
#[test]
fn an_untrusted_origin_sets_no_limit() {
    let mut w = LimitWorld::new();
    let err = w.set(true, "project", "--mem", "64M").unwrap_err();
    assert!(err.contains("from home"), "{err}");
    assert_eq!((w.writes, w.asked.len()), (0, 0));
    assert_eq!(w.file, ROOMS);
}

#[test]
fn a_trusted_origin_rewrites_the_line_and_asks_the_keeper() {
    let mut w = LimitWorld::new();
    assert_eq!(w.set(false, "project", "--mem", "64M"), Ok(()));
    assert_eq!(
        (w.writes, w.asked.clone()),
        (1, vec![String::from("project")])
    );
    assert!(
        String::from_utf8(w.file.clone())
            .unwrap()
            .contains("--mem 64M")
    );
}

#[test]
fn a_refusal_of_the_keeper_puts_the_old_line_back() {
    let mut w = LimitWorld::new();
    w.refuse = Some(KeeperFail::Denied(String::from("over the ceiling")));
    let err = w.set(false, "project", "--mem", "64M").unwrap_err();
    assert_eq!(err, "project: over the ceiling");
    assert_eq!(w.file, ROOMS, "the old line is back");
    assert_eq!(w.writes, 2);
    w.refuse = Some(KeeperFail::Gone);
    let err = w.set(false, "project", "--mem", "64M").unwrap_err();
    assert_eq!(err, "the room keeper does not answer");
    // A bad word is refused before anything is written.
    let before = w.writes;
    assert!(w.set(false, "project", "--nonsense", "1").is_err());
    assert_eq!(w.writes, before);
}

#[test]
fn a_refusal_of_the_keeper_keeps_an_edit_made_while_it_was_asked() {
    // Another writer adds a line while the keeper is asked about
    // project's: the old line of project goes back and the other edit stays.
    let file = core::cell::RefCell::new(ROOMS.to_vec());
    let r = set_room_limit(
        false,
        "project",
        "--mem",
        "64M",
        || file.borrow().clone(),
        |b| {
            *file.borrow_mut() = b.to_vec();
            Ok(())
        },
        Some(|_: &str| {
            file.borrow_mut().extend_from_slice(b"other --colour 2\n");
            Err(KeeperFail::Denied(String::from("over the ceiling")))
        }),
    );
    assert_eq!(r, Err(String::from("project: over the ceiling")));
    let now = file.borrow().clone();
    let text = core::str::from_utf8(&now).unwrap();
    assert!(text.starts_with("project --colour 1 +w ~/project +r ~/include\n"));
    assert!(text.ends_with("other --colour 2\n"), "{text}");
    assert!(!text.contains("--mem"), "{text}");
}

#[test]
fn without_a_keeper_the_line_is_written_and_kept() {
    let file = core::cell::RefCell::new(ROOMS.to_vec());
    let r = set_room_limit(
        false,
        "project",
        "--procs",
        "8",
        || file.borrow().clone(),
        |b| {
            *file.borrow_mut() = b.to_vec();
            Ok(())
        },
        None::<fn(&str) -> Result<(), KeeperFail>>,
    );
    assert_eq!(r, Ok(()));
    assert_ne!(*file.borrow(), ROOMS);
}

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
#[test]
fn a_launch_gives_exactly_its_record_in_its_room() {
    let mut p = Panel::default();
    assert!(p.reload(ROOMS));
    let home = terminal(None);
    assert_eq!(home.room, None);
    assert_eq!(home.entries, ["+w /"]);
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
            (5, Give::Root),
            (6, Give::Terminal)
        ]
    );
    assert!(s[1].2.is_empty());
    // Only the shell holds the root; the terminal holds no directory.
    for (image, handles, _) in &s {
        let root = handles.iter().any(|h| h.1 == Give::Root);
        assert_eq!(root, *image == "zosh", "{image}");
    }
    // Every room: its own grants, its name, nothing of another room.
    for (i, room) in p.rooms.rooms.clone().iter().enumerate() {
        assert_eq!(p.pressed(ROOM, i as u32 + 1), Action::Entered);
        let Action::Launch(l) = p.pressed(LAUNCH, 0) else {
            panic!()
        };
        assert_eq!(
            l.room.as_ref().map(|r| r.name.as_str()),
            Some(room.name.as_str())
        );
        assert_eq!(l.line, format!("room {} term", room.name));
        let mine: Vec<String> = room.grants.iter().map(|g| g.words()).collect();
        assert_eq!(l.entries[1..], mine[..]);
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
    assert_eq!(dock_text(), "term\tterm\n");
}

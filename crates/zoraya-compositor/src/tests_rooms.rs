//! Stage 7d: rooms and the dock (ADR-0172, ADR-0168), against the fake of
//! `tests.rs`: the client manager's ordinals, the records no client can
//! change (INV-0172-1), the frame's room pixels and the untrusted edge
//! (INV-0172-10), fields only between rooms (INV-0172-6), the dock's
//! presses and its place above every window (INV-0168-3), show-commands
//! and the gesture table (INV-0172-5).

use super::*;
use crate::draw_rooms::{GESTURES, TITLE_TINT, WITHOUT_TEXT};
use crate::rooms::Cell;

const ROOMS: &str =
    "project --colour 1 +w ~/project +r ~/include\nagent-1 --untrusted +w ~/sandbox/agent-1\n";

fn axis(p: i64, side: u32) -> u32 {
    let d = i64::from(side - 1);
    ((p * 65535 + d - 1) / d) as u32
}

fn done(s: &mut Setup, end: u32) -> u32 {
    let out = s.f().take(end);
    let last = out.last().expect("a reply");
    let (_, d) = decode::<gui::ManagerDone>(last, 0).unwrap_or_else(|e| panic!("{e:?}: {out:?}"));
    d.status.0
}

impl Setup {
    /// The panel's client manager, attached by the driver manager.
    fn panel(&mut self) -> u32 {
        let end = self.f().channel(64);
        let m = self.manager;
        self.f()
            .send(m, message(&gui::AttachClientManager {}, 9), vec![end]);
        self.c.step();
        assert_eq!(done(self, m), status::OK);
        end
    }

    /// A client of `room` (empty: `home`) for `app`, added by the panel.
    fn room_client(&mut self, panel: u32, room: &str, app: &str, entries: &str) -> u32 {
        let r = self.c.rooms().get(room).cloned();
        let (untrusted, colour) =
            r.map_or((0, 0), |r| (u32::from(r.untrusted), u32::from(r.colour)));
        let end = self.f().channel(64);
        let m = gui::AddRoomClient {
            untrusted,
            colour,
            room: zoraya_protocol::Data(room.as_bytes()),
            app: zoraya_protocol::Data(app.as_bytes()),
            entries: zoraya_protocol::Data(entries.as_bytes()),
            line: zoraya_protocol::Data(b"term"),
        };
        self.f().send(panel, message(&m, 7), vec![end]);
        self.c.step();
        assert_eq!(done(self, panel), status::OK);
        end
    }

    fn set_rooms(&mut self, panel: u32, current: &str, table: &str) -> u32 {
        let m = gui::SetRooms {
            current: zoraya_protocol::Data(current.as_bytes()),
            rooms: zoraya_protocol::Data(table.as_bytes()),
        };
        self.f().send(panel, message(&m, 4), vec![]);
        self.c.step();
        done(self, panel)
    }

    fn set_dock(&mut self, panel: u32, list: &str) -> u32 {
        let m = gui::SetDock {
            apps: zoraya_protocol::Data(list.as_bytes()),
        };
        self.f().send(panel, message(&m, 4), vec![]);
        self.c.step();
        done(self, panel)
    }

    /// A toplevel of an existing client.
    fn window_of(&mut self, end: u32, w: u32, h: u32, colour: u32) -> u32 {
        let s = self.c.scale().factor();
        let n = (w * s * h * s) as usize;
        let memory = self.f().memory(vec![colour; n], n as u64 * 4, false);
        let m = gui::CreateToplevel {
            width: w,
            height: h,
        };
        self.f().send(end, message(&m, 5), vec![memory]);
        self.c.step();
        let out = self.f().take(end);
        let (_, created) = out
            .iter()
            .find_map(|r| decode::<gui::Created>(r, 0).ok())
            .unwrap();
        let id = created.window;
        let c = gui::CommitTo {
            window: id,
            x: 0,
            y: 0,
            width: w * s,
            height: h * s,
        };
        self.f().send(end, message(&c, 7), vec![]);
        self.c.step();
        self.f().take(end);
        id
    }

    fn press_at(&mut self, x: i64, y: i64) {
        let (w, h) = self.f().display_size;
        self.point(axis(x, w), axis(y, h), Some(1));
        self.point(axis(x, w), axis(y, h), Some(0));
    }
}

fn dock_pressed(out: &[Vec<u8>]) -> Vec<(u32, u32)> {
    out.iter()
        .filter_map(|m| decode::<gui::DockPressed>(m, 0).ok())
        .map(|(_, d)| (d.kind, d.index))
        .collect()
}

#[test]
fn each_manager_takes_only_its_own_ordinals() {
    let mut s = setup();
    let panel = s.panel();
    // A second client manager is refused; so are the drivers on the panel's
    // channel and the panel's messages on init's.
    let other = s.f().channel(64);
    let m = s.manager;
    s.f()
        .send(m, message(&gui::AttachClientManager {}, 9), vec![other]);
    s.c.step();
    assert_eq!(done(&mut s, m), status::UNSUPPORTED);
    let d = s.f().channel(64);
    s.f()
        .send(panel, message(&gui::AttachDisplay {}, 9), vec![d]);
    s.c.step();
    assert_eq!(done(&mut s, panel), status::UNSUPPORTED);
    let m2 = s.manager;
    let rooms = gui::SetRooms {
        current: zoraya_protocol::Data(b""),
        rooms: zoraya_protocol::Data(b""),
    };
    s.f().send(m2, message(&rooms, 9), vec![]);
    s.c.step();
    assert_eq!(done(&mut s, m2), status::UNSUPPORTED);
    // Records that do not fit their room are refused whole.
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    for (room, untrusted, colour, app, entries) in [
        ("project", 0, 0, "term", ""),
        ("agent-1", 1, 2, "term", ""),
        ("", 1, 0, "term", ""),
        ("Bad", 0, 1, "term", ""),
        ("project", 0, 1, "", ""),
        ("project", 0, 1, "term", "+w ~/a\u{1b}[31m"),
        ("project", 2, 1, "term", ""),
    ] {
        let end = s.f().channel(64);
        let m = gui::AddRoomClient {
            untrusted,
            colour,
            room: zoraya_protocol::Data(room.as_bytes()),
            app: zoraya_protocol::Data(app.as_bytes()),
            entries: zoraya_protocol::Data(entries.as_bytes()),
            line: zoraya_protocol::Data(b""),
        };
        s.f().send(panel, message(&m, 7), vec![end]);
        s.c.step();
        assert_eq!(
            done(&mut s, panel),
            status::INVALID,
            "{room} {untrusted} {colour} {app}"
        );
    }
    assert_eq!(s.c.client_count(), 0);
    // A bad table, or a current room not in it, changes nothing.
    assert_eq!(s.set_rooms(panel, "nowhere", ROOMS), status::INVALID);
    assert_eq!(s.set_rooms(panel, "", "Bad line\n"), status::INVALID);
    assert_eq!(s.c.rooms().rooms.len(), 2);
    assert_eq!(s.set_dock(panel, "term"), status::INVALID);
    assert_eq!(s.set_dock(panel, "term\tterm\n"), status::OK);
    // The panel going away leaves the clients and the table.
    let a = s.room_client(panel, "project", "term", "+w ~/project");
    let _ = a;
    s.f().peers.get_mut(&panel).unwrap().gone = true;
    s.c.step();
    assert_eq!(s.c.client_count(), 1);
    assert_eq!(s.c.rooms().rooms.len(), 2);
}

/// INV-0172-1: nothing a client sends changes its record, the table or the
/// dock, and a title that imitates a badge leaves the badge's pixels.
#[test]
fn no_client_message_changes_a_record_or_its_pixels() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "project", ROOMS), status::OK);
    let a = s.room_client(panel, "project", "term", "+w ~/project\n+r ~/include");
    let _id = s.window_of(a, 400, 200, 0x0011_2233);
    let o = s.outer(0, 1);
    let badge_area: Vec<u32> = (o.right() - 120..o.right() - 2)
        .map(|x| s.pixel(x, o.y + 14))
        .collect();
    let record = s.c.record(0).cloned();
    let rooms = s.c.rooms().clone();
    let spoof = "agent-1 untrusted +w ~/ · project +w ~/project +w / ".repeat(4);
    s.request(
        a,
        &gui::SetTitle {
            window: 1,
            text: zoraya_protocol::Data(spoof.as_bytes()),
        },
        vec![],
    );
    for ordinal in gui::CLIENT_REQUESTS
        .iter()
        .chain(&gui::CLIENT_MANAGER_REQUESTS)
        .chain(&[32, 38])
    {
        let mut bytes = message(&gui::Stats {}, 1);
        bytes[..4].copy_from_slice(&ordinal.to_le_bytes());
        bytes.extend_from_slice(&[0x41; 64]);
        s.f().send(a, bytes, vec![]);
        s.c.step();
        if s.c.client_count() == 0 {
            break;
        }
    }
    assert_eq!(s.c.record(0).cloned().or(record.clone()), record);
    assert_eq!(s.c.rooms(), &rooms);
    if s.c.client_count() == 1 {
        let now: Vec<u32> = (o.right() - 120..o.right() - 2)
            .map(|x| s.pixel(x, o.y + 14))
            .collect();
        assert_eq!(now, badge_area, "the badge and pills are the compositor's");
    }
}

/// The frame of a room's window: its title bar tinted, the badge's dot in
/// the room's colour; the untrusted edge on every untrusted window, alone,
/// beside a trusted one and tiled, and on no trusted one (INV-0172-10).
#[test]
fn frames_carry_their_room_and_untrusted_windows_their_edge() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let p = *s.c.appearance().palette();
    let edge_depth = space::ROOM_UNTRUSTED_EDGE.physical(Scale::ONE) as i64;
    let agent = s.room_client(panel, "agent-1", "term", "+w ~/sandbox/agent-1");
    s.window_of(agent, 300, 160, 0x0040_4040);
    // Mid-height on the left side (a later window may cover the right).
    let check_edge = |s: &mut Setup, client: usize, untrusted: bool| {
        let o = s.outer(client, 1);
        let mid = o.y + o.height / 2;
        for d in 0..edge_depth {
            let left = s.pixel(o.x + 1 + d, mid);
            if untrusted {
                assert_eq!(left, p.room_untrusted.pixel(), "depth {d}");
            } else {
                assert_ne!(left, p.room_untrusted.pixel());
                for slot in 1..=4 {
                    assert_ne!(left, p.room(slot).pixel());
                }
            }
        }
        // Not past the edge's depth.
        let inner = s.pixel(o.x + 1 + edge_depth, mid);
        assert_ne!(inner, p.room_untrusted.pixel());
    };
    check_edge(&mut s, 0, true);
    // Alone, its right side too.
    let o = s.outer(0, 1);
    for d in 0..edge_depth {
        assert_eq!(
            s.pixel(o.right() - 2 - d, o.y + o.height / 2),
            p.room_untrusted.pixel()
        );
    }
    let project = s.room_client(panel, "project", "term", "+w ~/project");
    s.window_of(project, 300, 160, 0x0040_4040);
    check_edge(&mut s, 0, true);
    check_edge(&mut s, 1, false);
    // The project window's title bar: the focused ground tinted by room.1,
    // where nothing else is drawn (its top edge, past the buttons).
    let o = s.outer(1, 1);
    let ground = if s.c.focus() == Some(1) {
        p.bg_frame_focused
    } else {
        p.bg_frame
    };
    assert_eq!(
        s.pixel(o.x + 70, o.y + 3),
        ground.blend(p.room(1), TITLE_TINT).pixel()
    );
    // A home window has no tint.
    let home = s.room_client(panel, "", "edit", "+w notes.txt");
    s.window_of(home, 200, 100, 0x0040_4040);
    let o = s.outer(2, 1);
    let ground = if s.c.focus() == Some(2) {
        p.bg_frame_focused
    } else {
        p.bg_frame
    };
    assert_eq!(s.pixel(o.x + 70, o.y + 3), ground.pixel());
    check_edge(&mut s, 2, false);
    // Tiled to the left half, the untrusted window keeps its edge.
    s.press_at(s.outer(0, 1).x + 150, s.outer(0, 1).y + 14);
    let meta = code::RIGHT_META;
    s.keys(&[(meta, 1), (code::LEFT, 1), (code::LEFT, 0), (meta, 0)]);
    check_edge(&mut s, 0, true);
}

/// INV-0172-6: with one room's windows no wallpaper pixel is tinted; with
/// two, the padding around a window is.
#[test]
fn fields_are_drawn_only_between_rooms() {
    let mut s = setup_with(|f| f.display_size = (640, 480));
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let a = s.room_client(panel, "project", "term", "");
    s.window_of(a, 200, 120, 0x0040_4040);
    let b = s.room_client(panel, "project", "edit", "");
    s.window_of(b, 160, 80, 0x0040_4040);
    let wall = s.c.wallpaper.clone();
    let o = s.outer(0, 1);
    let probe = (o.x - 10, o.y + 30);
    let w = 640usize;
    let at = |v: &[u32], (x, y): (i64, i64)| v[y as usize * w + x as usize];
    assert!(s.c.fielded == wall, "one room: the wallpaper as it is");
    // A second room's window, far from the first: now a field.
    let h = s.room_client(panel, "", "files", "");
    s.window_of(h, 100, 60, 0x0040_4040);
    let p = *s.c.appearance().palette();
    let tinted = zoraya_tokens::Color::of_pixel(at(&wall, probe))
        .blend(p.room(1), crate::fields::GROUND_ALPHA);
    assert_eq!(at(&s.c.fielded, probe), tinted.pixel());
    // Every tinted pixel is out of the bar and the dock's strip.
    let work = s.c.work_area();
    for (i, (q, r)) in s.c.fielded.iter().zip(&wall).enumerate() {
        if q != r {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            assert!(work.contains(x, y), "({x}, {y})");
        }
    }
    // Hiding the home window (its minimise button) leaves one room on
    // screen: no field.
    let o = s.outer(2, 1);
    let f = s.c.frame();
    s.press_at(
        o.x + i64::from(crate::window::MINIMISE_X),
        o.y + f.title / 2,
    );
    assert!(s.c.window(Key { client: 2, id: 1 }).unwrap().hidden);
    assert!(s.c.fielded == wall, "one room on screen again");
}

#[test]
fn the_dock_launches_raises_restores_and_enters_rooms() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "project", ROOMS), status::OK);
    assert_eq!(
        s.set_dock(panel, "term\tterm\nedit\tedit FILE\n"),
        status::OK
    );
    let cells: Vec<Cell> = s.c.dock().cells.iter().map(|c| c.0).collect();
    assert_eq!(
        cells,
        [
            Cell::App(0),
            Cell::App(1),
            Cell::Chip(0),
            Cell::Chip(1),
            Cell::Chip(2),
            Cell::Plus
        ]
    );
    let centre = |s: &Setup, c: Cell| {
        let r = s.c.dock().cells.iter().find(|x| x.0 == c).unwrap().1;
        (r.x + r.width / 2, r.y + r.height / 2)
    };
    // No window yet: the panel is asked to launch.
    let (x, y) = centre(&s, Cell::App(0));
    s.press_at(x, y);
    assert_eq!(dock_pressed(&s.f().take(panel)), [(gui::dock::LAUNCH, 0)]);
    let (x, y) = centre(&s, Cell::Chip(2));
    s.press_at(x, y);
    assert_eq!(dock_pressed(&s.f().take(panel)), [(gui::dock::ROOM, 2)]);
    let (x, y) = centre(&s, Cell::Plus);
    s.press_at(x, y);
    assert_eq!(dock_pressed(&s.f().take(panel)), [(gui::dock::NEW_ROOM, 0)]);
    // System+Enter: a terminal.
    s.keys(&[
        (code::RIGHT_META, 1),
        (code::ENTER, 1),
        (code::ENTER, 0),
        (code::RIGHT_META, 0),
    ]);
    assert_eq!(dock_pressed(&s.f().take(panel)), [(gui::dock::LAUNCH, 0)]);
    // Two terminals in two rooms: two dots, home's first.
    let t1 = s.room_client(panel, "project", "term", "");
    s.window_of(t1, 200, 100, 1);
    let t2 = s.room_client(panel, "", "term", "");
    s.window_of(t2, 200, 100, 2);
    let rooms: Vec<String> = s.c.app_rooms("term").into_iter().map(|r| r.0).collect();
    assert_eq!(rooms, ["home", "project"]);
    // Hidden, the terminal's windows come back from its icon; no launch.
    s.keys(&[
        (code::RIGHT_META, 1),
        (code::M, 1),
        (code::M, 0),
        (code::RIGHT_META, 0),
    ]);
    let hidden =
        s.c.stack()
            .filter(|k| s.c.window(*k).unwrap().hidden)
            .count();
    assert_eq!(hidden, 1);
    let (x, y) = centre(&s, Cell::App(0));
    s.press_at(x, y);
    assert!(dock_pressed(&s.f().take(panel)).is_empty());
    assert!(s.c.stack().all(|k| !s.c.window(k).unwrap().hidden));
    // An application with a window but not in the list gets an icon.
    let other = s.room_client(panel, "", "files", "");
    s.window_of(other, 100, 60, 3);
    assert_eq!(s.c.dock_list().last().unwrap().name, "files");
}

/// INV-0168-3: the dock is above every window; a press there is the
/// dock's, whatever a window is under it.
#[test]
fn the_dock_is_never_covered() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_dock(panel, "term\tterm\n"), status::OK);
    let a = s.room_client(panel, "", "edit", "");
    s.window_of(a, 600, 400, 0x00ab_cdef);
    let (cell, r) = s.c.dock().cells[0];
    assert_eq!(cell, Cell::App(0));
    let (x, y) = (r.x + r.width / 2, r.y + r.height / 2);
    assert!(
        s.outer(0, 1).contains(x, y),
        "the window reaches under the dock"
    );
    assert_ne!(s.pixel(x, y), 0x00ab_cdef);
    s.press_at(x, y);
    assert_eq!(dock_pressed(&s.f().take(panel)), [(gui::dock::LAUNCH, 0)]);
    let out = s.f().take(a);
    assert!(
        !out.iter()
            .any(|m| decode::<gui::WindowPointer>(m, 0).is_ok())
    );
}

#[test]
fn show_commands_grows_the_dock_and_shrinks_the_work_area() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_dock(panel, "term\tterm\n"), status::OK);
    let (strip, work) = (s.c.dock_strip(), s.c.work_area());
    let toggle = [
        (code::RIGHT_META, 1),
        (code::SLASH, 1),
        (code::SLASH, 0),
        (code::RIGHT_META, 0),
    ];
    s.keys(&toggle);
    assert!(s.c.commands());
    assert_eq!(
        s.c.dock_strip(),
        strip + i64::from(crate::rooms::LABEL_ZONE)
    );
    assert_eq!(
        s.c.work_area().height,
        work.height - i64::from(crate::rooms::LABEL_ZONE)
    );
    s.keys(&toggle);
    assert!(!s.c.commands());
    assert_eq!(s.c.work_area(), work);
}

/// INV-0172-5: every gesture handler has a line or a stated reason for
/// none, and every line parses under ADR-0150's grammar.
#[test]
fn every_gesture_has_its_text() {
    // The handlers the input code dispatches to, by the names it uses.
    let source = String::from(include_str!("lib.rs")) + include_str!("rooms.rs");
    let handlers = [
        ("close", "fn ask_close"),
        ("minimise", "Zone::Minimise => self.set_hidden"),
        ("dock-launch", "gui::dock::LAUNCH"),
        ("dock-room", "gui::dock::ROOM"),
        ("dock-new-room", "gui::dock::NEW_ROOM"),
        ("new-terminal", "fn new_terminal"),
        ("layout", "fn layout_changed"),
        ("switcher", "fn open_switcher"),
        ("next-window", "fn next_window"),
        ("tile", "fn tile"),
        ("move", "Drag::Move"),
        ("resize", "Drag::Resize"),
        ("show-commands", "code::SLASH"),
    ];
    let named: Vec<&str> = GESTURES.iter().chain(&WITHOUT_TEXT).map(|g| g.0).collect();
    for (name, needle) in handlers {
        assert!(
            source.contains(needle),
            "{needle} is gone: update the table"
        );
        assert!(named.contains(&name), "{name} has no line and no reason");
    }
    assert_eq!(
        named.len(),
        handlers.len(),
        "a gesture in the table with no handler"
    );
    // The chords the compositor takes: each is a handler above.
    for chord in [
        "code::W =>",
        "code::M =>",
        "code::LEFT =>",
        "code::RIGHT =>",
        "code::ENTER",
        "code::SLASH",
        "code::SPACE",
    ] {
        assert!(source.contains(chord), "{chord}");
    }
    for (_, line) in GESTURES {
        let concrete = line
            .replace("APP", "term")
            .replace("NAME", "project")
            .replace("LAYOUT", "ua");
        assert!(zoraya_shell::syntax::parse(&concrete).is_ok(), "{concrete}");
    }
}

/// Scenes of rooms and the dock for TASK-24's report:
/// `ZORAYA_COMPOSITOR_SAMPLES=dir cargo test -p zoraya-compositor -- --ignored rooms_samples`.
#[test]
#[ignore = "writes images"]
fn rooms_samples() {
    let Ok(dir) = std::env::var("ZORAYA_COMPOSITOR_SAMPLES") else {
        return;
    };
    for (name, appearance) in [
        ("light", zoraya_tokens::Appearance::Light),
        ("dark", zoraya_tokens::Appearance::Dark),
    ] {
        let mut s = setup_with(|f| f.display_size = (1024, 768));
        s.c.appearance = appearance;
        s.c.make_wallpaper();
        let p = *appearance.palette();
        let panel = s.panel();
        assert_eq!(s.set_rooms(panel, "project", ROOMS), status::OK);
        assert_eq!(
            s.set_dock(
                panel,
                "term\tterm\nfiles\tfiles ~\nedit\tedit FILE\nsettings\tsettings\n"
            ),
            status::OK
        );
        let title = |s: &mut Setup, end: u32, t: &str| {
            s.request(
                end,
                &gui::SetTitle {
                    window: 1,
                    text: zoraya_protocol::Data(t.as_bytes()),
                },
                vec![],
            );
        };
        let drag = |s: &mut Setup, client: usize, to: (i64, i64)| {
            let o = s.outer(client, 1);
            let (w, h) = (1024, 768);
            s.point(axis(o.x + o.width / 2, w), axis(o.y + 14, h), Some(1));
            s.point(axis(to.0, w), axis(to.1, h), None);
            s.point(axis(to.0, w), axis(to.1, h), Some(0));
        };
        let home = s.room_client(panel, "", "files", "+w ~");
        s.window_of(home, 300, 200, p.bg_view.pixel());
        title(&mut s, home, "Files — ~");
        let project = s.room_client(panel, "project", "term", "+w ~/project\n+r ~/include");
        s.window_of(project, 460, 280, p.term_bg.pixel());
        title(&mut s, project, "zosh — ~/project");
        let agent = s.room_client(panel, "agent-1", "term", "+w ~/sandbox/agent-1");
        s.window_of(agent, 330, 200, p.term_bg.pixel());
        title(&mut s, agent, "zosh — ~/sandbox/agent-1");
        // The top-most first, so that no press lands on a moved window.
        drag(&mut s, 2, (800, 300));
        drag(&mut s, 1, (330, 90));
        drag(&mut s, 0, (230, 470));
        let write = |s: &mut Setup, file: &str| {
            let mut out = b"P6\n1024 768\n255\n".to_vec();
            for q in &s.f().frame {
                out.extend_from_slice(&[(q >> 16) as u8, (q >> 8) as u8, *q as u8]);
            }
            std::fs::write(std::format!("{dir}/{file}-{name}.ppm"), out).unwrap();
        };
        // The pointer on the terminal's icon: its tooltip.
        let r = s.c.dock().cells[0].1;
        s.point(axis(r.x + r.width / 2, 1024), axis(r.y + 10, 768), None);
        write(&mut s, "rooms");
        s.point(axis(1000, 1024), axis(40, 768), None);
        s.keys(&[
            (code::RIGHT_META, 1),
            (code::SLASH, 1),
            (code::SLASH, 0),
            (code::RIGHT_META, 0),
        ]);
        write(&mut s, "commands");
    }
}

/// INV-0243-1: a room's colour reaches a window's body only through the
/// untrusted edge; every other body pixel is the client's.
#[test]
fn a_rooms_window_shows_its_body_unchanged() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let body_colour = 0x0012_3456;
    let edge = space::ROOM_UNTRUSTED_EDGE.physical(Scale::ONE) as i64;
    // Each window checked while it is the top one (no other's shadow on
    // it): a trusted room's, then an untrusted room's.
    for (client, room, skip) in [(0usize, "project", 0i64), (1, "agent-1", edge)] {
        let end = s.room_client(panel, room, "term", "+w ~/x");
        s.window_of(end, 200, 100, body_colour);
        let body = s.c.window(Key { client, id: 1 }).unwrap().body_rect();
        // A row through the middle, inside the edge where there is one.
        let y = body.y + body.height / 2;
        for x in body.x + skip..body.right() - skip {
            assert_eq!(s.pixel(x, y), body_colour, "{room} at ({x}, {y})");
        }
        if skip > 0 {
            let p = *s.c.appearance().palette();
            assert_eq!(s.pixel(body.x, y), p.room_untrusted.pixel());
            assert_eq!(s.pixel(body.right() - 1, y), p.room_untrusted.pixel());
        }
    }
}

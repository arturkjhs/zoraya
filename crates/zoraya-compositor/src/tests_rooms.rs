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

pub(super) fn axis(p: i64, side: u32) -> u32 {
    let d = i64::from(side - 1);
    ((p * 65535 + d - 1) / d) as u32
}

pub(super) fn done(s: &mut Setup, end: u32) -> u32 {
    let out = s.f().take(end);
    let last = out.last().expect("a reply");
    let (_, d) = decode::<gui::ManagerDone>(last, 0).unwrap_or_else(|e| panic!("{e:?}: {out:?}"));
    d.status.0
}

impl Setup {
    /// The panel's client manager, attached by the driver manager.
    pub(super) fn panel(&mut self) -> u32 {
        let end = self.f().channel(64);
        let m = self.manager;
        self.f()
            .send(m, message(&gui::AttachClientManager {}, 9), vec![end]);
        self.c.step();
        assert_eq!(done(self, m), status::OK);
        end
    }

    /// A client of `room` (empty: `home`) for `app`, added by the panel.
    pub(super) fn room_client(&mut self, panel: u32, room: &str, app: &str, entries: &str) -> u32 {
        let r = self.c.rooms().get(room).cloned();
        let (untrusted, colour) =
            r.map_or((0, 0), |r| (u32::from(r.untrusted), u32::from(r.colour)));
        let end = self.f().channel(64);
        let m = gui::AddRoomClient {
            untrusted,
            colour,
            tag: 0,
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

    pub(super) fn set_rooms(&mut self, panel: u32, current: &str, table: &str) -> u32 {
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
    pub(super) fn window_of(&mut self, end: u32, w: u32, h: u32, colour: u32) -> u32 {
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

    pub(super) fn press_at(&mut self, x: i64, y: i64) {
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
    // SetDock on init's channel; AttachInput and Stats on the panel's.
    let m3 = s.manager;
    s.f().send(
        m3,
        message(
            &gui::SetDock {
                apps: zoraya_protocol::Data(b"x\tx\n"),
            },
            9,
        ),
        vec![],
    );
    s.c.step();
    assert_eq!(done(&mut s, m3), status::UNSUPPORTED);
    let i = s.f().channel(64);
    s.f().send(panel, message(&gui::AttachInput {}, 9), vec![i]);
    s.c.step();
    assert_eq!(done(&mut s, panel), status::UNSUPPORTED);
    s.f().send(panel, message(&gui::Stats {}, 9), vec![]);
    s.c.step();
    let out = s.f().take(panel);
    assert!(
        decode::<gui::StatsReply>(out.last().unwrap(), 0).is_ok(),
        "Stats on either"
    );
    // AddRoomClient on init's channel: its first terminal (ADR-0242).
    let t = s.f().channel(64);
    let m4 = s.manager;
    let first = gui::AddRoomClient {
        untrusted: 0,
        colour: 0,
        tag: 0,
        room: zoraya_protocol::Data(b""),
        app: zoraya_protocol::Data(b"term"),
        entries: zoraya_protocol::Data(b"+w /"),
        line: zoraya_protocol::Data(b"term"),
    };
    s.f().send(m4, message(&first, 9), vec![t]);
    s.c.step();
    assert_eq!(done(&mut s, m4), status::OK);
    assert_eq!(s.c.record(0).map(|r| r.app.as_str()), Some("term"));
    // Records that do not fit their room are refused whole.
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    for (room, untrusted, colour, app, entries) in [
        ("project", 0, 0, "term", ""),
        ("agent-1", 1, 2, "term", ""),
        ("", 1, 0, "term", ""),
        ("Bad", 0, 1, "term", ""),
        ("project", 0, 1, "", ""),
        ("project", 0, 1, "T_open", ""),
        ("project", 0, 1, "t open", ""),
        ("project", 0, 1, "t.open", ""),
        ("project", 0, 1, "term", "+w ~/a\u{1b}[31m"),
        ("project", 2, 1, "term", ""),
    ] {
        let end = s.f().channel(64);
        let m = gui::AddRoomClient {
            untrusted,
            colour,
            tag: 0,
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
    assert_eq!(s.c.client_count(), 1, "init's terminal only");
    // A bad table, or a current room not in it, changes nothing.
    assert_eq!(s.set_rooms(panel, "nowhere", ROOMS), status::INVALID);
    assert_eq!(s.set_rooms(panel, "", "Bad line\n"), status::INVALID);
    assert_eq!(s.c.rooms().rooms.len(), 2);
    assert_eq!(s.set_dock(panel, "term"), status::INVALID);
    assert_eq!(s.set_dock(panel, "term\tterm\n"), status::OK);
    // The panel going away leaves the clients and the table.
    let a = s.room_client(panel, "project", "term", "+w ~/project");
    let _ = a;
    // A program's name as the ramdisk has it, with `_` (t_open).
    let b = s.room_client(panel, "project", "t_open", "+r ~/notes.txt");
    let _ = b;
    s.f().peers.get_mut(&panel).unwrap().gone = true;
    s.c.step();
    assert_eq!(s.c.client_count(), 3);
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
    // The spoofing title leaves the badge and the pills as they were.
    let now: Vec<u32> = (o.right() - 120..o.right() - 2)
        .map(|x| s.pixel(x, o.y + 14))
        .collect();
    assert_eq!(now, badge_area, "the badge and pills are the compositor's");
    // Every manager message, well formed, on a client's channel: none
    // changes a record, the table or the dock (INV-0172-1, INV-0242-1). A
    // client the compositor drops for it is replaced and the next is sent.
    let dock = s.c.dock_apps().to_vec();
    let mut client = a;
    let manager: Vec<(Vec<u8>, bool)> = vec![
        (message(&gui::AddClient {}, 3), true),
        (
            message(
                &gui::AddRoomClient {
                    untrusted: 0,
                    colour: 1,
                    tag: 0,
                    room: zoraya_protocol::Data(b"project"),
                    app: zoraya_protocol::Data(b"term"),
                    entries: zoraya_protocol::Data(b"+w /"),
                    line: zoraya_protocol::Data(b"term"),
                },
                3,
            ),
            true,
        ),
        (message(&gui::AttachDisplay {}, 3), true),
        (message(&gui::AttachInput {}, 3), true),
        (message(&gui::AttachClientManager {}, 3), true),
        (
            message(
                &gui::SetRooms {
                    current: zoraya_protocol::Data(b""),
                    rooms: zoraya_protocol::Data(b"evil --colour 2 +w /\n"),
                },
                3,
            ),
            false,
        ),
        (
            message(
                &gui::SetDock {
                    apps: zoraya_protocol::Data(b"evil\tevil\n"),
                },
                3,
            ),
            false,
        ),
    ];
    for (bytes, handle) in manager {
        let clients = s.c.client_count();
        let handles = if handle {
            vec![s.f().channel(64)]
        } else {
            vec![]
        };
        s.f().send(client, bytes, handles);
        s.c.step();
        assert_eq!(s.c.rooms(), &rooms);
        assert_eq!(s.c.dock_apps(), &dock[..]);
        assert!(s.c.client_count() <= clients, "no client added by a client");
        if s.c.record(0).is_some() {
            assert_eq!(s.c.record(0).cloned(), record);
        } else {
            client = s.room_client(panel, "project", "term", "+w ~/project\n+r ~/include");
            s.window_of(client, 400, 200, 0x0011_2233);
        }
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
    // A window is placed in the work area; the person drags it down over
    // the dock's place.
    let o = s.outer(0, 1);
    let (w, h) = s.f().display_size;
    s.point(axis(o.x + 200, w), axis(o.y + 10, h), Some(1));
    s.point(axis(o.x + 200, w), axis(o.y + 300, h), None);
    s.point(axis(o.x + 200, w), axis(o.y + 300, h), Some(0));
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
            .any(|m| decode::<gui::WindowPointerMods>(m, 0).is_ok())
    );
    // In the dock's padding, outside every cell: still the dock's, nothing
    // for the window (review 7d, 5).
    let d = s.c.dock().rect;
    let (px, py) = (d.x + 3, d.y + d.height / 2);
    assert!(s.outer(0, 1).contains(px, py));
    s.press_at(px, py);
    assert!(dock_pressed(&s.f().take(panel)).is_empty());
    let out = s.f().take(a);
    assert!(
        !out.iter()
            .any(|m| decode::<gui::WindowPointerMods>(m, 0).is_ok())
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
            .replace("LAYOUT", "ua")
            .replace("PATH", "~/project");
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

// Mutants 7d: what the first tests left unread.

/// A request of the client manager's or the driver manager's with the
/// wrong number of handles is refused, and the handles are closed.
#[test]
fn manager_requests_take_exactly_their_handles() {
    let mut s = setup();
    let panel = s.panel();
    let record = gui::AddRoomClient {
        untrusted: 0,
        colour: 0,
        tag: 0,
        room: zoraya_protocol::Data(b""),
        app: zoraya_protocol::Data(b"term"),
        entries: zoraya_protocol::Data(b""),
        line: zoraya_protocol::Data(b"term"),
    };
    for to_init in [false, true] {
        let to = if to_init { s.manager } else { panel };
        let a = s.f().channel(64);
        let b = s.f().channel(64);
        for handles in [vec![], vec![a, b]] {
            s.f().send(to, message(&record, 7), handles.clone());
            s.c.step();
            assert_eq!(done(&mut s, to), status::UNSUPPORTED, "{handles:?}");
            assert!(handles.iter().all(|h| s.f().closed.contains(h)));
        }
    }
    assert_eq!(s.c.client_count(), 0);
    // AttachClientManager: one channel, on init's channel only.
    let m = s.manager;
    let a = s.f().channel(64);
    let b = s.f().channel(64);
    for (to, handles) in [(m, vec![]), (m, vec![a, b]), (panel, vec![])] {
        s.f().send(
            to,
            message(&gui::AttachClientManager {}, 4),
            handles.clone(),
        );
        s.c.step();
        assert_eq!(done(&mut s, to), status::UNSUPPORTED);
        assert!(handles.iter().all(|h| s.f().closed.contains(h)));
    }
}

/// The panel stays attached when its channel has nothing to read and when
/// init's channel closes.
#[test]
fn the_panel_stays_attached() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    assert_eq!(s.set_dock(panel, "term\tterm\n"), status::OK);
    let chip = |s: &Setup| {
        let r =
            s.c.dock()
                .cells
                .iter()
                .find(|x| x.0 == Cell::Chip(0))
                .unwrap()
                .1;
        (r.x + r.width / 2, r.y + r.height / 2)
    };
    s.c.manager_message(true);
    let (x, y) = chip(&s);
    s.press_at(x, y);
    assert_eq!(dock_pressed(&s.f().take(panel)), [(gui::dock::ROOM, 0)]);
    let m = s.manager;
    s.f().peers.get_mut(&m).unwrap().gone = true;
    s.c.step();
    assert!(!s.f().closed.contains(&panel));
    s.press_at(x, y);
    assert_eq!(dock_pressed(&s.f().take(panel)), [(gui::dock::ROOM, 0)]);
}

/// A room's window is badged with its room, an untrusted one says so, and
/// a `home` window has no badge.
#[test]
fn badges_name_the_room() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let p = *s.c.appearance().palette();
    for room in ["project", "agent-1", ""] {
        let end = s.room_client(panel, room, "term", "+w ~/a");
        s.window_of(end, 200, 100, 0x0040_4040);
    }
    let badges: Vec<Option<String>> = s.c.win_views().into_iter().map(|v| v.badge).collect();
    assert_eq!(
        badges,
        [
            Some(String::from("project")),
            Some(String::from("agent-1  untrusted")),
            None
        ]
    );
    // The project window's badge dot is in room.1, in its title row.
    let o = s.outer(0, 1);
    let row = o.y + s.c.frame().title / 2;
    assert!((o.x..o.right()).any(|x| s.pixel(x, row) == p.room(1).pixel()));
}

/// At scale 2 the untrusted edge is four pixels on every side.
#[test]
fn the_untrusted_edge_at_scale_two() {
    let mut s = setup_with(|f| f.display_size = (2048, 1536));
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let p = *s.c.appearance().palette();
    let agent = s.room_client(panel, "agent-1", "term", "");
    s.window_of(agent, 300, 160, 0x0040_4040);
    let o = s.outer(0, 1);
    let b = s.c.frame().border;
    let t = space::ROOM_UNTRUSTED_EDGE.physical(Scale::TWO) as i64;
    assert_eq!(t, 4);
    let (mx, my) = (o.x + o.width / 2, o.y + o.height / 2);
    let u = p.room_untrusted.pixel();
    // `Setup::pixel` reads rows of the 640-pixel test display.
    let px = |s: &mut Setup, x: i64, y: i64| {
        let w = i64::from(s.f().display_size.0);
        s.f().frame[(y * w + x) as usize]
    };
    for d in 0..t {
        assert_eq!(px(&mut s, o.x + b + d, my), u, "left {d}");
        assert_eq!(px(&mut s, o.right() - 1 - b - d, my), u, "right {d}");
        assert_eq!(px(&mut s, mx, o.bottom() - 1 - b - d), u, "bottom {d}");
    }
    assert_ne!(px(&mut s, o.x + b + t, my), u);
    assert_ne!(px(&mut s, mx, o.bottom() - 1 - b - t), u);
}

/// The switcher lists the focused window's room, then `home`, then the
/// table's order, whatever the order of use.
#[test]
fn the_switcher_groups_the_focused_room_then_home_then_the_table() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    // C (agent-1), A (project), B (home), apart from each other.
    let c = s.room_client(panel, "agent-1", "term", "");
    s.window_of(c, 150, 80, 1);
    let a = s.room_client(panel, "project", "term", "");
    s.window_of(a, 150, 80, 2);
    let b = s.room_client(panel, "", "term", "");
    s.window_of(b, 150, 80, 3);
    let k = |client| Key { client, id: 1 };
    let focus = |s: &mut Setup, client: usize| {
        let o = s.outer(client, 1);
        s.press_at(o.right() - 30, o.y + 6);
        assert_eq!(s.c.focused(), Some(k(client)), "{client} focused");
    };
    let open = |s: &mut Setup| {
        s.keys(&[(code::LEFT_ALT, 1), (code::TAB, 1), (code::TAB, 0)]);
        let w = s.c.switcher().unwrap().windows.clone();
        s.keys(&[(code::ESC, 1), (code::ESC, 0), (code::LEFT_ALT, 0)]);
        w
    };
    // Used in the order agent-1, home: home's first, then the table's
    // project before agent-1, which was used more recently.
    focus(&mut s, 0);
    focus(&mut s, 2);
    assert_eq!(open(&mut s), [k(2), k(1), k(0)]);
    // Used in the order home, agent-1, project: project's, then home
    // before agent-1, which was used more recently.
    focus(&mut s, 2);
    focus(&mut s, 0);
    focus(&mut s, 1);
    assert_eq!(open(&mut s), [k(1), k(2), k(0)]);
}

fn cell_rect(s: &Setup, c: Cell) -> Rect {
    s.c.dock().cells.iter().find(|x| x.0 == c).unwrap().1
}

/// The dock's dots in their rooms' colours, home's first; the current
/// chip's fill; a chip's dot; the hovered tile's lift and its tooltip; the
/// divider in `border`. Dark: in the light set the tile and the lift are
/// both white.
#[test]
fn the_dock_paints_its_parts_where_its_cells_are() {
    let mut s = setup();
    s.c.appearance = zoraya_tokens::Appearance::Dark;
    s.c.make_wallpaper();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    assert_eq!(
        s.set_dock(panel, "term\tterm\nedit\tedit FILE\n"),
        status::OK
    );
    let p = *s.c.appearance().palette();
    let t1 = s.room_client(panel, "project", "term", "");
    s.window_of(t1, 200, 100, 1);
    let t2 = s.room_client(panel, "", "term", "");
    s.window_of(t2, 200, 100, 2);

    let r = cell_rect(&s, Cell::App(0));
    let centre = r.x + r.width / 2;
    let dot_y = r.y + r.height + 3;
    assert_eq!(s.pixel(centre - 3, dot_y), p.fg_text_secondary.pixel());
    assert_eq!(s.pixel(centre + 4, dot_y), p.room(1).pixel());

    let chip = cell_rect(&s, Cell::Chip(1));
    let chip_h = i64::from(crate::rooms::CHIP_HEIGHT);
    let cy = chip.y + (r.height - chip_h) / 2 + chip_h / 2;
    assert_eq!(s.pixel(chip.x + 13, cy), p.room(1).pixel());
    let home_chip = cell_rect(&s, Cell::Chip(0));
    let before = (s.pixel(chip.x + 4, cy), s.pixel(home_chip.x + 4, cy));
    assert_eq!(s.set_rooms(panel, "project", ROOMS), status::OK);
    let after = (s.pixel(chip.x + 4, cy), s.pixel(home_chip.x + 4, cy));
    assert_ne!(before.0, after.0, "project's chip is filled as current");
    assert_ne!(before.1, after.1, "home's chip is no longer");

    let dock = s.c.dock().rect;
    let pad = i64::from(space::DOCK_PADDING.physical(Scale::ONE));
    let d = s.c.dock().divider;
    let mid = dock.y + pad + r.height / 2;
    assert_eq!(s.pixel(d, mid), p.border.pixel());
    assert_ne!(s.pixel(d + 1, mid), p.border.pixel());

    let edit = cell_rect(&s, Cell::App(1));
    let ec = edit.x + edit.width / 2;
    let tile = (ec - r.height / 2 + 3, edit.y + r.height / 2);
    let plus = cell_rect(&s, Cell::Plus);
    let above = |s: &mut Setup, x: i64| s.pixel(x, dock.y - 10);
    let quiet = (
        s.pixel(tile.0, tile.1),
        above(&mut s, ec),
        above(&mut s, plus.x + plus.width / 2),
    );
    let (w, h) = s.f().display_size;
    s.point(axis(ec, w), axis(edit.y + r.height / 2, h), None);
    assert_eq!(s.c.hovered(), Some(1));
    assert_ne!(
        s.pixel(tile.0, tile.1),
        quiet.0,
        "the hovered tile is lifted"
    );
    assert_ne!(above(&mut s, ec), quiet.1, "its tooltip is over it");
    assert_eq!(
        above(&mut s, plus.x + plus.width / 2),
        quiet.2,
        "and not elsewhere"
    );
}

/// The title is drawn beside the badge and the pills.
#[test]
fn a_rooms_window_shows_its_title() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let end = s.room_client(panel, "project", "term", "+w ~/project");
    let id = s.window_of(end, 400, 120, 0x0040_4040);
    let o = s.outer(0, id);
    let band = |s: &mut Setup| -> Vec<u32> {
        (o.y + 6..o.y + 22)
            .flat_map(|y| (o.x + 40..o.right() - 160).map(move |x| (x, y)))
            .map(|(x, y)| s.pixel(x, y))
            .collect()
    };
    let empty = band(&mut s);
    s.request(
        end,
        &gui::SetTitle {
            window: id,
            text: zoraya_protocol::Data("zosh".as_bytes()),
        },
        vec![],
    );
    assert_ne!(band(&mut s), empty, "the title is drawn");
}

/// The palette the frames are drawn in is the appearance set.
#[test]
fn the_appearance_is_the_one_set() {
    let mut s = setup();
    s.c.appearance = zoraya_tokens::Appearance::Dark;
    assert_eq!(s.c.appearance(), zoraya_tokens::Appearance::Dark);
}

/// App names are lowercase letters, digits, dashes and underscores.
#[test]
fn app_names_are_lowercase_letters_digits_dashes_and_underscores() {
    let mut s = setup();
    let panel = s.panel();
    for (app, want) in [
        ("edit-2", status::OK),
        ("t_open", status::OK),
        ("Edit", status::INVALID),
        ("edit 2", status::INVALID),
    ] {
        let end = s.f().channel(64);
        let m = gui::AddRoomClient {
            untrusted: 0,
            colour: 0,
            tag: 0,
            room: zoraya_protocol::Data(b""),
            app: zoraya_protocol::Data(app.as_bytes()),
            entries: zoraya_protocol::Data(b""),
            line: zoraya_protocol::Data(b"term"),
        };
        s.f().send(panel, message(&m, 7), vec![end]);
        s.c.step();
        assert_eq!(done(&mut s, panel), want, "{app}");
    }
}

/// Show-commands (ADR-0172 point 8): each window's tag is inside its own
/// frame, at its body's lower left, and the window above covers the tag of
/// the one below as it covers the window; nothing changes over another
/// window or outside the windows but the dock and the bar.
#[test]
fn a_windows_tag_stays_in_its_own_frame() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let a = s.room_client(panel, "project", "term", "+w ~/project");
    let ida = s.window_of(a, 300, 200, 0x0040_4040);
    let b = s.room_client(panel, "project", "term", "+w ~/project");
    let idb = s.window_of(b, 300, 200, 0x0060_6060);
    let (oa, ob) = (s.outer(0, ida), s.outer(1, idb));
    assert!(!oa.intersect(&ob).is_empty(), "the two overlap");
    let before = s.f().frame.clone();
    s.keys(&[
        (code::RIGHT_META, 1),
        (code::SLASH, 1),
        (code::SLASH, 0),
        (code::RIGHT_META, 0),
    ]);
    assert!(s.c.commands());
    assert_eq!(
        (s.outer(0, ida), s.outer(1, idb)),
        (oa, ob),
        "nothing moved"
    );
    let after = s.f().frame.clone();
    let bar = i64::from(space::BAR_HEIGHT.physical(s.c.scale()));
    let strip = s.c.dock_strip();
    // A tag's zone with its material's halo: the lower left of its
    // window, inside it.
    let zone = |o: Rect| Rect::new(o.x, o.bottom() - 64, o.width * 2 / 3, 64);
    let (mut in_a, mut in_b) = (0, 0);
    for y in bar..i64::from(H) - strip {
        for x in 0..i64::from(W) {
            let i = (y * i64::from(W) + x) as usize;
            if before[i] == after[i] {
                continue;
            }
            let p = Rect::new(x, y, 1, 1);
            if !ob.intersect(&p).is_empty() {
                assert!(
                    !zone(ob).intersect(&p).is_empty(),
                    "({x}, {y}) in the upper window"
                );
                in_b += 1;
            } else {
                assert!(
                    !oa.intersect(&p).is_empty(),
                    "({x}, {y}) outside the windows"
                );
                assert!(
                    !zone(oa).intersect(&p).is_empty(),
                    "({x}, {y}) in the lower window"
                );
                in_a += 1;
            }
        }
    }
    assert!(in_a > 0 && in_b > 0, "both tags are drawn: {in_a} {in_b}");
}

/// The architect's check of 2026-11-10: the dock's tooltip shows one line
/// when the name and the line are the same ("files", not "files files").
#[test]
fn a_tooltip_says_a_line_once() {
    use crate::draw_rooms::tooltip_lines;
    assert_eq!(tooltip_lines("files", "files"), [("files", true)]);
    assert_eq!(
        tooltip_lines("files", "room project files"),
        [("files", false), ("room project files", true)]
    );
}

/// The settings tile is a gear: teeth at its edge, a ring, and a hole in
/// the middle (not a ring with a dot), at both scales.
#[test]
fn the_settings_glyph_is_a_gear() {
    use crate::draw_rooms::icon_glyph;
    for k in [1u32, 2] {
        let icon = 40 * k as i32;
        let side = icon as usize;
        let mut px = vec![0u32; side * side];
        let mut t = zoraya_text::Text::new(crate::fonts(zoraya_fonts::INTER_REGULAR));
        {
            let mut c = zoraya_paint::Canvas::new(&mut px, side);
            let white = zoraya_tokens::Color::of_pixel(0x00FF_FFFF);
            icon_glyph(&mut c, "settings", 0, 0, icon, k, white, &mut t);
        }
        let at = |x: i32, y: i32| px[y as usize * side + x as usize] != 0;
        let (m, gs) = (icon / 2, icon - 2 * (icon / 5));
        assert!(!at(m, m), "a hole in the middle at scale {k}");
        // A tooth straight up, at the glyph's edge, and the gap between
        // two teeth at the same radius (22.5 degrees off).
        let r = gs / 2 - 2 * k as i32;
        assert!(at(m, m - r), "a tooth at scale {k}");
        let (gx, gy) = (m + r * 383 / 1000, m - r * 924 / 1000);
        assert!(!at(gx, gy), "a gap between teeth at scale {k}");
        // The ring between hole and teeth.
        assert!(at(m, m - gs * 7 / 20 + gs / 12), "the ring at scale {k}");
    }
}

/// ADR-0317 item 2: the panel hears that a client it made, by its tag, is
/// gone; a client with no tag is not told.
#[test]
fn the_panel_is_told_a_tagged_client_is_gone() {
    let mut s = setup();
    let panel = s.panel();
    let add = |s: &mut Setup, tag: u32| {
        let end = s.f().channel(64);
        let m = gui::AddRoomClient {
            untrusted: 0,
            colour: 0,
            tag,
            room: zoraya_protocol::Data(b""),
            app: zoraya_protocol::Data(b"netsurf"),
            entries: zoraya_protocol::Data(b""),
            line: zoraya_protocol::Data(b"netsurf"),
        };
        s.f().send(panel, message(&m, 7), vec![end]);
        s.c.step();
        assert_eq!(done(s, panel), status::OK);
        end
    };
    let tagged = add(&mut s, 5);
    let plain = add(&mut s, 0);
    let gone = |s: &mut Setup| -> Vec<u32> {
        s.f()
            .take(panel)
            .iter()
            .filter_map(|m| decode::<gui::ClientGone>(m, 0).ok())
            .map(|(_, g)| g.tag)
            .collect()
    };
    // An oversized message closes its client.
    s.f().send(plain, vec![0; 8192], vec![]);
    s.c.step();
    assert!(gone(&mut s).is_empty());
    s.f().send(tagged, vec![0; 8192], vec![]);
    s.c.step();
    assert_eq!(gone(&mut s), vec![5]);
}

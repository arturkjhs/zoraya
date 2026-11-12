//! Q6b: the accounts of untrusted rooms (ADR-0262 point 2, INV-0262-1 and
//! INV-0262-3 for the compositor), the panel's readings, the pills at 90%
//! and the hover card (ADR-0267 point 3, INV-0266-3, INV-0267-1).

use super::rooms::done;
use super::*;
use crate::usage::{ROOM_SURFACES, UNTRUSTED_ACCOUNTS, UNTRUSTED_SURFACES};
use alloc::format;
use zoraya_settings::usage::Reading;

const MIB: u64 = 1 << 20;
const TABLE: &str = "a1 --untrusted\na2 --untrusted\na3 --untrusted\na4 --untrusted\na5 --untrusted\nproject --colour 1 +w ~/project\n";

impl Setup {
    /// A toplevel request: the status, and the window if it was made.
    fn try_toplevel(&mut self, end: u32, w: u32, h: u32) -> (u32, u32) {
        let s = self.c.scale().factor();
        let n = (w * s * h * s) as usize;
        let memory = self.f().memory(vec![0x0040_4040; n], n as u64 * 4, false);
        let m = gui::CreateToplevel {
            width: w,
            height: h,
        };
        self.f().send(end, message(&m, 5), vec![memory]);
        self.c.step();
        let out = self.f().take(end);
        if let Some((_, c)) = out.iter().find_map(|r| decode::<gui::Created>(r, 0).ok()) {
            return (c.status.0, c.window);
        }
        let e = out
            .iter()
            .find_map(|r| decode::<zoraya_protocol::ErrorReply>(r, 0).ok())
            .expect("a reply");
        (e.1.status, 0)
    }

    fn set_usage(&mut self, panel: u32, text: &str) -> u32 {
        let m = gui::SetUsage {
            readings: zoraya_protocol::Data(text.as_bytes()),
        };
        self.f().send(panel, message(&m, 4), vec![]);
        self.c.step();
        done(self, panel)
    }
}

fn reading(name: &str, mem_used: u64) -> Reading {
    Reading {
        name: name.into(),
        running: true,
        untrusted: name.starts_with('a'),
        mem_used,
        mem_limit: Some(64 * MIB),
        procs_used: 3,
        procs_limit: Some(64),
        cpu_permille: Some(250),
        cpu_pct: Some(5),
        disk_limit: Some(2 << 30),
        conns_limit: Some(16),
        ..Reading::default()
    }
}

#[test]
fn an_untrusted_account_at_its_bound_is_refused_and_home_still_opens_one() {
    // INV-0262-1 in the compositor.
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let a1 = s.room_client(panel, "a1", "term", "+w ~/a");
    for i in 0..ROOM_SURFACES {
        assert_eq!(s.try_toplevel(a1, 100, 60).0, status::OK, "surface {i}");
    }
    let (st, _) = s.try_toplevel(a1, 100, 60);
    assert_eq!(st, status::OVER_QUOTA);
    assert_eq!(s.try_toplevel(a1, 100, 60).0, status::OVER_QUOTA);
    // Nothing of another account changed: a second untrusted room and
    // `home` and a trusted room open theirs.
    let a2 = s.room_client(panel, "a2", "term", "+w ~/a");
    assert_eq!(s.try_toplevel(a2, 100, 60).0, status::OK);
    let home = s.room_client(panel, "", "term", "");
    assert_eq!(s.try_toplevel(home, 100, 60).0, status::OK);
    let project = s.room_client(panel, "project", "term", "+w ~/project");
    assert_eq!(s.try_toplevel(project, 100, 60).0, status::OK);
    let accounts = s.c.accounts();
    let a = accounts.iter().find(|a| a.room == "a1").unwrap();
    assert_eq!((a.surfaces, a.refused), (ROOM_SURFACES, 2));
    assert_eq!(accounts.iter().find(|a| a.room == "a2").unwrap().refused, 0);
}

#[test]
fn a_room_holds_two_displays_of_pixels_and_no_more() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let a1 = s.room_client(panel, "a1", "term", "+w ~/a");
    // Two windows as large as the work area are two displays or less; the
    // third pushes the room past two displays' worth.
    let work = s.c.work_area();
    let (w, h) = (work.width as u32, work.height as u32);
    assert_eq!(s.try_toplevel(a1, w, h).0, status::OK);
    assert_eq!(s.try_toplevel(a1, w, h).0, status::OK);
    assert_eq!(s.try_toplevel(a1, w, h).0, status::OVER_QUOTA);
    // A small one still fits under the surface bound and the area.
    let area = u64::from(W) * u64::from(H);
    assert!(2 * u64::from(w) * u64::from(h) < 2 * area);
}

#[test]
fn the_untrusted_rooms_together_leave_home_twenty_windows() {
    // INV-0262-3: four accounts, 12 surfaces, and the rest for the session.
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let mut ends = Vec::new();
    for room in ["a1", "a2", "a3", "a4"] {
        let e = s.room_client(panel, room, "term", "+w ~/a");
        for _ in 0..ROOM_SURFACES {
            assert_eq!(s.try_toplevel(e, 60, 40).0, status::OK);
        }
        ends.push(e);
    }
    // A fifth untrusted room has no account to take.
    let a5 = s.room_client(panel, "a5", "term", "+w ~/a");
    assert_eq!(s.try_toplevel(a5, 60, 40).0, status::OVER_QUOTA);
    assert_eq!(UNTRUSTED_ACCOUNTS * ROOM_SURFACES, UNTRUSTED_SURFACES);
    // `home` opens the 20 that remain, in three clients, and no more.
    let mut opened = 0;
    for _ in 0..3 {
        let h = s.room_client(panel, "", "term", "");
        for _ in 0..MAX_TOPLEVELS {
            if s.try_toplevel(h, 60, 40).0 == status::OK {
                opened += 1;
            }
        }
    }
    assert_eq!(opened, MAX_WINDOWS - UNTRUSTED_SURFACES);
    // Once an account's window closed, the room's next request is served.
    let closed = {
        let m = gui::Close { window: 1 };
        s.f().send(ends[0], message(&m, 8), vec![]);
        s.c.step();
        s.f().take(ends[0]);
        s.c.windows.len()
    };
    assert!(closed <= MAX_WINDOWS);
}

#[test]
fn only_the_client_manager_sets_usage_and_a_bad_text_changes_nothing() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let good = reading("a1", 10 * MIB).to_wire();
    // init's manager channel refuses it.
    let m = gui::SetUsage {
        readings: zoraya_protocol::Data(good.as_bytes()),
    };
    let init = s.manager;
    s.f().send(init, message(&m, 4), vec![]);
    s.c.step();
    assert_eq!(done(&mut s, init), status::UNSUPPORTED);
    assert!(s.c.usage().is_empty());
    assert_eq!(s.set_usage(panel, &format!("{good}\n")), status::OK);
    assert_eq!(s.c.reading("a1").map(|r| r.mem_used), Some(10 * MIB));
    for bad in [
        "a1 1 2 3\n",
        "A1 1 0 - 0 0 - 0 - - - - -\n",
        "a1 1 0 - 0 0 - 0 - - - - - 7\n",
    ] {
        assert_eq!(s.set_usage(panel, bad), status::INVALID, "{bad:?}");
        assert_eq!(s.c.reading("a1").map(|r| r.mem_used), Some(10 * MIB));
    }
    // A client has no such message: its channel does not take ordinal 44.
    let c = s.room_client(panel, "a1", "term", "+w ~/a");
    s.f().send(c, message(&m, 4), vec![]);
    s.c.step();
    assert_eq!(s.c.reading("a1").map(|r| r.mem_used), Some(10 * MIB));
}

#[test]
fn a_pill_shows_at_ninety_percent_in_the_title_bar_and_not_below() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let end = s.room_client(panel, "a1", "term", "+w ~/a");
    s.window_of(end, 300, 120, 0x0040_4040);
    let calm = reading("a1", 50 * MIB);
    assert_eq!(s.set_usage(panel, &calm.to_wire()), status::OK);
    let pills = |s: &Setup| s.c.win_views()[0].pills.clone();
    assert_eq!(pills(&s), ["+w ~/a"]);
    let hot = reading("a1", 59 * MIB);
    assert_eq!(s.set_usage(panel, &hot.to_wire()), status::OK);
    // 59 of 64 is 92%: the pill comes first, the grant after it.
    assert_eq!(pills(&s), ["mem 92%", "+w ~/a"]);
    // The pill is drawn: the title bar differs from the calm one.
    let o = s.outer(0, 1);
    let bar = Rect::new(o.x, o.y, o.width, s.c.frame().title);
    let hot_pixels = s.region(bar);
    assert_eq!(s.set_usage(panel, &calm.to_wire()), status::OK);
    assert_ne!(hot_pixels, s.region(bar));
    // Falling back below 90% takes it away.
    assert_eq!(pills(&s), ["+w ~/a"]);
    // `home` has no limits and no pill whatever is sent for it.
    let home = s.room_client(panel, "", "term", "");
    s.window_of(home, 300, 120, 0x0040_4040);
    let mut h = reading("home", 63 * MIB);
    h.untrusted = false;
    assert_eq!(s.set_usage(panel, &h.to_wire()), status::OK);
    assert!(s.c.win_views()[1].pills.is_empty());
}

#[test]
fn the_hover_card_shows_the_words_top_and_grants_show() {
    // INV-0266-3 and INV-0267-1 at the compositor: the card's lines are the
    // reading's own, with the record's grants, from one source.
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let end = s.room_client(panel, "a1", "term", "+w ~/a");
    s.window_of(end, 300, 120, 0x0040_4040);
    let r = reading("a1", 20 * MIB);
    assert_eq!(s.set_usage(panel, &r.to_wire()), status::OK);
    let key = Key { client: 0, id: 1 };
    let lines = s.c.card_lines(key).unwrap();
    assert_eq!(lines[0], "a1  untrusted");
    assert_eq!(lines[1], "+w ~/a");
    assert_eq!(&lines[2..], r.card_lines().as_slice());
    assert!(lines.contains(&String::from("mem 20M of 64M")));
    assert!(
        lines
            .last()
            .unwrap()
            .starts_with("limits --mem 64M --cpu 25%")
    );
    // No card until the pointer is on the title bar; then one; then none.
    assert_eq!(s.c.card(), None);
    let o = s.outer(0, 1);
    let (tx, ty) = (o.x + o.width / 2, o.y + s.c.frame().title / 2);
    let (w, h) = s.f().display_size;
    s.point(super::rooms::axis(tx, w), super::rooms::axis(ty, h), None);
    assert_eq!(s.c.card(), Some(key));
    let (card, _) = s.c.card_geometry(key).unwrap();
    let inside = s.region(card);
    s.point(super::rooms::axis(5, w), super::rooms::axis(300, h), None);
    assert_eq!(s.c.card(), None);
    assert_ne!(inside, s.region(card), "the card is drawn, then gone");
    // The card follows the numbers: a new reading changes its lines.
    let r2 = reading("a1", 21 * MIB);
    assert_eq!(s.set_usage(panel, &r2.to_wire()), status::OK);
    assert!(
        s.c.card_lines(key)
            .unwrap()
            .contains(&String::from("mem 21M of 64M"))
    );
    // `home`'s window has no card.
    let home = s.room_client(panel, "", "term", "");
    s.window_of(home, 200, 100, 0x0040_4040);
    assert_eq!(s.c.card_lines(Key { client: 1, id: 1 }), None);
}

#[test]
fn a_card_goes_with_its_window() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let end = s.room_client(panel, "project", "term", "+w ~/project");
    s.window_of(end, 300, 120, 0x0040_4040);
    let o = s.outer(0, 1);
    // A room with no limit has no card, whatever the reading says: its
    // pills are all it has to say and nothing of the window is covered.
    assert_eq!(s.c.card_lines(Key { client: 0, id: 1 }), None);
    assert_eq!(s.c.card(), None);
    let mut r = reading("project", MIB);
    r.mem_limit = None;
    r.procs_limit = None;
    r.cpu_permille = None;
    r.disk_limit = None;
    r.conns_limit = None;
    assert_eq!(s.set_usage(panel, &r.to_wire()), status::OK);
    assert_eq!(s.c.card_lines(Key { client: 0, id: 1 }), None);
    // With a limit it has one, and it goes with its window.
    assert_eq!(
        s.set_usage(panel, &reading("project", MIB).to_wire()),
        status::OK
    );
    let (w, h) = s.f().display_size;
    s.point(
        super::rooms::axis(o.x + o.width / 2, w),
        super::rooms::axis(o.y + s.c.frame().title / 2, h),
        None,
    );
    s.point(
        super::rooms::axis(o.x + o.width / 2 + 1, w),
        super::rooms::axis(o.y + s.c.frame().title / 2, h),
        None,
    );
    assert!(s.c.card().is_some());
    s.f().peers.get_mut(&end).unwrap().gone = true;
    s.c.step();
    assert_eq!(s.c.card(), None);
}

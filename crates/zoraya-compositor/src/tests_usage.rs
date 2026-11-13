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
    // Once an account's window closed, the room's next request is served
    // and the one after it is not (the session's windows are all open).
    let m = gui::Close { window: 1 };
    s.f().send(ends[0], message(&m, 8), vec![]);
    s.c.step();
    s.f().take(ends[0]);
    assert_eq!(s.try_toplevel(ends[0], 60, 40).0, status::OK);
    assert_ne!(s.try_toplevel(ends[0], 60, 40).0, status::OK);
}

#[test]
fn a_fifth_untrusted_room_has_no_account_even_with_surfaces_to_spare() {
    // Four rooms with one surface each use 4 of the 12: the fifth is
    // refused for the accounts alone.
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    for room in ["a1", "a2", "a3", "a4"] {
        let e = s.room_client(panel, room, "term", "+w ~/a");
        assert_eq!(s.try_toplevel(e, 60, 40).0, status::OK);
    }
    let a5 = s.room_client(panel, "a5", "term", "+w ~/a");
    assert_eq!(s.try_toplevel(a5, 60, 40).0, status::OVER_QUOTA);
    // An existing account still takes its second surface.
    let a1 = s.room_client(panel, "a1", "term", "+w ~/a");
    assert_eq!(s.try_toplevel(a1, 60, 40).0, status::OK);
}

#[test]
fn a_popup_counts_as_a_surface_of_its_room() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let a1 = s.room_client(panel, "a1", "term", "+w ~/a");
    let (_, parent) = (s.try_toplevel(a1, 100, 60).0, 1);
    assert_eq!(s.try_toplevel(a1, 100, 60).0, status::OK);
    let popup = |s: &mut Setup| {
        let m = s.f().memory(vec![7; 20 * 10], 800, false);
        let out = s.request(
            a1,
            &gui::CreatePopup {
                parent,
                x: 10,
                y: 20,
                width: 20,
                height: 10,
            },
            vec![m],
        );
        out.iter()
            .find_map(|m| decode::<gui::PopupCreated>(m, 0).ok())
            .map(|(_, p)| p.status.0)
            .or_else(|| {
                out.iter()
                    .find_map(|r| decode::<zoraya_protocol::ErrorReply>(r, 0).ok())
                    .map(|e| e.1.status)
            })
            .expect("a reply")
    };
    // Two toplevels and a popup are the room's three; the next popup is not.
    assert_eq!(popup(&mut s), status::OK);
    assert_eq!(popup(&mut s), status::OVER_QUOTA);
    assert_eq!(s.try_toplevel(a1, 100, 60).0, status::OVER_QUOTA);
    let a = s.c.accounts();
    assert_eq!(a[0].surfaces, ROOM_SURFACES);
    assert_eq!(a[0].refused, 2);
}

#[test]
fn a_new_buffer_counts_in_the_account_until_it_replaces_the_old() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let a1 = s.room_client(panel, "a1", "term", "+w ~/a");
    let work = s.c.work_area();
    let k = s.c.scale().factor();
    let set_buffer = |s: &mut Setup, (w, h): (u32, u32)| {
        let (bw, bh) = (w, h);
        let n = (bw * k * bh * k) as usize;
        let m = s.f().memory(vec![0; n], n as u64 * 4, false);
        let out = s.request(
            a1,
            &gui::SetBuffer {
                window: 1,
                width: bw,
                height: bh,
            },
            vec![m],
        );
        decode::<gui::BufferDone>(&out[0], 0).unwrap().1.status.0
    };
    // A toplevel is made to fit the work area: its body is what it gets.
    let (work_w, work_h) = (work.width as u32, work.height as u32);
    assert_eq!(s.try_toplevel(a1, work_w, work_h).0, status::OK);
    let body = s.c.window(Key { client: 0, id: 1 }).unwrap().body_rect();
    let (w, h) = (body.width as u32 / k, body.height as u32 / k);
    let area = u64::from(W) * u64::from(H);
    let pixels = |s: &Setup| s.c.accounts()[0].pixels;
    let one = pixels(&s);
    let new = u64::from(w * k) * u64::from(h * k);
    // The test needs the shown buffer and a new one to fit two displays,
    // and a second window beside them not to.
    assert!(one + new <= 2 * area && 2 * one + new > 2 * area);
    // A new buffer, even at the size it has, is one more area until the
    // commit, and a newer one replaces the pending one.
    assert_eq!(set_buffer(&mut s, (w, h)), status::OK);
    assert_eq!(pixels(&s), one + new);
    // A small window beside them leaves less than a second new buffer
    // would take, so only a replacement of the pending one is served.
    assert_eq!(s.try_toplevel(a1, 300, 200).0, status::OK);
    let small = pixels(&s) - one - new;
    assert!(one + small + 2 * new > 2 * area && one + small + new <= 2 * area);
    assert_eq!(set_buffer(&mut s, (w, h)), status::OK);
    assert_eq!(pixels(&s), one + small + new);
    // The commit makes the new buffer the window's, and the old goes.
    assert_eq!(s.commit_to(a1, 1, 0, 0, w, h), status::OK);
    assert_eq!(pixels(&s), new + small);
    // A third window of the work area fits; then a new buffer for the first
    // is a fourth area-and-more: refused, and the account is as it was.
    assert_eq!(s.try_toplevel(a1, work_w, work_h).0, status::OK);
    let now = pixels(&s);
    assert!(now + new > 2 * area);
    assert_eq!(set_buffer(&mut s, (w, h)), status::OVER_QUOTA);
    assert_eq!(pixels(&s), now);
    assert_eq!(s.c.accounts()[0].refused, 1);
}

#[test]
fn a_client_that_dies_gives_its_surfaces_back() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let a1 = s.room_client(panel, "a1", "term", "+w ~/a");
    for _ in 0..ROOM_SURFACES {
        assert_eq!(s.try_toplevel(a1, 100, 60).0, status::OK);
    }
    assert_eq!(s.try_toplevel(a1, 100, 60).0, status::OVER_QUOTA);
    s.f().peers.get_mut(&a1).unwrap().gone = true;
    s.c.step();
    // The room starts again: a new client has all three.
    let again = s.room_client(panel, "a1", "term", "+w ~/a");
    for i in 0..ROOM_SURFACES {
        assert_eq!(s.try_toplevel(again, 100, 60).0, status::OK, "surface {i}");
    }
    assert_eq!(s.c.accounts()[0].surfaces, ROOM_SURFACES);
    assert_eq!(s.c.accounts()[0].refused, 0, "the old refusals are gone");
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

/// Shows the card of a window of `w` x `h` whose buffer is `colour`: the
/// body, the card and the card's pixels.
fn shown_card(w: u32, h: u32, colour: u32) -> (Rect, Rect, Vec<u32>) {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let end = s.room_client(panel, "a1", "term", "+w ~/a");
    s.window_of(end, w, h, colour);
    assert_eq!(
        s.set_usage(panel, &reading("a1", 20 * MIB).to_wire()),
        status::OK
    );
    let o = s.outer(0, 1);
    let (dw, dh) = s.f().display_size;
    s.point(
        super::rooms::axis(o.x + o.width / 2, dw),
        super::rooms::axis(o.y + s.c.frame().title / 2, dh),
        None,
    );
    let key = Key { client: 0, id: 1 };
    assert_eq!(s.c.card(), Some(key));
    let body = s.c.window(key).unwrap().body_rect();
    let (card, _) = s.c.card_geometry(key).unwrap();
    let pixels = s.region(card);
    (body, card, pixels)
}

#[test]
fn a_client_buffer_cannot_cover_or_imitate_the_card() {
    // INV-0172-1, ADR-0172 point 1: the card is where no pixel of the
    // client's buffer is, so a buffer that paints one stays a picture in
    // its own body, and whatever the buffer holds changes nothing of it.
    let (body, card, pixels) = shown_card(300, 120, 0x0040_4040);
    assert!(
        card.intersect(&body).is_empty(),
        "{card:?} overlaps the body {body:?}"
    );
    let (_, _, other) = shown_card(300, 120, 0x00ff_ffff);
    assert_eq!(pixels, other, "the buffer's colour reaches the card");
}

#[test]
fn a_card_with_no_room_beside_its_window_covers_the_frame_first() {
    // A window as large as the work area has no free side: the card then
    // starts on the title bar, which no client draws, so a copy in the body
    // lacks that part.
    let work = setup().c.work_area();
    let (body, card, _) = shown_card(work.width as u32, work.height as u32, 0x0040_4040);
    assert!(card.y < body.y, "{card:?} starts inside the body {body:?}");
}

#[test]
fn the_card_follows_a_restack_under_a_still_pointer() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let end = s.room_client(panel, "a1", "term", "+w ~/a");
    s.window_of(end, 300, 120, 0x0040_4040);
    assert_eq!(
        s.set_usage(panel, &reading("a1", 20 * MIB).to_wire()),
        status::OK
    );
    let o = s.outer(0, 1);
    let (dw, dh) = s.f().display_size;
    let (px, py) = (o.x + o.width / 2, o.y + s.c.frame().title - 1);
    s.point(super::rooms::axis(px, dw), super::rooms::axis(py, dh), None);
    assert_eq!(s.c.card(), Some(Key { client: 0, id: 1 }));
    // A window of `home` as large as the screen opens over the pointer.
    let home = s.room_client(panel, "", "term", "");
    s.window_of(home, 560, 380, 0x0040_4040);
    let covering = s.outer(1, 1);
    assert!(covering.y <= py && py < covering.bottom(), "{covering:?}");
    assert_eq!(s.c.card(), None, "the card of a covered title bar");
    // Raising the first window (a keyboard focus change does) puts its title
    // bar under the pointer again, and its card with it.
    s.c.raise(Key { client: 0, id: 1 });
    assert_eq!(s.c.card(), Some(Key { client: 0, id: 1 }));
    // Closing the covering window does the same.
    s.c.raise(Key { client: 1, id: 1 });
    assert_eq!(s.c.card(), None);
    s.c.remove_window(Key { client: 1, id: 1 });
    assert_eq!(s.c.card(), Some(Key { client: 0, id: 1 }));
}

#[test]
fn the_refusals_of_a_room_go_with_its_last_client() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let a1 = s.room_client(panel, "a1", "term", "+w ~/a");
    for _ in 0..=ROOM_SURFACES {
        s.try_toplevel(a1, 60, 40);
    }
    assert_eq!(s.c.accounts().len(), 1);
    s.f().peers.get_mut(&a1).unwrap().gone = true;
    s.c.step();
    assert!(s.c.accounts().is_empty());
    // And the list is never longer than the clients there are.
    let table: String = (0..8).map(|i| format!("r{i} --untrusted\n")).collect();
    assert_eq!(s.set_rooms(panel, "", &table), status::OK);
    let mut ends = Vec::new();
    for i in 0..8 {
        let e = s.room_client(panel, &format!("r{i}"), "term", "+w ~/a");
        for _ in 0..=ROOM_SURFACES {
            s.try_toplevel(e, 60, 40);
        }
        ends.push(e);
    }
    assert_eq!(s.c.accounts().len(), 8);
    for e in ends {
        s.f().peers.get_mut(&e).unwrap().gone = true;
    }
    s.c.step();
    assert!(s.c.accounts().is_empty());
}

/// A room `a1` window of 300 x 120 with its card shown by the pointer on
/// the title bar; the pointer's place.
fn hovered() -> (Setup, u32, (i64, i64)) {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let end = s.room_client(panel, "a1", "term", "+w ~/a");
    s.window_of(end, 300, 120, 0x0040_4040);
    assert_eq!(
        s.set_usage(panel, &reading("a1", 20 * MIB).to_wire()),
        status::OK
    );
    let o = s.outer(0, 1);
    let at = (o.x + o.width / 2, o.y + s.c.frame().title / 2);
    let (dw, dh) = s.f().display_size;
    s.point(
        super::rooms::axis(at.0, dw),
        super::rooms::axis(at.1, dh),
        None,
    );
    assert_eq!(s.c.card(), Some(Key { client: 0, id: 1 }));
    (s, panel, at)
}

/// The number of pixels the screen shows differently from a full redraw.
fn stale_pixels(s: &mut Setup) -> usize {
    let shown = s.f().frame.clone();
    s.c.damage = s.c.screen();
    s.c.scene = true;
    s.c.step();
    let now = s.f().frame.clone();
    shown.iter().zip(&now).filter(|(a, b)| a != b).count()
}

#[test]
fn the_card_of_a_dragged_window_is_damaged_where_it_was_and_where_it_is() {
    let (mut s, _, (x, y)) = hovered();
    assert_eq!(stale_pixels(&mut s), 0);
    let (dw, dh) = s.f().display_size;
    let ax = |p: i64| super::rooms::axis(p, dw);
    let ay = |p: i64| super::rooms::axis(p, dh);
    s.point(ax(x), ay(y), Some(1));
    s.point(ax(x + 40), ay(y + 100), Some(1));
    s.point(ax(x + 40), ay(y + 100), Some(0));
    assert_eq!(stale_pixels(&mut s), 0, "pixels of the card, old or new");
}

#[test]
fn the_card_of_a_tiled_window_is_damaged_where_it_was_and_where_it_is() {
    let (mut s, _, _) = hovered();
    let meta = code::RIGHT_META;
    s.keys(&[(meta, 1), (code::RIGHT, 1), (code::RIGHT, 0), (meta, 0)]);
    assert_eq!(stale_pixels(&mut s), 0);
}

#[test]
fn a_card_that_shrinks_leaves_nothing_of_the_old_one() {
    let (mut s, panel, _) = hovered();
    let mut r = reading("a1", 20 * MIB);
    r.procs_limit = None;
    r.cpu_permille = None;
    r.disk_limit = None;
    r.conns_limit = None;
    assert_eq!(s.set_usage(panel, &r.to_wire()), status::OK);
    assert!(s.c.card().is_some());
    assert_eq!(stale_pixels(&mut s), 0);
}

#[test]
fn a_hidden_window_has_no_card() {
    let (mut s, _, _) = hovered();
    let key = Key { client: 0, id: 1 };
    s.c.set_hidden(key, true);
    s.c.step();
    assert_eq!(s.c.card(), None);
    assert_eq!(s.c.card_geometry(key).map(|g| g.0), None);
    assert_eq!(stale_pixels(&mut s), 0);
}

#[test]
fn the_card_stays_on_the_screen_at_the_left_limit() {
    let (mut s, _, (x, y)) = hovered();
    let (dw, dh) = s.f().display_size;
    let ax = |p: i64| super::rooms::axis(p, dw);
    let ay = |p: i64| super::rooms::axis(p, dh);
    s.point(ax(x), ay(y), Some(1));
    s.point(ax(0), ay(y), Some(1));
    s.point(ax(0), ay(y), Some(0));
    let (card, _) = s.c.card_geometry(Key { client: 0, id: 1 }).unwrap();
    assert!(card.x >= 0 && card.right() <= i64::from(dw), "{card:?}");
    assert_eq!(stale_pixels(&mut s), 0);
}

impl Setup {
    /// `AddRoomClient` for `room` with `tag`: the status, the client's end
    /// and the tags of the `ClientGone`s the panel got.
    fn try_room_client(&mut self, panel: u32, room: &str, tag: u32) -> (u32, u32, Vec<u32>) {
        let r = self.c.rooms().get(room).cloned();
        let (untrusted, colour) =
            r.map_or((0, 0), |r| (u32::from(r.untrusted), u32::from(r.colour)));
        let end = self.f().channel(64);
        let m = gui::AddRoomClient {
            untrusted,
            colour,
            tag,
            room: zoraya_protocol::Data(room.as_bytes()),
            app: zoraya_protocol::Data(b"term"),
            entries: zoraya_protocol::Data(b""),
            line: zoraya_protocol::Data(b"term"),
        };
        self.f().send(panel, message(&m, 7), vec![end]);
        self.c.step();
        let out = self.f().take(panel);
        let gone = out
            .iter()
            .filter_map(|m| decode::<gui::ClientGone>(m, 0).ok())
            .map(|(_, g)| g.tag)
            .collect();
        let status = out
            .iter()
            .find_map(|m| decode::<gui::ManagerDone>(m, 0).ok())
            .expect("a reply")
            .1
            .status
            .0;
        (status, end, gone)
    }
}

/// INV-0323-1: an untrusted room's clients are held to `ROOM_CLIENTS`, all
/// of them to `UNTRUSTED_CLIENTS`, refused `OVER_QUOTA` with `ClientGone`
/// for the tag; home and a trusted room still add theirs, up to
/// `MAX_CLIENTS` together.
#[test]
fn an_untrusted_rooms_clients_are_counted_in_its_account() {
    use zoraya_proto_gui::ROOM_CLIENTS;
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", TABLE), status::OK);
    let mut tag = 100;
    for room in ["a1", "a2", "a3", "a4"] {
        for _ in 0..ROOM_CLIENTS {
            tag += 1;
            assert_eq!(s.try_room_client(panel, room, tag).0, status::OK, "{room}");
        }
    }
    // a1 at its bound; a5 under its own but past all untrusted rooms'.
    for room in ["a1", "a5"] {
        tag += 1;
        let (st, end, gone) = s.try_room_client(panel, room, tag);
        assert_eq!(st, status::OVER_QUOTA, "{room}");
        assert!(s.f().closed.contains(&end), "the refused end is closed");
        assert_eq!(gone, [tag]);
    }
    let a1 = s.c.accounts().into_iter().find(|a| a.room == "a1").unwrap();
    assert_eq!(a1.refused, 1);
    // The session's clients are not refused for the rooms': up to the
    // whole table.
    let untrusted = UNTRUSTED_ACCOUNTS * ROOM_CLIENTS;
    let before = s.c.client_count();
    assert_eq!(before, untrusted);
    for i in 0..MAX_CLIENTS - untrusted {
        let room = if i % 2 == 0 { "" } else { "project" };
        assert_eq!(s.try_room_client(panel, room, 0).0, status::OK, "{i}");
    }
    assert_eq!(s.c.client_count(), MAX_CLIENTS);
    assert_eq!(s.try_room_client(panel, "", 0).0, status::NO_MEMORY);
}

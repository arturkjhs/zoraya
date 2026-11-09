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
    a: u32,
    b: u32,
    ida: u32,
    oa: Rect,
    ob: Rect,
}

/// Two windows, `files` and `t_open`, the second above the first.
fn two() -> Two {
    let mut s = setup();
    let panel = s.panel();
    assert_eq!(s.set_rooms(panel, "", ROOMS), status::OK);
    let a = s.room_client(panel, "project", "files", "+w /");
    let ida = s.window_of(a, 300, 200, 0x0040_4040);
    let b = s.room_client(panel, "project", "t_open", "+w ~/project");
    let idb = s.window_of(b, 300, 200, 0x0060_6060);
    let (oa, ob) = (s.outer(0, ida), s.outer(1, idb));
    Two {
        s,
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

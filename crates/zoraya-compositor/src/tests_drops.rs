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
        .filter_map(|m| decode::<gui::WindowPointer>(m, 0).ok())
        .map(|(_, p)| p.serial)
        .last()
        .unwrap_or(0)
}

fn drag(s: &mut Setup, end: u32, window: u32, entry: u32) -> u32 {
    let serial = serial(s, end);
    drag_named(s, end, window, entry, serial)
}

fn drag_named(s: &mut Setup, end: u32, window: u32, entry: u32, serial: u32) -> u32 {
    let m = gui::Drag {
        window,
        serial,
        name: zoraya_protocol::Data(b"notes.txt"),
        words: zoraya_protocol::Data(b"+r ~/notes.txt"),
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
    assert_eq!(
        (d.name.0, d.words.0),
        (&b"notes.txt"[..], &b"+r ~/notes.txt"[..])
    );
    assert_eq!(t.s.f().given, [(t.b, entry)]);
    assert!(
        t.s.c
            .record(1)
            .is_some_and(|r| r.entries.iter().any(|e| e == "+r ~/notes.txt")),
        "the target's frame shows the grant"
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
    // Malformed words are refused.
    at(&mut t.s, px, py, Some(1));
    let e4 = t.s.f().channel(1);
    let m = gui::Drag {
        window: t.ida,
        serial: 0,
        name: zoraya_protocol::Data(b"a\x1b[31m"),
        words: zoraya_protocol::Data(b"+r ~/a"),
    };
    t.s.f().send(t.a, message(&m, 4), vec![e4]);
    t.s.c.step();
    assert!(t.s.f().closed.contains(&e4));
    let _ = done;
}

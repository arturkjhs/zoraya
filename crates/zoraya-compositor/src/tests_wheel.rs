//! Proposed ADR-0322: the wheel event, against the fake of `tests.rs`.

use super::drops::{drag, two};
use super::sheets::{sheet, sheet_at_once, system, tagged};
use super::windows::axis;
use super::*;

impl Setup {
    /// One tablet frame of wheel records `(code, value)`, with the pointer
    /// at `(x, y)` physical first.
    fn wheel_at(&mut self, x: i64, y: i64, turns: &[(u32, i32)]) {
        let (w, h) = self.c.size();
        let mut events = vec![
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 0,
                value: axis(x, w),
            },
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 1,
                value: axis(y, h),
            },
        ];
        events.extend(turns.iter().map(|(code, v)| input::Event {
            kind: Below(input::kind::WHEEL),
            code: *code,
            value: *v as u32,
        }));
        let t = self.tablet;
        let m = input::Frame {
            dropped: 0,
            events: Seq::Of(&events),
        };
        self.f().send(t, message(&m, 0), vec![]);
        self.c.step();
    }

    fn new_toplevel(&mut self, w: u32, h: u32) -> u32 {
        let end = self.client(64);
        let s = self.c.scale().factor();
        let n = (w * s * h * s) as usize;
        let memory = self.f().memory(vec![7; n], n as u64 * 4, false);
        self.f().send(
            end,
            message(
                &gui::CreateToplevel {
                    width: w,
                    height: h,
                },
                5,
            ),
            vec![memory],
        );
        self.c.step();
        self.f().take(end);
        let id = self.c.stack().last().unwrap().id;
        assert_eq!(self.commit_to(end, id, 0, 0, w * s, h * s), status::OK);
        end
    }
}

fn wheels(out: &[Vec<u8>]) -> Vec<gui::Wheel> {
    out.iter()
        .filter_map(|m| decode::<gui::Wheel>(m, 0).ok())
        .map(|(_, w)| w)
        .collect()
}

/// A frame's turns go, summed, to the window under the pointer, in its
/// logical pixels, once, after its pointer event; over the frame, the bar
/// or the wallpaper nothing goes.
#[test]
fn a_wheel_reaches_the_body_under_the_pointer_once_a_frame() {
    let mut s = setup();
    let a = s.new_toplevel(100, 60);
    let body = s.body(0, 1);
    s.wheel_at(body.x + 7, body.y + 9, &[(0, 120), (0, 120), (1, -60)]);
    let out = s.f().take(a);
    let w = wheels(&out);
    assert_eq!(w.len(), 1);
    assert_eq!(
        (w[0].window, w[0].x, w[0].y, w[0].dx, w[0].dy),
        (1, 7, 9, -60, 240)
    );
    // The position first, then the turn.
    let order: Vec<u32> = out
        .iter()
        .map(|m| Header::decode(m).unwrap().0.ordinal)
        .filter(|o| [30, 46].contains(o))
        .collect();
    assert_eq!(order, [30, 46]);
    for (x, y) in [
        (body.x + 50, body.y - 10),
        (body.x + 50, 2),
        (W as i64 - 3, H as i64 / 2),
    ] {
        s.wheel_at(x, y, &[(0, 120)]);
        assert!(wheels(&s.f().take(a)).is_empty(), "({x}, {y})");
    }
    // A frame with no turn sends none.
    s.at(body.x + 8, body.y + 9, None);
    assert!(wheels(&s.f().take(a)).is_empty());
}

/// The pointer's rule: an unfocused window under the pointer gets the turn,
/// without the modifiers and without focus; the focused one gets them.
#[test]
fn the_wheel_follows_the_pointer_and_moves_no_focus() {
    let mut s = setup();
    let a = s.new_toplevel(100, 60);
    let b = s.new_toplevel(100, 60);
    let focused = s.c.focused();
    assert_eq!(focused, Some(Key { client: 0, id: 1 }));
    let (ba, bb) = (s.body(0, 1), s.body(1, 1));
    s.keys(&[(code::LEFT_CTRL, 1)]);
    // A place in b's body that a's does not cover (b is above a).
    s.wheel_at(bb.right() - 10, bb.bottom() - 10, &[(0, -120)]);
    let wb = wheels(&s.f().take(b));
    assert_eq!(wb.len(), 1);
    assert_eq!((wb[0].dy, wb[0].modifiers.0), (-120, 0));
    assert!(wheels(&s.f().take(a)).is_empty());
    assert_eq!(s.c.focused(), focused, "a wheel does not focus");
    s.wheel_at(ba.x + 10, ba.y + 10, &[(0, 120)]);
    let wa = wheels(&s.f().take(a));
    assert_eq!(wa.len(), 1);
    assert_eq!(wa[0].modifiers.0, gui::modifiers::CTRL);
}

/// A frame's sum is held to `WHEEL_MAX` either way, and records the input
/// protocol refuses (0, past the bound, a third code) add nothing.
#[test]
fn a_frames_turn_is_bounded_and_bad_records_are_ignored() {
    let mut s = setup();
    let a = s.new_toplevel(100, 60);
    let body = s.body(0, 1);
    let max = input::WHEEL_MAX;
    s.wheel_at(body.x + 10, body.y + 10, &[(0, max); 20]);
    s.wheel_at(body.x + 10, body.y + 10, &[(1, -max); 20]);
    let w = wheels(&s.f().take(a));
    assert_eq!(w.len(), 2, "{w:?}");
    assert_eq!((w[0].dy, w[1].dx), (gui::WHEEL_MAX, -gui::WHEEL_MAX));
    s.wheel_at(
        body.x + 10,
        body.y + 10,
        &[(0, 0), (0, max + 1), (2, 120), (0, i32::MIN)],
    );
    assert!(wheels(&s.f().take(a)).is_empty());
}

/// Nothing during a window's move or with the switcher open.
#[test]
fn no_wheel_during_a_move_or_the_switcher() {
    let mut s = setup();
    let a = s.new_toplevel(200, 100);
    let body = s.body(0, 1);
    let (tx, ty) = (body.x + 100, body.y - 18);
    s.at(tx, ty, Some(1));
    s.wheel_at(tx, ty + 30, &[(0, 120)]);
    s.at(tx, ty + 30, Some(0));
    assert!(wheels(&s.f().take(a)).is_empty(), "during a move");
    let body = s.body(0, 1);
    s.keys(&[(code::LEFT_ALT, 1), (code::TAB, 1), (code::TAB, 0)]);
    assert!(s.c.switcher().is_some());
    s.wheel_at(body.x + 5, body.y + 5, &[(0, 120)]);
    s.keys(&[(code::LEFT_ALT, 0)]);
    assert!(wheels(&s.f().take(a)).is_empty(), "with the switcher open");
    s.wheel_at(body.x + 5, body.y + 5, &[(0, 120)]);
    assert_eq!(wheels(&s.f().take(a)).len(), 1);
}

/// At scale 2 the place is logical; the turn is not scaled.
#[test]
fn at_scale_two_the_place_is_logical() {
    let mut s = setup_with(|f| f.display_size = (2048, 1536));
    let a = s.new_toplevel(100, 60);
    let body = s.body(0, 1);
    s.wheel_at(body.x + 21, body.y + 41, &[(0, 30)]);
    let w = wheels(&s.f().take(a));
    assert_eq!((w[0].x, w[0].y, w[0].dy), (10, 20, 30));
}

/// Where two windows' bodies overlap, only the topmost gets the turn.
#[test]
fn where_two_bodies_overlap_only_the_topmost_gets_the_wheel() {
    let mut s = setup();
    let a = s.new_toplevel(300, 200);
    let b = s.new_toplevel(300, 200);
    let both = s.body(0, 1).intersect(&s.body(1, 1));
    assert!(both.width > 0 && both.height > 0, "the bodies overlap");
    s.wheel_at(
        both.x + both.width / 2,
        both.y + both.height / 2,
        &[(0, 120)],
    );
    assert_eq!(wheels(&s.f().take(b)).len(), 1, "the topmost");
    assert!(wheels(&s.f().take(a)).is_empty(), "the one under it");
}

/// A legacy surface's protocol has no wheel: it gets none.
#[test]
fn a_legacy_surface_gets_no_wheel() {
    let mut s = setup();
    let (end, _) = s.window(100, 60, 0x0040_4040);
    assert_eq!(s.commit(end, 0, 0, 100, 60), status::OK);
    s.f().take(end);
    let body = s.c.window(Key { client: 0, id: 0 }).unwrap().body_rect();
    s.wheel_at(body.x + 10, body.y + 10, &[(0, 120)]);
    assert!(wheels(&s.f().take(end)).is_empty());
}

/// A window with a sheet over it gets no turn, even where the sheet does
/// not cover its body; the sheet itself gets one, once past its guard.
#[test]
fn a_window_under_a_sheet_gets_no_wheel() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    s.window_of(app, 400, 300, 0x0040_4040);
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    s.f().take(app);
    s.f().take(chooser);
    let owner = s.c.stack().find(|k| k.client == 0).unwrap();
    let body = s.c.window(owner).unwrap().body_rect();
    let sheet = s.c.stack().find(|k| k.client == 1).unwrap();
    let over = s.c.window(sheet).unwrap().body_rect();
    let (x, y) = (body.x + 20, body.y + body.height / 2);
    assert!(!over.contains(x, y), "a place of the owner's body only");
    assert!(matches!(s.c.under(x, y), Some((k, Zone::Body)) if k == owner));
    s.wheel_at(x, y, &[(0, 120)]);
    assert!(wheels(&s.f().take(app)).is_empty(), "the owner");
    s.wheel_at(over.x + 10, over.y + 10, &[(0, 120)]);
    assert_eq!(wheels(&s.f().take(chooser)).len(), 1, "the sheet");
}

/// A sheet within its 500 ms guard (ADR-0245) gets no turn; after it, one.
#[test]
fn a_sheet_gets_no_wheel_within_its_guard() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    s.window_of(app, 400, 300, 0x0040_4040);
    let chooser = system(&mut s);
    assert_eq!(sheet_at_once(&mut s, chooser, 4, 160, 80).0, status::OK);
    s.f().take(chooser);
    let sheet = s.c.stack().find(|k| k.client == 1).unwrap();
    let over = s.c.window(sheet).unwrap().body_rect();
    s.wheel_at(over.x + 10, over.y + 10, &[(0, 120)]);
    assert!(wheels(&s.f().take(chooser)).is_empty(), "within the guard");
    s.f().clock += SHEET_GUARD_NS;
    s.wheel_at(over.x + 10, over.y + 10, &[(0, 120)]);
    assert_eq!(wheels(&s.f().take(chooser)).len(), 1, "after it");
}

/// While an entry is carried (ADR-0170) no window gets a turn; after the
/// drop, the window under the pointer does.
#[test]
fn no_wheel_while_an_entry_is_carried() {
    let mut t = two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    t.s.at(px, py, Some(1));
    let entry = t.s.f().channel(1);
    assert_eq!(drag(&mut t.s, t.a, t.ida, entry), status::OK);
    let (tx, ty) = (t.ob.right() - 15, t.ob.bottom() - 15);
    t.s.wheel_at(tx, ty, &[(0, 120)]);
    assert!(wheels(&t.s.f().take(t.b)).is_empty(), "the target");
    assert!(wheels(&t.s.f().take(t.a)).is_empty(), "the source");
    t.s.at(tx, ty, Some(0));
    t.s.f().take(t.b);
    t.s.wheel_at(tx, ty, &[(0, 120)]);
    assert_eq!(wheels(&t.s.f().take(t.b)).len(), 1, "after the drop");
}

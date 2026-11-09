//! Stage 7g: the display follows the window (ADR-0246). The fake driver
//! reports a new size with `Resized`; the compositor asks `Info`, attaches a
//! framebuffer of that size, picks the scale from `gui.scale` or the width,
//! keeps every window in the work area and configures it again.

use super::settings::apply;
use super::windows::configures;
use super::*;

/// The fake driver's display becomes `w × h` and says so.
fn resize(s: &mut Setup, w: u32, h: u32) {
    s.f().display_size = (w, h);
    let display = s.f().display.unwrap();
    let m = display::Resized {
        width: w,
        height: h,
    };
    s.f().send(display, message(&m, 0), vec![]);
    s.c.step();
    s.c.step();
}

/// A frame that lies inside the work area of the display it is on.
fn inside(s: &Setup, r: Rect) -> bool {
    let area = s.c.work_area();
    r.x >= area.x && r.y >= area.y && r.right() <= area.right() && r.bottom() <= area.bottom()
}

#[test]
fn a_new_size_is_attached_with_its_scale_and_the_windows_follow() {
    let mut s = setup();
    let end = s.client(64);
    let id = s.window_of(end, 400, 300, 0x0040_4040);
    s.f().take(end);
    let attaches = s.f().attaches;
    let shown = s.f().shown;

    // Larger, still under 1920 wide: scale 1, the same window.
    resize(&mut s, 1280, 800);
    assert_eq!(s.c.size(), (1280, 800));
    assert_eq!(s.f().attaches, attaches + 1, "a second Attach");
    assert_eq!(s.f().shown, shown + 1, "the old framebuffer may go");
    assert_eq!(s.f().frame.len(), 1280 * 800);
    assert_eq!(s.c.scale().factor(), 1);
    let c = configures(&s.f().take(end));
    assert_eq!(
        (c[0].width, c[0].height, c[0].scale),
        (400, 300, 1),
        "configured again at its size"
    );
    // Everything is drawn again, to the new size's edge.
    assert!(s.f().flushes.contains(&(0, 0, 1280, 800)));
    assert!(inside(&s, s.outer(0, id)));

    // 1920 wide and more: scale 2 under `auto`; the logical size stays.
    resize(&mut s, 2048, 1536);
    assert_eq!(s.c.scale().factor(), 2);
    let c = configures(&s.f().take(end));
    assert_eq!((c[0].width, c[0].height, c[0].scale), (400, 300, 2));
    assert!(inside(&s, s.outer(0, id)));

    // Smaller than the window: it shrinks to the work area and stays on it.
    resize(&mut s, 640, 360);
    assert_eq!(s.c.scale().factor(), 1);
    let c = configures(&s.f().take(end));
    let area = s.c.work_area();
    assert_eq!(c[0].scale, 1);
    assert!(i64::from(c[0].height) < area.height && c[0].height < 300);
    assert!(
        inside(&s, s.outer(0, id)),
        "{:?} in {area:?}",
        s.outer(0, id)
    );
    // Every flush since fits the display it was sent to.
    let last = *s.f().flushes.last().unwrap();
    assert!(last.0 + last.2 <= 640 && last.1 + last.3 <= 360);
}

#[test]
fn a_resize_waits_for_the_flush_in_flight() {
    let mut s = setup();
    let (_, _) = s.window(10, 10, 1);
    s.c.step();
    let display = s.f().display.unwrap();
    // Nothing answers the next flush: it stays in flight.
    s.f().display_dead = true;
    let (_, _) = s.window(10, 10, 2);
    let attaches = s.f().attaches;
    s.f().display_dead = false;
    // One step: a second with nothing ready would pass the flush's
    // deadline (the fake's clock jumps to it) and count the driver hung.
    s.f().display_size = (800, 600);
    let m = display::Resized {
        width: 800,
        height: 600,
    };
    s.f().send(display, message(&m, 0), vec![]);
    s.c.step();
    assert_eq!(s.c.size(), (W, H), "not while a flush is in flight");
    assert_eq!(s.f().attaches, attaches);
    // Its answer comes: then the new size.
    let mut done = vec![0; 64];
    let n = encode(&display::Done { status: Below(0) }, 3, &mut done).unwrap();
    done.truncate(n);
    s.f().send(display, done, vec![]);
    s.c.step();
    assert_eq!(s.c.size(), (800, 600));
    assert_eq!(s.f().attaches, attaches + 1);
}

#[test]
fn a_driver_that_fails_the_new_attach_is_dropped() {
    let mut s = setup();
    s.f().refuse_attach = true;
    resize(&mut s, 800, 600);
    let display = s.f().display.unwrap();
    assert!(
        s.f().closed.contains(&display),
        "the driver's channel is closed"
    );
}

#[test]
fn a_scale_set_in_the_settings_applies_at_once() {
    let mut s = setup_with(|f| f.display_size = (1280, 800));
    let panel = s.panel();
    let end = s.client(64);
    let id = s.window_of(end, 400, 200, 0x0040_4040);
    s.f().take(end);
    let words = |scale| ["light", "yes", scale, "", "us", "us"];
    assert_eq!(apply(&mut s, panel, words("2")), status::OK);
    assert_eq!(s.c.scale().factor(), 2);
    let c = configures(&s.f().take(end));
    assert_eq!(c.len(), 1, "one Configure");
    assert_eq!((c[0].width, c[0].height, c[0].scale), (400, 200, 2));
    assert!(inside(&s, s.outer(0, id)));
    assert_eq!(apply(&mut s, panel, words("auto")), status::OK);
    assert_eq!(s.c.scale().factor(), 1);
    let c = configures(&s.f().take(end));
    assert_eq!((c[0].width, c[0].height, c[0].scale), (400, 200, 1));
}

#[test]
fn a_size_back_to_the_attached_one_attaches_nothing_and_flushes_all() {
    let mut s = setup();
    let attaches = s.f().attaches;
    // The driver said 800 × 600, but the display was back at W × H when the
    // compositor asked: the driver would refuse a second framebuffer of the
    // same size.
    let display = s.f().display.unwrap();
    let m = display::Resized {
        width: 800,
        height: 600,
    };
    s.f().send(display, message(&m, 0), vec![]);
    s.c.step();
    s.c.step();
    assert_eq!(s.c.size(), (W, H));
    assert_eq!(s.f().attaches, attaches, "no Attach");
    assert!(!s.f().closed.contains(&display), "the driver stays");
    // The whole screen goes out: the driver's next request after its Info.
    assert_eq!(s.f().flushes.last(), Some(&(0, 0, W, H)));
}

#[test]
fn popups_close_and_legacy_windows_come_back_at_a_new_size() {
    let mut s = setup_with(|f| f.display_size = (1280, 800));
    let end = s.client(64);
    let id = s.window_of(end, 300, 200, 0x0040_4040);
    // The first window has the focus, so it may open a popup.
    let (legacy, _) = s.window(100, 80, 3);
    s.f().take(legacy);
    let m = s.f().memory(vec![7; 20 * 10], 800, false);
    let out = s.request(
        end,
        &gui::CreatePopup {
            parent: id,
            x: 10,
            y: 20,
            width: 20,
            height: 10,
        },
        vec![m],
    );
    let (_, p) = out
        .iter()
        .find_map(|m| decode::<gui::PopupCreated>(m, 0).ok())
        .unwrap();
    assert_eq!(p.status.0, status::OK);
    // The display shrinks below the legacy window's place (its frame
    // reaches y = 219; the new work area ends far above that).
    let lk = s.c.stack().find(|k| k.client == 1).unwrap();
    assert!(s.outer(1, lk.id).bottom() > 160);
    resize(&mut s, 320, 260);
    let closed = s
        .f()
        .take(end)
        .iter()
        .any(|m| decode::<gui::CloseRequested>(m, 0).is_ok_and(|(_, c)| c.window == p.window));
    assert!(closed, "the popup is closed, and its client told");
    assert!(
        s.c.window(Key {
            client: 0,
            id: p.window
        })
        .is_none()
    );
    let legacy_frame = s.outer(1, lk.id);
    assert!(inside(&s, legacy_frame), "{legacy_frame:?}");
}

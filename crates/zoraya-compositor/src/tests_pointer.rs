//! TASK-31 `gui-smooth`, proposed ADR-0467: the hardware cursor (the host
//! draws the pointer, the compositor tells where it is), the pointer's
//! messages coalesced to one position a round, and frames paced to the
//! display's refresh; against the fake of `tests.rs`.

use super::windows::axis;
use super::*;

impl Setup {
    /// The pointer to `(x, y)` physical, one tablet frame, one round.
    fn move_to(&mut self, x: i64, y: i64) {
        let (w, h) = self.c.size();
        let (ax, ay) = (axis(x, w), axis(y, h));
        self.point(ax, ay, None);
    }

    /// A tablet frame to `(x, y)` physical queued, no round run.
    fn queue_move(&mut self, x: i64, y: i64) {
        let (w, h) = self.c.size();
        let events = [
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
        let t = self.tablet;
        let m = input::Frame {
            dropped: 0,
            events: Seq::Of(&events),
        };
        self.f().send(t, message(&m, 0), vec![]);
    }
}

fn setup_cursor() -> Setup {
    setup_with(|f| f.accept_cursor = true)
}

#[test]
fn the_pointer_is_drawn_by_the_compositor_until_the_driver_holds_an_image() {
    let mut s = setup_with(|_| {});
    s.move_to(400, 300);
    assert!(!s.c.hw_cursor, "the driver refused the image");
    // The compositor's pointer is in the frame at its place.
    assert_ne!(s.pixel(400, 300), s.paper(400, 300));
    // And it is not asked again for the same image.
    let before = s.f().flushes.len();
    s.move_to(410, 310);
    assert!(s.f().cursor_moves.is_empty());
    assert!(s.f().flushes.len() > before, "a frame for the move");
}

#[test]
fn a_hardware_cursor_moves_with_no_frame() {
    let mut s = setup_cursor();
    s.move_to(400, 300);
    assert!(s.c.hw_cursor);
    let sets = s.f().cursor_sets.clone();
    assert_eq!(sets.len(), 1, "one image");
    let (hot, at, image) = &sets[0];
    assert_eq!(*hot, (0, 0));
    assert!(
        (399..=401).contains(&at.0) && (299..=301).contains(&at.1),
        "{at:?}"
    );
    assert_eq!(image.len(), zoraya_proto_display::CURSOR_BYTES);
    // The image is the compositor's: the pointer's dots in the palette's
    // colours, opaque; clear beyond them.
    let word = |x: usize, y: usize| {
        let i = (y * 64 + x) * 4;
        u32::from_le_bytes(image[i..i + 4].try_into().unwrap())
    };
    let p = s.c.palette();
    assert_eq!(word(0, 0), 0xFF00_0000 | p.fg_text.pixel());
    assert_eq!(word(1, 3), 0xFF00_0000 | p.bg_control.pixel());
    assert_eq!(word(40, 40), 0);
    // The frame that took the software pointer out comes next; after it, a
    // move is a message and no frame, and the frame holds no pointer.
    s.c.step();
    s.f().flushes.clear();
    let frames = s.c.stats().frames;
    s.move_to(500, 400);
    s.move_to(520, 410);
    assert_eq!(s.c.stats().frames, frames, "no frame composed");
    assert!(s.f().flushes.is_empty(), "no flush sent");
    assert_eq!(s.f().cursor_moves.len(), 2);
    let (x, y) = s.f().cursor_moves[1];
    assert!((519..=521).contains(&x) && (409..=411).contains(&y));
    assert_eq!(s.pixel(520, 410), s.paper(520, 410));
}

#[test]
fn the_software_pointer_goes_from_the_frame_when_the_host_takes_over() {
    let mut s = setup_cursor();
    // No pointer has spoken: nothing drawn, nothing sent.
    assert!(s.f().cursor_sets.is_empty() && !s.c.hw_cursor);
    s.move_to(300, 200);
    assert!(s.c.hw_cursor);
    // The frame composed after the image was taken has no pointer at its
    // place (the first move's frame had it, and is damaged again).
    s.move_to(300, 200);
    s.c.step();
    assert_eq!(s.pixel(300, 200), s.paper(300, 200));
}

#[test]
fn a_scale_or_appearance_change_gives_the_driver_a_new_image() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    assert_eq!(s.f().cursor_sets.len(), 1);
    let g = crate::settings::GuiSettings {
        dark: true,
        ..Default::default()
    };
    s.c.apply_settings(g);
    s.c.step();
    s.move_to(310, 200);
    assert_eq!(s.f().cursor_sets.len(), 2, "dark: another image");
    let sets = s.f().cursor_sets.clone();
    assert_ne!(sets[0].2, sets[1].2);
    assert!(s.c.hw_cursor);
}

#[test]
fn a_new_driver_has_no_image_and_the_compositor_draws_the_pointer_again() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    assert!(s.c.hw_cursor);
    s.c.lose_display();
    assert!(!s.c.hw_cursor && s.c.cursor_key.is_none());
}

#[test]
fn queued_pointer_messages_are_one_position_and_one_frame() {
    let mut s = setup_with(|_| {});
    let (a, _) = s.window(60, 60, 1);
    s.f().take(a);
    s.move_to(300, 200);
    let frames = s.c.stats().frames;
    // Ten messages waiting when the round starts: one round takes them all.
    for i in 0..10 {
        s.queue_move(310 + i * 5, 210 + i * 3);
    }
    s.c.step();
    assert_eq!(s.c.stats().frames, frames + 1, "one frame for ten messages");
    assert_eq!(s.c.pointer, Some((355, 237)));
    let t = s.tablet;
    assert!(s.f().peers[&t].inbox.is_empty());
}

#[test]
fn a_press_between_motions_is_delivered_in_its_own_place_and_order() {
    let mut s = setup_with(|_| {});
    let (a, _) = s.window(200, 200, 1);
    s.f().take(a);
    let at = (BX + i64::from(CASCADE) / 2 + 20, BY + 20);
    let (w, h) = s.c.size();
    s.queue_move(at.0, at.1);
    // A press with the pointer there, then a move away: the client hears
    // the press where it happened (buttons down), not only the last place.
    let events = [
        input::Event {
            kind: Below(input::kind::KEY),
            code: code::BTN_LEFT,
            value: 1,
        },
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 0,
            value: axis(at.0 + 30, w),
        },
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 1,
            value: axis(at.1 + 30, h),
        },
    ];
    let t = s.tablet;
    let m = input::Frame {
        dropped: 0,
        events: Seq::Of(&events),
    };
    s.f().send(t, message(&m, 0), vec![]);
    s.queue_move(at.0 + 60, at.1 + 60);
    s.c.step();
    let got: Vec<_> = s
        .f()
        .take(a)
        .iter()
        .filter_map(|m| decode::<gui::Pointer>(m, 0).ok())
        .map(|(_, p)| (p.x, p.y, p.buttons.0))
        .collect();
    assert!(!got.is_empty(), "the client hears the pointer");
    // A message that changes a button is delivered at once: the last event
    // of the round has the button still down at the final place.
    let last = got.last().unwrap();
    assert_eq!(last.2 & 1, 1, "{got:?}");
    assert!(
        got.len() >= 2,
        "the first move and the press come apart: {got:?}"
    );
}

#[test]
fn frames_are_at_most_one_per_interval_when_one_is_set() {
    let mut s = setup_with(|_| {});
    s.c.set_frame_interval(16_000_000);
    s.move_to(300, 200);
    let frames = s.c.stats().frames;
    // Moves 1 ms apart: held until the interval has passed since the last
    // frame, and then one frame for all of them.
    for i in 0..5 {
        s.f().clock += 1_000_000;
        s.move_to(310 + i * 10, 200);
    }
    assert_eq!(s.c.stats().frames, frames, "held");
    s.f().clock += 20_000_000;
    s.c.step();
    assert_eq!(s.c.stats().frames, frames + 1);
}

#[test]
fn a_commit_is_answered_in_its_round_even_when_a_frame_is_held() {
    let mut s = setup_with(|_| {});
    s.c.set_frame_interval(16_000_000);
    let (a, _) = s.window(60, 60, 1);
    s.f().take(a);
    s.move_to(300, 200);
    let m = gui::Commit {
        x: 0,
        y: 0,
        width: 60,
        height: 60,
    };
    s.f().send(a, message(&m, 3), vec![]);
    s.c.step();
    // Its frame_done is in this very round: the client is never left
    // readable with a commit unanswered (the loop would spin on it).
    assert_eq!(s.frame_done(a), Some(status::OK));
}

#[test]
fn a_held_frame_is_due_at_the_interval_and_the_wait_ends_there() {
    let mut s = setup_with(|_| {});
    s.c.set_frame_interval(16_000_000);
    s.move_to(300, 200);
    s.f().clock += 1_000_000;
    s.move_to(320, 210);
    let due = s.c.frame_due().expect("a frame is held");
    assert!(due > s.f().clock);
    s.f().clock = due;
    let frames = s.c.stats().frames;
    s.c.step();
    assert_eq!(s.c.stats().frames, frames + 1, "composed when due");
    assert_eq!(s.c.frame_due(), None);
}

/// A hardware-cursor desktop with two windows, the second on top, and the
/// person dragging its title bar along `path` (a point each round); the
/// frame after each round, as the compositor composed it.
fn drag_frames(blit: bool, scale_two: bool, path: &[(i64, i64)]) -> (Vec<Vec<u32>>, u64) {
    // A display of 2048 wide has scale 2 (`SCALE_TWO_FROM`).
    let mut s = setup_with(|f| {
        f.accept_cursor = true;
        if scale_two {
            f.display_size = (2048, 1536);
        }
    });
    assert_eq!(s.c.scale().factor(), if scale_two { 2 } else { 1 });
    s.c.drag_blit = blit;
    let (_a, _, _) = s.toplevel(180, 120, 0x30_6090);
    let (b, _, _) = s.toplevel(200, 140, 0x90_5030);
    s.f().take(b);
    // The pointer to the top window's title bar, once, so that the driver
    // holds the image (the hardware cursor).
    let body = s.body(1, 1);
    let (tx, ty) = (body.x + 60, body.y - 18);
    s.at(tx, ty, None);
    s.at(tx, ty, Some(1));
    s.at(tx + 1, ty, None);
    assert!(s.c.hw_cursor);
    let mut out = Vec::new();
    for (dx, dy) in path {
        s.at(tx + dx, ty + dy, None);
        out.push(s.f().frame.clone());
    }
    s.at(
        tx + path.last().unwrap().0,
        ty + path.last().unwrap().1,
        Some(0),
    );
    out.push(s.f().frame.clone());
    (out, s.c.stats().blits)
}

#[test]
fn a_moved_window_is_copied_and_the_frame_is_what_a_whole_compose_makes() {
    let path = [
        (5, 3),
        (12, 9),
        (20, 11),
        (31, 30),
        (25, 40),
        (10, 38),
        (-4, 20),
    ];
    let (whole, none) = drag_frames(false, false, &path);
    let (copied, some) = drag_frames(true, false, &path);
    assert_eq!(none, 0);
    assert!(some >= 3, "frames made by copying: {some}");
    assert_eq!(whole.len(), copied.len());
    for (i, (a, b)) in whole.iter().zip(&copied).enumerate() {
        let bad = a.iter().zip(b).filter(|(x, y)| x != y).count();
        assert_eq!(bad, 0, "round {i}: {bad} pixels differ");
    }
}

#[test]
fn the_copy_is_the_same_at_scale_two() {
    let path = [(6, 4), (14, 10), (30, 22), (20, 35)];
    let (whole, _) = drag_frames(false, true, &path);
    let (copied, blits) = drag_frames(true, true, &path);
    assert!(blits >= 2, "{blits}");
    for (i, (a, b)) in whole.iter().zip(&copied).enumerate() {
        assert_eq!(a, b, "round {i}");
    }
}

#[test]
fn a_change_in_the_moved_window_or_elsewhere_is_never_copied_over() {
    let mut s = setup_cursor();
    let (_a, _, _) = s.toplevel(180, 120, 0x30_6090);
    let (b, mem, _) = s.toplevel(200, 140, 0x90_5030);
    s.f().take(b);
    let body = s.body(1, 1);
    let (tx, ty) = (body.x + 60, body.y - 18);
    s.at(tx, ty, None);
    s.at(tx, ty, Some(1));
    s.at(tx + 1, ty, None);
    s.at(tx + 10, ty + 5, None);
    let blits = s.c.stats().blits;
    // The client draws in the window while it is moved: whole compose.
    s.f().memory.get_mut(&mem).unwrap().pixels.fill(0x11_2233);
    let m = gui::CommitTo {
        window: 1,
        x: 0,
        y: 0,
        width: 200,
        height: 140,
    };
    s.f().send(b, message(&m, 7), vec![]);
    s.at(tx + 20, ty + 9, None);
    assert_eq!(s.c.stats().blits, blits, "the commit's pixels are drawn");
    let (px, py) = (s.c.window(Key { client: 1, id: 1 }).unwrap().x, 0);
    let _ = py;
    let y = s.c.window(Key { client: 1, id: 1 }).unwrap().y;
    assert_eq!(s.pixel(px + 50, y + 50), 0x11_2233);
}

#[test]
fn a_window_moved_under_the_dock_or_the_bar_is_copied_only_where_nothing_is_over_it() {
    // Down into the dock's zone, up under the bar, and sideways off the
    // screen's edge: every frame as a whole compose makes it.
    let path = [
        (0, 100),
        (30, 250),
        (60, 420),
        (60, 560),
        (40, 300),
        (0, -20),
        (-300, -60),
        (-900, -40),
        (-30, 80),
    ];
    for scale_two in [false, true] {
        let (whole, _) = drag_frames(false, scale_two, &path);
        let (copied, blits) = drag_frames(true, scale_two, &path);
        assert!(blits >= 2, "scale two {scale_two}: {blits}");
        for (i, (a, b)) in whole.iter().zip(&copied).enumerate() {
            let bad = a.iter().zip(b).filter(|(x, y)| x != y).count();
            assert_eq!(bad, 0, "scale two {scale_two}, round {i}: {bad} differ");
        }
    }
}

#[test]
fn a_drag_with_a_move_every_ten_ms_is_a_frame_every_interval() {
    let mut s = setup_with(|f| f.accept_cursor = true);
    s.c.set_frame_interval(16_666_667);
    let (_a, _, _) = s.toplevel(180, 120, 0x30_6090);
    let body = s.body(0, 1);
    let (tx, ty) = (body.x + 60, body.y - 18);
    s.at(tx, ty, None);
    s.at(tx, ty, Some(1));
    s.at(tx + 1, ty, None);
    // A second of fake time: a move each 10 ms, the loop waiting to its
    // deadline in between.
    let start = s.f().clock;
    let frames = s.c.stats().frames;
    let mut i = 0;
    while s.f().clock < start + 1_000_000_000 {
        let next = start + (i + 1) * 10_000_000;
        // Rounds until the next move is due: the compositor's deadline
        // (a held frame) comes first when it is before.
        while s.f().clock < next {
            let due = s.c.frame_due().map_or(next, |d| d.min(next));
            s.f().clock = s.f().clock.max(due);
            if due == next {
                break;
            }
            s.c.step();
        }
        s.at(tx + 2 + (i as i64 % 200), ty + (i as i64 % 90), None);
        i += 1;
    }
    let made = s.c.stats().frames - frames;
    assert!((55..=65).contains(&made), "{made} frames in a second");
}

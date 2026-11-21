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
        // The driver's answer to an image comes with the next round's
        // messages (it is not waited for).
        let display = self.f().display.unwrap();
        if !self.f().peers[&display].inbox.is_empty() {
            self.c.step();
        }
    }

    /// The frames of input `end` not yet told: in its channel, and the one
    /// read as its head (proposed ADR-0469).
    fn untold(&mut self, end: u32) -> usize {
        let slot = self.c.inputs.iter().position(|e| *e == Some(end));
        let head = slot.is_some_and(|i| self.c.heads[i].held);
        self.f().peers[&end].inbox.len() + usize::from(head)
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
            time: self.f().time(),
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
        time: s.f().time(),
        events: Seq::Of(&events),
    };
    s.f().send(t, message(&m, 0), vec![]);
    s.queue_move(at.0 + 60, at.1 + 60);
    // The press ends the device's round; the move after it is the next.
    s.c.step();
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

/// A digest of a frame (FNV-1a over its words): frames of a 2048 by 1536
/// display are 12 MiB, too many to keep for the memory cap of a test.
fn digest(frame: &[u32]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325, |h, w| {
        (h ^ u64::from(*w)).wrapping_mul(0x100_0000_01b3)
    })
}

/// A hardware-cursor desktop with two windows, the second on top, and the
/// person dragging its title bar along `path` (a point each round); the
/// frame after each round, as the compositor composed it.
fn drag_frames(blit: bool, scale_two: bool, path: &[(i64, i64)]) -> (Vec<u64>, u64) {
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
        out.push(digest(&s.f().frame));
    }
    s.at(
        tx + path.last().unwrap().0,
        ty + path.last().unwrap().1,
        Some(0),
    );
    out.push(digest(&s.f().frame));
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
        assert_eq!(a, b, "round {i}: the frames differ");
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
            assert_eq!(a, b, "scale two {scale_two}, round {i}: the frames differ");
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

fn done_for_image(status: u32) -> Vec<u8> {
    message(
        &display::Done {
            status: Below(status),
        },
        CURSOR_TRANSACTION,
    )
}

/// The `Resized` of the fake driver at `w × h`, queued.
fn queue_resize(s: &mut Setup, w: u32, h: u32) {
    s.f().display_size = (w, h);
    let display = s.f().display.unwrap();
    let m = display::Resized {
        width: w,
        height: h,
    };
    s.f().send(display, message(&m, 0), vec![]);
}

#[test]
fn a_resize_met_while_the_image_is_unanswered_is_not_lost() {
    // INV-0467-3: the image is a request, not a call, so a `Resized` the
    // driver wrote before its answer is not taken for the answer.
    let mut s = setup_with(|f| {
        f.accept_cursor = true;
        f.cursor_hold = true;
    });
    s.move_to(300, 200);
    assert!(s.c.cursor_sent.is_some(), "the image is asked for");
    assert!(!s.c.hw_cursor);
    let display = s.f().display.unwrap();
    queue_resize(&mut s, 2048, 1536);
    s.f().send(display, done_for_image(0), vec![]);
    s.c.step();
    assert!(s.c.resized, "the size waits for the image's answer");
    s.c.step();
    assert!(s.c.display.is_some(), "the display driver is kept");
    assert_eq!(s.c.stats().hangs, 0);
    assert_eq!(s.c.size(), (2048, 1536), "the new size was taken");
    assert!(s.c.hw_cursor, "the image was taken");
}

#[test]
fn the_answers_to_a_flush_and_to_an_image_are_told_apart() {
    let mut s = setup_with(|f| {
        f.accept_cursor = true;
        f.cursor_hold = true;
    });
    s.move_to(300, 200);
    assert!(s.c.cursor_sent.is_some());
    // A frame is flushed (answered at once) while the image is unanswered.
    let (a, _) = s.window(60, 60, 1);
    s.f().take(a);
    let display = s.f().display.unwrap();
    // Only while an answer waits: an idle round would wait out the deadline.
    while !s.f().peers[&display].inbox.is_empty() {
        s.c.step();
    }
    assert_eq!(s.c.stats().hangs, 0);
    assert!(s.c.stats().flushes > 0 && s.c.in_flight.is_none());
    assert!(
        s.c.cursor_sent.is_some(),
        "the image's was not taken for it"
    );
    s.f().send(display, done_for_image(0), vec![]);
    s.c.step();
    assert!(s.c.cursor_sent.is_none() && s.c.hw_cursor);
    assert!(s.c.display.is_some());
}

#[test]
fn a_driver_that_never_answers_the_image_is_found_hung_with_only_the_pointer_moving() {
    let mut s = setup_with(|f| {
        f.accept_cursor = true;
        f.cursor_hold = true;
    });
    let start = s.f().clock;
    s.move_to(300, 200);
    assert!(s.c.cursor_sent.is_some());
    // The compositor was not waiting: it is here, and the loop's deadline
    // is the flush's.
    for i in 0..400 {
        if s.c.stats().hangs > 0 {
            break;
        }
        s.move_to(300 + i % 50, 200);
        s.c.step();
    }
    assert_eq!(s.c.stats().hangs, 1);
    assert!(s.f().clock >= start + FLUSH_DEADLINE_NS);
}

#[test]
fn a_driver_that_never_makes_room_for_a_move_is_found_hung() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    s.c.step();
    assert!(s.c.hw_cursor && s.c.cursor_sent.is_none());
    s.f().display_full = true;
    for i in 0..1000 {
        if s.c.stats().hangs > 0 {
            break;
        }
        s.move_to(310 + i % 50, 200);
        s.c.step();
    }
    assert_eq!(s.c.stats().hangs, 1, "found with no flush due");
}

#[test]
fn a_move_that_found_the_queue_full_is_told_again_with_the_next_place() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    s.c.step();
    let told = s.f().cursor_moves.len();
    s.f().display_full = true;
    s.move_to(500, 400);
    assert_eq!(s.f().cursor_moves.len(), told, "nothing was sent");
    s.f().display_full = false;
    s.move_to(520, 410);
    s.c.step();
    let (x, y) = *s.f().cursor_moves.last().unwrap();
    assert!(
        (519..=521).contains(&x) && (409..=411).contains(&y),
        "{x} {y}"
    );
}

#[test]
fn a_passing_failure_of_the_image_is_asked_again() {
    let mut s = setup_with(|f| {
        f.accept_cursor = true;
        f.cursor_fail_first = 2;
    });
    s.move_to(300, 200);
    for _ in 0..10 {
        s.c.step();
    }
    assert!(s.c.hw_cursor, "the third try was taken");
    assert_eq!(s.f().cursor_asked, 3);
    // Always failing: given up after three tries, the software pointer stays.
    let mut s = setup_with(|f| {
        f.accept_cursor = true;
        f.cursor_fail_first = 100;
    });
    s.move_to(300, 200);
    for _ in 0..20 {
        s.c.step();
    }
    assert!(!s.c.hw_cursor);
    assert_eq!(s.f().cursor_asked, 3);
}

#[test]
fn a_refused_new_image_leaves_the_old_one_moving_not_two_pointers() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    s.c.step();
    assert!(s.c.hw_cursor);
    s.f().cursor_fail_first = 100;
    let g = crate::settings::GuiSettings {
        dark: true,
        ..Default::default()
    };
    s.c.apply_settings(g);
    for _ in 0..20 {
        s.c.step();
    }
    assert!(s.c.hw_cursor, "the old image is still the host's pointer");
    s.move_to(400, 300);
    s.c.step();
    let (x, y) = *s.f().cursor_moves.last().unwrap();
    assert!((399..=401).contains(&x) && (299..=301).contains(&y));
    assert_eq!(s.pixel(400, 300), s.paper(400, 300), "none drawn here");
}

/// The image `cursor_image` should be: the dots of `POINTER` in the
/// palette's colours at `k`, clear elsewhere.
fn expected_image(p: &zoraya_tokens::Palette, k: usize) -> Vec<u8> {
    let mut out = vec![0u8; zoraya_proto_display::CURSOR_BYTES];
    for (dy, line) in draw::POINTER.iter().enumerate() {
        for (dx, dot) in line.iter().enumerate() {
            let color = match dot {
                0 => continue,
                1 => p.bg_control,
                _ => p.fg_text,
            };
            let word = 0xFF00_0000 | color.pixel();
            for y in dy * k..(dy + 1) * k {
                for x in dx * k..(dx + 1) * k {
                    let at = (y * 64 + x) * 4;
                    out[at..at + 4].copy_from_slice(&word.to_le_bytes());
                }
            }
        }
    }
    out
}

#[test]
fn the_whole_image_is_the_dots_at_the_scale() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    let p = s.c.palette();
    let sets = s.f().cursor_sets.clone();
    assert_eq!(sets[0].2, expected_image(p, 1), "scale one");
    // A display of 2048 by 1536 is scale two: another image, whole.
    queue_resize(&mut s, 2048, 1536);
    for _ in 0..6 {
        s.c.step();
    }
    s.move_to(310, 210);
    s.c.step();
    let p = s.c.palette();
    let sets = s.f().cursor_sets.clone();
    assert_eq!(sets.len(), 2, "a new image for the new scale");
    assert_eq!(sets[1].2, expected_image(p, 2), "scale two");
}

#[test]
fn a_round_reads_at_most_max_drain_messages_of_a_device() {
    let mut s = setup_with(|_| {});
    s.move_to(300, 200);
    for i in 0..40 {
        s.queue_move(310 + i, 210);
    }
    s.c.step();
    let t = s.tablet;
    assert_eq!(s.untold(t), 40 - MAX_DRAIN);
    s.c.step();
    assert_eq!(s.untold(t), 0);
}

#[test]
fn an_event_after_an_idle_interval_is_drawn_at_once_and_one_right_after_a_frame_waits() {
    let mut s = setup_with(|_| {});
    s.c.set_frame_interval(16_666_667);
    s.move_to(300, 200);
    let frames = s.c.stats().frames;
    // Idle for more than an interval: the next change is composed in its
    // own round, no wait.
    s.f().clock += 40_000_000;
    s.move_to(320, 210);
    assert_eq!(s.c.stats().frames, frames + 1, "no wait after idle");
    assert_eq!(s.c.frame_due(), None);
    // A change 2 ms after that frame waits for the slot.
    s.f().clock += 2_000_000;
    s.move_to(340, 220);
    assert_eq!(s.c.stats().frames, frames + 1, "held right after a frame");
    let due = s.c.frame_due().expect("a frame is held");
    assert!(due > s.f().clock && due <= s.f().clock + 16_666_667);
}

#[test]
fn a_move_that_found_the_queue_full_and_was_then_not_needed_is_not_a_hang() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    s.c.step();
    let at = s.c.pointer.unwrap();
    s.f().display_full = true;
    s.move_to(400, 300);
    assert!(s.c.cursor_stuck.is_some());
    // The pointer is back where the host has it: nothing to send.
    s.move_to(at.0, at.1);
    s.f().display_full = false;
    for _ in 0..400 {
        s.f().clock += 10_000_000;
        s.c.step();
    }
    assert_eq!(s.c.stats().hangs, 0);
    assert!(s.c.cursor_stuck.is_none());
}

#[test]
fn an_unanswered_image_is_found_even_while_flushes_are_answered() {
    let mut s = setup_with(|f| {
        f.accept_cursor = true;
        f.cursor_hold = true;
    });
    s.move_to(300, 200);
    assert!(s.c.cursor_sent.is_some());
    for i in 0..600 {
        if s.c.stats().hangs > 0 {
            break;
        }
        s.f().clock += 10_000_000;
        // The software pointer is drawn until the image is held: a frame
        // and a flush for each move.
        s.move_to(300 + i % 50, 200);
    }
    assert_eq!(s.c.stats().hangs, 1);
    assert!(s.c.stats().flushes > 10, "flushes were answered meanwhile");
}

#[test]
fn an_image_answer_nobody_asked_for_is_a_broken_driver() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    s.c.step();
    assert!(s.c.cursor_sent.is_none());
    let display = s.f().display.unwrap();
    s.f().send(display, done_for_image(0), vec![]);
    while s.c.display.is_some() && !s.f().peers[&display].inbox.is_empty() {
        s.c.step();
    }
    assert!(s.c.display.is_none(), "the display is dropped");
}

#[test]
fn moves_go_while_a_new_image_is_unanswered() {
    let mut s = setup_cursor();
    s.move_to(300, 200);
    s.c.step();
    s.f().cursor_hold = true;
    let g = crate::settings::GuiSettings {
        dark: true,
        ..Default::default()
    };
    s.c.apply_settings(g);
    s.move_to(310, 200);
    assert!(s.c.cursor_sent.is_some());
    let told = s.f().cursor_moves.len();
    s.move_to(400, 300);
    assert!(s.f().cursor_moves.len() > told, "a move went meanwhile");
    // The image's place and the moves are the host's: the compositor knows
    // where it is.
    assert_eq!(s.c.cursor_at, s.c.pointer);
}

/// A press held in one window's body and the pointer's moves after it, all
/// waiting when the round starts, to a place over another window: the
/// window that has the press hears the pointer move with the button down
/// before the pointer leaves it, so that it can start a drag (ADR-0170).
/// Told only at the end, the move went to the window under the last place,
/// and the source never heard its press move: the file manager under TCG
/// started no drag, and `t_open` took the moves and the release for a click
/// on Save (`make test-gui GUI_RUNS=files:tcg:1`, step 45, 2026-11-21).
#[test]
fn a_held_press_moving_out_of_its_window_is_told_to_it_on_the_way() {
    let mut t = super::drops::two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    let (tx, ty) = (t.ob.right() - 15, t.ob.bottom() - 15);
    assert!(!t.oa.contains(tx, ty) && t.ob.contains(tx, ty));
    t.s.f().take(t.a);
    t.s.f().take(t.b);
    // The runner's `drag`: the press, then eight moves, all read in one
    // round by a slow guest.
    let (w, h) = t.s.f().display_size;
    t.s.point(axis(px, w), axis(py, h), Some(1));
    for i in 1..=8 {
        t.s.queue_move(px + (tx - px) * i / 8, py + (ty - py) * i / 8);
    }
    t.s.c.step();
    // Read, not taken: the drag below names the press by its serial.
    let heard: Vec<_> = t.s.f().peers[&t.a]
        .outbox
        .iter()
        .filter_map(|m| decode::<gui::WindowPointerMods>(m, 0).ok())
        .map(|(_, p)| (p.x, p.y, p.buttons.0))
        .collect();
    let press = heard.first().copied().expect("the press is told");
    assert_eq!(press.2 & gui::buttons::LEFT, gui::buttons::LEFT);
    assert!(
        heard
            .iter()
            .any(|p| p.2 & gui::buttons::LEFT != 0 && (p.0, p.1) != (press.0, press.1)),
        "the source hears its press move: {heard:?}"
    );
    // The source answers with a drag, which the compositor takes: the
    // press is still held. The release over the other window drops there.
    let entry = t.s.f().channel(1);
    assert_eq!(super::drops::drag(&mut t.s, t.a, t.ida, entry), status::OK);
    t.s.f().take(t.b);
    t.s.point(axis(tx, w), axis(ty, h), Some(0));
    assert!(
        t.s.f()
            .take(t.b)
            .iter()
            .any(|r| decode::<gui::Drop<'_>>(r, 1).is_ok()),
        "the other window gets the drop"
    );
}

/// A press held from one window over another, before the source names its
/// drag: the other window hears the pointer with no button, so that what
/// it passes over (a button) is not pressed; the drop that follows is its
/// only news. Under TCG `t_open` took such moves for a press on Save, and
/// its sheet then refused the drop (`files:tcg:1`, 2026-11-21).
#[test]
fn a_press_from_another_window_is_never_held_in_the_window_it_crosses() {
    let mut t = super::drops::two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    let (tx, ty) = (t.ob.right() - 15, t.ob.bottom() - 15);
    let (w, h) = t.s.f().display_size;
    t.s.point(axis(px, w), axis(py, h), Some(1));
    t.s.f().take(t.b);
    // The moves, one a round, reach the other window before the drag.
    for i in 1..=8 {
        t.s.move_to(px + (tx - px) * i / 8, py + (ty - py) * i / 8);
    }
    let heard = |out: &[Vec<u8>]| -> Vec<u32> {
        out.iter()
            .filter_map(|m| decode::<gui::WindowPointerMods>(m, 0).ok())
            .map(|(_, p)| p.buttons.0)
            .collect()
    };
    let b = heard(&t.s.f().take(t.b));
    assert!(!b.is_empty(), "the other window hears the pointer");
    assert!(b.iter().all(|&k| k == 0), "with no button held: {b:?}");
    // The source's drag is still taken (its press is held), and the release
    // drops on the other window.
    let entry = t.s.f().channel(1);
    assert_eq!(super::drops::drag(&mut t.s, t.a, t.ida, entry), status::OK);
    t.s.point(axis(tx, w), axis(ty, h), Some(0));
    let got = t.s.f().take(t.b);
    assert!(got.iter().any(|r| decode::<gui::Drop<'_>>(r, 1).is_ok()));
    assert!(heard(&got).iter().all(|&k| k == 0));
    // The same window, pressed in itself, hears its own press held.
    t.s.point(axis(tx, w), axis(ty, h), Some(1));
    t.s.move_to(tx - 5, ty - 5);
    let b = heard(&t.s.f().take(t.b));
    assert!(b.iter().any(|&k| k & gui::buttons::LEFT != 0), "{b:?}");
    t.s.point(axis(tx - 5, w), axis(ty - 5, h), Some(0));
    // A press outside every window is held in none.
    t.s.point(axis(2, w), axis(H as i64 / 2, h), Some(1));
    t.s.move_to(tx, ty);
    let b = heard(&t.s.f().take(t.b));
    assert!(b.iter().all(|&k| k == 0), "pressed on the desktop: {b:?}");
}

/// The four ways two devices reach the compositor: the port serving the
/// keyboard first or the tablet first, and the keyboard or the tablet in
/// input slot 0 (the real system's: `init` attaches the tablet first).
const CASES: [(bool, bool); 4] = [(false, false), (true, false), (false, true), (true, true)];

/// What window A heard of two devices' frames.
struct Heard {
    /// The modifiers of the press.
    modifiers: u32,
    keys: Vec<(u32, bool, Option<char>)>,
    /// The press came before the first key.
    press_first: bool,
    /// Times counted as odd.
    odd: u64,
}

/// What two devices' frames waiting together tell window A (focused by a
/// plain click first): the keyboard's frames and the tablet's, each with
/// its driver's time, all queued before the rounds run, in `case` (the
/// port serving the tablet first, the tablet in slot 0). `late`: the
/// compositor reads them this long after they were stamped.
fn two_devices(
    case: (bool, bool),
    keyboard: &[(u64, u32, u32)],
    tablet: &[(u64, u32)],
    late: u64,
) -> Heard {
    let (reversed, tablet_first) = case;
    let mut t = if tablet_first {
        super::drops::two_tablet_first()
    } else {
        super::drops::two()
    };
    t.s.f().inputs_reversed = reversed;
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    let (w, h) = t.s.f().display_size;
    t.s.point(axis(px, w), axis(py, h), Some(1));
    t.s.point(axis(px, w), axis(py, h), Some(0));
    t.s.f().take(t.a);
    let key = |code: u32, value: u32| input::Event {
        kind: Below(input::kind::KEY),
        code,
        value,
    };
    // Times just before the clock, as a driver's are when the compositor
    // reads them.
    let base = t.s.f().clock;
    let frame = |time: u64, events: &[input::Event]| {
        message(
            &input::Frame {
                dropped: 0,
                time: base + time,
                events: Seq::Of(events),
            },
            0,
        )
    };
    let (k, tab) = (t.s.keyboard, t.s.tablet);
    for &(time, code, value) in keyboard {
        t.s.f().send(k, frame(time, &[key(code, value)]), vec![]);
    }
    for &(time, value) in tablet {
        let events = [
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 0,
                value: axis(px, w),
            },
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 1,
                value: axis(py, h),
            },
            key(code::BTN_LEFT, value),
        ];
        t.s.f().send(tab, frame(time, &events), vec![]);
    }
    t.s.f().clock += late;
    for _ in 0..12 {
        t.s.c.step();
    }
    assert!(t.s.f().peers[&k].inbox.is_empty() && t.s.f().peers[&tab].inbox.is_empty());
    let out = t.s.f().take(t.a);
    let (at, press) = out
        .iter()
        .enumerate()
        .filter_map(|(i, m)| Some((i, decode::<gui::WindowPointerMods>(m, 0).ok()?.1)))
        .map(|(i, p)| (i, (p.buttons.0, p.modifiers.0)))
        .find(|(_, (b, _))| b & gui::buttons::LEFT != 0)
        .expect("the press is told");
    let first_key = out.iter().position(|m| decode::<gui::Key>(m, 0).is_ok());
    Heard {
        modifiers: press.1,
        keys: super::keys_of(&out),
        press_first: first_key.is_none_or(|k| at < k),
        odd: t.s.c.stats().odd_input_times,
    }
}

/// A Shift-click whose frames all wait when the round starts: the
/// keyboard's Shift down, the pointer's press and release, the keyboard's
/// Shift up. The press carries Shift in every case (served in the port's
/// order, the tablet first made the click a plain one: `files-manage:auto:2`,
/// step 22, CI run 44859568963, proposed ADR-0469; the keyboard read whole
/// first did too, before ADR-0467's rounds ended on a key).
#[test]
fn a_modifier_click_waiting_in_two_devices_keeps_its_order() {
    for case in CASES {
        let heard = two_devices(
            case,
            &[(1, code::LEFT_SHIFT, 1), (4, code::LEFT_SHIFT, 0)],
            &[(2, 1), (3, 0)],
            0,
        );
        assert_eq!(
            heard.modifiers & gui::modifiers::SHIFT,
            gui::modifiers::SHIFT,
            "the press carries Shift (port reversed, tablet first: {case:?})"
        );
        assert_eq!(heard.odd, 0, "honest times");
    }
}

/// evdev's A.
const KEY_A: u32 = 30;

/// A click, then Shift and a key, all waiting: the press carries no Shift
/// and comes before the key, in every case (the keyboard served first gave
/// the press the Shift that came after it).
#[test]
fn a_click_before_a_modifier_waiting_in_two_devices_keeps_its_order() {
    for case in CASES {
        let heard = two_devices(
            case,
            &[
                (3, code::LEFT_SHIFT, 1),
                (4, KEY_A, 1),
                (5, KEY_A, 0),
                (6, code::LEFT_SHIFT, 0),
            ],
            &[(1, 1), (2, 0)],
            0,
        );
        assert_eq!(
            heard.modifiers & gui::modifiers::SHIFT,
            0,
            "the press carries no Shift ({case:?})"
        );
        assert!(heard.press_first, "the press before the key ({case:?})");
        assert!(
            heard.keys.contains(&(KEY_A, true, Some('A'))),
            "the key after it has Shift: {:?}",
            heard.keys
        );
    }
}

/// The same two gestures read 3 s after they were made (a backlog longer
/// than `MAX_INPUT_LAG_NS`): the frames read together keep their times'
/// order (a bound on the times themselves made every head equal and the
/// slot decided: review of 5add3833, 1).
#[test]
fn frames_read_together_keep_their_order_at_any_backlog() {
    const MS: u64 = 1_000_000;
    for case in CASES {
        let shift_click = two_devices(
            case,
            &[(MS, code::LEFT_SHIFT, 1), (900 * MS, code::LEFT_SHIFT, 0)],
            &[(400 * MS, 1), (401 * MS, 0)],
            3_000 * MS,
        );
        assert_eq!(
            shift_click.modifiers & gui::modifiers::SHIFT,
            gui::modifiers::SHIFT,
            "Shift-click, {case:?}"
        );
        let click_then_shift = two_devices(
            case,
            &[
                (400 * MS, code::LEFT_SHIFT, 1),
                (500 * MS, KEY_A, 1),
                (600 * MS, KEY_A, 0),
                (700 * MS, code::LEFT_SHIFT, 0),
            ],
            &[(MS, 1), (2 * MS, 0)],
            3_000 * MS,
        );
        assert_eq!(
            click_then_shift.modifiers & gui::modifiers::SHIFT,
            0,
            "click then Shift, {case:?}"
        );
        assert!(click_then_shift.press_first, "{case:?}");
        assert_eq!(
            shift_click.odd + click_then_shift.odd,
            0,
            "honest, though late"
        );
    }
}

/// Of equal times the first slot's frame is told first, and a device's own
/// frames keep their channel's order whatever their times: a driver's times
/// only order the devices' heads.
#[test]
fn equal_and_backward_times_keep_each_devices_order() {
    // The keyboard's Shift up has a time before its Shift down: it is
    // still told after it, and before the press (time 2), which comes
    // after both.
    for case in CASES {
        let heard = two_devices(
            case,
            &[(1, code::LEFT_SHIFT, 1), (0, code::LEFT_SHIFT, 0)],
            &[(2, 1), (3, 0)],
            0,
        );
        assert_eq!(heard.modifiers & gui::modifiers::SHIFT, 0, "{case:?}");
        assert_eq!(heard.odd, 1, "the backward time is counted");
    }
    // Equal times: the device in slot 0 first, whichever the port serves.
    for case in CASES {
        let heard = two_devices(
            case,
            &[(2, code::LEFT_SHIFT, 1), (9, code::LEFT_SHIFT, 0)],
            &[(2, 1), (3, 0)],
            0,
        );
        let shift = if case.1 { 0 } else { gui::modifiers::SHIFT };
        assert_eq!(heard.modifiers & gui::modifiers::SHIFT, shift, "{case:?}");
    }
}

/// A time later than the compositor's clock is taken as when the frame was
/// read: a driver's future time cannot put its frame behind another
/// device's frame sent after it was read.
#[test]
fn a_future_time_is_taken_as_when_the_frame_was_read() {
    let mut t = super::drops::two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    let (w, h) = t.s.f().display_size;
    t.s.point(axis(px, w), axis(py, h), Some(1));
    t.s.point(axis(px, w), axis(py, h), Some(0));
    t.s.f().take(t.a);
    let (k, tab) = (t.s.keyboard, t.s.tablet);
    let frame = |time: u64, events: &[input::Event]| {
        message(
            &input::Frame {
                dropped: 0,
                time,
                events: Seq::Of(events),
            },
            0,
        )
    };
    let key = |code: u32, value: u32| input::Event {
        kind: Below(input::kind::KEY),
        code,
        value,
    };
    // A key, which ends the round, then Shift down from the future: read
    // as the keyboard's head and held.
    let now = t.s.f().clock;
    t.s.f().send(k, frame(now, &[key(KEY_A, 1)]), vec![]);
    t.s.f()
        .send(k, frame(u64::MAX, &[key(code::LEFT_SHIFT, 1)]), vec![]);
    t.s.c.step();
    assert_eq!(t.s.untold(k), 1, "Shift is the held head");
    // The press, sent after Shift was read.
    let later = t.s.f().clock + 1;
    let press = [
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 0,
            value: axis(px, w),
        },
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 1,
            value: axis(py, h),
        },
        key(code::BTN_LEFT, 1),
    ];
    t.s.f().send(tab, frame(later, &press), vec![]);
    for _ in 0..4 {
        t.s.c.step();
    }
    let p =
        t.s.f()
            .take(t.a)
            .iter()
            .filter_map(|m| decode::<gui::WindowPointerMods>(m, 0).ok())
            .map(|(_, p)| (p.buttons.0, p.modifiers.0))
            .find(|(b, _)| b & gui::buttons::LEFT != 0)
            .expect("the press is told");
    assert_eq!(p.1 & gui::modifiers::SHIFT, gui::modifiers::SHIFT);
    assert_eq!(t.s.c.stats().odd_input_times, 1, "the future time is odd");
}

/// A frame sent while a round runs, with a time before the head that ends
/// the round, is told before it (review of 5add3833, 5): the keyboard's
/// channel is empty when the round starts and its Shift, made before the
/// press, arrives while the tablet's moves are told.
#[test]
fn a_frame_sent_during_the_round_is_told_before_a_later_press() {
    let mut t = super::drops::two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    let (w, h) = t.s.f().display_size;
    t.s.point(axis(px, w), axis(py, h), Some(1));
    t.s.point(axis(px, w), axis(py, h), Some(0));
    t.s.f().take(t.a);
    let (k, tab) = (t.s.keyboard, t.s.tablet);
    let base = t.s.f().clock;
    let mv = |x: i64, button: Option<u32>, time: u64| {
        let mut events = vec![
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 0,
                value: axis(x, w),
            },
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 1,
                value: axis(py, h),
            },
        ];
        if let Some(v) = button {
            events.push(input::Event {
                kind: Below(input::kind::KEY),
                code: code::BTN_LEFT,
                value: v,
            });
        }
        message(
            &input::Frame {
                dropped: 0,
                time,
                events: Seq::Of(&events),
            },
            0,
        )
    };
    t.s.f().send(tab, mv(px + 1, None, base + 1), vec![]);
    t.s.f().send(tab, mv(px, Some(1), base + 5), vec![]);
    // The Shift arrives once the round has read the heads: the fake's
    // read of the keyboard's empty channel is when it lands.
    t.s.f().arrive_on_read = Some((
        k,
        message(
            &input::Frame {
                dropped: 0,
                time: base + 3,
                events: Seq::Of(&[input::Event {
                    kind: Below(input::kind::KEY),
                    code: code::LEFT_SHIFT,
                    value: 1,
                }]),
            },
            0,
        ),
    ));
    for _ in 0..4 {
        t.s.c.step();
    }
    let p =
        t.s.f()
            .take(t.a)
            .iter()
            .filter_map(|m| decode::<gui::WindowPointerMods>(m, 0).ok())
            .map(|(_, p)| (p.buttons.0, p.modifiers.0))
            .find(|(b, _)| b & gui::buttons::LEFT != 0)
            .expect("the press is told");
    assert_eq!(p.1 & gui::modifiers::SHIFT, gui::modifiers::SHIFT);
}

/// A head read and not told (the round's input ended on a key before it) is
/// told in the next round although its channel is empty, and that round
/// does not wait: the port would not wake for it.
#[test]
fn a_frame_read_and_not_told_is_told_next_round_without_a_wait() {
    let mut s = setup_with(|_| {});
    s.move_to(300, 200);
    let k = s.keyboard;
    let key = [input::Event {
        kind: Below(input::kind::KEY),
        code: KEY_A,
        value: 1,
    }];
    let time = s.f().time();
    let m = message(
        &input::Frame {
            dropped: 0,
            time,
            events: Seq::Of(&key),
        },
        0,
    );
    s.f().send(k, m, vec![]);
    s.queue_move(320, 210);
    s.c.step();
    let tab = s.tablet;
    assert_eq!(s.untold(tab), 1, "the move waits behind the key");
    assert!(s.f().peers[&tab].inbox.is_empty(), "read, as the head");
    let before = s.c.pointer;
    s.c.step();
    let (deadline, clock) = s.f().waited.unwrap();
    assert!(
        deadline.is_some_and(|d| d <= clock + 1000),
        "no wait: {deadline:?} at {clock}"
    );
    assert_eq!(s.untold(tab), 0);
    assert_ne!(s.c.pointer, before, "the move is told");
}

/// A driver whose times are all 0 (a stopped clock, or a hostile one) and
/// that floods holds another device's frame back by at most
/// `MAX_INPUT_LAG_NS`, not for as long as it floods (review of 8e418c42,
/// 1); its times are counted as odd.
#[test]
fn a_flood_with_early_times_holds_another_device_back_only_by_the_lag() {
    let mut s = setup_with(|_| {});
    s.move_to(300, 200);
    let (k, tab) = (s.keyboard, s.tablet);
    let time = s.f().clock;
    let key = [input::Event {
        kind: Below(input::kind::KEY),
        code: KEY_A,
        value: 1,
    }];
    let m = message(
        &input::Frame {
            dropped: 0,
            time,
            events: Seq::Of(&key),
        },
        0,
    );
    s.f().send(k, m, vec![]);
    let flood = |s: &mut Setup| {
        let events = [input::Event {
            kind: Below(input::kind::AXIS),
            code: 0,
            value: 100,
        }];
        let m = message(
            &input::Frame {
                dropped: 0,
                time: 0,
                events: Seq::Of(&events),
            },
            0,
        );
        for _ in 0..MAX_DRAIN + 1 {
            s.f().send(tab, m.clone(), vec![]);
        }
    };
    let mut rounds = 0;
    while s.untold(k) > 0 {
        assert!(rounds < 60, "the key is still held after {rounds} rounds");
        flood(&mut s);
        // 100 ms a round: the key is told once the lag has passed.
        s.f().clock += 100_000_000;
        s.c.step();
        rounds += 1;
    }
    assert!(
        (15..=30).contains(&rounds),
        "told after the lag of 2 s: {rounds} rounds"
    );
    let odd = s.c.stats().odd_input_times;
    assert!(
        odd >= MAX_DRAIN as u64 * (rounds - 1),
        "{odd} odd times in {rounds} rounds"
    );
}

/// A pointer move before a key is told to the window under it before the
/// key, with the modifiers before it: a move, then Shift down, both
/// waiting (review of 8e418c42, 2).
#[test]
fn a_move_before_a_key_is_told_before_it_with_the_modifiers_before_it() {
    let mut t = super::drops::two();
    let (px, py) = (t.oa.x + 10, t.oa.y + 60);
    let (w, h) = t.s.f().display_size;
    t.s.point(axis(px, w), axis(py, h), Some(1));
    t.s.point(axis(px, w), axis(py, h), Some(0));
    t.s.f().take(t.a);
    let base = t.s.f().clock;
    let (k, tab) = (t.s.keyboard, t.s.tablet);
    t.s.f().inputs_reversed = false;
    let moved = [
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 0,
            value: axis(px + 5, w),
        },
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 1,
            value: axis(py + 5, h),
        },
    ];
    let shift = [input::Event {
        kind: Below(input::kind::KEY),
        code: code::LEFT_SHIFT,
        value: 1,
    }];
    let a = [input::Event {
        kind: Below(input::kind::KEY),
        code: KEY_A,
        value: 1,
    }];
    let frame = |time: u64, events: &[input::Event]| {
        message(
            &input::Frame {
                dropped: 0,
                time: base + time,
                events: Seq::Of(events),
            },
            0,
        )
    };
    t.s.f().send(tab, frame(1, &moved), vec![]);
    t.s.f().send(k, frame(2, &shift), vec![]);
    t.s.f().send(k, frame(3, &a), vec![]);
    for _ in 0..4 {
        t.s.c.step();
    }
    let out = t.s.f().take(t.a);
    let pointer = out
        .iter()
        .position(|m| decode::<gui::WindowPointerMods>(m, 0).is_ok())
        .expect("the move is told");
    let key = out
        .iter()
        .position(|m| decode::<gui::Key>(m, 0).is_ok())
        .expect("the key is told");
    assert!(pointer < key, "the move first: {pointer} {key}");
    let (_, p) = decode::<gui::WindowPointerMods>(&out[pointer], 0).unwrap();
    assert_eq!(p.modifiers.0 & gui::modifiers::SHIFT, 0, "no Shift yet");
}

/// The buttons each client hears in `WindowPointerMods` (taken).
fn held_heard(s: &mut Setup, end: u32) -> Vec<(u32, u32)> {
    s.f()
        .take(end)
        .iter()
        .filter_map(|m| decode::<gui::WindowPointerMods>(m, 0).ok())
        .map(|(_, p)| (p.window, p.buttons.0))
        .collect()
}

/// A press on the upper window's minimise button, over the lower window's
/// body: the press names nobody, so the lower window, uncovered by it,
/// never hears the button held (review of e5d1c3c3, 2).
#[test]
fn a_press_that_minimises_a_window_is_held_in_none_it_uncovers() {
    let mut t = super::drops::two();
    let (mx, my) = (t.ob.x + 42, t.ob.y + 18);
    assert!(t.oa.contains(mx, my) && !t.oa.contains(t.ob.right() - 2, t.ob.bottom() - 2));
    let (w, h) = t.s.f().display_size;
    t.s.point(axis(mx, w), axis(my, h), Some(1));
    let ib = t.s.c.stack().find(|k| k.client == 1).unwrap();
    assert!(t.s.c.window(ib).unwrap().hidden, "the press minimised it");
    t.s.f().take(t.a);
    t.s.move_to(mx + 6, my + 6);
    let a = held_heard(&mut t.s, t.a);
    assert!(!a.is_empty() && a.iter().all(|p| p.1 == 0), "{a:?}");
    assert_eq!(t.s.c.pressed_in, [None; 3]);
}

/// A client that ends with its press held leaves no press behind for the
/// next client in its slot (review of e5d1c3c3, 3).
#[test]
fn a_client_that_ends_holding_a_press_leaves_none() {
    let mut t = super::drops::two();
    let (bx, by) = (t.ob.right() - 15, t.ob.bottom() - 15);
    let (w, h) = t.s.f().display_size;
    t.s.point(axis(bx, w), axis(by, h), Some(1));
    assert_eq!(t.s.c.pressed_in[0], Some(1));
    t.s.f().peers.get_mut(&t.b).unwrap().gone = true;
    t.s.c.step();
    assert_eq!(t.s.c.pressed_in, [None; 3]);
}

/// Each button is held in the client it was pressed over: the left in A,
/// the right in B; once the left is let go, A hears nothing held, though
/// the right still is (review of e5d1c3c3, 4).
#[test]
fn each_button_is_held_only_in_the_client_it_was_pressed_over() {
    let mut t = super::drops::two();
    let (w, h) = t.s.f().display_size;
    let a_at = (t.oa.x + 10, t.oa.y + 60);
    let b_at = (t.ob.right() - 15, t.ob.bottom() - 15);
    assert!(!t.oa.contains(b_at.0, b_at.1));
    let button = |s: &mut Setup, at: (i64, i64), code: u32, value: u32| {
        let events = [
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 0,
                value: axis(at.0, w),
            },
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 1,
                value: axis(at.1, h),
            },
            input::Event {
                kind: Below(input::kind::KEY),
                code,
                value,
            },
        ];
        let tab = s.tablet;
        s.f().send(
            tab,
            message(
                &input::Frame {
                    dropped: 0,
                    time: 0,
                    events: Seq::Of(&events),
                },
                0,
            ),
            vec![],
        );
        s.c.step();
    };
    button(&mut t.s, a_at, code::BTN_LEFT, 1);
    t.s.move_to(b_at.0, b_at.1);
    button(&mut t.s, b_at, code::BTN_RIGHT, 1);
    let b = held_heard(&mut t.s, t.b);
    assert!(
        b.iter().any(|p| p.1 == gui::buttons::RIGHT),
        "B hears its right: {b:?}"
    );
    assert!(
        b.iter().all(|p| p.1 & gui::buttons::LEFT == 0),
        "never A's left: {b:?}"
    );
    button(&mut t.s, b_at, code::BTN_LEFT, 0);
    t.s.f().take(t.a);
    t.s.move_to(a_at.0 + 4, a_at.1 + 4);
    let a = held_heard(&mut t.s, t.a);
    assert!(
        !a.is_empty() && a.iter().all(|p| p.1 == 0),
        "A hears no right: {a:?}"
    );
}

/// The pressing client's own popup, under the pointer while the press is
/// held, hears the button held: the mask is the client's, not the
/// window's (mutation M4 of the review of e5d1c3c3).
#[test]
fn the_pressing_clients_popup_hears_its_press_held() {
    let mut t = super::drops::two();
    let (w, h) = t.s.f().display_size;
    let a_at = (t.oa.x + 10, t.oa.y + 60);
    // The press in the window (which focuses it), held; the window opens
    // a popup on its body, as a menu opens on a press.
    t.s.point(axis(a_at.0, w), axis(a_at.1, h), Some(1));
    let m = t.s.f().memory(vec![7; 20 * 10], 800, false);
    let out = t.s.request(
        t.a,
        &gui::CreatePopup {
            parent: t.ida,
            x: 60,
            y: 80,
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
    let pb = t.s.body(0, p.window);
    assert!(!pb.contains(a_at.0, a_at.1));
    t.s.f().take(t.a);
    t.s.move_to(pb.x + 5, pb.y + 5);
    let a = held_heard(&mut t.s, t.a);
    assert!(
        a.iter()
            .any(|q| q.0 == p.window && q.1 & gui::buttons::LEFT != 0),
        "{a:?}"
    );
}

/// A wheel's message ends the device's round like a button's: the move
/// behind it waits for the next round (mutation M5 of the review).
#[test]
fn a_wheel_message_ends_the_devices_round() {
    let mut s = setup_with(|_| {});
    s.move_to(300, 200);
    let tab = s.tablet;
    let wheel = [input::Event {
        kind: Below(input::kind::WHEEL),
        code: 0,
        value: 120,
    }];
    let m = message(
        &input::Frame {
            dropped: 0,
            time: s.f().time(),
            events: Seq::Of(&wheel),
        },
        0,
    );
    s.f().send(tab, m, vec![]);
    s.queue_move(320, 210);
    s.c.step();
    assert_eq!(s.untold(tab), 1, "the move waits");
    s.c.step();
    assert_eq!(s.untold(tab), 0);
}

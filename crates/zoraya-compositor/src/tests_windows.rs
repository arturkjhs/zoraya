//! Stage 7b: windows, frames, the person's gestures, the system chords,
//! the switcher and the scale (ADR-0160, ADR-0164), against the fake of
//! `tests.rs`.

use super::*;

/// A pointer axis value for a pixel of a `side`-pixel display.
pub(super) fn axis(p: i64, side: u32) -> u32 {
    let d = i64::from(side - 1);
    ((p * 65535 + d - 1) / d) as u32
}

impl Setup {
    /// A toplevel of `w × h` logical pixels filled with `colour`, from a
    /// new client; gives the client's end, its memory and the window id.
    fn toplevel(&mut self, w: u32, h: u32, colour: u32) -> (u32, u32, u32) {
        let end = self.client(64);
        let (memory, id) = self.toplevel_of(end, w, h, colour);
        (end, memory, id)
    }

    fn toplevel_of(&mut self, end: u32, w: u32, h: u32, colour: u32) -> (u32, u32) {
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
            .expect("a Created reply");
        assert_eq!(created.status.0, status::OK);
        let id = created.window;
        assert!(self.commit_to(end, id, 0, 0, w * s, h * s) == status::OK);
        (memory, id)
    }

    fn commit_to(&mut self, end: u32, window: u32, x: u32, y: u32, w: u32, h: u32) -> u32 {
        let m = gui::CommitTo {
            window,
            x,
            y,
            width: w,
            height: h,
        };
        self.f().send(end, message(&m, 7), vec![]);
        self.c.step();
        let out = self.f().take(end);
        out.iter()
            .find_map(|r| decode::<gui::CommitDone>(r, 0).ok())
            .map(|(_, d)| d.status.0)
            .expect("a CommitDone")
    }

    pub(super) fn request<'a, M: Message<'a>>(
        &mut self,
        end: u32,
        m: &M,
        handles: Vec<u32>,
    ) -> Vec<Vec<u8>> {
        self.f().send(end, message(m, 11), handles);
        self.c.step();
        self.f().take(end)
    }

    /// Moves the pointer to `(x, y)` physical, with the left button's
    /// change if any.
    pub(super) fn at(&mut self, x: i64, y: i64, button: Option<u32>) {
        let (w, h) = self.c.size();
        self.point(axis(x, w), axis(y, h), button);
    }

    pub(super) fn body(&self, client: usize, id: u32) -> Rect {
        self.c.window(Key { client, id }).unwrap().body_rect()
    }

    pub(super) fn outer(&self, client: usize, id: u32) -> Rect {
        let f = self.c.frame();
        self.c.window(Key { client, id }).unwrap().outer(&f)
    }

    fn region(&mut self, r: Rect) -> Vec<u32> {
        let mut v = Vec::new();
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                v.push(self.pixel(x, y));
            }
        }
        v
    }
}

pub(super) fn configures(out: &[Vec<u8>]) -> Vec<gui::Configure> {
    out.iter()
        .filter_map(|m| decode::<gui::Configure>(m, 0).ok())
        .map(|(_, c)| c)
        .collect()
}

fn ordinals(out: &[Vec<u8>]) -> Vec<u32> {
    out.iter()
        .map(|m| Header::decode(m).unwrap().0.ordinal)
        .collect()
}

#[test]
fn a_toplevel_is_configured_framed_and_shown() {
    let mut s = setup();
    let end = s.client(64);
    let out = s.request(end, &gui::DisplayScale {}, vec![]);
    let (_, d) = decode::<gui::DisplayScaleReply>(&out[0], 0).unwrap();
    assert_eq!(
        (d.status.0, d.width, d.height, d.scale),
        (status::OK, W, H, 1)
    );
    let n = 120 * 80;
    let memory = s.f().memory(vec![0x0011_2233; n], n as u64 * 4, false);
    let out = s.request(
        end,
        &gui::CreateToplevel {
            width: 120,
            height: 80,
        },
        vec![memory],
    );
    let (_, created) = out
        .iter()
        .find_map(|m| decode::<gui::Created>(m, 0).ok())
        .unwrap();
    assert_eq!((created.status.0, created.window), (status::OK, 1));
    let c = configures(&out);
    assert_eq!(c.len(), 1);
    assert_eq!(
        (c[0].window, c[0].width, c[0].height, c[0].scale),
        (1, 120, 80, 1)
    );
    // The light theme, and motion, by the declarations' defaults (ADR-0169).
    assert!(!c[0].dark && c[0].motion);
    assert!(ordinals(&out).contains(&22), "the first window takes focus");
    assert_eq!(s.body(0, 1), Rect::new(BX, BY, 120, 80));
    assert_eq!(s.commit_to(end, 1, 0, 0, 120, 80), status::OK);
    assert_eq!(s.pixel(BX + 60, BY + 40), 0x0011_2233);
    // A second toplevel of the same client: id 2, cascaded.
    let n2 = 10 * 10;
    let m2 = s.f().memory(vec![1; n2], 400, false);
    let out = s.request(
        end,
        &gui::CreateToplevel {
            width: 10,
            height: 10,
        },
        vec![m2],
    );
    let (_, created) = out
        .iter()
        .find_map(|m| decode::<gui::Created>(m, 0).ok())
        .unwrap();
    assert_eq!(created.window, 2);
    assert_eq!(s.body(0, 2), Rect::new(BX + 32, BY + 32, 10, 10));
    // A commit to a window that is not the client's, or to none.
    assert_eq!(s.commit_to(end, 9, 0, 0, 1, 1), status::NO_SURFACE);
    assert_eq!(s.commit_to(end, 0, 0, 0, 1, 1), status::NO_SURFACE);
}

/// INV-0160-1: the frame is the compositor's drawing, whatever the client
/// commits.
#[test]
fn the_frame_does_not_depend_on_the_clients_pixels() {
    let mut scenes = Vec::new();
    for colour in [0x00ff_ffff, 0x0000_0000] {
        let mut s = setup();
        s.toplevel(200, 100, colour);
        let outer = s.outer(0, 1);
        let body = s.body(0, 1);
        let frame: Vec<u32> = (outer.y..outer.bottom())
            .flat_map(|y| (outer.x..outer.right()).map(move |x| (x, y)))
            .filter(|(x, y)| !body.contains(*x, *y))
            .map(|(x, y)| s.pixel(x, y))
            .collect();
        assert!(
            frame.iter().any(|p| *p != colour),
            "the frame is not the client's colour"
        );
        scenes.push(frame);
    }
    assert_eq!(scenes[0], scenes[1]);
}

/// INV-0160-2: nothing an unfocused client sends moves, raises or focuses
/// a window, or decorates one.
#[test]
fn an_unfocused_client_changes_no_position_order_or_focus() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 60, 1);
    let (b, _, _) = s.toplevel(100, 60, 2);
    let _ = a;
    // a's window has focus (the first); b is unfocused, above it.
    let focused = s.c.focused();
    assert_eq!(focused, Some(Key { client: 0, id: 1 }));
    let order: Vec<Key> = s.c.stack().collect();
    let places = (s.body(0, 1), s.body(1, 1));
    let n = 40 * 40;
    let mem = |s: &mut Setup| s.f().memory(vec![3; n], n as u64 * 4, false);
    let m1 = mem(&mut s);
    let m2 = mem(&mut s);
    let m3 = mem(&mut s);
    s.request(b, &gui::Display {}, vec![]);
    s.request(b, &gui::DisplayScale {}, vec![]);
    s.request(
        b,
        &gui::CommitTo {
            window: 1,
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        },
        vec![],
    );
    s.request(
        b,
        &gui::SetTitle {
            window: 1,
            text: zoraya_protocol::Data(b"I am on top"),
        },
        vec![],
    );
    s.request(
        b,
        &gui::SetBuffer {
            window: 1,
            width: 40,
            height: 40,
        },
        vec![m1],
    );
    s.request(
        b,
        &gui::CreatePopup {
            parent: 1,
            x: 0,
            y: 0,
            width: 40,
            height: 40,
        },
        vec![m2],
    );
    s.request(
        b,
        &gui::Create {
            width: 40,
            height: 40,
        },
        vec![m3],
    );
    s.request(
        b,
        &gui::Commit {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        vec![],
    );
    // The manager's ordinals, from a client, are unknown to it.
    s.request(b, &gui::Stats {}, vec![]);
    s.request(b, &gui::AddClient {}, vec![]);
    s.request(b, &gui::AttachInput {}, vec![]);
    assert_eq!(s.c.focused(), focused);
    let toplevels: Vec<Key> = s.c.stack().filter(|k| k.id == 1).collect();
    assert_eq!(
        toplevels,
        order
            .iter()
            .copied()
            .filter(|k| k.id == 1)
            .collect::<Vec<_>>()
    );
    assert_eq!((s.body(0, 1), s.body(1, 1)), places);
}

/// INV-0160-3: the system key's chords, Alt+Tab and F12 reach no client,
/// and the switcher reaches every window, hidden ones too.
#[test]
fn the_chords_are_the_compositors_and_the_switcher_reaches_every_window() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 60, 1);
    let (b, _, _) = s.toplevel(100, 60, 2);
    let (c, _, _) = s.toplevel(100, 60, 3);
    for end in [a, b, c] {
        s.f().take(end);
    }
    // c is hidden (System+M on it, once focused).
    s.c.step();
    let meta = code::LEFT_META;
    for k in [code::SPACE, code::R, code::SLASH, 28] {
        s.keys(&[(meta, 1), (k, 1), (k, 0), (meta, 0)]);
    }
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    for end in [a, b, c] {
        let got = keys_of(&s.f().take(end));
        assert!(got.is_empty(), "keys reached a client: {got:?}");
    }
    // A chord the compositor does not take reaches the focused client
    // alone, with `SYSTEM` and no text (ADR-0248, INV-0248-1).
    let focused = s.c.focused().unwrap();
    for k in [30, code::TAB, code::UP] {
        s.keys(&[(meta, 1), (k, 1), (k, 0), (meta, 0)]);
    }
    for (i, end) in [a, b, c].into_iter().enumerate() {
        let got: Vec<gui::Key> = s
            .f()
            .take(end)
            .iter()
            .filter_map(|m| decode::<gui::Key>(m, 0).ok())
            .map(|(_, k)| k)
            .collect();
        if i == focused.client {
            let pressed: Vec<u32> = got.iter().filter(|k| k.pressed).map(|k| k.code).collect();
            assert_eq!(pressed, [30, code::TAB, code::UP]);
            assert!(
                got.iter()
                    .filter(|k| k.pressed)
                    .all(|k| { k.modifiers.0 & gui::modifiers::SYSTEM != 0 && k.text.0 == 0 })
            );
            assert_eq!(
                got.iter().filter(|k| !k.pressed).count(),
                3,
                "and their releases"
            );
        } else {
            assert!(got.is_empty(), "{i}: {}", got.len());
        }
    }
    // Every key of the compositor's table is still its own.
    for k in TAKEN_CHORDS {
        if [
            code::W,
            code::M,
            code::LEFT,
            code::RIGHT,
            code::ENTER,
            code::SLASH,
            code::SPACE,
        ]
        .contains(&k)
        {
            continue;
        }
        s.keys(&[(meta, 1), (k, 1), (k, 0), (meta, 0)]);
        for end in [a, b, c] {
            assert!(keys_of(&s.f().take(end)).is_empty(), "{k}");
        }
    }
    let target = s.c.focused().unwrap();
    s.keys(&[(meta, 1), (code::M, 1), (code::M, 0), (meta, 0)]);
    assert!(s.c.window(target).unwrap().hidden);
    // Alt+Tab lists every window, the hidden one last; Tab steps to it,
    // and releasing Alt shows and focuses it.
    s.keys(&[(code::LEFT_ALT, 1), (code::TAB, 1), (code::TAB, 0)]);
    let list = s.c.switcher().unwrap().windows.clone();
    assert_eq!(list.len(), 3);
    assert_eq!(list.last(), Some(&target), "the hidden window is listed");
    while s.c.switcher().unwrap().chosen != 2 {
        s.keys(&[(code::TAB, 1), (code::TAB, 0)]);
    }
    s.keys(&[(code::LEFT_ALT, 0)]);
    assert!(s.c.switcher().is_none());
    assert_eq!(s.c.focused(), Some(target));
    assert!(!s.c.window(target).unwrap().hidden);
    // Shift+Tab steps back.
    s.keys(&[(code::LEFT_ALT, 1), (code::TAB, 1), (code::TAB, 0)]);
    assert_eq!(s.c.switcher().unwrap().chosen, 1);
    s.keys(&[
        (code::LEFT_SHIFT, 1),
        (code::TAB, 1),
        (code::TAB, 0),
        (code::LEFT_SHIFT, 0),
    ]);
    assert_eq!(s.c.switcher().unwrap().chosen, 0);
    s.keys(&[(code::LEFT_ALT, 0)]);
    assert_eq!(s.c.focused(), Some(target));
    for end in [a, b, c] {
        let got: Vec<u32> = keys_of(&s.f().take(end)).iter().map(|k| k.0).collect();
        assert!(
            !got.contains(&code::TAB),
            "Tab under Alt reached a client: {got:?}"
        );
    }
    // Escape leaves the switcher with focus where it was.
    let before = s.c.focused();
    s.keys(&[
        (code::LEFT_ALT, 1),
        (code::TAB, 1),
        (code::TAB, 0),
        (code::ESC, 1),
        (code::ESC, 0),
        (code::LEFT_ALT, 0),
    ]);
    assert!(s.c.switcher().is_none());
    assert_eq!(s.c.focused(), before);
}

#[test]
fn system_w_asks_to_close_and_left_and_right_tile() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 60, 1);
    s.f().take(a);
    let meta = code::RIGHT_META;
    s.keys(&[(meta, 1), (code::W, 1), (code::W, 0), (meta, 0)]);
    let out = s.f().take(a);
    assert!(
        out.iter()
            .any(|m| decode::<gui::CloseRequested>(m, 0).is_ok_and(|(_, c)| c.window == 1))
    );
    s.keys(&[(meta, 1), (code::LEFT, 1), (code::LEFT, 0), (meta, 0)]);
    let c = configures(&s.f().take(a));
    // Half of 640 less the border each side; the work area's height (the
    // screen less the bar and the dock's strip, 76) less the title bar and
    // the bottom border.
    assert_eq!((c[0].width, c[0].height), (318, 480 - 28 - 76 - 36 - 1));
    assert_eq!(s.body(0, 1).x, 1);
    s.keys(&[(meta, 1), (code::RIGHT, 1), (code::RIGHT, 0), (meta, 0)]);
    assert_eq!(s.body(0, 1).x, 321);
}

/// Mutants 7d: at scale 2 a tiled window is half the logical width less
/// its two borders.
#[test]
fn tiling_at_scale_two() {
    let mut s = setup_with(|f| f.display_size = (2048, 1536));
    let (a, _, _) = s.toplevel(100, 60, 1);
    s.f().take(a);
    let meta = code::RIGHT_META;
    s.keys(&[(meta, 1), (code::LEFT, 1), (code::LEFT, 0), (meta, 0)]);
    let c = configures(&s.f().take(a));
    assert_eq!(c[0].width, 1024 / 2 - 2);
    assert_eq!(s.body(0, 1).width, 1020);
}

/// The title bar moves the window; the buttons minimise and close; a
/// grip resizes, with `Configure` only at the end (ADR-0160).
#[test]
fn the_person_moves_minimises_resizes_and_closes() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(200, 100, 5);
    s.f().take(a);
    let body = s.body(0, 1);
    // Drag the title bar by (40, 30).
    let (tx, ty) = (body.x + 100, body.y - 18);
    s.at(tx, ty, Some(1));
    s.at(tx + 40, ty + 30, None);
    s.at(tx + 40, ty + 30, Some(0));
    assert_eq!(s.body(0, 1), Rect::new(body.x + 40, body.y + 30, 200, 100));
    assert!(
        configures(&s.f().take(a)).is_empty(),
        "a move is not configured"
    );
    // Resize from the bottom right corner by (50, 20).
    let o = s.outer(0, 1);
    s.at(o.right() - 2, o.bottom() - 2, Some(1));
    s.at(o.right() + 48, o.bottom() + 18, None);
    assert!(
        configures(&s.f().take(a)).is_empty(),
        "nothing until the release"
    );
    s.at(o.right() + 48, o.bottom() + 18, Some(0));
    let c = configures(&s.f().take(a));
    assert_eq!((c[0].width, c[0].height), (250, 120));
    // Minimise: the dash, 42 logical pixels from the window's left.
    let o = s.outer(0, 1);
    s.at(o.x + 42, o.y + 18, Some(1));
    s.at(o.x + 42, o.y + 18, Some(0));
    assert!(s.c.window(Key { client: 0, id: 1 }).unwrap().hidden);
    let out = s.f().take(a);
    assert!(
        out.iter()
            .any(|m| decode::<gui::Minimised>(m, 0).is_ok_and(|(_, m)| m.hidden))
    );
    assert_eq!(s.c.focused(), None);
    // F12 brings it back; the cross asks to close.
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    assert!(!s.c.window(Key { client: 0, id: 1 }).unwrap().hidden);
    s.at(o.x + 18, o.y + 18, Some(1));
    s.at(o.x + 18, o.y + 18, Some(0));
    let out = s.f().take(a);
    assert!(
        out.iter()
            .any(|m| decode::<gui::CloseRequested>(m, 0).is_ok())
    );
    // The client closes it.
    let out = s.request(a, &gui::Close { window: 1 }, vec![]);
    let (_, done) = decode::<gui::CloseDone>(&out[0], 0).unwrap();
    assert_eq!(done.status.0, status::OK);
    assert!(s.c.window(Key { client: 0, id: 1 }).is_none());
    assert_eq!(
        s.c.client_count(),
        1,
        "closing its last window does not end a client"
    );
}

/// INV-0160-4: a client that reads nothing loses its events only, and its
/// window still moves, hides and closes.
#[test]
fn a_deaf_client_is_still_moved_hidden_and_closed() {
    let mut s = setup();
    let end = s.client(64);
    s.toplevel_of(end, 120, 60, 9);
    s.f().peers.get_mut(&end).unwrap().room = 0;
    let body = s.body(0, 1);
    s.at(body.x + 60, body.y - 18, Some(1));
    s.at(body.x + 80, body.y - 8, None);
    s.at(body.x + 80, body.y - 8, Some(0));
    assert_eq!(s.body(0, 1).x, body.x + 20);
    let o = s.outer(0, 1);
    // Close: asked, ignored; five seconds later, a second press ends it.
    s.at(o.x + 18, o.y + 18, Some(1));
    s.at(o.x + 18, o.y + 18, Some(0));
    assert_eq!(s.c.client_count(), 1);
    s.f().clock += CLOSE_GRACE_NS;
    s.at(o.x + 18, o.y + 18, Some(1));
    assert_eq!(s.c.client_count(), 0);
}

/// INV-0160-5: a client draws at a size `Configure` gave, and the old
/// pixels stay until it commits at that size.
#[test]
fn old_pixels_stay_until_the_client_commits_at_the_new_size() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 50, 0x0000_aaaa);
    s.f().take(a);
    let o = s.outer(0, 1);
    s.at(o.right() - 2, o.bottom() - 2, Some(1));
    s.at(o.right() + 38, o.bottom() + 28, Some(0));
    let c = configures(&s.f().take(a));
    assert_eq!((c[0].width, c[0].height), (140, 80));
    let body = s.body(0, 1);
    // The old pixels where the old buffer reaches; the paper beyond.
    assert_eq!(s.pixel(body.x + 50, body.y + 25), 0x0000_aaaa);
    assert_eq!(
        s.pixel(body.x + 120, body.y + 25),
        zoraya_tokens::LIGHT.bg_window.pixel()
    );
    // A buffer of another size is refused.
    let wrong = s.f().memory(vec![0; 100 * 50], 100 * 50 * 4, false);
    let out = s.request(
        a,
        &gui::SetBuffer {
            window: 1,
            width: 100,
            height: 50,
        },
        vec![wrong],
    );
    assert_eq!(
        decode::<gui::BufferDone>(&out[0], 0).unwrap().1.status.0,
        status::BAD_BUFFER
    );
    let n = 140 * 80;
    let right = s.f().memory(vec![0x00bb_0000; n], n as u64 * 4, false);
    let out = s.request(
        a,
        &gui::SetBuffer {
            window: 1,
            width: 140,
            height: 80,
        },
        vec![right],
    );
    assert_eq!(
        decode::<gui::BufferDone>(&out[0], 0).unwrap().1.status.0,
        status::OK
    );
    assert_eq!(
        s.pixel(body.x + 50, body.y + 25),
        0x0000_aaaa,
        "still the old pixels"
    );
    assert_eq!(s.commit_to(a, 1, 0, 0, 140, 80), status::OK);
    assert_eq!(s.pixel(body.x + 50, body.y + 25), 0x00bb_0000);
    assert_eq!(s.pixel(body.x + 120, body.y + 25), 0x00bb_0000);
}

/// INV-0160-6: a title is UTF-8 of at most 256 bytes, drawn only inside
/// the title bar.
#[test]
fn hostile_titles_stay_in_the_title_bar() {
    let long = "W".repeat(256);
    let titles: [&[u8]; 5] = [
        long.as_bytes(),
        b"\x07\x1b[31mred\x00\x7f",
        "\u{202e}gnp.exe\u{202c} — שלום".as_bytes(),
        "e\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}".as_bytes(),
        "Жжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжжж".as_bytes(),
    ];
    let mut base = setup();
    base.toplevel(160, 60, 4);
    let outer = base.outer(0, 1);
    let bar = Rect::new(outer.x, outer.y, outer.width, 36);
    let screen = Rect::new(0, 0, W.into(), H.into());
    let reference = base.region(screen);
    for t in titles {
        let mut s = setup();
        let (a, _, _) = s.toplevel(160, 60, 4);
        let out = s.request(
            a,
            &gui::SetTitle {
                window: 1,
                text: zoraya_protocol::Data(t),
            },
            vec![],
        );
        assert_eq!(
            decode::<gui::TitleDone>(&out[0], 0).unwrap().1.status.0,
            status::OK
        );
        let now = s.region(screen);
        for (i, (p, q)) in now.iter().zip(&reference).enumerate() {
            let (x, y) = (i as i64 % i64::from(W), i as i64 / i64::from(W));
            if p != q {
                assert!(
                    bar.contains(x, y),
                    "a title changed ({x}, {y}) outside the bar"
                );
            }
        }
    }
    let mut s = setup();
    let (a, _, _) = s.toplevel(160, 60, 4);
    let out = s.request(
        a,
        &gui::SetTitle {
            window: 1,
            text: zoraya_protocol::Data(b"\xff\xfe"),
        },
        vec![],
    );
    assert_eq!(
        decode::<gui::TitleDone>(&out[0], 0).unwrap().1.status.0,
        status::BAD_RANGE
    );
}

/// INV-0160-7: no client ordinal paints the bar or puts a window over it.
#[test]
fn no_client_reaches_the_bar() {
    let mut s = setup();
    s.c.step();
    let bar = Rect::new(0, 0, W.into(), 28);
    let before = s.region(bar);
    let (a, _, _) = s.toplevel(200, 100, 0x00ff_00ff);
    let n = 200 * 100;
    let m = s.f().memory(vec![0x00ff_00ff; n], n as u64 * 4, false);
    s.request(
        a,
        &gui::CreatePopup {
            parent: 1,
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        },
        vec![m],
    );
    s.request(
        a,
        &gui::CommitTo {
            window: 2,
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        },
        vec![],
    );
    // Dragging the title bar far up leaves it under the bar.
    let body = s.body(0, 1);
    s.at(body.x + 100, body.y - 18, Some(1));
    s.at(body.x + 100, 0, None);
    s.at(body.x + 100, 0, Some(0));
    assert!(s.outer(0, 1).y >= 28);
    // Whatever else is on the screen, the bar is as it was but for the
    // pointer, which is the compositor's own.
    s.at(i64::from(W) - 1, i64::from(H) - 1, None);
    let after = s.region(bar);
    let popup = s.c.window(Key { client: 0, id: 2 });
    assert!(popup.is_none() || popup.unwrap().body_rect().y >= 0);
    let differ = before.iter().zip(&after).filter(|(p, q)| p != q).count();
    assert_eq!(differ, 0, "the bar changed");
}

#[test]
fn popups_sit_on_their_parent_and_close_on_a_press_outside() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(200, 120, 1);
    let mut ids = Vec::new();
    for i in 0..5u32 {
        let m = s.f().memory(vec![7; 20 * 10], 800, false);
        let out = s.request(
            a,
            &gui::CreatePopup {
                parent: 1,
                x: 10 + i,
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
        ids.push((p.status.0, p.window));
    }
    assert!(ids[..4].iter().all(|(s, _)| *s == status::OK));
    assert_eq!(ids[4].0, status::NO_MEMORY, "at most four popups");
    let body = s.body(0, 1);
    assert_eq!(
        s.body(0, ids[0].1),
        Rect::new(body.x + 10, body.y + 20, 20, 10)
    );
    // A popup of a window that is not a toplevel of its own is refused.
    let m = s.f().memory(vec![7; 100], 400, false);
    let out = s.request(
        a,
        &gui::CreatePopup {
            parent: 9,
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        },
        vec![m],
    );
    assert_eq!(
        decode::<gui::PopupCreated>(&out[0], 0).unwrap().1.status.0,
        status::NO_SURFACE
    );
    // They stack above their parent; a press on one keeps them.
    let stack: Vec<u32> = s.c.stack().map(|k| k.id).collect();
    assert_eq!(stack, [1, ids[0].1, ids[1].1, ids[2].1, ids[3].1]);
    s.at(body.x + 15, body.y + 25, Some(1));
    s.at(body.x + 15, body.y + 25, Some(0));
    assert_eq!(s.c.stack().count(), 5);
    s.f().take(a);
    // A press outside closes all four, and the client is told.
    s.at(5, i64::from(H) - 5, Some(1));
    let out = s.f().take(a);
    let closed = out
        .iter()
        .filter(|m| decode::<gui::CloseRequested>(m, 0).is_ok())
        .count();
    assert_eq!(closed, 4);
    assert!(s.c.stack().all(|k| k.id == 1));
}

#[test]
fn pointer_events_to_a_toplevel_are_logical_and_per_window() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 60, 1);
    s.f().take(a);
    let body = s.body(0, 1);
    s.at(body.x + 7, body.y + 9, None);
    let out = s.f().take(a);
    let p: Vec<(u32, u32, u32)> = out
        .iter()
        .filter_map(|m| decode::<gui::WindowPointerMods>(m, 0).ok())
        .map(|(_, p)| (p.window, p.x, p.y))
        .collect();
    assert_eq!(p, [(1, 7, 9)]);
    // Over the frame, nothing goes to the client.
    s.at(body.x + 50, body.y - 10, None);
    assert!(
        s.f()
            .take(a)
            .iter()
            .all(|m| decode::<gui::WindowPointerMods>(m, 0).is_err())
    );
}

/// INV-0164-1 and INV-0164-3 at scale 2.
#[test]
fn at_scale_two_sizes_are_logical_and_pixels_unchanged() {
    let mut s = setup_with(|f| f.display_size = (2048, 1536));
    assert_eq!(s.c.scale(), zoraya_tokens::Scale::TWO);
    let end = s.client(64);
    let out = s.request(end, &gui::DisplayScale {}, vec![]);
    let (_, d) = decode::<gui::DisplayScaleReply>(&out[0], 0).unwrap();
    assert_eq!((d.width, d.height, d.scale), (1024, 768, 2));
    // A buffer of the logical size only, not times the scale: refused.
    let small = s.f().memory(vec![0; 100 * 50], 100 * 50 * 4, false);
    let out = s.request(
        end,
        &gui::CreateToplevel {
            width: 100,
            height: 50,
        },
        vec![small],
    );
    let refused = out
        .iter()
        .find_map(|m| decode::<gui::Created>(m, 0).ok())
        .unwrap();
    assert_eq!(refused.1.status.0, status::BAD_BUFFER);
    // A pattern reaches the screen pixel for pixel.
    let pattern: Vec<u32> = (0..200 * 100u32)
        .map(|i| i.wrapping_mul(2_654_435_761) & 0x00ff_ffff)
        .collect();
    let memory = s.f().memory(pattern.clone(), 200 * 100 * 4, false);
    let out = s.request(
        end,
        &gui::CreateToplevel {
            width: 100,
            height: 50,
        },
        vec![memory],
    );
    let (_, created) = out
        .iter()
        .find_map(|m| decode::<gui::Created>(m, 0).ok())
        .unwrap();
    let c = configures(&out);
    assert_eq!((c[0].width, c[0].height, c[0].scale), (100, 50, 2));
    assert_eq!(s.commit_to(end, created.window, 0, 0, 200, 100), status::OK);
    let body = s.body(0, created.window);
    assert_eq!((body.width, body.height), (200, 100));
    assert_eq!((body.x, body.y), first_body(zoraya_tokens::Scale::TWO));
    for y in 0..60 {
        for x in 0..200 {
            let at = ((body.y + y) * 2048 + body.x + x) as usize;
            assert_eq!(s.f().frame[at], pattern[(y * 200 + x) as usize]);
        }
    }
}

#[test]
fn the_first_body_is_where_the_runner_reads_it() {
    assert_eq!(first_body(zoraya_tokens::Scale::ONE), (BX, BY));
    assert_eq!(first_body(zoraya_tokens::Scale::TWO), (2 * BX, 2 * BY));
    assert_eq!(scale_for(1024, 0), zoraya_tokens::Scale::ONE);
    assert_eq!(scale_for(1920, 0), zoraya_tokens::Scale::TWO);
}

#[test]
fn the_switcher_is_drawn_in_the_material_and_closes_cleanly() {
    let mut s = setup();
    s.toplevel(200, 100, 0x0000_ff00);
    s.toplevel(200, 100, 0x00ff_0000);
    let before = s.region(Rect::new(0, 0, W.into(), H.into()));
    s.keys(&[(code::LEFT_ALT, 1), (code::TAB, 1), (code::TAB, 0)]);
    let open = s.region(Rect::new(0, 0, W.into(), H.into()));
    assert_ne!(before, open);
    // Two thumbnails, the windows' own pixels shrunk: their colours appear
    // in the panel.
    let panel = Rect::new(i64::from(W - 640) / 2, 100, 640, 200);
    let px = s.region(panel);
    assert!(px.contains(&0x0000_ff00) && px.contains(&0x00ff_0000));
    s.keys(&[(code::ESC, 1), (code::ESC, 0), (code::LEFT_ALT, 0)]);
    let after = s.region(Rect::new(0, 0, W.into(), H.into()));
    assert_eq!(before, after, "nothing of the switcher stays");
}

#[test]
fn the_shrunk_thumbnail_averages_its_pixels() {
    let src = [0u32, 0x00ff_ffff, 0x00ff_ffff, 0];
    assert_eq!(draw::shrink(&src, 2, 2, 1, 1), [0x007f_7f7f]);
    assert_eq!(draw::shrink(&src, 2, 2, 2, 2), src);
    assert_eq!(draw::shrink(&src, 2, 2, 4, 4)[0], 0);
    assert!(draw::shrink(&[], 0, 0, 3, 3).iter().all(|p| *p == 0));
}

/// Scenes of the real compositor at 1024 × 768 in both sets, for TASK-24's
/// report (the dark set cannot be shown in QEMU until 7f's settings):
/// `ZORAYA_COMPOSITOR_SAMPLES=dir cargo test -p zoraya-compositor -- --ignored samples`.
#[test]
#[ignore = "writes images"]
fn samples() {
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
        let p = appearance.palette();
        let (a, _, _) = s.toplevel(560, 360, p.term_bg.pixel());
        let out = s.request(
            a,
            &gui::SetTitle {
                window: 1,
                text: zoraya_protocol::Data("zosh — ~/project".as_bytes()),
            },
            vec![],
        );
        let _ = out;
        let (b, _, _) = s.toplevel(420, 260, p.bg_view.pixel());
        s.request(
            b,
            &gui::SetTitle {
                window: 1,
                text: zoraya_protocol::Data("notes.txt — edit".as_bytes()),
            },
            vec![],
        );
        // Move the second window aside by its title bar.
        let body = s.body(1, 1);
        let (w, h) = (1024, 768);
        let ax =
            |v: i64, side: u32| ((v * 65535 + i64::from(side) - 2) / i64::from(side - 1)) as u32;
        s.point(ax(body.x + 200, w), ax(body.y - 18, h), Some(1));
        s.point(ax(560, w), ax(330, h), None);
        s.point(ax(560, w), ax(330, h), Some(0));
        s.point(ax(1000, w), ax(740, h), None);
        let write = |s: &mut Setup, file: &str| {
            let mut out = b"P6\n1024 768\n255\n".to_vec();
            for q in &s.f().frame {
                out.extend_from_slice(&[(q >> 16) as u8, (q >> 8) as u8, *q as u8]);
            }
            std::fs::write(std::format!("{dir}/{file}-{name}.ppm"), out).unwrap();
        };
        write(&mut s, "windows");
        s.keys(&[(code::LEFT_ALT, 1), (code::TAB, 1), (code::TAB, 0)]);
        write(&mut s, "switcher");
    }
}

/// Review 7b, 1: an unfocused client cannot open a popup, and a popup sits
/// directly above its parent, takes focus while open and gives it back.
#[test]
fn only_a_focused_window_opens_popups_and_they_stay_with_it() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(200, 120, 1);
    let (b, _, _) = s.toplevel(200, 120, 2);
    // b's window is above, a's has focus (the first).
    let m = s.f().memory(vec![9; 640 * 400], 640 * 400 * 4, false);
    let out = s.request(
        b,
        &gui::CreatePopup {
            parent: 1,
            x: 0,
            y: 0,
            width: 640,
            height: 400,
        },
        vec![m],
    );
    let r = out
        .iter()
        .find_map(|m| decode::<gui::PopupCreated>(m, 0).ok())
        .unwrap();
    assert_eq!(r.1.status.0, status::UNSUPPORTED);
    // Raise a below b: a popup of a goes right above a, under nothing else.
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    assert_eq!(s.c.focused(), Some(Key { client: 0, id: 1 }));
    let m = s.f().memory(vec![9; 40 * 20], 40 * 20 * 4, false);
    let out = s.request(
        a,
        &gui::CreatePopup {
            parent: 1,
            x: 5,
            y: 5,
            width: 40,
            height: 20,
        },
        vec![m],
    );
    let (_, p) = out
        .iter()
        .find_map(|m| decode::<gui::PopupCreated>(m, 0).ok())
        .unwrap();
    assert_eq!(p.status.0, status::OK);
    let stack: Vec<Key> = s.c.stack().collect();
    let parent = stack
        .iter()
        .position(|k| *k == Key { client: 0, id: 1 })
        .unwrap();
    assert_eq!(
        stack[parent + 1],
        Key {
            client: 0,
            id: p.window
        }
    );
    assert_eq!(
        s.c.focused(),
        Some(Key {
            client: 0,
            id: p.window
        }),
        "a popup takes focus"
    );
    // Closed by the client, it gives focus back to its parent.
    s.request(a, &gui::Close { window: p.window }, vec![]);
    assert_eq!(s.c.focused(), Some(Key { client: 0, id: 1 }));
    // Minimising the parent closes its popups.
    let m = s.f().memory(vec![9; 40 * 20], 40 * 20 * 4, false);
    s.request(
        a,
        &gui::CreatePopup {
            parent: 1,
            x: 5,
            y: 5,
            width: 40,
            height: 20,
        },
        vec![m],
    );
    s.keys(&[
        (code::LEFT_META, 1),
        (code::M, 1),
        (code::M, 0),
        (code::LEFT_META, 0),
    ]);
    assert!(s.c.stack().all(|k| k.id == 1));
    let _ = b;
}

/// Review 7b, 2: a resize never makes a body larger than the work area.
#[test]
fn a_resize_stops_at_the_work_area() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(200, 100, 1);
    s.f().take(a);
    for _ in 0..4 {
        let o = s.outer(0, 1);
        // Move it far left by its title bar, then grow it from the right.
        s.at(o.x + 100, o.y + 18, Some(1));
        s.at(10, o.y + 18, Some(0));
        let o = s.outer(0, 1);
        s.at(o.right() - 2, o.bottom() - 2, Some(1));
        s.at(i64::from(W) - 1, i64::from(H) - 1, Some(0));
    }
    let body = s.body(0, 1);
    assert!(body.width <= i64::from(W) - 2, "{body:?}");
    assert!(body.height <= i64::from(H) - 28 - 37, "{body:?}");
}

/// Review 7b, 4: a client that answers is asked again, not ended.
#[test]
fn a_client_that_commits_is_asked_again_on_the_next_close() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(200, 100, 1);
    let o = s.outer(0, 1);
    s.at(o.x + 18, o.y + 18, Some(1));
    s.at(o.x + 18, o.y + 18, Some(0));
    s.f().take(a);
    // It shows a "save?" prompt: it commits, and the person cancels.
    assert_eq!(s.commit_to(a, 1, 0, 0, 10, 10), status::OK);
    s.f().clock += CLOSE_GRACE_NS;
    s.at(o.x + 18, o.y + 18, Some(1));
    assert_eq!(s.c.client_count(), 1);
    assert!(
        s.f()
            .take(a)
            .iter()
            .any(|m| decode::<gui::CloseRequested>(m, 0).is_ok())
    );
    // A long time later, an unanswered ask has lapsed too.
    s.f().clock += 10 * CLOSE_GRACE_NS;
    s.at(o.x + 18, o.y + 18, Some(0));
    s.at(o.x + 18, o.y + 18, Some(1));
    assert_eq!(s.c.client_count(), 1);
}

/// Review 7b, 5: the bar takes presses; nothing under it is reached.
#[test]
fn a_press_on_the_bar_reaches_no_window() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(200, 100, 1);
    s.f().take(a);
    s.at(BX + 50, 10, Some(1));
    s.at(BX + 50, 10, Some(0));
    assert!(
        s.f()
            .take(a)
            .iter()
            .all(|m| decode::<gui::WindowPointerMods>(m, 0).is_err())
    );
}

/// Review 7b, 7: the first commit to a new buffer shows all of it.
#[test]
fn the_first_commit_to_a_new_buffer_reads_it_whole() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 50, 1);
    let o = s.outer(0, 1);
    s.at(o.right() - 2, o.bottom() - 2, Some(1));
    s.at(o.right() + 18, o.bottom() + 8, Some(0));
    let c = configures(&s.f().take(a));
    let (w, h) = (c[0].width, c[0].height);
    let n = (w * h) as usize;
    let m = s.f().memory(vec![0x0012_3456; n], n as u64 * 4, false);
    s.request(
        a,
        &gui::SetBuffer {
            window: 1,
            width: w,
            height: h,
        },
        vec![m],
    );
    assert_eq!(s.commit_to(a, 1, 0, 0, 1, 1), status::OK);
    let body = s.body(0, 1);
    assert_eq!(s.pixel(body.x + 60, body.y + 30), 0x0012_3456);
}

/// Review 7b, 10: one client cannot take more than its share of buffers.
#[test]
fn a_client_has_a_budget_of_buffers() {
    let mut s = setup();
    let end = s.client(64);
    let mut ok = 0;
    for _ in 0..8 {
        let n = (W * (H - 70)) as usize;
        let m = s.f().memory(vec![0; n], n as u64 * 4, false);
        let out = s.request(
            end,
            &gui::CreateToplevel {
                width: W,
                height: H - 70,
            },
            vec![m],
        );
        if out
            .iter()
            .find_map(|m| decode::<gui::Created>(m, 0).ok())
            .unwrap()
            .1
            .status
            .0
            == status::OK
        {
            ok += 1;
        }
    }
    assert_eq!(ok, 4, "four displays' worth");
}

/// Review 7b, 12: a key whose press reached the client is released to it,
/// even when the release comes during a chord.
#[test]
fn a_release_reaches_the_client_that_saw_the_press() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 50, 1);
    s.f().take(a);
    s.keys(&[(30, 1), (code::LEFT_META, 1), (30, 0), (code::LEFT_META, 0)]);
    let got = keys_of(&s.f().take(a));
    assert_eq!(
        got.iter().map(|k| (k.0, k.1)).collect::<Vec<_>>(),
        [(30, true), (30, false)]
    );
    // A key pressed during a chord of the compositor's is released to
    // nobody.
    s.keys(&[
        (code::LEFT_META, 1),
        (code::R, 1),
        (code::LEFT_META, 0),
        (code::R, 0),
    ]);
    assert!(keys_of(&s.f().take(a)).is_empty());
    // One of a chord it does not take reaches the client, its release too
    // (ADR-0248).
    s.keys(&[(code::LEFT_META, 1), (31, 1), (code::LEFT_META, 0), (31, 0)]);
    let got = keys_of(&s.f().take(a));
    assert_eq!(
        got.iter().map(|k| (k.0, k.1)).collect::<Vec<_>>(),
        [(31, true), (31, false)]
    );
}

/// Mutation testing (7b): every clamp and side of a drag.
#[test]
fn drags_clamp_the_title_bar_and_resize_from_every_side() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(200, 100, 1);
    s.f().take(a);
    let title = |s: &Setup| {
        let b = s.body(0, 1);
        (b.x + 100, b.y - 18)
    };
    // Far left, held near its right end: at most all but two title
    // heights off the screen.
    let (tx, ty) = title(&s);
    s.at(tx + 90, ty, Some(1));
    s.at(0, ty, None);
    s.at(0, ty, Some(0));
    assert_eq!(s.body(0, 1).x, -200 + 72);
    // Back to the middle, then far right, held just right of the buttons.
    let b = s.body(0, 1);
    s.at(b.x + 150, b.y - 18, Some(1));
    s.at(300, b.y - 18, Some(0));
    let b = s.body(0, 1);
    s.at(b.x + 60, b.y - 18, Some(1));
    s.at(i64::from(W) - 1, b.y - 18, Some(0));
    assert_eq!(s.body(0, 1).x, i64::from(W) - 72);
    // Far up: the title bar under the bar; far down: one row left.
    let b = s.body(0, 1);
    s.at(b.x + 60, b.y - 18, Some(1));
    s.at(b.x + 60, 0, Some(0));
    assert_eq!(s.body(0, 1).y, 28 + 36);
    let b = s.body(0, 1);
    s.at(b.x + 60, b.y - 18, Some(1));
    s.at(b.x + 60, i64::from(H) - 1, Some(0));
    assert_eq!(
        s.body(0, 1).y,
        i64::from(H) - 1,
        "the title bar above the bottom edge"
    );
}

#[test]
fn resizes_from_the_left_and_the_top_and_down_to_one_pixel() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(200, 100, 1);
    s.f().take(a);
    let body = s.body(0, 1);
    // The left grip by -30, the top grip by -20.
    let o = s.outer(0, 1);
    s.at(o.x + 1, o.y + 60, Some(1));
    s.at(o.x - 29, o.y + 60, Some(0));
    let b = s.body(0, 1);
    assert_eq!(
        (b.x, b.width, b.y, b.height),
        (body.x - 30, body.width + 30, body.y, body.height)
    );
    let o = s.outer(0, 1);
    s.at(o.x + 100, o.y + 1, Some(1));
    s.at(o.x + 100, o.y - 9, Some(0));
    let c = s.body(0, 1);
    assert_eq!(
        (c.y, c.height, c.x, c.width),
        (b.y - 10, b.height + 10, b.x, b.width)
    );
    // Shrunk past nothing: one logical pixel at least.
    let o = s.outer(0, 1);
    s.at(o.right() - 2, o.bottom() - 2, Some(1));
    s.at(0, 29, Some(0));
    let d = s.body(0, 1);
    assert_eq!((d.width, d.height), (1, 1));
    let conf = configures(&s.f().take(a));
    assert_eq!(
        (conf.last().unwrap().width, conf.last().unwrap().height),
        (1, 1)
    );
}

#[test]
fn a_resize_at_scale_two_keeps_whole_logical_pixels() {
    let mut s = setup_with(|f| f.display_size = (2048, 1536));
    let (a, _, _) = s.toplevel(100, 50, 1);
    s.f().take(a);
    let o = s.outer(0, 1);
    s.at(o.right() - 2, o.bottom() - 2, Some(1));
    s.at(o.right() + 9, o.bottom() + 7, Some(0));
    let b = s.body(0, 1);
    assert_eq!((b.width % 2, b.height % 2), (0, 0));
    let c = configures(&s.f().take(a));
    assert_eq!(
        (c[0].width, c[0].height, c[0].scale),
        (b.width as u32 / 2, b.height as u32 / 2, 2)
    );
}

/// Review 7b, 13, and mutation testing: a display of a new size gives
/// every toplevel its logical size at the new scale, configured.
#[test]
fn a_new_scale_doubles_every_toplevel_and_configures_each() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 50, 1);
    let (b, _, _) = s.toplevel(60, 40, 2);
    let (pa, pb) = (s.body(0, 1), s.body(1, 1));
    s.f().take(a);
    s.f().take(b);
    // The display driver hangs and is dropped; one of twice the size
    // attaches.
    let old = s.f().display.unwrap();
    s.f().display_dead = true;
    s.commit_to(a, 1, 0, 0, 1, 1);
    for _ in 0..4 {
        if s.f().closed.contains(&old) {
            break;
        }
        s.c.step();
    }
    assert!(s.f().closed.contains(&old));
    s.f().display_dead = false;
    s.f().display_size = (2048, 1536);
    let d = s.f().channel(8);
    assert_eq!(super::attach_display(&mut s, d), status::OK);
    assert_eq!(s.c.scale(), zoraya_tokens::Scale::TWO);
    for (end, client, p, (w, h)) in [(a, 0, pa, (100, 50)), (b, 1, pb, (60, 40))] {
        let c = configures(&s.f().take(end));
        let last = c.last().expect("a Configure");
        assert_eq!((last.width, last.height, last.scale), (w, h, 2));
        let now = s.body(client, 1);
        assert_eq!(
            (now.width, now.height),
            (2 * i64::from(w), 2 * i64::from(h))
        );
        assert_eq!((now.x, now.y), (2 * p.x, 2 * p.y));
    }
}

fn layouts_of(out: &[Vec<u8>]) -> Vec<std::string::String> {
    out.iter()
        .filter_map(|m| decode::<gui::Layout<'_>>(m, 0).ok())
        .map(|(_, l)| std::string::String::from_utf8_lossy(l.name.0).into_owned())
        .collect()
}

/// ADR-0163 in the compositor: System+Space cycles us, ua, cz (Shift:
/// back), the focused client hears the layout, keys give the layout's
/// letters, and a switch reaches no client as a key (INV-0163-4).
#[test]
fn system_space_switches_the_layout_and_the_client_hears_it() {
    let mut s = setup();
    // The first client hears the layout when it gains focus.
    let end = s.client(64);
    let n = 100 * 50;
    let m = s.f().memory(vec![1; n], n as u64 * 4, false);
    let out = s.request(
        end,
        &gui::CreateToplevel {
            width: 100,
            height: 50,
        },
        vec![m],
    );
    assert_eq!(layouts_of(&out), ["us"], "on focus");
    let a = end;
    assert_eq!(s.c.layout_short(), "US");
    let meta = code::LEFT_META;
    // With Shift held from before: the switch goes back (us → cz), and
    // the client sees Shift's press and release and nothing of Space.
    s.keys(&[
        (code::LEFT_SHIFT, 1),
        (meta, 1),
        (code::SPACE, 1),
        (code::SPACE, 0),
        (meta, 0),
        (code::LEFT_SHIFT, 0),
    ]);
    assert_eq!(s.c.layout_short(), "CZ");
    let out = s.f().take(a);
    assert_eq!(layouts_of(&out), ["cz"]);
    let keys: Vec<(u32, bool)> = keys_of(&out).iter().map(|k| (k.0, k.1)).collect();
    assert_eq!(keys, [(code::LEFT_SHIFT, true), (code::LEFT_SHIFT, false)]);
    s.keys(&[(meta, 1), (code::SPACE, 1), (code::SPACE, 0), (meta, 0)]);
    assert_eq!(s.c.layout_short(), "US");
    s.keys(&[(meta, 1), (code::SPACE, 1), (code::SPACE, 0), (meta, 0)]);
    assert_eq!(s.c.layout_short(), "UA");
    s.f().take(a);
    s.keys(&[(16, 1), (16, 0), (43, 1), (43, 0)]);
    let text: std::string::String = keys_of(&s.f().take(a)).iter().filter_map(|k| k.2).collect();
    assert_eq!(text, "йґ");
}

#[test]
fn a_dead_key_does_not_carry_to_another_window() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 50, 1);
    let (b, _, _) = s.toplevel(100, 50, 2);
    let meta = code::LEFT_META;
    // To cz (back from us), the dead acute, then focus moves.
    s.keys(&[
        (code::LEFT_SHIFT, 1),
        (meta, 1),
        (code::SPACE, 1),
        (code::SPACE, 0),
        (meta, 0),
        (code::LEFT_SHIFT, 0),
    ]);
    s.keys(&[(13, 1), (13, 0)]);
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    s.f().take(a);
    s.f().take(b);
    s.keys(&[(18, 1), (18, 0)]);
    let got: std::string::String = [a, b]
        .iter()
        .flat_map(|e| keys_of(&s.f().take(*e)))
        .filter_map(|k| k.2)
        .collect();
    assert_eq!(got, "e", "not é");
}

/// INV-0163-4 with a key held across the switch, `Layout` to the focused
/// client only, and a chord that does not arm a dead key (review 7c, 3, 10).
#[test]
fn a_switch_keeps_a_held_key_and_tells_only_the_focused_client() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 50, 1);
    let (b, _, _) = s.toplevel(100, 50, 2);
    s.f().take(a);
    s.f().take(b);
    let meta = code::LEFT_META;
    // 'a' (code 30) held across a switch to ua: the client gets its press
    // and its release with the same code, nothing else.
    s.keys(&[
        (30, 1),
        (meta, 1),
        (code::SPACE, 1),
        (code::SPACE, 0),
        (meta, 0),
        (30, 0),
    ]);
    let out = s.f().take(a);
    let keys: Vec<(u32, bool)> = keys_of(&out).iter().map(|k| (k.0, k.1)).collect();
    assert_eq!(keys, [(30, true), (30, false)]);
    assert_eq!(layouts_of(&out), ["ua"]);
    assert!(layouts_of(&s.f().take(b)).is_empty(), "b is not focused");
    // To cz; System+= is a chord and arms nothing: the next e is e.
    s.keys(&[(meta, 1), (code::SPACE, 1), (code::SPACE, 0), (meta, 0)]);
    assert_eq!(s.c.layout_short(), "CZ");
    s.keys(&[(meta, 1), (13, 1), (13, 0), (meta, 0), (18, 1), (18, 0)]);
    let text: std::string::String = keys_of(&s.f().take(a)).iter().filter_map(|k| k.2).collect();
    assert_eq!(text, "e");
}

/// A window is raised with its popups; another window of the same client
/// stays where it was.
#[test]
fn raising_a_window_carries_its_popups_and_no_other_window() {
    let mut s = setup();
    let (a, _, ida) = s.toplevel(200, 120, 1);
    let m = s.f().memory(vec![7; 20 * 10], 800, false);
    let out = s.request(
        a,
        &gui::CreatePopup {
            parent: ida,
            x: 100,
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
    let (_, second) = s.toplevel_of(a, 200, 120, 2);
    let (_, _, idb) = s.toplevel(200, 120, 3);
    let key = |client, id| Key { client, id };
    assert_eq!(
        s.c.stack().collect::<Vec<_>>(),
        [key(0, ida), key(0, p.window), key(0, second), key(1, idb)]
    );
    // Raised (a press would close the popup: the switcher's way).
    s.c.activate(key(0, ida));
    assert_eq!(
        s.c.stack().collect::<Vec<_>>(),
        [key(0, second), key(1, idb), key(0, ida), key(0, p.window)]
    );
}

/// A popup's outline is its body: no title bar, no border.
#[test]
fn a_popups_outline_is_its_body() {
    let mut s = setup();
    let (a, _, ida) = s.toplevel(200, 120, 1);
    let m = s.f().memory(vec![7; 20 * 10], 800, false);
    let out = s.request(
        a,
        &gui::CreatePopup {
            parent: ida,
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
    assert_eq!(s.outer(0, p.window), s.body(0, p.window));
}

/// INV-0248-1 over every key: with the system key held, a key of the
/// compositor's table never reaches a client; every other key reaches the
/// focused client, with `SYSTEM`.
#[test]
fn every_system_chord_is_the_compositors_or_the_focused_clients() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 60, 1);
    s.f().take(a);
    let meta = code::LEFT_META;
    for k in 1u32..0x110 {
        if [code::LEFT_META, code::RIGHT_META].contains(&k)
            || [
                code::W,
                code::M,
                code::LEFT,
                code::RIGHT,
                code::ENTER,
                code::SLASH,
                code::SPACE,
                code::R,
            ]
            .contains(&k)
        {
            // Those act (close, hide, tile, a terminal, the modes) and
            // change the scene; R is checked below with the rest.
            continue;
        }
        s.keys(&[(meta, 1), (k, 1), (k, 0), (meta, 0)]);
        let got: Vec<gui::Key> = s
            .f()
            .take(a)
            .iter()
            .filter_map(|m| decode::<gui::Key>(m, 0).ok())
            .map(|(_, k)| k)
            .collect();
        if TAKEN_CHORDS.contains(&k) {
            assert!(got.is_empty(), "{k} is the compositor's");
        } else {
            let press = got.iter().find(|g| g.pressed);
            assert!(
                press.is_some_and(|p| p.code == k && p.modifiers.0 & gui::modifiers::SYSTEM != 0),
                "{k} reaches the focused client"
            );
        }
    }
    s.keys(&[(meta, 1), (code::R, 1), (code::R, 0), (meta, 0)]);
    assert!(keys_of(&s.f().take(a)).is_empty());
}

/// Mutation testing (7h): a chord of the compositor's acts once, on the
/// press, not on a repeat or a release; a forwarded chord carries the
/// modifiers held with `SYSTEM`, no more and no fewer.
#[test]
fn a_chord_acts_on_its_press_and_a_forwarded_one_keeps_its_modifiers() {
    let mut s = setup();
    let (a, _, _) = s.toplevel(100, 60, 1);
    s.f().take(a);
    let meta = code::LEFT_META;
    // System+? with a repeat and a release: the mode toggles once.
    s.keys(&[
        (meta, 1),
        (code::SLASH, 1),
        (code::SLASH, 2),
        (code::SLASH, 0),
        (meta, 0),
    ]);
    assert!(s.c.commands, "toggled once, not three times");
    s.keys(&[(meta, 1), (code::SLASH, 1), (code::SLASH, 0), (meta, 0)]);
    assert!(!s.c.commands);
    // Shift and Ctrl held with System and a free key: exactly those and SYSTEM.
    s.f().take(a);
    s.keys(&[
        (meta, 1),
        (code::LEFT_SHIFT, 1),
        (49, 1),
        (49, 0),
        (code::LEFT_SHIFT, 0),
        (meta, 0),
    ]);
    let k: Vec<gui::Key> = s
        .f()
        .take(a)
        .iter()
        .filter_map(|m| decode::<gui::Key>(m, 0).ok())
        .map(|(_, k)| k)
        .filter(|k| k.pressed && k.code == 49)
        .collect();
    assert_eq!(k.len(), 1);
    assert_eq!(
        k[0].modifiers.0,
        gui::modifiers::SHIFT | gui::modifiers::SYSTEM
    );
}

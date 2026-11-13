use super::*;
use alloc::string::String;
use zoraya_protocol::Below;

/// A fake compositor: it answers each request at once, keeps what was sent
/// and gives the events a test queues.
#[derive(Default)]
struct Fake {
    sent: Vec<u32>,
    inbox: VecDeque<Vec<u8>>,
    buffers: Vec<Vec<u32>>,
    freed: Vec<usize>,
    scale: u32,
    titles: Vec<String>,
    /// Each commit's rectangle, and the transactions not yet answered.
    commits: Vec<(u32, u32, u32, u32)>,
    unanswered: VecDeque<u32>,
    refuse_create: bool,
    refuse_buffer: bool,
    gone: bool,
}

fn reply<'a, M: Message<'a>>(m: &M, t: u32) -> Vec<u8> {
    message(m, t)
}

impl Platform for Fake {
    type End = u32;

    fn send(&mut self, bytes: &[u8], _handle: Option<u32>) -> bool {
        if self.gone {
            return false;
        }
        let (h, _) = Header::decode(bytes).unwrap();
        self.sent.push(h.ordinal);
        let t = h.transaction;
        let ok = Below(status::OK);
        match h.ordinal {
            4 => self.inbox.push_back(reply(
                &gui::DisplayScaleReply {
                    status: ok,
                    width: 1024,
                    height: 768,
                    scale: self.scale.max(1),
                },
                t,
            )),
            5 => {
                let st = if self.refuse_create {
                    Below(status::NO_MEMORY)
                } else {
                    ok
                };
                self.inbox.push_back(reply(
                    &gui::Created {
                        status: st,
                        window: 3,
                    },
                    t,
                ));
            }
            7 => {
                let (_, c) = decode::<gui::CommitTo>(bytes, 0).unwrap();
                assert_eq!(c.window, 3);
                self.commits.push((c.x, c.y, c.width, c.height));
                self.unanswered.push_back(t);
            }
            8 => {
                let st = if self.refuse_buffer {
                    Below(status::BAD_BUFFER)
                } else {
                    ok
                };
                self.inbox
                    .push_back(reply(&gui::BufferDone { status: st }, t));
            }
            9 => {
                let (_, m) = decode::<gui::SetTitle<'_>>(bytes, 0).unwrap();
                self.titles
                    .push(String::from_utf8(m.text.0.to_vec()).unwrap());
                self.inbox
                    .push_back(reply(&gui::TitleDone { status: ok }, t));
            }
            10 => self
                .inbox
                .push_back(reply(&gui::CloseDone { status: ok }, t)),
            _ => {}
        }
        true
    }

    fn receive(&mut self, bytes: &mut [u8]) -> Option<usize> {
        let m = self.inbox.pop_front()?;
        bytes[..m.len()].copy_from_slice(&m);
        Some(m.len())
    }

    fn poll(&mut self, bytes: &mut [u8]) -> Poll {
        match self.inbox.pop_front() {
            Some(m) => {
                bytes[..m.len()].copy_from_slice(&m);
                Poll::Message(m.len())
            }
            None if self.gone => Poll::Gone,
            None => Poll::Empty,
        }
    }

    fn buffer(&mut self, pixels: usize) -> Option<(u32, usize)> {
        self.buffers.push(vec![0; pixels]);
        Some((self.buffers.len() as u32, self.buffers.len() - 1))
    }

    fn pixels(&mut self, buffer: usize) -> &mut [u32] {
        &mut self.buffers[buffer]
    }

    fn free(&mut self, buffer: usize) {
        self.freed.push(buffer);
    }
}

impl Fake {
    fn push<'a, M: Message<'a>>(&mut self, m: &M) {
        self.inbox.push_back(reply(m, 0));
    }
    /// Answers the oldest commit.
    fn answer(&mut self) {
        let t = self.unanswered.pop_front().unwrap();
        self.inbox.push_back(reply(
            &gui::CommitDone {
                status: Below(status::OK),
            },
            t,
        ));
    }
}

fn open(scale: u32) -> Surface<Fake> {
    let f = Fake {
        scale,
        ..Fake::default()
    };
    Surface::open(f, "Test", 100, 50).unwrap()
}

#[test]
fn opens_a_toplevel_with_a_title_and_a_buffer_at_the_scale() {
    let s = open(2);
    assert_eq!(s.size(), (200, 100));
    assert_eq!(s.scale(), 2);
    assert_eq!(s.platform.titles, ["Test"]);
    assert_eq!(s.platform.buffers[0].len(), 200 * 100);
    assert_eq!(s.platform.sent, [4, 5, 9]);
}

#[test]
fn a_refused_toplevel_frees_the_buffer_and_opens_nothing() {
    let f = Fake {
        refuse_create: true,
        ..Fake::default()
    };
    assert!(Surface::open(f, "x", 10, 10).is_none());
    let f = Fake::default();
    assert!(Surface::open(f, "x", 0, 10).is_none());
}

#[test]
fn only_client_requests_are_sent() {
    let mut s = open(1);
    s.damage(0, 0, 10, 10);
    s.commit();
    s.answer_and_close();
    for o in &s.platform.sent {
        assert!(zoraya_proto_gui::CLIENT_REQUESTS.contains(o), "{o}");
    }
}

impl Surface<Fake> {
    fn answer_and_close(&mut self) {
        self.platform.answer();
        assert_eq!(self.event(false), None);
        let w = self.window;
        self.call(&gui::Close { window: w }, None);
    }
}

#[test]
fn damage_is_clipped_to_the_buffer_before_anything_is_sent() {
    // Every rectangle, in and out of the buffer: what is committed lies
    // inside it (INV-0317-2, INV-0317-3).
    let edges: [i64; 9] = [i64::MIN, -1000, -1, 0, 1, 99, 100, 101, i64::MAX];
    for &x in &edges {
        for &y in &edges {
            for &w in &edges {
                for &h in &edges {
                    let mut s = open(1);
                    s.damage(x, y, w, h);
                    s.commit();
                    for &(cx, cy, cw, ch) in &s.platform.commits {
                        assert!(cw > 0 && ch > 0);
                        assert!(cx + cw <= 100 && cy + ch <= 50, "{x} {y} {w} {h}");
                    }
                }
            }
        }
    }
}

#[test]
fn an_empty_or_outside_rectangle_commits_nothing() {
    let mut s = open(1);
    s.damage(10, 10, 0, 5);
    s.damage(100, 0, 5, 5);
    s.damage(-9, -9, 9, 9);
    s.commit();
    assert!(s.platform.commits.is_empty());
}

#[test]
fn one_frame_is_in_flight_and_damage_meanwhile_is_united() {
    let mut s = open(1);
    s.damage(10, 10, 5, 5);
    s.commit();
    s.damage(20, 30, 5, 5);
    s.damage(2, 4, 3, 3);
    s.commit();
    assert_eq!(s.platform.commits, [(10, 10, 5, 5)]);
    s.platform.answer();
    assert_eq!(s.event(false), None);
    // The reply sent the damage that waited, as one rectangle.
    assert_eq!(s.platform.commits, [(10, 10, 5, 5), (2, 4, 23, 31)]);
    s.platform.answer();
    assert_eq!(s.event(false), None);
    s.commit();
    assert_eq!(s.platform.commits.len(), 2);
}

#[test]
fn the_pixels_are_the_buffer() {
    let mut s = open(1);
    assert_eq!(s.pixels().len(), 100 * 50);
    s.pixels()[7] = 0x00ff_00ff;
    assert_eq!(s.platform.buffers[0][7], 0x00ff_00ff);
}

#[test]
fn keys_arrive_with_their_text_and_modifiers() {
    let mut s = open(1);
    s.platform.push(&gui::Key {
        dropped: 0,
        code: 30,
        pressed: true,
        modifiers: zoraya_protocol::Flags(gui::modifiers::SHIFT),
        text: Below(65),
    });
    s.platform.push(&gui::Key {
        dropped: 0,
        code: 30,
        pressed: false,
        modifiers: zoraya_protocol::Flags(0),
        text: Below(0),
    });
    assert_eq!(
        s.event(false),
        Some(Event::Key {
            code: 30,
            text: 65,
            pressed: true,
            modifiers: 1
        })
    );
    assert!(matches!(
        s.event(false),
        Some(Event::Key { pressed: false, .. })
    ));
    assert_eq!(s.event(false), None);
}

fn pointer(s: &mut Surface<Fake>, x: u32, y: u32, buttons: u32) {
    s.platform.push(&gui::WindowPointerMods {
        dropped: 0,
        window: 3,
        x,
        y,
        buttons: zoraya_protocol::Flags(buttons),
        serial: 1,
        modifiers: zoraya_protocol::Flags(0),
    });
}

#[test]
fn pointer_states_become_moves_and_button_changes() {
    let mut s = open(2);
    pointer(&mut s, 5, 6, 0);
    pointer(&mut s, 5, 6, buttons::LEFT);
    pointer(&mut s, 7, 6, buttons::LEFT | buttons::RIGHT);
    pointer(&mut s, 7, 6, 0);
    let mut got = Vec::new();
    while let Some(e) = s.event(false) {
        got.push(e);
    }
    assert_eq!(
        got,
        [
            Event::Move { x: 10, y: 12 },
            Event::Button {
                x: 10,
                y: 12,
                button: buttons::LEFT,
                pressed: true
            },
            Event::Move { x: 14, y: 12 },
            Event::Button {
                x: 14,
                y: 12,
                button: buttons::RIGHT,
                pressed: true
            },
            Event::Button {
                x: 14,
                y: 12,
                button: buttons::LEFT,
                pressed: false
            },
            Event::Button {
                x: 14,
                y: 12,
                button: buttons::RIGHT,
                pressed: false
            },
        ]
    );
}

#[test]
fn the_pointer_is_kept_inside_the_buffer() {
    let mut s = open(1);
    pointer(&mut s, 5000, 4000, 0);
    assert_eq!(s.event(false), Some(Event::Move { x: 99, y: 49 }));
}

#[test]
fn a_new_size_gets_a_new_buffer_and_frees_the_old_one() {
    let mut s = open(1);
    s.platform.push(&gui::Configure {
        window: 3,
        width: 120,
        height: 80,
        scale: 1,
        dark: false,
        motion: true,
    });
    assert_eq!(
        s.event(false),
        Some(Event::Resize {
            width: 120,
            height: 80
        })
    );
    assert_eq!(s.size(), (120, 80));
    assert_eq!(s.pixels().len(), 120 * 80);
    assert_eq!(s.platform.freed, [0]);
    // The same size again changes nothing.
    s.platform.push(&gui::Configure {
        window: 3,
        width: 120,
        height: 80,
        scale: 1,
        dark: true,
        motion: true,
    });
    assert_eq!(s.event(false), None);
}

#[test]
fn a_refused_buffer_keeps_the_old_one() {
    let mut s = open(1);
    s.platform.refuse_buffer = true;
    s.platform.push(&gui::Configure {
        window: 3,
        width: 120,
        height: 80,
        scale: 1,
        dark: false,
        motion: true,
    });
    assert_eq!(s.event(false), None);
    assert_eq!(s.size(), (100, 50));
    assert_eq!(s.platform.freed, [1]);
}

#[test]
fn close_focus_and_a_vanished_compositor_are_events() {
    let mut s = open(1);
    s.platform.push(&gui::WindowFocus {
        dropped: 0,
        window: 3,
        focused: true,
    });
    s.platform.push(&gui::WindowFocus {
        dropped: 0,
        window: 9,
        focused: true,
    });
    s.platform.push(&gui::CloseRequested { window: 3 });
    assert_eq!(s.event(false), Some(Event::Focus(true)));
    assert_eq!(s.event(false), Some(Event::Close));
    assert_eq!(s.event(false), None);
    s.platform.gone = true;
    assert_eq!(s.event(false), Some(Event::Gone));
    assert_eq!(s.event(true), Some(Event::Gone));
}

#[test]
fn a_long_title_is_cut_at_a_character() {
    let mut s = open(1);
    let t: String = "\u{e9}".repeat(gui::MAX_TITLE);
    assert!(s.set_title(&t));
    let sent = s.platform.titles.last().unwrap();
    assert!(sent.len() <= gui::MAX_TITLE && sent.chars().all(|c| c == '\u{e9}'));
}

#[test]
fn close_sends_close_and_frees_the_buffer() {
    let s = open(1);
    s.close();
}

/// The open size is fitted to the display's logical size (W2 of the review):
/// the fake's display is 1024 x 768 logical.
#[test]
fn the_open_size_is_fitted_to_the_display() {
    for scale in [1, 2] {
        let f = Fake {
            scale,
            ..Fake::default()
        };
        let s = Surface::open(f, "big", 5000, 4000).unwrap();
        assert_eq!(s.size(), (1024 * scale, 768 * scale));
        assert_eq!(
            s.platform.buffers[0].len() as u32,
            1024 * 768 * scale * scale
        );
    }
}

/// A `Configure` whose size overflows the physical size is ignored.
#[test]
fn a_configure_with_overflowing_sizes_changes_nothing() {
    let mut s = open(1);
    for (w, h, k) in [
        (u32::MAX, 10, 4),
        (10, u32::MAX, 4),
        (0x4000_0000, 0x4000_0000, 1),
        (0, 5, 1),
    ] {
        s.platform.push(&gui::Configure {
            window: 3,
            width: w,
            height: h,
            scale: k,
            dark: false,
            motion: true,
        });
        assert_eq!(s.event(false), None, "{w} {h} {k}");
    }
    assert_eq!(s.size(), (100, 50));
    assert_eq!(s.platform.buffers.len(), 1);
}

/// A resize while a frame is in flight: the frame's reply still comes, no
/// second commit is sent before it, and damage after the resize is clipped
/// to the new buffer.
#[test]
fn a_resize_with_a_frame_in_flight_and_damage_after_it() {
    let mut s = open(1);
    s.damage(0, 0, 100, 50);
    s.commit();
    assert_eq!(s.platform.commits.len(), 1);
    s.platform.push(&gui::Configure {
        window: 3,
        width: 40,
        height: 30,
        scale: 1,
        dark: false,
        motion: true,
    });
    assert_eq!(
        s.event(false),
        Some(Event::Resize {
            width: 40,
            height: 30
        })
    );
    // Damage beyond the new buffer, while the old frame is unanswered.
    s.damage(0, 0, 100, 50);
    s.commit();
    assert_eq!(s.platform.commits.len(), 1, "one frame in flight");
    s.platform.answer();
    assert_eq!(s.event(false), None);
    assert_eq!(s.platform.commits.len(), 2);
    assert_eq!(s.platform.commits[1], (0, 0, 40, 30));
}

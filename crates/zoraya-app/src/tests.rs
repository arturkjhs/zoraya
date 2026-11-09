use super::*;
use alloc::string::String;
use zoraya_ui::{Button, Column, TextField, keys};

/// A fake compositor: it answers each request at once, keeps every
/// ordinal sent, and gives the events a test queues.
#[derive(Default)]
struct Fake {
    sent: Vec<u32>,
    /// Messages the app will receive, in order.
    inbox: VecDeque<Vec<u8>>,
    buffers: Vec<Vec<u32>>,
    next_window: u32,
    /// Commits not yet answered: answered when the test says.
    commits: VecDeque<u32>,
    titles: Vec<String>,
    sizes: Vec<(u32, u32)>,
    /// The display's scale (0 reads as 1).
    scale: u32,
    /// Each commit's width and height in pixels.
    committed: Vec<(u32, u32)>,
    /// Handles queued with the inbox's messages, by position; the one the
    /// last received message carried; and those sent with a request.
    carried: VecDeque<Option<u32>>,
    last: Option<u32>,
    given: Vec<u32>,
    /// What a `Drag` is answered, and the serials drags named.
    drag_status: u32,
    drag_serials: Vec<u32>,
    /// What a `CreateSheet` is answered: refused, or the sheet 5.
    refuse_sheet: bool,
}

fn reply<'a, M: Message<'a>>(m: &M, t: u32) -> Vec<u8> {
    message(m, t)
}

impl Platform for Fake {
    type End = u32;

    fn send(&mut self, bytes: &[u8], handle: Option<u32>) -> bool {
        let (h, _) = Header::decode(bytes).unwrap();
        self.sent.push(h.ordinal);
        self.given.extend(handle);
        let ok = zoraya_protocol::Below(status::OK);
        let t = h.transaction;
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
                let (_, c) = decode::<gui::CreateToplevel>(bytes, 1).unwrap();
                self.sizes.push((c.width, c.height));
                self.next_window += 1;
                self.inbox.push_back(reply(
                    &gui::Created {
                        status: ok,
                        window: self.next_window,
                    },
                    t,
                ));
            }
            7 => {
                let (_, c) = decode::<gui::CommitTo>(bytes, 0).unwrap();
                self.committed.push((c.width, c.height));
                self.commits.push_back(t);
            }
            8 => self
                .inbox
                .push_back(reply(&gui::BufferDone { status: ok }, t)),
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
            11 => {
                let status = if self.refuse_sheet {
                    status::UNSUPPORTED
                } else {
                    status::OK
                };
                self.inbox.push_back(reply(
                    &gui::SheetCreated {
                        status: zoraya_protocol::Below(status),
                        window: 5,
                    },
                    t,
                ));
            }
            13 => {
                let (_, d) = decode::<gui::Drag<'_>>(bytes, 1).unwrap();
                self.drag_serials.push(d.serial);
                self.inbox.push_back(reply(
                    &gui::DragDone {
                        status: zoraya_protocol::Below(self.drag_status),
                    },
                    t,
                ));
            }
            _ => {}
        }
        true
    }

    fn handle(&mut self) -> Option<u32> {
        self.last.take()
    }

    fn receive(&mut self, bytes: &mut [u8]) -> Option<usize> {
        let m = self.inbox.pop_front()?;
        // Messages queued before the handles' list was kept carry none.
        let n = self.inbox.len() + 1;
        while self.carried.len() > n {
            self.carried.pop_front();
        }
        self.last = if self.carried.len() == n {
            self.carried.pop_front().flatten()
        } else {
            None
        };
        bytes[..m.len()].copy_from_slice(&m);
        Some(m.len())
    }

    fn buffer(&mut self, pixels: usize) -> Option<(u32, usize)> {
        self.buffers.push(vec![0; pixels]);
        Some((900 + self.buffers.len() as u32, self.buffers.len() - 1))
    }

    fn pixels(&mut self, buffer: usize) -> &mut [u32] {
        &mut self.buffers[buffer]
    }

    fn free(&mut self, buffer: usize) {
        self.buffers[buffer].clear();
    }
}

impl Fake {
    fn event<'a, M: Message<'a>>(&mut self, m: &M) {
        self.inbox.push_back(message(m, 0));
    }

    /// An event with a handle: the handles' list then covers the inbox.
    fn event_with<'a, M: Message<'a>>(&mut self, m: &M, h: u32) {
        while self.carried.len() < self.inbox.len() {
            self.carried.push_back(None);
        }
        self.inbox.push_back(message(m, 0));
        self.carried.push_back(Some(h));
    }

    /// Answers the oldest commit.
    fn frame(&mut self) {
        let t = self.commits.pop_front().expect("a commit in flight");
        let ok = zoraya_protocol::Below(status::OK);
        self.inbox
            .push_back(reply(&gui::CommitDone { status: ok }, t));
    }
}

fn key(code: u32) -> gui::Key {
    gui::Key {
        dropped: 0,
        code,
        pressed: true,
        modifiers: zoraya_protocol::Flags(0),
        text: zoraya_protocol::Below(0),
    }
}

fn form() -> Box<dyn Widget> {
    Box::new(Column::new(vec![
        Box::new(TextField::new(1, "notes.txt")),
        Box::new(Button::new(2, "Save")),
    ]))
}

/// INV-0165-2: the application sends only client requests, and none that
/// asks for focus, position or stacking (the protocol has none).
#[test]
fn an_application_sends_only_client_requests() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    let id = app.open("edit — notes.txt", form(), 320, 200).unwrap();
    assert_eq!(app.platform.titles, ["edit — notes.txt"]);
    app.platform.event(&gui::WindowFocus {
        dropped: 0,
        window: id,
        focused: true,
    });
    app.platform.event(&key(keys::TAB));
    app.platform.event(&key(keys::TAB));
    app.platform.event(&key(keys::ENTER));
    app.platform.frame();
    assert_eq!(
        app.wait(),
        Happened::Ui {
            window: id,
            response: Response::Activated(2)
        }
    );
    app.platform.event(&gui::Configure {
        window: id,
        width: 400,
        height: 260,
        scale: 1,
        dark: false,
        motion: true,
    });
    app.platform.event(&gui::CloseRequested { window: id });
    assert_eq!(app.wait(), Happened::CloseRequested(id));
    app.close(id);
    assert_eq!(app.wait(), Happened::Gone);
    for o in &app.platform.sent {
        assert!(gui::CLIENT_REQUESTS.contains(o), "ordinal {o}");
    }
    // DisplayScale, CreateToplevel, SetTitle, CommitTo, SetBuffer, Close.
    for o in [4, 5, 9, 7, 8, 10] {
        assert!(app.platform.sent.contains(&o), "{o} never sent");
    }
}

/// One commit in flight (ADR-0093): a change while one waits is painted
/// and committed when its reply comes.
#[test]
fn one_commit_at_a_time() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    let id = app.open("t", form(), 200, 120).unwrap();
    let commits = |app: &App<Fake>| app.platform.sent.iter().filter(|o| **o == 7).count();
    assert_eq!(commits(&app), 1);
    app.platform.event(&gui::WindowFocus {
        dropped: 0,
        window: id,
        focused: true,
    });
    app.platform.event(&key(keys::TAB));
    let mut typed = key(30);
    typed.text = zoraya_protocol::Below(u32::from('ж'));
    app.platform.event(&typed);
    // The field changed twice while the first commit was in flight.
    let h = app.wait();
    assert!(
        matches!(
            h,
            Happened::Ui {
                response: Response::Changed(1),
                ..
            }
        ),
        "{h:?}"
    );
    assert_eq!(
        commits(&app),
        1,
        "no second commit before the first's reply"
    );
    app.platform.frame();
    app.platform.event(&gui::CloseRequested { window: id });
    assert_eq!(app.wait(), Happened::CloseRequested(id));
    assert_eq!(commits(&app), 2, "the change committed after the reply");
    // The paint reached the buffer: not all zero.
    let painted = app
        .platform
        .buffers
        .iter()
        .any(|b| b.iter().any(|p| *p != 0));
    assert!(painted);
}

#[test]
fn a_configure_gives_a_new_buffer_and_pointer_events_reach_widgets() {
    let mut app = App::connect(Fake::default(), Appearance::Dark).unwrap();
    let root: Box<dyn Widget> = Box::new(Button::new(5, "OK"));
    let id = app.open("t", root, 120, 40).unwrap();
    app.platform.frame();
    app.platform.event(&gui::Configure {
        window: id,
        width: 200,
        height: 60,
        scale: 1,
        dark: true,
        motion: true,
    });
    app.platform.event(&gui::WindowPointer {
        dropped: 0,
        window: id,
        x: 10,
        y: 10,
        buttons: zoraya_protocol::Flags(gui::buttons::LEFT),
        serial: 1,
    });
    assert_eq!(
        app.wait(),
        Happened::Ui {
            window: id,
            response: Response::Activated(5)
        }
    );
    assert_eq!(app.platform.buffers.len(), 2);
    assert!(app.platform.buffers[0].is_empty(), "the old buffer freed");
    assert_eq!(app.platform.buffers[1].len(), 200 * 60);
    // A key with no focused window goes nowhere.
    app.platform.event(&gui::WindowFocus {
        dropped: 0,
        window: id,
        focused: false,
    });
    app.platform.event(&key(keys::ENTER));
    assert_eq!(app.wait(), Happened::Gone);
}

/// Mutants 7d: at scale 2 the buffer and each commit are in pixels, twice
/// the logical size each way.
#[test]
fn scale_two_buffers_and_commits_are_in_pixels() {
    let fake = Fake {
        scale: 2,
        ..Fake::default()
    };
    let mut app = App::connect(fake, Appearance::Light).unwrap();
    let id = app.open("t", form(), 30, 20).unwrap();
    assert_eq!(app.platform.buffers[0].len(), 60 * 40);
    assert_eq!(app.platform.committed, [(60, 40)]);
    app.platform.frame();
    app.platform.event(&gui::Configure {
        window: id,
        width: 50,
        height: 10,
        scale: 2,
        dark: false,
        motion: true,
    });
    assert_eq!(app.wait(), Happened::Gone);
    assert_eq!(app.platform.buffers[1].len(), 100 * 20);
    assert_eq!(app.platform.committed, [(60, 40), (100, 20)]);
}

/// Mutants 7d: a reply is matched by its ordinal and its transaction; a
/// stale one of the same ordinal waits in the queue.
#[test]
fn a_stale_reply_is_not_taken_for_the_answer() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    app.platform.inbox.push_back(reply(
        &gui::Created {
            status: zoraya_protocol::Below(status::OK),
            window: 77,
        },
        9999,
    ));
    let id = app.open("t", form(), 40, 20).unwrap();
    assert_eq!(id, 1);
}

/// Mutants 7d: two windows are each their own: the second is found, and
/// closing it frees its buffer and not the first's.
#[test]
fn two_windows_are_each_found() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    let a = app.open("a", form(), 40, 20).unwrap();
    let b = app.open("b", form(), 50, 20).unwrap();
    assert!(app.window(a).is_some());
    assert_eq!(app.window(b).unwrap().size.0, Scale::ONE.logical(50));
    app.close(b);
    assert!(app.platform.buffers[1].is_empty());
    assert_eq!(app.platform.buffers[0].len(), 40 * 20);
    assert!(app.window(a).is_some());
    assert!(app.window(b).is_none());
}

/// Mutants 7d: a Configure at the window's size or of zero width or
/// height gives no new buffer.
#[test]
fn a_configure_that_changes_nothing_is_ignored() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    let id = app.open("t", form(), 40, 20).unwrap();
    app.platform.frame();
    for (width, height) in [(40, 20), (0, 30), (30, 0)] {
        app.platform.event(&gui::Configure {
            window: id,
            width,
            height,
            scale: 1,
            dark: false,
            motion: true,
        });
    }
    assert_eq!(app.wait(), Happened::Gone);
    assert!(!app.platform.sent.contains(&8), "{:?}", app.platform.sent);
    assert_eq!(app.platform.buffers.len(), 1);
}

/// Mutants 7d: another window losing focus leaves the focused one's.
#[test]
fn another_window_losing_focus_keeps_the_focus() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    let a = app.open("a", form(), 200, 120).unwrap();
    let b = app.open("b", form(), 200, 120).unwrap();
    for (window, focused) in [(a, true), (b, false)] {
        app.platform.event(&gui::WindowFocus {
            dropped: 0,
            window,
            focused,
        });
    }
    app.platform.event(&key(keys::TAB));
    app.platform.event(&key(keys::TAB));
    app.platform.event(&key(keys::ENTER));
    assert_eq!(
        app.wait(),
        Happened::Ui {
            window: a,
            response: Response::Activated(2)
        }
    );
}

/// ADR-0170: a dropped entry is taken with its message, even when it came
/// while a reply was awaited, and given once; a drag starts only from the
/// window whose body holds the left button.
#[test]
fn drops_are_taken_and_drags_start_only_from_a_held_press() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    let id = app.open("t_open", form(), 200, 120).unwrap();
    let drop = gui::Drop {
        window: id,
        x: 5,
        y: 6,
        name: Data(b"notes.txt"),
    };
    app.platform.event_with(&drop, 77);
    assert_eq!(app.wait(), Happened::Dropped(id));
    let d = app.take_drop().unwrap();
    assert_eq!(
        (d.window, d.x, d.y, d.name.as_str(), d.entry),
        (id, 5, 6, "notes.txt", 77)
    );
    assert!(app.take_drop().is_none());
    // No press held: no request is sent.
    assert_eq!(app.drag(id, 5, "a"), Err(Some(5)));
    assert!(!app.platform.sent.contains(&13));
    let pointer = |buttons| gui::WindowPointer {
        dropped: 0,
        window: id,
        x: 10,
        y: 10,
        buttons: zoraya_protocol::Flags(buttons),
        serial: 3,
    };
    app.platform.event(&pointer(gui::buttons::LEFT));
    while !matches!(app.poll(), None | Some(Happened::Gone)) {}
    assert!(app.held(id));
    assert_eq!(app.drag(id, 5, &"x".repeat(65)), Err(Some(5)), "too long");
    assert_eq!(app.drag(id, 5, &"x".repeat(64)), Ok(()), "the longest");
    assert_eq!(app.drag(id, 5, "a"), Ok(()));
    assert_eq!(app.platform.drag_serials, [3, 3]);
    assert_eq!(app.platform.given.last(), Some(&5));
    app.platform.drag_status = status::BAD_RANGE;
    assert_eq!(app.drag(id, 6, "a"), Err(None));
    app.platform.event(&pointer(0));
    while !matches!(app.poll(), None | Some(Happened::Gone)) {}
    assert!(!app.held(id));
}

/// A sheet opens under its tag with the compositor's window id; a refused
/// one gives nothing and frees its buffer.
#[test]
fn a_sheet_is_opened_under_its_tag_and_its_id_returned() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    assert_eq!(app.open_sheet(4, 0, form(), 160, 80), Some(5));
    assert!(app.window(5).is_some());
    assert!(app.platform.sent.contains(&11));
    app.platform.refuse_sheet = true;
    let buffers = app.platform.buffers.len();
    assert_eq!(app.open_sheet(4, 0, form(), 160, 80), None);
    assert_eq!(app.platform.buffers.len(), buffers + 1);
    assert!(app.platform.buffers.last().unwrap().is_empty(), "freed");
}

/// A window's tree replaced is the one its values are read from.
#[test]
fn replacing_a_tree_shows_the_new_one() {
    let mut app = App::connect(Fake::default(), Appearance::Light).unwrap();
    let id = app.open("edit", form(), 200, 120).unwrap();
    app.replace(id, Box::new(TextField::new(1, "other.txt")));
    let w = app.window(id).unwrap();
    assert_eq!(w.value(1), zoraya_ui::Value::Text("other.txt"));
}

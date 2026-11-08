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
}

fn reply<'a, M: Message<'a>>(m: &M, t: u32) -> Vec<u8> {
    message(m, t)
}

impl Platform for Fake {
    type End = u32;

    fn send(&mut self, bytes: &[u8], _handle: Option<u32>) -> bool {
        let (h, _) = Header::decode(bytes).unwrap();
        self.sent.push(h.ordinal);
        let ok = zoraya_protocol::Below(status::OK);
        let t = h.transaction;
        match h.ordinal {
            4 => self.inbox.push_back(reply(
                &gui::DisplayScaleReply {
                    status: ok,
                    width: 1024,
                    height: 768,
                    scale: 1,
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
            7 => self.commits.push_back(t),
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
            _ => {}
        }
        true
    }

    fn receive(&mut self, bytes: &mut [u8]) -> Option<usize> {
        let m = self.inbox.pop_front()?;
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

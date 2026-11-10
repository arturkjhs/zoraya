//! The sheet against a fake compositor and a fake file service that
//! records every connection made: INV-0167-2 (nothing is chosen without
//! the person's key or press in the sheet) and INV-0167-3's host half (the
//! connection has exactly its kind's rights).

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_app::{App, Happened, Platform, Poll};
use zoraya_fsclient::{Client, Link};
use zoraya_proto_file::{self as fp, rights};
use zoraya_proto_gui as gui;
use zoraya_protocol::{Below, Flags, Header, Message, decode, encode};
use zoraya_tokens::Appearance;
use zoraya_ui::{Response, keys};

use crate::session::{CANCEL, Outcome, ROWS, Sheet, UP};
use crate::{Access, Ask, Browser};

fn message<'a, M: Message<'a>>(m: &M, t: u32) -> Vec<u8> {
    let mut out = vec![0u8; 8192];
    let n = encode(m, t, &mut out).unwrap();
    out.truncate(n);
    out
}

/// The compositor: answers at once; events come only when a test sends
/// them.
#[derive(Default)]
struct Gui {
    inbox: VecDeque<Vec<u8>>,
    buffers: Vec<Vec<u32>>,
    sheets: Vec<(u32, u32)>,
    sent: Vec<u32>,
}

impl Platform for Gui {
    type End = u32;

    fn send(&mut self, bytes: &[u8], _h: Option<u32>) -> bool {
        let (h, _) = Header::decode(bytes).unwrap();
        self.sent.push(h.ordinal);
        let ok = Below(gui::status::OK);
        let t = h.transaction;
        match h.ordinal {
            4 => self.inbox.push_back(message(
                &gui::DisplayScaleReply {
                    status: ok,
                    width: 1024,
                    height: 768,
                    scale: 1,
                },
                t,
            )),
            11 => {
                let (_, m) = decode::<gui::CreateSheet>(bytes, 1).unwrap();
                self.sheets.push((m.tag, m.kind.0));
                self.inbox.push_back(message(
                    &gui::SheetCreated {
                        status: ok,
                        window: 1,
                    },
                    t,
                ));
            }
            7 => self
                .inbox
                .push_back(message(&gui::CommitDone { status: ok }, t)),
            10 => self
                .inbox
                .push_back(message(&gui::CloseDone { status: ok }, t)),
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
        match self.receive(bytes) {
            Some(n) => Poll::Message(n),
            None => Poll::Empty,
        }
    }

    fn buffer(&mut self, pixels: usize) -> Option<(u32, usize)> {
        self.buffers.push(vec![0; pixels]);
        Some((500 + self.buffers.len() as u32, self.buffers.len() - 1))
    }

    fn pixels(&mut self, buffer: usize) -> &mut [u32] {
        &mut self.buffers[buffer]
    }

    fn free(&mut self, _buffer: usize) {}
}

/// A connection the fake service holds: its absolute path, its rights,
/// and the one name it is narrowed to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Conn {
    path: String,
    rights: u32,
    narrowed: Option<String>,
}

/// The volume: directories and files with sizes; and every connection.
struct Disk {
    tree: BTreeMap<String, Option<u64>>,
    conns: BTreeMap<u32, Conn>,
    next: u32,
    made: Vec<Conn>,
}

impl Disk {
    fn new() -> Disk {
        let mut tree = BTreeMap::new();
        for (p, size) in [
            ("/home", None),
            ("/home/project", None),
            ("/home/project/include", None),
            ("/home/project/main.c", Some(1900)),
            ("/home/project/plán.txt", Some(1200)),
            ("/home/notes.txt", Some(10)),
        ] {
            tree.insert(String::from(p), size);
        }
        let mut conns = BTreeMap::new();
        conns.insert(
            1,
            Conn {
                path: String::from("/"),
                rights: rights::ALL,
                narrowed: None,
            },
        );
        Disk {
            tree,
            conns,
            next: 100,
            made: Vec::new(),
        }
    }

    fn join(dir: &str, rel: &str) -> String {
        if rel.is_empty() {
            return String::from(dir);
        }
        if dir == "/" {
            alloc::format!("/{rel}")
        } else {
            alloc::format!("{dir}/{rel}")
        }
    }

    fn exists(&self, p: &str) -> bool {
        p == "/" || self.tree.contains_key(p)
    }
}

impl Link for Disk {
    type Handle = u32;

    fn call(
        &mut self,
        on: u32,
        request: &[u8],
        handles: &[u32],
        reply: &mut [u8],
        _got: &mut Vec<u32>,
    ) -> Option<usize> {
        let (h, _) = Header::decode(request).ok()?;
        let c = self.conns.get(&on)?.clone();
        let t = h.transaction;
        let done = |s: u32| {
            message(
                &fp::Done {
                    status: Below(s),
                    count: 0,
                },
                t,
            )
        };
        let out = match h.ordinal {
            1 => {
                let (_, m) = decode::<fp::Open<'_>>(request, 1).ok()?;
                let path = Disk::join(&c.path, m.path.0);
                let allowed = c.narrowed.as_deref().is_none_or(|n| n == m.path.0);
                let s = if !allowed || m.rights.0 & !c.rights != 0 {
                    fp::status::ACCESS_DENIED
                } else if !self.exists(&path) {
                    fp::status::NOT_FOUND
                } else {
                    let conn = Conn {
                        path,
                        rights: m.rights.0,
                        narrowed: None,
                    };
                    self.made.push(conn.clone());
                    self.conns.insert(handles[0], conn);
                    fp::status::OK
                };
                message(
                    &fp::OpenReply {
                        status: Below(s),
                        remaining: zoraya_protocol::Path(""),
                        target: zoraya_protocol::Text(""),
                    },
                    t,
                )
            }
            36 => {
                let (_, m) = decode::<fp::Narrow<'_>>(request, 1).ok()?;
                if m.rights.0 & !c.rights != 0 {
                    done(fp::status::ACCESS_DENIED)
                } else {
                    let conn = Conn {
                        path: c.path.clone(),
                        rights: m.rights.0,
                        narrowed: Some(String::from(m.name.0)),
                    };
                    self.made.push(conn.clone());
                    self.conns.insert(handles[0], conn);
                    done(fp::status::OK)
                }
            }
            10 => {
                let (_, m) = decode::<fp::ReadDirectory>(request, 0).ok()?;
                let prefix = if c.path == "/" {
                    String::from("/")
                } else {
                    alloc::format!("{}/", c.path)
                };
                let names: Vec<(String, u32)> = if m.cookie != 0 || c.narrowed.is_some() {
                    Vec::new()
                } else {
                    self.tree
                        .iter()
                        .filter_map(|(p, size)| {
                            let rest = p.strip_prefix(&prefix)?;
                            (!rest.contains('/')).then(|| {
                                let kind = if size.is_none() {
                                    fp::kind::DIRECTORY
                                } else {
                                    fp::kind::FILE
                                };
                                (String::from(rest), kind)
                            })
                        })
                        .collect()
                };
                let entries: Vec<fp::Entry<'_>> = names
                    .iter()
                    .map(|(n, k)| fp::Entry {
                        next: 0,
                        kind: Below(*k),
                        name: zoraya_protocol::Name(n),
                    })
                    .collect();
                message(
                    &fp::ReadDirectoryReply {
                        status: Below(fp::status::OK),
                        entries: zoraya_protocol::Seq::Of(&entries),
                    },
                    t,
                )
            }
            9 => {
                let size = self.tree.get(&c.path).copied().flatten().unwrap_or(0);
                message(
                    &fp::Attributes {
                        status: Below(fp::status::OK),
                        size,
                        modified: 0,
                        changed: 0,
                        mode: 0,
                        kind: Below(fp::kind::FILE),
                    },
                    t,
                )
            }
            _ => done(fp::status::UNSUPPORTED),
        };
        reply[..out.len()].copy_from_slice(&out);
        Some(out.len())
    }

    fn channel(&mut self) -> Option<(u32, u32)> {
        self.next += 1;
        Some((self.next, self.next))
    }

    fn close(&mut self, h: u32) {
        self.conns.remove(&h);
    }
}

struct Rig {
    app: App<Gui>,
    fs: Client<Disk>,
    sheet: Sheet,
}

fn rig(ask: Ask) -> Rig {
    rig_with(ask, &[])
}

/// The same, with more files in the volume.
fn rig_with(ask: Ask, files: &[&str]) -> Rig {
    let mut app = App::connect(Gui::default(), Appearance::Light).unwrap();
    let mut disk = Disk::new();
    for f in files {
        disk.tree.insert(String::from(*f), Some(1));
    }
    let mut fs = Client::new(disk);
    let browser = Browser::new(ask, "edit", "/home");
    let sheet = Sheet::open(&mut app, &mut fs, 1, browser, 4).unwrap();
    assert_eq!(app.platform.sheets, [(4, sheet.browser.ask.sheet())]);
    app.platform.inbox.push_back(message(
        &gui::WindowFocus {
            dropped: 0,
            window: 1,
            focused: true,
        },
        0,
    ));
    Rig { app, fs, sheet }
}

fn key(r: &mut Rig, code: u32, text: Option<char>) {
    let m = gui::Key {
        dropped: 0,
        code,
        pressed: true,
        modifiers: Flags(0),
        text: Below(text.map_or(0, u32::from)),
    };
    r.app.platform.inbox.push_back(message(&m, 0));
}

/// Runs the sheet over what the compositor has sent; its outcome, if any.
fn run(r: &mut Rig) -> Option<Outcome<u32>> {
    while let Some(h) = r.app.poll() {
        if let Some(o) = r.sheet.event(&mut r.app, &mut r.fs, 1, h) {
            return Some(o);
        }
    }
    None
}

/// INV-0167-2: with no input, a long run gives nothing and makes no
/// connection but the listing's read-only ones.
#[test]
fn nothing_is_chosen_without_the_persons_input() {
    let mut r = rig(Ask::Open {
        filters: Vec::new(),
    });
    for _ in 0..1000 {
        assert_eq!(run(&mut r), None);
    }
    assert!(
        r.fs.link
            .made
            .iter()
            .all(|c| c.rights == rights::READ && c.narrowed.is_none())
    );
    // Events for another window change nothing either.
    let m = gui::CloseRequested { window: 9 };
    r.app.platform.inbox.push_back(message(&m, 0));
    assert_eq!(run(&mut r), None);
}

#[test]
fn escape_and_the_owners_close_cancel() {
    let mut r = rig(Ask::Open {
        filters: Vec::new(),
    });
    key(&mut r, keys::ESC, None);
    assert_eq!(
        run(&mut r),
        Some(Outcome::Cancelled(zoraya_proto_chooser::status::CANCELLED))
    );
    let mut r = rig(Ask::Pick { write: false });
    let m = gui::CloseRequested { window: 1 };
    r.app.platform.inbox.push_back(message(&m, 0));
    assert!(matches!(run(&mut r), Some(Outcome::Cancelled(_))));
}

/// The person goes into `project`, picks `plán.txt` and presses Enter:
/// a read-only connection to that file.
#[test]
fn a_file_opened_by_keys_is_given_to_read() {
    let mut r = rig(Ask::Open {
        filters: Vec::new(),
    });
    assert_eq!(r.sheet.browser.dir(), "/home");
    // `project` is the first row: Down, Enter.
    key(&mut r, keys::DOWN, None);
    key(&mut r, keys::ENTER, None);
    assert_eq!(run(&mut r), None);
    assert_eq!(r.sheet.browser.dir(), "/home/project");
    // include, main.c, plán.txt: Down three times, Enter.
    for _ in 0..3 {
        key(&mut r, keys::DOWN, None);
    }
    key(&mut r, keys::ENTER, None);
    let Some(Outcome::Chosen { handle, grant }) = run(&mut r) else {
        panic!("not chosen");
    };
    assert_eq!(grant.access, Access::Read);
    assert_eq!(grant.line, "+r ~/project/plán.txt edit");
    let c = &r.fs.link.conns[&handle];
    assert_eq!(c.path, "/home/project/plán.txt");
    assert_eq!(c.rights, rights::READ | rights::CLONE);
}

/// Save: a name typed into the field, then Save: the folder narrowed to
/// that name with create's rights.
#[test]
fn a_name_saved_is_given_to_create_or_replace() {
    let mut r = rig(Ask::Save {
        suggested: String::from("new.txt"),
        filters: Vec::new(),
    });
    // From the list: Tab to the field, add a letter; Tab, Tab to Save.
    key(&mut r, keys::TAB, None);
    key(&mut r, keys::END, None);
    key(&mut r, keys::BACKSPACE, None);
    key(&mut r, keys::BACKSPACE, None);
    key(&mut r, keys::BACKSPACE, None);
    key(&mut r, 30, Some('m'));
    key(&mut r, 31, Some('d'));
    key(&mut r, keys::TAB, None);
    key(&mut r, keys::TAB, None);
    key(&mut r, keys::ENTER, None);
    let Some(Outcome::Chosen { handle, grant }) = run(&mut r) else {
        panic!("not chosen");
    };
    assert_eq!(grant.access, Access::Create(String::from("new.md")));
    let c = &r.fs.link.conns[&handle];
    assert_eq!(
        (c.path.as_str(), c.narrowed.as_deref()),
        ("/home", Some("new.md"))
    );
    assert_eq!(c.rights, Access::Create(String::new()).rights());
}

/// The sheet is laid out again at every change of the name, and the
/// field keeps its cursor: an edit in the middle of the name stays there.
#[test]
fn a_name_edited_in_the_middle_keeps_its_place() {
    let mut r = rig(Ask::Save {
        suggested: String::from("new.txt"),
        filters: Vec::new(),
    });
    key(&mut r, keys::TAB, None);
    // "new.txt": before the dot, insert "er", then delete the "w".
    for _ in 0..4 {
        key(&mut r, keys::LEFT, None);
    }
    key(&mut r, 18, Some('e'));
    key(&mut r, 19, Some('r'));
    for _ in 0..2 {
        key(&mut r, keys::LEFT, None);
    }
    key(&mut r, keys::BACKSPACE, None);
    key(&mut r, keys::ENTER, None);
    let Some(Outcome::Chosen { grant, .. }) = run(&mut r) else {
        panic!("not chosen");
    };
    assert_eq!(grant.access, Access::Create(String::from("neer.txt")));
}

/// PickDirectory with write: the folder with tree's rights.
#[test]
fn a_folder_picked_to_change_is_given_whole() {
    let mut r = rig(Ask::Pick { write: true });
    key(&mut r, keys::DOWN, None);
    // Tab to Cancel, then Choose.
    for _ in 0..2 {
        key(&mut r, keys::TAB, None);
    }
    key(&mut r, keys::ENTER, None);
    let Some(Outcome::Chosen { handle, grant }) = run(&mut r) else {
        panic!("not chosen");
    };
    assert_eq!(grant.access, Access::Tree);
    let c = &r.fs.link.conns[&handle];
    assert_eq!(c.path, "/home/project");
    assert_eq!(c.rights, Access::Tree.rights());
}

/// The list scrolls only as far as keeps the selection in view.
#[test]
fn the_list_scrolls_only_to_keep_the_selection_in_view() {
    let files: Vec<String> = (0..10).map(|i| alloc::format!("/home/f{i}.txt")).collect();
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    let mut r = rig_with(
        Ask::Open {
            filters: Vec::new(),
        },
        &names,
    );
    assert!(r.sheet.browser.entries.len() > ROWS as usize + 2);
    let keys_ = |r: &mut Rig, code: u32, n: usize| {
        for _ in 0..n {
            key(r, code, None);
        }
        assert_eq!(run(r), None);
    };
    // Down to row 7: rows 2 to 7 shown.
    keys_(&mut r, keys::DOWN, 8);
    assert_eq!(r.sheet.browser.selected, Some(7));
    assert_eq!(r.sheet.top(), 2);
    // Up to row 3, still in view: no scroll.
    keys_(&mut r, keys::UP, 4);
    assert_eq!(r.sheet.top(), 2);
    // Up to row 1, above it: it becomes the first.
    keys_(&mut r, keys::UP, 2);
    assert_eq!(r.sheet.browser.selected, Some(1));
    assert_eq!(r.sheet.top(), 1);
}

/// The sheet ends when the compositor goes, and answers only for its own
/// window.
#[test]
fn the_sheet_ends_when_its_compositor_is_gone_and_ignores_other_windows() {
    let mut r = rig(Ask::Open {
        filters: Vec::new(),
    });
    let other = Happened::Ui {
        window: 9,
        response: Response::Activated(CANCEL),
    };
    assert_eq!(r.sheet.event(&mut r.app, &mut r.fs, 1, other), None);
    assert_eq!(
        r.sheet.event(&mut r.app, &mut r.fs, 1, Happened::Gone),
        Some(Outcome::Cancelled(zoraya_proto_chooser::status::CANCELLED))
    );
}

/// The ‹ button goes one folder up.
#[test]
fn the_up_button_goes_one_folder_up() {
    let mut r = rig(Ask::Open {
        filters: Vec::new(),
    });
    key(&mut r, keys::DOWN, None);
    key(&mut r, keys::ENTER, None);
    assert_eq!(run(&mut r), None);
    assert_eq!(r.sheet.browser.dir(), "/home/project");
    let up = Happened::Ui {
        window: 1,
        response: Response::Activated(UP),
    };
    assert_eq!(r.sheet.event(&mut r.app, &mut r.fs, 1, up), None);
    assert_eq!(r.sheet.browser.dir(), "/home");
}

/// Open with a folder selected: its button enters the folder and gives
/// nothing.
#[test]
fn a_folder_selected_for_open_is_entered_by_the_open_button() {
    let mut r = rig(Ask::Open {
        filters: Vec::new(),
    });
    key(&mut r, keys::DOWN, None);
    // Tab to Cancel, then Open.
    for _ in 0..2 {
        key(&mut r, keys::TAB, None);
    }
    key(&mut r, keys::ENTER, None);
    assert_eq!(run(&mut r), None);
    assert_eq!(r.sheet.browser.dir(), "/home/project");
    assert!(r.fs.link.made.iter().all(|c| c.rights == rights::READ));
}

/// The sheet is laid out where a press finds it: a press down the
/// middle of the sheet reaches a row of the list before anything else.
#[test]
fn a_press_on_the_list_selects_its_row() {
    let mut r = rig(Ask::Open {
        filters: Vec::new(),
    });
    let mut found = None;
    for y in (0..crate::session::HEIGHT).step_by(4) {
        for buttons in [gui::buttons::LEFT, 0] {
            let m = gui::WindowPointer {
                dropped: 0,
                window: 1,
                x: crate::session::WIDTH / 2,
                y,
                buttons: Flags(buttons),
                serial: 1,
                modifiers: Flags(0),
            };
            r.app.platform.inbox.push_back(message(&m, 0));
        }
        assert_eq!(run(&mut r), None, "at {y}");
        if r.sheet.browser.selected.is_some() {
            found = Some(y);
            break;
        }
    }
    assert!(found.is_some(), "no press selects a row");
}

/// ADR-0245's condition (2): the sheet says its grant in its own words,
/// and in show-commands mode in `zosh`'s; `Chosen`'s words stay the same.
#[test]
fn show_commands_mode_shows_the_shells_line() {
    let mut r = rig(Ask::Save {
        suggested: String::from("new.txt"),
        filters: Vec::new(),
    });
    assert!(!r.sheet.commands);
    assert_eq!(r.sheet.shown_line(), "create or replace ~/new.txt");
    let none = r
        .sheet
        .event(&mut r.app, &mut r.fs, 1, Happened::Commands(true));
    assert!(none.is_none());
    assert!(r.sheet.commands);
    assert_eq!(
        r.sheet.shown_line(),
        "+w ~ edit \u{b7} gives less: create or replace new.txt"
    );
    r.sheet
        .event(&mut r.app, &mut r.fs, 1, Happened::Commands(false));
    assert_eq!(r.sheet.shown_line(), "create or replace ~/new.txt");
}

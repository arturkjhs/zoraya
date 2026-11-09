//! One open sheet: its widgets, its listing through the file client, and
//! the connection made only when the person confirms (INV-0167-2): no
//! path here answers `Chosen` without a press or a key the compositor
//! delivered to the sheet.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_app::{App, Happened, Platform};
use zoraya_fsclient::{Client, Link};
use zoraya_paint::Canvas;
use zoraya_proto_chooser::status as answer;
use zoraya_proto_file::{open_flags, rights};
use zoraya_tokens::{Logical, space};
use zoraya_ui::layout::{Column, Row};
use zoraya_ui::widgets::{Button, Label, ListView, TextField};
use zoraya_ui::{Event, LRect, Painter, Response, Value, Widget};

use crate::{Access, Ask, Browser, Entry, Grant, Step, size_text};

/// The sheet's size in logical pixels (the mockup's: 520 wide).
pub const WIDTH: u32 = 520;
pub const HEIGHT: u32 = 392;
/// Rows the list shows.
pub const ROWS: u32 = 6;
/// The most names a folder's listing shows.
pub const MAX_LISTED: usize = 512;

const UP: u32 = 1;
const LIST: u32 = 2;
const NAME: u32 = 3;
const CANCEL: u32 = 4;
const CONFIRM: u32 = 5;

/// How a sheet ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<H> {
    /// The person confirmed: the connection made with the grant's rights.
    Chosen { handle: H, grant: Grant },
    /// Nothing given, and why (`zoraya-proto-chooser`'s `status`).
    Cancelled(u32),
}

pub struct Sheet {
    pub browser: Browser,
    pub window: u32,
    /// The list's first shown row.
    top: usize,
    /// Where the name field's cursor is, kept across the sheet's rebuilds
    /// while the person types (review 7e, check of 7).
    name_cursor: Option<usize>,
}

impl Sheet {
    /// Lists the starting folder and opens the sheet under the window of
    /// the client tagged `tag`; `None` when the compositor refused it.
    pub fn open<P: Platform, L: Link>(
        app: &mut App<P>,
        fs: &mut Client<L>,
        root: L::Handle,
        browser: Browser,
        tag: u32,
    ) -> Option<Sheet> {
        let mut sheet = Sheet {
            browser,
            window: 0,
            top: 0,
            name_cursor: None,
        };
        sheet.list(fs, root);
        let kind = sheet.browser.ask.sheet();
        sheet.window = app.open_sheet(tag, kind, sheet.tree(), WIDTH, HEIGHT)?;
        // The keyboard starts on the list (after the Up button).
        if let Some(w) = app.window(sheet.window) {
            w.move_focus(false);
            w.move_focus(false);
        }
        Some(sheet)
    }

    /// Lists the browser's folder: its names, and each file's size.
    fn list<L: Link>(&mut self, fs: &mut Client<L>, root: L::Handle) {
        let dir = self.browser.dir();
        let mut entries = Vec::new();
        if let Ok(d) = fs.open(root, &dir, open_flags::DIRECTORY, rights::READ) {
            if let Ok(names) = fs.list(d) {
                for n in names.into_iter().take(MAX_LISTED) {
                    let size = if n.is_dir() {
                        0
                    } else {
                        match fs.open(d, &n.name, 0, rights::READ) {
                            Ok(f) => {
                                let size = fs.attributes(f).map_or(0, |a| a.0);
                                fs.close(f);
                                size
                            }
                            Err(_) => 0,
                        }
                    };
                    entries.push(Entry {
                        dir: n.is_dir(),
                        name: n.name,
                        size,
                    });
                }
            }
            fs.close(d);
        }
        self.browser.show(entries);
        self.top = 0;
    }

    /// The sheet's widgets for the browser's state.
    fn tree(&self) -> Box<dyn Widget> {
        let b = &self.browser;
        let mut list = ListView::new(
            LIST,
            &b.entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<&str>>(),
        );
        list.details = b
            .entries
            .iter()
            .map(|e| {
                if e.dir {
                    String::new()
                } else {
                    size_text(e.size)
                }
            })
            .collect();
        list.selected = b.selected;
        list.top = self.top;
        let mut column: Vec<Box<dyn Widget>> = vec![
            Box::new(Row::new(vec![
                Box::new(Button::new(UP, "‹")),
                Box::new(Label::new(&b.crumbs())),
            ])),
            Box::new(Sized {
                child: vec![Box::new(list)],
                height: space::CONTROL_HEIGHT.times(ROWS),
            }),
        ];
        if matches!(b.ask, Ask::Save { .. }) {
            column.push(Box::new(match self.name_cursor {
                Some(c) => TextField::with_cursor(NAME, &b.name, c),
                None => TextField::new(NAME, &b.name),
            }));
        }
        let (sentence, line) = match b.pending() {
            Some(g) => (b.sentence(), g.line),
            None => (b.sentence(), String::new()),
        };
        let mut s = Label::new(&sentence);
        s.secondary = true;
        column.push(Box::new(s));
        column.push(Box::new(Label::new(&line)));
        let verb = match b.ask {
            Ask::Open { .. } => "Open",
            Ask::Save { .. } => "Save",
            Ask::Pick { .. } => "Choose",
        };
        column.push(Box::new(Row::new(vec![
            Box::new(Button::new(CANCEL, "Cancel")),
            Box::new(Button::new(CONFIRM, verb)),
        ])));
        Box::new(Escapes(vec![Box::new(Column::new(column))]))
    }

    fn refresh<P: Platform>(&mut self, app: &mut App<P>) {
        // Keep the selection in view, as the list itself does.
        if let Some(i) = self.browser.selected {
            let rows = ROWS as usize;
            if i < self.top {
                self.top = i;
            } else if i >= self.top + rows {
                self.top = i + 1 - rows;
            }
        }
        app.replace(self.window, self.tree());
    }

    /// Acts on what happened; an `Outcome` ends the sheet (the caller
    /// closes its window).
    pub fn event<P: Platform, L: Link>(
        &mut self,
        app: &mut App<P>,
        fs: &mut Client<L>,
        root: L::Handle,
        happened: Happened,
    ) -> Option<Outcome<L::Handle>> {
        let response = match happened {
            Happened::Gone => return Some(Outcome::Cancelled(answer::CANCELLED)),
            Happened::CloseRequested(w) if w == self.window => {
                return Some(Outcome::Cancelled(answer::CANCELLED));
            }
            Happened::Ui { window, response } if window == self.window => response,
            _ => return None,
        };
        let step = match response {
            Response::Closed | Response::Activated(CANCEL) => {
                return Some(Outcome::Cancelled(answer::CANCELLED));
            }
            Response::Activated(UP) => self.browser.up(),
            Response::Changed(LIST) => {
                let picked = app.window(self.window).map(|w| w.value(LIST));
                if let Some(Value::Index(Some(i))) = picked {
                    self.browser.select(i);
                }
                self.refresh(app);
                Step::None
            }
            Response::Activated(LIST) => match self.browser.selected {
                Some(i) => self.browser.activate(i),
                None => Step::None,
            },
            Response::Changed(NAME) => {
                if let Some(w) = app.window(self.window)
                    && let Value::Text(t) = w.value(NAME)
                {
                    // One edit: an insertion leaves the cursor after the
                    // first character that differs; a deletion, at it.
                    let old = &self.browser.name;
                    let same = old
                        .chars()
                        .zip(t.chars())
                        .take_while(|(a, b)| a == b)
                        .count();
                    let grew = t.chars().count() > old.chars().count();
                    self.name_cursor = Some(same + usize::from(grew));
                    self.browser.name = String::from(t);
                }
                // The grant's line follows the name typed (review 7e, 7).
                self.refresh(app);
                Step::None
            }
            Response::Activated(NAME) | Response::Activated(CONFIRM) => {
                // A folder picked for Open or Save is entered, not given.
                match self
                    .browser
                    .selected
                    .and_then(|i| self.browser.entries.get(i))
                {
                    Some(e) if e.dir && !matches!(self.browser.ask, Ask::Pick { .. }) => {
                        let i = self.browser.selected.unwrap_or(0);
                        self.browser.activate(i)
                    }
                    _ => self.browser.confirm(),
                }
            }
            _ => Step::None,
        };
        match step {
            Step::None => None,
            Step::List => {
                self.list(fs, root);
                self.refresh(app);
                None
            }
            Step::Confirm(g) => Some(self.make(fs, root, g)),
        }
    }

    /// The connection for a confirmed grant, with exactly its rights.
    fn make<L: Link>(
        &mut self,
        fs: &mut Client<L>,
        root: L::Handle,
        g: Grant,
    ) -> Outcome<L::Handle> {
        let rights = g.rights();
        let made = match (&g.access, &g.name) {
            (Access::Create(name), _) => {
                match fs.open(root, &g.dir, open_flags::DIRECTORY, rights) {
                    Ok(d) => {
                        let h = fs.narrow(d, name, rights);
                        fs.close(d);
                        h
                    }
                    Err(e) => Err(e),
                }
            }
            (_, Some(name)) => {
                let path = if g.dir == "/" {
                    alloc::format!("/{name}")
                } else {
                    alloc::format!("{}/{name}", g.dir)
                };
                fs.open(root, &path, 0, rights)
            }
            (_, None) => fs.open(root, &g.dir, open_flags::DIRECTORY, rights),
        };
        match made {
            Ok(handle) => Outcome::Chosen { handle, grant: g },
            Err(_) => Outcome::Cancelled(answer::FAILED),
        }
    }
}

/// A container whose window closes on Escape (the sheet's Cancel).
struct Escapes(Vec<Box<dyn Widget>>);

impl Widget for Escapes {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        self.0[0].measure(p)
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.0[0].layout(r, p);
    }
    fn rect(&self) -> LRect {
        self.0[0].rect()
    }
    fn paint(&mut self, _p: &mut Painter, _c: &mut Canvas<'_>, _focused: bool) {}
    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.0
    }
    fn closes_on_escape(&self) -> bool {
        true
    }
    fn event(&mut self, _e: &Event) -> Response {
        Response::None
    }
}

/// A child at a fixed height (the list's rows).
struct Sized {
    child: Vec<Box<dyn Widget>>,
    height: Logical,
}

impl Widget for Sized {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        (self.child[0].measure(p).0, self.height)
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.child[0].layout(r, p);
    }
    fn rect(&self) -> LRect {
        self.child[0].rect()
    }
    fn paint(&mut self, _p: &mut Painter, _c: &mut Canvas<'_>, _focused: bool) {}
    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.child
    }
}

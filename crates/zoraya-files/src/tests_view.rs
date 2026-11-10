//! The view: what is under a point, and what each press, double click,
//! right-click and drag does through the widget; painting at both scales.

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::Cell;
use core::cell::RefCell;

use zoraya_tokens::{Appearance, Logical, Scale};
use zoraya_ui::{Event, LRect, Painter, Response, Widget};

use crate::model::{Column, Focus, MenuItem, mods};
use crate::view::{DOUBLE_NS, FilesView, HEADER_H, Hit, ROW_H, SIDEBAR_W, TOOLBAR_H};
use crate::*;

std::thread_local! {
    /// Each test's own clock: tests run on threads of their own.
    static CLOCK: Cell<u64> = const { Cell::new(1_000_000_000) };
}

fn clock() -> u64 {
    CLOCK.with(Cell::get)
}

fn later(ns: u64) {
    CLOCK.with(|c| c.set(c.get() + ns));
}

const HOME: &str = "/home/ann";

fn item(name: &str, dir: bool, size: u64) -> Item {
    Item {
        name: String::from(name),
        dir,
        size,
        modified: 0,
    }
}

fn l(v: i32) -> Logical {
    Scale::ONE.logical(v as u32)
}

struct Rig {
    view: FilesView,
    model: Rc<RefCell<Model>>,
    painter: Painter,
}

/// The window at `k` (1 or 2), 880 by 536 logical, over the mockup's
/// folder.
fn rig(k: u32) -> Rig {
    let holds = vec![
        (String::from("/"), false),
        (String::from("~/project"), true),
        (String::from("~/include"), false),
    ];
    let dir = "/home/ann/project";
    let mut m = Model::new(dir, HOME, Some("project"), places(&holds, HOME));
    m.show(
        dir,
        vec![
            item("docs", true, 0),
            item("include", true, 0),
            item("main.c", false, 1900),
            item("notes.txt", false, 2100),
            item("plán.txt", false, 1200),
        ],
    );
    let model = Rc::new(RefCell::new(m));
    let scale = Scale::new(k).unwrap();
    let mut painter = Painter::new(scale, Appearance::Light);
    let mut view = FilesView::new(9, model.clone(), clock);
    view.layout(LRect::new(l(0), l(0), l(880), l(536)), &mut painter);
    Rig {
        view,
        model,
        painter,
    }
}

impl Rig {
    /// A pointer event at logical `(x, y)`.
    fn at(&mut self, x: i32, y: i32, buttons: u32, modifiers: u32) -> Response {
        self.view.event(&Event::Pointer {
            x: l(x),
            y: l(y),
            buttons,
            modifiers,
        })
    }

    fn click(&mut self, x: i32, y: i32, modifiers: u32) -> Response {
        let r = self.at(x, y, 1, modifiers);
        self.at(x, y, 0, modifiers);
        r
    }

    fn acts(&mut self) -> Vec<Act> {
        self.model.borrow_mut().acts.drain(..).collect()
    }

    /// The centre of row `pos` (the list's first row at the top).
    fn row(pos: i32) -> (i32, i32) {
        (
            SIDEBAR_W + 200,
            TOOLBAR_H + HEADER_H + 4 + pos * ROW_H + ROW_H / 2,
        )
    }

    fn paint(&mut self) -> Vec<u32> {
        let k = self.painter.scale.factor() as usize;
        let (w, h) = (880 * k, 536 * k);
        let mut px = vec![0u32; w * h];
        let mut c = zoraya_paint::Canvas::new(&mut px, w);
        self.view.paint(&mut self.painter, &mut c, true);
        px
    }
}

#[test]
fn the_parts_are_where_the_mockup_draws_them() {
    let r = rig(1);
    let k = 1;
    assert_eq!(r.view.hit(12 + 14, 24), Hit::Back);
    assert_eq!(r.view.hit(12 + 28 + 4 + 14, 24), Hit::Forward);
    assert_eq!(r.view.hit(12 + 2 * 28 + 14 + 14, 24), Hit::Up);
    assert_eq!(r.view.hit(880 - 12 - 100, 24), Hit::Search);
    let (x, y) = Rig::row(0);
    assert_eq!(r.view.hit(x * k, y * k), Hit::Row(0));
    assert_eq!(r.view.hit(x, Rig::row(4).1), Hit::Row(4));
    assert_eq!(r.view.hit(x, Rig::row(7).1), Hit::ListEnd);
    assert_eq!(
        r.view.hit(SIDEBAR_W + 60, TOOLBAR_H + 10),
        Hit::Header(Column::Name)
    );
    assert_eq!(
        r.view.hit(880 - 180, TOOLBAR_H + 10),
        Hit::Header(Column::Size)
    );
    assert_eq!(
        r.view.hit(880 - 100, TOOLBAR_H + 10),
        Hit::Header(Column::Modified)
    );
    // The places: Home, project, include, then /system under System.
    assert_eq!(r.view.hit(60, TOOLBAR_H + 34 + 14), Hit::Place(0));
    assert_eq!(r.view.hit(60, TOOLBAR_H + 34 + 30 + 14), Hit::Place(1));
    assert_eq!(r.view.hit(60, TOOLBAR_H + 34 + 60 + 14), Hit::Place(2));
    assert_eq!(r.view.hit(60, TOOLBAR_H + 34 + 90 + 30 + 14), Hit::Place(3));
    // Scale 2: twice the pixels, the same parts.
    let r2 = rig(2);
    assert_eq!(r2.view.hit(2 * x, 2 * y), Hit::Row(0));
    assert_eq!(r2.view.hit(2 * 26, 2 * 24), Hit::Back);
}

#[test]
fn a_click_selects_and_a_double_click_opens() {
    let mut r = rig(1);
    let (x, y) = Rig::row(3);
    assert_eq!(r.click(x, y, 0), Response::Repaint);
    assert_eq!(r.model.borrow().selected_names(), ["notes.txt"]);
    later(DOUBLE_NS / 2);
    assert_eq!(r.click(x, y, 0), Response::Activated(9));
    assert_eq!(
        r.acts(),
        [Act::Open(String::from("/home/ann/project/notes.txt"))]
    );
    // Too slow: two clicks.
    later(DOUBLE_NS * 2);
    r.click(x, y, 0);
    later(DOUBLE_NS + 1);
    assert_eq!(r.click(x, y, 0), Response::Repaint);
    // A folder opens on a double click.
    let (fx, fy) = Rig::row(0);
    later(DOUBLE_NS * 2);
    r.click(fx, fy, 0);
    later(1000);
    r.click(fx, fy, 0);
    assert_eq!(
        r.acts(),
        [Act::List(String::from("/home/ann/project/docs"))]
    );
}

#[test]
fn shift_and_command_clicks_select_through_the_widget() {
    let mut r = rig(1);
    later(DOUBLE_NS * 2);
    r.click(Rig::row(2).0, Rig::row(2).1, 0);
    later(DOUBLE_NS * 2);
    r.click(Rig::row(4).0, Rig::row(4).1, mods::SHIFT);
    assert_eq!(
        r.model.borrow().selected_names(),
        ["main.c", "notes.txt", "plán.txt"]
    );
    later(DOUBLE_NS * 2);
    r.click(Rig::row(3).0, Rig::row(3).1, mods::SYSTEM);
    assert_eq!(r.model.borrow().selected_names(), ["main.c", "plán.txt"]);
    // A plain click on a selected row of several: that one alone, at the
    // release.
    later(DOUBLE_NS * 2);
    r.click(Rig::row(4).0, Rig::row(4).1, 0);
    assert_eq!(r.model.borrow().selected_names(), ["plán.txt"]);
}

#[test]
fn a_press_moved_from_a_row_drags_its_file() {
    let mut r = rig(1);
    let (x, y) = Rig::row(3);
    later(DOUBLE_NS * 2);
    r.at(x, y, 1, 0);
    assert_eq!(r.at(x + 2, y, 1, 0), Response::None, "not yet");
    assert_eq!(r.at(x + 10, y + 3, 1, 0), Response::Activated(9));
    assert_eq!(
        r.acts(),
        [Act::Drag {
            path: String::from("/home/ann/project/notes.txt"),
            name: String::from("notes.txt")
        }]
    );
    // Once per press.
    assert_eq!(r.at(x + 30, y + 3, 1, 0), Response::None);
    r.at(x + 30, y + 3, 0, 0);
}

#[test]
fn the_toolbar_header_crumbs_and_places_act() {
    let mut r = rig(1);
    // A header sorts, again reverses.
    later(DOUBLE_NS * 2);
    r.click(880 - 100, TOOLBAR_H + 10, 0);
    assert_eq!(r.model.borrow().sort, Column::Modified);
    // A place goes there; back returns.
    r.click(60, TOOLBAR_H + 34 + 60 + 14, 0);
    assert_eq!(r.acts(), [Act::List(String::from("/home/ann/include"))]);
    r.model.borrow_mut().show("/home/ann/include", Vec::new());
    r.click(26, 24, 0);
    assert_eq!(r.acts(), [Act::List(String::from("/home/ann/project"))]);
    r.model.borrow_mut().show("/home/ann/project", Vec::new());
    // Up, and the first crumb (painted first to measure it).
    r.click(12 + 2 * 28 + 14 + 14, 24, 0);
    assert_eq!(r.acts(), [Act::List(String::from(HOME))]);
    r.model.borrow_mut().show("/home/ann/project", Vec::new());
    r.paint();
    let crumbs = r.view.hit(122 + 12, 24);
    assert_eq!(crumbs, Hit::Crumb(0));
    r.click(122 + 12, 24, 0);
    assert_eq!(r.acts(), [Act::List(String::from(HOME))]);
    // The search field takes the keys.
    r.click(880 - 100, 24, 0);
    assert_eq!(r.model.borrow().focus, Focus::Search);
}

#[test]
fn a_right_click_opens_the_menu_and_an_item_acts() {
    let mut r = rig(1);
    let (x, y) = Rig::row(3);
    r.at(x, y, 2, 0);
    r.at(x, y, 0, 0);
    let menu = r.model.borrow().menu.unwrap();
    assert_eq!(
        menu.item.map(|i| r.model.borrow().items[i].name.clone()),
        Some(String::from("notes.txt"))
    );
    r.paint();
    // The items from the menu's top: Open, Open in edit, (group) Rename...
    let top = y.min(536 - 221);
    let open = (x + 30, top + 6 + 13);
    assert_eq!(r.view.hit(open.0, open.1), Hit::MenuItem(MenuItem::Open));
    let copy = (x + 30, top + 6 + 2 * 26 + 9 + 2 * 26 + 9 + 26 + 9 + 13);
    assert_eq!(
        r.view.hit(copy.0, copy.1),
        Hit::MenuItem(MenuItem::CopyPath)
    );
    later(DOUBLE_NS * 2);
    assert_eq!(r.click(copy.0, copy.1, 0), Response::Activated(9));
    assert_eq!(
        r.acts(),
        [Act::CopyPath(String::from("~/project/notes.txt"))]
    );
    // A press outside a menu closes it and does nothing else.
    r.at(x, y, 2, 0);
    r.at(x, y, 0, 0);
    later(DOUBLE_NS * 2);
    r.click(26, 24, 0);
    assert!(r.model.borrow().menu.is_none());
    assert!(r.acts().is_empty());
}

#[test]
fn the_delete_card_answers_with_its_buttons() {
    let mut r = rig(1);
    later(DOUBLE_NS * 2);
    r.click(Rig::row(3).0, Rig::row(3).1, 0);
    r.view.event(&Event::Key {
        code: crate::model::keys::DELETE,
        text: None,
        modifiers: 0,
    });
    assert!(r.model.borrow().confirm.is_some());
    // The card is the only thing that answers.
    assert_eq!(r.view.hit(26, 24), Hit::Nothing);
    let (cx, cy) = (SIDEBAR_W / 2 + (880 - 380) / 2, 150);
    let delete = (cx + 380 - 20 - 48, cy + 168 - 20 - 14);
    let cancel = (delete.0 - 96 - 8, delete.1);
    assert_eq!(r.view.hit(cancel.0, cancel.1), Hit::CardCancel);
    r.click(cancel.0, cancel.1, 0);
    assert!(r.model.borrow().confirm.is_none() && r.acts().is_empty());
    r.model.borrow_mut().ask_delete();
    assert_eq!(r.click(delete.0, delete.1, 0), Response::Activated(9));
    assert_eq!(
        r.acts(),
        [Act::Remove {
            dir: String::from("/home/ann/project"),
            names: vec![String::from("notes.txt")]
        }]
    );
}

#[test]
fn keys_reach_the_model_and_the_list_follows_the_cursor() {
    let mut r = rig(1);
    let mut items = Vec::new();
    for i in 0..40 {
        items.push(item(&alloc::format!("f{i:02}.txt"), false, 10));
    }
    r.model.borrow_mut().show("/home/ann/project", items);
    for _ in 0..30 {
        r.view.event(&Event::Key {
            code: crate::model::keys::DOWN,
            text: None,
            modifiers: 0,
        });
    }
    r.paint();
    // The cursor's row is shown: a hit on the last visible row is it.
    let rows = r.view.rows() as i32;
    let (x, y) = Rig::row(rows - 1);
    assert_eq!(r.view.hit(x, y), Hit::Row(29));
    let resp = r.view.event(&Event::Key {
        code: crate::model::keys::ENTER,
        text: None,
        modifiers: 0,
    });
    assert_eq!(resp, Response::Activated(9));
}

#[test]
fn it_paints_at_both_scales_with_the_selection_drawn() {
    for k in [1u32, 2] {
        let mut r = rig(k);
        later(DOUBLE_NS * 2);
        r.click(Rig::row(3).0, Rig::row(3).1, 0);
        let px = r.paint();
        let w = 880 * k as usize;
        let sel = r.painter.palette.bg_selection.pixel();
        let (x, y) = (SIDEBAR_W as usize + 300, Rig::row(3).1 as usize);
        assert_eq!(px[(y * k as usize) * w + x * k as usize], sel, "scale {k}");
        // The status line's ground and the toolbar's are the window's.
        let win = r.painter.palette.bg_window.pixel();
        assert_eq!(px[(536 * k as usize - 2) * w + 400 * k as usize], win);
        // The menu and the card paint over it.
        r.model.borrow_mut().open_menu(300, 200, Some(3));
        r.model.borrow_mut().commands = true;
        let px = r.paint();
        assert_eq!(
            px[((200 + 2) * k as usize) * w + (300 + 200) * k as usize],
            win
        );
        r.model.borrow_mut().menu = None;
        r.model.borrow_mut().ask_delete();
        let _ = r.paint();
    }
}

#[test]
fn a_drops_place_names_its_row() {
    let r = rig(1);
    let m = r.model.borrow();
    let (x, y) = Rig::row(1);
    assert_eq!(crate::view::row_at(&m, x as u32, y as u32), Some(1));
    assert_eq!(crate::view::row_at(&m, 20, y as u32), None, "the sidebar");
    assert_eq!(crate::view::row_at(&m, x as u32, 10), None, "the toolbar");
    let (_, far) = Rig::row(9);
    assert_eq!(
        crate::view::row_at(&m, x as u32, far as u32),
        None,
        "below the rows"
    );
}

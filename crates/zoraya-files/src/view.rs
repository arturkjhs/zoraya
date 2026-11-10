//! The file manager's window as one `zoraya-ui` widget over a shared
//! [`Model`], drawn as the approved mockups draw it: the toolbar (back,
//! forward, up, the path as crumbs, search), the sidebar of places with
//! what each allows, the list with its sortable header, the status line, and
//! over them the context menu, the delete card and the rename field. Every
//! gesture becomes a call of the model; the program reads the model's
//! [`Act`](crate::Act)s after an `Activated(id)`.
//!
//! Geometry is in logical pixels times the scale (`k`), as the mockup's.

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use zoraya_paint::{self as paint, Canvas};
use zoraya_text::{Rect as TRect, Target, Text};
use zoraya_tokens::{Color, Logical, shape, space, text};
use zoraya_ui::{Event, LRect, Painter, Response, Value, Widget};

use crate::model::{Column, Focus, MENU, MenuItem, Model, mods};

/// The mockup's measures, logical.
pub const TOOLBAR_H: i32 = 48;
pub const SIDEBAR_W: i32 = 184;
pub const HEADER_H: i32 = 28;
pub const ROW_H: i32 = 28;
pub const STATUS_H: i32 = 28;
pub const SEARCH_W: i32 = 200;
pub const BUTTON: i32 = 28;
/// The menu's rows, its width, and its width in show-commands mode.
pub const MENU_ROW: i32 = 26;
pub const MENU_W: i32 = 236;
pub const MENU_W_COMMANDS: i32 = 430;
/// The delete card.
pub const CARD_W: i32 = 380;
pub const CARD_H: i32 = 168;
/// Two presses on one row within this many nanoseconds are a double click.
pub const DOUBLE_NS: u64 = 400_000_000;
/// A press that moves this far (logical) with the button held drags.
pub const DRAG_START: i32 = 4;

const LEFT: u32 = 1;
const RIGHT: u32 = 2;

/// What is under a point of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Back,
    Forward,
    Up,
    Crumb(usize),
    Search,
    Place(usize),
    Header(Column),
    /// A row of the list, by view position.
    Row(usize),
    /// The list below its last row.
    ListEnd,
    MenuItem(MenuItem),
    /// Inside the menu, not on an item.
    Menu,
    CardCancel,
    CardDelete,
    /// Inside the card, not on a button.
    Card,
    Nothing,
}

pub struct FilesView {
    pub id: u32,
    pub model: Rc<RefCell<Model>>,
    /// The monotonic clock, for double clicks.
    pub clock: fn() -> u64,
    rect: LRect,
    /// The scale's factor at the last layout.
    k: i32,
    /// The window's body, physical.
    body: paint::Rect,
    buttons: u32,
    /// The last left press: where, on which row, when.
    press: Option<(i32, i32, Option<usize>, u64)>,
    /// The press before it, for a double click: on which item (by name,
    /// in which folder) and when.
    last_press: Option<(String, u64)>,
    dragged: bool,
    /// The crumbs' boxes from the last paint, physical x ranges.
    crumbs: Vec<(i32, i32)>,
}

fn mix(a: Color, b: Color, t: u32) -> Color {
    // t in 1/1000.
    let ch = |x: u8, y: u8| -> u32 { (u32::from(x) * (1000 - t) + u32::from(y) * t + 500) / 1000 };
    Color::of_pixel(ch(a.r(), b.r()) << 16 | ch(a.g(), b.g()) << 8 | ch(a.b(), b.b()))
}

fn prect(x: i32, y: i32, w: i32, h: i32) -> paint::Rect {
    paint::Rect::new(x, y, w.max(0), h.max(0))
}

/// Draws `s` with its pen at `(x, baseline)`, clipped to `clip`.
#[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
fn draw_text(
    c: &mut Canvas<'_>,
    t: &mut Text<'static>,
    s: &str,
    px: u32,
    x: i32,
    baseline: i32,
    color: Color,
    clip: paint::Rect,
) -> i32 {
    let clip = clip.intersect(&c.clip);
    if clip.is_empty() {
        return 0;
    }
    let mut target = Target {
        pixels: c.pixels,
        stride: c.width,
        clip: TRect::new(clip.x, clip.y, clip.w as u32, clip.h as u32),
    };
    t.draw(&mut target, s, px, x, baseline, color)
}

/// The baseline that centres text of `px` in a box of height `h` at `y`.
fn baseline(y: i32, h: i32, px: u32) -> i32 {
    y + h / 2 + px as i32 * 7 / 20
}

impl FilesView {
    pub fn new(id: u32, model: Rc<RefCell<Model>>, clock: fn() -> u64) -> FilesView {
        FilesView {
            id,
            model,
            clock,
            rect: LRect::default(),
            k: 1,
            body: paint::Rect::new(0, 0, 0, 0),
            buttons: 0,
            press: None,
            last_press: None,
            dragged: false,
            crumbs: Vec::new(),
        }
    }

    fn list_rect(&self) -> paint::Rect {
        let (b, k) = (self.body, self.k);
        prect(
            b.x + SIDEBAR_W * k,
            b.y + TOOLBAR_H * k,
            b.w - SIDEBAR_W * k,
            b.h - (TOOLBAR_H + STATUS_H) * k,
        )
    }

    /// How many rows the list shows.
    pub fn rows(&self) -> usize {
        let l = self.list_rect();
        ((l.h - (HEADER_H + 4) * self.k) / (ROW_H * self.k)).max(1) as usize
    }

    /// Keeps the cursor's row in view.
    fn follow(&mut self) {
        let rows = self.rows();
        let mut m = self.model.borrow_mut();
        let n = m.view().len();
        if let Some(c) = m.cursor {
            if c < m.top {
                m.top = c;
            } else if c >= m.top + rows {
                m.top = c + 1 - rows;
            }
        }
        m.top = m.top.min(n.saturating_sub(rows));
        m.visible_rows = rows;
    }

    fn menu_rect(&self, m: &Model) -> Option<paint::Rect> {
        let menu = m.menu?;
        let k = self.k;
        let w = if m.commands { MENU_W_COMMANDS } else { MENU_W } * k;
        let groups = MENU.iter().filter(|i| i.starts_group()).count() as i32;
        let h = (12 + MENU_ROW * MENU.len() as i32 + groups * 9) * k;
        let b = self.body;
        // Kept inside the body.
        let x = (b.x + menu.x as i32 * k).min(b.x + b.w - w).max(b.x);
        let y = (b.y + menu.y as i32 * k).min(b.y + b.h - h).max(b.y);
        Some(prect(x, y, w, h))
    }

    /// The menu items' rows, physical.
    fn menu_rows(&self, r: paint::Rect) -> Vec<(MenuItem, paint::Rect)> {
        let k = self.k;
        let mut y = r.y + 6 * k;
        let mut out = Vec::new();
        for m in MENU {
            if m.starts_group() {
                y += 9 * k;
            }
            out.push((m, prect(r.x + 4 * k, y, r.w - 8 * k, MENU_ROW * k)));
            y += MENU_ROW * k;
        }
        out
    }

    fn card_rect(&self) -> paint::Rect {
        let (b, k) = (self.body, self.k);
        let (w, h) = (CARD_W * k, CARD_H * k);
        prect(b.x + (b.w - w) / 2 + SIDEBAR_W * k / 2, b.y + 150 * k, w, h)
    }

    fn card_buttons(&self) -> (paint::Rect, paint::Rect) {
        let r = self.card_rect();
        let k = self.k;
        let (bw, bh, pad) = (96 * k, 28 * k, 20 * k);
        let by = r.y + r.h - pad - bh;
        let del = prect(r.x + r.w - pad - bw, by, bw, bh);
        let cancel = prect(del.x - bw - 8 * k, by, bw, bh);
        (cancel, del)
    }

    /// The buttons and fields of the toolbar, physical.
    fn toolbar_parts(&self) -> [paint::Rect; 5] {
        let (b, k) = (self.body, self.k);
        let y = b.y + (TOOLBAR_H - BUTTON) * k / 2;
        let s = BUTTON * k;
        let back = prect(b.x + 12 * k, y, s, s);
        let fwd = prect(back.x + s + 4 * k, y, s, s);
        let up = prect(fwd.x + s + 10 * k, y, s, s);
        let search = prect(b.x + b.w - 12 * k - SEARCH_W * k, y, SEARCH_W * k, s);
        let cx = up.x + s + 12 * k;
        let crumbs = prect(cx, y, search.x - 10 * k - cx, s);
        [back, fwd, up, crumbs, search]
    }

    /// The sidebar's rows: place index and rectangle.
    fn place_rows(&self, m: &Model) -> Vec<(usize, paint::Rect)> {
        let (b, k) = (self.body, self.k);
        let x = b.x + 6 * k;
        let w = SIDEBAR_W * k - 13 * k;
        let mut y = b.y + TOOLBAR_H * k + 14 * k + 20 * k;
        let mut out = Vec::new();
        let mut system = false;
        for (i, p) in m.places.iter().enumerate() {
            if p.path == "/" {
                continue;
            }
            if p.system && !system {
                y += 10 * k + 20 * k;
                system = true;
            }
            out.push((i, prect(x, y, w, ROW_H * k)));
            y += (ROW_H + 2) * k;
        }
        out
    }

    fn header_columns(&self) -> (i32, i32) {
        let l = self.list_rect();
        let k = self.k;
        (l.x + l.w - 170 * k, l.x + l.w - 150 * k)
    }

    /// What is at physical `(x, y)`.
    pub fn hit(&self, x: i32, y: i32) -> Hit {
        let m = self.model.borrow();
        let k = self.k;
        if m.confirm.is_some() {
            let r = self.card_rect();
            let (cancel, del) = self.card_buttons();
            return if cancel.contains(x, y) {
                Hit::CardCancel
            } else if del.contains(x, y) {
                Hit::CardDelete
            } else if r.contains(x, y) {
                Hit::Card
            } else {
                Hit::Nothing
            };
        }
        if let Some(r) = self.menu_rect(&m)
            && r.contains(x, y)
        {
            return self
                .menu_rows(r)
                .into_iter()
                .find(|(_, row)| row.contains(x, y))
                .map_or(Hit::Menu, |(i, _)| Hit::MenuItem(i));
        }
        let [back, fwd, up, crumbs, search] = self.toolbar_parts();
        if back.contains(x, y) {
            return Hit::Back;
        }
        if fwd.contains(x, y) {
            return Hit::Forward;
        }
        if up.contains(x, y) {
            return Hit::Up;
        }
        if search.contains(x, y) {
            return Hit::Search;
        }
        if crumbs.contains(x, y) {
            return self
                .crumbs
                .iter()
                .position(|(a, z)| x >= *a && x < *z)
                .map_or(Hit::Nothing, Hit::Crumb);
        }
        for (i, r) in self.place_rows(&m) {
            if r.contains(x, y) {
                return Hit::Place(i);
            }
        }
        let l = self.list_rect();
        if l.contains(x, y) {
            if y < l.y + HEADER_H * k {
                let (size_right, modified_x) = self.header_columns();
                return Hit::Header(if x >= modified_x - 12 * k {
                    Column::Modified
                } else if x >= size_right - 60 * k {
                    Column::Size
                } else {
                    Column::Name
                });
            }
            let rel = y - (l.y + (HEADER_H + 4) * k);
            if rel < 0 {
                return Hit::ListEnd;
            }
            let pos = m.top + (rel / (ROW_H * k)) as usize;
            return if pos < m.view().len() && (rel / (ROW_H * k)) < self.rows() as i32 {
                Hit::Row(pos)
            } else {
                Hit::ListEnd
            };
        }
        Hit::Nothing
    }

    /// Every change is the program's to see: it carries out what the model
    /// asks, if anything, and tells the serial line the status line.
    fn answer(&self) -> Response {
        Response::Activated(self.id)
    }

    /// A left press at physical `(x, y)` with `modifiers`.
    fn left_press(&mut self, x: i32, y: i32, modifiers: u32) -> Response {
        let now = (self.clock)();
        let hit = self.hit(x, y);
        let row = match hit {
            Hit::Row(p) => Some(p),
            _ => None,
        };
        self.press = Some((x, y, row, now));
        self.dragged = false;
        let mut m = self.model.borrow_mut();
        // A press outside the menu closes it; one outside the rename field
        // keeps the name.
        if m.menu.is_some() && !matches!(hit, Hit::MenuItem(_) | Hit::Menu) {
            m.menu = None;
            return Response::Activated(self.id);
        }
        match hit {
            Hit::Back => m.back(),
            Hit::Forward => m.forward(),
            Hit::Up => m.up(),
            Hit::Crumb(i) => {
                if let Some((_, path)) = m.crumbs().get(i).cloned() {
                    m.navigate(&path);
                }
            }
            Hit::Search => {
                m.commit_rename();
                m.focus = Focus::Search;
            }
            Hit::Place(i) => {
                let path = m.places[i].path.clone();
                m.commit_rename();
                m.navigate(&path);
            }
            Hit::Header(c) => m.sort_by(c),
            Hit::Row(p) => {
                let id = m
                    .item_at(p)
                    .map(|it| alloc::format!("{}/{}", m.dir, it.name))
                    .unwrap_or_default();
                let double = self
                    .last_press
                    .as_ref()
                    .is_some_and(|(q, at)| *q == id && now.saturating_sub(*at) <= DOUBLE_NS)
                    && modifiers & (mods::SHIFT | mods::COMMAND) == 0;
                if double {
                    self.last_press = None;
                    m.activate(p);
                } else {
                    self.last_press = Some((id, now));
                    // A press on a selected row keeps the selection, so that
                    // a drag carries it; the click alone happens at release.
                    if !(m.is_selected(p) && modifiers & (mods::SHIFT | mods::COMMAND) == 0) {
                        m.click(p, modifiers);
                    } else {
                        m.cursor = Some(p);
                    }
                }
            }
            Hit::ListEnd => m.click(usize::MAX, modifiers),
            Hit::MenuItem(i) => m.choose(i),
            Hit::CardCancel => m.answer_delete(false),
            Hit::CardDelete => m.answer_delete(true),
            Hit::Menu | Hit::Card | Hit::Nothing => {}
        }
        drop(m);
        self.answer()
    }

    fn right_press(&mut self, x: i32, y: i32) -> Response {
        let hit = self.hit(x, y);
        let k = self.k;
        let (lx, ly) = ((x - self.body.x) / k, (y - self.body.y) / k);
        let mut m = self.model.borrow_mut();
        if m.confirm.is_some() {
            return Response::None;
        }
        match hit {
            Hit::Row(p) => m.open_menu(lx as u32, ly as u32, Some(p)),
            Hit::ListEnd => m.open_menu(lx as u32, ly as u32, None),
            _ => m.menu = None,
        }
        Response::Activated(self.id)
    }

    fn pointer(&mut self, x: Logical, y: Logical, buttons: u32, modifiers: u32) -> Response {
        let k = self.k;
        let s = zoraya_tokens::Scale::new(k as u32).unwrap_or(zoraya_tokens::Scale::ONE);
        let (x, y) = (x.physical(s) as i32, y.physical(s) as i32);
        let was = self.buttons;
        self.buttons = buttons;
        let pressed = |b: u32| buttons & b != 0 && was & b == 0;
        if pressed(LEFT) {
            return self.left_press(x, y, modifiers);
        }
        if pressed(RIGHT) {
            return self.right_press(x, y);
        }
        // A release on a selected row that did not drag: the click alone.
        if was & LEFT != 0 && buttons & LEFT == 0 {
            let press = self.press.take();
            if let Some((_, _, Some(p), _)) = press {
                let mut m = self.model.borrow_mut();
                if !self.dragged && m.is_selected(p) && m.selection().len() > 1 {
                    m.click(p, modifiers);
                    return Response::Activated(self.id);
                }
            }
            return Response::None;
        }
        // Moved with the button held from a row: a drag.
        if buttons & LEFT != 0
            && !self.dragged
            && let Some((x0, y0, Some(p), _)) = self.press
            && ((x - x0).abs() > DRAG_START * k || (y - y0).abs() > DRAG_START * k)
        {
            self.dragged = true;
            // A drag is no first click of a double click.
            self.last_press = None;
            self.model.borrow_mut().start_drag(p);
            return self.answer();
        }
        Response::None
    }

    // ---- painting --------------------------------------------------------

    fn round(c: &mut Canvas<'_>, r: paint::Rect, radius: u32, color: Color) {
        let m = paint::rounded_rect(r.w as usize, r.h as usize, radius, shape::CORNER);
        c.mask(&m, r.x, r.y, color, 255);
    }

    fn outline(c: &mut Canvas<'_>, r: paint::Rect, radius: u32, t: u32, color: Color) {
        let m = paint::rounded_outline(r.w as usize, r.h as usize, radius, t, shape::CORNER);
        c.mask(&m, r.x, r.y, color, 255);
    }

    fn line(c: &mut Canvas<'_>, a: (i32, i32), b: (i32, i32), t: u32, color: Color) {
        let (x0, y0) = (a.0.min(b.0) - t as i32, a.1.min(b.1) - t as i32);
        let (w, h) = (
            (a.0 - b.0).abs() + 2 * t as i32 + 1,
            (a.1 - b.1).abs() + 2 * t as i32 + 1,
        );
        let m = paint::segment(
            w as usize,
            h as usize,
            (a.0 - x0, a.1 - y0),
            (b.0 - x0, b.1 - y0),
            t,
        );
        c.mask(&m, x0, y0, color, 255);
    }

    fn shadow(c: &mut Canvas<'_>, r: paint::Rect, radius: u32, k: i32, color: Color) {
        let blur = (9 * k) as usize;
        let (m, margin) = paint::shadow(r.w as usize, r.h as usize, radius, shape::CORNER, blur);
        let off = margin as i32;
        c.mask(&m, r.x - off, r.y - off + 3 * k, color, 70);
    }

    /// A folder or a page, `s` square at `(x, y)`.
    #[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
    fn icon(
        c: &mut Canvas<'_>,
        dir: bool,
        x: i32,
        y: i32,
        s: i32,
        k: i32,
        color: Color,
        bg: Color,
    ) {
        let t = (3 * k as u32).div_ceil(2);
        if dir {
            let body = prect(x + k, y + s / 4, s - 2 * k, s * 5 / 8);
            Self::outline(c, body, 2 * k as u32, t, color);
            c.fill(prect(x + k, y + s / 4, s / 2 - k, 3 * k), color);
        } else {
            let page = prect(x + s / 5, y + k, s * 3 / 5, s - 2 * k);
            c.fill(page, bg);
            Self::outline(c, page, 2 * k as u32, t, color);
            for i in 0..3 {
                let ly = y + s * (3 + 2 * i) / 10 + k;
                c.fill(prect(page.x + s / 8, ly, page.w - s / 4, t as i32), color);
            }
        }
    }

    fn tool_button(c: &mut Canvas<'_>, p: &Painter, r: paint::Rect, k: i32) {
        let radius = space::RADIUS_CONTROL.physical(p.scale);
        Self::round(c, r, radius, p.palette.bg_control);
        Self::outline(c, r, radius, k as u32, p.palette.border);
    }

    fn paint_toolbar(&mut self, p: &mut Painter, c: &mut Canvas<'_>, m: &Model) {
        let (b, k) = (self.body, self.k);
        let pal = p.palette;
        c.fill(prect(b.x, b.y, b.w, TOOLBAR_H * k), pal.bg_window);
        c.fill(prect(b.x, b.y + TOOLBAR_H * k - k, b.w, k), pal.border);
        let [back, fwd, up, crumbs, search] = self.toolbar_parts();
        let t = (8 * k as u32).div_ceil(5);
        for (r, on) in [
            (back, m.can_back()),
            (fwd, m.can_forward()),
            (up, crate::parent(&m.dir).is_some()),
        ] {
            Self::tool_button(c, p, r, k);
            let col = if on {
                pal.fg_text
            } else {
                pal.fg_text_disabled
            };
            let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
            let d = 5 * k / 2;
            if r == back {
                Self::line(c, (cx + d, cy - 2 * d), (cx - d, cy), t, col);
                Self::line(c, (cx - d, cy), (cx + d, cy + 2 * d), t, col);
            } else if r == fwd {
                Self::line(c, (cx - d, cy - 2 * d), (cx + d, cy), t, col);
                Self::line(c, (cx + d, cy), (cx - d, cy + 2 * d), t, col);
            } else {
                Self::line(c, (cx, cy - 6 * k), (cx, cy + 6 * k), t, col);
                Self::line(c, (cx - 5 * k, cy - k), (cx, cy - 6 * k), t, col);
                Self::line(c, (cx, cy - 6 * k), (cx + 5 * k, cy - k), t, col);
            }
        }
        // The path as crumbs.
        let radius = space::RADIUS_CONTROL.physical(p.scale);
        Self::round(c, crumbs, radius, pal.bg_control);
        Self::outline(c, crumbs, radius, k as u32, pal.border);
        let px = text::BODY.physical(p.scale);
        let base = baseline(crumbs.y, crumbs.h, px);
        let list = m.crumbs();
        let mut x = crumbs.x + 10 * k;
        self.crumbs.clear();
        for (i, (label, _)) in list.iter().enumerate() {
            let last = i + 1 == list.len();
            let (t_, col) = if last {
                (&mut p.medium, pal.fg_text)
            } else {
                (&mut p.ui, pal.fg_text_secondary)
            };
            let w = draw_text(c, t_, label, px, x, base, col, crumbs);
            self.crumbs.push((x - 4 * k, x + w + 4 * k));
            x += w;
            if !last {
                x += 6 * k;
                let (cx, cy) = (x + 2 * k, crumbs.y + crumbs.h / 2);
                let d = 5 * k / 2;
                Self::line(
                    c,
                    (cx - d, cy - 2 * d),
                    (cx + d, cy),
                    k as u32,
                    pal.fg_text_secondary,
                );
                Self::line(
                    c,
                    (cx + d, cy),
                    (cx - d, cy + 2 * d),
                    k as u32,
                    pal.fg_text_secondary,
                );
                x += 11 * k;
            }
        }
        // Search in the folder shown.
        Self::round(c, search, radius, pal.bg_control);
        let focus = m.focus == Focus::Search;
        Self::outline(
            c,
            search,
            radius,
            if focus { 2 * k as u32 } else { k as u32 },
            if focus { pal.accent } else { pal.border },
        );
        let (mx, my) = (search.x + 9 * k, search.y + search.h / 2);
        let ring = paint::rounded_outline(
            (10 * k) as usize,
            (10 * k) as usize,
            (5 * k) as u32,
            (3 * k as u32).div_ceil(2),
            zoraya_tokens::CornerShape::Circle,
        );
        c.mask(&ring, mx, my - 6 * k, pal.fg_text_secondary, 255);
        Self::line(
            c,
            (mx + 8 * k, my + 2 * k),
            (mx + 12 * k, my + 6 * k),
            (8 * k as u32).div_ceil(5),
            pal.fg_text_secondary,
        );
        let place = crate::model::Model::shown(m, &m.dir);
        let shown_place = place.rsplit('/').next().unwrap_or(&place);
        let (s, col) = if m.search.is_empty() && !focus {
            (
                alloc::format!("Search in {shown_place}"),
                pal.fg_text_disabled,
            )
        } else {
            (m.search.clone(), pal.fg_text)
        };
        let w = draw_text(c, &mut p.ui, &s, px, search.x + 28 * k, base, col, search);
        if focus {
            c.fill(
                prect(
                    search.x + 28 * k + w + k,
                    search.y + 6 * k,
                    k.max(1) + k / 2,
                    search.h - 12 * k,
                ),
                pal.fg_text,
            );
        }
    }

    fn paint_sidebar(&mut self, p: &mut Painter, c: &mut Canvas<'_>, m: &Model) {
        let (b, k) = (self.body, self.k);
        let pal = p.palette;
        let r = prect(
            b.x,
            b.y + TOOLBAR_H * k,
            SIDEBAR_W * k,
            b.h - (TOOLBAR_H + STATUS_H) * k,
        );
        let bg = mix(pal.bg_window, pal.fg_text, 35);
        c.fill(r, bg);
        c.fill(prect(r.x + r.w - k, r.y, k, r.h), pal.border);
        let cap = text::CAPTION.physical(p.scale);
        let px = text::BODY.physical(p.scale);
        let rows = self.place_rows(m);
        let current = m.place_of(&m.dir);
        let section = |c: &mut Canvas<'_>, p: &mut Painter, y: i32, title: &str| {
            draw_text(
                c,
                &mut p.medium,
                title,
                cap,
                r.x + 14 * k,
                baseline(y, 16 * k, cap),
                pal.fg_text_secondary,
                r,
            );
        };
        section(c, p, r.y + 14 * k, "Places");
        let mut drew_system = false;
        let radius = space::RADIUS_CONTROL.physical(p.scale);
        for (i, row) in rows {
            let place = &m.places[i];
            if place.system && !drew_system {
                section(c, p, row.y - 20 * k, "System");
                drew_system = true;
            }
            let selected = current == Some(i);
            let inner = if selected { pal.bg_selection } else { bg };
            if selected {
                Self::round(c, row, radius, inner);
            }
            Self::icon(
                c,
                true,
                r.x + 14 * k,
                row.y + 5 * k,
                ROW_H * k - 10 * k,
                k,
                pal.fg_text_secondary,
                inner,
            );
            let tx = r.x + 14 * k + ROW_H * k - 2 * k;
            let col = if selected {
                pal.fg_text_on_selection
            } else {
                pal.fg_text
            };
            let w = draw_text(
                c,
                &mut p.ui,
                &place.label,
                px,
                tx,
                baseline(row.y, row.h, px),
                col,
                row,
            );
            if place.room {
                // The room's own grant; its colour is the compositor's alone
                // (ADR-0172), so a neutral ring marks it.
                let d = 7 * k;
                let ring = paint::rounded_outline(
                    d as usize,
                    d as usize,
                    (d / 2) as u32,
                    k as u32,
                    zoraya_tokens::CornerShape::Circle,
                );
                c.mask(
                    &ring,
                    tx + w + 5 * k,
                    row.y + (row.h - d) / 2,
                    pal.fg_text_secondary,
                    255,
                );
            }
            let g = if place.write { "+w" } else { "+r" };
            let gw = p.mono.measure(g, cap);
            draw_text(
                c,
                &mut p.mono,
                g,
                cap,
                r.x + r.w - 16 * k - gw,
                baseline(row.y, row.h, cap),
                pal.fg_text_secondary,
                row,
            );
        }
        // What the window may change, said once under the places.
        let changes: Vec<&str> = m
            .places
            .iter()
            .filter(|pl| pl.write)
            .map(|pl| pl.label.as_str())
            .collect();
        let line1 = if changes.is_empty() {
            String::from("Read only here.")
        } else {
            alloc::format!("Changes only in {};", changes.join(", "))
        };
        let y = r.y + r.h - 54 * k;
        draw_text(
            c,
            &mut p.ui,
            &line1,
            cap,
            r.x + 14 * k,
            baseline(y, 16 * k, cap),
            pal.fg_text_secondary,
            r,
        );
        if !changes.is_empty() {
            draw_text(
                c,
                &mut p.ui,
                "the rest is read only.",
                cap,
                r.x + 14 * k,
                baseline(y + 16 * k, 16 * k, cap),
                pal.fg_text_secondary,
                r,
            );
        }
    }

    fn paint_list(&mut self, p: &mut Painter, c: &mut Canvas<'_>, m: &Model, focused: bool) {
        let k = self.k;
        let pal = p.palette;
        let l = self.list_rect();
        c.fill(l, pal.bg_view);
        let cap = text::CAPTION.physical(p.scale);
        let px = text::BODY.physical(p.scale);
        let pad = 14 * k;
        let (size_right, modified_x) = self.header_columns();
        let hb = mix(pal.bg_view, pal.fg_text, 25);
        c.fill(prect(l.x, l.y, l.w, HEADER_H * k), hb);
        c.fill(prect(l.x, l.y + HEADER_H * k - k, l.w, k), pal.border);
        let hbase = baseline(l.y, HEADER_H * k, cap);
        let heads = [
            ("Name", l.x + pad + 28 * k, Column::Name, false),
            ("Size", size_right, Column::Size, true),
            ("Modified", modified_x, Column::Modified, false),
        ];
        for (i, (label, x, col, right)) in heads.into_iter().enumerate() {
            let w = p.medium.measure(label, cap);
            let x = if right { x - w } else { x };
            let on = m.sort == col;
            draw_text(
                c,
                &mut p.medium,
                label,
                cap,
                x,
                hbase,
                if on {
                    pal.fg_text
                } else {
                    pal.fg_text_secondary
                },
                l,
            );
            if on {
                let (mx, cy) = (x + w + 6 * k, l.y + HEADER_H * k / 2);
                let d = 5 * k / 2;
                let (a, z) = if m.ascending {
                    (cy + d, cy - d)
                } else {
                    (cy - d, cy + d)
                };
                Self::line(
                    c,
                    (mx, a),
                    (mx + 4 * k, z),
                    (7 * k as u32).div_ceil(5),
                    pal.fg_text,
                );
                Self::line(
                    c,
                    (mx + 4 * k, z),
                    (mx + 8 * k, a),
                    (7 * k as u32).div_ceil(5),
                    pal.fg_text,
                );
            }
            if i > 0 {
                let sep = if right {
                    size_right - 60 * k
                } else {
                    modified_x - 12 * k
                };
                c.fill(
                    prect(sep, l.y + 7 * k, k, HEADER_H * k - 14 * k),
                    pal.border,
                );
            }
        }
        let rows = self.rows();
        let radius = space::RADIUS_CONTROL.physical(p.scale);
        let view: Vec<usize> = m.view().to_vec();
        let list_clip = prect(l.x, l.y + HEADER_H * k, l.w, l.h - HEADER_H * k);
        let top = m.top;
        for (vi, item) in view.iter().enumerate().skip(top).take(rows) {
            let row = vi - top;
            let y = l.y + (HEADER_H + 4) * k + row as i32 * ROW_H * k;
            let selected = m.is_selected(vi);
            let inner = if selected {
                pal.bg_selection
            } else {
                pal.bg_view
            };
            if selected {
                let up = vi > 0 && m.is_selected(vi - 1) && row > 0;
                let down = m.is_selected(vi + 1) && row + 1 < rows;
                let (top, h) = (
                    if up { y - 2 * k } else { y },
                    if up || down {
                        ROW_H * k + 2 * k
                    } else {
                        ROW_H * k
                    },
                );
                Self::round(
                    c,
                    prect(l.x + 6 * k, top, l.w - 12 * k, h),
                    if up || down { 2 * k as u32 } else { radius },
                    inner,
                );
            }
            if focused && m.cursor == Some(vi) && m.focus == Focus::List && m.rename.is_none() {
                Self::outline(
                    c,
                    prect(l.x + 6 * k, y, l.w - 12 * k, ROW_H * k),
                    radius,
                    (3 * k as u32).div_ceil(2),
                    pal.accent,
                );
            }
            let it = &m.items[*item];
            let icol = if it.dir {
                pal.accent
            } else {
                pal.fg_text_secondary
            };
            Self::icon(
                c,
                it.dir,
                l.x + pad,
                y + 4 * k,
                ROW_H * k - 8 * k,
                k,
                icol,
                inner,
            );
            let base = baseline(y, ROW_H * k, px);
            let tx = l.x + pad + 28 * k;
            let col = if selected {
                pal.fg_text_on_selection
            } else {
                pal.fg_text
            };
            match &m.rename {
                Some(r) if r.item == *item => {
                    // The field over the name, the stem selected at first.
                    let tw = p.ui.measure(&r.text, px);
                    let fw = (tw + 60 * k).max(180 * k);
                    let field = prect(tx - 6 * k, y + 2 * k, fw, ROW_H * k - 4 * k);
                    Self::round(c, field, 4 * k as u32, pal.bg_control);
                    Self::outline(
                        c,
                        field,
                        4 * k as u32,
                        (3 * k as u32).div_ceil(2),
                        pal.accent,
                    );
                    let before: String = r.text.chars().take(r.cursor).collect();
                    let bw = p.ui.measure(&before, px);
                    if r.stem_selected {
                        c.fill(
                            prect(tx - k, y + 6 * k, bw + 2 * k, ROW_H * k - 12 * k),
                            mix(pal.bg_control, pal.accent, 220),
                        );
                    }
                    draw_text(c, &mut p.ui, &r.text, px, tx, base, pal.fg_text, field);
                    if !r.stem_selected {
                        c.fill(
                            prect(tx + bw, y + 6 * k, k.max(1) + k / 2, ROW_H * k - 12 * k),
                            pal.fg_text,
                        );
                    }
                }
                _ => {
                    draw_text(
                        c,
                        &mut p.ui,
                        &it.name,
                        px,
                        tx,
                        base,
                        col,
                        prect(tx, y, size_right - 70 * k - tx, ROW_H * k).intersect(&list_clip),
                    );
                }
            }
            let (s, d) = m.columns(*item);
            let sc = if selected { col } else { pal.fg_text_secondary };
            let sw = p.ui.measure(&s, px);
            let sc2 = if it.dir { pal.fg_text_disabled } else { sc };
            draw_text(c, &mut p.ui, &s, px, size_right - sw, base, sc2, list_clip);
            draw_text(c, &mut p.ui, &d, px, modified_x, base, sc, list_clip);
        }
        if view.is_empty() {
            let msg = if m.search.is_empty() {
                "This folder is empty."
            } else {
                "Nothing matches."
            };
            draw_text(
                c,
                &mut p.ui,
                msg,
                px,
                l.x + pad + 28 * k,
                baseline(l.y + (HEADER_H + 4) * k, ROW_H * k, px),
                pal.fg_text_secondary,
                l,
            );
        }
    }

    fn paint_status(&mut self, p: &mut Painter, c: &mut Canvas<'_>, m: &Model) {
        let (b, k) = (self.body, self.k);
        let pal = p.palette;
        let r = prect(b.x, b.y + b.h - STATUS_H * k, b.w, STATUS_H * k);
        c.fill(r, pal.bg_window);
        c.fill(prect(r.x, r.y, r.w, k), pal.border);
        let cap = text::CAPTION.physical(p.scale);
        let base = baseline(r.y, r.h, cap);
        let left = m.status();
        draw_text(
            c,
            &mut p.ui,
            &left,
            cap,
            r.x + 14 * k,
            base,
            pal.fg_text_secondary,
            r,
        );
        let free = m.free_text();
        let w = p.ui.measure(&free, cap);
        draw_text(
            c,
            &mut p.ui,
            &free,
            cap,
            r.x + r.w - 14 * k - w,
            base,
            pal.fg_text_secondary,
            r,
        );
    }

    fn paint_menu(&mut self, p: &mut Painter, c: &mut Canvas<'_>, m: &Model) {
        let Some(r) = self.menu_rect(m) else { return };
        let k = self.k;
        let pal = p.palette;
        let radius = space::RADIUS_POPUP.physical(p.scale);
        Self::shadow(c, r, radius, k, pal.shadow);
        Self::round(c, r, radius, pal.bg_window);
        Self::outline(c, r, radius, k as u32, pal.border);
        let px = text::BODY.physical(p.scale);
        let cap = text::CAPTION.physical(p.scale);
        for (item, row) in self.menu_rows(r) {
            if item.starts_group() {
                c.fill(
                    prect(r.x + 8 * k, row.y - 5 * k, r.w - 16 * k, k),
                    pal.border,
                );
            }
            let on = m.menu_enabled(item);
            let col = if on {
                pal.fg_text
            } else {
                pal.fg_text_disabled
            };
            draw_text(
                c,
                &mut p.ui,
                item.label(),
                px,
                row.x + 10 * k,
                baseline(row.y, row.h, px),
                col,
                row,
            );
            if m.commands {
                let line = m.menu_line(item);
                draw_text(
                    c,
                    &mut p.mono,
                    &line,
                    cap,
                    row.x + 146 * k,
                    baseline(row.y, row.h, cap),
                    pal.fg_text_secondary,
                    row,
                );
            } else if !item.key().is_empty() {
                let kw = p.ui.measure(item.key(), cap);
                draw_text(
                    c,
                    &mut p.ui,
                    item.key(),
                    cap,
                    row.x + row.w - 10 * k - kw,
                    baseline(row.y, row.h, cap),
                    pal.fg_text_secondary,
                    row,
                );
            }
        }
    }

    fn paint_card(&mut self, p: &mut Painter, c: &mut Canvas<'_>, m: &Model) {
        let Some((heading, line)) = m.confirm_text() else {
            return;
        };
        let k = self.k;
        let pal = p.palette;
        let r = self.card_rect();
        let radius = space::RADIUS_POPUP.physical(p.scale);
        Self::shadow(c, r, radius, k, pal.shadow);
        Self::round(c, r, radius, pal.bg_window);
        Self::outline(c, r, radius, k as u32, pal.border);
        let pad = 20 * k;
        let hpx = text::HEADING.physical(p.scale);
        let px = text::BODY.physical(p.scale);
        draw_text(
            c,
            &mut p.medium,
            &heading,
            hpx,
            r.x + pad,
            r.y + pad + hpx as i32,
            pal.fg_text,
            r,
        );
        let y = r.y + pad + hpx as i32 + 14 * k;
        draw_text(
            c,
            &mut p.ui,
            &line,
            px,
            r.x + pad,
            y + px as i32,
            pal.fg_text,
            r,
        );
        draw_text(
            c,
            &mut p.ui,
            "This cannot be undone.",
            px,
            r.x + pad,
            y + px as i32 + 18 * k,
            pal.fg_text_secondary,
            r,
        );
        let (cancel, del) = self.card_buttons();
        let br = space::RADIUS_CONTROL.physical(p.scale);
        Self::round(c, cancel, br, pal.bg_control);
        Self::outline(c, cancel, br, k as u32, pal.border);
        Self::round(c, del, br, pal.error);
        for (b, label, col) in [
            (cancel, "Cancel", pal.fg_text),
            (del, "Delete", pal.fg_text_on_accent),
        ] {
            let w = p.medium.measure(label, px);
            draw_text(
                c,
                &mut p.medium,
                label,
                px,
                b.x + (b.w - w) / 2,
                baseline(b.y, b.h, px),
                col,
                b,
            );
        }
    }
}

/// The row at logical `(x, y)` of the window's body, as a drop's place
/// names it (`DragEnded`, ADR-0248): by the list's geometry and
/// the model's first row shown.
pub fn row_at(m: &Model, x: u32, y: u32) -> Option<usize> {
    let (x, y) = (x as i32, y as i32);
    let first = TOOLBAR_H + HEADER_H + 4;
    if x < SIDEBAR_W || y < first {
        return None;
    }
    // Only a row shown: not one scrolled out of view under the status line
    // (review 7h, 5).
    let row = ((y - first) / ROW_H) as usize;
    let pos = m.top + row;
    (row < m.visible_rows && pos < m.view().len()).then_some(pos)
}

impl Widget for FilesView {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (
            space::CONTROL_HEIGHT.times(32),
            space::CONTROL_HEIGHT.times(20),
        )
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        self.k = p.scale.factor() as i32;
        self.body = r.physical(p.scale);
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
    fn value(&self) -> Value<'_> {
        Value::None
    }

    fn event(&mut self, e: &Event) -> Response {
        match *e {
            Event::Key {
                code,
                text,
                modifiers,
            } => {
                self.model.borrow_mut().key(code, text, modifiers);
                self.follow();
                self.answer()
            }
            Event::Pointer {
                x,
                y,
                buttons,
                modifiers,
            } => self.pointer(x, y, buttons, modifiers),
            Event::Focus(_) => Response::Repaint,
        }
    }

    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        self.k = p.scale.factor() as i32;
        self.body = self.rect.physical(p.scale);
        self.follow();
        let model = self.model.clone();
        let m = model.borrow();
        c.fill(self.body, p.palette.bg_view);
        self.paint_toolbar(p, c, &m);
        self.paint_sidebar(p, c, &m);
        self.paint_list(p, c, &m, focused);
        self.paint_status(p, c, &m);
        self.paint_menu(p, c, &m);
        self.paint_card(p, c, &m);
    }
}

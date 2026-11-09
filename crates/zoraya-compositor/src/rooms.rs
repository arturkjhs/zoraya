//! Rooms on the screen (ADR-0172, ADR-0168): each client's launch record,
//! the room table and the dock's applications from the client manager, the
//! dock's geometry and its presses, the fields' copy of the wallpaper. All
//! of it changes only through the manager channels (INV-0172-1); nothing a
//! client sends reaches it.

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_proto_gui::{self as gui, status};
use zoraya_protocol::NO_REPLY;
use zoraya_settings::rooms::{self, Rooms, is_room_name};
use zoraya_tokens::{Color, Scale, space, text};

use crate::fields::{self, Shape, Sizes};
use crate::{Compositor, Key, Platform, Rect, reply};

/// Entries one record may carry.
pub const MAX_RECORD_ENTRIES: usize = 32;
/// Applications in the dock.
pub const MAX_DOCK_APPS: usize = 12;

/// A client's launch record as the panel gave it (ADR-0168 point 1,
/// ADR-0172 point 4); `home` for a client added with `AddClient`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    /// Empty for `home`.
    pub room: String,
    pub untrusted: bool,
    /// 1 to 4 for a trusted room; 0 for `home` and an untrusted room.
    pub colour: u8,
    pub app: String,
    /// In ADR-0153's words (`+w ~/project`).
    pub entries: Vec<String>,
    /// The line that made the record.
    pub line: String,
    /// The panel's tag (proposed ADR-0245): how a sheet finds this client;
    /// 0 for none.
    pub tag: u32,
}

fn is_app_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 16
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn text_line(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

impl Record {
    pub fn home() -> Record {
        Record::default()
    }

    /// The record of `AddRoomClient`, if it is whole: a room name or
    /// `home`, a colour that fits the room's trust, UTF-8 words with no
    /// control characters.
    pub fn of(m: &gui::AddRoomClient<'_>) -> Option<Record> {
        let room = core::str::from_utf8(m.room.0).ok()?;
        let app = core::str::from_utf8(m.app.0).ok()?;
        let entries = core::str::from_utf8(m.entries.0).ok()?;
        let line = core::str::from_utf8(m.line.0).ok()?;
        let untrusted = match m.untrusted {
            0 => false,
            1 => true,
            _ => return None,
        };
        let colour = u8::try_from(m.colour).ok()?;
        let fits = if room.is_empty() {
            !untrusted && colour == 0
        } else if untrusted {
            colour == 0
        } else {
            (1..=rooms::SLOTS).contains(&colour)
        };
        if !(room.is_empty() || is_room_name(room))
            || !fits
            || !is_app_name(app)
            || !text_line(line)
        {
            return None;
        }
        let entries: Vec<String> = entries
            .split('\n')
            .filter(|e| !e.is_empty())
            .map(String::from)
            .collect();
        if entries.len() > MAX_RECORD_ENTRIES || !entries.iter().all(|e| text_line(e)) {
            return None;
        }
        Some(Record {
            room: room.into(),
            untrusted,
            colour,
            app: app.into(),
            entries,
            line: line.into(),
            tag: m.tag,
        })
    }

    /// The name shown: `home` for the session's own.
    pub fn room_name(&self) -> &str {
        if self.room.is_empty() {
            rooms::HOME
        } else {
            &self.room
        }
    }
}

/// A part of the dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// An application, by its index in the dock's list.
    App(usize),
    /// A room's chip: 0 `home`, then the table's rooms.
    Chip(usize),
    /// `+`: a new room.
    Plus,
}

/// Where the dock and its cells are, physical pixels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DockGeometry {
    pub rect: Rect,
    /// Each cell's column: as wide as the cell, as tall as an icon, at
    /// the icons' top.
    pub cells: Vec<(Cell, Rect)>,
    /// The divider's x.
    pub divider: i64,
}

/// The dock's sizes in logical pixels, as the mockups draw it: the margin
/// under the dock, a chip's height, the show-commands label zone, the gap
/// between an icon and its dots.
pub const DOCK_MARGIN: u32 = 10;
pub const CHIP_HEIGHT: u32 = 26;
pub const LABEL_ZONE: u32 = 30;
pub const DOT_ZONE: u32 = 6;

/// One application in the dock: its name and its launch line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockApp {
    pub name: String,
    pub line: String,
}

impl<P: Platform> Compositor<P> {
    /// A client's launch record, for tests and the drawing.
    pub fn record(&self, client: usize) -> Option<&Record> {
        self.clients.get(client)?.as_ref().map(|c| &c.record)
    }

    pub fn rooms(&self) -> &Rooms {
        &self.rooms
    }

    pub fn current_room(&self) -> &str {
        &self.current_room
    }

    pub fn dock_apps(&self) -> &[DockApp] {
        &self.dock_apps
    }

    pub fn dock(&self) -> &DockGeometry {
        &self.dock
    }

    pub fn commands(&self) -> bool {
        self.commands
    }

    pub(crate) fn set_rooms(&mut self, current: &[u8], table: &[u8]) -> u32 {
        let Ok(current) = core::str::from_utf8(current) else {
            return status::INVALID;
        };
        let read = rooms::read(table);
        if !read.problems.is_empty() || !(current.is_empty() || read.get(current).is_some()) {
            return status::INVALID;
        }
        self.rooms = read;
        self.current_room = current.into();
        self.dock_changed();
        self.fields_dirty = true;
        status::OK
    }

    pub(crate) fn set_dock(&mut self, list: &[u8]) -> u32 {
        let Ok(list) = core::str::from_utf8(list) else {
            return status::INVALID;
        };
        let mut apps = Vec::new();
        for l in list.split('\n').filter(|l| !l.is_empty()) {
            let Some((name, line)) = l.split_once('\t') else {
                return status::INVALID;
            };
            if !is_app_name(name) || !text_line(line) || apps.len() == MAX_DOCK_APPS {
                return status::INVALID;
            }
            apps.push(DockApp {
                name: name.into(),
                line: line.into(),
            });
        }
        self.dock_apps = apps;
        self.dock_changed();
        status::OK
    }

    /// The applications the dock shows: its list, then every other
    /// application that has a window (ADR-0168: "every application that
    /// has a window has an icon").
    pub fn dock_list(&self) -> Vec<DockApp> {
        let mut list = self.dock_apps.clone();
        for w in self.windows.iter().filter(|w| w.framed()) {
            let Some(r) = self.record(w.client) else {
                continue;
            };
            if !r.app.is_empty() && !list.iter().any(|a| a.name == r.app) {
                list.push(DockApp {
                    name: r.app.clone(),
                    line: r.app.clone(),
                });
            }
        }
        list
    }

    /// The rooms `app` has framed windows in, `home` first, then in the
    /// table's order, each once, with each room's colour slot and trust.
    pub fn app_rooms(&self, app: &str) -> Vec<(String, u8, bool)> {
        let mut out: Vec<(String, u8, bool)> = Vec::new();
        for w in self.windows.iter().filter(|w| w.framed()) {
            let Some(r) = self.record(w.client) else {
                continue;
            };
            if r.app == app && !out.iter().any(|(n, _, _)| n == r.room_name()) {
                out.push((r.room_name().into(), r.colour, r.untrusted));
            }
        }
        let order = |n: &str| {
            if n == rooms::HOME {
                0
            } else {
                1 + self
                    .rooms
                    .rooms
                    .iter()
                    .position(|r| r.name == n)
                    .unwrap_or(rooms::MAX_ROOMS)
            }
        };
        out.sort_by_key(|(n, _, _)| order(n));
        out
    }

    /// The height of the dock's reserved strip at the bottom, physical
    /// pixels: the dock and its margin, taller in show-commands mode.
    pub fn dock_strip(&self) -> i64 {
        let s = self.scale.factor();
        let h = 2 * space::DOCK_PADDING.physical(Scale::ONE)
            + space::DOCK_ICON.physical(Scale::ONE)
            + DOT_ZONE
            + DOCK_MARGIN
            + if self.commands { LABEL_ZONE } else { 0 };
        i64::from(h * s)
    }

    /// The work area: between the bar and the dock's strip.
    pub fn work_area(&self) -> Rect {
        let bar = i64::from(space::BAR_HEIGHT.physical(self.scale));
        let bottom = (i64::from(self.height) - self.dock_strip()).max(bar);
        Rect::new(0, bar, i64::from(self.width), bottom - bar)
    }

    /// A chip's width, physical: the dot, the name, `untrusted`.
    fn chip_width(&mut self, name: &str, untrusted: bool) -> i64 {
        let s = self.scale.factor();
        let px = text::CAPTION.physical(Scale::ONE) * s;
        let mut w = (10 + 7 + 6 + 10) * s as i32 + self.title_text.measure(name, px);
        if untrusted {
            w += 5 * s as i32 + self.ui.measure("untrusted", px);
        }
        i64::from(w.max((CHIP_HEIGHT * s) as i32))
    }

    /// Lays the dock out again after its contents, the rooms, the scale or
    /// the mode changed, and marks its strip (and the tooltip's place) to
    /// be drawn again.
    pub(crate) fn dock_changed(&mut self) {
        let old = self.dock.rect;
        let s = i64::from(self.scale.factor());
        let k = self.scale.factor();
        let icon = i64::from(space::DOCK_ICON.physical(self.scale));
        let pad = i64::from(space::DOCK_PADDING.physical(self.scale));
        let cap = text::CAPTION.physical(Scale::ONE) * k;
        let apps = self.dock_list();
        let mut cells: Vec<(Cell, i64)> = Vec::new();
        for (i, app) in apps.iter().enumerate() {
            let mut w = icon;
            if self.commands {
                let a = self.title_text.measure(&app.name, cap);
                let b = self.mono.measure(&app.line, cap);
                w = w.max(i64::from(a.max(b)));
            }
            cells.push((Cell::App(i), w));
        }
        let chips: Vec<(String, bool)> = core::iter::once((String::from(rooms::HOME), false))
            .chain(
                self.rooms
                    .rooms
                    .iter()
                    .map(|r| (r.name.clone(), r.untrusted)),
            )
            .collect();
        for (i, (name, untrusted)) in chips.iter().enumerate() {
            let mut w = self.chip_width(name, *untrusted);
            if self.commands {
                let line = alloc::format!("room {name}");
                w = w.max(i64::from(self.mono.measure(&line, cap)));
            }
            cells.push((Cell::Chip(i), w));
        }
        let mut plus = i64::from(CHIP_HEIGHT) * s;
        if self.commands {
            plus = plus.max(i64::from(self.mono.measure("room new", cap)));
        }
        cells.push((Cell::Plus, plus));
        let gap = pad;
        // The divider counts as a one-pixel cell between apps and chips.
        let n = cells.len() as i64 + 1;
        let total = 2 * pad + cells.iter().map(|c| c.1).sum::<i64>() + s + (n - 1) * gap;
        let h = self.dock_strip() - i64::from(DOCK_MARGIN) * s;
        let w = total.min(i64::from(self.width));
        let x = (i64::from(self.width) - w) / 2;
        let y = i64::from(self.height) - i64::from(DOCK_MARGIN) * s - h;
        let mut out = DockGeometry {
            rect: Rect::new(x, y, w, h),
            cells: Vec::new(),
            divider: 0,
        };
        let iy = y + pad;
        let mut cx = x + pad;
        for (cell, cw) in cells {
            if cell == Cell::Chip(0) {
                out.divider = cx;
                cx += s + gap;
            }
            out.cells.push((cell, Rect::new(cx, iy, cw, icon)));
            cx += cw + gap;
        }
        // What the dock shows besides its geometry: drawn again only when
        // something of it changed.
        let mut sig = String::new();
        for a in &apps {
            let rooms = self.app_rooms(&a.name);
            sig.push_str(&alloc::format!("{}\t{}\t{rooms:?}\n", a.name, a.line));
        }
        let table = &self.rooms.rooms;
        sig.push_str(&alloc::format!(
            "{table:?} {} {}",
            self.current_room,
            self.commands
        ));
        let same = out == self.dock && sig == self.dock_sig;
        self.dock = out;
        self.dock_sig = sig;
        if self.hover.is_some_and(|h| h >= apps.len()) {
            self.hover = None;
        }
        if same {
            return;
        }
        let tooltip = 8 * s + 38 * s;
        for r in [old, self.dock.rect] {
            self.dirty(&Rect::new(
                0,
                r.y - tooltip,
                i64::from(self.width),
                r.height + tooltip,
            ));
        }
        // The strip's height can change with the mode: the work area and
        // the fields with it.
        self.fields_dirty = true;
    }

    /// The dock's cell under `(x, y)`, if the point is on the dock.
    pub fn dock_cell(&self, x: i64, y: i64) -> Option<Cell> {
        if !self.dock.rect.contains(x, y) {
            return None;
        }
        self.dock
            .cells
            .iter()
            .find(|(_, r)| x >= r.x && x < r.right())
            .map(|(c, _)| *c)
    }

    /// The pointer moved: the tooltip follows the hovered icon.
    pub(crate) fn dock_hover(&mut self, x: i64, y: i64) {
        let hover = match self.dock_cell(x, y) {
            Some(Cell::App(i)) => Some(i),
            _ => None,
        };
        if hover != self.hover {
            self.hover = hover;
            let s = i64::from(self.scale.factor());
            let r = self.dock.rect;
            let tooltip = 8 * s + 38 * s;
            self.dirty(&Rect::new(0, r.y - tooltip, i64::from(self.width), tooltip));
        }
    }

    pub fn hovered(&self) -> Option<usize> {
        self.hover
    }

    /// A press on the dock (ADR-0168): an application's icon raises its
    /// windows, or shows them when they are all hidden; with no window it
    /// asks the panel to launch it; a chip asks the panel to enter its
    /// room; `+` asks for a new one.
    pub(crate) fn dock_press(&mut self, cell: Cell) {
        match cell {
            Cell::App(i) => {
                let Some(app) = self.dock_list().get(i).map(|a| a.name.clone()) else {
                    return;
                };
                let mine: Vec<Key> = self
                    .windows
                    .iter()
                    .filter(|w| w.framed())
                    .filter(|w| self.record(w.client).is_some_and(|r| r.app == app))
                    .map(Self::key_of)
                    .collect();
                if mine.is_empty() {
                    if i < self.dock_apps.len() {
                        self.tell_panel(gui::dock::LAUNCH, i as u32);
                    }
                    return;
                }
                for k in &mine {
                    self.set_hidden(*k, false);
                    self.raise(*k);
                }
                self.set_focus(mine.last().copied());
            }
            Cell::Chip(i) => self.tell_panel(gui::dock::ROOM, i as u32),
            Cell::Plus => self.tell_panel(gui::dock::NEW_ROOM, 0),
        }
    }

    /// System+Enter: a new terminal from the panel (ADR-0168 point 2).
    pub(crate) fn new_terminal(&mut self) {
        if let Some(i) = self.dock_apps.iter().position(|a| a.name == "term") {
            self.tell_panel(gui::dock::LAUNCH, i as u32);
        }
    }

    fn tell_panel(&mut self, kind: u32, index: u32) {
        let Some(end) = self.client_manager else {
            return;
        };
        let length = reply(&gui::DockPressed { kind, index }, NO_REPLY, &mut self.out);
        // A full queue loses the press; the person presses again.
        let _ = self.platform.write(end, &self.out[..length], &[]);
    }

    /// A room's colour, `None` for `home`.
    pub fn room_colour(&self, r: &Record) -> Option<Color> {
        if r.room.is_empty() {
            None
        } else if r.untrusted {
            Some(self.appearance.palette().room_untrusted)
        } else {
            Some(self.appearance.palette().room(r.colour))
        }
    }

    /// Whether a window fills the work area (a maximised window gives no
    /// shape to the fields, ADR-0172 point 5(e)).
    fn fills_work_area(&self, r: &Rect) -> bool {
        let w = self.work_area();
        r.x <= w.x && r.y <= w.y && r.right() >= w.right() && r.bottom() >= w.bottom()
    }

    /// The wallpaper with the rooms' fields, drawn again when the rooms or
    /// their windows changed, never per frame (ADR-0172 point 5); marks
    /// what changed.
    pub(crate) fn update_fields(&mut self) {
        self.fields_dirty = false;
        let (width, height) = (self.width as usize, self.height as usize);
        if self.wallpaper.len() != width * height {
            return;
        }
        let f = self.frame();
        let mut names: Vec<String> = Vec::new();
        let mut colours: Vec<Option<(Color, Color)>> = Vec::new();
        let mut shapes: Vec<Shape> = Vec::new();
        let mut tops: Vec<Rect> = Vec::new();
        let p = self.appearance.palette();
        for w in self.windows.iter().filter(|w| w.framed() && !w.hidden) {
            let Some(r) = self.record(w.client) else {
                continue;
            };
            let i = match names.iter().position(|n| n == r.room_name()) {
                Some(i) => i,
                None => {
                    names.push(r.room_name().into());
                    colours.push(
                        self.room_colour(r)
                            .map(|c| (c, c.blend(p.wallpaper_a, 128))),
                    );
                    tops.push(Rect::default());
                    names.len() - 1
                }
            };
            let outer = w.outer(&f);
            if !self.fills_work_area(&outer) {
                shapes.push(Shape {
                    room: i,
                    rect: outer,
                });
                if tops[i].is_empty() || outer.y < tops[i].y {
                    tops[i] = outer;
                }
            }
        }
        let mut next = self.wallpaper.clone();
        // INV-0172-6: only between rooms.
        if names.len() >= 2 {
            let sizes = Sizes {
                padding: i64::from(space::FIELD_PADDING.physical(self.scale)),
                gap: i64::from(space::FIELD_GAP.physical(self.scale)),
                radius: i64::from(space::RADIUS_WINDOW.physical(self.scale)),
            };
            let work = self.work_area();
            fields::paint(&mut next, width, &shapes, &colours, work, sizes);
            self.field_headers(&mut next, &names, &colours, &tops, work, sizes);
        }
        let changed = changed_rect(&self.fielded, &next, width).unwrap_or_else(|| self.screen());
        self.fielded = next;
        self.dirty(&changed);
    }

    /// Each field's header on a small neutral ground above its top-most
    /// window: the name, `untrusted`, the room's grants.
    fn field_headers(
        &mut self,
        px: &mut [u32],
        names: &[String],
        colours: &[Option<(Color, Color)>],
        tops: &[Rect],
        work: Rect,
        sizes: Sizes,
    ) {
        let width = self.width as usize;
        let k = self.scale.factor();
        let s = k as i32;
        let p = self.appearance.palette();
        let cap = text::CAPTION.physical(Scale::ONE) * k;
        for (i, name) in names.iter().enumerate() {
            if colours[i].is_none() || tops[i].is_empty() {
                continue;
            }
            let mut words = String::from(name.as_str());
            if let Some(room) = self.rooms.get(name) {
                if room.untrusted {
                    words.push_str("  untrusted");
                }
                for g in &room.grants {
                    words.push_str("  ");
                    words.push_str(&g.words());
                }
            }
            let t = tops[i];
            let h = 18 * s;
            let y = (t.y - sizes.padding + 3 * i64::from(s)) as i32;
            let x = (t.x + 4 * i64::from(s)) as i32;
            let tw = self.ui.measure(&words, cap) + 16 * s;
            let inner = Rect::new(
                work.x + sizes.gap,
                work.y + sizes.gap,
                work.width - 2 * sizes.gap,
                work.height - 2 * sizes.gap,
            );
            let mut c = zoraya_paint::Canvas::new(px, width);
            c.set_clip(crate::draw::prect(inner));
            let ground = zoraya_paint::rounded_rect(
                tw.max(0) as usize,
                h as usize,
                (h / 2) as u32,
                zoraya_tokens::shape::CORNER,
            );
            c.mask(&ground, x, y, p.bg_window, 200);
            let baseline = y + h / 2 + cap as i32 * 7 / 20;
            crate::draw::draw_text(
                &mut c,
                &mut self.ui,
                &words,
                cap,
                x + 8 * s,
                baseline,
                p.fg_text,
            );
        }
    }
}

/// The smallest rectangle holding every pixel that differs, if any; `None`
/// also when the sizes differ.
fn changed_rect(a: &[u32], b: &[u32], width: usize) -> Option<Rect> {
    if a.len() != b.len() || width == 0 {
        return None;
    }
    let mut r = Rect::default();
    for (y, (ra, rb)) in a.chunks(width).zip(b.chunks(width)).enumerate() {
        if ra == rb {
            continue;
        }
        let first = ra.iter().zip(rb).position(|(p, q)| p != q).unwrap_or(0);
        let last = ra.iter().zip(rb).rposition(|(p, q)| p != q).unwrap_or(0);
        r = r.union(&Rect::new(
            first as i64,
            y as i64,
            (last - first + 1) as i64,
            1,
        ));
    }
    Some(r)
}

//! The file manager's redesign (the architect's request of 2026-11-10): a
//! sidebar of the places the window holds, a toolbar (back, forward, up, the
//! path as crumbs, search), a list with icons and sortable columns (name,
//! size, modified), selection by click, Shift and Command, rename in place,
//! delete with confirmation, and a status line. Four screens, drawn in a
//! room so that the sidebar shows the room's grants.

use crate::canvas::{Face, mix};
use crate::clear::{Content, DOCK_REST, PROJECT, RUN_FILES_EDIT, Win};
use crate::scene::{Icon, Painter, Rect};

/// A row of the list: name, folder or not, size, modified.
struct Row {
    name: &'static str,
    dir: bool,
    size: &'static str,
    modified: &'static str,
    /// For sorting by modified time: larger is newer.
    age: u32,
}

const ROWS: [Row; 9] = [
    Row {
        name: "docs",
        dir: true,
        size: "",
        modified: "Yesterday 18:02",
        age: 7,
    },
    Row {
        name: "include",
        dir: true,
        size: "",
        modified: "3 Nov 2026",
        age: 2,
    },
    Row {
        name: "tests",
        dir: true,
        size: "",
        modified: "Today 09:41",
        age: 8,
    },
    Row {
        name: "main.c",
        dir: false,
        size: "1.9 kB",
        modified: "Today 14:20",
        age: 9,
    },
    Row {
        name: "Makefile",
        dir: false,
        size: "412 B",
        modified: "1 Nov 2026",
        age: 1,
    },
    Row {
        name: "notes.txt",
        dir: false,
        size: "2.1 kB",
        modified: "Today 11:05",
        age: 8,
    },
    Row {
        name: "plán.txt",
        dir: false,
        size: "1.2 kB",
        modified: "Yesterday 16:30",
        age: 6,
    },
    Row {
        name: "звіт-2026.txt",
        dir: false,
        size: "2.1 kB",
        modified: "5 Nov 2026",
        age: 4,
    },
    Row {
        name: "Žďár.md",
        dir: false,
        size: "913 B",
        modified: "4 Nov 2026",
        age: 3,
    },
];

/// Which column the list is sorted by, and the direction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    NameUp,
    ModifiedDown,
}

/// What a screen shows over the common window.
struct State {
    sort: Sort,
    /// Indices into the sorted rows.
    selected: &'static [usize],
    /// The row under the keyboard's cursor (a focus ring).
    cursor: Option<usize>,
    /// The row being renamed in place, with the text typed so far.
    renaming: Option<(usize, &'static str)>,
    /// The confirmation card for a delete.
    confirm: bool,
    status: &'static str,
    pointer: Option<(f32, f32)>,
}

const SIDEBAR_W: f32 = 184.0;
const TOOLBAR_H: f32 = 48.0;
const HEADER_H: f32 = 28.0;
const STATUS_H: f32 = 28.0;

impl Painter<'_> {
    fn files_window(&self) -> Win<'static> {
        Win {
            r: Rect {
                x: 64.0,
                y: 52.0,
                w: 896.0,
                h: 600.0,
            },
            title: "project \u{2014} files",
            room: &PROJECT,
            extra: &["+r /"],
            focused: true,
            content: Content::Files,
        }
    }

    /// A square button with a glyph drawn by `glyph`; disabled greys it.
    fn tool_button(
        &mut self,
        x: f32,
        y: f32,
        s: f32,
        enabled: bool,
        glyph: fn(&mut Self, f32, f32, f32, u32),
    ) {
        let radius = self.n("radius.control");
        let (bg, border) = (self.c("bg.control"), self.c("border"));
        self.canvas.rounded_rect(x, y, s, s, radius, bg);
        self.canvas
            .stroke_rounded_rect(x, y, s, s, radius, 1.0, border);
        let fg = if enabled {
            self.c("fg.text")
        } else {
            self.c("fg.text.disabled")
        };
        glyph(self, x + s / 2.0, y + s / 2.0, s, fg);
    }

    fn chevron_left(&mut self, cx: f32, cy: f32, _s: f32, c: u32) {
        self.canvas
            .segment((cx + 2.5, cy - 5.0), (cx - 2.5, cy), 1.6, c);
        self.canvas
            .segment((cx - 2.5, cy), (cx + 2.5, cy + 5.0), 1.6, c);
    }

    fn chevron_right(&mut self, cx: f32, cy: f32, _s: f32, c: u32) {
        self.canvas
            .segment((cx - 2.5, cy - 5.0), (cx + 2.5, cy), 1.6, c);
        self.canvas
            .segment((cx + 2.5, cy), (cx - 2.5, cy + 5.0), 1.6, c);
    }

    fn arrow_up(&mut self, cx: f32, cy: f32, _s: f32, c: u32) {
        self.canvas.segment((cx, cy - 6.0), (cx, cy + 6.0), 1.6, c);
        self.canvas
            .segment((cx - 5.0, cy - 1.0), (cx, cy - 6.0), 1.6, c);
        self.canvas
            .segment((cx, cy - 6.0), (cx + 5.0, cy - 1.0), 1.6, c);
    }

    /// A small triangle: the sorted column's direction.
    fn sort_mark(&mut self, x: f32, cy: f32, up: bool, c: u32) {
        let (a, b) = if up {
            (cy + 2.5, cy - 2.5)
        } else {
            (cy - 2.5, cy + 2.5)
        };
        self.canvas.segment((x, a), (x + 4.0, b), 1.4, c);
        self.canvas.segment((x + 4.0, b), (x + 8.0, a), 1.4, c);
    }

    fn magnifier(&mut self, x: f32, cy: f32, c: u32, inner: u32) {
        self.canvas.circle(x + 5.0, cy - 1.0, 5.0, c);
        self.canvas.circle(x + 5.0, cy - 1.0, 3.5, inner);
        self.canvas
            .segment((x + 8.5, cy + 2.5), (x + 12.0, cy + 6.0), 1.6, c);
    }

    fn files_toolbar(&mut self, r: Rect) {
        let bg = self.c("bg.window");
        self.canvas.rect(r.x, r.y, r.w, r.h, bg);
        let s = self.n("control.height");
        let y = r.y + (r.h - s) / 2.0;
        let mut x = r.x + 12.0;
        self.tool_button(x, y, s, true, Self::chevron_left);
        x += s + 4.0;
        self.tool_button(x, y, s, false, Self::chevron_right);
        x += s + 10.0;
        self.tool_button(x, y, s, true, Self::arrow_up);
        x += s + 12.0;
        // The path as crumbs, each one a place to go back to.
        let search_w = 200.0;
        let crumbs_w = r.x + r.w - 12.0 - search_w - 10.0 - x;
        let radius = self.n("radius.control");
        let (ctrl, border) = (self.c("bg.control"), self.c("border"));
        let (fg, sec) = (self.c("fg.text"), self.c("fg.text.secondary"));
        self.canvas.rounded_rect(x, y, crumbs_w, s, radius, ctrl);
        self.canvas
            .stroke_rounded_rect(x, y, crumbs_w, s, radius, 1.0, border);
        let size = self.n("text.body");
        let base = self.baseline(Face::Ui, size, y, s);
        let mut cx = x + 10.0;
        let crumbs = ["~", "project"];
        for (i, c) in crumbs.iter().enumerate() {
            let last = i + 1 == crumbs.len();
            let face = if last { Face::UiMedium } else { Face::Ui };
            cx += self.text(face, size, cx, base, c, if last { fg } else { sec });
            if !last {
                cx += 6.0;
                self.chevron_right(cx + 2.5, y + s / 2.0, 0.0, sec);
                cx += 11.0;
            }
        }
        // Search in the folder shown.
        let sx = r.x + r.w - 12.0 - search_w;
        self.canvas.rounded_rect(sx, y, search_w, s, radius, ctrl);
        self.canvas
            .stroke_rounded_rect(sx, y, search_w, s, radius, 1.0, border);
        self.magnifier(sx + 9.0, y + s / 2.0, sec, ctrl);
        let dis = self.c("fg.text.disabled");
        self.text(Face::Ui, size, sx + 28.0, base, "Search in project", dis);
        self.canvas.line_h(r.x, r.y + r.h - 1.0, r.w, 1.0, border);
    }

    /// The places the window holds: home, each of its room's grants, the
    /// system image; with what each grant allows, in ADR-0153's words.
    fn files_sidebar(&mut self, r: Rect) {
        let bg = mix(self.c("bg.window"), self.c("fg.text"), 0.035);
        self.canvas.rect(r.x, r.y, r.w, r.h, bg);
        let border = self.c("border");
        self.canvas.rect(r.x + r.w - 1.0, r.y, 1.0, r.h, border);
        let (fg, sec) = (self.c("fg.text"), self.c("fg.text.secondary"));
        let cap = self.n("text.caption");
        let size = self.n("text.body");
        let row_h = self.n("control.height");
        let radius = self.n("radius.control");
        let room = self.c("room.1");
        let mut y = r.y + 14.0;
        let section = |p: &mut Self, y: &mut f32, title: &str| {
            let base = p.baseline(Face::UiMedium, cap, *y, 16.0);
            p.text(Face::UiMedium, cap, r.x + 14.0, base, title, sec);
            *y += 20.0;
        };
        // (name, grant, selected, room dot)
        let places: [(&str, &str, bool, bool); 3] = [
            ("Home", "+r", false, false),
            ("project", "+w", true, true),
            ("include", "+r", false, true),
        ];
        section(self, &mut y, "Places");
        for (name, grant, selected, dot) in places {
            let inner = if selected { self.c("bg.selection") } else { bg };
            if selected {
                self.canvas
                    .rounded_rect(r.x + 6.0, y, r.w - 13.0, row_h, radius, inner);
            }
            self.icon_on(Icon::Files, r.x + 14.0, y + 5.0, row_h - 10.0, sec, inner);
            let base = self.baseline(Face::Ui, size, y, row_h);
            let tx = r.x + 14.0 + row_h - 2.0;
            let col = if selected {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            let w = self.text(Face::Ui, size, tx, base, name, col);
            if dot {
                self.canvas.circle(tx + w + 8.0, y + row_h / 2.0, 3.5, room);
            }
            let gw = self.fonts.width(Face::Mono, cap, grant);
            let base = self.baseline(Face::Mono, cap, y, row_h);
            self.text(Face::Mono, cap, r.x + r.w - 16.0 - gw, base, grant, sec);
            y += row_h + 2.0;
        }
        y += 10.0;
        section(self, &mut y, "System");
        self.icon_on(Icon::Files, r.x + 14.0, y + 5.0, row_h - 10.0, sec, bg);
        let base = self.baseline(Face::Ui, size, y, row_h);
        self.text(
            Face::Ui,
            size,
            r.x + 14.0 + row_h - 2.0,
            base,
            "/system",
            fg,
        );
        let gw = self.fonts.width(Face::Mono, cap, "+r");
        let base = self.baseline(Face::Mono, cap, y, row_h);
        self.text(Face::Mono, cap, r.x + r.w - 16.0 - gw, base, "+r", sec);
        // What the window may change, said once under the places.
        let y = r.y + r.h - 54.0;
        let base = self.baseline(Face::Ui, cap, y, 16.0);
        self.text(
            Face::Ui,
            cap,
            r.x + 14.0,
            base,
            "Changes only in project;",
            sec,
        );
        let base = self.baseline(Face::Ui, cap, y + 16.0, 16.0);
        self.text(
            Face::Ui,
            cap,
            r.x + 14.0,
            base,
            "the rest is read only.",
            sec,
        );
    }

    fn sorted(sort: Sort) -> Vec<&'static Row> {
        let mut rows: Vec<&Row> = ROWS.iter().collect();
        match sort {
            Sort::NameUp => {}
            // Folders first, then the newest.
            Sort::ModifiedDown => rows.sort_by(|a, b| b.dir.cmp(&a.dir).then(b.age.cmp(&a.age))),
        }
        rows
    }

    fn files_list(&mut self, r: Rect, st: &State) {
        let view = self.c("bg.view");
        self.canvas.rect(r.x, r.y, r.w, r.h, view);
        let (fg, sec, border) = (
            self.c("fg.text"),
            self.c("fg.text.secondary"),
            self.c("border"),
        );
        let cap = self.n("text.caption");
        let size = self.n("text.body");
        let radius = self.n("radius.control");
        // Columns: name, size (right-aligned), modified.
        let pad = 14.0;
        let size_right = r.x + r.w - 170.0;
        let modified_x = r.x + r.w - 150.0;
        // The header: a click sorts by the column, again reverses it.
        let hb = mix(view, fg, 0.025);
        self.canvas.rect(r.x, r.y, r.w, HEADER_H, hb);
        self.canvas
            .line_h(r.x, r.y + HEADER_H - 1.0, r.w, 1.0, border);
        let base = self.baseline(Face::UiMedium, cap, r.y, HEADER_H);
        let heads = [
            ("Name", r.x + pad + 28.0, Sort::NameUp, false),
            ("Size", size_right, Sort::ModifiedDown, true),
            ("Modified", modified_x, Sort::ModifiedDown, false),
        ];
        for (i, (label, x, sort, right)) in heads.iter().enumerate() {
            let w = self.fonts.width(Face::UiMedium, cap, label);
            let x = if *right { x - w } else { *x };
            let on = (i == 0 && st.sort == Sort::NameUp) || (i == 2 && *sort == st.sort);
            self.text(
                Face::UiMedium,
                cap,
                x,
                base,
                label,
                if on { fg } else { sec },
            );
            if on {
                self.sort_mark(
                    x + w + 6.0,
                    r.y + HEADER_H / 2.0,
                    st.sort == Sort::NameUp,
                    fg,
                );
            }
            if i > 0 {
                let sep = if *right {
                    size_right - 60.0
                } else {
                    modified_x - 12.0
                };
                self.canvas
                    .rect(sep, r.y + 7.0, 1.0, HEADER_H - 14.0, border);
            }
        }
        let row_h = self.n("control.height");
        let old = self.canvas.clip(r.x, r.y + HEADER_H, r.w, r.h - HEADER_H);
        let rows = Self::sorted(st.sort);
        let sel = self.c("bg.selection");
        let accent = self.c("accent");
        for (i, row) in rows.iter().enumerate() {
            let y = r.y + HEADER_H + 4.0 + i as f32 * row_h;
            let selected = st.selected.contains(&i);
            let inner = if selected { sel } else { view };
            if selected {
                // Adjacent selected rows join into one band.
                let up = i > 0 && st.selected.contains(&(i - 1));
                let down = st.selected.contains(&(i + 1));
                let (top, h) = (
                    if up { y - 2.0 } else { y },
                    if up || down { row_h + 2.0 } else { row_h },
                );
                self.canvas.rounded_rect(
                    r.x + 6.0,
                    top,
                    r.w - 12.0,
                    h,
                    if up || down { 2.0 } else { radius },
                    sel,
                );
            }
            if st.cursor == Some(i) {
                self.canvas.stroke_rounded_rect(
                    r.x + 6.0,
                    y,
                    r.w - 12.0,
                    row_h,
                    radius,
                    1.5,
                    accent,
                );
            }
            let icon = if row.dir { Icon::Files } else { Icon::Edit };
            let icol = if row.dir { accent } else { sec };
            self.icon_on(icon, r.x + pad, y + 4.0, row_h - 8.0, icol, inner);
            let base = self.baseline(Face::Ui, size, y, row_h);
            let tx = r.x + pad + 28.0;
            let col = if selected {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            match st.renaming {
                Some((at, typed)) if at == i => {
                    // Rename in place: a field over the name, the stem
                    // selected, Enter keeps it and Escape leaves it.
                    let w = (self.fonts.width(Face::Ui, size, typed) + 60.0).max(180.0);
                    let ctrl = self.c("bg.control");
                    self.canvas
                        .rounded_rect(tx - 6.0, y + 2.0, w, row_h - 4.0, 4.0, ctrl);
                    self.canvas.stroke_rounded_rect(
                        tx - 6.0,
                        y + 2.0,
                        w,
                        row_h - 4.0,
                        4.0,
                        1.5,
                        accent,
                    );
                    let stem = typed.rsplit_once('.').map_or(typed, |(s, _)| s);
                    let sw = self.fonts.width(Face::Ui, size, stem);
                    let hl = mix(ctrl, accent, 0.22);
                    self.canvas
                        .rect(tx - 1.0, y + 6.0, sw + 2.0, row_h - 12.0, hl);
                    self.text(Face::Ui, size, tx, base, typed, fg);
                }
                _ => {
                    self.text(Face::Ui, size, tx, base, row.name, col);
                }
            }
            let sc = if selected { col } else { sec };
            if !row.size.is_empty() {
                let w = self.fonts.width(Face::Ui, size, row.size);
                self.text(Face::Ui, size, size_right - w, base, row.size, sc);
            } else {
                let w = self.fonts.width(Face::Ui, size, "\u{2014}");
                self.text(
                    Face::Ui,
                    size,
                    size_right - w,
                    base,
                    "\u{2014}",
                    self.c("fg.text.disabled"),
                );
            }
            self.text(Face::Ui, size, modified_x, base, row.modified, sc);
        }
        self.canvas.restore_clip(old);
    }

    fn files_status(&mut self, r: Rect, text: &str) {
        let bg = self.c("bg.window");
        self.canvas.rect(r.x, r.y, r.w, r.h, bg);
        let border = self.c("border");
        self.canvas.line_h(r.x, r.y, r.w, 1.0, border);
        let cap = self.n("text.caption");
        let sec = self.c("fg.text.secondary");
        let base = self.baseline(Face::Ui, cap, r.y, r.h);
        self.text(Face::Ui, cap, r.x + 14.0, base, text, sec);
        let free = "412 MB free";
        let w = self.fonts.width(Face::Ui, cap, free);
        self.text(Face::Ui, cap, r.x + r.w - 14.0 - w, base, free, sec);
    }

    /// The application's own confirmation card, on its paper (no glow: the
    /// trusted material is the compositor's alone).
    fn files_confirm(&mut self, body: Rect) {
        let (w, h) = (380.0, 168.0);
        let x = body.x + (body.w - w) / 2.0 + SIDEBAR_W / 2.0;
        let y = body.y + 150.0;
        let radius = self.n("radius.popup");
        let scrim = self.c("shadow");
        let paper = self.c("bg.window");
        let (blur, off, alpha) = (
            self.n("shadow.popup.blur"),
            self.n("shadow.popup.offset"),
            self.n("shadow.popup.alpha") as u8,
        );
        self.canvas
            .shadow(x, y, w, h, radius, blur, off, scrim, alpha);
        self.canvas.rounded_rect(x, y, w, h, radius, paper);
        let border = self.c("border");
        self.canvas
            .stroke_rounded_rect(x, y, w, h, radius, 1.0, border);
        let pad = self.n("padding.window");
        let mut yy = y + pad;
        yy += self.heading(x + pad, yy, "Delete 2 items?") + 8.0;
        yy += self.label(x + pad, yy, "plán.txt and Žďár.md will be deleted.", false) + 2.0;
        self.label(x + pad, yy, "This cannot be undone.", true);
        let bw = 96.0;
        let by = y + h - pad - self.n("control.height");
        // The destructive action in the error colour; Escape or Cancel keep.
        let (ctrl_h, r) = (self.n("control.height"), self.n("radius.control"));
        let (err, on) = (self.c("error"), self.c("fg.text.on-accent"));
        let dx = x + w - pad - bw;
        self.canvas.rounded_rect(dx, by, bw, ctrl_h, r, err);
        let size = self.n("text.body");
        let base = self.baseline(Face::UiMedium, size, by, ctrl_h);
        self.text_centered(Face::UiMedium, size, dx + bw / 2.0, base, "Delete", on);
        self.button(dx - bw - 8.0, by, bw, "Cancel", false);
    }

    fn files_screen(&mut self, st: State) {
        let win = self.files_window();
        self.clear_background(&[(win.r, win.room)]);
        let body = self.clear_window(&win);
        let old = self
            .canvas
            .clip(body.x + 1.0, body.y, body.w - 2.0, body.h - 1.0);
        let view = self.c("bg.view");
        self.body_fill(body, view);
        let toolbar = Rect {
            x: body.x,
            y: body.y,
            w: body.w,
            h: TOOLBAR_H,
        };
        self.files_toolbar(toolbar);
        let main_y = body.y + TOOLBAR_H;
        let main_h = body.h - TOOLBAR_H - STATUS_H;
        self.files_sidebar(Rect {
            x: body.x,
            y: main_y,
            w: SIDEBAR_W,
            h: main_h,
        });
        self.files_list(
            Rect {
                x: body.x + SIDEBAR_W,
                y: main_y,
                w: body.w - SIDEBAR_W,
                h: main_h,
            },
            &st,
        );
        self.files_status(
            Rect {
                x: body.x,
                y: body.y + body.h - STATUS_H,
                w: body.w,
                h: STATUS_H,
            },
            st.status,
        );
        self.canvas.restore_clip(old);
        if st.confirm {
            self.files_confirm(body);
        }
        if let Some((x, y)) = st.pointer {
            self.pointer(x, y);
        }
        self.clear_bar("US", None);
        self.clear_dock(DOCK_REST, RUN_FILES_EDIT, Some(&PROJECT));
    }

    /// Files 1: a folder sorted by name, one file selected.
    pub fn clear_files(&mut self) {
        self.files_screen(State {
            sort: Sort::NameUp,
            selected: &[5],
            cursor: None,
            renaming: None,
            confirm: false,
            status: "9 items \u{b7} notes.txt selected (2.1 kB)",
            pointer: Some((420.0, 318.0)),
        });
    }

    /// Files 2: sorted by modified, newest first (the header clicked); a
    /// range picked with Shift and one more with Command; the cursor ring.
    pub fn clear_files_select(&mut self) {
        self.files_screen(State {
            sort: Sort::ModifiedDown,
            selected: &[3, 4, 5, 7],
            cursor: Some(7),
            renaming: None,
            confirm: false,
            status: "9 items \u{b7} 4 selected (6.1 kB)",
            pointer: Some((560.0, 340.0)),
        });
    }

    /// Files 3: rename in place (Enter or F2), the stem selected.
    pub fn clear_files_rename(&mut self) {
        self.files_screen(State {
            sort: Sort::NameUp,
            selected: &[6],
            cursor: None,
            renaming: Some((6, "plán-v2.txt")),
            confirm: false,
            status: "Enter keeps the name \u{b7} Escape leaves it",
            pointer: None,
        });
    }

    /// Files 4: Delete asks first.
    pub fn clear_files_delete(&mut self) {
        self.files_screen(State {
            sort: Sort::NameUp,
            selected: &[6, 8],
            cursor: None,
            renaming: None,
            confirm: true,
            status: "9 items \u{b7} 2 selected (2.1 kB)",
            pointer: None,
        });
    }
}

//! The "Clear boundaries" screens: rooms, grants on frames, the trusted
//! material, drops and the "show commands" mode. Drawn from a direction's
//! tokens like the classic screens; the data (rooms, windows, sessions) is
//! fixed here, since the mockups illustrate one working day.

use crate::canvas::{Face, Image, blur_image, mix};
use crate::scene::{Icon, LOGICAL_HEIGHT, LOGICAL_WIDTH, Painter, Rect, TermLine};

/// A room: a named set of grants in which programs run (ADR-0172).
pub struct Room {
    pub name: &'static str,
    /// The colour token, or none for the person's own room.
    pub color: Option<&'static str>,
    pub untrusted: bool,
    /// The room's grants in ADR-0153's words.
    pub grants: &'static [&'static str],
}

pub const HOME: Room = Room {
    name: "home",
    color: None,
    untrusted: false,
    grants: &["+w ~"],
};
pub const PROJECT: Room = Room {
    name: "project",
    color: Some("room.1"),
    untrusted: false,
    grants: &["+w ~/project", "+r ~/include"],
};
pub const AGENT: Room = Room {
    name: "agent-1",
    color: Some("room.untrusted"),
    untrusted: true,
    grants: &["+w ~/sandbox/agent-1"],
};

pub enum Content {
    Terminal(Vec<TermLine>, bool),
    Editor(&'static [&'static str]),
    Files,
    Agent,
}

pub struct Win<'r> {
    pub r: Rect,
    pub title: &'static str,
    pub room: &'r Room,
    /// Grants beyond the room's: what this launch designated.
    pub extra: &'static [&'static str],
    pub focused: bool,
    pub content: Content,
}

/// Which gesture a show-commands tag explains.
pub struct Tag {
    pub x: f32,
    pub y: f32,
    pub text: &'static str,
}

/// The dock's state (the architect's dock B): the room chips are always
/// there; a tooltip stands over the hovered icon; in show-commands mode a
/// line is written under every icon and chip and the dock grows for it.
#[derive(Clone, Copy)]
pub struct DockState {
    pub hover: Option<usize>,
    pub commands: bool,
}

pub const DOCK_REST: DockState = DockState {
    hover: None,
    commands: false,
};
pub const DOCK_HOVER: DockState = DockState {
    hover: Some(2),
    commands: false,
};
pub const DOCK_COMMANDS: DockState = DockState {
    hover: None,
    commands: true,
};

/// Height of the label zone under the icons in show-commands mode (two
/// caption lines).
const LABEL_ZONE: f32 = 30.0;

/// The dock's bottom margin.
const DOCK_MARGIN: f32 = 10.0;

pub struct DockGeometry {
    pub r: Rect,
    /// The centre of each application's icon.
    pub centres: Vec<f32>,
    /// Where the room chips start.
    pub chips_x: f32,
}

enum Cell {
    App(usize),
    Divider,
    Chip(&'static Room),
    Plus,
}

/// An installed application: its icon, its name and the `zosh` line that
/// starts it.
pub(crate) struct DockApp {
    pub icon: Icon,
    pub name: &'static str,
    pub command: &'static str,
}

pub(crate) const DOCK_APPS: [DockApp; 5] = [
    DockApp {
        icon: Icon::Terminal,
        name: "Terminal",
        command: "term",
    },
    DockApp {
        icon: Icon::Files,
        name: "Files",
        command: "files ~",
    },
    DockApp {
        icon: Icon::Edit,
        name: "Edit",
        command: "edit FILE",
    },
    DockApp {
        icon: Icon::Agent,
        name: "Agent",
        command: "agent TASK",
    },
    DockApp {
        icon: Icon::Settings,
        name: "Settings",
        command: "settings",
    },
];

/// The rooms each application has windows in on the desktop screen: the
/// terminal in `project` and, hidden, in `home`; the editor in `project`;
/// the agent in `agent-1`.
pub const DESKTOP_RUNNING: &[&[&Room]] = &[&[&PROJECT, &HOME], &[], &[&PROJECT], &[&AGENT], &[]];
/// Other screens' running applications, in the same order as `DOCK_APPS`.
pub const RUN_EDIT: &[&[&Room]] = &[&[], &[], &[&PROJECT], &[], &[]];
pub const RUN_FILES_EDIT: &[&[&Room]] = &[&[], &[&HOME], &[&PROJECT], &[], &[]];
pub const RUN_SETTINGS: &[&[&Room]] = &[&[&PROJECT], &[], &[], &[&AGENT], &[&HOME]];
pub const RUN_TERMINAL: &[&[&Room]] = &[&[&PROJECT], &[], &[], &[&AGENT], &[]];
pub const RUN_TRUST: &[&[&Room]] = &[&[], &[], &[&PROJECT], &[&AGENT], &[]];

/// Signed distance from `p` to a rounded rectangle, negative inside.
fn sdf_rounded(p: (f32, f32), r: &Rect, radius: f32) -> f32 {
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (hx, hy) = (r.w / 2.0 - radius, r.h / 2.0 - radius);
    let qx = (p.0 - cx).abs() - hx;
    let qy = (p.1 - cy).abs() - hy;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

impl Painter<'_> {
    fn room_color(&self, room: &Room) -> Option<u32> {
        room.color.map(|t| self.c(t))
    }

    fn style(&self) -> String {
        self.tokens.str("room.style").to_string()
    }

    /// The strip at the bottom the dock is reserved: nothing else is drawn
    /// in it (fields end above it).
    pub fn dock_strip(&self, commands: bool) -> f32 {
        DOCK_MARGIN + self.dock_height(commands)
    }

    /// The deep wallpaper and its blurred copy; with `wallpaper = rooms`
    /// the rooms are fields on it, computed from the windows on screen
    /// (ADR-0172 point 5): a field is drawn only when two or more rooms
    /// have windows, as the union of a room's windows grown by
    /// `field.padding`, clipped to the work area, and eroded by every
    /// other room's field grown by `field.gap`, so that two fields never
    /// touch.
    pub fn clear_background(&mut self, wins: &[(Rect, &Room)]) {
        self.clear_background_with(wins, false);
    }

    /// `clear_background` with the dock's strip in its show-commands size,
    /// where the dock is grown: the fields are recomputed for the strip.
    pub fn clear_background_with(&mut self, wins: &[(Rect, &Room)], commands: bool) {
        let (w, h) = (self.canvas.width, self.canvas.height);
        let (a, b, c, d) = (
            self.c("wallpaper.a"),
            self.c("wallpaper.b"),
            self.c("wallpaper.c"),
            self.c("wallpaper.d"),
        );
        let img = wallpaper_deep(w, h, a, b, c, d);
        self.canvas.fill_image(&img);
        if self.tokens.str("wallpaper") == "rooms" {
            let mut names: Vec<&str> = wins.iter().map(|(_, r)| r.name).collect();
            names.sort_unstable();
            names.dedup();
            if names.len() >= 2 {
                for name in &names {
                    let room = wins.iter().find(|(_, r)| r.name == *name).map(|(_, r)| *r);
                    let Some(room) = room else { continue };
                    let Some(col) = self.room_color(room) else {
                        continue;
                    };
                    let mine: Vec<Rect> = wins
                        .iter()
                        .filter(|(_, r)| r.name == *name)
                        .map(|(r, _)| *r)
                        .collect();
                    let others: Vec<Rect> = wins
                        .iter()
                        .filter(|(_, r)| r.name != *name && r.color.is_some())
                        .map(|(r, _)| *r)
                        .collect();
                    self.field(&mine, &others, room, col, a, commands);
                }
            }
        }
        // the blurred copy includes the fields: the material blurs what is under it
        let radius = (self.n("blur.panel") * self.canvas.scale) as usize;
        let now = self
            .canvas
            .snapshot(0.0, 0.0, LOGICAL_WIDTH as f32, LOGICAL_HEIGHT as f32);
        let blurred = blur_image(&now.px, w, h, radius);
        self.wallpaper = Some(img);
        self.blurred = Some(blurred);
    }

    /// The work area fields may cover: between the bar and the dock's
    /// strip, `field.gap` from each.
    fn work_area(&self, commands: bool) -> Rect {
        let gap = self.n("field.gap");
        let top = self.n("bar.height") + gap;
        let bottom = LOGICAL_HEIGHT as f32 - self.dock_strip(commands) - gap;
        Rect {
            x: gap,
            y: top,
            w: LOGICAL_WIDTH as f32 - 2.0 * gap,
            h: bottom - top,
        }
    }

    /// One room's field: tint and outline, per pixel from the distance to
    /// the room's padded windows and to the other rooms' padded windows.
    fn field(
        &mut self,
        mine: &[Rect],
        others: &[Rect],
        room: &Room,
        col: u32,
        wallpaper_a: u32,
        commands: bool,
    ) {
        let pad = self.n("field.padding");
        let gap = self.n("field.gap");
        let rr = self.n("radius.window") + pad;
        let work = self.work_area(commands);
        let padded = |r: &Rect| Rect {
            x: r.x - pad,
            y: r.y - pad,
            w: r.w + 2.0 * pad,
            h: r.h + 2.0 * pad,
        };
        let mine: Vec<Rect> = mine.iter().map(padded).collect();
        let others: Vec<Rect> = others.iter().map(padded).collect();
        // bounding box, clipped to the work area
        let x0 = mine
            .iter()
            .map(|r| r.x)
            .fold(f32::MAX, f32::min)
            .max(work.x);
        let y0 = mine
            .iter()
            .map(|r| r.y)
            .fold(f32::MAX, f32::min)
            .max(work.y);
        let x1 = mine
            .iter()
            .map(|r| r.x + r.w)
            .fold(f32::MIN, f32::max)
            .min(work.x + work.w);
        let y1 = mine
            .iter()
            .map(|r| r.y + r.h)
            .fold(f32::MIN, f32::max)
            .min(work.y + work.h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let s = self.canvas.scale;
        let line = mix(col, wallpaper_a, 0.5);
        let (px0, py0) = ((x0 * s).floor() as i32, (y0 * s).floor() as i32);
        let (px1, py1) = ((x1 * s).ceil() as i32, (y1 * s).ceil() as i32);
        for py in py0..py1 {
            for px in px0..px1 {
                let p = ((px as f32 + 0.5) / s, (py as f32 + 0.5) / s);
                let da = mine
                    .iter()
                    .map(|r| sdf_rounded(p, r, rr))
                    .fold(f32::MAX, f32::min);
                if da > 0.5 {
                    continue;
                }
                let db = others
                    .iter()
                    .map(|r| sdf_rounded(p, r, rr))
                    .fold(f32::MAX, f32::min);
                if db < gap - 0.5 {
                    continue;
                }
                let cov = (0.5 - da).clamp(0.0, 1.0) * (db - gap + 0.5).clamp(0.0, 1.0);
                let on_edge = da > -1.0 || db < gap + 1.0;
                let (colour, alpha) = if on_edge { (line, 255.0) } else { (col, 34.0) };
                self.canvas.put(px, py, colour, (cov * alpha) as u8);
            }
        }
        // the label at the top of the field: the room's name and grants
        let top = mine
            .iter()
            .min_by(|a, b| a.y.total_cmp(&b.y))
            .copied()
            .unwrap_or(work);
        // on a small neutral ground, so that it reads on any wallpaper
        let (lx, ly) = (top.x.max(work.x) + 12.0, top.y.max(work.y) + 6.0);
        let size = self.n("text.caption");
        let fg = self.c("fg.text");
        let sec = self.c("fg.text.secondary");
        let label = if room.untrusted {
            format!("{} \u{00B7} untrusted", room.name)
        } else {
            room.name.to_string()
        };
        let grants = room.grants.join("  ");
        let gw = self.fonts.width(Face::UiMedium, size, &label);
        let tw = gw + 12.0 + self.fonts.width(Face::Mono, size, &grants);
        let (hw, hh) = (tw + 16.0, 20.0);
        let ground = self.c("bg.overlay");
        self.canvas
            .rounded_rect_alpha(lx, ly, hw, hh, hh / 2.0, ground, 200);
        let base = self.baseline(Face::UiMedium, size, ly, hh);
        self.text(Face::UiMedium, size, lx + 8.0, base, &label, fg);
        self.text(Face::Mono, size, lx + 8.0 + gw + 12.0, base, &grants, sec);
    }

    /// The trusted material: light through blurred glass. Only surfaces
    /// the compositor draws use it (ADR-0161).
    pub fn glow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let glow = self.c("glow");
        let halo = self.n("glow.halo") as u8;
        let blur = self.n("glow.blur");
        if halo > 0 && blur > 0.0 {
            self.canvas.shadow(x, y, w, h, r, blur, 0.0, glow, halo);
        }
        let tint = self.c("bg.overlay");
        let alpha_t = self.n("panel.alpha") as u8;
        let blur_r = self.n("blur.panel");
        self.canvas
            .rounded_rect_blurred(x, y, w, h, r, blur_r, tint, alpha_t);
        let alpha = self.n("glow.alpha") as u8;
        if alpha > 0 {
            // the inner light: a band from the top edge and a bright rim
            let old = self.canvas.clip(x, y, w, h);
            let band = (h / 3.0).min(48.0);
            self.canvas
                .rect_gradient_v(x, y, w, band, glow, alpha / 2, 0);
            self.canvas.restore_clip(old);
            let rim = mix(tint, glow, alpha as f32 / 255.0);
            self.canvas.stroke_rounded_rect(
                x + 1.0,
                y + 1.0,
                w - 2.0,
                h - 2.0,
                (r - 1.0).max(0.0),
                1.0,
                rim,
            );
        }
        let border = self.c("border");
        self.canvas.stroke_rounded_rect(x, y, w, h, r, 1.0, border);
    }

    /// A small label in the trusted material: the text form of a gesture.
    fn tag(&mut self, x: f32, y: f32, text: &str) -> Rect {
        let size = self.n("text.caption");
        let tw = self.fonts.width(Face::Mono, size, text);
        let (w, h) = (tw + 16.0, 22.0);
        let r = self.n("radius.control");
        self.glow(x, y, w, h, r);
        let fg = self.c("fg.text");
        let base = self.baseline(Face::Mono, size, y, h);
        self.text(Face::Mono, size, x + 8.0, base, text, fg);
        Rect { x, y, w, h }
    }

    fn pill(&mut self, x: f32, y: f32, text: &str, color: u32, fg: u32) -> f32 {
        let size = self.n("text.caption");
        let tw = self.fonts.width(Face::Mono, size, text);
        let (w, h) = (tw + 12.0, 18.0);
        let r = h / 2.0;
        self.canvas.rounded_rect(x, y, w, h, r, color);
        let base = self.baseline(Face::Mono, size, y, h);
        self.text(Face::Mono, size, x + 6.0, base, text, fg);
        w
    }

    /// A window as a sheet of paper, carrying its room's colour the way the
    /// direction's `room.style` says, and its grants in the title bar.
    pub fn clear_window(&mut self, win: &Win) -> Rect {
        let r = win.r;
        let fh = self.n("frame.height");
        let radius = self.n("radius.window");
        let border = self.n("frame.border");
        let style = self.style();
        let room_col = self.room_color(win.room);
        // shadow
        let blur = self.n("shadow.window.blur");
        if blur > 0.0 {
            let (off, col) = (self.n("shadow.window.offset"), self.c("shadow"));
            let alpha = if win.focused {
                self.n("shadow.window.alpha") as u8
            } else {
                self.n("shadow.window.unfocused.alpha") as u8
            };
            let layers = (self.n("shadow.window.layers") as u8).max(1);
            for i in 1..=layers {
                let f = i as f32 / layers as f32;
                self.canvas.shadow(
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    radius,
                    blur * f,
                    off * f,
                    col,
                    alpha / layers,
                );
            }
        }
        let paper = self.c("bg.window");
        let frame = if win.focused {
            self.c("bg.frame.focused")
        } else {
            self.c("bg.frame")
        };
        let (body_bg, frame_bg) = match (style.as_str(), room_col) {
            ("ground", Some(c)) => (mix(paper, c, 0.10), mix(frame, c, 0.18)),
            ("band", Some(c)) => (paper, mix(frame, c, if win.focused { 0.26 } else { 0.18 })),
            _ => (paper, frame),
        };
        self.canvas
            .rounded_rect(r.x, r.y, r.w, r.h, radius, body_bg);
        let old = self.canvas.clip(r.x, r.y, r.w, fh);
        self.canvas
            .rounded_rect(r.x, r.y, r.w, fh + radius + 2.0, radius, frame_bg);
        self.canvas.restore_clip(old);
        let (bw, bcol) = match (style.as_str(), room_col) {
            ("edge", Some(c)) => (self.n("room.edge").max(border), c),
            _ => (border, self.c("border")),
        };
        self.canvas
            .stroke_rounded_rect(r.x, r.y, r.w, r.h, radius, bw, bcol);
        // ADR-0172 point 6: every window of an untrusted room carries an
        // edge in `room.untrusted`, field or no field; a trusted room never
        // has one (INV-0172-10).
        if win.room.untrusted {
            let ew = self.n("room.untrusted.edge");
            if ew > 0.0 {
                let c = self.c("room.untrusted");
                self.canvas
                    .stroke_rounded_rect(r.x, r.y, r.w, r.h, radius, ew, c);
            }
        }
        // buttons: glyphs, coloured on the focused window
        let cy = r.y + fh / 2.0;
        let (close, min) = if win.focused {
            (self.c("button.close"), self.c("button.minimise"))
        } else {
            (self.c("fg.text.disabled"), self.c("fg.text.disabled"))
        };
        let (t, g) = (1.6, 4.5);
        let (cx1, cx2) = (r.x + 18.0, r.x + 42.0);
        self.canvas
            .segment((cx1 - g, cy - g), (cx1 + g, cy + g), t, close);
        self.canvas
            .segment((cx1 + g, cy - g), (cx1 - g, cy + g), t, close);
        self.canvas.line_h(cx2 - g, cy - t / 2.0, 2.0 * g, t, min);
        // the room badge
        let text_col = self.contrasting(frame_bg, win.focused);
        let cap = self.n("text.caption");
        let mut x = r.x + 60.0;
        if let Some(c) = room_col {
            self.canvas.circle(x + 4.0, cy, 4.0, c);
            x += 14.0;
            let base = self.baseline(Face::UiMedium, cap, r.y, fh);
            x += self.text(Face::UiMedium, cap, x, base, win.room.name, text_col) + 6.0;
            if win.room.untrusted {
                x += self.text(Face::Ui, cap, x, base, "untrusted", text_col) + 6.0;
            }
        }
        // the title, centred, or after the badge when the window is narrow
        let size = self.n("text.body");
        let base = self.baseline(Face::UiMedium, size, r.y, fh);
        let tw = self.fonts.width(Face::UiMedium, size, win.title);
        let tx = (r.x + r.w / 2.0 - tw / 2.0).max(x + 10.0);
        self.text(Face::UiMedium, size, tx, base, win.title, text_col);
        // grants, right-aligned, in ADR-0153's words; drawn by the compositor.
        // The window's own grants always; the room's only when they fit
        // beside the title, since the badge already names the room.
        let pill_bg = mix(frame_bg, self.c("fg.text"), 0.07);
        let pill_fg = self.contrasting(pill_bg, false);
        let mut right = r.x + r.w - 10.0;
        let title_right = tx + tw + 12.0;
        let width_of = |p: &mut Self, list: &[&str]| -> f32 {
            list.iter()
                .map(|g| p.fonts.width(Face::Mono, cap, g) + 18.0)
                .sum::<f32>()
        };
        let extra_w = width_of(self, win.extra);
        let room_w = width_of(self, win.room.grants);
        let all: Vec<&str> = if r.x + r.w - 10.0 - extra_w - room_w >= title_right {
            win.room
                .grants
                .iter()
                .chain(win.extra.iter())
                .copied()
                .collect()
        } else {
            win.extra.to_vec()
        };
        for g in all.iter().rev() {
            let w = self.fonts.width(Face::Mono, cap, g) + 12.0;
            right -= w;
            self.pill(right, cy - 9.0, g, pill_bg, pill_fg);
            right -= 6.0;
        }
        // the client's area lies inside the untrusted edge
        let inset = if win.room.untrusted {
            (self.n("room.untrusted.edge") - border).max(0.0)
        } else {
            0.0
        };
        Rect {
            x: r.x + inset,
            y: r.y + fh,
            w: r.w - 2.0 * inset,
            h: r.h - fh - inset,
        }
    }

    fn draw_content(&mut self, body: Rect, content: &Content) {
        match content {
            Content::Terminal(lines, cursor) => self.terminal_body(body, lines, *cursor),
            Content::Editor(lines) => self.editor_body(body, lines),
            Content::Files => self.files_body(body),
            Content::Agent => self.agent_body(body),
        }
    }

    pub fn draw_win(&mut self, win: &Win) -> Rect {
        let body = self.clear_window(win);
        self.draw_content(body, &win.content);
        body
    }

    fn editor_body(&mut self, body: Rect, lines: &[&str]) {
        let view = self.c("bg.view");
        self.body_fill(body, view);
        let size = self.n("text.mono");
        let lh = self.fonts.line_height(Face::Mono, size).ceil();
        let fg = self.c("fg.text");
        let pad = self.n("padding.terminal");
        let old = self
            .canvas
            .clip(body.x + 1.0, body.y + 1.0, body.w - 2.0, body.h - 2.0);
        let mut y = body.y + pad + self.fonts.ascent(Face::Mono, size);
        for line in lines {
            self.text(Face::Mono, size, body.x + pad, y, line, fg);
            y += lh;
        }
        self.canvas.restore_clip(old);
    }

    fn files_body(&mut self, body: Rect) {
        let view = self.c("bg.view");
        self.body_fill(body, view);
        let pad = self.n("padding.window");
        let x = body.x + pad;
        let mut y = body.y + pad * 0.6;
        y += self.label(x, y, "~  \u{203A}  Документи", true) + 6.0;
        let rows = [
            ("Фотографії", "folder", ""),
            ("plán.txt", "file", "1.2 kB"),
            ("příklad.txt", "file", "48 B"),
            ("звіт-2026.txt", "file", "2.1 kB"),
            ("Žďár.md", "file", "913 B"),
        ];
        let row_h = self.n("control.height");
        let size = self.n("text.body");
        let (fg, sec) = (self.c("fg.text"), self.c("fg.text.secondary"));
        let old = self
            .canvas
            .clip(body.x + 1.0, body.y + 1.0, body.w - 2.0, body.h - 2.0);
        for (i, (name, kind, sz)) in rows.iter().enumerate() {
            let ry = y + i as f32 * row_h;
            let ic = if *kind == "folder" {
                Icon::Files
            } else {
                Icon::Edit
            };
            self.icon_on(ic, x, ry + 4.0, row_h - 8.0, sec, view);
            let base = self.baseline(Face::Ui, size, ry, row_h);
            self.text(Face::Ui, size, x + row_h + 2.0, base, name, fg);
            if !sz.is_empty() {
                let w = self.fonts.width(Face::Ui, size, sz);
                self.text(Face::Ui, size, body.x + body.w - pad - w, base, sz, sec);
            }
        }
        self.canvas.restore_clip(old);
    }

    fn agent_body(&mut self, body: Rect) {
        let lines = vec![
            TermLine(vec![("agent-1: reading task.md", "term.fg")]),
            TermLine(vec![("agent-1: writing plan.md", "term.fg")]),
            TermLine(vec![(
                "agent-1: ~/project/notes.txt: not granted",
                "term.red",
            )]),
            TermLine(vec![("agent-1: waiting for a grant", "term.fg")]),
        ];
        self.terminal_body(body, &lines, false);
    }

    /// The system bar in the trusted material: the system menu at the
    /// left, the layout and the clock at the right, and in the middle the
    /// heading of an open sheet or the mode. The room is in the dock's
    /// chips and on every frame, not here (dock B).
    pub fn clear_bar(&mut self, layout: &str, middle: Option<&str>) {
        let h = self.n("bar.height");
        self.glow(
            0.0,
            -self.n("radius.window"),
            LOGICAL_WIDTH as f32,
            h + self.n("radius.window"),
            self.n("radius.window"),
        );
        let fg = self.c("fg.text");
        let size = self.n("text.body");
        let base = self.baseline(Face::UiMedium, size, 0.0, h);
        self.text(Face::UiMedium, size, 14.0, base, "Zoraya", fg);
        let clock = "14:32";
        let cw = self.fonts.width(Face::Ui, size, clock);
        let base_r = self.baseline(Face::Ui, size, 0.0, h);
        self.text(
            Face::Ui,
            size,
            LOGICAL_WIDTH as f32 - cw - 14.0,
            base_r,
            clock,
            fg,
        );
        let lw = self.fonts.width(Face::UiMedium, size, layout);
        self.text(
            Face::UiMedium,
            size,
            LOGICAL_WIDTH as f32 - cw - 14.0 - 18.0 - lw,
            base,
            layout,
            fg,
        );
        if let Some(text) = middle {
            let tw = self.fonts.width(Face::UiMedium, size, text);
            let em = h - 10.0;
            let total = em + 8.0 + tw;
            let x = (LOGICAL_WIDTH as f32 - total) / 2.0;
            self.emblem(x, 5.0, em);
            self.text(Face::UiMedium, size, x + em + 8.0, base, text, fg);
        }
    }

    /// Height of the dock: in show-commands mode the label zone is added.
    pub fn dock_height(&self, commands: bool) -> f32 {
        let icon = self.n("dock.icon");
        let pad = self.n("dock.padding");
        2.0 * pad + icon + 6.0 + if commands { LABEL_ZONE } else { 0.0 }
    }

    /// The dock in its strip at the bottom of the screen.
    pub fn clear_dock(
        &mut self,
        state: DockState,
        rooms: &[&[&Room]],
        current: Option<&Room>,
    ) -> DockGeometry {
        self.dock_at(state, rooms, current, LOGICAL_HEIGHT as f32 - DOCK_MARGIN)
    }

    /// The dock (the architect's B) with its bottom edge at `bottom`:
    /// icons on tiles of neutral material, one dot per room under each
    /// running application (`rooms[i]` lists them), the untrusted
    /// application ringed, then a divider and the room chips, the current
    /// one filled, and `+` for a new room. Cells are equal; a hovered icon
    /// gets a tooltip above the dock; in show-commands mode every icon and
    /// chip has its line under it and the dock grows for them.
    pub fn dock_at(
        &mut self,
        state: DockState,
        rooms: &[&[&Room]],
        current: Option<&Room>,
        bottom: f32,
    ) -> DockGeometry {
        let icon = self.n("dock.icon");
        let pad = self.n("dock.padding");
        let gap = pad;
        let cap = self.n("text.caption");
        let fg = self.c("fg.text");
        let sec = self.c("fg.text.secondary");
        let untrusted_col = self.c("room.untrusted");
        let chip_h: f32 = 26.0;
        let mut cells: Vec<(Cell, f32)> = Vec::new();
        for (i, app) in DOCK_APPS.iter().enumerate() {
            let mut w = icon;
            if state.commands {
                w = w
                    .max(self.fonts.width(Face::UiMedium, cap, app.name))
                    .max(self.fonts.width(Face::Mono, cap, app.command));
            }
            cells.push((Cell::App(i), w.ceil()));
        }
        cells.push((Cell::Divider, 1.0));
        for room in [&HOME, &PROJECT, &AGENT] {
            let mut w = self.chip_width(room, chip_h);
            if state.commands {
                let cmd = format!("room {}", room.name);
                w = w.max(self.fonts.width(Face::Mono, cap, &cmd));
            }
            cells.push((Cell::Chip(room), w.ceil()));
        }
        let mut w = chip_h;
        if state.commands {
            w = w.max(self.fonts.width(Face::Mono, cap, "room new"));
        }
        cells.push((Cell::Plus, w.ceil()));
        let n = cells.len() as f32;
        let w = 2.0 * pad + cells.iter().map(|c| c.1).sum::<f32>() + (n - 1.0) * gap;
        let h = self.dock_height(state.commands);
        let x = (LOGICAL_WIDTH as f32 - w) / 2.0;
        let y = bottom - h;
        let radius = self.n("dock.radius");
        self.glow(x, y, w, h, radius);
        let glow = self.c("glow");
        let shadow = self.c("shadow");
        let border = self.c("border");
        let dark = crate::scene::luminance(self.c("bg.control")) < 0.5;
        let tile_r = self.n("radius.control") * 2.0;
        let iy = y + pad;
        let label_y = iy + icon + 10.0;
        let mut centres = Vec::new();
        let mut chips_x = x;
        let mut cx = x + pad;
        for (cell, cw) in &cells {
            let centre = cx + cw / 2.0;
            match cell {
                Cell::App(i) => {
                    let app = &DOCK_APPS[*i];
                    let ix = centre - icon / 2.0;
                    let hovered = state.hover == Some(*i);
                    centres.push(centre);
                    let base = self.c("bg.control");
                    let (top, bottom_c) = if dark {
                        (
                            mix(base, glow, if hovered { 0.22 } else { 0.12 }),
                            mix(base, shadow, 0.35),
                        )
                    } else {
                        (
                            mix(base, glow, if hovered { 0.9 } else { 0.5 }),
                            mix(base, border, 0.35),
                        )
                    };
                    let glyph_bg = mix(top, bottom_c, 0.5);
                    self.canvas
                        .rounded_rect_gradient_v(ix, iy, icon, icon, tile_r, top, bottom_c);
                    // the inner light at the top, the inner shadow at the bottom
                    let hi = mix(top, glow, if dark { 0.25 } else { 0.7 });
                    self.canvas
                        .line_h(ix + tile_r, iy + 1.0, icon - 2.0 * tile_r, 1.0, hi);
                    let lo = mix(bottom_c, shadow, 0.22);
                    self.canvas
                        .line_h(ix + tile_r, iy + icon - 2.0, icon - 2.0 * tile_r, 1.0, lo);
                    let edge = if dark {
                        mix(border, shadow, 0.5)
                    } else {
                        border
                    };
                    self.canvas
                        .stroke_rounded_rect(ix, iy, icon, icon, tile_r, 1.0, edge);
                    if rooms
                        .get(*i)
                        .is_some_and(|r| r.iter().any(|room| room.untrusted))
                    {
                        self.canvas.stroke_rounded_rect(
                            ix - 2.5,
                            iy - 2.5,
                            icon + 5.0,
                            icon + 5.0,
                            tile_r + 2.5,
                            1.5,
                            untrusted_col,
                        );
                    }
                    let inset = icon * 0.2;
                    let gs = icon - 2.0 * inset;
                    // embossed: the glyph's relief one pixel below it
                    let relief = if dark {
                        mix(bottom_c, shadow, 0.6)
                    } else {
                        glow
                    };
                    self.icon_on(app.icon, ix + inset, iy + inset + 1.0, gs, relief, glyph_bg);
                    self.icon_on(app.icon, ix + inset, iy + inset, gs, fg, glyph_bg);
                    // one dot per room the application has windows in
                    if let Some(list) = rooms.get(*i) {
                        let k = list.len() as f32;
                        let step = 7.0;
                        let mut dx = centre - (k - 1.0) * step / 2.0;
                        for room in list.iter() {
                            let col = self.room_color(room).unwrap_or(sec);
                            self.canvas.circle(dx, iy + icon + 3.5, 2.0, col);
                            dx += step;
                        }
                    }
                    if state.commands {
                        let b1 = label_y + self.fonts.ascent(Face::UiMedium, cap).round();
                        self.text_centered(Face::UiMedium, cap, centre, b1, app.name, fg);
                        self.text_centered(Face::Mono, cap, centre, b1 + 14.0, app.command, sec);
                    }
                }
                Cell::Divider => {
                    self.canvas
                        .rect(cx, iy + icon * 0.15, 1.0, icon * 0.7, border);
                    chips_x = cx + gap;
                }
                Cell::Chip(room) => {
                    let col = self.room_color(room).unwrap_or(sec);
                    let cy = iy + (icon - chip_h) / 2.0;
                    let is_current = current.is_some_and(|c| c.name == room.name);
                    let chip_w = self.chip_width(room, chip_h);
                    let chx = centre - chip_w / 2.0;
                    if is_current {
                        self.canvas.rounded_rect_alpha(
                            chx,
                            cy,
                            chip_w,
                            chip_h,
                            chip_h / 2.0,
                            col,
                            46,
                        );
                        self.canvas.stroke_rounded_rect(
                            chx,
                            cy,
                            chip_w,
                            chip_h,
                            chip_h / 2.0,
                            1.0,
                            col,
                        );
                    } else {
                        let base = self.c("bg.control");
                        self.canvas.rounded_rect_alpha(
                            chx,
                            cy,
                            chip_w,
                            chip_h,
                            chip_h / 2.0,
                            base,
                            120,
                        );
                        self.canvas.stroke_rounded_rect(
                            chx,
                            cy,
                            chip_w,
                            chip_h,
                            chip_h / 2.0,
                            1.0,
                            border,
                        );
                    }
                    let base_t = self.baseline(Face::UiMedium, cap, cy, chip_h);
                    let mut tx = chx + 10.0;
                    self.canvas.circle(tx + 3.5, cy + chip_h / 2.0, 3.5, col);
                    tx += 13.0;
                    tx += self.text(Face::UiMedium, cap, tx, base_t, room.name, fg) + 5.0;
                    if room.untrusted {
                        self.text(Face::Ui, cap, tx, base_t, "untrusted", fg);
                    }
                    if state.commands {
                        let cmd = format!("room {}", room.name);
                        let b2 = label_y + self.fonts.ascent(Face::UiMedium, cap).round() + 14.0;
                        self.text_centered(Face::Mono, cap, centre, b2, &cmd, sec);
                    }
                }
                Cell::Plus => {
                    let cy = iy + (icon - chip_h) / 2.0;
                    let base = self.c("bg.control");
                    let px = centre - chip_h / 2.0;
                    self.canvas
                        .rounded_rect_alpha(px, cy, chip_h, chip_h, chip_h / 2.0, base, 120);
                    self.canvas.stroke_rounded_rect(
                        px,
                        cy,
                        chip_h,
                        chip_h,
                        chip_h / 2.0,
                        1.0,
                        border,
                    );
                    let c = (centre, cy + chip_h / 2.0);
                    self.canvas
                        .segment((c.0 - 5.0, c.1), (c.0 + 5.0, c.1), 1.5, sec);
                    self.canvas
                        .segment((c.0, c.1 - 5.0), (c.0, c.1 + 5.0), 1.5, sec);
                    if state.commands {
                        let b2 = label_y + self.fonts.ascent(Face::UiMedium, cap).round() + 14.0;
                        self.text_centered(Face::Mono, cap, centre, b2, "room new", sec);
                    }
                }
            }
            cx += cw + gap;
        }
        // the tooltip over the hovered icon: the name and the line
        if let Some(i) = state.hover {
            let app = &DOCK_APPS[i];
            let centre = centres[i];
            let tw = self
                .fonts
                .width(Face::UiMedium, cap, app.name)
                .max(self.fonts.width(Face::Mono, cap, app.command));
            let (w, h) = (tw + 20.0, 38.0);
            let (tx, ty) = (centre - w / 2.0, y - 8.0 - h);
            let r = self.n("radius.control");
            self.glow(tx, ty, w, h, r);
            let b1 = ty + 6.0 + self.fonts.ascent(Face::UiMedium, cap).round();
            self.text_centered(Face::UiMedium, cap, centre, b1, app.name, fg);
            self.text_centered(Face::Mono, cap, centre, b1 + 15.0, app.command, fg);
        }
        DockGeometry {
            r: Rect { x, y, w, h },
            centres,
            chips_x,
        }
    }

    fn chip_width(&self, room: &Room, chip_h: f32) -> f32 {
        let cap = self.n("text.caption");
        let mut w = 10.0 + 7.0 + 6.0 + self.fonts.width(Face::UiMedium, cap, room.name) + 10.0;
        if room.untrusted {
            w += 5.0 + self.fonts.width(Face::Ui, cap, "untrusted");
        }
        w.max(chip_h).ceil()
    }

    /// Screen `dock`: the desktop with the dock, the editor's icon hovered.
    pub fn clear_dock_screen(&mut self) {
        let wins = self.desktop_windows(true);
        let placed: Vec<(Rect, &Room)> = wins.iter().map(|w| (w.r, w.room)).collect();
        self.clear_background(&placed);
        for w in &wins {
            self.draw_win(w);
        }
        self.clear_bar("US", None);
        self.clear_dock(DOCK_HOVER, DESKTOP_RUNNING, Some(&PROJECT));
    }

    /// Screen `dock-compare`: the dock at rest, hovered and in
    /// show-commands mode, one under the other.
    pub fn clear_dock_compare(&mut self) {
        self.clear_background(&[]);
        let rows: [(&str, DockState); 3] = [
            (
                "At rest \u{00B7} tiles, one dot per room, the untrusted application ringed, the room chips, the current room filled",
                DOCK_REST,
            ),
            (
                "Hover \u{00B7} the name and the line in a tooltip over the icon; the dock does not change",
                DOCK_HOVER,
            ),
            (
                "Show commands \u{00B7} every icon and chip has its line under it; the dock grows only here",
                DOCK_COMMANDS,
            ),
        ];
        let size = self.n("text.body");
        let fg = self.c("fg.text");
        let mut y = 100.0;
        for (caption, state) in rows.iter() {
            let base = y + self.fonts.ascent(Face::UiMedium, size).round();
            self.text(Face::UiMedium, size, 40.0, base, caption, fg);
            let h = self.dock_height(state.commands);
            let top = y + 30.0 + if state.hover.is_some() { 48.0 } else { 0.0 };
            self.dock_at(*state, DESKTOP_RUNNING, Some(&PROJECT), top + h);
            y = top + h + 44.0;
        }
    }

    /// Screen `trust`: a focused trusted window beside a focused untrusted
    /// one, so that what carries trust can be judged.
    pub fn clear_trust(&mut self) {
        let [edit, agent] = trust_windows();
        self.clear_background(&[(edit.r, &PROJECT), (agent.r, &AGENT)]);
        self.draw_win(&edit);
        self.draw_win(&agent);
        self.clear_bar("US", None);
        self.clear_dock(DOCK_REST, RUN_TRUST, Some(&PROJECT));
    }

    // ---- the day's windows -------------------------------------------------

    fn desktop_windows(&self, focus_edit: bool) -> Vec<Win<'static>> {
        vec![
            Win {
                r: Rect {
                    x: 48.0,
                    y: 80.0,
                    w: 520.0,
                    h: 340.0,
                },
                title: "zosh \u{2014} ~/project",
                room: &PROJECT,
                extra: &[],
                focused: false,
                content: Content::Terminal(session_build(), false),
            },
            Win {
                r: Rect {
                    x: 672.0,
                    y: 96.0,
                    w: 312.0,
                    h: 300.0,
                },
                title: "agent \u{2014} task.md",
                room: &AGENT,
                extra: &["+r task.md"],
                focused: false,
                content: Content::Agent,
            },
            Win {
                r: Rect {
                    x: 120.0,
                    y: 300.0,
                    w: 500.0,
                    h: 350.0,
                },
                title: "notes.txt \u{2014} edit",
                room: &PROJECT,
                extra: &["+w notes.txt"],
                focused: focus_edit,
                content: Content::Editor(NOTES),
            },
        ]
    }

    /// Screen 1: windows of two rooms, a project and an untrusted one.
    pub fn clear_desktop(&mut self) {
        let wins = self.desktop_windows(true);
        let placed: Vec<(Rect, &Room)> = wins.iter().map(|w| (w.r, w.room)).collect();
        self.clear_background(&placed);
        for w in &wins {
            self.draw_win(w);
        }
        self.clear_bar("US", None);
        self.clear_dock(DOCK_REST, DESKTOP_RUNNING, Some(&PROJECT));
    }

    /// Screen 2: the grants on a window's frame, and the full record on
    /// hover, in the trusted material, with the line that made it.
    pub fn clear_grants(&mut self) {
        let win = Win {
            r: Rect {
                x: 112.0,
                y: 90.0,
                w: 800.0,
                h: 520.0,
            },
            title: "notes.txt \u{2014} edit",
            room: &PROJECT,
            extra: &["+w notes.txt"],
            focused: true,
            content: Content::Editor(NOTES),
        };
        self.clear_background(&[(win.r, win.room)]);
        self.draw_win(&win);
        // the hover card under the pills
        let (x, y, w, h) = (560.0, 128.0, 380.0, 196.0);
        let r = self.n("radius.popup");
        self.glow(x, y, w, h, r);
        let pad = 14.0;
        let mut yy = y + pad;
        yy += self.heading(x + pad, yy, "edit has") + 6.0;
        let rows = [
            ("~/project", "change anything in", "room project"),
            ("~/include", "read", "room project"),
            ("~/project/notes.txt", "change", "operand FILE"),
            ("in, out, err", "nothing: a window", ""),
        ];
        let size = self.n("text.caption");
        let (fg, sec) = (self.c("fg.text"), self.c("fg.text.secondary"));
        let lh = 20.0;
        for (path, right, why) in rows {
            let base = self.baseline(Face::Mono, size, yy, lh);
            self.text(Face::Mono, size, x + pad, base, path, fg);
            self.text(Face::Ui, size, x + pad + 150.0, base, right, fg);
            self.text(Face::Ui, size, x + pad + 272.0, base, why, sec);
            yy += lh;
        }
        yy += 8.0;
        let border = self.c("border");
        self.canvas.line_h(x + pad, yy, w - 2.0 * pad, 1.0, border);
        yy += 8.0;
        let base = self.baseline(Face::Ui, size, yy, lh);
        self.text(Face::Ui, size, x + pad, base, "the line that made it:", sec);
        yy += lh;
        let base = self.baseline(Face::Mono, size, yy, lh);
        self.text(
            Face::Mono,
            size,
            x + pad,
            base,
            "room project edit notes.txt",
            fg,
        );
        self.clear_bar("US", None);
        self.clear_dock(DOCK_REST, RUN_EDIT, Some(&PROJECT));
    }

    /// Screen 3: a file dragged from the file manager onto a window of
    /// another room: the target lights up and the grant is shown as text.
    pub fn clear_drop(&mut self) {
        let files_r = Rect {
            x: 40.0,
            y: 90.0,
            w: 440.0,
            h: 360.0,
        };
        let edit_r = Rect {
            x: 556.0,
            y: 150.0,
            w: 420.0,
            h: 420.0,
        };
        self.clear_background(&[(files_r, &HOME), (edit_r, &PROJECT)]);
        let files = Win {
            r: files_r,
            title: "Документи \u{2014} files",
            room: &HOME,
            extra: &[],
            focused: false,
            content: Content::Files,
        };
        self.draw_win(&files);
        let edit = Win {
            r: edit_r,
            title: "notes.txt \u{2014} edit",
            room: &PROJECT,
            extra: &["+w notes.txt"],
            focused: true,
            content: Content::Editor(NOTES),
        };
        self.draw_win(&edit);
        // the drop target in the trusted material: a rim of light
        let glow = self.c("glow");
        let alpha = (self.n("glow.alpha") as u8).max(90);
        let r = self.n("radius.window");
        let halo = self.n("glow.halo") as u8;
        let blur = self.n("glow.blur");
        if blur > 0.0 {
            self.canvas.shadow(
                edit.r.x,
                edit.r.y,
                edit.r.w,
                edit.r.h,
                r,
                blur,
                0.0,
                glow,
                halo.max(60),
            );
        }
        let rim = mix(self.c("accent"), glow, 0.3);
        let _ = alpha;
        self.canvas.stroke_rounded_rect(
            edit.r.x - 2.0,
            edit.r.y - 2.0,
            edit.r.w + 4.0,
            edit.r.h + 4.0,
            r + 2.0,
            2.0,
            rim,
        );
        // the dragged entry
        let (gx, gy) = (604.0, 330.0);
        let r_ctrl = self.n("radius.control");
        self.glow(gx, gy, 170.0, 30.0, r_ctrl);
        let fg = self.c("fg.text");
        let tint = self.c("bg.overlay");
        self.icon_on(Icon::Edit, gx + 8.0, gy + 5.0, 20.0, fg, tint);
        let size = self.n("text.body");
        let base = self.baseline(Face::Ui, size, gy, 30.0);
        self.text(Face::Ui, size, gx + 34.0, base, "звіт-2026.txt", fg);
        self.pointer(gx + 160.0, gy + 24.0);
        // what the drop means, as text
        let (tx, ty) = (gx - 40.0, gy + 42.0);
        let sec = self.c("fg.text.secondary");
        let w = 300.0;
        let h = 58.0;
        self.glow(tx, ty, w, h, r_ctrl);
        let cap = self.n("text.caption");
        let base = self.baseline(Face::Ui, cap, ty + 6.0, 20.0);
        self.text(
            Face::Ui,
            cap,
            tx + 10.0,
            base,
            "edit will be given this file, to read:",
            sec,
        );
        let base = self.baseline(Face::Mono, cap, ty + 28.0, 20.0);
        self.text(
            Face::Mono,
            cap,
            tx + 10.0,
            base,
            "+r ~/Документи/звіт-2026.txt edit",
            fg,
        );
        self.clear_bar("UA", None);
        self.clear_dock(DOCK_REST, RUN_FILES_EDIT, Some(&PROJECT));
    }

    pub(crate) fn pointer(&mut self, x: f32, y: f32) {
        let fg = self.c("fg.text");
        let bg = self.c("bg.view");
        let pts = [
            (x, y),
            (x, y + 16.0),
            (x + 4.0, y + 12.5),
            (x + 7.0, y + 18.0),
            (x + 9.5, y + 17.0),
            (x + 6.5, y + 11.5),
            (x + 11.5, y + 11.5),
        ];
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            self.canvas.segment(a, b, 2.4, bg);
        }
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            self.canvas.segment(a, b, 1.2, fg);
        }
    }

    /// Screen 4: the chooser as a sheet in the trusted material.
    pub fn clear_chooser(&mut self) {
        let edit = Win {
            r: Rect {
                x: 112.0,
                y: 80.0,
                w: 800.0,
                h: 560.0,
            },
            title: "notes.txt \u{2014} edit",
            room: &PROJECT,
            extra: &["+w notes.txt"],
            focused: true,
            content: Content::Editor(NOTES),
        };
        self.clear_background(&[(edit.r, edit.room)]);
        let body = self.draw_win(&edit);
        let title = "Open a file for edit";
        let (sw, sh) = (520.0, 420.0);
        let sx = edit.r.x + (edit.r.w - sw) / 2.0;
        let sy = body.y;
        let radius = self.n("radius.window");
        let old = self.canvas.clip(body.x, body.y, body.w, body.h);
        self.glow(sx, sy - radius, sw, sh + radius, radius);
        self.canvas.restore_clip(old);
        // the strip the compositor paints: emblem and heading
        let strip = 36.0;
        let em = strip - 14.0;
        self.emblem(sx + 12.0, sy + 7.0, em);
        let fg = self.c("fg.text");
        let tsize = self.n("text.body");
        let base = self.baseline(Face::UiMedium, tsize, sy, strip);
        self.text(Face::UiMedium, tsize, sx + 12.0 + em + 8.0, base, title, fg);
        let border = self.c("border");
        self.canvas.line_h(sx, sy + strip - 1.0, sw, 1.0, border);
        let pad = self.n("padding.window");
        let x = sx + pad;
        let mut y = sy + strip + pad * 0.7;
        y += self.label(x, y, "~  \u{203A}  project", true) + 8.0;
        let list = Rect {
            x,
            y,
            w: sw - 2.0 * pad,
            h: 220.0,
        };
        let view = self.c("bg.view");
        let r_ctrl = self.n("radius.control");
        self.canvas
            .rounded_rect(list.x, list.y, list.w, list.h, r_ctrl, view);
        self.canvas
            .stroke_rounded_rect(list.x, list.y, list.w, list.h, r_ctrl, 1.0, border);
        let rows = [
            ("include", "folder", ""),
            ("main.c", "file", "1.9 kB"),
            ("Makefile", "file", "412 B"),
            ("notes.txt", "file", "2.1 kB"),
            ("plán.txt", "file", "1.2 kB"),
            ("звіт-2026.txt", "file", "2.1 kB"),
        ];
        let row_h = self.n("control.height");
        let size = self.n("text.body");
        let sec = self.c("fg.text.secondary");
        for (i, (name, kind, sz)) in rows.iter().enumerate() {
            let ry = list.y + 6.0 + i as f32 * row_h;
            let selected = i == 4;
            if selected {
                let sel = self.c("bg.selection");
                self.canvas
                    .rounded_rect(list.x + 4.0, ry, list.w - 8.0, row_h, r_ctrl, sel);
            }
            let ic = if *kind == "folder" {
                Icon::Files
            } else {
                Icon::Edit
            };
            let inner = if selected {
                self.c("bg.selection")
            } else {
                view
            };
            self.icon_on(ic, list.x + 10.0, ry + 4.0, row_h - 8.0, sec, inner);
            let base = self.baseline(Face::Ui, size, ry, row_h);
            let col = if selected {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            self.text(Face::Ui, size, list.x + 12.0 + row_h, base, name, col);
            if !sz.is_empty() {
                let w = self.fonts.width(Face::Ui, size, sz);
                self.text(Face::Ui, size, list.x + list.w - 12.0 - w, base, sz, sec);
            }
        }
        let by = sy + sh - pad - row_h;
        let bw = 88.0;
        self.button(sx + sw - pad - bw, by, bw, "Open", true);
        self.button(sx + sw - pad - 2.0 * bw - 8.0, by, bw, "Cancel", false);
        let cap = self.n("text.caption");
        let hy = list.y + list.h + 14.0;
        let base = self.baseline(Face::Ui, cap, hy, 18.0);
        self.text(
            Face::Ui,
            cap,
            x,
            base,
            "edit will be given this file, to read:",
            sec,
        );
        let base = self.baseline(Face::Mono, cap, hy + 20.0, 18.0);
        self.text(Face::Mono, cap, x, base, "+r ~/project/plán.txt edit", fg);
        self.clear_bar("CZ", Some(title));
        self.clear_dock(DOCK_REST, RUN_EDIT, Some(&PROJECT));
    }

    /// Screen 5: the rooms overview, where rooms are seen, entered and made.
    pub fn clear_rooms(&mut self) {
        let wins = self.desktop_windows(false);
        let placed: Vec<(Rect, &Room)> = wins.iter().map(|w| (w.r, w.room)).collect();
        self.clear_background(&placed);
        for w in &wins {
            self.draw_win(w);
        }
        let (w, h) = (880.0, 440.0);
        let x = (LOGICAL_WIDTH as f32 - w) / 2.0;
        let y = (LOGICAL_HEIGHT as f32 - h) / 2.0 - 10.0;
        let r = self.n("radius.popup");
        self.glow(x, y, w, h, r);
        let pad = 20.0;
        let tsize = self.n("text.title");
        let fg = self.c("fg.text");
        let sec = self.c("fg.text.secondary");
        let base = y + pad + self.fonts.ascent(Face::UiMedium, tsize).round();
        self.text(Face::UiMedium, tsize, x + pad, base, "Rooms", fg);
        let cap = self.n("text.caption");
        let base_c = self.baseline(
            Face::Ui,
            cap,
            y + pad,
            self.fonts.line_height(Face::UiMedium, tsize),
        );
        let hint = "System+R \u{00B7} in zosh: room";
        let hw = self.fonts.width(Face::Ui, cap, hint);
        self.text(Face::Ui, cap, x + w - pad - hw, base_c, hint, sec);
        let cards: [(&Room, &str, &str); 3] = [
            (
                &HOME,
                "3 windows",
                "your own: the terminal, files, settings",
            ),
            (&PROJECT, "2 windows", "zosh, edit"),
            (&AGENT, "1 window", "agent, without the chooser"),
        ];
        let cy = y + pad + 44.0;
        let cw = (w - 2.0 * pad - 3.0 * 12.0) / 4.0;
        let ch = h - pad - 44.0 - pad;
        let r_card = self.n("radius.control") * 2.0;
        let paper = self.c("bg.window");
        let border = self.c("border");
        let size = self.n("text.body");
        for (i, (room, count, who)) in cards.iter().enumerate() {
            let cx = x + pad + i as f32 * (cw + 12.0);
            self.canvas.rounded_rect(cx, cy, cw, ch, r_card, paper);
            self.canvas
                .stroke_rounded_rect(cx, cy, cw, ch, r_card, 1.0, border);
            let col = self.room_color(room);
            if let Some(c) = col {
                let old = self.canvas.clip(cx, cy, cw, 6.0);
                self.canvas
                    .rounded_rect(cx, cy, cw, r_card * 2.0, r_card, c);
                self.canvas.restore_clip(old);
            }
            let mut yy = cy + 18.0;
            let base = yy + self.fonts.ascent(Face::UiMedium, size).round();
            // the name in the text colour: the card's band carries the colour
            self.text(Face::UiMedium, size, cx + 14.0, base, room.name, fg);
            if room.untrusted {
                let nw = self.fonts.width(Face::UiMedium, size, room.name);
                self.text(Face::Ui, size, cx + 14.0 + nw + 8.0, base, "untrusted", fg);
            }
            yy += 26.0;
            let base = yy + self.fonts.ascent(Face::Ui, cap).round();
            self.text(Face::Ui, cap, cx + 14.0, base, count, sec);
            yy += 18.0;
            let base = yy + self.fonts.ascent(Face::Ui, cap).round();
            self.text(Face::Ui, cap, cx + 14.0, base, who, sec);
            yy += 30.0;
            self.canvas.line_h(cx + 14.0, yy, cw - 28.0, 1.0, border);
            yy += 12.0;
            for g in room.grants {
                let base = yy + self.fonts.ascent(Face::Mono, cap).round();
                self.text(Face::Mono, cap, cx + 14.0, base, g, fg);
                yy += 20.0;
            }
            if room.untrusted {
                let base = yy + self.fonts.ascent(Face::Ui, cap).round();
                self.text(
                    Face::Ui,
                    cap,
                    cx + 14.0,
                    base,
                    "no chooser; drops ask first",
                    sec,
                );
            }
            let by = cy + ch - 14.0 - self.n("control.height");
            if i == 1 {
                self.button(cx + 14.0, by, cw - 28.0, "Here", false);
            } else {
                self.button(cx + 14.0, by, cw - 28.0, "Enter", false);
            }
            let text = format!("room {}", room.name);
            let base = self.baseline(Face::Mono, cap, by - 22.0, 18.0);
            self.text(Face::Mono, cap, cx + 14.0, base, &text, sec);
        }
        // the new room
        let cx = x + pad + 3.0 * (cw + 12.0);
        self.canvas
            .rounded_rect(cx, cy, cw, ch, r_card, mix(paper, fg, 0.03));
        self.canvas
            .stroke_rounded_rect(cx, cy, cw, ch, r_card, 1.0, border);
        let mut yy = cy + 18.0;
        let base = yy + self.fonts.ascent(Face::UiMedium, size).round();
        self.text(Face::UiMedium, size, cx + 14.0, base, "New room", fg);
        yy += 34.0;
        let view = self.c("bg.view");
        let r_ctrl = self.n("radius.control");
        let ctl_h = self.n("control.height");
        for (label, value) in [("name", "agent-2"), ("grant", "+w ~/sandbox/agent-2")] {
            let base = yy + self.fonts.ascent(Face::Ui, cap).round();
            self.text(Face::Ui, cap, cx + 14.0, base, label, sec);
            yy += 16.0;
            self.canvas
                .rounded_rect(cx + 14.0, yy, cw - 28.0, ctl_h, r_ctrl, view);
            self.canvas
                .stroke_rounded_rect(cx + 14.0, yy, cw - 28.0, ctl_h, r_ctrl, 1.0, border);
            let base = self.baseline(Face::Mono, cap, yy, ctl_h);
            self.text(Face::Mono, cap, cx + 22.0, base, value, fg);
            yy += ctl_h + 10.0;
        }
        self.checkbox(cx + 14.0, yy, "untrusted", true);
        let by = cy + ch - 14.0 - ctl_h;
        self.button(cx + 14.0, by, cw - 28.0, "Create", true);
        let base = self.baseline(Face::Mono, cap, by - 40.0, 18.0);
        self.text(
            Face::Mono,
            cap,
            cx + 14.0,
            base,
            "room new agent-2 --untrusted",
            sec,
        );
        let base = self.baseline(Face::Mono, cap, by - 22.0, 18.0);
        self.text(
            Face::Mono,
            cap,
            cx + 14.0,
            base,
            "  +w ~/sandbox/agent-2",
            sec,
        );
        self.clear_bar("US", None);
        self.clear_dock(DOCK_REST, DESKTOP_RUNNING, Some(&PROJECT));
    }

    /// Screen 6: the switcher, grouped by room, with the windows' own
    /// pixels as thumbnails.
    pub fn clear_switcher(&mut self) {
        let wins = self.desktop_windows(true);
        let placed: Vec<(Rect, &Room)> = wins.iter().map(|w| (w.r, w.room)).collect();
        self.clear_background(&placed);
        for w in &wins {
            self.draw_win(w);
        }
        let shots: Vec<Image> = wins
            .iter()
            .map(|w| self.canvas.snapshot(w.r.x, w.r.y, w.r.w, w.r.h))
            .collect();
        self.clear_bar("US", None);
        self.clear_dock(DOCK_REST, DESKTOP_RUNNING, Some(&PROJECT));
        let (w, h) = (640.0, 372.0);
        let x = (LOGICAL_WIDTH as f32 - w) / 2.0;
        let y = (LOGICAL_HEIGHT as f32 - h) / 2.0 - 20.0;
        let r = self.n("radius.popup");
        self.glow(x, y, w, h, r);
        let pad = 16.0;
        let cap = self.n("text.caption");
        let size = self.n("text.body");
        let (fg, sec) = (self.c("fg.text"), self.c("fg.text.secondary"));
        let groups: [(&Room, &[usize]); 2] = [(&PROJECT, &[0, 2]), (&AGENT, &[1])];
        let mut yy = y + pad;
        let row_h = 84.0;
        let th = row_h - 16.0;
        let r_ctrl = self.n("radius.control");
        let selected = 2usize;
        for (room, idx) in groups {
            let col = self.room_color(room).unwrap_or(fg);
            self.canvas.circle(x + pad + 4.0, yy + 8.0, 4.0, col);
            let base = self.baseline(Face::UiMedium, cap, yy, 16.0);
            // the name in the text colour: the dot carries the colour
            let nw = self.text(Face::UiMedium, cap, x + pad + 14.0, base, room.name, fg);
            if room.untrusted {
                self.text(
                    Face::Ui,
                    cap,
                    x + pad + 14.0 + nw + 6.0,
                    base,
                    "untrusted",
                    fg,
                );
            }
            let grants = room.grants.join("  ");
            let gw = self.fonts.width(Face::Mono, cap, &grants);
            self.text(Face::Mono, cap, x + w - pad - gw, base, &grants, sec);
            yy += 22.0;
            for &i in idx {
                let win = &wins[i];
                if i == selected {
                    let sel = self.c("bg.selection");
                    self.canvas
                        .rounded_rect(x + pad, yy, w - 2.0 * pad, row_h, r_ctrl, sel);
                }
                let tw = (th * win.r.w / win.r.h).min(160.0);
                self.canvas
                    .image_scaled(&shots[i], x + pad + 8.0, yy + 8.0, tw, th, 3.0);
                let border = self.c("border.strong");
                self.canvas
                    .stroke_rounded_rect(x + pad + 8.0, yy + 8.0, tw, th, 3.0, 1.0, border);
                let base = self.baseline(Face::Ui, size, yy, row_h);
                let tcol = if i == selected {
                    self.c("fg.text.on-selection")
                } else {
                    fg
                };
                self.text(Face::Ui, size, x + pad + 8.0 + 170.0, base, win.title, tcol);
                if !win.extra.is_empty() {
                    let extra = win.extra.join("  ");
                    let ew = self.fonts.width(Face::Mono, cap, &extra);
                    self.text(Face::Mono, cap, x + w - pad - 8.0 - ew, base, &extra, sec);
                }
                yy += row_h + 6.0;
            }
            yy += 10.0;
        }
        let base = self.baseline(Face::Ui, cap, y + h - pad - 16.0, 16.0);
        self.text(Face::Ui, cap, x + pad, base, "Alt+Tab", sec);
    }

    /// Screen 7: settings, each page with only its own parameters.
    pub fn clear_settings(&mut self) {
        let win = Win {
            r: Rect {
                x: 192.0,
                y: 96.0,
                w: 640.0,
                h: 500.0,
            },
            title: "Settings",
            room: &HOME,
            extra: &["+w /config/settings"],
            focused: true,
            content: Content::Editor(&[]),
        };
        self.clear_background(&[(win.r, win.room)]);
        let body = self.clear_window(&win);
        let ground = self.c("bg.panel");
        self.body_fill(body, ground);
        let pad = self.n("padding.window");
        let side_w = 150.0;
        let border = self.c("border");
        self.canvas
            .rect(body.x + side_w, body.y, 1.0, body.h - 1.0, border);
        let radius = self.n("radius.control");
        let row_h = self.n("control.height");
        let size = self.n("text.body");
        let fg = self.c("fg.text");
        for (i, item) in ["Appearance", "Keyboard", "Rooms", "About"]
            .iter()
            .enumerate()
        {
            let ry = body.y + pad + i as f32 * (row_h + 4.0);
            if i == 0 {
                let sel = self.c("bg.selection");
                self.canvas
                    .rounded_rect(body.x + 10.0, ry, side_w - 20.0, row_h, radius, sel);
            }
            let base = self.baseline(Face::Ui, size, ry, row_h);
            let col = if i == 0 {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            self.text(Face::Ui, size, body.x + 22.0, base, item, col);
        }
        let x = body.x + side_w + pad;
        let cw = body.w - side_w - 2.0 * pad;
        let mut y = body.y + pad;
        let tsize = self.n("text.title");
        let asc = self.fonts.ascent(Face::UiMedium, tsize);
        self.text(Face::UiMedium, tsize, x, y + asc, "Appearance", fg);
        y += self.fonts.line_height(Face::UiMedium, tsize) + 12.0;
        let r1 = self.row_height(false);
        let r3 = self.row_height(true);
        let card = self.card(x, y, cw, 2.0 * r1 + 2.0 * r3);
        let mut ry = y;
        let right = self.card_row(card, ry, "Theme", None, false);
        let light = self.appearance == crate::tokens::Appearance::Light;
        self.radio(right - 150.0, ry + (r1 - 18.0) / 2.0, "Light", light);
        self.radio(right - 70.0, ry + (r1 - 18.0) / 2.0, "Dark", !light);
        ry += r1;
        let right = self.card_row(card, ry, "Scale", None, false);
        self.radio(right - 230.0, ry + (r1 - 18.0) / 2.0, "1\u{00D7}", true);
        self.radio(right - 170.0, ry + (r1 - 18.0) / 2.0, "2\u{00D7}", false);
        self.radio(right - 110.0, ry + (r1 - 18.0) / 2.0, "Automatic", false);
        ry += r1;
        let right = self.card_row(
            card,
            ry,
            "Motion",
            Some("Animate the switcher, the rooms and sheets."),
            false,
        );
        self.switch(
            right - 36.0,
            ry + (r1 - 20.0) / 2.0,
            self.n("motion.short") > 0.0,
        );
        ry += r3;
        let right = self.card_row(
            card,
            ry,
            "Wallpaper follows the time of day",
            Some("Only the wallpaper changes; nothing else does."),
            true,
        );
        self.switch(right - 36.0, ry + (r1 - 20.0) / 2.0, false);
        y += card.h + 12.0;
        let sec = self.c("fg.text.secondary");
        let cap = self.n("text.caption");
        let asc = self.fonts.ascent(Face::Mono, cap);
        self.text(Face::Mono, cap, x, y + asc, "settings gui.theme light \u{00B7} gui.scale 1 \u{00B7} gui.motion yes \u{00B7} gui.wallpaper-by-time no", sec);
        self.clear_bar("US", None);
        self.clear_dock(DOCK_REST, RUN_SETTINGS, Some(&HOME));
    }

    /// Screen 8: a terminal with `zosh` in a room.
    pub fn clear_terminal(&mut self) {
        let win = Win {
            r: Rect {
                x: 72.0,
                y: 70.0,
                w: 880.0,
                h: 600.0,
            },
            title: "zosh \u{2014} ~/project",
            room: &PROJECT,
            extra: &[],
            focused: true,
            content: Content::Terminal(session_rooms(), true),
        };
        self.clear_background(&[(win.r, win.room)]);
        self.draw_win(&win);
        self.clear_bar("UA", None);
        self.clear_dock(DOCK_REST, RUN_TERMINAL, Some(&PROJECT));
    }

    /// Screen 9: the desktop in "show commands" mode: every gesture the
    /// compositor owns is labelled with its text in `zosh`.
    pub fn clear_commands(&mut self) {
        let wins = self.desktop_windows(true);
        let placed: Vec<(Rect, &Room)> = wins.iter().map(|w| (w.r, w.room)).collect();
        self.clear_background_with(&placed, true);
        for w in &wins {
            self.draw_win(w);
        }
        self.clear_bar("US", Some("show commands \u{00B7} System+? to leave"));
        let geo = self.clear_dock(DOCK_COMMANDS, DESKTOP_RUNNING, Some(&PROJECT));
        let edit = &wins[2].r;
        let agent = &wins[1].r;
        let fh = self.n("frame.height");
        let dock_y = geo.r.y;
        let bar = self.n("bar.height");
        let tags = [
            Tag {
                x: edit.x + 8.0,
                y: edit.y - 26.0,
                text: "kill edit  \u{00B7}  bg edit",
            },
            Tag {
                x: edit.x + edit.w - 110.0,
                y: edit.y + fh + 6.0,
                text: "grants edit",
            },
            Tag {
                x: edit.x + edit.w / 2.0 - 50.0,
                y: edit.y - 26.0,
                text: "room project",
            },
            Tag {
                x: agent.x + 100.0,
                y: agent.y + agent.h + 8.0,
                text: "room agent-1 \u{00B7} untrusted",
            },
            Tag {
                x: agent.x + 8.0,
                y: agent.y + agent.h + 8.0,
                text: "kill agent",
            },
            Tag {
                x: LOGICAL_WIDTH as f32 - 290.0,
                y: bar + 8.0,
                text: "settings gui.keyboard-layout ua",
            },
            Tag {
                x: geo.chips_x,
                y: dock_y - 30.0,
                text: "room  \u{00B7}  System+R",
            },
        ];
        for t in tags {
            self.tag(t.x, t.y, t.text);
        }
    }
}

const NOTES: &[&str] = &[
    "# Poznámky k etapě 7",
    "",
    "- kompozitor kreslí rámy a výdeje",
    "- místnost = sada výdejů (ADR-0172)",
    "- Завтра: написати звіт для архітектора.",
    "",
    "Co má edit: +w ~/project, +r ~/include,",
    "a tento soubor. Nic jiného.",
];

/// The desktop's project terminal: a build.
/// The `trust` screen's windows: a focused editor in `project` beside a
/// focused agent in `agent-1`.
pub(crate) fn trust_windows() -> [Win<'static>; 2] {
    [
        Win {
            r: Rect {
                x: 40.0,
                y: 110.0,
                w: 440.0,
                h: 430.0,
            },
            title: "notes.txt \u{2014} edit",
            room: &PROJECT,
            extra: &["+w notes.txt"],
            focused: true,
            content: Content::Editor(NOTES),
        },
        Win {
            r: Rect {
                x: 536.0,
                y: 110.0,
                w: 448.0,
                h: 430.0,
            },
            title: "agent \u{2014} task.md",
            room: &AGENT,
            extra: &["+r task.md"],
            focused: true,
            content: Content::Agent,
        },
    ]
}

fn session_build() -> Vec<TermLine> {
    let p = "project ~/project> ";
    vec![
        TermLine(vec![(p, "term.green"), ("tcc main.c -o main", "term.fg")]),
        TermLine(vec![(p, "term.green"), ("./main", "term.fg")]),
        TermLine(vec![("Привіт, світе!", "term.fg")]),
        TermLine(vec![(p, "term.green"), ("ls", "term.fg")]),
        TermLine(vec![
            ("include/", "term.blue"),
            ("  main  main.c  Makefile  notes.txt  plán.txt", "term.fg"),
        ]),
        TermLine(vec![(p, "term.green"), ("edit notes.txt &", "term.fg")]),
        TermLine(vec![("[1] edit", "term.yellow")]),
        TermLine(vec![(p, "term.green")]),
    ]
}

/// The terminal screen: rooms in `zosh`.
fn session_rooms() -> Vec<TermLine> {
    let p = "project ~/project> ";
    vec![
        TermLine(vec![(p, "term.green"), ("room", "term.fg")]),
        TermLine(vec![("home      +w ~", "term.fg")]),
        TermLine(vec![
            ("project   +w ~/project  +r ~/include", "term.fg"),
            ("   here", "term.cyan"),
        ]),
        TermLine(vec![
            ("agent-1   +w ~/sandbox/agent-1", "term.fg"),
            ("   untrusted", "term.yellow"),
        ]),
        TermLine(vec![
            (p, "term.green"),
            ("grants tcc main.c -o main", "term.fg"),
        ]),
        TermLine(vec![("tcc gets:", "term.fg")]),
        TermLine(vec![("  in, out, err         the terminal", "term.fg")]),
        TermLine(vec![(
            "  ~/project            change anything in (room project)",
            "term.fg",
        )]),
        TermLine(vec![(
            "  ~/include            read (room project)",
            "term.fg",
        )]),
        TermLine(vec![(
            "  ~/project/main.c     read (operand FILE)",
            "term.fg",
        )]),
        TermLine(vec![(
            "  ~/project            create or replace main (operand -o)",
            "term.fg",
        )]),
        TermLine(vec![(p, "term.green"), ("tcc main.c -o main", "term.fg")]),
        TermLine(vec![(p, "term.green"), ("./main", "term.fg")]),
        TermLine(vec![("Привіт, світе!", "term.fg")]),
        TermLine(vec![
            (p, "term.green"),
            (
                "room new agent-2 --untrusted +w ~/sandbox/agent-2",
                "term.fg",
            ),
        ]),
        TermLine(vec![
            (p, "term.green"),
            ("room agent-2 ./agent task.md", "term.fg"),
        ]),
        TermLine(vec![(
            "agent: ~/project/notes.txt: not granted",
            "term.red",
        )]),
        TermLine(vec![(
            "  hint: agent tried to read ~/project/notes.txt, which it was not given",
            "term.fg",
        )]),
        TermLine(vec![(
            "  to allow it: +r ~/project/notes.txt room agent-2 ./agent task.md",
            "term.fg",
        )]),
        TermLine(vec![(p, "term.green"), ("room", "term.fg")]),
        TermLine(vec![("home      +w ~", "term.fg")]),
        TermLine(vec![
            ("project   +w ~/project  +r ~/include", "term.fg"),
            ("   here", "term.cyan"),
        ]),
        TermLine(vec![
            ("agent-1   +w ~/sandbox/agent-1", "term.fg"),
            ("   untrusted", "term.yellow"),
        ]),
        TermLine(vec![
            ("agent-2   +w ~/sandbox/agent-2", "term.fg"),
            ("   untrusted", "term.yellow"),
        ]),
        TermLine(vec![(p, "term.green")]),
    ]
}

/// The deep wallpaper: a diagonal gradient from `a` to `b`, four soft
/// blobs of `c`, `d`, a darker `a` and a lighter `b`, a gentle fold and a
/// vignette. Pure arithmetic, deterministic, no image.
pub fn wallpaper_deep(w: usize, h: usize, a: u32, b: u32, c: u32, d: u32) -> Vec<u32> {
    let mut img = vec![0; w * h];
    let (fw, fh) = (w as f32, h as f32);
    let dark_a = mix(a, 0x000000, 0.35);
    let light_b = mix(b, 0xFFFFFF, 0.35);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / fw, y as f32 / fh);
            let t = (fx * 0.5 + fy * 0.5).clamp(0.0, 1.0);
            let mut col = mix(a, b, t);
            let blob = |cx: f32, cy: f32, r: f32| -> f32 {
                let dx = fx - cx;
                let dy = (fy - cy) * (fh / fw);
                let dd = (dx * dx + dy * dy).sqrt() / r;
                let k = (1.0 - dd).clamp(0.0, 1.0);
                k * k * (3.0 - 2.0 * k)
            };
            col = mix(col, c, 0.85 * blob(0.68, 0.22, 0.42));
            col = mix(col, d, 0.75 * blob(0.22, 0.78, 0.40));
            col = mix(col, dark_a, 0.55 * blob(0.92, 0.88, 0.30));
            col = mix(col, light_b, 0.50 * blob(0.40, 0.38, 0.22));
            // a gentle fold across the field
            let fold = ((fx * 9.0 + fy * 4.0).sin() * 0.5 + 0.5) * 0.06;
            col = mix(col, 0xFFFFFF, fold);
            // vignette
            let vx = (fx - 0.5).abs();
            let vy = (fy - 0.5).abs();
            let v = ((vx * vx + vy * vy).sqrt() - 0.35).clamp(0.0, 0.4) * 0.35;
            col = mix(col, 0x000000, v);
            img[y * w + x] = col;
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;

    /// The cost estimate for ADR-0161: `cargo test -p zoraya-mockups --release -- --ignored --nocapture glow_cost`.
    #[test]
    #[ignore]
    fn glow_cost() {
        for scale in [1usize, 2] {
            let mut c = Canvas::new(LOGICAL_WIDTH, LOGICAL_HEIGHT, scale);
            c.smooth = true;
            let img = wallpaper_deep(c.width, c.height, 0x203040, 0xC0D0E0, 0x80A0C0, 0xA08090);
            c.fill_image(&img);
            let (x, y, w, h, r) = (192.0, 178.0, 640.0, 372.0, 12.0);
            let n = 5;
            let t = std::time::Instant::now();
            let mut px = 0;
            for _ in 0..n {
                px = c.rounded_rect_blurred(x, y, w, h, r, 36.0, 0xF4F5F7, 190);
            }
            let glass = t.elapsed() / n;
            let t = std::time::Instant::now();
            for _ in 0..n {
                c.shadow(x, y, w, h, r, 22.0, 0.0, 0xFFFFFF, 70);
            }
            let halo = t.elapsed() / n;
            let t = std::time::Instant::now();
            for _ in 0..n {
                c.rect_gradient_v(x, y, w, 48.0, 0xFFFFFF, 55, 0);
                c.stroke_rounded_rect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, r - 1.0, 1.0, 0xFFFFFF);
            }
            let light = t.elapsed() / n;
            println!(
                "glow at scale {scale}: surface {w}x{h} logical; blur of {px} physical pixels {glass:?}; halo {halo:?}; inner light {light:?}"
            );
        }
    }

    /// INV-0161-3 applied to the clear screens, on the backgrounds the
    /// tool composes (ADR-0172's fields, chips, badges, the material): every
    /// text keeps 4.5 to 1 in both sets, at the darkest and the lightest
    /// pixel of the wallpaper. `--nocapture` prints the README's table.
    #[test]
    fn clear_field_text_contrast() {
        use crate::canvas::blend;
        use crate::tokens::{Appearance, Tokens, contrast};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/research/desktop/mockups/clear-field/tokens.txt");
        let t = Tokens::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let rooms = ["room.1", "room.2", "room.3", "room.4", "room.untrusted"];
        let mut rows: Vec<(String, Vec<f64>)> = Vec::new();
        let mut push = |what: String, app: usize, ratio: f64| match rows
            .iter_mut()
            .find(|(w, _)| *w == what)
        {
            Some((_, v)) => v[app] = ratio,
            None => {
                let mut v = vec![0.0, 0.0];
                v[app] = ratio;
                rows.push((what, v));
            }
        };
        for (ai, app) in [Appearance::Light, Appearance::Dark]
            .into_iter()
            .enumerate()
        {
            let c = |n: &str| t.color(n, app);
            let wp = wallpaper_deep(
                256,
                192,
                c("wallpaper.a"),
                c("wallpaper.b"),
                c("wallpaper.c"),
                c("wallpaper.d"),
            );
            let lum = crate::tokens::luminance;
            let darkest = *wp
                .iter()
                .min_by(|a, b| lum(**a).total_cmp(&lum(**b)))
                .unwrap();
            let lightest = *wp
                .iter()
                .max_by(|a, b| lum(**a).total_cmp(&lum(**b)))
                .unwrap();
            let extremes = [darkest, lightest];
            // the worst ratio of a text over a background composed on each extreme
            let worst = |fg: u32, bg: &dyn Fn(u32) -> u32| -> f64 {
                extremes
                    .iter()
                    .map(|w| contrast(fg, bg(*w)))
                    .fold(f64::MAX, f64::min)
            };
            let alpha_t = t.num("panel.alpha") as u8;
            let glow = c("glow");
            // the inner light band of a surface of height h, at a text's top y
            let band = |h: f32, y: f32| -> u8 {
                let bh = (h / 3.0).min(48.0);
                let a0 = t.num("glow.alpha") / 2.0;
                (a0 * (1.0 - y / bh).max(0.0)) as u8
            };
            let material = |w: u32| blend(w, c("bg.overlay"), alpha_t);
            let (fg, sec) = (c("fg.text"), c("fg.text.secondary"));
            let header = |w: u32, col: u32| blend(blend(w, col, 34), c("bg.overlay"), 200);
            for room in rooms {
                let col = c(room);
                push(
                    format!("field header name, `fg.text` on the header of `{room}`"),
                    ai,
                    worst(fg, &|w| header(w, col)),
                );
                push(
                    format!("field header grants, `fg.text.secondary` on the header of `{room}`"),
                    ai,
                    worst(sec, &|w| header(w, col)),
                );
            }
            push(
                "bar, `fg.text` on the material".into(),
                ai,
                worst(fg, &material),
            );
            push(
                "show-commands name under an icon, `fg.text` on the material".into(),
                ai,
                worst(fg, &material),
            );
            push(
                "show-commands line under an icon or chip, `fg.text.secondary` on the material"
                    .into(),
                ai,
                worst(sec, &material),
            );
            let tip = |w: u32| blend(material(w), glow, band(38.0, 6.0));
            push(
                "tooltip name and line, `fg.text` in the tooltip".into(),
                ai,
                worst(fg, &tip),
            );
            let chip_band = band(2.0 * t.num("dock.padding") + t.num("dock.icon") + 6.0, 17.0);
            let other_chip =
                |w: u32| blend(blend(material(w), glow, chip_band), c("bg.control"), 120);
            push(
                "chip of another room, `fg.text` on `bg.control` over the material".into(),
                ai,
                worst(fg, &other_chip),
            );
            for room in rooms {
                let col = c(room);
                let current = |w: u32| blend(blend(material(w), glow, chip_band), col, 46);
                push(
                    format!("current chip, `fg.text` on `{room}` at 46 over the material"),
                    ai,
                    worst(fg, &current),
                );
            }
            // the title bar's text: `fg.text.secondary` on an unfocused bar when it
            // keeps 4.5 to 1 there, `fg.text` otherwise (Painter::contrasting)
            let on = |bg: u32, focused: bool| -> u32 {
                if !focused && contrast(sec, bg) >= 4.5 {
                    sec
                } else {
                    fg
                }
            };
            for room in rooms {
                let col = c(room);
                for (frame, which, focused) in [
                    ("bg.frame.focused", "focused", true),
                    ("bg.frame", "other", false),
                ] {
                    let bar = mix(c(frame), col, 0.18);
                    let title = on(bar, focused);
                    let which_text = if focused {
                        "`fg.text`"
                    } else {
                        "`fg.text.secondary` where it keeps 4.5, else `fg.text`"
                    };
                    push(
                        format!(
                            "title and badge on the {which} title bar tinted by `{room}`, {which_text}"
                        ),
                        ai,
                        contrast(title, bar),
                    );
                    let pill = mix(bar, fg, 0.07);
                    push(
                        format!(
                            "grant pill on the {which} title bar tinted by `{room}`, `fg.text.secondary` where it keeps 4.5, else `fg.text`"
                        ),
                        ai,
                        contrast(on(pill, false), pill),
                    );
                }
                push(
                    format!("body text, `fg.text` on `bg.window` tinted by `{room}`"),
                    ai,
                    contrast(fg, mix(c("bg.window"), col, 0.10)),
                );
            }
        }
        println!("| Text on background | Light | Dark |");
        println!("|---|---|---|");
        let mut failed = Vec::new();
        for (what, v) in &rows {
            println!("| {what} | {:.1} | {:.1} |", v[0], v[1]);
            if v.iter().any(|r| *r < 4.5) {
                failed.push(what.clone());
            }
        }
        // why the badge's name is not in the room's colour: the pairs that
        // are not used, printed for the README and not asserted
        println!();
        println!("| Not used: a room's colour as text on its tinted title bar | Light | Dark |");
        println!("|---|---|---|");
        for room in rooms {
            for (frame, which) in [("bg.frame.focused", "focused"), ("bg.frame", "other")] {
                let v: Vec<f64> = [Appearance::Light, Appearance::Dark]
                    .into_iter()
                    .map(|app| {
                        let col = t.color(room, app);
                        contrast(col, mix(t.color(frame, app), col, 0.18))
                    })
                    .collect();
                println!(
                    "| `{room}` on the {which} title bar | {:.1} | {:.1} |",
                    v[0], v[1]
                );
            }
        }
        assert!(failed.is_empty(), "under 4.5 to 1: {failed:#?}");
    }

    /// INV-0172-10 on the tool's own render: every window of an untrusted
    /// room carries an edge of `room.untrusted.edge` pixels in
    /// `room.untrusted`, beside a trusted window (`trust`), alone with no
    /// field, and maximised; the trusted window's edge carries no room
    /// colour. Without fonts the text is boxes; the edges are what is read.
    #[test]
    fn untrusted_windows_carry_the_edge() {
        use crate::canvas::FontSet;
        use crate::tokens::{Appearance, Tokens};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/research/desktop/mockups/clear-field/tokens.txt");
        let t = Tokens::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let edge = t.num("room.untrusted.edge") as usize;
        assert!(
            edge >= 2,
            "room.untrusted.edge = {edge}: two pixels at least"
        );
        // the pixels of a window's left and right edges at mid-height, `edge` deep
        let edge_pixels = |p: &Painter, r: Rect| -> Vec<u32> {
            let row = (r.y + r.h / 2.0) as usize * p.canvas.width;
            let (x0, x1) = (r.x as usize, (r.x + r.w) as usize);
            (0..edge)
                .flat_map(|i| [p.canvas.px[row + x0 + i], p.canvas.px[row + x1 - 1 - i]])
                .collect()
        };
        for app in [Appearance::Light, Appearance::Dark] {
            let untrusted = t.color("room.untrusted", app);
            let rooms: Vec<u32> = ["room.1", "room.2", "room.3", "room.4", "room.untrusted"]
                .iter()
                .map(|n| t.color(n, app))
                .collect();
            let mut fonts = FontSet::new();
            fn painter<'a>(fonts: &'a mut FontSet, t: &'a Tokens, app: Appearance) -> Painter<'a> {
                let mut canvas = Canvas::new(LOGICAL_WIDTH, LOGICAL_HEIGHT, 1);
                canvas.smooth = true;
                Painter {
                    canvas,
                    wallpaper: None,
                    blurred: None,
                    fonts,
                    tokens: t,
                    appearance: app,
                }
            }
            // 1. beside a trusted window, with fields
            let mut p = painter(&mut fonts, &t, app);
            p.clear_trust();
            let [edit, agent] = trust_windows();
            for px in edge_pixels(&p, agent.r) {
                assert_eq!(
                    px, untrusted,
                    "{app:?}: the untrusted window's edge in trust"
                );
            }
            for px in edge_pixels(&p, edit.r) {
                assert!(
                    !rooms.contains(&px),
                    "{app:?}: a room colour on the trusted window's edge"
                );
            }
            // 2. alone: one room on screen, no field
            let mut p = painter(&mut fonts, &t, app);
            p.clear_background(&[(agent.r, &AGENT)]);
            p.draw_win(&agent);
            for px in edge_pixels(&p, agent.r) {
                assert_eq!(px, untrusted, "{app:?}: the untrusted window's edge alone");
            }
            // 3. maximised: the work area
            let mut p = painter(&mut fonts, &t, app);
            let [_, mut big] = trust_windows();
            big.r = p.work_area(false);
            p.clear_background(&[(big.r, &AGENT)]);
            p.draw_win(&big);
            for px in edge_pixels(&p, big.r) {
                assert_eq!(
                    px, untrusted,
                    "{app:?}: the untrusted window's edge maximised"
                );
            }
        }
    }

    #[test]
    fn deep_wallpaper_is_deterministic_and_varied() {
        let a = wallpaper_deep(96, 72, 0x203040, 0xC0D0E0, 0x80A0C0, 0xA08090);
        let b = wallpaper_deep(96, 72, 0x203040, 0xC0D0E0, 0x80A0C0, 0xA08090);
        assert_eq!(a, b);
        assert!(a.iter().all(|p| *p <= 0xFFFFFF));
        let distinct: std::collections::HashSet<u32> = a.iter().copied().collect();
        assert!(distinct.len() > 500, "{} distinct colours", distinct.len());
    }

    #[test]
    fn rooms_have_grants_in_the_shell_syntax() {
        for room in [&HOME, &PROJECT, &AGENT] {
            assert!(!room.grants.is_empty());
            for g in room.grants {
                assert!(g.starts_with("+r ") || g.starts_with("+w "), "{g}");
            }
        }
        assert!(AGENT.untrusted && !PROJECT.untrusted && HOME.color.is_none());
    }
}

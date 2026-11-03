//! The five screens of the mockups, drawn from a direction's tokens.

use crate::canvas::{Canvas, Face, FontSet, mix};
use crate::tokens::{Appearance, Tokens};

pub const LOGICAL_WIDTH: usize = 1024;
pub const LOGICAL_HEIGHT: usize = 768;

pub struct Painter<'a> {
    pub canvas: Canvas,
    pub fonts: &'a mut FontSet,
    pub tokens: &'a Tokens,
    pub appearance: Appearance,
    /// The wallpaper (procedural, `wallpaper = gradient`) and its blurred
    /// copy, computed once per screen as the compositor would compute them
    /// once per wallpaper change; the bar, the dock and the switcher are
    /// drawn from the blurred copy.
    pub wallpaper: Option<Vec<u32>>,
    pub blurred: Option<Vec<u32>>,
}

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A terminal line: text and the colour token of each run.
pub struct TermLine(pub Vec<(&'static str, &'static str)>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Normal,
    System,
}

impl Painter<'_> {
    pub(crate) fn c(&self, name: &str) -> u32 {
        self.tokens.color(name, self.appearance)
    }
    pub(crate) fn n(&self, name: &str) -> f32 {
        self.tokens.num(name)
    }

    pub fn text(&mut self, face: Face, size: f32, x: f32, y: f32, s: &str, color: u32) -> f32 {
        self.fonts
            .draw(&mut self.canvas, face, size, x, y, s, color)
    }

    pub(crate) fn text_centered(
        &mut self,
        face: Face,
        size: f32,
        cx: f32,
        y: f32,
        s: &str,
        color: u32,
    ) {
        let w = self.fonts.width(face, size, s);
        self.text(face, size, cx - w / 2.0, y, s, color);
    }

    /// Baseline for text vertically centred in a box of height `h` at `y`.
    pub(crate) fn baseline(&self, face: Face, size: f32, y: f32, h: f32) -> f32 {
        let asc = self.fonts.ascent(face, size);
        let lh = self.fonts.line_height(face, size);
        (y + (h - lh) / 2.0 + asc).round()
    }

    /// The text token that contrasts most with `bg`; for an unfocused frame
    /// the secondary text if it still contrasts.
    pub(crate) fn contrasting(&self, bg: u32, focused: bool) -> u32 {
        let lb = luminance(bg);
        if !focused {
            let sec = self.c("fg.text.secondary");
            if crate::tokens::contrast(sec, bg) >= 4.5 {
                return sec;
            }
        }
        ["fg.text", "fg.text.on-accent", "fg.text.on-selection"]
            .iter()
            .map(|n| self.c(n))
            .max_by(|a, b| {
                (luminance(*a) - lb)
                    .abs()
                    .total_cmp(&(luminance(*b) - lb).abs())
            })
            .unwrap_or(0)
    }

    pub fn desktop_background(&mut self) {
        if self.tokens.str("wallpaper") == "gradient" {
            let (w, h) = (self.canvas.width, self.canvas.height);
            let (a, b, c) = (
                self.c("wallpaper.a"),
                self.c("wallpaper.b"),
                self.c("wallpaper.c"),
            );
            let img = wallpaper(w, h, a, b, c);
            let radius = (self.n("blur.panel") * self.canvas.scale) as usize;
            let blurred = crate::canvas::blur_image(&img, w, h, radius);
            self.canvas.fill_image(&img);
            self.wallpaper = Some(img);
            self.blurred = Some(blurred);
        } else {
            let bg = self.c("bg.desktop");
            self.canvas.fill(bg);
        }
    }

    pub(crate) fn dock_layout(&self) -> bool {
        self.tokens.str("desktop.layout") == "dock"
    }

    /// A translucent surface: the blurred wallpaper tinted, or the plain
    /// colour where there is no wallpaper.
    pub(crate) fn glass(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, tint: u32) {
        let alpha = self.n("panel.alpha") as u8;
        if let Some(img) = self.blurred.take() {
            self.canvas
                .rounded_rect_from(x, y, w, h, r, &img, tint, alpha);
            self.blurred = Some(img);
        } else {
            self.canvas.rounded_rect(x, y, w, h, r, tint);
        }
    }

    /// Draws a window frame and returns its body rectangle.
    pub fn window(&mut self, r: Rect, title: &str, focused: bool, kind: Kind) -> Rect {
        let fh = self.n("frame.height");
        let radius = self.n("radius.window");
        let border = self.n("frame.border");
        let blur = self.n("shadow.window.blur");
        if blur > 0.0 {
            let (off, alpha, col) = (
                self.n("shadow.window.offset"),
                self.n("shadow.window.alpha") as u8,
                self.c("shadow"),
            );
            let alpha = if focused {
                alpha
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
        let frame_bg = match (kind, focused) {
            (Kind::System, _) => self.c("bg.frame.system"),
            (_, true) => self.c("bg.frame.focused"),
            (_, false) => self.c("bg.frame"),
        };
        let body_bg = self.c("bg.window");
        self.canvas
            .rounded_rect(r.x, r.y, r.w, r.h, radius, body_bg);
        // title bar with rounded top corners only
        let old = self.canvas.clip(r.x, r.y, r.w, fh);
        self.canvas
            .rounded_rect(r.x, r.y, r.w, fh + radius + 2.0, radius, frame_bg);
        self.canvas.restore_clip(old);
        if self.tokens.str("frame.separator") == "yes" {
            let sep = if focused {
                self.c("border.strong")
            } else {
                self.c("border")
            };
            self.canvas.line_h(r.x, r.y + fh - border, r.w, border, sep);
        }
        let border_col = match (self.tokens.str("frame.border.focused"), focused) {
            ("accent", true) => self.c("accent"),
            ("accent", false) => self.c("border.strong"),
            _ => self.c("border"),
        };
        self.canvas
            .stroke_rounded_rect(r.x, r.y, r.w, r.h, radius, border, border_col);
        // buttons
        // The title's colour is whichever text token contrasts most with
        // the title bar: a solid accent-coloured bar (the quiet direction)
        // takes light text on a light theme.
        let text_col = self.contrasting(frame_bg, focused);
        let cy = r.y + fh / 2.0;
        if self.tokens.str("frame.buttons") == "circles" {
            let (close, min) = (self.c("button.close"), self.c("button.minimise"));
            let (close, min) = if focused {
                (close, min)
            } else {
                (mix(close, frame_bg, 0.55), mix(min, frame_bg, 0.55))
            };
            self.canvas.circle(r.x + 14.0, cy, 6.0, close);
            self.canvas.circle(r.x + 34.0, cy, 6.0, min);
        } else if self.tokens.str("frame.buttons") == "glyphs" {
            // Monochrome glyphs: coloured on the focused window (and on
            // hover), grey on the others.
            let (close, min) = if focused {
                (self.c("button.close"), self.c("button.minimise"))
            } else {
                (self.c("fg.text.disabled"), self.c("fg.text.disabled"))
            };
            let t = 1.6;
            let g = 4.5;
            let (cx1, cx2) = (r.x + 18.0, r.x + 42.0);
            self.canvas
                .segment((cx1 - g, cy - g), (cx1 + g, cy + g), t, close);
            self.canvas
                .segment((cx1 + g, cy - g), (cx1 - g, cy + g), t, close);
            self.canvas.line_h(cx2 - g, cy - t / 2.0, 2.0 * g, t, min);
        } else {
            let col = text_col;
            let s = 11.0;
            let bx = r.x + 8.0;
            let by = cy - s / 2.0;
            self.canvas.stroke_rounded_rect(bx, by, s, s, 0.0, 1.0, col);
            self.canvas
                .segment((bx + 3.0, by + 3.0), (bx + s - 3.0, by + s - 3.0), 1.2, col);
            self.canvas
                .segment((bx + s - 3.0, by + 3.0), (bx + 3.0, by + s - 3.0), 1.2, col);
            let bx = r.x + 8.0 + s + 6.0;
            self.canvas.stroke_rounded_rect(bx, by, s, s, 0.0, 1.0, col);
            self.canvas
                .line_h(bx + 3.0, by + s - 4.0, s - 6.0, 1.5, col);
        }
        let size = self.n("text.body");
        let base = self.baseline(Face::UiMedium, size, r.y, fh);
        self.text_centered(Face::UiMedium, size, r.x + r.w / 2.0, base, title, text_col);
        Rect {
            x: r.x,
            y: r.y + fh,
            w: r.w,
            h: r.h - fh,
        }
    }

    /// Fills a window body with a colour, keeping the window's bottom corners.
    pub fn body_fill(&mut self, body: Rect, color: u32) {
        let radius = self.n("radius.window");
        let border = self.n("frame.border");
        let old = self.canvas.clip(
            body.x + border,
            body.y,
            body.w - 2.0 * border,
            body.h - border,
        );
        self.canvas.rounded_rect(
            body.x,
            body.y - radius - 2.0,
            body.w,
            body.h + radius + 2.0,
            radius,
            color,
        );
        self.canvas.restore_clip(old);
    }

    pub fn terminal(
        &mut self,
        r: Rect,
        title: &str,
        focused: bool,
        lines: &[TermLine],
        cursor: bool,
    ) {
        let body = self.window(r, title, focused, Kind::Normal);
        self.terminal_body(body, lines, cursor);
    }

    /// The grid of a terminal inside a window body.
    pub fn terminal_body(&mut self, body: Rect, lines: &[TermLine], cursor: bool) {
        let bg = self.c("term.bg");
        self.body_fill(body, bg);
        let size = self.n("text.mono");
        let cell = self.fonts.cell_width(Face::Mono, size).ceil();
        let lh = self
            .fonts
            .line_height(Face::Mono, size)
            .ceil()
            .max(size + 4.0);
        let pad = self.n("padding.terminal");
        let old = self
            .canvas
            .clip(body.x + 1.0, body.y + 1.0, body.w - 2.0, body.h - 2.0);
        let asc = self.fonts.ascent(Face::Mono, size).round();
        let mut y = body.y + pad + asc;
        let mut last_x = body.x + pad;
        // the grid wraps at the last column, as a terminal does
        let columns = (((body.w - 2.0 * pad) / cell).floor() as usize).max(1);
        for line in lines {
            let mut x = body.x + pad;
            let mut column = 0usize;
            for (text, tok) in &line.0 {
                let col = self.c(tok);
                let chars: Vec<char> = text.chars().collect();
                let mut start = 0;
                while start < chars.len() {
                    let room = columns - column;
                    let end = (start + room).min(chars.len());
                    let piece: String = chars[start..end].iter().collect();
                    self.fonts.draw_cells(
                        &mut self.canvas,
                        Face::Mono,
                        size,
                        x,
                        y,
                        cell,
                        &piece,
                        col,
                    );
                    let n = end - start;
                    x += n as f32 * cell;
                    column += n;
                    start = end;
                    if column >= columns && start < chars.len() {
                        column = 0;
                        x = body.x + pad;
                        y += lh;
                    }
                }
            }
            last_x = x;
            y += lh;
        }
        if cursor {
            let fg = self.c("term.fg");
            self.canvas.rect(
                last_x,
                y - lh - asc + (lh - size) / 2.0,
                cell,
                size + 2.0,
                fg,
            );
        }
        self.canvas.restore_clip(old);
    }

    pub fn panel(&mut self, windows: &[(&str, bool)], layout: &str) {
        let h = self.n("panel.height");
        let y = LOGICAL_HEIGHT as f32 - h;
        let bg = self.c("bg.panel");
        self.canvas.rect(0.0, y, LOGICAL_WIDTH as f32, h, bg);
        let border = self.c("border");
        self.canvas
            .line_h(0.0, y, LOGICAL_WIDTH as f32, 1.0, border);
        let accent = self.c("accent");
        let on_accent = self.c("fg.text.on-accent");
        let fg = self.c("fg.text");
        let secondary = self.c("fg.text.secondary");
        let radius = self.n("radius.control");
        // launcher
        let s = h - 12.0;
        let mut x = 8.0;
        self.canvas.rounded_rect(x, y + 6.0, s, s, radius, accent);
        let size = self.n("text.heading");
        let base = self.baseline(Face::UiMedium, size, y + 6.0, s);
        self.text_centered(Face::UiMedium, size, x + s / 2.0, base, "Z", on_accent);
        x += s + 10.0;
        // application icons
        let icons = [Icon::Terminal, Icon::Files, Icon::Edit, Icon::Settings];
        for icon in icons {
            self.icon(icon, x, y + 6.0, s, secondary);
            x += s + 6.0;
        }
        x += 8.0;
        self.canvas.rect(x, y + 8.0, 1.0, h - 16.0, border);
        x += 12.0;
        // window list
        let size = self.n("text.body");
        for (title, focused) in windows {
            let w = self.fonts.width(Face::Ui, size, title) + 20.0;
            if *focused {
                let sel = self.c("bg.selection");
                self.canvas
                    .rounded_rect(x, y + 6.0, w, h - 12.0, radius, sel);
            }
            let base = self.baseline(Face::Ui, size, y, h);
            let col = if *focused {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            self.text(Face::Ui, size, x + 10.0, base, title, col);
            x += w + 6.0;
        }
        // right side: layout and clock
        let clock = "14:32";
        let cw = self.fonts.width(Face::Ui, size, clock);
        let base = self.baseline(Face::Ui, size, y, h);
        self.text(
            Face::Ui,
            size,
            LOGICAL_WIDTH as f32 - cw - 12.0,
            base,
            clock,
            fg,
        );
        let lw = self.fonts.width(Face::UiMedium, size, layout) + 12.0;
        let lx = LOGICAL_WIDTH as f32 - cw - 12.0 - 12.0 - lw;
        let ctrl = self.c("bg.control");
        self.canvas
            .rounded_rect(lx, y + 8.0, lw, h - 16.0, radius, ctrl);
        self.canvas
            .stroke_rounded_rect(lx, y + 8.0, lw, h - 16.0, radius, 1.0, border);
        let base = self.baseline(Face::UiMedium, size, y + 8.0, h - 16.0);
        self.text_centered(Face::UiMedium, size, lx + lw / 2.0, base, layout, fg);
    }

    pub(crate) fn icon(&mut self, icon: Icon, x: f32, y: f32, s: f32, color: u32) {
        let inner = self.c("bg.panel");
        self.icon_on(icon, x, y, s, color, inner);
    }

    /// `icon` with the colour behind it (the gear's ring is a hole).
    pub(crate) fn icon_on(&mut self, icon: Icon, x: f32, y: f32, s: f32, color: u32, inner: u32) {
        let t = 1.5 * (s / 24.0).max(1.0);
        match icon {
            Icon::Terminal => {
                self.canvas
                    .stroke_rounded_rect(x + 2.0, y + 3.0, s - 4.0, s - 6.0, 3.0, t, color);
                self.canvas
                    .segment((x + 6.0, y + 8.0), (x + 10.0, y + s / 2.0), t, color);
                self.canvas
                    .segment((x + 10.0, y + s / 2.0), (x + 6.0, y + s - 8.0), t, color);
                self.canvas
                    .line_h(x + 12.0, y + s - 8.0, s - 18.0, t, color);
            }
            Icon::Files => {
                self.canvas
                    .stroke_rounded_rect(x + 2.0, y + 5.0, s - 4.0, s - 8.0, 2.0, t, color);
                self.canvas
                    .rect(x + 2.0, y + 5.0, s / 2.0 - 2.0, 4.0, color);
            }
            Icon::Edit => {
                self.canvas
                    .stroke_rounded_rect(x + 4.0, y + 2.0, s - 8.0, s - 4.0, 2.0, t, color);
                for i in 0..3 {
                    self.canvas
                        .line_h(x + 8.0, y + 8.0 + i as f32 * 4.0, s - 16.0, t, color);
                }
            }
            Icon::Agent => {
                // a head with two eyes and an antenna: the program that acts
                // on its own
                self.canvas
                    .stroke_rounded_rect(x + 3.0, y + 7.0, s - 6.0, s - 10.0, 4.0, t, color);
                let eye = 1.4 * (s / 24.0).max(1.0);
                self.canvas.circle(x + s * 0.36, y + s * 0.58, eye, color);
                self.canvas.circle(x + s * 0.64, y + s * 0.58, eye, color);
                self.canvas
                    .segment((x + s / 2.0, y + 7.0), (x + s / 2.0, y + 3.0), t, color);
                self.canvas.circle(x + s / 2.0, y + 2.5, eye, color);
            }
            Icon::Settings => {
                // a gear: eight teeth, a ring, a hole
                let c = (x + s / 2.0, y + s / 2.0);
                let k = (s / 24.0).max(1.0);
                let r_out = s / 2.0 - 2.0;
                let r_ring = r_out - 2.5 * k;
                for i in 0..8 {
                    let a = std::f32::consts::PI * i as f32 / 4.0;
                    let (ca, sa) = (a.cos(), a.sin());
                    self.canvas.segment(
                        (c.0 + ca * (r_ring - 1.0), c.1 + sa * (r_ring - 1.0)),
                        (c.0 + ca * r_out, c.1 + sa * r_out),
                        3.0 * k,
                        color,
                    );
                }
                self.canvas.circle(c.0, c.1, r_ring, color);
                self.canvas.circle(c.0, c.1, r_ring - 2.0 * t, inner);
                self.canvas.circle(c.0, c.1, 2.5 * k, color);
            }
        }
    }

    pub fn button(&mut self, x: f32, y: f32, w: f32, label: &str, primary: bool) {
        let h = self.n("control.height");
        let radius = self.n("radius.control");
        let (bg, fg) = if primary {
            (self.c("accent"), self.c("fg.text.on-accent"))
        } else {
            (self.c("bg.control"), self.c("fg.text"))
        };
        self.canvas.rounded_rect(x, y, w, h, radius, bg);
        if !primary {
            let b = self.c("border");
            self.canvas.stroke_rounded_rect(x, y, w, h, radius, 1.0, b);
        }
        let size = self.n("text.body");
        let base = self.baseline(Face::UiMedium, size, y, h);
        self.text_centered(Face::UiMedium, size, x + w / 2.0, base, label, fg);
    }

    pub(crate) fn radio(&mut self, x: f32, y: f32, label: &str, on: bool) {
        let (accent, border, fg, ctrl) = (
            self.c("accent"),
            self.c("border.strong"),
            self.c("fg.text"),
            self.c("bg.control"),
        );
        let r = 7.0;
        self.canvas
            .circle(x + r, y + r, r, if on { accent } else { ctrl });
        if !on {
            self.canvas.circle(x + r, y + r, r - 1.0, ctrl);
            self.canvas.circle(x + r, y + r, r, border);
            self.canvas.circle(x + r, y + r, r - 1.2, ctrl);
        } else {
            let on_accent = self.c("fg.text.on-accent");
            self.canvas.circle(x + r, y + r, 2.8, on_accent);
        }
        let size = self.n("text.body");
        let base = self.baseline(Face::Ui, size, y, 2.0 * r);
        self.text(Face::Ui, size, x + 2.0 * r + 8.0, base, label, fg);
    }

    pub(crate) fn checkbox(&mut self, x: f32, y: f32, label: &str, on: bool) {
        let (accent, border, fg, ctrl) = (
            self.c("accent"),
            self.c("border.strong"),
            self.c("fg.text"),
            self.c("bg.control"),
        );
        let s = 14.0;
        let radius = (self.n("radius.control") / 2.0).min(4.0);
        if on {
            self.canvas.rounded_rect(x, y, s, s, radius, accent);
            let on_accent = self.c("fg.text.on-accent");
            self.canvas
                .segment((x + 3.5, y + 7.0), (x + 6.0, y + 10.0), 1.8, on_accent);
            self.canvas
                .segment((x + 6.0, y + 10.0), (x + 11.0, y + 4.0), 1.8, on_accent);
        } else {
            self.canvas.rounded_rect(x, y, s, s, radius, ctrl);
            self.canvas
                .stroke_rounded_rect(x, y, s, s, radius, 1.0, border);
        }
        let size = self.n("text.body");
        let base = self.baseline(Face::Ui, size, y, s);
        self.text(Face::Ui, size, x + s + 8.0, base, label, fg);
    }

    pub(crate) fn switch(&mut self, x: f32, y: f32, on: bool) {
        let (accent, ctrl, border) = (
            self.c("accent"),
            self.c("bg.control.pressed"),
            self.c("border.strong"),
        );
        let (w, h) = (34.0, 20.0);
        let bg = if on { accent } else { ctrl };
        self.canvas.rounded_rect(x, y, w, h, h / 2.0, bg);
        if !on {
            self.canvas
                .stroke_rounded_rect(x, y, w, h, h / 2.0, 1.0, border);
        }
        let knob = 0xFFFFFF;
        let kx = if on { x + w - h / 2.0 } else { x + h / 2.0 };
        self.canvas.circle(kx, y + h / 2.0, h / 2.0 - 3.0, knob);
    }

    pub(crate) fn heading(&mut self, x: f32, y: f32, s: &str) -> f32 {
        let size = self.n("text.heading");
        let fg = self.c("fg.text");
        let base = y + self.fonts.ascent(Face::UiMedium, size).round();
        self.text(Face::UiMedium, size, x, base, s, fg);
        self.fonts.line_height(Face::UiMedium, size).ceil()
    }

    pub(crate) fn label(&mut self, x: f32, y: f32, s: &str, secondary: bool) -> f32 {
        let size = self.n("text.body");
        let fg = if secondary {
            self.c("fg.text.secondary")
        } else {
            self.c("fg.text")
        };
        let base = y + self.fonts.ascent(Face::Ui, size).round();
        self.text(Face::Ui, size, x, base, s, fg);
        self.fonts.line_height(Face::Ui, size).ceil()
    }

    // ---- screens -------------------------------------------------------

    pub fn screen_desktop(&mut self, selected_in_switcher: Option<usize>) {
        self.desktop_background();
        let a = Rect {
            x: 72.0,
            y: 56.0,
            w: 600.0,
            h: 400.0,
        };
        let b = Rect {
            x: 318.0,
            y: 196.0,
            w: 640.0,
            h: 440.0,
        };
        let titles = ["zosh \u{2014} ~/src/hello", "zosh \u{2014} ~/Документи"];
        self.terminal(a, titles[0], false, &session_build(), false);
        self.terminal(b, titles[1], true, &session_short(), true);
        if self.dock_layout() {
            self.top_bar("US", None);
            self.dock(&[true, false, false, false]);
        } else {
            self.panel(&[(titles[0], false), (titles[1], true)], "US");
        }
        if let Some(sel) = selected_in_switcher {
            self.switcher(&titles, sel);
        }
    }

    pub fn screen_terminal(&mut self) {
        self.desktop_background();
        let r = Rect {
            x: 60.0,
            y: 40.0,
            w: 904.0,
            h: 640.0,
        };
        self.terminal(r, "zosh \u{2014} ~", true, &session_names(), true);
        if self.dock_layout() {
            self.top_bar("UA", None);
            self.dock(&[true, false, false, false]);
        } else {
            self.panel(&[("zosh \u{2014} ~", true)], "UA");
        }
    }

    pub fn screen_chooser(&mut self) {
        if self.tokens.str("chooser.style") == "sheet" {
            return self.screen_chooser_sheet();
        }
        self.desktop_background();
        let editor = Rect {
            x: 112.0,
            y: 64.0,
            w: 800.0,
            h: 580.0,
        };
        let body = self.window(editor, "Untitled \u{2014} Edit", false, Kind::Normal);
        let view = self.c("bg.view");
        self.body_fill(body, view);
        let size = self.n("text.mono");
        let lh = self.fonts.line_height(Face::Mono, size).ceil();
        let fg = self.c("fg.text.secondary");
        let mut y = body.y + 14.0 + self.fonts.ascent(Face::Mono, size);
        for line in [
            "# Poznámky",
            "",
            "Dnes: zkontrolovat kompozitor,",
            "Завтра: написати звіт.",
        ] {
            self.text(Face::Mono, size, body.x + 16.0, y, line, fg);
            y += lh;
        }
        // the chooser
        let d = Rect {
            x: 262.0,
            y: 160.0,
            w: 500.0,
            h: 420.0,
        };
        let body = self.window(d, "Open a file for Edit", true, Kind::System);
        let pad = self.n("padding.window");
        let mut y = body.y + pad;
        let x = body.x + pad;
        y += self.label(x, y, "Documents  \u{203A}  Projects", true) + 8.0;
        let list = Rect {
            x,
            y,
            w: body.w - 2.0 * pad,
            h: 250.0,
        };
        let view = self.c("bg.view");
        let border = self.c("border");
        let radius = self.n("radius.control");
        self.canvas
            .rounded_rect(list.x, list.y, list.w, list.h, radius, view);
        self.canvas
            .stroke_rounded_rect(list.x, list.y, list.w, list.h, radius, 1.0, border);
        let rows = [
            ("Документи", "folder", ""),
            ("Фотографії", "folder", ""),
            ("Makefile", "file", "1.2 kB"),
            ("příklad.txt", "file", "48 B"),
            ("звіт-2026.txt", "file", "2.1 kB"),
            ("Žďár.md", "file", "913 B"),
            ("čeština.md", "file", "4.0 kB"),
        ];
        let row_h = self.n("control.height");
        let size = self.n("text.body");
        let secondary = self.c("fg.text.secondary");
        let fg = self.c("fg.text");
        for (i, (name, kind, sz)) in rows.iter().enumerate() {
            let ry = list.y + 6.0 + i as f32 * row_h;
            let selected = i == 4;
            if selected {
                let sel = self.c("bg.selection");
                self.canvas
                    .rounded_rect(list.x + 4.0, ry, list.w - 8.0, row_h, radius, sel);
            }
            let icon_col = if *kind == "folder" {
                self.c("accent")
            } else {
                secondary
            };
            if *kind == "folder" {
                self.icon(Icon::Files, list.x + 10.0, ry + 4.0, row_h - 8.0, icon_col);
            } else {
                self.icon(Icon::Edit, list.x + 10.0, ry + 4.0, row_h - 8.0, icon_col);
            }
            let base = self.baseline(Face::Ui, size, ry, row_h);
            let col = if selected {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            self.text(Face::Ui, size, list.x + 12.0 + row_h, base, name, col);
            if !sz.is_empty() {
                let w = self.fonts.width(Face::Ui, size, sz);
                self.text(
                    Face::Ui,
                    size,
                    list.x + list.w - 12.0 - w,
                    base,
                    sz,
                    secondary,
                );
            }
        }
        let by = body.y + body.h - pad - row_h;
        let bw = 88.0;
        self.button(body.x + body.w - pad - bw, by, bw, "Open", true);
        self.button(
            body.x + body.w - pad - 2.0 * bw - 8.0,
            by,
            bw,
            "Cancel",
            false,
        );
        let hint_y = by + self.baseline(Face::Ui, self.n("text.caption"), 0.0, row_h);
        self.text(
            Face::Ui,
            self.n("text.caption"),
            x,
            hint_y,
            "Edit will be given this file, to read.",
            secondary,
        );
        self.panel(
            &[
                ("Untitled \u{2014} Edit", false),
                ("Open a file for Edit", true),
            ],
            "CZ",
        );
    }

    pub fn screen_settings(&mut self) {
        if self.tokens.str("settings.style") == "cards" {
            return self.screen_settings_cards();
        }
        self.desktop_background();
        let w = Rect {
            x: 212.0,
            y: 110.0,
            w: 600.0,
            h: 480.0,
        };
        let body = self.window(w, "Settings", true, Kind::Normal);
        let pad = self.n("padding.window");
        let side_w = 160.0;
        let side_bg = self.c("bg.panel");
        let old = self.canvas.clip(body.x + 1.0, body.y, side_w, body.h - 1.0);
        self.body_fill(body, side_bg);
        self.canvas.restore_clip(old);
        let border = self.c("border");
        self.canvas
            .rect(body.x + side_w, body.y, 1.0, body.h - 1.0, border);
        let radius = self.n("radius.control");
        let row_h = self.n("control.height");
        let size = self.n("text.body");
        let fg = self.c("fg.text");
        for (i, item) in ["Appearance", "Keyboard", "About"].iter().enumerate() {
            let ry = body.y + pad + i as f32 * (row_h + 2.0);
            if i == 0 {
                let sel = self.c("bg.selection");
                self.canvas
                    .rounded_rect(body.x + 8.0, ry, side_w - 16.0, row_h, radius, sel);
            }
            let base = self.baseline(Face::Ui, size, ry, row_h);
            let col = if i == 0 {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            self.text(Face::Ui, size, body.x + 20.0, base, item, col);
        }
        let x = body.x + side_w + pad + 8.0;
        let mut y = body.y + pad;
        y += self.heading(x, y, "Appearance") + 12.0;
        y += self.label(x, y, "Theme", true) + 6.0;
        self.radio(x, y, "Light", self.appearance == Appearance::Light);
        self.radio(x + 120.0, y, "Dark", self.appearance == Appearance::Dark);
        y += 30.0;
        y += self.label(x, y, "Scale", true) + 6.0;
        self.radio(x, y, "1\u{00D7}", true);
        self.radio(x + 120.0, y, "2\u{00D7}", false);
        self.radio(x + 240.0, y, "Automatic", false);
        y += 30.0;
        y += 6.0;
        self.canvas
            .line_h(x, y, body.w - side_w - 2.0 * pad - 8.0, 1.0, border);
        y += 16.0;
        let lh = self.label(x, y, "Motion", false);
        self.label(
            x,
            y + lh,
            "Animate the switcher, menus and minimising.",
            true,
        );
        self.switch(x + 320.0, y + 2.0, self.n("motion.short") > 0.0);
        y += 52.0;
        y += self.heading(x, y, "Keyboard") + 12.0;
        y += self.label(x, y, "Enabled layouts, switched with System+Space", true) + 8.0;
        for (i, (name, on)) in [
            ("English (US)", true),
            ("Ukrainian", true),
            ("Czech (QWERTZ)", true),
            ("Russian", false),
        ]
        .iter()
        .enumerate()
        {
            self.checkbox(
                x + (i % 2) as f32 * 180.0,
                y + (i / 2) as f32 * 26.0,
                name,
                *on,
            );
        }
        y += 60.0;
        self.label(x, y, "Active now: English (US)", true);
        self.panel(&[("Settings", true)], "US");
    }

    /// The thin bar at the top (second mist): clock and layout on the
    /// right, the system menu on the left, and in the middle the trusted
    /// mark of a sheet while one is open (ADR-0167).
    pub(crate) fn top_bar(&mut self, layout: &str, trusted: Option<&str>) {
        let h = self.n("bar.height");
        let bg = self.c("bg.panel");
        self.glass(0.0, 0.0, LOGICAL_WIDTH as f32, h, 0.0, bg);
        let border = self.c("border");
        self.canvas
            .line_h(0.0, h - 1.0, LOGICAL_WIDTH as f32, 1.0, border);
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
        if let Some(text) = trusted {
            let tw = self.fonts.width(Face::UiMedium, size, text);
            let em = h - 10.0;
            let total = em + 8.0 + tw;
            let x = (LOGICAL_WIDTH as f32 - total) / 2.0;
            self.emblem(x, 5.0, em);
            self.text(Face::UiMedium, size, x + em + 8.0, base, text, fg);
        }
    }

    /// The system emblem: a rounded square in the system tint with a Z.
    /// Applications cannot draw into the bar, so the emblem there is the
    /// mark of a surface the compositor itself draws.
    pub(crate) fn emblem(&mut self, x: f32, y: f32, s: f32) {
        let tint = self.c("bg.frame.system");
        let fg = self.contrasting(tint, true);
        self.canvas.rounded_rect(x, y, s, s, s / 4.0, tint);
        let size = s * 0.8;
        let base = self.baseline(Face::UiMedium, size, y, s);
        self.text_centered(Face::UiMedium, size, x + s / 2.0, base, "Z", fg);
    }

    /// The floating dock (second mist): large icons on a translucent
    /// rounded surface, a dot under each running application, no text.
    pub(crate) fn dock(&mut self, running: &[bool]) {
        let icon = self.n("dock.icon");
        let pad = self.n("dock.padding");
        let gap = pad;
        let icons = [Icon::Terminal, Icon::Files, Icon::Edit, Icon::Settings];
        let n = icons.len() as f32;
        let w = 2.0 * pad + n * icon + (n - 1.0) * gap;
        let h = 2.0 * pad + icon + 6.0;
        let x = (LOGICAL_WIDTH as f32 - w) / 2.0;
        let y = LOGICAL_HEIGHT as f32 - h - 10.0;
        let radius = self.n("dock.radius");
        let (blur, off, alpha, col) = (
            self.n("shadow.popup.blur"),
            self.n("shadow.popup.offset"),
            self.n("shadow.popup.alpha") as u8,
            self.c("shadow"),
        );
        if blur > 0.0 {
            self.canvas
                .shadow(x, y, w, h, radius, blur, off, col, alpha);
        }
        let bg = self.c("bg.panel");
        self.glass(x, y, w, h, radius, bg);
        let border = self.c("border");
        self.canvas
            .stroke_rounded_rect(x, y, w, h, radius, 1.0, border);
        let tile = self.c("bg.control");
        let fg = self.c("fg.text");
        let tile_r = self.n("radius.control") * 2.0;
        for (i, ic) in icons.iter().enumerate() {
            let ix = x + pad + i as f32 * (icon + gap);
            let iy = y + pad;
            self.canvas.rounded_rect(ix, iy, icon, icon, tile_r, tile);
            let inset = icon * 0.2;
            self.icon_on(*ic, ix + inset, iy + inset, icon - 2.0 * inset, fg, tile);
            if running.get(i).copied().unwrap_or(false) {
                self.canvas
                    .circle(ix + icon / 2.0, iy + icon + 3.5, 2.0, fg);
            }
        }
    }

    /// The file chooser as a sheet (second mist): it slides out from under
    /// the owner's title bar, has no buttons of its own, and carries the
    /// system emblem; the bar shows the same emblem and title while it is
    /// open.
    pub(crate) fn screen_chooser_sheet(&mut self) {
        self.desktop_background();
        let editor = Rect {
            x: 112.0,
            y: 64.0,
            w: 800.0,
            h: 580.0,
        };
        let body = self.window(editor, "Untitled \u{2014} Edit", true, Kind::Normal);
        let view = self.c("bg.view");
        self.body_fill(body, view);
        let size = self.n("text.mono");
        let lh = self.fonts.line_height(Face::Mono, size).ceil();
        let fg_sec = self.c("fg.text.secondary");
        let pad_t = self.n("padding.terminal");
        let mut y = body.y + pad_t + self.fonts.ascent(Face::Mono, size);
        for line in [
            "# Poznámky",
            "",
            "Dnes: zkontrolovat kompozitor,",
            "Завтра: написати звіт.",
        ] {
            self.text(Face::Mono, size, body.x + pad_t, y, line, fg_sec);
            y += lh;
        }
        // the sheet
        let title = "Open a file for Edit";
        let (sw, sh) = (520.0, 430.0);
        let sx = editor.x + (editor.w - sw) / 2.0;
        let sy = body.y;
        let radius = self.n("radius.window");
        let (blur, off, alpha, col) = (
            self.n("shadow.window.blur"),
            self.n("shadow.window.offset"),
            self.n("shadow.window.alpha") as u8,
            self.c("shadow"),
        );
        let layers = (self.n("shadow.window.layers") as u8).max(1);
        let old = self.canvas.clip(body.x, body.y, body.w, body.h);
        for i in 1..=layers {
            let f = i as f32 / layers as f32;
            self.canvas.shadow(
                sx,
                sy - radius,
                sw,
                sh + radius,
                radius,
                blur * f,
                off * f,
                col,
                alpha / layers,
            );
        }
        let win = self.c("bg.window");
        self.canvas
            .rounded_rect(sx, sy - radius, sw, sh + radius, radius, win);
        let border = self.c("border");
        self.canvas
            .stroke_rounded_rect(sx, sy - radius, sw, sh + radius, radius, 1.0, border);
        self.canvas.restore_clip(old);
        // the system strip: tint, emblem and the title, drawn by the compositor
        let strip = 36.0;
        let tint = self.c("bg.frame.system");
        let old = self.canvas.clip(sx, sy, sw, strip);
        self.canvas
            .rounded_rect(sx, sy - radius, sw, strip + radius, radius, tint);
        self.canvas.restore_clip(old);
        self.canvas.line_h(sx, sy + strip - 1.0, sw, 1.0, border);
        let em = strip - 14.0;
        self.emblem(sx + 12.0, sy + 7.0, em);
        let tsize = self.n("text.body");
        let tcol = self.contrasting(tint, true);
        let base = self.baseline(Face::UiMedium, tsize, sy, strip);
        self.text(
            Face::UiMedium,
            tsize,
            sx + 12.0 + em + 8.0,
            base,
            title,
            tcol,
        );
        let pad = self.n("padding.window");
        let x = sx + pad;
        let mut y = sy + strip + pad;
        y += self.label(x, y, "Documents  \u{203A}  Projects", true) + 8.0;
        let list = Rect {
            x,
            y,
            w: sw - 2.0 * pad,
            h: 240.0,
        };
        let r_ctrl = self.n("radius.control");
        self.canvas
            .rounded_rect(list.x, list.y, list.w, list.h, r_ctrl, view);
        self.canvas
            .stroke_rounded_rect(list.x, list.y, list.w, list.h, r_ctrl, 1.0, border);
        let rows = [
            ("Документи", "folder", ""),
            ("Фотографії", "folder", ""),
            ("Makefile", "file", "1.2 kB"),
            ("příklad.txt", "file", "48 B"),
            ("звіт-2026.txt", "file", "2.1 kB"),
            ("Žďár.md", "file", "913 B"),
            ("čeština.md", "file", "4.0 kB"),
        ];
        let row_h = self.n("control.height");
        let size = self.n("text.body");
        let fg = self.c("fg.text");
        for (i, (name, kind, sz)) in rows.iter().enumerate() {
            let ry = list.y + 6.0 + i as f32 * row_h;
            let selected = i == 4;
            if selected {
                let sel = self.c("bg.selection");
                self.canvas
                    .rounded_rect(list.x + 4.0, ry, list.w - 8.0, row_h, r_ctrl, sel);
            }
            let icon_col = if *kind == "folder" {
                self.c("accent")
            } else {
                fg_sec
            };
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
            self.icon_on(ic, list.x + 10.0, ry + 4.0, row_h - 8.0, icon_col, inner);
            let base = self.baseline(Face::Ui, size, ry, row_h);
            let col = if selected {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            self.text(Face::Ui, size, list.x + 12.0 + row_h, base, name, col);
            if !sz.is_empty() {
                let w = self.fonts.width(Face::Ui, size, sz);
                self.text(Face::Ui, size, list.x + list.w - 12.0 - w, base, sz, fg_sec);
            }
        }
        let by = sy + sh - pad - row_h;
        let bw = 88.0;
        self.button(sx + sw - pad - bw, by, bw, "Open", true);
        self.button(sx + sw - pad - 2.0 * bw - 8.0, by, bw, "Cancel", false);
        let hint_y = by + self.baseline(Face::Ui, self.n("text.caption"), 0.0, row_h);
        self.text(
            Face::Ui,
            self.n("text.caption"),
            x,
            hint_y,
            "Edit will be given this file, to read.",
            fg_sec,
        );
        self.top_bar("CZ", Some(title));
        self.dock(&[false, false, true, false]);
    }

    /// A settings row: label on the left; the caller draws the control at
    /// the returned x (the right edge of the row's control area).
    pub(crate) fn card_row(
        &mut self,
        card: Rect,
        y: f32,
        label: &str,
        caption: Option<&str>,
        last: bool,
    ) -> f32 {
        let pad = self.n("padding.window");
        let row_h = self.row_height(caption.is_some());
        let base = self.baseline(
            Face::Ui,
            self.n("text.body"),
            y,
            if caption.is_some() {
                row_h - 18.0
            } else {
                row_h
            },
        );
        let fg = self.c("fg.text");
        self.text(Face::Ui, self.n("text.body"), card.x + pad, base, label, fg);
        if let Some(cap) = caption {
            let sec = self.c("fg.text.secondary");
            self.text(
                Face::Ui,
                self.n("text.caption"),
                card.x + pad,
                base + 18.0,
                cap,
                sec,
            );
        }
        if !last {
            let border = self.c("border");
            self.canvas.line_h(
                card.x + pad,
                y + row_h - 1.0,
                card.w - 2.0 * pad,
                1.0,
                border,
            );
        }
        card.x + card.w - pad
    }

    pub(crate) fn row_height(&self, with_caption: bool) -> f32 {
        let h = self.n("control.height") + 2.0 * self.n("padding.control");
        if with_caption { h + 18.0 } else { h }
    }

    pub(crate) fn card(&mut self, x: f32, y: f32, w: f32, rows: f32) -> Rect {
        let h = rows;
        let radius = self.n("radius.control") * 2.0;
        let bg = self.c("bg.window");
        let border = self.c("border");
        self.canvas.rounded_rect(x, y, w, h, radius, bg);
        self.canvas
            .stroke_rounded_rect(x, y, w, h, radius, 1.0, border);
        Rect { x, y, w, h }
    }

    /// Settings as grouped cards on a grey ground (second mist): rows of
    /// "label left, control right", thin separators, a heading hierarchy.
    pub(crate) fn screen_settings_cards(&mut self) {
        self.desktop_background();
        let w = Rect {
            x: 192.0,
            y: 96.0,
            w: 640.0,
            h: 520.0,
        };
        let body = self.window(w, "Settings", true, Kind::Normal);
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
        for (i, item) in ["Appearance", "Keyboard", "About"].iter().enumerate() {
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
        // page title
        let tsize = self.n("text.title");
        let asc = self.fonts.ascent(Face::UiMedium, tsize);
        self.text(Face::UiMedium, tsize, x, y + asc, "Appearance", fg);
        y += self.fonts.line_height(Face::UiMedium, tsize) + 12.0;
        // card 1: theme, scale, motion
        let r1 = self.row_height(false);
        let r3 = self.row_height(true);
        let card = self.card(x, y, cw, 2.0 * r1 + r3);
        let mut ry = y;
        let right = self.card_row(card, ry, "Theme", None, false);
        let light = self.appearance == Appearance::Light;
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
            Some("Animate the switcher, menus and minimising."),
            true,
        );
        let sw = 36.0;
        self.switch(
            right - sw,
            ry + (r1 - 20.0) / 2.0,
            self.n("motion.short") > 0.0,
        );
        y += card.h + 20.0;
        // section heading
        let hsize = self.n("text.heading");
        let asc = self.fonts.ascent(Face::UiMedium, hsize);
        self.text(Face::UiMedium, hsize, x, y + asc, "Keyboard", fg);
        y += self.fonts.line_height(Face::UiMedium, hsize) + 4.0;
        let sec = self.c("fg.text.secondary");
        let csize = self.n("text.caption");
        let asc = self.fonts.ascent(Face::Ui, csize);
        self.text(
            Face::Ui,
            csize,
            x,
            y + asc,
            "Enabled layouts, switched with System+Space",
            sec,
        );
        y += self.fonts.line_height(Face::Ui, csize) + 8.0;
        let layouts = [
            ("English (US)", true),
            ("Ukrainian", true),
            ("Czech (QWERTZ)", true),
            ("Russian", false),
        ];
        let card = self.card(x, y, cw, layouts.len() as f32 * r1);
        let mut ry = y;
        for (i, (name, on)) in layouts.iter().enumerate() {
            let right = self.card_row(card, ry, name, None, i + 1 == layouts.len());
            self.checkbox(right - 18.0, ry + (r1 - 18.0) / 2.0, "", *on);
            ry += r1;
        }
        y += card.h + 10.0;
        let asc = self.fonts.ascent(Face::Ui, csize);
        self.text(Face::Ui, csize, x, y + asc, "Active now: English (US)", sec);
        self.top_bar("US", None);
        self.dock(&[false, false, false, true]);
    }

    pub fn screen_switcher(&mut self) {
        self.screen_desktop(Some(1));
    }

    pub(crate) fn switcher(&mut self, titles: &[&str], selected: usize) {
        let w = 520.0;
        let row_h = 64.0;
        let pad = 12.0;
        let h = pad * 2.0 + titles.len() as f32 * row_h;
        let x = (LOGICAL_WIDTH as f32 - w) / 2.0;
        let y = (LOGICAL_HEIGHT as f32 - h) / 2.0 - 40.0;
        let radius = self.n("radius.popup");
        let blur = self.n("shadow.popup.blur");
        if blur > 0.0 {
            let (off, alpha, col) = (
                self.n("shadow.popup.offset"),
                self.n("shadow.popup.alpha") as u8,
                self.c("shadow"),
            );
            self.canvas
                .shadow(x, y, w, h, radius, blur, off, col, alpha);
        }
        let bg = self.c("bg.overlay");
        if self.blurred.is_some() {
            self.glass(x, y, w, h, radius, bg);
        } else {
            self.canvas.rounded_rect_alpha(x, y, w, h, radius, bg, 240);
        }
        let border = self.c("border.strong");
        self.canvas
            .stroke_rounded_rect(x, y, w, h, radius, 1.0, border);
        let size = self.n("text.body");
        let fg = self.c("fg.text");
        let r_ctrl = self.n("radius.control");
        for (i, title) in titles.iter().enumerate() {
            let ry = y + pad + i as f32 * row_h;
            if i == selected {
                let sel = self.c("bg.selection");
                self.canvas
                    .rounded_rect(x + pad, ry, w - 2.0 * pad, row_h, r_ctrl, sel);
            }
            // a thumbnail: a tiny framed window
            let tx = x + pad + 10.0;
            let ty = ry + 10.0;
            let (tw, th) = (70.0, row_h - 20.0);
            let frame = self.c("bg.frame");
            let view = self.c("term.bg");
            self.canvas.rounded_rect(tx, ty, tw, th, 2.0, frame);
            self.canvas
                .rect(tx + 1.0, ty + 7.0, tw - 2.0, th - 8.0, view);
            let fgt = self.c("term.fg");
            for k in 0..4 {
                self.canvas.rect(
                    tx + 4.0,
                    ty + 10.0 + k as f32 * 6.0,
                    20.0 + (k * 11) as f32,
                    2.0,
                    fgt,
                );
            }
            self.canvas
                .stroke_rounded_rect(tx, ty, tw, th, 2.0, 1.0, border);
            let base = self.baseline(Face::Ui, size, ry, row_h);
            let col = if i == selected {
                self.c("fg.text.on-selection")
            } else {
                fg
            };
            self.text(Face::Ui, size, tx + tw + 16.0, base, title, col);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    Terminal,
    Files,
    Edit,
    Agent,
    Settings,
}

/// The procedural wallpaper: a diagonal gradient from `a` to `b` with two
/// soft blobs of `c` and `a`. Pure arithmetic, deterministic, no image.
pub fn wallpaper(w: usize, h: usize, a: u32, b: u32, c: u32) -> Vec<u32> {
    let mut img = vec![0; w * h];
    let (fw, fh) = (w as f32, h as f32);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / fw, y as f32 / fh);
            let t = (fx * 0.55 + fy * 0.45).clamp(0.0, 1.0);
            let mut col = mix(a, b, t);
            let blob = |cx: f32, cy: f32, r: f32| -> f32 {
                let d = ((fx - cx).powi(2) + (fy - cy).powi(2) * (fh / fw).powi(2)).sqrt() / r;
                let k = (1.0 - d).clamp(0.0, 1.0);
                k * k * (3.0 - 2.0 * k)
            };
            col = mix(col, c, 0.75 * blob(0.72, 0.28, 0.55));
            col = mix(col, a, 0.5 * blob(0.15, 0.95, 0.45));
            img[y * w + x] = col;
        }
    }
    img
}

pub(crate) fn luminance(c: u32) -> f32 {
    let r = ((c >> 16) & 0xFF) as f32 / 255.0;
    let g = ((c >> 8) & 0xFF) as f32 / 255.0;
    let b = (c & 0xFF) as f32 / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn session_names() -> Vec<TermLine> {
    vec![
        TermLine(vec![("zosh$ ", "term.green"), ("ls", "term.fg")]),
        TermLine(vec![
            ("Документи/   ", "term.blue"),
            ("Фотографії/   ", "term.blue"),
            (
                "Makefile   příklad.txt   Žďár.md   čeština.md   звіт-2026.txt",
                "term.fg",
            ),
        ]),
        TermLine(vec![
            ("zosh$ ", "term.green"),
            ("cat příklad.txt", "term.fg"),
        ]),
        TermLine(vec![("Příliš žluťoučký kůň úpěl ďábelské ódy.", "term.fg")]),
        TermLine(vec![
            ("zosh$ ", "term.green"),
            ("cat звіт-2026.txt", "term.fg"),
        ]),
        TermLine(vec![(
            "Щороку ґазда їде до Львова: їжа, євро, ґудзики.",
            "term.fg",
        )]),
        TermLine(vec![
            ("zosh$ ", "term.green"),
            ("mv Žďár.md Документи/", "term.fg"),
        ]),
        TermLine(vec![("zosh$ ", "term.green"), ("ls Документи", "term.fg")]),
        TermLine(vec![
            ("Žďár.md   ", "term.fg"),
            ("звіт.md   ", "term.fg"),
            ("plán.txt", "term.fg"),
        ]),
        TermLine(vec![
            ("zosh$ ", "term.green"),
            ("tcc hello.c -o hello", "term.fg"),
        ]),
        TermLine(vec![("zosh$ ", "term.green"), ("./hello", "term.fg")]),
        TermLine(vec![("Привіт, світе! Ahoj, světe!", "term.fg")]),
        TermLine(vec![
            ("zosh$ ", "term.green"),
            ("stat звіт-2026.txt", "term.fg"),
        ]),
        TermLine(vec![(
            "звіт-2026.txt  file  2,134 bytes  modified 2026-11-03 14:21",
            "term.fg",
        )]),
        TermLine(vec![("zosh$ ", "term.green")]),
    ]
}

fn session_short() -> Vec<TermLine> {
    vec![
        TermLine(vec![("zosh$ ", "term.green"), ("cd Документи", "term.fg")]),
        TermLine(vec![("zosh$ ", "term.green"), ("ls", "term.fg")]),
        TermLine(vec![("Žďár.md   звіт.md   plán.txt   nápady/", "term.fg")]),
        TermLine(vec![("zosh$ ", "term.green"), ("cat plán.txt", "term.fg")]),
        TermLine(vec![("1. kompozitor: rámy oken", "term.fg")]),
        TermLine(vec![("2. вибір файлу через chooser", "term.fg")]),
        TermLine(vec![("3. nastavení", "term.fg")]),
        TermLine(vec![("zosh$ ", "term.green")]),
    ]
}

fn session_build() -> Vec<TermLine> {
    vec![
        TermLine(vec![("zosh$ ", "term.green"), ("make", "term.fg")]),
        TermLine(vec![("cc -O2 -c main.c", "term.fg")]),
        TermLine(vec![("cc -O2 -c draw.c", "term.fg")]),
        TermLine(vec![("cc main.o draw.o -o hello", "term.fg")]),
        TermLine(vec![("zosh$ ", "term.green"), ("./hello", "term.fg")]),
        TermLine(vec![("hello from Zoraya", "term.fg")]),
        TermLine(vec![("zosh$ ", "term.green"), ("run server &", "term.fg")]),
        TermLine(vec![("[1] server", "term.yellow")]),
        TermLine(vec![("zosh$ ", "term.green"), ("jobs", "term.fg")]),
        TermLine(vec![("[1] running  server", "term.fg")]),
        TermLine(vec![("zosh$ ", "term.green")]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_is_deterministic_and_within_its_colours() {
        let a = wallpaper(64, 48, 0x102030, 0xE0D0C0, 0x80A0C0);
        let b = wallpaper(64, 48, 0x102030, 0xE0D0C0, 0x80A0C0);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64 * 48);
        for p in &a {
            assert!(*p <= 0xFFFFFF);
            let r = (p >> 16) & 0xFF;
            assert!((0x10..=0xE0).contains(&r), "{p:06x}");
        }
        // not flat: the gradient changes along the diagonal
        assert_ne!(a[0], a[64 * 48 - 1]);
    }
}

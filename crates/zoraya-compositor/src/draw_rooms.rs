//! What the compositor draws of rooms and grants (ADR-0172 points 5 to 8,
//! ADR-0168's dock B), from its own records only (INV-0172-1): a window's
//! room tint, badge, grant pills and untrusted edge; the dock with its
//! tiles, dots, ring, chips and tooltip; the show-commands tags.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_paint::{self as paint, Canvas, Mask, Material};
use zoraya_tokens::{Appearance, Color, CornerShape, Palette, Scale, glow, shape, space, text};

use crate::draw::{draw_text, prect};
use crate::rooms::{CHIP_HEIGHT, Cell, DockApp, DockGeometry};
use crate::{Compositor, Platform};

/// The title bar's tint by its room (the README's Field variant: 0.18).
pub const TITLE_TINT: u8 = 46;
/// A chip's fill: the current room's colour, another's control ground.
pub const CHIP_CURRENT: u8 = 46;
pub const CHIP_OTHER: u8 = 120;

/// What a window's frame shows of its record.
#[derive(Debug, Clone, Default)]
pub struct WinView {
    pub colour: Option<Color>,
    pub untrusted: bool,
    /// The badge: the room's name, `untrusted`; none for `home`.
    pub badge: Option<String>,
    pub pills: Vec<String>,
    pub app: String,
}

/// What the dock shows.
#[derive(Debug, Clone, Default)]
pub struct DockView {
    pub geometry: DockGeometry,
    pub apps: Vec<DockApp>,
    /// Per application: a dot per room it has windows in (`None` is
    /// `home`'s grey), and whether one is untrusted.
    pub dots: Vec<(Vec<Option<Color>>, bool)>,
    /// Per chip: the name, `untrusted`, the colour.
    pub chips: Vec<(String, bool, Option<Color>)>,
    pub current: usize,
    pub hover: Option<usize>,
    pub commands: bool,
    pub dark: bool,
}

impl<P: Platform> Compositor<P> {
    /// Each window's view, in the windows' order.
    pub(crate) fn win_views(&self) -> Vec<WinView> {
        self.windows
            .iter()
            .map(|w| {
                let Some(r) = self.record(w.client) else {
                    return WinView::default();
                };
                let badge = (!r.room.is_empty()).then(|| {
                    if r.untrusted {
                        format!("{}  untrusted", r.room)
                    } else {
                        r.room.clone()
                    }
                });
                // The grants, then the pills of a room at 90% of a limit
                // (ADR-0263 point 7): they come from the panel's readings.
                // They come first: a window too narrow for all folds the
                // grants into `+N`, never the room's warning.
                let mut pills = if r.room.is_empty() {
                    Vec::new()
                } else {
                    self.usage_pills(&r.room)
                };
                pills.extend(r.entries.iter().cloned());
                WinView {
                    colour: self.room_colour(r),
                    untrusted: r.untrusted,
                    badge,
                    pills,
                    app: r.app.clone(),
                }
            })
            .collect()
    }

    pub(crate) fn dock_view(&self) -> DockView {
        let p = self.appearance.palette();
        let apps = self.dock_list();
        let dots = apps
            .iter()
            .map(|a| {
                let rooms = self.app_rooms(&a.name);
                let untrusted = rooms.iter().any(|r| r.2);
                let colours = rooms
                    .iter()
                    .map(|(n, slot, u)| {
                        if n == zoraya_settings::rooms::HOME {
                            None
                        } else if *u {
                            Some(p.room_untrusted)
                        } else {
                            Some(p.room(*slot))
                        }
                    })
                    .collect();
                (colours, untrusted)
            })
            .collect();
        let mut chips = alloc::vec![(String::from(zoraya_settings::rooms::HOME), false, None)];
        for r in &self.rooms.rooms {
            let c = if r.untrusted {
                p.room_untrusted
            } else {
                p.room(r.colour)
            };
            chips.push((r.name.clone(), r.untrusted, Some(c)));
        }
        let current = if self.current_room.is_empty() {
            0
        } else {
            1 + self
                .rooms
                .rooms
                .iter()
                .position(|r| r.name == self.current_room)
                .unwrap_or(0)
        };
        DockView {
            geometry: self.dock.clone(),
            apps,
            dots,
            chips,
            current,
            hover: self.hover,
            commands: self.commands,
            dark: self.appearance == Appearance::Dark,
        }
    }
}

fn circle(c: &mut Canvas<'_>, cx: i32, cy: i32, d: i32, colour: Color, alpha: u8) {
    let m = paint::rounded_rect(d as usize, d as usize, (d / 2) as u32, CornerShape::Circle);
    c.mask(&m, cx - d / 2, cy - d / 2, colour, alpha);
}

/// The title bar's room part: the tint over the bar's ground. Called after
/// the ground and before the body.
pub fn tint(c: &mut Canvas<'_>, outer: &Mask, o: paint::Rect, v: &WinView) {
    if let Some(col) = v.colour {
        c.mask(outer, o.x, o.y, col, TITLE_TINT);
    }
}

/// The untrusted edge (INV-0172-10): `room.untrusted.edge` pixels in
/// `room.untrusted` inside the border, for untrusted records only.
pub fn edge(
    c: &mut Canvas<'_>,
    o: paint::Rect,
    f: &crate::Frame,
    s: u32,
    v: &WinView,
    p: &Palette,
) {
    if !v.untrusted {
        return;
    }
    let b = f.border as i32;
    let (w, h) = ((o.w - 2 * b).max(0) as usize, (o.h - 2 * b).max(0) as usize);
    let t = space::ROOM_UNTRUSTED_EDGE.physical(Scale::ONE) * s;
    let m = paint::rounded_outline(w, h, f.radius.saturating_sub(b as u32), t, shape::CORNER);
    c.mask(&m, o.x + b, o.y + b, p.room_untrusted, 255);
}

/// The badge and the pills at the right of the title bar, right to left;
/// gives the x where they begin, so that the title ends before it. Pills
/// that do not fit are folded into one `+N`.
#[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
pub fn badge_and_pills(
    c: &mut Canvas<'_>,
    o: paint::Rect,
    f: &crate::Frame,
    s: u32,
    v: &WinView,
    p: &Palette,
    title_start: i32,
    texts: (
        &mut zoraya_text::Text<'static>,
        &mut zoraya_text::Text<'static>,
    ),
) -> i32 {
    let (ui, mono) = texts;
    let si = s as i32;
    let cap = text::CAPTION.physical(Scale::ONE) * s;
    let cy = o.y + f.title as i32 / 2;
    let baseline = cy + cap as i32 * 7 / 20;
    let mut right = o.right() - 10 * si;
    if let Some(name) = &v.badge {
        let w = ui.measure(name, cap);
        let x = right - w;
        draw_text(c, ui, name, cap, x, baseline, p.fg_text);
        if let Some(col) = v.colour {
            circle(c, x - 8 * si, cy, 7 * si, col, 255);
        }
        right = x - 18 * si;
    }
    let h = 18 * si;
    let room_for = right - (title_start + 60 * si);
    let widths: Vec<i32> = v
        .pills
        .iter()
        .map(|t| mono.measure(t, cap) + 12 * si)
        .collect();
    let mut shown = 0;
    let mut used = 0;
    for w in &widths {
        if used + w + 6 * si > room_for {
            break;
        }
        used += w + 6 * si;
        shown += 1;
    }
    let mut labels: Vec<String> = v.pills[..shown].to_vec();
    if shown < v.pills.len() {
        let fold = format!("+{}", v.pills.len() - shown);
        let fw = mono.measure(&fold, cap) + 12 * si;
        // The fold replaces the last shown pill if it does not fit.
        while !labels.is_empty() && used + fw + 6 * si > room_for {
            let last = labels.pop().map_or(0, |l| mono.measure(&l, cap) + 18 * si);
            used -= last;
        }
        let rest = v.pills.len() - labels.len();
        labels.push(format!("+{rest}"));
    }
    for label in labels.iter().rev() {
        let w = mono.measure(label, cap) + 12 * si;
        let x = right - w;
        if x < title_start {
            break;
        }
        let m = paint::rounded_rect(w as usize, h as usize, (h / 2) as u32, CornerShape::Circle);
        c.mask(&m, x, cy - h / 2, p.bg_control, 200);
        let edge = paint::rounded_outline(
            w as usize,
            h as usize,
            (h / 2) as u32,
            s,
            CornerShape::Circle,
        );
        c.mask(&edge, x, cy - h / 2, p.border, 255);
        draw_text(
            c,
            mono,
            label,
            cap,
            x + 6 * si,
            baseline,
            p.fg_text_secondary,
        );
        right = x - 6 * si;
    }
    right
}

fn material(p: &Palette, k: u32, radius: u32) -> Material {
    Material {
        radius,
        shape: shape::CORNER,
        blur: (glow::PANEL_BLUR.physical(Scale::ONE) * k) as usize,
        halo_blur: (glow::BLUR.physical(Scale::ONE) * k) as usize,
        tint: p.bg_overlay,
        tint_alpha: glow::PANEL_ALPHA,
        glow: p.glow,
        glow_alpha: glow::ALPHA,
        halo_alpha: glow::HALO,
        border: p.border,
        scale: k,
    }
}

/// A tag of the show-commands mode or a tooltip: lines on the trusted
/// material, centred on `centre`, its bottom at `bottom`.
pub fn tag(
    c: &mut Canvas<'_>,
    lines: &[(&str, bool)],
    centre: i32,
    bottom: i32,
    k: u32,
    p: &Palette,
    texts: (
        &mut zoraya_text::Text<'static>,
        &mut zoraya_text::Text<'static>,
    ),
) {
    let (name_t, mono) = texts;
    let si = k as i32;
    let cap = text::CAPTION.physical(Scale::ONE) * k;
    let mut tw = 0;
    for (l, is_mono) in lines {
        let w = if *is_mono {
            mono.measure(l, cap)
        } else {
            name_t.measure(l, cap)
        };
        tw = tw.max(w);
    }
    let (w, h) = (tw + 20 * si, (8 + 15 * lines.len() as i32) * si);
    let (x, y) = (centre - w / 2, bottom - h);
    material(p, k, space::RADIUS_CONTROL.physical(Scale::ONE) * k)
        .draw(c, paint::Rect::new(x, y, w, h));
    let mut baseline = y + 4 * si + cap as i32;
    for (l, is_mono) in lines {
        let t: &mut zoraya_text::Text<'static> = if *is_mono { mono } else { name_t };
        let lw = t.measure(l, cap);
        draw_text(c, t, l, cap, centre - lw / 2, baseline, p.fg_text);
        baseline += 15 * si;
    }
}

/// A window's hover card (ADR-0267 point 3): the room, its grants and its
/// use, one line each in the monospaced face, on the trusted material at
/// `r`. The first line is the room's name.
pub fn card(
    c: &mut Canvas<'_>,
    lines: &[String],
    r: paint::Rect,
    k: u32,
    p: &Palette,
    texts: (
        &mut zoraya_text::Text<'static>,
        &mut zoraya_text::Text<'static>,
    ),
) {
    let (name_t, mono) = texts;
    let si = k as i32;
    let cap = text::CAPTION.physical(Scale::ONE) * k;
    material(p, k, space::RADIUS_CONTROL.physical(Scale::ONE) * k).draw(c, r);
    let mut baseline = r.y + 4 * si + cap as i32;
    for (i, l) in lines.iter().enumerate() {
        let t: &mut zoraya_text::Text<'static> = if i == 0 { name_t } else { mono };
        draw_text(c, t, l, cap, r.x + 10 * si, baseline, p.fg_text);
        baseline += 15 * si;
    }
}

/// The dock (ADR-0168, the README's dock B) over the blurred wallpaper.
pub fn dock(
    c: &mut Canvas<'_>,
    v: &DockView,
    blurred: &[u32],
    width: usize,
    k: u32,
    p: &Palette,
    texts: (
        &mut zoraya_text::Text<'static>,
        &mut zoraya_text::Text<'static>,
        &mut zoraya_text::Text<'static>,
    ),
) {
    let (title_t, ui, mono) = texts;
    let g = &v.geometry;
    let r = prect(g.rect);
    if r.is_empty() || (r.intersect(&c.clip).is_empty() && v.hover.is_none()) {
        return;
    }
    let si = k as i32;
    let radius = space::DOCK_RADIUS.physical(Scale::ONE) * k;
    let shape_m = paint::rounded_rect(r.w as usize, r.h as usize, radius, shape::CORNER);
    // The glass: the blurred wallpaper under the dock, tinted, with its
    // band of light and its rim.
    let mut under = Vec::with_capacity((r.w * r.h).max(0) as usize);
    for y in r.y..r.y + r.h {
        let row = y.max(0) as usize * width;
        for x in r.x..r.x + r.w {
            under.push(blurred.get(row + x.max(0) as usize).copied().unwrap_or(0));
        }
    }
    c.image(&under, r.w as usize, r.x, r.y, Some(&shape_m));
    c.mask(&shape_m, r.x, r.y, p.bg_overlay, glow::PANEL_ALPHA);
    c.band(
        paint::Rect::new(r.x + radius as i32, r.y, r.w - 2 * radius as i32, r.h),
        p.glow,
        glow::ALPHA / 2,
        0,
    );
    let rim = paint::rounded_outline(r.w as usize, r.h as usize, radius, k, shape::CORNER);
    c.mask(&rim, r.x, r.y, p.border, 255);
    let icon = space::DOCK_ICON.physical(Scale::ONE) as i32 * si;
    let tile_r = space::RADIUS_CONTROL.physical(Scale::ONE) * 2 * k;
    let cap = text::CAPTION.physical(Scale::ONE) * k;
    let chip_h = CHIP_HEIGHT as i32 * si;
    let tile = paint::rounded_rect(icon as usize, icon as usize, tile_r, shape::CORNER);
    let tile_edge = paint::rounded_outline(icon as usize, icon as usize, tile_r, k, shape::CORNER);
    let label_y = |iy: i32| iy + icon + 10 * si + cap as i32;
    for (cell, cr) in &g.cells {
        let cr = prect(*cr);
        let centre = cr.x + cr.w / 2;
        match cell {
            Cell::App(i) => {
                let Some(app) = v.apps.get(*i) else { continue };
                let ix = centre - icon / 2;
                let hovered = v.hover == Some(*i);
                let lift: u8 = match (v.dark, hovered) {
                    (true, false) => 31,
                    (true, true) => 56,
                    (false, false) => 128,
                    (false, true) => 230,
                };
                c.mask(&tile, ix, cr.y, p.bg_control.blend(p.glow, lift), 255);
                c.fill(
                    paint::Rect::new(ix + tile_r as i32, cr.y + si, icon - 2 * tile_r as i32, si),
                    p.glow,
                );
                c.mask(&tile_edge, ix, cr.y, p.border, 255);
                let (dots, untrusted) = v.dots.get(*i).cloned().unwrap_or_default();
                if untrusted {
                    let d = 3 * si;
                    let ring = paint::rounded_outline(
                        (icon + 2 * d) as usize,
                        (icon + 2 * d) as usize,
                        tile_r + d as u32,
                        (3 * k).div_ceil(2),
                        shape::CORNER,
                    );
                    c.mask(&ring, ix - d, cr.y - d, p.room_untrusted, 255);
                }
                icon_glyph(c, &app.name, ix, cr.y, icon, k, p.fg_text, title_t);
                let n = dots.len() as i32;
                let step = 7 * si;
                let mut dx = centre - (n - 1) * step / 2;
                for d in &dots {
                    circle(
                        c,
                        dx,
                        cr.y + icon + 3 * si + si / 2,
                        4 * si,
                        d.unwrap_or(p.fg_text_secondary),
                        255,
                    );
                    dx += step;
                }
                if v.commands {
                    let b1 = label_y(cr.y);
                    // The name above its line, or the line alone when they
                    // are the same word (`term`).
                    if app.name != app.line {
                        let w = title_t.measure(&app.name, cap);
                        draw_text(c, title_t, &app.name, cap, centre - w / 2, b1, p.fg_text);
                    }
                    let w = mono.measure(&app.line, cap);
                    draw_text(
                        c,
                        mono,
                        &app.line,
                        cap,
                        centre - w / 2,
                        b1 + 14 * si,
                        p.fg_text_secondary,
                    );
                }
            }
            Cell::Chip(i) => {
                let Some((name, untrusted, colour)) = v.chips.get(*i) else {
                    continue;
                };
                let col = colour.unwrap_or(p.fg_text_secondary);
                let cy = cr.y + (icon - chip_h) / 2;
                // The chip at its own width, centred in its cell (wider in
                // show-commands mode, for the line under it).
                let mut natural = 33 * si + title_t.measure(name, cap);
                if *untrusted {
                    natural += 5 * si + ui.measure("untrusted", cap);
                }
                let natural = natural.max(chip_h).min(cr.w);
                let cr = paint::Rect::new(centre - natural / 2, cr.y, natural, cr.h);
                let m = paint::rounded_rect(
                    cr.w as usize,
                    chip_h as usize,
                    (chip_h / 2) as u32,
                    CornerShape::Circle,
                );
                let o = paint::rounded_outline(
                    cr.w as usize,
                    chip_h as usize,
                    (chip_h / 2) as u32,
                    k,
                    CornerShape::Circle,
                );
                if *i == v.current {
                    c.mask(&m, cr.x, cy, col, CHIP_CURRENT);
                    c.mask(&o, cr.x, cy, col, 255);
                } else {
                    c.mask(&m, cr.x, cy, p.bg_control, CHIP_OTHER);
                    c.mask(&o, cr.x, cy, p.border, 255);
                }
                let baseline = cy + chip_h / 2 + cap as i32 * 7 / 20;
                circle(
                    c,
                    cr.x + 10 * si + 3 * si,
                    cy + chip_h / 2,
                    7 * si,
                    col,
                    255,
                );
                let tx = cr.x + 23 * si;
                let w = draw_text(c, title_t, name, cap, tx, baseline, p.fg_text);
                if *untrusted {
                    draw_text(
                        c,
                        ui,
                        "untrusted",
                        cap,
                        tx + w + 5 * si,
                        baseline,
                        p.fg_text,
                    );
                }
                if v.commands {
                    let line = format!("room {name}");
                    let lw = mono.measure(&line, cap);
                    draw_text(
                        c,
                        mono,
                        &line,
                        cap,
                        centre - lw / 2,
                        label_y(cr.y) + 14 * si,
                        p.fg_text_secondary,
                    );
                }
            }
            Cell::Plus => {
                let cy = cr.y + (icon - chip_h) / 2;
                let x = centre - chip_h / 2;
                let m = paint::rounded_rect(
                    chip_h as usize,
                    chip_h as usize,
                    (chip_h / 2) as u32,
                    CornerShape::Circle,
                );
                let o = paint::rounded_outline(
                    chip_h as usize,
                    chip_h as usize,
                    (chip_h / 2) as u32,
                    k,
                    CornerShape::Circle,
                );
                c.mask(&m, x, cy, p.bg_control, CHIP_OTHER);
                c.mask(&o, x, cy, p.border, 255);
                let mid = cy + chip_h / 2;
                c.fill(
                    paint::Rect::new(
                        centre - 5 * si,
                        mid - si / 2 - (si + 1) / 4,
                        10 * si,
                        (3 * si + 1) / 2,
                    ),
                    p.fg_text_secondary,
                );
                c.fill(
                    paint::Rect::new(
                        centre - si / 2 - (si + 1) / 4,
                        mid - 5 * si,
                        (3 * si + 1) / 2,
                        10 * si,
                    ),
                    p.fg_text_secondary,
                );
                if v.commands {
                    let lw = mono.measure("room new", cap);
                    draw_text(
                        c,
                        mono,
                        "room new",
                        cap,
                        centre - lw / 2,
                        label_y(cr.y) + 14 * si,
                        p.fg_text_secondary,
                    );
                }
            }
        }
    }
    c.fill(
        paint::Rect::new(
            g.divider as i32,
            r.y + space::DOCK_PADDING.physical(Scale::ONE) as i32 * si + icon * 15 / 100,
            si,
            icon * 70 / 100,
        ),
        p.border,
    );
    // The tooltip over the hovered icon: its name and its line.
    if let Some(i) = v.hover
        && let (Some(app), Some((_, cr))) = (
            v.apps.get(i),
            g.cells.iter().find(|(c, _)| *c == Cell::App(i)),
        )
    {
        let cr = prect(*cr);
        let lines = tooltip_lines(&app.name, &app.line);
        tag(
            c,
            &lines,
            cr.x + cr.w / 2,
            r.y - 8 * si,
            k,
            p,
            (title_t, mono),
        );
    }
}

/// The dock tooltip's lines: the name and the line, or the line alone when
/// the name is the line already ("files", not "files files").
pub fn tooltip_lines<'a>(name: &'a str, line: &'a str) -> Vec<(&'a str, bool)> {
    if name == line {
        alloc::vec![(line, true)]
    } else {
        alloc::vec![(name, false), (line, true)]
    }
}

/// A monochrome glyph for an application's tile (ADR-0161: icons are
/// monochrome): a terminal's prompt, a folder, a page, a gear; any other
/// application its initial.
#[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
pub fn icon_glyph(
    c: &mut Canvas<'_>,
    app: &str,
    x: i32,
    y: i32,
    icon: i32,
    k: u32,
    fg: Color,
    t: &mut zoraya_text::Text<'static>,
) {
    let si = k as i32;
    let inset = icon / 5;
    let (gx, gy, gs) = (x + inset, y + inset, icon - 2 * inset);
    let thick = (3 * k).div_ceil(2);
    match app {
        "term" => {
            let frame =
                paint::rounded_outline(gs as usize, gs as usize, 3 * k, thick, shape::CORNER);
            c.mask(&frame, gx, gy, fg, 255);
            let a = (gs / 4, gs / 3);
            let m = (gs / 2, gs / 2);
            let b = (gs / 4, gs * 2 / 3);
            for (p, q) in [(a, m), (b, m)] {
                let seg = paint::segment(gs as usize, gs as usize, p, q, thick);
                c.mask(&seg, gx, gy, fg, 255);
            }
            c.fill(
                paint::Rect::new(gx + gs / 2 + si, gy + gs * 2 / 3 - si, gs / 4, thick as i32),
                fg,
            );
        }
        "files" => {
            let body =
                paint::rounded_rect(gs as usize, (gs * 3 / 4) as usize, 2 * k, shape::CORNER);
            c.mask(&body, gx, gy + gs / 4, fg, 255);
            let tab = paint::rounded_rect(
                (gs * 2 / 5) as usize,
                (gs / 4 + 2 * si) as usize,
                2 * k,
                shape::CORNER,
            );
            c.mask(&tab, gx, gy + gs / 8, fg, 255);
        }
        "edit" => {
            let w = gs * 3 / 4;
            let page = paint::rounded_outline(w as usize, gs as usize, 2 * k, thick, shape::CORNER);
            let px = gx + (gs - w) / 2;
            c.mask(&page, px, gy, fg, 255);
            for i in 1..4 {
                c.fill(
                    paint::Rect::new(
                        px + w / 5,
                        gy + gs * i / 4 - si / 2,
                        w * 3 / 5,
                        thick as i32,
                    ),
                    fg,
                );
            }
        }
        "settings" => {
            // A gear: eight teeth around a thick ring with a hole.
            let m = gs / 2;
            let ring_r = gs * 7 / 20;
            let band = (gs / 6).max(thick as i32);
            let d = ring_r * 2;
            let ring = paint::rounded_outline(
                d as usize,
                d as usize,
                ring_r as u32,
                band as u32,
                CornerShape::Circle,
            );
            c.mask(&ring, gx + m - ring_r, gy + m - ring_r, fg, 255);
            // Unit directions in thousandths: every 45 degrees.
            const DIRS: [(i32, i32); 8] = [
                (1000, 0),
                (707, 707),
                (0, 1000),
                (-707, 707),
                (-1000, 0),
                (-707, -707),
                (0, -1000),
                (707, -707),
            ];
            let (inner, outer) = (ring_r - band / 2, m - si);
            let tooth = (gs / 6).max(2 * si) as u32;
            for (dx, dy) in DIRS {
                let a = (m + dx * inner / 1000, m + dy * inner / 1000);
                let b = (m + dx * outer / 1000, m + dy * outer / 1000);
                let seg = paint::segment(gs as usize, gs as usize, a, b, tooth);
                c.mask(&seg, gx, gy, fg, 255);
            }
        }
        _ => {
            let initial: String = app.chars().take(1).flat_map(char::to_uppercase).collect();
            let px = text::TITLE.physical(Scale::ONE) * k;
            let w = t.measure(&initial, px);
            draw_text(
                c,
                t,
                &initial,
                px,
                x + icon / 2 - w / 2,
                y + icon / 2 + px as i32 * 7 / 20,
                fg,
            );
        }
    }
}

/// The show-commands tag of a window (ADR-0172 point 8): its close
/// gesture's line, anchored inside its own frame at the body's lower left.
/// The caller draws it with the window and clipped to it, so a window above
/// covers it as it covers the window, and it never lies over another.
pub fn frame_tags(
    c: &mut Canvas<'_>,
    o: paint::Rect,
    v: &WinView,
    k: u32,
    p: &Palette,
    texts: (
        &mut zoraya_text::Text<'static>,
        &mut zoraya_text::Text<'static>,
    ),
) {
    if v.app.is_empty() {
        return;
    }
    let (t, mono) = texts;
    let si = k as i32;
    let line = format!("kill {}", v.app);
    let cap = text::CAPTION.physical(Scale::ONE) * k;
    let w = mono.measure(&line, cap) + 20 * si;
    tag(
        c,
        &[(line.as_str(), true)],
        o.x + 12 * si + w / 2,
        o.y + o.h - 12 * si,
        k,
        p,
        (t, mono),
    );
}

/// The text form of every gesture the compositor and the panel own (ADR-0172
/// point 8, INV-0172-5): the handler and its `zosh` line, with `APP`,
/// `NAME` and `LAYOUT` for what the gesture names.
pub const GESTURES: [(&str, &str); 6] = [
    ("close", "kill APP"),
    ("dock-launch", "APP"),
    ("dock-room", "room NAME"),
    ("dock-new-room", "room new NAME +w PATH"),
    ("new-terminal", "term"),
    ("layout", "settings gui.keyboard-layout LAYOUT"),
];

/// The gestures with no `zosh` form, and why: the table is whole, and what
/// has no line says so (ADR-0172's open question on focus).
pub const WITHOUT_TEXT: [(&str, &str); 7] = [
    (
        "minimise",
        "hiding a window is not the shell's bg: the program keeps running in the foreground",
    ),
    (
        "switcher",
        "focus is not the shell's fg (ADR-0172, open question)",
    ),
    (
        "next-window",
        "focus is not the shell's fg (ADR-0172, open question)",
    ),
    ("tile", "window geometry has no zosh form yet"),
    ("move", "window geometry has no zosh form yet"),
    ("resize", "window geometry has no zosh form yet"),
    ("show-commands", "the mode that shows the lines"),
];

/// The bar's trusted mark while the show-commands mode is open.
pub const COMMANDS_HEADING: &str = "Show commands · System+? to leave";

/// A sheet's heading (ADR-0167): what it asks and for which application,
/// the application from the compositor's own record, never the request.
pub fn sheet_heading(kind: u32, app: &str) -> String {
    let what = match kind {
        1 => "Save a file",
        2 => "Choose a folder",
        _ => "Open a file",
    };
    if app.is_empty() {
        String::from(what)
    } else {
        alloc::format!("{what} for {app}")
    }
}

/// The system emblem (ADR-0167): a Z on a rounded square in
/// `bg.frame.system`, `18 × 18` logical pixels from `x`, centred on `cy`.
pub fn emblem(
    c: &mut Canvas<'_>,
    x: i32,
    cy: i32,
    k: u32,
    p: &Palette,
    t: &mut zoraya_text::Text<'static>,
) -> i32 {
    let si = k as i32;
    let side = 18 * si;
    let y = cy - side / 2;
    let m = paint::rounded_rect(side as usize, side as usize, 4 * k, shape::CORNER);
    c.mask(&m, x, y, p.bg_frame_system, 255);
    let o = paint::rounded_outline(side as usize, side as usize, 4 * k, k, shape::CORNER);
    c.mask(&o, x, y, p.border, 255);
    let px = text::BODY.physical(Scale::ONE) * k;
    let w = t.measure("Z", px);
    draw_text(
        c,
        t,
        "Z",
        px,
        x + (side - w) / 2,
        cy + px as i32 * 7 / 20,
        p.fg_text,
    );
    side
}

/// A sheet's strip, above its body: the trusted material, the emblem and
/// the heading, all the compositor's (ADR-0167's first mark; INV-0167-4).
pub fn sheet_strip(
    c: &mut Canvas<'_>,
    w: &crate::window::Window,
    f: &crate::Frame,
    k: u32,
    p: &Palette,
    t: &mut zoraya_text::Text<'static>,
) {
    let o = prect(w.outer(f));
    let strip = paint::Rect::new(o.x, o.y, o.w, f.title as i32);
    if strip.intersect(&c.clip).is_empty() {
        return;
    }
    material(p, k, 0).draw(c, strip);
    let si = k as i32;
    c.fill(
        paint::Rect::new(o.x, strip.y + strip.h - si, o.w, si),
        p.border,
    );
    let cy = strip.y + strip.h / 2;
    let x = o.x + 12 * si;
    let side = emblem(c, x, cy, k, p, t);
    let px = text::BODY.physical(Scale::ONE) * k;
    draw_text(
        c,
        t,
        &w.title,
        px,
        x + side + 10 * si,
        cy + px as i32 * 7 / 20,
        p.fg_text,
    );
}

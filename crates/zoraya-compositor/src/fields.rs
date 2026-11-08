//! Rooms as fields on the wallpaper (ADR-0172 point 5, the README's Field
//! rules), in integer arithmetic over pixel centres in half pixels.
//!
//! A room's *shape* is the union of its windows' rectangles, each a rounded
//! rectangle of `radius` grown by `padding`. A pixel is in room R's field
//! when it is in R's shape, at least `gap` inside the work area, and
//! farther than `padding + gap` from every other coloured room's windows.
//! The distance to a rounded rectangle moves by at most one pixel per
//! pixel, so a pixel of one field and a pixel of another are more than
//! `gap` apart (INV-0172-8), and every tinted pixel is within `padding` of
//! a window of its room (INV-0172-7). The caller draws fields only when two
//! or more rooms have windows on screen (INV-0172-6) and leaves a maximised
//! window out of the shapes (INV-0172-9).

use alloc::vec::Vec;

use zoraya_tokens::Color;

use crate::rect::Rect;

/// One window's part in the fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// The room's index in the caller's list.
    pub room: usize,
    /// The window's outer rectangle, physical pixels.
    pub rect: Rect,
}

/// The fields' sizes in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    pub padding: i64,
    pub gap: i64,
    /// The windows' corner radius.
    pub radius: i64,
}

/// Twice the distance from the pixel centre `(x, y)` to the rounded
/// rectangle `r` with corners of `radius`, squared, compared against
/// `limit`: true when the distance is at most `limit` pixels.
fn within(r: &Rect, radius: i64, x: i64, y: i64, limit: i64) -> bool {
    let radius = radius.min(r.width / 2).min(r.height / 2).max(0);
    let (px, py) = (2 * x + 1, 2 * y + 1);
    let (lo_x, hi_x) = (2 * (r.x + radius), 2 * (r.right() - radius));
    let (lo_y, hi_y) = (2 * (r.y + radius), 2 * (r.bottom() - radius));
    let dx = (lo_x - px).max(px - hi_x).max(0);
    let dy = (lo_y - py).max(py - hi_y).max(0);
    let reach = 2 * (limit + radius);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) <= reach.saturating_mul(reach)
}

/// Whether the pixel `(x, y)` is in `room`'s field.
pub fn in_field(shapes: &[Shape], room: usize, work: Rect, s: Sizes, x: i64, y: i64) -> bool {
    let inner = Rect::new(
        work.x + s.gap,
        work.y + s.gap,
        work.width - 2 * s.gap,
        work.height - 2 * s.gap,
    );
    inner.contains(x, y)
        && shapes
            .iter()
            .any(|w| w.room == room && within(&w.rect, s.radius, x, y, s.padding))
        && !shapes
            .iter()
            .any(|w| w.room != room && within(&w.rect, s.radius, x, y, s.padding + s.gap))
}

/// Whether the pixel is on the field's edge: in the field, with a
/// neighbour (of the four) outside it.
fn on_edge(shapes: &[Shape], room: usize, work: Rect, s: Sizes, x: i64, y: i64) -> bool {
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .iter()
        .any(|(dx, dy)| !in_field(shapes, room, work, s, x + dx, y + dy))
}

/// The smallest rectangle holding `room`'s field.
pub fn bounds(shapes: &[Shape], room: usize, work: Rect, s: Sizes) -> Rect {
    let mut b = Rect::default();
    for w in shapes.iter().filter(|w| w.room == room) {
        let r = w.rect;
        b = b.union(&Rect::new(
            r.x - s.padding,
            r.y - s.padding,
            r.width + 2 * s.padding,
            r.height + 2 * s.padding,
        ));
    }
    b.intersect(&work)
}

/// The alpha of a field's ground and of its one-pixel line (the mockups'
/// 34 and 255).
pub const GROUND_ALPHA: u8 = 34;

/// Tints `px` (`width` wide, the wallpaper's copy) with each room's field:
/// `colours[i]` is room i's ground colour and line colour, or `None` for a
/// room with no colour (`home`), which has no field and cuts no other.
/// Gives the rectangles it changed.
pub fn paint(
    px: &mut [u32],
    width: usize,
    shapes: &[Shape],
    colours: &[Option<(Color, Color)>],
    work: Rect,
    s: Sizes,
) -> Vec<Rect> {
    let coloured: Vec<Shape> = shapes
        .iter()
        .copied()
        .filter(|w| colours.get(w.room).copied().flatten().is_some())
        .collect();
    let height = px.len().checked_div(width).unwrap_or(0);
    let screen = Rect::new(0, 0, width as i64, height as i64);
    let mut changed = Vec::new();
    for (room, c) in colours.iter().enumerate() {
        let Some((ground, line)) = *c else { continue };
        let b = bounds(&coloured, room, work, s).intersect(&screen);
        if b.is_empty() {
            continue;
        }
        for y in b.y..b.bottom() {
            for x in b.x..b.right() {
                if !in_field(&coloured, room, work, s, x, y) {
                    continue;
                }
                let i = y as usize * width + x as usize;
                px[i] = if on_edge(&coloured, room, work, s, x, y) {
                    line.pixel()
                } else {
                    Color::of_pixel(px[i]).blend(ground, GROUND_ALPHA).pixel()
                };
            }
        }
        changed.push(b);
    }
    changed
}

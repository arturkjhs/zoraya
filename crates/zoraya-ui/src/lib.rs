//! Zoraya's widget toolkit (ADR-0165, Option D): a tree of widgets that lay
//! out in logical pixels from the tokens, paint into a window's buffer at
//! the display's scale through `zoraya-paint` and `zoraya-text`, and take
//! the compositor's events. It sends nothing to anyone (INV-0165-2): it
//! depends on `zoraya-proto-gui` for nothing but key codes' meaning, and an
//! application (`zoraya-app`) commits what it painted.
//!
//! Every colour, size and radius comes from `zoraya-tokens` (INV-0165-3):
//! sizes are `Logical`, which only the tokens crate makes, so a widget can
//! only combine token sizes and text measures.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod layout;
pub mod widgets;

use alloc::boxed::Box;
use alloc::vec::Vec;

use zoraya_paint::{Canvas, Rect as PRect};
use zoraya_text::{Font, FontList, Rect as TRect, Target, Text};
use zoraya_tokens::{Appearance, Color, Logical, Palette, Scale, space, text};

pub use layout::{Column, Grid, Row};
pub use widgets::*;

/// A rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LRect {
    pub x: Logical,
    pub y: Logical,
    pub w: Logical,
    pub h: Logical,
}

impl LRect {
    pub fn new(x: Logical, y: Logical, w: Logical, h: Logical) -> LRect {
        LRect { x, y, w, h }
    }

    pub fn right(&self) -> Logical {
        self.x + self.w
    }

    pub fn bottom(&self) -> Logical {
        self.y + self.h
    }

    /// The rectangle `pad` in on every side.
    pub fn inset(&self, pad: Logical) -> LRect {
        LRect::new(
            self.x + pad,
            self.y + pad,
            self.w - pad.times(2),
            self.h - pad.times(2),
        )
    }

    /// Whether the logical point `(x, y)` lies in it.
    pub fn contains(&self, x: Logical, y: Logical) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    /// In physical pixels at `scale`.
    pub fn physical(&self, scale: Scale) -> PRect {
        let p = |l: Logical| l.physical(scale).min(i32::MAX as u32 / 4) as i32;
        PRect::new(p(self.x), p(self.y), p(self.w), p(self.h))
    }
}

/// What a widget is given: a key, the pointer, focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A key pressed (releases are not given to widgets), with the text it
    /// makes under the layout and the modifier bits of `zoraya-proto-gui`.
    Key {
        code: u32,
        text: Option<char>,
        modifiers: u32,
    },
    /// The pointer at a logical point, with the buttons held and the
    /// modifiers (`zoraya-proto-gui`'s bits, the system key included).
    Pointer {
        x: Logical,
        y: Logical,
        buttons: u32,
        modifiers: u32,
    },
    /// The widget gained or lost the window's keyboard focus.
    Focus(bool),
    /// The wheel turned with the pointer at a logical point (proposed
    /// ADR-0329, over ADR-0322's `Wheel`): `dx` and `dy` in 120ths of a
    /// notch ([`WHEEL_NOTCH`]), `dy` above 0 toward the person (the view
    /// moves down the content), `dx` above 0 to the right. The window
    /// gives it to the widget under the pointer and then to each of its
    /// ancestors until one takes it; it moves no focus.
    Wheel {
        x: Logical,
        y: Logical,
        dx: i32,
        dy: i32,
    },
}

/// One notch of a wheel in [`Event::Wheel`]'s units (ADR-0322's
/// `WHEEL_STEP`, the 120 of Linux's high-resolution axes).
pub const WHEEL_NOTCH: i32 = 120;
/// The rows, lines or children a notch scrolls: the terminal's three
/// lines (`zoraya_term::WHEEL_LINES`), proposed ADR-0329.
pub const ROWS_PER_NOTCH: i32 = 3;
/// The most one turn counts either way: ADR-0322's `WHEEL_MAX`, 64
/// notches.
const WHEEL_MAX: i32 = 64 * WHEEL_NOTCH;

/// A wheel's turns made into whole rows (proposed ADR-0329): a turn of
/// less than a row (a high-resolution wheel's part of a notch) is kept
/// until the next turns make a row, so a fine wheel scrolls as far as one
/// with notches, and smoothly, a row at a time.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WheelSteps {
    rest: i32,
}

impl WheelSteps {
    /// The rows `d` (above 0: toward the person, down the content) moves
    /// the view, with what was kept from earlier turns; at most 64 notches'
    /// worth either way.
    pub fn turn(&mut self, d: i32) -> i32 {
        let per_row = WHEEL_NOTCH / ROWS_PER_NOTCH;
        // A turn the other way drops the part kept from the first way.
        if (d < 0 && self.rest > 0) || (d > 0 && self.rest < 0) {
            self.rest = 0;
        }
        let sum = self.rest.saturating_add(d.clamp(-WHEEL_MAX, WHEEL_MAX));
        let sum = sum.clamp(-WHEEL_MAX, WHEEL_MAX);
        self.rest = sum % per_row;
        sum / per_row
    }

    /// Forgets the part of a row kept: the view met its end, so a turn
    /// toward that end does not count later.
    pub fn stop(&mut self) {
        self.rest = 0;
    }

    /// The part of a row kept, in 120ths of a notch.
    pub fn rest(&self) -> i32 {
        self.rest
    }
}

/// Where a scrolling widget is (proposed ADR-0329): its first row, line or
/// child shown, and whether the person moved it away from its cursor with
/// the wheel (then it does not follow the cursor until a key moves it or
/// the cursor leaves `cursor`, where it was then).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Scrolled {
    /// What scrolls: a restore only goes to a widget of the same kind.
    pub kind: &'static str,
    pub at: usize,
    pub free: bool,
    pub cursor: usize,
}

/// `at` moved by `rows` within `0..=last`.
pub fn scrolled_by(at: usize, rows: i32, last: usize) -> usize {
    let to = if rows < 0 {
        at.saturating_sub(rows.unsigned_abs() as usize)
    } else {
        at.saturating_add(rows as usize)
    };
    to.min(last)
}

/// What handling an event did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// Nothing the application needs to know.
    None,
    /// The widget changed its own look: paint again.
    Repaint,
    /// A button, a list row or a menu item was activated: its id.
    Activated(u32),
    /// A value changed (a field, a check box, a switch, a radio group).
    Changed(u32),
    /// A menu or a dialog asks to be closed.
    Closed,
}

/// Key codes the toolkit acts on (evdev, as `zoraya-keymap` names them).
pub mod keys {
    pub const ESC: u32 = 1;
    pub const BACKSPACE: u32 = 14;
    pub const TAB: u32 = 15;
    pub const ENTER: u32 = 28;
    pub const SPACE: u32 = 57;
    pub const HOME: u32 = 102;
    pub const UP: u32 = 103;
    pub const LEFT: u32 = 105;
    pub const RIGHT: u32 = 106;
    pub const END: u32 = 107;
    pub const DOWN: u32 = 108;
    pub const DELETE: u32 = 111;
    /// The Shift bit of the modifiers.
    pub const SHIFT: u32 = 1;
}

/// What painting needs: the scale, the colours, the text.
pub struct Painter {
    pub scale: Scale,
    pub palette: &'static Palette,
    pub ui: Text<'static>,
    pub medium: Text<'static>,
    pub mono: Text<'static>,
}

fn list(primary: &'static [u8]) -> FontList<'static> {
    FontList::new(
        [
            primary,
            zoraya_fonts::INTER_REGULAR,
            zoraya_fonts::MONO_REGULAR,
        ]
        .into_iter()
        .filter_map(|d| Font::parse(d).ok()),
    )
}

impl Painter {
    pub fn new(scale: Scale, appearance: Appearance) -> Painter {
        Painter {
            scale,
            palette: appearance.palette(),
            ui: Text::new(list(zoraya_fonts::INTER_REGULAR)),
            medium: Text::new(list(zoraya_fonts::INTER_MEDIUM)),
            mono: Text::new(list(zoraya_fonts::MONO_REGULAR)),
        }
    }

    /// The width of `s` at the body size, in logical pixels (rounded up).
    pub fn text_width(&mut self, s: &str) -> Logical {
        let px = text::BODY.physical(self.scale);
        let w = self.ui.measure(s, px).max(0) as u32;
        self.scale.logical(w + self.scale.factor() - 1)
    }

    /// Draws `s` at the body size, vertically centred in `r`, from its left
    /// plus `pad`, clipped to `r` and the canvas's clip.
    pub fn label(
        &mut self,
        c: &mut Canvas<'_>,
        s: &str,
        r: LRect,
        pad: Logical,
        color: Color,
        medium: bool,
    ) {
        let px = text::BODY.physical(self.scale);
        let pr = r.physical(self.scale);
        let clip = pr.intersect(&c.clip);
        if clip.is_empty() {
            return;
        }
        let baseline = pr.y + pr.h / 2 + px as i32 * 7 / 20;
        let x = pr.x + pad.physical(self.scale) as i32;
        let mut target = Target {
            pixels: c.pixels,
            stride: c.width,
            clip: TRect::new(clip.x, clip.y, clip.w as u32, clip.h as u32),
        };
        let t = if medium {
            &mut self.medium
        } else {
            &mut self.ui
        };
        t.draw(&mut target, s, px, x, baseline, color);
    }
}

/// A widget: it measures itself, takes a rectangle, paints inside it and
/// handles events. Containers hold children and hand them on.
pub trait Widget {
    /// The size it wants, in logical pixels.
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical);
    /// Takes its rectangle (and lays out its children).
    fn layout(&mut self, r: LRect, p: &mut Painter);
    fn rect(&self) -> LRect;
    /// Paints itself inside `rect()` (and the canvas's clip), and only
    /// there (INV-0165-1).
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool);
    /// Handles an event; a container passes it to the child it is for.
    fn event(&mut self, _e: &Event) -> Response {
        Response::None
    }
    /// Takes keyboard focus with Tab.
    fn focusable(&self) -> bool {
        false
    }
    /// Its children, in tree order (for focus and hits).
    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut []
    }
    /// A dialog or an open menu: Escape closes it.
    fn closes_on_escape(&self) -> bool {
        false
    }
    /// The id the application gave it, if it has one.
    fn id(&self) -> Option<u32> {
        None
    }
    /// What the person set in it, for the application to read after a
    /// `Changed` or `Activated` (proposed ADR-0244).
    fn value(&self) -> Value<'_> {
        Value::None
    }
    /// Where it is scrolled, if it scrolls (proposed ADR-0329).
    fn scrolled(&self) -> Option<Scrolled> {
        None
    }
    /// Puts it where `s` says (clamped at its next layout), if it scrolls.
    fn set_scrolled(&mut self, _s: Scrolled) {}
    /// Scrolls so that its child `child` is in view, if it scrolls its
    /// children: the keyboard's focus is never on a widget out of view.
    /// Returns whether it moved.
    fn reveal(&mut self, _child: usize) -> bool {
        false
    }
}

/// A widget's value, by [`Widget::value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    None,
    /// A list's selected row or a radio group's choice.
    Index(Option<usize>),
    /// A field's text.
    Text(&'a str),
    /// A check box's or a switch's state.
    On(bool),
}

/// The path to the widget with `id` in the tree, depth first.
fn find(w: &mut dyn Widget, id: u32, path: &mut Vec<usize>) -> bool {
    if w.id() == Some(id) {
        return true;
    }
    let n = w.children().len();
    for i in 0..n {
        path.push(i);
        if find(w.children()[i].as_mut(), id, path) {
            return true;
        }
        path.pop();
    }
    false
}

/// A window's widget tree with its keyboard focus (ADR-0165): Tab and
/// Shift+Tab move focus over the focusable widgets in tree order, keys go
/// to the focused widget, the pointer to the widget under it, Escape closes
/// a menu or dialog.
pub struct Window {
    pub root: Box<dyn Widget>,
    pub size: (Logical, Logical),
    /// The focused widget, as its index among the focusable ones.
    focus: Option<usize>,
}

/// The paths (child indices from the root) of the focusable widgets.
fn focusables(w: &mut dyn Widget, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    if w.focusable() {
        out.push(path.clone());
    }
    let n = w.children().len();
    for i in 0..n {
        path.push(i);
        focusables(w.children()[i].as_mut(), path, out);
        path.pop();
    }
}

fn at_path<'a>(w: &'a mut dyn Widget, path: &[usize]) -> Option<&'a mut dyn Widget> {
    match path.split_first() {
        None => Some(w),
        Some((i, rest)) => {
            let child = w.children().get_mut(*i)?;
            at_path(child.as_mut(), rest)
        }
    }
}

/// The deepest widget under a logical point.
fn hit(w: &mut dyn Widget, x: Logical, y: Logical, path: &mut Vec<usize>) -> bool {
    if !w.rect().contains(x, y) {
        return false;
    }
    let n = w.children().len();
    for i in (0..n).rev() {
        path.push(i);
        if hit(w.children()[i].as_mut(), x, y, path) {
            return true;
        }
        path.pop();
    }
    true
}

impl Window {
    pub fn new(root: Box<dyn Widget>, size: (Logical, Logical)) -> Window {
        Window {
            root,
            size,
            focus: None,
        }
    }

    /// The value of the widget with `id`.
    pub fn value(&mut self, id: u32) -> Value<'_> {
        let mut path = Vec::new();
        if !find(self.root.as_mut(), id, &mut path) {
            return Value::None;
        }
        at_path(self.root.as_mut(), &path).map_or(Value::None, |w| w.value())
    }

    /// Lays the tree out over the whole window.
    pub fn layout(&mut self, p: &mut Painter) {
        let r = LRect::new(Logical::ZERO, Logical::ZERO, self.size.0, self.size.1);
        self.root.layout(r, p);
    }

    fn paths(&mut self) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        focusables(self.root.as_mut(), &mut Vec::new(), &mut out);
        out
    }

    /// The focused widget's path, if any.
    pub fn focused(&mut self) -> Option<Vec<usize>> {
        let paths = self.paths();
        self.focus.and_then(|i| paths.get(i).cloned())
    }

    /// Moves focus to the next (or previous) focusable widget, around.
    pub fn move_focus(&mut self, back: bool) {
        let paths = self.paths();
        if paths.is_empty() {
            self.focus = None;
            return;
        }
        let n = paths.len();
        let next = match self.focus {
            None if back => n - 1,
            None => 0,
            Some(i) if back => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
        };
        if let Some(old) = self.focus.and_then(|i| paths.get(i))
            && let Some(w) = at_path(self.root.as_mut(), old)
        {
            w.event(&Event::Focus(false));
        }
        self.focus = Some(next);
        if let Some(w) = at_path(self.root.as_mut(), &paths[next]) {
            w.event(&Event::Focus(true));
        }
        self.reveal_focus();
    }

    /// Hands an event to the tree.
    pub fn event(&mut self, e: Event) -> Response {
        match e {
            Event::Key {
                code: keys::TAB,
                modifiers,
                ..
            } => {
                self.move_focus(modifiers & keys::SHIFT != 0);
                Response::Repaint
            }
            Event::Key {
                code: keys::ESC, ..
            } if self.root.closes_on_escape() => Response::Closed,
            Event::Key { .. } => match self.focused() {
                Some(path) => {
                    // A key goes to a widget the person sees.
                    let moved = self.reveal_focus();
                    let r =
                        at_path(self.root.as_mut(), &path).map_or(Response::None, |w| w.event(&e));
                    // A view that scrolled to show the widget is painted
                    // again whatever the widget made of the key.
                    if moved && r == Response::None {
                        Response::Repaint
                    } else {
                        r
                    }
                }
                None => Response::None,
            },
            Event::Pointer { x, y, buttons, .. } => {
                let mut path = Vec::new();
                if !hit(self.root.as_mut(), x, y, &mut path) {
                    return Response::None;
                }
                // A press focuses what it lands on, if it takes focus.
                if buttons != 0 {
                    let paths = self.paths();
                    if let Some(i) = paths.iter().position(|p| *p == path)
                        && self.focus != Some(i)
                    {
                        self.focus = Some(i);
                    }
                }
                at_path(self.root.as_mut(), &path).map_or(Response::None, |w| w.event(&e))
            }
            Event::Wheel { x, y, .. } => {
                let mut path = Vec::new();
                if !hit(self.root.as_mut(), x, y, &mut path) {
                    return Response::None;
                }
                // The deepest widget first, then its ancestors: the
                // nearest one that scrolls that way takes the turn.
                loop {
                    let r =
                        at_path(self.root.as_mut(), &path).map_or(Response::None, |w| w.event(&e));
                    if r != Response::None || path.pop().is_none() {
                        return r;
                    }
                }
            }
            Event::Focus(_) => Response::None,
        }
    }

    /// Scrolls every ancestor of the focused widget so that it is in view.
    /// Returns whether any of them moved.
    fn reveal_focus(&mut self) -> bool {
        let Some(path) = self.focused() else {
            return false;
        };
        let mut moved = false;
        for d in 0..path.len() {
            if let Some(w) = at_path(self.root.as_mut(), &path[..d]) {
                moved |= w.reveal(path[d]);
            }
        }
        moved
    }

    /// Where the widget with `id` is scrolled, if it scrolls.
    pub fn scrolled(&mut self, id: u32) -> Option<Scrolled> {
        let mut path = Vec::new();
        if !find(self.root.as_mut(), id, &mut path) {
            return None;
        }
        at_path(self.root.as_mut(), &path).and_then(|w| w.scrolled())
    }

    /// Puts `root` in place of the tree, keeping the focus and where each
    /// scrolling widget was (by its place in the tree, when a widget that
    /// scrolls is there again), and lays it out.
    pub fn replace_keeping_scroll(&mut self, root: Box<dyn Widget>, p: &mut Painter) {
        let mut kept = Vec::new();
        scroll_positions(self.root.as_mut(), &mut Vec::new(), &mut kept);
        self.root = root;
        for (path, s) in kept {
            if let Some(w) = at_path(self.root.as_mut(), &path)
                && w.scrolled().is_some_and(|now| now.kind == s.kind)
            {
                w.set_scrolled(s);
            }
        }
        self.layout(p);
    }

    /// Paints the whole tree into `pixels` (rows of `stride` physical
    /// pixels), the window's ground first.
    pub fn paint(&mut self, p: &mut Painter, pixels: &mut [u32], stride: usize) {
        let mut c = Canvas::new(pixels, stride);
        c.fill(c.clip, p.palette.bg_window);
        let focused = self.focused();
        paint_tree(
            self.root.as_mut(),
            p,
            &mut c,
            &mut Vec::new(),
            focused.as_deref(),
        );
    }
}

/// The paths of the widgets that scroll, with where each is.
fn scroll_positions(
    w: &mut dyn Widget,
    path: &mut Vec<usize>,
    out: &mut Vec<(Vec<usize>, Scrolled)>,
) {
    if let Some(s) = w.scrolled() {
        out.push((path.clone(), s));
    }
    let n = w.children().len();
    for i in 0..n {
        path.push(i);
        scroll_positions(w.children()[i].as_mut(), path, out);
        path.pop();
    }
}

fn paint_tree(
    w: &mut dyn Widget,
    p: &mut Painter,
    c: &mut Canvas<'_>,
    path: &mut Vec<usize>,
    focus: Option<&[usize]>,
) {
    let r = w.rect().physical(p.scale);
    let old = c.set_clip(r.intersect(&c.clip));
    w.paint(p, c, focus == Some(path.as_slice()));
    let n = w.children().len();
    for i in 0..n {
        path.push(i);
        paint_tree(w.children()[i].as_mut(), p, c, path, focus);
        path.pop();
    }
    c.clip = old;
}

/// The focus ring: two logical pixels of `accent` inside a widget.
pub fn focus_ring(p: &Painter, c: &mut Canvas<'_>, r: LRect) {
    let pr = r.physical(p.scale);
    let m = zoraya_paint::rounded_outline(
        pr.w.max(0) as usize,
        pr.h.max(0) as usize,
        space::RADIUS_CONTROL.physical(p.scale),
        2 * p.scale.factor(),
        zoraya_tokens::shape::CORNER,
    );
    c.mask(&m, pr.x, pr.y, p.palette.accent, 255);
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_wheel;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

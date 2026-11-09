//! The widgets of stage 7 (ADR-0165): `Label`, `Button`, `TextField`,
//! `CheckBox`, `Switch`, `RadioGroup`, `ListView`, `ScrollView`,
//! `Separator`, `Menu`, `Dialog`, `ProgressBar`. Each takes its colours,
//! sizes and radii from the tokens, paints only inside its rectangle, and
//! answers events with a `Response`.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_paint::{Canvas, rounded_outline, rounded_rect};
use zoraya_tokens::{Color, Logical, shape, space, text};

use crate::{Event, LRect, Painter, Response, Value, Widget, focus_ring, keys};

/// Fills `r` with `color` in a rounded shape of `radius`.
fn shape_fill(p: &Painter, c: &mut Canvas<'_>, r: LRect, radius: Logical, color: Color) {
    let pr = r.physical(p.scale);
    if pr.is_empty() {
        return;
    }
    let m = rounded_rect(
        pr.w as usize,
        pr.h as usize,
        radius.physical(p.scale),
        shape::CORNER,
    );
    c.mask(&m, pr.x, pr.y, color, 255);
}

fn shape_edge(p: &Painter, c: &mut Canvas<'_>, r: LRect, radius: Logical, color: Color) {
    let pr = r.physical(p.scale);
    if pr.is_empty() {
        return;
    }
    let m = rounded_outline(
        pr.w as usize,
        pr.h as usize,
        radius.physical(p.scale),
        space::FRAME_BORDER.physical(p.scale),
        shape::CORNER,
    );
    c.mask(&m, pr.x, pr.y, color, 255);
}

/// A press of the primary button inside the widget.
fn pressed_in(e: &Event, r: LRect) -> bool {
    matches!(e, Event::Pointer { x, y, buttons } if *buttons & 1 != 0 && r.contains(*x, *y))
}

/// Text, in `fg.text` (or `fg.text.secondary`).
pub struct Label {
    pub text: String,
    pub secondary: bool,
    rect: LRect,
}

impl Label {
    pub fn new(text: &str) -> Label {
        Label {
            text: String::from(text),
            secondary: false,
            rect: LRect::default(),
        }
    }
}

impl Widget for Label {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        (p.text_width(&self.text), space::CONTROL_HEIGHT)
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        let color = if self.secondary {
            p.palette.fg_text_secondary
        } else {
            p.palette.fg_text
        };
        p.label(c, &self.text, self.rect, Logical::ZERO, color, false);
    }
}

/// A button: `bg.control` (or `accent` for the default one), its label in
/// the body size, `radius.control`; Enter, Space or a press activate it.
pub struct Button {
    pub id: u32,
    pub label: String,
    pub default: bool,
    pub pressed: bool,
    rect: LRect,
}

impl Button {
    pub fn new(id: u32, label: &str) -> Button {
        Button {
            id,
            label: String::from(label),
            default: false,
            pressed: false,
            rect: LRect::default(),
        }
    }
}

impl Widget for Button {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        (
            p.text_width(&self.label) + space::PADDING_CONTROL.times(4),
            space::CONTROL_HEIGHT,
        )
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let (bg, fg) = match (self.default, self.pressed) {
            (true, _) => (p.palette.accent, p.palette.fg_text_on_accent),
            (false, true) => (p.palette.bg_control_pressed, p.palette.fg_text),
            (false, false) => (p.palette.bg_control, p.palette.fg_text),
        };
        shape_fill(p, c, self.rect, space::RADIUS_CONTROL, bg);
        shape_edge(p, c, self.rect, space::RADIUS_CONTROL, p.palette.border);
        let w = p.text_width(&self.label);
        let pad = (self.rect.w - w).divided(2);
        p.label(c, &self.label.clone(), self.rect, pad, fg, true);
        if focused {
            focus_ring(p, c, self.rect);
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        match e {
            Event::Key {
                code: keys::ENTER | keys::SPACE,
                ..
            } => Response::Activated(self.id),
            e if pressed_in(e, self.rect) => Response::Activated(self.id),
            _ => Response::None,
        }
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
}

/// One line of editable text: a cursor as a character index, editing by
/// character (ADR-0154 rule 7's rules where they overlap).
pub struct TextField {
    pub id: u32,
    pub value: String,
    /// The cursor, in characters.
    pub cursor: usize,
    rect: LRect,
}

impl TextField {
    pub fn new(id: u32, value: &str) -> TextField {
        TextField {
            id,
            value: String::from(value),
            cursor: value.chars().count(),
            rect: LRect::default(),
        }
    }

    fn byte(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map_or(self.value.len(), |(i, _)| i)
    }
}

impl Widget for TextField {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (space::CONTROL_HEIGHT.times(8), space::CONTROL_HEIGHT)
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        shape_fill(p, c, self.rect, space::RADIUS_CONTROL, p.palette.bg_view);
        shape_edge(
            p,
            c,
            self.rect,
            space::RADIUS_CONTROL,
            p.palette.border_strong,
        );
        p.label(
            c,
            &self.value.clone(),
            self.rect,
            space::PADDING_CONTROL,
            p.palette.fg_text,
            false,
        );
        if focused {
            // The cursor: a bar after the text before it.
            let before: String = self.value.chars().take(self.cursor).collect();
            let x = self.rect.x + space::PADDING_CONTROL + p.text_width(&before);
            let bar = LRect::new(
                x,
                self.rect.y + space::UNIT,
                space::FRAME_BORDER,
                self.rect.h - space::UNIT.times(2),
            );
            c.fill(bar.physical(p.scale), p.palette.fg_text);
            focus_ring(p, c, self.rect);
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        let Event::Key { code, text, .. } = *e else {
            return Response::None;
        };
        let n = self.value.chars().count();
        match code {
            keys::LEFT => self.cursor = self.cursor.saturating_sub(1),
            keys::RIGHT => self.cursor = (self.cursor + 1).min(n),
            keys::HOME => self.cursor = 0,
            keys::END => self.cursor = n,
            keys::BACKSPACE if self.cursor > 0 => {
                let (a, b) = (self.byte(self.cursor - 1), self.byte(self.cursor));
                self.value.replace_range(a..b, "");
                self.cursor -= 1;
                return Response::Changed(self.id);
            }
            keys::DELETE if self.cursor < n => {
                let (a, b) = (self.byte(self.cursor), self.byte(self.cursor + 1));
                self.value.replace_range(a..b, "");
                return Response::Changed(self.id);
            }
            _ => match text {
                Some(ch) if !ch.is_control() => {
                    let at = self.byte(self.cursor);
                    self.value.insert(at, ch);
                    self.cursor += 1;
                    return Response::Changed(self.id);
                }
                _ => return Response::None,
            },
        }
        Response::Repaint
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
    fn value(&self) -> Value<'_> {
        Value::Text(&self.value)
    }
}

/// A box with a check mark and a label.
pub struct CheckBox {
    pub id: u32,
    pub label: String,
    pub checked: bool,
    rect: LRect,
}

impl CheckBox {
    pub fn new(id: u32, label: &str, checked: bool) -> CheckBox {
        CheckBox {
            id,
            label: String::from(label),
            checked,
            rect: LRect::default(),
        }
    }

    fn side() -> Logical {
        text::BODY + space::UNIT
    }

    fn toggle(&mut self) -> Response {
        self.checked = !self.checked;
        Response::Changed(self.id)
    }
}

impl Widget for CheckBox {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        (
            CheckBox::side() + space::GAP_CONTROLS + p.text_width(&self.label),
            space::CONTROL_HEIGHT,
        )
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let side = CheckBox::side();
        let b = LRect::new(
            self.rect.x,
            self.rect.y + (self.rect.h - side).divided(2),
            side,
            side,
        );
        let fill = if self.checked {
            p.palette.accent
        } else {
            p.palette.bg_control
        };
        shape_fill(p, c, b, space::RADIUS_CONTROL.divided(2), fill);
        shape_edge(
            p,
            c,
            b,
            space::RADIUS_CONTROL.divided(2),
            p.palette.border_strong,
        );
        if self.checked {
            let inner = b.inset(space::UNIT);
            c.fill(inner.physical(p.scale), p.palette.fg_text_on_accent);
        }
        let pad = side + space::GAP_CONTROLS;
        p.label(
            c,
            &self.label.clone(),
            self.rect,
            pad,
            p.palette.fg_text,
            false,
        );
        if focused {
            focus_ring(p, c, b);
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        match e {
            Event::Key {
                code: keys::SPACE, ..
            } => self.toggle(),
            e if pressed_in(e, self.rect) => self.toggle(),
            _ => Response::None,
        }
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
    fn value(&self) -> Value<'_> {
        Value::On(self.checked)
    }
}

/// An on and off switch: a track in `accent` when on, a knob.
pub struct Switch {
    pub id: u32,
    pub on: bool,
    rect: LRect,
}

impl Switch {
    pub fn new(id: u32, on: bool) -> Switch {
        Switch {
            id,
            on,
            rect: LRect::default(),
        }
    }
}

impl Widget for Switch {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (
            space::CONTROL_HEIGHT + space::PADDING_WINDOW,
            space::CONTROL_HEIGHT,
        )
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let h = self.rect.h;
        let track = if self.on {
            p.palette.accent
        } else {
            p.palette.border_strong
        };
        shape_fill(p, c, self.rect, h.divided(2), track);
        let knob = h - space::UNIT;
        let x = if self.on {
            self.rect.right() - knob - space::UNIT.divided(2)
        } else {
            self.rect.x + space::UNIT.divided(2)
        };
        let k = LRect::new(x, self.rect.y + space::UNIT.divided(2), knob, knob);
        shape_fill(p, c, k, knob.divided(2), p.palette.bg_control);
        if focused {
            focus_ring(p, c, self.rect);
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        let flip = matches!(
            e,
            Event::Key {
                code: keys::SPACE | keys::ENTER,
                ..
            }
        ) || pressed_in(e, self.rect);
        if flip {
            self.on = !self.on;
            return Response::Changed(self.id);
        }
        Response::None
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
    fn value(&self) -> Value<'_> {
        Value::On(self.on)
    }
}

/// One choice of several: arrows move it.
pub struct RadioGroup {
    pub id: u32,
    pub options: Vec<String>,
    pub chosen: usize,
    rect: LRect,
}

impl RadioGroup {
    pub fn new(id: u32, options: &[&str], chosen: usize) -> RadioGroup {
        RadioGroup {
            id,
            options: options.iter().map(|o| String::from(*o)).collect(),
            chosen: chosen.min(options.len().saturating_sub(1)),
            rect: LRect::default(),
        }
    }
}

impl Widget for RadioGroup {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        let mut w = Logical::ZERO;
        for o in &self.options {
            w = w.max(p.text_width(o));
        }
        (
            w + CheckBox::side() + space::GAP_CONTROLS,
            space::CONTROL_HEIGHT.times(self.options.len() as u32),
        )
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let side = CheckBox::side();
        for (i, o) in self.options.clone().iter().enumerate() {
            let row = LRect::new(
                self.rect.x,
                self.rect.y + space::CONTROL_HEIGHT.times(i as u32),
                self.rect.w,
                space::CONTROL_HEIGHT,
            );
            let dot = LRect::new(row.x, row.y + (row.h - side).divided(2), side, side);
            let fill = if i == self.chosen {
                p.palette.accent
            } else {
                p.palette.bg_control
            };
            shape_fill(p, c, dot, side.divided(2), fill);
            shape_edge(p, c, dot, side.divided(2), p.palette.border_strong);
            p.label(
                c,
                o,
                row,
                side + space::GAP_CONTROLS,
                p.palette.fg_text,
                false,
            );
            if focused && i == self.chosen {
                focus_ring(p, c, dot);
            }
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        let n = self.options.len();
        if n == 0 {
            return Response::None;
        }
        let next = match e {
            Event::Key {
                code: keys::DOWN | keys::RIGHT,
                ..
            } => (self.chosen + 1) % n,
            Event::Key {
                code: keys::UP | keys::LEFT,
                ..
            } => (self.chosen + n - 1) % n,
            Event::Pointer { y, buttons, .. } if buttons & 1 != 0 => {
                let mut i = 0;
                while i + 1 < n && self.rect.y + space::CONTROL_HEIGHT.times(i as u32 + 1) <= *y {
                    i += 1;
                }
                i
            }
            _ => return Response::None,
        };
        self.chosen = next;
        Response::Changed(self.id)
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
    fn value(&self) -> Value<'_> {
        Value::Index(Some(self.chosen))
    }
}

/// Rows of text with a selection; arrows move it, Enter activates it.
pub struct ListView {
    pub id: u32,
    pub rows: Vec<String>,
    /// Each row's detail, right-aligned in `fg.text.secondary` (a size);
    /// empty for none.
    pub details: Vec<String>,
    pub selected: Option<usize>,
    /// The first row shown.
    pub top: usize,
    rect: LRect,
}

impl ListView {
    pub fn new(id: u32, rows: &[&str]) -> ListView {
        ListView {
            id,
            rows: rows.iter().map(|r| String::from(*r)).collect(),
            details: Vec::new(),
            selected: None,
            top: 0,
            rect: LRect::default(),
        }
    }

    fn visible(&self) -> usize {
        let mut n = 0;
        while space::CONTROL_HEIGHT.times(n as u32 + 1) <= self.rect.h {
            n += 1;
        }
        n.max(1)
    }

    fn select(&mut self, i: usize) -> Response {
        self.selected = Some(i);
        let v = self.visible();
        if i < self.top {
            self.top = i;
        } else if i >= self.top + v {
            self.top = i + 1 - v;
        }
        Response::Changed(self.id)
    }
}

impl Widget for ListView {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        let mut w = space::CONTROL_HEIGHT.times(6);
        for r in &self.rows {
            w = w.max(p.text_width(r) + space::PADDING_CONTROL.times(2));
        }
        (
            w,
            space::CONTROL_HEIGHT.times((self.rows.len() as u32).clamp(1, 8)),
        )
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        c.fill(self.rect.physical(p.scale), p.palette.bg_view);
        let v = self.visible();
        for (k, i) in (self.top..self.rows.len()).take(v).enumerate() {
            let row = LRect::new(
                self.rect.x,
                self.rect.y + space::CONTROL_HEIGHT.times(k as u32),
                self.rect.w,
                space::CONTROL_HEIGHT,
            );
            let (bg, fg) = if self.selected == Some(i) {
                (p.palette.bg_selection, p.palette.fg_text_on_selection)
            } else {
                (p.palette.bg_view, p.palette.fg_text)
            };
            c.fill(row.physical(p.scale), bg);
            let label = self.rows[i].clone();
            p.label(c, &label, row, space::PADDING_CONTROL, fg, false);
            if let Some(d) = self.details.get(i).filter(|d| !d.is_empty()).cloned() {
                let pad = row.w - space::PADDING_CONTROL - p.text_width(&d);
                let secondary = if self.selected == Some(i) {
                    fg
                } else {
                    p.palette.fg_text_secondary
                };
                p.label(c, &d, row, pad, secondary, false);
            }
        }
        if focused {
            focus_ring(p, c, self.rect);
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        let n = self.rows.len();
        if n == 0 {
            return Response::None;
        }
        match e {
            Event::Key {
                code: keys::DOWN, ..
            } => self.select(self.selected.map_or(0, |s| (s + 1).min(n - 1))),
            Event::Key { code: keys::UP, .. } => {
                self.select(self.selected.map_or(0, |s| s.saturating_sub(1)))
            }
            Event::Key {
                code: keys::HOME, ..
            } => self.select(0),
            Event::Key {
                code: keys::END, ..
            } => self.select(n - 1),
            Event::Key {
                code: keys::ENTER, ..
            } => self
                .selected
                .map_or(Response::None, |_| Response::Activated(self.id)),
            Event::Pointer { y, buttons, .. } if buttons & 1 != 0 => {
                let mut k = 0;
                while space::CONTROL_HEIGHT.times(k as u32 + 1) + self.rect.y <= *y {
                    k += 1;
                }
                let i = self.top + k;
                if i < n {
                    self.select(i)
                } else {
                    Response::None
                }
            }
            _ => Response::None,
        }
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
    fn value(&self) -> Value<'_> {
        Value::Index(self.selected)
    }
}

/// A child larger than its frame, shifted by a scroll offset; the child
/// paints only inside the frame.
pub struct ScrollView {
    pub child: Vec<Box<dyn Widget>>,
    /// How far down the child is scrolled, in logical pixels.
    pub offset: Logical,
    rect: LRect,
}

impl ScrollView {
    pub fn new(child: Box<dyn Widget>) -> ScrollView {
        ScrollView {
            child: alloc::vec![child],
            offset: Logical::ZERO,
            rect: LRect::default(),
        }
    }

    /// Scrolls by `lines` control heights (negative: up), within the child.
    pub fn scroll(&mut self, down: bool, p: &mut Painter) {
        let step = space::CONTROL_HEIGHT;
        // `layout` keeps the offset within the content.
        self.offset = if down {
            self.offset + step
        } else {
            self.offset - step
        };
        let r = self.rect;
        self.layout(r, p);
    }
}

impl Widget for ScrollView {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        let (w, _) = self.child[0].measure(p);
        (w, space::CONTROL_HEIGHT.times(4))
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        let (_, h) = self.child[0].measure(p);
        // At most the content less the frame, also after the frame grows:
        // a frame of a new size shows no blank past the content.
        if self.offset > h - r.h {
            self.offset = h - r.h;
        }
        // The child's top is above the frame by the offset: its rectangle
        // starts at the frame's top with its height cut by the offset, so
        // that nothing is placed above the frame in logical units.
        let shown = h - self.offset;
        self.child[0].layout(
            LRect::new(r.x, r.y, r.w, if shown > r.h { r.h } else { shown }),
            p,
        );
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        c.fill(self.rect.physical(p.scale), p.palette.bg_view);
    }
    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.child
    }
}

/// A line of `border`.
pub struct Separator {
    rect: LRect,
}

impl Separator {
    pub fn new() -> Separator {
        Separator {
            rect: LRect::default(),
        }
    }
}

impl Default for Separator {
    fn default() -> Separator {
        Separator::new()
    }
}

impl Widget for Separator {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (Logical::ZERO, space::UNIT)
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        let mid = LRect::new(
            self.rect.x,
            self.rect.y + self.rect.h.divided(2),
            self.rect.w,
            space::FRAME_BORDER,
        );
        c.fill(mid.physical(p.scale), p.palette.border);
    }
}

/// A menu: items in a popup (ADR-0160); Escape closes it.
pub struct Menu {
    pub list: ListView,
}

impl Menu {
    pub fn new(id: u32, items: &[&str]) -> Menu {
        let mut list = ListView::new(id, items);
        list.selected = (!items.is_empty()).then_some(0);
        Menu { list }
    }
}

impl Widget for Menu {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        self.list.measure(p)
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.list.layout(r, p);
    }
    fn rect(&self) -> LRect {
        self.list.rect()
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        self.list.paint(p, c, false);
        shape_edge(
            p,
            c,
            self.list.rect(),
            space::RADIUS_POPUP,
            p.palette.border,
        );
    }
    fn event(&mut self, e: &Event) -> Response {
        match e {
            Event::Key {
                code: keys::ESC, ..
            } => Response::Closed,
            _ => self.list.event(e),
        }
    }
    fn focusable(&self) -> bool {
        true
    }
    fn closes_on_escape(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.list.id)
    }
    fn value(&self) -> Value<'_> {
        Value::Index(self.list.selected)
    }
}

/// A dialog: a message and buttons in a column; Escape closes it, Enter
/// activates the default button.
pub struct Dialog {
    pub column: Vec<Box<dyn Widget>>,
    rect: LRect,
}

impl Dialog {
    pub fn new(message: &str, buttons: Vec<Button>) -> Dialog {
        let mut row: Vec<Box<dyn Widget>> = Vec::new();
        for b in buttons {
            row.push(Box::new(b));
        }
        let col: Vec<Box<dyn Widget>> = alloc::vec![
            Box::new(Label::new(message)),
            Box::new(crate::layout::Row::new(row)),
        ];
        Dialog {
            column: alloc::vec![Box::new(crate::layout::Column::new(col))],
            rect: LRect::default(),
        }
    }
}

impl Widget for Dialog {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        self.column[0].measure(p)
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        self.column[0].layout(r, p);
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        c.fill(self.rect.physical(p.scale), p.palette.bg_window);
    }
    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.column
    }
    fn closes_on_escape(&self) -> bool {
        true
    }
}

/// How far a task has come, from 0 to 1000.
pub struct ProgressBar {
    pub permille: u32,
    rect: LRect,
}

impl ProgressBar {
    pub fn new(permille: u32) -> ProgressBar {
        ProgressBar {
            permille: permille.min(1000),
            rect: LRect::default(),
        }
    }
}

impl Widget for ProgressBar {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (space::CONTROL_HEIGHT.times(6), space::UNIT.times(2))
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        shape_fill(p, c, self.rect, self.rect.h.divided(2), p.palette.border);
        let done = self.rect.w.times(self.permille.min(1000)).divided(1000);
        let r = LRect::new(self.rect.x, self.rect.y, done, self.rect.h);
        shape_fill(p, c, r, self.rect.h.divided(2), p.palette.accent);
    }
}

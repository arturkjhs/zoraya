//! The editor's text view, a `zoraya-ui` widget over a shared [`Buffer`]:
//! the lines in the monospace face, the selection, the cursor, a vertical
//! scroll that keeps the cursor in view; keys move and type, Ctrl+A
//! selects all, Ctrl+Z undoes, Ctrl+Y and Ctrl+Shift+Z redo; a press puts
//! the cursor in the cell under it. The wheel scrolls three lines a notch
//! (proposed ADR-0329) and leaves the cursor where it is; the next key
//! brings the view back to it.

use alloc::rc::Rc;
use alloc::string::String;
use core::cell::RefCell;

use zoraya_paint::Canvas;
use zoraya_text::{Rect as TRect, Target};
use zoraya_tokens::{Logical, space, text};
use zoraya_ui::{
    Event, LRect, Painter, Response, Scrolled, Value, WheelSteps, Widget, keys, widgets::wheel_rows,
};

use crate::{Buffer, Move};

/// evdev codes of the letters the view's chords use.
const KEY_A: u32 = 30;
const KEY_Z: u32 = 44;
const KEY_Y: u32 = 21;
/// The Ctrl bit of the modifiers (`zoraya-keymap`'s).
const CTRL: u32 = 2;

pub struct EditorView {
    pub id: u32,
    pub buffer: Rc<RefCell<Buffer>>,
    rect: LRect,
    /// The first line shown.
    top: usize,
    /// The wheel moved the view away from the cursor: it does not follow
    /// the cursor until a key or a press, or the cursor moves (a find).
    free: bool,
    /// The cursor at the last `follow`, to see that it moved.
    seen: usize,
    wheel: WheelSteps,
    /// From the last paint: the scale's factor, the line height, a cell's
    /// width and the padding, physical; what a press maps through.
    cell: (u32, i32, i32, i32),
}

impl EditorView {
    pub fn new(id: u32, buffer: Rc<RefCell<Buffer>>) -> EditorView {
        EditorView {
            id,
            buffer,
            rect: LRect::default(),
            top: 0,
            free: false,
            // Unknown: the first paint follows the cursor.
            seen: usize::MAX,
            wheel: WheelSteps::default(),
            cell: (1, 1, 1, 0),
        }
    }

    /// The line height and one character's advance, physical.
    fn metrics(p: &mut Painter) -> (i32, i32, u32) {
        let px = text::BODY.physical(p.scale);
        let lh = (px as i32 * 3) / 2;
        let cw = p.mono.measure("M", px).max(1);
        (lh, cw, px)
    }

    /// The lines that fit, at least one.
    fn rows(&self, p: &mut Painter) -> usize {
        let (lh, _, _) = Self::metrics(p);
        let pad = space::PADDING_CONTROL.physical(p.scale) as i32;
        let h = self.rect.physical(p.scale).h - 2 * pad;
        (h / lh.max(1)).max(1) as usize
    }

    /// Keeps the cursor's line in view, unless the wheel moved the view
    /// away from it; the last line no higher than the bottom either way.
    fn follow(&mut self, rows: usize) {
        let b = self.buffer.borrow();
        if b.cursor() != self.seen {
            self.seen = b.cursor();
            self.free = false;
        }
        if !self.free {
            let (line, _) = b.line_col(b.cursor());
            if line < self.top {
                self.top = line;
            } else if line >= self.top + rows {
                self.top = line + 1 - rows;
            }
        }
        self.top = self.top.min(b.lines().len().saturating_sub(rows));
    }

    /// The lines that fit, by the last paint's metrics.
    fn rows_painted(&self) -> usize {
        let (k, lh, _, pad) = self.cell;
        let scale = zoraya_tokens::Scale::new(k).unwrap_or(zoraya_tokens::Scale::ONE);
        let h = self.rect.physical(scale).h - 2 * pad;
        (h / lh.max(1)).max(1) as usize
    }

    /// The first line shown.
    pub fn top(&self) -> usize {
        self.top
    }
}

impl Widget for EditorView {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (
            space::CONTROL_HEIGHT.times(12),
            space::CONTROL_HEIGHT.times(8),
        )
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
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

    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let rows = self.rows(p);
        self.follow(rows);
        let pr = self.rect.physical(p.scale);
        c.fill(pr, p.palette.bg_view);
        let clip = pr.intersect(&c.clip);
        if clip.is_empty() {
            return;
        }
        let (lh, cw, px) = Self::metrics(p);
        let pad = space::PADDING_CONTROL.physical(p.scale) as i32;
        self.cell = (p.scale.factor(), lh, cw, pad);
        let b = self.buffer.borrow();
        let (sa, sb) = b.selection();
        let lines = b.lines();
        let mut start = 0usize;
        for (i, line) in lines.iter().enumerate() {
            let n = line.chars().count();
            if i >= self.top && i < self.top + rows {
                let y = pr.y + pad + (i - self.top) as i32 * lh;
                let x0 = pr.x + pad;
                // The selection on this line (a line end counts as a cell).
                let (a, z) = (sa.max(start), sb.min(start + n + 1));
                if a < z {
                    let r = zoraya_paint::Rect::new(
                        x0 + (a - start) as i32 * cw,
                        y,
                        (z - a) as i32 * cw,
                        lh,
                    );
                    c.fill(r.intersect(&clip), p.palette.bg_selection);
                }
                let mut target = Target {
                    pixels: c.pixels,
                    stride: c.width,
                    clip: TRect::new(clip.x, clip.y, clip.w as u32, clip.h as u32),
                };
                let baseline = y + lh / 2 + px as i32 * 7 / 20;
                // Tabs as four cells.
                let shown: String = line.replace('\t', "    ");
                p.mono
                    .draw(&mut target, &shown, px, x0, baseline, p.palette.fg_text);
                let cur = b.cursor();
                if focused && cur >= start && cur <= start + n {
                    let col = line
                        .chars()
                        .take(cur - start)
                        .map(|ch| if ch == '\t' { 4 } else { 1 })
                        .sum::<usize>();
                    let bar = zoraya_paint::Rect::new(
                        x0 + col as i32 * cw,
                        y + lh / 8,
                        space::FRAME_BORDER.physical(p.scale).max(1) as i32,
                        lh - lh / 4,
                    );
                    c.fill(bar.intersect(&clip), p.palette.fg_text);
                }
            }
            start += n + 1;
        }
    }

    fn event(&mut self, e: &Event) -> Response {
        match *e {
            Event::Key {
                code,
                text,
                modifiers,
            } => {
                self.free = false;
                let shift = modifiers & keys::SHIFT != 0;
                let ctrl = modifiers & CTRL != 0;
                let mut b = self.buffer.borrow_mut();
                let changed = match code {
                    KEY_A if ctrl => {
                        b.select_all();
                        false
                    }
                    KEY_Z if ctrl && shift => b.redo(),
                    KEY_Z if ctrl => b.undo(),
                    KEY_Y if ctrl => b.redo(),
                    keys::LEFT => {
                        b.move_cursor(Move::Left, shift);
                        false
                    }
                    keys::RIGHT => {
                        b.move_cursor(Move::Right, shift);
                        false
                    }
                    keys::UP => {
                        b.move_cursor(Move::Up, shift);
                        false
                    }
                    keys::DOWN => {
                        b.move_cursor(Move::Down, shift);
                        false
                    }
                    keys::HOME => {
                        b.move_cursor(if ctrl { Move::Start } else { Move::LineStart }, shift);
                        false
                    }
                    keys::END => {
                        b.move_cursor(if ctrl { Move::End } else { Move::LineEnd }, shift);
                        false
                    }
                    keys::BACKSPACE => b.backspace(),
                    keys::DELETE => b.delete(),
                    keys::ENTER => b.insert("\n"),
                    keys::TAB => b.insert("\t"),
                    _ => match text {
                        Some(ch) if !ctrl && !ch.is_control() => {
                            let mut s = [0u8; 4];
                            b.insert(ch.encode_utf8(&mut s))
                        }
                        _ => return Response::None,
                    },
                };
                if changed {
                    Response::Changed(self.id)
                } else {
                    Response::Repaint
                }
            }
            Event::Pointer { x, y, buttons, .. }
                if buttons & 1 != 0 && self.rect.contains(x, y) =>
            {
                // The cell under the press, by the last paint's metrics.
                let (k, lh, cw, pad) = self.cell;
                let scale = zoraya_tokens::Scale::new(k).unwrap_or(zoraya_tokens::Scale::ONE);
                let pr = self.rect.physical(scale);
                let (px, py) = (x.physical(scale) as i32, y.physical(scale) as i32);
                let line = self.top + ((py - pr.y - pad).max(0) / lh.max(1)) as usize;
                let col = ((px - pr.x - pad).max(0) + cw / 2) / cw.max(1);
                let mut b = self.buffer.borrow_mut();
                let at = b.at(line, col as usize);
                b.select(at, at);
                self.free = false;
                Response::Repaint
            }
            Event::Wheel { dy, .. } => {
                let last = self
                    .buffer
                    .borrow()
                    .lines()
                    .len()
                    .saturating_sub(self.rows_painted());
                let r = wheel_rows(&mut self.wheel, &mut self.top, dy, last);
                if r != Response::None {
                    self.free = true;
                    self.seen = self.buffer.borrow().cursor();
                }
                r
            }
            _ => Response::None,
        }
    }
    fn scrolled(&self) -> Option<Scrolled> {
        Some(Scrolled {
            at: self.top,
            free: self.free,
            cursor: self.seen,
        })
    }
    fn set_scrolled(&mut self, s: Scrolled) {
        self.top = s.at;
        self.free = s.free;
        self.seen = s.cursor;
    }
}

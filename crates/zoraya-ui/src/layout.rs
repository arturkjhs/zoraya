//! Containers (ADR-0165): a row, a column and a grid, gaps of `gap.controls`
//! and an inset of `padding`; sizes come from the children's own. No
//! constraint solver.

use alloc::boxed::Box;
use alloc::vec::Vec;

use zoraya_paint::Canvas;
use zoraya_tokens::{Logical, space};

use crate::{LRect, Painter, Widget};

fn min(a: Logical, b: Logical) -> Logical {
    if a <= b { a } else { b }
}

/// Children side by side, each its own width, the row's height.
pub struct Row {
    pub children: Vec<Box<dyn Widget>>,
    pub padding: Logical,
    rect: LRect,
}

/// Children one under another, the column's width each.
pub struct Column {
    pub children: Vec<Box<dyn Widget>>,
    pub padding: Logical,
    rect: LRect,
}

impl Row {
    pub fn new(children: Vec<Box<dyn Widget>>) -> Row {
        Row {
            children,
            padding: Logical::ZERO,
            rect: LRect::default(),
        }
    }
}

impl Column {
    pub fn new(children: Vec<Box<dyn Widget>>) -> Column {
        Column {
            children,
            padding: space::PADDING_WINDOW,
            rect: LRect::default(),
        }
    }
}

impl Widget for Row {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        let mut w = Logical::ZERO;
        let mut h = Logical::ZERO;
        for (i, c) in self.children.iter_mut().enumerate() {
            let (cw, ch) = c.measure(p);
            w = w
                + cw
                + if i > 0 {
                    space::GAP_CONTROLS
                } else {
                    Logical::ZERO
                };
            h = h.max(ch);
        }
        (w + self.padding.times(2), h + self.padding.times(2))
    }

    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        let inner = r.inset(self.padding);
        let mut x = inner.x;
        for c in self.children.iter_mut() {
            let (cw, _) = c.measure(p);
            let w = min(cw, inner.right() - x);
            c.layout(LRect::new(x, inner.y, w, inner.h), p);
            x = x + w + space::GAP_CONTROLS;
        }
    }

    fn rect(&self) -> LRect {
        self.rect
    }

    fn paint(&mut self, _p: &mut Painter, _c: &mut Canvas<'_>, _focused: bool) {}

    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.children
    }
}

impl Widget for Column {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        let mut w = Logical::ZERO;
        let mut h = Logical::ZERO;
        for (i, c) in self.children.iter_mut().enumerate() {
            let (cw, ch) = c.measure(p);
            h = h
                + ch
                + if i > 0 {
                    space::GAP_CONTROLS
                } else {
                    Logical::ZERO
                };
            w = w.max(cw);
        }
        (w + self.padding.times(2), h + self.padding.times(2))
    }

    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        let inner = r.inset(self.padding);
        let mut y = inner.y;
        for c in self.children.iter_mut() {
            let (_, ch) = c.measure(p);
            let h = min(ch, inner.bottom() - y);
            c.layout(LRect::new(inner.x, y, inner.w, h), p);
            y = y + h + space::GAP_CONTROLS;
        }
    }

    fn rect(&self) -> LRect {
        self.rect
    }

    fn paint(&mut self, _p: &mut Painter, _c: &mut Canvas<'_>, _focused: bool) {}

    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.children
    }
}

/// Children in rows of `columns` equal cells.
pub struct Grid {
    pub children: Vec<Box<dyn Widget>>,
    pub columns: usize,
    rect: LRect,
}

impl Grid {
    pub fn new(columns: usize, children: Vec<Box<dyn Widget>>) -> Grid {
        Grid {
            children,
            columns: columns.max(1),
            rect: LRect::default(),
        }
    }

    fn cell(&mut self, p: &mut Painter) -> (Logical, Logical) {
        let mut w = Logical::ZERO;
        let mut h = Logical::ZERO;
        for c in self.children.iter_mut() {
            let (cw, ch) = c.measure(p);
            w = w.max(cw);
            h = h.max(ch);
        }
        (w, h)
    }
}

impl Widget for Grid {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        let (cw, ch) = self.cell(p);
        let rows = self.children.len().div_ceil(self.columns) as u32;
        let cols = self.columns.min(self.children.len()).max(1) as u32;
        let gap = space::GAP_CONTROLS;
        (
            cw.times(cols) + gap.times(cols.saturating_sub(1)),
            ch.times(rows) + gap.times(rows.saturating_sub(1)),
        )
    }

    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        let (cw, ch) = self.cell(p);
        let gap = space::GAP_CONTROLS;
        let columns = self.columns;
        for (i, c) in self.children.iter_mut().enumerate() {
            let (col, row) = ((i % columns) as u32, (i / columns) as u32);
            let x = r.x + (cw + gap).times(col);
            let y = r.y + (ch + gap).times(row);
            let w = min(cw, r.right() - x);
            let h = min(ch, r.bottom() - y);
            c.layout(LRect::new(x, y, w, h), p);
        }
    }

    fn rect(&self) -> LRect {
        self.rect
    }

    fn paint(&mut self, _p: &mut Painter, _c: &mut Canvas<'_>, _focused: bool) {}

    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.children
    }
}

//! Rectangles and their arithmetic, in signed screen coordinates so that a
//! window can start left of or above the screen without wrapping.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl Rect {
    pub const fn new(x: i64, y: i64, width: i64, height: i64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    pub const fn right(&self) -> i64 {
        self.x + self.width
    }

    pub const fn bottom(&self) -> i64 {
        self.y + self.height
    }

    /// The common part, empty if none.
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let right = self.right().min(o.right());
        let bottom = self.bottom().min(o.bottom());
        if right <= x || bottom <= y {
            return Rect::default();
        }
        Rect::new(x, y, right - x, bottom - y)
    }

    /// The smallest rectangle holding both. An empty one adds nothing.
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        let right = self.right().max(o.right());
        let bottom = self.bottom().max(o.bottom());
        Rect::new(x, y, right - x, bottom - y)
    }

    pub const fn contains(&self, px: i64, py: i64) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// What is left of `self` when `hole` is taken out: up to four
    /// rectangles (above, below, left and right of the hole), none empty,
    /// none overlapping.
    pub fn minus(&self, hole: &Rect) -> [Rect; 4] {
        let mut out = [Rect::default(); 4];
        if self.is_empty() {
            return out;
        }
        let h = self.intersect(hole);
        if h.is_empty() {
            out[0] = *self;
            return out;
        }
        out[0] = Rect::new(self.x, self.y, self.width, h.y - self.y);
        out[1] = Rect::new(self.x, h.bottom(), self.width, self.bottom() - h.bottom());
        out[2] = Rect::new(self.x, h.y, h.x - self.x, h.height);
        out[3] = Rect::new(h.right(), h.y, self.right() - h.right(), h.height);
        for r in &mut out {
            if r.is_empty() {
                *r = Rect::default();
            }
        }
        out
    }

    pub const fn translate(&self, dx: i64, dy: i64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// A rectangle a client sent, in `u32`s, checked against its surface:
    /// not empty, and inside `width × height`.
    pub fn from_client(x: u32, y: u32, w: u32, h: u32, width: u32, height: u32) -> Option<Rect> {
        let (x, y, w, h) = (i64::from(x), i64::from(y), i64::from(w), i64::from(h));
        let r = Rect::new(x, y, w, h);
        let ok = !r.is_empty() && r.right() <= i64::from(width) && r.bottom() <= i64::from(height);
        ok.then_some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(r: &[Rect]) -> i64 {
        r.iter().map(|r| r.width.max(0) * r.height.max(0)).sum()
    }

    #[test]
    fn minus_covers_exactly_what_the_hole_leaves() {
        let a = Rect::new(10, 20, 100, 80);
        for hole in [
            Rect::new(30, 30, 20, 20),
            Rect::new(0, 0, 40, 40),
            Rect::new(90, 90, 100, 100),
            Rect::new(10, 20, 100, 80),
            Rect::new(200, 200, 5, 5),
            Rect::new(-5, 30, 500, 10),
        ] {
            let parts = a.minus(&hole);
            assert_eq!(
                area(&parts),
                area(&[a]) - area(&[a.intersect(&hole)]),
                "{hole:?}"
            );
            for (i, p) in parts.iter().enumerate() {
                assert!(p.is_empty() || a.intersect(p) == *p, "inside");
                assert!(p.intersect(&hole).is_empty(), "outside the hole");
                for q in &parts[i + 1..] {
                    assert!(p.intersect(q).is_empty(), "disjoint");
                }
            }
        }
    }
}

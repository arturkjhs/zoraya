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

//! A window as the compositor holds it (ADR-0160), and its frame's
//! geometry in physical pixels: the title bar, the buttons, the grips, the
//! shadow's reach. Nothing here depends on a client's pixels: the frame is
//! the compositor's own (INV-0160-1).

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_paint::Mask;
use zoraya_tokens::{Scale, shadow, space};

use crate::rect::Rect;

/// What a window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// Stage 5's one surface per connection (`Create`): framed, never
    /// resized, events in the old form.
    Legacy,
    /// ADR-0160's toplevel: framed, configured, resizable.
    Toplevel,
    /// A popup of the toplevel `parent` (a window id of the same client).
    Popup { parent: u32 },
    /// A sheet (ADR-0160, ADR-0167): a system client's window hung under
    /// the title bar of another client's window, moved and stacked with it,
    /// under a strip the compositor draws.
    Sheet { owner_client: usize, owner_id: u32 },
}

/// A buffer a client gave: its mapping, its size in buffer pixels.
pub struct Buffer {
    pub address: u64,
    pub length: u64,
    pub width: u32,
    pub height: u32,
}

/// The frame's masks, made once per size.
pub struct Chrome {
    /// The body's size in physical pixels they were made for.
    pub size: (u32, u32),
    /// The whole window's shape (title bar and body).
    pub outer: Mask,
    /// The border, inside the shape.
    pub outline: Mask,
    /// The shadow's layers, each with its margin.
    pub shadows: Vec<(Mask, usize)>,
    /// The body's coverage: the window's shape over the body.
    pub body: Mask,
}

pub struct Window {
    pub client: usize,
    /// The client's name for it; 0 for a `Legacy` surface.
    pub id: u32,
    pub kind: WindowKind,
    /// The body's top left on the screen, physical pixels.
    pub x: i64,
    pub y: i64,
    /// The body's size on the screen, physical pixels: what the last
    /// `Configure` gave, or the buffer's size for a legacy surface.
    pub body: (u32, u32),
    /// The buffer whose pixels are shown, and the compositor's copy of it.
    pub buffer: Buffer,
    pub copy: Vec<u32>,
    /// A buffer given after a `Configure`, shown from its first commit
    /// (INV-0160-5).
    pub next: Option<Buffer>,
    pub title: String,
    pub hidden: bool,
    /// When the person pressed close, for "close anyway" (ADR-0160).
    pub close_asked: Option<u64>,
    pub chrome: Option<Chrome>,
}

/// The frame's sizes at a scale, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub title: i64,
    pub border: i64,
    pub grip: i64,
    pub radius: u32,
    /// The farthest the shadow reaches past the window, down included.
    pub shadow: i64,
}

impl Frame {
    pub fn at(scale: Scale) -> Frame {
        let blur = shadow::WINDOW_BLUR.physical(scale) as i64;
        let offset = shadow::WINDOW_OFFSET.physical(scale) as i64;
        Frame {
            title: space::FRAME_HEIGHT.physical(scale) as i64,
            border: space::FRAME_BORDER.physical(scale) as i64,
            grip: space::FRAME_GRIP.physical(scale) as i64,
            radius: space::RADIUS_WINDOW.physical(scale),
            shadow: blur * 2 + 2 + offset,
        }
    }
}

impl Window {
    pub fn framed(&self) -> bool {
        !matches!(
            self.kind,
            WindowKind::Popup { .. } | WindowKind::Sheet { .. }
        )
    }

    /// The window a sheet hangs under: its client and id.
    pub fn owner(&self) -> Option<(usize, u32)> {
        match self.kind {
            WindowKind::Sheet {
                owner_client,
                owner_id,
            } => Some((owner_client, owner_id)),
            _ => None,
        }
    }

    /// The body on the screen.
    pub fn body_rect(&self) -> Rect {
        Rect::new(
            self.x,
            self.y,
            i64::from(self.body.0),
            i64::from(self.body.1),
        )
    }

    /// The whole window: the title bar above the body and the border
    /// around both. A popup is its body.
    pub fn outer(&self, f: &Frame) -> Rect {
        let b = self.body_rect();
        // A sheet's strip, the compositor's, is above its body.
        if self.owner().is_some() {
            return Rect::new(b.x, b.y - f.title, b.width, b.height + f.title);
        }
        if !self.framed() {
            return b;
        }
        Rect::new(
            b.x - f.border,
            b.y - f.title,
            b.width + 2 * f.border,
            b.height + f.title + f.border,
        )
    }

    /// The title bar.
    pub fn title_bar(&self, f: &Frame) -> Rect {
        let o = self.outer(f);
        Rect::new(o.x, o.y, o.width, f.title)
    }

    /// What drawing the window can change: the window and its shadow.
    pub fn reach(&self, f: &Frame) -> Rect {
        let o = self.outer(f);
        let d = if self.framed() {
            f.shadow
        } else {
            f.shadow / 2
        };
        Rect::new(o.x - d, o.y - d, o.width + 2 * d, o.height + 2 * d)
    }
}

/// Where a press lands on a framed window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Close,
    Minimise,
    Title,
    /// An edge or corner grip: which sides move (left, top, right, bottom).
    Grip {
        left: bool,
        top: bool,
        right: bool,
        bottom: bool,
    },
    Body,
}

/// The buttons' centres from the window's left, and their half size, in
/// logical pixels (the mockups: 18 and 42, glyphs 4.5 wide each way).
pub const CLOSE_X: u32 = 18;
pub const MINIMISE_X: u32 = 42;
pub const BUTTON_HALF: u32 = 10;

impl Window {
    /// The zone of the point `(x, y)` in this window, if it is in it.
    /// Grips lie inside the window's edge, `frame.grip` deep, and only a
    /// toplevel has them.
    pub fn zone(&self, f: &Frame, scale: Scale, x: i64, y: i64) -> Option<Zone> {
        let o = self.outer(f);
        if !o.contains(x, y) {
            return None;
        }
        // On a sheet, the strip is no client's: `Title`, which moves nothing.
        if self.owner().is_some() {
            return Some(if self.body_rect().contains(x, y) {
                Zone::Body
            } else {
                Zone::Title
            });
        }
        if !self.framed() {
            return Some(Zone::Body);
        }
        let s = i64::from(scale.factor());
        let bar = self.title_bar(f);
        let cy = bar.y + f.title / 2;
        let half = i64::from(BUTTON_HALF) * s;
        let near = |cx: i64| (x - cx).abs() <= half && (y - cy).abs() <= half;
        if near(o.x + i64::from(CLOSE_X) * s) {
            return Some(Zone::Close);
        }
        if near(o.x + i64::from(MINIMISE_X) * s) {
            return Some(Zone::Minimise);
        }
        if self.kind == WindowKind::Toplevel {
            let g = f.grip;
            let (left, right) = (x < o.x + g, x >= o.right() - g);
            let (top, bottom) = (y < o.y + g / 2, y >= o.bottom() - g);
            if left || right || top || bottom {
                return Some(Zone::Grip {
                    left,
                    top,
                    right,
                    bottom,
                });
            }
        }
        if bar.contains(x, y) {
            return Some(Zone::Title);
        }
        Some(Zone::Body)
    }
}

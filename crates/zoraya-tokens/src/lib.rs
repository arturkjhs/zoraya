//! Zoraya's design tokens (ADR-0161, Option C): every colour, size, radius,
//! shadow and duration the desktop draws with, read at compile time from
//! one file, `tokens.txt` (the architect's `clear-field` direction, Field
//! with dock B), with a light and a dark value for each colour.
//!
//! GUI crates take their values from here and nowhere else (INV-0161-1):
//! a size is a [`Logical`], whose constructors are private to this crate,
//! so a bare integer cannot become one elsewhere; it becomes physical
//! pixels only through a [`Scale`] (ADR-0164). A colour is a [`Color`] of
//! a [`Palette`], a duration a [`Millis`].

#![no_std]
#![forbid(unsafe_code)]

pub mod palette;
pub mod parse;

pub use palette::{COLOR_NAMES, DARK, LIGHT, Palette};

use core::ops::{Add, Sub};
use parse::{int, is, parse_milli, value, word};

/// The token file, as compiled in.
pub const SOURCE: &str = include_str!("../tokens.txt");

/// A size in logical pixels (ADR-0164). Only this crate makes one from a
/// number; others get them from the tokens and combine them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Logical(u32);

impl Logical {
    pub const ZERO: Logical = Logical(0);

    const fn token(name: &str) -> Logical {
        Logical(int(name))
    }

    /// Physical pixels at `scale`.
    pub const fn physical(self, scale: Scale) -> u32 {
        self.0 * scale.0
    }

    /// `n` times this size.
    pub const fn times(self, n: u32) -> Logical {
        Logical(self.0.saturating_mul(n))
    }

    /// This size divided by `n` (rounded down); `n` of 0 gives zero.
    pub const fn divided(self, n: u32) -> Logical {
        match self.0.checked_div(n) {
            Some(v) => Logical(v),
            None => Logical(0),
        }
    }

    /// The larger of the two.
    pub const fn max(self, other: Logical) -> Logical {
        if self.0 >= other.0 { self } else { other }
    }
}

impl Add for Logical {
    type Output = Logical;
    fn add(self, other: Logical) -> Logical {
        Logical(self.0.saturating_add(other.0))
    }
}

impl Sub for Logical {
    type Output = Logical;
    /// Saturating: a size is never negative.
    fn sub(self, other: Logical) -> Logical {
        Logical(self.0.saturating_sub(other.0))
    }
}

/// Physical pixels per logical pixel: 1 or 2 (ADR-0164).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale(u32);

impl Scale {
    pub const ONE: Scale = Scale(1);
    pub const TWO: Scale = Scale(2);

    /// The scales ADR-0164 allows; None for any other.
    pub const fn new(factor: u32) -> Option<Scale> {
        match factor {
            1 => Some(Scale::ONE),
            2 => Some(Scale::TWO),
            _ => None,
        }
    }

    pub const fn factor(self) -> u32 {
        self.0
    }

    /// Physical pixels to logical ones, rounded down.
    pub const fn logical(self, physical: u32) -> Logical {
        Logical(physical / self.0)
    }
}

/// A colour, `0x00rrggbb`, the pixel format of a surface (ADR-0093).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub(crate) u32);

impl Color {
    /// The pixel value: `0x00rrggbb`.
    pub const fn pixel(self) -> u32 {
        self.0
    }

    /// The colour of a pixel value (its top byte ignored): for code that
    /// reads pixels back, never for naming a colour.
    pub const fn of_pixel(pixel: u32) -> Color {
        Color(pixel & 0x00ff_ffff)
    }

    pub const fn r(self) -> u8 {
        (self.0 >> 16) as u8
    }
    pub const fn g(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub const fn b(self) -> u8 {
        self.0 as u8
    }

    /// `other` over this colour at `alpha` of 255, per channel in sRGB
    /// space, rounded (ADR-0162: no gamma correction). Integer arithmetic.
    pub const fn blend(self, other: Color, alpha: u8) -> Color {
        let a = alpha as u32;
        let r = mix(self.r() as u32, other.r() as u32, a);
        let g = mix(self.g() as u32, other.g() as u32, a);
        let b = mix(self.b() as u32, other.b() as u32, a);
        Color(r << 16 | g << 8 | b)
    }
}

/// One channel of `o` over `s` at `a` of 255, rounded.
const fn mix(s: u32, o: u32, a: u32) -> u32 {
    (s * (255 - a) + o * a + 127) / 255
}

/// A duration in milliseconds (ADR-0161's motion tokens).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Millis(u32);

impl Millis {
    pub const ZERO: Millis = Millis(0);

    pub const fn millis(self) -> u32 {
        self.0
    }

    pub const fn nanos(self) -> u64 {
        self.0 as u64 * 1_000_000
    }
}

/// A cubic Bézier easing curve, its control points in thousandths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ease {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

const fn milli(name: &str, n: usize) -> u32 {
    match parse_milli(word(name, n)) {
        Some(v) => v,
        None => panic!("an easing token that is not four decimals"),
    }
}

const fn ease(name: &str) -> Ease {
    Ease {
        x1: milli(name, 0),
        y1: milli(name, 1),
        x2: milli(name, 2),
        y2: milli(name, 3),
    }
}

/// Light or dark (ADR-0161 driver 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

impl Appearance {
    /// The colours of this set.
    pub const fn palette(self) -> &'static Palette {
        match self {
            Appearance::Light => &LIGHT,
            Appearance::Dark => &DARK,
        }
    }
}

/// An 8-bit alpha from a token.
const fn alpha(name: &str) -> u8 {
    let v = int(name);
    assert!(v <= 255, "an alpha token above 255");
    v as u8
}

/// How a corner is drawn (`radius.shape`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerShape {
    Circle,
    Smooth,
}

/// How the frame's buttons look (`frame.buttons`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameButtons {
    Circles,
    Squares,
    Glyphs,
}

/// How a window carries its room's colour (`room.style`, ADR-0160).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomStyle {
    None,
    Edge,
    Band,
    Ground,
}

/// The desktop's wallpaper (`wallpaper`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wallpaper {
    Flat,
    Gradient,
    Deep,
    Rooms,
}

const fn corner_shape() -> CornerShape {
    if is("radius.shape", "circle") {
        CornerShape::Circle
    } else if is("radius.shape", "smooth") {
        CornerShape::Smooth
    } else {
        panic!("radius.shape is circle or smooth")
    }
}

const fn frame_buttons() -> FrameButtons {
    if is("frame.buttons", "circles") {
        FrameButtons::Circles
    } else if is("frame.buttons", "squares") {
        FrameButtons::Squares
    } else if is("frame.buttons", "glyphs") {
        FrameButtons::Glyphs
    } else {
        panic!("frame.buttons is circles, squares or glyphs")
    }
}

const fn room_style() -> RoomStyle {
    if is("room.style", "none") {
        RoomStyle::None
    } else if is("room.style", "edge") {
        RoomStyle::Edge
    } else if is("room.style", "band") {
        RoomStyle::Band
    } else if is("room.style", "ground") {
        RoomStyle::Ground
    } else {
        panic!("room.style is none, edge, band or ground")
    }
}

const fn wallpaper() -> Wallpaper {
    if is("wallpaper", "flat") {
        Wallpaper::Flat
    } else if is("wallpaper", "gradient") {
        Wallpaper::Gradient
    } else if is("wallpaper", "deep") {
        Wallpaper::Deep
    } else if is("wallpaper", "rooms") {
        Wallpaper::Rooms
    } else {
        panic!("wallpaper is flat, gradient, deep or rooms")
    }
}

const fn yes_no(name: &str) -> bool {
    if is(name, "yes") {
        true
    } else if is(name, "no") {
        false
    } else {
        panic!("a token that is yes or no")
    }
}

/// The direction's name.
pub const NAME: &str = value("name");

/// The font files (ADR-0162), names under `fonts/`.
pub mod fonts {
    use crate::parse::value;
    pub const UI: &str = value("font.ui");
    pub const UI_MEDIUM: &str = value("font.ui.medium");
    pub const MONO: &str = value("font.mono");
    pub const MONO_BOLD: &str = value("font.mono.bold");
}

/// The type scale.
pub mod text {
    use crate::Logical;
    pub const CAPTION: Logical = Logical::token("text.caption");
    pub const BODY: Logical = Logical::token("text.body");
    pub const HEADING: Logical = Logical::token("text.heading");
    pub const TITLE: Logical = Logical::token("text.title");
    pub const LARGE: Logical = Logical::token("text.large");
    pub const MONO: Logical = Logical::token("text.mono");
}

/// Spacing, shape and the frame.
pub mod space {
    use crate::Logical;
    pub const UNIT: Logical = Logical::token("space.unit");
    pub const PADDING_CONTROL: Logical = Logical::token("padding.control");
    pub const PADDING_WINDOW: Logical = Logical::token("padding.window");
    pub const PADDING_TERMINAL: Logical = Logical::token("padding.terminal");
    pub const GAP_CONTROLS: Logical = Logical::token("gap.controls");
    pub const CONTROL_HEIGHT: Logical = Logical::token("control.height");
    pub const RADIUS_CONTROL: Logical = Logical::token("radius.control");
    pub const RADIUS_WINDOW: Logical = Logical::token("radius.window");
    pub const RADIUS_POPUP: Logical = Logical::token("radius.popup");
    pub const FRAME_HEIGHT: Logical = Logical::token("frame.height");
    pub const FRAME_BORDER: Logical = Logical::token("frame.border");
    pub const FRAME_GRIP: Logical = Logical::token("frame.grip");
    pub const PANEL_HEIGHT: Logical = Logical::token("panel.height");
    pub const BAR_HEIGHT: Logical = Logical::token("bar.height");
    pub const DOCK_ICON: Logical = Logical::token("dock.icon");
    pub const DOCK_RADIUS: Logical = Logical::token("dock.radius");
    pub const DOCK_PADDING: Logical = Logical::token("dock.padding");
    pub const ROOM_EDGE: Logical = Logical::token("room.edge");
    pub const ROOM_UNTRUSTED_EDGE: Logical = Logical::token("room.untrusted.edge");
    pub const FIELD_PADDING: Logical = Logical::token("field.padding");
    pub const FIELD_GAP: Logical = Logical::token("field.gap");
}

/// The shape and frame choices.
pub mod shape {
    use crate::{CornerShape, FrameButtons, RoomStyle, Wallpaper};
    pub const CORNER: CornerShape = crate::corner_shape();
    pub const FRAME_BUTTONS: FrameButtons = crate::frame_buttons();
    /// `frame.border.focused = accent`: the focused frame's border is the
    /// accent; `none`: it is not drawn differently.
    pub const FRAME_BORDER_FOCUSED_ACCENT: bool =
        crate::parse::is("frame.border.focused", "accent");
    pub const FRAME_SEPARATOR: bool = crate::yes_no("frame.separator");
    pub const ROOM_STYLE: RoomStyle = crate::room_style();
    pub const WALLPAPER: Wallpaper = crate::wallpaper();
    /// `desktop.layout = dock`: the top bar and the dock (ADR-0168).
    pub const DOCK: bool = crate::parse::is("desktop.layout", "dock");
    /// `chooser.style = sheet` (ADR-0167).
    pub const CHOOSER_SHEET: bool = crate::parse::is("chooser.style", "sheet");
    /// `settings.style = cards`.
    pub const SETTINGS_CARDS: bool = crate::parse::is("settings.style", "cards");
    /// `panel.position = top`.
    pub const PANEL_TOP: bool = crate::parse::is("panel.position", "top");
    /// Degrees of hue around `room.untrusted` no trusted room colour enters
    /// (INV-0161-6).
    pub const ROOM_HUE_DISTANCE: u32 = crate::parse::int("room.hue-distance");
}

/// Shadows: blur and offset in logical pixels, alpha of 255.
pub mod shadow {
    use crate::Logical;
    pub const WINDOW_BLUR: Logical = Logical::token("shadow.window.blur");
    pub const WINDOW_OFFSET: Logical = Logical::token("shadow.window.offset");
    pub const WINDOW_ALPHA: u8 = crate::alpha("shadow.window.alpha");
    pub const WINDOW_LAYERS: u32 = crate::parse::int("shadow.window.layers");
    pub const WINDOW_UNFOCUSED_ALPHA: u8 = crate::alpha("shadow.window.unfocused.alpha");
    pub const POPUP_BLUR: Logical = Logical::token("shadow.popup.blur");
    pub const POPUP_OFFSET: Logical = Logical::token("shadow.popup.offset");
    pub const POPUP_ALPHA: u8 = crate::alpha("shadow.popup.alpha");
}

/// The trusted material and translucency (ADR-0161, ADR-0172).
pub mod glow {
    use crate::Logical;
    pub const ALPHA: u8 = crate::alpha("glow.alpha");
    pub const HALO: u8 = crate::alpha("glow.halo");
    pub const BLUR: Logical = Logical::token("glow.blur");
    pub const PANEL_ALPHA: u8 = crate::alpha("panel.alpha");
    pub const PANEL_BLUR: Logical = Logical::token("blur.panel");
}

/// Motion: durations and curves. `gui.motion = no` (ADR-0169) and the
/// screendump tests use none of it.
pub mod motion {
    use crate::{Ease, Millis};
    pub const SHORT: Millis = Millis(crate::parse::int("motion.short"));
    pub const MEDIUM: Millis = Millis(crate::parse::int("motion.medium"));
    pub const LONG: Millis = Millis(crate::parse::int("motion.long"));
    pub const STANDARD: Ease = crate::ease("ease.standard");
    pub const DECELERATE: Ease = crate::ease("ease.decelerate");
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

//! The two colour sets.

use crate::Color;
use crate::parse::{Set, color};

/// Every colour of one set (ADR-0161's vocabulary), generated from the
/// colour names of `tokens.txt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// `wallpaper.a`
    pub wallpaper_a: Color,
    /// `wallpaper.b`
    pub wallpaper_b: Color,
    /// `wallpaper.c`
    pub wallpaper_c: Color,
    /// `wallpaper.d`
    pub wallpaper_d: Color,
    /// `bg.desktop`
    pub bg_desktop: Color,
    /// `bg.window`
    pub bg_window: Color,
    /// `bg.view`
    pub bg_view: Color,
    /// `bg.panel`
    pub bg_panel: Color,
    /// `bg.frame`
    pub bg_frame: Color,
    /// `bg.frame.focused`
    pub bg_frame_focused: Color,
    /// `bg.frame.system`
    pub bg_frame_system: Color,
    /// `bg.control`
    pub bg_control: Color,
    /// `bg.control.hover`
    pub bg_control_hover: Color,
    /// `bg.control.pressed`
    pub bg_control_pressed: Color,
    /// `bg.selection`
    pub bg_selection: Color,
    /// `bg.overlay`
    pub bg_overlay: Color,
    /// `fg.text`
    pub fg_text: Color,
    /// `fg.text.secondary`
    pub fg_text_secondary: Color,
    /// `fg.text.disabled`
    pub fg_text_disabled: Color,
    /// `fg.text.on-accent`
    pub fg_text_on_accent: Color,
    /// `fg.text.on-selection`
    pub fg_text_on_selection: Color,
    /// `accent`
    pub accent: Color,
    /// `border`
    pub border: Color,
    /// `border.strong`
    pub border_strong: Color,
    /// `shadow`
    pub shadow: Color,
    /// `success`
    pub success: Color,
    /// `warning`
    pub warning: Color,
    /// `error`
    pub error: Color,
    /// `button.close`
    pub button_close: Color,
    /// `button.minimise`
    pub button_minimise: Color,
    /// `room.1`
    pub room_1: Color,
    /// `room.2`
    pub room_2: Color,
    /// `room.3`
    pub room_3: Color,
    /// `room.4`
    pub room_4: Color,
    /// `room.untrusted`
    pub room_untrusted: Color,
    /// `glow`
    pub glow: Color,
    /// `term.bg`
    pub term_bg: Color,
    /// `term.fg`
    pub term_fg: Color,
    /// `term.black`
    pub term_black: Color,
    /// `term.red`
    pub term_red: Color,
    /// `term.green`
    pub term_green: Color,
    /// `term.yellow`
    pub term_yellow: Color,
    /// `term.blue`
    pub term_blue: Color,
    /// `term.magenta`
    pub term_magenta: Color,
    /// `term.cyan`
    pub term_cyan: Color,
    /// `term.white`
    pub term_white: Color,
}

impl Palette {
    /// A room's colour (ADR-0172 point 7): slot 1 to 4 for a trusted room,
    /// anything else (0) for the untrusted one.
    pub const fn room(&self, slot: u8) -> Color {
        match slot {
            1 => self.room_1,
            2 => self.room_2,
            3 => self.room_3,
            4 => self.room_4,
            _ => self.room_untrusted,
        }
    }
}

/// The light set.
pub const LIGHT: Palette = Palette {
    wallpaper_a: color(Set::Light, "wallpaper.a"),
    wallpaper_b: color(Set::Light, "wallpaper.b"),
    wallpaper_c: color(Set::Light, "wallpaper.c"),
    wallpaper_d: color(Set::Light, "wallpaper.d"),
    bg_desktop: color(Set::Light, "bg.desktop"),
    bg_window: color(Set::Light, "bg.window"),
    bg_view: color(Set::Light, "bg.view"),
    bg_panel: color(Set::Light, "bg.panel"),
    bg_frame: color(Set::Light, "bg.frame"),
    bg_frame_focused: color(Set::Light, "bg.frame.focused"),
    bg_frame_system: color(Set::Light, "bg.frame.system"),
    bg_control: color(Set::Light, "bg.control"),
    bg_control_hover: color(Set::Light, "bg.control.hover"),
    bg_control_pressed: color(Set::Light, "bg.control.pressed"),
    bg_selection: color(Set::Light, "bg.selection"),
    bg_overlay: color(Set::Light, "bg.overlay"),
    fg_text: color(Set::Light, "fg.text"),
    fg_text_secondary: color(Set::Light, "fg.text.secondary"),
    fg_text_disabled: color(Set::Light, "fg.text.disabled"),
    fg_text_on_accent: color(Set::Light, "fg.text.on-accent"),
    fg_text_on_selection: color(Set::Light, "fg.text.on-selection"),
    accent: color(Set::Light, "accent"),
    border: color(Set::Light, "border"),
    border_strong: color(Set::Light, "border.strong"),
    shadow: color(Set::Light, "shadow"),
    success: color(Set::Light, "success"),
    warning: color(Set::Light, "warning"),
    error: color(Set::Light, "error"),
    button_close: color(Set::Light, "button.close"),
    button_minimise: color(Set::Light, "button.minimise"),
    room_1: color(Set::Light, "room.1"),
    room_2: color(Set::Light, "room.2"),
    room_3: color(Set::Light, "room.3"),
    room_4: color(Set::Light, "room.4"),
    room_untrusted: color(Set::Light, "room.untrusted"),
    glow: color(Set::Light, "glow"),
    term_bg: color(Set::Light, "term.bg"),
    term_fg: color(Set::Light, "term.fg"),
    term_black: color(Set::Light, "term.black"),
    term_red: color(Set::Light, "term.red"),
    term_green: color(Set::Light, "term.green"),
    term_yellow: color(Set::Light, "term.yellow"),
    term_blue: color(Set::Light, "term.blue"),
    term_magenta: color(Set::Light, "term.magenta"),
    term_cyan: color(Set::Light, "term.cyan"),
    term_white: color(Set::Light, "term.white"),
};

/// The dark set.
pub const DARK: Palette = Palette {
    wallpaper_a: color(Set::Dark, "wallpaper.a"),
    wallpaper_b: color(Set::Dark, "wallpaper.b"),
    wallpaper_c: color(Set::Dark, "wallpaper.c"),
    wallpaper_d: color(Set::Dark, "wallpaper.d"),
    bg_desktop: color(Set::Dark, "bg.desktop"),
    bg_window: color(Set::Dark, "bg.window"),
    bg_view: color(Set::Dark, "bg.view"),
    bg_panel: color(Set::Dark, "bg.panel"),
    bg_frame: color(Set::Dark, "bg.frame"),
    bg_frame_focused: color(Set::Dark, "bg.frame.focused"),
    bg_frame_system: color(Set::Dark, "bg.frame.system"),
    bg_control: color(Set::Dark, "bg.control"),
    bg_control_hover: color(Set::Dark, "bg.control.hover"),
    bg_control_pressed: color(Set::Dark, "bg.control.pressed"),
    bg_selection: color(Set::Dark, "bg.selection"),
    bg_overlay: color(Set::Dark, "bg.overlay"),
    fg_text: color(Set::Dark, "fg.text"),
    fg_text_secondary: color(Set::Dark, "fg.text.secondary"),
    fg_text_disabled: color(Set::Dark, "fg.text.disabled"),
    fg_text_on_accent: color(Set::Dark, "fg.text.on-accent"),
    fg_text_on_selection: color(Set::Dark, "fg.text.on-selection"),
    accent: color(Set::Dark, "accent"),
    border: color(Set::Dark, "border"),
    border_strong: color(Set::Dark, "border.strong"),
    shadow: color(Set::Dark, "shadow"),
    success: color(Set::Dark, "success"),
    warning: color(Set::Dark, "warning"),
    error: color(Set::Dark, "error"),
    button_close: color(Set::Dark, "button.close"),
    button_minimise: color(Set::Dark, "button.minimise"),
    room_1: color(Set::Dark, "room.1"),
    room_2: color(Set::Dark, "room.2"),
    room_3: color(Set::Dark, "room.3"),
    room_4: color(Set::Dark, "room.4"),
    room_untrusted: color(Set::Dark, "room.untrusted"),
    glow: color(Set::Dark, "glow"),
    term_bg: color(Set::Dark, "term.bg"),
    term_fg: color(Set::Dark, "term.fg"),
    term_black: color(Set::Dark, "term.black"),
    term_red: color(Set::Dark, "term.red"),
    term_green: color(Set::Dark, "term.green"),
    term_yellow: color(Set::Dark, "term.yellow"),
    term_blue: color(Set::Dark, "term.blue"),
    term_magenta: color(Set::Dark, "term.magenta"),
    term_cyan: color(Set::Dark, "term.cyan"),
    term_white: color(Set::Dark, "term.white"),
};

/// The colour names the palettes read, in the file's order.
pub const COLOR_NAMES: &[&str] = &[
    "wallpaper.a",
    "wallpaper.b",
    "wallpaper.c",
    "wallpaper.d",
    "bg.desktop",
    "bg.window",
    "bg.view",
    "bg.panel",
    "bg.frame",
    "bg.frame.focused",
    "bg.frame.system",
    "bg.control",
    "bg.control.hover",
    "bg.control.pressed",
    "bg.selection",
    "bg.overlay",
    "fg.text",
    "fg.text.secondary",
    "fg.text.disabled",
    "fg.text.on-accent",
    "fg.text.on-selection",
    "accent",
    "border",
    "border.strong",
    "shadow",
    "success",
    "warning",
    "error",
    "button.close",
    "button.minimise",
    "room.1",
    "room.2",
    "room.3",
    "room.4",
    "room.untrusted",
    "glow",
    "term.bg",
    "term.fg",
    "term.black",
    "term.red",
    "term.green",
    "term.yellow",
    "term.blue",
    "term.magenta",
    "term.cyan",
    "term.white",
];

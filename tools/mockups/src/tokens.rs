//! The token file of a direction: `name = value` lines, `#` comments, and
//! `light:` or `dark:` prefixes for colours (the shape ADR-0161 proposes
//! for `zoraya-tokens`). Values stay strings here and are read by typed
//! accessors.

use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct Tokens {
    pub plain: BTreeMap<String, String>,
    pub light: BTreeMap<String, String>,
    pub dark: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Tokens {
    pub fn parse(text: &str) -> Result<Tokens, String> {
        let mut t = Tokens::default();
        for (n, line) in text.lines().enumerate() {
            // A comment is `#` at the start or after a space and followed by a
            // space (so that `#rrggbb` values survive).
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("line {}: no '='", n + 1))?;
            let (key, value) = (key.trim(), value.trim());
            if key.is_empty() || value.is_empty() {
                return Err(format!("line {}: empty name or value", n + 1));
            }
            let map = if let Some(k) = key.strip_prefix("light:") {
                t.light.insert(k.trim().to_string(), value.to_string());
                continue;
            } else if let Some(k) = key.strip_prefix("dark:") {
                t.dark.insert(k.trim().to_string(), value.to_string());
                continue;
            } else {
                &mut t.plain
            };
            if map.insert(key.to_string(), value.to_string()).is_some() {
                return Err(format!("line {}: {key} defined twice", n + 1));
            }
        }
        let missing: Vec<&String> = t
            .light
            .keys()
            .filter(|k| !t.dark.contains_key(*k))
            .collect();
        let extra: Vec<&String> = t
            .dark
            .keys()
            .filter(|k| !t.light.contains_key(*k))
            .collect();
        if !missing.is_empty() || !extra.is_empty() {
            return Err(format!(
                "light and dark differ: only light {missing:?}, only dark {extra:?}"
            ));
        }
        Ok(t)
    }

    pub fn str(&self, name: &str) -> &str {
        self.plain
            .get(name)
            .unwrap_or_else(|| panic!("token {name} missing"))
    }

    pub fn num(&self, name: &str) -> f32 {
        let s = self.str(name);
        s.parse::<f32>()
            .unwrap_or_else(|_| panic!("token {name} = {s} is not a number"))
    }

    pub fn color(&self, name: &str, appearance: Appearance) -> u32 {
        let map = match appearance {
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
        };
        let s = map
            .get(name)
            .unwrap_or_else(|| panic!("colour token {name} missing"));
        parse_color(s).unwrap_or_else(|| panic!("token {name} = {s} is not #rrggbb"))
    }

    pub fn has(&self, name: &str) -> bool {
        self.plain.contains_key(name)
    }
}

fn strip_comment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    let b = line.as_bytes();
    for i in 1..b.len() {
        if b[i] == b'#' && b[i - 1] == b' ' && (i + 1 == b.len() || b[i + 1] == b' ') {
            return &line[..i];
        }
    }
    line
}

pub fn parse_color(s: &str) -> Option<u32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok()
}

/// The tokens every direction must define (plain names; colours are
/// listed once and must exist in both sets).
pub const REQUIRED_PLAIN: &[&str] = &[
    "field.padding",
    "field.gap",
    "room.hue-distance",
    "room.untrusted.edge",
    "name",
    "appearance",
    "font.ui",
    "font.ui.medium",
    "font.mono",
    "font.mono.bold",
    "text.caption",
    "text.body",
    "text.heading",
    "text.title",
    "text.large",
    "text.mono",
    "space.unit",
    "padding.control",
    "padding.window",
    "gap.controls",
    "control.height",
    "radius.control",
    "radius.window",
    "radius.popup",
    "frame.height",
    "frame.border",
    "frame.grip",
    "frame.buttons",
    "shadow.window.blur",
    "shadow.window.offset",
    "shadow.window.alpha",
    "shadow.popup.blur",
    "shadow.popup.offset",
    "shadow.popup.alpha",
    "panel.height",
    "panel.position",
    "motion.short",
    "motion.medium",
    "motion.long",
    "ease.standard",
    "ease.decelerate",
    "frame.border.focused",
    "frame.separator",
    "radius.shape",
    "shadow.window.layers",
    "shadow.window.unfocused.alpha",
    "padding.terminal",
    "desktop.layout",
    "bar.height",
    "dock.icon",
    "dock.radius",
    "dock.padding",
    "panel.alpha",
    "blur.panel",
    "wallpaper",
    "chooser.style",
    "settings.style",
    "concept",
    "screens",
    "room.style",
    "room.edge",
    "glow.alpha",
    "glow.halo",
    "glow.blur",
];
pub const REQUIRED_COLORS: &[&str] = &[
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
    "wallpaper.a",
    "wallpaper.b",
    "wallpaper.c",
    "wallpaper.d",
    "room.1",
    "room.2",
    "room.3",
    "room.4",
    "room.untrusted",
    "glow",
];

/// WCAG relative luminance of an sRGB colour.
pub fn luminance(c: u32) -> f64 {
    let ch = |v: u32| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch((c >> 16) & 0xFF) + 0.7152 * ch((c >> 8) & 0xFF) + 0.0722 * ch(c & 0xFF)
}

/// WCAG contrast ratio of two colours.
pub fn contrast(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a) + 0.05, luminance(b) + 0.05);
    if la > lb { la / lb } else { lb / la }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HSL saturation of an `#rrggbb` colour.
    fn saturation(c: u32) -> f32 {
        let r = ((c >> 16) & 0xFF) as f32 / 255.0;
        let g = ((c >> 8) & 0xFF) as f32 / 255.0;
        let b = (c & 0xFF) as f32 / 255.0;
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            0.0
        } else {
            d / (1.0 - (2.0 * l - 1.0).abs())
        }
    }

    /// HSL hue of an `#rrggbb` colour, in degrees.
    fn hue(c: u32) -> f32 {
        let r = ((c >> 16) & 0xFF) as f32 / 255.0;
        let g = ((c >> 8) & 0xFF) as f32 / 255.0;
        let b = (c & 0xFF) as f32 / 255.0;
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        if d == 0.0 {
            return 0.0;
        }
        let h = if max == r {
            ((g - b) / d) % 6.0
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h * 60.0 + 360.0) % 360.0
    }

    /// Every direction file under docs/research/desktop/mockups parses,
    /// defines the required tokens in both sets, and keeps body text at a
    /// contrast of at least 4.5 to 1 on its backgrounds (ADR-0161's
    /// INV-0161-2 and INV-0161-3, applied to the mockups).
    #[test]
    fn direction_files_are_complete_and_readable() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/research/desktop/mockups");
        let mut seen = 0;
        for entry in std::fs::read_dir(&root).unwrap().flatten() {
            let path = entry.path().join("tokens.txt");
            if !path.is_file() {
                continue;
            }
            seen += 1;
            let text = std::fs::read_to_string(&path).unwrap();
            let t = Tokens::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for name in REQUIRED_PLAIN {
                assert!(t.has(name), "{}: missing {name}", path.display());
            }
            for name in REQUIRED_COLORS {
                for app in [Appearance::Light, Appearance::Dark] {
                    let _ = t.color(name, app);
                }
            }
            // INV-0161-6: the untrusted hue is reserved
            if t.str("concept") == "clear" {
                let min = t.num("room.hue-distance");
                for app in [Appearance::Light, Appearance::Dark] {
                    let u = hue(t.color("room.untrusted", app));
                    for name in ["room.1", "room.2", "room.3", "room.4"] {
                        let h = hue(t.color(name, app));
                        let d = (h - u).abs().min(360.0 - (h - u).abs());
                        assert!(
                            d >= min,
                            "{}: {name} ({app:?}) hue {h:.0} is {d:.0} degrees from room.untrusted {u:.0}, under {min}",
                            path.display()
                        );
                    }
                }
            }
            // INV-0161-5: in a clear direction, colour means a room
            if t.str("concept") == "clear" {
                for name in REQUIRED_COLORS {
                    let exempt = ["room.", "button.", "term."]
                        .iter()
                        .any(|p| name.starts_with(p))
                        || ["success", "warning", "error"].contains(name);
                    if exempt {
                        continue;
                    }
                    for app in [Appearance::Light, Appearance::Dark] {
                        let s = saturation(t.color(name, app));
                        assert!(
                            s <= 0.4,
                            "{}: {name} ({app:?}) saturation {s:.2} above 0.4",
                            path.display()
                        );
                    }
                }
            }
            for app in [Appearance::Light, Appearance::Dark] {
                for (fg, bg) in [
                    ("fg.text", "bg.window"),
                    ("fg.text", "bg.view"),
                    ("fg.text", "bg.control"),
                    ("fg.text.on-accent", "accent"),
                    ("fg.text.on-selection", "bg.selection"),
                    ("term.fg", "term.bg"),
                ] {
                    let r = contrast(t.color(fg, app), t.color(bg, app));
                    assert!(
                        r >= 4.5,
                        "{}: {fg} on {bg} ({app:?}) contrast {r:.2}",
                        path.display()
                    );
                }
            }
        }
        assert_eq!(seen, 1, "one direction file: clear-field");
    }

    #[test]
    fn parses_prefixes_and_rejects_mismatch() {
        let t = Tokens::parse("a = 1 # c\nlight: x = #ff0000\ndark: x = #00ff00\n").unwrap();
        assert_eq!(t.num("a"), 1.0);
        assert_eq!(t.color("x", Appearance::Light), 0xFF0000);
        assert_eq!(t.color("x", Appearance::Dark), 0x00FF00);
        assert!(Tokens::parse("light: x = #ff0000\n").is_err());
        assert!(Tokens::parse("a = 1\na = 2\n").is_err());
        assert!(Tokens::parse("a\n").is_err());
    }
}

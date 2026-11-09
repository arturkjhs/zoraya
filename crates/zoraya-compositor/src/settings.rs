//! The desktop's settings (ADR-0169): the `gui.*` keys the compositor
//! declares in its description, read from `/config/settings` when it
//! starts and from the panel's `ApplySettings` after a change in the
//! settings window. Every value is checked against its declaration: the
//! file is the person's, the panel is trusted but checked again
//! (INV-0169-2).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_describe::{Setting, list_items, list_text};

/// The compositor's description, whose `setting` lines declare the keys.
pub const DESCRIPTION: &str = include_str!("../../../user/describe/system/compd.cmd");

/// What the settings ask of the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiSettings {
    pub dark: bool,
    pub motion: bool,
    /// 0 for `auto` (ADR-0164), else 1 or 2.
    pub scale: u32,
    /// `''` is the theme's own; nothing else can be drawn yet.
    pub wallpaper: String,
    /// Indices into `zoraya_keymap::LAYOUTS`, in switching order.
    pub layouts: Vec<usize>,
    /// The active one, an index into `zoraya_keymap::LAYOUTS` and one of
    /// `layouts`.
    pub layout: usize,
}

/// The declared keys.
pub fn declared() -> Vec<Setting> {
    zoraya_describe::parse(DESCRIPTION.as_bytes())
        .map(|d| d.settings)
        .unwrap_or_default()
}

impl Default for GuiSettings {
    /// The declarations' defaults.
    fn default() -> GuiSettings {
        read(b"").0
    }
}

/// The settings in `bytes` (a `/config/settings`), and the problems to
/// report, each as the console line ADR-0157 rule 5 asks for. A bad line
/// keeps its key's default; a layout that is not one of the list is
/// reported and the list's first is used.
pub fn read(bytes: &[u8]) -> (GuiSettings, Vec<String>) {
    let d = declared();
    let v = zoraya_settings::read(bytes, &d);
    let mut problems: Vec<String> = v
        .problems
        .iter()
        .map(|p| format!("compd: /config/settings line {}: {}", p.line, p.problem))
        .collect();
    let get = |name: &str| v.get(name).unwrap_or("");
    let s = from_values(get, &mut problems);
    (s, problems)
}

/// Values already checked against their declarations, as settings; what
/// the declarations cannot say (the active layout in the list, a picture)
/// is reported here.
fn from_values<'a>(get: impl Fn(&str) -> &'a str, problems: &mut Vec<String>) -> GuiSettings {
    let layouts: Vec<usize> = list_items(get("gui.keyboard-layouts"))
        .unwrap_or_default()
        .iter()
        .filter_map(|n| zoraya_keymap::by_name(n))
        .collect();
    let layouts = if layouts.is_empty() {
        alloc::vec![0]
    } else {
        layouts
    };
    let wanted = zoraya_keymap::by_name(get("gui.keyboard-layout"));
    let layout = match wanted {
        Some(l) if layouts.contains(&l) => l,
        _ => {
            problems.push(format!(
                "compd: gui.keyboard-layout {} is not one of gui.keyboard-layouts; {} is used",
                get("gui.keyboard-layout"),
                zoraya_keymap::LAYOUTS[layouts[0]].name
            ));
            layouts[0]
        }
    };
    let wallpaper = String::from(get("gui.wallpaper"));
    if !wallpaper.is_empty() {
        problems.push(String::from(
            "compd: gui.wallpaper: no picture can be drawn yet (no image decoder); the theme's own is used",
        ));
    }
    GuiSettings {
        dark: get("gui.theme") == "dark",
        motion: get("gui.motion") != "no",
        scale: get("gui.scale").parse().unwrap_or(0),
        wallpaper,
        layouts,
        layout,
    }
}

/// `ApplySettings`'s values, checked against the declarations as the file's
/// are; `None` when any is not of its type or the active layout is not in
/// the list (the panel then hears `INVALID`, and nothing changes).
pub fn checked(
    theme: &str,
    motion: &str,
    scale: &str,
    wallpaper: &str,
    layouts: &str,
    layout: &str,
) -> Option<GuiSettings> {
    let d = declared();
    let pairs = [
        ("gui.theme", theme),
        ("gui.motion", motion),
        ("gui.scale", scale),
        ("gui.wallpaper", wallpaper),
        ("gui.keyboard-layouts", layouts),
        ("gui.keyboard-layout", layout),
    ];
    for (name, value) in pairs {
        let s = d.iter().find(|s| s.name == name)?;
        s.ty.check(value).ok()?;
    }
    let mut problems = Vec::new();
    let get = |name: &str| pairs.iter().find(|p| p.0 == name).map_or("", |p| p.1);
    let s = from_values(get, &mut problems);
    problems.is_empty().then_some(s)
}

/// The words `ApplySettings` carries for `s`, as the panel sends them.
pub fn words(s: &GuiSettings) -> [String; 6] {
    let names: Vec<String> = s
        .layouts
        .iter()
        .map(|i| String::from(zoraya_keymap::LAYOUTS[*i].name))
        .collect();
    [
        String::from(if s.dark { "dark" } else { "light" }),
        String::from(if s.motion { "yes" } else { "no" }),
        match s.scale {
            1 => String::from("1"),
            2 => String::from("2"),
            _ => String::from("auto"),
        },
        s.wallpaper.clone(),
        list_text(&names),
        String::from(zoraya_keymap::LAYOUTS[s.layout].name),
    ]
}

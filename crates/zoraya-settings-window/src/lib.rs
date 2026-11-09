//! The settings window's logic (ADR-0168 point 6, ADR-0169): one page of
//! the desktop's settings, appearance (theme, motion, scale) and keyboard
//! (the enabled layouts and the active one), each control bound to its
//! declaration in the compositor's description; and each change as the
//! new `/config/settings`, every other line kept (ADR-0157's writer). The
//! program writes it atomically and tells the panel.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_compositor::settings::{self, GuiSettings, words};
use zoraya_ui::layout::{Column, Row};
use zoraya_ui::widgets::{CheckBox, Label, RadioGroup, Switch};
use zoraya_ui::{Value, Widget};

pub const THEME: u32 = 1;
pub const MOTION: u32 = 2;
pub const SCALE: u32 = 3;
/// The active layout, among the enabled ones.
pub const ACTIVE: u32 = 4;
/// One check box per layout: `LAYOUT + i` for `zoraya_keymap::LAYOUTS[i]`.
pub const LAYOUT: u32 = 10;

/// A layout's name as the page shows it.
fn layout_label(i: usize) -> &'static str {
    match zoraya_keymap::LAYOUTS[i].name {
        "us" => "English (US)",
        "ua" => "Ukrainian",
        "ru" => "Russian",
        "cz" => "Czech",
        other => other,
    }
}

fn heading(text: &str) -> Box<dyn Widget> {
    Box::new(Label::new(text))
}

fn note(text: &str) -> Box<dyn Widget> {
    let mut l = Label::new(text);
    l.secondary = true;
    Box::new(l)
}

fn row(label: &str, control: Box<dyn Widget>) -> Box<dyn Widget> {
    Box::new(Row::new(vec![Box::new(Label::new(label)), control]))
}

/// The page for `s`, with `status` under it (what the last change did).
pub fn page(s: &GuiSettings, status: &str) -> Box<dyn Widget> {
    let scale = match s.scale {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    let mut column: Vec<Box<dyn Widget>> = vec![
        heading("Appearance"),
        row(
            "Theme",
            Box::new(RadioGroup::new(
                THEME,
                &["Light", "Dark"],
                usize::from(s.dark),
            )),
        ),
        row("Motion", Box::new(Switch::new(MOTION, s.motion))),
        row(
            "Scale",
            Box::new(RadioGroup::new(SCALE, &["Automatic", "1", "2"], scale)),
        ),
        note("Automatic is 2 on a display 1920 pixels wide or wider."),
        heading("Keyboard"),
    ];
    let boxes: Vec<Box<dyn Widget>> = (0..zoraya_keymap::LAYOUTS.len())
        .map(|i| {
            Box::new(CheckBox::new(
                LAYOUT + i as u32,
                layout_label(i),
                s.layouts.contains(&i),
            )) as Box<dyn Widget>
        })
        .collect();
    column.push(Box::new(Row::new(boxes)));
    let enabled: Vec<&str> = s.layouts.iter().map(|i| layout_label(*i)).collect();
    let active = s.layouts.iter().position(|l| *l == s.layout).unwrap_or(0);
    column.push(row(
        "In use (System+Space switches)",
        Box::new(RadioGroup::new(ACTIVE, &enabled, active)),
    ));
    column.push(note(status));
    Box::new(Column::new(column))
}

/// `s` after control `id` took `value`; `Err` with the reason when the
/// change cannot be made (the last layout cannot be turned off).
pub fn changed(s: &GuiSettings, id: u32, value: Value<'_>) -> Result<GuiSettings, &'static str> {
    let mut n = s.clone();
    match (id, value) {
        (THEME, Value::Index(Some(i))) => n.dark = i == 1,
        (MOTION, Value::On(on)) => n.motion = on,
        (SCALE, Value::Index(Some(i))) => {
            n.scale = [0, 1, 2].get(i).copied().ok_or("no such scale")?
        }
        (ACTIVE, Value::Index(Some(i))) => n.layout = *s.layouts.get(i).ok_or("no such layout")?,
        (id, Value::On(on))
            if (LAYOUT..LAYOUT + zoraya_keymap::LAYOUTS.len() as u32).contains(&id) =>
        {
            let l = (id - LAYOUT) as usize;
            if on && !n.layouts.contains(&l) {
                n.layouts.push(l);
            } else if !on {
                if n.layouts == [l] {
                    return Err("one layout at least stays on");
                }
                n.layouts.retain(|x| *x != l);
                if n.layout == l {
                    n.layout = n.layouts[0];
                }
            }
        }
        _ => return Err("not a control of this page"),
    }
    Ok(n)
}

/// The file `bytes` with `s`'s keys written (only the ones that differ
/// from what the file says), every other line kept.
pub fn written(bytes: &[u8], s: &GuiSettings) -> Result<Vec<u8>, String> {
    let declared = settings::declared();
    let (now, _) = settings::read(bytes);
    let old = words(&now);
    let new = words(s);
    let names = [
        "gui.theme",
        "gui.motion",
        "gui.scale",
        "gui.wallpaper",
        "gui.keyboard-layouts",
        "gui.keyboard-layout",
    ];
    let mut out = Vec::from(bytes);
    for ((name, a), b) in names.iter().zip(&old).zip(&new) {
        if a != b {
            out = zoraya_settings::set(&out, &declared, name, b).map_err(|r| match r {
                zoraya_settings::Refusal::Value(p) => p,
                zoraya_settings::Refusal::Unknown(_) => String::from("not declared"),
            })?;
        }
    }
    Ok(out)
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

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
use zoraya_settings::rooms::{LIMIT_WORDS, Limits, format_size};
use zoraya_settings::usage::RoomRow;
use zoraya_ui::layout::{Column, Row};
use zoraya_ui::widgets::{Button, CheckBox, Label, RadioGroup, ScrollView, Switch, TextField};
use zoraya_ui::{Value, Widget};

pub const THEME: u32 = 1;
pub const MOTION: u32 = 2;
pub const SCALE: u32 = 3;
/// The active layout, among the enabled ones.
pub const ACTIVE: u32 = 4;
/// One check box per layout: `LAYOUT + i` for `zoraya_keymap::LAYOUTS[i]`.
pub const LAYOUT: u32 = 10;

/// The Rooms section's button (ADR-0267 point 4).
pub const APPLY_ROOMS: u32 = 500;
/// The field of room `r`'s limit word `w` (`LIMIT_WORDS[w]`): `ROOM_FIELD + r * 8 + w`.
pub const ROOM_FIELD: u32 = 1000;
/// Rooms the section lists (the room table's most and `home` is not one).
pub const MAX_ROOM_ROWS: usize = 32;

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
    page_with_rooms(s, &[], &[], status)
}

/// The text of room `row`'s word `w` as its field shows it: the line's
/// value, empty for a word the line does not carry.
pub fn field_text(l: &Limits, w: usize) -> String {
    match w {
        0 => l.mem.map(format_size),
        1 => l.cpu.map(|v| alloc::format!("{v}%")),
        2 => l.procs.map(|v| alloc::format!("{v}")),
        3 => l.disk.map(format_size),
        4 => l.conns.map(|v| alloc::format!("{v}")),
        5 => l.tmp.map(format_size),
        _ => None,
    }
    .unwrap_or_default()
}

/// Whether word `w` is a field of this room (`--tmp` is for untrusted
/// rooms only, ADR-0266 point 1).
pub fn has_field(row: &RoomRow, w: usize) -> bool {
    w < LIMIT_WORDS.len() && (w != 5 || row.untrusted)
}

/// The id of the field of room `r`, word `w`, and its inverse.
pub fn field_id(r: usize, w: usize) -> u32 {
    ROOM_FIELD + (r * 8 + w) as u32
}

pub fn field_of(id: u32) -> Option<(usize, usize)> {
    let n = id.checked_sub(ROOM_FIELD)? as usize;
    let (r, w) = (n / 8, n % 8);
    (r < MAX_ROOM_ROWS && w < LIMIT_WORDS.len()).then_some((r, w))
}

/// What the fields say, if the person has typed in them: `drafts` holds
/// `(field id, text)`; a field with no draft shows the line's value.
fn text_of(rows: &[RoomRow], drafts: &[(u32, String)], r: usize, w: usize) -> String {
    let id = field_id(r, w);
    drafts
        .iter()
        .find(|(d, _)| *d == id)
        .map(|(_, t)| t.clone())
        .unwrap_or_else(|| field_text(&rows[r].limits, w))
}

/// The changes the fields ask for: `(room, word, value)` for each field
/// whose text differs from the line's; an empty text is `none`.
pub fn room_changes(
    rows: &[RoomRow],
    drafts: &[(u32, String)],
) -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    for (r, row) in rows.iter().enumerate().take(MAX_ROOM_ROWS) {
        for w in 0..LIMIT_WORDS.len() {
            if !has_field(row, w) {
                continue;
            }
            let now = field_text(&row.limits, w);
            let want = text_of(rows, drafts, r, w);
            let want = want.trim();
            if want != now {
                let value = if want.is_empty() { "none" } else { want };
                out.push((row.name.clone(), LIMIT_WORDS[w], String::from(value)));
            }
        }
    }
    out
}

/// The `room set` line under a room's fields while its words are being
/// edited (ADR-0267 point 4, ADR-0172's rule that everything has its text).
pub fn preview(rows: &[RoomRow], drafts: &[(u32, String)], r: usize) -> Option<String> {
    let row = rows.get(r)?;
    let changes: Vec<String> = room_changes(rows, drafts)
        .into_iter()
        .filter(|(n, _, _)| *n == row.name)
        .map(|(_, w, v)| alloc::format!("{w} {v}"))
        .collect();
    (!changes.is_empty()).then(|| alloc::format!("room set {} {}", row.name, changes.join(" ")))
}

/// The Rooms section: for each room its limit words as fields with the use
/// beside them, the `room set` line of what is being edited, and Apply.
fn rooms_section(rows: &[RoomRow], drafts: &[(u32, String)]) -> Vec<Box<dyn Widget>> {
    let mut out: Vec<Box<dyn Widget>> = vec![heading("Rooms")];
    for (r, row) in rows.iter().enumerate().take(MAX_ROOM_ROWS) {
        let name = if row.untrusted {
            alloc::format!("{}  untrusted", row.name)
        } else {
            row.name.clone()
        };
        out.push(Box::new(Label::new(&name)));
        // The use from the same reading as `top` and the hover card
        // (INV-0267-1); a room that does not run has none.
        let use_line = match &row.reading {
            Some(rd) if rd.running => rd
                .card_lines()
                .into_iter()
                .filter(|l| !l.starts_with("limits") && !l.starts_with("raise"))
                .collect::<Vec<_>>()
                .join("  "),
            _ => String::from("not running"),
        };
        out.push(note(&use_line));
        for w in 0..LIMIT_WORDS.len() {
            if !has_field(row, w) {
                continue;
            }
            let text = text_of(rows, drafts, r, w);
            out.push(row_of(
                LIMIT_WORDS[w],
                Box::new(TextField::new(field_id(r, w), &text)),
            ));
        }
        if let Some(p) = preview(rows, drafts, r) {
            out.push(note(&p));
        }
    }
    out.push(Box::new(Button::new(APPLY_ROOMS, "Apply")));
    out
}

fn row_of(label: &str, control: Box<dyn Widget>) -> Box<dyn Widget> {
    row(label, control)
}

/// The page with the Rooms section under the keyboard's, when the panel
/// knows the rooms; scrolled, since the rooms make it long.
pub fn page_with_rooms(
    s: &GuiSettings,
    rows: &[RoomRow],
    drafts: &[(u32, String)],
    status: &str,
) -> Box<dyn Widget> {
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
        note("Automatic is 2 from 1920 pixels wide, or 1280 on a Retina display."),
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
    if rows.is_empty() {
        return Box::new(Column::new(column));
    }
    column.extend(rooms_section(rows, drafts));
    Box::new(ScrollView::new(Box::new(Column::new(column))))
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

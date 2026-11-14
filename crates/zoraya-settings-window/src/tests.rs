use alloc::vec;

use super::*;
use zoraya_ui::Value;

fn s(file: &[u8]) -> GuiSettings {
    settings::read(file).0
}

#[test]
fn each_control_changes_its_setting_and_the_file_keeps_the_rest() {
    let file = b"# mine\nshell.prompt = 'x> '\ngui.keyboard-layouts = us ua\n";
    let now = s(file);
    let dark = changed(&now, THEME, Value::Index(Some(1))).unwrap();
    assert!(dark.dark);
    let f = written(file, &dark).unwrap();
    assert_eq!(
        f,
        b"# mine\nshell.prompt = 'x> '\ngui.keyboard-layouts = us ua\ngui.theme = dark\n"
    );
    assert_eq!(s(&f), dark);
    // Nothing changed: the file is the same, byte for byte.
    assert_eq!(written(&f, &dark).unwrap(), f);
    let still = changed(&dark, MOTION, Value::On(false)).unwrap();
    assert!(!still.motion);
    let two = changed(&still, SCALE, Value::Index(Some(2))).unwrap();
    assert_eq!(two.scale, 2);
    assert!(changed(&still, SCALE, Value::Index(Some(3))).is_err());
    let f = written(&f, &two).unwrap();
    assert_eq!(s(&f), two);
}

#[test]
fn layouts_turn_on_and_off_and_the_active_one_follows() {
    let ua = zoraya_keymap::by_name("ua").unwrap();
    let cz = zoraya_keymap::by_name("cz").unwrap();
    let now = s(b"gui.keyboard-layouts = us ua\ngui.keyboard-layout = ua\n");
    let with_cz = changed(&now, LAYOUT + cz as u32, Value::On(true)).unwrap();
    assert_eq!(with_cz.layouts, vec![0, ua, cz]);
    // The active one, by its place among the enabled.
    let active = changed(&with_cz, ACTIVE, Value::Index(Some(2))).unwrap();
    assert_eq!(active.layout, cz);
    assert!(changed(&with_cz, ACTIVE, Value::Index(Some(3))).is_err());
    // Turning the active one off makes the first active.
    let off = changed(&active, LAYOUT + cz as u32, Value::On(false)).unwrap();
    assert_eq!((off.layouts.clone(), off.layout), (vec![0, ua], 0));
    let f = written(b"", &off).unwrap();
    assert_eq!(s(&f), off);
    // The last one stays.
    let one = s(b"");
    assert!(changed(&one, LAYOUT, Value::On(false)).is_err());
    assert!(changed(&one, 99, Value::On(true)).is_err());
}

#[test]
fn the_page_shows_the_settings() {
    let now = s(b"gui.theme = dark\ngui.keyboard-layouts = us cz\n");
    let l = |v| zoraya_tokens::Scale::ONE.logical(v);
    let mut w = zoraya_ui::Window::new(page(&now, "Saved."), (l(480), l(400)));
    assert_eq!(w.value(THEME), Value::Index(Some(1)));
    assert_eq!(w.value(MOTION), Value::On(true));
    let cz = zoraya_keymap::by_name("cz").unwrap();
    assert_eq!(w.value(LAYOUT + cz as u32), Value::On(true));
    assert_eq!(w.value(LAYOUT + 1), Value::On(false));
    assert_eq!(w.value(ACTIVE), Value::Index(Some(0)));
}

#[test]
fn each_layout_has_its_name_on_the_page() {
    let names: alloc::vec::Vec<&str> = (0..zoraya_keymap::LAYOUTS.len())
        .map(layout_label)
        .collect();
    assert_eq!(names, ["English (US)", "Ukrainian", "Russian", "Czech"]);
}

#[test]
fn the_page_shows_a_fixed_scale() {
    let l = |v| zoraya_tokens::Scale::ONE.logical(v);
    for (file, want) in [
        (&b"gui.scale = 1\n"[..], 1),
        (b"gui.scale = 2\n", 2),
        (b"", 0),
    ] {
        let mut w = zoraya_ui::Window::new(page(&s(file), ""), (l(480), l(400)));
        assert_eq!(w.value(SCALE), Value::Index(Some(want)));
    }
}

#[test]
fn a_control_past_the_last_layout_is_not_a_control() {
    let now = s(b"");
    let past = LAYOUT + zoraya_keymap::LAYOUTS.len() as u32;
    assert!(changed(&now, past, Value::On(true)).is_err());
}

#[test]
fn a_layout_turned_to_the_state_it_has_changes_nothing() {
    let cz = zoraya_keymap::by_name("cz").unwrap();
    let now = s(b"");
    let on = changed(&now, LAYOUT, Value::On(true)).unwrap();
    assert_eq!(on.layouts, vec![0], "us is on already");
    let off = changed(&now, LAYOUT + cz as u32, Value::On(false)).unwrap();
    assert_eq!(off.layouts, vec![0], "cz is off already");
}

fn rows() -> alloc::vec::Vec<zoraya_settings::usage::RoomRow> {
    use zoraya_settings::rooms::Limits;
    use zoraya_settings::usage::{Reading, RoomRow};
    let agent = Limits {
        mem: Some(64 << 20),
        cpu: Some(25),
        procs: Some(64),
        disk: Some(2 << 30),
        conns: Some(16),
        tmp: Some(16 << 20),
    };
    vec![
        RoomRow {
            name: "project".into(),
            untrusted: false,
            limits: Limits {
                mem: Some(512 << 20),
                ..Limits::default()
            },
            reading: None,
        },
        RoomRow {
            name: "agent-1".into(),
            untrusted: true,
            limits: agent,
            reading: Some(Reading {
                name: "agent-1".into(),
                running: true,
                untrusted: true,
                mem_used: 58 << 20,
                mem_limit: Some(64 << 20),
                procs_used: 3,
                procs_limit: Some(64),
                ..Reading::default()
            }),
        },
    ]
}

#[test]
fn the_rooms_section_has_a_field_per_word_and_the_untrusted_have_tmp() {
    let rows = rows();
    let l = |v| zoraya_tokens::Scale::ONE.logical(v);
    let mut w = zoraya_ui::Window::new(page_with_rooms(&s(b""), &rows, &[], ""), (l(600), l(520)));
    // The line's values, as the line writes them; empty for a word it lacks.
    assert_eq!(w.value(field_id(0, 0)), Value::Text("512M"));
    assert_eq!(w.value(field_id(0, 1)), Value::Text(""));
    assert_eq!(w.value(field_id(1, 1)), Value::Text("25%"));
    assert_eq!(w.value(field_id(1, 5)), Value::Text("16M"));
    // `project` is trusted: no --tmp field.
    assert_eq!(w.value(field_id(0, 5)), Value::None);
    assert!(has_field(&rows[1], 5) && !has_field(&rows[0], 5));
    assert_eq!(field_of(field_id(1, 3)), Some((1, 3)));
    assert_eq!(field_of(THEME), None);
    assert_eq!(field_of(ROOM_FIELD + 6), None);
    // Without rooms the page is the old one.
    let mut old = zoraya_ui::Window::new(page(&s(b""), ""), (l(600), l(520)));
    assert_eq!(old.value(field_id(0, 0)), Value::None);
}

#[test]
fn only_the_fields_that_differ_are_changes_and_an_empty_one_is_none() {
    let rows = rows();
    assert!(room_changes(&rows, &[]).is_empty());
    let drafts = vec![
        (field_id(0, 0), String::from("1G")),
        (field_id(0, 1), String::from("10%")),
        (field_id(1, 0), String::new()),
        (field_id(1, 1), String::from("25%")),
        (field_id(1, 2), String::from(" 64 ")),
    ];
    let c = room_changes(&rows, &drafts);
    assert_eq!(
        c,
        vec![
            ("project".into(), "--mem", "1G".into()),
            ("project".into(), "--cpu", "10%".into()),
            ("agent-1".into(), "--mem", "none".into()),
        ]
    );
    // The line under the fields is the command that would do it.
    assert_eq!(
        preview(&rows, &drafts, 0).as_deref(),
        Some("room set project --mem 1G --cpu 10%")
    );
    assert_eq!(
        preview(&rows, &drafts, 1).as_deref(),
        Some("room set agent-1 --mem none")
    );
    assert_eq!(preview(&rows, &[], 0), None);
    assert_eq!(preview(&rows, &drafts, 9), None);
}

/// Proposed ADR-0329: the page taller than the window scrolls with the
/// wheel to its last control (Apply at the bottom) and back, and the page
/// built again after a change keeps where it was.
#[test]
fn the_page_scrolls_with_the_wheel_to_apply_and_keeps_its_place() {
    use zoraya_ui::{Event, Painter, Response};
    let rows = rows();
    let l = |v| zoraya_tokens::Scale::ONE.logical(v);
    let mut p = Painter::new(zoraya_tokens::Scale::ONE, zoraya_tokens::Appearance::Light);
    let mut w = zoraya_ui::Window::new(page_with_rooms(&s(b""), &rows, &[], ""), (l(600), l(520)));
    w.layout(&mut p);
    let wheel = |dy| Event::Wheel {
        x: l(300),
        y: l(260),
        dx: 0,
        dy,
    };
    // Apply is the last child: out of the frame (no height) at first.
    let apply_rect = |w: &mut zoraya_ui::Window| w.root.children().last().unwrap().rect();
    assert_eq!(apply_rect(&mut w).h, l(0));
    let press = |r: zoraya_ui::LRect| Event::Pointer {
        x: r.x + l(4),
        y: r.y + l(4),
        buttons: 1,
        modifiers: 0,
    };
    assert_eq!(w.event(wheel(-120)), Response::None, "at the top already");
    let mut notches = 0;
    while w.event(wheel(120)) == Response::Repaint {
        notches += 1;
        assert!(notches < 40);
    }
    assert!(notches >= 2, "{notches}");
    w.layout(&mut p);
    let r = apply_rect(&mut w);
    assert_eq!(r.h, l(28));
    assert!(r.bottom() <= l(500), "{r:?}");
    assert_eq!(w.event(press(r)), Response::Activated(APPLY_ROOMS));
    // Built again (a field's draft), the page stays at the end.
    let at = w.root.scrolled();
    let drafts = vec![(field_id(1, 0), String::from("32M"))];
    w.replace_keeping_scroll(page_with_rooms(&s(b""), &rows, &drafts, ""), &mut p);
    assert_eq!(w.root.scrolled(), at);
    let r = apply_rect(&mut w);
    assert_eq!(w.event(press(r)), Response::Activated(APPLY_ROOMS));
    // Back to the top: Apply out of the frame again.
    w.event(wheel(-64 * 120));
    w.layout(&mut p);
    assert_eq!(w.root.scrolled().map(|s| s.at), Some(0));
    assert_eq!(apply_rect(&mut w).h, l(0));
}

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

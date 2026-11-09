//! Stage 7f: the desktop's settings (ADR-0169) against the fake of
//! `tests.rs`: read at start with each bad line's default, applied from the
//! panel's `ApplySettings` only and only when every value is of its type
//! (INV-0169-2, INV-0169-4), the theme reaching every window's `Configure`
//! and the frames, the layouts the bar.

use super::rooms::done;
use super::*;
use crate::settings::{GuiSettings, checked, read, words};

fn apply(s: &mut Setup, end: u32, w: [&str; 6]) -> u32 {
    let m = gui::ApplySettings {
        theme: zoraya_protocol::Data(w[0].as_bytes()),
        motion: zoraya_protocol::Data(w[1].as_bytes()),
        scale: zoraya_protocol::Data(w[2].as_bytes()),
        wallpaper: zoraya_protocol::Data(w[3].as_bytes()),
        layouts: zoraya_protocol::Data(w[4].as_bytes()),
        layout: zoraya_protocol::Data(w[5].as_bytes()),
    };
    s.f().send(end, message(&m, 6), vec![]);
    s.c.step();
    done(s, end)
}

#[test]
fn the_file_is_read_with_each_bad_lines_default() {
    let (s, problems) = read(b"");
    assert_eq!(s, GuiSettings::default());
    assert_eq!((s.dark, s.motion, s.scale), (false, true, 0));
    assert_eq!((s.layouts.as_slice(), s.layout), (&[0][..], 0));
    assert!(problems.is_empty());
    let file = b"gui.theme = dark\ngui.motion = no\ngui.scale = 2\n\
                 gui.keyboard-layouts = us ua cz\ngui.keyboard-layout = cz\n";
    let (s, problems) = read(file);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!((s.dark, s.motion, s.scale), (true, false, 2));
    let cz = zoraya_keymap::by_name("cz").unwrap();
    let ua = zoraya_keymap::by_name("ua").unwrap();
    assert_eq!((s.layouts, s.layout), (vec![0, ua, cz], cz));
    // Bad values: reported with their line, the default kept.
    let (s, problems) =
        read(b"gui.theme = blue\ngui.keyboard-layouts = us us\ngui.keyboard-layout = ru\n");
    assert!(!s.dark);
    assert_eq!(s.layouts, vec![0]);
    assert!(problems[0].contains("line 1"), "{problems:?}");
    assert!(problems[1].contains("line 2"), "{problems:?}");
    assert!(
        problems[2].contains("gui.keyboard-layout ru"),
        "{problems:?}"
    );
    let (_, problems) = read(b"gui.wallpaper = /home/sea.png\n");
    assert!(problems[0].contains("no picture"), "{problems:?}");
    // What the panel sends for settings is what they are.
    let all = read(file).0;
    let w = words(&all);
    let w: Vec<&str> = w.iter().map(String::as_str).collect();
    assert_eq!(checked(w[0], w[1], w[2], w[3], w[4], w[5]), Some(all));
}

#[test]
fn only_the_panel_applies_settings_and_only_valid_ones() {
    let mut s = setup();
    let panel = s.panel();
    let client = s.client(64);
    let _ = s.window_of(client, 40, 30, 0x0011_2233);
    s.f().take(client);
    let dark = ["dark", "no", "auto", "", "us ua", "ua"];
    // A client's channel and the driver manager's: refused, nothing changes.
    s.f().send(
        client,
        message(
            &gui::ApplySettings {
                theme: zoraya_protocol::Data(b"dark"),
                motion: zoraya_protocol::Data(b"no"),
                scale: zoraya_protocol::Data(b"auto"),
                wallpaper: zoraya_protocol::Data(b""),
                layouts: zoraya_protocol::Data(b"us ua"),
                layout: zoraya_protocol::Data(b"ua"),
            },
            6,
        ),
        vec![],
    );
    s.c.step();
    let m = s.manager;
    assert_ne!(apply(&mut s, m, dark), status::OK);
    assert_eq!(s.c.appearance(), Appearance::Light);
    // Any value not of its type, or a layout outside the list: refused.
    for bad in [
        ["blue", "no", "auto", "", "us ua", "ua"],
        ["dark", "maybe", "auto", "", "us ua", "ua"],
        ["dark", "no", "3", "", "us ua", "ua"],
        ["dark", "no", "auto", "", "us us", "us"],
        ["dark", "no", "auto", "", "us ua", "cz"],
        ["dark", "no", "auto", "", "", "us"],
    ] {
        assert_eq!(apply(&mut s, panel, bad), status::INVALID, "{bad:?}");
        assert_eq!(s.c.appearance(), Appearance::Light, "{bad:?}");
        assert_eq!(s.c.layout_short(), "US");
    }
    // A valid one: the theme everywhere, and the layout at once.
    s.f().take(client);
    assert_eq!(apply(&mut s, panel, dark), status::OK);
    assert_eq!(s.c.appearance(), Appearance::Dark);
    let got: Vec<gui::Configure> = s
        .f()
        .take(client)
        .iter()
        .filter_map(|m| decode::<gui::Configure>(m, 0).ok().map(|(_, c)| c))
        .collect();
    assert!(got.iter().any(|c| c.dark && !c.motion), "{got:?}");
    assert_ne!(s.c.layout_short(), "US");
}

#[test]
fn the_scale_set_applies_to_the_display_at_start() {
    let mut s = setup();
    assert_eq!(s.c.scale().factor(), 1);
    let fixed = GuiSettings {
        scale: 2,
        ..GuiSettings::default()
    };
    let f = Fake::new();
    let mut f2 = f;
    let manager = f2.channel(64);
    let display = f2.channel(64);
    f2.display = Some(display);
    let c = Compositor::with_settings(f2, manager, Some(display), &[], fixed);
    assert_eq!(c.scale().factor(), 2, "640 wide, but the setting says 2");
    let _ = &mut s;
}

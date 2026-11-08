use std::boxed::Box;
use std::string::String;
use std::vec;
use std::vec::Vec;

use zoraya_tokens::{Appearance, LIGHT, Scale, space};

use crate::*;

fn painter() -> Painter {
    Painter::new(Scale::ONE, Appearance::Light)
}

/// A window of `w × h` logical pixels at scale 1 around `root`.
fn window(root: Box<dyn Widget>, w: u32, h: u32, p: &mut Painter) -> (Window, Vec<u32>) {
    let size = (Scale::ONE.logical(w), Scale::ONE.logical(h));
    let mut win = Window::new(root, size);
    win.layout(p);
    (win, vec![0u32; (w * h) as usize])
}

fn key(code: u32) -> Event {
    Event::Key {
        code,
        text: None,
        modifiers: 0,
    }
}

fn typed(c: char) -> Event {
    Event::Key {
        code: 30,
        text: Some(c),
        modifiers: 0,
    }
}

#[test]
fn a_button_paints_in_its_tokens_and_activates() {
    let mut p = painter();
    let (mut w, mut px) = window(Box::new(Button::new(7, "Open")), 120, 28, &mut p);
    w.paint(&mut p, &mut px, 120);
    // The middle of the button's left part: bg.control; its edge: border.
    assert_eq!(px[14 * 120 + 6], LIGHT.bg_control.pixel());
    assert_eq!(px[14 * 120], LIGHT.border.pixel());
    // The label is drawn in fg.text somewhere in the middle row band.
    assert!((8..20).any(|y| (0..120).any(|x| px[y * 120 + x] == LIGHT.fg_text.pixel())));
    assert_eq!(w.event(key(keys::TAB)), Response::Repaint);
    assert_eq!(w.event(key(keys::ENTER)), Response::Activated(7));
    let press = Event::Pointer {
        x: Scale::ONE.logical(10),
        y: Scale::ONE.logical(10),
        buttons: 1,
    };
    assert_eq!(w.event(press), Response::Activated(7));
    // Focused: the ring in accent.
    w.paint(&mut p, &mut px, 120);
    assert!(px.contains(&LIGHT.accent.pixel()));
    let mut b = Button::new(1, "OK");
    b.default = true;
    let (mut w, mut px) = window(Box::new(b), 80, 28, &mut p);
    w.paint(&mut p, &mut px, 80);
    assert_eq!(px[14 * 80 + 4], LIGHT.accent.pixel());
}

#[test]
fn a_text_field_edits_by_character() {
    let mut p = painter();
    let (mut w, mut px) = window(Box::new(TextField::new(3, "звіт")), 200, 28, &mut p);
    w.move_focus(false);
    assert_eq!(w.event(typed('-')), Response::Changed(3));
    assert_eq!(w.event(typed('ř')), Response::Changed(3));
    assert_eq!(w.event(key(keys::LEFT)), Response::Repaint);
    assert_eq!(w.event(key(keys::BACKSPACE)), Response::Changed(3));
    w.event(key(keys::HOME));
    assert_eq!(w.event(key(keys::DELETE)), Response::Changed(3));
    w.event(key(keys::END));
    assert_eq!(
        w.event(typed('\u{7}')),
        Response::None,
        "no control characters"
    );
    w.paint(&mut p, &mut px, 200);
    assert_eq!(px[14 * 200 + 100], LIGHT.bg_view.pixel());
}

#[test]
fn check_box_switch_and_radio_change_on_keys_and_presses() {
    let mut p = painter();
    let (mut w, _) = window(Box::new(CheckBox::new(1, "Motion", false)), 200, 28, &mut p);
    w.move_focus(false);
    assert_eq!(w.event(key(keys::SPACE)), Response::Changed(1));
    let (mut w, _) = window(Box::new(Switch::new(2, true)), 80, 28, &mut p);
    w.move_focus(false);
    assert_eq!(w.event(key(keys::ENTER)), Response::Changed(2));
    let (mut w, mut px) = window(
        Box::new(RadioGroup::new(5, &["Light", "Dark"], 0)),
        200,
        56,
        &mut p,
    );
    w.move_focus(false);
    assert_eq!(w.event(key(keys::DOWN)), Response::Changed(5));
    let press = Event::Pointer {
        x: Scale::ONE.logical(5),
        y: Scale::ONE.logical(5),
        buttons: 1,
    };
    assert_eq!(w.event(press), Response::Changed(5));
    w.paint(&mut p, &mut px, 200);
    assert!(px.contains(&LIGHT.accent.pixel()));
}

#[test]
fn a_list_moves_its_selection_and_scrolls_it_into_view() {
    let mut p = painter();
    let rows: Vec<String> = (0..20).map(|i| std::format!("row {i}")).collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let (mut w, mut px) = window(Box::new(ListView::new(9, &refs)), 200, 112, &mut p);
    w.move_focus(false);
    for _ in 0..6 {
        w.event(key(keys::DOWN));
    }
    assert_eq!(w.event(key(keys::ENTER)), Response::Activated(9));
    w.event(key(keys::END));
    w.paint(&mut p, &mut px, 200);
    // The last row is selected and shown at the bottom (four rows fit).
    assert_eq!(px[(3 * 28 + 2) * 200 + 190], LIGHT.bg_selection.pixel());
    w.event(key(keys::HOME));
    w.paint(&mut p, &mut px, 200);
    assert_eq!(px[2 * 200 + 190], LIGHT.bg_selection.pixel());
}

#[test]
fn a_progress_bar_and_a_separator() {
    let mut p = painter();
    let (mut w, mut px) = window(Box::new(ProgressBar::new(500)), 100, 8, &mut p);
    w.paint(&mut p, &mut px, 100);
    assert_eq!(px[4 * 100 + 25], LIGHT.accent.pixel());
    assert_eq!(px[4 * 100 + 75], LIGHT.border.pixel());
    let (mut w, mut px) = window(Box::new(Separator::new()), 50, 4, &mut p);
    w.paint(&mut p, &mut px, 50);
    assert_eq!(px[2 * 50 + 10], LIGHT.border.pixel());
}

/// INV-0165-1: no widget paints outside its rectangle and the clip, with
/// hostile content (long text, zero size, a scroll past the end).
#[test]
fn widgets_paint_only_inside_their_rectangle() {
    const GUARD: u32 = 0x0055_5555;
    let make: Vec<Box<dyn Fn() -> Box<dyn Widget>>> = vec![
        Box::new(|| Box::new(Label::new(&"W".repeat(300)))),
        Box::new(|| Box::new(Button::new(1, &"Ж".repeat(200)))),
        Box::new(|| Box::new(TextField::new(1, &"ř".repeat(400)))),
        Box::new(|| Box::new(CheckBox::new(1, &"x".repeat(300), true))),
        Box::new(|| Box::new(Switch::new(1, true))),
        Box::new(|| Box::new(RadioGroup::new(1, &["a", "b", "c", "d", "e", "f"], 9))),
        Box::new(|| Box::new(ListView::new(1, &["a"; 50]))),
        Box::new(|| Box::new(Separator::new())),
        Box::new(|| Box::new(Menu::new(1, &["Open", "Save"]))),
        Box::new(|| Box::new(ProgressBar::new(5000))),
        Box::new(|| {
            Box::new(Dialog::new(
                &"Unsaved changes. ".repeat(20),
                vec![Button::new(1, "Save"), Button::new(2, "Discard")],
            ))
        }),
        Box::new(|| Box::new(ScrollView::new(Box::new(ListView::new(1, &["z"; 80]))))),
    ];
    for (i, f) in make.iter().enumerate() {
        for (w, h) in [(0u32, 0u32), (1, 1), (37, 9), (90, 140)] {
            let mut p = painter();
            // The widget inside a 20-pixel guard border of a larger canvas.
            let (cw, ch) = (w + 40, h + 40);
            let mut px = vec![GUARD; (cw * ch) as usize];
            let mut widget = f();
            let r = LRect::new(
                Scale::ONE.logical(20),
                Scale::ONE.logical(20),
                Scale::ONE.logical(w),
                Scale::ONE.logical(h),
            );
            widget.layout(r, &mut p);
            {
                let mut c = zoraya_paint::Canvas::new(&mut px, cw as usize);
                c.set_clip(r.physical(Scale::ONE));
                widget.paint(&mut p, &mut c, true);
                for child in widget.children() {
                    child.paint(&mut p, &mut c, true);
                }
            }
            for y in 0..ch {
                for x in 0..cw {
                    let inside = (20..20 + w).contains(&x) && (20..20 + h).contains(&y);
                    if !inside {
                        assert_eq!(
                            px[(y * cw + x) as usize],
                            GUARD,
                            "widget {i} at {w}x{h}: ({x}, {y})"
                        );
                    }
                }
            }
        }
    }
    // A scroll past the end stays at the end.
    let mut p = painter();
    let mut s = ScrollView::new(Box::new(ListView::new(1, &["z"; 8])));
    s.layout(
        LRect::new(
            Logical::ZERO,
            Logical::ZERO,
            space::CONTROL_HEIGHT.times(4),
            space::CONTROL_HEIGHT.times(2),
        ),
        &mut p,
    );
    for _ in 0..30 {
        s.scroll(true, &mut p);
    }
    let end = s.offset;
    s.scroll(true, &mut p);
    assert_eq!(s.offset, end);
    for _ in 0..40 {
        s.scroll(false, &mut p);
    }
    assert_eq!(s.offset, Logical::ZERO);
}

/// INV-0165-4: Tab reaches every focusable widget and comes round;
/// Escape closes a menu or a dialog from any state.
#[test]
fn tab_reaches_every_focusable_widget_and_escape_closes() {
    let mut p = painter();
    let children: Vec<Box<dyn Widget>> = vec![
        Box::new(Label::new("Name")),
        Box::new(TextField::new(1, "")),
        Box::new(Row::new(vec![
            Box::new(Button::new(2, "Cancel")),
            Box::new(Button::new(3, "Save")),
        ])),
        Box::new(CheckBox::new(4, "Keep", false)),
        Box::new(Separator::new()),
        Box::new(Switch::new(5, false)),
    ];
    let (mut w, _) = window(Box::new(Column::new(children)), 400, 300, &mut p);
    let mut seen = Vec::new();
    for _ in 0..5 {
        w.event(key(keys::TAB));
        seen.push(w.focused().unwrap());
    }
    w.event(key(keys::TAB));
    assert_eq!(w.focused().unwrap(), seen[0], "round");
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5, "every focusable widget once");
    let back = Event::Key {
        code: keys::TAB,
        text: None,
        modifiers: keys::SHIFT,
    };
    w.event(back);
    assert_eq!(w.focused().unwrap(), vec![5]);
    w.event(back);
    assert_eq!(w.focused().unwrap(), vec![3]);
    // Escape in a plain window does nothing; in a menu or dialog it closes.
    assert_eq!(w.event(key(keys::ESC)), Response::None);
    let (mut m, _) = window(Box::new(Menu::new(1, &["Open", "Save"])), 100, 60, &mut p);
    assert_eq!(m.event(key(keys::ESC)), Response::Closed);
    m.move_focus(false);
    m.event(key(keys::DOWN));
    assert_eq!(m.event(key(keys::ESC)), Response::Closed);
    let (mut d, _) = window(
        Box::new(Dialog::new("Close?", vec![Button::new(1, "Yes")])),
        300,
        120,
        &mut p,
    );
    d.move_focus(false);
    assert_eq!(d.event(key(keys::ENTER)), Response::Activated(1));
    assert_eq!(d.event(key(keys::ESC)), Response::Closed);
}

/// INV-0165-2: the toolkit depends on no channel or runtime crate.
#[test]
fn the_toolkit_sends_nothing() {
    let manifest = include_str!("../Cargo.toml");
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap()
        .split("\n[")
        .next()
        .unwrap();
    for forbidden in ["zoraya-rt", "zoraya-channel", "zoraya-user", "zoraya-abi"] {
        assert!(!deps.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn layout_places_children_with_gaps_and_scales() {
    let mut p = Painter::new(Scale::TWO, Appearance::Dark);
    let children: Vec<Box<dyn Widget>> =
        vec![Box::new(Button::new(1, "A")), Box::new(Button::new(2, "B"))];
    let mut row = Row::new(children);
    let (w, h) = row.measure(&mut p);
    assert_eq!(h, space::CONTROL_HEIGHT);
    row.layout(LRect::new(Logical::ZERO, Logical::ZERO, w, h), &mut p);
    let a = row.children()[0].rect();
    let b = row.children()[1].rect();
    assert_eq!(b.x, a.right() + space::GAP_CONTROLS);
    assert_eq!(a.physical(Scale::TWO).h, 56);
    let mut grid = Grid::new(
        2,
        vec![
            Box::new(Label::new("x")),
            Box::new(Label::new("y")),
            Box::new(Label::new("z")),
        ],
    );
    let (_, gh) = grid.measure(&mut p);
    assert_eq!(gh, space::CONTROL_HEIGHT.times(2) + space::GAP_CONTROLS);
    let size = (Scale::TWO.logical(400), Scale::TWO.logical(200));
    let mut w = Window::new(Box::new(Button::new(1, "Dark")), size);
    w.layout(&mut p);
    let mut px = vec![0u32; 400 * 200];
    w.paint(&mut p, &mut px, 400);
    assert_eq!(px[100 * 400 + 8], zoraya_tokens::DARK.bg_control.pixel());
}

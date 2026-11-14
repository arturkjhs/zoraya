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
        modifiers: 0,
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
    assert_eq!(w.event(key(keys::ENTER)), Response::Activated(3));
    assert_eq!(w.value(3), Value::Text("вітř"));
    // Rebuilt with its cursor: typing goes where it was.
    let (mut w, _) = window(
        Box::new(TextField::with_cursor(4, "abc", 1)),
        200,
        28,
        &mut p,
    );
    w.move_focus(false);
    w.event(typed('x'));
    assert_eq!(w.value(4), Value::Text("axbc"));
    let (mut w, _) = window(
        Box::new(TextField::with_cursor(4, "ab", 9)),
        200,
        28,
        &mut p,
    );
    w.move_focus(false);
    w.event(typed('x'));
    assert_eq!(w.value(4), Value::Text("abx"));
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
        modifiers: 0,
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
        Box::new(|| {
            Box::new(ScrollView::new(vec![Box::new(ListView::new(
                1, &["z"; 80],
            ))]))
        }),
        Box::new(|| {
            let mut s = ScrollView::new(
                (0..40)
                    .map(|i| Box::new(Label::new(&"row ".repeat(i))) as Box<dyn Widget>)
                    .collect(),
            );
            s.first = 1000;
            Box::new(s)
        }),
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
    let mut s = ScrollView::new(
        (0..8)
            .map(|_| Box::new(Label::new("z")) as Box<dyn Widget>)
            .collect(),
    );
    s.layout(
        LRect::new(
            Logical::ZERO,
            Logical::ZERO,
            space::CONTROL_HEIGHT.times(4),
            space::CONTROL_HEIGHT.times(4),
        ),
        &mut p,
    );
    for _ in 0..30 {
        s.scroll_by(1, &mut p);
    }
    let end = s.first;
    assert!(end > 0);
    s.scroll_by(1, &mut p);
    assert_eq!(s.first, end);
    for _ in 0..40 {
        s.scroll_by(-1, &mut p);
    }
    assert_eq!(s.first, 0);
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
    for forbidden in [
        "zoraya-rt",
        "zoraya-channel",
        "zoraya-user",
        "zoraya-abi",
        "zoraya-proto",
        "zoraya-protocol",
    ] {
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

// Mutants 7d: what the first tests left unread (rectangles, values, the
// order of focus, the pixels of the small controls).

fn l(n: u32) -> zoraya_tokens::Logical {
    Scale::ONE.logical(n)
}

fn rect(x: u32, y: u32, w: u32, h: u32) -> LRect {
    LRect::new(l(x), l(y), l(w), l(h))
}

fn pointer(x: u32, y: u32, buttons: u32) -> Event {
    Event::Pointer {
        x: l(x),
        y: l(y),
        buttons,
        modifiers: 0,
    }
}

fn button(id: u32) -> Box<dyn Widget> {
    Box::new(Button::new(id, "Save"))
}

#[test]
fn rows_columns_and_grids_place_their_children_exactly() {
    let mut p = painter();
    let (bw, bh) = Button::new(0, "Save").measure(&mut p);
    assert_eq!(bw, p.text_width("Save") + l(32));
    assert_eq!(bh, l(28));

    let mut col = Column::new(vec![button(1), button(2), button(3)]);
    assert_eq!(col.measure(&mut p), (bw + l(40), l(3 * 28 + 2 * 8 + 40)));
    col.layout(rect(0, 0, 300, 200), &mut p);
    assert_eq!(col.rect(), rect(0, 0, 300, 200));
    for (i, c) in col.children.iter().enumerate() {
        assert_eq!(c.rect(), rect(20, 20 + 36 * i as u32, 260, 28));
    }
    let mut short = Column::new(vec![button(1), button(2)]);
    short.layout(rect(0, 0, 300, 60), &mut p);
    assert_eq!(short.children[0].rect(), rect(20, 20, 260, 20));
    assert_eq!(short.children[1].rect(), rect(20, 48, 260, 0));

    let mut row = Row::new(vec![button(1), button(2)]);
    assert_eq!(row.measure(&mut p), (bw.times(2) + l(8), l(28)));
    row.layout(rect(5, 7, 500, 28), &mut p);
    assert_eq!(row.rect(), rect(5, 7, 500, 28));
    assert_eq!(row.children[0].rect(), LRect::new(l(5), l(7), bw, l(28)));
    assert_eq!(
        row.children[1].rect(),
        LRect::new(l(13) + bw, l(7), bw, l(28))
    );
    row.padding = space::PADDING_WINDOW;
    assert_eq!(row.measure(&mut p), (bw.times(2) + l(48), l(68)));
    let mut narrow = Row::new(vec![button(1), button(2)]);
    let width = bw.times(2) + l(8) - l(10);
    narrow.layout(LRect::new(l(0), l(0), width, l(28)), &mut p);
    assert_eq!(narrow.children[1].rect().right(), width);

    let mut grid = Grid::new(2, vec![button(1), button(2), button(3)]);
    assert_eq!(grid.measure(&mut p), (bw.times(2) + l(8), l(64)));
    let gr = LRect::new(l(10), l(10), bw.times(2) + l(8), l(64));
    grid.layout(gr, &mut p);
    assert_eq!(grid.rect(), gr);
    assert_eq!(grid.children[0].rect(), LRect::new(l(10), l(10), bw, l(28)));
    assert_eq!(
        grid.children[1].rect(),
        LRect::new(l(18) + bw, l(10), bw, l(28))
    );
    assert_eq!(grid.children[2].rect(), LRect::new(l(10), l(46), bw, l(28)));
    let mut tight = Grid::new(2, vec![button(1), button(2)]);
    let gn = LRect::new(l(0), l(0), bw.times(2), l(20));
    tight.layout(gn, &mut p);
    assert_eq!(tight.children[1].rect().right(), gn.right());
    assert_eq!(tight.children[0].rect().h, l(20));
    let (mut w, _) = window(
        Box::new(Grid::new(2, vec![button(1), button(2)])),
        300,
        100,
        &mut p,
    );
    w.event(key(keys::TAB));
    assert_eq!(w.focused(), Some(vec![0]));
}

#[test]
fn a_press_lands_only_in_its_rectangle_and_focuses_what_it_lands_on() {
    let mut p = painter();
    let lh = Label::new("x").measure(&mut p).1.physical(Scale::ONE);
    let y2 = 20 + 28 + 8 + lh + 8;
    let col = Column::new(vec![button(1), Box::new(Label::new("x")), button(2)]);
    let (mut w, _) = window(Box::new(col), 300, 200, &mut p);
    // The first button is (20, 20, 260, 28).
    for (x, y) in [(19, 25), (280, 25), (100, 48), (25, 19)] {
        assert_eq!(w.event(pointer(x, y, 1)), Response::None, "{x},{y}");
    }
    assert_eq!(w.focused(), None);
    assert_eq!(w.event(pointer(20, 20, 1)), Response::Activated(1));
    assert_eq!(w.event(pointer(279, 47, 1)), Response::Activated(1));
    assert_eq!(w.focused(), Some(vec![0]));
    // A press on the label, which takes no focus, leaves it.
    w.event(pointer(30, 60, 1));
    assert_eq!(w.focused(), Some(vec![0]));
    // A hover neither activates nor focuses.
    assert_eq!(w.event(pointer(30, y2 + 4, 0)), Response::None);
    assert_eq!(w.focused(), Some(vec![0]));
    assert_eq!(w.event(pointer(30, y2 + 4, 1)), Response::Activated(2));
    assert_eq!(w.focused(), Some(vec![2]));

    // Outside the window, nothing.
    let radio = RadioGroup::new(3, &["a", "b"], 0);
    let (mut w, _) = window(Box::new(radio), 200, 56, &mut p);
    assert_eq!(w.event(pointer(500, 20, 1)), Response::None);
}

#[test]
fn tab_goes_forward_in_tree_order_and_shift_tab_from_nothing_to_the_last() {
    let mut p = painter();
    let tree = || -> Box<dyn Widget> {
        Box::new(Column::new(vec![
            button(1),
            Box::new(TextField::new(2, "")),
            button(3),
        ]))
    };
    let (mut w, _) = window(tree(), 300, 200, &mut p);
    w.event(key(keys::TAB));
    assert_eq!(w.focused(), Some(vec![0]));
    w.event(key(keys::TAB));
    assert_eq!(w.focused(), Some(vec![1]));
    let back = Event::Key {
        code: keys::TAB,
        text: None,
        modifiers: keys::SHIFT,
    };
    w.event(back);
    assert_eq!(w.focused(), Some(vec![0]));
    let (mut w, _) = window(tree(), 300, 200, &mut p);
    w.event(back);
    assert_eq!(w.focused(), Some(vec![2]));
}

#[test]
fn a_text_field_edits_at_its_cursor() {
    let mut f = TextField::new(1, "ab");
    assert_eq!(f.cursor, 2);
    assert_eq!(f.event(&typed('c')), Response::Changed(1));
    assert_eq!((f.value.as_str(), f.cursor), ("abc", 3));
    assert_eq!(f.event(&key(keys::LEFT)), Response::Repaint);
    assert_eq!(f.event(&typed('X')), Response::Changed(1));
    assert_eq!((f.value.as_str(), f.cursor), ("abXc", 3));
    assert_eq!(f.event(&key(keys::BACKSPACE)), Response::Changed(1));
    assert_eq!((f.value.as_str(), f.cursor), ("abc", 2));
    f.event(&key(keys::RIGHT));
    f.event(&key(keys::RIGHT));
    assert_eq!(f.cursor, 3);
    f.event(&key(keys::HOME));
    assert_eq!(f.cursor, 0);
    assert_eq!(f.event(&key(keys::BACKSPACE)), Response::None);
    assert_eq!(f.event(&key(keys::DELETE)), Response::Changed(1));
    assert_eq!((f.value.as_str(), f.cursor), ("bc", 0));
    f.event(&key(keys::RIGHT));
    assert_eq!(f.event(&key(keys::DELETE)), Response::Changed(1));
    assert_eq!((f.value.as_str(), f.cursor), ("b", 1));
    f.event(&key(keys::END));
    assert_eq!(f.event(&key(keys::DELETE)), Response::None);
    f.event(&key(keys::LEFT));
    f.event(&key(keys::LEFT));
    assert_eq!(f.cursor, 0);
    let mut p = painter();
    assert_eq!(f.measure(&mut p).1, l(28));
}

#[test]
fn a_check_box_and_a_switch_paint_their_parts_and_flip() {
    let mut p = painter();
    let mut c = CheckBox::new(1, "Bold", false);
    assert_eq!(c.measure(&mut p), (l(17 + 8) + p.text_width("Bold"), l(28)));
    assert_eq!(c.event(&key(keys::SPACE)), Response::Changed(1));
    assert!(c.checked);
    c.layout(rect(0, 0, 200, 28), &mut p);
    assert_eq!(c.rect(), rect(0, 0, 200, 28));
    assert_eq!(c.event(&pointer(5, 5, 1)), Response::Changed(1));
    assert!(!c.checked);
    assert_eq!(c.event(&pointer(5, 5, 0)), Response::None);

    let (mut w, mut px) = window(Box::new(CheckBox::new(1, "Bold", true)), 200, 28, &mut p);
    w.paint(&mut p, &mut px, 200);
    // The box is (0, 5, 17, 17), its mark (4, 9, 9, 9).
    for (x, y) in [(2, 10), (6, 6), (15, 13), (6, 20)] {
        assert_eq!(px[y * 200 + x], LIGHT.accent.pixel(), "{x},{y}");
    }
    assert_eq!(px[13 * 200 + 8], LIGHT.fg_text_on_accent.pixel());
    // The label starts past the box and the gap.
    assert!((0..28).all(|y| (17..25).all(|x| px[y * 200 + x] != LIGHT.fg_text.pixel())));
    assert!((0..28).any(|y| (25..60).any(|x| px[y * 200 + x] == LIGHT.fg_text.pixel())));

    let mut s = Switch::new(2, true);
    assert_eq!(s.measure(&mut p), (l(48), l(28)));
    assert_eq!(s.event(&key(keys::ENTER)), Response::Changed(2));
    assert!(!s.on);
    for on in [true, false] {
        let (mut w, mut px) = window(Box::new(Switch::new(2, on)), 48, 28, &mut p);
        w.paint(&mut p, &mut px, 48);
        let at = |x: usize| px[14 * 48 + x];
        if on {
            // The knob (22, 2, 24, 24) at the right of an accent track.
            assert_eq!(at(20), LIGHT.accent.pixel());
            assert_eq!(at(24), LIGHT.bg_control.pixel());
            assert_eq!(at(34), LIGHT.bg_control.pixel());
        } else {
            assert_eq!(at(4), LIGHT.bg_control.pixel());
            assert_eq!(at(14), LIGHT.bg_control.pixel());
            assert_eq!(at(28), LIGHT.border_strong.pixel());
            assert_eq!(at(36), LIGHT.border_strong.pixel());
        }
    }
}

#[test]
fn a_radio_group_wraps_its_arrows_and_picks_the_row_under_a_press() {
    let mut p = painter();
    let mut g = RadioGroup::new(1, &["one", "two", "three"], 0);
    assert_eq!(g.measure(&mut p), (p.text_width("three") + l(25), l(84)));
    assert_eq!(g.event(&key(keys::DOWN)), Response::Changed(1));
    assert_eq!(g.chosen, 1);
    assert_eq!(g.event(&key(keys::UP)), Response::Changed(1));
    assert_eq!(g.chosen, 0);
    g.event(&key(keys::UP));
    assert_eq!(g.chosen, 2);
    g.event(&key(keys::DOWN));
    assert_eq!(g.chosen, 0);
    g.layout(rect(0, 10, 200, 84), &mut p);
    for (y, chosen) in [
        (20, 0),
        (37, 0),
        (38, 1),
        (40, 1),
        (66, 2),
        (80, 2),
        (500, 2),
    ] {
        g.chosen = 1;
        assert_eq!(g.event(&pointer(5, y, 1)), Response::Changed(1));
        assert_eq!(g.chosen, chosen, "y {y}");
    }
    assert_eq!(g.event(&pointer(5, 40, 0)), Response::None);

    let (mut w, mut px) = window(
        Box::new(RadioGroup::new(1, &["a", "b"], 0)),
        200,
        56,
        &mut p,
    );
    w.paint(&mut p, &mut px, 200);
    // The chosen dot is filled with the accent, the other is not.
    assert_eq!(px[14 * 200 + 8], LIGHT.accent.pixel());
    assert_eq!(px[42 * 200 + 8], LIGHT.bg_control.pixel());
    assert_ne!(px[2 * 200 + 8], LIGHT.accent.pixel());
}

#[test]
fn a_list_moves_its_selection_by_key_and_by_press_from_its_top_row() {
    let mut p = painter();
    let rows: Vec<String> = (0..20).map(|i| std::format!("row {i}")).collect();
    let names: Vec<&str> = rows.iter().map(String::as_str).collect();
    let mut list = ListView::new(1, &names);
    assert_eq!(list.measure(&mut p), (l(168), l(224)));
    list.layout(rect(0, 10, 200, 112), &mut p);
    assert_eq!(list.event(&key(keys::ENTER)), Response::None);
    list.event(&key(keys::DOWN));
    assert_eq!(list.selected, Some(0));
    list.event(&key(keys::DOWN));
    assert_eq!(list.selected, Some(1));
    assert_eq!(list.event(&key(keys::ENTER)), Response::Activated(1));
    assert_eq!(list.event(&key(keys::END)), Response::Changed(1));
    assert_eq!((list.selected, list.top), (Some(19), 16));
    list.event(&key(keys::DOWN));
    assert_eq!(list.selected, Some(19));
    list.event(&key(keys::UP));
    assert_eq!(list.selected, Some(18));
    list.event(&key(keys::HOME));
    assert_eq!((list.selected, list.top), (Some(0), 0));
    assert_eq!(list.event(&pointer(5, 40, 0)), Response::None);
    assert_eq!(list.event(&pointer(5, 40, 2)), Response::None);
    assert_eq!(list.event(&pointer(5, 40, 1)), Response::Changed(1));
    assert_eq!(list.selected, Some(1));
    list.event(&pointer(5, 37, 1));
    assert_eq!(list.selected, Some(0));
    list.event(&pointer(5, 38, 1));
    assert_eq!(list.selected, Some(1));
    list.top = 10;
    list.event(&pointer(5, 40, 1));
    assert_eq!(list.selected, Some(11));

    let mut few = ListView::new(2, &["a", "b", "c"]);
    few.layout(rect(0, 10, 200, 112), &mut p);
    assert_eq!(few.event(&pointer(5, 100, 1)), Response::None);
    assert_eq!(few.event(&pointer(5, 20, 1)), Response::Changed(2));
    assert_eq!(few.selected, Some(0));
    let long = "a long row of the list, wider than its least width of six rows";
    let mut wide = ListView::new(3, &[long]);
    assert_eq!(wide.measure(&mut p).0, p.text_width(long) + l(16));
}

#[test]
fn a_scroll_view_stays_within_its_content_also_when_it_grows() {
    let mut p = painter();
    // Eight labels of 28 with gaps of 8 inside a padding of 20.
    let labels = || -> Vec<Box<dyn Widget>> {
        (0..8)
            .map(|_| Box::new(Label::new("a")) as Box<dyn Widget>)
            .collect()
    };
    let mut v = ScrollView::new(labels());
    assert_eq!(v.measure(&mut p), (l(40) + p.text_width("a"), l(112)));
    // Room for three: 20 + 3 × 28 + 2 × 8 + 20.
    v.layout(rect(0, 0, 200, 140), &mut p);
    assert_eq!(v.rect(), rect(0, 0, 200, 140));
    assert_eq!(v.children[0].rect(), rect(20, 20, 160, 28));
    assert_eq!(v.last_first(), 5);
    v.scroll_by(1, &mut p);
    assert_eq!(v.first, 1);
    assert_eq!(v.children[0].rect().w, l(0));
    assert_eq!(v.children[1].rect(), rect(20, 20, 160, 28));
    assert_eq!(v.children[2].rect(), rect(20, 56, 160, 28));
    for _ in 0..30 {
        v.scroll_by(1, &mut p);
    }
    assert_eq!(v.first, 5);
    v.scroll_by(-1, &mut p);
    assert_eq!(v.first, 4);
    v.scroll_by(1, &mut p);
    // A taller frame: the first comes back so the frame is full.
    v.layout(rect(0, 0, 200, 248), &mut p);
    assert_eq!(v.first, 2);
    assert_eq!(v.children[7].rect(), rect(20, 200, 160, 28));
    let (mut w, mut px) = window(
        Box::new(ScrollView::new(vec![Box::new(ListView::new(1, &["a"; 8]))])),
        200,
        56,
        &mut p,
    );
    w.event(key(keys::TAB));
    assert_eq!(w.focused(), Some(vec![0]));
    w.paint(&mut p, &mut px, 200);
    assert_eq!(px[50 * 200 + 190], LIGHT.bg_view.pixel());
}

#[test]
fn menus_dialogs_separators_and_bars_measure_and_lay_out() {
    let mut p = painter();
    let mut m = Menu::new(4, &["Open", "Save"]);
    assert_eq!(m.event(&key(keys::ESC)), Response::Closed);
    let mut list = ListView::new(4, &["Open", "Save"]);
    assert_eq!(m.measure(&mut p), list.measure(&mut p));
    m.layout(rect(3, 4, 100, 56), &mut p);
    assert_eq!(m.rect(), rect(3, 4, 100, 56));
    assert_eq!(m.list.rect(), rect(3, 4, 100, 56));
    assert!(m.focusable());

    let mut d = Dialog::new(
        "Delete it?",
        vec![Button::new(1, "No"), Button::new(2, "Yes")],
    );
    let measured = d.column[0].measure(&mut p);
    assert_eq!(d.measure(&mut p), measured);
    d.layout(rect(0, 0, 300, 120), &mut p);
    assert_eq!(d.rect(), rect(0, 0, 300, 120));
    assert_eq!(d.column[0].rect(), rect(0, 0, 300, 120));
    let (mut w, mut px) = window(Box::new(d), 300, 120, &mut p);
    w.paint(&mut p, &mut px, 300);
    assert_eq!(px[110 * 300 + 290], LIGHT.bg_window.pixel());

    assert_eq!(Separator::new().measure(&mut p), (l(0), l(4)));
    assert_eq!(ProgressBar::new(500).measure(&mut p), (l(168), l(8)));
    let mut label = Label::new("Name");
    label.layout(rect(1, 2, 80, 28), &mut p);
    assert_eq!(label.rect(), rect(1, 2, 80, 28));
}

/// Mutants 7d, second round: text widths and baselines, the cursor bar,
/// the knob's and the label's places, the focus ring on the chosen dot
/// only, a row of three, a grid away from the top.
#[test]
fn text_and_the_small_parts_are_where_they_belong() {
    let mut p = painter();
    let w1 = p.ui.measure("Save", 13).max(0) as u32;
    assert_eq!(p.text_width("Save"), l(w1));
    let mut p2 = Painter::new(Scale::TWO, Appearance::Light);
    let w2 = p2.ui.measure("Save", 26).max(0) as u32;
    assert_eq!(p2.text_width("Save"), Scale::TWO.logical(w2 + 1));

    // A label's ink sits on its baseline, 18 in a 28-high row at 13 px.
    let (mut w, mut px) = window(Box::new(Label::new("Name")), 120, 28, &mut p);
    w.paint(&mut p, &mut px, 120);
    let ground = px[0];
    let inked = |y: usize, px: &[u32]| (0..120).any(|x| px[y * 120 + x] != ground);
    let lowest = (0..28).rev().find(|y| inked(*y, &px)).expect("ink");
    assert!((16..=18).contains(&lowest), "lowest inked row {lowest}");

    // A button's label is centred.
    let tw = p.text_width("Save").physical(Scale::ONE) as usize;
    let (mut w, mut px) = window(button(1), 120, 28, &mut p);
    w.paint(&mut p, &mut px, 120);
    let left = (0..120)
        .find(|x| (8..20).any(|y| px[y * 120 + x] == LIGHT.fg_text.pixel()))
        .expect("label ink");
    let pad = (120 - tw) / 2;
    assert!(left + 3 >= pad && left <= pad + 3, "left {left}, pad {pad}");

    // The cursor bar after the text before it, inset by the unit.
    let mut field = TextField::new(1, "ab");
    field.cursor = 2;
    let (mut w, mut px) = window(Box::new(field), 200, 28, &mut p);
    w.event(key(keys::TAB));
    w.paint(&mut p, &mut px, 200);
    let x = 8 + p.text_width("ab").physical(Scale::ONE) as usize;
    let at = |x: usize, y: usize| px[y * 200 + x];
    assert_eq!(at(x, 4), LIGHT.fg_text.pixel());
    assert_eq!(at(x, 23), LIGHT.fg_text.pixel());
    assert_ne!(at(x, 3), LIGHT.fg_text.pixel());
    assert_ne!(at(x, 24), LIGHT.fg_text.pixel());

    // The knob is inset by half the unit from the top and the start.
    let (mut w, mut px) = window(Box::new(Switch::new(1, true)), 48, 28, &mut p);
    w.paint(&mut p, &mut px, 48);
    assert_eq!(px[48 + 34], LIGHT.accent.pixel(), "above the knob");
    let (mut w, mut px) = window(Box::new(Switch::new(1, false)), 48, 28, &mut p);
    w.paint(&mut p, &mut px, 48);
    assert_eq!(
        px[14 * 48 + 1],
        LIGHT.border_strong.pixel(),
        "before the knob"
    );

    // A radio group's labels start past the dot and the gap; the ring is
    // on the chosen dot only.
    let (mut w, mut px) = window(
        Box::new(RadioGroup::new(1, &["a", "b"], 0)),
        200,
        56,
        &mut p,
    );
    w.event(key(keys::TAB));
    w.paint(&mut p, &mut px, 200);
    assert!((0..56).all(|y| (18..25).all(|x| px[y * 200 + x] != LIGHT.fg_text.pixel())));
    assert_eq!(px[13 * 200], LIGHT.accent.pixel());
    assert_ne!(px[41 * 200], LIGHT.accent.pixel());
    assert_ne!(px[41 * 200 + 1], LIGHT.accent.pixel());

    // At scale 2 the ring is four pixels deep.
    let size = (l(120), l(28));
    let mut win = Window::new(button(1), size);
    win.layout(&mut p2);
    win.event(key(keys::TAB));
    let mut px = vec![0u32; 240 * 56];
    win.paint(&mut p2, &mut px, 240);
    assert_eq!(px[28 * 240 + 3], LIGHT.accent.pixel());

    let (bw, _) = Button::new(0, "Save").measure(&mut p);
    let mut row = Row::new(vec![button(1), button(2), button(3)]);
    assert_eq!(row.measure(&mut p).0, bw.times(3) + l(16));
    let mut grid = Grid::new(2, vec![button(1), button(2)]);
    grid.layout(LRect::new(l(0), l(10), bw.times(2) + l(8), l(20)), &mut p);
    assert_eq!(grid.children[0].rect().h, l(20));

    let mut f = TextField::new(1, "abc");
    f.event(&key(keys::HOME));
    f.event(&key(keys::END));
    assert_eq!(f.cursor, 3);
}

/// Proposed ADR-0244: the application reads what the person set by the
/// widget's id; a list's details stand right-aligned in the secondary ink.
#[test]
fn values_are_read_by_id_and_details_stand_at_the_right() {
    let mut p = painter();
    let mut list = ListView::new(7, &["include", "main.c"]);
    list.details = vec![String::new(), String::from("1.9 kB")];
    let tree = Column::new(vec![
        Box::new(TextField::new(3, "plán.txt")),
        Box::new(CheckBox::new(4, "untrusted", true)),
        Box::new(Switch::new(5, false)),
        Box::new(RadioGroup::new(6, &["a", "b"], 1)),
        Box::new(list),
        Box::new(Menu::new(8, &["Open"])),
        button(9),
        // Ids 0 and 1 after a button: found only where they are.
        Box::new(TextField::new(0, "zero")),
        Box::new(TextField::new(1, "one")),
    ]);
    let (mut w, mut px) = window(Box::new(tree), 300, 400, &mut p);
    assert_eq!(w.value(3), Value::Text("plán.txt"));
    assert_eq!(w.value(4), Value::On(true));
    assert_eq!(w.value(5), Value::On(false));
    assert_eq!(w.value(6), Value::Index(Some(1)));
    assert_eq!(w.value(7), Value::Index(None));
    assert_eq!(w.value(8), Value::Index(Some(0)));
    assert_eq!(w.value(9), Value::None);
    assert_eq!(w.value(99), Value::None);
    assert_eq!(w.value(0), Value::Text("zero"));
    assert_eq!(w.value(1), Value::Text("one"));
    // Tab to the list (field, check box, switch, radio, list) and Down.
    for _ in 0..5 {
        w.event(key(keys::TAB));
    }
    w.event(key(keys::DOWN));
    assert_eq!(w.value(7), Value::Index(Some(0)));
    w.paint(&mut p, &mut px, 300);
    // The list's second row: the size's ink at the right, none in the
    // first row's right half.
    let list_y = 20 + 28 + 8 + 28 + 8 + 28 + 8 + 56 + 8;
    let secondary = LIGHT.fg_text_secondary.pixel();
    let ink = |y0: usize, xs: core::ops::Range<usize>, px: &[u32]| {
        (y0..y0 + 28).any(|y| xs.clone().any(|x| px[y * 300 + x] == secondary))
    };
    assert!(ink(list_y + 28, 200..280, &px), "the detail at the right");
    assert!(!ink(list_y, 150..280, &px), "no detail on the first row");
    // The detail ends a control's padding inside the row's right edge.
    assert!(
        !ink(list_y + 28, 272..280, &px),
        "nothing in the row's right padding"
    );
}

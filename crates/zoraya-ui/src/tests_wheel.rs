//! The wheel in the toolkit (proposed ADR-0329): turns into rows,
//! clamping, parts of a notch, direction, and which widget takes a turn.

use std::boxed::Box;
use std::string::String;
use std::vec;
use std::vec::Vec;

use zoraya_tokens::{Appearance, Scale};

use crate::*;

fn painter() -> Painter {
    Painter::new(Scale::ONE, Appearance::Light)
}

fn l(n: u32) -> zoraya_tokens::Logical {
    Scale::ONE.logical(n)
}

fn wheel(x: u32, y: u32, dy: i32) -> Event {
    Event::Wheel {
        x: l(x),
        y: l(y),
        dx: 0,
        dy,
    }
}

fn press(x: u32, y: u32) -> Event {
    Event::Pointer {
        x: l(x),
        y: l(y),
        buttons: 1,
        modifiers: 0,
    }
}

fn key(code: u32) -> Event {
    Event::Key {
        code,
        text: None,
        modifiers: 0,
    }
}

/// Ten buttons, ids 1 to 10, in a scroll view of a window 200 × 140: room
/// for three (20 + 3 × 28 + 2 × 8 + 20), so the last first is 7.
fn ten(p: &mut Painter) -> Window {
    let buttons: Vec<Box<dyn Widget>> = (1..=10)
        .map(|i| Box::new(Button::new(i, "b")) as Box<dyn Widget>)
        .collect();
    let mut w = Window::new(Box::new(ScrollView::new(buttons)), (l(200), l(140)));
    w.layout(p);
    w
}

fn first(w: &mut Window) -> usize {
    w.root.scrolled().unwrap().at
}

#[test]
fn turns_become_rows_three_a_notch_with_parts_kept() {
    let mut s = WheelSteps::default();
    assert_eq!(s.turn(120), 3);
    assert_eq!(s.turn(-240), -6);
    // A high-resolution wheel: parts of a notch gather into a row.
    assert_eq!(s.turn(15), 0);
    assert_eq!(s.rest(), 15);
    assert_eq!(s.turn(15), 0);
    assert_eq!(s.turn(15), 1);
    assert_eq!(s.rest(), 5);
    // Back the other way: what was kept counts against it.
    assert_eq!(s.turn(-45), -1);
    assert_eq!(s.rest(), 0);
    assert_eq!(s.turn(0), 0);
    s.turn(30);
    s.stop();
    assert_eq!(s.rest(), 0);
    // Extreme turns are bounded at 64 notches.
    let mut s = WheelSteps::default();
    assert_eq!(s.turn(i32::MAX), 64 * 3);
    assert_eq!(s.turn(i32::MIN), -64 * 3);
    let mut s = WheelSteps::default();
    s.turn(39);
    assert_eq!(s.turn(i32::MAX), 64 * 3);
}

#[test]
fn scrolled_by_stays_within_the_content() {
    assert_eq!(scrolled_by(0, -3, 7), 0);
    assert_eq!(scrolled_by(2, -3, 7), 0);
    assert_eq!(scrolled_by(2, 3, 7), 5);
    assert_eq!(scrolled_by(6, 3, 7), 7);
    assert_eq!(scrolled_by(usize::MAX, i32::MAX, 7), 7);
    assert_eq!(scrolled_by(3, i32::MIN, 7), 0);
    assert_eq!(scrolled_by(3, 0, 0), 0);
}

#[test]
fn a_scroll_view_moves_three_children_a_notch_and_stops_at_its_ends() {
    let mut p = painter();
    let mut w = ten(&mut p);
    // Up at the top: nothing takes it.
    assert_eq!(w.event(wheel(100, 70, -120)), Response::None);
    assert_eq!(first(&mut w), 0);
    assert_eq!(w.event(wheel(100, 70, 120)), Response::Repaint);
    assert_eq!(first(&mut w), 3);
    assert_eq!(w.event(wheel(100, 70, 120)), Response::Repaint);
    assert_eq!(first(&mut w), 6);
    // Clamped: the last button at the bottom.
    assert_eq!(w.event(wheel(100, 70, 120)), Response::Repaint);
    assert_eq!(first(&mut w), 7);
    assert_eq!(w.event(wheel(100, 70, 120)), Response::None);
    assert_eq!(first(&mut w), 7);
    // Laid out again (the application does, and painting does): the
    // first slot holds button 8, and the hidden ones take no press.
    w.layout(&mut p);
    assert_eq!(w.event(press(100, 30)), Response::Activated(8));
    assert_eq!(w.event(press(100, 110)), Response::Activated(10));
    // Parts of a notch: two turns of 20 make one child.
    assert_eq!(w.event(wheel(100, 70, -20)), Response::Repaint);
    assert_eq!(first(&mut w), 7);
    assert_eq!(w.event(wheel(100, 70, -20)), Response::Repaint);
    assert_eq!(first(&mut w), 6);
    // Up past the top: clamped at 0.
    assert_eq!(w.event(wheel(100, 70, -64 * 120)), Response::Repaint);
    assert_eq!(first(&mut w), 0);
}

#[test]
fn a_part_of_a_notch_at_the_top_is_kept_toward_the_content() {
    let mut p = painter();
    let mut w = ten(&mut p);
    for _ in 0..3 {
        assert_eq!(w.event(wheel(100, 70, 10)), Response::Repaint);
        assert_eq!(first(&mut w), 0);
    }
    assert_eq!(w.event(wheel(100, 70, 10)), Response::Repaint);
    assert_eq!(first(&mut w), 1);
}

#[test]
fn painting_lays_out_where_the_wheel_left_it() {
    let mut p = painter();
    let mut w = ten(&mut p);
    let mut px = vec![0u32; 200 * 140];
    w.event(wheel(100, 70, 120));
    w.paint(&mut p, &mut px, 200);
    assert_eq!(w.event(press(100, 30)), Response::Activated(4));
}

#[test]
fn the_keyboards_focus_scrolls_into_view() {
    let mut p = painter();
    let mut w = ten(&mut p);
    let mut px = vec![0u32; 200 * 140];
    for _ in 0..5 {
        w.event(key(keys::TAB));
    }
    assert_eq!(w.focused(), Some(vec![4]));
    // Button 5 at the bottom: the first is 2.
    assert_eq!(first(&mut w), 2);
    w.paint(&mut p, &mut px, 200);
    // The press focuses button 3, in the first slot.
    assert_eq!(w.event(press(100, 30)), Response::Activated(3));
    // Scrolled away with the wheel, then a key: back into view.
    w.event(wheel(100, 70, 120 * 3));
    assert_eq!(first(&mut w), 7);
    assert_eq!(w.event(key(keys::ENTER)), Response::Activated(3));
    assert_eq!(first(&mut w), 2);
    // Shift+Tab to the first, then round to the last.
    let back = Event::Key {
        code: keys::TAB,
        text: None,
        modifiers: keys::SHIFT,
    };
    w.event(back);
    w.event(back);
    assert_eq!(w.focused(), Some(vec![0]));
    assert_eq!(first(&mut w), 0);
    w.event(back);
    assert_eq!(w.focused(), Some(vec![9]));
    assert_eq!(first(&mut w), 7);
}

#[test]
fn a_list_scrolls_its_rows_and_keeps_its_selection() {
    let mut p = painter();
    let rows: Vec<String> = (0..20).map(|i| alloc::format!("row {i}")).collect();
    let rows: Vec<&str> = rows.iter().map(|r| r.as_str()).collect();
    let mut list = ListView::new(1, &rows);
    list.selected = Some(1);
    // Five rows of 28.
    let mut w = Window::new(Box::new(list), (l(200), l(140)));
    w.layout(&mut p);
    assert_eq!(w.event(wheel(50, 50, 120)), Response::Repaint);
    assert_eq!(
        w.scrolled(1),
        Some(Scrolled {
            at: 3,
            free: false,
            cursor: 0
        })
    );
    assert_eq!(w.value(1), Value::Index(Some(1)));
    // To the end: 20 rows, five shown.
    w.event(wheel(50, 50, 120 * 10));
    assert_eq!(w.scrolled(1).unwrap().at, 15);
    assert_eq!(w.event(wheel(50, 50, 120)), Response::None);
    // A press picks the row under it, as scrolled.
    assert_eq!(w.event(press(50, 30)), Response::Changed(1));
    assert_eq!(w.value(1), Value::Index(Some(16)));
    // Scrolled back to the top, the arrow brings the selection into view.
    w.event(wheel(50, 50, -120 * 10));
    assert_eq!(w.scrolled(1).unwrap().at, 0);
    w.move_focus(false);
    assert_eq!(w.event(key(keys::DOWN)), Response::Changed(1));
    assert_eq!(w.value(1), Value::Index(Some(17)));
    assert_eq!(w.scrolled(1).unwrap().at, 13);
}

#[test]
fn a_turn_goes_to_the_innermost_widget_that_can_go_that_way() {
    let mut p = painter();
    let rows = ["r"; 8];
    // A list of eight with room for three (its own height, 84), then ten
    // buttons, in a scroll view of a window 200 × 200.
    let mut children: Vec<Box<dyn Widget>> = vec![Box::new(Sized3(ListView::new(100, &rows)))];
    children.extend((1..=10).map(|i| Box::new(Button::new(i, "b")) as Box<dyn Widget>));
    let mut w = Window::new(Box::new(ScrollView::new(children)), (l(200), l(200)));
    w.layout(&mut p);
    // Over the list: the list moves, the view does not.
    w.event(wheel(50, 40, 120));
    assert_eq!(w.scrolled(100).unwrap().at, 3);
    assert_eq!(first(&mut w), 0);
    w.event(wheel(50, 40, 120));
    assert_eq!(w.scrolled(100).unwrap().at, 5);
    assert_eq!(first(&mut w), 0);
    // The list at its end: the turn goes to the view around it.
    assert_eq!(w.event(wheel(50, 40, 120)), Response::Repaint);
    assert_eq!(w.scrolled(100).unwrap().at, 5);
    assert_eq!(first(&mut w), 3);
    // Over a button: the view.
    w.layout(&mut p);
    w.event(wheel(50, 40, -120));
    assert_eq!(first(&mut w), 0);
    // Outside the window: nobody.
    assert_eq!(w.event(wheel(500, 40, 120)), Response::None);
}

#[test]
fn a_wheel_moves_no_focus() {
    let mut p = painter();
    let mut w = ten(&mut p);
    w.event(wheel(100, 30, 120));
    assert_eq!(w.focused(), None);
    w.move_focus(false);
    w.event(wheel(100, 30, -120));
    assert_eq!(w.focused(), Some(vec![0]));
}

#[test]
fn a_replaced_tree_keeps_where_it_was_scrolled() {
    let mut p = painter();
    let mut w = ten(&mut p);
    w.event(wheel(100, 70, 240));
    assert_eq!(first(&mut w), 6);
    let buttons: Vec<Box<dyn Widget>> = (1..=10)
        .map(|i| Box::new(Button::new(i, "c")) as Box<dyn Widget>)
        .collect();
    w.replace_keeping_scroll(Box::new(ScrollView::new(buttons)), &mut p);
    assert_eq!(first(&mut w), 6);
    assert_eq!(w.event(press(100, 30)), Response::Activated(7));
    // A tree with fewer: clamped at its layout.
    let buttons: Vec<Box<dyn Widget>> = (1..=4)
        .map(|i| Box::new(Button::new(i, "c")) as Box<dyn Widget>)
        .collect();
    w.replace_keeping_scroll(Box::new(ScrollView::new(buttons)), &mut p);
    assert_eq!(first(&mut w), 1);
    // A widget that does not scroll at that place takes nothing.
    w.replace_keeping_scroll(Box::new(Button::new(1, "c")), &mut p);
    assert_eq!(w.root.scrolled(), None);
}

/// A list given the height of three rows.
struct Sized3(ListView);

impl Widget for Sized3 {
    fn measure(&mut self, p: &mut Painter) -> (zoraya_tokens::Logical, zoraya_tokens::Logical) {
        (self.0.measure(p).0, l(84))
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.0.layout(r, p);
    }
    fn rect(&self) -> LRect {
        self.0.rect()
    }
    fn paint(&mut self, p: &mut Painter, c: &mut zoraya_paint::Canvas<'_>, focused: bool) {
        self.0.paint(p, c, focused);
    }
    fn event(&mut self, e: &Event) -> Response {
        self.0.event(e)
    }
    fn id(&self) -> Option<u32> {
        Some(100)
    }
    fn scrolled(&self) -> Option<Scrolled> {
        self.0.scrolled()
    }
}

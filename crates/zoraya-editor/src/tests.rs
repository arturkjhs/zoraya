use super::*;

#[test]
fn typing_moving_and_deleting_by_characters() {
    let mut b = Buffer::new("Příliš\nžluťoučký kůň");
    assert!(!b.modified());
    b.move_cursor(Move::End, false);
    b.insert(" úpěl");
    assert_eq!(b.text(), "Příliš\nžluťoučký kůň úpěl");
    assert!(b.modified());
    b.backspace();
    assert_eq!(b.text(), "Příliš\nžluťoučký kůň úpě");
    b.move_cursor(Move::Start, false);
    b.delete();
    assert_eq!(b.text(), "říliš\nžluťoučký kůň úpě");
    assert!(!b.backspace(), "nothing before the start");
    // Up and down keep the column; line start and end.
    let mut b = Buffer::new("abcdef\nxy\n12345");
    b.move_cursor(Move::Right, false);
    b.move_cursor(Move::Right, false);
    b.move_cursor(Move::Right, false);
    assert_eq!(b.line_col(b.cursor()), (0, 3));
    b.move_cursor(Move::Down, false);
    assert_eq!(b.line_col(b.cursor()), (1, 2), "clamped to the short line");
    b.move_cursor(Move::Down, false);
    assert_eq!(b.line_col(b.cursor()), (2, 3), "the column kept");
    b.move_cursor(Move::Down, false);
    assert_eq!(b.cursor(), b.len());
    b.move_cursor(Move::Up, false);
    b.move_cursor(Move::LineStart, false);
    assert_eq!(b.line_col(b.cursor()), (1, 0));
    b.move_cursor(Move::LineEnd, false);
    assert_eq!(b.line_col(b.cursor()), (1, 2));
    b.move_cursor(Move::Up, false);
    b.move_cursor(Move::Up, false);
    assert_eq!(b.cursor(), 0);
    assert_eq!(b.lines(), ["abcdef", "xy", "12345"]);
    // Control characters are not typed; a line end and a tab are.
    b.insert("a\u{7}\tb\n");
    assert_eq!(b.lines()[0], "a\tb");
}

#[test]
fn a_selection_is_replaced_cut_and_collapsed() {
    let mut b = Buffer::new("hello world");
    b.move_cursor(Move::Right, true);
    b.move_cursor(Move::Right, true);
    assert_eq!(b.selected(), "he");
    b.insert("J");
    assert_eq!(b.text(), "Jllo world");
    b.select(0, 4);
    assert_eq!(b.cut(), Some("Jllo".into()));
    assert_eq!(b.text(), " world");
    assert_eq!(b.cut(), None);
    b.select(1, 3);
    b.move_cursor(Move::Left, false);
    assert_eq!((b.cursor(), b.selection()), (1, (1, 1)), "to its start");
    b.select(1, 3);
    b.move_cursor(Move::Right, false);
    assert_eq!(b.cursor(), 3, "to its end");
    b.select_all();
    assert_eq!(b.selected(), " world");
    b.select(4, 1);
    b.delete();
    assert_eq!(b.text(), " ld");
}

#[test]
fn undo_and_redo_and_what_was_saved() {
    let mut b = Buffer::new("a");
    b.move_cursor(Move::End, false);
    b.insert("b");
    b.insert("c");
    b.saved();
    assert!(!b.modified());
    b.backspace();
    assert!(b.modified());
    assert!(b.undo());
    assert_eq!(b.text(), "abc");
    assert!(!b.modified(), "back to what was saved");
    assert!(b.undo());
    assert_eq!((b.text().as_str(), b.cursor()), ("ab", 2));
    assert!(b.modified());
    assert!(b.redo());
    assert_eq!(b.text(), "abc");
    assert!(!b.modified());
    b.undo();
    b.insert("x");
    assert!(!b.redo(), "a new edit drops what could be redone");
    assert!(b.modified());
    assert!(b.undo() && b.undo(), "b, then x");
    assert!(!b.undo());
    assert_eq!(b.text(), "a");
    assert!(b.modified(), "the saved state is gone");
}

#[test]
fn find_selects_the_next_and_wraps() {
    let mut b = Buffer::new("kůň a kůň a kůň");
    assert!(b.find("kůň"));
    assert_eq!(b.selection(), (0, 3));
    assert!(b.find("kůň"));
    assert_eq!(b.selection(), (6, 9));
    assert!(b.find("kůň"));
    assert_eq!(b.selection(), (12, 15));
    assert!(b.find("kůň"));
    assert_eq!(b.selection(), (0, 3), "from the start again");
    assert!(!b.find("pes"));
    assert!(!b.find(""));
    assert!(!b.find("kůň a kůň a kůň a"));
}

#[test]
fn the_undo_list_is_bounded() {
    let mut b = Buffer::new("");
    for _ in 0..MAX_UNDO + 5 {
        b.insert("x");
    }
    let mut n = 0;
    while b.undo() {
        n += 1;
    }
    assert_eq!(n, MAX_UNDO);
    assert_eq!(b.len(), 5);
}

mod view {
    use alloc::boxed::Box;
    use alloc::rc::Rc;
    use alloc::vec;
    use core::cell::RefCell;

    use zoraya_tokens::{Appearance, LIGHT, Scale};
    use zoraya_ui::{Event, Painter, Response, Window, keys};

    use crate::Buffer;
    use crate::view::EditorView;

    fn key(code: u32, text: Option<char>, modifiers: u32) -> Event {
        Event::Key {
            code,
            text,
            modifiers,
        }
    }

    fn rig(text: &str) -> (Window, Rc<RefCell<Buffer>>, Painter) {
        let mut p = Painter::new(Scale::ONE, Appearance::Light);
        let b = Rc::new(RefCell::new(Buffer::new(text)));
        let size = (Scale::ONE.logical(300), Scale::ONE.logical(120));
        let mut w = Window::new(Box::new(EditorView::new(7, b.clone())), size);
        w.layout(&mut p);
        w.move_focus(false);
        (w, b, p)
    }

    #[test]
    fn keys_type_move_and_undo() {
        let (mut w, b, _) = rig("ab\ncd");
        assert_eq!(w.event(key(30, Some('ž'), 0)), Response::Changed(7));
        assert_eq!(b.borrow().text(), "žab\ncd");
        assert_eq!(w.event(key(keys::DOWN, None, 0)), Response::Repaint);
        w.event(key(keys::END, None, 0));
        assert_eq!(w.event(key(keys::ENTER, None, 0)), Response::Changed(7));
        assert_eq!(b.borrow().text(), "žab\ncd\n");
        // Ctrl+Z undoes, Ctrl+Y redoes, Ctrl+A selects all.
        assert_eq!(w.event(key(44, Some('\u{1a}'), 2)), Response::Changed(7));
        assert_eq!(b.borrow().text(), "žab\ncd");
        assert_eq!(w.event(key(21, None, 2)), Response::Changed(7));
        assert_eq!(b.borrow().text(), "žab\ncd\n");
        w.event(key(30, None, 2));
        assert_eq!(b.borrow().selected(), "žab\ncd\n");
        // A control character is not typed.
        assert_eq!(w.event(key(46, Some('\u{3}'), 2)), Response::None);
        w.event(key(keys::BACKSPACE, None, 0));
        assert!(b.borrow().is_empty());
    }

    #[test]
    fn a_press_puts_the_cursor_and_the_text_is_painted() {
        let (mut w, b, mut p) = rig("hello\nworld");
        let mut px = vec![0u32; 300 * 120];
        w.paint(&mut p, &mut px, 300);
        assert!(
            px.iter().any(|c| *c == LIGHT.fg_text.pixel()),
            "text and cursor"
        );
        // A press on the second line, past its end: the end of that line.
        let l = |v| Scale::ONE.logical(v);
        w.event(Event::Pointer {
            x: l(290),
            y: l(30),
            buttons: 1,
        });
        assert_eq!(b.borrow().line_col(b.borrow().cursor()), (1, 5));
    }
}

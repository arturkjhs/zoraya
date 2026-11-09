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

#[test]
fn is_empty_tells_text_from_no_text() {
    assert!(!Buffer::new("a").is_empty());
    assert!(Buffer::new("").is_empty());
}

#[test]
fn a_left_without_a_selection_steps_back_one_character() {
    let mut b = Buffer::new("abc");
    b.move_cursor(Move::End, false);
    b.move_cursor(Move::Left, false);
    assert_eq!(b.cursor(), 2);
    // Shift+Left over a selection steps, keeping the anchor.
    b.select(0, 2);
    b.move_cursor(Move::Left, true);
    assert_eq!((b.cursor(), b.selection()), (1, (0, 1)));
}

#[test]
fn delete_at_the_end_does_nothing() {
    let mut b = Buffer::new("ab");
    b.move_cursor(Move::End, false);
    assert!(!b.delete());
    assert_eq!(b.text(), "ab");
}

#[test]
fn typing_and_backspace_mid_text_touch_only_their_span() {
    let mut b = Buffer::new("hello");
    b.move_cursor(Move::Right, false);
    b.insert("J");
    assert_eq!(b.text(), "hJello");
    let mut b = Buffer::new("hello");
    b.select(1, 3);
    b.backspace();
    assert_eq!(b.text(), "hlo");
}

#[test]
fn the_buffer_holds_max_chars_and_no_more() {
    let text: String = core::iter::repeat_n('a', MAX_CHARS - 1).collect();
    let mut b = Buffer::new(&text);
    b.move_cursor(Move::End, false);
    assert!(!b.insert("xy"), "one past the most");
    assert!(b.insert("x"), "up to the most");
    assert_eq!(b.len(), MAX_CHARS);
    assert!(!b.insert("x"));
    // Replacing one character keeps the length.
    b.select(0, 1);
    assert!(b.insert("x"));
    assert_eq!(b.len(), MAX_CHARS);
}

#[test]
fn redo_removes_again_what_undo_put_back() {
    let mut b = Buffer::new("ab");
    b.move_cursor(Move::End, false);
    b.backspace();
    assert!(b.undo());
    assert_eq!(b.text(), "ab");
    assert!(b.redo());
    assert_eq!((b.text().as_str(), b.cursor()), ("a", 1));
}

#[test]
fn redo_puts_the_cursor_after_what_it_typed() {
    let mut b = Buffer::new("a");
    b.move_cursor(Move::End, false);
    b.insert("bc");
    b.undo();
    assert!(b.redo());
    assert_eq!(b.cursor(), 3);
}

#[test]
fn find_accepts_a_needle_as_long_as_the_text() {
    let mut b = Buffer::new("kůň");
    assert!(b.find("kůň"));
    assert_eq!(b.selection(), (0, 3));
}

#[test]
fn find_wraps_to_a_match_at_the_last_position() {
    let mut b = Buffer::new("xa");
    b.move_cursor(Move::End, false);
    assert!(b.find("a"));
    assert_eq!(b.selection(), (1, 2));
}

mod view {
    use alloc::boxed::Box;
    use alloc::rc::Rc;
    use alloc::vec;
    use core::cell::RefCell;

    use alloc::vec::Vec;

    use zoraya_paint::Canvas;
    use zoraya_tokens::{Appearance, Color, LIGHT, Scale};
    use zoraya_ui::{Event, LRect, Painter, Response, Widget, Window, keys};

    use crate::view::EditorView;
    use crate::{Buffer, Move};

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

    /// The view's geometry at scale 1, from the tokens, as `paint` uses it.
    struct Geo {
        lh: i32,
        cw: i32,
        pad: i32,
    }

    fn geo(p: &mut Painter) -> Geo {
        let px = zoraya_tokens::text::BODY.physical(p.scale);
        Geo {
            lh: px as i32 * 3 / 2,
            cw: p.mono.measure("M", px).max(1),
            pad: zoraya_tokens::space::PADDING_CONTROL.physical(p.scale) as i32,
        }
    }

    /// Where the view stands in its canvas: away from the origin, so a
    /// position that forgets the view's own place shows.
    const AT: (i32, i32) = (10, 20);
    const W: usize = 340;
    const H: usize = 160;

    /// A view of `text`, laid out at `AT` (300 by 120), prepared by `set`.
    fn view(text: &str, set: impl FnOnce(&mut Buffer)) -> (EditorView, Painter) {
        let mut p = Painter::new(Scale::ONE, Appearance::Light);
        let mut b = Buffer::new(text);
        set(&mut b);
        let mut v = EditorView::new(7, Rc::new(RefCell::new(b)));
        let l = |n: i32| Scale::ONE.logical(n as u32);
        v.layout(LRect::new(l(AT.0), l(AT.1), l(300), l(120)), &mut p);
        (v, p)
    }

    fn paint(v: &mut EditorView, p: &mut Painter, focused: bool) -> Vec<u32> {
        let mut px = vec![0u32; W * H];
        let mut c = Canvas::new(&mut px, W);
        v.paint(p, &mut c, focused);
        px
    }

    /// The pixels of colour `c`, as points.
    fn of(px: &[u32], c: Color) -> Vec<(i32, i32)> {
        let want = c.pixel();
        (0..px.len())
            .filter(|i| px[*i] == want)
            .map(|i| ((i % W) as i32, (i / W) as i32))
            .collect()
    }

    /// The text's ink: anything but the view's ground.
    fn ink(px: &[u32]) -> Vec<(i32, i32)> {
        let ground = LIGHT.bg_view.pixel();
        (0..px.len())
            .filter(|i| px[*i] != ground && px[*i] != 0)
            .map(|i| ((i % W) as i32, (i / W) as i32))
            .collect()
    }

    #[test]
    fn the_view_asks_for_twelve_by_eight_controls() {
        let (mut v, mut p) = view("", |_| {});
        let h = zoraya_tokens::space::CONTROL_HEIGHT;
        assert_eq!(v.measure(&mut p), (h.times(12), h.times(8)));
    }

    #[test]
    fn the_arrow_home_delete_and_tab_keys_act() {
        let (mut v, _) = view("ab\ncd", |_| {});
        let b = v.buffer.clone();
        assert_eq!(v.event(&key(keys::RIGHT, None, 0)), Response::Repaint);
        assert_eq!(b.borrow().cursor(), 1);
        assert_eq!(v.event(&key(keys::LEFT, None, 0)), Response::Repaint);
        assert_eq!(b.borrow().cursor(), 0);
        v.event(&key(keys::DOWN, None, 0));
        v.event(&key(keys::RIGHT, None, 0));
        assert_eq!(v.event(&key(keys::UP, None, 0)), Response::Repaint);
        assert_eq!(b.borrow().cursor(), 1);
        assert_eq!(v.event(&key(keys::HOME, None, 0)), Response::Repaint);
        assert_eq!(b.borrow().cursor(), 0);
        assert_eq!(v.event(&key(keys::DELETE, None, 0)), Response::Changed(7));
        assert_eq!(b.borrow().text(), "b\ncd");
        assert_eq!(v.event(&key(keys::TAB, None, 0)), Response::Changed(7));
        assert_eq!(b.borrow().text(), "\tb\ncd");
    }

    #[test]
    fn ctrl_shift_z_redoes_and_a_plain_z_or_y_is_typed() {
        let (mut v, _) = view("", |_| {});
        let b = v.buffer.clone();
        v.event(&key(30, Some('a'), 0));
        assert_eq!(v.event(&key(44, None, 2)), Response::Changed(7));
        assert_eq!(b.borrow().text(), "");
        assert_eq!(
            v.event(&key(44, None, 2 | keys::SHIFT)),
            Response::Changed(7)
        );
        assert_eq!(b.borrow().text(), "a");
        assert_eq!(v.event(&key(44, Some('z'), 0)), Response::Changed(7));
        assert_eq!(v.event(&key(21, Some('y'), 0)), Response::Changed(7));
        assert_eq!(b.borrow().text(), "azy");
    }

    #[test]
    fn control_characters_and_ctrl_letters_are_not_typed() {
        let (mut v, _) = view("ab", |_| {});
        assert_eq!(v.event(&key(46, Some('\u{7}'), 0)), Response::None);
        assert_eq!(v.event(&key(46, Some('c'), 2)), Response::None);
        assert_eq!(v.buffer.borrow().text(), "ab");
    }

    #[test]
    fn a_press_needs_the_left_button_inside_the_view() {
        let (mut v, mut p) = view("abc\ndef", |b| b.move_cursor(Move::End, false));
        paint(&mut v, &mut p, true);
        let l = |n: i32| Scale::ONE.logical(n as u32);
        let (x, y) = (l(AT.0 + 20), l(AT.1 + 10));
        let moved = Event::Pointer { x, y, buttons: 0 };
        assert_eq!(v.event(&moved), Response::None);
        let outside = Event::Pointer {
            x: l(AT.0 + 20),
            y: l(AT.1 + 130),
            buttons: 1,
        };
        assert_eq!(v.event(&outside), Response::None);
        assert_eq!(v.buffer.borrow().cursor(), 7, "the cursor stays");
    }

    #[test]
    fn each_line_is_painted_in_its_row_from_the_padding() {
        let (mut v, mut p) = view("abc\ndef", |_| {});
        let g = geo(&mut p);
        let px = paint(&mut v, &mut p, false);
        let ink = ink(&px);
        let (x0, y0) = (AT.0 + g.pad, AT.1 + g.pad);
        let row = |k: i32| y0 + k * g.lh..y0 + (k + 1) * g.lh;
        assert!(
            ink.iter()
                .all(|(_, y)| row(0).contains(y) || row(1).contains(y))
        );
        assert!(
            ink.iter().any(|(_, y)| row(0).contains(y)),
            "the first line"
        );
        assert!(
            ink.iter().any(|(_, y)| row(1).contains(y)),
            "the second line"
        );
        // The text starts at the padding and takes three cells.
        let left = ink.iter().map(|(x, _)| *x).min().unwrap();
        let right = ink.iter().map(|(x, _)| *x).max().unwrap();
        assert!((x0..x0 + g.cw / 2).contains(&left), "{left} from {x0}");
        assert!(right < x0 + 3 * g.cw, "{right}");
        // Each line sits in the lower part of its row, as body text does.
        let top = ink.iter().map(|(_, y)| *y).min().unwrap();
        assert!(top >= y0 + g.lh / 8, "{top}");
    }

    #[test]
    fn a_selection_covers_its_cells_and_the_line_end() {
        let (mut v, mut p) = view("abc\ndef", |b| b.select(1, 5));
        let g = geo(&mut p);
        let px = paint(&mut v, &mut p, false);
        let (x0, y0) = (AT.0 + g.pad, AT.1 + g.pad);
        // Cells 1 to 3 of the first line (3 is its end), cell 0 of the
        // second; the glyphs drawn over them leave their top rows clear.
        let rects = [(x0 + g.cw, y0, 3 * g.cw, g.lh), (x0, y0 + g.lh, g.cw, g.lh)];
        let inside = |x: i32, y: i32| {
            rects
                .iter()
                .any(|(rx, ry, rw, rh)| (*rx..rx + rw).contains(&x) && (*ry..ry + rh).contains(&y))
        };
        let sel = of(&px, LIGHT.bg_selection);
        assert!(sel.iter().all(|(x, y)| inside(*x, *y)), "only its cells");
        let at = |x: i32, y: i32| px[y as usize * W + x as usize];
        for (rx, ry, rw, _) in rects {
            for x in rx..rx + rw {
                assert_eq!(at(x, ry), LIGHT.bg_selection.pixel(), "({x}, {ry})");
            }
        }
    }

    #[test]
    fn the_cursor_bar_sits_in_its_cell_on_its_line() {
        let bar = |text: &str, at: usize| {
            let (mut v, mut p) = view(text, |b| b.select(at, at));
            let g = geo(&mut p);
            let px = paint(&mut v, &mut p, true);
            let off = paint(&mut v, &mut p, false);
            // The bar: the pixels the focus adds.
            let mut pts: Vec<(i32, i32)> = (0..px.len())
                .filter(|i| px[*i] != off[*i])
                .map(|i| ((i % W) as i32, (i / W) as i32))
                .collect();
            pts.sort();
            (pts, g)
        };
        let check = |pts: &[(i32, i32)], g: &Geo, col: i32, line: i32| {
            let (x0, y0) = (AT.0 + g.pad, AT.1 + g.pad);
            let x = x0 + col * g.cw;
            let (top, bottom) = (
                y0 + line * g.lh + g.lh / 8,
                y0 + line * g.lh + g.lh - g.lh / 4 + g.lh / 8,
            );
            assert!(!pts.is_empty());
            assert!(
                pts.iter().all(|(px, _)| *px == x),
                "column {col}: {:?}",
                pts.first()
            );
            assert_eq!(pts.first().unwrap().1, top);
            assert_eq!(pts.last().unwrap().1, bottom - 1);
        };
        let (pts, g) = bar("abc", 3);
        check(&pts, &g, 3, 0);
        // A tab is four cells; the line's start is counted over the first.
        let (pts, g) = bar("abc\n\txy", 6);
        check(&pts, &g, 5, 1);
    }

    #[test]
    fn a_scrolled_view_keeps_the_cursor_row_last() {
        let (mut v, mut p) = view(&"\n".repeat(39), |b| b.move_cursor(Move::End, false));
        let g = geo(&mut p);
        let rows = (120 - 2 * g.pad) / g.lh;
        let y0 = AT.1 + g.pad;
        // Empty lines: the only text colour is the bar's.
        let row_of = |v: &mut EditorView, p: &mut Painter| {
            let px = paint(v, p, true);
            let ys: Vec<i32> = of(&px, LIGHT.fg_text).iter().map(|(_, y)| *y).collect();
            assert!(!ys.is_empty(), "the bar is in view");
            (ys[0] - y0) / g.lh
        };
        assert_eq!(
            row_of(&mut v, &mut p),
            rows - 1,
            "the last line in the last row"
        );
        v.buffer.borrow_mut().move_cursor(Move::Start, false);
        assert_eq!(row_of(&mut v, &mut p), 0, "back to the top");
        v.buffer.borrow_mut().move_cursor(Move::Down, false);
        assert_eq!(row_of(&mut v, &mut p), 1, "no scroll while in view");
    }

    #[test]
    fn a_line_past_the_last_row_is_not_painted() {
        let mut p = Painter::new(Scale::ONE, Appearance::Light);
        let g = geo(&mut p);
        let rows = ((120 - 2 * g.pad) / g.lh) as usize;
        let lines = |n: usize| alloc::vec!["Hg"; n].join("\n");
        let (mut a, mut pa) = view(&lines(rows), |_| {});
        let (mut b, mut pb) = view(&lines(rows + 1), |_| {});
        assert!(paint(&mut a, &mut pa, false) == paint(&mut b, &mut pb, false));
    }

    #[test]
    fn a_press_maps_to_the_line_and_column_under_it() {
        let (mut v, mut p) = view("abcdef\nabcdef\nabcdef", |_| {});
        let g = geo(&mut p);
        paint(&mut v, &mut p, true);
        let (x0, y0) = (AT.0 + g.pad, AT.1 + g.pad);
        let l = |n: i32| Scale::ONE.logical(n as u32);
        let mut press = |x: i32, y: i32| {
            v.event(&Event::Pointer {
                x: l(x),
                y: l(y),
                buttons: 1,
            });
            let b = v.buffer.borrow();
            b.line_col(b.cursor())
        };
        // A press goes to the nearer side of the cell under it.
        assert_eq!(
            press(x0 + 3 * g.cw + g.cw / 4, y0 + g.lh + g.lh / 2),
            (1, 3)
        );
        assert_eq!(
            press(x0 + 3 * g.cw + 3 * g.cw / 4, y0 + g.lh + g.lh / 2),
            (1, 4)
        );
        assert_eq!(press(x0 + g.cw / 4, y0 + 2 * g.lh + 1), (2, 0));
        assert_eq!(press(x0 + 5 * g.cw + g.cw / 4, y0 + 1), (0, 5));
    }
}

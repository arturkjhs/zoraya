//! The model: selection, sorting, search, history, rename, delete, the
//! menu and its lines, drops, the status line, sizes and dates.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::format::{civil, date, size};
use crate::model::{Focus, keys, mods};
use crate::*;

const HOME: &str = "/home/ann";
const DAY: u64 = 86_400 * 1_000_000_000;
const HOUR: u64 = 3_600 * 1_000_000_000;
/// 2026-11-10 14:32 UTC.
const NOW: u64 = 1_794_321_120 * 1_000_000_000;

fn item(name: &str, dir: bool, size: u64, modified: u64) -> Item {
    Item {
        name: String::from(name),
        dir,
        size,
        modified,
    }
}

/// The mockup's folder: ~/project.
fn project_items() -> Vec<Item> {
    vec![
        item("docs", true, 0, NOW - DAY),
        item("include", true, 0, NOW - 7 * DAY),
        item("tests", true, 0, NOW - 5 * HOUR),
        item("main.c", false, 1900, NOW - HOUR),
        item("Makefile", false, 412, NOW - 9 * DAY),
        item("notes.txt", false, 2100, NOW - 3 * HOUR),
        item("plán.txt", false, 1200, NOW - DAY - 2 * HOUR),
        item("звіт-2026.txt", false, 2100, NOW - 5 * DAY),
        item("Žďár.md", false, 913, NOW - 6 * DAY),
    ]
}

/// The holds of `files` in the room `project`.
fn project_holds() -> Vec<(String, bool)> {
    vec![
        (String::from("/"), false),
        (String::from("~/project"), true),
        (String::from("~/include"), false),
    ]
}

fn model() -> Model {
    let dir = "/home/ann/project";
    let mut m = Model::new(dir, HOME, Some("project"), places(&project_holds(), HOME));
    m.now = NOW;
    m.openers = vec![
        (String::from("txt"), String::from("edit")),
        (String::from("md"), String::from("edit")),
    ];
    m.show(dir, project_items());
    m
}

fn names(m: &Model) -> Vec<String> {
    m.view().iter().map(|i| m.items[*i].name.clone()).collect()
}

fn acts(m: &mut Model) -> Vec<Act> {
    m.acts.drain(..).collect()
}

#[test]
fn sizes_and_dates_read_as_the_mockup_writes_them() {
    assert_eq!(size(0), "0 B");
    assert_eq!(size(412), "412 B");
    assert_eq!(size(913), "913 B");
    assert_eq!(size(1200), "1.2 kB");
    assert_eq!(size(2100), "2.1 kB");
    assert_eq!(size(6100), "6.1 kB");
    assert_eq!(size(16_732_160), "16 MB");
    assert_eq!(size(412_000_000), "412 MB");
    assert_eq!(size(1_500_000_000), "1.5 GB");
    assert_eq!(civil(0), (1970, 1, 1));
    assert_eq!(civil(20_767), (2026, 11, 10));
    assert_eq!(civil(-1), (1969, 12, 31));
    assert_eq!(date(NOW - HOUR, NOW), "Today 13:32");
    assert_eq!(date(NOW - DAY, NOW), "Yesterday 14:32");
    assert_eq!(date(NOW - 7 * DAY, NOW), "3 Nov 2026");
    assert_eq!(date(0, NOW), "\u{2014}");
    // No clock: a date, never "Today".
    assert_eq!(date(NOW, 0), "10 Nov 2026");
}

#[test]
fn the_sidebar_shows_home_the_rooms_grants_and_the_system() {
    let p = places(&project_holds(), HOME);
    let shown: Vec<(&str, bool, bool, bool)> = p
        .iter()
        .map(|p| (p.label.as_str(), p.write, p.room, p.system))
        .collect();
    assert_eq!(
        shown,
        [
            ("Home", false, false, false),
            ("project", true, true, false),
            ("include", false, true, false),
            ("/system", false, false, true),
            ("/", false, false, true),
        ]
    );
    // In home: Home to change.
    let home = places(
        &[(String::from("/"), false), (String::from("~"), true)],
        HOME,
    );
    assert!(home[0].write);
    assert_eq!(home.len(), 3);
}

#[test]
fn folders_come_first_and_a_header_sorts_and_reverses() {
    let mut m = model();
    assert_eq!(
        names(&m),
        [
            "docs",
            "include",
            "tests",
            "main.c",
            "Makefile",
            "notes.txt",
            "plán.txt",
            "Žďár.md",
            "звіт-2026.txt"
        ]
    );
    // Modified: newest first, folders still first.
    m.sort_by(Column::Modified);
    assert!(!m.ascending);
    assert_eq!(
        names(&m),
        [
            "tests",
            "docs",
            "include",
            "main.c",
            "notes.txt",
            "plán.txt",
            "звіт-2026.txt",
            "Žďár.md",
            "Makefile"
        ]
    );
    // Again: oldest first.
    m.sort_by(Column::Modified);
    assert_eq!(names(&m)[3], "Makefile");
    // Size: largest first; equal sizes by name.
    m.sort_by(Column::Size);
    assert_eq!(&names(&m)[3..6], ["notes.txt", "звіт-2026.txt", "main.c"]);
    m.sort_by(Column::Name);
    assert!(m.ascending);
    assert_eq!(names(&m)[3], "main.c");
}

#[test]
fn a_sort_keeps_the_selection_and_the_cursor() {
    let mut m = model();
    m.click(5, 0); // notes.txt
    m.sort_by(Column::Size);
    assert_eq!(m.selected_names(), ["notes.txt"]);
    assert_eq!(m.cursor, Some(3));
}

#[test]
fn click_shift_and_command_select_as_the_mockup_shows() {
    let mut m = model();
    m.sort_by(Column::Modified);
    // main.c, then Shift to plán.txt, then Command on Žďár.md.
    m.click(3, 0);
    m.click(5, mods::SHIFT);
    m.click(7, mods::SYSTEM);
    assert_eq!(
        m.selected_names(),
        ["main.c", "notes.txt", "plán.txt", "Žďár.md"]
    );
    assert_eq!(m.status(), "9 items \u{b7} 4 selected (6.1 kB)");
    // Ctrl does what Command does; again takes it away.
    m.click(7, mods::CTRL);
    assert_eq!(m.selected_names().len(), 3);
    // Shift from the new anchor (7).
    m.click(8, mods::SHIFT);
    assert_eq!(m.selected_names(), ["Žďár.md", "Makefile"]);
    // A plain click: alone; below the rows: nothing.
    m.click(1, 0);
    assert_eq!(m.selected_names(), ["docs"]);
    m.click(usize::MAX, 0);
    assert!(m.selected_names().is_empty());
    assert_eq!(m.status(), "9 items");
}

#[test]
fn the_status_line_counts_and_says_the_free_space() {
    let mut m = model();
    m.click(5, 0);
    assert_eq!(m.status(), "9 items \u{b7} notes.txt selected (2.1 kB)");
    assert_eq!(m.free_text(), "");
    m.free = Some(412_000_000);
    assert_eq!(m.free_text(), "412 MB free");
    m.show("/home/ann/project", vec![item("a", false, 1, 0)]);
    assert_eq!(m.status(), "1 item");
}

#[test]
fn enter_and_a_double_click_open_a_folder_or_a_file() {
    let mut m = model();
    m.activate(0);
    assert_eq!(
        acts(&mut m),
        [Act::List(String::from("/home/ann/project/docs"))]
    );
    m.activate(5);
    assert_eq!(
        acts(&mut m),
        [Act::Open(String::from("/home/ann/project/notes.txt"))]
    );
    m.click(5, 0);
    m.key(keys::DOWN, None, 0);
    m.key(keys::ENTER, None, 0);
    assert_eq!(
        acts(&mut m),
        [Act::Open(String::from("/home/ann/project/plán.txt"))]
    );
}

#[test]
fn arrows_move_shift_extends_command_up_goes_up() {
    let mut m = model();
    m.key(keys::DOWN, None, 0);
    assert_eq!(m.cursor, Some(0));
    m.key(keys::DOWN, None, 0);
    m.key(keys::DOWN, None, mods::SHIFT);
    m.key(keys::DOWN, None, mods::SHIFT);
    assert_eq!(m.selected_names(), ["include", "tests", "main.c"]);
    m.key(keys::UP, None, 0);
    assert_eq!(m.selected_names(), ["tests"]);
    m.key(keys::END, None, 0);
    assert_eq!(m.cursor, Some(8));
    m.key(keys::DOWN, None, 0);
    assert_eq!(m.cursor, Some(8), "stays on the last");
    m.key(keys::HOME, None, 0);
    assert_eq!(m.cursor, Some(0));
    // Command+Up and Ctrl+Up: the folder above.
    m.key(keys::UP, None, mods::SYSTEM);
    assert_eq!(acts(&mut m), [Act::List(String::from(HOME))]);
    m.show(HOME, vec![item("project", true, 0, 0)]);
    m.key(keys::UP, None, mods::CTRL);
    assert_eq!(acts(&mut m), [Act::List(String::from("/home"))]);
}

#[test]
fn back_forward_and_up_remember_where_from() {
    let mut m = model();
    assert!(!m.can_back() && !m.can_forward());
    m.navigate("/home/ann/project/docs");
    m.show("/home/ann/project/docs", Vec::new());
    m.up();
    m.show("/home/ann/project", project_items());
    assert!(m.can_back());
    m.back();
    assert_eq!(
        acts(&mut m).last(),
        Some(&Act::List(String::from("/home/ann/project/docs")))
    );
    m.show("/home/ann/project/docs", Vec::new());
    assert!(m.can_forward());
    m.forward();
    assert_eq!(
        acts(&mut m).last(),
        Some(&Act::List(String::from("/home/ann/project")))
    );
    m.show("/home/ann/project", project_items());
    // Going somewhere new forgets forward.
    m.back();
    m.show("/home/ann/project/docs", Vec::new());
    m.navigate("/system");
    assert!(!m.can_forward());
    // The root has no folder above.
    m.show("/", Vec::new());
    acts(&mut m);
    m.up();
    assert!(acts(&mut m).is_empty());
}

#[test]
fn crumbs_go_from_home_or_from_the_root() {
    let m = model();
    assert_eq!(
        m.crumbs(),
        [
            (String::from("~"), String::from(HOME)),
            (String::from("project"), String::from("/home/ann/project")),
        ]
    );
    let mut m = model();
    m.show("/system/fonts", Vec::new());
    assert_eq!(
        m.crumbs().iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
        ["/", "system", "fonts"]
    );
    // A folder whose name only starts like the home is not under it.
    m.show("/home/annex", Vec::new());
    assert_eq!(m.crumbs()[0].0, "/");
}

#[test]
fn search_filters_by_name_and_returns_to_the_list() {
    let mut m = model();
    m.key(keys::F, None, mods::SYSTEM);
    assert_eq!(m.focus, Focus::Search);
    for t in "TXT".chars() {
        m.key(0, Some(t), 0);
    }
    assert_eq!(names(&m), ["notes.txt", "plán.txt", "звіт-2026.txt"]);
    assert_eq!(m.status(), "3 items matching \u{201c}TXT\u{201d}");
    m.key(keys::BACKSPACE, None, 0);
    assert_eq!(m.search, "TX");
    // Enter: the first match, keys to the list.
    m.key(keys::ENTER, None, 0);
    assert_eq!(m.focus, Focus::List);
    assert_eq!(m.selected_names(), ["notes.txt"]);
    // Typing in the list starts a search; Escape clears it.
    m.search.clear();
    m.key(0, Some('m'), 0);
    m.key(0, Some('d'), 0);
    assert_eq!(m.focus, Focus::Search);
    assert_eq!(names(&m), ["Žďár.md"]);
    m.key(keys::ESC, None, 0);
    assert_eq!(m.view().len(), 9);
    // Another folder clears the search.
    m.key(0, Some('q'), 0);
    m.show(HOME, project_items());
    assert!(m.search.is_empty());
}

#[test]
fn rename_in_place_selects_the_stem_and_keeps_on_enter() {
    let mut m = model();
    m.click(6, 0); // plán.txt
    m.key(keys::F2, None, 0);
    let r = m.rename.clone().unwrap();
    assert_eq!(
        (r.text.as_str(), r.cursor, r.stem_selected),
        ("plán.txt", 4, true)
    );
    assert_eq!(m.status(), "Enter keeps the name \u{b7} Escape leaves it");
    // Typing replaces the stem.
    for t in "plán-v2".chars() {
        m.key(0, Some(t), 0);
    }
    assert_eq!(m.rename.as_ref().unwrap().text, "plán-v2.txt");
    m.key(keys::ENTER, None, 0);
    assert_eq!(
        acts(&mut m),
        [Act::Rename {
            dir: String::from("/home/ann/project"),
            old: String::from("plán.txt"),
            new: String::from("plán-v2.txt"),
        }]
    );
    assert!(m.rename.is_none());
}

#[test]
fn rename_edits_by_character_and_refuses_bad_names() {
    let mut m = model();
    m.click(5, 0); // notes.txt
    m.key(keys::F2, None, 0);
    m.key(keys::END, None, 0);
    m.key(keys::BACKSPACE, None, 0);
    m.key(keys::LEFT, None, 0);
    m.key(keys::DELETE, None, 0);
    m.key(keys::HOME, None, 0);
    m.key(0, Some('_'), 0);
    // A slash is never typed.
    m.key(0, Some('/'), 0);
    assert_eq!(m.rename.as_ref().unwrap().text, "_notes.t");
    m.key(keys::ESC, None, 0);
    assert!(m.rename.is_none() && m.acts.is_empty());
    // A name taken.
    m.key(keys::F2, None, 0);
    m.key(keys::END, None, 0);
    for _ in 0..9 {
        m.key(keys::BACKSPACE, None, 0);
    }
    for t in "main.c".chars() {
        m.key(0, Some(t), 0);
    }
    m.key(keys::ENTER, None, 0);
    assert!(m.acts.is_empty());
    assert_eq!(m.status(), "main.c is there already.");
    // No name.
    m.key(keys::F2, None, 0);
    m.key(keys::END, None, 0);
    for _ in 0..9 {
        m.key(keys::BACKSPACE, None, 0);
    }
    m.key(keys::ENTER, None, 0);
    assert!(m.acts.is_empty());
    // Unchanged: nothing to do.
    m.key(keys::F2, None, 0);
    m.key(keys::ENTER, None, 0);
    assert!(m.acts.is_empty());
    // A click elsewhere keeps what was typed.
    m.key(keys::F2, None, 0);
    m.key(0, Some('x'), 0);
    m.click(0, 0);
    assert_eq!(
        acts(&mut m),
        [Act::Rename {
            dir: String::from("/home/ann/project"),
            old: String::from("notes.txt"),
            new: String::from("x.txt"),
        }]
    );
}

#[test]
fn delete_asks_first_and_names_what_goes() {
    let mut m = model();
    m.click(6, 0);
    m.click(7, mods::SYSTEM);
    m.key(keys::DELETE, None, 0);
    assert_eq!(
        m.confirm_text(),
        Some((
            String::from("Delete 2 items?"),
            String::from("plán.txt and Žďár.md will be deleted.")
        ))
    );
    // Escape keeps them; nothing else reaches the list meanwhile.
    m.key(keys::DOWN, None, 0);
    assert_eq!(m.selected_names().len(), 2);
    m.key(keys::ESC, None, 0);
    assert!(m.confirm.is_none() && m.acts.is_empty());
    m.key(keys::DELETE, None, 0);
    m.key(keys::ENTER, None, 0);
    assert_eq!(
        acts(&mut m),
        [Act::Remove {
            dir: String::from("/home/ann/project"),
            names: vec![String::from("plán.txt"), String::from("Žďár.md")],
        }]
    );
    // One, then the card's buttons.
    m.click(5, 0);
    m.ask_delete();
    assert_eq!(m.confirm_text().unwrap().0, "Delete notes.txt?");
    m.answer_delete(false);
    assert!(m.acts.is_empty());
    m.ask_delete();
    m.answer_delete(true);
    assert_eq!(acts(&mut m).len(), 1);
    // Three or more.
    m.click(3, 0);
    m.click(6, mods::SHIFT);
    m.ask_delete();
    assert_eq!(
        m.confirm_text().unwrap().1,
        "main.c, Makefile and 2 more will be deleted."
    );
}

#[test]
fn nothing_changes_where_the_window_only_reads() {
    let mut m = model();
    m.show("/home/ann/include", vec![item("a.h", false, 10, 0)]);
    m.click(0, 0);
    m.key(keys::DELETE, None, 0);
    assert!(m.confirm.is_none());
    assert_eq!(m.status(), "~/include is read only here.");
    m.key(keys::F2, None, 0);
    assert!(m.rename.is_none());
    m.key(keys::N, None, mods::SYSTEM | mods::SHIFT);
    assert!(m.acts.is_empty());
    assert!(!m.can_write("/home/ann/include"));
    assert!(m.can_write("/home/ann/project/src"));
    assert!(!m.can_write("/config"));
    assert!(!m.can_write(HOME));
}

#[test]
fn command_shift_n_makes_the_first_free_new_folder() {
    let mut m = model();
    m.key(keys::N, None, mods::SYSTEM | mods::SHIFT);
    assert_eq!(
        acts(&mut m),
        [Act::MakeDirectory {
            dir: String::from("/home/ann/project"),
            name: String::from("New folder"),
        }]
    );
    let mut items = project_items();
    items.push(item("New folder", true, 0, 0));
    items.push(item("New folder 2", true, 0, 0));
    m.show("/home/ann/project", items);
    m.key(keys::N, None, mods::CTRL | mods::SHIFT);
    assert_eq!(
        acts(&mut m),
        [Act::MakeDirectory {
            dir: String::from("/home/ann/project"),
            name: String::from("New folder 3"),
        }]
    );
    m.select_name("New folder 2");
    assert_eq!(m.selected_names(), ["New folder 2"]);
}

#[test]
fn the_menu_acts_and_shows_each_items_zosh_line() {
    let mut m = model();
    m.open_menu(380, 328, Some(5)); // notes.txt
    assert_eq!(m.selected_names(), ["notes.txt"]);
    let lines: Vec<(&str, String, &str)> = MENU
        .iter()
        .map(|i| (i.label(), m.menu_line(*i), i.key()))
        .collect();
    assert_eq!(
        lines,
        [
            ("Open", String::from("edit ~/project/notes.txt"), "Enter"),
            ("Open in edit", String::from("edit ~/project/notes.txt"), ""),
            (
                "Rename",
                String::from("mv ~/project/notes.txt ~/project/NAME"),
                "F2"
            ),
            ("Delete", String::from("rm ~/project/notes.txt"), "Delete"),
            (
                "New folder",
                String::from("mkdir ~/project/NAME"),
                "\u{2318}\u{21e7}N"
            ),
            (
                "Copy path",
                String::from("(the clipboard: no zosh form)"),
                ""
            ),
            (
                "Open terminal here",
                String::from("room project term \u{b7} cd ~/project"),
                ""
            ),
        ]
    );
    m.choose(MenuItem::CopyPath);
    assert_eq!(
        acts(&mut m),
        [Act::CopyPath(String::from("~/project/notes.txt"))]
    );
    assert!(m.menu.is_none());
    m.open_menu(0, 0, Some(5));
    m.choose(MenuItem::OpenInEdit);
    assert_eq!(
        acts(&mut m),
        [Act::OpenIn {
            path: String::from("/home/ann/project/notes.txt"),
            app: "edit"
        }]
    );
    m.open_menu(0, 0, Some(5));
    m.choose(MenuItem::Open);
    assert_eq!(
        acts(&mut m),
        [Act::Open(String::from("/home/ann/project/notes.txt"))]
    );
    m.open_menu(0, 0, Some(5));
    m.choose(MenuItem::Rename);
    assert!(m.rename.is_some());
    m.key(keys::ESC, None, 0);
    m.open_menu(0, 0, Some(5));
    m.choose(MenuItem::Delete);
    assert!(m.confirm.is_some());
    m.answer_delete(false);
    m.open_menu(0, 0, Some(5));
    m.choose(MenuItem::NewFolder);
    assert!(matches!(acts(&mut m)[..], [Act::MakeDirectory { .. }]));
    m.open_menu(0, 0, Some(5));
    m.choose(MenuItem::OpenTerminal);
    assert_eq!(
        acts(&mut m),
        [Act::OpenTerminal(String::from("/home/ann/project"))]
    );
    // On a folder: Open is cd, Delete recursive, the terminal opens in it,
    // and there is no Open in edit.
    m.open_menu(0, 0, Some(0));
    assert_eq!(m.menu_line(MenuItem::Open), "cd ~/project/docs");
    assert_eq!(
        m.menu_line(MenuItem::Delete),
        "rm --recursive ~/project/docs"
    );
    assert!(!m.menu_enabled(MenuItem::OpenInEdit));
    m.choose(MenuItem::OpenInEdit);
    assert!(m.acts.is_empty() && m.menu.is_some());
    m.choose(MenuItem::OpenTerminal);
    assert_eq!(
        acts(&mut m),
        [Act::OpenTerminal(String::from("/home/ann/project/docs"))]
    );
    // On no row: only New folder and the terminal.
    m.open_menu(0, 0, None);
    let on: Vec<bool> = MENU.iter().map(|i| m.menu_enabled(*i)).collect();
    assert_eq!(on, [false, false, false, false, true, false, true]);
    m.key(keys::ESC, None, 0);
    assert!(m.menu.is_none());
    // In home the terminal's line has no room.
    let mut h = Model::new(
        HOME,
        HOME,
        None,
        places(
            &[(String::from("/"), false), (String::from("~"), true)],
            HOME,
        ),
    );
    h.show(HOME, vec![item("a.txt", false, 1, 0)]);
    h.open_menu(0, 0, None);
    assert_eq!(h.menu_line(MenuItem::OpenTerminal), "term \u{b7} cd ~");
}

#[test]
fn a_menu_on_a_selected_row_keeps_the_selection() {
    let mut m = model();
    m.click(3, 0);
    m.click(5, mods::SHIFT);
    m.open_menu(0, 0, Some(4));
    assert_eq!(m.selected_names().len(), 3);
    m.open_menu(0, 0, Some(8));
    assert_eq!(m.selected_names(), ["звіт-2026.txt"]);
}

#[test]
fn a_drop_on_a_folder_moves_in_one_place_and_copies_otherwise() {
    let mut m = model();
    m.click(5, 0); // notes.txt
    m.start_drag(5);
    assert_eq!(
        acts(&mut m),
        [Act::Drag {
            path: String::from("/home/ann/project/notes.txt"),
            name: String::from("notes.txt")
        }]
    );
    // Onto docs, in the same place: a move; with Alt: a copy.
    m.dropped_here(Some(0), 0);
    assert_eq!(
        acts(&mut m),
        [Act::Transfer {
            sources: vec![String::from("/home/ann/project/notes.txt")],
            target: String::from("/home/ann/project/docs"),
            copy: false,
        }]
    );
    m.dropped_here(Some(0), mods::ALT);
    assert!(matches!(
        acts(&mut m)[..],
        [Act::Transfer { copy: true, .. }]
    ));
    // Onto a file or nothing: nothing; a folder onto itself: nothing.
    m.dropped_here(Some(5), 0);
    m.dropped_here(None, 0);
    m.click(0, 0);
    m.dropped_here(Some(0), 0);
    assert!(m.acts.is_empty());
    // Into a place the window only reads: refused.
    m.show(
        "/home/ann/include",
        vec![item("a.h", false, 1, 0), item("sub", true, 0, 0)],
    );
    m.click(1, 0);
    m.dropped_here(Some(0), 0);
    assert!(m.acts.is_empty());
    assert_eq!(m.status(), "~/include/sub is read only here.");
    // The drag of a row not selected selects it alone.
    let mut m = model();
    m.click(3, 0);
    m.start_drag(6);
    assert_eq!(m.selected_names(), ["plán.txt"]);
}

/// Review 7h, 8: a drop onto a folder that is another place copies even
/// where both may change; from a folder the window only reads it copies.
#[test]
fn a_drop_into_another_place_or_from_a_read_only_one_copies() {
    let holds = vec![
        (String::from("/"), false),
        (String::from("~"), true),
        (String::from("~/project"), true),
    ];
    let mut m = Model::new(HOME, HOME, Some("wide"), places(&holds, HOME));
    m.show(
        HOME,
        vec![item("project", true, 0, 0), item("a.txt", false, 2, 0)],
    );
    m.click(1, 0);
    m.dropped_here(Some(0), 0);
    assert_eq!(
        acts(&mut m),
        [Act::Transfer {
            sources: vec![String::from("/home/ann/a.txt")],
            target: String::from("/home/ann/project"),
            copy: true,
        }]
    );
    // Home read only: still a copy into the project.
    let holds = vec![
        (String::from("/"), false),
        (String::from("~/project"), true),
    ];
    let mut m = Model::new(HOME, HOME, Some("narrow"), places(&holds, HOME));
    m.show(
        HOME,
        vec![item("project", true, 0, 0), item("a.txt", false, 2, 0)],
    );
    m.click(1, 0);
    m.dropped_here(Some(0), 0);
    assert!(matches!(
        acts(&mut m)[..],
        [Act::Transfer { copy: true, .. }]
    ));
}

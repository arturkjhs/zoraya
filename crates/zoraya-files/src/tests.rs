use alloc::string::String;
use alloc::vec;

use super::*;

fn e(name: &str, dir: bool) -> Entry {
    Entry {
        name: String::from(name),
        dir,
    }
}

fn project() -> Manager {
    let mut m = Manager::new("/", "/home");
    m.show(
        "/home/project",
        vec![
            e("plán.txt", false),
            e("src", true),
            e("a.md", false),
            e("include", true),
        ],
    );
    m
}

#[test]
fn folders_come_first_then_files_each_by_name() {
    let m = project();
    assert_eq!(m.rows(), ["include/", "src/", "a.md", "plán.txt"]);
    assert_eq!(m.shown(), "~/project");
}

#[test]
fn a_folder_is_shown_and_a_file_is_opened_read_only() {
    let mut m = project();
    assert_eq!(
        m.activate(Some(1)),
        Act::Show(String::from("/home/project/src"))
    );
    assert_eq!(
        m.activate(Some(3)),
        Act::OpenWith {
            path: String::from("/home/project/plán.txt"),
            words: String::from("+r ~/project/plán.txt"),
        }
    );
    assert!(matches!(m.activate(None), Act::Say(_)));
    assert!(matches!(m.activate(Some(4)), Act::Say(_)));
    // Outside home the path is shown whole.
    m.show("/texts", vec![e("t.txt", false)]);
    assert_eq!(
        m.activate(Some(0)),
        Act::OpenWith {
            path: String::from("/texts/t.txt"),
            words: String::from("+r /texts/t.txt"),
        }
    );
}

#[test]
fn up_goes_to_the_parent_until_the_top() {
    let mut m = project();
    assert_eq!(m.up(), Act::Show(String::from("/home")));
    m.show("/home", vec![]);
    assert_eq!(m.up(), Act::Show(String::from("/")));
    m.show("/", vec![]);
    assert!(matches!(m.up(), Act::Say(_)));
    assert_eq!(parent("/a"), Some(String::from("/")));
    assert_eq!(parent("/a/b/c"), Some(String::from("/a/b")));
    assert_eq!(join("/", "x"), "/x");
    assert_eq!(join("/a", "x"), "/a/x");
}

#[test]
fn a_new_folder_needs_a_new_valid_name() {
    let mut m = project();
    assert_eq!(
        m.new_folder(" docs "),
        Act::MakeDirectory {
            dir: String::from("/home/project"),
            name: String::from("docs"),
        }
    );
    for bad in ["", ".", "..", "a/b", "src"] {
        assert!(matches!(m.new_folder(bad), Act::Say(_)), "{bad:?}");
    }
}

#[test]
fn rename_takes_the_selected_row_and_a_free_name() {
    let mut m = project();
    assert_eq!(
        m.rename(Some(2), "b.md"),
        Act::Rename {
            dir: String::from("/home/project"),
            old: String::from("a.md"),
            new: String::from("b.md"),
        }
    );
    assert!(matches!(m.rename(Some(2), "a.md"), Act::Say(_)));
    assert!(matches!(m.rename(Some(2), "src"), Act::Say(_)));
    assert!(matches!(m.rename(Some(2), "x/y"), Act::Say(_)));
    assert!(matches!(m.rename(None, "c.md"), Act::Say(_)));
}

#[test]
fn delete_asks_once_then_removes_the_same_row() {
    let mut m = project();
    let Act::Say(s) = m.delete(Some(2)) else {
        panic!()
    };
    assert_eq!(s, "Delete a.md? Press Delete again.");
    assert_eq!(
        m.delete(Some(2)),
        Act::Remove {
            dir: String::from("/home/project"),
            name: String::from("a.md"),
        }
    );
    // Disarmed after removing: the next press asks again.
    assert!(matches!(m.delete(Some(2)), Act::Say(_)));
    // Another row, or anything between, asks again.
    assert!(matches!(m.delete(Some(3)), Act::Say(_)));
    let _ = m.delete(Some(2));
    let _ = m.up();
    assert!(matches!(m.delete(Some(2)), Act::Say(_)));
    for between in 0..5 {
        let _ = m.delete(Some(2));
        match between {
            0 => drop(m.activate(Some(0))),
            1 => drop(m.new_folder("n")),
            2 => drop(m.rename(Some(3), "q.txt")),
            3 => m.show("/home/project", m.entries.clone()),
            _ => drop(m.delete(None)),
        }
        assert!(matches!(m.delete(Some(2)), Act::Say(_)), "{between}");
        let _ = m.delete(None);
    }
}

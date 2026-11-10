//! The file manager's state and what each gesture asks for (stage 7h, the
//! approved mockups `docs/research/desktop/mockups/clear-field/files*.png`):
//! the folder shown and its items, sorted by a column and filtered by the
//! search, the selection (a click, Shift for a range, Command or Ctrl to add
//! or take away), the keyboard's cursor, back and forward, rename in place,
//! delete asked first, the context menu with each item's `zosh` line, and the
//! status line. No I/O: each request is an [`Act`] the program carries out.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::format::{date, size};
use crate::{join, parent};

/// Key codes the model acts on (evdev, as `zoraya-keymap` names them).
pub mod keys {
    pub const ESC: u32 = 1;
    pub const BACKSPACE: u32 = 14;
    pub const TAB: u32 = 15;
    pub const ENTER: u32 = 28;
    pub const N: u32 = 49;
    pub const F: u32 = 33;
    pub const F2: u32 = 60;
    pub const HOME: u32 = 102;
    pub const UP: u32 = 103;
    pub const LEFT: u32 = 105;
    pub const RIGHT: u32 = 106;
    pub const END: u32 = 107;
    pub const DOWN: u32 = 108;
    pub const DELETE: u32 = 111;
}

/// Modifier bits (`zoraya-proto-gui`'s).
pub mod mods {
    pub const SHIFT: u32 = 1;
    pub const CTRL: u32 = 2;
    pub const ALT: u32 = 4;
    pub const SYSTEM: u32 = 16;
    /// Command (the system key, proposed ADR-0248) or Ctrl: the file
    /// manager takes either, so that it works if the system key's chords
    /// stay the compositor's.
    pub const COMMAND: u32 = SYSTEM | CTRL;
}

/// The longest name the rename field takes, in bytes (`zoraya-protocol`'s
/// `Name`).
pub const MAX_NAME: usize = 255;
/// The new folder's name; a second is "New folder 2", and so on.
pub const NEW_FOLDER: &str = "New folder";

/// One item of the folder shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub dir: bool,
    /// Bytes (0 for a folder).
    pub size: u64,
    /// Nanoseconds since the epoch, 0 when unknown.
    pub modified: u64,
}

/// A column of the list, which a click on its header sorts by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Name,
    Size,
    Modified,
}

/// A place in the sidebar: a grant the window holds, with what it allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub label: String,
    /// Absolute.
    pub path: String,
    /// May change what is in it (`+w`), or only read it (`+r`).
    pub write: bool,
    /// One of the room's grants (drawn with the room's dot).
    pub room: bool,
    /// Under "System" (the image).
    pub system: bool,
}

/// Where the keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Search,
}

/// Rename in place: the item, the text typed and the cursor (in chars).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub item: usize,
    pub text: String,
    pub cursor: usize,
    /// The stem, selected when the field opens: typing replaces it.
    pub stem_selected: bool,
}

/// The context menu: where (logical, in the body) and on which item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Menu {
    pub x: u32,
    pub y: u32,
    pub item: Option<usize>,
}

/// The context menu's items (the architect's additions of 2026-11-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Open,
    OpenInEdit,
    Rename,
    Delete,
    NewFolder,
    CopyPath,
    OpenTerminal,
}

pub const MENU: [MenuItem; 7] = [
    MenuItem::Open,
    MenuItem::OpenInEdit,
    MenuItem::Rename,
    MenuItem::Delete,
    MenuItem::NewFolder,
    MenuItem::CopyPath,
    MenuItem::OpenTerminal,
];

impl MenuItem {
    pub fn label(self) -> &'static str {
        match self {
            MenuItem::Open => "Open",
            MenuItem::OpenInEdit => "Open in edit",
            MenuItem::Rename => "Rename",
            MenuItem::Delete => "Delete",
            MenuItem::NewFolder => "New folder",
            MenuItem::CopyPath => "Copy path",
            MenuItem::OpenTerminal => "Open terminal here",
        }
    }

    /// The key that does the same, as the menu shows it.
    pub fn key(self) -> &'static str {
        match self {
            MenuItem::Open => "Enter",
            MenuItem::Rename => "F2",
            MenuItem::Delete => "Delete",
            MenuItem::NewFolder => "\u{2318}\u{21e7}N",
            _ => "",
        }
    }

    /// A separator above it.
    pub fn starts_group(self) -> bool {
        matches!(
            self,
            MenuItem::Rename | MenuItem::NewFolder | MenuItem::CopyPath
        )
    }
}

/// What a gesture asks the program to do. Paths are absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// List this folder and show it ([`Model::show`]).
    List(String),
    /// Open a file through the panel with the application for its name.
    Open(String),
    /// Open a file in this application through the panel.
    OpenIn {
        path: String,
        app: &'static str,
    },
    MakeDirectory {
        dir: String,
        name: String,
    },
    Rename {
        dir: String,
        old: String,
        new: String,
    },
    /// Remove these names of `dir`, asked and confirmed.
    Remove {
        dir: String,
        names: Vec<String>,
    },
    /// Offer this text to the clipboard (proposed ADR-0248).
    CopyPath(String),
    /// A terminal in this folder, in the window's room.
    OpenTerminal(String),
    /// Copy (or move) these paths into the folder `target`.
    Transfer {
        sources: Vec<String>,
        target: String,
        copy: bool,
    },
    /// Start a drag of this file (ADR-0170): a drop on another window
    /// grants it, a drop on one of this window's folders copies or moves.
    Drag {
        path: String,
        name: String,
    },
}

/// The file manager's state.
#[derive(Debug, Clone)]
pub struct Model {
    /// The folder shown, absolute.
    pub dir: String,
    /// The person's home (`shell.home`), for `~`.
    pub home: String,
    /// The room the window runs in (`None` is `home`), for the menu's lines.
    pub room: Option<String>,
    pub places: Vec<Place>,
    /// Everything the folder holds, as listed.
    pub items: Vec<Item>,
    /// The items shown: filtered by the search, sorted; indices of `items`.
    view: Vec<usize>,
    pub sort: Column,
    pub ascending: bool,
    /// One flag per item.
    selected: Vec<bool>,
    /// The list's first row shown (the view keeps the cursor in sight).
    pub top: usize,
    /// View positions.
    pub anchor: Option<usize>,
    pub cursor: Option<usize>,
    back: Vec<String>,
    forward: Vec<String>,
    pub search: String,
    pub focus: Focus,
    pub rename: Option<Rename>,
    /// The names a Delete asks about, until confirmed or cancelled.
    pub confirm: Option<Vec<String>>,
    pub menu: Option<Menu>,
    /// The show-commands mode (System+?).
    pub commands: bool,
    /// The volume's free bytes, when known.
    pub free: Option<u64>,
    /// Now, in nanoseconds since the epoch, for "Today" and "Yesterday".
    pub now: u64,
    /// The last outcome to say (a failure, a deletion), until the next
    /// gesture.
    pub said: Option<String>,
    pub acts: VecDeque<Act>,
}

fn lower(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// Whether `outer` covers `path` (absolute, normal).
fn covers(outer: &str, path: &str) -> bool {
    outer == "/"
        || path == outer
        || path
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl Model {
    pub fn new(dir: &str, home: &str, room: Option<&str>, places: Vec<Place>) -> Model {
        Model {
            dir: String::from(dir),
            home: String::from(home),
            room: room.map(String::from),
            places,
            items: Vec::new(),
            view: Vec::new(),
            sort: Column::Name,
            ascending: true,
            selected: Vec::new(),
            top: 0,
            anchor: None,
            cursor: None,
            back: Vec::new(),
            forward: Vec::new(),
            search: String::new(),
            focus: Focus::List,
            rename: None,
            confirm: None,
            menu: None,
            commands: false,
            free: None,
            now: 0,
            said: None,
            acts: VecDeque::new(),
        }
    }

    // ---- the folder ------------------------------------------------------

    /// The folder `dir` with what it holds. The same folder again keeps
    /// what was selected, by name.
    pub fn show(&mut self, dir: &str, items: Vec<Item>) {
        let keep: Vec<String> = if dir == self.dir {
            self.selected_names()
        } else {
            self.search.clear();
            Vec::new()
        };
        let cursor_name = self.cursor_item().map(|i| self.items[i].name.clone());
        if dir != self.dir {
            self.top = 0;
        }
        self.dir = String::from(dir);
        self.items = items;
        self.selected = self
            .items
            .iter()
            .map(|it| keep.contains(&it.name))
            .collect();
        self.rename = None;
        self.confirm = None;
        self.menu = None;
        self.refilter();
        self.cursor = cursor_name
            .and_then(|n| self.position_of(&n))
            .filter(|_| dir == self.dir);
        self.anchor = self.cursor;
    }

    /// Selects `name` alone (a folder just made, a file just renamed).
    pub fn select_name(&mut self, name: &str) {
        if let Some(p) = self.position_of(name) {
            self.select_only(p);
        }
    }

    fn position_of(&self, name: &str) -> Option<usize> {
        self.view.iter().position(|i| self.items[*i].name == name)
    }

    /// The items shown, as indices of `items`.
    pub fn view(&self) -> &[usize] {
        &self.view
    }

    pub fn item_at(&self, pos: usize) -> Option<&Item> {
        self.view.get(pos).map(|i| &self.items[*i])
    }

    fn cursor_item(&self) -> Option<usize> {
        self.cursor.and_then(|p| self.view.get(p).copied())
    }

    /// Filters by the search and sorts: folders first, then by the column.
    fn refilter(&mut self) {
        let needle = lower(&self.search);
        let mut view: Vec<usize> = (0..self.items.len())
            .filter(|i| needle.is_empty() || lower(&self.items[*i].name).contains(&needle))
            .collect();
        let items = &self.items;
        let (sort, up) = (self.sort, self.ascending);
        view.sort_by(|a, b| {
            let (x, y) = (&items[*a], &items[*b]);
            let by = match sort {
                Column::Name => lower(&x.name).cmp(&lower(&y.name)),
                Column::Size => x.size.cmp(&y.size),
                Column::Modified => x.modified.cmp(&y.modified),
            };
            // Folders first; the column either way; equal ones by name.
            y.dir
                .cmp(&x.dir)
                .then(if up { by } else { by.reverse() })
                .then_with(|| x.name.cmp(&y.name))
        });
        self.view = view;
        if self.cursor.is_some_and(|c| c >= self.view.len()) {
            self.cursor = None;
        }
    }

    /// A click on a column's header: sort by it, again to reverse; a new
    /// column starts newest or largest first, the name A to Z.
    pub fn sort_by(&mut self, column: Column) {
        let keep: Vec<String> = self.selected_names();
        let cursor = self.cursor_item().map(|i| self.items[i].name.clone());
        if self.sort == column {
            self.ascending = !self.ascending;
        } else {
            self.sort = column;
            self.ascending = column == Column::Name;
        }
        self.refilter();
        for (i, it) in self.items.iter().enumerate() {
            self.selected[i] = keep.contains(&it.name);
        }
        self.cursor = cursor.and_then(|n| self.position_of(&n));
        self.anchor = self.cursor;
        self.said = None;
    }

    // ---- selection -------------------------------------------------------

    pub fn is_selected(&self, pos: usize) -> bool {
        self.view
            .get(pos)
            .is_some_and(|i| self.selected.get(*i).copied().unwrap_or(false))
    }

    /// The selected items, in the order shown.
    pub fn selection(&self) -> Vec<usize> {
        self.view
            .iter()
            .copied()
            .filter(|i| self.selected[*i])
            .collect()
    }

    pub fn selected_names(&self) -> Vec<String> {
        self.selection()
            .into_iter()
            .map(|i| self.items[i].name.clone())
            .collect()
    }

    fn select_only(&mut self, pos: usize) {
        self.selected.fill(false);
        if let Some(i) = self.view.get(pos) {
            self.selected[*i] = true;
        }
        self.anchor = Some(pos);
        self.cursor = Some(pos);
    }

    fn select_range(&mut self, a: usize, b: usize) {
        self.selected.fill(false);
        let (lo, hi) = (a.min(b), a.max(b));
        for p in lo..=hi.min(self.view.len().saturating_sub(1)) {
            let i = self.view[p];
            self.selected[i] = true;
        }
        self.cursor = Some(b);
    }

    /// A click on the row at view position `pos`: alone; with Shift, the
    /// range from the anchor; with Command or Ctrl, added or taken away.
    pub fn click(&mut self, pos: usize, modifiers: u32) {
        self.said = None;
        self.menu = None;
        self.commit_rename();
        self.focus = Focus::List;
        if pos >= self.view.len() {
            if modifiers & (mods::SHIFT | mods::COMMAND) == 0 {
                self.selected.fill(false);
                self.cursor = None;
                self.anchor = None;
            }
            return;
        }
        if modifiers & mods::SHIFT != 0 {
            let a = self.anchor.unwrap_or(pos);
            self.anchor = Some(a);
            self.select_range(a, pos);
        } else if modifiers & mods::COMMAND != 0 {
            let i = self.view[pos];
            self.selected[i] = !self.selected[i];
            self.anchor = Some(pos);
            self.cursor = Some(pos);
        } else {
            self.select_only(pos);
        }
    }

    /// A double click on, or Enter with the cursor on, the row at `pos`: a
    /// folder is entered, a file opened.
    pub fn activate(&mut self, pos: usize) {
        self.said = None;
        let Some(it) = self.item_at(pos) else { return };
        let path = join(&self.dir, &it.name);
        if it.dir {
            self.navigate(&path);
        } else {
            self.acts.push_back(Act::Open(path));
        }
    }

    // ---- moving about ----------------------------------------------------

    /// Goes to `dir`: back remembers where from, forward is forgotten.
    pub fn navigate(&mut self, dir: &str) {
        if dir == self.dir {
            return;
        }
        self.back.push(self.dir.clone());
        self.forward.clear();
        self.go(dir);
    }

    fn go(&mut self, dir: &str) {
        self.menu = None;
        self.rename = None;
        self.confirm = None;
        self.acts.push_back(Act::List(String::from(dir)));
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn back(&mut self) {
        if let Some(d) = self.back.pop() {
            self.forward.push(self.dir.clone());
            self.go(&d);
        }
    }

    pub fn forward(&mut self) {
        if let Some(d) = self.forward.pop() {
            self.back.push(self.dir.clone());
            self.go(&d);
        }
    }

    pub fn up(&mut self) {
        if let Some(p) = parent(&self.dir) {
            self.navigate(&p);
        }
    }

    /// The path as crumbs: labels and the folder each goes to, from `~` when
    /// under the home, else from `/`.
    pub fn crumbs(&self) -> Vec<(String, String)> {
        let (first, base, rest) = match self.dir.strip_prefix(&self.home) {
            Some(r) if r.is_empty() || r.starts_with('/') => ("~", self.home.clone(), r),
            _ => ("/", String::from("/"), self.dir.as_str()),
        };
        let mut out = vec![(String::from(first), base.clone())];
        let mut at = base;
        for c in rest.split('/').filter(|c| !c.is_empty()) {
            at = join(&at, c);
            out.push((String::from(c), at.clone()));
        }
        out
    }

    /// The place that serves the folder shown (the longest that covers it).
    pub fn place_of(&self, dir: &str) -> Option<usize> {
        self.places
            .iter()
            .enumerate()
            .filter(|(_, p)| covers(&p.path, dir))
            .max_by_key(|(_, p)| p.path.len())
            .map(|(i, _)| i)
    }

    /// Whether the window may change what is in `dir`.
    pub fn can_write(&self, dir: &str) -> bool {
        // The longest place among all, as the namespace resolves it.
        self.place_of(dir).is_some_and(|i| self.places[i].write)
    }

    /// `path` as the shell writes it (`~/project/notes.txt`).
    pub fn shown(&self, path: &str) -> String {
        match path.strip_prefix(&self.home) {
            Some("") => String::from("~"),
            Some(r) if r.starts_with('/') => format!("~{r}"),
            _ => String::from(path),
        }
    }

    // ---- the keyboard ----------------------------------------------------

    /// A key: in the rename field, the search field, the confirmation, the
    /// menu, or the list.
    pub fn key(&mut self, code: u32, text: Option<char>, modifiers: u32) {
        let command = modifiers & mods::COMMAND != 0;
        let shift = modifiers & mods::SHIFT != 0;
        if self.rename.is_some() {
            self.rename_key(code, text);
            return;
        }
        if let Some(names) = self.confirm.clone() {
            match code {
                keys::ENTER => {
                    self.confirm = None;
                    self.acts.push_back(Act::Remove {
                        dir: self.dir.clone(),
                        names,
                    });
                }
                keys::ESC => self.confirm = None,
                _ => {}
            }
            return;
        }
        if self.menu.is_some() {
            if code == keys::ESC {
                self.menu = None;
            }
            return;
        }
        // Chords, wherever the focus is.
        if command && code == keys::UP {
            self.up();
            return;
        }
        if command && shift && code == keys::N {
            self.new_folder();
            return;
        }
        if command && code == keys::F {
            self.focus = Focus::Search;
            return;
        }
        if self.focus == Focus::Search {
            self.search_key(code, text);
            return;
        }
        if command {
            return;
        }
        let n = self.view.len();
        match code {
            keys::DOWN | keys::UP if n > 0 => {
                let next = match (self.cursor, code) {
                    (None, keys::DOWN) => 0,
                    (None, _) => n - 1,
                    (Some(c), keys::DOWN) => (c + 1).min(n - 1),
                    (Some(c), _) => c.saturating_sub(1),
                };
                if shift {
                    let a = self.anchor.unwrap_or(self.cursor.unwrap_or(next));
                    self.anchor = Some(a);
                    self.select_range(a, next);
                } else {
                    self.select_only(next);
                }
            }
            keys::HOME if n > 0 => self.select_only(0),
            keys::END if n > 0 => self.select_only(n - 1),
            keys::ENTER => {
                if let Some(c) = self.cursor {
                    self.activate(c);
                }
            }
            keys::DELETE => self.ask_delete(),
            keys::F2 => self.start_rename(),
            keys::BACKSPACE => self.up(),
            keys::ESC => {
                self.selected.fill(false);
            }
            _ => {
                // Typing in the list starts a search.
                if let Some(t) = text.filter(|t| !t.is_control()) {
                    self.focus = Focus::Search;
                    self.search.push(t);
                    self.refilter();
                }
            }
        }
    }

    fn search_key(&mut self, code: u32, text: Option<char>) {
        match code {
            keys::ESC => {
                self.search.clear();
                self.focus = Focus::List;
            }
            keys::ENTER | keys::DOWN | keys::TAB => {
                self.focus = Focus::List;
                if !self.view.is_empty() {
                    self.select_only(0);
                }
                return;
            }
            keys::BACKSPACE => {
                self.search.pop();
            }
            _ => match text.filter(|t| !t.is_control()) {
                Some(t) if self.search.len() < MAX_NAME => self.search.push(t),
                _ => return,
            },
        }
        self.refilter();
    }

    // ---- rename in place -------------------------------------------------

    /// F2 or the menu's Rename: a field over the cursor's (or the one
    /// selected) item's name, its stem selected.
    pub fn start_rename(&mut self) {
        self.said = None;
        let item = self.cursor_item().or_else(|| {
            let s = self.selection();
            (s.len() == 1).then(|| s[0])
        });
        let Some(item) = item else { return };
        if !self.can_write(&self.dir) {
            self.said = Some(format!("{} is read only here.", self.shown(&self.dir)));
            return;
        }
        let text = self.items[item].name.clone();
        let stem = match text.rsplit_once('.') {
            Some((s, _)) if !s.is_empty() && !self.items[item].dir => s.chars().count(),
            _ => text.chars().count(),
        };
        self.rename = Some(Rename {
            item,
            text,
            cursor: stem,
            stem_selected: true,
        });
    }

    fn rename_key(&mut self, code: u32, text: Option<char>) {
        let Some(mut r) = self.rename.take() else {
            return;
        };
        let len = r.text.chars().count();
        let byte = |s: &str, c: usize| s.char_indices().nth(c).map_or(s.len(), |(b, _)| b);
        match code {
            keys::ESC => return,
            keys::ENTER => {
                self.rename = Some(r);
                self.commit_rename();
                return;
            }
            keys::LEFT => {
                r.cursor = r.cursor.saturating_sub(1);
                r.stem_selected = false;
            }
            keys::RIGHT => {
                r.cursor = (r.cursor + 1).min(len);
                r.stem_selected = false;
            }
            keys::HOME => {
                r.cursor = 0;
                r.stem_selected = false;
            }
            keys::END => {
                r.cursor = len;
                r.stem_selected = false;
            }
            keys::BACKSPACE | keys::DELETE if r.stem_selected => {
                let at = byte(&r.text, r.cursor);
                r.text.replace_range(..at, "");
                r.cursor = 0;
                r.stem_selected = false;
            }
            keys::BACKSPACE if r.cursor > 0 => {
                let at = byte(&r.text, r.cursor - 1);
                let to = byte(&r.text, r.cursor);
                r.text.replace_range(at..to, "");
                r.cursor -= 1;
            }
            keys::DELETE if r.cursor < len => {
                let at = byte(&r.text, r.cursor);
                let to = byte(&r.text, r.cursor + 1);
                r.text.replace_range(at..to, "");
            }
            _ => {
                if let Some(t) = text.filter(|t| !t.is_control() && *t != '/') {
                    if r.stem_selected {
                        let at = byte(&r.text, r.cursor);
                        r.text.replace_range(..at, "");
                        r.cursor = 0;
                        r.stem_selected = false;
                    }
                    if r.text.len() + t.len_utf8() <= MAX_NAME {
                        let at = byte(&r.text, r.cursor);
                        r.text.insert(at, t);
                        r.cursor += 1;
                    }
                }
            }
        }
        self.rename = Some(r);
    }

    /// Keeps the rename's name, if it changed and is a name.
    pub fn commit_rename(&mut self) {
        let Some(r) = self.rename.take() else { return };
        let old = self.items[r.item].name.clone();
        let new = String::from(r.text.trim());
        if new == old {
            return;
        }
        if new.is_empty() || new == "." || new == ".." || new.contains('/') {
            self.said = Some(format!("\u{201c}{new}\u{201d} is not a name."));
            return;
        }
        if self.items.iter().any(|it| it.name == new) {
            self.said = Some(format!("{new} is there already."));
            return;
        }
        self.acts.push_back(Act::Rename {
            dir: self.dir.clone(),
            old,
            new,
        });
    }

    // ---- delete, new folder ---------------------------------------------

    /// Delete or the menu's Delete: the confirmation, naming what goes.
    pub fn ask_delete(&mut self) {
        self.said = None;
        let names = self.selected_names();
        if names.is_empty() {
            return;
        }
        if !self.can_write(&self.dir) {
            self.said = Some(format!("{} is read only here.", self.shown(&self.dir)));
            return;
        }
        self.confirm = Some(names);
    }

    /// The card's question and its line of names.
    pub fn confirm_text(&self) -> Option<(String, String)> {
        let names = self.confirm.as_ref()?;
        let heading = if names.len() == 1 {
            format!("Delete {}?", names[0])
        } else {
            format!("Delete {} items?", names.len())
        };
        let line = match names.len() {
            1 => String::from("It will be deleted."),
            2 => format!("{} and {} will be deleted.", names[0], names[1]),
            n => format!(
                "{}, {} and {} more will be deleted.",
                names[0],
                names[1],
                n - 2
            ),
        };
        Some((heading, line))
    }

    pub fn answer_delete(&mut self, yes: bool) {
        let Some(names) = self.confirm.take() else {
            return;
        };
        if yes {
            self.acts.push_back(Act::Remove {
                dir: self.dir.clone(),
                names,
            });
        }
    }

    /// Command+Shift+N or the menu's New folder: "New folder", or the first
    /// "New folder N" not taken.
    pub fn new_folder(&mut self) {
        self.said = None;
        self.menu = None;
        if !self.can_write(&self.dir) {
            self.said = Some(format!("{} is read only here.", self.shown(&self.dir)));
            return;
        }
        let taken = |n: &str| self.items.iter().any(|it| it.name == n);
        let mut name = String::from(NEW_FOLDER);
        let mut k = 2;
        while taken(&name) {
            name = format!("{NEW_FOLDER} {k}");
            k += 1;
        }
        self.acts.push_back(Act::MakeDirectory {
            dir: self.dir.clone(),
            name,
        });
    }

    // ---- the context menu -----------------------------------------------

    /// A right-click at `(x, y)` on the row at `pos` (or on no row): the
    /// row is selected unless it already is, and the menu opens.
    pub fn open_menu(&mut self, x: u32, y: u32, pos: Option<usize>) {
        self.commit_rename();
        self.said = None;
        if let Some(p) = pos.filter(|p| *p < self.view.len()) {
            if !self.is_selected(p) {
                self.select_only(p);
            }
            self.cursor = Some(p);
        }
        self.menu = Some(Menu {
            x,
            y,
            item: pos.and_then(|p| self.view.get(p).copied()),
        });
    }

    /// Whether an item of the menu can be chosen now.
    pub fn menu_enabled(&self, m: MenuItem) -> bool {
        let on = self.menu.and_then(|m| m.item);
        let write = self.can_write(&self.dir);
        match m {
            MenuItem::Open | MenuItem::CopyPath => on.is_some(),
            MenuItem::OpenInEdit => on.is_some_and(|i| !self.items[i].dir),
            MenuItem::Rename => on.is_some() && write,
            MenuItem::Delete => on.is_some() && write,
            MenuItem::NewFolder => write,
            MenuItem::OpenTerminal => true,
        }
    }

    /// The `zosh` line that does what the item does (ADR-0172 point 8), as
    /// show-commands mode writes it in the menu.
    pub fn menu_line(&self, m: MenuItem) -> String {
        let target = self
            .menu
            .and_then(|m| m.item)
            .map(|i| self.shown(&join(&self.dir, &self.items[i].name)));
        let here = self.shown(&self.dir);
        let t = target.clone().unwrap_or_else(|| here.clone());
        let dir_of = target
            .as_ref()
            .and(self.menu.and_then(|m| m.item))
            .is_some_and(|i| self.items[i].dir);
        match m {
            MenuItem::Open if dir_of => format!("cd {t}"),
            MenuItem::Open | MenuItem::OpenInEdit => format!("edit {t}"),
            MenuItem::Rename => format!("mv {t} {}", self.shown(&join(&self.dir, "NAME"))),
            MenuItem::Delete => {
                if dir_of {
                    format!("rm --recursive {t}")
                } else {
                    format!("rm {t}")
                }
            }
            MenuItem::NewFolder => format!("mkdir {}", self.shown(&join(&self.dir, "NAME"))),
            MenuItem::CopyPath => String::from("(the clipboard: no zosh form)"),
            MenuItem::OpenTerminal => match &self.room {
                Some(r) => format!("room {r} term \u{b7} cd {here}"),
                None => format!("term \u{b7} cd {here}"),
            },
        }
    }

    /// The person chose `m` in the menu.
    pub fn choose(&mut self, m: MenuItem) {
        if !self.menu_enabled(m) {
            return;
        }
        let item = self.menu.and_then(|m| m.item);
        self.menu = None;
        let path = item.map(|i| join(&self.dir, &self.items[i].name));
        match m {
            MenuItem::Open => {
                if let Some(p) = item.and_then(|i| self.position_of(&self.items[i].name.clone())) {
                    self.activate(p);
                }
            }
            MenuItem::OpenInEdit => {
                if let Some(path) = path {
                    self.acts.push_back(Act::OpenIn { path, app: "edit" });
                }
            }
            MenuItem::Rename => self.start_rename(),
            MenuItem::Delete => self.ask_delete(),
            MenuItem::NewFolder => self.new_folder(),
            MenuItem::CopyPath => {
                if let Some(path) = path {
                    self.acts.push_back(Act::CopyPath(self.shown(&path)));
                }
            }
            MenuItem::OpenTerminal => {
                let dir = match item {
                    Some(i) if self.items[i].dir => join(&self.dir, &self.items[i].name),
                    _ => self.dir.clone(),
                };
                self.acts.push_back(Act::OpenTerminal(dir));
            }
        }
    }

    // ---- drags -----------------------------------------------------------

    /// A drag starts from the row at `pos`: that file is carried (a folder
    /// is not, ADR-0170's drop is a file).
    pub fn start_drag(&mut self, pos: usize) {
        let Some(it) = self.item_at(pos) else { return };
        let path = join(&self.dir, &it.name);
        let name = it.name.clone();
        if !self.is_selected(pos) {
            self.select_only(pos);
        }
        self.acts.push_back(Act::Drag { path, name });
    }

    /// A drag of this window ended over its own row at `pos` (or none):
    /// on a folder, what is selected is copied there with Alt or from a
    /// place the window may not change or into another place, else moved.
    pub fn dropped_here(&mut self, pos: Option<usize>, modifiers: u32) {
        let Some(target) = pos.and_then(|p| self.item_at(p)).filter(|it| it.dir) else {
            return;
        };
        let target = join(&self.dir, &target.name);
        let sources: Vec<String> = self
            .selected_names()
            .into_iter()
            .map(|n| join(&self.dir, &n))
            .filter(|s| *s != target)
            .collect();
        if sources.is_empty() {
            return;
        }
        if !self.can_write(&target) {
            self.said = Some(format!("{} is read only here.", self.shown(&target)));
            return;
        }
        let same_place = self.place_of(&self.dir) == self.place_of(&target);
        let copy = modifiers & mods::ALT != 0 || !same_place || !self.can_write(&self.dir);
        self.acts.push_back(Act::Transfer {
            sources,
            target,
            copy,
        });
    }

    // ---- the status line -------------------------------------------------

    /// The status line's left part: what is shown and selected, or the last
    /// outcome.
    pub fn status(&self) -> String {
        if let Some(s) = &self.said {
            return s.clone();
        }
        if let Some(r) = &self.rename {
            let _ = r;
            return String::from("Enter keeps the name \u{b7} Escape leaves it");
        }
        let n = self.view.len();
        let items = if n == 1 {
            String::from("1 item")
        } else {
            format!("{n} items")
        };
        let items = if self.search.is_empty() {
            items
        } else {
            format!("{items} matching \u{201c}{}\u{201d}", self.search)
        };
        let sel = self.selection();
        let bytes: u64 = sel.iter().map(|i| self.items[*i].size).sum();
        match sel.len() {
            0 => items,
            1 => format!(
                "{items} \u{b7} {} selected ({})",
                self.items[sel[0]].name,
                size(bytes)
            ),
            k => format!("{items} \u{b7} {k} selected ({})", size(bytes)),
        }
    }

    /// The status line's right part.
    pub fn free_text(&self) -> String {
        match self.free {
            Some(f) => format!("{} free", size(f)),
            None => String::new(),
        }
    }

    /// An item's columns as the list shows them: size and modified.
    pub fn columns(&self, item: usize) -> (String, String) {
        let it = &self.items[item];
        let s = if it.dir {
            String::from("\u{2014}")
        } else {
            size(it.size)
        };
        (s, date(it.modified, self.now))
    }
}

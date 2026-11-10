//! The file manager (stage 7h, the approved mockups): `zoraya-files` holds
//! the model and draws the window; this program lists folders and carries
//! out each `Act` through `zoraya-fsclient`, opens files only through the
//! panel (`OpenWith` with one read-only connection, ADR-0168 point 4), asks
//! the panel for a terminal at a folder (`OpenTerminal`), offers a path to
//! the clipboard, and starts drags (ADR-0170).
//!
//! Launched by the panel (`zoraya_panel::application`): slot 1 the
//! console, 5 its compositor channel, 6 its chooser channel (unused), 7 its
//! data directory, 9 its channel to the panel, and its start-up record: the
//! volume to read and, to change, `~` in `home` or the room's grants
//! (INV-0168-2 as changed on 2026-11-10; never `/config`, INV-0169-4); with
//! `--room NAME` in a room. Each place's rights are found by asking for a
//! connection that may write.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use zoraya_abi::{Handle, ObjectType};
use zoraya_app::{App, Happened};
use zoraya_files::model::Model;
use zoraya_files::view::{FilesView, row_at};
use zoraya_files::{Act, Item, join};
use zoraya_fsclient::Client;
use zoraya_proto_chooser as pc;
use zoraya_proto_file::{kind, open_flags, rights};
use zoraya_protocol::{Data, decode, encode};
use zoraya_rt::{channel_read, channel_write, clock_monotonic, handle_close, handle_type, println};
use zoraya_tokens::Appearance;
use zoraya_ui::Response;
use zoraya_user::app::{Files, Gui};

zoraya_user::describe!("system", "files");

/// The view's widget id.
const VIEW: u32 = 1;
/// The window's logical size (the mockup's 896 by 600 frame); the
/// compositor makes it smaller where the work area is.
const WIDTH: u32 = 880;
const HEIGHT: u32 = 536;
/// The most names a folder shows.
const MAX_LISTED: usize = 2048;
/// The most entries one copy or one delete walks, and how deep.
const MAX_TREE: usize = 4096;
const MAX_DEPTH: usize = 32;
/// The rights to change what is in a folder: ADR-0153's `tree`.
const TREE: u32 = rights::ALL & !rights::PUBLISH;

type Status = zoraya_fsclient::Status;

/// The places the start-up record gives, absolute, with their connections
/// and whether each may change what is in it.
struct Places(Vec<(String, Handle, bool)>);

impl Places {
    fn take(fs: &mut Client<Files>) -> Places {
        let mut v = Vec::new();
        if let Some(ns) = zoraya_user::namespace::take() {
            for e in ns.entries {
                if let Some(f) = e.file {
                    let h = f.end;
                    core::mem::forget(f);
                    // A connection that may write: the place allows it.
                    let write = match fs.clone_with(h, rights::READ | rights::WRITE | rights::CLONE)
                    {
                        Ok(w) => {
                            fs.close(w);
                            true
                        }
                        Err(_) => false,
                    };
                    v.push((e.path, h, write));
                }
            }
        }
        Places(v)
    }

    /// The volume's root, read only.
    fn root(&self) -> Option<Handle> {
        self.0.iter().find(|(p, _, _)| p == "/").map(|(_, h, _)| *h)
    }

    fn open(
        &self,
        fs: &mut Client<Files>,
        path: &str,
        flags: u32,
        wanted: u32,
    ) -> Result<Handle, Status> {
        let (i, rest) = zoraya_resolve::longest(self.0.iter().map(|(p, _, _)| p.as_str()), path)
            .ok_or(zoraya_proto_file::status::ACCESS_DENIED)?;
        fs.open(self.0[i].1, rest, flags, wanted)
    }

    fn holds(&self) -> Vec<(String, bool)> {
        self.0.iter().map(|(p, _, w)| (p.clone(), *w)).collect()
    }
}

/// The folder `dir`'s items with their sizes and times, or why not.
fn list(fs: &mut Client<Files>, places: &Places, dir: &str) -> Result<Vec<Item>, String> {
    let d = places
        .open(fs, dir, open_flags::DIRECTORY, rights::READ)
        .map_err(|e| format!("{dir}: cannot open ({e})"))?;
    let listed = match fs.list(d) {
        Ok(l) => l,
        Err(e) => {
            fs.close(d);
            return Err(format!("{dir}: cannot list ({e})"));
        }
    };
    let mut items = Vec::new();
    for l in listed.into_iter().take(MAX_LISTED) {
        let (size, modified) = match fs.open(d, &l.name, 0, rights::READ) {
            Ok(f) => {
                let info = fs.info(f).map(|(s, _, m)| (s, m)).unwrap_or((0, 0));
                fs.close(f);
                info
            }
            Err(_) => (0, 0),
        };
        let dir = l.is_dir();
        items.push(Item {
            name: l.name,
            dir,
            size: if dir { 0 } else { size },
            modified,
        });
    }
    fs.close(d);
    Ok(items)
}

/// One request to the panel and its status (`u32::MAX` if none came).
fn ask_panel<'a, M: zoraya_protocol::Message<'a>>(
    panel: Handle,
    m: &M,
    handle: Option<Handle>,
) -> u32 {
    let mut out = vec![0u8; 2048];
    let Ok(n) = encode(m, 1, &mut out) else {
        if let Some(h) = handle {
            let _ = handle_close(h);
        }
        return u32::MAX;
    };
    let entries: Vec<zoraya_abi::ChannelEntry> = handle
        .into_iter()
        .map(|h| zoraya_abi::ChannelEntry {
            handle: h,
            rights: zoraya_abi::ChannelObject::RIGHTS,
            transfer: zoraya_abi::spawn::Transfer::Move,
        })
        .collect();
    if channel_write(panel, &out[..n], &entries).is_err() {
        if let Some(h) = handle {
            let _ = handle_close(h);
        }
        return u32::MAX;
    }
    let mut bytes = [0u8; 64];
    let mut handles = [panel; 1];
    loop {
        match channel_read(panel, &mut bytes, &mut handles) {
            Ok(s) => {
                let msg = &bytes[..s.bytes as usize];
                // Opening and TerminalOpened carry the same status.
                break decode::<pc::Opening>(msg, 0)
                    .map(|(_, o)| o.status.0)
                    .or_else(|_| decode::<pc::TerminalOpened>(msg, 0).map(|(_, o)| o.status.0))
                    .unwrap_or(u32::MAX);
            }
            Err((zoraya_abi::Status::WouldBlock, _)) => {
                if !zoraya_user::app::wait(panel) {
                    break u32::MAX;
                }
            }
            Err(_) => break u32::MAX,
        }
    }
}

/// Copies `name` of the folder `from` (a file, or a folder whole) into the
/// folder `to`; counts what it walked.
fn copy_tree(
    fs: &mut Client<Files>,
    from: Handle,
    to: Handle,
    name: &str,
    depth: usize,
    walked: &mut usize,
) -> Result<(), Status> {
    *walked += 1;
    if *walked > MAX_TREE || depth > MAX_DEPTH {
        return Err(zoraya_proto_file::status::LIMIT_REACHED);
    }
    let src = fs.open(from, name, 0, rights::READ)?;
    let kind_of = fs.info(src).map(|(_, k, _)| k);
    fs.close(src);
    if kind_of == Ok(kind::DIRECTORY) {
        let src = fs.open(from, name, open_flags::DIRECTORY, rights::READ)?;
        let made = fs.make_directory(to, name);
        let r = made.and_then(|()| {
            let dst = fs.open(to, name, open_flags::DIRECTORY, TREE)?;
            let names = fs.list(src).unwrap_or_default();
            let mut r = Ok(());
            for n in names {
                r = copy_tree(fs, src, dst, &n.name, depth + 1, walked);
                if r.is_err() {
                    break;
                }
            }
            fs.close(dst);
            r
        });
        fs.close(src);
        return r;
    }
    let src = fs.open(from, name, 0, rights::READ)?;
    let bytes = fs.read_all(src);
    fs.close(src);
    let bytes = bytes?;
    let dst = fs.open(
        to,
        name,
        open_flags::CREATE | open_flags::EXCLUSIVE,
        rights::READ | rights::WRITE,
    )?;
    let r = fs.write_all(dst, &bytes);
    fs.close(dst);
    r
}

/// Removes `name` of `dir`, a folder with everything in it.
fn remove_tree(
    fs: &mut Client<Files>,
    dir: Handle,
    name: &str,
    depth: usize,
    walked: &mut usize,
) -> Result<(), Status> {
    *walked += 1;
    if *walked > MAX_TREE || depth > MAX_DEPTH {
        return Err(zoraya_proto_file::status::LIMIT_REACHED);
    }
    if let Ok(sub) = fs.open(dir, name, open_flags::DIRECTORY, TREE) {
        let names = fs.list(sub).unwrap_or_default();
        let mut r = Ok(());
        for n in names {
            r = remove_tree(fs, sub, &n.name, depth + 1, walked);
            if r.is_err() {
                break;
            }
        }
        fs.close(sub);
        r?;
    }
    fs.remove(dir, name)
}

struct FileManager {
    app: App<Gui>,
    fs: Client<Files>,
    places: Places,
    panel: Handle,
    model: Rc<RefCell<Model>>,
    window: u32,
    /// What the serial line was last told: the status line and the sort.
    told: Told,
}

/// What the serial line was last told, for the GUI tests: they wait for
/// each answer before the next key, which goes by another device.
#[derive(Default)]
struct Told {
    status: String,
    order: String,
    keys: &'static str,
    menu: bool,
    asks: String,
}

impl FileManager {
    /// Tells the serial line a new status line or a new order, for the GUI
    /// tests (the window's text is not read from the screen).
    fn report(&mut self) {
        let (status, order, keys, menu, asks) = {
            let m = self.model.borrow();
            let col = match m.sort {
                zoraya_files::Column::Name => "name",
                zoraya_files::Column::Size => "size",
                zoraya_files::Column::Modified => "modified",
            };
            let names: Vec<&str> = m.view().iter().map(|i| m.items[*i].name.as_str()).collect();
            let dir = if m.ascending { "up" } else { "down" };
            (
                m.status(),
                format!("{col} {dir}: {}", names.join(" ")),
                match m.focus {
                    zoraya_files::model::Focus::List => "list",
                    zoraya_files::model::Focus::Search => "search",
                },
                m.menu.is_some(),
                m.confirm_text().map(|c| c.0).unwrap_or_default(),
            )
        };
        if status != self.told.status {
            println!("files: status {status}");
            self.told.status = status;
        }
        if order != self.told.order {
            println!("files: sorted by {order}");
            self.told.order = order;
        }
        if keys != self.told.keys {
            println!("files: keys go to the {keys}");
            self.told.keys = keys;
        }
        if menu != self.told.menu {
            println!("files: menu {}", if menu { "open" } else { "closed" });
            self.told.menu = menu;
        }
        if asks != self.told.asks {
            if asks.is_empty() {
                println!("files: asks nothing");
            } else {
                println!("files: asks {asks}");
            }
            self.told.asks = asks;
        }
    }

    fn say(&mut self, s: String) {
        if !s.is_empty() {
            println!("files: {s}");
        }
        self.model.borrow_mut().said = Some(s);
    }

    /// Lists `dir` into the model; on a failure says why and stays.
    fn show(&mut self, dir: &str) -> bool {
        let start = clock_monotonic();
        match list(&mut self.fs, &self.places, dir) {
            Ok(items) => {
                // A folder's listing with each item's size and time (7h's
                // measurement, design.md).
                let us = clock_monotonic().saturating_sub(start) / 1000;
                println!("files: listed {} items in {us} us", items.len());
                let mut m = self.model.borrow_mut();
                m.show(dir, items);
                let names: Vec<String> = m
                    .view()
                    .iter()
                    .map(|i| {
                        let it = &m.items[*i];
                        if it.dir {
                            format!("{}/", it.name)
                        } else {
                            it.name.clone()
                        }
                    })
                    .collect();
                println!("files: {} lists {}", m.shown(dir), names.join(" "));
                true
            }
            Err(e) => {
                self.say(e);
                false
            }
        }
    }

    fn refresh_free(&mut self) {
        if let Some(root) = self.places.root() {
            self.model.borrow_mut().free = self.fs.free_space(root).ok();
        }
    }

    /// Hands `path` to the panel with one read-only connection, for `app`
    /// (empty: the application for its extension).
    fn open_with(&mut self, path: &str, app: &str) {
        let shown = self.model.borrow().shown(path);
        let file = match self
            .places
            .open(&mut self.fs, path, 0, rights::READ | rights::CLONE)
        {
            Ok(f) => f,
            Err(e) => return self.say(format!("{shown}: cannot open ({e})")),
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        let words = format!("+r {shown}");
        let msg = pc::OpenWith {
            name: Data(name.as_bytes()),
            words: Data(words.as_bytes()),
            app: Data(app.as_bytes()),
        };
        let s = match ask_panel(self.panel, &msg, Some(file)) {
            0 => {
                println!("files: opened {words}");
                format!("Opened {name}.")
            }
            _ => format!("No application opens {name}."),
        };
        self.say(s);
    }

    /// Opens the folder `dir` to change what is in it.
    fn folder(&mut self, dir: &str, wanted: u32) -> Result<Handle, Status> {
        self.places
            .open(&mut self.fs, dir, open_flags::DIRECTORY, wanted)
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::List(dir) => {
                self.show(&dir);
            }
            Act::Open(path) => self.open_with(&path, ""),
            Act::OpenIn { path, app } => self.open_with(&path, app),
            Act::MakeDirectory { dir, name } => {
                let r = self.folder(&dir, TREE).and_then(|d| {
                    let r = self.fs.make_directory(d, &name);
                    self.fs.close(d);
                    r
                });
                if self.show(&dir) {
                    match r {
                        Ok(()) => {
                            self.model.borrow_mut().select_name(&name);
                            self.say(format!("Made {name}."));
                        }
                        Err(e) => self.say(format!("Cannot make {name} ({e}).")),
                    }
                }
            }
            Act::Rename { dir, old, new } => {
                let r = self.folder(&dir, TREE).and_then(|d| {
                    let r = self.fs.rename(d, &old, &new);
                    self.fs.close(d);
                    r
                });
                if self.show(&dir) {
                    match r {
                        Ok(()) => {
                            self.model.borrow_mut().select_name(&new);
                            self.say(format!("Renamed {old} to {new}."));
                        }
                        Err(e) => self.say(format!("Cannot rename {old} ({e}).")),
                    }
                }
            }
            Act::Remove { dir, names } => {
                let mut gone = 0;
                let mut failed = None;
                match self.folder(&dir, TREE) {
                    Ok(d) => {
                        for n in &names {
                            let mut walked = 0;
                            match remove_tree(&mut self.fs, d, n, 0, &mut walked) {
                                Ok(()) => gone += 1,
                                Err(e) => failed = Some(format!("Cannot delete {n} ({e}).")),
                            }
                        }
                        self.fs.close(d);
                    }
                    Err(e) => failed = Some(format!("Cannot delete here ({e}).")),
                }
                if self.show(&dir) {
                    let s = failed.unwrap_or_else(|| {
                        if gone == 1 {
                            format!("Deleted {}.", names[0])
                        } else {
                            format!("Deleted {gone} items.")
                        }
                    });
                    self.say(s);
                }
                self.refresh_free();
            }
            Act::CopyPath(text) => {
                let s = if self.app.offer(&text) {
                    format!("Copied {text}.")
                } else {
                    String::from("The clipboard did not take it.")
                };
                self.say(s);
            }
            Act::OpenTerminal(dir) => {
                let m = pc::OpenTerminal {
                    path: Data(dir.as_bytes()),
                };
                let shown = self.model.borrow().shown(&dir);
                let s = match ask_panel(self.panel, &m, None) {
                    0 => format!("A terminal in {shown}."),
                    _ => format!("No terminal can open in {shown}."),
                };
                self.say(s);
            }
            Act::Transfer {
                sources,
                target,
                copy,
            } => self.transfer(&sources, &target, copy),
            Act::Drag { path, name } => {
                let Ok(file) =
                    self.places
                        .open(&mut self.fs, &path, 0, rights::READ | rights::CLONE)
                else {
                    return;
                };
                match self.app.drag(self.window, file, &name) {
                    Ok(()) => println!("files: dragging {name}"),
                    // Not sent: still ours to close.
                    Err(Some(f)) => self.fs.close(f),
                    Err(None) => {}
                }
            }
        }
    }

    /// Copies or moves `sources` into the folder `target`.
    fn transfer(&mut self, sources: &[String], target: &str, copy: bool) {
        let dir = self.model.borrow().dir.clone();
        let mut done = 0;
        let mut failed = None;
        let to = self.folder(target, TREE);
        for src in sources {
            let (parent, name) = src.rsplit_once('/').unwrap_or(("", src));
            let parent = if parent.is_empty() { "/" } else { parent };
            let wanted = if copy { rights::READ } else { TREE };
            let r = to.and_then(|t| {
                self.folder(parent, wanted).and_then(|from| {
                    let r = if copy {
                        let mut walked = 0;
                        copy_tree(&mut self.fs, from, t, name, 0, &mut walked)
                    } else {
                        self.fs.rename_into(from, name, t, name)
                    };
                    self.fs.close(from);
                    r
                })
            });
            match r {
                Ok(()) => done += 1,
                Err(e) => {
                    let verb = if copy { "copy" } else { "move" };
                    failed = Some(format!("Cannot {verb} {name} ({e})."));
                }
            }
        }
        if let Ok(t) = to {
            self.fs.close(t);
        }
        if self.show(&dir) {
            let into = self.model.borrow().shown(target);
            let verb = if copy { "Copied" } else { "Moved" };
            let s = failed.unwrap_or_else(|| {
                let first = sources[0].rsplit('/').next().unwrap_or("");
                if done == 1 {
                    format!("{verb} {first} to {into}.")
                } else {
                    format!("{verb} {done} items to {into}.")
                }
            });
            self.say(s);
        }
        self.refresh_free();
    }

    /// Carries out every pending act, and shows the window again.
    fn drain(&mut self) {
        loop {
            let next = self.model.borrow_mut().acts.pop_front();
            let Some(a) = next else { break };
            self.act(a);
        }
        self.app.repaint(self.window);
    }

    fn run(&mut self) -> i32 {
        loop {
            self.report();
            match self.app.wait() {
                Happened::Gone | Happened::CloseRequested(_) => return 0,
                Happened::Ui {
                    response: Response::Activated(VIEW),
                    ..
                } => self.drain(),
                Happened::Commands(on) => {
                    self.model.borrow_mut().commands = on;
                    self.app.repaint(self.window);
                }
                // A drag of ours over one of our own folders: copy or move
                // (proposed ADR-0248).
                Happened::DroppedHere {
                    x, y, modifiers, ..
                } => {
                    {
                        let mut m = self.model.borrow_mut();
                        let pos = row_at(&m, x, y);
                        m.dropped_here(pos, modifiers);
                    }
                    self.drain();
                }
                // System+C: the selection's paths, one per line.
                Happened::Copy { .. } => {
                    let text = {
                        let m = self.model.borrow();
                        let names = m.selected_names();
                        names
                            .iter()
                            .map(|n| m.shown(&join(&m.dir, n)))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    if !text.is_empty() {
                        let _ = self.app.offer(&text);
                    }
                }
                // Nothing is dropped on the file manager from elsewhere.
                Happened::Dropped(_) => {
                    while let Some(d) = self.app.take_drop() {
                        let _ = handle_close(d.entry);
                    }
                }
                _ => {}
            }
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = zoraya_user::command::start(DESCRIPTION);
    let room = c.parse().value(&c.d, "room").map(String::from);
    let channel = |slot: u64| Handle::new(slot).filter(|h| handle_type(*h) == Ok("channel"));
    let mut fs = Client::new(Files);
    let places = Places::take(&mut fs);
    let (Some(compositor), Some(volume), Some(panel)) = (channel(5), places.root(), channel(9))
    else {
        println!("files: missing the compositor, the volume or the panel");
        return 1;
    };
    let Some(app) = App::connect(Gui::new(compositor), Appearance::Light) else {
        println!("files: no compositor");
        return 1;
    };
    let home = zoraya_user::app::home(&mut fs, volume);
    let holds = places.holds();
    // A place that is not there (the image's folder on a volume without
    // one) is not shown.
    let model_places: Vec<_> = zoraya_files::places(&holds, &home)
        .into_iter()
        .filter(|p| {
            p.path == "/"
                || places
                    .open(&mut fs, &p.path, open_flags::DIRECTORY, rights::READ)
                    .map(|h| fs.close(h))
                    .is_ok()
        })
        .collect();
    // It starts in the first place it may change, else in the home.
    let start = holds
        .iter()
        .find(|(p, w)| *w && p != "/")
        .map_or(home.clone(), |(p, _)| p.clone());
    let mut m = Model::new(&start, &home, room.as_deref(), model_places);
    m.openers = zoraya_panel::OPENERS
        .iter()
        .map(|(e, a)| (String::from(*e), String::from(*a)))
        .collect();
    m.now = fs
        .time_base(volume)
        .map(|b| b.saturating_add(clock_monotonic()))
        .unwrap_or(0);
    let model = Rc::new(RefCell::new(m));
    let view = FilesView::new(VIEW, model.clone(), clock_monotonic);
    let mut f = FileManager {
        app,
        fs,
        places,
        panel,
        model,
        window: 0,
        told: Told {
            keys: "list",
            ..Told::default()
        },
    };
    // What the places allow, as the file service answered (INV-0168-2:
    // never `/` nor `/config` to change).
    let mut holds = f.places.holds();
    holds.sort();
    let held: Vec<String> = holds
        .iter()
        .map(|(p, w)| {
            format!(
                "{} {}",
                if *w { "+w" } else { "+r" },
                f.model.borrow().shown(p)
            )
        })
        .collect();
    println!("files: holds {}", held.join(" "));
    f.refresh_free();
    f.show(&start);
    let Some(window) = f.app.open("Files", Box::new(view), WIDTH, HEIGHT) else {
        return 1;
    };
    f.window = window;
    // The keyboard starts on the list.
    if let Some(w) = f.app.window(window) {
        w.move_focus(false);
    }
    f.app.repaint(window);
    f.run()
}

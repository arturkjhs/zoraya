//! The file manager (plan 7e): a folder's list, Up, Open, New folder,
//! Rename and Delete (`zoraya-files` decides, this program does the I/O
//! through `zoraya-fsclient`). It opens a file only through the panel:
//! `OpenWith` with one read-only connection, and the panel launches the
//! application for its name with it (ADR-0168 point 4).
//!
//! Launched by the panel (`zoraya_panel::application`): slot 1 the
//! console, 5 its compositor channel, 6 its chooser channel (unused), 7 its
//! data directory, 8 the volume's root, 9 its channel to the panel.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::{Handle, ObjectType};
use zoraya_app::{App, Happened};
use zoraya_files::{Act, Entry, Manager};
use zoraya_fsclient::Client;
use zoraya_proto_chooser as pc;
use zoraya_proto_file::{open_flags, rights};
use zoraya_protocol::{Data, decode, encode};
use zoraya_rt::{channel_read, channel_write, handle_close, handle_type, println};
use zoraya_tokens::Appearance;
use zoraya_ui::layout::{Column, Row};
use zoraya_ui::widgets::{Button, Label, ListView, TextField};
use zoraya_ui::{Response, Value, Widget};
use zoraya_user::app::{Files, Gui};

const LIST: u32 = 1;
const NAME: u32 = 2;
const UP: u32 = 3;
const OPEN: u32 = 4;
const NEW_FOLDER: u32 = 5;
const RENAME: u32 = 6;
const DELETE: u32 = 7;

/// The window's logical size.
const WIDTH: u32 = 480;
const HEIGHT: u32 = 360;
/// The most names a folder's list shows.
const MAX_LISTED: usize = 512;

fn tree(m: &Manager, status: &str, name: &str) -> Box<dyn Widget> {
    let rows = m.rows();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    Box::new(Column::new(vec![
        Box::new(Label::new(&m.shown())),
        Box::new(ListView::new(LIST, &rows)),
        Box::new(TextField::new(NAME, name)),
        Box::new(Row::new(vec![
            Box::new(Button::new(UP, "Up")),
            Box::new(Button::new(OPEN, "Open")),
            Box::new(Button::new(NEW_FOLDER, "New folder")),
            Box::new(Button::new(RENAME, "Rename")),
            Box::new(Button::new(DELETE, "Delete")),
        ])),
        Box::new(Label::new(status)),
    ]))
}

/// The folder `dir`'s names, or why not.
fn list(fs: &mut Client<Files>, root: Handle, dir: &str) -> Result<Vec<Entry>, String> {
    let d = fs
        .open(root, dir, open_flags::DIRECTORY, rights::READ)
        .map_err(|e| format!("{dir}: cannot open ({e})"))?;
    let listed = fs.list(d);
    fs.close(d);
    let listed = listed.map_err(|e| format!("{dir}: cannot list ({e})"))?;
    Ok(listed
        .into_iter()
        .take(MAX_LISTED)
        .map(|l| Entry {
            dir: l.is_dir(),
            name: l.name,
        })
        .collect())
}

/// Hands `path` to the panel with one read-only connection.
fn open_with(
    fs: &mut Client<Files>,
    root: Handle,
    panel: Handle,
    path: &str,
    words: &str,
) -> String {
    let file = match fs.open(root, path, 0, rights::READ | rights::CLONE) {
        Ok(f) => f,
        Err(e) => return format!("{path}: cannot open ({e})"),
    };
    let name = path.rsplit('/').next().unwrap_or(path);
    let m = pc::OpenWith {
        name: Data(name.as_bytes()),
        words: Data(words.as_bytes()),
    };
    let mut out = vec![0u8; 2048];
    let Ok(n) = encode(&m, 1, &mut out) else {
        fs.close(file);
        return String::from("The name is too long to open.");
    };
    let entry = zoraya_abi::ChannelEntry {
        handle: file,
        rights: zoraya_abi::ChannelObject::RIGHTS,
        transfer: zoraya_abi::spawn::Transfer::Move,
    };
    if channel_write(panel, &out[..n], &[entry]).is_err() {
        let _ = handle_close(file);
        return String::from("The panel is gone.");
    }
    // The panel answers at once: the launch is done or refused.
    let mut bytes = [0u8; 64];
    let mut handles = [panel; 1];
    let status = loop {
        match channel_read(panel, &mut bytes, &mut handles) {
            Ok(s) => {
                let msg = &bytes[..s.bytes as usize];
                break decode::<pc::Opening>(msg, 0).map_or(u32::MAX, |(_, o)| o.status.0);
            }
            Err((zoraya_abi::Status::WouldBlock, _)) => {
                if !zoraya_user::app::wait(panel) {
                    break u32::MAX;
                }
            }
            Err(_) => break u32::MAX,
        }
    };
    match status {
        0 => {
            println!("files: opened {words}");
            format!("Opened {name}.")
        }
        _ => format!("No application opens {name}."),
    }
}

/// Starts a drag of `path` from `window`, with one read-only connection.
fn drag(app: &mut App<Gui>, fs: &mut Client<Files>, root: Handle, window: u32, path: &str) {
    let Ok(file) = fs.open(root, path, 0, rights::READ | rights::CLONE) else {
        return;
    };
    let name = path.rsplit('/').next().unwrap_or(path);
    match app.drag(window, file, name) {
        Ok(()) => println!("files: dragging {name}"),
        // Not sent: still ours to close (refused, the compositor did).
        Err(Some(f)) => fs.close(f),
        Err(None) => {}
    }
}

/// The folder shown and its rows, on the console (for the GUI tests).
fn say_listed(m: &Manager) {
    println!("files: {} lists {}", m.shown(), m.rows().join(" "));
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let channel = |slot: u64| Handle::new(slot).filter(|h| handle_type(*h) == Ok("channel"));
    let (Some(compositor), Some(root), Some(panel)) = (channel(5), channel(8), channel(9)) else {
        println!("files: missing the compositor, the root or the panel");
        return 1;
    };
    let Some(mut app) = App::connect(Gui::new(compositor), Appearance::Light) else {
        println!("files: no compositor");
        return 1;
    };
    let mut fs = Client::new(Files);
    let home = zoraya_user::app::home(&mut fs, root);
    let mut m = Manager::new(&home, &home);
    let mut status = String::new();
    match list(&mut fs, root, &home) {
        Ok(entries) => {
            m.show(&home, entries);
            say_listed(&m);
        }
        Err(e) => status = e,
    }
    let Some(window) = app.open("Files", tree(&m, &status, ""), WIDTH, HEIGHT) else {
        return 1;
    };
    loop {
        let id = match app.wait() {
            Happened::Gone | Happened::CloseRequested(_) => return 0,
            Happened::Ui {
                response: Response::Activated(id),
                ..
            } => id,
            // A press that picks a file starts a drag of it, read only
            // (ADR-0170: a dropped file is a `read` entry).
            Happened::Ui {
                window: w,
                response: Response::Changed(LIST),
            } if app.held(w) => {
                let row = match app.window(w).map(|x| x.value(LIST)) {
                    Some(Value::Index(i)) => i,
                    _ => None,
                };
                if let Act::OpenWith { path, .. } = m.activate(row) {
                    drag(&mut app, &mut fs, root, w, &path);
                }
                continue;
            }
            // Nothing is dropped on the file manager in 7e.
            Happened::Dropped(_) => {
                while let Some(d) = app.take_drop() {
                    let _ = handle_close(d.entry);
                }
                continue;
            }
            _ => continue,
        };
        let (row, name) = match app.window(window) {
            Some(w) => (
                match w.value(LIST) {
                    Value::Index(i) => i,
                    _ => None,
                },
                match w.value(NAME) {
                    Value::Text(t) => String::from(t),
                    _ => String::new(),
                },
            ),
            None => (None, String::new()),
        };
        let act = match id {
            LIST | OPEN => m.activate(row),
            UP => m.up(),
            NEW_FOLDER => m.new_folder(&name),
            RENAME => m.rename(row, &name),
            DELETE => m.delete(row),
            _ => continue,
        };
        let mut keep_name = name.clone();
        let mut show = None;
        status = match act {
            Act::Say(s) => s,
            Act::Show(dir) => {
                show = Some(dir);
                String::new()
            }
            Act::OpenWith { path, words } => open_with(&mut fs, root, panel, &path, &words),
            Act::MakeDirectory { dir, name } => {
                let r = fs
                    .open(root, &dir, open_flags::DIRECTORY, rights::ALL)
                    .and_then(|d| {
                        let r = fs.make_directory(d, &name);
                        fs.close(d);
                        r
                    });
                show = Some(dir);
                keep_name.clear();
                match r {
                    Ok(()) => format!("Made {name}."),
                    Err(e) => format!("Cannot make {name} ({e})."),
                }
            }
            Act::Rename { dir, old, new } => {
                let r = fs
                    .open(root, &dir, open_flags::DIRECTORY, rights::ALL)
                    .and_then(|d| {
                        let r = fs.rename(d, &old, &new);
                        fs.close(d);
                        r
                    });
                show = Some(dir);
                keep_name.clear();
                match r {
                    Ok(()) => format!("Renamed {old} to {new}."),
                    Err(e) => format!("Cannot rename {old} ({e})."),
                }
            }
            Act::Remove { dir, name } => {
                let r = fs
                    .open(root, &dir, open_flags::DIRECTORY, rights::ALL)
                    .and_then(|d| {
                        let r = fs.remove(d, &name);
                        fs.close(d);
                        r
                    });
                show = Some(dir);
                match r {
                    Ok(()) => format!("Deleted {name}."),
                    Err(e) => format!("Cannot delete {name} ({e})."),
                }
            }
        };
        if let Some(dir) = show {
            match list(&mut fs, root, &dir) {
                Ok(entries) => {
                    m.show(&dir, entries);
                    say_listed(&m);
                }
                Err(e) => status = e,
            }
        }
        if !status.is_empty() {
            println!("files: {status}");
        }
        app.replace(window, tree(&m, &status, &keep_name));
    }
}

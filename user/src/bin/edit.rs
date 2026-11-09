//! The text editor (ADR-0168 point 5, plan 7f): one file in a
//! `zoraya-editor` view, opened from the file manager or through the
//! chooser, saved through the chooser (`Save`'s one name in a folder) or
//! again to that name; find; and before a close with unsaved changes, a
//! question. The console says what was opened and saved, for the GUI test.
//!
//! Launched by the panel (`zoraya_panel::application`): slot 1 the
//! console, 5 its compositor channel, 6 its chooser channel, 7 its data
//! directory, and 10 the file opened from the file manager, read only.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use core::cell::RefCell;

use zoraya_abi::{DEADLINE_INFINITE, Handle, Mode, Packet, channel_signals};
use zoraya_app::{App, Happened};
use zoraya_editor::Buffer;
use zoraya_editor::view::EditorView;
use zoraya_fsclient::Client;
use zoraya_proto_file::{open_flags, rights};
use zoraya_rt::{handle_close, handle_type, port_create, port_subscribe, port_wait, println};
use zoraya_tokens::Appearance;
use zoraya_ui::layout::{Column, Row};
use zoraya_ui::widgets::{Button, Label, TextField};
use zoraya_ui::{Response, Value, Widget};
use zoraya_user::app::chooser::{Answer, answer, ask};
use zoraya_user::app::{Files, Gui};

const OPEN: u32 = 1;
const SAVE: u32 = 2;
const SAVE_AS: u32 = 3;
const FIND: u32 = 4;
const FIND_NEXT: u32 = 5;
const TEXT: u32 = 6;
const DISCARD: u32 = 7;
const KEEP: u32 = 8;
const SAVE_CLOSE: u32 = 9;

const KEY_GUI: u64 = 1;
const KEY_CHOOSER: u64 = 2;
const WIDTH: u32 = 560;
const HEIGHT: u32 = 380;
const FILTERS: &str = "txt md";

/// Where the text is saved: the folder narrowed to its one name (a
/// `Save` connection), and that name.
struct Target {
    dir: Handle,
    name: String,
}

/// The text of a file, or why it is not opened: not UTF-8 (saving would
/// corrupt it), or larger than the editor holds (saving would cut it;
/// review 7f, 6 and 14).
fn text_of(bytes: &[u8]) -> Result<&str, String> {
    let t = core::str::from_utf8(bytes).map_err(|_| String::from("it is not UTF-8 text"))?;
    if t.chars().count() > zoraya_editor::MAX_CHARS {
        return Err(String::from("it is larger than the editor holds"));
    }
    Ok(t)
}

fn editor(buffer: &Rc<RefCell<Buffer>>, status: &str, find: &str) -> Box<dyn Widget> {
    Box::new(Column::new(vec![
        Box::new(Row::new(vec![
            Box::new(Button::new(OPEN, "Open…")),
            Box::new(Button::new(SAVE, "Save")),
            Box::new(Button::new(SAVE_AS, "Save as…")),
            Box::new(TextField::new(FIND, find)),
            Box::new(Button::new(FIND_NEXT, "Find")),
        ])),
        Box::new(EditorView::new(TEXT, buffer.clone())),
        Box::new(Label::new(status)),
    ]))
}

/// The question before a close with unsaved changes.
fn question(name: &str) -> Box<dyn Widget> {
    Box::new(Column::new(vec![
        Box::new(Label::new(&format!("Save the changes to {name}?"))),
        Box::new(Row::new(vec![
            Box::new(Button::new(DISCARD, "Don't save")),
            Box::new(Button::new(KEEP, "Cancel")),
            Box::new(Button::new(SAVE_CLOSE, "Save")),
        ])),
    ]))
}

/// Writes the text to the target's name, created or replaced.
fn write(fs: &mut Client<Files>, t: &Target, text: &str) -> Result<(), u32> {
    let wanted = rights::READ | rights::WRITE | rights::SYNC;
    let f = fs.open(t.dir, &t.name, open_flags::CREATE, wanted)?;
    let r = fs.write_all(f, text.as_bytes());
    fs.close(f);
    r
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let channel = |slot: u64| Handle::new(slot).filter(|h| handle_type(*h) == Ok("channel"));
    let Some(compositor) = channel(5) else {
        println!("edit: no compositor");
        return 1;
    };
    let chooser = channel(6);
    let Some(mut app) = App::connect(Gui::new(compositor), Appearance::Light) else {
        println!("edit: no compositor");
        return 1;
    };
    let mut fs = Client::new(Files);
    let buffer = Rc::new(RefCell::new(Buffer::new("")));
    let mut name = String::from("untitled.txt");
    let mut status = String::new();
    // A file opened from the file manager: read it; it is read-only, so a
    // save asks where.
    if let Some(file) = channel(10) {
        let read = fs
            .read_all(file)
            .map_err(|e| format!("it could not be read ({e})"))
            .and_then(|b| text_of(&b).map(|t| (b.len(), Buffer::new(t))));
        match read {
            Ok((n, b)) => {
                *buffer.borrow_mut() = b;
                println!("edit: opened {n} bytes");
            }
            Err(e) => status = format!("The file is not opened: {e}."),
        }
        fs.close(file);
    }
    let Some(window) = app.open(
        &format!("{name} — edit"),
        editor(&buffer, &status, ""),
        WIDTH,
        HEIGHT,
    ) else {
        return 1;
    };
    let Ok(port) = port_create() else { return 1 };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, compositor, wanted, KEY_GUI, Mode::Level).is_err() {
        return 1;
    }
    if let Some(c) = chooser {
        let _ = port_subscribe(port, c, wanted, KEY_CHOOSER, Mode::Level);
    }
    let mut chooser = chooser;
    let mut target: Option<Target> = None;
    // A request waiting: whether it is a save, and whether the window
    // closes after it.
    let mut asked: Option<(bool, bool)> = None;
    let mut asking_to_close = false;
    let mut discard_armed = false;
    let mut transaction = 1;
    let mut packets = [Packet::default(); 4];
    loop {
        let mut rebuild = false;
        let mut save_then_close = false;
        while let Some(h) = app.poll() {
            let id = match h {
                Happened::Gone => return 0,
                Happened::CloseRequested(_) => {
                    if !buffer.borrow().modified() {
                        return 0;
                    }
                    asking_to_close = true;
                    app.replace(window, question(&name));
                    continue;
                }
                Happened::Ui {
                    response: Response::Activated(id),
                    ..
                } => id,
                Happened::Ui {
                    response: Response::Changed(TEXT),
                    ..
                } => {
                    status.clear();
                    continue;
                }
                // The clipboard (ADR-0170): the selection offered, cut on
                // a cut; a paste typed over the selection.
                Happened::Copy { cut, .. } => {
                    let selected = buffer.borrow().selected();
                    if !selected.is_empty() && app.offer(&selected) && cut {
                        buffer.borrow_mut().cut();
                        rebuild = true;
                    }
                    continue;
                }
                Happened::Pasted(_) => {
                    if let Some(text) = app.take_paste() {
                        buffer.borrow_mut().insert(&text);
                        rebuild = true;
                    }
                    continue;
                }
                _ => continue,
            };
            match id {
                DISCARD => return 0,
                KEEP => {
                    asking_to_close = false;
                    rebuild = true;
                }
                SAVE_CLOSE => {
                    asking_to_close = false;
                    save_then_close = true;
                }
                // Open over unsaved changes asks first: a second Open
                // discards them (review 7f, 5).
                OPEN if asked.is_none() && buffer.borrow().modified() && !discard_armed => {
                    discard_armed = true;
                    status = String::from(
                        "There are unsaved changes: Save first, or press Open again to discard them.",
                    );
                    rebuild = true;
                }
                OPEN | SAVE_AS if asked.is_none() => {
                    discard_armed = false;
                    if let Some(c) = chooser {
                        transaction += 1;
                        let save = id == SAVE_AS;
                        let suggested = save.then_some(name.as_str());
                        if ask(c, suggested, FILTERS, transaction) {
                            asked = Some((save, false));
                        }
                    } else {
                        status = String::from("No chooser: this window cannot open or save files.");
                        rebuild = true;
                    }
                }
                SAVE => save_then_close = false,
                FIND | FIND_NEXT => {
                    let needle = match app.window(window).map(|w| w.value(FIND)) {
                        Some(Value::Text(t)) => String::from(t),
                        _ => String::new(),
                    };
                    let found = buffer.borrow_mut().find(&needle);
                    status = if found {
                        String::new()
                    } else {
                        format!("{needle} is not in the text.")
                    };
                    rebuild = true;
                }
                _ => {}
            }
            if id == SAVE || save_then_close {
                match &target {
                    Some(t) => {
                        let text = buffer.borrow().text();
                        match write(&mut fs, t, &text) {
                            Ok(()) => {
                                buffer.borrow_mut().saved();
                                println!("edit: saved {} ({} bytes)", t.name, text.len());
                                status = format!("Saved {}.", t.name);
                                if save_then_close {
                                    return 0;
                                }
                            }
                            Err(e) => status = format!("Not saved ({e})."),
                        }
                        rebuild = true;
                    }
                    // Nowhere yet: ask where, as Save as.
                    None => {
                        if let (Some(c), None) = (chooser, asked) {
                            transaction += 1;
                            if ask(c, Some(&name), FILTERS, transaction) {
                                asked = Some((true, save_then_close));
                            }
                        }
                    }
                }
            }
        }
        if let Some(c) = chooser {
            match answer(c) {
                Answer::Waiting => {}
                Answer::Ended(_) => asked = None,
                Answer::Gone => {
                    asked = None;
                    let _ = handle_close(c);
                    chooser = None;
                }
                Answer::Chosen(h, _) if asked.is_none() => {
                    let _ = handle_close(h);
                }
                Answer::Chosen(h, shown) => {
                    let (save, close) = asked.take().unwrap_or((false, false));
                    if save {
                        if let Some(old) = target.take() {
                            fs.close(old.dir);
                        }
                        let t = Target {
                            dir: h,
                            name: shown.clone(),
                        };
                        let text = buffer.borrow().text();
                        match write(&mut fs, &t, &text) {
                            Ok(()) => {
                                buffer.borrow_mut().saved();
                                println!("edit: saved {} ({} bytes)", t.name, text.len());
                                status = format!("Saved {}.", t.name);
                                if close {
                                    return 0;
                                }
                            }
                            Err(e) => status = format!("Not saved ({e})."),
                        }
                        target = Some(t);
                    } else {
                        let read = fs
                            .read_all(h)
                            .map_err(|e| format!("it could not be read ({e})"))
                            .and_then(|b| text_of(&b).map(|t| (b.len(), Buffer::new(t))));
                        match read {
                            Ok((n, b)) => {
                                *buffer.borrow_mut() = b;
                                println!("edit: opened {shown} ({n} bytes)");
                                status = String::new();
                                if let Some(old) = target.take() {
                                    fs.close(old.dir);
                                }
                            }
                            Err(e) => status = format!("{shown} is not opened: {e}."),
                        }
                        fs.close(h);
                    }
                    name = shown;
                    let _ = app.set_title(window, &format!("{name} — edit"));
                    rebuild = true;
                }
            }
        }
        if rebuild && !asking_to_close {
            let find = match app.window(window).map(|w| w.value(FIND)) {
                Some(Value::Text(t)) => String::from(t),
                _ => String::new(),
            };
            app.replace(window, editor(&buffer, &status, &find));
        }
        let _ = port_wait(port, DEADLINE_INFINITE, &mut packets);
    }
}

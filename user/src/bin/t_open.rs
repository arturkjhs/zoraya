//! A test application for the chooser (stage 7e's acceptance test): a
//! window with Open and Save. Each asks the chooser (`zoraya-proto-chooser`);
//! what came back is read, and written to, and the console says what the
//! connection allowed, for the GUI script to check:
//!
//! ```text
//! t_open: opened ~/plán.txt: 6 bytes: "hello"
//! t_open: write to ~/plán.txt refused
//! t_open: saved ~/new.txt
//! ```
//!
//! Launched by the panel (`zoraya_panel::application`): slot 1 the
//! console, 5 its compositor channel, 6 its chooser channel, 7 its data
//! directory, and 10 the file opened from the file manager, if any.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;

use zoraya_abi::{DEADLINE_INFINITE, Handle, Mode, Packet, Status, channel_signals};
use zoraya_app::{App, Happened};
use zoraya_fsclient::Client;
use zoraya_proto_chooser as pc;
use zoraya_proto_file::{open_flags, rights};
use zoraya_protocol::{Data, Header, decode, encode};
use zoraya_rt::{
    channel_read, channel_write, handle_close, handle_type, port_create, port_subscribe, port_wait,
    println,
};
use zoraya_tokens::Appearance;
use zoraya_ui::Response;
use zoraya_ui::layout::{Column, Row};
use zoraya_ui::widgets::{Button, Label};
use zoraya_user::app::{Files, Gui};

const OPEN: u32 = 1;
const SAVE: u32 = 2;
const KEY_GUI: u64 = 1;
const KEY_CHOOSER: u64 = 2;
/// What Save writes.
const SAVED: &[u8] = "saved by t_open: ž\n".as_bytes();

fn tree(status: &str) -> Box<dyn zoraya_ui::Widget> {
    Box::new(Column::new(vec![
        Box::new(Label::new(status)),
        Box::new(Row::new(vec![
            Box::new(Button::new(OPEN, "Open…")),
            Box::new(Button::new(SAVE, "Save…")),
        ])),
    ]))
}

/// The last part of a shown path.
fn base(shown: &str) -> &str {
    shown.rsplit('/').next().unwrap_or(shown)
}

/// Reads `file`, says what it holds, then tries to change it: the console
/// line says whether the connection allowed it.
fn read_then_write(fs: &mut Client<Files>, file: Handle, shown: &str) -> String {
    let said = match fs.read_all(file) {
        Ok(b) => {
            let text = String::from_utf8_lossy(&b);
            let first = text.lines().next().unwrap_or("");
            format!("opened {shown}: {} bytes: \"{first}\"", b.len())
        }
        Err(e) => format!("reading {shown} failed: {e}"),
    };
    println!("t_open: {said}");
    match fs.write_all(file, b"changed by t_open\n") {
        Ok(()) => println!("t_open: write to {shown} allowed"),
        Err(_) => println!("t_open: write to {shown} refused"),
    }
    said
}

/// Creates the chosen name in the narrowed folder and writes [`SAVED`].
fn save(fs: &mut Client<Files>, dir: Handle, shown: &str) -> String {
    let wanted = rights::READ | rights::WRITE | rights::SYNC;
    let said = match fs.open(dir, base(shown), open_flags::CREATE, wanted) {
        Ok(f) => {
            let r = fs.write_all(f, SAVED);
            fs.close(f);
            match r {
                Ok(()) => format!("saved {shown}"),
                Err(e) => format!("saving {shown} failed: {e}"),
            }
        }
        Err(e) => format!("creating {shown} failed: {e}"),
    };
    println!("t_open: {said}");
    // The narrowed folder reaches nothing else.
    if fs
        .open(dir, "other.txt", open_flags::CREATE, wanted)
        .is_ok()
    {
        println!("t_open: other.txt beside {shown} allowed");
    } else {
        println!("t_open: other.txt beside {shown} refused");
    }
    said
}

/// Sends a request to the chooser.
fn ask(chooser: Handle, save: bool, t: u32) -> bool {
    let mut out = vec![0u8; 1024];
    let n = if save {
        encode(
            &pc::Save {
                title: Data(b""),
                suggested: Data("nový.txt".as_bytes()),
                filters: Data(b"txt md"),
            },
            t,
            &mut out,
        )
    } else {
        encode(
            &pc::Open {
                title: Data(b""),
                filters: Data(b"txt md"),
            },
            t,
            &mut out,
        )
    };
    n.is_ok_and(|n| channel_write(chooser, &out[..n], &[]).is_ok())
}

/// What the chooser's channel held.
enum Answer {
    /// Nothing yet.
    Waiting,
    /// The connection and the name shown.
    Chosen(Handle, String),
    /// Nothing chosen, or the chooser is gone.
    Ended,
}

/// The chooser's answer, if one is there.
fn answer(chooser: Handle) -> Answer {
    let mut bytes = vec![0u8; 2048];
    let mut handles = [chooser; 1];
    let sizes = match channel_read(chooser, &mut bytes, &mut handles) {
        Ok(s) => s,
        Err((Status::WouldBlock, _)) => return Answer::Waiting,
        Err(_) => {
            println!("t_open: the chooser is gone");
            return Answer::Ended;
        }
    };
    let got = &handles[..sizes.handles as usize];
    let msg = &bytes[..sizes.bytes as usize];
    let ordinal = Header::decode(msg).map_or(0, |(h, _)| h.ordinal);
    if ordinal == 8
        && let Ok((_, m)) = decode::<pc::Chosen<'_>>(msg, got.len())
    {
        let shown = String::from(core::str::from_utf8(m.name.0).unwrap_or("?"));
        println!(
            "t_open: given {}",
            core::str::from_utf8(m.words.0).unwrap_or("?")
        );
        return Answer::Chosen(got[0], shown);
    }
    for h in got {
        let _ = handle_close(*h);
    }
    let status = decode::<pc::Cancelled>(msg, 0).map_or(u32::MAX, |(_, c)| c.status.0);
    println!("t_open: nothing chosen ({status})");
    Answer::Ended
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let channel = |slot: u64| Handle::new(slot).filter(|h| handle_type(*h) == Ok("channel"));
    let Some(compositor) = channel(5) else {
        println!("t_open: no compositor");
        return 1;
    };
    let chooser = channel(6);
    let Some(mut app) = App::connect(Gui::new(compositor), Appearance::Light) else {
        println!("t_open: no compositor");
        return 1;
    };
    let mut fs = Client::new(Files);
    let mut status = String::from("Open or save a file.");
    if let Some(file) = channel(10) {
        status = read_then_write(&mut fs, file, "the opened file");
        fs.close(file);
    }
    let Some(window) = app.open("t_open", tree(&status), 360, 120) else {
        return 1;
    };
    let Ok(port) = port_create() else { return 1 };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, compositor, wanted, KEY_GUI, Mode::Level).is_err() {
        return 1;
    }
    if let Some(c) = chooser
        && port_subscribe(port, c, wanted, KEY_CHOOSER, Mode::Level).is_err()
    {
        return 1;
    }
    // The request waiting for its answer: whether it is a Save.
    let mut asked: Option<bool> = None;
    let mut transaction = 1;
    let mut packets = [Packet::default(); 4];
    loop {
        while let Some(h) = app.poll() {
            match h {
                Happened::Gone => return 0,
                Happened::CloseRequested(_) => {
                    println!("t_open: closed");
                    return 0;
                }
                Happened::Ui {
                    response: Response::Activated(id @ (OPEN | SAVE)),
                    ..
                } if asked.is_none() => {
                    let Some(c) = chooser else {
                        println!("t_open: no chooser");
                        continue;
                    };
                    transaction += 1;
                    if ask(c, id == SAVE, transaction) {
                        asked = Some(id == SAVE);
                    }
                }
                _ => {}
            }
        }
        if let (Some(c), Some(was_save)) = (chooser, asked) {
            match answer(c) {
                Answer::Waiting => {}
                Answer::Ended => asked = None,
                Answer::Chosen(h, shown) => {
                    asked = None;
                    let said = if was_save {
                        save(&mut fs, h, &shown)
                    } else {
                        read_then_write(&mut fs, h, &shown)
                    };
                    fs.close(h);
                    app.replace(window, tree(&said));
                }
            }
        }
        let _ = port_wait(port, DEADLINE_INFINITE, &mut packets);
    }
}

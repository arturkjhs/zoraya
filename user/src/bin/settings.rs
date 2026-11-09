//! The settings window (ADR-0168 point 6, ADR-0169): one page of the
//! desktop's settings (`zoraya-settings-window`). Every change is written
//! at once, atomically (a new file published over the old one, ADR-0010),
//! and announced to the panel, which tells the compositor
//! (`SettingsChanged`, then `ApplySettings`).
//!
//! Launched by the panel (`zoraya_panel::application`): slot 1 the
//! console, 5 its compositor channel, 6 its chooser channel (unused), 7 its
//! data directory, 8 `/config` narrowed to `settings`, 9 the panel.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec;

use zoraya_abi::Handle;
use zoraya_app::{App, Happened};
use zoraya_proto_chooser as pc;
use zoraya_protocol::{decode, encode};
use zoraya_rt::{channel_read, channel_write, handle_type, println};
use zoraya_settings_window::{changed, page, written};
use zoraya_tokens::Appearance;
use zoraya_ui::Response;
use zoraya_user::app::Gui;
use zoraya_user::files::{File, read_all};

/// The window's logical size.
const WIDTH: u32 = 600;
const HEIGHT: u32 = 520;

/// The file as it is now (empty when there is none).
fn current(config: &File) -> vec::Vec<u8> {
    config
        .open("settings", 0, zoraya_proto_file::rights::READ)
        .ok()
        .and_then(|f| read_all(&f).ok())
        .unwrap_or_default()
}

/// Writes `bytes` as a new file published over the old one.
fn publish(config: &File, bytes: &[u8]) -> Result<(), u32> {
    let wanted = zoraya_proto_file::rights::READ
        | zoraya_proto_file::rights::WRITE
        | zoraya_proto_file::rights::PUBLISH
        | zoraya_proto_file::rights::SYNC;
    let file = config.create_unnamed_with(wanted)?;
    if !bytes.is_empty() {
        file.write(0, 0, bytes)?;
    }
    file.publish(config, "settings")
}

/// Tells the panel, and waits for its answer.
fn announce(panel: Handle) -> bool {
    let mut out = [0u8; 64];
    let Ok(n) = encode(&pc::SettingsChanged {}, 1, &mut out) else {
        return false;
    };
    if channel_write(panel, &out[..n], &[]).is_err() {
        return false;
    }
    let mut bytes = [0u8; 64];
    let mut handles = [panel; 1];
    loop {
        match channel_read(panel, &mut bytes, &mut handles) {
            Ok(s) => {
                let msg = &bytes[..s.bytes as usize];
                return decode::<pc::SettingsApplied>(msg, 0)
                    .is_ok_and(|(_, a)| a.status.0 == pc::status::CANCELLED);
            }
            Err((zoraya_abi::Status::WouldBlock, _)) => {
                if !zoraya_user::app::wait(panel) {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let channel = |slot: u64| Handle::new(slot).filter(|h| handle_type(*h) == Ok("channel"));
    let (Some(compositor), Some(config), Some(panel)) = (channel(5), channel(8), channel(9)) else {
        println!("settings: missing the compositor, /config or the panel");
        return 1;
    };
    let config = File { end: config };
    let bytes = current(&config);
    let (mut s, _) = zoraya_compositor::settings::read(&bytes);
    let appearance = if s.dark {
        Appearance::Dark
    } else {
        Appearance::Light
    };
    let Some(mut app) = App::connect(Gui::new(compositor), appearance) else {
        println!("settings: no compositor");
        return 1;
    };
    let Some(window) = app.open("Settings", page(&s, ""), WIDTH, HEIGHT) else {
        return 1;
    };
    loop {
        let result = match app.wait() {
            Happened::Gone | Happened::CloseRequested(_) => return 0,
            Happened::Ui {
                response: Response::Changed(id),
                ..
            } => match app.window(window) {
                Some(w) => changed(&s, id, w.value(id)),
                None => continue,
            },
            _ => continue,
        };
        let status: String = match result {
            Err(why) => why.into(),
            Ok(new) => {
                let bytes = current(&config);
                match written(&bytes, &new).map(|f| publish(&config, &f)) {
                    Ok(Ok(())) => {
                        s = new;
                        if announce(panel) {
                            "Saved and applied.".into()
                        } else {
                            "Saved; it applies at the next session.".into()
                        }
                    }
                    Ok(Err(e)) => alloc::format!("Not saved ({e})."),
                    Err(e) => alloc::format!("Not saved: {e}."),
                }
            }
        };
        if !status.is_empty() {
            println!("settings: {status}");
        }
        app.replace(window, page(&s, &status));
    }
}

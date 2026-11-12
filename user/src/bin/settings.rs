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
use zoraya_rt::{channel_read, channel_write, clock_monotonic, handle_type, println};
use zoraya_settings::usage::{RoomRow, rows_from_wire};
use zoraya_settings_window::{
    APPLY_ROOMS, changed, field_of, page_with_rooms, room_changes, written,
};
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

/// One request to the panel and its reply, read without a limit on the
/// wait the panel's own work needs (a limit written and applied).
fn call_panel(panel: Handle, request: &[u8]) -> Option<vec::Vec<u8>> {
    channel_write(panel, request, &[]).ok()?;
    let mut bytes = vec![0u8; pc::MAX_ROOMS_REPLY + 256];
    let mut handles = [panel; 1];
    loop {
        match channel_read(panel, &mut bytes, &mut handles) {
            Ok(s) => {
                bytes.truncate(s.bytes as usize);
                return Some(bytes);
            }
            Err((zoraya_abi::Status::WouldBlock, _)) => {
                if !zoraya_user::app::wait(panel) {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
}

/// The rooms of the table with their limits and use (ADR-0267 point 4),
/// from the panel: this window cannot read `/config/rooms`. None when the
/// panel does not answer; the page then has no Rooms section.
fn query_rooms(panel: Handle) -> vec::Vec<RoomRow> {
    let mut out = [0u8; 64];
    let Ok(n) = encode(&pc::RoomsQuery {}, 2, &mut out) else {
        return vec::Vec::new();
    };
    call_panel(panel, &out[..n])
        .and_then(|b| {
            let (_, r) = decode::<pc::RoomsReply<'_>>(&b, 0).ok()?;
            rows_from_wire(core::str::from_utf8(r.text.0).ok()?)
        })
        .unwrap_or_default()
}

/// `room set ROOM WORD VALUE`, done by the panel: `Ok` or the sentence the
/// shell would print.
fn set_limit(panel: Handle, room: &str, word: &str, value: &str) -> Result<(), String> {
    let m = pc::SetRoomLimit {
        room: zoraya_protocol::Data(room.as_bytes()),
        word: zoraya_protocol::Data(word.as_bytes()),
        value: zoraya_protocol::Data(value.as_bytes()),
    };
    let mut out = [0u8; 128];
    let n = encode(&m, 3, &mut out).map_err(|_| String::from("the request is too long"))?;
    let bytes =
        call_panel(panel, &out[..n]).ok_or_else(|| String::from("the panel does not answer"))?;
    let (_, r) = decode::<pc::RoomLimitSet<'_>>(&bytes, 0)
        .map_err(|_| String::from("the panel's answer is not understood"))?;
    if r.status.0 == 0 {
        Ok(())
    } else {
        Err(String::from(
            core::str::from_utf8(r.text.0).unwrap_or("refused"),
        ))
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
    let mut rows = query_rooms(panel);
    // What the person typed in the Rooms fields and has not applied.
    let mut drafts: vec::Vec<(u32, String)> = vec::Vec::new();
    let Some(window) = app.open(
        "Settings",
        page_with_rooms(&s, &rows, &drafts, ""),
        WIDTH,
        HEIGHT,
    ) else {
        return 1;
    };
    loop {
        let result = match app.wait() {
            Happened::Gone | Happened::CloseRequested(_) => return 0,
            // A Rooms field: the draft is kept and the `room set` line under
            // it follows (ADR-0267 point 4); nothing is written until Apply.
            Happened::Ui {
                response: Response::Changed(id),
                ..
            } if field_of(id).is_some() => {
                if let Some(w) = app.window(window)
                    && let zoraya_ui::Value::Text(t) = w.value(id)
                {
                    drafts.retain(|(d, _)| *d != id);
                    drafts.push((id, String::from(t)));
                }
                app.replace(window, page_with_rooms(&s, &rows, &drafts, ""));
                continue;
            }
            // Apply: each changed word through the panel, which writes the
            // line and asks the room keeper (it refuses above the ceilings).
            Happened::Ui {
                response: Response::Activated(APPLY_ROOMS),
                ..
            } => {
                let mut said = vec::Vec::new();
                let changes = room_changes(&rows, &drafts);
                for (room, word, value) in &changes {
                    match set_limit(panel, room, word, value) {
                        Ok(()) => said.push(alloc::format!("room set {room} {word} {value}: done")),
                        Err(e) => {
                            // After one answer that says nobody answers, the
                            // rest would each wait as long: they are not
                            // sent (Q6b review, ADR-0294).
                            let silent = e == "the room keeper does not answer"
                                || e == "the panel does not answer";
                            said.push(alloc::format!("room: {room}: {e}"));
                            if silent {
                                said.push(String::from("the other changes were not sent"));
                                break;
                            }
                        }
                    }
                }
                rows = query_rooms(panel);
                drafts.clear();
                let status = if said.is_empty() {
                    String::from("No change.")
                } else {
                    said.join(" | ")
                };
                println!("settings: {status}");
                app.replace(window, page_with_rooms(&s, &rows, &drafts, &status));
                continue;
            }
            Happened::Ui {
                response: Response::Changed(id),
                ..
            } => {
                // The change applies to the file as it is now: a `settings`
                // command run meanwhile is kept (review 7f, 4).
                let shown = s.clone();
                s = zoraya_compositor::settings::read(&current(&config)).0;
                match app.window(window) {
                    // The layout in use is chosen by its place in the list
                    // the page showed: that layout, if the file still
                    // enables it (review 7f's check).
                    Some(w) if id == zoraya_settings_window::ACTIVE => {
                        match changed(&shown, id, w.value(id)) {
                            Ok(want) if s.layouts.contains(&want.layout) => {
                                let mut n = s.clone();
                                n.layout = want.layout;
                                Ok(n)
                            }
                            Ok(_) => Err("that layout is no longer enabled"),
                            Err(e) => Err(e),
                        }
                    }
                    Some(w) => changed(&s, id, w.value(id)),
                    None => continue,
                }
            }
            _ => continue,
        };
        let status: String = match result {
            Err(why) => why.into(),
            Ok(new) => {
                let t0 = clock_monotonic();
                let bytes = current(&config);
                match written(&bytes, &new).map(|f| publish(&config, &f)) {
                    Ok(Ok(())) => {
                        s = new;
                        let t1 = clock_monotonic();
                        let applied = announce(panel);
                        // The change's cost, for design.md: the file read,
                        // written and published, then the panel's and the
                        // compositor's round trip.
                        let t2 = clock_monotonic();
                        println!(
                            "settings: measure: written {} us, applied {} us",
                            (t1 - t0) / 1000,
                            (t2 - t1) / 1000
                        );
                        if applied {
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
        app.replace(window, page_with_rooms(&s, &rows, &drafts, &status));
    }
}

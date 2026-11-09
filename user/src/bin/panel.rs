#![no_std]
#![no_main]

//! The panel (ADR-0168, ADR-0172 point 4): the desktop's launcher. It holds
//! the compositor's client manager at slot 6 and the volume's root at
//! slot 5. It reads the room table in `/config/rooms`, tells the
//! compositor the table, the current room and the dock's applications,
//! and launches an application when the person presses its icon (or
//! System+Enter for a terminal) into the current room, giving each program
//! exactly the handles its launch record lists (`zoraya-panel`, INV-0168-1)
//! and telling the compositor the record with the client (`AddRoomClient`).
//!
//! Stage 7d: `init` still starts the first terminal and its shell, with
//! its restart rules; the panel starts every other (proposed ADR-0242).

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelEntry, ChannelObject, DIRECTORY, DirectoryObject, Handle, JOB, JobFlags,
    JobObject, Mode, ObjectType, Packet, Rights, Status, channel_signals,
};
use zoraya_panel::{Action, Give, Launch, Panel};
use zoraya_proto_gui as gui;
use zoraya_protocol::{Data, Message, NO_REPLY, decode, encode};
use zoraya_user::files::{File, read_all};
use zoraya_user::{
    channel_create, channel_read, channel_write, clock_monotonic, directory_lookup, dup,
    handle_close, handle_type, job_create, port_create, port_subscribe, port_wait, println, spawn,
};

const KEY_MANAGER: u64 = 1;
/// How often the room table is read again, for a `room new` in a shell
/// (the shell has no channel to the panel, ADR-0172's open question).
const RELOAD_NS: u64 = 1_000_000_000;
const ROOMS: &str = "config/rooms";
// The panel keeps the table it can tell the compositor (review 7d, 6).
const _: () = assert!(zoraya_panel::MAX_ROOMS_TEXT == gui::MAX_ROOMS_TEXT);

fn send<'a, M: Message<'a>>(manager: Handle, m: &M, handle: Option<Handle>) -> bool {
    let mut out = alloc::vec![0u8; 8192];
    let Ok(n) = encode(m, NO_REPLY, &mut out) else {
        if let Some(h) = handle {
            let _ = handle_close(h);
        }
        return false;
    };
    let entries: Vec<ChannelEntry> = handle
        .into_iter()
        .map(|h| ChannelEntry {
            handle: h,
            rights: ChannelObject::RIGHTS,
            transfer: Transfer::Move,
        })
        .collect();
    if channel_write(manager, &out[..n], &entries).is_err() {
        if let Some(h) = handle {
            let _ = handle_close(h);
        }
        return false;
    }
    true
}

fn tell_rooms(manager: Handle, panel: &Panel) {
    let text = panel.rooms_text();
    let m = gui::SetRooms {
        current: Data(panel.current_name().as_bytes()),
        rooms: Data(text.as_bytes()),
    };
    if !send(manager, &m, None) {
        println!("panel: the room table is too large for the compositor");
    }
}

fn reload(panel: &mut Panel, root: Option<&File>) -> bool {
    let bytes = root
        .and_then(|r| r.open(ROOMS, 0, zoraya_proto_file::rights::READ).ok())
        .and_then(|f| read_all(&f).ok())
        .unwrap_or_default();
    panel.reload(&bytes)
}

fn moved(slot: u32, handle: Handle) -> HandleEntry {
    HandleEntry {
        slot,
        handle,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    }
}

/// Spawns one program of a launch with exactly the handles its record
/// lists; `take` gives the channel ends and the root for the slots that
/// carry them.
fn spawn_program(
    image: &str,
    handles: &[zoraya_panel::Slot],
    args: &[String],
    mut take: impl FnMut(Give) -> Option<Handle>,
) -> bool {
    let Ok(job) = job_create(JOB, JobFlags::NONE) else {
        return false;
    };
    let Ok(file) = directory_lookup(DIRECTORY, image) else {
        let _ = handle_close(job);
        return false;
    };
    let mut buffer = [0u8; 1024];
    let mut e = Encoder::new(&mut buffer, job, file);
    // Handles moved in: a failed spawn leaves them to the caller
    // (INV-0042-3), which closes them here.
    let mut taken = Vec::new();
    for s in handles {
        let entry = match s.give {
            Give::Console => dup(s.slot, CONSOLE, Rights::WRITE),
            Give::Job => dup(s.slot, job, JobObject::RIGHTS),
            Give::Directory => dup(s.slot, DIRECTORY, DirectoryObject::RIGHTS),
            other => match take(other) {
                Some(h) => {
                    taken.push(h);
                    moved(s.slot, h)
                }
                None => continue,
            },
        };
        e.handle(entry);
    }
    for a in args {
        e.arg(a.as_bytes());
    }
    let ok = e.finish().is_some_and(|n| spawn(&buffer[..n]).is_ok());
    let _ = handle_close(file);
    if !ok {
        for h in taken {
            let _ = handle_close(h);
        }
    }
    // The job stays with the program: ending the panel does not end it
    // (ADR-0168: launched applications keep running).
    let _ = handle_close(job);
    ok
}

/// What the panel keeps between launches.
struct State {
    manager: Handle,
    root: Option<File>,
    /// The chooser's registration channel (proposed ADR-0245), from `init`.
    chooser: Option<Handle>,
    next_tag: u32,
    port: Handle,
    /// The channels of running file managers, watched for `OpenWith`.
    launchers: Vec<Option<Handle>>,
}

const KEY_LAUNCHER: u64 = 100;
/// The rights of a data directory: ADR-0153's `tree`.
const TREE: u32 = zoraya_proto_file::rights::ALL & !zoraya_proto_file::rights::PUBLISH;

/// The application's data directory, made if it is not there.
fn data_dir(root: &File, path: &str) -> Option<File> {
    let mut at = String::new();
    for part in path.trim_start_matches('/').split('/') {
        if !at.is_empty() {
            at.push('/');
        }
        at.push_str(part);
        if root
            .open(&at, zoraya_proto_file::open_flags::DIRECTORY, TREE)
            .is_err()
        {
            let (parent, name) = at.rsplit_once('/').unwrap_or(("", at.as_str()));
            let made = if parent.is_empty() {
                root.make_directory(name)
            } else {
                root.open(parent, zoraya_proto_file::open_flags::DIRECTORY, TREE)
                    .and_then(|d| d.make_directory(name))
            };
            if made.is_err() {
                return None;
            }
        }
    }
    root.open(&at, zoraya_proto_file::open_flags::DIRECTORY, TREE)
        .ok()
}

/// Registers an application's chooser channel under its tag; gives the
/// application's end.
fn chooser_channel(chooser: Handle, tag: u32, app: &str) -> Option<Handle> {
    let (app_end, chooser_end) = channel_create().ok()?;
    let m = zoraya_proto_chooser::AddApplication {
        tag,
        name: Data(app.as_bytes()),
    };
    let mut out = alloc::vec![0u8; 512];
    let n = encode(&m, 1, &mut out).ok()?;
    let entry = ChannelEntry {
        handle: chooser_end,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    };
    if channel_write(chooser, &out[..n], &[entry]).is_err() {
        let _ = handle_close(chooser_end);
        let _ = handle_close(app_end);
        return None;
    }
    let mut reply = [0u8; 64];
    let ok = zoraya_user::gui::wait_reply(chooser, &mut reply, 2_000_000_000)
        .and_then(|n| decode::<zoraya_proto_chooser::Added>(&reply[..n], 0).ok())
        .is_some_and(|(_, a)| a.status.0 == 0);
    if ok {
        Some(app_end)
    } else {
        let _ = handle_close(app_end);
        None
    }
}

/// A launch (ADR-0168): the compositor learns the client with its record
/// and its tag, then the record's programs are spawned with exactly the
/// handles it lists; `opened` is the file the person opened, if any.
fn launch(st: &mut State, l: &Launch, mut opened: Option<Handle>) {
    let Ok((client, compositor_end)) = channel_create() else {
        return;
    };
    let tag = st.next_tag;
    st.next_tag = st.next_tag.wrapping_add(1).max(1);
    let entries = l.entries.join("\n");
    let room = l.room.as_ref();
    let m = gui::AddRoomClient {
        untrusted: u32::from(room.is_some_and(|r| r.untrusted)),
        colour: room.map_or(0, |r| u32::from(r.colour)),
        tag,
        room: Data(room.map_or(&b""[..], |r| r.name.as_bytes())),
        app: Data(l.app.as_bytes()),
        entries: Data(entries.as_bytes()),
        line: Data(l.line.as_bytes()),
    };
    if !send(st.manager, &m, Some(compositor_end)) {
        println!("panel: the launch record is too large for the compositor");
        let _ = handle_close(client);
        if let Some(h) = opened {
            let _ = handle_close(h);
        }
        return;
    }
    let terminal = if l.app == "term" {
        channel_create().ok()
    } else {
        None
    };
    let mut client = Some(client);
    let mut ends = [terminal.map(|t| t.0), terminal.map(|t| t.1)];
    // What the shell holds: its room's grants or home's, never the root
    // (ADR-0242's condition); it starts in the first it may change.
    let home = st.root.as_ref().map(zoraya_user::namespace::shell_home);
    let cwd = l
        .holds
        .iter()
        .find(|(_, w)| *w)
        .map(|(p, _)| p.as_str())
        .unwrap_or("/");
    let data_path = zoraya_panel::data_dir(room, l.app);
    for (i, p) in l.programs.iter().enumerate() {
        let root = st.root.as_ref();
        let chooser = st.chooser;
        let mut launcher = None;
        let ok = spawn_program(p.image, &p.handles, &p.args, |g| match g {
            Give::Compositor => client.take(),
            Give::Terminal => ends[i.min(1)].take(),
            Give::Startup => {
                let home = home.as_deref()?;
                let start = zoraya_user::namespace::expand(cwd, home);
                zoraya_user::namespace::record(root?, home, &start, &l.holds)
            }
            Give::Chooser => chooser_channel(chooser?, tag, l.app),
            Give::Data => {
                let d = data_dir(root?, &data_path)?;
                let h = d.end;
                core::mem::forget(d);
                Some(h)
            }
            Give::Root => {
                let r = root?.clone_as(zoraya_user::files::ALL).ok()?;
                let h = r.end;
                core::mem::forget(r);
                Some(h)
            }
            Give::Launcher => {
                let (ours, theirs) = channel_create().ok()?;
                launcher = Some(ours);
                Some(theirs)
            }
            Give::Opened => opened.take(),
            // `/config` narrowed to the one file (ADR-0169).
            Give::Config => {
                let rights = zoraya_panel::CONFIG_RIGHTS;
                let config = root?
                    .open("config", zoraya_proto_file::open_flags::DIRECTORY, rights)
                    .ok()?;
                let f = config.narrow("settings", rights).ok()?;
                let h = f.end;
                core::mem::forget(f);
                Some(h)
            }
            _ => None,
        });
        if let Some(ours) = launcher {
            if ok {
                watch_launcher(st, ours);
            } else {
                let _ = handle_close(ours);
            }
        }
        if !ok {
            println!("panel: {} did not start", p.image);
        }
    }
    for h in client
        .into_iter()
        .chain(ends.into_iter().flatten())
        .chain(opened)
    {
        let _ = handle_close(h);
    }
}

fn watch_launcher(st: &mut State, end: Handle) {
    let i = match st.launchers.iter().position(Option::is_none) {
        Some(i) => i,
        None => {
            st.launchers.push(None);
            st.launchers.len() - 1
        }
    };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(st.port, end, wanted, KEY_LAUNCHER + i as u64, Mode::Level).is_ok() {
        st.launchers[i] = Some(end);
    } else {
        let _ = handle_close(end);
    }
}

/// `OpenWith` from a file manager: the file's application is launched in
/// `home` with the file, read only (ADR-0167 point 1, ADR-0168 point 4).
/// `/config/settings` as the compositor reads it, sent as `ApplySettings`.
fn apply_settings(st: &State) -> bool {
    let Some(root) = st.root.as_ref() else {
        return false;
    };
    let bytes = root
        .open("config/settings", 0, zoraya_proto_file::rights::READ)
        .ok()
        .and_then(|f| zoraya_user::files::read_all(&f).ok())
        .unwrap_or_default();
    let (s, _) = zoraya_compositor::settings::read(&bytes);
    let w = zoraya_compositor::settings::words(&s);
    let m = gui::ApplySettings {
        theme: Data(w[0].as_bytes()),
        motion: Data(w[1].as_bytes()),
        scale: Data(w[2].as_bytes()),
        wallpaper: Data(w[3].as_bytes()),
        layouts: Data(w[4].as_bytes()),
        layout: Data(w[5].as_bytes()),
    };
    send(st.manager, &m, None)
}

fn launcher_requests(st: &mut State, i: usize) {
    let Some(end) = st.launchers[i] else { return };
    let mut bytes = alloc::vec![0u8; 2048];
    loop {
        let mut handles = [end; 1];
        let sizes = match channel_read(end, &mut bytes, &mut handles) {
            Ok(s) => s,
            Err((Status::WouldBlock, _)) => return,
            // Gone, or a message it cannot read (too large): it would stay
            // at the head and wake the panel for ever (review 7e, 10).
            Err(_) => {
                let _ = zoraya_user::port_unsubscribe(st.port, KEY_LAUNCHER + i as u64);
                let _ = handle_close(end);
                st.launchers[i] = None;
                return;
            }
        };
        let got: Vec<Handle> = handles[..sizes.handles as usize].to_vec();
        let msg = &bytes[..sizes.bytes as usize];
        let t = zoraya_protocol::Header::decode(msg).map_or(0, |(h, _)| h.transaction);
        // The settings window changed /config/settings (ADR-0169): read it
        // and tell the compositor, which checks every value again.
        if decode::<zoraya_proto_chooser::SettingsChanged>(msg, got.len()).is_ok() {
            let sent = apply_settings(st);
            let r = zoraya_proto_chooser::SettingsApplied {
                status: zoraya_protocol::Below(if sent {
                    zoraya_proto_chooser::status::CANCELLED
                } else {
                    zoraya_proto_chooser::status::FAILED
                }),
            };
            let mut out = [0u8; 64];
            if let Ok(n) = encode(&r, t, &mut out) {
                let _ = channel_write(end, &out[..n], &[]);
            }
            continue;
        }
        let status = match decode::<zoraya_proto_chooser::OpenWith<'_>>(msg, got.len()) {
            Ok((_, m)) => {
                let name = core::str::from_utf8(m.name.0).unwrap_or("");
                let words = core::str::from_utf8(m.words.0).unwrap_or("");
                match zoraya_panel::opener(name) {
                    Some(app) if !words.chars().any(char::is_control) => {
                        // Whatever the sender gave, the application gets
                        // the file to read only (ADR-0153's `+r`).
                        let given = zoraya_user::files::File { end: got[0] };
                        let read = given.clone_as(
                            zoraya_proto_file::rights::READ | zoraya_proto_file::rights::CLONE,
                        );
                        drop(given);
                        match read {
                            Ok(f) => {
                                let h = f.end;
                                core::mem::forget(f);
                                let l = zoraya_panel::application(None, app, Some(words));
                                launch(st, &l, Some(h));
                                0
                            }
                            Err(_) => zoraya_proto_chooser::status::FAILED,
                        }
                    }
                    _ => {
                        let _ = handle_close(got[0]);
                        zoraya_proto_chooser::status::FAILED
                    }
                }
            }
            Err(_) => {
                for h in &got {
                    let _ = handle_close(*h);
                }
                zoraya_proto_chooser::status::INVALID
            }
        };
        let r = zoraya_proto_chooser::Opening {
            status: zoraya_protocol::Below(status),
        };
        let mut out = [0u8; 64];
        if let Ok(n) = encode(&r, t, &mut out) {
            let _ = channel_write(end, &out[..n], &[]);
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let channel = |slot: u64| Handle::new(slot).filter(|h| handle_type(*h) == Ok("channel"));
    let Some(manager) = channel(6) else {
        println!("panel: no client manager");
        return 1;
    };
    let root = channel(5).map(|end| File { end });
    let mut panel = Panel::default();
    reload(&mut panel, root.as_ref());
    tell_rooms(manager, &panel);
    let dock = zoraya_panel::dock_text();
    send(
        manager,
        &gui::SetDock {
            apps: Data(dock.as_bytes()),
        },
        None,
    );
    let Ok(port) = port_create() else { return 1 };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, manager, wanted, KEY_MANAGER, Mode::Level).is_err() {
        return 1;
    }
    let mut st = State {
        manager,
        root,
        chooser: channel(7),
        next_tag: 1,
        port,
        launchers: Vec::new(),
    };
    let mut next_reload = clock_monotonic() + RELOAD_NS;
    let mut bytes = alloc::vec![0u8; 512];
    loop {
        let mut packets = [Packet::default(); 4];
        let _ = port_wait(port, next_reload, &mut packets);
        loop {
            let mut handles = [manager; 1];
            let sizes = match channel_read(manager, &mut bytes, &mut handles) {
                Ok(s) => s,
                // The compositor is gone: so is the desktop.
                Err((Status::PeerClosed, _)) => return 0,
                Err(_) => break,
            };
            for h in &handles[..sizes.handles as usize] {
                let _ = handle_close(*h);
            }
            let Ok((_, d)) = decode::<gui::DockPressed>(&bytes[..sizes.bytes as usize], 0) else {
                continue;
            };
            // The press against the table the compositor drew (review 7d,
            // 6); the file is read again after it.
            match panel.pressed(d.kind, d.index) {
                Action::Launch(l) => launch(&mut st, &l, None),
                Action::Entered => tell_rooms(manager, &panel),
                Action::Nothing => {}
            }
            if reload(&mut panel, st.root.as_ref()) {
                tell_rooms(manager, &panel);
            }
        }
        for i in 0..st.launchers.len() {
            launcher_requests(&mut st, i);
        }
        if clock_monotonic() >= next_reload {
            if reload(&mut panel, st.root.as_ref()) {
                tell_rooms(manager, &panel);
            }
            next_reload = clock_monotonic() + RELOAD_NS;
        }
    }
}

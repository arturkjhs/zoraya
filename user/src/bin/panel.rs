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

/// A launch (ADR-0168): the compositor learns the client with its record,
/// then the record's programs are spawned.
fn launch(l: &Launch, manager: Handle, root: Option<&File>) {
    let Ok((client, compositor_end)) = channel_create() else {
        return;
    };
    let entries = l.entries.join("\n");
    let room = l.room.as_ref();
    let m = gui::AddRoomClient {
        untrusted: u32::from(room.is_some_and(|r| r.untrusted)),
        colour: room.map_or(0, |r| u32::from(r.colour)),
        tag: 0,
        room: Data(room.map_or(&b""[..], |r| r.name.as_bytes())),
        app: Data(l.app.as_bytes()),
        entries: Data(entries.as_bytes()),
        line: Data(l.line.as_bytes()),
    };
    if !send(manager, &m, Some(compositor_end)) {
        println!("panel: the launch record is too large for the compositor");
        let _ = handle_close(client);
        return;
    }
    let Ok((term_end, shell_end)) = channel_create() else {
        let _ = handle_close(client);
        return;
    };
    let mut client = Some(client);
    let mut ends = [Some(term_end), Some(shell_end)];
    // What the shell holds: its room's grants or home's, never the root
    // (ADR-0242's condition); it starts in the first it may change.
    let home = root.map(zoraya_user::namespace::shell_home);
    let cwd = l
        .holds
        .iter()
        .find(|(_, w)| *w)
        .map(|(p, _)| p.as_str())
        .unwrap_or("/");
    for (i, p) in l.programs.iter().enumerate() {
        let ok = spawn_program(p.image, &p.handles, &p.args, |g| match g {
            Give::Compositor => client.take(),
            Give::Terminal => ends[i.min(1)].take(),
            Give::Startup => {
                let home = home.as_deref()?;
                let start = zoraya_user::namespace::expand(cwd, home);
                zoraya_user::namespace::record(root?, home, &start, &l.holds)
            }
            _ => None,
        });
        if !ok {
            println!("panel: {} did not start", p.image);
        }
    }
    for h in client.into_iter().chain(ends.into_iter().flatten()) {
        let _ = handle_close(h);
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
    let mut next_reload = clock_monotonic() + RELOAD_NS;
    let mut bytes = alloc::vec![0u8; 512];
    loop {
        let mut packets = [Packet::default()];
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
                Action::Launch(l) => launch(&l, manager, root.as_ref()),
                Action::Entered => tell_rooms(manager, &panel),
                Action::Nothing => {}
            }
            if reload(&mut panel, root.as_ref()) {
                tell_rooms(manager, &panel);
            }
        }
        if clock_monotonic() >= next_reload {
            if reload(&mut panel, root.as_ref()) {
                tell_rooms(manager, &panel);
            }
            next_reload = clock_monotonic() + RELOAD_NS;
        }
    }
}

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
use zoraya_settings::rooms::Room;
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
// The wire's caps are the worst case of what the panel sends (Q6b review):
// `to_wire` leaves no room out for want of space.
const _: () = {
    assert!(gui::MAX_USAGE_TEXT >= zoraya_settings::usage::MAX_USAGE_WIRE);
    assert!(zoraya_proto_chooser::MAX_ROOMS_REPLY >= zoraya_settings::usage::MAX_ROWS_WIRE);
};

fn send<'a, M: Message<'a>>(manager: Handle, m: &M, handle: Option<Handle>) -> bool {
    let mut out = alloc::vec![0u8; 16384];
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

/// Reads every room's use from the keeper and tells the compositor, which
/// draws the pills at 90% and the hover card from it (ADR-0267 point 3).
/// Called once a second at most (the loop's reload tick; INV-0267-2); a
/// keeper that does not answer gives the compositor an empty list, so no
/// pill outlives the numbers it came from.
fn tell_usage(st: &mut State) {
    let Some(keeper) = st.keeper.as_mut() else {
        return;
    };
    let t0 = clock_monotonic();
    let text = match keeper.usage("") {
        Ok(records) => {
            let readings = st.tracker.readings(&records, clock_monotonic());
            let text = zoraya_settings::usage::to_wire(&readings, gui::MAX_USAGE_TEXT);
            st.readings = readings;
            text
        }
        Err(_) => {
            st.readings.clear();
            String::new()
        }
    };
    let t1 = clock_monotonic();
    // The first five readings' cost, once (the measurement of Q6b).
    let changed = text != st.told_usage;
    if changed {
        let m = gui::SetUsage {
            readings: Data(text.as_bytes()),
        };
        if send(st.manager, &m, None) {
            st.told_usage = text;
        }
    }
    let t2 = clock_monotonic();
    if st.cost.2 < 5 {
        st.cost.0 += t1 - t0;
        st.cost.1 += t2 - t1;
        st.cost.2 += 1;
        if st.cost.2 == 5 {
            println!(
                "panel: measure: usage read {} us, told {} us (average of 5 ticks)",
                st.cost.0 / 5000,
                st.cost.1 / 5000
            );
        }
    }
}

/// `/config/rooms` as it is now.
fn rooms_file(st: &State) -> Vec<u8> {
    st.root
        .as_ref()
        .and_then(|r| r.open(ROOMS, 0, zoraya_proto_file::rights::READ).ok())
        .and_then(|f| read_all(&f).ok())
        .unwrap_or_default()
}

/// The table written as a new file published over the old, or in place
/// where the root holds no publish (as the shell does).
fn write_rooms(st: &State, bytes: &[u8]) -> Result<(), String> {
    use zoraya_user::files::{ALL, DIRECTORY};
    let root = st.root.as_ref().ok_or("no volume")?;
    let dir = root
        .open("config", DIRECTORY, ALL)
        .map_err(|_| String::from("/config cannot be opened"))?;
    let published = dir.create_unnamed().and_then(|file| {
        if !bytes.is_empty() {
            file.write(0, 0, bytes)?;
        }
        file.publish(&dir, "rooms")
    });
    match published {
        Ok(()) => Ok(()),
        Err(zoraya_proto_file::status::ACCESS_DENIED) => {
            let file = root
                .open(
                    ROOMS,
                    zoraya_proto_file::open_flags::CREATE | zoraya_proto_file::open_flags::TRUNCATE,
                    ALL,
                )
                .map_err(|_| String::from("/config/rooms cannot be written"))?;
            file.write(0, 0, bytes)
                .map(|_| ())
                .map_err(|_| String::from("/config/rooms cannot be written"))
        }
        Err(_) => Err(String::from("/config/rooms cannot be written")),
    }
}

/// The rooms of the table with their limits and use, for the settings
/// window's Rooms section (ADR-0267 point 4).
fn rooms_rows(st: &State) -> String {
    let table = zoraya_settings::rooms::read(&rooms_file(st));
    let rows: Vec<zoraya_settings::usage::RoomRow> = table
        .rooms
        .iter()
        .map(|r| zoraya_settings::usage::RoomRow {
            name: r.name.clone(),
            untrusted: r.untrusted,
            limits: r.limits,
            reading: st.readings.iter().find(|x| x.name == r.name).cloned(),
        })
        .collect();
    zoraya_settings::usage::rows_to_wire(&rows, zoraya_proto_chooser::MAX_ROOMS_REPLY)
}

/// `room set ROOM WORD VALUE` for the settings window: the logic is
/// `zoraya_panel::set_room_limit`; this gives it the file and the keeper.
fn set_room_limit(
    st: &mut State,
    from: &Option<Room>,
    room: &str,
    word: &str,
    value: &str,
) -> Result<(), String> {
    let untrusted = from.as_ref().is_some_and(|r| r.untrusted);
    // The three closures share the state through a cell.
    let st = core::cell::RefCell::new(st);
    zoraya_panel::set_room_limit(
        untrusted,
        room,
        word,
        value,
        || rooms_file(&st.borrow()),
        |b| write_rooms(&st.borrow(), b),
        Some(|name: &str| {
            let mut st = st.borrow_mut();
            let Some(k) = st.keeper.as_mut() else {
                return Ok(());
            };
            k.set(name).map_err(|e| match e {
                zoraya_user::roomsclient::Fail::Denied(_, t) => zoraya_panel::KeeperFail::Denied(t),
                zoraya_user::roomsclient::Fail::Gone => zoraya_panel::KeeperFail::Gone,
            })
        }),
    )
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
    parent: Handle,
    image: &str,
    handles: &[zoraya_panel::Slot],
    args: &[String],
    mut take: impl FnMut(Give) -> Option<Handle>,
) -> bool {
    let Ok(job) = job_create(parent, JobFlags::NONE) else {
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
    /// Each launcher channel, the application it was given to and the room
    /// it was launched in (`None` is `home`): `files` may ask `OpenWith` and
    /// `OpenTerminal`, `settings` `SettingsChanged`, nothing else (review
    /// 7f, 8).
    launchers: Vec<Option<(Handle, &'static str, Option<Room>)>>,
    /// How many windows each launcher channel (a shell's) has been given
    /// (ADR-0317: at most `MAX_WINDOWS` a shell).
    windows: Vec<u8>,
    /// The room keeper (ADR-0269): the launch job of every launch comes
    /// from it. Without one, a trusted room launches in the panel's own job.
    keeper: Option<zoraya_user::roomsclient::Keeper>,
    /// The CPU share of each room since the last reading (Q6b).
    tracker: zoraya_rooms::Tracker,
    /// The last readings, for the settings window's Rooms section.
    readings: Vec<zoraya_settings::usage::Reading>,
    /// What the compositor was last told of the rooms' use, so that a
    /// calm second sends nothing.
    told_usage: String,
    /// What the once-a-second reading costs, for `docs/design.md`: the
    /// time of `Usage` and the readings' text, and of the send, summed over
    /// the first ticks.
    cost: (u64, u64, u32),
}

const KEY_LAUNCHER: u64 = 100;
/// The windows of POSIX programs one shell may be given in its life.
const MAX_WINDOWS: u8 = 8;
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
    // `files` never gets /config to change, whatever `~` is (INV-0169-4).
    let mut confined = l.clone();
    if let Some(root) = st.root.as_ref() {
        zoraya_panel::confine_files(&mut confined, &zoraya_user::namespace::shell_home(root));
    }
    let l = &confined;
    // The room's one job (ADR-0269): a launch job from the keeper, asked for
    // before anything is made. A refusal is the whole launch's (an untrusted
    // room is used from one launcher at a time). Without a keeper a trusted
    // room launches in the panel's own job, as before.
    let room_name = l.room.as_ref().map_or("home", |r| r.name.as_str());
    let untrusted = l.room.as_ref().is_some_and(|r| r.untrusted);
    let launch_job = match st.keeper.as_mut().map(|k| k.launch(room_name)) {
        Some(Ok(job)) => Some(job),
        Some(Err(why)) => {
            println!("panel: room {room_name}: {why:?}");
            if let Some(h) = opened {
                let _ = handle_close(h);
            }
            return;
        }
        None if untrusted => {
            println!("panel: room {room_name}: an untrusted room needs the room keeper");
            if let Some(h) = opened {
                let _ = handle_close(h);
            }
            return;
        }
        // No keeper: the line's --mem, --procs and --cpu cannot be applied,
        // and a room that cannot have them is not entered.
        None if l.room.as_ref().is_some_and(|r| r.limits.needs_job()) => {
            println!("panel: room {room_name}: its limits need the room keeper");
            if let Some(h) = opened {
                let _ = handle_close(h);
            }
            return;
        }
        None => None,
    };
    let Ok((client, compositor_end)) = channel_create() else {
        if let Some(j) = launch_job {
            let _ = handle_close(j);
        }
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
    let cwd = l.cwd.as_deref().unwrap_or_else(|| {
        l.holds
            .iter()
            .find(|(_, w)| *w)
            .map(|(p, _)| p.as_str())
            .unwrap_or("/")
    });
    let data_path = zoraya_panel::data_dir(room, l.app);
    // A broker beside the shell, on a volume with /bin (ADR-0153 rule 10):
    // it ends when the shell's client closes and nothing it started runs.
    let wants_broker = l
        .programs
        .iter()
        .any(|p| p.handles.iter().any(|s| s.give == Give::Broker));
    let parent = launch_job.unwrap_or(JOB);
    let beside = if wants_broker {
        st.root
            .as_ref()
            .and_then(|r| zoraya_user::beside::start_broker_in(r, parent))
    } else {
        None
    };
    let (mut broker, mut tmp) = (
        beside.as_ref().map(|b| b.client),
        beside.as_ref().map(|b| b.tmp),
    );
    for (i, p) in l.programs.iter().enumerate() {
        let root = st.root.as_ref();
        let chooser = st.chooser;
        let mut launcher = None;
        let ok = spawn_program(parent, p.image, &p.handles, &p.args, |g| match g {
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
            Give::Broker => broker.take(),
            // A `Room(R)` channel for the shell in a room (ADR-0269 point 3).
            Give::Keeper => st.keeper.as_mut().and_then(|k| k.open(room_name).ok()),
            Give::Tmp => tmp.take(),
            _ => None,
        });
        if let Some(ours) = launcher {
            if ok {
                watch_launcher(st, ours, l.app, l.room.clone());
            } else {
                let _ = handle_close(ours);
            }
        }
        if !ok {
            println!("panel: {} did not start", p.image);
        }
    }
    let unused = client
        .into_iter()
        .chain(ends.into_iter().flatten())
        .chain(opened);
    for h in unused.chain(broker).chain(tmp) {
        let _ = handle_close(h);
    }
    // The broker's job stays with it, as a launched program's does; with
    // its client unused (the shell did not start) it ends at once.
    if let Some(b) = beside {
        let _ = handle_close(b.job.0);
        let _ = handle_close(b.job.1);
    }
    // Done launching: the keeper may let an untrusted room go to the other
    // launcher once its processes have ended.
    if let Some(j) = launch_job {
        let _ = handle_close(j);
        if let Some(k) = st.keeper.as_mut() {
            let _ = k.leave(room_name);
        }
    }
}

fn watch_launcher(st: &mut State, end: Handle, app: &'static str, room: Option<Room>) {
    let i = match st.launchers.iter().position(Option::is_none) {
        Some(i) => i,
        None => {
            st.launchers.push(None);
            st.launchers.len() - 1
        }
    };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if st.windows.len() <= i {
        st.windows.resize(i + 1, 0);
    }
    st.windows[i] = 0;
    if port_subscribe(st.port, end, wanted, KEY_LAUNCHER + i as u64, Mode::Level).is_ok() {
        st.launchers[i] = Some((end, app, room));
    } else {
        let _ = handle_close(end);
    }
}

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

/// The window of a POSIX program for the shell of launcher `i` in `room`
/// (ADR-0317): the compositor learns the client with the room's record and
/// the line's pills; the client end is returned to move to the shell.
fn open_window(
    st: &mut State,
    i: usize,
    room: Option<&Room>,
    app: &str,
    title: &str,
    entries: &str,
) -> Result<Handle, u32> {
    use zoraya_proto_chooser::window_refusal as why;
    let record = zoraya_panel::window_record(room, app, title, entries)?;
    if st.windows.get(i).copied().unwrap_or(MAX_WINDOWS) >= MAX_WINDOWS {
        return Err(why::BUSY);
    }
    let (client, compositor_end) = channel_create().map_err(|_| why::FAILED)?;
    let tag = st.next_tag;
    st.next_tag = st.next_tag.wrapping_add(1).max(1);
    let m = gui::AddRoomClient {
        untrusted: 0,
        colour: record.colour,
        tag,
        room: Data(record.room.as_bytes()),
        app: Data(record.app.as_bytes()),
        entries: Data(record.entries.as_bytes()),
        line: Data(record.line.as_bytes()),
    };
    if !send(st.manager, &m, Some(compositor_end)) {
        let _ = handle_close(client);
        return Err(why::FAILED);
    }
    st.windows[i] += 1;
    Ok(client)
}

/// `OpenWith` from the file manager: the file's application is launched in
/// `home` with the file, read only (ADR-0167 point 1, ADR-0168 point 4).
/// `SettingsChanged` from the settings window: the file is read and sent
/// to the compositor.
fn launcher_requests(st: &mut State, i: usize) {
    let Some((end, app, room)) = st.launchers[i].clone() else {
        return;
    };
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
        if app == "settings"
            && decode::<zoraya_proto_chooser::SettingsChanged>(msg, got.len()).is_ok()
        {
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
        // The Rooms section of the settings window (ADR-0267 point 4): the
        // table with its limits and use, and one word changed through the
        // same code as `room set`. Only the settings window's channel.
        if app == "settings" {
            if decode::<zoraya_proto_chooser::RoomsQuery>(msg, got.len()).is_ok() {
                let text = rooms_rows(st);
                let r = zoraya_proto_chooser::RoomsReply {
                    text: Data(text.as_bytes()),
                };
                let mut out = alloc::vec![0u8; zoraya_proto_chooser::MAX_ROOMS_REPLY + 64];
                if let Ok(n) = encode(&r, t, &mut out) {
                    let _ = channel_write(end, &out[..n], &[]);
                }
                continue;
            }
            if let Ok((_, m)) = decode::<zoraya_proto_chooser::SetRoomLimit<'_>>(msg, got.len()) {
                let text = |d: &[u8]| String::from(core::str::from_utf8(d).unwrap_or("?"));
                let (r, w, v) = (text(m.room.0), text(m.word.0), text(m.value.0));
                let done = set_room_limit(st, &room, &r, &w, &v);
                let reply_text = done.clone().err().unwrap_or_default();
                let mut clipped = reply_text.as_str();
                while clipped.len() > 160 {
                    clipped = &clipped[..clipped.floor_char_boundary(clipped.len() - 1)];
                }
                let reply = zoraya_proto_chooser::RoomLimitSet {
                    status: zoraya_protocol::Below(u32::from(done.is_err())),
                    text: Data(clipped.as_bytes()),
                };
                let mut out = [0u8; 256];
                if let Ok(n) = encode(&reply, t, &mut out) {
                    let _ = channel_write(end, &out[..n], &[]);
                }
                continue;
            }
        }
        // A shell asks for the window of a POSIX program (ADR-0317). Only
        // the channel of a `term` launch is read for it.
        if app == "term"
            && let Ok((_, m)) = decode::<zoraya_proto_chooser::OpenWindow<'_>>(msg, got.len())
        {
            fn text(d: &[u8]) -> &str {
                core::str::from_utf8(d).unwrap_or("\u{1}")
            }
            let given = open_window(
                st,
                i,
                room.as_ref(),
                text(m.app.0),
                text(m.title.0),
                text(m.entries.0),
            );
            match given {
                Ok(client) => {
                    let mut out = [0u8; 64];
                    if let Ok(n) = encode(&zoraya_proto_chooser::WindowOpened {}, t, &mut out) {
                        let entry = ChannelEntry {
                            handle: client,
                            rights: ChannelObject::RIGHTS,
                            transfer: Transfer::Move,
                        };
                        if channel_write(end, &out[..n], &[entry]).is_err() {
                            let _ = handle_close(client);
                        }
                    } else {
                        let _ = handle_close(client);
                    }
                }
                Err(status) => {
                    let r = zoraya_proto_chooser::WindowRefused {
                        status: zoraya_protocol::Below(status),
                    };
                    let mut out = [0u8; 64];
                    if let Ok(n) = encode(&r, t, &mut out) {
                        let _ = channel_write(end, &out[..n], &[]);
                    }
                }
            }
            continue;
        }
        // A terminal at a folder, in the file manager's own room (proposed
        // ADR-0248): only where that room's holds reach.
        if app == "files"
            && let Ok((_, m)) = decode::<zoraya_proto_chooser::OpenTerminal<'_>>(msg, got.len())
        {
            let path = core::str::from_utf8(m.path.0).unwrap_or("");
            let home = st.root.as_ref().map(zoraya_user::namespace::shell_home);
            // The room it was launched in, as it was then: never home's
            // holds for a room since gone or renamed (review 7h, 4).
            let status = match zoraya_panel::terminal_at(
                room.as_ref(),
                path,
                home.as_deref().unwrap_or("/home"),
            ) {
                Some(l) => {
                    launch(st, &l, None);
                    0
                }
                None => zoraya_proto_chooser::status::FAILED,
            };
            let r = zoraya_proto_chooser::TerminalOpened {
                status: zoraya_protocol::Below(status),
            };
            let mut out = [0u8; 64];
            if let Ok(n) = encode(&r, t, &mut out) {
                let _ = channel_write(end, &out[..n], &[]);
            }
            continue;
        }
        // `OpenWithApp` names the application; the older `OpenWith` does
        // not (ADR-0052: a changed message is a new ordinal), so it asks
        // for the one of the name's extension.
        // The reply follows the request's ordinal, whether or not it decodes.
        let request = zoraya_protocol::Header::decode(msg)
            .is_ok_and(|(h, _)| h.ordinal == zoraya_proto_chooser::OpenWithApp::ORDINAL);
        let named = if request {
            decode::<zoraya_proto_chooser::OpenWithApp<'_>>(msg, got.len())
                .ok()
                .map(|(_, m)| (m.name.0, m.words.0, m.app.0))
        } else {
            decode::<zoraya_proto_chooser::OpenWith<'_>>(msg, got.len())
                .ok()
                .map(|(_, m)| (m.name.0, m.words.0, &b""[..]))
        };
        let open_with = named.filter(|_| app == "files");
        let status = match open_with.ok_or(()) {
            Ok((name, words, asked)) => {
                let name = core::str::from_utf8(name).unwrap_or("");
                let words = core::str::from_utf8(words).unwrap_or("");
                let asked = core::str::from_utf8(asked).unwrap_or("?");
                match zoraya_panel::opener_for(name, asked) {
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
        let status = zoraya_protocol::Below(status);
        let mut out = [0u8; 64];
        let n = if request {
            encode(&zoraya_proto_chooser::OpeningApp { status }, t, &mut out)
        } else {
            encode(&zoraya_proto_chooser::Opening { status }, t, &mut out)
        };
        if let Ok(n) = n {
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
        windows: Vec::new(),
        keeper: channel(8)
            .and_then(|manage| zoraya_user::roomsclient::Keeper::new(manage, channel(9))),
        tracker: zoraya_rooms::Tracker::new(),
        readings: Vec::new(),
        told_usage: String::new(),
        cost: (0, 0, 0),
    };
    // The first shell's channel (slot 10, ADR-0317): it may ask for the
    // window of a POSIX program, as a terminal's shell launched here does.
    if let Some(first) = channel(10) {
        watch_launcher(&mut st, first, "term", None);
    }
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
            tell_usage(&mut st);
        }
    }
}

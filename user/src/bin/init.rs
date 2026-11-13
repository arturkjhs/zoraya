#![no_std]
#![no_main]

//! The one process the kernel starts (ADR-0053). Its table starts the driver,
//! the filesystem service and the shell, each in a sub-job with only the
//! handles it needs, and it supervises them. On any failure in the storage
//! stack it ends the stack and the shell and starts them again, up to three
//! times (ADR-0080). When the shell ends, it starts a new one in the same
//! terminal (ADR-0155's Decision, ADR-0053's note): on the console, or in
//! GUI mode in a new terminal window, since `exit` closed the last one.
//! Only `poweroff` turns the machine off. A shell that ends other than with
//! exit 0 three times in a row is not started again: `init` syncs and
//! powers off, so a broken shell cannot loop forever.
//!
//! With a GPU and an input device, each alone on its page (ADR-0091), it
//! also starts the GUI (ADR-0097): the drivers, the compositor, a terminal,
//! and the shell in that terminal.
//!
//! When the volume has `/etc/zoraya/session` it starts a POSIX session
//! instead of `zosh` (ADR-0104 item 12, ADR-0110 item 1): `ttyd` over the
//! console, or over the GUI terminal's channel, and the broker, which starts
//! the session's command; the session ending is the shell ending. A dead GUI driver is restarted alone; a
//! dead compositor or terminal ends the GUI and the shell, and starts them
//! again, up to three times.

extern crate alloc;

use zoraya_abi::spawn::Encoder;
use zoraya_abi::spawn::{HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelObject, ConsoleObject, DEADLINE_INFINITE, DIRECTORY, DirectoryObject, Handle,
    JOB, JobFlags, JobObject, Mode, ObjectType, POWER, Packet, PowerObject, Rights,
    StreamReadObject, StreamWriteObject, channel_signals, process_signals,
};
use zoraya_fs_service::Restarts;
use zoraya_user::beside::{Beside, start_broker};
use zoraya_user::files::{ALL, File};
use zoraya_user::gui::{
    Gui, MAX_RESTARTS, Running, read_manager_events, scan, start_terminal, take_display_hung,
};
use zoraya_user::netclient::{Network, send_down, send_restarted, start_network_with};
use zoraya_user::stack::{Stack, end_job, moved, read_rtc, start_stack_at};
use zoraya_user::{
    channel_create, directory_lookup, dup, handle_close, job_create, port_create, port_subscribe,
    port_wait, power_off, println, process_exit_record, spawn, stream_create, wait_exit,
};

const KEY_DRIVER: u64 = 1;
const KEY_SERVICE: u64 = 2;
const KEY_SHELL: u64 = 3;
const KEY_SUPERVISOR: u64 = 4;
const KEY_GPU: u64 = 5;
const KEY_COMPOSITOR: u64 = 6;
const KEY_TERMINAL: u64 = 7;
const KEY_MANAGER: u64 = 8;
const KEY_TTYD: u64 = 9;
const KEY_NETD: u64 = 10;
const KEY_ETHD: u64 = 11;
const KEY_ROOMS: u64 = 12;
const KEY_ROOMS_CHAN: u64 = 13;
const KEY_RECONNECT_SHELL: u64 = 14;
const KEY_RECONNECT_PANEL: u64 = 15;
const KEY_INPUT: u64 = 16;
/// The shell's services channel, read only in the test image: its request
/// to end `netd` (ADR-0296 item 5). Matched before `KEY_INPUT`'s range.
#[cfg(feature = "net-test")]
const KEY_SHELL_SERVICES: u64 = 1 << 40;

/// A moved handle with its type's rights.
fn give(slot: u32, handle: Handle, rights: Rights) -> HandleEntry {
    HandleEntry {
        slot,
        handle,
        rights,
        transfer: Transfer::Move,
    }
}

/// The POSIX session (ADR-0104 item 12, ADR-0105 item 1): `ttyd` with the
/// backend, then the broker with the root connection, the terminal's
/// control and first connections and the output stream's write end. `init`
/// makes the stream and the channels and hands each end to its holder.
/// The session job (ADR-0261 point 10, ADR-0266 point 4): everything the
/// person launches is under it, with three quarters of `init`'s memory and 200
/// of the 255 address space identifiers, so that the drivers and services keep
/// the rest. The values are `[assumption]` until Q1's measurements.
const SESSION_PROCESSES: u32 = 200;

static SESSION: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// The session job; made on first use. If it cannot be made or limited,
/// `init`'s own job stands in: nothing starts without a parent.
fn session() -> Handle {
    use core::sync::atomic::Ordering;
    if let Some(h) = Handle::new(SESSION.load(Ordering::Relaxed)) {
        return h;
    }
    let made = job_create(JOB, JobFlags::NONE).ok().and_then(|job| {
        let own = zoraya_user::job_get_usage(JOB, false).ok()?.memory.limit;
        let limits = zoraya_abi::quota::JobLimits {
            memory: own.map(|m| m / 4 * 3),
            processes: Some(SESSION_PROCESSES),
            cpu: None,
            // Three times the processes (ADR-0307 item 4, INV-0307-3).
            threads: zoraya_rooms::threads_for(SESSION_PROCESSES),
        };
        zoraya_user::job_set_limits(job, &limits).ok()?;
        Some(job)
    });
    match made {
        Some(job) => {
            SESSION.store(job.raw(), Ordering::Relaxed);
            job
        }
        None => JOB,
    }
}

struct Session {
    ttyd: (Handle, Handle),
    broker: (Handle, Handle),
}

fn start_session(root: &File, window: Option<Handle>) -> Option<Session> {
    let base = root.time_base().unwrap_or(0);
    let (out_read, out_write) = stream_create().ok()?;
    let (control_service, control_broker) = channel_create().ok()?;
    let (first_service, first_broker) = channel_create().ok()?;
    let job = job_create(session(), JobFlags::NONE).ok()?;
    let image = directory_lookup(DIRECTORY, "ttyd").ok()?;
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, job, image);
    let console = if window.is_some() {
        Rights::WRITE
    } else {
        ConsoleObject::RIGHTS
    };
    e.handle(dup(CONSOLE.slot(), CONSOLE, console));
    if let Some(w) = window {
        e.handle(moved(5, w));
    }
    e.handle(moved(6, control_service));
    e.handle(moved(7, first_service));
    e.handle(give(8, out_read, StreamReadObject::RIGHTS));
    e.arg(b"ttyd");
    if window.is_some() {
        e.arg(b"--window");
    }
    let length = e.finish()?;
    let ttyd = spawn(&buffer[..length]).ok();
    let _ = handle_close(image);
    let ttyd = (job, ttyd?);
    let job = job_create(session(), JobFlags::POSIX).ok()?;
    let image = directory_lookup(DIRECTORY, "broker").ok()?;
    let root = root.clone_as(ALL).ok()?;
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, job, image);
    // Written to by the broker and handed on to the temporary service.
    e.handle(dup(
        CONSOLE.slot(),
        CONSOLE,
        Rights::WRITE.union(Rights::DUPLICATE),
    ));
    e.handle(dup(JOB.slot(), job, JobObject::RIGHTS));
    e.handle(dup(DIRECTORY.slot(), DIRECTORY, DirectoryObject::RIGHTS));
    e.handle(give(5, root.end, ChannelObject::RIGHTS));
    e.handle(moved(6, control_broker));
    e.handle(moved(7, first_broker));
    e.handle(give(8, out_write, StreamWriteObject::RIGHTS));
    let base = alloc::format!("{base}");
    for a in [&b"broker"[..], b"--time-base", base.as_bytes()] {
        e.arg(a);
    }
    let length = e.finish()?;
    let broker = spawn(&buffer[..length]).ok();
    let _ = handle_close(image);
    if broker.is_some() {
        core::mem::forget(root);
    }
    Some(Session {
        ttyd,
        broker: (job, broker?),
    })
}

/// `/config` read-only, for `netd`'s settings (ADR-0229 item 2).
fn config_connection(stack: &Stack) -> Option<Handle> {
    let rights = zoraya_proto_file::rights::READ | zoraya_proto_file::rights::CLONE;
    let f = stack
        .root
        .open("config", zoraya_proto_file::open_flags::DIRECTORY, rights)
        .ok()?;
    let end = f.end;
    core::mem::forget(f);
    Some(end)
}

/// The room keeper (ADR-0269, proposed ADR-0293), started with the storage
/// stack (it reads `/config/rooms`) and restarted at most [`MAX_RESTARTS`]
/// times; `kept` is a duplicate, with `LIMIT`, of the untrusted job (room
/// name empty) and of each room job the keeper made, for a restarted keeper.
struct RoomKeeper {
    job: Handle,
    process: Handle,
    /// `init`'s end of the start-up channel.
    chan: Handle,
}

/// What `init` keeps for a restarted keeper.
struct Kept {
    /// The room's name; empty for the untrusted job.
    room: alloc::string::String,
    job: Handle,
    /// The room's job is under the untrusted job.
    untrusted: bool,
    /// `Launcher::raw()` of the room's last holder, or 0.
    launcher: u32,
}

/// The shell's slots for the keeper: the `Manage` channel and the channel to
/// `init` that gives a new one.
const SHELL_ROOMS_SLOT: u32 = 11;
const SHELL_ROOMS_RECONNECT_SLOT: u32 = 12;
/// The shell's services channel, from `init` (ADR-0221 item 6, proposed
/// ADR-0296): `NetworkRestarted` with a new root connection, or `NetworkDown`.
const SHELL_SERVICES_SLOT: u32 = 13;

fn send_keep(chan: Handle, k: &Kept) {
    use zoraya_proto_rooms::Keep;
    let mut b = [0u8; 128];
    let untrusted = k.room.is_empty();
    if let Ok(n) = zoraya_protocol::encode(
        &Keep {
            untrusted: zoraya_protocol::Below(u32::from(untrusted)),
            room_untrusted: zoraya_protocol::Below(u32::from(k.untrusted)),
            launcher: zoraya_protocol::Below(k.launcher),
            room: zoraya_protocol::Text(&k.room),
        },
        0,
        &mut b,
    ) {
        let entry = zoraya_abi::ChannelEntry {
            handle: k.job,
            rights: JobObject::RIGHTS,
            transfer: Transfer::Duplicate,
        };
        let _ = zoraya_user::channel_write(chan, &b[..n], &[entry]);
    }
}

fn start_rooms(stack: &Stack, kept: &[Kept]) -> Option<RoomKeeper> {
    let (mine, theirs) = channel_create().ok()?;
    // Queued before the keeper starts: it reads them first (the untrusted job
    // before the rooms', which `kept` has in order).
    for k in kept {
        send_keep(mine, k);
    }
    let Some(config) = config_connection(stack) else {
        let _ = handle_close(mine);
        let _ = handle_close(theirs);
        return None;
    };
    let entries = [
        zoraya_user::stack::console(),
        dup(5, session(), JobObject::RIGHTS),
        moved(6, theirs),
        moved(7, config),
    ];
    match zoraya_user::stack::start("rooms", &entries, &[]) {
        Some((job, process)) => Some(RoomKeeper {
            job,
            process,
            chan: mine,
        }),
        None => {
            let _ = handle_close(mine);
            None
        }
    }
}

impl RoomKeeper {
    fn end(self) {
        end_job((self.job, self.process));
        let _ = handle_close(self.chan);
    }

    /// A new channel of `kind` to the keeper: the keeper's end is sent over
    /// the start-up channel, the other is returned.
    fn channel(&self, kind: u32, room: &str) -> Option<Handle> {
        let (client, server) = channel_create().ok()?;
        let mut b = [0u8; 128];
        let n = zoraya_protocol::encode(
            &zoraya_proto_rooms::Attach {
                kind: zoraya_protocol::Below(kind),
                room: zoraya_protocol::Text(room),
            },
            0,
            &mut b,
        )
        .ok()?;
        let entry = zoraya_abi::ChannelEntry {
            handle: server,
            rights: ChannelObject::RIGHTS,
            transfer: Transfer::Move,
        };
        if zoraya_user::channel_write(self.chan, &b[..n], &[entry]).is_err() {
            let _ = handle_close(client);
            let _ = handle_close(server);
            return None;
        }
        Some(client)
    }

    /// What the keeper sent: a job it made (`Keep`) or let go (`Forget`).
    fn pump(&self, kept: &mut alloc::vec::Vec<Kept>) {
        loop {
            let mut buf = [0u8; 128];
            let mut handles = [self.chan; 1];
            let Ok(sizes) = zoraya_user::channel_read(self.chan, &mut buf, &mut handles) else {
                return;
            };
            let bytes = &buf[..sizes.bytes as usize];
            let count = sizes.handles as usize;
            if let Ok((_, m)) =
                zoraya_protocol::decode::<zoraya_proto_rooms::Keep<'_>>(bytes, count)
            {
                let room = if m.untrusted.0 == 1 {
                    alloc::string::String::new()
                } else {
                    alloc::string::String::from(m.room.0)
                };
                if let Some(old) = kept.iter().position(|k| k.room == room) {
                    let _ = handle_close(kept.remove(old).job);
                }
                kept.push(Kept {
                    room,
                    job: handles[0],
                    untrusted: m.room_untrusted.0 == 1,
                    launcher: m.launcher.0,
                });
            } else if let Ok((_, m)) =
                zoraya_protocol::decode::<zoraya_proto_rooms::Forget<'_>>(bytes, count)
            {
                if let Some(old) = kept.iter().position(|k| k.room == m.room.0) {
                    let _ = handle_close(kept.remove(old).job);
                }
            } else {
                for h in &handles[..count.min(1)] {
                    let _ = handle_close(*h);
                }
            }
        }
    }
}

/// A launcher's request for a new channel to the keeper (ADR-0269 point 5):
/// answered with one, or with none if there is no keeper.
fn answer_reconnect(reconnect: Handle, rooms: Option<&RoomKeeper>, kind: u32) {
    let mut buf = [0u8; 16];
    let mut handles = [reconnect; 1];
    while let Ok(sizes) = zoraya_user::channel_read(reconnect, &mut buf, &mut handles) {
        // The test image's `room crash-keeper` (proposed ADR-0298): the
        // shell asks for the keeper to end, so that the run of `init`'s
        // restart, the reconnect and the prompt's rebind can be tested in
        // QEMU. No other image looks.
        #[cfg(feature = "net-test")]
        if &buf[..(sizes.bytes as usize).min(buf.len())] == b"crash" {
            if let Some(r) = rooms {
                println!("init: the shell asked the room keeper to end (a test)");
                let _ = zoraya_rt::job_kill(r.job);
            }
            continue;
        }
        #[cfg(not(feature = "net-test"))]
        let _ = sizes;
        // The test image says so, so that keeper.script can tell the prompt's
        // rebind from the lazy one of the first call (prompt-rebind row).
        #[cfg(feature = "net-test")]
        println!("init: the shell reconnected to the room keeper (a test)");
        let new = rooms.and_then(|r| r.channel(kind, ""));
        let entry = new.map(|h| zoraya_abi::ChannelEntry {
            handle: h,
            rights: ChannelObject::RIGHTS,
            transfer: Transfer::Move,
        });
        let list = entry.as_slice();
        if zoraya_user::channel_write(reconnect, b"m", list).is_err()
            && let Some(h) = new
        {
            let _ = handle_close(h);
        }
    }
}

/// The shell's root network connection (ADR-0221 item 4), present when the
/// network runs.
const SHELL_NET_SLOT: u32 = 9;
/// The shell's connection to the random service (ADR-0227 item 3).
const SHELL_RANDOM_SLOT: u32 = 10;
/// The first shell's channel to the panel, for the window of a POSIX program
/// (ADR-0317; `zoraya_panel::WINDOW_SLOT`).
const SHELL_WINDOW_SLOT: u32 = zoraya_panel::WINDOW_SLOT;

/// Starts `rngd` with the boot seed (ADR-0227 item 2) and gives the
/// connection `init` keeps to make the launchers'. `None` without a seed
/// object: the service would have no entropy, so it does not run and
/// `getrandom` fails (stated here, never silent).
fn start_rng() -> Option<((Handle, Handle), Handle)> {
    let seed = Handle::new(u64::from(zoraya_abi::SEED_SLOT))?;
    if zoraya_user::handle_type(seed) != Ok("memory") {
        println!("init: no entropy source: no seed in the device tree, no random service");
        return None;
    }
    let (mine, theirs) = channel_create().ok()?;
    let entry = zoraya_abi::spawn::HandleEntry {
        slot: 5,
        handle: seed,
        rights: Rights::READ.union(Rights::MAP),
        transfer: zoraya_abi::spawn::Transfer::Move,
    };
    let running = zoraya_user::stack::start(
        "rngd",
        &[zoraya_user::stack::console(), entry, moved(6, theirs)],
        &[],
    )?;
    Some((running, mine))
}

/// A new root network connection for a launcher, made from `init`'s own
/// (ADR-0221 item 4).
fn root_connection(n: &Network) -> Option<Handle> {
    zoraya_user::netclient::narrow(
        n.control,
        zoraya_abi::net::rights::ALL,
        &zoraya_user::netclient::root_scope(),
    )
    .ok()
}

/// Whether the volume holds `/etc/zoraya/net-crash`: the test of the
/// network's restart then starts `netd` so that it ends on request
/// (proposed ADR-0296). Only the test image (feature `net-test`) looks;
/// any other is never started so (N6 review, finding 2).
#[cfg(feature = "net-test")]
fn net_crash(stack: Option<&Stack>) -> bool {
    stack.is_some_and(|s| {
        s.root
            .open("etc/zoraya/net-crash", 0, zoraya_proto_file::rights::READ)
            .is_ok()
    })
}

#[cfg(not(feature = "net-test"))]
fn net_crash(_: Option<&Stack>) -> bool {
    false
}

/// The test image's shell asked, on its services channel, for `netd` to
/// end: `init` sends `Crash` on its own control connection, the only one
/// `netd` honours it on (ADR-0296 item 5).
#[cfg(feature = "net-test")]
fn crash_on_request(services: Handle, network: Option<&Network>) {
    let mut bytes = [0u8; 64];
    while let Ok(sizes) = zoraya_user::channel_read(services, &mut bytes, &mut []) {
        let message = &bytes[..(sizes.bytes as usize).min(bytes.len())];
        let asked = sizes.handles == 0
            && zoraya_protocol::Header::decode(message)
                .is_ok_and(|(h, _)| h.ordinal == zoraya_proto_net::CRASH_ORDINAL);
        if asked && let Some(n) = network {
            println!("init: the shell asked the network service to end (a test)");
            let _ = zoraya_user::netclient::crash(n.control);
        }
    }
}

/// Starts the shell, with the volume's root at slot 5 and, in GUI mode, its
/// terminal at slot 6; with a broker beside it, its client connection at 7
/// and the temporary service's root at 8. With a terminal the shell gets the console to write
/// only: its input is the terminal's (INV-0097-3).
// Each argument is a handle `init` moves into the shell at a slot of its own; a
// struct would only rename them.
#[allow(clippy::too_many_arguments)]
fn start_shell(
    root: Option<File>,
    terminal: Option<Handle>,
    broker: Option<&Beside>,
    network: Option<Handle>,
    random: Option<Handle>,
    keeper: Option<(Handle, Handle)>,
    services: Option<Handle>,
    window: Option<Handle>,
) -> Option<(Handle, Handle)> {
    let job = job_create(session(), JobFlags::NONE).ok()?;
    let image = directory_lookup(DIRECTORY, "zosh").ok()?;
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, job, image);
    // In a window the shell writes the console and hands it to the programs
    // it starts (which print there until byte streams), but never reads it
    // (INV-0097-3).
    let console = if terminal.is_some() {
        Rights::WRITE.union(Rights::DUPLICATE)
    } else {
        ConsoleObject::RIGHTS
    };
    e.handle(dup(CONSOLE.slot(), CONSOLE, console));
    e.handle(dup(JOB.slot(), job, JobObject::RIGHTS));
    e.handle(dup(DIRECTORY.slot(), DIRECTORY, DirectoryObject::RIGHTS));
    // The shell turns the machine off, and hands that to nobody.
    let power = PowerObject::RIGHTS
        .without(Rights::DUPLICATE)
        .without(Rights::TRANSFER);
    e.handle(dup(POWER.slot(), POWER, power));
    // On the console the shell holds the volume's root; in a window, what
    // `home` gives (ADR-0242's condition), as a start-up record.
    let mut root_moved = false;
    if let Some(root) = &root {
        if terminal.is_some() {
            let home = zoraya_user::namespace::shell_home(root);
            let holds = zoraya_panel::terminal(None).holds;
            if let Some(r) = zoraya_user::namespace::record(root, &home, &home, &holds) {
                e.handle(moved(zoraya_panel::STARTUP_SLOT, r));
            }
        } else {
            e.handle(moved(5, root.end));
            root_moved = true;
        }
    }
    if let Some(terminal) = terminal {
        e.handle(moved(6, terminal));
    }
    if let Some(b) = broker {
        e.handle(moved(7, b.client));
        e.handle(moved(8, b.tmp));
    }
    if let Some(n) = network {
        e.handle(moved(SHELL_NET_SLOT, n));
    }
    if let Some(r) = random {
        e.handle(moved(SHELL_RANDOM_SLOT, r));
    }
    // The room keeper's `Manage` channel and the way to a new one.
    if let Some((manage, reconnect)) = keeper {
        e.handle(moved(SHELL_ROOMS_SLOT, manage));
        e.handle(moved(SHELL_ROOMS_RECONNECT_SLOT, reconnect));
    }
    if let Some(s) = services {
        e.handle(moved(SHELL_SERVICES_SLOT, s));
    }
    if let Some(w) = window {
        e.handle(moved(SHELL_WINDOW_SLOT, w));
    }
    let length = e.finish()?;
    let process = spawn(&buffer[..length]).ok();
    if process.is_none() {
        if let Some(w) = window {
            let _ = handle_close(w);
        }
        if let Some(s) = services {
            let _ = handle_close(s);
        }
        for h in [network, random].into_iter().flatten() {
            let _ = handle_close(h);
        }
        if let Some((a, b)) = keeper {
            let _ = handle_close(a);
            let _ = handle_close(b);
        }
    }
    if process.is_some() && root_moved {
        // Moved: the shell's now.
        core::mem::forget(root);
    } else if process.is_none()
        && let Some(terminal) = terminal
    {
        let _ = handle_close(terminal);
    }
    let _ = handle_close(image);
    Some((job, process?))
}

fn power_down(root: Option<&File>, status_line: zoraya_abi::ExitRecord) -> ! {
    println!("init: powering off after: {status_line:?}");
    if let Some(root) = root
        && root.sync().is_err()
    {
        // Nobody else can say it: the last writes may be lost.
        println!("init: sync failed: what was written since the last sync may be lost");
    }
    let status = power_off(POWER);
    println!("init: power off: {status:?}");
    zoraya_user::process_exit(2)
}

/// The shell's record if it has ended with exit 0, which only `exit` gives:
/// a shell whose terminal went exits 1.
fn chose_to_end(shell: Handle) -> Option<zoraya_abi::ExitRecord> {
    process_exit_record(shell)
        .ok()
        .filter(|r| r.reason == zoraya_abi::ExitReason::Exited && r.code == 0)
}

/// Why the shell and its terminal ended.
enum Ended {
    Storage,
    Gui,
    /// The shell ended by itself, with this record.
    Shell(zoraya_abi::ExitRecord),
    /// The person closed the terminal's window: it ended with 0 (ADR-0160's
    /// close button), which ends its shell as `exit` would.
    Closed,
}

/// Shells in a row that may end otherwise than with exit 0.
const SHELL_FAILURES: u32 = 3;

fn subscribe(port: Handle, process: Handle, key: u64) {
    let terminated = process_signals::TERMINATED;
    port_subscribe(port, process, terminated, key, Mode::Level).expect("subscribe");
}

fn gui_subscriptions(port: Handle, gui: &Gui) {
    subscribe(port, gui.compd.process, KEY_COMPOSITOR);
    let readable = channel_signals::READABLE;
    port_subscribe(port, gui.manager, readable, KEY_MANAGER, Mode::Level).expect("subscribe");
    if let Some(g) = gui.gpud {
        subscribe(port, g.process, KEY_GPU);
    }
    for (i, r) in gui.inputd.iter().enumerate() {
        if let Some(r) = r {
            subscribe(port, r.process, KEY_INPUT + i as u64);
        }
    }
}

/// Starts the GPU driver again, hands it to the compositor and watches it.
fn restart_gpu(g: &mut Gui, port: Handle) {
    if !g.restart_gpu() && g.gpud.is_some() {
        println!("init: the compositor did not take the new GPU driver");
    }
    // A hang reported before this attach was the old driver's: it is
    // handled by this restart, not by another (check of ADR-0093, error 2).
    read_manager_events(g.manager);
    let _ = take_display_hung();
    // Watched whenever it runs, whatever the compositor said.
    if let Some(r) = g.gpud {
        println!("init: the GPU driver was restarted");
        subscribe(port, r.process, KEY_GPU);
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let devices = scan();
    if read_rtc() == 0 {
        println!("init: no real-time clock reading; dates count from the volume");
    }
    let rng = start_rng();
    let mut storage_allowed = devices.disk.is_some();
    let mut restarts = Restarts::new();
    let mut stack: Option<Stack> = None;
    let gui_devices = devices.gpu.filter(|_| !devices.inputs.is_empty());
    let mut gui_allowed = gui_devices.is_some();
    let mut gui_restarts = 0;
    let mut gui: Option<Gui> = None;
    // The panel (ADR-0168): started once per GUI, with the volume; it
    // launches every terminal but the first (proposed ADR-0242).
    let mut panel: Option<Running> = None;
    // The chooser (ADR-0167), started with the panel and ended with it.
    let mut chooser: Option<Running> = None;
    let mut driver_restarts = (0, alloc::vec![0; devices.inputs.len()]);
    // The network driver and service (ADR-0221 item 4). They are started
    // once the storage stack is up, for the settings; restarted together at
    // most three times per boot when either ends (item 5).
    let mut network: Option<Network> = None;
    let mut network_allowed = devices.net.is_some();
    let mut network_restarts = 0;
    let mut shell_failures = 0;
    // The room keeper (ADR-0269): started with the storage stack.
    let mut rooms: Option<RoomKeeper> = None;
    let mut rooms_allowed = true;
    let mut rooms_restarts = 0;
    let mut kept: alloc::vec::Vec<Kept> = alloc::vec::Vec::new();
    // `init`'s end of the panel's way to a new keeper channel.
    let mut panel_reconnect: Option<Handle> = None;
    // The first shell's end of its channel to the panel (ADR-0317), made
    // with the panel and moved into the shell when it starts.
    let mut shell_window: Option<Handle> = None;
    if gui_devices.is_some() {
        println!(
            "init: gui: gpu on its own page, {} input device(s)",
            devices.inputs.len()
        );
    }
    loop {
        if stack.is_none()
            && storage_allowed
            && let Some((device, range)) = devices.disk
        {
            stack = start_stack_at(device, range, read_rtc());
            if stack.is_none() {
                println!("init: the storage stack did not start");
                storage_allowed = false;
            }
        }
        if rooms.is_none()
            && rooms_allowed
            && let Some(s) = stack.as_ref()
        {
            rooms = start_rooms(s, &kept);
            if rooms.is_none() {
                println!("init: the room keeper did not start");
                rooms_allowed = false;
            }
        }
        if network.is_none()
            && network_allowed
            && let Some(device) = devices.net
        {
            let config = stack.as_ref().and_then(config_connection);
            network = start_network_with(device, config, net_crash(stack.as_ref()));
            if network.is_none() {
                println!("init: the network did not start");
                network_allowed = false;
            }
        }
        if gui.is_none()
            && gui_allowed
            && let Some(gpu) = gui_devices
        {
            // `/config` to read, for the compositor's settings (ADR-0169).
            let config = stack.as_ref().and_then(|s| {
                let rights = zoraya_proto_file::rights::READ | zoraya_proto_file::rights::CLONE;
                let f = s
                    .root
                    .open("config", zoraya_proto_file::open_flags::DIRECTORY, rights)
                    .ok()?;
                let end = f.end;
                core::mem::forget(f);
                Some(end)
            });
            gui = Gui::start(gpu, &devices.inputs, config);
            // A driver's restarts count over the boot, not per GUI start
            // (review R6).
            if let Some(g) = gui.as_mut() {
                g.gpu_restarts = driver_restarts.0;
                g.input_restarts.clone_from(&driver_restarts.1);
            }
            if gui.is_none() {
                println!("init: the gui did not start");
                gui_allowed = false;
            }
        }
        let terminal: Option<(Running, Handle)> = gui.as_ref().and_then(start_terminal);
        if panel.is_none()
            && let (Some(g), Some(s)) = (gui.as_ref(), stack.as_ref())
        {
            let registration = match g.start_chooser(&s.root) {
                Some((c, end)) => {
                    chooser = Some(c);
                    Some(end)
                }
                None => {
                    println!("init: the chooser did not start");
                    None
                }
            };
            // Its `Manage` channel to the keeper and the way to a new one.
            let keeper = rooms.as_ref().and_then(|r| {
                let manage = r.channel(zoraya_proto_rooms::attach::MANAGE_DESKTOP, "")?;
                let (theirs, ours) = channel_create().ok()?;
                Some((manage, theirs, ours))
            });
            let window = channel_create().ok();
            panel = g.start_panel(
                &s.root,
                registration,
                keeper.map(|k| (k.0, k.1)),
                window.map(|w| w.1),
            );
            if panel.is_some() {
                shell_window = window.map(|w| w.0);
            } else if let Some((theirs, _)) = window {
                let _ = handle_close(theirs);
            }
            if panel.is_none() {
                println!("init: the panel did not start");
                if let Some((_, _, ours)) = keeper {
                    let _ = handle_close(ours);
                }
            } else {
                panel_reconnect = keeper.map(|k| k.2);
            }
        }
        let wants_session = stack.as_ref().is_some_and(|s| {
            s.root
                .open("etc/zoraya/session", 0, zoraya_proto_file::rights::READ)
                .is_ok()
        });
        let mut session = None;
        let mut beside = None;
        let mut shell_reconnect: Option<Handle> = None;
        // `init`'s end of the shell's services channel (item 6).
        let mut shell_services: Option<Handle> = None;
        let shell = if wants_session {
            println!("init: /etc/zoraya/session: a POSIX session");
            let root = &stack.as_ref().expect("a stack").root;
            session = start_session(root, terminal.map(|t| t.1));
            session.as_ref().map(|s| s.broker)
        } else {
            beside = stack.as_ref().and_then(|s| start_broker(&s.root));
            let root = stack.as_ref().and_then(|s| s.root.clone_as(ALL).ok());
            // Its root network connection, made from `init`'s own (item 4).
            let root_network = network.as_ref().and_then(root_connection);
            let services = if network_allowed || network.is_some() {
                channel_create().ok().map(|(ours, theirs)| {
                    shell_services = Some(ours);
                    theirs
                })
            } else {
                None
            };
            let random = rng
                .as_ref()
                .and_then(|(_, conn)| zoraya_user::random::share(*conn));
            // The console shell's `Manage` channel (ADR-0269 point 3).
            let keeper = rooms.as_ref().and_then(|r| {
                let manage = r.channel(zoraya_proto_rooms::attach::MANAGE_CONSOLE, "")?;
                let (theirs, ours) = channel_create().ok()?;
                shell_reconnect = Some(ours);
                Some((manage, theirs))
            });
            start_shell(
                root,
                terminal.map(|t| t.1),
                beside.as_ref(),
                root_network,
                random,
                keeper,
                services,
                match shell_window.take() {
                    Some(w) if terminal.is_some() => Some(w),
                    // No terminal to give it to: not left open (N5).
                    Some(w) => {
                        let _ = handle_close(w);
                        None
                    }
                    None => None,
                },
            )
        };
        let Some(shell) = shell else {
            println!("init: the shell did not start");
            if let Some(s) = stack.take() {
                s.end();
            }
            zoraya_user::process_exit(1);
        };
        let port = port_create().expect("port");
        if let Some(s) = &stack {
            subscribe(port, s.driver.1, KEY_DRIVER);
            subscribe(port, s.service.1, KEY_SERVICE);
            let readable = channel_signals::READABLE | channel_signals::PEER_CLOSED;
            port_subscribe(port, s.supervisor, readable, KEY_SUPERVISOR, Mode::Level)
                .expect("subscribe");
        }
        if let Some(g) = &gui {
            gui_subscriptions(port, g);
        }
        if let Some((t, _)) = terminal {
            subscribe(port, t.process, KEY_TERMINAL);
        }
        subscribe(port, shell.1, KEY_SHELL);
        if let Some(n) = &network {
            subscribe(port, n.netd.process, KEY_NETD);
            subscribe(port, n.ethd.process, KEY_ETHD);
        }
        if let Some(s) = &session {
            subscribe(port, s.ttyd.1, KEY_TTYD);
        }
        if let Some(r) = &rooms {
            subscribe(port, r.process, KEY_ROOMS);
            let readable = channel_signals::READABLE;
            port_subscribe(port, r.chan, readable, KEY_ROOMS_CHAN, Mode::Level).expect("subscribe");
        }
        #[cfg(feature = "net-test")]
        if let Some(services) = shell_services {
            let readable = channel_signals::READABLE;
            port_subscribe(port, services, readable, KEY_SHELL_SERVICES, Mode::Level)
                .expect("subscribe");
        }
        for (end, key) in [
            (shell_reconnect, KEY_RECONNECT_SHELL),
            (panel_reconnect, KEY_RECONNECT_PANEL),
        ] {
            if let Some(end) = end {
                let readable = channel_signals::READABLE;
                port_subscribe(port, end, readable, key, Mode::Level).expect("subscribe");
            }
        }
        let ended = loop {
            let mut packets = [Packet::default()];
            port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
            match packets[0].key {
                KEY_SHELL => {
                    // A shell that ended because its terminal went is a GUI
                    // failure; otherwise it chose to end.
                    if let Some(record) = chose_to_end(shell.1) {
                        break Ended::Shell(record);
                    }
                    let terminal_gone =
                        terminal.is_some_and(|(t, _)| process_exit_record(t.process).is_ok());
                    if !terminal_gone {
                        break Ended::Shell(wait_exit(shell.1));
                    }
                    break Ended::Gui;
                }
                KEY_GPU => restart_gpu(gui.as_mut().expect("a GUI"), port),
                // Read below, with every other wake-up.
                KEY_MANAGER => {}
                #[cfg(feature = "net-test")]
                KEY_SHELL_SERVICES => {
                    if let Some(services) = shell_services {
                        crash_on_request(services, network.as_ref());
                    }
                }
                k if k >= KEY_INPUT => {
                    let g = gui.as_mut().expect("a GUI");
                    let i = (k - KEY_INPUT) as usize;
                    if !g.restart_input(i) && g.inputd[i].is_some() {
                        println!("init: the compositor did not take the new input driver");
                    }
                    if let Some(r) = g.inputd[i] {
                        println!("init: an input driver was restarted");
                        subscribe(port, r.process, k);
                    }
                }
                // `exit` closes the window too: a terminal that ends after
                // its shell chose to end is no GUI failure (ADR-0155).
                KEY_TERMINAL if chose_to_end(shell.1).is_some() => {
                    break Ended::Shell(wait_exit(shell.1));
                }
                KEY_TERMINAL
                    if terminal.is_some_and(|(t, _)| chose_to_end(t.process).is_some()) =>
                {
                    break Ended::Closed;
                }
                KEY_COMPOSITOR | KEY_TERMINAL => break Ended::Gui,
                KEY_NETD | KEY_ETHD => {
                    // Either ending ends both (item 5): running programs'
                    // connections are gone, new grants get a new service.
                    if let Some(n) = network.take() {
                        let which = if packets[0].key == KEY_NETD {
                            "service"
                        } else {
                            "driver"
                        };
                        println!("init: the network {which} ended");
                        n.end();
                    }
                    network_restarts += 1;
                    if network_restarts > MAX_RESTARTS {
                        println!("init: no more network restarts");
                        network_allowed = false;
                        if let Some(services) = shell_services {
                            let _ = send_down(services);
                        }
                    } else if let Some(device) = devices.net {
                        let config = stack.as_ref().and_then(config_connection);
                        network = start_network_with(device, config, net_crash(stack.as_ref()));
                        match &network {
                            Some(n) => {
                                println!("init: the network was restarted");
                                subscribe(port, n.netd.process, KEY_NETD);
                                subscribe(port, n.ethd.process, KEY_ETHD);
                                // Item 6: the launcher gets a new root.
                                if let (Some(services), Some(root)) =
                                    (shell_services, root_connection(n))
                                    && !send_restarted(services, root)
                                {
                                    println!("init: the shell's services channel is closed");
                                }
                            }
                            None => {
                                println!("init: the network did not start");
                                network_allowed = false;
                            }
                        }
                    }
                }
                KEY_ROOMS_CHAN => {
                    if let Some(r) = &rooms {
                        r.pump(&mut kept);
                    }
                }
                KEY_RECONNECT_SHELL | KEY_RECONNECT_PANEL => {
                    let (end, kind) = if packets[0].key == KEY_RECONNECT_SHELL {
                        (shell_reconnect, zoraya_proto_rooms::attach::MANAGE_CONSOLE)
                    } else {
                        (panel_reconnect, zoraya_proto_rooms::attach::MANAGE_DESKTOP)
                    };
                    if let Some(end) = end {
                        answer_reconnect(end, rooms.as_ref(), kind);
                    }
                }
                KEY_ROOMS => {
                    // The keeper ended: running rooms keep their jobs and
                    // limits; the new one gets the jobs `init` kept
                    // (ADR-0080's bound on restarts).
                    println!("init: the room keeper ended");
                    if let Some(r) = rooms.take() {
                        r.pump(&mut kept);
                        r.end();
                    }
                    rooms_restarts += 1;
                    if rooms_restarts > MAX_RESTARTS {
                        println!("init: no more room keeper restarts");
                        rooms_allowed = false;
                    } else if let Some(s) = stack.as_ref() {
                        rooms = start_rooms(s, &kept);
                        match &rooms {
                            Some(r) => {
                                println!("init: the room keeper was restarted");
                                subscribe(port, r.process, KEY_ROOMS);
                                let readable = channel_signals::READABLE;
                                port_subscribe(port, r.chan, readable, KEY_ROOMS_CHAN, Mode::Level)
                                    .expect("subscribe");
                            }
                            None => rooms_allowed = false,
                        }
                    }
                }
                KEY_TTYD => {
                    let ttyd = session.as_ref().expect("a session").ttyd;
                    let record = wait_exit(ttyd.1);
                    println!("init: ttyd ended");
                    end_job(shell);
                    power_down(stack.as_ref().map(|s| &s.root), record);
                }
                _ => break Ended::Storage,
            }
            // ADR-0093, Decision: the compositor dropped a display driver
            // that missed its deadline. The report may have been read here
            // or inside any manager call, so the flag is looked at after
            // every wake-up.
            if let Some(g) = gui.as_mut() {
                read_manager_events(g.manager);
                if take_display_hung() {
                    println!("init: the GPU driver hung");
                    restart_gpu(g, port);
                }
            }
        };
        end_job(shell);
        if let Some(c) = shell_reconnect.take() {
            let _ = handle_close(c);
        }
        // Each shell gets a services channel of its own (N6 review, finding 3).
        if let Some(s) = shell_services.take() {
            let _ = handle_close(s);
        }
        if let Some(b) = beside.take() {
            end_job(b.job);
        }
        let was_session = session.is_some();
        if let Some(s) = session.take() {
            end_job(s.ttyd);
        }
        if let Some((t, _)) = terminal {
            t.end();
        }
        let _ = handle_close(port);
        match ended {
            // A POSIX session's end is the machine's (ADR-0104 item 12): only
            // `zosh` is started again (ADR-0155's Decision).
            Ended::Shell(record) if was_session => {
                power_down(stack.as_ref().map(|s| &s.root), record);
            }
            Ended::Shell(record) => {
                println!("init: shell ended: {record:?}");
                let clean = record.reason == zoraya_abi::ExitReason::Exited && record.code == 0;
                shell_failures = if clean { 0 } else { shell_failures + 1 };
                if shell_failures >= SHELL_FAILURES {
                    println!("init: the shell failed {SHELL_FAILURES} times in a row");
                    power_down(stack.as_ref().map(|s| &s.root), record);
                }
                // Written data is safe whatever the next shell does.
                if let Some(s) = &stack
                    && s.root.sync().is_err()
                {
                    println!("init: sync failed: what was written since the last sync may be lost");
                }
            }
            // A new terminal and shell, as after `exit`; not a failure.
            Ended::Closed => println!("init: the terminal was closed"),
            Ended::Gui => {
                println!("init: the gui failed");
                if let Some(c) = panel_reconnect.take() {
                    let _ = handle_close(c);
                }
                if let Some(p) = panel.take() {
                    p.end();
                }
                if let Some(c) = chooser.take() {
                    c.end();
                }
                if let Some(g) = gui.take() {
                    driver_restarts = (g.gpu_restarts, g.input_restarts.clone());
                    g.end();
                }
                gui_restarts += 1;
                if gui_restarts > MAX_RESTARTS {
                    println!("init: no more gui restarts: the shell is on the console");
                    gui_allowed = false;
                }
            }
            Ended::Storage => {
                println!("init: storage failed");
                // Its root was the volume's: started again with the next. The
                // keeper's `/config` was the volume's too; the jobs of the
                // rooms it made stay with `init`, for the next.
                if let Some(r) = rooms.take() {
                    r.pump(&mut kept);
                    r.end();
                }
                if let Some(c) = panel_reconnect.take() {
                    let _ = handle_close(c);
                }
                if let Some(p) = panel.take() {
                    p.end();
                }
                if let Some(c) = chooser.take() {
                    c.end();
                }
                if let Some(s) = stack.take() {
                    s.end();
                }
                if !restarts.take() {
                    println!("init: no more restarts: the volume stays down");
                    storage_allowed = false;
                }
            }
        }
    }
}

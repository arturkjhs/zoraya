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
use zoraya_user::stack::{Stack, end_job, moved, start_stack};
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
const KEY_INPUT: u64 = 16;

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
struct Session {
    ttyd: (Handle, Handle),
    broker: (Handle, Handle),
}

fn start_session(root: &File, window: Option<Handle>) -> Option<Session> {
    let base = root.time_base().unwrap_or(0);
    let (out_read, out_write) = stream_create().ok()?;
    let (control_service, control_broker) = channel_create().ok()?;
    let (first_service, first_broker) = channel_create().ok()?;
    let job = job_create(JOB, JobFlags::NONE).ok()?;
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
    let job = job_create(JOB, JobFlags::POSIX).ok()?;
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

/// Starts the shell, with the volume's root at slot 5 and, in GUI mode, its
/// terminal at slot 6; with a broker beside it, its client connection at 7
/// and the temporary service's root at 8. With a terminal the shell gets the console to write
/// only: its input is the terminal's (INV-0097-3).
fn start_shell(
    root: Option<File>,
    terminal: Option<Handle>,
    broker: Option<&Beside>,
) -> Option<(Handle, Handle)> {
    let job = job_create(JOB, JobFlags::NONE).ok()?;
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
    let length = e.finish()?;
    let process = spawn(&buffer[..length]).ok();
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
    let mut driver_restarts = (0, alloc::vec![0; devices.inputs.len()]);
    let mut shell_failures = 0;
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
            stack = start_stack(device, range);
            if stack.is_none() {
                println!("init: the storage stack did not start");
                storage_allowed = false;
            }
        }
        if gui.is_none()
            && gui_allowed
            && let Some(gpu) = gui_devices
        {
            gui = Gui::start(gpu, &devices.inputs);
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
            panel = g.start_panel(&s.root);
            if panel.is_none() {
                println!("init: the panel did not start");
            }
        }
        let wants_session = stack.as_ref().is_some_and(|s| {
            s.root
                .open("etc/zoraya/session", 0, zoraya_proto_file::rights::READ)
                .is_ok()
        });
        let mut session = None;
        let mut beside = None;
        let shell = if wants_session {
            println!("init: /etc/zoraya/session: a POSIX session");
            let root = &stack.as_ref().expect("a stack").root;
            session = start_session(root, terminal.map(|t| t.1));
            session.as_ref().map(|s| s.broker)
        } else {
            beside = stack.as_ref().and_then(|s| start_broker(&s.root));
            let root = stack.as_ref().and_then(|s| s.root.clone_as(ALL).ok());
            start_shell(root, terminal.map(|t| t.1), beside.as_ref())
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
        if let Some(s) = &session {
            subscribe(port, s.ttyd.1, KEY_TTYD);
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
                if let Some(p) = panel.take() {
                    p.end();
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
                // Its root was the volume's: started again with the next.
                if let Some(p) = panel.take() {
                    p.end();
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

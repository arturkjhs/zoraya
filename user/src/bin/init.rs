#![no_std]
#![no_main]

//! The one process the kernel starts (ADR-0053). Its table starts the driver,
//! the filesystem service and the shell, each in a sub-job with only the
//! handles it needs, and it supervises them. On any failure in the storage
//! stack it ends the stack and the shell and starts them again, up to three
//! times (ADR-0080). When the shell ends, it syncs and turns the machine off.
//!
//! With a GPU and an input device, each alone on its page (ADR-0091), it
//! also starts the GUI (ADR-0097): the drivers, the compositor, a terminal,
//! and the shell in that terminal. A dead GUI driver is restarted alone; a
//! dead compositor or terminal ends the GUI and the shell, and starts them
//! again, up to three times.

extern crate alloc;

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{
    CONSOLE, ConsoleObject, DEADLINE_INFINITE, DIRECTORY, DirectoryObject, Handle, JOB, JobFlags,
    JobObject, Mode, ObjectType, POWER, Packet, PowerObject, Rights, channel_signals,
    process_signals,
};
use zoraya_fs_service::Restarts;
use zoraya_user::files::{ALL, File};
use zoraya_user::gui::{
    Gui, MAX_RESTARTS, Running, read_manager_events, scan, start_terminal, take_display_hung,
};
use zoraya_user::stack::{Stack, end_job, moved, start_stack};
use zoraya_user::{
    directory_lookup, dup, handle_close, job_create, port_create, port_subscribe, port_wait,
    power_off, println, process_exit_record, spawn, wait_exit,
};

const KEY_DRIVER: u64 = 1;
const KEY_SERVICE: u64 = 2;
const KEY_SHELL: u64 = 3;
const KEY_SUPERVISOR: u64 = 4;
const KEY_GPU: u64 = 5;
const KEY_COMPOSITOR: u64 = 6;
const KEY_TERMINAL: u64 = 7;
const KEY_MANAGER: u64 = 8;
const KEY_INPUT: u64 = 16;

/// Starts the shell, with the volume's root at slot 5 and, in GUI mode, its
/// terminal at slot 6. With a terminal the shell gets the console to write
/// only: its input is the terminal's (INV-0097-3).
fn start_shell(root: Option<File>, terminal: Option<Handle>) -> Option<(Handle, Handle)> {
    let job = job_create(JOB, JobFlags::NONE).ok()?;
    let image = directory_lookup(DIRECTORY, "zosh").ok()?;
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, job, image);
    let console = if terminal.is_some() {
        Rights::WRITE
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
    if let Some(root) = &root {
        e.handle(moved(5, root.end));
    }
    if let Some(terminal) = terminal {
        e.handle(moved(6, terminal));
    }
    let length = e.finish()?;
    let process = spawn(&buffer[..length]).ok();
    if process.is_some() {
        // Moved: the shell's now.
        core::mem::forget(root);
    } else if let Some(terminal) = terminal {
        let _ = handle_close(terminal);
    }
    let _ = handle_close(image);
    Some((job, process?))
}

fn power_down(root: Option<&File>, status_line: zoraya_abi::ExitRecord) -> ! {
    println!("init: shell ended: {status_line:?}");
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

/// Why the shell and its terminal ended.
enum Ended {
    Storage,
    Gui,
}

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
    let mut driver_restarts = (0, alloc::vec![0; devices.inputs.len()]);
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
        let root = stack.as_ref().and_then(|s| s.root.clone_as(ALL).ok());
        let shell = start_shell(root, terminal.map(|t| t.1));
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
        let ended = loop {
            let mut packets = [Packet::default()];
            port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
            match packets[0].key {
                KEY_SHELL => {
                    // A shell that ended because its terminal went is a GUI
                    // failure; otherwise it chose to end.
                    let terminal_gone =
                        terminal.is_some_and(|(t, _)| process_exit_record(t.process).is_ok());
                    if !terminal_gone {
                        power_down(stack.as_ref().map(|s| &s.root), wait_exit(shell.1));
                    }
                    break Ended::Gui;
                }
                KEY_GPU | KEY_MANAGER => {
                    let g = gui.as_mut().expect("a GUI");
                    if packets[0].key == KEY_MANAGER {
                        read_manager_events(g.manager);
                        if !take_display_hung() {
                            continue;
                        }
                        // ADR-0093, Decision: the compositor dropped a
                        // display driver that missed its deadline.
                        println!("init: the GPU driver hung");
                    }
                    if !g.restart_gpu() && g.gpud.is_some() {
                        println!("init: the compositor did not take the new GPU driver");
                    }
                    // Watched whenever it runs, whatever the compositor said.
                    if let Some(r) = g.gpud {
                        println!("init: the GPU driver was restarted");
                        subscribe(port, r.process, KEY_GPU);
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
                KEY_COMPOSITOR | KEY_TERMINAL => break Ended::Gui,
                _ => break Ended::Storage,
            }
        };
        end_job(shell);
        if let Some((t, _)) = terminal {
            t.end();
        }
        let _ = handle_close(port);
        match ended {
            Ended::Gui => {
                println!("init: the gui failed");
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

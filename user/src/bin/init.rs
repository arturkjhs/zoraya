#![no_std]
#![no_main]

//! The one process the kernel starts (ADR-0053). Its table starts the driver,
//! the filesystem service and the shell, each in a sub-job with only the
//! handles it needs, and it supervises them. On any failure in the storage
//! stack it ends all three and starts them again, up to three times
//! (ADR-0080). When the shell ends, it syncs and turns the machine off.

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{
    CONSOLE, ConsoleObject, DEADLINE_INFINITE, DIRECTORY, DirectoryObject, Handle, JOB, JobFlags,
    JobObject, Mode, ObjectType, POWER, Packet, PowerObject, Rights, channel_signals,
    process_signals,
};
use zoraya_fs_service::Restarts;
use zoraya_user::files::{ALL, File};
use zoraya_user::stack::{end_job, find_disk, moved, start_stack};
use zoraya_user::{
    directory_lookup, dup, handle_close, job_create, port_create, port_subscribe, port_wait,
    power_off, println, spawn, wait_exit,
};

const KEY_DRIVER: u64 = 1;
const KEY_SERVICE: u64 = 2;
const KEY_SHELL: u64 = 3;
const KEY_SUPERVISOR: u64 = 4;

fn start_shell(root: Option<File>) -> Option<(Handle, Handle)> {
    let job = job_create(JOB, JobFlags::NONE).ok()?;
    let image = directory_lookup(DIRECTORY, "zosh").ok()?;
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, job, image);
    e.handle(dup(CONSOLE.slot(), CONSOLE, ConsoleObject::RIGHTS));
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
    let length = e.finish()?;
    let process = spawn(&buffer[..length]).ok();
    if process.is_some() {
        // Moved: the shell's now.
        core::mem::forget(root);
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

fn no_storage() -> ! {
    let Some(shell) = start_shell(None) else {
        println!("init: the shell did not start");
        zoraya_user::process_exit(1);
    };
    power_down(None, wait_exit(shell.1))
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let Some((device, range)) = find_disk() else {
        no_storage();
    };
    let mut restarts = Restarts::new();
    loop {
        let Some(stack) = start_stack(device, range) else {
            println!("init: the storage stack did not start");
            no_storage();
        };
        let shell = match stack.root.clone_as(ALL) {
            Ok(root) => start_shell(Some(root)),
            Err(_) => None,
        };
        // A healthy stack with no shell would leave nothing to wake `init`.
        if shell.is_none() && stack.root.sync().is_ok() {
            println!("init: the shell did not start");
            stack.end();
            zoraya_user::process_exit(1);
        }
        let port = port_create().expect("port");
        let terminated = process_signals::TERMINATED;
        let watched = [(stack.driver.1, KEY_DRIVER), (stack.service.1, KEY_SERVICE)];
        for (process, key) in watched {
            port_subscribe(port, process, terminated, key, Mode::Level).expect("subscribe");
        }
        let readable = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        port_subscribe(
            port,
            stack.supervisor,
            readable,
            KEY_SUPERVISOR,
            Mode::Level,
        )
        .expect("subscribe");
        if let Some(shell) = shell {
            port_subscribe(port, shell.1, terminated, KEY_SHELL, Mode::Level).expect("subscribe");
        }
        let mut packets = [Packet::default()];
        port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
        if packets[0].key == KEY_SHELL
            && let Some(shell) = shell
        {
            power_down(Some(&stack.root), wait_exit(shell.1));
        }
        println!("init: storage failed");
        if let Some(shell) = shell {
            end_job(shell);
        }
        stack.end();
        let _ = handle_close(port);
        if !restarts.take() {
            break;
        }
    }
    println!("init: no more restarts: the volume stays down");
    no_storage();
}

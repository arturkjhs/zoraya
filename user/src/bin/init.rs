#![no_std]
#![no_main]

//! The one process the kernel starts (ADR-0053). Its table starts the driver,
//! the filesystem service and the shell, each in a sub-job with only the
//! handles it needs, and it supervises them. On any failure in the storage
//! stack it ends all three and starts them again, up to three times
//! (ADR-0080). When the shell ends, it syncs and turns the machine off.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelObject, ConsoleObject, DEADLINE_INFINITE, DIRECTORY, DirectoryObject,
    FIRST_DEVICE_SLOT, Handle, JOB, JobFlags, JobObject, Mode, ObjectType, POWER, Packet,
    PowerObject, Rights, channel_signals, process_signals,
};
use zoraya_user::files::{ALL, File};
use zoraya_user::{
    channel_create, device_map, directory_lookup, dup, handle_close, handle_type, job_create,
    job_kill, memory_unmap, port_create, port_subscribe, port_wait, power_off, println, spawn,
    wait_exit,
};

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const DEVICE_BLOCK: u32 = 2;
const MAX_RESTARTS: u32 = 3;
const DRIVER_RIGHTS: Rights = Rights::MAP.union(Rights::INTERRUPT).union(Rights::DMA);

const KEY_DRIVER: u64 = 1;
const KEY_SERVICE: u64 = 2;
const KEY_SHELL: u64 = 3;
const KEY_SUPERVISOR: u64 = 4;

/// The block device, by its register values.
fn find_disk() -> Option<(Handle, u32)> {
    for slot in FIRST_DEVICE_SLOT..FIRST_DEVICE_SLOT + 16 {
        let device = Handle::new(u64::from(slot))?;
        if handle_type(device) != Ok("device") {
            continue;
        }
        for range in 0..8 {
            let Ok((window, offset)) = device_map(device, range) else {
                break;
            };
            let register = |at: u64| {
                // SAFETY: a device mapping of this process, at a register.
                unsafe { ((window + offset + at) as *const u32).read_volatile() }
            };
            let found = register(0) == VIRTIO_MAGIC && register(8) == DEVICE_BLOCK;
            let _ = memory_unmap(window, 4096);
            if found {
                return Some((device, range));
            }
        }
    }
    None
}

fn moved(slot: u32, handle: Handle) -> HandleEntry {
    HandleEntry {
        slot,
        handle,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    }
}

fn console() -> HandleEntry {
    dup(CONSOLE.slot(), CONSOLE, Rights::WRITE)
}

/// A process in a sub-job of its own: the job and the process.
fn start(name: &str, entries: &[HandleEntry], args: &[&[u8]]) -> Option<(Handle, Handle)> {
    let job = job_create(JOB, JobFlags::NONE).ok()?;
    let image = directory_lookup(DIRECTORY, name).ok()?;
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, job, image);
    for entry in entries {
        e.handle(*entry);
    }
    for arg in args {
        e.arg(arg);
    }
    let length = e.finish()?;
    let process = spawn(&buffer[..length]).ok();
    let _ = handle_close(image);
    Some((job, process?))
}

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

/// What is running on the disk.
struct Stack {
    driver: (Handle, Handle),
    service: (Handle, Handle),
    supervisor: Handle,
    root: File,
}

fn start_stack(device: Handle, range: u32) -> Option<Stack> {
    let (service_end, driver_end) = channel_create().ok()?;
    let (root_client, root_server) = channel_create().ok()?;
    let (supervisor, supervisor_end) = channel_create().ok()?;
    let mut digits = [0u8; 1];
    digits[0] = b'0' + range as u8;
    let driver = start(
        "blkd",
        &[
            console(),
            dup(5, device, DRIVER_RIGHTS),
            moved(6, driver_end),
        ],
        &[&digits],
    )?;
    let service = start(
        "fsd",
        &[
            console(),
            moved(5, service_end),
            moved(6, root_server),
            moved(7, supervisor_end),
        ],
        &[],
    )?;
    Some(Stack {
        driver,
        service,
        supervisor,
        root: File { end: root_client },
    })
}

fn end_job(job: (Handle, Handle)) {
    let _ = job_kill(job.0);
    wait_exit(job.1);
    let _ = handle_close(job.1);
    let _ = handle_close(job.0);
}

fn power_down(root: Option<&File>, status_line: zoraya_abi::ExitRecord) -> ! {
    println!("init: shell ended: {status_line:?}");
    if let Some(root) = root {
        let _ = root.sync();
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
    for restart in 0..=MAX_RESTARTS {
        let Some(stack) = start_stack(device, range) else {
            break;
        };
        let shell = match stack.root.clone_as(ALL) {
            Ok(root) => start_shell(Some(root)),
            Err(_) => None,
        };
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
        println!("init: storage failed, restart {}", restart + 1);
        if let Some(shell) = shell {
            end_job(shell);
        }
        end_job(stack.service);
        end_job(stack.driver);
        let _ = handle_close(stack.supervisor);
        let _ = handle_close(port);
    }
    println!("init: no more restarts: the volume stays down");
    no_storage();
}

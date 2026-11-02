//! The storage stack as `init` starts it (ADR-0053, ADR-0054): the driver and
//! the service, each in a sub-job. A test starts the same one.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelObject, DIRECTORY, FIRST_DEVICE_SLOT, Handle, JOB, JobFlags, ObjectType, Rights,
};
use zoraya_rt::{
    channel_create, device_map, directory_lookup, handle_close, handle_type, job_create, job_kill,
    memory_unmap, spawn,
};

use crate::files::File;
use crate::{dup, wait_exit};

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const DEVICE_BLOCK: u32 = 2;
const DRIVER_RIGHTS: Rights = Rights::MAP.union(Rights::INTERRUPT).union(Rights::DMA);

/// The block device, by its register values.
pub fn find_disk() -> Option<(Handle, u32)> {
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

pub fn moved(slot: u32, handle: Handle) -> HandleEntry {
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
pub fn start(name: &str, entries: &[HandleEntry], args: &[&[u8]]) -> Option<(Handle, Handle)> {
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

/// What is running on the disk.
pub struct Stack {
    pub driver: (Handle, Handle),
    pub service: (Handle, Handle),
    pub supervisor: Handle,
    pub root: File,
}

impl Stack {
    /// Ends what is left of it.
    pub fn end(self) {
        end_job(self.service);
        end_job(self.driver);
        let _ = handle_close(self.supervisor);
    }
}

pub fn start_stack(device: Handle, range: u32) -> Option<Stack> {
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

pub fn end_job(job: (Handle, Handle)) {
    // Already ended, it is `BadState` or nothing: either way it is over.
    let _ = job_kill(job.0);
    wait_exit(job.1);
    let _ = handle_close(job.1);
    let _ = handle_close(job.0);
}

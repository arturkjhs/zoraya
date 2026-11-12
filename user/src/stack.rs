//! The storage stack as `init` starts it (ADR-0053, ADR-0054): the driver and
//! the service, each in a sub-job. A test starts the same one.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelObject, DIRECTORY, Handle, JOB, JobFlags, ObjectType, RTC_SLOT, Rights,
};
use zoraya_rt::{
    channel_create, device_map, directory_lookup, handle_close, handle_type, job_create, job_kill,
    memory_unmap, spawn,
};

use crate::files::File;
use crate::{dup, wait_exit};

pub(crate) const DRIVER_RIGHTS: Rights = Rights::MAP.union(Rights::INTERRUPT).union(Rights::DMA);

/// The block device: the one page holding exactly one, a block device
/// (ADR-0091).
pub fn find_disk() -> Option<(Handle, u32)> {
    crate::gui::scan().disk
}

pub fn moved(slot: u32, handle: Handle) -> HandleEntry {
    HandleEntry {
        slot,
        handle,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    }
}

pub fn console() -> HandleEntry {
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

/// The real-time clock in nanoseconds since the epoch, read once through the
/// PL031 `init` holds at `RTC_SLOT` (ADR-0226 item 2); 0 when there is no
/// device or it gave nothing.
pub fn read_rtc() -> u64 {
    let Some(device) = Handle::new(u64::from(RTC_SLOT)) else {
        return 0;
    };
    if handle_type(device) != Ok("device") {
        return 0;
    }
    let Ok((window, offset)) = device_map(device, 0) else {
        return 0;
    };
    // SAFETY: a device mapping of this process at range 0, whose offset the
    // kernel checked holds the PL031's registers; `RTC_DR` is the 32-bit
    // word at offset 0, aligned.
    let seconds = unsafe { ((window + offset) as *const u32).read_volatile() };
    let _ = memory_unmap(window, 4096);
    u64::from(seconds) * 1_000_000_000
}

/// A volume under test: `/system` is not sealed.
pub fn start_stack(device: Handle, range: u32) -> Option<Stack> {
    start_stack_with(device, range, 0, false)
}

/// As [`start_stack`], with the root volume's time floor (ADR-0226): the
/// nanoseconds [`read_rtc`] gave, or 0. This is the root volume: its service
/// keeps `/system` read-only (ADR-0289 item 8).
pub fn start_stack_at(device: Handle, range: u32, floor: u64) -> Option<Stack> {
    start_stack_with(device, range, floor, true)
}

fn start_stack_with(device: Handle, range: u32, floor: u64, sealed: bool) -> Option<Stack> {
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
    let mut floor_digits = [0u8; 20];
    let floor_text = decimal(floor, &mut floor_digits);
    let fsd_handles = [
        console(),
        moved(5, service_end),
        moved(6, root_server),
        moved(7, supervisor_end),
    ];
    let mut args: [&[u8]; 3] = [&[]; 3];
    let mut count = 0;
    if floor != 0 {
        args[0] = b"--time-floor";
        args[1] = floor_text;
        count = 2;
    }
    if sealed {
        args[count] = b"--sealed-system";
        count += 1;
    }
    let service = start("fsd", &fsd_handles, &args[..count])?;
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

/// `n` in decimal, in the tail of `buffer`.
fn decimal(mut n: u64, buffer: &mut [u8; 20]) -> &[u8] {
    let mut at = buffer.len();
    loop {
        at -= 1;
        buffer[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    &buffer[at..]
}

#![no_std]
#![no_main]

//! The filesystem service (ADR-0055) as a process: one volume, one thread.
//! Slot 5 is the channel to the driver, slot 6 the end of the root connection,
//! slot 7 the supervisor channel.
//!
//! `fsd --memory MIB` is the temporary directory service (ADR-0107 item 12):
//! a fresh volume of that size in memory, with FIFO nodes, and no driver;
//! slot 6 is the root connection's end and slot 7, when present, the
//! supervisor channel.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, DEADLINE_INFINITE, EventObject, Handle, MapFlags, MemoryFlags, MemoryObject,
    Mode, ObjectType, Packet, Rights, Status, StreamReadObject, StreamWriteObject, channel_signals,
};
use zoraya_fs::{Clock, MkfsOptions, MountOptions, Timestamp, Volume, mkfs};
use zoraya_fs_service::block::{ChannelDevice, Link, LinkError};
use zoraya_fs_service::memory::MemoryDevice;
use zoraya_fs_service::{
    Device, HandleKind, Incoming, Interest, Outgoing, Service, Transport, Wake, time_base,
    wall_time,
};
use zoraya_proto_block as blk;
use zoraya_proto_file::Failure;
use zoraya_protocol::{Below, decode, encode};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, event_create, handle_duplicate, handle_type,
    memory_create, memory_map, memory_unmap, object_same, port_create, port_subscribe,
    port_unsubscribe, port_wait, println, startup, stream_create,
};

extern crate alloc;

/// The block buffer: about what one commit batch writes at once.
const DATA_SIZE: usize = 2 << 20;
/// ADR-0054's deadline on a block request.
const BLOCK_DEADLINE: u64 = 30_000_000_000;
const DRIVER_KEY: u64 = 1;
const FIRST_KEY: u64 = 16;

struct DriverLink {
    end: Handle,
    port: Handle,
    data: u64,
}

impl Link for DriverLink {
    fn call(&mut self, request: &[u8], reply: &mut [u8]) -> Result<usize, LinkError> {
        channel_write(self.end, request, &[]).map_err(|_| LinkError::Lost)?;
        let mut packets = [Packet::default()];
        let deadline = clock_monotonic() + BLOCK_DEADLINE;
        port_wait(self.port, deadline, &mut packets).map_err(|_| LinkError::Lost)?;
        match channel_read(self.end, reply, &mut []) {
            Ok(sizes) => Ok(sizes.bytes as usize),
            Err(_) => Err(LinkError::Lost),
        }
    }

    fn data(&mut self) -> &mut [u8] {
        // SAFETY: this process's mapping of `DATA_SIZE` bytes, kept for its
        // life, and the driver touches it only during a `call`.
        unsafe { core::slice::from_raw_parts_mut(self.data as *mut u8, DATA_SIZE) }
    }
}

/// The boot's time base (ADR-0191): the root volume's service sets it from
/// its volume at mount, a temporary one from `--base`.
static BASE: AtomicU64 = AtomicU64::new(0);

struct Time;

impl Clock for Time {
    fn wall(&mut self) -> Timestamp {
        // No device gives a date (ADR-0080): one base for the boot, plus the
        // monotonic clock; the engine still keeps times rising.
        wall_time(BASE.load(Ordering::Relaxed), clock_monotonic())
    }

    fn monotonic_ns(&mut self) -> u64 {
        clock_monotonic()
    }
}

/// The rights a handle keeps when it is moved or duplicated: a stream end's
/// own, else a token's (the only other kind this service hands out).
fn rights_of(h: Handle) -> Rights {
    match handle_type(h) {
        Ok("stream-read") => StreamReadObject::RIGHTS,
        Ok("stream-write") => StreamWriteObject::RIGHTS,
        _ => EventObject::RIGHTS,
    }
}

struct Kernel {
    port: Handle,
    /// `None` for a temporary service started with no supervisor.
    supervisor: Option<Handle>,
    /// Which keys hold a subscription.
    subscribed: Vec<u64>,
}

impl Transport for Kernel {
    type End = Handle;

    fn read(&mut self, end: Handle, bytes: &mut [u8], handles: &mut Vec<Handle>) -> Incoming {
        let mut got = [end; 64];
        match channel_read(end, bytes, &mut got) {
            Ok(sizes) => {
                handles.extend_from_slice(&got[..sizes.handles as usize]);
                Incoming::Message(sizes.bytes as usize)
            }
            Err((Status::WouldBlock, _)) => Incoming::Empty,
            Err((Status::PeerClosed, _)) => Incoming::Closed,
            Err(_) => Incoming::Refused,
        }
    }

    fn write(&mut self, end: Handle, bytes: &[u8], handles: &[Handle]) -> Outgoing {
        let mut entries = [ChannelEntry {
            handle: end,
            rights: EventObject::RIGHTS,
            transfer: Transfer::Move,
        }; 1];
        let list = match handles.first() {
            Some(handle) => {
                entries[0].handle = *handle;
                entries[0].rights = rights_of(*handle);
                &entries[..1]
            }
            None => &entries[..0],
        };
        match channel_write(end, bytes, list) {
            Ok(()) => Outgoing::Sent,
            Err(Status::WouldBlock) => Outgoing::Full,
            Err(Status::NoMemory) => Outgoing::NoMemory,
            Err(_) => Outgoing::Closed,
        }
    }

    fn interest(&mut self, end: Handle, key: u64, interest: Interest) -> bool {
        let key = key + FIRST_KEY;
        if let Some(at) = self.subscribed.iter().position(|k| *k == key) {
            self.subscribed.swap_remove(at);
            let _ = port_unsubscribe(self.port, key);
        }
        let signals = match interest {
            Interest::Requests => channel_signals::READABLE | channel_signals::PEER_CLOSED,
            Interest::Reply => channel_signals::WRITABLE | channel_signals::PEER_CLOSED,
            Interest::Closed => channel_signals::PEER_CLOSED,
        };
        let done = port_subscribe(self.port, end, signals, key, Mode::Level).is_ok();
        if done {
            self.subscribed.push(key);
        }
        done
    }

    fn close(&mut self, end: Handle) {
        let _ = zoraya_user::handle_close(end);
    }

    fn kind(&mut self, end: Handle) -> HandleKind {
        match handle_type(end) {
            Ok("channel") => HandleKind::Channel,
            Ok("memory") => HandleKind::Memory,
            Ok("event") => HandleKind::Token,
            _ => HandleKind::Other,
        }
    }

    fn duplicate(&mut self, end: Handle) -> Option<Handle> {
        handle_duplicate(end, rights_of(end)).ok()
    }

    fn stream(&mut self) -> Option<(Handle, Handle)> {
        stream_create().ok()
    }

    fn new_token(&mut self) -> Option<Handle> {
        event_create().ok()
    }

    fn same(&mut self, a: Handle, b: Handle) -> bool {
        object_same(a, b).unwrap_or(false)
    }

    fn map(&mut self, memory: Handle, length: u64) -> Option<u64> {
        memory_map(memory, 0, length, MapFlags::WRITE).ok()
    }

    fn unmap(&mut self, address: u64, length: u64) {
        let _ = memory_unmap(address, length);
    }

    fn copy_from(&mut self, address: u64, out: &mut [u8]) -> bool {
        // SAFETY: inside a mapping `Service` checked this range against, and
        // one the client can change, so this reads it only once, as bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(address as *const u8, out.as_mut_ptr(), out.len());
        }
        true
    }

    fn copy_to(&mut self, address: u64, bytes: &[u8]) -> bool {
        // SAFETY: as in `copy_from`.
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
        }
        true
    }

    fn report(&mut self, reason: u32) {
        let mut out = [0; 32];
        let m = Failure {
            reason: Below(reason),
        };
        // A report that cannot be sent would leave `init` waiting on a
        // volume that has failed: ending the service is a report it hears.
        let sent = self.supervisor.is_some_and(|supervisor| {
            encode(&m, 0, &mut out)
                .is_ok_and(|length| channel_write(supervisor, &out[..length], &[]).is_ok())
        });
        if !sent {
            println!("fsd: the report did not go: ending");
            zoraya_user::process_exit(1);
        }
    }

    fn wait(&mut self, deadline: Option<u64>) -> Wake {
        let mut packets = [Packet::default()];
        let until = deadline.unwrap_or(DEADLINE_INFINITE);
        match port_wait(self.port, until, &mut packets) {
            Ok(_) if packets[0].key == DRIVER_KEY => {
                // Level: it would come back on every wait.
                let _ = port_unsubscribe(self.port, DRIVER_KEY);
                Wake::DriverClosed
            }
            Ok(_) => Wake::Ready(packets[0].key - FIRST_KEY),
            Err(Status::DeadlineExceeded) => Wake::Deadline,
            Err(_) => Wake::Stop,
        }
    }

    fn now(&mut self) -> u64 {
        clock_monotonic()
    }
}

fn slot(n: u64) -> Handle {
    Handle::new(n).expect("slot is not zero")
}

/// Serves the volume until every connection is gone.
fn serve<D: Device>(volume: Volume<D, Time>, kernel: Kernel, root: Handle, fifos: bool) -> i32 {
    let mut service = Box::new(Service::new(volume, kernel));
    service.set_time_base(BASE.load(Ordering::Relaxed));
    if fifos {
        service.enable_fifos();
    }
    if !service.add_root(root) {
        println!("fsd: the root connection cannot be served");
        return 1;
    }
    service.run();
    0
}

/// The value after `name` among the arguments, as a number.
fn option(name: &[u8]) -> Option<u64> {
    let mut argv = startup().argv;
    while let Some(a) = argv.next() {
        if a == name {
            return core::str::from_utf8(argv.next()?).ok()?.parse().ok();
        }
    }
    None
}

/// `--memory MIB`: the size of the temporary volume, if the option is there.
fn memory_option() -> Option<u64> {
    option(b"--memory").map(|mib| mib.clamp(1, 1024) << 20)
}

/// The temporary directory service: a fresh volume in memory (ADR-0107
/// item 12), the root world-writable and sticky as `/tmp` is.
fn temporary(size: u64) -> i32 {
    // The boot's base, from whoever started this service (ADR-0191).
    BASE.store(option(b"--base").unwrap_or(0), Ordering::Relaxed);
    let root = slot(6);
    let supervisor = slot(7);
    let supervisor = handle_type(supervisor).is_ok().then_some(supervisor);
    let Ok(memory) = memory_create(size, MemoryFlags::NONE) else {
        println!("fsd: no memory for a {} MiB volume", size >> 20);
        return 1;
    };
    let address = memory_map(memory, 0, size, MapFlags::WRITE).expect("map");
    // SAFETY: this process's mapping of `size` bytes, kept for its life and
    // touched by nothing but the device made over it here.
    let bytes = unsafe { core::slice::from_raw_parts_mut(address as *mut u8, size as usize) };
    let mut device = MemoryDevice::new(bytes);
    let options = MkfsOptions {
        volume_id: clock_monotonic() | 1,
        now: Time.wall(),
        root_mode: 0o1777,
    };
    if let Err(e) = mkfs(&mut device, options) {
        println!("fsd: mkfs: {e:?}");
        return 1;
    }
    let volume = match Volume::mount(device, Time, MountOptions::default()) {
        Ok(volume) => volume,
        Err(e) => {
            println!("fsd: mount: {e:?}");
            return 1;
        }
    };
    let kernel = Kernel {
        port: port_create().expect("port"),
        supervisor,
        subscribed: Vec::new(),
    };
    serve(volume, kernel, root, true)
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    if let Some(size) = memory_option() {
        return temporary(size);
    }
    let driver = slot(5);
    let root = slot(6);
    let supervisor = slot(7);

    let size = DATA_SIZE as u64;
    let memory = memory_create(size, MemoryFlags::CONTIGUOUS).expect("data memory");
    let data = memory_map(memory, 0, size, MapFlags::WRITE).expect("map");
    let block_port = port_create().expect("port");
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(block_port, driver, wanted, 1, Mode::Level).expect("subscribe");

    // The driver pins this object for its one transfer buffer.
    let mut out = [0; 64];
    let length = encode(&blk::Attach {}, 1, &mut out).expect("fits");
    let entry = ChannelEntry {
        handle: memory,
        rights: MemoryObject::RIGHTS,
        transfer: Transfer::Duplicate,
    };
    channel_write(driver, &out[..length], &[entry]).expect("attach");
    let mut packets = [Packet::default()];
    port_wait(block_port, DEADLINE_INFINITE, &mut packets).expect("attach reply");
    let sizes = channel_read(driver, &mut out, &mut []).expect("attach reply");
    let attached = decode::<blk::Done>(&out[..sizes.bytes as usize], 0);
    if !attached.is_ok_and(|(_, m)| m.status.0 == blk::status::OK) {
        println!("fsd: the driver refused the transfer buffer");
        return 1;
    }

    let link = DriverLink {
        end: driver,
        port: block_port,
        data,
    };
    let Ok(device) = ChannelDevice::new(link) else {
        println!("fsd: no answer from the driver");
        return 1;
    };
    let volume = match Volume::mount(device, Time, MountOptions::default()) {
        Ok(volume) => volume,
        Err(e) => {
            println!("fsd: mount: {e:?}");
            return 1;
        }
    };
    // The root volume's service sets the boot's base (ADR-0191).
    let latest = volume.last_assigned();
    // The floor is the real-time clock's reading `init` took (ADR-0226 item 3).
    let floor = option(b"--time-floor").unwrap_or(0);
    // The lowest bit of the base says that the clock gave the date.
    BASE.store(
        zoraya_abi::native::tag_base(time_base(latest, clock_monotonic(), floor), floor != 0),
        Ordering::Relaxed,
    );
    let port = port_create().expect("port");
    port_subscribe(
        port,
        driver,
        channel_signals::PEER_CLOSED,
        DRIVER_KEY,
        Mode::Level,
    )
    .expect("subscribe");
    let kernel = Kernel {
        port,
        supervisor: Some(supervisor),
        subscribed: Vec::new(),
    };
    serve(volume, kernel, root, false)
}

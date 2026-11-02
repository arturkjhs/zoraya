#![no_std]
#![no_main]

//! The filesystem service (ADR-0055) as a process: one volume, one thread.
//! Slot 5 is the channel to the driver, slot 6 the end of the root connection,
//! slot 7 the supervisor channel.

use alloc::boxed::Box;
use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, DEADLINE_INFINITE, EventObject, Handle, MapFlags, MemoryFlags, MemoryObject,
    Mode, ObjectType, Packet, Status, channel_signals,
};
use zoraya_fs::{Clock, MountOptions, Timestamp, Volume};
use zoraya_fs_service::block::{ChannelDevice, Link, LinkError};
use zoraya_fs_service::{HandleKind, Incoming, Interest, Outgoing, Service, Transport, Wake};
use zoraya_proto_block as blk;
use zoraya_proto_file::Failure;
use zoraya_protocol::{Below, decode, encode};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, event_create, handle_duplicate, handle_type,
    memory_create, memory_map, memory_unmap, object_same, port_create, port_subscribe,
    port_unsubscribe, port_wait, println,
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

struct Time;

impl Clock for Time {
    fn wall(&mut self) -> Timestamp {
        // No device gives a date (ADR-0080): the engine keeps times rising.
        Timestamp {
            secs: (clock_monotonic() / 1_000_000_000) as i64,
            nanos: 0,
        }
    }

    fn monotonic_ns(&mut self) -> u64 {
        clock_monotonic()
    }
}

struct Kernel {
    port: Handle,
    supervisor: Handle,
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
        handle_duplicate(end, EventObject::RIGHTS).ok()
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
        let sent = encode(&m, 0, &mut out)
            .is_ok_and(|length| channel_write(self.supervisor, &out[..length], &[]).is_ok());
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

#[unsafe(no_mangle)]
fn main() -> i32 {
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
        supervisor,
        subscribed: Vec::new(),
    };
    let mut service = Box::new(Service::new(volume, kernel));
    service.add_root(root);
    service.run();
    0
}

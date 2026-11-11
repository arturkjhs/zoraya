#![no_std]
#![no_main]

//! The network service (ADR-0221 item 3): one process, one thread, one port.
//! It is the client of `ethd` (frames in a shared object, `zoraya-proto-nic`),
//! runs smoltcp over them (`zoraya-net`), and serves the scope channels of
//! `zoraya-proto-net`. It holds no device, interrupt or pin (INV-0221-1).
//!
//! Slots: 5 the channel to `ethd`, 6 a read-only `/config` directory
//! connection (optional; read once at start and closed), 7 the root
//! connection `init` holds the other end of; `init` makes every launcher's
//! connection with `narrow` on it. Everything it parses from outside the
//! machine goes through `zoraya-net` (smoltcp's parsers over a private copy
//! of each frame); this file only moves bytes between the channel, the
//! frames object and that crate.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, MemoryObject, Mode, ObjectType,
    Packet, Status, channel_signals,
};
use zoraya_net::config;
use zoraya_net::connection::Connection;
use zoraya_net::nic::{Nic, Read, SlotMemory, Wire, Write};
use zoraya_net::service::{Action, Service};
use zoraya_net::stack::{Event, Stack};
use zoraya_proto_nic::{
    Attach, Done, Info, InfoReply, OBJECT_SIZE, SLOT_SIZE, SLOTS, status as nic_status,
};
use zoraya_protocol::{Header, decode, encode};
use zoraya_user::files::{File, read_all};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, handle_close, handle_type, memory_create,
    memory_map, port_create, port_subscribe, port_unsubscribe, port_wait, println, process_exit,
};

zoraya_user::describe!("system", "netd");

const ETHD: u64 = 5;
const CONFIG: u64 = 6;
const ROOT: u64 = 7;

const KEY_ETHD: u64 = 1;
const KEY_WRITABLE: u64 = 2;
const KEY_CONNECTION: u64 = 100;

/// How long the driver may take to answer `info` and `attach`.
const HANDSHAKE_NS: u64 = 5_000_000_000;
/// The room for one request: a header and the largest scope.
const REQUEST: usize = 32 * 1024;
/// Requests read from one connection per turn.
const PER_TURN: usize = 16;

fn slot(n: u64) -> Handle {
    Handle::new(n).expect("a slot is not zero")
}

/// The channel to `ethd`.
struct Channel(Handle);

impl Wire for Channel {
    fn read(&mut self, buf: &mut [u8]) -> Read {
        match channel_read(self.0, buf, &mut []) {
            Ok(sizes) => Read::Message(sizes.bytes as usize),
            Err((Status::WouldBlock, _)) => Read::Empty,
            // Closed, or a message with a handle or too large: either way
            // the driver is not following the protocol.
            Err(_) => Read::Closed,
        }
    }

    fn write(&mut self, bytes: &[u8]) -> Write {
        match channel_write(self.0, bytes, &[]) {
            Ok(()) => Write::Done,
            Err(Status::WouldBlock) => Write::Full,
            Err(_) => Write::Closed,
        }
    }
}

/// The frames object, mapped. The driver can write a lent slot while we read
/// it, so reads are volatile bytes into a private buffer.
struct Frames {
    base: u64,
}

impl SlotMemory for Frames {
    fn read_slot(&self, slot: u32, out: &mut [u8]) {
        // `Nic` checks the slot and the length; checked again, because an
        // index out of the object would read someone else's memory.
        if slot >= 2 * SLOTS || out.len() > SLOT_SIZE as usize {
            return;
        }
        let at = self.base + u64::from(slot) * u64::from(SLOT_SIZE);
        for (i, b) in out.iter_mut().enumerate() {
            // SAFETY: `at + i` is inside the object this process mapped
            // (`OBJECT_SIZE` bytes from `base`): the slot is below `2 *
            // SLOTS`, `i` below `SLOT_SIZE`, and the object holds `2 * SLOTS`
            // slots of `SLOT_SIZE`.
            *b = unsafe { ((at + i as u64) as *const u8).read_volatile() };
        }
    }

    fn write_slot(&mut self, slot: u32, data: &[u8]) {
        if slot >= 2 * SLOTS || data.len() > SLOT_SIZE as usize {
            return;
        }
        let at = self.base + u64::from(slot) * u64::from(SLOT_SIZE);
        for (i, b) in data.iter().enumerate() {
            // SAFETY: as in `read_slot`; the map is writable.
            unsafe { ((at + i as u64) as *mut u8).write_volatile(*b) };
        }
    }
}

type Link = Nic<Channel, Frames>;

/// Reads one reply of `ordinal` from the driver, waiting up to `HANDSHAKE_NS`;
/// pushed messages that arrive first (a link change) are skipped.
fn await_reply(port: Handle, end: Handle, ordinal: u32) -> Option<Vec<u8>> {
    let until = clock_monotonic() + HANDSHAKE_NS;
    let mut bytes = vec![0u8; 256];
    loop {
        match channel_read(end, &mut bytes, &mut []) {
            Ok(sizes) => {
                let message = &bytes[..sizes.bytes as usize];
                if Header::decode(message).is_ok_and(|(h, _)| h.ordinal == ordinal) {
                    return Some(message.to_vec());
                }
            }
            Err((Status::WouldBlock, _)) => {
                let mut packets = [Packet::default()];
                port_wait(port, until, &mut packets).ok()?;
            }
            Err(_) => return None,
        }
    }
}

/// Everything `netd` has once it is running.
struct Netd {
    port: Handle,
    stack: Stack<Link>,
    service: Service,
    /// The channel of each connection, by the service's id.
    channels: Vec<Option<Handle>>,
    ethd: Handle,
    watching: bool,
    request: Vec<u8>,
}

impl Netd {
    fn now_us() -> i64 {
        (clock_monotonic() / 1000) as i64
    }

    fn attach_connection(&mut self, id: usize, end: Handle) {
        if self.channels.len() <= id {
            self.channels.resize(id + 1, None);
        }
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        let key = KEY_CONNECTION + id as u64;
        if port_subscribe(self.port, end, wanted, key, Mode::Level).is_err() {
            let _ = handle_close(end);
            self.service.remove(id);
            return;
        }
        self.channels[id] = Some(end);
    }

    fn drop_connection(&mut self, id: usize) {
        if let Some(end) = self.channels.get_mut(id).and_then(Option::take) {
            let _ = port_unsubscribe(self.port, KEY_CONNECTION + id as u64);
            let _ = handle_close(end);
        }
        self.service.remove(id);
    }

    fn write_to(&mut self, id: usize, bytes: &[u8]) {
        let Some(end) = self.channels.get(id).copied().flatten() else {
            return;
        };
        // One request at a time per connection: a full channel means the
        // client does not read its replies, and it loses the connection.
        if bytes.is_empty() || channel_write(end, bytes, &[]).is_err() {
            self.drop_connection(id);
        }
    }

    fn serve(&mut self, id: usize) {
        for _ in 0..PER_TURN {
            let Some(end) = self.channels.get(id).copied().flatten() else {
                return;
            };
            let mut handles = [end; 4];
            let sizes = match channel_read(end, &mut self.request, &mut handles) {
                Ok(s) => s,
                Err((Status::WouldBlock, _)) => return,
                Err(_) => {
                    self.drop_connection(id);
                    return;
                }
            };
            let count = (sizes.handles as usize).min(handles.len());
            let message = self.request[..sizes.bytes as usize].to_vec();
            let action = self
                .service
                .handle(id, &message, count, &mut self.stack, Self::now_us());
            match action {
                Action::Narrowed { id: new, reply } => {
                    let given = handles[0];
                    if count == 1 && handle_type(given) == Ok("channel") {
                        self.attach_connection(new, given);
                        self.write_to(id, &reply);
                    } else {
                        // A handle that is not a channel end: the request
                        // was not the one it said.
                        self.service.remove(new);
                        for h in &handles[..count] {
                            let _ = handle_close(*h);
                        }
                        self.drop_connection(id);
                        return;
                    }
                }
                other => {
                    for h in &handles[..count] {
                        let _ = handle_close(*h);
                    }
                    match other {
                        Action::Reply(bytes) => self.write_to(id, &bytes),
                        Action::Close => {
                            self.drop_connection(id);
                            return;
                        }
                        Action::Deferred | Action::Narrowed { .. } => {}
                    }
                }
            }
        }
    }

    /// Moves the stack, writes what it queued, and reports what happened.
    /// Returns the delay to the next turn in microseconds.
    fn turn(&mut self) -> Option<i64> {
        let now = Self::now_us();
        self.stack.link().pump();
        let delay = self.stack.poll(now);
        self.stack.link().flush();
        for event in self.stack.take_events() {
            match event {
                Event::Configured => {
                    let s = self.stack.status(now);
                    let (a, g) = (s.address, s.gateway);
                    println!(
                        "netd: {}.{}.{}.{}/{} gateway {}.{}.{}.{}, {} dns server(s) ({})",
                        a[0],
                        a[1],
                        a[2],
                        a[3],
                        s.prefix,
                        g[0],
                        g[1],
                        g[2],
                        g[3],
                        s.dns.len(),
                        if s.lease_left.is_some() {
                            "dhcp"
                        } else {
                            "static"
                        }
                    );
                }
                Event::Deconfigured => println!("netd: the lease is gone"),
                Event::PingDone(result) => {
                    if let Some((id, bytes)) = self.service.ping_done(result) {
                        self.write_to(id, &bytes);
                    }
                }
            }
        }
        if self.stack.link().closed() {
            println!("netd: the network driver is gone");
            process_exit(1);
        }
        // What the channel to the driver could not take waits for it.
        let pending = self.stack.link().pending();
        if pending && !self.watching {
            let _ = port_subscribe(
                self.port,
                self.ethd,
                channel_signals::WRITABLE,
                KEY_WRITABLE,
                Mode::Level,
            );
            self.watching = true;
        } else if !pending && self.watching {
            let _ = port_unsubscribe(self.port, KEY_WRITABLE);
            self.watching = false;
        }
        delay
    }
}

/// The settings, read once from the `/config` connection, then closed.
fn settings() -> Vec<u8> {
    let end = slot(CONFIG);
    if handle_type(end) != Ok("channel") {
        return Vec::new();
    }
    let config = File { end };
    let bytes = config
        .open("settings", 0, zoraya_proto_file::rights::READ)
        .ok()
        .and_then(|f| read_all(&f).ok())
        .unwrap_or_default();
    drop(config);
    bytes
}

/// INV-0221-1: `netd` holds no `Device`, `Interrupt` or `Pin`. Its start-up
/// table is looked at, and it says so on the console, where the test reads it.
fn check_table() -> bool {
    let table = zoraya_user::start_table();
    let held = |kind: &str| table.iter().any(|(k, _)| *k == kind);
    if held("device") || held("interrupt") || held("pin") {
        println!("netd: holds a device, interrupt or pin");
        return false;
    }
    println!("netd: holds no device, interrupt or pin");
    true
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    if !check_table() {
        return 1;
    }
    let (config, problems) = config::read(&settings());
    for p in problems {
        println!("{p}");
    }
    let ethd = slot(ETHD);
    let port = port_create().expect("port");
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, ethd, wanted, KEY_ETHD, Mode::Level).expect("subscribe");
    // The driver: what the device is, then the frames object.
    let mut out = [0u8; 256];
    let n = encode(&Info {}, 1, &mut out).expect("fits");
    if channel_write(ethd, &out[..n], &[]).is_err() {
        println!("netd: no network driver");
        return 1;
    }
    let Some(reply) = await_reply(port, ethd, 1) else {
        println!("netd: the network driver did not answer");
        return 1;
    };
    let Ok((_, info)) = decode::<InfoReply>(&reply, 0) else {
        println!("netd: the network driver's answer is not understood");
        return 1;
    };
    let mac: Option<[u8; 6]> = info.mac.0.try_into().ok();
    let (true, Some(mac)) = (info.status.0 == nic_status::OK, mac) else {
        println!("netd: the network driver has no usable device");
        return 1;
    };
    let Ok(frames) = memory_create(OBJECT_SIZE, MemoryFlags::CONTIGUOUS) else {
        println!("netd: no memory for frames");
        return 1;
    };
    let Ok(base) = memory_map(frames, 0, OBJECT_SIZE, MapFlags::WRITE) else {
        println!("netd: cannot map the frames");
        return 1;
    };
    let n = encode(&Attach {}, 2, &mut out).expect("fits");
    let entry = ChannelEntry {
        handle: frames,
        rights: MemoryObject::RIGHTS,
        transfer: Transfer::Duplicate,
    };
    if channel_write(ethd, &out[..n], &[entry]).is_err() {
        println!("netd: no network driver");
        return 1;
    }
    let attached = await_reply(port, ethd, 2)
        .and_then(|r| decode::<Done>(&r, 0).ok().map(|(_, d)| d.status.0));
    if attached != Some(nic_status::OK) {
        println!("netd: the network driver refused the frames");
        return 1;
    }
    let _ = handle_close(frames);
    println!(
        "netd: net0 {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
    let nic = Nic::new(Channel(ethd), Frames { base }, info.link);
    // Not a secret: smoltcp's port and sequence number seed. The time of
    // start and the MAC differ between boots and machines; the entropy
    // service of ADR-0227 is not there yet.
    let now = clock_monotonic();
    let seed =
        now ^ u64::from_le_bytes([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], 0x5a, 0x50]);
    let Ok(stack) = Stack::new(nic, mac, config, (now / 1000) as i64, seed) else {
        println!("netd: the driver's address is not a unicast address");
        return 1;
    };
    let mut netd = Netd {
        port,
        stack,
        service: Service::new(),
        channels: Vec::new(),
        ethd,
        watching: false,
        request: vec![0u8; REQUEST],
    };
    // The connection `init` holds; it makes the launchers' with `narrow`.
    let root = slot(ROOT);
    if handle_type(root) == Ok("channel") {
        match netd.service.add(Connection::root()) {
            Some(id) => netd.attach_connection(id, root),
            None => return 1,
        }
    }
    let mut delay = netd.turn();
    let mut packets = [Packet::default(); 8];
    loop {
        let deadline = match delay {
            Some(us) => clock_monotonic() + (us.max(0) as u64) * 1000,
            None => DEADLINE_INFINITE,
        };
        let count = match port_wait(port, deadline, &mut packets) {
            Ok(n) => n,
            Err(Status::DeadlineExceeded) => 0,
            Err(_) => process_exit(2),
        };
        for p in &packets[..count] {
            match p.key {
                // Read in `turn`, with the timers.
                KEY_ETHD | KEY_WRITABLE => {}
                k if k >= KEY_CONNECTION => netd.serve((k - KEY_CONNECTION) as usize),
                _ => {}
            }
        }
        delay = netd.turn();
    }
}

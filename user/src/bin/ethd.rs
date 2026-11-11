#![no_std]
#![no_main]

//! The virtio-net driver (ADR-0221 item 1) as a process. Slot 5 is its
//! device page, slot 6 the channel to its one client, the network service;
//! its first argument is the range. It speaks `zoraya-proto-nic`. It treats
//! everything the device writes and every message from the client as
//! hostile (INV-0221-2); the logic that does so is `zoraya-virtio-net`, on
//! the host under test. It parses nothing above the virtio header.
//!
//! Buffers in flight: the queue memory and the client's frames object are
//! pinned (`dma_pin`) and stay pinned until the device is reset. If this
//! process ends with them in flight, the kernel keeps their frames
//! quarantined (ADR-0054). The kernel resets the device and releases the
//! frames in `dma_release_quarantine`, which is the first thing the next
//! `ethd` does, before it maps the device (ADR-0070).

use zoraya_abi::{
    DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, Mode, Packet, Status, channel_signals,
    interrupt_signals,
};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, device_interrupt, device_map, dma_pin,
    dma_release_quarantine, dma_unpin, handle_close, interrupt_ack, memory_create, memory_map,
    port_create, port_subscribe, port_unsubscribe, port_wait, println, startup,
};
use zoraya_virtio::{Registers, Shared};
use zoraya_virtio_net::serve::{Flow, Host, Read, Server, Write};
use zoraya_virtio_net::{Net, OBJECT_SIZE, SHARED_MEMORY};

extern crate alloc;

const KEY_DEVICE: u64 = 1;
const KEY_CLIENT: u64 = 2;
const KEY_WRITABLE: u64 = 3;

struct Window(u64);

impl Registers for Window {
    fn read(&self, offset: usize) -> u32 {
        // SAFETY: a device mapping of this process, and `offset` a register.
        unsafe { ((self.0 + offset as u64) as *const u32).read_volatile() }
    }

    fn write(&mut self, offset: usize, value: u32) {
        // SAFETY: as in `read`.
        unsafe { ((self.0 + offset as u64) as *mut u32).write_volatile(value) }
    }

    // The configuration has byte fields: one byte access each (virtio 1.2,
    // 4.2.2.2).
    fn read8(&self, offset: usize) -> u8 {
        // SAFETY: as in `read`; a byte access needs no alignment.
        unsafe { ((self.0 + offset as u64) as *const u8).read_volatile() }
    }

    fn write8(&mut self, offset: usize, value: u8) {
        // SAFETY: as in `read8`.
        unsafe { ((self.0 + offset as u64) as *mut u8).write_volatile(value) }
    }
}

struct Queues(u64);

macro_rules! access {
    ($read:ident, $write:ident, $type:ty) => {
        fn $read(&self, offset: usize) -> $type {
            // SAFETY: inside the mapped queue memory, which the driver's
            // layout keeps aligned and within `SHARED_MEMORY`.
            unsafe { ((self.0 + offset as u64) as *const $type).read_volatile() }
        }

        fn $write(&mut self, offset: usize, value: $type) {
            // SAFETY: as in the read.
            unsafe { ((self.0 + offset as u64) as *mut $type).write_volatile(value) }
        }
    };
}

impl Shared for Queues {
    access!(read8, write8, u8);
    access!(read16, write16, u16);
    access!(read32, write32, u32);

    fn write64(&mut self, offset: usize, value: u64) {
        // SAFETY: as in the reads.
        unsafe { ((self.0 + offset as u64) as *mut u64).write_volatile(value) }
    }

    fn publish(&self) {
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }

    fn acquire(&self) {
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
}

fn slot(n: u64) -> Handle {
    Handle::new(n).expect("slot is not zero")
}

/// The channel, the pin and the clock of the driver (`serve::Host`).
struct Chan {
    device: Handle,
    client: Handle,
    /// The pin of the client's frames object.
    frames_pin: Option<Handle>,
    handles: [Handle; 1],
}

impl Host for Chan {
    type Handle = Handle;

    fn write(&mut self, bytes: &[u8]) -> Write {
        match channel_write(self.client, bytes, &[]) {
            Ok(()) => Write::Done,
            Err(Status::WouldBlock) => Write::Full,
            Err(_) => Write::Closed,
        }
    }

    fn read(&mut self, buffer: &mut [u8]) -> Read<Handle> {
        match channel_read(self.client, buffer, &mut self.handles) {
            Ok(sizes) => Read::Message {
                length: sizes.bytes as usize,
                handle: (sizes.handles > 0).then_some(self.handles[0]),
            },
            Err((Status::WouldBlock, _)) => Read::Empty,
            Err(_) => Read::Closed,
        }
    }

    fn close(&mut self, handle: Handle) {
        let _ = handle_close(handle);
    }

    fn pin(&mut self, memory: Handle) -> Option<u64> {
        // One run of device addresses, all of the object: the slot
        // arithmetic of `Net` needs it.
        let mut runs = [[0u64; 2]; 2];
        let pin = dma_pin(memory, self.device, &mut runs).ok()?;
        if runs[0][1] < OBJECT_SIZE || runs[1][1] != 0 {
            let _ = dma_unpin(pin);
            return None;
        }
        self.frames_pin = Some(pin);
        Some(runs[0][0])
    }

    fn unpin(&mut self) {
        if let Some(pin) = self.frames_pin.take() {
            let _ = dma_unpin(pin);
        }
    }

    fn now(&mut self) -> u64 {
        clock_monotonic()
    }
}

struct Ethd {
    server: Server<Window, Queues, Chan>,
    interrupt: Handle,
    /// The pin of the queue memory.
    queue_pin: Handle,
    port: Handle,
    /// Whether the port watches the channel for `WRITABLE`.
    watching: bool,
}

impl Ethd {
    /// Stops the device; frees the pins only once it is stopped. Without
    /// that, the pins stay and the kernel quarantines the frames (ADR-0054).
    /// Exit code 0: the device was reset and both pins released; 3: the
    /// device did not reset, so they stay (`t_net` tells the two apart).
    fn stop(&mut self) -> ! {
        if self.server.net.reset() {
            self.server.host.unpin();
            let _ = dma_unpin(self.queue_pin);
            zoraya_user::process_exit(0)
        }
        zoraya_user::process_exit(3)
    }

    fn follow(&mut self, flow: Flow) {
        match flow {
            Flow::Go => {}
            Flow::Stop => self.stop(),
            Flow::Broken => {
                println!("ethd: the device broke the protocol");
                self.stop();
            }
        }
        // What could not be written waits for the channel to take messages.
        let pending = self.server.pending();
        if pending && !self.watching {
            let _ = port_subscribe(
                self.port,
                self.server.host.client,
                channel_signals::WRITABLE,
                KEY_WRITABLE,
                Mode::Level,
            );
            self.watching = true;
        } else if !pending && self.watching {
            let _ = port_unsubscribe(self.port, KEY_WRITABLE);
            self.watching = false;
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let device = slot(5);
    let client = slot(6);
    // INV-0221-1: no connection to any program but `netd`, whose channel is
    // slot 6. The start-up table is looked at and the test reads the line.
    let channels = zoraya_user::start_table()
        .iter()
        .find(|(kind, _)| *kind == "channel")
        .map_or(0, |(_, n)| *n);
    if channels != 1 {
        println!("ethd: holds {channels} channels, not one");
        return 1;
    }
    println!("ethd: holds one channel, to the network service");
    let index: u32 = startup()
        .argv
        .next()
        .and_then(|a| core::str::from_utf8(a).ok())
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    // What a dead driver left: the device is reset first, then its pinned
    // frames are released (ADR-0054).
    let _ = dma_release_quarantine(device);
    let (window, offset) = device_map(device, index).expect("map the device");
    let size = SHARED_MEMORY as u64;
    let queues = memory_create(size, MemoryFlags::CONTIGUOUS).expect("queue memory");
    let at = memory_map(queues, 0, size, MapFlags::WRITE).expect("map the queues");
    let mut runs = [[0; 2]; 1];
    let queue_pin = dma_pin(queues, device, &mut runs).expect("pin the queues");
    let interrupt = device_interrupt(device, index).expect("interrupt");
    let port = port_create().expect("port");
    port_subscribe(
        port,
        interrupt,
        interrupt_signals::INTERRUPT,
        KEY_DEVICE,
        Mode::Level,
    )
    .expect("subscribe");
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, client, wanted, KEY_CLIENT, Mode::Level).expect("subscribe");
    let net = match Net::new(Window(window + offset), Queues(at), runs[0][0]) {
        Ok(net) => net,
        Err(e) => {
            println!("ethd: {e:?}");
            return 1;
        }
    };
    let mac = net.mac();
    println!(
        "ethd: virtio-net {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
    let chan = Chan {
        device,
        client,
        frames_pin: None,
        handles: [client; 1],
    };
    let mut ethd = Ethd {
        server: Server::new(net, chan),
        interrupt,
        queue_pin,
        port,
        watching: false,
    };
    let mut packets = [Packet::default(); 3];
    loop {
        let Ok(count) = port_wait(port, DEADLINE_INFINITE, &mut packets) else {
            ethd.stop();
        };
        for p in &packets[..count] {
            // Also the end of the client: its messages first, then a
            // closed channel ends the read with an error.
            let flow = match p.key {
                KEY_CLIENT => ethd.server.serve(),
                KEY_WRITABLE => ethd.server.flush(),
                _ => continue,
            };
            ethd.follow(flow);
        }
        if packets[..count].iter().any(|p| p.key == KEY_DEVICE) {
            let flow = ethd.server.device_event();
            let _ = interrupt_ack(ethd.interrupt);
            ethd.follow(flow);
        }
    }
}

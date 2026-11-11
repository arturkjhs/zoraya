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
//! quarantined (ADR-0054) until the next driver resets the device and calls
//! `dma_release_quarantine`, which is the first thing `main` does.

use alloc::vec::Vec;

use zoraya_abi::{
    DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, Mode, Packet, Status, channel_signals,
    interrupt_signals,
};
use zoraya_proto_nic::{
    Attach, Done, Frame, Info, InfoReply, LinkChanged, Received, Release, Sent, Slot, Transmit,
    status,
};
use zoraya_protocol::{Below, Data, Header, Message, NO_REPLY, Seq, decode, encode};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, device_interrupt, device_map, dma_pin,
    dma_release_quarantine, dma_unpin, handle_close, interrupt_ack, memory_create, memory_map,
    port_create, port_subscribe, port_wait, println, startup,
};
use zoraya_virtio::{Registers, Shared};
use zoraya_virtio_net::{Net, OBJECT_SIZE, SHARED_MEMORY};

extern crate alloc;

const KEY_DEVICE: u64 = 1;
const KEY_CLIENT: u64 = 2;

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

struct Ethd {
    net: Net<Window, Queues>,
    device: Handle,
    client: Handle,
    interrupt: Handle,
    /// The pin of the queue memory and of the frames object.
    queue_pin: Handle,
    frames_pin: Option<Handle>,
    out: Vec<u8>,
    message: Vec<u8>,
}

impl Ethd {
    /// Stops the device; frees the pins only once it is stopped.
    fn stop(&mut self) -> ! {
        if self.net.reset() {
            if let Some(pin) = self.frames_pin.take() {
                let _ = dma_unpin(pin);
            }
            let _ = dma_unpin(self.queue_pin);
        }
        zoraya_user::process_exit(1)
    }

    fn send<'a, M: Message<'a>>(&mut self, transaction: u32, m: &M) -> bool {
        match encode(m, transaction, &mut self.out) {
            Ok(n) => match channel_write(self.client, &self.out[..n], &[]) {
                Ok(()) => true,
                Err(Status::PeerClosed) => self.stop(),
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    fn attach(&mut self, memory: Handle) -> u32 {
        if self.frames_pin.is_some() {
            return status::BAD_OBJECT;
        }
        // One run of device addresses, all of the object: the slot
        // arithmetic of `Net` needs it.
        let mut runs = [[0u64; 2]; 2];
        let Ok(pin) = dma_pin(memory, self.device, &mut runs) else {
            return status::BAD_OBJECT;
        };
        if runs[0][1] < OBJECT_SIZE || runs[1][1] != 0 || self.net.attach(runs[0][0]).is_err() {
            let _ = dma_unpin(pin);
            return status::BAD_OBJECT;
        }
        self.frames_pin = Some(pin);
        status::OK
    }

    fn serve(&mut self) {
        let mut handles = [self.client; 1];
        loop {
            let mut message = core::mem::take(&mut self.message);
            let (length, received) = match channel_read(self.client, &mut message, &mut handles) {
                Ok(sizes) => (sizes.bytes as usize, sizes.handles > 0),
                Err((Status::WouldBlock, _)) => {
                    self.message = message;
                    return;
                }
                Err(_) => self.stop(),
            };
            let bytes = &message[..length.min(message.len())];
            self.handle(bytes, received.then_some(handles[0]));
            self.message = message;
        }
    }

    fn handle(&mut self, bytes: &[u8], memory: Option<Handle>) {
        let Ok((header, _)) = Header::decode(bytes) else {
            self.stop();
        };
        let t = header.transaction;
        // Only `Attach` takes a handle; any other is closed, not kept.
        if let Some(h) = memory
            && header.ordinal != 2
        {
            let _ = handle_close(h);
        }
        match header.ordinal {
            1 if decode::<Info>(bytes, 0).is_ok() => {
                let mac = self.net.mac();
                let m = InfoReply {
                    status: Below(status::OK),
                    link: self.net.link(),
                    mac: Data(&mac),
                };
                self.send(t, &m);
            }
            2 => {
                let s = match memory {
                    Some(h) if decode::<Attach>(bytes, 1).is_ok() => {
                        let s = self.attach(h);
                        let _ = handle_close(h);
                        s
                    }
                    Some(h) => {
                        let _ = handle_close(h);
                        status::BAD_OBJECT
                    }
                    None => status::BAD_OBJECT,
                };
                self.send(t, &Done { status: Below(s) });
            }
            3 => {
                if let Ok((_, m)) = decode::<Transmit>(bytes, 0) {
                    let mut refused: Vec<Slot> = Vec::new();
                    for f in m.frames.iter() {
                        if self.net.transmit(f.slot, f.length).is_err() {
                            refused.push(Slot { slot: f.slot });
                        }
                    }
                    // A refused frame is reported as sent at once, so the
                    // service's slot accounting never stalls.
                    if !refused.is_empty() {
                        for chunk in refused.chunks(zoraya_proto_nic::MAX_BATCH) {
                            self.send(
                                NO_REPLY,
                                &Sent {
                                    slots: Seq::Of(chunk),
                                },
                            );
                        }
                    }
                } else {
                    self.stop();
                }
            }
            4 => {
                if let Ok((_, m)) = decode::<Release>(bytes, 0) {
                    for s in m.slots.iter() {
                        let _ = self.net.release(s.slot);
                    }
                } else {
                    self.stop();
                }
            }
            _ => self.stop(),
        }
    }

    fn device_event(&mut self) {
        let woke = clock_monotonic();
        self.net.acknowledge();
        let mut frames: Vec<Frame> = Vec::new();
        let mut slots: Vec<Slot> = Vec::new();
        let polled = self.net.poll(
            &mut |slot, length| {
                frames.push(Frame { slot, length });
                true
            },
            &mut |slot| slots.push(Slot { slot }),
        );
        if polled.is_err() {
            println!("ethd: the device broke the protocol");
            self.stop();
        }
        // The slots are the client's from here; if a message is lost, they
        // are given back so that the device does not run out of buffers.
        for chunk in frames.chunks(zoraya_proto_nic::MAX_BATCH) {
            let m = Received {
                at: woke,
                frames: Seq::Of(chunk),
            };
            if !self.send(NO_REPLY, &m) {
                for f in chunk {
                    let _ = self.net.release(f.slot);
                }
            }
        }
        for chunk in slots.chunks(zoraya_proto_nic::MAX_BATCH) {
            self.send(
                NO_REPLY,
                &Sent {
                    slots: Seq::Of(chunk),
                },
            );
        }
        if let Some(link) = self.net.link_changed() {
            self.send(NO_REPLY, &LinkChanged { link });
        }
        let _ = interrupt_ack(self.interrupt);
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let device = slot(5);
    let client = slot(6);
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
    let mut ethd = Ethd {
        net,
        device,
        client,
        interrupt,
        queue_pin,
        frames_pin: None,
        out: alloc::vec![0; 2048],
        message: alloc::vec![0; 2048],
    };
    let mut packets = [Packet::default(); 2];
    loop {
        let Ok(count) = port_wait(port, DEADLINE_INFINITE, &mut packets) else {
            ethd.stop();
        };
        for p in &packets[..count] {
            if p.key == KEY_CLIENT {
                // Also the end of the client: its messages first, then a
                // closed channel ends the read with an error.
                ethd.serve();
            }
        }
        if packets[..count].iter().any(|p| p.key == KEY_DEVICE) {
            ethd.device_event();
        }
    }
}

#![no_std]
#![no_main]

//! A virtio-input driver (ADR-0092) as a process: a keyboard or a tablet.
//! Slot 5 is its device page, slot 6 the channel to the compositor; its
//! first argument is the range. It never waits for the compositor: a frame
//! that does not fit is dropped and counted.

use alloc::vec::Vec;

use zoraya_abi::{
    DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, Mode, Packet, Status, channel_signals,
    interrupt_signals,
};
use zoraya_proto_input::{Event, Frame};
use zoraya_protocol::{Below, NO_REPLY, Seq, encode};
use zoraya_user::{
    channel_write, device_interrupt, device_map, dma_pin, dma_release_quarantine, dma_unpin,
    interrupt_ack, memory_create, memory_map, port_create, port_subscribe, port_wait, println,
    startup,
};
use zoraya_virtio::{Registers, Shared};
use zoraya_virtio_input::{Input, Kind, MAX_FRAME, Record, SHARED_MEMORY};

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

fn stop(input: &mut Input<Window, Queues>, pin: Handle) -> ! {
    if input.reset() {
        let _ = dma_unpin(pin);
    }
    zoraya_user::process_exit(1)
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
    let _ = dma_release_quarantine(device);
    let (window, offset) = device_map(device, index).expect("map the device");
    let size = SHARED_MEMORY as u64;
    let queues = memory_create(size, MemoryFlags::CONTIGUOUS).expect("queue memory");
    let at = memory_map(queues, 0, size, MapFlags::WRITE).expect("map the queues");
    let mut runs = [[0; 2]; 1];
    let pin = dma_pin(queues, device, &mut runs).expect("pin the queues");
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
    // Only its end: this driver reads nothing from the compositor.
    port_subscribe(
        port,
        client,
        channel_signals::PEER_CLOSED,
        KEY_CLIENT,
        Mode::Level,
    )
    .expect("subscribe");
    let mut input = match Input::new(Window(window + offset), Queues(at), runs[0][0]) {
        Ok(input) => input,
        Err(e) => {
            println!("inputd: {e:?}");
            return 1;
        }
    };
    match input.kind() {
        Kind::Keyboard => println!("inputd: keyboard"),
        Kind::Pointer { .. } => println!("inputd: pointer"),
    }
    let mut out = alloc::vec![0u8; 2048];
    let mut events: Vec<Event> = Vec::with_capacity(MAX_FRAME);
    let mut packets = [Packet::default(); 2];
    loop {
        let Ok(count) = port_wait(port, DEADLINE_INFINITE, &mut packets) else {
            stop(&mut input, pin);
        };
        if packets[..count].iter().any(|p| p.key == KEY_CLIENT) {
            // The compositor is gone: stop the device and end.
            stop(&mut input, pin);
        }
        input.acknowledge();
        let mut gone = false;
        let mut send = |records: &[Record], dropped: u32| -> bool {
            events.clear();
            events.extend(records.iter().map(|r| Event {
                kind: Below(r.kind),
                code: r.code,
                value: r.value,
            }));
            let m = Frame {
                dropped,
                events: Seq::Of(&events),
            };
            let Ok(n) = encode(&m, NO_REPLY, &mut out) else {
                return false;
            };
            match channel_write(client, &out[..n], &[]) {
                Ok(()) => true,
                Err(Status::PeerClosed) => {
                    gone = true;
                    false
                }
                Err(_) => false,
            }
        };
        if input.poll(&mut send).is_err() {
            println!("inputd: the device broke the protocol");
            stop(&mut input, pin);
        }
        if gone {
            stop(&mut input, pin);
        }
        let _ = interrupt_ack(interrupt);
    }
}

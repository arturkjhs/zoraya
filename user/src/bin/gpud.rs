#![no_std]
#![no_main]

//! The virtio-gpu driver (ADR-0090) as a process. Slot 5 is its device page,
//! slot 6 the channel to its one client, the compositor; its first argument
//! is the range. Every message from the compositor and everything the device
//! writes is treated as hostile.

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use zoraya_abi::{
    Handle, MapFlags, MemoryFlags, Mode, Packet, Status, channel_signals, interrupt_signals,
};
use zoraya_proto_display::{Attach, Done, FORMAT_XRGB, Flush, Info, InfoReply, Resized, status};
use zoraya_protocol::{Below, ErrorReply, Header, Message, NO_REPLY, decode, encode};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, device_interrupt, device_map, dma_pin,
    dma_release_quarantine, dma_unpin, handle_close, handle_type, interrupt_ack, memory_create,
    memory_map, port_create, port_subscribe, port_wait, println, startup,
};
use zoraya_virtio::{Registers, Shared};
use zoraya_virtio_gpu::{Announce, Error, Gpu, MAX_RUNS, Rect, SHARED_MEMORY};

extern crate alloc;

/// How long one command may take before the device counts as lost.
const COMMAND_NS: u64 = 1_000_000_000;
/// How often a wait looks at the queue without an interrupt.
const POLL_NS: u64 = 10_000_000;
/// How often the driver looks for a display event between requests
/// (ADR-0246): one register read.
const DISPLAY_POLL_NS: u64 = 100_000_000;
/// Reported once: a wait passed 10 ms with no interrupt.
static MISSED: AtomicBool = AtomicBool::new(false);

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

struct Driver {
    gpu: Gpu<Window, Queues>,
    client: Handle,
    device: Handle,
    interrupt: Handle,
    port: Handle,
    pin: Option<Handle>,
    reply: [u8; 64],
    /// When a new display size may be told (ADR-0246).
    announce: Announce,
}

impl Driver {
    /// Runs a command, waiting for the device's interrupt with a deadline.
    fn run(
        &mut self,
        f: impl FnOnce(
            &mut Gpu<Window, Queues>,
            &mut dyn FnMut() -> Result<(), Error>,
        ) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let (port, interrupt) = (self.port, self.interrupt);
        // Wait, then unmask: the driver took the device's interrupt down
        // before, so the line is low and the next completion is a new edge.
        // The driver takes the device's interrupt down and looks at the
        // queue again before every wait (TASK-14 review, R1). As a guard,
        // the wait also looks every 10 ms, and gives up after a second.
        let mut polls = 0;
        let mut wait = || {
            let mut packets = [Packet::default()];
            match port_wait(port, clock_monotonic() + POLL_NS, &mut packets) {
                Ok(_) => interrupt_ack(interrupt).map_err(|_| Error::Device),
                Err(Status::DeadlineExceeded) if polls < COMMAND_NS / POLL_NS => {
                    polls += 1;
                    if !MISSED.swap(true, Ordering::Relaxed) {
                        println!("gpud: a wait passed 10 ms: looking at the queue");
                    }
                    Ok(())
                }
                Err(e) => {
                    println!("gpud: no answer from the device in time: {e:?}");
                    Err(Error::Device)
                }
            }
        };
        f(&mut self.gpu, &mut wait)
    }

    /// The device broke the protocol or went silent: stop it and end. The
    /// compositor sees the channel close.
    fn lost(&mut self) -> ! {
        println!("gpud: the device broke the protocol");
        self.stop()
    }

    fn stop(&mut self) -> ! {
        if self.gpu.reset()
            && let Some(pin) = self.pin.take()
        {
            let _ = dma_unpin(pin);
        }
        zoraya_user::process_exit(1)
    }

    fn status(&mut self, e: Error) -> u32 {
        match e {
            Error::Device | Error::Transport(_) => self.lost(),
            Error::BadRange => status::BAD_RANGE,
            Error::NotAttached => status::NOT_ATTACHED,
            Error::BadBacking | Error::Attached => status::BAD_BUFFER,
            Error::NoDisplay | Error::Command(_) => status::DEVICE,
        }
    }

    /// Attaches `memory`: the first time, or again at a new display size
    /// (ADR-0246), when the old framebuffer is unpinned once the new one is
    /// shown. The same size twice is `BAD_BUFFER`, as before.
    fn attach(&mut self, memory: Handle) -> u32 {
        if handle_type(memory) != Ok("memory") {
            return status::BAD_BUFFER;
        }
        let mut raw: Vec<[u64; 2]> = vec![[0; 2]; MAX_RUNS];
        let Ok(pin) = dma_pin(memory, self.device, &mut raw) else {
            return status::BAD_BUFFER;
        };
        let runs: Vec<(u64, u64)> = raw
            .iter()
            .take_while(|r| r[1] != 0)
            .map(|r| (r[0], r[1]))
            .collect();
        match self.run(|gpu, wait| gpu.attach(&runs, wait)) {
            Ok(()) => {
                if let Some(old) = self.pin.replace(pin) {
                    // The device unreferenced the old resource: it no
                    // longer reads that memory.
                    let _ = dma_unpin(old);
                }
                status::OK
            }
            // Refused before any command: nothing reached the device.
            Err(e @ (Error::BadBacking | Error::Attached | Error::NoDisplay)) => {
                let _ = dma_unpin(pin);
                self.status(e)
            }
            // A command failed part-way: the device may hold the backing,
            // and a resource exists. Stop the device first (review R4); an
            // older pin, if any, stays quarantined by the kernel with the
            // device (ADR-0070), never unpinned while the device may read it.
            Err(_) => {
                self.pin = Some(pin);
                self.lost()
            }
        }
    }

    /// After a display event, tells the compositor the new size (ADR-0246),
    /// when [`Announce`] allows: never between an `Info` and the request
    /// after it (the size is not even asked then, so `Attach` uses the size
    /// `Info` gave), and one `Resized` before each `Info`.
    fn display_event(&mut self) {
        if !self.announce.may_ask() {
            return;
        }
        if self.gpu.display_changed() {
            match self.run(|gpu, wait| gpu.display_info(wait).map(|_| ())) {
                Ok(()) => {}
                // Scanout 0 off or too large: the display keeps its size.
                Err(Error::NoDisplay | Error::Command(_)) => return,
                Err(_) => self.lost(),
            }
        }
        let size = self.gpu.size();
        if self.pin.is_none() || !self.announce.tell(size) {
            return;
        }
        let m = Resized {
            width: size.0,
            height: size.1,
        };
        let mut out = [0u8; 64];
        match encode(&m, NO_REPLY, &mut out) {
            Ok(n) => match channel_write(self.client, &out[..n], &[]) {
                Ok(()) => println!("gpud: display {}x{}", size.0, size.1),
                // A full queue: the next look tries again.
                Err(Status::WouldBlock) => self.announce.unsent(),
                Err(_) => self.stop(),
            },
            Err(_) => self.stop(),
        }
    }

    fn respond<'a, M: Message<'a>>(&mut self, transaction: u32, m: &M) {
        let mut reply = self.reply;
        match encode(m, transaction, &mut reply) {
            Ok(n) if channel_write(self.client, &reply[..n], &[]).is_ok() => {}
            _ => self.stop(),
        }
    }

    fn done(&mut self, transaction: u32, s: u32) {
        self.respond(transaction, &Done { status: Below(s) });
    }

    fn serve(&mut self, message: &mut [u8]) {
        let mut handles = [self.client; 1];
        let (length, received) = match channel_read(self.client, message, &mut handles) {
            Ok(sizes) => (sizes.bytes as usize, sizes.handles as usize),
            Err((Status::WouldBlock, _)) => return,
            Err(_) => self.stop(),
        };
        let bytes = &message[..length];
        let Ok((header, _)) = Header::decode(bytes) else {
            self.stop();
        };
        let t = header.transaction;
        // Only `Attach` takes a handle; any other is closed, not kept.
        if received == 1 && header.ordinal != 2 {
            let _ = handle_close(handles[0]);
        }
        match header.ordinal {
            1 if decode::<Info>(bytes, received).is_ok() => {
                let (width, height) = self.gpu.size();
                let m = InfoReply {
                    status: Below(status::OK),
                    width,
                    height,
                    format: FORMAT_XRGB,
                };
                self.respond(t, &m);
                self.announce.info((width, height));
                return;
            }
            2 if decode::<Attach>(bytes, received).is_ok() => {
                let memory = handles[0];
                let s = self.attach(memory);
                let _ = handle_close(memory);
                self.done(t, s);
                let attached = (s == status::OK).then(|| self.gpu.size());
                self.announce.served(attached);
                return;
            }
            3 => match decode::<Flush>(bytes, received) {
                Ok((_, f)) => {
                    let rect = Rect {
                        x: f.x,
                        y: f.y,
                        width: f.width,
                        height: f.height,
                    };
                    let s = match self.run(|gpu, wait| gpu.flush(rect, wait)) {
                        Ok(()) => status::OK,
                        Err(e) => self.status(e),
                    };
                    self.done(t, s);
                }
                Err(_) => self.done(t, status::BAD_RANGE),
            },
            2 => {
                if received == 1 {
                    let _ = handle_close(handles[0]);
                }
                self.done(t, status::BAD_BUFFER);
            }
            _ => {
                self.respond(
                    t,
                    &ErrorReply {
                        status: status::UNSUPPORTED,
                    },
                );
            }
        }
        // Any other request ends the window after an `Info`.
        self.announce.served(None);
    }
}

fn slot(n: u64) -> Handle {
    Handle::new(n).expect("slot is not zero")
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
    // What a dead driver left: the kernel resets the device first (ADR-0070).
    let _ = dma_release_quarantine(device);
    let (window, offset) = device_map(device, index).expect("map the device");
    let size = SHARED_MEMORY as u64;
    let queues = memory_create(size, MemoryFlags::CONTIGUOUS).expect("queue memory");
    let at = memory_map(queues, 0, size, MapFlags::WRITE).expect("map the queues");
    let mut runs = [[0; 2]; 1];
    dma_pin(queues, device, &mut runs).expect("pin the queues");
    let interrupt = device_interrupt(device, index).expect("interrupt");
    let port = port_create().expect("port");
    port_subscribe(
        port,
        interrupt,
        interrupt_signals::INTERRUPT,
        1,
        Mode::Level,
    )
    .expect("subscribe");
    let requests = port_create().expect("port");
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(requests, client, wanted, 1, Mode::Level).expect("subscribe");
    let gpu = match Gpu::new(Window(window + offset), Queues(at), runs[0][0]) {
        Ok(gpu) => gpu,
        Err(e) => {
            println!("gpud: {e:?}");
            return 1;
        }
    };
    let mut driver = Driver {
        gpu,
        client,
        device,
        interrupt,
        port,
        pin: None,
        reply: [0; 64],
        announce: Announce::default(),
    };
    match driver.run(|gpu, wait| gpu.display_info(wait).map(|_| ())) {
        Ok(()) => {
            let (w, h) = driver.gpu.size();
            println!("gpud: display {w}x{h}");
        }
        Err(e) => {
            println!("gpud: no display: {e:?}");
            driver.stop();
        }
    }
    let mut message = vec![0u8; 256];
    let mut packets = [Packet::default()];
    loop {
        match port_wait(requests, clock_monotonic() + DISPLAY_POLL_NS, &mut packets) {
            Ok(_) => driver.serve(&mut message),
            Err(Status::DeadlineExceeded) => {}
            Err(_) => driver.stop(),
        }
        driver.display_event();
    }
}

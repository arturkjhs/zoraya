#![no_std]
#![no_main]

//! The virtio-gpu driver (ADR-0090) as a process. Slot 5 is its device page,
//! slot 6 the channel to its one client, the compositor; its first argument
//! is the range. Every message from the compositor and everything the device
//! writes is treated as hostile.

use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::{
    DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, Mode, Packet, Status, channel_signals,
    interrupt_signals,
};
use zoraya_proto_display::{Attach, Done, FORMAT_XRGB, Flush, Info, InfoReply, status};
use zoraya_protocol::{Below, ErrorReply, Header, Message, decode, encode};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, device_interrupt, device_map, dma_pin,
    dma_release_quarantine, dma_unpin, handle_close, handle_type, interrupt_ack, memory_create,
    memory_map, port_create, port_subscribe, port_wait, println, startup,
};
use zoraya_virtio::{Registers, Shared};
use zoraya_virtio_gpu::{Error, Gpu, MAX_RUNS, Rect, SHARED_MEMORY};

extern crate alloc;

/// How long one command may take before the device counts as lost.
const COMMAND_NS: u64 = 1_000_000_000;

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
        let mut wait = || {
            let mut packets = [Packet::default()];
            match port_wait(port, clock_monotonic() + COMMAND_NS, &mut packets) {
                Ok(_) => interrupt_ack(interrupt).map_err(|_| Error::Device),
                Err(_) => Err(Error::Device),
            }
        };
        let result = f(&mut self.gpu, &mut wait);
        self.gpu.acknowledge();
        result
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

    fn attach(&mut self, memory: Handle) -> u32 {
        if self.pin.is_some() {
            return status::BAD_BUFFER;
        }
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
                self.pin = Some(pin);
                status::OK
            }
            Err(e) => {
                // The device holds no reference to a backing it refused.
                let _ = dma_unpin(pin);
                self.status(e)
            }
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
            }
            2 if decode::<Attach>(bytes, received).is_ok() => {
                let memory = handles[0];
                let s = self.attach(memory);
                let _ = handle_close(memory);
                self.done(t, s);
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
        if port_wait(requests, DEADLINE_INFINITE, &mut packets).is_err() {
            driver.stop();
        }
        driver.serve(&mut message);
    }
}

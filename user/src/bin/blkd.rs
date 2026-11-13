#![no_std]
#![no_main]

//! The virtio-blk driver (ADR-0054, ADR-0015) as a process. Slot 5 is its
//! device, slot 6 the channel to its one client, the filesystem service. Its
//! first argument is the range of the device. It treats every message from
//! the service, and everything the device writes, as hostile.

use alloc::vec::Vec;
use core::sync::atomic::{Ordering, fence};

use zoraya_abi::{
    DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, Mode, Packet, channel_signals,
    interrupt_signals,
};
use zoraya_proto_block::{Attach, Done, Flush, Info, InfoReply, Range, Read, Write, status};
use zoraya_protocol::{Below, ErrorReply, Header, Message, decode, encode};
use zoraya_user::mmio::Window;
use zoraya_user::{
    channel_read, channel_write, device_interrupt, dma_pin, dma_release_quarantine, dma_unpin,
    handle_close, interrupt_ack, memory_create, memory_map, port_create, port_subscribe, port_wait,
    println, startup,
};
use zoraya_virtio::{
    Driver, Error, Kind, MAX_IN_FLIGHT, MAX_PIECES, Piece, QUEUE_MEMORY, Request, Shared, place,
};

extern crate alloc;

const CLIENT_KEY: u64 = 1;
const MAX_RUNS: usize = 512;

struct Queues(u64);

macro_rules! access {
    ($read:ident, $write:ident, $type:ty) => {
        fn $read(&self, offset: usize) -> $type {
            // SAFETY: inside the mapped queue memory, aligned by the layout.
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
        fence(Ordering::SeqCst);
    }

    fn acquire(&self) {
        fence(Ordering::SeqCst);
    }
}

struct Attached {
    pin: Handle,
    runs: Vec<(u64, u64)>,
}

struct Blk {
    driver: Driver<Window, Queues>,
    client: Handle,
    device: Handle,
    interrupt: Handle,
    port: Handle,
    attached: Option<Attached>,
    message: Vec<u8>,
    reply: Vec<u8>,
}

impl Blk {
    /// Waits until nothing is outstanding. The device's status decides.
    fn finish(&mut self) -> Result<(), Error> {
        self.driver.notify();
        loop {
            if self.driver.collect()? == 0 {
                return Ok(());
            }
            let mut packets = [Packet::default()];
            port_wait(self.port, DEADLINE_INFINITE, &mut packets).map_err(|_| Error::Device)?;
            self.driver.acknowledge();
            interrupt_ack(self.interrupt).map_err(|_| Error::Device)?;
        }
    }

    fn check(&self, range: &Range, out: &mut [Piece]) -> Option<usize> {
        let attached = self.attached.as_ref()?;
        let sectors = self.driver.geometry().sectors;
        place(
            &attached.runs,
            sectors,
            range.sector,
            range.count,
            range.offset,
            out,
        )
    }

    fn transfer(&mut self, kind: Kind, ranges: &mut dyn Iterator<Item = Range>) -> u32 {
        let mut piece = [Piece {
            address: 0,
            length: 0,
        }; MAX_PIECES];
        // All of them are checked before any is queued: a refused batch
        // writes nothing.
        let ranges: Vec<Range> = ranges.collect();
        if ranges.iter().any(|r| self.check(r, &mut piece).is_none()) {
            return status::BAD_RANGE;
        }
        let mut queued = 0;
        for range in ranges {
            let Some(count) = self.check(&range, &mut piece) else {
                return status::BAD_RANGE;
            };
            let request = Request {
                kind,
                sector: range.sector,
                pieces: &piece[..count],
            };
            // Checked above, and a slot is free: this cannot fail, and if it
            // did, what is queued would go out with a later request.
            if self.driver.queue(&request).is_err() {
                return self.lost();
            }
            queued += 1;
            if queued == MAX_IN_FLIGHT {
                if self.finish().is_err() {
                    return self.lost();
                }
                queued = 0;
            }
        }
        if self.finish().is_err() {
            return self.lost();
        }
        if self.driver.take_failure() {
            status::FAILED
        } else {
            status::OK
        }
    }

    /// The device broke the protocol: stop it and end. The service sees the
    /// channel close, which is ADR-0054's device loss.
    fn lost(&mut self) -> u32 {
        println!("blkd: the device broke the protocol");
        self.stop()
    }

    fn stop(&mut self) -> ! {
        let stopped = self.driver.reset();
        if stopped && let Some(attached) = self.attached.take() {
            let _ = dma_unpin(attached.pin);
        }
        zoraya_user::process_exit(1)
    }

    fn attach(&mut self, memory: Handle) -> u32 {
        if self.attached.is_some() {
            return status::BAD_RANGE;
        }
        let mut raw = [[0u64; 2]; MAX_RUNS];
        let Ok(pin) = dma_pin(memory, self.device, &mut raw) else {
            return status::BAD_RANGE;
        };
        let runs: Vec<(u64, u64)> = raw
            .iter()
            .take_while(|r| r[1] != 0)
            .map(|r| (r[0], r[1]))
            .collect();
        self.attached = Some(Attached { pin, runs });
        status::OK
    }

    fn respond<'a, M: Message<'a>>(&mut self, transaction: u32, m: &M) {
        match encode(m, transaction, &mut self.reply) {
            Ok(length) if channel_write(self.client, &self.reply[..length], &[]).is_ok() => {}
            _ => self.stop(),
        }
    }

    fn done(&mut self, transaction: u32, s: u32) {
        let m = Done { status: Below(s) };
        self.respond(transaction, &m);
    }

    fn serve(&mut self) {
        let mut handles = [self.client; 1];
        let mut message = core::mem::take(&mut self.message);
        let (length, received) = match channel_read(self.client, &mut message, &mut handles) {
            Ok(sizes) => (sizes.bytes as usize, sizes.handles > 0),
            Err((zoraya_abi::Status::WouldBlock, _)) => {
                self.message = message;
                return;
            }
            Err(_) => self.stop(),
        };
        let bytes = &message[..length];
        let Ok((header, _)) = Header::decode(bytes) else {
            self.stop();
        };
        let t = header.transaction;
        let refused = status::BAD_RANGE;
        // Only `Attach` takes a handle; any other is closed, not kept.
        if received && header.ordinal != 2 {
            let _ = handle_close(handles[0]);
        }
        match header.ordinal {
            1 if decode::<Info>(bytes, 0).is_ok() => {
                let g = self.driver.geometry();
                let m = InfoReply {
                    status: Below(status::OK),
                    sectors: g.sectors,
                    flush: g.flush,
                };
                self.respond(t, &m);
            }
            2 if received && decode::<Attach>(bytes, 1).is_ok() => {
                let s = self.attach(handles[0]);
                let _ = handle_close(handles[0]);
                self.done(t, s);
            }
            2 => {
                if received {
                    let _ = handle_close(handles[0]);
                }
                self.done(t, refused);
            }
            3 => match decode::<Read>(bytes, 0) {
                Ok((_, m)) => {
                    let s = self.transfer(Kind::Read, &mut core::iter::once(m.range));
                    self.done(t, s);
                }
                Err(_) => self.done(t, refused),
            },
            4 => match decode::<Write<'_>>(bytes, 0) {
                Ok((_, m)) => {
                    let s = self.transfer(Kind::Write, &mut m.ranges.iter());
                    self.done(t, s);
                }
                Err(_) => self.done(t, refused),
            },
            5 if decode::<Flush>(bytes, 0).is_ok() => {
                let request = Request {
                    kind: Kind::Flush,
                    sector: 0,
                    pieces: &[],
                };
                let s = match self.driver.queue(&request) {
                    Ok(()) if self.finish().is_ok() => {
                        if self.driver.take_failure() {
                            status::FAILED
                        } else {
                            status::OK
                        }
                    }
                    _ => self.lost(),
                };
                self.done(t, s);
            }
            _ => {
                let m = ErrorReply {
                    status: status::UNSUPPORTED,
                };
                self.respond(t, &m);
            }
        }
        self.message = message;
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
    let window = Window::open(device, index).expect("map the device");
    let size = (QUEUE_MEMORY as u64).next_multiple_of(4096);
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
    port_subscribe(requests, client, wanted, CLIENT_KEY, Mode::Level).expect("subscribe");
    let driver = match Driver::new(window, Queues(at), runs[0][0]) {
        Ok(driver) => driver,
        Err(e) => {
            println!("blkd: {e:?}");
            return 1;
        }
    };
    let mut blk = Blk {
        driver,
        client,
        device,
        interrupt,
        port,
        attached: None,
        message: alloc::vec![0; 64 * 1024],
        reply: alloc::vec![0; 256],
    };
    let mut packets = [Packet::default()];
    loop {
        if port_wait(requests, DEADLINE_INFINITE, &mut packets).is_err() {
            blk.stop();
        }
        blk.serve();
    }
}

//! A virtio-blk driver over virtio-mmio version 2 (ADR-0054, ADR-0015), as
//! logic over two traits, so that it runs on the host against a fake device.
//! The device is not trusted: everything it writes is read once and checked
//! (ADR-0015 rule 8).
//!
//! Register offsets and the split virtqueue layout are those of the virtio
//! 1.2 specification, sections 4.2.2 and 2.7. One block request is a chain of
//! three or more descriptors: a device-readable header, the data, and a
//! device-writable status byte.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

/// The transport's registers.
pub trait Registers {
    fn read(&self, offset: usize) -> u32;
    fn write(&mut self, offset: usize, value: u32);
}

/// Memory the device reaches: the queues, shared with it. Accesses are single
/// and volatile, and `publish` is a store barrier the device can see.
pub trait Shared {
    fn read8(&self, offset: usize) -> u8;
    fn read16(&self, offset: usize) -> u16;
    fn read32(&self, offset: usize) -> u32;
    fn write8(&mut self, offset: usize, value: u8);
    fn write16(&mut self, offset: usize, value: u16);
    fn write32(&mut self, offset: usize, value: u32);
    fn write64(&mut self, offset: usize, value: u64);
    /// Everything written before this is seen before anything after it.
    fn publish(&self);
    /// Reads after this see writes the device made before its interrupt.
    fn acquire(&self);
}

pub mod reg {
    pub const MAGIC: usize = 0x000;
    pub const VERSION: usize = 0x004;
    pub const DEVICE_ID: usize = 0x008;
    pub const DEVICE_FEATURES: usize = 0x010;
    pub const DEVICE_FEATURES_SEL: usize = 0x014;
    pub const DRIVER_FEATURES: usize = 0x020;
    pub const DRIVER_FEATURES_SEL: usize = 0x024;
    pub const QUEUE_SEL: usize = 0x030;
    pub const QUEUE_NUM_MAX: usize = 0x034;
    pub const QUEUE_NUM: usize = 0x038;
    pub const QUEUE_READY: usize = 0x044;
    pub const QUEUE_NOTIFY: usize = 0x050;
    pub const INTERRUPT_STATUS: usize = 0x060;
    pub const INTERRUPT_ACK: usize = 0x064;
    pub const STATUS: usize = 0x070;
    pub const QUEUE_DESC: usize = 0x080;
    pub const QUEUE_AVAIL: usize = 0x090;
    pub const QUEUE_USED: usize = 0x0a0;
    pub const CONFIG: usize = 0x100;
}

pub const MAGIC: u32 = 0x7472_6976;
pub const DEVICE_BLOCK: u32 = 2;

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_FAILED: u32 = 128;

/// Word 0 of the features.
const F_BLK_FLUSH: u32 = 1 << 9;
/// Word 1 of the features: bit 32.
const F_VERSION_1: u32 = 1;

const DESC_NEXT: u16 = 1;
const DESC_WRITE: u16 = 2;

const BLK_T_IN: u32 = 0;
const BLK_T_OUT: u32 = 1;
const BLK_T_FLUSH: u32 = 4;

pub const SECTOR_SIZE: u64 = 512;

/// Descriptors in the queue.
pub const QUEUE_SIZE: u16 = 256;
const N: usize = QUEUE_SIZE as usize;
/// Descriptors a request may use, so that a request's slot is its head index.
const SLOT: usize = 32;
/// Requests in flight.
pub const MAX_IN_FLIGHT: usize = N / SLOT;
/// Data pieces in one request.
pub const MAX_PIECES: usize = SLOT - 2;

const DESC_AT: usize = 0;
const AVAIL_AT: usize = 16 * N;
const USED_AT: usize = (AVAIL_AT + 6 + 2 * N).next_multiple_of(4096);
const HEADERS_AT: usize = USED_AT + (6 + 8 * N).next_multiple_of(4096);
/// The memory the queues need: pass a contiguous object of this many bytes.
pub const QUEUE_MEMORY: usize = HEADERS_AT + MAX_IN_FLIGHT * 32;

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not a virtio-blk device of version 2.
    NotBlockDevice,
    /// The device refused what the driver offered, or lacks version 1.
    Negotiation,
    /// The queue cannot have the size the driver needs.
    Queue,
    /// A request failed, or was unsupported.
    Io,
    /// The device broke the protocol, or did not answer. Reset it.
    Device,
    /// A request does not fit the queue.
    TooLarge,
}

/// A piece of a transfer: device address and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub address: u64,
    pub length: u32,
}

/// What a request does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Read,
    Write,
    Flush,
}

/// One request: `pieces` is empty for a flush.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub kind: Kind,
    pub sector: u64,
    pub pieces: &'a [Piece],
}

/// What the device offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub sectors: u64,
    pub flush: bool,
}

pub struct Driver<R, S> {
    registers: R,
    shared: S,
    /// The device address of `shared`.
    base: u64,
    geometry: Geometry,
    next_avail: u16,
    last_used: u16,
    in_flight: [bool; MAX_IN_FLIGHT],
    failed: bool,
    outstanding: usize,
}

impl<R: Registers, S: Shared> Driver<R, S> {
    /// Resets the device and brings it up. `base` is the device address of
    /// `shared`, which is `QUEUE_MEMORY` bytes, physically contiguous, zeroed.
    pub fn new(mut registers: R, shared: S, base: u64) -> Result<Driver<R, S>, Error> {
        if registers.read(reg::MAGIC) != MAGIC
            || registers.read(reg::VERSION) != 2
            || registers.read(reg::DEVICE_ID) != DEVICE_BLOCK
        {
            return Err(Error::NotBlockDevice);
        }
        registers.write(reg::STATUS, 0);
        let mut spins = 0u32;
        while registers.read(reg::STATUS) != 0 {
            spins += 1;
            if spins > 1_000_000 {
                return Err(Error::Device);
            }
        }
        registers.write(reg::STATUS, STATUS_ACKNOWLEDGE);
        registers.write(reg::STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        registers.write(reg::DEVICE_FEATURES_SEL, 1);
        if registers.read(reg::DEVICE_FEATURES) & F_VERSION_1 == 0 {
            return Err(fail(&mut registers, Error::Negotiation));
        }
        registers.write(reg::DEVICE_FEATURES_SEL, 0);
        let flush = registers.read(reg::DEVICE_FEATURES) & F_BLK_FLUSH != 0;
        registers.write(reg::DRIVER_FEATURES_SEL, 0);
        registers.write(reg::DRIVER_FEATURES, if flush { F_BLK_FLUSH } else { 0 });
        registers.write(reg::DRIVER_FEATURES_SEL, 1);
        registers.write(reg::DRIVER_FEATURES, F_VERSION_1);
        let features_ok = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
        registers.write(reg::STATUS, features_ok);
        if registers.read(reg::STATUS) & STATUS_FEATURES_OK == 0 {
            return Err(fail(&mut registers, Error::Negotiation));
        }

        registers.write(reg::QUEUE_SEL, 0);
        if registers.read(reg::QUEUE_READY) != 0 {
            return Err(fail(&mut registers, Error::Queue));
        }
        let max = registers.read(reg::QUEUE_NUM_MAX);
        if max < u32::from(QUEUE_SIZE) {
            return Err(fail(&mut registers, Error::Queue));
        }
        registers.write(reg::QUEUE_NUM, u32::from(QUEUE_SIZE));
        write_address(&mut registers, reg::QUEUE_DESC, base + DESC_AT as u64);
        write_address(&mut registers, reg::QUEUE_AVAIL, base + AVAIL_AT as u64);
        write_address(&mut registers, reg::QUEUE_USED, base + USED_AT as u64);
        registers.write(reg::QUEUE_READY, 1);
        shared.publish();

        let low = u64::from(registers.read(reg::CONFIG));
        let high = u64::from(registers.read(reg::CONFIG + 4));
        let geometry = Geometry {
            sectors: high << 32 | low,
            flush,
        };
        registers.write(reg::STATUS, features_ok | STATUS_DRIVER_OK);
        Ok(Driver {
            registers,
            shared,
            base,
            geometry,
            next_avail: 0,
            last_used: 0,
            in_flight: [false; MAX_IN_FLIGHT],
            failed: false,
            outstanding: 0,
        })
    }

    pub const fn geometry(&self) -> Geometry {
        self.geometry
    }

    /// Requests a batch can hold.
    pub const fn capacity(&self) -> usize {
        MAX_IN_FLIGHT - self.outstanding
    }

    fn descriptor(&mut self, index: usize, address: u64, length: u32, flags: u16, next: u16) {
        let at = DESC_AT + 16 * index;
        self.shared.write64(at, address);
        self.shared.write32(at + 8, length);
        self.shared.write16(at + 12, flags);
        self.shared.write16(at + 14, next);
    }

    /// Queues a request, without telling the device.
    pub fn queue(&mut self, request: &Request<'_>) -> Result<(), Error> {
        if request.pieces.len() > MAX_PIECES {
            return Err(Error::TooLarge);
        }
        let Some(slot) = self.in_flight.iter().position(|busy| !busy) else {
            return Err(Error::TooLarge);
        };
        let (kind, data_flags) = match request.kind {
            Kind::Read => (BLK_T_IN, DESC_WRITE),
            Kind::Write => (BLK_T_OUT, 0),
            Kind::Flush => (BLK_T_FLUSH, 0),
        };
        let header = HEADERS_AT + slot * 32;
        self.shared.write32(header, kind);
        self.shared.write32(header + 4, 0);
        self.shared.write64(header + 8, request.sector);
        // Anything but 0 and the device's own values: a status it never wrote
        // must not look like success.
        self.shared.write8(header + 16, 0xff);
        let head = slot * SLOT;
        let base = self.base + header as u64;
        self.descriptor(head, base, 16, DESC_NEXT, (head + 1) as u16);
        for (i, piece) in request.pieces.iter().enumerate() {
            let at = head + 1 + i;
            let flags = data_flags | DESC_NEXT;
            self.descriptor(at, piece.address, piece.length, flags, (at + 1) as u16);
        }
        let status = head + 1 + request.pieces.len();
        self.descriptor(status, base + 16, 1, DESC_WRITE, 0);

        let index = self.next_avail;
        let ring = AVAIL_AT + 4 + 2 * (usize::from(index) % N);
        self.shared.write16(ring, head as u16);
        self.next_avail = index.wrapping_add(1);
        self.in_flight[slot] = true;
        self.outstanding += 1;
        Ok(())
    }

    /// Tells the device what is queued.
    pub fn notify(&mut self) {
        self.shared.publish();
        self.shared.write16(AVAIL_AT + 2, self.next_avail);
        self.shared.publish();
        self.registers.write(reg::QUEUE_NOTIFY, 0);
    }

    /// Takes every finished request. Gives how many are still outstanding, or
    /// `Error::Device` if the device broke the protocol: it must be reset.
    pub fn collect(&mut self) -> Result<usize, Error> {
        self.shared.acquire();
        let used = self.shared.read16(USED_AT + 2);
        // Read once: the device can change it. More than is outstanding is a
        // lie, and following it would read stale rings.
        let new = used.wrapping_sub(self.last_used);
        if usize::from(new) > self.outstanding {
            return Err(Error::Device);
        }
        for _ in 0..new {
            let entry = USED_AT + 4 + 8 * (usize::from(self.last_used) % N);
            let id = self.shared.read32(entry) as usize;
            self.last_used = self.last_used.wrapping_add(1);
            if !id.is_multiple_of(SLOT) || id / SLOT >= MAX_IN_FLIGHT {
                return Err(Error::Device);
            }
            let slot = id / SLOT;
            if !self.in_flight[slot] {
                return Err(Error::Device);
            }
            self.in_flight[slot] = false;
            self.outstanding -= 1;
            let status = self.shared.read8(HEADERS_AT + slot * 32 + 16);
            // The used length is advisory, and a whole transfer is all this
            // driver accepts: the status says whether it happened.
            match status {
                0 => {}
                1 | 2 => self.failed = true,
                _ => return Err(Error::Device),
            }
        }
        Ok(self.outstanding)
    }

    /// True once if a request failed since the last call.
    pub fn take_failure(&mut self) -> bool {
        core::mem::take(&mut self.failed)
    }

    /// Takes down an interrupt.
    pub fn acknowledge(&mut self) {
        let bits = self.registers.read(reg::INTERRUPT_STATUS);
        self.registers.write(reg::INTERRUPT_ACK, bits);
    }

    /// Stops the device: it touches no queue after this returns true.
    pub fn reset(&mut self) -> bool {
        self.registers.write(reg::STATUS, 0);
        for _ in 0..1_000_000 {
            if self.registers.read(reg::STATUS) == 0 {
                return true;
            }
        }
        false
    }
}

fn write_address<R: Registers>(registers: &mut R, at: usize, address: u64) {
    registers.write(at, address as u32);
    registers.write(at + 4, (address >> 32) as u32);
}

fn fail<R: Registers>(registers: &mut R, error: Error) -> Error {
    let status = registers.read(reg::STATUS);
    registers.write(reg::STATUS, status | STATUS_FAILED);
    error
}

/// Splits `[offset, offset + length)` of a memory pinned as `runs`
/// (`(device address, length)` in order) into pieces. None if it runs off the
/// end, or needs more than `out` holds.
pub fn pieces(
    runs: &[(u64, u64)],
    mut offset: u64,
    mut length: u64,
    out: &mut [Piece],
) -> Option<usize> {
    let mut count = 0;
    for (address, size) in runs {
        if length == 0 {
            break;
        }
        if offset >= *size {
            offset -= size;
            continue;
        }
        let take = (size - offset).min(length).min(u64::from(u32::MAX));
        *out.get_mut(count)? = Piece {
            address: address.checked_add(offset)?,
            length: take as u32,
        };
        count += 1;
        length -= take;
        offset = 0;
    }
    (length == 0).then_some(count)
}

#[cfg(test)]
mod tests;

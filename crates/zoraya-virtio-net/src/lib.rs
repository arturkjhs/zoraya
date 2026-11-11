//! A virtio-net driver (ADR-0221 item 1) as logic over `zoraya-virtio`'s
//! traits, so that it runs on the host against a fake device.
//!
//! One receive queue and one transmit queue, `VIRTIO_NET_F_MAC` and
//! `VIRTIO_NET_F_STATUS` only: no offloads, no merged buffers, no control
//! queue (virtio 1.2, section 5.1). The frames live in slots of a memory
//! object that the caller pinned; the driver never reads or writes them.
//! Each receive buffer is a chain of two descriptors, the driver's own
//! 12-byte header and the slot (12 + 1514 = 1526 bytes); each transmit is
//! the driver's all-zero header and the frame. The device is not trusted: a
//! used index that is not what was offered, or a length beyond the buffer, is
//! `Error::Device` and the device must be reset. Everything the service
//! asks for is checked and refused with a [`Refused`], never a panic.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

use zoraya_virtio::queue::{Buffer, Layout, Queue, begin, ready, reset};
use zoraya_virtio::{DEVICE_NET, Registers, Shared, reg};

pub use zoraya_virtio::Error as TransportError;

pub mod serve;

/// Slots in each direction; the service's `zoraya-proto-nic::SLOTS` (the
/// two are compared by the user program's build).
pub const SLOTS: usize = 64;
/// Bytes of a slot.
pub const SLOT_SIZE: u64 = 1536;
/// Bytes of the object the driver pins.
pub const OBJECT_SIZE: u64 = 2 * SLOTS as u64 * SLOT_SIZE;
/// The longest and the shortest frame.
pub const MAX_FRAME: u32 = 1514;
pub const MIN_FRAME: u32 = 14;
/// The virtio-net header with `VERSION_1`: 12 bytes (5.1.6).
pub const HEADER: u32 = 12;

const F_MAC: u32 = 1 << 5;
const F_STATUS: u32 = 1 << 16;
const STATUS_LINK_UP: u16 = 1;
const CONFIG_STATUS: usize = reg::CONFIG + 6;
const INTERRUPT_CONFIG: u32 = 2;

/// Two descriptors a slot.
const QUEUE_SIZE: u16 = 2 * SLOTS as u16;
const RX: Layout = Layout::new(QUEUE_SIZE, 0);
const TX: Layout = Layout::new(QUEUE_SIZE, RX.end);
const RX_HEADERS_AT: usize = TX.end;
const TX_HEADER_AT: usize = RX_HEADERS_AT + 16 * SLOTS;
/// The shared memory the driver needs for its queues: a contiguous, zeroed
/// object of this many bytes, pinned, whose device address is passed to
/// [`Net::new`].
pub const SHARED_MEMORY: usize = (TX_HEADER_AT + 16).next_multiple_of(4096);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Transport(TransportError),
    /// The device broke the protocol: reset it.
    Device,
    /// No MAC address in the configuration, or an all-zero or multicast one.
    Unusable,
}

impl From<TransportError> for Error {
    fn from(e: TransportError) -> Error {
        match e {
            TransportError::Device => Error::Device,
            other => Error::Transport(other),
        }
    }
}

/// Why a request of the service was not carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No frames object attached yet.
    NotAttached,
    /// A frames object is attached already.
    Attached,
    /// A slot number outside the direction's range.
    BadSlot,
    /// A slot not in the state the request needs.
    WrongState,
    /// A frame length outside `MIN_FRAME..=MAX_FRAME`.
    BadLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rx {
    /// With the device (or not yet offered).
    Posted,
    /// With the service, until released.
    Lent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tx {
    Free,
    /// With the device.
    Queued,
    /// Finished by the device, not yet reported to the service: not free
    /// until [`Net::reported`], so that the service never sees a slot
    /// reused before its `Sent`.
    Done,
}

pub struct Net<R, S> {
    registers: R,
    shared: S,
    base: u64,
    rx: Queue,
    tx: Queue,
    mac: [u8; 6],
    status: bool,
    /// The device address of the frames object, once attached.
    frames: Option<u64>,
    rx_state: [Rx; SLOTS],
    tx_state: [Tx; SLOTS],
    link: bool,
    /// Frames the driver dropped and the service never saw: runts, and
    /// frames the service could not be told of.
    dropped: u32,
    /// Requests refused.
    refused: u32,
}

impl<R: Registers, S: Shared> Net<R, S> {
    /// Resets the device, reads its address and sets up both queues.
    /// `shared` is `SHARED_MEMORY` bytes at device address `base`, zeroed.
    pub fn new(mut registers: R, shared: S, base: u64) -> Result<Net<R, S>, Error> {
        let agreed = begin(&mut registers, DEVICE_NET, F_MAC | F_STATUS)?;
        if agreed & F_MAC == 0 {
            reset(&mut registers);
            return Err(Error::Unusable);
        }
        let mut mac = [0; 6];
        for (i, byte) in mac.iter_mut().enumerate() {
            *byte = registers.read8(reg::CONFIG + i);
        }
        // Not all zero, and unicast (the group bit of the first byte clear).
        if mac == [0; 6] || mac[0] & 1 != 0 {
            reset(&mut registers);
            return Err(Error::Unusable);
        }
        let rx = Queue::new(0, RX, base);
        let tx = Queue::new(1, TX, base);
        rx.set_up(&mut registers)?;
        tx.set_up(&mut registers)?;
        ready(&mut registers);
        let mut net = Net {
            registers,
            shared,
            base,
            rx,
            tx,
            mac,
            status: agreed & F_STATUS != 0,
            frames: None,
            rx_state: [Rx::Posted; SLOTS],
            tx_state: [Tx::Free; SLOTS],
            link: true,
            dropped: 0,
            refused: 0,
        };
        net.link = net.read_link();
        Ok(net)
    }

    pub const fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// The link state as last read.
    pub const fn link(&self) -> bool {
        self.link
    }

    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    pub const fn refused(&self) -> u32 {
        self.refused
    }

    fn read_link(&self) -> bool {
        if !self.status {
            return true;
        }
        let low = u16::from(self.registers.read8(CONFIG_STATUS));
        let high = u16::from(self.registers.read8(CONFIG_STATUS + 1));
        (high << 8 | low) & STATUS_LINK_UP != 0
    }

    fn refuse<T>(&mut self, why: Refused) -> Result<T, Refused> {
        self.refused = self.refused.saturating_add(1);
        Err(why)
    }

    fn address(&self, frames: u64, slot: usize) -> u64 {
        frames + SLOT_SIZE * slot as u64
    }

    fn post(&mut self, frames: u64, slot: usize) -> Result<(), Error> {
        let chain = [
            Buffer {
                address: self.base + (RX_HEADERS_AT + 16 * slot) as u64,
                length: HEADER,
                writable: true,
            },
            Buffer {
                address: self.address(frames, slot),
                length: MAX_FRAME,
                writable: true,
            },
        ];
        self.rx.offer(&mut self.shared, 2 * slot as u16, &chain)?;
        self.rx_state[slot] = Rx::Posted;
        Ok(())
    }

    /// Attaches the frames object: `frames` is the device address of its
    /// first byte, `OBJECT_SIZE` bytes contiguous and pinned. Offers every
    /// receive slot to the device.
    pub fn attach(&mut self, frames: u64) -> Result<(), Refused> {
        if self.frames.is_some() {
            return self.refuse(Refused::Attached);
        }
        if frames.checked_add(OBJECT_SIZE).is_none() {
            return self.refuse(Refused::BadSlot);
        }
        self.frames = Some(frames);
        for slot in 0..SLOTS {
            // A fresh queue has room for every slot; failing here is a bug
            // of this crate, and the slot then stays unposted.
            if self.post(frames, slot).is_err() {
                self.rx_state[slot] = Rx::Lent;
            }
        }
        self.rx.notify(&mut self.registers, &mut self.shared);
        Ok(())
    }

    /// The service gives receive slot `slot` back.
    pub fn release(&mut self, slot: u32) -> Result<(), Refused> {
        let Some(frames) = self.frames else {
            return self.refuse(Refused::NotAttached);
        };
        let index = slot as usize;
        if slot as u64 >= SLOTS as u64 {
            return self.refuse(Refused::BadSlot);
        }
        if self.rx_state[index] != Rx::Lent {
            return self.refuse(Refused::WrongState);
        }
        if self.post(frames, index).is_err() {
            return self.refuse(Refused::WrongState);
        }
        self.rx.notify(&mut self.registers, &mut self.shared);
        Ok(())
    }

    /// A receive slot that was handed out in a message that then could not
    /// be delivered: it goes back to the device and the frame is counted
    /// dropped.
    pub fn undo(&mut self, slot: u32) -> Result<(), Refused> {
        self.release(slot)?;
        self.dropped = self.dropped.saturating_add(1);
        Ok(())
    }

    /// Transmit slot `slot` (a number in `SLOTS..2 * SLOTS`) has been
    /// reported to the service: it is free again. False if it was not
    /// finished.
    pub fn reported(&mut self, slot: u32) -> bool {
        let Some(index) = (slot as usize).checked_sub(SLOTS).filter(|i| *i < SLOTS) else {
            return false;
        };
        if self.tx_state[index] == Tx::Done {
            self.tx_state[index] = Tx::Free;
            return true;
        }
        false
    }

    /// The service sends `length` bytes of transmit slot `slot` (a number
    /// in `SLOTS..2 * SLOTS`).
    pub fn transmit(&mut self, slot: u32, length: u32) -> Result<(), Refused> {
        let Some(frames) = self.frames else {
            return self.refuse(Refused::NotAttached);
        };
        let Some(index) = (slot as usize)
            .checked_sub(SLOTS)
            .filter(|i| *i < SLOTS && slot < 2 * SLOTS as u32)
        else {
            return self.refuse(Refused::BadSlot);
        };
        if self.tx_state[index] != Tx::Free {
            return self.refuse(Refused::WrongState);
        }
        if !(MIN_FRAME..=MAX_FRAME).contains(&length) {
            return self.refuse(Refused::BadLength);
        }
        let chain = [
            Buffer {
                address: self.base + TX_HEADER_AT as u64,
                length: HEADER,
                writable: false,
            },
            Buffer {
                address: self.address(frames, SLOTS + index),
                length,
                writable: false,
            },
        ];
        if self
            .tx
            .offer(&mut self.shared, 2 * index as u16, &chain)
            .is_err()
        {
            return self.refuse(Refused::WrongState);
        }
        self.tx_state[index] = Tx::Queued;
        self.tx.notify(&mut self.registers, &mut self.shared);
        Ok(())
    }

    /// Reads and acknowledges the interrupt status: true if the
    /// configuration changed.
    pub fn acknowledge(&mut self) -> bool {
        let bits = self.registers.read(reg::INTERRUPT_STATUS);
        self.registers.write(reg::INTERRUPT_ACK, bits);
        bits & INTERRUPT_CONFIG != 0
    }

    /// Reads the link again: `Some(state)` if it changed since the last read.
    pub fn link_changed(&mut self) -> Option<bool> {
        let now = self.read_link();
        (now != self.link).then(|| {
            self.link = now;
            now
        })
    }

    pub fn reset(&mut self) -> bool {
        reset(&mut self.registers)
    }

    /// Takes everything the device has finished. `received` is called with
    /// each receive slot and the length of its frame, and returns false if
    /// the service could not be told: the slot then goes back to the device
    /// at once and the frame is counted dropped. `sent` is called with each
    /// transmit slot (a number in `SLOTS..2 * SLOTS`) the device is done
    /// with; the slot stays unusable until [`Net::reported`]. A call takes
    /// at most `2 * SLOTS` used entries of the receive queue, so that a
    /// device that keeps completing runts cannot keep the driver here; the
    /// level-triggered interrupt brings the rest.
    pub fn poll(
        &mut self,
        received: &mut dyn FnMut(u32, u32) -> bool,
        sent: &mut dyn FnMut(u32),
    ) -> Result<(), Error> {
        let Some(frames) = self.frames else {
            return Ok(());
        };
        let mut posted = false;
        for _ in 0..2 * SLOTS {
            let Some(used) = self.rx.take(&self.shared)? else {
                break;
            };
            let head = usize::from(used.head);
            let slot = head / 2;
            if head % 2 != 0 || slot >= SLOTS || self.rx_state[slot] != Rx::Posted {
                return Err(Error::Device);
            }
            // Only the buffer: the header and then at most MAX_FRAME bytes.
            let Some(length) = used.length.checked_sub(HEADER) else {
                return Err(Error::Device);
            };
            if length > MAX_FRAME {
                return Err(Error::Device);
            }
            self.rx_state[slot] = Rx::Lent;
            let told = length >= MIN_FRAME && received(slot as u32, length);
            if !told {
                self.dropped = self.dropped.saturating_add(1);
                self.post(frames, slot)?;
                posted = true;
            }
        }
        if posted {
            self.rx.notify(&mut self.registers, &mut self.shared);
        }
        while let Some(used) = self.tx.take(&self.shared)? {
            let head = usize::from(used.head);
            let slot = head / 2;
            if head % 2 != 0 || slot >= SLOTS || self.tx_state[slot] != Tx::Queued {
                return Err(Error::Device);
            }
            self.tx_state[slot] = Tx::Done;
            sent((SLOTS + slot) as u32);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

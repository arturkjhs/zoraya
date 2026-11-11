//! The service's end of the frame protocol (ADR-0221 item 2, ADR-0280): the
//! slot bookkeeping between `netd` and the driver, as a [`Link`] for the
//! stack. Everything the driver says is hostile: a slot number, a length, a
//! message that does not decode. This module is where those numbers enter
//! `netd`, and the only place that indexes by them (`netd` itself only reads
//! and writes slot bytes through [`SlotMemory`], with a slot and a length
//! this module checked).
//!
//! Slots `0..SLOTS` receive and `SLOTS..2 * SLOTS` transmit. A receive slot is
//! *device* (the driver may write it) or *lent* (ours until released); a
//! transmit slot is *free*, *queued* (written, `Transmit` not yet written to
//! the channel), or *in flight* (until `Sent`, or a `Rejected` that returns
//! it). Frame bytes are copied out of a lent slot before anything parses
//! them (see `link`).

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use zoraya_proto_nic::{
    Frame, LinkChanged, MAX_BATCH, MAX_FRAME, MIN_FRAME, Received, Rejected, Release, SLOTS, Sent,
    Slot, Transmit, reason,
};
use zoraya_protocol::{Header, Seq, decode, encode};

use crate::link::Link;

/// The most messages read from the driver in one `pump`, so that a driver
/// that never stops talking cannot hold the single loop.
pub const MAX_MESSAGES: usize = 64;
/// The room for one message.
const BUFFER: usize = 2048;

/// What a read of the channel gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// A message of this many bytes is in the buffer.
    Message(usize),
    Empty,
    /// The driver's end is closed.
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Write {
    Done,
    /// The channel is full; try again when it is writable.
    Full,
    Closed,
}

/// The channel to the driver.
pub trait Wire {
    fn read(&mut self, buf: &mut [u8]) -> Read;
    fn write(&mut self, bytes: &[u8]) -> Write;
}

/// The frames object. `slot` is below `2 * SLOTS` and `length` is at most
/// `SLOT_SIZE`: this module checked both.
pub trait SlotMemory {
    fn read_slot(&self, slot: u32, out: &mut [u8]);
    fn write_slot(&mut self, slot: u32, data: &[u8]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rx {
    Device,
    Lent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tx {
    Free,
    Queued,
    InFlight,
}

/// What went wrong with the driver, counted for `status`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Faults {
    /// A frame, `Sent` or `Rejected` entry naming a slot in the wrong state
    /// or out of range, or a frame of a length no frame has.
    pub bad_entries: u64,
    /// Messages that did not decode or had an ordinal we do not know.
    pub bad_messages: u64,
}

pub struct Nic<W: Wire, M: SlotMemory> {
    wire: W,
    mem: M,
    rx: [Rx; SLOTS as usize],
    tx: [Tx; SLOTS as usize],
    /// Received frames waiting for the stack: slot and length.
    arrived: VecDeque<(u32, u32)>,
    /// Receive slots to give back.
    release: Vec<Slot>,
    /// Transmit slots written and not yet announced.
    transmit: Vec<Frame>,
    free: Vec<u32>,
    up: bool,
    closed: bool,
    pub faults: Faults,
}

fn record(slot: u32) -> Slot {
    Slot { slot }
}

impl<W: Wire, M: SlotMemory> Nic<W, M> {
    /// `up` is the link state `Info` gave.
    pub fn new(wire: W, mem: M, up: bool) -> Nic<W, M> {
        Nic {
            wire,
            mem,
            rx: [Rx::Device; SLOTS as usize],
            tx: [Tx::Free; SLOTS as usize],
            arrived: VecDeque::new(),
            release: Vec::new(),
            transmit: Vec::new(),
            free: (SLOTS..2 * SLOTS).rev().collect(),
            up,
            closed: false,
            faults: Faults::default(),
        }
    }

    /// The driver's end closed, or it spoke nonsense: `netd` ends and `init`
    /// starts the network again (ADR-0221 item 5).
    pub fn closed(&self) -> bool {
        self.closed
    }

    pub fn wire(&mut self) -> &mut W {
        &mut self.wire
    }

    pub fn mem_mut(&mut self) -> &mut M {
        &mut self.mem
    }

    /// Whether `flush` has something it could not write.
    pub fn pending(&self) -> bool {
        !self.release.is_empty() || !self.transmit.is_empty()
    }

    fn rx_index(slot: u32) -> Option<usize> {
        (slot < SLOTS).then_some(slot as usize)
    }

    fn tx_index(slot: u32) -> Option<usize> {
        slot.checked_sub(SLOTS)
            .filter(|i| *i < SLOTS)
            .map(|i| i as usize)
    }

    /// Reads what the driver sent: frames, finished transmits, refusals and
    /// link changes.
    pub fn pump(&mut self) {
        let mut buf = [0u8; BUFFER];
        for _ in 0..MAX_MESSAGES {
            match self.wire.read(&mut buf) {
                Read::Empty => return,
                Read::Closed => {
                    self.closed = true;
                    return;
                }
                Read::Message(n) => {
                    let bytes = &buf[..n.min(BUFFER)];
                    if !self.handle(bytes) {
                        self.faults.bad_messages += 1;
                        // A driver that breaks the protocol is not trusted
                        // with more frames.
                        self.closed = true;
                        return;
                    }
                }
            }
        }
    }

    fn handle(&mut self, bytes: &[u8]) -> bool {
        let Ok((header, _)) = Header::decode(bytes) else {
            return false;
        };
        match header.ordinal {
            16 => {
                let Ok((_, m)) = decode::<Received>(bytes, 0) else {
                    return false;
                };
                for f in m.frames.iter() {
                    self.arrive(f);
                }
            }
            17 => {
                let Ok((_, m)) = decode::<Sent>(bytes, 0) else {
                    return false;
                };
                for s in m.slots.iter() {
                    self.finish(s.slot, false);
                }
            }
            19 => {
                let Ok((_, m)) = decode::<Rejected>(bytes, 0) else {
                    return false;
                };
                for r in m.slots.iter() {
                    // Still with the device: its `Sent` comes. Any other
                    // reason leaves the slot ours again.
                    if r.reason != reason::STILL_QUEUED {
                        self.finish(r.slot, true);
                    }
                }
            }
            18 => {
                let Ok((_, m)) = decode::<LinkChanged>(bytes, 0) else {
                    return false;
                };
                self.up = m.link;
            }
            _ => return false,
        }
        true
    }

    /// A received frame: checked, then queued for the stack; a frame that is
    /// no frame is given straight back.
    fn arrive(&mut self, f: Frame) {
        let Some(i) = Self::rx_index(f.slot) else {
            self.faults.bad_entries += 1;
            return;
        };
        if self.rx[i] != Rx::Device {
            // The driver says a slot is its that it already gave us.
            self.faults.bad_entries += 1;
            return;
        }
        self.rx[i] = Rx::Lent;
        if f.length < MIN_FRAME || f.length > MAX_FRAME {
            self.faults.bad_entries += 1;
            self.give_back(f.slot);
        } else {
            self.arrived.push_back((f.slot, f.length));
        }
    }

    fn give_back(&mut self, slot: u32) {
        if let Some(i) = Self::rx_index(slot)
            && self.rx[i] == Rx::Lent
        {
            self.rx[i] = Rx::Device;
            self.release.push(record(slot));
        }
    }

    /// A transmit slot comes back (`Sent`, or a refusal that did not take
    /// it). One that is not in flight is a fault and changes nothing.
    fn finish(&mut self, slot: u32, rejected: bool) {
        let _ = rejected;
        match Self::tx_index(slot) {
            Some(i) if self.tx[i] == Tx::InFlight => {
                self.tx[i] = Tx::Free;
                self.free.push(slot);
            }
            _ => self.faults.bad_entries += 1,
        }
    }

    /// Writes the releases and transmits that wait. What the channel does
    /// not take stays pending for the next call.
    pub fn flush(&mut self) {
        let mut out = [0u8; BUFFER];
        while !self.release.is_empty() {
            let n = self.release.len().min(MAX_BATCH);
            let message = Release {
                slots: Seq::Of(&self.release[..n]),
            };
            let Ok(length) = encode(&message, 0, &mut out) else {
                // Cannot happen: the batch is bounded. Dropping keeps the
                // loop from spinning.
                self.release.drain(..n);
                continue;
            };
            match self.wire.write(&out[..length]) {
                Write::Done => {
                    self.release.drain(..n);
                }
                Write::Full => return,
                Write::Closed => {
                    self.closed = true;
                    return;
                }
            }
        }
        while !self.transmit.is_empty() {
            let n = self.transmit.len().min(MAX_BATCH);
            let message = Transmit {
                frames: Seq::Of(&self.transmit[..n]),
            };
            let Ok(length) = encode(&message, 0, &mut out) else {
                self.transmit.drain(..n);
                continue;
            };
            match self.wire.write(&out[..length]) {
                Write::Done => {
                    for f in self.transmit.drain(..n) {
                        if let Some(i) = Self::tx_index(f.slot) {
                            self.tx[i] = Tx::InFlight;
                        }
                    }
                }
                Write::Full => return,
                Write::Closed => {
                    self.closed = true;
                    return;
                }
            }
        }
    }
}

impl<W: Wire, M: SlotMemory> Link for Nic<W, M> {
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        let (slot, length) = self.arrived.pop_front()?;
        let n = length as usize;
        // Checked on arrival; checked again against the buffer, because this
        // is the copy that matters.
        if n > buf.len() || n > MAX_FRAME as usize {
            self.faults.bad_entries += 1;
            self.give_back(slot);
            return None;
        }
        self.mem.read_slot(slot, &mut buf[..n]);
        self.give_back(slot);
        Some(n)
    }

    fn has_input(&self) -> bool {
        !self.arrived.is_empty()
    }

    fn can_send(&self) -> bool {
        !self.free.is_empty() && self.transmit.len() < MAX_BATCH && !self.closed
    }

    fn send(&mut self, frame: &[u8]) -> bool {
        let n = frame.len();
        if n < MIN_FRAME as usize || n > MAX_FRAME as usize || self.closed {
            return false;
        }
        let Some(slot) = self.free.pop() else {
            return false;
        };
        let Some(i) = Self::tx_index(slot) else {
            return false;
        };
        self.mem.write_slot(slot, frame);
        self.tx[i] = Tx::Queued;
        self.transmit.push(Frame {
            slot,
            length: n as u32,
        });
        true
    }

    fn is_up(&self) -> bool {
        self.up
    }
}

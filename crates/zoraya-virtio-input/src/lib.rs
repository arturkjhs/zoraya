//! A virtio-input driver (ADR-0092) as logic over `zoraya-virtio`'s traits,
//! so that it runs on the host against a fake device.
//!
//! The device writes 8-byte evdev events into buffers the driver offers on
//! queue 0 (virtio 1.2, section 5.8). Each is read once, checked, and either
//! kept in the current frame or dropped and counted; a frame ends at
//! `SYN_REPORT`. A broken used ring is `Error::Device`: reset and stop.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

use zoraya_virtio::queue::{Buffer, Layout, Queue, begin, ready, reset};
use zoraya_virtio::{DEVICE_INPUT, Registers, Shared, reg};

pub use zoraya_virtio::Error as TransportError;

/// evdev event types and codes the driver knows.
pub mod ev {
    pub const SYN: u16 = 0;
    pub const KEY: u16 = 1;
    pub const ABS: u16 = 3;
    pub const SYN_REPORT: u16 = 0;
    pub const ABS_X: u16 = 0;
    pub const ABS_Y: u16 = 1;
    /// Key codes, buttons included, are below this.
    pub const KEY_LIMIT: u16 = 0x300;
}

/// Configuration selectors (5.8.2).
mod select {
    pub const EV_BITS: u8 = 0x11;
    pub const ABS_INFO: u8 = 0x12;
}

/// Buffers on the event queue `[assumption, ADR-0092]`.
pub const BUFFERS: u16 = 64;
/// Records in one frame (ADR-0092).
pub const MAX_FRAME: usize = 64;
/// The scaled range of an axis.
pub const AXIS_MAX: u32 = 65535;

const QUEUE: Layout = Layout::new(BUFFERS, 0);
const EVENTS_AT: usize = QUEUE.end;
/// The shared memory the driver needs: a contiguous, zeroed object of this
/// many bytes, pinned, whose device address is passed to [`Input::new`].
pub const SHARED_MEMORY: usize = (EVENTS_AT + 8 * BUFFERS as usize).next_multiple_of(4096);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Transport(TransportError),
    /// The device broke the protocol: reset it.
    Device,
    /// Neither a keyboard nor an absolute pointer, or an axis with
    /// `max <= min`.
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

/// An axis's range, as the device declared it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axis {
    pub min: u32,
    pub max: u32,
}

impl Axis {
    /// `value` scaled to `0..=AXIS_MAX`, or None outside the range.
    pub fn scale(&self, value: u32) -> Option<u32> {
        if value < self.min || value > self.max {
            return None;
        }
        let span = u64::from(self.max - self.min);
        Some((u64::from(value - self.min) * u64::from(AXIS_MAX) / span) as u32)
    }
}

/// What the device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Keyboard,
    Pointer { x: Axis, y: Axis },
}

/// One checked event, as the input protocol carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub kind: u32,
    pub code: u32,
    pub value: u32,
}

/// Record kinds (ADR-0092).
pub const KEY: u32 = 0;
pub const AXIS: u32 = 1;

pub struct Input<R, S> {
    registers: R,
    shared: S,
    base: u64,
    queue: Queue,
    kind: Kind,
    frame: [Record; MAX_FRAME],
    length: usize,
    /// Events dropped since the last frame that reached the client.
    dropped: u32,
}

fn config_read<R: Registers>(r: &mut R, which: u8, sub: u8, out: &mut [u8; 128]) -> usize {
    r.write8(reg::CONFIG, which);
    r.write8(reg::CONFIG + 1, sub);
    let size = usize::from(r.read8(reg::CONFIG + 2)).min(128);
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = if i < size {
            r.read8(reg::CONFIG + 8 + i)
        } else {
            0
        };
    }
    size
}

fn axis<R: Registers>(r: &mut R, code: u16) -> Result<Axis, Error> {
    let mut data = [0; 128];
    if config_read(r, select::ABS_INFO, code as u8, &mut data) < 8 {
        return Err(Error::Unusable);
    }
    let word = |at: usize| u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
    let (min, max) = (word(0), word(4));
    if max <= min {
        return Err(Error::Unusable);
    }
    Ok(Axis { min, max })
}

/// What the configuration says the device is.
fn probe<R: Registers>(r: &mut R) -> Result<Kind, Error> {
    let mut bits = [0; 128];
    let size = config_read(r, select::EV_BITS, ev::ABS as u8, &mut bits);
    let has = |code: u16| bits[usize::from(code / 8)] & (1 << (code % 8)) != 0;
    if size > 0 && has(ev::ABS_X) && has(ev::ABS_Y) {
        let x = axis(r, ev::ABS_X)?;
        let y = axis(r, ev::ABS_Y)?;
        return Ok(Kind::Pointer { x, y });
    }
    if config_read(r, select::EV_BITS, ev::KEY as u8, &mut bits) > 0 {
        return Ok(Kind::Keyboard);
    }
    Err(Error::Unusable)
}

impl<R: Registers, S: Shared> Input<R, S> {
    /// Resets the device, finds what it is, sets up the event queue and
    /// offers every buffer. `shared` is `SHARED_MEMORY` bytes at device
    /// address `base`, zeroed.
    pub fn new(mut registers: R, shared: S, base: u64) -> Result<Input<R, S>, Error> {
        begin(&mut registers, DEVICE_INPUT, 0)?;
        let kind = match probe(&mut registers) {
            Ok(kind) => kind,
            Err(e) => {
                reset(&mut registers);
                return Err(e);
            }
        };
        let queue = Queue::new(0, QUEUE, base);
        queue.set_up(&mut registers)?;
        ready(&mut registers);
        let mut input = Input {
            registers,
            shared,
            base,
            queue,
            kind,
            frame: [Record {
                kind: 0,
                code: 0,
                value: 0,
            }; MAX_FRAME],
            length: 0,
            dropped: 0,
        };
        for head in 0..BUFFERS {
            input.offer(head)?;
        }
        input.queue.notify(&mut input.registers, &mut input.shared);
        Ok(input)
    }

    pub const fn kind(&self) -> Kind {
        self.kind
    }

    fn offer(&mut self, head: u16) -> Result<(), Error> {
        let buffer = Buffer {
            address: self.base + (EVENTS_AT + 8 * usize::from(head)) as u64,
            length: 8,
            writable: true,
        };
        Ok(self.queue.offer(&mut self.shared, head, &[buffer])?)
    }

    pub fn acknowledge(&mut self) {
        let bits = self.registers.read(reg::INTERRUPT_STATUS);
        self.registers.write(reg::INTERRUPT_ACK, bits);
    }

    pub fn reset(&mut self) -> bool {
        reset(&mut self.registers)
    }

    /// Events the driver has dropped and not yet reported.
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    fn keep(&mut self, record: Record) {
        match self.frame.get_mut(self.length) {
            Some(slot) => {
                *slot = record;
                self.length += 1;
            }
            None => self.dropped = self.dropped.saturating_add(1),
        }
    }

    fn event(
        &mut self,
        kind: u16,
        code: u16,
        value: u32,
        send: &mut dyn FnMut(&[Record], u32) -> bool,
    ) {
        match (kind, self.kind) {
            (ev::SYN, _) if code == ev::SYN_REPORT => {
                if self.length > 0 {
                    // The client never blocks the driver (INV-0092-2).
                    if send(&self.frame[..self.length], self.dropped) {
                        self.dropped = 0;
                    } else {
                        self.dropped = self.dropped.saturating_add(self.length as u32);
                    }
                    self.length = 0;
                }
            }
            (ev::KEY, _) if code < ev::KEY_LIMIT && value <= 2 => self.keep(Record {
                kind: KEY,
                code: u32::from(code),
                value,
            }),
            (ev::ABS, Kind::Pointer { x, y }) if code == ev::ABS_X || code == ev::ABS_Y => {
                let axis = if code == ev::ABS_X { x } else { y };
                match axis.scale(value) {
                    Some(scaled) => self.keep(Record {
                        kind: AXIS,
                        code: u32::from(code),
                        value: scaled,
                    }),
                    None => self.dropped = self.dropped.saturating_add(1),
                }
            }
            _ => self.dropped = self.dropped.saturating_add(1),
        }
    }

    /// Takes every event the device has written, offers its buffer again,
    /// and calls `send` with each finished frame and the count dropped
    /// before it. `send` returns false if the frame did not fit.
    pub fn poll(&mut self, send: &mut dyn FnMut(&[Record], u32) -> bool) -> Result<(), Error> {
        let mut offered = false;
        while let Some(used) = self.queue.take(&self.shared)? {
            if used.length != 8 {
                return Err(Error::Device);
            }
            // Copied out once; the device may write the buffer again as soon
            // as it is offered.
            let at = EVENTS_AT + 8 * usize::from(used.head);
            let kind = self.shared.read16(at);
            let code = self.shared.read16(at + 2);
            let value = self.shared.read32(at + 4);
            self.offer(used.head)?;
            offered = true;
            self.event(kind, code, value, send);
        }
        if offered {
            self.queue.notify(&mut self.registers, &mut self.shared);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

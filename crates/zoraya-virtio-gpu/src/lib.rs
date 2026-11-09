//! A virtio-gpu 2D driver (ADR-0090) as logic over `zoraya-virtio`'s traits,
//! so that it runs on the host against a fake device.
//!
//! One control queue, one command at a time: a device-readable request and a
//! device-writable response. Every response is checked for its type and its
//! length before anything in it is used, and anything unexpected is
//! `Error::Device`: the caller resets the device and stops (ADR-0090). The
//! layouts are those of the virtio 1.2 specification, section 5.7.6.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

use zoraya_virtio::queue::{Buffer, Layout, Queue, begin, ready, reset};
use zoraya_virtio::{DEVICE_GPU, Registers, Shared};

pub use zoraya_virtio::Error as TransportError;

/// Commands and responses (5.7.6.7).
pub mod cmd {
    pub const GET_DISPLAY_INFO: u32 = 0x0100;
    pub const RESOURCE_CREATE_2D: u32 = 0x0101;
    pub const RESOURCE_UNREF: u32 = 0x0102;
    pub const SET_SCANOUT: u32 = 0x0103;
    pub const RESOURCE_FLUSH: u32 = 0x0104;
    pub const TRANSFER_TO_HOST_2D: u32 = 0x0105;
    pub const RESOURCE_ATTACH_BACKING: u32 = 0x0106;
    pub const OK_NODATA: u32 = 0x1100;
    pub const OK_DISPLAY_INFO: u32 = 0x1101;
    /// The first error response; errors run up from here.
    pub const ERR_UNSPEC: u32 = 0x1200;
    pub const ERR_LAST: u32 = 0x1205;
}

/// `B8G8R8X8_UNORM`: a pixel is the little-endian word `0x00RRGGBB`.
pub const FORMAT: u32 = 2;
/// The resource the driver makes first; a new display size gets the other
/// one, `3 - RESOURCE` (ADR-0246), and the old one is unreferenced.
pub const RESOURCE: u32 = 1;

/// The configuration space (5.7.4): what the device has to say.
pub mod config {
    /// `events_read`: the events not yet cleared.
    pub const EVENTS_READ: usize = 0;
    /// `events_clear`: a bit written here clears it in `events_read`.
    pub const EVENTS_CLEAR: usize = 4;
    /// `VIRTIO_GPU_EVENT_DISPLAY`: the display's configuration changed.
    pub const EVENT_DISPLAY: u32 = 1;
}
/// The largest display side accepted `[assumption, ADR-0090]`.
pub const MAX_SIDE: u32 = 4096;
/// The most backing runs one attach carries.
pub const MAX_RUNS: usize = 1024;

const HEADER: usize = 24;
const DISPLAY_INFO: usize = HEADER + 16 * 24;
const QUEUE: Layout = Layout::new(16, 0);
const REQUEST_AT: usize = QUEUE.end;
const MAX_REQUEST: usize = 32 + 16 * MAX_RUNS;
const RESPONSE_AT: usize = (REQUEST_AT + MAX_REQUEST).next_multiple_of(4096);
/// The shared memory the driver needs: a contiguous, zeroed object of this
/// many bytes, pinned, whose device address is passed to [`Gpu::new`].
pub const SHARED_MEMORY: usize = (RESPONSE_AT + 512).next_multiple_of(4096);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Setting up the transport failed.
    Transport(TransportError),
    /// The device broke the protocol: reset it.
    Device,
    /// The device answered a command with this error.
    Command(u32),
    /// Scanout 0 is not enabled, or its size is not from 1 to `MAX_SIDE`.
    NoDisplay,
    /// A rectangle outside the resource, or empty.
    BadRange,
    /// The backing does not cover the resource, or has too many runs.
    BadBacking,
    /// Nothing is attached yet.
    NotAttached,
    /// A resource of the display's size exists already.
    Attached,
}

impl From<TransportError> for Error {
    fn from(e: TransportError) -> Error {
        match e {
            TransportError::Device => Error::Device,
            other => Error::Transport(other),
        }
    }
}

/// A rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// True if not empty and inside `width × height`.
    pub fn inside(&self, width: u32, height: u32) -> bool {
        let right = self.x.checked_add(self.width);
        let bottom = self.y.checked_add(self.height);
        self.width > 0
            && self.height > 0
            && right.is_some_and(|r| r <= width)
            && bottom.is_some_and(|b| b <= height)
    }
}

/// Writes a request into shared memory, field by field.
struct Request<'s, S: Shared> {
    shared: &'s mut S,
    at: usize,
}

impl<S: Shared> Request<'_, S> {
    fn u32(&mut self, value: u32) -> &mut Self {
        self.shared.write32(REQUEST_AT + self.at, value);
        self.at += 4;
        self
    }

    fn u64(&mut self, value: u64) -> &mut Self {
        self.shared.write64(REQUEST_AT + self.at, value);
        self.at += 8;
        self
    }

    fn header(&mut self, kind: u32) -> &mut Self {
        // type, flags, fence id (two words), context, ring and padding.
        self.u32(kind).u32(0).u64(0).u32(0).u32(0)
    }

    fn rect(&mut self, r: Rect) -> &mut Self {
        self.u32(r.x).u32(r.y).u32(r.width).u32(r.height)
    }
}

pub struct Gpu<R, S> {
    registers: R,
    shared: S,
    /// The device address of `shared`.
    base: u64,
    queue: Queue,
    /// The display's size as `GET_DISPLAY_INFO` last gave it.
    width: u32,
    height: u32,
    /// The resource on scanout 0 and its size, once attached.
    shown: Option<(u32, u32, u32)>,
}

impl<R: Registers, S: Shared> Gpu<R, S> {
    /// Resets the device and brings it up with `VERSION_1` alone
    /// (INV-0090-1) and the control queue. `shared` is `SHARED_MEMORY`
    /// bytes at device address `base`, zeroed.
    pub fn new(mut registers: R, shared: S, base: u64) -> Result<Gpu<R, S>, Error> {
        begin(&mut registers, DEVICE_GPU, 0)?;
        let queue = Queue::new(0, QUEUE, base);
        queue.set_up(&mut registers)?;
        ready(&mut registers);
        Ok(Gpu {
            registers,
            shared,
            base,
            queue,
            width: 0,
            height: 0,
            shown: None,
        })
    }

    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// True once after the device reported that the display changed
    /// (`VIRTIO_GPU_EVENT_DISPLAY`), which it then clears; the new size is
    /// [`Gpu::display_info`]'s to ask (ADR-0246).
    pub fn display_changed(&mut self) -> bool {
        let events = self
            .registers
            .read(zoraya_virtio::reg::CONFIG + config::EVENTS_READ);
        if events & config::EVENT_DISPLAY == 0 {
            return false;
        }
        self.registers.write(
            zoraya_virtio::reg::CONFIG + config::EVENTS_CLEAR,
            config::EVENT_DISPLAY,
        );
        true
    }

    /// Takes down an interrupt.
    pub fn acknowledge(&mut self) {
        let bits = self.registers.read(zoraya_virtio::reg::INTERRUPT_STATUS);
        self.registers
            .write(zoraya_virtio::reg::INTERRUPT_ACK, bits);
    }

    /// Stops the device: it touches no queue after this returns true.
    pub fn reset(&mut self) -> bool {
        reset(&mut self.registers)
    }

    /// Sends the request written so far and waits for its response, which
    /// must be `expected` and at least `length` bytes. `wait` blocks until
    /// the device may have answered, then unmasks its interrupt; an error
    /// from it is the caller's.
    fn call(
        &mut self,
        request: usize,
        expected: u32,
        length: usize,
        wait: &mut dyn FnMut() -> Result<(), Error>,
    ) -> Result<(), Error> {
        // A response the device never wrote must not read as success.
        self.shared.write32(RESPONSE_AT, 0);
        let chain = [
            Buffer {
                address: self.base + REQUEST_AT as u64,
                length: request as u32,
                writable: false,
            },
            Buffer {
                address: self.base + RESPONSE_AT as u64,
                length: length as u32,
                writable: true,
            },
        ];
        self.queue.offer(&mut self.shared, 0, &chain)?;
        self.queue.notify(&mut self.registers, &mut self.shared);
        // virtio-mmio is edge-triggered on QEMU `virt`: a completion that
        // lands while the line is still high from an earlier one makes no
        // edge. So the device's interrupt is taken down, and the queue looked
        // at again, before every wait (TASK-14 review, R1).
        let used = loop {
            if let Some(used) = self.queue.take(&self.shared)? {
                break used;
            }
            self.acknowledge();
            if let Some(used) = self.queue.take(&self.shared)? {
                break used;
            }
            wait()?;
        };
        if used.head != 0 || (used.length as usize) < HEADER || used.length as usize > length {
            return Err(Error::Device);
        }
        let kind = self.shared.read32(RESPONSE_AT);
        if kind == expected && used.length as usize >= length {
            Ok(())
        } else if (cmd::ERR_UNSPEC..=cmd::ERR_LAST).contains(&kind) {
            Err(Error::Command(kind))
        } else {
            Err(Error::Device)
        }
    }

    /// Asks for the display, and keeps scanout 0's size.
    pub fn display_info(
        &mut self,
        wait: &mut dyn FnMut() -> Result<(), Error>,
    ) -> Result<(u32, u32), Error> {
        let mut r = Request {
            shared: &mut self.shared,
            at: 0,
        };
        r.header(cmd::GET_DISPLAY_INFO);
        let length = r.at;
        self.call(length, cmd::OK_DISPLAY_INFO, DISPLAY_INFO, wait)?;
        // Read once: the device could change it.
        let at = RESPONSE_AT + HEADER;
        let width = self.shared.read32(at + 8);
        let height = self.shared.read32(at + 12);
        let enabled = self.shared.read32(at + 16);
        let side = |v: u32| (1..=MAX_SIDE).contains(&v);
        if enabled == 0 || !side(width) || !side(height) {
            return Err(Error::NoDisplay);
        }
        self.width = width;
        self.height = height;
        Ok((width, height))
    }

    /// Makes a resource of the display's size, backs it with `runs` (device
    /// address, length) and shows it on scanout 0. The runs must cover `width ×
    /// height × 4` bytes. Once attached, only a new display size (ADR-0246)
    /// takes another: the other resource is made and shown, then the old one
    /// is unreferenced, so the old backing stays shown until the new is.
    pub fn attach(
        &mut self,
        runs: &[(u64, u64)],
        wait: &mut dyn FnMut() -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self
            .shown
            .is_some_and(|(_, w, h)| (w, h) == (self.width, self.height))
        {
            return Err(Error::Attached);
        }
        let old = self.shown.map(|(r, _, _)| r);
        let resource = old.map_or(RESOURCE, |r| 3 - r);
        if self.width == 0 {
            return Err(Error::NoDisplay);
        }
        let needed = u64::from(self.width) * u64::from(self.height) * 4;
        let mut covered = 0u64;
        let mut count = 0;
        for (_, length) in runs {
            if covered >= needed {
                break;
            }
            if *length == 0 || *length > u64::from(u32::MAX) {
                return Err(Error::BadBacking);
            }
            covered += length;
            count += 1;
        }
        if covered < needed || count > MAX_RUNS {
            return Err(Error::BadBacking);
        }
        let (width, height) = (self.width, self.height);
        let mut r = Request {
            shared: &mut self.shared,
            at: 0,
        };
        r.header(cmd::RESOURCE_CREATE_2D)
            .u32(resource)
            .u32(FORMAT)
            .u32(width)
            .u32(height);
        let length = r.at;
        self.call(length, cmd::OK_NODATA, HEADER, wait)?;
        let mut r = Request {
            shared: &mut self.shared,
            at: 0,
        };
        r.header(cmd::RESOURCE_ATTACH_BACKING)
            .u32(resource)
            .u32(count as u32);
        for (address, length) in &runs[..count] {
            r.u64(*address).u32(*length as u32).u32(0);
        }
        let length = r.at;
        self.call(length, cmd::OK_NODATA, HEADER, wait)?;
        let whole = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        let mut r = Request {
            shared: &mut self.shared,
            at: 0,
        };
        r.header(cmd::SET_SCANOUT).rect(whole).u32(0).u32(resource);
        let length = r.at;
        self.call(length, cmd::OK_NODATA, HEADER, wait)?;
        self.shown = Some((resource, width, height));
        if let Some(old) = old {
            let mut r = Request {
                shared: &mut self.shared,
                at: 0,
            };
            r.header(cmd::RESOURCE_UNREF).u32(old).u32(0);
            let length = r.at;
            self.call(length, cmd::OK_NODATA, HEADER, wait)?;
        }
        Ok(())
    }

    /// Transfers `rect` of the backing to the host and shows it. A rectangle
    /// outside the resource reaches no device (INV-0090-2).
    pub fn flush(
        &mut self,
        rect: Rect,
        wait: &mut dyn FnMut() -> Result<(), Error>,
    ) -> Result<(), Error> {
        // The resource shown, at its own size: after a display event the
        // display may already be larger than it.
        let Some((resource, width, height)) = self.shown else {
            return Err(Error::NotAttached);
        };
        if !rect.inside(width, height) {
            return Err(Error::BadRange);
        }
        let offset = (u64::from(rect.y) * u64::from(width) + u64::from(rect.x)) * 4;
        let mut r = Request {
            shared: &mut self.shared,
            at: 0,
        };
        r.header(cmd::TRANSFER_TO_HOST_2D)
            .rect(rect)
            .u64(offset)
            .u32(resource)
            .u32(0);
        let length = r.at;
        self.call(length, cmd::OK_NODATA, HEADER, wait)?;
        let mut r = Request {
            shared: &mut self.shared,
            at: 0,
        };
        r.header(cmd::RESOURCE_FLUSH)
            .rect(rect)
            .u32(resource)
            .u32(0);
        let length = r.at;
        self.call(length, cmd::OK_NODATA, HEADER, wait)
    }
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();

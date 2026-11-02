//! What every virtio-mmio driver of stage 5 shares (ADR-0090, ADR-0092): the
//! transport's set-up and reset, and one split virtqueue (virtio 1.2,
//! sections 4.2.3 and 2.7). The blk driver above keeps its own queue, which is
//! older and mutation-tested as it stands.
//!
//! The device is not trusted. A used index that moves past what is
//! outstanding, or an entry that names no descriptor chain the driver
//! offered, is `Error::Device`, and the driver must reset the device.

use crate::{Error, MAGIC, Registers, Shared, reg};

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_FAILED: u32 = 128;
/// Word 1 of the features: bit 32.
const F_VERSION_1: u32 = 1;

pub const DESC_NEXT: u16 = 1;
pub const DESC_WRITE: u16 = 2;

/// How long to poll a register that the device should change at once.
const SPINS: u32 = 1_000_000;

/// Brings a transport to `FEATURES_OK` as device `device_id`, offering only
/// `VERSION_1` and the bits of `wanted` (word 0) the device offers. Gives
/// the word-0 features agreed. Set up the queues next, then call [`ready`].
pub fn begin<R: Registers>(r: &mut R, device_id: u32, wanted: u32) -> Result<u32, Error> {
    if r.read(reg::MAGIC) != MAGIC || r.read(reg::VERSION) != 2 {
        return Err(Error::WrongDevice);
    }
    if r.read(reg::DEVICE_ID) != device_id {
        return Err(Error::WrongDevice);
    }
    if !reset(r) {
        return Err(Error::Device);
    }
    r.write(reg::STATUS, STATUS_ACKNOWLEDGE);
    r.write(reg::STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
    r.write(reg::DEVICE_FEATURES_SEL, 1);
    if r.read(reg::DEVICE_FEATURES) & F_VERSION_1 == 0 {
        return Err(fail(r, Error::Negotiation));
    }
    r.write(reg::DEVICE_FEATURES_SEL, 0);
    let agreed = r.read(reg::DEVICE_FEATURES) & wanted;
    r.write(reg::DRIVER_FEATURES_SEL, 0);
    r.write(reg::DRIVER_FEATURES, agreed);
    r.write(reg::DRIVER_FEATURES_SEL, 1);
    r.write(reg::DRIVER_FEATURES, F_VERSION_1);
    let features_ok = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
    r.write(reg::STATUS, features_ok);
    if r.read(reg::STATUS) & STATUS_FEATURES_OK == 0 {
        return Err(fail(r, Error::Negotiation));
    }
    Ok(agreed)
}

/// Tells the device the driver is ready: its queues are set up.
pub fn ready<R: Registers>(r: &mut R) {
    let features_ok = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
    r.write(reg::STATUS, features_ok | STATUS_DRIVER_OK);
}

/// Stops the device: it touches no queue after this returns true.
pub fn reset<R: Registers>(r: &mut R) -> bool {
    r.write(reg::STATUS, 0);
    (0..SPINS).any(|_| r.read(reg::STATUS) == 0)
}

fn fail<R: Registers>(r: &mut R, error: Error) -> Error {
    let status = r.read(reg::STATUS);
    r.write(reg::STATUS, status | STATUS_FAILED);
    error
}

/// Where a queue of `size` descriptors lies in shared memory, from `at`:
/// descriptors, then the available ring, then the used ring, each aligned
/// as 2.7 requires (16, 2 and 4) and on a page of its own for clarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub size: u16,
    pub desc: usize,
    pub avail: usize,
    pub used: usize,
    /// The first byte after the queue.
    pub end: usize,
}

impl Layout {
    /// `size` is a power of two from 1 to 256.
    pub const fn new(size: u16, at: usize) -> Layout {
        assert!(size.is_power_of_two() && size <= 256);
        let n = size as usize;
        let desc = at.next_multiple_of(4096);
        let avail = (desc + 16 * n).next_multiple_of(4096);
        let used = (avail + 6 + 2 * n).next_multiple_of(4096);
        let end = (used + 6 + 8 * n).next_multiple_of(4096);
        Layout {
            size,
            desc,
            avail,
            used,
            end,
        }
    }
}

/// One descriptor of a chain: a device address, a length, and whether the
/// device writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Buffer {
    pub address: u64,
    pub length: u32,
    pub writable: bool,
}

/// A finished chain: its head and the bytes the device says it wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Used {
    pub head: u16,
    pub length: u32,
}

/// A split virtqueue. The caller chooses descriptor indices: a chain of `k`
/// buffers at head `h` uses `h..h + k`, which the caller keeps disjoint.
pub struct Queue {
    index: u32,
    layout: Layout,
    /// The device address of the start of shared memory.
    base: u64,
    next_avail: u16,
    last_used: u16,
    /// Heads offered and not yet returned.
    offered: [bool; 256],
    outstanding: usize,
}

impl Queue {
    pub const fn new(index: u32, layout: Layout, base: u64) -> Queue {
        Queue {
            index,
            layout,
            base,
            next_avail: 0,
            last_used: 0,
            offered: [false; 256],
            outstanding: 0,
        }
    }

    pub const fn layout(&self) -> Layout {
        self.layout
    }

    pub const fn outstanding(&self) -> usize {
        self.outstanding
    }

    /// Tells the device where the queue is, and enables it.
    pub fn set_up<R: Registers>(&self, r: &mut R) -> Result<(), Error> {
        r.write(reg::QUEUE_SEL, self.index);
        if r.read(reg::QUEUE_READY) != 0 {
            return Err(fail(r, Error::Queue));
        }
        if r.read(reg::QUEUE_NUM_MAX) < u32::from(self.layout.size) {
            return Err(fail(r, Error::Queue));
        }
        r.write(reg::QUEUE_NUM, u32::from(self.layout.size));
        let address = |at: usize| self.base + at as u64;
        write_address(r, reg::QUEUE_DESC, address(self.layout.desc));
        write_address(r, reg::QUEUE_AVAIL, address(self.layout.avail));
        write_address(r, reg::QUEUE_USED, address(self.layout.used));
        r.write(reg::QUEUE_READY, 1);
        Ok(())
    }

    /// Offers a chain at descriptor `head`, without telling the device.
    pub fn offer<S: Shared>(
        &mut self,
        shared: &mut S,
        head: u16,
        chain: &[Buffer],
    ) -> Result<(), Error> {
        let n = usize::from(self.layout.size);
        let first = usize::from(head);
        let end = first.checked_add(chain.len()).ok_or(Error::TooLarge)?;
        if chain.is_empty() || end > n {
            return Err(Error::TooLarge);
        }
        if self.offered[first] {
            return Err(Error::TooLarge);
        }
        for (i, buffer) in chain.iter().enumerate() {
            let at = self.layout.desc + 16 * (first + i);
            let last = i + 1 == chain.len();
            let mut flags = if buffer.writable { DESC_WRITE } else { 0 };
            if !last {
                flags |= DESC_NEXT;
            }
            let next = if last { 0 } else { (first + i + 1) as u16 };
            shared.write64(at, buffer.address);
            shared.write32(at + 8, buffer.length);
            shared.write16(at + 12, flags);
            shared.write16(at + 14, next);
        }
        let ring = self.layout.avail + 4 + 2 * (usize::from(self.next_avail) % n);
        shared.write16(ring, head);
        self.next_avail = self.next_avail.wrapping_add(1);
        self.offered[first] = true;
        self.outstanding += 1;
        Ok(())
    }

    /// Publishes what was offered and tells the device.
    pub fn notify<R: Registers, S: Shared>(&mut self, r: &mut R, shared: &mut S) {
        shared.publish();
        shared.write16(self.layout.avail + 2, self.next_avail);
        shared.publish();
        r.write(reg::QUEUE_NOTIFY, self.index);
    }

    /// The next finished chain, if any. Reads the used index once per call:
    /// more than is outstanding, or an entry naming no offered head, is a
    /// broken device.
    pub fn take<S: Shared>(&mut self, shared: &S) -> Result<Option<Used>, Error> {
        shared.acquire();
        let used = shared.read16(self.layout.used + 2);
        let new = used.wrapping_sub(self.last_used);
        if usize::from(new) > self.outstanding {
            return Err(Error::Device);
        }
        if new == 0 {
            return Ok(None);
        }
        // The entry was written before the index: read it after (2.7.14).
        shared.acquire();
        let n = usize::from(self.layout.size);
        let entry = self.layout.used + 4 + 8 * (usize::from(self.last_used) % n);
        let id = shared.read32(entry);
        let length = shared.read32(entry + 4);
        self.last_used = self.last_used.wrapping_add(1);
        let head = usize::try_from(id).map_err(|_| Error::Device)?;
        if head >= n || !self.offered[head] {
            return Err(Error::Device);
        }
        self.offered[head] = false;
        self.outstanding -= 1;
        Ok(Some(Used {
            head: head as u16,
            length,
        }))
    }
}

fn write_address<R: Registers>(r: &mut R, at: usize, address: u64) {
    r.write(at, address as u32);
    r.write(at + 4, (address >> 32) as u32);
}

/// One transport in a device page, as `init` read it (ADR-0091).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transport {
    /// The range index of the device object.
    pub range: u32,
    pub magic: u32,
    pub version: u32,
    pub device_id: u32,
}

/// What a device page holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// No virtio device: empty transports, or not virtio-mmio version 2.
    Empty,
    /// Exactly one device, at `range`.
    One { range: u32, device_id: u32 },
    /// More than one device: no driver gets this page (ADR-0091).
    Several,
}

/// ADR-0091: a page goes to a driver only if exactly one transport in it is
/// a virtio-mmio version 2 device. A transport with another magic or
/// version holds nothing a driver could use, and counts as empty; but a
/// transport that is virtio and not version 2 still makes the page shared.
pub fn identify(transports: &[Transport]) -> Page {
    let mut found = None;
    let mut count = 0;
    for t in transports {
        if t.magic != MAGIC || t.device_id == 0 {
            continue;
        }
        count += 1;
        if t.version == 2 {
            found = Some((t.range, t.device_id));
        }
    }
    match (count, found) {
        (0, _) => Page::Empty,
        (1, Some((range, device_id))) => Page::One { range, device_id },
        (1, None) => Page::Empty,
        _ => Page::Several,
    }
}

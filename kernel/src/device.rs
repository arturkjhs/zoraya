//! Device objects (ADR-0054): the kernel makes one per page of MMIO ranges at
//! boot, and hands them to the first process. An interrupt object masks its
//! line when it fires, until `interrupt_ack`. A pin holds a memory object for
//! a device, and when it ends without `dma_unpin` its frames stay quarantined
//! until the kernel has reset the device (ADR-0070).

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use zoraya_abi::{Status, interrupt_signals};
use zoraya_dtb::Fdt;
use zoraya_dtb::platform::{VirtioTransport, rng_seed, rtc, virtio_transports};
use zoraya_events::SourceId;
use zoraya_pmm::FRAME_SIZE;
use zoraya_time::Instant;

use crate::memory::heap::uncharged_arc;
use crate::memory::object::MemoryObject;
use crate::memory::paging;
use crate::sync::SpinLock;
use crate::{gic, irq, port, println, timer};

/// Transports in a page: QEMU's are 0x200 apart.
const MAX_RANGES: usize = 8;
const VIRTIO_MAGIC: u32 = 0x7472_6976;
const REG_MAGIC: usize = 0x00;
const REG_VERSION: usize = 0x04;
const REG_STATUS: usize = 0x70;
/// Where a virtio-mmio transport's configuration starts.
const CONFIG: u64 = 0x100;
/// A transport's register block (virtio-mmio): the bound of `read_register`
/// and `write_register` (ADR-0295).
const TRANSPORT_SIZE: u64 = 0x200;
/// How long a reset may take (ADR-0070).
const RESET_NANOS: u64 = 100_000_000;

#[derive(Clone, Copy)]
struct Range {
    /// From the start of the window.
    offset: u64,
    intid: u32,
    edge: bool,
    /// The bytes the transport's registers take from `offset`: the device
    /// tree's size, never past `TRANSPORT_SIZE` (ADR-0295).
    size: u64,
}

#[derive(Default)]
struct State {
    /// A process maps the window: at most one (ADR-0054).
    mapped: bool,
    /// A transport device's holder: a process that wrote its registers and
    /// still holds a handle that can (ADR-0295, INV-0295-3). At most one,
    /// and never while the page is mapped: either could restart it.
    claimed: bool,
    live_pins: usize,
    /// Objects a device may still write. Each is space reserved in advance by
    /// `Pin::new`, so ending a pin cannot fail.
    quarantine: Vec<Arc<MemoryObject>>,
}

pub struct Device {
    /// Physical address of the page.
    base: u64,
    /// The kernel's own mapping of the page, for the reset.
    kernel: usize,
    ranges: [Option<Range>; MAX_RANGES],
    /// One device per transport of the page, reached by register calls only
    /// (ADR-0295). Empty for a transport device itself.
    transports: [Option<Arc<Device>>; MAX_RANGES],
    /// A transport device: it has no mapping, `ranges[0]` is its transport,
    /// and `read_register` and `write_register` work on it alone.
    by_call: bool,
    /// A transport device's page (empty for a page). Devices live as long
    /// as `DEVICES`, so it always upgrades; weak, so the two do not hold
    /// each other.
    page: Weak<Device>,
    state: SpinLock<State>,
}

static DEVICES: SpinLock<Vec<Arc<Device>>> = SpinLock::new(Vec::new());
/// The real-time clock's page, if the device tree has a PL031 (ADR-0226).
static RTC: SpinLock<Option<Arc<Device>>> = SpinLock::new(None);
/// The boot's entropy seed (ADR-0227 item 1): a memory object holding
/// `/chosen/rng-seed` as a length (u32, little-endian) and the bytes.
static SEED: SpinLock<Option<Arc<MemoryObject>>> = SpinLock::new(None);
/// The interrupts objects own, by line.
static LINES: SpinLock<Vec<(u32, SourceId)>> = SpinLock::new(Vec::new());

/// Finds the devices. Fixed at boot, so a boot-time allocation (ADR-0022).
pub fn init(fdt: &Fdt<'_>) {
    let mut transports: Vec<VirtioTransport> = virtio_transports(fdt).collect();
    transports.sort_unstable_by_key(|t| t.base);
    let mut devices: Vec<Device> = Vec::new();
    for transport in transports {
        let base = transport.base & !(FRAME_SIZE - 1);
        // Every register must lie in the window: `read` and `write` rely on it.
        let end = transport
            .base
            .checked_add(transport.size.max(REG_STATUS as u64 + 4));
        if !transport.base.is_multiple_of(4) || end.is_none_or(|end| end > base + FRAME_SIZE) {
            println!(
                "device: {:#x} does not fit a page, left out",
                transport.base
            );
            continue;
        }
        let range = Range {
            offset: transport.base - base,
            intid: transport.interrupt.intid,
            edge: transport.interrupt.edge,
            size: transport.size.clamp(REG_STATUS as u64 + 4, TRANSPORT_SIZE),
        };
        if let Some(device) = devices.iter_mut().find(|d| d.base == base) {
            match place(&mut device.ranges, range) {
                Ok(()) => {}
                Err(LeftOut::Overlaps) => println!(
                    "device: {:#x} overlaps the transport before it, left out",
                    transport.base
                ),
                Err(LeftOut::Full) => println!(
                    "device: {:#x} is past {MAX_RANGES} in a page, left out",
                    transport.base
                ),
            }
            continue;
        }
        let mut ranges = [None; MAX_RANGES];
        ranges[0] = Some(range);
        devices.push(Device {
            base,
            kernel: paging::map_device(base, FRAME_SIZE) as usize,
            ranges,
            transports: Default::default(),
            by_call: false,
            page: Weak::new(),
            state: SpinLock::new(State::default()),
        });
    }
    *DEVICES.lock() = devices
        .into_iter()
        .map(|mut device| {
            Arc::new_cyclic(move |page| {
                device.make_transports(page);
                device
            })
        })
        .collect();
    init_rtc(fdt);
    init_seed(fdt);
}

/// Whether `range` may follow `last` in a page: their blocks do not
/// overlap, so no transport device reaches a neighbour's registers
/// (INV-0295-1).
fn follows(last: &Range, range: &Range) -> bool {
    last.offset
        .checked_add(last.size)
        .is_some_and(|end| end <= range.offset)
}

/// Why `init` left a transport out of its page.
enum LeftOut {
    /// Its block overlaps the one before it (INV-0295-1).
    Overlaps,
    /// The page has [`MAX_RANGES`] transports already.
    Full,
}

/// Adds `range` to a page's `ranges`, which `init` fills in order of
/// offset, so the page's last range is the one before it.
fn place(ranges: &mut [Option<Range>; MAX_RANGES], range: Range) -> Result<(), LeftOut> {
    let last = ranges.iter().flatten().last();
    if last.is_some_and(|last| !follows(last, &range)) {
        return Err(LeftOut::Overlaps);
    }
    let slot = ranges
        .iter_mut()
        .find(|r| r.is_none())
        .ok_or(LeftOut::Full)?;
    *slot = Some(range);
    Ok(())
}

/// What `init` keeps of a page's transports, given as (offset, size)
/// blocks in order of offset, the first already placed as `init` places
/// it: bit `i` is set if block `i` was kept. For the test kernel.
#[cfg(feature = "qemu-test")]
pub fn blocks_kept(blocks: &[(u64, u64)]) -> u32 {
    let range = |&(offset, size): &(u64, u64)| Range {
        offset,
        intid: 0,
        edge: false,
        size,
    };
    let mut ranges = [None; MAX_RANGES];
    let mut kept = 0;
    for (i, block) in blocks.iter().enumerate() {
        if i == 0 {
            ranges[0] = Some(range(block));
            kept |= 1;
        } else if place(&mut ranges, range(block)).is_ok() {
            kept |= 1 << i;
        }
    }
    kept
}

/// [`follows`] for blocks at `a` and `b` of the given sizes, for the test
/// kernel.
#[cfg(feature = "qemu-test")]
pub fn blocks_follow(a: (u64, u64), b: (u64, u64)) -> bool {
    let range = |(offset, size)| Range {
        offset,
        intid: 0,
        edge: false,
        size,
    };
    follows(&range(a), &range(b))
}

/// Copies the device tree's seed into a memory object for `init`. The
/// boot loader's copy stays in the device tree blob, which only the kernel
/// maps (a limitation recorded in ADR-0287's note on the seed).
fn init_seed(fdt: &Fdt<'_>) {
    let Some(seed) = rng_seed(fdt) else {
        println!("device: no /chosen/rng-seed in the device tree");
        return;
    };
    let seed = &seed[..seed.len().min(FRAME_SIZE as usize - 4)];
    let Ok(object) = MemoryObject::new(FRAME_SIZE, false, None) else {
        println!("device: no memory for the seed");
        return;
    };
    let wrote = object.write(0, &(seed.len() as u32).to_le_bytes()) && object.write(4, seed);
    if wrote {
        println!("device: entropy seed of {} bytes", seed.len());
        *SEED.lock() = Some(object);
    }
}

/// The seed's memory object, if the device tree had one, handed on once:
/// the kernel keeps no reference, so the object and its frame live only as
/// long as `init`'s and then `rngd`'s handle.
pub fn seed_object() -> Option<Arc<MemoryObject>> {
    SEED.lock().take()
}

/// The PL031's page as a device with one range and no interrupt line (its
/// `intid` is 0, which `interrupt` refuses): `init` maps it and reads
/// `RTC_DR` (ADR-0226 item 2).
fn init_rtc(fdt: &Fdt<'_>) {
    let Some(clock) = rtc(fdt) else { return };
    let base = clock.base & !(FRAME_SIZE - 1);
    let offset = clock.base - base;
    if !clock.base.is_multiple_of(4)
        || offset
            .checked_add(clock.size)
            .is_none_or(|end| end > FRAME_SIZE)
    {
        println!("device: the rtc at {:#x} does not fit a page", clock.base);
        return;
    }
    let mut ranges = [None; MAX_RANGES];
    ranges[0] = Some(Range {
        offset,
        intid: 0,
        edge: false,
        size: clock.size.min(TRANSPORT_SIZE),
    });
    *RTC.lock() = Some(Arc::new(Device {
        base,
        kernel: paging::map_device(base, FRAME_SIZE) as usize,
        ranges,
        transports: Default::default(),
        by_call: false,
        page: Weak::new(),
        state: SpinLock::new(State::default()),
    }));
}

/// The real-time clock's device, if there is one.
pub fn rtc_device() -> Option<Arc<Device>> {
    RTC.lock().clone()
}

/// Every device.
pub fn all() -> Vec<Arc<Device>> {
    DEVICES.lock().clone()
}

/// The first object's line, or 0.
#[cfg(feature = "qemu-test")]
pub fn first_line() -> u32 {
    LINES.lock().first().map_or(0, |(intid, _)| *intid)
}

/// Makes the first object's line pending, as its device could.
#[cfg(feature = "qemu-test")]
pub fn trigger_first_line() {
    if let Some((intid, _)) = LINES.lock().first() {
        gic::trigger(*intid);
    }
}

/// A device with no virtio transport, for tests of ADR-0070.
#[cfg(feature = "qemu-test")]
pub fn without_transport(base: u64, kernel: usize) -> Arc<Device> {
    Arc::new(Device {
        base,
        kernel,
        ranges: [None; MAX_RANGES],
        transports: Default::default(),
        by_call: false,
        page: Weak::new(),
        state: SpinLock::new(State::default()),
    })
}

impl Device {
    pub fn base(&self) -> u64 {
        self.base
    }

    /// Makes the transport devices of a page just built (a boot-time
    /// allocation, ADR-0022). Each shares the page's kernel mapping and has
    /// its own state, so pins, quarantine and reset are per transport; each
    /// knows its page, so that a holder of one is a holder of the other
    /// (INV-0295-3).
    fn make_transports(&mut self, page: &Weak<Device>) {
        for (slot, range) in self.transports.iter_mut().zip(self.ranges) {
            let Some(range) = range else { continue };
            let mut ranges = [None; MAX_RANGES];
            ranges[0] = Some(range);
            *slot = Some(Arc::new(Device {
                base: self.base,
                kernel: self.kernel,
                ranges,
                transports: Default::default(),
                by_call: true,
                page: page.clone(),
                state: SpinLock::new(State::default()),
            }));
        }
    }

    /// The device for transport `index` of this page (ADR-0295): no mapping,
    /// register calls only. A transport device has none of its own.
    pub fn transport(&self, index: u32) -> Result<Arc<Device>, Status> {
        if self.by_call {
            return Err(Status::AccessDenied);
        }
        self.transports
            .get(index as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(Status::InvalidArgument)
    }

    /// The transport's register at `offset`, an access of `width` bytes
    /// (1, 2 or 4), for a transport device. The offset is the caller's: it
    /// is checked to be aligned to the width and inside the transport's
    /// block, so no other transport of the page is reached.
    pub fn read_register(&self, offset: u64, width: u64) -> Result<u32, Status> {
        let at = self.register_offset(offset, width)?;
        Ok(self.read_sized(at, width))
    }

    /// As [`Device::read_register`], for a write. The caller holds the
    /// transport ([`Device::claim`]).
    pub fn write_register(
        &self,
        held: &DeviceClaim,
        offset: u64,
        width: u64,
        value: u32,
    ) -> Result<(), Status> {
        if !core::ptr::eq(Arc::as_ptr(&held.0), self) {
            return Err(Status::BadState);
        }
        let at = self.register_offset(offset, width)?;
        self.write_sized(at, width, value);
        Ok(())
    }

    /// Whether an access of `width` at `offset` is one of this transport
    /// device's.
    pub fn check_register(&self, offset: u64, width: u64) -> Result<(), Status> {
        self.register_offset(offset, width).map(|_| ())
    }

    /// Makes the caller the transport device's holder (INV-0295-3): refused
    /// with `BadState` while another holds it or a process maps its page,
    /// either of which could restart the device. Lock order: the page's
    /// state, then the transport's.
    pub fn claim(self: &Arc<Self>) -> Result<DeviceClaim, Status> {
        if !self.by_call {
            return Err(Status::AccessDenied);
        }
        let page = self.page.upgrade().ok_or(Status::BadState)?;
        let page_state = page.state.lock();
        let mut state = self.state.lock();
        if page_state.mapped || state.claimed {
            return Err(Status::BadState);
        }
        state.claimed = true;
        drop(state);
        drop(page_state);
        Ok(DeviceClaim(self.clone()))
    }

    /// Whether a transport device of this page has a holder or a live pin.
    /// The caller holds the page's state (the lock order of `claim`).
    fn transports_busy(&self) -> bool {
        self.transports.iter().flatten().any(|t| {
            let state = t.state.lock();
            state.claimed || state.live_pins != 0
        })
    }

    /// The offset in the page of an access of `width` at `offset` of this
    /// transport.
    fn register_offset(&self, offset: u64, width: u64) -> Result<usize, Status> {
        if !self.by_call {
            return Err(Status::AccessDenied);
        }
        let range = self.ranges[0].ok_or(Status::BadState)?;
        // Narrower accesses are for the configuration only: the control
        // registers below it take 32-bit ones (virtio 1.2 4.2.2.2).
        if !matches!(width, 1 | 2 | 4)
            || (width != 4 && offset < CONFIG)
            || !offset.is_multiple_of(width)
            || offset.checked_add(width).is_none_or(|end| end > range.size)
        {
            return Err(Status::InvalidArgument);
        }
        // `init` checked that the transport's block lies in the page, and
        // `range.size` is at most that block.
        Ok((range.offset + offset) as usize)
    }

    /// Range `index`'s offset in the window.
    pub fn offset(&self, index: u32) -> Option<u64> {
        let range = (*self.ranges.get(index as usize)?)?;
        Some(range.offset)
    }

    /// Takes the one mapping.
    pub fn begin_mapping(self: &Arc<Self>) -> Result<DeviceMapping, Status> {
        if self.by_call {
            return Err(Status::AccessDenied);
        }
        let mut state = self.state.lock();
        // A transport's holder or live pin is the page's (INV-0295-3).
        if state.mapped || self.transports_busy() {
            return Err(Status::BadState);
        }
        state.mapped = true;
        Ok(DeviceMapping(self.clone()))
    }

    pub fn interrupt(self: &Arc<Self>, index: u32) -> Result<Arc<Interrupt>, Status> {
        let range = self
            .ranges
            .get(index as usize)
            .copied()
            .flatten()
            .filter(|range| range.intid != 0)
            .ok_or(Status::InvalidArgument)?;
        let mut lines = LINES.lock();
        if lines.iter().any(|(intid, _)| *intid == range.intid) {
            return Err(Status::BadState);
        }
        lines.try_reserve(1).map_err(|_| Status::NoMemory)?;
        let source = port::new_source(0)?;
        // On failure the value was dropped, and its `Drop` frees the source
        // alone: the line is taken only after this.
        // uncharged: device objects have no paying job (ADR-0261 point 6)
        let interrupt = uncharged_arc(Interrupt {
            intid: range.intid,
            source,
            active: false,
        });
        let mut interrupt = interrupt.map_err(|_| Status::NoMemory)?;
        if let Some(i) = Arc::get_mut(&mut interrupt) {
            i.active = true;
        }
        lines.push((range.intid, source));
        drop(lines);
        irq::register(range.intid, range.edge, line_fired);
        Ok(interrupt)
    }

    fn register(&self, offset: usize) -> *mut u32 {
        (self.kernel + offset) as *mut u32
    }

    fn read(&self, offset: usize) -> u32 {
        // SAFETY: `kernel` maps this device's whole page as Device memory, and
        // `offset` is an aligned offset inside it.
        unsafe { self.register(offset).read_volatile() }
    }

    fn write(&self, offset: usize, value: u32) {
        // SAFETY: as in `read`.
        unsafe { self.register(offset).write_volatile(value) }
    }

    /// An access of `width` bytes (1, 2 or 4) at `offset`, aligned to it.
    fn read_sized(&self, offset: usize, width: u64) -> u32 {
        let at = self.kernel + offset;
        match width {
            // SAFETY: `kernel` maps this device's whole page as Device
            // memory, and `offset` is inside it and aligned to the width
            // (`register_offset`).
            1 => u32::from(unsafe { (at as *const u8).read_volatile() }),
            // SAFETY: as above.
            2 => u32::from(unsafe { (at as *const u16).read_volatile() }),
            _ => self.read(offset),
        }
    }

    /// As [`Device::read_sized`], for a write of `value`, which fits `width`.
    fn write_sized(&self, offset: usize, width: u64, value: u32) {
        let at = self.kernel + offset;
        match width {
            // SAFETY: as in `read_sized`.
            1 => unsafe { (at as *mut u8).write_volatile(value as u8) },
            // SAFETY: as in `read_sized`.
            2 => unsafe { (at as *mut u16).write_volatile(value as u16) },
            _ => self.write(offset, value),
        }
    }

    /// Stops every virtio transport in the window and sees it stopped. An
    /// error if there is none, or one does not stop (ADR-0070).
    fn reset(&self) -> Result<(), Status> {
        let mut any = false;
        for range in self.ranges.iter().flatten() {
            let at = range.offset as usize;
            if self.read(at + REG_MAGIC) != VIRTIO_MAGIC || self.read(at + REG_VERSION) == 0 {
                continue;
            }
            any = true;
            self.write(at + REG_STATUS, 0);
            let deadline: Instant = timer::now().plus_nanos(RESET_NANOS);
            while self.read(at + REG_STATUS) != 0 {
                if timer::now() >= deadline {
                    return Err(Status::DeadlineExceeded);
                }
            }
        }
        if any { Ok(()) } else { Err(Status::BadState) }
    }

    /// Ends the quarantine, if that is safe (ADR-0070): with no mapping no
    /// process can restart the device, and with no live pin nothing is using
    /// it, so a reset the kernel sees finishes it. A page and its transport
    /// devices are one device here (INV-0295-3): a transport's release waits
    /// for no mapping of its page and no holder of its own; a page's for no
    /// holder or live pin of any of its transports. Lock order: the page's
    /// state, then a transport's, held across the reset.
    pub fn release_quarantine(&self) -> Result<(), Status> {
        let page = if self.by_call {
            Some(self.page.upgrade().ok_or(Status::BadState)?)
        } else {
            None
        };
        let page_state = page.as_ref().map(|p| p.state.lock());
        if page_state.as_ref().is_some_and(|s| s.mapped) {
            return Err(Status::BadState);
        }
        let mut state = self.state.lock();
        if state.mapped || state.claimed || state.live_pins != 0 || self.transports_busy() {
            return Err(Status::BadState);
        }
        self.reset()?;
        let held = core::mem::take(&mut state.quarantine);
        drop(state);
        drop(page_state);
        drop(held);
        Ok(())
    }

    /// Objects held in quarantine.
    #[cfg(feature = "qemu-test")]
    pub fn quarantined(&self) -> usize {
        self.state.lock().quarantine.len()
    }
}

/// A process's mapping of a device's window.
pub struct DeviceMapping(Arc<Device>);

impl Drop for DeviceMapping {
    fn drop(&mut self) {
        self.0.state.lock().mapped = false;
    }
}

/// A process's hold of a transport device (INV-0295-3), kept in its
/// address space: it ends when the process holds no handle that can write
/// the transport, or ends.
pub struct DeviceClaim(Arc<Device>);

impl DeviceClaim {
    pub fn device(&self) -> &Arc<Device> {
        &self.0
    }
}

impl Drop for DeviceClaim {
    fn drop(&mut self) {
        self.0.state.lock().claimed = false;
    }
}

/// A device's interrupt line, for one holder.
pub struct Interrupt {
    intid: u32,
    source: SourceId,
    active: bool,
}

/// A line fired: mask it and tell the holder. The driver asks for the next
/// with `interrupt_ack`, so a stuck driver cannot storm the kernel.
fn line_fired() {
    let intid = irq::current();
    gic::disable(intid);
    let lines = LINES.lock();
    if let Some((_, source)) = lines.iter().find(|(line, _)| *line == intid) {
        port::set_signals(*source, interrupt_signals::INTERRUPT);
    }
}

impl Interrupt {
    pub fn source(&self) -> SourceId {
        self.source
    }

    pub fn ack(&self) {
        port::clear_signals(self.source, interrupt_signals::INTERRUPT);
        gic::unmask(self.intid);
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        // Masked first: a device not yet reset could otherwise raise a line
        // with no handler, which ADR-0024 makes a panic.
        if self.active {
            irq::unregister(self.intid);
            LINES.lock().retain(|(line, _)| *line != self.intid);
        }
        port::free_source(self.source);
    }
}

/// A memory object held for a device.
pub struct Pin {
    device: Arc<Device>,
    memory: Arc<MemoryObject>,
    /// It reached the device's count, which a failed `new` did not.
    live: bool,
    unpinned: AtomicBool,
}

impl Pin {
    pub fn new(device: Arc<Device>, memory: Arc<MemoryObject>) -> Result<Arc<Pin>, Status> {
        {
            let mut state = device.state.lock();
            let room = state.live_pins + 1;
            state
                .quarantine
                .try_reserve(room)
                .map_err(|_| Status::NoMemory)?;
        }
        // uncharged: pins are made by driver processes for their devices (ADR-0261 point 6)
        let mut pin = uncharged_arc(Pin {
            device: device.clone(),
            memory,
            live: false,
            unpinned: AtomicBool::new(false),
        })
        .map_err(|_| Status::NoMemory)?;
        if let Some(pin) = Arc::get_mut(&mut pin) {
            pin.live = true;
        }
        device.state.lock().live_pins += 1;
        Ok(pin)
    }

    /// The caller says the device has done with these frames.
    pub fn unpin(&self) {
        self.unpinned.store(true, Ordering::Relaxed);
    }
}

impl Drop for Pin {
    fn drop(&mut self) {
        if !self.live {
            return;
        }
        let mut state = self.device.state.lock();
        state.live_pins -= 1;
        if !self.unpinned.load(Ordering::Relaxed) {
            // `Pin::new` reserved this room, so this does not allocate.
            state.quarantine.push(self.memory.clone());
        }
    }
}

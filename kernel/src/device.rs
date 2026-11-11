//! Device objects (ADR-0054): the kernel makes one per page of MMIO ranges at
//! boot, and hands them to the first process. An interrupt object masks its
//! line when it fires, until `interrupt_ack`. A pin holds a memory object for
//! a device, and when it ends without `dma_unpin` its frames stay quarantined
//! until the kernel has reset the device (ADR-0070).

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use zoraya_abi::{Status, interrupt_signals};
use zoraya_dtb::Fdt;
use zoraya_dtb::platform::{VirtioTransport, virtio_transports};
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
/// How long a reset may take (ADR-0070).
const RESET_NANOS: u64 = 100_000_000;

#[derive(Clone, Copy)]
struct Range {
    /// From the start of the window.
    offset: u64,
    intid: u32,
    edge: bool,
}

#[derive(Default)]
struct State {
    /// A process maps the window: at most one (ADR-0054).
    mapped: bool,
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
    state: SpinLock<State>,
}

static DEVICES: SpinLock<Vec<Arc<Device>>> = SpinLock::new(Vec::new());
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
        };
        if let Some(device) = devices.iter_mut().find(|d| d.base == base) {
            match device.ranges.iter_mut().find(|r| r.is_none()) {
                Some(slot) => *slot = Some(range),
                None => println!(
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
            state: SpinLock::new(State::default()),
        });
    }
    *DEVICES.lock() = devices.into_iter().map(Arc::new).collect();
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
        state: SpinLock::new(State::default()),
    })
}

impl Device {
    pub fn base(&self) -> u64 {
        self.base
    }

    /// Range `index`'s offset in the window.
    pub fn offset(&self, index: u32) -> Option<u64> {
        let range = (*self.ranges.get(index as usize)?)?;
        Some(range.offset)
    }

    /// Takes the one mapping.
    pub fn begin_mapping(self: &Arc<Self>) -> Result<DeviceMapping, Status> {
        let mut state = self.state.lock();
        if state.mapped {
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
            .ok_or(Status::InvalidArgument)?;
        let mut lines = LINES.lock();
        if lines.iter().any(|(intid, _)| *intid == range.intid) {
            return Err(Status::BadState);
        }
        lines.try_reserve(1).map_err(|_| Status::NoMemory)?;
        let source = port::new_source(0)?;
        // On failure the value was dropped, and its `Drop` frees the source
        // alone: the line is taken only after this.
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
    /// it, so a reset the kernel sees finishes it.
    pub fn release_quarantine(&self) -> Result<(), Status> {
        let mut state = self.state.lock();
        if state.mapped || state.live_pins != 0 {
            return Err(Status::BadState);
        }
        self.reset()?;
        let held = core::mem::take(&mut state.quarantine);
        drop(state);
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

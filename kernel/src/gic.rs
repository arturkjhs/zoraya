//! The interrupt controller: GICv2 or GICv3, as the device tree says
//! (ADR-0023). Register offsets and sequences are from the GIC Architecture
//! Specification (ARM IHI 0069).
//!
//! One CPU, so only that CPU's interfaces are set up. Every interrupt has the
//! same priority, and handlers run with IRQ masked, so nothing nests.
//!
//! GICv2 stays in group 0, the only group a GIC without security extensions
//! has, and is signalled as IRQ. GICv3 uses group 1 with affinity routing and
//! the system register interface; there group 0 would be signalled as FIQ.

use core::ptr;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use zoraya_dtb::platform::{GicKind, InterruptController};

use crate::arch;
use crate::memory::paging::map_device;

/// First interrupt ID of the shared peripheral interrupts.
const FIRST_SPI: u32 = 32;
/// Interrupt IDs 1020 to 1023 are special: no interrupt to handle.
pub const FIRST_SPECIAL: u32 = 1020;
/// All interrupts run at this priority; 0xff (the mask below) masks nothing
/// above it.
const PRIORITY: u8 = 0xa0;
const PRIORITY_MASK: u32 = 0xff;

// Distributor, both versions.
const GICD_CTLR: usize = 0x0000;
const GICD_TYPER: usize = 0x0004;
const GICD_ISENABLER: usize = 0x0100;
const GICD_ICENABLER: usize = 0x0180;
#[cfg(feature = "qemu-test")]
const GICD_ISPENDR: usize = 0x0200;
const GICD_ICPENDR: usize = 0x0280;
const GICD_IPRIORITYR: usize = 0x0400;
const GICD_ICFGR: usize = 0x0c00;
// GICv2 only.
const GICD_ITARGETSR: usize = 0x0800;
const GICC_CTLR: usize = 0x0000;
const GICC_PMR: usize = 0x0004;
const GICC_BPR: usize = 0x0008;
const GICC_IAR: usize = 0x000c;
const GICC_EOIR: usize = 0x0010;
// GICv3 only.
const GICD_IGROUPR: usize = 0x0080;
const GICD_IROUTER: usize = 0x6000;
const GICD_CTLR_ENABLE_ALL: u32 = 0b11;
const GICD_CTLR_ARE: u32 = 1 << 4;
const GICD_CTLR_RWP: u32 = 1 << 31;
const GICR_TYPER: usize = 0x0008;
const GICR_WAKER: usize = 0x0014;
const GICR_TYPER_VLPIS: u64 = 1 << 1;
const GICR_TYPER_LAST: u64 = 1 << 4;
const GICR_WAKER_PROCESSOR_SLEEP: u32 = 1 << 1;
const GICR_WAKER_CHILDREN_ASLEEP: u32 = 1 << 2;
/// Offset of the SGI and PPI registers from a redistributor's base.
const GICR_SGI_FRAME: usize = 0x1_0000;
const GICR_FRAME: usize = 0x2_0000;
const GICR_VLPI_FRAMES: usize = 0x2_0000;

const VERSION_UNSET: u8 = 0;
const VERSION_2: u8 = 2;
const VERSION_3: u8 = 3;

static VERSION: AtomicU8 = AtomicU8::new(VERSION_UNSET);
static DISTRIBUTOR: AtomicUsize = AtomicUsize::new(0);
/// GICv2: the CPU interface. GICv3: this CPU's redistributor.
static CPU: AtomicUsize = AtomicUsize::new(0);

fn read32(base: usize, offset: usize) -> u32 {
    // SAFETY: `base` is a GIC register block mapped as Device memory by
    // `init`, `offset` a register in it, accessed with its natural size.
    unsafe { ptr::read_volatile((base + offset) as *const u32) }
}

fn write32(base: usize, offset: usize, value: u32) {
    // SAFETY: as in `read32`.
    unsafe { ptr::write_volatile((base + offset) as *mut u32, value) }
}

fn read64(base: usize, offset: usize) -> u64 {
    // SAFETY: as in `read32`, for a 64-bit register.
    unsafe { ptr::read_volatile((base + offset) as *const u64) }
}

fn write64(base: usize, offset: usize, value: u64) {
    // SAFETY: as in `read32`, for a 64-bit register.
    unsafe { ptr::write_volatile((base + offset) as *mut u64, value) }
}

fn dist() -> usize {
    DISTRIBUTOR.load(Ordering::Relaxed)
}

fn cpu() -> usize {
    CPU.load(Ordering::Relaxed)
}

fn is_v3() -> bool {
    VERSION.load(Ordering::Relaxed) == VERSION_3
}

/// The version found, for the banner.
pub fn version() -> u8 {
    VERSION.load(Ordering::Relaxed)
}

/// The distributor's virtual address.
#[cfg(feature = "qemu-test")]
pub fn distributor() -> usize {
    dist()
}

/// Number of usable interrupt IDs. `GICD_TYPER` counts in steps of 32, and
/// its largest value includes the special IDs, which have no registers.
fn lines() -> u32 {
    (((read32(dist(), GICD_TYPER) & 0x1f) + 1) * 32).min(FIRST_SPECIAL)
}

/// The same bit in a register array with one bit per interrupt.
fn bit_register(array: usize, intid: u32) -> (usize, u32) {
    (array + (intid / 32) as usize * 4, 1 << (intid % 32))
}

/// Maps the controller, sets it up and enables this CPU's interface. Every
/// interrupt starts disabled. IRQ stays masked at the CPU.
pub fn init(controller: &InterruptController<'_>) {
    let regs = controller.regs();
    assert!(regs.len() >= 2, "GIC needs two register blocks");
    let dist = map_device(regs[0].address, regs[0].size) as usize;
    let second = map_device(regs[1].address, regs[1].size) as usize;
    DISTRIBUTOR.store(dist, Ordering::Relaxed);
    match controller.kind {
        GicKind::V2 => {
            VERSION.store(VERSION_2, Ordering::Relaxed);
            CPU.store(second, Ordering::Relaxed);
            init_v2();
        }
        GicKind::V3 => {
            VERSION.store(VERSION_3, Ordering::Relaxed);
            CPU.store(
                find_redistributor(second, regs[1].size as usize),
                Ordering::Relaxed,
            );
            init_v3();
        }
        GicKind::Other => panic!("unsupported interrupt controller {}", controller.node_name),
    }
}

/// Disables every shared interrupt and gives it the common priority.
fn reset_spis() {
    let lines = lines();
    for n in (FIRST_SPI..lines).step_by(32) {
        write32(dist(), GICD_ICENABLER + (n / 32) as usize * 4, !0);
        write32(dist(), GICD_ICPENDR + (n / 32) as usize * 4, !0);
    }
    for intid in FIRST_SPI..lines {
        write8_priority(dist() + GICD_IPRIORITYR, intid);
    }
}

fn write8_priority(array: usize, intid: u32) {
    // SAFETY: the priority registers take byte accesses; `array` is a mapped
    // priority array, and the ID is one the GIC implements.
    unsafe { ptr::write_volatile((array + intid as usize) as *mut u8, PRIORITY) }
}

fn init_v2() {
    write32(dist(), GICD_CTLR, 0);
    reset_spis();
    // GICD_ITARGETSR0 reads back this CPU's own target bit, in each byte. In
    // a uniprocessor GIC it, and all the others, read as zero and ignore
    // writes, since there is only one target.
    let me = read32(dist(), GICD_ITARGETSR) & 0xff;
    for intid in (FIRST_SPI..lines()).step_by(4) {
        write32(dist(), GICD_ITARGETSR + intid as usize, me * 0x0101_0101);
    }
    // SGIs and PPIs are banked per CPU.
    write32(dist(), GICD_ICENABLER, !0);
    write32(dist(), GICD_ICPENDR, !0);
    for intid in 0..FIRST_SPI {
        write8_priority(dist() + GICD_IPRIORITYR, intid);
    }
    write32(dist(), GICD_CTLR, 1);
    write32(cpu(), GICC_PMR, PRIORITY_MASK);
    write32(cpu(), GICC_BPR, 0);
    write32(cpu(), GICC_CTLR, 1);
}

fn wait_for_distributor() {
    while read32(dist(), GICD_CTLR) & GICD_CTLR_RWP != 0 {
        core::hint::spin_loop();
    }
}

/// Affinity of this CPU in the layout of `GICR_TYPER` bits 63:32.
fn affinity() -> u32 {
    let m = arch::mpidr();
    (((m >> 32) & 0xff) << 24) as u32 | (m & 0xff_ffff) as u32
}

/// This CPU's redistributor in the region at `base`.
fn find_redistributor(base: usize, size: usize) -> usize {
    let mut at = base;
    while at < base + size {
        let typer = read64(at, GICR_TYPER);
        if (typer >> 32) as u32 == affinity() {
            return at;
        }
        if typer & GICR_TYPER_LAST != 0 {
            break;
        }
        at += GICR_FRAME;
        if typer & GICR_TYPER_VLPIS != 0 {
            at += GICR_VLPI_FRAMES;
        }
    }
    panic!("no GICv3 redistributor for affinity {:#x}", affinity());
}

fn init_v3() {
    // Affinity routing first; the enable bits are written once it is on.
    write32(dist(), GICD_CTLR, 0);
    wait_for_distributor();
    write32(dist(), GICD_CTLR, GICD_CTLR_ARE);
    wait_for_distributor();
    reset_spis();
    wait_for_distributor();
    let lines = lines();
    let route = (arch::mpidr() & 0xff_00ff_ffff) & !(1 << 31);
    for intid in FIRST_SPI..lines {
        write64(dist(), GICD_IROUTER + intid as usize * 8, route);
    }
    for n in (FIRST_SPI..lines).step_by(32) {
        write32(dist(), GICD_IGROUPR + (n / 32) as usize * 4, !0);
    }
    write32(dist(), GICD_CTLR, GICD_CTLR_ARE | GICD_CTLR_ENABLE_ALL);
    wait_for_distributor();

    // Wake the redistributor.
    let waker = read32(cpu(), GICR_WAKER);
    write32(cpu(), GICR_WAKER, waker & !GICR_WAKER_PROCESSOR_SLEEP);
    while read32(cpu(), GICR_WAKER) & GICR_WAKER_CHILDREN_ASLEEP != 0 {
        core::hint::spin_loop();
    }
    let sgi = cpu() + GICR_SGI_FRAME;
    write32(sgi, GICD_ICENABLER, !0);
    write32(sgi, GICD_ICPENDR, !0);
    write32(sgi, GICD_IGROUPR, !0);
    for intid in 0..FIRST_SPI {
        write8_priority(sgi + GICD_IPRIORITYR, intid);
    }
    arch::gic_v3_cpu_interface_on(PRIORITY_MASK.into());
}

/// The register block holding `intid`'s per-interrupt registers: SGIs and
/// PPIs are in the redistributor on GICv3.
fn block(intid: u32) -> usize {
    if is_v3() && intid < FIRST_SPI {
        cpu() + GICR_SGI_FRAME
    } else {
        dist()
    }
}

/// Enables an interrupt, level or edge triggered as `edge` says. SGIs (below
/// 16) are always edge-triggered.
pub fn enable(intid: u32, edge: bool) {
    if intid >= 16 {
        let field = 2 * (intid % 16);
        let offset = GICD_ICFGR + (intid / 16) as usize * 4;
        let current = read32(block(intid), offset);
        let config = (current & !(0b11 << field)) | (u32::from(edge) << (field + 1));
        write32(block(intid), offset, config);
    }
    let (offset, mask) = bit_register(GICD_ISENABLER, intid);
    write32(block(intid), offset, mask);
}

/// Disables an interrupt.
pub fn disable(intid: u32) {
    let (offset, mask) = bit_register(GICD_ICENABLER, intid);
    write32(block(intid), offset, mask);
}

/// Number of interrupt IDs the distributor implements, read through its
/// mapping.
#[cfg(feature = "qemu-test")]
pub fn line_count() -> u32 {
    lines()
}

/// Makes an enabled interrupt pending, as if its device had raised it.
#[cfg(feature = "qemu-test")]
pub fn trigger(intid: u32) {
    let (offset, mask) = bit_register(GICD_ISPENDR, intid);
    write32(block(intid), offset, mask);
}

/// Takes the highest-priority pending interrupt, returning its ID: at or
/// above [`FIRST_SPECIAL`] if there is none.
pub fn acknowledge() -> u32 {
    if is_v3() {
        arch::gic_v3_acknowledge()
    } else {
        read32(cpu(), GICC_IAR)
    }
}

/// Ends an interrupt from [`acknowledge`].
pub fn end_of_interrupt(intid: u32) {
    if is_v3() {
        arch::gic_v3_end_of_interrupt(intid);
    } else {
        write32(cpu(), GICC_EOIR, intid);
    }
}

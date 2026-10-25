//! What the kernel needs from the device tree at boot: the console UART,
//! the memory map, the CPU count, the interrupt controller and PSCI.
//!
//! Fixed-capacity results only; nothing here allocates.

use crate::{Fdt, Node, Reg};

/// Most memory regions (and reservations) recorded; further ones are
/// counted in `dropped` but not stored.
pub const MAX_REGIONS: usize = 8;

/// Most `reg` entries recorded for the interrupt controller.
pub const MAX_IRQC_REGS: usize = 4;

/// Why the platform description could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformError {
    /// No usable PL011 UART: `/chosen/stdout-path` names none and no enabled
    /// `arm,pl011` node exists.
    NoUart,
    /// The UART node has no usable `reg`.
    BadUartReg,
    /// No enabled `device_type = "memory"` node with a non-empty region.
    NoMemory,
}

impl core::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            PlatformError::NoUart => "no PL011 UART in the device tree",
            PlatformError::BadUartReg => "UART node has no usable reg",
            PlatformError::NoMemory => "no memory regions in the device tree",
        })
    }
}

/// A fixed-capacity list of address ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionList {
    items: [Reg; MAX_REGIONS],
    len: usize,
    /// Entries found beyond `MAX_REGIONS`.
    pub dropped: usize,
}

impl RegionList {
    pub const fn new() -> RegionList {
        RegionList {
            items: [Reg {
                address: 0,
                size: 0,
            }; MAX_REGIONS],
            len: 0,
            dropped: 0,
        }
    }

    fn push(&mut self, r: Reg) {
        match self.items.get_mut(self.len) {
            Some(slot) => {
                *slot = r;
                self.len += 1;
            }
            None => self.dropped += 1,
        }
    }

    pub fn as_slice(&self) -> &[Reg] {
        &self.items[..self.len]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for RegionList {
    fn default() -> Self {
        RegionList::new()
    }
}

/// How the console UART was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UartSource<'a> {
    /// Named by `/chosen/stdout-path` (the path or alias as written there,
    /// without options).
    StdoutPath(&'a str),
    /// No usable `stdout-path`; first enabled `arm,pl011` node.
    FirstCompatible,
}

/// The console UART.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uart<'a> {
    pub base: u64,
    pub size: u64,
    pub node_name: &'a str,
    pub source: UartSource<'a>,
}

/// Interrupt controller family, from `compatible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GicKind {
    V2,
    V3,
    Other,
}

/// The interrupt controller node that interrupts are routed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptController<'a> {
    pub node_name: &'a str,
    pub kind: GicKind,
    regs: [Reg; MAX_IRQC_REGS],
    reg_count: usize,
}

impl InterruptController<'_> {
    /// The first `MAX_IRQC_REGS` `reg` entries (distributor first).
    pub fn regs(&self) -> &[Reg] {
        &self.regs[..self.reg_count]
    }
}

/// PSCI conduit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsciMethod {
    Hvc,
    Smc,
}

/// The PSCI node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Psci {
    pub method: PsciMethod,
    /// True for `arm,psci-0.2` and later, which define `SYSTEM_OFF` with a
    /// fixed function ID. PSCI 0.1 (`arm,psci` only) does not.
    pub has_system_off: bool,
    /// The most recent `arm,psci-X.Y` compatible string, if any.
    pub compatible: Option<&'static str>,
}

/// Everything the kernel reads from the device tree at stage 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformInfo<'a> {
    pub uart: Uart<'a>,
    pub memory: RegionList,
    pub reserved: RegionList,
    pub cpu_count: usize,
    pub interrupt_controller: Option<InterruptController<'a>>,
    pub psci: Option<Psci>,
}

impl<'a> PlatformInfo<'a> {
    /// Extracts the platform description. A UART and at least one memory
    /// region are required; the other parts are optional.
    pub fn from_fdt(fdt: &Fdt<'a>) -> Result<PlatformInfo<'a>, PlatformError> {
        let memory = memory_regions(fdt);
        if memory.is_empty() {
            return Err(PlatformError::NoMemory);
        }
        Ok(PlatformInfo {
            uart: console_uart(fdt)?,
            memory,
            reserved: reserved_regions(fdt),
            cpu_count: cpu_count(fdt),
            interrupt_controller: interrupt_controller(fdt),
            psci: psci(fdt),
        })
    }
}

fn first_reg(node: &Node<'_>) -> Option<Reg> {
    node.reg()?.next()
}

/// The console UART: the PL011 named by `/chosen/stdout-path`, else the first
/// enabled `arm,pl011` node.
pub fn console_uart<'a>(fdt: &Fdt<'a>) -> Result<Uart<'a>, PlatformError> {
    let from_chosen = fdt
        .find_node("/chosen")
        .and_then(|c| c.property("stdout-path"))
        .and_then(|p| p.as_str())
        .and_then(|s| {
            // "path:options" (Devicetree Specification 3.6).
            let target = s.split(':').next().unwrap_or(s);
            let node = if target.starts_with('/') {
                fdt.find_node(target)
            } else {
                fdt.alias(target)
            }?;
            Some((node, UartSource::StdoutPath(target)))
        })
        .filter(|(n, _)| n.is_compatible("arm,pl011") && n.is_enabled());
    let (node, source) = match from_chosen {
        Some(found) => found,
        None => {
            let n = fdt
                .nodes()
                .find(|n| n.is_compatible("arm,pl011") && n.is_enabled())
                .ok_or(PlatformError::NoUart)?;
            (n, UartSource::FirstCompatible)
        }
    };
    let reg = first_reg(&node)
        .filter(|r| r.size != 0)
        .ok_or(PlatformError::BadUartReg)?;
    Ok(Uart {
        base: reg.address,
        size: reg.size,
        node_name: node.name(),
        source,
    })
}

/// Every non-empty `reg` range of every enabled `device_type = "memory"` node.
pub fn memory_regions(fdt: &Fdt<'_>) -> RegionList {
    let mut list = RegionList::new();
    for node in fdt.nodes() {
        if node.device_type() != Some("memory") || !node.is_enabled() {
            continue;
        }
        if let Some(regs) = node.reg() {
            regs.filter(|r| r.size != 0).for_each(|r| list.push(r));
        }
    }
    list
}

/// The memory reservation block followed by the `reg` ranges of the children
/// of `/reserved-memory`.
pub fn reserved_regions(fdt: &Fdt<'_>) -> RegionList {
    let mut list = RegionList::new();
    fdt.reservations().for_each(|r| list.push(r));
    if let Some(rm) = fdt.find_node("/reserved-memory") {
        for child in rm.children() {
            if let Some(regs) = child.reg() {
                regs.for_each(|r| list.push(r));
            }
        }
    }
    list
}

/// Number of `device_type = "cpu"` children of `/cpus`.
pub fn cpu_count(fdt: &Fdt<'_>) -> usize {
    fdt.find_node("/cpus").map_or(0, |cpus| {
        cpus.children()
            .filter(|c| c.device_type() == Some("cpu"))
            .count()
    })
}

/// The controller named by the root's `interrupt-parent`, else the first node
/// with an `interrupt-controller` property.
pub fn interrupt_controller<'a>(fdt: &Fdt<'a>) -> Option<InterruptController<'a>> {
    let is_irqc = |n: &Node<'_>| n.property("interrupt-controller").is_some();
    let node = fdt
        .root()
        .property("interrupt-parent")
        .and_then(|p| p.as_u32())
        .and_then(|ph| fdt.node_by_phandle(ph))
        .filter(is_irqc)
        .or_else(|| fdt.nodes().find(is_irqc))?;
    let kind = if node.is_compatible("arm,gic-v3") {
        GicKind::V3
    } else if ["arm,cortex-a15-gic", "arm,gic-400", "arm,cortex-a9-gic"]
        .iter()
        .any(|c| node.is_compatible(c))
    {
        GicKind::V2
    } else {
        GicKind::Other
    };
    let mut regs = [Reg {
        address: 0,
        size: 0,
    }; MAX_IRQC_REGS];
    let mut reg_count = 0;
    if let Some(it) = node.reg() {
        for (slot, r) in regs.iter_mut().zip(it) {
            *slot = r;
            reg_count += 1;
        }
    }
    Some(InterruptController {
        node_name: node.name(),
        kind,
        regs,
        reg_count,
    })
}

/// The first `arm,psci*` node with a known `method`.
pub fn psci(fdt: &Fdt<'_>) -> Option<Psci> {
    const VERSIONED: [&str; 2] = ["arm,psci-1.0", "arm,psci-0.2"];
    let node = fdt.nodes().find(|n| {
        n.property("compatible")
            .is_some_and(|p| p.strings().any(|s| s.starts_with("arm,psci")))
    })?;
    let method = match node.property("method")?.as_str()? {
        "hvc" => PsciMethod::Hvc,
        "smc" => PsciMethod::Smc,
        _ => return None,
    };
    let compatible = VERSIONED.iter().copied().find(|c| node.is_compatible(c));
    Some(Psci {
        method,
        has_system_off: compatible.is_some(),
        compatible,
    })
}

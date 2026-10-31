//! Tests against device trees dumped from QEMU (see fixtures/README.md).

use zoraya_dtb::platform::{GicKind, PlatformInfo, PsciMethod, UartSource};
use zoraya_dtb::{Fdt, Reg};

const DEFAULT: &[u8] = include_bytes!("fixtures/virt-default.dtb");
const SMP4_2G: &[u8] = include_bytes!("fixtures/virt-smp4-2g.dtb");
const GICV3: &[u8] = include_bytes!("fixtures/virt-gicv3.dtb");
const EL2: &[u8] = include_bytes!("fixtures/virt-el2.dtb");
const HVF: &[u8] = include_bytes!("fixtures/virt-hvf-default.dtb");

const ALL: [(&str, &[u8]); 5] = [
    ("default", DEFAULT),
    ("smp4-2g", SMP4_2G),
    ("gicv3", GICV3),
    ("el2", EL2),
    ("hvf", HVF),
];

const MIB: u64 = 1 << 20;

fn info(blob: &[u8]) -> PlatformInfo<'_> {
    let fdt = Fdt::new(blob).expect("fixture parses");
    PlatformInfo::from_fdt(&fdt).expect("platform info")
}

#[test]
fn every_fixture_parses_with_virt_uart() {
    for (name, blob) in ALL {
        let fdt = Fdt::new(blob).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(fdt.total_size(), blob.len(), "{name}");
        assert_eq!(fdt.header().version, 17, "{name}");
        let i = PlatformInfo::from_fdt(&fdt).unwrap();
        assert_eq!(i.uart.base, 0x0900_0000, "{name}");
        assert_eq!(i.uart.size, 0x1000, "{name}");
        assert_eq!(i.uart.node_name, "pl011@9000000", "{name}");
        assert_eq!(
            i.uart.source,
            UartSource::StdoutPath("/pl011@9000000"),
            "{name}"
        );
        assert!(i.reserved.is_empty(), "{name}");
    }
}

#[test]
fn default_virt() {
    let i = info(DEFAULT);
    assert_eq!(
        i.memory.as_slice(),
        &[Reg {
            address: 0x4000_0000,
            size: 128 * MIB
        }]
    );
    assert_eq!(i.cpu_count, 1);
    let gic = i.interrupt_controller.unwrap();
    assert_eq!(gic.node_name, "intc@8000000");
    assert_eq!(gic.kind, GicKind::V2);
    assert_eq!(
        gic.regs(),
        &[
            Reg {
                address: 0x0800_0000,
                size: 0x1_0000
            },
            Reg {
                address: 0x0801_0000,
                size: 0x1_0000
            },
        ]
    );
    let psci = i.psci.unwrap();
    assert_eq!(psci.method, PsciMethod::Hvc);
    assert!(psci.has_system_off);
    assert_eq!(psci.compatible, Some("arm,psci-1.0"));
}

#[test]
fn four_cpus_two_gib() {
    let i = info(SMP4_2G);
    assert_eq!(i.cpu_count, 4);
    assert_eq!(
        i.memory.as_slice(),
        &[Reg {
            address: 0x4000_0000,
            size: 2048 * MIB
        }]
    );
}

#[test]
fn gic_version_3() {
    let i = info(GICV3);
    let gic = i.interrupt_controller.unwrap();
    assert_eq!(gic.kind, GicKind::V3);
    // Distributor, then the redistributor region.
    assert_eq!(gic.regs()[0].address, 0x0800_0000);
    assert_eq!(gic.regs()[1].address, 0x080a_0000);
}

#[test]
fn virtualization_on_uses_smc_and_adds_gic_virtualization_regs() {
    let i = info(EL2);
    assert_eq!(i.psci.unwrap().method, PsciMethod::Smc);
    let gic = i.interrupt_controller.unwrap();
    assert_eq!(gic.kind, GicKind::V2);
    // GICD, GICC, GICH, GICV.
    assert_eq!(gic.regs().len(), 4);
}

#[test]
fn hvf_default_has_gic_v3() {
    let i = info(HVF);
    assert_eq!(i.interrupt_controller.unwrap().kind, GicKind::V3);
    assert_eq!(i.psci.unwrap().method, PsciMethod::Hvc);
}

#[test]
fn timer_and_uart_interrupts() {
    for (name, blob) in ALL {
        let i = info(blob);
        let t = i.timer.unwrap_or_else(|| panic!("{name}: no timer"));
        // QEMU hw/arm/virt.c: PPIs 13, 14, 11 and 10, level-triggered.
        assert_eq!(t.secure_physical.intid, 29, "{name}");
        assert_eq!(t.physical.intid, 30, "{name}");
        assert_eq!(t.virtual_timer.intid, 27, "{name}");
        assert_eq!(t.hypervisor.intid, 26, "{name}");
        assert!(!t.virtual_timer.edge, "{name}");
        // SPI 1, level-triggered.
        let uart = i.uart.interrupt.unwrap_or_else(|| panic!("{name}"));
        assert_eq!(uart.intid, 33, "{name}");
        assert!(!uart.edge, "{name}");
    }
}

#[test]
fn tree_navigation() {
    let fdt = Fdt::new(DEFAULT).unwrap();
    let root = fdt.root();
    assert_eq!(root.name(), "");
    assert!(root.is_compatible("linux,dummy-virt"));
    assert_eq!(
        root.property("model").and_then(|p| p.as_str()),
        Some("linux,dummy-virt")
    );
    let cpu = fdt.find_node("/cpus/cpu@0").unwrap();
    assert_eq!(cpu.device_type(), Some("cpu"));
    assert_eq!(cpu.reg().unwrap().next().unwrap().address, 0);
    // A name without unit address matches.
    assert_eq!(fdt.find_node("/memory").unwrap().name(), "memory@40000000");
    assert!(fdt.find_node("/no-such-node").is_none());
    assert!(fdt.find_node("relative").is_none());
    // Alias.
    assert_eq!(fdt.alias("serial0").unwrap().name(), "pl011@9000000");
    // Phandle round trip.
    let intc = fdt.find_node("/intc@8000000").unwrap();
    let ph = intc.property("phandle").unwrap().as_u32().unwrap();
    assert_eq!(fdt.node_by_phandle(ph).unwrap().name(), "intc@8000000");
    // Depth-first order starts with the root and visits every node once.
    let names: Vec<&str> = fdt.nodes().map(|n| n.name()).collect();
    assert_eq!(names[0], "");
    assert!(names.contains(&"cpu@0"));
    assert!(names.contains(&"v2m@8020000"));
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert!(sorted.len() >= names.len() - 5, "few duplicate names");
}

#[test]
fn nodes_matches_dtc_node_count() {
    // `dtc -I dtb -O dts virt-default.dtb | grep -c '{$'` counts every node
    // opening line.
    let fdt = Fdt::new(DEFAULT).unwrap();
    let opened = fdt.nodes().count();
    let begin_tokens = count_begin_tokens(DEFAULT);
    assert_eq!(opened, begin_tokens);
}

/// Counts FDT_BEGIN_NODE tokens with an independent, simple walk.
fn count_begin_tokens(blob: &[u8]) -> usize {
    let w = |o: usize| u32::from_be_bytes(blob[o..o + 4].try_into().unwrap()) as usize;
    let base = w(8);
    let mut off = 0;
    let mut n = 0;
    loop {
        match w(base + off) {
            1 => {
                n += 1;
                let name_len = blob[base + off + 4..].iter().position(|&b| b == 0).unwrap();
                off = (off + 4 + name_len + 1 + 3) & !3;
            }
            2 | 4 => off += 4,
            3 => {
                let len = w(base + off + 4);
                off = (off + 12 + len + 3) & !3;
            }
            9 => return n,
            t => panic!("token {t}"),
        }
    }
}

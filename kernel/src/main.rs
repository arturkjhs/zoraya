//! Zoraya kernel, stage 1: boot, exceptions, MMU, memory, interrupts, timer.
//!
//! Only machine-dependent code lives here; logic that needs no hardware is in
//! the host-tested crates under `crates/`.

#![no_std]
#![no_main]

mod arch;
mod console;
mod dtb;
mod memory;
mod panic;
mod pl011;
mod psci;
mod sync;
#[cfg(feature = "qemu-test")]
mod testing;

use core::sync::atomic::{AtomicUsize, Ordering};

use zoraya_dtb::Fdt;
use zoraya_dtb::platform::{GicKind, PlatformInfo, Psci, PsciMethod, UartSource};

/// Version from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Git commit the kernel was built from (build.rs).
pub const GIT_COMMIT: &str = env!("ZORAYA_GIT_COMMIT");

/// PSCI conduit for shutdown: 0 none, 1 HVC, 2 SMC.
static PSCI_CONDUIT: AtomicUsize = AtomicUsize::new(0);

/// Called from boot.rs with the loader's `x0`, the exception level at entry
/// and the image's physical base, at EL1, on the boot stack, with `.bss`
/// cleared, on the boot tables.
#[unsafe(no_mangle)]
extern "C" fn kernel_main(x0: usize, entry_el: u64, phys_base: u64) -> ! {
    memory::set_phys_base(phys_base);
    arch::stack_canary_set();
    arch::exception_init();
    let (fdt, source) = match dtb::locate(x0) {
        Ok(found) => found,
        Err(e) => fatal_before_console(format_args!("{e}")),
    };
    let info = match PlatformInfo::from_fdt(&fdt) {
        Ok(i) => i,
        Err(e) => fatal_before_console(format_args!("{e}")),
    };
    // SAFETY: the base is the `reg` of the device tree node named by
    // /chosen/stdout-path (or the first enabled node) whose compatible list
    // contains "arm,pl011", and nothing else drives that UART.
    unsafe { console::init(info.uart.base as usize) };
    if let Some(p) = info.psci.filter(|p| p.has_system_off) {
        let conduit = match p.method {
            PsciMethod::Hvc => 1,
            PsciMethod::Smc => 2,
        };
        PSCI_CONDUIT.store(conduit, Ordering::Relaxed);
    }

    let reserved_dropped = info.reserved.dropped;
    assert!(
        reserved_dropped == 0,
        "{reserved_dropped} device tree reservations not recorded"
    );
    memory::paging::init(&info);

    // Everything from the first parse pointed through the identity map.
    // SAFETY: `locate` succeeded above, and the final tables are on.
    let blob = unsafe { dtb::rebase() };
    let fdt = Fdt::new(blob).expect("device tree still valid");
    let info = PlatformInfo::from_fdt(&fdt).expect("platform info still valid");

    print_banner(&info, x0, source, entry_el, fdt.total_size());

    #[cfg(feature = "qemu-test")]
    testing::run(0);

    #[cfg(not(feature = "qemu-test"))]
    {
        println!("stage 1 complete; powering off");
        shutdown()
    }
}

/// Reports an error found before the console exists, through the one
/// fallback UART address, and halts.
fn fatal_before_console(msg: core::fmt::Arguments<'_>) -> ! {
    console::print_fallback(format_args!("\nZORAYA-PANIC at boot: {msg}\nhalting\n"));
    arch::halt()
}

fn print_banner(
    info: &PlatformInfo<'_>,
    x0: usize,
    source: dtb::Source,
    entry_el: u64,
    dtb_len: usize,
) {
    println!();
    println!("Zoraya kernel {VERSION} (git {GIT_COMMIT})");
    println!(
        "exception level at entry: EL{entry_el}, now EL{}",
        arch::current_el()
    );
    println!(
        "boot cpu: MPIDR_EL1 {:#x}; physical address size {} bits",
        arch::mpidr(),
        arch::pa_bits()
    );
    let (addr, how) = match source {
        dtb::Source::Register => (x0, "from x0"),
        dtb::Source::StartOfRam => (dtb::RAM_BASE, "at start of RAM"),
    };
    println!("device tree: {addr:#x} ({how}, x0 was {x0:#x}), {dtb_len} bytes");
    let from = match info.uart.source {
        UartSource::StdoutPath(p) => p,
        UartSource::FirstCompatible => "first arm,pl011 node",
    };
    println!("console: pl011 at {:#x} ({from})", info.uart.base);
    println!("cpus: {}", info.cpu_count);
    match &info.interrupt_controller {
        Some(ic) => {
            let kind = match ic.kind {
                GicKind::V2 => "GICv2",
                GicKind::V3 => "GICv3",
                GicKind::Other => "unknown type",
            };
            print!("interrupt controller: {} ({kind})", ic.node_name);
            for r in ic.regs() {
                print!(" {:#x}+{:#x}", r.address, r.size);
            }
            println!();
        }
        None => println!("interrupt controller: none found"),
    }
    match &info.psci {
        Some(Psci {
            method, compatible, ..
        }) => {
            let m = match method {
                PsciMethod::Hvc => "hvc",
                PsciMethod::Smc => "smc",
            };
            println!("psci: {} via {m}", compatible.unwrap_or("arm,psci (0.1)"));
        }
        None => println!("psci: none found"),
    }
    println!("memory map:");
    for r in info.memory.as_slice() {
        println!(
            "  {:#018x}-{:#018x} {:>6} MiB  RAM",
            r.address,
            r.address.saturating_add(r.size - 1),
            r.size >> 20
        );
    }
    if info.memory.dropped > 0 {
        println!("  ({} more regions not recorded)", info.memory.dropped);
    }
    for r in info.reserved.as_slice() {
        println!(
            "  {:#018x}-{:#018x}  reserved",
            r.address,
            r.address.saturating_add(r.size.max(1) - 1)
        );
    }
    let image = memory::kernel_image_phys();
    println!(
        "  {:#018x}-{:#018x}  kernel image and boot stack, at {:#x}",
        image.start,
        image.end - 1,
        memory::layout::KERNEL_IMAGE,
    );
    println!(
        "  {addr:#018x}-{:#018x}  device tree blob",
        addr + dtb_len - 1
    );
}

/// Powers off through PSCI `SYSTEM_OFF`; halts if that is unavailable or fails.
pub fn shutdown() -> ! {
    if !arch::stack_canary_intact() {
        println!("warning: boot stack canary overwritten (stack overflow)");
    }
    let method = match PSCI_CONDUIT.load(Ordering::Relaxed) {
        1 => Some(PsciMethod::Hvc),
        2 => Some(PsciMethod::Smc),
        _ => None,
    };
    match method {
        Some(method) => {
            let psci = Psci {
                method,
                has_system_off: true,
                compatible: None,
            };
            let err = psci::system_off(&psci);
            println!("PSCI SYSTEM_OFF failed ({err}); halting");
        }
        None => println!("no PSCI SYSTEM_OFF in the device tree; halting"),
    }
    arch::halt()
}

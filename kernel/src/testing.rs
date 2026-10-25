//! In-QEMU tests (feature `qemu-test`).
//!
//! Output protocol, one line each, parsed by tools/test-runner:
//!
//! ```text
//! ZORAYA-TEST begin <count>
//! ZORAYA-TEST ok <name>
//! ZORAYA-TEST fail <name> <message>
//! ZORAYA-TEST end pass=<n> fail=<m>
//! ```
//!
//! A test fails by panicking. The panic handler calls [`on_panic`], which
//! prints the `fail` line, abandons the test's stack and continues with the
//! next test on a fresh stack. Tests hold no resources that would need
//! unwinding, so dropping their frames loses nothing.

use core::sync::atomic::{AtomicUsize, Ordering};

use zoraya_dtb::Fdt;
use zoraya_dtb::platform::PlatformInfo;

use crate::{arch, console, dtb, println};

struct Test {
    name: &'static str,
    run: fn(),
}

// The two `harness_reports_*` tests are expected to fail; the runner is
// told so (`--expect-panic`) and checks that each is reported as a panic.

static TESTS: &[Test] = &[
    Test {
        name: "device_tree_found_and_parsed",
        run: device_tree_found_and_parsed,
    },
    Test {
        name: "uart_base_matches_console",
        run: uart_base_matches_console,
    },
    Test {
        name: "memory_regions_inside_pa_space",
        run: memory_regions_inside_pa_space,
    },
    Test {
        name: "kernel_image_inside_memory",
        run: kernel_image_inside_memory,
    },
    Test {
        name: "device_tree_in_memory_below_kernel",
        run: device_tree_in_memory_below_kernel,
    },
    Test {
        name: "exception_level_is_el1",
        run: exception_level_is_el1,
    },
    Test {
        name: "boot_cpu_is_affinity_zero",
        run: boot_cpu_is_affinity_zero,
    },
    Test {
        name: "cpu_count_nonzero",
        run: cpu_count_nonzero,
    },
    Test {
        name: "interrupt_controller_found",
        run: interrupt_controller_found,
    },
    Test {
        name: "psci_system_off_available",
        run: psci_system_off_available,
    },
    Test {
        name: "harness_reports_panic_as_failure",
        run: harness_reports_panic_as_failure,
    },
    Test {
        name: "harness_reports_exception_as_failure",
        run: harness_reports_exception_as_failure,
    },
    Test {
        name: "harness_resumes_after_panic",
        run: harness_resumes_after_panic,
    },
];

const NONE: usize = usize::MAX;

// Plain loads and stores only: one CPU, interrupts masked.
static CURRENT: AtomicUsize = AtomicUsize::new(NONE);
static PASSED: AtomicUsize = AtomicUsize::new(0);
static FAILED: AtomicUsize = AtomicUsize::new(0);

fn bump(counter: &AtomicUsize) {
    counter.store(counter.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
}

/// Runs the tests from index `start` on, then powers off.
pub fn run(start: usize) -> ! {
    if start == 0 {
        println!("ZORAYA-TEST begin {}", TESTS.len());
    }
    for (i, t) in TESTS.iter().enumerate().skip(start) {
        CURRENT.store(i, Ordering::Relaxed);
        (t.run)();
        CURRENT.store(NONE, Ordering::Relaxed);
        bump(&PASSED);
        println!("ZORAYA-TEST ok {}", t.name);
    }
    println!(
        "ZORAYA-TEST end pass={} fail={}",
        PASSED.load(Ordering::Relaxed),
        FAILED.load(Ordering::Relaxed)
    );
    crate::shutdown()
}

extern "C" fn resume(next: usize) -> ! {
    run(next)
}

/// Called by the panic handler. Returns only if no test is running.
pub fn on_panic(info: &core::panic::PanicInfo<'_>) {
    use core::fmt::Write;
    let i = CURRENT.load(Ordering::Relaxed);
    let Some(t) = TESTS.get(i) else { return };
    CURRENT.store(NONE, Ordering::Relaxed);
    bump(&FAILED);
    crate::print!("ZORAYA-TEST fail {} panic ", t.name);
    let _ = write!(console::OneLine, "{}", crate::panic::Report(info));
    println!();
    crate::panic::clear();
    // SAFETY: the panicking test's frames and the panic machinery's frames
    // are all on the boot stack; none holds anything still referenced (tests
    // keep no state outside the atomics above), one CPU runs, and
    // interrupts are masked.
    unsafe { arch::reset_stack_and_call(resume, i + 1) }
}

fn platform() -> (Fdt<'static>, PlatformInfo<'static>) {
    let blob = dtb::blob().expect("device tree located at boot");
    let fdt = Fdt::new(blob).expect("device tree parses");
    let info = PlatformInfo::from_fdt(&fdt).expect("platform info");
    (fdt, info)
}

fn device_tree_found_and_parsed() {
    let (fdt, info) = platform();
    assert!(
        fdt.root().is_compatible("linux,dummy-virt"),
        "root is QEMU virt"
    );
    assert!(!info.memory.is_empty());
}

fn uart_base_matches_console() {
    let (_, info) = platform();
    assert_ne!(console::base(), 0);
    assert_eq!(info.uart.base as usize, console::base());
}

fn memory_regions_inside_pa_space() {
    let (_, info) = platform();
    let limit = 1u64 << arch::pa_bits();
    assert!(!info.memory.is_empty());
    for r in info.memory.as_slice() {
        assert!(r.size > 0, "empty region at {:#x}", r.address);
        let end = r.address.checked_add(r.size).expect("region end overflows");
        assert!(
            end <= limit,
            "region {:#x}+{:#x} beyond {} PA bits",
            r.address,
            r.size,
            arch::pa_bits()
        );
    }
}

fn contains(info: &PlatformInfo<'_>, start: usize, end: usize) -> bool {
    info.memory
        .as_slice()
        .iter()
        .any(|r| r.address <= start as u64 && end as u64 <= r.address + r.size)
}

fn kernel_image_inside_memory() {
    let (_, info) = platform();
    let (start, end) = arch::kernel_image();
    assert!(start < end);
    assert!(
        contains(&info, start, end),
        "image {start:#x}..{end:#x} not in RAM"
    );
}

fn device_tree_in_memory_below_kernel() {
    let (_, info) = platform();
    let blob = dtb::blob().expect("device tree located at boot");
    let start = blob.as_ptr() as usize;
    let end = start + blob.len();
    assert!(contains(&info, start, end));
    assert!(end <= arch::kernel_image().0);
}

fn exception_level_is_el1() {
    assert_eq!(arch::current_el(), 1);
}

fn boot_cpu_is_affinity_zero() {
    assert_eq!(arch::mpidr() & 0xff_00ff_ffff, 0);
}

fn cpu_count_nonzero() {
    let (_, info) = platform();
    assert!(info.cpu_count >= 1);
}

fn interrupt_controller_found() {
    let (_, info) = platform();
    let ic = info.interrupt_controller.expect("interrupt controller");
    assert!(!ic.regs().is_empty());
}

fn psci_system_off_available() {
    let (_, info) = platform();
    let psci = info.psci.expect("PSCI node");
    assert!(psci.has_system_off);
}

fn harness_reports_panic_as_failure() {
    panic!("deliberate panic to test the harness");
}

fn harness_reports_exception_as_failure() {
    // SAFETY: `udf` raises an undefined-instruction exception, which the
    // catch-all vector turns into a panic; nothing after it executes.
    unsafe { core::arch::asm!("udf #0", options(nomem, nostack)) };
}

fn harness_resumes_after_panic() {
    // Runs after the two deliberate failures: the harness recorded exactly
    // those and came back to run this test.
    assert_eq!(FAILED.load(Ordering::Relaxed), 2);
}

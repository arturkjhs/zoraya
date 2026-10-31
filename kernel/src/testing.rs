//! In-QEMU tests (feature `qemu-test`).
//!
//! Output protocol, one line each, parsed by tools/test-runner:
//!
//! ```text
//! ZORAYA-TEST begin <count>
//! ZORAYA-TEST run <name>
//! ZORAYA-TEST ok <name>
//! ZORAYA-TEST fail <name> <message>
//! ZORAYA-TEST end pass=<n> fail=<m>
//! ```
//!
//! A test fails by panicking. The panic handler calls [`on_panic`], which
//! prints the `fail` line and leaves through `eret` to the next test, on a
//! fresh stack and out of any exception context. Tests hold no resources that
//! would need unwinding, so dropping their frames loses nothing.

use core::hint::black_box;
use core::ptr::{addr_of, read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use zoraya_dtb::Fdt;
use zoraya_dtb::platform::PlatformInfo;
use zoraya_pagetable::{MemoryType, PAGE_SIZE};

use crate::arch::mmu;
use crate::memory::layout::{DEVICE_WINDOW, DEVICE_WINDOW_SIZE, STACK_WINDOW};
use crate::memory::paging;
use crate::sync::SpinLock;
use crate::{arch, console, dtb, memory, println};

struct Test {
    name: &'static str,
    run: fn(),
}

// The `harness_reports_*` and `fault_report_*` tests are expected to fail;
// the runner is told so (`--expect-panic`) and checks that each is reported
// as a panic, and `--expect-line` checks what the report said.
const EXPECTED_FAILURES: usize = 5;

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
        name: "brk_is_taken_with_its_class",
        run: brk_is_taken_with_its_class,
    },
    Test {
        name: "exception_returns_with_eret",
        run: exception_returns_with_eret,
    },
    Test {
        name: "mmu_and_caches_are_on",
        run: mmu_and_caches_are_on,
    },
    Test {
        name: "atomic_read_modify_write_works",
        run: atomic_read_modify_write_works,
    },
    Test {
        name: "lock_masks_interrupts",
        run: lock_masks_interrupts,
    },
    Test {
        name: "pmu_and_debug_registers_do_not_trap",
        run: pmu_and_debug_registers_do_not_trap,
    },
    Test {
        name: "unmapped_access_is_taken",
        run: unmapped_access_is_taken,
    },
    Test {
        name: "null_pointer_read_faults",
        run: null_pointer_read_faults,
    },
    Test {
        name: "write_to_text_faults",
        run: write_to_text_faults,
    },
    Test {
        name: "write_to_rodata_faults",
        run: write_to_rodata_faults,
    },
    Test {
        name: "write_to_device_tree_faults",
        run: write_to_device_tree_faults,
    },
    Test {
        name: "execute_from_data_faults",
        run: execute_from_data_faults,
    },
    Test {
        name: "execute_from_stack_faults",
        run: execute_from_stack_faults,
    },
    Test {
        name: "execute_from_device_faults",
        run: execute_from_device_faults,
    },
    Test {
        name: "identity_map_is_gone",
        run: identity_map_is_gone,
    },
    Test {
        name: "image_is_mapped_once",
        run: image_is_mapped_once,
    },
    Test {
        name: "live_tables_obey_wx_and_device_rules",
        run: live_tables_obey_wx_and_device_rules,
    },
    Test {
        name: "uart_is_reached_through_the_device_window",
        run: uart_is_reached_through_the_device_window,
    },
    Test {
        name: "stack_guard_is_unmapped",
        run: stack_guard_is_unmapped,
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
        name: "fault_report_for_brk",
        run: fault_report_for_brk,
    },
    Test {
        name: "fault_report_for_unmapped_access",
        run: fault_report_for_unmapped_access,
    },
    Test {
        name: "fault_report_for_stack_overflow",
        run: fault_report_for_stack_overflow,
    },
    Test {
        name: "harness_resumes_after_panic",
        run: harness_resumes_after_panic,
    },
    // Last, so that it covers every test before it.
    Test {
        name: "stack_canary_intact",
        run: stack_canary_intact,
    },
];

const NONE: usize = usize::MAX;

/// Set by the test that overflows the stack on purpose.
static REARM_CANARY: AtomicBool = AtomicBool::new(false);
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
        println!("ZORAYA-TEST run {}", t.name);
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
    if REARM_CANARY.swap(false, Ordering::Relaxed) {
        arch::stack_canary_set();
    }
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
    // are all on the boot stack, and none holds anything still referenced:
    // tests keep no state outside the atomics above, one CPU runs, and the
    // panic handler masked interrupts.
    unsafe { arch::recover_to(resume, i + 1) }
}

fn platform() -> (Fdt<'static>, PlatformInfo<'static>) {
    let fdt = Fdt::new(dtb::blob()).expect("device tree parses");
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
    let va = console::base() as u64;
    let m = paging::translate(va).expect("console mapping");
    assert_eq!(m.pa + (va - m.va), info.uart.base);
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
    info.memory.as_slice().iter().any(|r| {
        r.address <= start as u64
            && r.address
                .checked_add(r.size)
                .is_some_and(|e| end as u64 <= e)
    })
}

fn kernel_image_inside_memory() {
    let (_, info) = platform();
    let image = memory::kernel_image_phys();
    let (start, end) = (image.start as usize, image.end as usize);
    assert!(start < end);
    assert!(
        contains(&info, start, end),
        "image {start:#x}..{end:#x} not in RAM"
    );
}

fn device_tree_in_memory_below_kernel() {
    let (_, info) = platform();
    let (start, len) = dtb::phys();
    let end = start + len;
    assert!(contains(&info, start, end));
    assert!(end as u64 <= memory::kernel_image_phys().start);
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
    // fault policy turns into a panic; nothing after it executes.
    unsafe { core::arch::asm!("udf #0", options(nomem, nostack)) };
}

extern "C" fn do_brk(_: usize) {
    // SAFETY: `brk` raises a breakpoint exception that `catch` handles;
    // nothing after it executes.
    unsafe { core::arch::asm!("brk #0x123", options(nomem, nostack)) };
}

fn brk_is_taken_with_its_class() {
    let fault = arch::catch(do_brk, 0).expect("brk faults");
    assert_eq!(fault.ec, 0x3c, "ESR_EL1.EC of BRK");
    assert_eq!(fault.esr & 0xffff, 0x123, "ISS holds the immediate");
    assert_eq!(fault.far, None);
    assert_eq!(arch::current_el(), 1, "back at EL1 after eret");
}

fn exception_returns_with_eret() {
    extern "C" fn ok(arg: usize) {
        assert_eq!(arg, 7);
    }
    assert!(arch::catch(ok, 7).is_none(), "no fault, no record");
    let first = arch::catch(do_brk, 0).expect("brk faults");
    let second = arch::catch(do_brk, 0).expect("brk faults again");
    assert_eq!(first.elr, second.elr, "same instruction both times");
}

fn fault_report_for_brk() {
    // SAFETY: as in `do_brk`, with no `catch` active: the fault policy panics.
    unsafe { core::arch::asm!("brk #0x321", options(nomem, nostack)) };
}

const EC_INSTRUCTION_ABORT: u64 = 0x21;
const EC_DATA_ABORT: u64 = 0x25;

/// Bits 5:2 of the fault status code (`DFSC`/`IFSC`; bits 1:0 are the level).
const FSC_TRANSLATION: u64 = 0b0001;
const FSC_PERMISSION: u64 = 0b0011;

fn fault_status(esr: u64) -> u64 {
    (esr & 0x3f) >> 2
}

/// Data abort `ISS.WnR`: the access was a write.
fn is_write(esr: u64) -> bool {
    esr & (1 << 6) != 0
}

extern "C" fn read_at(addr: usize) {
    // SAFETY: only ever run under `catch`, on an address expected to fault;
    // a read has no effect if it does not.
    unsafe { read_volatile(addr as *const u64) };
}

extern "C" fn write_at(addr: usize) {
    // SAFETY: only ever run under `catch`, on an address expected to fault.
    unsafe { write_volatile(addr as *mut u64, 0) };
}

extern "C" fn jump_to(addr: usize) {
    // SAFETY: only ever run under `catch`, on an address expected to fault
    // at the first instruction fetch.
    let f: extern "C" fn() = unsafe { core::mem::transmute(addr) };
    f();
}

fn expect_data_abort(f: extern "C" fn(usize), addr: u64, status: u64, write: bool) {
    let fault = arch::catch(f, addr as usize).expect("access must fault");
    assert_eq!(fault.ec, EC_DATA_ABORT, "ESR {:#x}", fault.esr);
    assert_eq!(fault.far, Some(addr));
    assert_eq!(fault_status(fault.esr), status, "ESR {:#x}", fault.esr);
    assert_eq!(is_write(fault.esr), write, "ESR {:#x}", fault.esr);
}

fn expect_execute_fault(addr: u64) {
    let fault = arch::catch(jump_to, addr as usize).expect("fetch must fault");
    assert_eq!(fault.ec, EC_INSTRUCTION_ABORT, "ESR {:#x}", fault.esr);
    assert_eq!(fault.far, Some(addr));
    assert_eq!(fault.elr, addr);
    assert_eq!(
        fault_status(fault.esr),
        FSC_PERMISSION,
        "ESR {:#x}",
        fault.esr
    );
}

fn mmu_and_caches_are_on() {
    let sctlr = mmu::sctlr();
    for (bit, name) in [(0, "M"), (2, "C"), (12, "I"), (19, "WXN")] {
        assert!(
            sctlr & (1 << bit) != 0,
            "SCTLR_EL1.{name} clear: {sctlr:#x}"
        );
    }
}

fn atomic_read_modify_write_works() {
    let a = AtomicUsize::new(1);
    assert_eq!(a.fetch_add(2, Ordering::SeqCst), 1);
    assert_eq!(a.swap(9, Ordering::SeqCst), 3);
    assert_eq!(
        a.compare_exchange(9, 4, Ordering::SeqCst, Ordering::SeqCst),
        Ok(9)
    );
}

fn lock_masks_interrupts() {
    static LOCK: SpinLock<u32> = SpinLock::new(0);
    let before = arch::save_and_mask_interrupts();
    arch::restore_interrupts(before);
    assert_eq!(before & (0b11 << 6), arch::interrupt_mask() & (0b11 << 6));
    {
        let mut held = LOCK.lock();
        *held += 1;
        assert_eq!(
            arch::interrupt_mask() & (0b11 << 6),
            0b11 << 6,
            "I and F masked while held"
        );
    }
    assert_eq!(arch::interrupt_mask(), before, "restored on release");
    assert_eq!(*LOCK.lock(), 1);
}

fn pmu_and_debug_registers_do_not_trap() {
    // MDCR_EL2 set by the boot code: these would trap to an EL2 that has no
    // vectors if its traps were on.
    let (pmcr, mdscr): (u64, u64);
    // SAFETY: reading these registers has no side effects at EL1.
    unsafe {
        core::arch::asm!("mrs {}, pmcr_el0", "mrs {}, mdscr_el1", out(reg) pmcr, out(reg) mdscr,
                         options(nomem, nostack, preserves_flags));
    }
    black_box((pmcr, mdscr));
}

fn unmapped_access_is_taken() {
    expect_data_abort(read_at, STACK_WINDOW + 0x1000, FSC_TRANSLATION, false);
    expect_data_abort(write_at, STACK_WINDOW + 0x1000, FSC_TRANSLATION, true);
}

fn null_pointer_read_faults() {
    expect_data_abort(read_at, 0, FSC_TRANSLATION, false);
}

fn write_to_text_faults() {
    let addr = symbol_of(write_to_text_faults as *const ());
    // SAFETY: reading kernel text, which is mapped readable.
    let before = unsafe { read_volatile(addr as *const u64) };
    expect_data_abort(write_at, addr, FSC_PERMISSION, true);
    // SAFETY: as above.
    assert_eq!(unsafe { read_volatile(addr as *const u64) }, before);
}

fn symbol_of(f: *const ()) -> u64 {
    // Rounded so the access is aligned.
    (f as u64) & !7
}

static RODATA_PROBE: u64 = 0x1234_5678;

fn write_to_rodata_faults() {
    let addr = addr_of!(RODATA_PROBE) as u64;
    expect_data_abort(write_at, addr, FSC_PERMISSION, true);
    assert_eq!(black_box(RODATA_PROBE), 0x1234_5678);
}

fn write_to_device_tree_faults() {
    let addr = dtb::blob().as_ptr() as u64;
    expect_data_abort(write_at, addr, FSC_PERMISSION, true);
}

/// `ret`, in writable memory. Non-zero, so it is in .data, not .bss.
static mut DATA_CODE: [u32; 4] = [0xd65f_03c0; 4];

fn execute_from_data_faults() {
    expect_execute_fault((&raw const DATA_CODE) as u64);
}

fn execute_from_stack_faults() {
    let code = black_box([0xd65f_03c0u32; 4]);
    expect_execute_fault(code.as_ptr() as u64);
}

fn execute_from_device_faults() {
    expect_execute_fault(console::base() as u64);
}

fn identity_map_is_gone() {
    /// `TCR_EL1.EPD0`.
    const EPD0: u64 = 1 << 7;
    let (_, info) = platform();
    assert!(mmu::tcr() & EPD0 != 0, "TCR_EL1 {:#x}", mmu::tcr());
    assert_eq!(mmu::ttbr0(), 0);
    let image = memory::kernel_image_phys();
    for addr in [info.uart.base, image.start, dtb::phys().0 as u64] {
        expect_data_abort(read_at, addr, FSC_TRANSLATION, false);
    }
    let ic = info.interrupt_controller.expect("interrupt controller");
    let gic = ic.regs()[0].address;
    expect_data_abort(read_at, gic, FSC_TRANSLATION, false);
}

fn image_is_mapped_once() {
    let image = memory::kernel_image_phys();
    let mut frames = [0u8; 4096];
    let size = (image.end - image.start) / PAGE_SIZE;
    assert!(
        size as usize <= frames.len(),
        "image too large for the test"
    );
    paging::for_each_mapping(&mut |m| {
        // A mapping may be a block that overlaps the image only in part.
        let mut at = m.pa.max(image.start);
        while at < (m.pa + m.size).min(image.end) {
            frames[((at - image.start) / PAGE_SIZE) as usize] += 1;
            at += PAGE_SIZE;
        }
    });
    assert!(
        frames.iter().all(|n| *n <= 1),
        "an image frame mapped twice"
    );
    assert!(frames.iter().take(size as usize).any(|n| *n == 1));
    expect_data_abort(
        read_at,
        memory::phys_to_virt(image.start),
        FSC_TRANSLATION,
        false,
    );
}

fn live_tables_obey_wx_and_device_rules() {
    let mut count = 0;
    paging::for_each_mapping(&mut |m| {
        count += 1;
        let a = m.attributes;
        assert!(!(a.write && a.execute), "W+X at {:#x}", m.va);
        assert!(!a.user, "EL0 access at {:#x}", m.va);
        let in_device_window = (DEVICE_WINDOW..DEVICE_WINDOW + DEVICE_WINDOW_SIZE).contains(&m.va);
        assert_eq!(a.memory == MemoryType::Device, in_device_window, "{m:?}");
        if a.memory == MemoryType::Device {
            assert!(!a.execute, "executable device memory at {:#x}", m.va);
        }
        if a.execute {
            assert!(m.va >= memory::layout::KERNEL_IMAGE, "{m:?}");
        }
    });
    assert!(count > 3);
}

fn uart_is_reached_through_the_device_window() {
    let va = console::base() as u64;
    assert!((DEVICE_WINDOW..DEVICE_WINDOW + DEVICE_WINDOW_SIZE).contains(&va));
    let m = paging::translate(va).expect("console mapping");
    assert_eq!(m.attributes.memory, MemoryType::Device);
}

fn stack_guard_is_unmapped() {
    let guard = addr_of!(memory::__stack_guard) as u64;
    let bottom = addr_of!(memory::__stack_bottom) as u64;
    assert_eq!(bottom - guard, PAGE_SIZE);
    assert!(paging::translate(guard).is_none());
    assert!(paging::translate(bottom).is_some());
    expect_data_abort(read_at, guard, FSC_TRANSLATION, false);
}

fn fault_report_for_unmapped_access() {
    // No `catch`: the fault policy reports and panics.
    read_at((STACK_WINDOW + 0x2000) as usize);
}

#[inline(never)]
fn use_stack(depth: u64) -> u64 {
    let frame = [depth; 64];
    if depth == u64::MAX {
        return frame[1];
    }
    use_stack(black_box(depth + 1)) + black_box(&frame)[1]
}

fn fault_report_for_stack_overflow() {
    REARM_CANARY.store(true, Ordering::Relaxed);
    black_box(use_stack(0));
}

fn stack_canary_intact() {
    assert!(
        arch::stack_canary_intact(),
        "boot stack overflowed into its canary"
    );
}

fn harness_resumes_after_panic() {
    // Runs after the deliberate failures: the harness recorded exactly those
    // and came back to run this test.
    assert_eq!(FAILED.load(Ordering::Relaxed), EXPECTED_FAILURES);
}

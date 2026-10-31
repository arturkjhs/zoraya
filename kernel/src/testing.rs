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

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::hint::black_box;
use core::ptr::{addr_of, read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use zoraya_abi::{ExitReason, ExitRecord};
use zoraya_dtb::Fdt;
use zoraya_dtb::platform::PlatformInfo;
use zoraya_elf::Error as ElfError;
use zoraya_pagetable::{MemoryType, PAGE_SIZE};
use zoraya_pmm::{Frame, Range};
use zoraya_time::{Deadline, Instant};

use crate::arch::mmu;
use crate::memory::layout::{DEVICE_WINDOW, DEVICE_WINDOW_SIZE, KERNEL_IMAGE};
use crate::memory::stack::KernelStack;
use crate::memory::user::{USER_HIGH, USER_LOW};
use crate::memory::{frames, heap, paging};
use crate::process::{self, SpawnError};
use crate::sync::SpinLock;
use crate::{arch, console, dtb, gic, irq, memory, println, sched, timer};

struct Test {
    name: &'static str,
    run: fn(),
}

// The `harness_reports_*` and `fault_report_*` tests are expected to fail;
// the runner is told so (`--expect-panic`) and checks that each is reported
// as a panic, and `--expect-line` checks what the report said.
const EXPECTED_FAILURES: usize = 9;

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
        name: "gic_is_reached_through_the_device_window",
        run: gic_is_reached_through_the_device_window,
    },
    Test {
        name: "stack_guard_is_unmapped",
        run: stack_guard_is_unmapped,
    },
    Test {
        name: "box_vec_btreemap_string_work",
        run: box_vec_btreemap_string_work,
    },
    Test {
        name: "heap_stress_grows_and_reuses",
        run: heap_stress_grows_and_reuses,
    },
    Test {
        name: "heap_blocks_are_aligned",
        run: heap_blocks_are_aligned,
    },
    Test {
        name: "fallible_allocation_returns_failure",
        run: fallible_allocation_returns_failure,
    },
    Test {
        name: "frames_avoid_every_reserved_region",
        run: frames_avoid_every_reserved_region,
    },
    Test {
        name: "timer_interrupts_agree_with_the_clock",
        run: timer_interrupts_agree_with_the_clock,
    },
    Test {
        name: "clock_never_decreases_across_interrupts",
        run: clock_never_decreases_across_interrupts,
    },
    Test {
        name: "one_shot_deadline_is_not_early",
        run: one_shot_deadline_is_not_early,
    },
    Test {
        name: "deadline_wakes_a_waiting_cpu",
        run: deadline_wakes_a_waiting_cpu,
    },
    Test {
        name: "deadlines_fire_in_order_and_cancel",
        run: deadlines_fire_in_order_and_cancel,
    },
    Test {
        name: "timer_is_off_with_no_deadlines",
        run: timer_is_off_with_no_deadlines,
    },
    Test {
        name: "shared_interrupt_is_delivered",
        run: shared_interrupt_is_delivered,
    },
    Test {
        name: "interrupt_preserves_fp_registers",
        run: interrupt_preserves_fp_registers,
    },
    Test {
        name: "kernel_threads_take_turns",
        run: kernel_threads_take_turns,
    },
    Test {
        name: "sleep_ends_no_earlier_than_its_deadline",
        run: sleep_ends_no_earlier_than_its_deadline,
    },
    Test {
        name: "thread_stack_has_a_guard_page",
        run: thread_stack_has_a_guard_page,
    },
    Test {
        name: "user_process_prints_and_exits",
        run: user_process_prints_and_exits,
    },
    Test {
        name: "user_tables_obey_wx",
        run: user_tables_obey_wx,
    },
    Test {
        name: "user_faults_end_the_process_only",
        run: user_faults_end_the_process_only,
    },
    Test {
        name: "bad_syscall_arguments_return_errors",
        run: bad_syscall_arguments_return_errors,
    },
    Test {
        name: "random_syscalls_do_not_hurt_the_kernel",
        run: random_syscalls_do_not_hurt_the_kernel,
    },
    Test {
        name: "processes_cannot_see_each_other",
        run: processes_cannot_see_each_other,
    },
    Test {
        name: "cpu_bound_processes_share_the_cpu",
        run: cpu_bound_processes_share_the_cpu,
    },
    Test {
        name: "registers_survive_preemption",
        run: registers_survive_preemption,
    },
    Test {
        name: "bad_programs_are_refused",
        run: bad_programs_are_refused,
    },
    Test {
        name: "exited_processes_leave_nothing_behind",
        run: exited_processes_leave_nothing_behind,
    },
    Test {
        name: "running_out_of_asids_is_an_error",
        run: running_out_of_asids_is_an_error,
    },
    Test {
        name: "ttbr0_is_off_with_no_process",
        run: ttbr0_is_off_with_no_process,
    },
    Test {
        name: "copy_fault_on_kernel_memory_is_not_resumed",
        run: copy_fault_on_kernel_memory_is_not_resumed,
    },
    Test {
        name: "wait_keeps_the_interrupt_mask",
        run: wait_keeps_the_interrupt_mask,
    },
    Test {
        name: "every_waiter_is_woken",
        run: every_waiter_is_woken,
    },
    Test {
        name: "registers_survive_system_calls",
        run: registers_survive_system_calls,
    },
    Test {
        name: "lone_process_has_no_timer",
        run: lone_process_has_no_timer,
    },
    Test {
        name: "full_deadline_queue_keeps_the_slice",
        run: full_deadline_queue_keeps_the_slice,
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
        name: "fault_report_for_unexpected_interrupt",
        run: fault_report_for_unexpected_interrupt,
    },
    Test {
        name: "interrupts_still_arrive_after_a_fault",
        run: interrupts_still_arrive_after_a_fault,
    },
    Test {
        name: "fault_report_for_el0_exception",
        run: fault_report_for_el0_exception,
    },
    Test {
        name: "fault_report_for_thread_stack_overflow",
        run: fault_report_for_thread_stack_overflow,
    },
    Test {
        name: "fault_report_for_stack_overflow_from_el0",
        run: fault_report_for_stack_overflow_from_el0,
    },
    Test {
        name: "fault_report_for_stack_overflow",
        run: fault_report_for_stack_overflow,
    },
    Test {
        name: "every_overflow_reached_the_limit_check",
        run: every_overflow_reached_the_limit_check,
    },
    Test {
        name: "emergency_stack_kept_a_margin",
        run: emergency_stack_kept_a_margin,
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
    crate::sched::adopt_boot_thread();
    // SAFETY: the panicking test's frames and the panic machinery's frames
    // are on the boot stack, a thread's stack, or the emergency stack after a
    // stack overflow, and none holds anything still referenced: tests keep no state outside
    // the atomics above, one CPU runs, and the panic handler masked
    // interrupts.
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

fn box_vec_btreemap_string_work() {
    let boxed = Box::new(41u64);
    assert_eq!(*boxed + 1, 42);
    let squares: Vec<u64> = (0..2000).map(|i| i * i).collect();
    assert_eq!(squares[1999], 1999 * 1999);
    let mut map = BTreeMap::new();
    for i in 0..500u32 {
        map.insert(i.wrapping_mul(2_654_435_761), i);
    }
    assert_eq!(map.len(), 500);
    assert!(map.keys().is_sorted());
    let mut text = String::new();
    for word in ["kernel", " ", "heap"] {
        text.push_str(word);
    }
    assert_eq!(text, "kernel heap");
    assert_eq!(alloc::format!("{text}-{}", 7), "kernel heap-7");
}

fn heap_stress_grows_and_reuses() {
    const BLOCKS: usize = 64;
    const BLOCK: usize = 256 * 1024;
    let run = || {
        let mut blocks = Vec::new();
        for i in 0..BLOCKS {
            let mut block = alloc::vec![i as u8; BLOCK];
            block[BLOCK - 1] = !(i as u8);
            blocks.push(block);
            // Small blocks in between, freed at once and kept in part.
            let keep: Vec<Box<u64>> = (0..200).map(|n| Box::new(n as u64)).collect();
            assert_eq!(*keep[199], 199);
        }
        for (i, block) in blocks.iter().enumerate() {
            assert_eq!(block[0], i as u8);
            assert_eq!(block[BLOCK - 1], !(i as u8));
        }
    };
    let before = heap::stats();
    run();
    let first = heap::stats();
    assert!(
        first.backing - before.backing >= BLOCKS * BLOCK / 2,
        "the stress grows the heap: {first:?}"
    );
    assert!(first.grows > before.grows);
    for _ in 0..4 {
        run();
    }
    let last = heap::stats();
    assert_eq!(last.backing, first.backing, "later rounds reuse memory");
    assert_eq!(last.live, before.live, "nothing leaked");
}

fn heap_blocks_are_aligned() {
    for shift in 0..=16 {
        let align = 1usize << shift;
        for size in [1, 100, 5000] {
            let layout = Layout::from_size_align(size, align).expect("layout");
            // SAFETY: the size is not zero.
            let p = unsafe { alloc::alloc::alloc(layout) };
            assert!(!p.is_null());
            assert!((p as usize).is_multiple_of(align));
            // SAFETY: the block has at least `size` bytes.
            unsafe { p.write_bytes(0xa5, size) };
            // SAFETY: from `alloc` above with this layout, freed once.
            unsafe { alloc::alloc::dealloc(p, layout) };
        }
    }
}

fn fallible_allocation_returns_failure() {
    let before = (heap::stats(), frames::stats());
    let mut too_big: Vec<u8> = Vec::new();
    let free_bytes = frames::stats().free * zoraya_pmm::FRAME_SIZE;
    assert!(
        too_big
            .try_reserve_exact(free_bytes as usize + (1 << 20))
            .is_err()
    );
    assert_eq!((heap::stats(), frames::stats()), before, "no trace of it");
    assert_eq!(*heap::try_box(7u32).expect("small block"), 7);
    // Heap addresses are near the top of the address space, so rounding up to
    // this alignment overflows; it must fail like any other request.
    let huge_align = Layout::from_size_align(8192, 1 << 47).expect("layout");
    // SAFETY: the size is not zero; a block would be freed below.
    let block = unsafe { alloc::alloc::alloc(huge_align) };
    assert!(block.is_null(), "no span has that alignment");
    assert_eq!(*heap::try_box(()).map(|_| 5).as_ref().expect("no block"), 5);
}

/// Drains the allocator, links every frame through the direct map, and checks
/// each against what the live device tree and the kernel say is reserved.
fn frames_avoid_every_reserved_region() {
    let (fdt, info) = platform();
    let mut forbidden: Vec<Range> = Vec::new();
    forbidden.extend(fdt.reservations().map(|r| Range::at(r.address, r.size)));
    forbidden.extend(
        info.reserved
            .as_slice()
            .iter()
            .map(|r| Range::at(r.address, r.size)),
    );
    let (blob, blob_len) = dtb::phys();
    forbidden.push(Range::at(blob as u64, blob_len as u64));
    forbidden.push(memory::kernel_image_phys());
    forbidden.push(frames::metadata());
    let ram: Vec<Range> = info
        .memory
        .as_slice()
        .iter()
        .map(|r| Range::at(r.address, r.size))
        .collect();

    let before = frames::stats();
    assert!(before.reserved > 0);
    assert_eq!(
        before.total * zoraya_pmm::FRAME_SIZE,
        ram.iter().map(Range::len).sum::<u64>()
    );

    const NO_FRAME: u64 = 0;
    let mut head = NO_FRAME;
    let mut count = 0;
    while let Ok(frame) = frames::alloc() {
        let range = Range::at(frame.addr(), zoraya_pmm::FRAME_SIZE);
        assert!(frame.addr() != 0);
        assert!(
            !forbidden.iter().any(|r| r.overlaps(&range)),
            "frame {:#x} overlaps a reserved region",
            frame.addr()
        );
        assert!(
            ram.iter()
                .any(|r| r.start <= range.start && range.end <= r.end)
        );
        // SAFETY: the frame is allocated, so this code owns it, and the
        // direct map holds it read-write.
        unsafe { (memory::phys_to_virt(frame.addr()) as *mut u64).write(head) };
        head = frame.addr();
        count += 1;
    }
    assert_eq!(count, before.free, "every free frame was returned");
    assert_eq!(frames::stats().free, 0);

    while head != NO_FRAME {
        // SAFETY: written above; the frame is still allocated.
        let next = unsafe { (memory::phys_to_virt(head) as *const u64).read() };
        frames::free(Frame::from_addr(head).expect("aligned"));
        head = next;
    }
    assert_eq!(frames::stats(), before);
}

/// How long to wait for an interrupt before failing, in nanoseconds. Far
/// longer than any lateness expected: this is a hang guard, not a limit.
const PATIENCE_NS: u64 = 10_000_000_000;

/// Spins until `done`, failing rather than hanging.
fn wait_until(done: impl Fn() -> bool) {
    let start = timer::now();
    while !done() {
        assert!(
            timer::now().nanos_since(start) < PATIENCE_NS,
            "no interrupt in time"
        );
        core::hint::spin_loop();
    }
}

const TICK_PERIOD_NS: u64 = 5_000_000;
const TICK_TARGET: usize = 40;
/// Largest shortfall of ticks at the end of the interval: 20 % of the count.
/// A tick is never early, and each is armed from the last tick's deadline, so
/// none is lost; but when the host deschedules the HVF vCPU or a TCG thread,
/// ticks arrive late and catch up after. 20 % allows a pause of 40 ms at the
/// worst moment of the 200 ms interval. Too many ticks is always a failure.
const TICK_SLACK: u64 = TICK_TARGET as u64 / 5;

static TICKS: AtomicUsize = AtomicUsize::new(0);
static TICK_ORIGIN: AtomicU64 = AtomicU64::new(0);

fn on_tick(_: usize) {
    let n = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    if n < TICK_TARGET * 2 {
        let origin = Instant::from_nanos(TICK_ORIGIN.load(Ordering::Relaxed));
        let next = origin.plus_nanos((n as u64 + 1) * TICK_PERIOD_NS);
        timer::arm(next, on_tick, 0).expect("deadline queue has room");
    }
}

fn timer_interrupts_agree_with_the_clock() {
    let start_ticks = arch::timer::counter();
    let start = timer::now();
    TICK_ORIGIN.store(start.as_nanos(), Ordering::Relaxed);
    TICKS.store(0, Ordering::Relaxed);
    timer::arm(start.plus_nanos(TICK_PERIOD_NS), on_tick, 0).expect("queue");

    let interval = TICK_TARGET as u64 * TICK_PERIOD_NS;
    wait_until(|| timer::now().nanos_since(start) >= interval);
    let count = TICKS.load(Ordering::Relaxed) as u64;
    let elapsed = timer::now().nanos_since(start);
    let end_ticks = arch::timer::counter();

    // The clock agrees with the raw counter and its frequency, to within what
    // two reads apart can differ by: the interval between them is bounded by
    // the same 40 ms allowance.
    let raw_ns =
        u128::from(end_ticks - start_ticks) * 1_000_000_000 / u128::from(arch::timer::frequency());
    let diff = (raw_ns as u64).abs_diff(elapsed);
    assert!(
        diff < TICK_SLACK * TICK_PERIOD_NS,
        "clock {elapsed}, raw {raw_ns}"
    );

    let expected = elapsed / TICK_PERIOD_NS;
    assert!(count <= expected, "{count} ticks in {elapsed} ns: too many");
    assert!(
        count + TICK_SLACK >= expected,
        "{count} ticks in {elapsed} ns: too few"
    );
    println!(
        "timer: {count} ticks in {} us at {} Hz",
        elapsed / 1000,
        timer::frequency()
    );
    wait_until(|| TICKS.load(Ordering::Relaxed) == TICK_TARGET * 2);
}

fn clock_never_decreases_across_interrupts() {
    static FIRED: AtomicBool = AtomicBool::new(false);
    fn fired(_: usize) {
        FIRED.store(true, Ordering::Relaxed);
    }
    let mut last = timer::now();
    for _ in 0..5 {
        FIRED.store(false, Ordering::Relaxed);
        timer::arm(last.plus_nanos(1_000_000), fired, 0).expect("queue");
        while !FIRED.load(Ordering::Relaxed) {
            let now = timer::now();
            assert!(now >= last, "{now:?} after {last:?}");
            last = now;
        }
    }
}

fn one_shot_deadline_is_not_early() {
    static FIRED_AT: AtomicU64 = AtomicU64::new(0);
    fn fired(_: usize) {
        FIRED_AT.store(timer::now().as_nanos(), Ordering::Relaxed);
    }
    for delay in [1, 1_000, 1_000_000, 10_000_000] {
        FIRED_AT.store(0, Ordering::Relaxed);
        let deadline = timer::now().plus_nanos(delay);
        timer::arm(deadline, fired, 0).expect("queue");
        wait_until(|| FIRED_AT.load(Ordering::Relaxed) != 0);
        let at = FIRED_AT.load(Ordering::Relaxed);
        assert!(
            at >= deadline.as_nanos(),
            "fired {at}, deadline {deadline:?}"
        );
    }
}

fn deadline_wakes_a_waiting_cpu() {
    static FIRED: AtomicBool = AtomicBool::new(false);
    fn fired(_: usize) {
        FIRED.store(true, Ordering::Relaxed);
    }
    timer::arm(timer::now().plus_nanos(1_000_000), fired, 0).expect("queue");
    // The runner's timeout is the guard: a CPU that never wakes hangs here.
    // Masked around the check, or the interrupt could land between it and
    // `wfi` and leave nothing to wake the CPU. `wfi` still wakes on an
    // interrupt that is pending but masked.
    loop {
        let daif = arch::save_and_mask_interrupts();
        let fired = FIRED.load(Ordering::Relaxed);
        if !fired {
            arch::wait_for_interrupt();
        }
        arch::restore_interrupts(daif);
        if fired {
            break;
        }
    }
}

fn deadlines_fire_in_order_and_cancel() {
    static ORDER: AtomicU64 = AtomicU64::new(0);
    fn fired(digit: usize) {
        let so_far = ORDER.load(Ordering::Relaxed);
        ORDER.store(so_far * 10 + digit as u64, Ordering::Relaxed);
    }
    ORDER.store(0, Ordering::Relaxed);
    let now = timer::now();
    timer::arm(now.plus_nanos(30_000_000), fired, 3).expect("queue");
    timer::arm(now.plus_nanos(10_000_000), fired, 1).expect("queue");
    let gone = timer::arm(now.plus_nanos(20_000_000), fired, 9).expect("queue");
    timer::arm(now.plus_nanos(20_000_000), fired, 2).expect("queue");
    assert!(timer::cancel(gone));
    assert!(!timer::cancel(gone), "cancelled once");
    wait_until(|| ORDER.load(Ordering::Relaxed) >= 100);
    assert_eq!(ORDER.load(Ordering::Relaxed), 123);
}

fn timer_is_off_with_no_deadlines() {
    assert!(!arch::timer::is_enabled(), "no deadline is queued");
    let id = timer::arm(timer::now().plus_nanos(1 << 40), |_| {}, 0).expect("queue");
    assert!(arch::timer::is_enabled());
    assert!(timer::cancel(id));
    assert!(!arch::timer::is_enabled());
}

fn shared_interrupt_is_delivered() {
    static TAKEN: AtomicUsize = AtomicUsize::new(0);
    fn handler() {
        TAKEN.fetch_add(1, Ordering::Relaxed);
    }
    let (_, info) = platform();
    let uart = info
        .uart
        .interrupt
        .expect("UART interrupt in the device tree");
    let spurious = irq::spurious_count();
    irq::register(uart.intid, uart.edge, handler);
    for n in 1..=3 {
        gic::trigger(uart.intid);
        wait_until(|| TAKEN.load(Ordering::Relaxed) == n);
    }
    irq::unregister(uart.intid);
    assert_eq!(irq::spurious_count(), spurious);
}

fn interrupt_preserves_fp_registers() {
    static FIRED: AtomicBool = AtomicBool::new(false);
    fn clobber(_: usize) {
        // SAFETY: only caller-saved vector registers, declared as clobbered;
        // the vectors restore them for the interrupted code.
        unsafe {
            core::arch::asm!(
                "movi v0.16b, #0x11",
                "movi v15.16b, #0x22",
                "movi v31.16b, #0x33",
                "msr fpcr, xzr",
                out("v0") _, out("v15") _, out("v31") _,
                options(nomem, nostack, preserves_flags),
            )
        };
        FIRED.store(true, Ordering::Relaxed);
    }
    FIRED.store(false, Ordering::Relaxed);
    timer::arm(timer::now().plus_nanos(1_000_000), clobber, 0).expect("queue");
    const FPCR_RMODE_UP: u64 = 0b01 << 22;
    let same: u64;
    let fpcr: u64;
    // SAFETY: only the named registers are used; the loop ends when the
    // interrupt sets FIRED, which the deadline above guarantees. The rounding
    // mode is put back before Rust code does any arithmetic.
    unsafe {
        core::arch::asm!(
            "movi v0.16b, #0xa5",
            "movi v15.16b, #0x5a",
            "movi v31.16b, #0xc3",
            "msr fpcr, {rmode}",
            "2:",
            "ldrb {t:w}, [{fired}]",
            "cbz {t:w}, 2b",
            "umov {a}, v0.d[1]",
            "umov {b}, v15.d[1]",
            "eor {a}, {a}, {b}",
            "umov {b}, v31.d[0]",
            "eor {a}, {a}, {b}",
            "mrs {f}, fpcr",
            "msr fpcr, xzr",
            rmode = in(reg) FPCR_RMODE_UP,
            f = out(reg) fpcr,
            t = out(reg) _,
            a = out(reg) same,
            b = out(reg) _,
            fired = in(reg) (&raw const FIRED),
            out("v0") _, out("v15") _, out("v31") _,
            options(nostack),
        )
    };
    let expected = 0xa5a5_a5a5_a5a5_a5a5u64 ^ 0x5a5a_5a5a_5a5a_5a5a ^ 0xc3c3_c3c3_c3c3_c3c3;
    assert_eq!(same, expected);
    assert_eq!(fpcr, FPCR_RMODE_UP);
}

/// Next free SPI ID in QEMU virt's distributor that no device uses in the
/// tests. Its only use is to be raised by hand.
const UNUSED_SPI: u32 = 40;

fn fault_report_for_unexpected_interrupt() {
    gic::enable(UNUSED_SPI, false);
    gic::trigger(UNUSED_SPI);
    wait_until(|| false);
}

fn interrupts_still_arrive_after_a_fault() {
    one_shot_deadline_is_not_early();
}

fn kernel_threads_take_turns() {
    static TURNS: AtomicUsize = AtomicUsize::new(0);
    fn take_turns(me: usize) {
        for round in 0..50 {
            while TURNS.load(Ordering::Relaxed) != round * 2 + me {
                sched::yield_now();
            }
            TURNS.fetch_add(1, Ordering::Relaxed);
        }
    }
    TURNS.store(0, Ordering::Relaxed);
    sched::spawn_kernel(take_turns, 0).expect("thread");
    sched::spawn_kernel(take_turns, 1).expect("thread");
    wait_until_yielding(|| TURNS.load(Ordering::Relaxed) == 100);
}

/// As `wait_until`, for what another thread has to do.
fn wait_until_yielding(done: impl Fn() -> bool) {
    let start = timer::now();
    while !done() {
        assert!(timer::now().nanos_since(start) < PATIENCE_NS, "no progress");
        sched::yield_now();
    }
}

fn sleep_ends_no_earlier_than_its_deadline() {
    for delay in [1, 1_000_000, 5_000_000] {
        let deadline = timer::now().plus_nanos(delay);
        sched::sleep_until(Deadline::At(deadline)).expect("queue");
        assert!(timer::now() >= deadline);
    }
}

fn thread_stack_has_a_guard_page() {
    let stack = KernelStack::new().expect("stack");
    let bottom = stack.bottom();
    assert!(paging::translate(bottom).is_some());
    assert!(paging::translate(stack.top() - 1).is_some());
    assert!(paging::translate(bottom - PAGE_SIZE).is_none());
    drop(stack);
    assert!(paging::translate(bottom).is_none(), "unmapped when freed");
}

fn run_program(name: &str) -> ExitRecord {
    process::spawn(name).expect("program in the ramdisk").wait()
}

fn exited(code: i32) -> ExitRecord {
    ExitRecord {
        code,
        reason: ExitReason::Exited,
        detail: 0,
    }
}

fn user_process_prints_and_exits() {
    assert_eq!(run_program("hello"), exited(7));
}

fn user_tables_obey_wx() {
    let process = process::spawn("spin").expect("spin");
    let mut count = 0;
    process.for_each_mapping(&mut |m| {
        count += 1;
        let a = m.attributes;
        assert!(a.user, "{m:?}");
        assert!(!(a.write && a.execute), "W+X at {:#x}", m.va);
        assert_eq!(a.memory, MemoryType::Normal);
        assert!(m.va >= USER_LOW && m.va < USER_HIGH, "{m:?}");
    });
    assert!(count > 3);
    assert_eq!(process.wait(), exited(1));
    let mut left = 0;
    process.for_each_mapping(&mut |_| left += 1);
    assert_eq!(left, 0, "memory released at exit");
}

fn user_faults_end_the_process_only() {
    /// Exception classes: unknown, WFI trapped, instruction and data abort
    /// from EL0, PC alignment, BRK.
    const CASES: [(&str, u32); 10] = [
        ("f_kread", 0x24),
        ("f_kwrite", 0x24),
        ("f_null", 0x24),
        ("f_priv", 0x00),
        ("f_undef", 0x00),
        ("f_brk", 0x3c),
        ("f_exdata", 0x20),
        ("f_stack", 0x24),
        ("f_wfi", 0x01),
        ("f_align", 0x22),
    ];
    for (name, class) in CASES {
        let record = run_program(name);
        assert_eq!(record.reason, ExitReason::Fault, "{name}");
        assert_eq!(record.detail, class, "{name}");
    }
}

fn bad_syscall_arguments_return_errors() {
    assert_eq!(run_program("badptr"), exited(0));
}

fn random_syscalls_do_not_hurt_the_kernel() {
    assert_eq!(run_program("fuzz"), exited(0));
}

fn processes_cannot_see_each_other() {
    let a = process::spawn("isolate").expect("isolate");
    let b = process::spawn("isolate").expect("isolate");
    assert_eq!(a.wait(), exited(0));
    assert_eq!(b.wait(), exited(0));
}

fn cpu_bound_processes_share_the_cpu() {
    let before = sched::preemptions();
    let start = timer::now();
    let a = process::spawn("spin").expect("spin");
    let b = process::spawn("spin").expect("spin");
    assert_eq!(a.wait(), exited(1));
    assert_eq!(b.wait(), exited(1));
    let elapsed = timer::now().nanos_since(start);
    let preempted = sched::preemptions() - before;
    assert!(preempted >= 2, "{preempted} preemptions in {elapsed} ns");
    println!("preemption: {preempted} in {} us", elapsed / 1000);
}

fn registers_survive_preemption() {
    let a = process::spawn("regs").expect("regs");
    let b = process::spawn("regs").expect("regs");
    assert_eq!(a.wait(), exited(0));
    assert_eq!(b.wait(), exited(0));
}

/// A minimal ELF header with one `PT_LOAD`, for what the build cannot make.
fn one_segment_elf(flags: u32, vaddr: u64) -> Vec<u8> {
    let mut f = alloc::vec![0u8; 0x2000];
    f[..4].copy_from_slice(b"\x7fELF");
    f[4..7].copy_from_slice(&[2, 1, 1]);
    f[16..18].copy_from_slice(&2u16.to_le_bytes());
    f[18..20].copy_from_slice(&183u16.to_le_bytes());
    f[24..32].copy_from_slice(&vaddr.to_le_bytes());
    f[32..40].copy_from_slice(&64u64.to_le_bytes());
    f[54..56].copy_from_slice(&56u16.to_le_bytes());
    f[56..58].copy_from_slice(&1u16.to_le_bytes());
    let ph = &mut f[64..120];
    ph[..4].copy_from_slice(&1u32.to_le_bytes());
    ph[4..8].copy_from_slice(&flags.to_le_bytes());
    ph[8..16].copy_from_slice(&0x1000u64.to_le_bytes());
    ph[16..24].copy_from_slice(&vaddr.to_le_bytes());
    ph[32..40].copy_from_slice(&0x100u64.to_le_bytes());
    ph[40..48].copy_from_slice(&0x100u64.to_le_bytes());
    f
}

fn bad_programs_are_refused() {
    const RX: u32 = 5;
    const RWX: u32 = 7;
    let refused = |file: &[u8]| process::spawn_bytes("bad", file).err();
    assert_eq!(
        refused(&one_segment_elf(RWX, USER_LOW)),
        Some(SpawnError::BadProgram(ElfError::WritableExecutable))
    );
    assert_eq!(
        refused(&one_segment_elf(RX, 0x1000)),
        Some(SpawnError::BadProgram(ElfError::OutOfRange))
    );
    assert_eq!(
        refused(&one_segment_elf(RX, 0xffff_ffff_8000_0000)),
        Some(SpawnError::BadProgram(ElfError::OutOfRange))
    );
    assert_eq!(
        refused(b"not a program"),
        Some(SpawnError::BadProgram(ElfError::Truncated))
    );
    assert_eq!(
        process::spawn("no such file").err(),
        Some(SpawnError::NotFound)
    );
}

fn exited_processes_leave_nothing_behind() {
    // The first round settles the heap and the slot list.
    for name in ["hello", "f_null"] {
        run_program(name);
    }
    sched::yield_now();
    let before = frames::stats();
    for _ in 0..20 {
        assert_eq!(run_program("hello"), exited(7));
        assert_eq!(run_program("f_null").reason, ExitReason::Fault);
    }
    sched::yield_now();
    assert_eq!(frames::stats(), before);
}

fn running_out_of_asids_is_an_error() {
    let mut live = Vec::new();
    let failure = loop {
        match process::spawn("hello") {
            Ok(process) => live.push(process),
            Err(e) => break e,
        }
        assert!(live.len() <= 255, "more live processes than ASIDs");
    };
    assert_eq!(failure, SpawnError::NoMemory);
    assert_eq!(live.len(), 255, "ASID 0 belongs to no process");
    for process in live {
        assert_eq!(process.wait(), exited(7));
    }
    assert_eq!(run_program("hello"), exited(7), "ASIDs come back");
}

fn ttbr0_is_off_with_no_process() {
    /// `TCR_EL1.EPD0`.
    const EPD0: u64 = 1 << 7;
    assert!(mmu::tcr() & EPD0 != 0);
    assert_eq!(mmu::ttbr0(), 0);
    expect_data_abort(read_at, USER_LOW, FSC_TRANSLATION, false);
}

fn wait_keeps_the_interrupt_mask() {
    let before = arch::interrupt_mask();
    assert_eq!(run_program("hello"), exited(7));
    assert_eq!(arch::interrupt_mask(), before);
}

fn every_waiter_is_woken() {
    static DONE: AtomicUsize = AtomicUsize::new(0);
    fn waiter(raw: usize) {
        // SAFETY: from `Arc::into_raw` below, once per waiter.
        let process = unsafe { Arc::from_raw(raw as *const process::Process) };
        assert_eq!(process.wait(), exited(1));
        DONE.fetch_add(1, Ordering::Relaxed);
    }
    DONE.store(0, Ordering::Relaxed);
    let process = process::spawn("spin").expect("spin");
    for _ in 0..3 {
        let raw = Arc::into_raw(process.clone()) as usize;
        sched::spawn_kernel(waiter, raw).expect("thread");
    }
    assert_eq!(process.wait(), exited(1));
    wait_until_yielding(|| DONE.load(Ordering::Relaxed) == 3);
}

fn registers_survive_system_calls() {
    assert_eq!(run_program("regsave"), exited(0));
}

/// INV-0030-1: the program reads the timer while it is the only thread that
/// can run: the boot thread blocks in `wait`, and no deadline is queued.
fn lone_process_has_no_timer() {
    assert_eq!(run_program("lone"), exited(0));
}

fn full_deadline_queue_keeps_the_slice() {
    fn nothing(_: usize) {}
    let far = timer::now().plus_nanos(1 << 50);
    let mut ids = Vec::new();
    while let Ok(id) = timer::arm(far, nothing, 0) {
        ids.push(id);
    }
    let before = sched::preemptions();
    let a = process::spawn("spin").expect("spin");
    let b = process::spawn("spin").expect("spin");
    assert_eq!(a.wait(), exited(1));
    assert_eq!(b.wait(), exited(1));
    for id in ids {
        assert!(timer::cancel(id));
    }
    assert!(sched::preemptions() - before >= 2, "no slice was armed");
}

/// For the test-only system call: faults on the kernel's own destination in
/// the copy routine. `catch` sees it only if `fixup` does not resume it.
pub fn copy_to_bad_kernel(user_source: u64) -> u64 {
    extern "C" fn copy(source: usize) {
        // SAFETY: under `catch`, to an address that faults.
        let result = unsafe { arch::copy_from_user_to(UNMAPPED as *mut u8, source as u64, 1) };
        black_box(result.is_err());
    }
    match arch::catch(copy, user_source as usize) {
        Some(fault) if fault.ec == EC_DATA_ABORT && fault.far == Some(UNMAPPED) => 1,
        _ => 0,
    }
}

/// INV-0036-1, with `badptr` showing the other side.
fn copy_fault_on_kernel_memory_is_not_resumed() {
    assert_eq!(run_program("copyfar"), exited(0));
}

/// The test-only system call that makes the kernel overflow a thread's stack.
pub fn overflow_stack() {
    black_box(use_stack(0));
}

fn fault_report_for_stack_overflow_from_el0() {
    let _ = run_program("koverflow");
}

fn fault_report_for_thread_stack_overflow() {
    fn overflow(_: usize) {
        black_box(use_stack(0));
    }
    sched::spawn_kernel(overflow, 0).expect("thread");
    // The panic in that thread ends this test, through `on_panic`.
    wait_until_yielding(|| false);
}

/// Between the windows and the image, where nothing is ever mapped.
const UNMAPPED: u64 = KERNEL_IMAGE - 0x2000;

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
    expect_data_abort(read_at, UNMAPPED, FSC_TRANSLATION, false);
    expect_data_abort(write_at, UNMAPPED, FSC_TRANSLATION, true);
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

fn gic_is_reached_through_the_device_window() {
    let (_, info) = platform();
    let ic = info.interrupt_controller.expect("interrupt controller");
    let va = gic::distributor() as u64;
    assert!((DEVICE_WINDOW..DEVICE_WINDOW + DEVICE_WINDOW_SIZE).contains(&va));
    let m = paging::translate(va).expect("distributor mapping");
    assert_eq!(m.pa + (va - m.va), ic.regs()[0].address);
    assert_eq!(m.attributes.memory, MemoryType::Device);
    // QEMU virt implements more than the 32 private IDs plus the UART's.
    let lines = gic::line_count();
    assert!(lines > 32 && lines <= 1020, "{lines} lines");
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
    read_at(UNMAPPED as usize);
}

fn fault_report_for_el0_exception() {
    arch::enter_el0(0x1000);
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

/// The three overflow reports above are expected panics, but a panic alone
/// proves little: with no limit in `SP_EL0`, an overflow from a system call
/// runs on through the guard page into the stack below it, and panics anyway.
fn every_overflow_reached_the_limit_check() {
    assert_eq!(arch::STACK_OVERFLOWS.load(Ordering::Relaxed), 3);
}

/// Runs after the stack overflow report, which is the deepest use of the
/// emergency stack: the panic path of the debug build, formatting included.
/// The stack is in memory the ELF loader zeroes.
fn emergency_stack_kept_a_margin() {
    const MARGIN: usize = 4096;
    let bottom = addr_of!(memory::__emergency_stack_bottom);
    let top = addr_of!(memory::__emergency_stack_top) as usize;
    let size = top - bottom as usize;
    // SAFETY: the range is the emergency stack, mapped read-write, and
    // nothing runs on it now.
    let stack = unsafe { core::slice::from_raw_parts(bottom, size) };
    let untouched = stack.iter().take_while(|b| **b == 0).count();
    assert!(
        untouched >= MARGIN,
        "only {untouched} of {size} bytes left unused"
    );
    println!("emergency stack: {} of {size} bytes used", size - untouched);
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

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

use zoraya_abi::spawn::Strings;
use zoraya_abi::{
    Disposition, ExitReason, ExitRecord, Handle, REQUEST_KINDS, RequestKind, process_signals,
};
use zoraya_dtb::Fdt;
use zoraya_dtb::platform::PlatformInfo;
use zoraya_elf::{Error as ElfError, Program};
use zoraya_pagetable::{MemoryType, PAGE_SIZE};
use zoraya_pmm::{Frame, Range};
use zoraya_time::{Deadline, Instant};

use crate::arch::mmu;
use crate::job::Job;
use crate::memory::layout::{DEVICE_WINDOW, DEVICE_WINDOW_SIZE, KERNEL_IMAGE};
use crate::memory::stack::KernelStack;
use crate::memory::user::{USER_HIGH, USER_LOW};
use crate::memory::{frames, heap, paging};
use crate::object::{self, HandleTable};
use crate::process::{self, SpawnError};
use crate::sync::SpinLock;
use crate::{arch, console, dtb, gic, irq, memory, port, println, ramdisk, sched, timer};

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
    Test {
        name: "spawned_child_is_waited_for_through_a_port",
        run: spawned_child_is_waited_for_through_a_port,
    },
    Test {
        name: "handles_and_rights_hold_from_user_space",
        run: handles_and_rights_hold_from_user_space,
    },
    Test {
        name: "spawn_keeps_its_rules",
        run: spawn_keeps_its_rules,
    },
    Test {
        name: "requests_and_jobs_keep_their_rules",
        run: requests_and_jobs_keep_their_rules,
    },
    Test {
        name: "ports_keep_their_rules",
        run: ports_keep_their_rules,
    },
    Test {
        name: "many_processes_come_and_go",
        run: many_processes_come_and_go,
    },
    Test {
        name: "suspended_process_stays_at_its_entry",
        run: suspended_process_stays_at_its_entry,
    },
    Test {
        name: "terminate_stops_user_code_at_once",
        run: terminate_stops_user_code_at_once,
    },
    Test {
        name: "closing_a_port_ends_a_blocked_wait",
        run: closing_a_port_ends_a_blocked_wait,
    },
    Test {
        name: "disposition_change_ends_at_once",
        run: disposition_change_ends_at_once,
    },
    Test {
        name: "job_signal_reaches_every_process_of_its_tree",
        run: job_signal_reaches_every_process_of_its_tree,
    },
    Test {
        name: "nothing_is_left_after_spawn_exit_close",
        run: nothing_is_left_after_spawn_exit_close,
    },
    Test {
        name: "an_ended_process_holds_only_its_record",
        run: an_ended_process_holds_only_its_record,
    },
    Test {
        name: "port_wait_ignores_idle_subscriptions",
        run: port_wait_ignores_idle_subscriptions,
    },
    Test {
        name: "try_arc_reports_failure",
        run: try_arc_reports_failure,
    },
    Test {
        name: "requests_come_out_in_order",
        run: requests_come_out_in_order,
    },
    Test {
        name: "an_ended_process_asserts_only_terminated",
        run: an_ended_process_asserts_only_terminated,
    },
    Test {
        name: "failed_allocation_frees_each_object_once",
        run: failed_allocation_frees_each_object_once,
    },
    Test {
        name: "console_and_listing_from_a_process",
        run: console_and_listing_from_a_process,
    },
    Test {
        name: "memory_objects_from_a_process",
        run: memory_objects_from_a_process,
    },
    Test {
        name: "channels_from_a_process",
        run: channels_from_a_process,
    },
    Test {
        name: "devices_from_a_process",
        run: devices_from_a_process,
    },
    Test {
        name: "a_device_with_no_transport_keeps_its_quarantine",
        run: a_device_with_no_transport_keeps_its_quarantine,
    },
    Test {
        name: "no_leak_after_rounds_of_objects",
        run: no_leak_after_rounds_of_objects,
    },
    Test {
        name: "channel_measurements",
        run: channel_measurements,
    },
    Test {
        name: "storage_from_a_process",
        run: storage_from_a_process,
    },
    Test {
        name: "gui_from_a_process",
        run: gui_from_a_process,
    },
    Test {
        name: "network_driver_answers_arp",
        run: network_driver_answers_arp,
    },
    Test {
        name: "network_service_answers_ping_and_dhcp",
        run: network_service_answers_ping_and_dhcp,
    },
    Test {
        name: "text_renders_the_reference_digests",
        run: text_renders_the_reference_digests,
    },
    // Last, so that it covers every test before it.
    Test {
        name: "streams_from_a_process",
        run: streams_from_a_process,
    },
    Test {
        name: "images_from_a_process",
        run: images_from_a_process,
    },
    Test {
        name: "job_suspend_from_a_process",
        run: job_suspend_from_a_process,
    },
    Test {
        name: "quotas_from_a_process",
        run: quotas_from_a_process,
    },
    Test {
        name: "kernel_work_is_bounded",
        run: kernel_work_is_bounded,
    },
    Test {
        name: "cpu_ceiling_from_a_process",
        run: cpu_ceiling_from_a_process,
    },
    Test {
        name: "snapshots_from_a_process",
        run: snapshots_from_a_process,
    },
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
        sched::set_test_slice(0);
        crate::cpu::set_test_period(0);
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
    // INV-0063-2 and ADR-0053: `hello` ends with no call to power off.
    crate::run_first("hello")
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
    let before = heap::settled_stats();
    run();
    let first = heap::settled_stats();
    assert!(
        first.backing - before.backing >= BLOCKS * BLOCK / 2,
        "the stress grows the heap: {first:?}"
    );
    assert!(first.grows > before.grows);
    for _ in 0..4 {
        run();
    }
    let last = heap::settled_stats();
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
    let before = (heap::settled_stats(), frames::settled_stats());
    let mut too_big: Vec<u8> = Vec::new();
    let free_bytes = frames::settled_stats().free * zoraya_pmm::FRAME_SIZE;
    assert!(
        too_big
            .try_reserve_exact(free_bytes as usize + (1 << 20))
            .is_err()
    );
    assert_eq!(
        (heap::settled_stats(), frames::settled_stats()),
        before,
        "no trace of it"
    );
    assert_eq!(*heap::uncharged_box(7u32).expect("small block"), 7);
    // Heap addresses are near the top of the address space, so rounding up to
    // this alignment overflows; it must fail like any other request.
    let huge_align = Layout::from_size_align(8192, 1 << 47).expect("layout");
    // SAFETY: the size is not zero; a block would be freed below.
    let block = unsafe { alloc::alloc::alloc(huge_align) };
    assert!(block.is_null(), "no span has that alignment");
    assert_eq!(
        *heap::uncharged_box(())
            .map(|_| 5)
            .as_ref()
            .expect("no block"),
        5
    );
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

    let before = frames::settled_stats();
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
    assert_eq!(frames::settled_stats().free, 0);

    while head != NO_FRAME {
        // SAFETY: written above; the frame is still allocated.
        let next = unsafe { (memory::phys_to_virt(head) as *const u64).read() };
        frames::free(Frame::from_addr(head).expect("aligned"));
        head = next;
    }
    assert_eq!(frames::settled_stats(), before);
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

/// ADR-0060 to ADR-0062 from a process, with input put in the ring by hand.
fn console_and_listing_from_a_process() {
    console::receive(b'a');
    console::receive(b'b');
    // The ring's whole size, for a read as long as it.
    for _ in 2..zoraya_console::INPUT_SIZE {
        console::receive(b'c');
    }
    // It ends with the number of names it listed.
    assert_eq!(
        run_program("t_console"),
        exited(ramdisk::get().len() as i32)
    );
    // INV-0159-1: a `Power` handle without `POWER` (review K1). The program
    // returns 0 only if both calls were refused and the machine went on.
    let rights = <zoraya_abi::PowerObject as zoraya_abi::ObjectType>::RIGHTS
        .without(zoraya_abi::Rights::POWER);
    let extra = (zoraya_abi::POWER.slot(), object::Object::Power, rights);
    let process = process::spawn_with_devices_and("t_console", Some(extra));
    assert_eq!(process.expect("t_console").wait(), exited(0));
}

/// ADR-0051 from a process, and INV-0051-1 and -4 by the counters: after it
/// ends, the frames and heap are as they were.
fn memory_objects_from_a_process() {
    let frames_before = frames::settled_stats().free;
    let heap_before = heap::settled_stats().live;
    assert_eq!(run_program("t_memory"), exited(0));
    assert_eq!(frames::settled_stats().free, frames_before);
    assert_eq!(heap::settled_stats().live, heap_before);
}

/// ADR-0050 from a process. It ends with nothing left in the kernel.
fn channels_from_a_process() {
    // The first run grows the signal table to its peak, which it keeps: every
    // channel end is two entries of a table that does not shrink. A second run
    // must then change nothing.
    assert_eq!(run_program("t_channels"), exited(0));
    let frames_before = frames::settled_stats().free;
    let heap_before = heap::settled_stats().live;
    assert_eq!(run_program("t_channels"), exited(0));
    assert_eq!(frames::settled_stats().free, frames_before);
    assert_eq!(heap::settled_stats().live, heap_before);
}

/// ADR-0054 and ADR-0070 from a process, with the test disk attached. What it
/// pinned is gone at the end.
fn devices_from_a_process() {
    let run = || {
        process::spawn_with_devices("t_devices")
            .expect("program in the ramdisk")
            .wait()
    };
    assert_eq!(run(), exited(0));
    // Twice, so that the heap's growth is behind the counts.
    let frames_before = frames::settled_stats().free;
    let heap_before = heap::settled_stats().live;
    assert_eq!(run(), exited(0));
    assert_eq!(frames::settled_stats().free, frames_before);
    assert_eq!(heap::settled_stats().live, heap_before);
}

/// Leaks: rounds of channels with handles in flight, mappings and pins, with
/// the counts compared across a run.
fn no_leak_after_rounds_of_objects() {
    let run = || {
        process::spawn_with_devices("t_loop")
            .expect("program in the ramdisk")
            .wait()
    };
    assert_eq!(run(), exited(0));
    let frames_before = frames::settled_stats().free;
    let heap_before = heap::settled_stats().live;
    assert_eq!(run(), exited(0));
    assert_eq!(frames::settled_stats().free, frames_before);
    assert_eq!(heap::settled_stats().live, heap_before);
}

/// Runs `run` twice and checks that the second run left the frame and heap
/// counts as it found them (INV-0041-1): the first grows what grows once,
/// such as the signal table and the heap's chunks.
fn twice_with_nothing_left(run: impl Fn() -> ExitRecord) {
    assert_eq!(run(), exited(0));
    let frames_before = frames::settled_stats().free;
    let heap_before = heap::settled_stats().live;
    assert_eq!(run(), exited(0));
    assert_eq!(frames::settled_stats().free, frames_before, "frames");
    assert_eq!(heap::settled_stats().live, heap_before, "heap");
}

/// ADR-0101 from a process (INV-0101-1 to -4), and the stream leak loop.
fn streams_from_a_process() {
    twice_with_nothing_left(|| run_program("t_streams"));
}

/// ADR-0108 and ADR-0109 item 5 from a process. It gets the ramdisk's
/// `hello` in a memory object: length at 0, the object's size at 8, the file
/// from 4096 (INV-0108-2 writes it to the test volume and runs it from there).
fn images_from_a_process() {
    use crate::memory::object::MemoryObject;
    let (_, file) = ramdisk::find("hello").expect("hello");
    let size = (PAGE_SIZE + file.len() as u64).next_multiple_of(PAGE_SIZE);
    twice_with_nothing_left(|| {
        let object = MemoryObject::new(size, false, None).expect("memory");
        assert!(object.write(0, &(file.len() as u64).to_le_bytes()));
        assert!(object.write(8, &size.to_le_bytes()));
        assert!(object.write(PAGE_SIZE, file));
        let rights = <zoraya_abi::MemoryObject as zoraya_abi::ObjectType>::RIGHTS;
        let extra = (100, object::Object::Memory(object), rights);
        process::spawn_with_devices_and("t_image", Some(extra))
            .expect("program in the ramdisk")
            .wait()
    });
}

/// ADR-0105 item 8 (INV-0105-5) and the latency of suspending 16 processes.
fn job_suspend_from_a_process() {
    assert_eq!(run_program("t_suspend"), exited(0));
}

/// ADR-0261 and ADR-0263: limits on jobs, from processes.
fn quotas_from_a_process() {
    assert_eq!(run_program("t_quota"), exited(0));
}

/// ADR-0264: the CPU ceiling, parking and the wake-up latency next to capped
/// rooms (INV-0264-1, -2, -4 and the kernel half of -3).
fn cpu_ceiling_from_a_process() {
    assert_eq!(run_program("t_cpu"), exited(0));
}

/// ADR-0270, INV-0270-1 and -2: the longest masked time over every call that
/// scales, measured by the test kernel's counter.
fn kernel_work_is_bounded() {
    // With the device handles: the program snapshots itself (`posix` job).
    let record = process::spawn_with_devices("t_bound")
        .expect("program in the ramdisk")
        .wait();
    assert_eq!(record, exited(0));
}

/// ADR-0102 from a process: the criterion and every INV-0102 check, with the
/// device handles for INV-0102-5.
fn snapshots_from_a_process() {
    twice_with_nothing_left(|| {
        process::spawn_with_devices("t_snapshot")
            .expect("program in the ramdisk")
            .wait()
    });
}

/// The storage stack: failure paths and hostile clients, on the test disk.
fn storage_from_a_process() {
    let record = process::spawn_with_devices("t_storage")
        .expect("program in the ramdisk")
        .wait();
    assert_eq!(record, exited(0));
}

/// The GUI stack (ADR-0090 to ADR-0097): hostile clients, focus, driver
/// restarts and a measured full redraw, on the GUI devices when a
/// configuration attaches them. Without them it passes having done nothing.
fn gui_from_a_process() {
    let record = process::spawn_with_devices("t_gui")
        .expect("program in the ramdisk")
        .wait();
    assert_eq!(record, exited(0));
}

/// N1 (ADR-0221, ADR-0230): `ethd` on a virtio-net device under `--net`,
/// ARP through its transmit and receive paths. Without the device it passes
/// having done nothing.
fn network_driver_answers_arp() {
    let record = process::spawn_with_devices("t_net")
        .expect("program in the ramdisk")
        .wait();
    assert_eq!(record, exited(0));
}

/// N2 (ADR-0221, ADR-0229): `netd` over `ethd` gets the DHCP lease and pings
/// the gateway, and survives neither being killed with frames in flight. It
/// passes having done nothing without the device.
fn network_service_answers_ping_and_dhcp() {
    let record = process::spawn_with_devices("t_netd")
        .expect("program in the ramdisk")
        .wait();
    assert_eq!(record, exited(0));
}

/// INV-0162-2 in QEMU: the fonts render to the host's digests, and the
/// grid measurement of 7a (`t_text`).
fn text_renders_the_reference_digests() {
    assert_eq!(run_program("t_text"), exited(0));
}

/// plan.md's measurements: printed, and not judged.
fn channel_measurements() {
    assert_eq!(run_program("chanbench"), exited(0));
}

/// INV-0070-3: with no virtio transport nothing proves the device stopped, so
/// the quarantine stays.
fn a_device_with_no_transport_keeps_its_quarantine() {
    let (_, info) = platform();
    let window = memory::paging::map_device(info.uart.base, 4096) as usize;
    let device = crate::device::without_transport(info.uart.base, window);
    let frames_before = frames::settled_stats().free;
    let memory = crate::memory::object::MemoryObject::new(4096, false, None).expect("memory");
    let pin = crate::device::Pin::new(device.clone(), memory).expect("pin");
    assert_eq!(
        device.release_quarantine(),
        Err(zoraya_abi::Status::BadState)
    );
    drop(pin);
    assert_eq!(device.quarantined(), 1);
    assert_eq!(
        device.release_quarantine(),
        Err(zoraya_abi::Status::BadState)
    );
    assert_eq!(device.quarantined(), 1, "nothing proved a stop");
    assert!(frames::settled_stats().free < frames_before);
    drop(device);
}

fn run_program(name: &str) -> ExitRecord {
    process::spawn(name).expect("program in the ramdisk").wait()
}

fn exited(code: i32) -> ExitRecord {
    ExitRecord {
        code,
        reason: ExitReason::Exited,
        detail: 0,
        request_kind: None,
    }
}

fn user_process_prints_and_exits() {
    assert_eq!(run_program("hello"), exited(7));
}

fn user_tables_obey_wx() {
    let process = process::spawn("busy").expect("busy");
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
    let a = process::spawn("busy").expect("busy");
    let b = process::spawn("busy").expect("busy");
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
    let before = frames::settled_stats();
    for _ in 0..20 {
        assert_eq!(run_program("hello"), exited(7));
        assert_eq!(run_program("f_null").reason, ExitReason::Fault);
    }
    sched::yield_now();
    assert_eq!(frames::settled_stats(), before);
}

/// The 255 processes are held suspended, and run one at a time afterwards.
/// A `hello` that runs takes a heap step of `zoraya_heap::GROW_SIZE` (1 MiB)
/// for its first allocation, so 255 of them running at once would need more
/// than the 128 MiB of most configurations: how many were part-way through
/// at the same time depended on how far each got in its slices, and a slow
/// host failed the test with "memory allocation of 5 bytes failed" (exit
/// 101; docs/backlog.md). The short slice stands for such a host: with the
/// processes running while they were spawned, 219 of the 255 failed so.
fn running_out_of_asids_is_an_error() {
    /// Frames one running `hello` may need on top of its suspended self:
    /// its heap step, twice over for the step's tables and what else it
    /// takes while it runs.
    const ROOM: u64 = 2 * (zoraya_heap::GROW_SIZE as u64) / zoraya_pmm::FRAME_SIZE;
    sched::set_test_slice(500_000);
    let before = frames::settled_stats().free;
    let mut live = Vec::new();
    let failure = loop {
        match process::spawn_suspended("hello") {
            Ok(process) => live.push(process),
            Err(e) => break Some(e),
        }
        if live.len() > 255 {
            break None;
        }
    };
    let free = frames::settled_stats().free;
    println!(
        "running_out_of_asids: {} suspended processes take {} frames, {free} stay free",
        live.len(),
        before.saturating_sub(free)
    );
    // Nothing is asserted until every process has ended: a failed test does
    // not unwind (`on_panic`), so a panic while they are held would keep
    // their ASIDs and frames from every later test.
    let held = live.len();
    let room = free >= ROOM;
    let mut records = Vec::new();
    let mut again = None;
    if failure == Some(SpawnError::NoMemory) && held == 255 && room {
        for (i, process) in live.into_iter().enumerate() {
            process.resume().expect("resume");
            records.push(process.wait());
            if i == 0 {
                // The first one's ASID comes back while 254 are still held.
                again = Some(process::spawn("hello").map(|p| p.wait()));
            }
        }
    } else {
        // Ended before their first instruction (`user_thread_start`).
        for process in &live {
            let _ = process.signal(RequestKind::Terminate, 0);
            let _ = process.resume();
        }
        for process in live {
            process.wait();
        }
    }
    assert_eq!(failure, Some(SpawnError::NoMemory));
    assert_eq!(held, 255, "ASID 0 belongs to no process");
    assert!(room, "no room for a running process beside them");
    assert!(records.iter().all(|r| *r == exited(7)), "{records:?}");
    assert_eq!(again, Some(Ok(exited(7))), "an ASID comes back at once");
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
    let process = process::spawn("busy").expect("busy");
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
    let a = process::spawn("busy").expect("busy");
    let b = process::spawn("busy").expect("busy");
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

/// As above, with the fault on the kernel's own source of a copy to the user.
pub fn copy_from_bad_kernel(user_target: u64) -> u64 {
    extern "C" fn copy(target: usize) {
        // SAFETY: under `catch`, from an address that faults.
        let result = unsafe { arch::copy_to_user_from(target as u64, UNMAPPED as *const u8, 1) };
        black_box(result.is_err());
    }
    match arch::catch(copy, user_target as usize) {
        Some(fault) if fault.ec == EC_DATA_ABORT && fault.far == Some(UNMAPPED) => 1,
        _ => 0,
    }
}

/// INV-0036-1, with `badptr` showing the other side.
fn copy_fault_on_kernel_memory_is_not_resumed() {
    assert_eq!(run_program("copyfar"), exited(0));
}

fn exited_in(name: &str) {
    assert_eq!(run_program(name), exited(0));
}

/// The stage 3 roadmap criterion.
fn spawned_child_is_waited_for_through_a_port() {
    exited_in("parent");
}

fn handles_and_rights_hold_from_user_space() {
    exited_in("t_handles");
}

fn spawn_keeps_its_rules() {
    exited_in("t_spawn");
}

fn requests_and_jobs_keep_their_rules() {
    exited_in("t_signals");
}

fn ports_keep_their_rules() {
    exited_in("t_ports");
}

fn many_processes_come_and_go() {
    let before = sched::preemptions();
    exited_in("t_stress");
    assert!(sched::preemptions() > before, "the stress never preempted");
}

/// A `Start` for program `name`, holding nothing, in `job`.
fn start(name: &str, job: Arc<Job>, suspended: bool) -> Arc<process::Process> {
    let (name, file) = ramdisk::find(name).expect("program in the ramdisk");
    process::create(process::Start {
        name,
        image: process::Loadable::from_file(file).expect("a good program"),
        job,
        handles: HandleTable::new(),
        argv: Strings::empty(),
        envp: Strings::empty(),
        dispositions: [Disposition::Terminate; REQUEST_KINDS],
        suspended,
    })
    .expect("create")
}

fn root() -> Arc<Job> {
    Job::root().expect("root job")
}

/// As `wait_until`, for what another thread must do: the kernel is not
/// preempted, so nothing else runs unless this yields.
fn yield_until(done: impl Fn() -> bool) {
    let start = timer::now();
    while !done() {
        sched::yield_now();
        assert!(timer::now().nanos_since(start) < PATIENCE_NS, "no progress");
    }
}

fn run_a_while() {
    let until = timer::now().plus_nanos(20_000_000);
    while timer::now() < until {
        sched::yield_now();
    }
}

/// INV-0003-5. The first instruction would end the process; the thread's
/// frame still says the image's entry point, after others have had the CPU.
fn suspended_process_stays_at_its_entry() {
    let (_, file) = ramdisk::find("hello").expect("hello");
    let entry = Program::parse(file, USER_LOW, USER_HIGH)
        .expect("hello")
        .entry;
    let process = start("hello", root(), true);
    let thread = process.thread().expect("a thread");
    run_a_while();
    assert_eq!(thread.first_pc(), Some(entry));
    assert!(thread.is_blocked());
    assert_eq!(process.exit_record(), None);
    drop(thread);
    process.resume().expect("resume");
    assert_eq!(process.wait().reason, ExitReason::Exited);
}

/// INV-0004-4: the count read right after the request, while the target is
/// not running, is the count it ended with.
fn terminate_stops_user_code_at_once() {
    let process = process::spawn("counter").expect("counter");
    let start = timer::now();
    let (address, before) = loop {
        sched::yield_now();
        if let Some(address) = process.watch_address()
            && let Some(v) = process.peek(address)
            && v > 0
        {
            break (address, v);
        }
        assert!(timer::now().nanos_since(start) < PATIENCE_NS, "no progress");
    };
    process.signal(RequestKind::Terminate, 9).expect("signal");
    let at_request = process.peek(address).expect("still mapped");
    assert!(at_request >= before);
    let record = process.wait();
    assert_eq!(
        record,
        ExitRecord {
            code: 0,
            reason: ExitReason::Terminated,
            detail: 9,
            request_kind: Some(RequestKind::Terminate),
        }
    );
    assert_eq!(process.watched(), Some(at_request));
}

/// INV-0006-6: closing the handle from another thread.
fn closing_a_port_ends_a_blocked_wait() {
    let process = process::spawn("portclose").expect("portclose");
    let thread = process.thread().expect("a thread");
    yield_until(|| process.handles.lock().len() == 4 && thread.is_blocked());
    drop(thread);
    let port = Handle::from_parts(4, 0).expect("slot");
    let entry = process.handles.lock().remove(port).expect("its port");
    object::close(entry);
    assert_eq!(process.wait(), exited(55));
}

/// ADR-0004 rule 2.
fn disposition_change_ends_at_once() {
    let process = process::spawn("counter").expect("counter");
    process.set_disposition(RequestKind::Quit, Disposition::Deliver);
    process.signal(RequestKind::Quit, 5).expect("signal");
    assert_eq!(process.requests_queued(), 1);
    assert_eq!(process.exit_record(), None);
    process.set_disposition(RequestKind::Quit, Disposition::Terminate);
    let record = process.wait();
    assert_eq!(
        (record.request_kind, record.detail),
        (Some(RequestKind::Quit), 5)
    );
}

/// ADR-0004: first in, first out, and `Deliver` asserts until the queue empties.
fn requests_come_out_in_order() {
    let process = process::spawn("counter").expect("counter");
    process.set_disposition(RequestKind::Quit, Disposition::Deliver);
    for detail in 1..=3 {
        process.signal(RequestKind::Quit, detail).expect("signal");
    }
    for detail in 1..=3 {
        assert_eq!(process.take_request(), Some((RequestKind::Quit, detail)));
    }
    assert_eq!(process.take_request(), None);
    process.signal(RequestKind::Terminate, 0).expect("signal");
    process.wait();
}

/// ADR-0006 rule 1: an ended process asserts `TERMINATED` and nothing else.
fn an_ended_process_asserts_only_terminated() {
    let process = process::spawn("hello").expect("hello");
    process.set_disposition(RequestKind::Quit, Disposition::Deliver);
    process.signal(RequestKind::Quit, 1).expect("signal");
    process.wait();
    let bits = port::EVENTS.lock().signals(process.source());
    assert_eq!(bits, process_signals::TERMINATED);
}

/// R-01 of the TASK-09 review: a failing `uncharged_arc` drops the object, whose
/// `Drop` is the only free.
fn failed_allocation_frees_each_object_once() {
    use core::sync::atomic::Ordering::Relaxed;
    let before = port::EVENTS.lock().live();
    let fail = || heap::FAIL_NEXT_ARC.store(true, Relaxed);
    fail();
    assert!(port::Port::new(None).is_err());
    fail();
    assert!(Job::create(&root(), false).is_err());
    fail();
    let (name, file) = ramdisk::find("child").expect("child");
    let result = process::create(process::Start {
        name,
        image: process::Loadable::from_file(file).expect("a good program"),
        job: root(),
        handles: HandleTable::new(),
        argv: Strings::empty(),
        envp: Strings::empty(),
        dispositions: [Disposition::Terminate; REQUEST_KINDS],
        suspended: false,
    });
    assert!(result.is_err());
    assert_eq!(port::EVENTS.lock().live(), before);
    // And the id table still works.
    let port = port::Port::new(None).expect("port");
    port.close();
}

/// `[assumption]` of INV-0004-3, which states the count.
const JOB_SIGNAL_ROUNDS: usize = 10_000;
/// Processes alive at a time, within ADR-0031's 255.
const JOB_SIGNAL_WIDTH: usize = 8;

/// INV-0004-3: another thread keeps starting processes, numbered, while this
/// one signals. Every process numbered before the call has the request after
/// it, which a walk that missed a job or a process would break.
fn job_signal_reaches_every_process_of_its_tree() {
    static STARTED: AtomicUsize = AtomicUsize::new(0);
    static STOP: AtomicBool = AtomicBool::new(false);
    static LIVE: SpinLock<Vec<(usize, Arc<process::Process>)>> = SpinLock::new(Vec::new());
    static JOBS: SpinLock<Option<[Arc<Job>; 2]>> = SpinLock::new(None);

    fn spawner(_: usize) {
        let mut number = 0;
        while !STOP.load(Ordering::Relaxed) {
            if LIVE.lock().len() < JOB_SIGNAL_WIDTH {
                let job = JOBS.lock().as_ref().expect("jobs")[number % 2].clone();
                let p = start("child", job, true);
                p.set_disposition(RequestKind::Hangup, Disposition::Deliver);
                LIVE.lock().push((number, p));
                number += 1;
                STARTED.store(number, Ordering::Relaxed);
            }
            sched::yield_now();
        }
    }

    let top = Job::create(&root(), false).expect("job");
    let inner = Job::create(&top, false).expect("job");
    *JOBS.lock() = Some([top.clone(), inner.clone()]);
    STOP.store(false, Ordering::Relaxed);
    STARTED.store(0, Ordering::Relaxed);
    sched::spawn_kernel(spawner, 0).expect("thread");
    for _ in 0..JOB_SIGNAL_ROUNDS {
        sched::yield_now();
        let before = STARTED.load(Ordering::Relaxed);
        top.signal(RequestKind::Hangup, 1).expect("signal");
        // Taken out one at a time: ending one lets the spawner go on.
        let mut live = LIVE.lock();
        for (number, process) in live.iter() {
            if *number < before {
                assert!(process.requests_queued() > 0, "process {number} missed");
            }
            // Its queue must not fill over the rounds it lives.
            while process.take_request().is_some() {}
        }
        let done = live.pop();
        drop(live);
        if let Some((_, process)) = done {
            process.request_exit(exited(0));
            process.wait();
        }
    }
    assert!(STARTED.load(Ordering::Relaxed) >= JOB_SIGNAL_ROUNDS / 2);
    STOP.store(true, Ordering::Relaxed);
    sched::yield_now();
    for (_, process) in LIVE.lock().drain(..) {
        process.request_exit(exited(0));
    }
    *JOBS.lock() = None;
    yield_until(|| top.is_empty());
}

fn settle() {
    for _ in 0..4 {
        sched::yield_now();
    }
}

/// INV-0041-1: after many rounds of `t_leak`, which makes and ends each kind of
/// object in each way, frames, heap and event tables are where they were.
fn nothing_is_left_after_spawn_exit_close() {
    let snapshot = || {
        settle();
        let events = port::EVENTS.lock();
        (
            frames::settled_stats().free,
            heap::settled_stats().live,
            events.live(),
            events.subscription_count(),
        )
    };
    let round = || assert_eq!(run_program("t_leak"), exited(0));
    // The first rounds grow what is kept for reuse, such as the run queue.
    for _ in 0..8 {
        round();
    }
    let before = snapshot();
    for _ in 0..64 {
        round();
    }
    assert_eq!(snapshot(), before);
}

/// INV-0041-2: a held process keeps its record, and nothing else.
fn an_ended_process_holds_only_its_record() {
    let process = process::spawn("hello").expect("hello");
    assert_eq!(process.wait(), exited(7));
    assert_eq!(process.peek(USER_LOW), None);
    assert!(process.handles.lock().is_empty());
    assert!(process.thread().is_none());
    let mut mappings = 0;
    process.for_each_mapping(&mut |_| mappings += 1);
    assert_eq!(mappings, 0);
    assert_eq!(process.exit_record(), Some(exited(7)));
}

/// The bound INV-0006-3 states, in hundredths.
const PORT_RATIO_LIMIT: i32 = 150;

fn port_wait_ignores_idle_subscriptions() {
    let record = run_program("portbench");
    assert_eq!(record.reason, ExitReason::Exited);
    assert!(record.code <= PORT_RATIO_LIMIT, "ratio {}%", record.code);
}

/// ADR-0041: `uncharged_arc` rests on this, so that it fails where `Arc::new` would
/// panic. A value too large for the machine cannot be passed to `uncharged_arc`
/// itself: it would not fit on a stack.
fn try_arc_reports_failure() {
    assert!(!heap::can_allocate(Layout::new::<[u8; 1 << 40]>()));
    assert!(heap::can_allocate(Layout::new::<u64>()));
    assert_eq!(heap::uncharged_arc(7u64).map(|a| *a), Ok(7));
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
    // A bit per image frame seen once and one per frame seen twice: 64 MiB
    // of image in 4 KiB of stack (the ramdisk grows with each program).
    const WORDS: usize = 256;
    let mut once = [0u64; WORDS];
    let mut twice = [0u64; WORDS];
    let size = (image.end - image.start) / PAGE_SIZE;
    assert!(size as usize <= WORDS * 64, "image too large for the test");
    paging::for_each_mapping(&mut |m| {
        // A mapping may be a block that overlaps the image only in part.
        let mut at = m.pa.max(image.start);
        while at < (m.pa + m.size).min(image.end) {
            let i = ((at - image.start) / PAGE_SIZE) as usize;
            let (w, b) = (i / 64, 1u64 << (i % 64));
            if once[w] & b != 0 {
                twice[w] |= b;
            }
            once[w] |= b;
            at += PAGE_SIZE;
        }
    });
    assert!(twice.iter().all(|w| *w == 0), "an image frame mapped twice");
    assert!(once.iter().any(|w| *w != 0));
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

/// Frames `hold_frames` took, as a list through the frames themselves: the
/// first 8 bytes of each hold the next one's address, so holding needs no
/// memory of its own. The head, and the count.
static HELD: SpinLock<(u64, u64)> = SpinLock::new((0, 0));

/// Takes frames until only `leave` are free, for tests of exhaustion from a
/// process, and returns how many are held.
pub fn hold_frames(leave: u64) -> u64 {
    // The reaper's queue first: `settled_stats` yields, and no lock may be
    // held while it does. The frames this loop takes come from the free ones.
    crate::reaper::drain();
    let mut held = HELD.lock();
    while frames::stats().free > leave {
        let Ok(frame) = frames::alloc() else {
            break;
        };
        // SAFETY: a frame just allocated, ordinary RAM in the direct map,
        // which only this list reaches until `release_frames`.
        unsafe { (memory::phys_to_virt(frame.addr()) as *mut u64).write(held.0) };
        *held = (frame.addr(), held.1 + 1);
    }
    held.1
}

/// Gives back `limit` of the frames `hold_frames` took, or all of them if
/// `limit` is 0, and returns how many it gave.
pub fn release_frames(limit: u64) -> u64 {
    let mut held = HELD.lock();
    let mut count = 0;
    while held.0 != 0 && (limit == 0 || count < limit) {
        let frame = Frame::from_addr(held.0).expect("an aligned frame");
        // SAFETY: a held frame, written by `hold_frames`.
        held.0 = unsafe { (memory::phys_to_virt(frame.addr()) as *const u64).read() };
        frames::free(frame);
        held.1 -= 1;
        count += 1;
    }
    count
}

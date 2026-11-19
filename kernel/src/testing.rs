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
use alloc::vec;
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
const EXPECTED_FAILURES: usize = 10;

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
        name: "a_switch_at_a_step_point_starts_a_new_interval",
        run: a_switch_at_a_step_point_starts_a_new_interval,
    },
    Test {
        name: "an_allocation_short_of_frames_takes_one_from_the_reaper",
        run: an_allocation_short_of_frames_takes_one_from_the_reaper,
    },
    Test {
        name: "frames_the_reaper_holds_are_available_and_counted",
        run: frames_the_reaper_holds_are_available_and_counted,
    },
    Test {
        name: "the_worst_single_allocation_is_bounded_by_the_machines_ram",
        run: the_worst_single_allocation_is_bounded_by_the_machines_ram,
    },
    Test {
        name: "a_run_short_of_frames_waits_for_the_reaper_in_steps",
        run: a_run_short_of_frames_waits_for_the_reaper_in_steps,
    },
    Test {
        name: "a_run_queued_behind_many_frames_is_still_found_by_a_create",
        run: a_run_queued_behind_many_frames_is_still_found_by_a_create,
    },
    Test {
        name: "a_run_the_reaper_frees_at_the_step_before_its_help_is_found",
        run: a_run_the_reaper_frees_at_the_step_before_its_help_is_found,
    },
    Test {
        name: "a_run_the_reaper_frees_during_a_later_scan_is_found",
        run: a_run_the_reaper_frees_during_a_later_scan_is_found,
    },
    Test {
        name: "a_create_under_churn_is_refused_within_the_round_cap",
        run: a_create_under_churn_is_refused_within_the_round_cap,
    },
    Test {
        name: "a_run_search_over_a_fragmented_map_goes_in_steps",
        run: a_run_search_over_a_fragmented_map_goes_in_steps,
    },
    Test {
        name: "a_prepared_allocation_grows_the_heap_in_steps",
        run: a_prepared_allocation_grows_the_heap_in_steps,
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
        name: "a_run_freed_behind_the_scan_is_found_by_a_second_pass",
        run: a_run_freed_behind_the_scan_is_found_by_a_second_pass,
    },
    Test {
        name: "a_space_holds_4096_mappings_and_pays_for_the_list",
        run: a_space_holds_4096_mappings_and_pays_for_the_list,
    },
    Test {
        name: "a_space_list_growth_is_counted_and_prepared",
        run: a_space_list_growth_is_counted_and_prepared,
    },
    // Ordering: this test needs a free 4 MiB run, so it must run before
    // anything fragments the heap's frames.
    Test {
        name: "a_contiguous_object_is_zeroed_apart_from_its_search",
        run: a_contiguous_object_is_zeroed_apart_from_its_search,
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
        name: "harness_fails_with_a_process_left",
        run: harness_fails_with_a_process_left,
    },
    Test {
        name: "nothing_is_live_after_a_failed_test",
        run: nothing_is_live_after_a_failed_test,
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
        name: "threads_and_futexes_keep_their_rules",
        run: threads_and_futexes_keep_their_rules,
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
        name: "overlapping_transports_are_left_out",
        run: overlapping_transports_are_left_out,
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
        name: "images_share_their_read_only_pages",
        run: images_share_their_read_only_pages,
    },
    Test {
        name: "an_images_pages_outlive_its_handle_and_go_with_its_last_process",
        run: an_images_pages_outlive_its_handle_and_go_with_its_last_process,
    },
    Test {
        name: "an_images_pages_are_paid_by_its_creator_alone",
        run: an_images_pages_are_paid_by_its_creator_alone,
    },
    Test {
        name: "a_fork_child_of_an_image_process_maps_the_images_frames",
        run: a_fork_child_of_an_image_process_maps_the_images_frames,
    },
    Test {
        name: "a_fork_child_of_an_image_process_ending_first_frees_with_the_last",
        run: a_fork_child_of_an_image_process_ending_first_frees_with_the_last,
    },
    Test {
        name: "an_images_pages_hold_the_files_bytes_piece_by_piece",
        run: an_images_pages_hold_the_files_bytes_piece_by_piece,
    },
    Test {
        name: "an_image_of_a_large_program_is_bounded",
        run: an_image_of_a_large_program_is_bounded,
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
        name: "a_killed_room_ends_in_the_period_after_the_kill",
        run: a_killed_room_ends_in_the_period_after_the_kill,
    },
    Test {
        name: "a_start_and_end_loop_keeps_its_ceiling",
        run: a_start_and_end_loop_keeps_its_ceiling,
    },
    Test {
        name: "room_keeper_from_a_process",
        run: room_keeper_from_a_process,
    },
    Test {
        name: "snapshots_from_a_process",
        run: snapshots_from_a_process,
    },
    Test {
        name: "stack_canary_intact",
        run: stack_canary_intact,
    },
    // Ordering: this test and the kernel-steps tests must run after the timer
    // tests, because they leave a slice deadline queued. The spawn test runs
    // last (the heap keeps the frames it takes).
    Test {
        name: "a_spawn_with_a_big_space_is_bounded",
        run: a_spawn_with_a_big_space_is_bounded,
    },
];

const NONE: usize = usize::MAX;

/// Set by the test that overflows the stack on purpose.
static REARM_CANARY: AtomicBool = AtomicBool::new(false);
static CURRENT: AtomicUsize = AtomicUsize::new(NONE);
static PASSED: AtomicUsize = AtomicUsize::new(0);
static FAILED: AtomicUsize = AtomicUsize::new(0);
/// Live processes when the current test began (`run`).
static LIVE_BEFORE: AtomicUsize = AtomicUsize::new(0);
/// Processes the current test lost for good: their thread was abandoned on a
/// panic (`sched::adopt_boot_thread`), as `fault_report_for_stack_overflow_from_el0`
/// leaves `koverflow`. Those of earlier tests are in `LIVE_BEFORE`.
static LOST: AtomicUsize = AtomicUsize::new(0);

fn bump(counter: &AtomicUsize) {
    counter.store(counter.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
}

/// The first test of shard 2 of 2 (`--shard 2/2` of tools/run-qemu.sh). Shard
/// 1 runs the tests before it, shard 2 this one and those after it, in the
/// registered order, so each test runs in exactly one shard
/// (`tools/check-test-shards.sh`). Everything the runner expects of a
/// deliberate failure (`EXPECT` in the Makefile) comes before it, so shard 1
/// holds the fault tests and `harness_resumes_after_panic`.
const SHARD_2_FIRST: &str = "job_suspend_from_a_process";

/// One past the last test of this run (`TESTS.len()` unless sharded).
static END: AtomicUsize = AtomicUsize::new(usize::MAX);

/// The `zoraya.shard=N/M` word of `/chosen/bootargs`, if any.
fn shard_argument() -> Option<(usize, usize)> {
    let (fdt, _) = platform();
    let args = fdt.find_node("/chosen")?.property("bootargs")?.as_str()?;
    let spec = args
        .split_whitespace()
        .find_map(|w| w.strip_prefix("zoraya.shard="))?;
    // A word that is present and malformed is an error, not "no shard": the
    // run would otherwise cover every test without a word.
    let parsed = spec
        .split_once('/')
        .and_then(|(n, m)| Some((n.parse().ok()?, m.parse().ok()?)));
    Some(parsed.unwrap_or_else(|| panic!("malformed zoraya.shard={spec}")))
}

/// The tests this run covers, `lo..hi` (the whole list without a shard).
fn shard_range() -> (usize, usize) {
    let Some((n, m)) = shard_argument() else {
        return (0, TESTS.len());
    };
    assert!(m == 2 && (n == 1 || n == 2), "bad shard {n}/{m}");
    let split = TESTS
        .iter()
        .position(|t| t.name == SHARD_2_FIRST)
        .expect("SHARD_2_FIRST is a registered test");
    let (lo, hi) = if n == 1 {
        (0, split)
    } else {
        (split, TESTS.len())
    };
    println!("shard: {n}/{m} runs tests {lo}..{hi} of {}", TESTS.len());
    (lo, hi)
}

/// Runs the tests from index `start` on, then powers off. The first call
/// (`start` 0) picks the shard's range.
pub fn run(start: usize) -> ! {
    let mut start = start;
    if start == 0 {
        let (lo, hi) = shard_range();
        END.store(hi, Ordering::Relaxed);
        start = lo;
        println!("ZORAYA-TEST begin {}", hi - lo);
    }
    let end = END.load(Ordering::Relaxed);
    for (i, t) in TESTS.iter().enumerate().take(end).skip(start) {
        println!("ZORAYA-TEST run {}", t.name);
        CURRENT.store(i, Ordering::Relaxed);
        LIVE_BEFORE.store(live_processes(), Ordering::Relaxed);
        LOST.store(0, Ordering::Relaxed);
        sched::set_test_slice(0);
        crate::cpu::set_test_period(0);
        sched::ending_watch::watch(None);
        (t.run)();
        CURRENT.store(NONE, Ordering::Relaxed);
        bump(&PASSED);
        println!("ZORAYA-TEST ok {}", t.name);
    }
    // Outside any test, so that every run, sharded or not, ends with it: the
    // overflow test (`fault_report_for_stack_overflow`, shard 1) re-arms the
    // canary in `resume`, and `stack_canary_intact` (shard 2) alone would not
    // see it. A failure panics with no test running, which fails the run.
    assert!(
        arch::stack_canary_intact(),
        "boot stack overflowed into its canary (end of the run)"
    );
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
    end_what_a_failed_test_left();
    run(next)
}

fn live_processes() -> usize {
    Job::root().map_or(0, |root| root.live())
}

/// A failed test may leave processes running: a user program that panics
/// leaves the children it started (`t_cpu`'s 41 spinners, in a room at 25%
/// and outside it, made `t_rooms` 35 times slower and `t_snapshot` and the
/// big-space spawn fail after them, in a TCG run of `tcg-smp4-gicv2`). They
/// are ended here, and the next test starts once the live count is back at
/// what it was before the test, plus the processes it lost for good, and the
/// reaper has given their frames back. The tree is walked again every 100 ms,
/// for a process that started a child between a walk and its own end. A
/// wait that runs out panics outside any test, which fails the run.
fn end_what_a_failed_test_left() {
    let Ok(root) = Job::root() else { return };
    let target = LIVE_BEFORE.load(Ordering::Relaxed) + LOST.load(Ordering::Relaxed);
    if root.live() <= target {
        return;
    }
    let start = timer::now();
    let mut walked = None;
    while root.live() > target {
        let now = timer::now();
        assert!(
            now.nanos_since(start) <= PATIENCE_NS,
            "harness: {} processes the failed test left are still live after 10 s",
            root.live() - target
        );
        if walked.is_none_or(|at: Instant| now.nanos_since(at) >= 100_000_000) {
            root.end_every_process();
            walked = Some(now);
        }
        sched::yield_now();
    }
    frames::settled_stats();
    println!(
        "harness: ended what the failed test left in {} ms",
        timer::now().nanos_since(start) / 1_000_000
    );
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
    if crate::sched::adopt_boot_thread() {
        // That process never ends now (the next test's LIVE_BEFORE
        // includes it).
        LOST.fetch_add(1, Ordering::Relaxed);
    }
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

/// Fails with a process of its own still live: suspended, so it would stay
/// so for ever if the harness did not end it (`end_what_a_failed_test_left`).
fn harness_fails_with_a_process_left() {
    LIVE_AT_THE_FAILURE.store(live_processes(), Ordering::Relaxed);
    let left = process::spawn_suspended("busy").expect("busy");
    drop(left);
    assert!(live_processes() > LIVE_AT_THE_FAILURE.load(Ordering::Relaxed));
    panic!("deliberate panic with a process left live");
}

/// The live processes before `harness_fails_with_a_process_left` spawned.
static LIVE_AT_THE_FAILURE: AtomicUsize = AtomicUsize::new(0);

/// After the failure above, nothing it started is live.
fn nothing_is_live_after_a_failed_test() {
    assert_eq!(
        live_processes(),
        LIVE_AT_THE_FAILURE.load(Ordering::Relaxed),
        "a failed test's process outlived it"
    );
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
    let frames_before = frames_accounted();
    let heap_before = heap::settled_stats().live;
    assert_eq!(run_program("t_memory"), exited(0));
    assert_eq!(frames_accounted(), frames_before);
    assert_eq!(heap::settled_stats().live, heap_before);
}

/// ADR-0050 from a process. It ends with nothing left in the kernel.
fn channels_from_a_process() {
    // The first run grows the signal table to its peak, which it keeps: every
    // channel end is two entries of a table that does not shrink. A second run
    // must then change nothing.
    assert_eq!(run_program("t_channels"), exited(0));
    let frames_before = frames_accounted();
    let heap_before = heap::settled_stats().live;
    assert_eq!(run_program("t_channels"), exited(0));
    assert_eq!(frames_accounted(), frames_before);
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
    let frames_before = frames_accounted();
    let heap_before = heap::settled_stats().live;
    assert_eq!(run(), exited(0));
    assert_eq!(frames_accounted(), frames_before);
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
    let frames_before = frames_accounted();
    let heap_before = heap::settled_stats().live;
    assert_eq!(run(), exited(0));
    assert_eq!(frames_accounted(), frames_before);
    assert_eq!(heap::settled_stats().live, heap_before);
}

/// Runs `run` twice and checks that the second run left the frame and heap
/// counts as it found them (INV-0041-1): the first grows what grows once,
/// such as the signal table and the heap's chunks.
fn twice_with_nothing_left(run: impl Fn() -> ExitRecord) {
    assert_eq!(run(), exited(0));
    let frames_before = frames_accounted();
    let heap_before = heap::settled_stats().live;
    assert_eq!(run(), exited(0));
    assert_eq!(frames_accounted(), frames_before, "frames");
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

/// An object holding `file`, for `process::image_from`.
fn file_object(file: &[u8]) -> Arc<memory::object::MemoryObject> {
    let size = (file.len() as u64).next_multiple_of(PAGE_SIZE);
    let object = memory::object::MemoryObject::new(size, false, None).expect("memory");
    assert!(object.write(0, file));
    object
}

/// A process of `image`, suspended, in a job of its own made by `job`.
fn from_image(
    image: &Arc<object::MemoryImage>,
    job: Arc<Job>,
) -> Result<Arc<process::Process>, SpawnError> {
    process::create(process::Start {
        name: "hello",
        image: process::Loadable::from_image(image),
        job,
        handles: HandleTable::new(),
        argv: Strings::empty(),
        envp: Strings::empty(),
        dispositions: [Disposition::Terminate; REQUEST_KINDS],
        suspended: true,
    })
}

/// Every page a process maps: address to frame and attributes.
fn pages_of(process: &process::Process) -> BTreeMap<u64, (u64, zoraya_pagetable::Attributes)> {
    let mut pages = BTreeMap::new();
    process.for_each_mapping(&mut |m| {
        for offset in (0..m.size).step_by(PAGE_SIZE as usize) {
            pages.insert(m.va + offset, (m.pa + offset, m.attributes));
        }
    });
    pages
}

/// Ends a process that never ran.
fn end_suspended(process: &Arc<process::Process>) {
    let _ = process.signal(RequestKind::Terminate, 0);
    let _ = process.resume();
    process.wait();
}

/// Lets a suspended process run to its end and gives its record.
fn run_to_the_end(process: &Arc<process::Process>) -> ExitRecord {
    process.resume().expect("resume");
    process.wait()
}

/// INV-0415-2 (ADR-0415 item 2, proposed ADR-0438): two processes of one
/// image map the same frames for the segments that are not writable, never
/// writable, and each has frames of its own for the rest; the creator's job
/// pays for the shared frames once and the two children's jobs for their own
/// pages alone; both run what the image holds.
fn images_share_their_read_only_pages() {
    let (_, file) = ramdisk::find("hello").expect("hello");
    let before = frames_accounted();
    let creator = Job::create(&root(), false).expect("job");
    let image = process::image_from(&file_object(file), file.len() as u64, &creator)
        .expect("an image of hello");
    let kept = image.program.kept();
    let shared: u64 = image
        .program
        .segments()
        .iter()
        .filter(|s| s.is_shared())
        .map(|s| (s.pages().1 - s.pages().0) / PAGE_SIZE)
        .sum();
    let writable: u64 = image
        .program
        .segments()
        .iter()
        .filter(|s| !s.is_shared())
        .map(|s| s.kept_pages())
        .sum();
    assert!(shared > 0, "hello has a text");
    assert_eq!(
        shared + writable,
        kept.pages(),
        "every kept page is one or the other"
    );
    let paid = creator.usage(false);
    assert_eq!(paid.user, kept.bytes(), "the creator pays for the pages");

    let (job1, job2, job0) = (
        Job::create(&root(), false).expect("job"),
        Job::create(&root(), false).expect("job"),
        Job::create(&root(), false).expect("job"),
    );
    let p1 = from_image(&image, job1.clone()).expect("first");
    let p2 = from_image(&image, job2.clone()).expect("second");
    // The same program from the ramdisk copies every page: the control.
    let p0 = start("hello", job0.clone(), true);
    let (m1, m2, m0) = (pages_of(&p1), pages_of(&p2), pages_of(&p0));
    assert_eq!(m1.len(), m0.len(), "the same pages");
    for (index, s) in image.program.segments().iter().enumerate() {
        let (start, end) = s.pages();
        let first = kept.first_page(index) * PAGE_SIZE;
        for va in (start..end).step_by(PAGE_SIZE as usize) {
            let (pa1, a1) = m1[&va];
            let (pa2, a2) = m2[&va];
            let (pa0, _) = m0[&va];
            assert!(a1.user && a2.user);
            assert!(!(a1.write && a1.execute), "W+X at {va:#x}");
            assert_eq!((a1.write, a1.execute), (s.write, s.execute));
            assert_eq!((a2.write, a2.execute), (s.write, s.execute));
            if s.is_shared() {
                // The frames of the image's object, in both, writable in
                // neither.
                assert!(!a1.write && !a2.write, "a shared page writable at {va:#x}");
                assert_eq!(pa1, image.kept.page(first + (va - start)));
                assert_eq!(pa1, pa2, "one frame for {va:#x}");
                assert_ne!(pa1, pa0, "the control has a copy of its own");
            } else {
                assert_ne!(pa1, pa2, "two frames for the writable {va:#x}");
                let n = (va - start) / PAGE_SIZE;
                if n < s.kept_pages() {
                    assert_ne!(pa1, image.kept.page(first + n * PAGE_SIZE));
                }
            }
        }
    }
    // The text is what the file says, through both processes' pages.
    let text = image
        .program
        .segments()
        .iter()
        .find(|s| s.execute)
        .expect("a text");
    let at = text.vaddr.next_multiple_of(8);
    let from = (text.offset + (at - text.vaddr)) as usize;
    let word = u64::from_le_bytes(file[from..from + 8].try_into().expect("8"));
    assert_eq!(p1.peek(at), Some(word));
    assert_eq!(p2.peek(at), Some(word));
    // Who pays: the creator's job is as it was and each child's holds its
    // own pages only: the control's user bytes are the child's plus the
    // shared pages.
    assert_eq!(creator.usage(false), paid, "spawning charged the creator");
    let (u1, u2, u0) = (job1.usage(false), job2.usage(false), job0.usage(false));
    assert_eq!(u1.user, u2.user, "two children, one cost");
    assert_eq!(
        u1.user + shared * PAGE_SIZE,
        u0.user,
        "the copy pays for the pages the shared child does not"
    );
    // Both run the image, and the control as before.
    assert_eq!(run_to_the_end(&p1), exited(7));
    assert_eq!(run_to_the_end(&p2), exited(7));
    assert_eq!(run_to_the_end(&p0), exited(7));
    drop((p0, p1, p2));
    drop(image);
    crate::reaper::drain();
    assert_eq!(frames_accounted(), before, "every frame went back");
    assert_eq!(creator.usage(false).user, 0, "and the creator's charge");
}

/// INV-0415-2: the pages live while any holder does. The image's handle may
/// go while a process of it runs, and the process's pages stay valid; the
/// frames go with the last of them, no sooner and no later.
fn an_images_pages_outlive_its_handle_and_go_with_its_last_process() {
    let (_, file) = ramdisk::find("hello").expect("hello");
    let before = frames_accounted();
    let creator = Job::create(&root(), false).expect("job");
    let image = process::image_from(&file_object(file), file.len() as u64, &creator)
        .expect("an image of hello");
    let kept = image.program.kept().bytes();
    let alive = frames_accounted();
    assert!(
        alive + kept / PAGE_SIZE <= before + 8,
        "the pages are taken"
    );
    let p1 = from_image(&image, Job::create(&root(), false).expect("job")).expect("first");
    let p2 = from_image(&image, Job::create(&root(), false).expect("job")).expect("second");
    drop(image);
    crate::reaper::drain();
    assert_eq!(creator.usage(false).user, kept, "held by the processes");
    // The first ends: the second's text is still there and runs.
    assert_eq!(run_to_the_end(&p1), exited(7));
    drop(p1);
    crate::reaper::drain();
    assert_eq!(creator.usage(false).user, kept, "one process is left");
    assert_eq!(run_to_the_end(&p2), exited(7), "and its text is intact");
    drop(p2);
    crate::reaper::drain();
    assert_eq!(creator.usage(false).user, 0, "the last holder frees them");
    assert_eq!(frames_accounted(), before);
}

/// ADR-0438 item 2 and driver 3: a `fork` child (a snapshot of the space) of a
/// process of an image maps the image's frames read-only for its read-only
/// segments, takes no frame and no charge for them, and the creator's use does
/// not move; the frames go with the last holder, whichever ends first.
fn a_fork_child_of_an_image_process_maps_the_images_frames() {
    a_fork_child_of_an_image_process(false);
}

/// The same, with the child ending first.
fn a_fork_child_of_an_image_process_ending_first_frees_with_the_last() {
    a_fork_child_of_an_image_process(true);
}

fn a_fork_child_of_an_image_process(child_first: bool) {
    use crate::memory::user::{Copied, PlanItem, SnapshotBuild};
    let (_, file) = ramdisk::find("hello").expect("hello");
    let before = frames_accounted();
    let creator = Job::create(&root(), false).expect("job");
    let image = process::image_from(&file_object(file), file.len() as u64, &creator)
        .expect("an image of hello");
    let kept = image.program.kept();
    let paid = creator.usage(false);
    let (job1, job2) = (
        Job::create(&root(), false).expect("job"),
        Job::create(&root(), false).expect("job"),
    );
    let parent = from_image(&image, job1.clone()).expect("parent");
    // The steps of `snapshot_process`, on the parent's space, into job 2.
    let child = {
        let guard = parent.space.lock();
        let space = guard.as_ref().expect("a space");
        let mut plan: Vec<PlanItem> = Vec::new();
        plan.try_reserve_exact(space.mapping_count()).expect("room");
        assert_eq!(space.snapshot_plan_step(&mut plan, 0), Ok(None));
        let mut build = SnapshotBuild::new(plan, Some(&job2)).expect("a build");
        let mut copied = Copied::default();
        loop {
            build.prepare_step();
            if space
                .snapshot_step(&mut build, &mut copied)
                .expect("a step")
            {
                break;
            }
        }
        build.finish().expect("a child")
    };
    let mut child_pages = BTreeMap::new();
    child.for_each_mapping(&mut |m| {
        for offset in (0..m.size).step_by(PAGE_SIZE as usize) {
            child_pages.insert(m.va + offset, (m.pa + offset, m.attributes));
        }
    });
    let parent_pages = pages_of(&parent);
    assert_eq!(child_pages.len(), parent_pages.len(), "the same pages");
    let mut shared = 0;
    for (index, s) in image.program.segments().iter().enumerate() {
        let (start, end) = s.pages();
        let first = kept.first_page(index) * PAGE_SIZE;
        for va in (start..end).step_by(PAGE_SIZE as usize) {
            let (pa, a) = child_pages[&va];
            assert_eq!((a.write, a.execute), (s.write, s.execute), "{va:#x}");
            if s.is_shared() {
                shared += 1;
                assert!(!a.write, "a shared page writable in a fork child");
                assert_eq!(pa, image.kept.page(first + (va - start)));
                assert_eq!(pa, parent_pages[&va].0, "the parent's frame");
            } else {
                assert_ne!(pa, parent_pages[&va].0, "a writable page is copied");
            }
        }
    }
    assert!(shared > 0);
    // The child pays for every page the parent does but the shared ones, which
    // nobody but the creator pays; the creator's use is as it was.
    assert_eq!(job2.usage(false).user, job1.usage(false).user);
    assert_eq!(creator.usage(false), paid, "a fork charged the creator");
    let held = kept.bytes();
    if child_first {
        // The child first: the image and the parent still hold the pages.
        drop(child);
        crate::reaper::drain();
        assert_eq!(
            creator.usage(false).user,
            held,
            "the parent holds the pages"
        );
        end_suspended(&parent);
        drop(parent);
        crate::reaper::drain();
        assert_eq!(creator.usage(false).user, held, "the image holds the pages");
        assert!(frames_accounted() < before);
        drop(image);
    } else {
        // The creator's side first: the image, the parent, then the child last.
        drop(image);
        end_suspended(&parent);
        drop(parent);
        crate::reaper::drain();
        assert_eq!(creator.usage(false).user, held, "the child holds the pages");
        assert!(
            frames_accounted() < before,
            "and the frames are still there"
        );
        drop(child);
    }
    crate::reaper::drain();
    assert_eq!(creator.usage(false).user, 0);
    assert_eq!(frames_accounted(), before);
}

/// ADR-0261 point 8 and INV-0415-2: a creator that cannot pay for an image
/// gets `OverQuota` and holds nothing; one that can pays once, and no number
/// of processes in other jobs changes what it owes; a job that can pay for
/// its own pages spawns the image whatever the creator's limit.
fn an_images_pages_are_paid_by_its_creator_alone() {
    let (_, file) = ramdisk::find("hello").expect("hello");
    let before = frames_accounted();
    let limits = |memory| zoraya_abi::quota::JobLimits {
        memory: Some(memory),
        processes: None,
        cpu: None,
        threads: None,
    };
    let make = |job: &Arc<Job>| process::image_from(&file_object(file), file.len() as u64, job);
    // A job with no limit makes the image and shows what the pages cost.
    let probe = Job::create(&root(), false).expect("job");
    let image = make(&probe).expect("image");
    let (kept, cost) = (image.program.kept().bytes(), probe.usage(false).memory.used);
    assert!(cost >= kept);
    drop(image);
    crate::reaper::drain();
    // One byte short: refused, and nothing is held.
    let poor = Job::create(&root(), false).expect("job");
    // A job's own record is charged to it.
    let record = poor.usage(false).memory.used;
    poor.set_limits(limits(cost - 1)).expect("limits");
    assert!(matches!(make(&poor), Err(zoraya_abi::Status::OverQuota)));
    let seen = poor.usage(false);
    assert_eq!(
        (seen.memory.used, seen.user),
        (record, 0),
        "a refusal holds nothing"
    );
    assert_eq!(seen.memory.refused, 1);
    assert_eq!(frames_accounted(), before, "no frame was kept");
    // Exactly enough: made; the pages are the creator's.
    let exact = Job::create(&root(), false).expect("job");
    exact.set_limits(limits(cost)).expect("limits");
    let image = make(&exact).expect("image");
    assert_eq!(exact.usage(false).memory.used, cost);
    // A child job sized to its own pages: the probe's first spawn shows them.
    let first = Job::create(&root(), false).expect("job");
    let p = from_image(&image, first.clone()).expect("spawn");
    let own = first.usage(false).memory.peak;
    end_suspended(&p);
    drop(p);
    crate::reaper::drain();
    let tight = Job::create(&root(), false).expect("job");
    tight.set_limits(limits(own - 1)).expect("limits");
    assert!(matches!(
        from_image(&image, tight.clone()),
        Err(SpawnError::OverQuota)
    ));
    assert_eq!(
        tight.usage(false).memory.used,
        record,
        "a refused spawn holds nothing"
    );
    let snug = Job::create(&root(), false).expect("job");
    snug.set_limits(limits(own)).expect("limits");
    // The creator is at its limit: the children do not need its room.
    let kids: Vec<_> = (0..4).map(|_| from_image(&image, snug.clone())).collect();
    // Four in one job of `own`: the first fits and each later one is refused
    // for the job's own room, none of them charged to the creator.
    assert!(kids[0].is_ok());
    for kid in &kids[1..] {
        assert!(
            matches!(kid, Err(SpawnError::OverQuota)),
            "a second process in a job with room for one"
        );
    }
    assert_eq!(
        exact.usage(false).memory.used,
        cost,
        "the creator owes the same"
    );
    for kid in kids.iter().flatten() {
        end_suspended(kid);
    }
    drop(kids);
    drop(image);
    crate::reaper::drain();
    assert_eq!(exact.usage(false).memory.used, record);
    assert_eq!(frames_accounted(), before);
}

/// `file` with its last writable segment made read-only and `extra` bytes of
/// zeroed memory after it: a large segment that is shared, to measure the
/// image's call and the spawn with.
fn with_shared_bss(file: &[u8], extra: u64) -> Vec<u8> {
    let mut bytes = with_bss(file, extra);
    let word = |b: &[u8], at: usize| u64::from_le_bytes(b[at..at + 8].try_into().expect("8"));
    let phoff = word(&bytes, 32) as usize;
    let phnum = u16::from_le_bytes(bytes[56..58].try_into().expect("2")) as usize;
    for i in 0..phnum {
        let ph = phoff + i * 56;
        let flags = u32::from_le_bytes(bytes[ph + 4..ph + 8].try_into().expect("4"));
        if flags & 2 != 0 {
            bytes[ph + 4..ph + 8].copy_from_slice(&(flags & !2).to_le_bytes());
        }
    }
    bytes
}

/// An ELF with one text segment of `pages` pages of a known pattern, for
/// `image_from` (the pattern byte of offset `i` is `i % 251`).
fn patterned_elf(pages: u64) -> Vec<u8> {
    let size = (pages + 1) * PAGE_SIZE;
    let mut f = vec![0u8; size as usize];
    f[..4].copy_from_slice(b"\x7fELF");
    f[4] = 2;
    f[5] = 1;
    f[6] = 1;
    f[16..18].copy_from_slice(&2u16.to_le_bytes());
    f[18..20].copy_from_slice(&183u16.to_le_bytes());
    f[24..32].copy_from_slice(&0x40_0000u64.to_le_bytes());
    f[32..40].copy_from_slice(&64u64.to_le_bytes());
    f[54..56].copy_from_slice(&56u16.to_le_bytes());
    f[56..58].copy_from_slice(&1u16.to_le_bytes());
    let ph = 64;
    f[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
    f[ph + 4..ph + 8].copy_from_slice(&5u32.to_le_bytes()); // R X
    f[ph + 8..ph + 16].copy_from_slice(&PAGE_SIZE.to_le_bytes());
    f[ph + 16..ph + 24].copy_from_slice(&0x40_0000u64.to_le_bytes());
    f[ph + 32..ph + 40].copy_from_slice(&(pages * PAGE_SIZE).to_le_bytes());
    f[ph + 40..ph + 48].copy_from_slice(&(pages * PAGE_SIZE).to_le_bytes());
    for (i, b) in f[PAGE_SIZE as usize..].iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    f
}

/// As [`patterned_elf`], with an RW segment of `data_pages` pages of file bytes
/// after the text, as a real program is laid out.
fn patterned_elf_with_data(text_pages: u64, data_pages: u64) -> Vec<u8> {
    let mut f = patterned_elf(text_pages + data_pages);
    f[56..58].copy_from_slice(&2u16.to_le_bytes());
    // The text: the first `text_pages`.
    f[64 + 32..64 + 40].copy_from_slice(&(text_pages * PAGE_SIZE).to_le_bytes());
    f[64 + 40..64 + 48].copy_from_slice(&(text_pages * PAGE_SIZE).to_le_bytes());
    let ph = 64 + 56;
    f[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
    f[ph + 4..ph + 8].copy_from_slice(&6u32.to_le_bytes()); // R W
    f[ph + 8..ph + 16].copy_from_slice(&((1 + text_pages) * PAGE_SIZE).to_le_bytes());
    f[ph + 16..ph + 24].copy_from_slice(&(0x40_0000 + text_pages * PAGE_SIZE).to_le_bytes());
    f[ph + 32..ph + 40].copy_from_slice(&(data_pages * PAGE_SIZE).to_le_bytes());
    f[ph + 40..ph + 48].copy_from_slice(&(data_pages * PAGE_SIZE).to_le_bytes());
    f
}

/// INV-0438-1: a segment of many steps (200 pages, four pieces) is in the
/// image byte for byte, where the file had it, and nothing else is.
fn an_images_pages_hold_the_files_bytes_piece_by_piece() {
    let file = patterned_elf(200);
    let job = Job::create(&root(), false).expect("job");
    let before = frames_accounted();
    let image = process::image_from(&file_object(&file), file.len() as u64, &job).expect("image");
    assert_eq!(image.program.kept().pages(), 200);
    let mut seen = vec![0u8; 200 * PAGE_SIZE as usize];
    assert!(image.kept.read(0, &mut seen));
    assert!(
        seen.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8),
        "the kept pages differ from the file"
    );
    drop(image);
    crate::reaper::drain();
    assert_eq!(frames_accounted(), before);
}

/// INV-0270-1: `image_create` of a program whose read-only segments are
/// large, and a spawn from it, hold no more than a step of work in an
/// interval; a spawn of the same image maps what it shares in steps too.
fn an_image_of_a_large_program_is_bounded() {
    const PAGES: u64 = 2048;
    let (_, file) = ramdisk::find("hello").expect("hello");
    let big = with_shared_bss(file, PAGES * PAGE_SIZE);
    let before = frames_accounted();
    let creator = Job::create(&root(), false).expect("job");
    let object = file_object(&big);
    let measured = |work: &mut dyn FnMut()| {
        crate::steps::set_kernel_steps(true);
        let daif = arch::save_and_mask_interrupts();
        let _ = test_step_point();
        crate::steps::masked_max_units(true);
        work();
        let _ = test_step_point();
        arch::restore_interrupts(daif);
        crate::steps::set_kernel_steps(false);
        let units = crate::steps::masked_max_units(true);
        if units > crate::steps::STEP as u64 + 16 {
            crate::steps::diagnose_units();
        }
        units
    };
    // The copy and the instruction-cache loops of `image_create` (INV-0438-4): a
    // text of 2048 pages of file bytes, executable, so that both run over it.
    let text = patterned_elf(PAGES);
    let text_object = file_object(&text);
    let mut made_text = None;
    let units = measured(&mut || {
        made_text = Some(process::image_from(
            &text_object,
            text.len() as u64,
            &creator,
        ));
    });
    let text_image = made_text.expect("ran").expect("an image of 8 MiB of text");
    assert!(
        units <= crate::steps::STEP as u64 + 16,
        "{units} units in an interval of image_create of {PAGES} pages of text (INV-0270-1)"
    );
    drop((text_image, text_object));
    crate::reaper::drain();
    // A real program is laid out text then data: the last cleans of the text
    // and the first piece of the data copy must not share an interval.
    let both = patterned_elf_with_data(PAGES, 128);
    let both_object = file_object(&both);
    let mut made_both = None;
    let units = measured(&mut || {
        made_both = Some(process::image_from(
            &both_object,
            both.len() as u64,
            &creator,
        ));
    });
    let both_image = made_both.expect("ran").expect("an image of text and data");
    assert!(
        units <= crate::steps::STEP as u64 + 16,
        "{units} units in an interval of image_create of text then data (INV-0270-1)"
    );
    assert_eq!(both_image.program.segments().len(), 2);
    drop((both_image, both_object));
    crate::reaper::drain();
    let mut made = None;
    let units = measured(&mut || {
        made = Some(process::image_from(&object, big.len() as u64, &creator));
    });
    let image = made
        .expect("ran")
        .expect("an image of 8 MiB of shared pages");
    assert!(
        units <= crate::steps::STEP as u64 + 16,
        "{units} units in an interval of image_create (INV-0270-1)"
    );
    assert!(image.program.kept().pages() >= PAGES);
    let mut spawned = None;
    let units = measured(&mut || {
        spawned = Some(from_image(
            &image,
            Job::create(&root(), false).expect("job"),
        ));
    });
    let process = spawned.expect("ran").expect("a spawn of the large image");
    assert!(
        units <= crate::steps::STEP as u64 + 16,
        "{units} units in an interval of a spawn of shared pages (INV-0270-1)"
    );
    let _ = process.signal(RequestKind::Terminate, 0);
    let _ = process.resume();
    process.wait();
    drop((process, image, object));
    crate::reaper::drain();
    assert_eq!(frames_accounted(), before);
}

/// ADR-0105 item 8 (INV-0105-5) and the latency of suspending 16 processes.
fn job_suspend_from_a_process() {
    assert_eq!(run_program("t_suspend"), exited(0));
}

/// ADR-0300, ADR-0301 and ADR-0307: threads, the wait on an address and the
/// threads quota, at the raw ABI.
fn threads_and_futexes_keep_their_rules() {
    assert_eq!(run_program("t_thread"), exited(0));
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

/// ADR-0285 item 7 with ADR-0460: a room at a ceiling of one thousandth
/// (100 us of a 100 ms period) is killed with its 16 spinners parked. They
/// come back at the next boundary and, their processes ending, are not
/// parked again: the room has no process left before the end of the period
/// after the kill's (INV-0460-1). Their 16 ways out take several times the
/// room's budget, so without the exception each period's budget ends a few
/// of them and the room takes a period for each few (TASK-31 4.3: up to nine
/// periods for 20 at 10%).
///
/// The bound assumes the 16 ways out fit in one period. Where in its period
/// the kill lands does not matter: the parked threads wait for the next
/// boundary whatever the phase, and those released at a boundary are parked
/// again within microseconds, so the ways out always have exactly the period
/// after the kill's. Under TCG 40 ways out ran 58 to 71 ms; 16 take some 25
/// ms, a quarter of the period.
///
/// The way out is charged to the room at least for the time the scheduler
/// saw the room's ending threads run (`sched::ending_watch`, counted apart
/// from the charge), and to its parent, which holds nothing else
/// (INV-0460-2).
fn a_killed_room_ends_in_the_period_after_the_kill() {
    const PERIOD: u64 = 100_000_000;
    const SPINNERS: usize = 16;
    let began = timer::now();
    crate::cpu::set_test_period(PERIOD);
    let parent = Job::create(&root(), false).expect("parent");
    let room = Job::create(&parent, false).expect("room");
    crate::cpu::set_limit(&room, Some(1)).expect("limit");
    let budget = zoraya_cpu::budget_ns(1, PERIOD);
    sched::ending_watch::watch(Some(&room));
    let spinners: Vec<_> = (0..SPINNERS)
        .map(|_| process::spawn_in(&room, "spin", false).expect("spin"))
        .collect();
    let nap = || {
        sched::sleep_until(Deadline::At(timer::now().plus_nanos(1_000_000))).expect("queue");
    };
    let start = timer::now();
    loop {
        let u = crate::cpu::usage(&room);
        if u.parked > 0 && u.period_used >= budget {
            break;
        }
        assert!(
            timer::now().nanos_since(start) < PATIENCE_NS,
            "the room never spent its budget"
        );
        nap();
    }
    drop(spinners);
    let parked_before = crate::cpu::usage(&room).parked;
    let used_before = (
        crate::cpu::usage(&room).used_ns,
        crate::cpu::usage(&parent).used_ns,
    );
    crate::steps::masked_max_ns(true);
    room.kill();
    let killed = zoraya_cpu::period_of(timer::now().as_nanos(), PERIOD);
    let start = timer::now();
    while room.live() > 0 {
        assert!(
            timer::now().nanos_since(start) < PATIENCE_NS,
            "the killed room still has {} processes after 10 s",
            room.live()
        );
        nap();
    }
    let died = zoraya_cpu::period_of(timer::now().as_nanos(), PERIOD);
    let masked = crate::steps::masked_max_ns(true);
    let ran = sched::ending_watch::ran_ns();
    sched::ending_watch::watch(None);
    let parks = crate::cpu::usage(&room).parked - parked_before;
    let charged = crate::cpu::usage(&room).used_ns - used_before.0;
    let parent_charged = crate::cpu::usage(&parent).used_ns - used_before.1;
    println!(
        "a killed room ended {} periods after the kill's period ({} ms), {parks} parks after the kill, {charged} ns charged after it, {ran} ns run by its ending threads, {masked} ns longest masked; {} ms in all",
        died - killed,
        timer::now().nanos_since(start) / 1_000_000,
        timer::now().nanos_since(began) / 1_000_000
    );
    // Every one of the ways out ran after the kill (the spinners were
    // parked or spinning), so the room is charged for them, and the parent
    // as much: each charge walks up (ADR-0264 point 3).
    assert!(ran > 0, "no ending thread of the room ran");
    assert!(
        charged >= ran,
        "the room was charged {charged} ns, its ending threads ran {ran} ns"
    );
    assert!(
        parent_charged >= charged,
        "the parent was charged {parent_charged} ns, the room {charged} ns"
    );
    // INV-0460-1 holds while the ways out fit in one period. The wall time
    // the room's ending threads ran is what the test can read (`ran`): the
    // 16 ways out took some 25 ms where this was written and 102 ms on CI's
    // TCG (scheduled run 43843667050), more than the period. What never
    // depends on the speed is that no ending thread is parked again, and
    // that the room goes in the period after the kill's plus one for each
    // whole period the ways out ran.
    assert!(
        parks == 0,
        "{parks} parks after the kill: an ending thread was parked again"
    );
    assert!(
        died <= killed + 1 + ran / PERIOD,
        "the killed room ended {} periods after the kill's period, {parks} parks after the kill, its ending threads ran {ran} ns",
        died - killed
    );
}

/// ADR-0460, the architect's decision on its open question 2 (INV-0460-4):
/// a room whose processes end one after the other, each with a way out far
/// longer than the room's budget, gains nothing from them beyond their own
/// work. Each process is `spin`, started suspended with 900 one-page
/// objects the test puts in its table (free to the room), so that its way
/// out, closing them, takes many budgets. The test ends it (as `job_kill`
/// does, `request_exit`) as soon as it has run, that is, as soon as the room
/// had budget for it, and starts the next when the room is empty. So with
/// the debt the loop waits while the room pays; without it a new way out
/// began about every period. (A process that ends itself by a fault, as
/// `f_null` did, overran the budget by the kernel's fault report, a console
/// line written before the process is ending, 2.2 ms under slowed TCG: the
/// overrun to the next return to EL0 that ADR-0264 point 4 allows, not this
/// test's subject.)
///
/// Over the 11 periods after the one in which the first way out began, both
/// measured in this boot:
/// - the room's charge is at most 11 budgets, a floor slice a period and
///   one more budget, plus what its ending threads ran in the window (read
///   from the clock at each switch apart from the charge,
///   `sched::ending_watch`): the room's own code keeps the ceiling, however
///   long a way out takes on this machine;
/// - no way out begins in the window: the first way out's debt (its run
///   past the budget, some 500 us on the fastest host for 900 objects
///   against 20 us a period, capped at 2 ms) parks the room for longer than
///   the window, on any host slower than that. (With 300 objects a way out
///   cost 170 us under HVF on the Mac mini, 8 periods' budgets, so the second
///   began inside the window: the number of objects, now 900 (a table holds 1024 handles), is what sets
///   the debt, not the host's speed.)
fn a_start_and_end_loop_keeps_its_ceiling() {
    use crate::memory::object::MemoryObject;
    const LIMIT: u32 = 1; // 20 us of the 20 ms period.
    const OBJECTS: usize = 900;
    let began = timer::now();
    let period_ns = crate::cpu::period_ns();
    let parent = Job::create(&root(), false).expect("parent");
    let room = Job::create(&parent, false).expect("room");
    crate::cpu::set_limit(&room, Some(LIMIT)).expect("limit");
    let budget = zoraya_cpu::budget_ns(LIMIT, period_ns);
    sched::ending_watch::watch(Some(&room));
    let period_of = |t: u64| zoraya_cpu::period_of(t, period_ns);
    let now_period = || period_of(timer::now().as_nanos());
    // (charge, all turns, ending turns, period)
    let sample = || {
        (
            crate::cpu::usage(&room).used_ns,
            sched::ending_watch::all_ns(),
            sched::ending_watch::ran_ns(),
            now_period(),
        )
    };
    let start = now_period();
    // The window: the 11 periods after the one in which the first way out
    // began, so that the first process's own turn (which may run late past
    // the budget on a slow host, ADR-0264 point 4) is not in it.
    let mut window: Option<(u64, u64)> = None;
    let mut at_first = None;
    let mut at_last = None;
    let mut ends: Vec<(u64, bool)> = Vec::new(); // (period its end was seen, inside the window)
    let rights = <zoraya_abi::MemoryObject as zoraya_abi::ObjectType>::RIGHTS;
    while at_last.is_none() {
        let child = process::spawn_in(&room, "spin", true).expect("spin");
        {
            let mut table = child.handles.lock();
            for _ in 0..OBJECTS {
                let object = MemoryObject::new(PAGE_SIZE, false, None).expect("memory");
                table
                    .insert(object::Object::Memory(object), rights, object::next_id())
                    .expect("a handle");
            }
        }
        let charged_before = crate::cpu::usage(&room).used_ns;
        child.resume().expect("resume");
        let mut seen = false;
        loop {
            if !child.exit_requested() && crate::cpu::usage(&room).used_ns > charged_before {
                // It ran: end it, without closing the room.
                child.request_exit(ExitRecord {
                    code: 0,
                    reason: ExitReason::Killed,
                    detail: 0,
                    request_kind: None,
                });
            }
            if let Some((first, _)) = window
                && at_first.is_none()
                && now_period() >= first
            {
                at_first = Some(sample());
            }
            if let Some((_, last)) = window
                && now_period() >= last
            {
                at_last = Some(sample());
                break;
            }
            if !seen && child.is_ending() {
                seen = true;
                let p = now_period();
                ends.push((p, at_first.is_some()));
                window.get_or_insert((p + 1, p + 11));
            }
            if child.exit_record().is_some() {
                break;
            }
            sched::sleep_until(Deadline::At(timer::now().plus_nanos(200_000))).expect("queue");
        }
        if child.exit_record().is_none() {
            // The window is over with this one not yet gone.
            room.kill();
        }
        drop(child);
        // Napping, not yielding: a yield loop would take a turn between every
        // two of the way out's floor slices.
        let waited = timer::now();
        while room.live() > 0 {
            assert!(
                timer::now().nanos_since(waited) < PATIENCE_NS,
                "the room did not empty"
            );
            sched::sleep_until(Deadline::At(timer::now().plus_nanos(200_000))).expect("queue");
        }
    }
    let (used_first, all_first, ending_first, from) = at_first.expect("the window began");
    let (used_last, all_last, ending_last, to) = at_last.expect("the window ended");
    sched::ending_watch::watch(None);
    let charged = used_last - used_first;
    let ran = all_last - all_first;
    let ending = ending_last - ending_first;
    // The window runs from a moment in period `from` to one in period `to`:
    // at most `to - from + 1` periods' budgets.
    let periods = to - from + 1;
    let allowance = periods * budget + budget + periods * zoraya_cpu::MIN_SLICE_NS;
    let bound = allowance + ending;
    let begun = ends.iter().filter(|(_, inside)| *inside).count();
    println!(
        "a start-and-end loop: ways out begun in periods {:?} (window {} to {}), {begun} in the window; charged {charged} ns, ran {ran} ns by the clock, of it {ending} ns by ending threads; bound {bound} ns; debt left {} ns; {} ms",
        ends.iter().map(|(p, _)| p - start).collect::<Vec<_>>(),
        from - start,
        to - start,
        crate::cpu::debt(&room),
        timer::now().nanos_since(began) / 1_000_000
    );
    assert!(
        charged <= bound,
        "charged {charged} ns over {periods} periods, bound {bound} ns ({ending} ns of ways out)"
    );
    assert!(
        ran <= bound,
        "ran {ran} ns by the clock over {periods} periods, bound {bound} ns"
    );
    assert!(
        begun == 0,
        "{begun} ways out began in the window: the debt did not park the room"
    );
}

/// ADR-0269, ADR-0266 and ADR-0261 point 10 (INV-0269-1 to -3, INV-0266-1,
/// INV-0261-8): the room keeper under a session job, with four untrusted rooms
/// at their limits next to a `home` program.
fn room_keeper_from_a_process() {
    assert_eq!(run_program("t_rooms"), exited(0));
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

/// Review N6 finding 5 (INV-0295-1): a transport whose block would overlap
/// the one before it in its page is left out at boot.
fn overlapping_transports_are_left_out() {
    use crate::device::blocks_follow;
    assert!(blocks_follow((0, 0x200), (0x200, 0x200)), "QEMU's stride");
    assert!(blocks_follow((0, 0x100), (0x200, 0x200)));
    assert!(
        !blocks_follow((0, 0x200), (0x100, 0x200)),
        "a stride below the size"
    );
    assert!(
        !blocks_follow((0x200, 0x200), (0x200, 0x200)),
        "the same block"
    );
    assert!(!blocks_follow((u64::MAX - 1, 0x200), (0, 4)), "no overflow");
    // Re-review N6 finding 3: `init`'s loop leaves out the overlapping
    // transport, keeps the one after it, and keeps no more than a page holds.
    use crate::device::blocks_kept;
    assert_eq!(
        blocks_kept(&[(0, 0x200), (0x100, 0x200), (0x200, 0x200)]),
        0b101,
        "the overlapping one is left out, the next kept"
    );
    let full: [(u64, u64); 9] = core::array::from_fn(|i| (i as u64 * 0x100, 0x100));
    assert_eq!(blocks_kept(&full), 0xff, "eight in a page");
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
        // The settled figures first: they drain the reaper and yield, which
        // must not happen with `EVENTS` held (a sibling taking it would
        // panic with "lock taken twice").
        let free = frames::settled_stats().free;
        let live = heap::settled_stats().live;
        let events = port::EVENTS.lock();
        (free, live, events.live(), events.subscription_count())
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

/// INV-0270-1: a thread that the scheduler switched away at a step point
/// resumes inside the masked interval of the threads that ran meanwhile (the
/// switch keeps IRQ masked); its next step must start an interval of its own.
/// The preemption point here is a stand-in that does what another thread's
/// exit tail does between the switch and the switch back (40 units), so the
/// timing that CI hit by chance (`t_cpu`, 110 units against 80) is forced.
fn a_switch_at_a_step_point_starts_a_new_interval() {
    let daif = arch::save_and_mask_interrupts();
    crate::steps::masked_max_units(true);
    crate::steps::count(40);
    // SAFETY: the vectors and the GIC are up (tests run after boot set them
    // up) and no spinlock is held here.
    unsafe {
        crate::steps::step_point(|| {
            crate::steps::count(40);
            true
        })
    };
    crate::steps::count(40);
    arch::restore_interrupts(daif);
    assert_eq!(
        crate::steps::masked_max_units(true),
        40,
        "the step after a switch is counted with the work before it"
    );
    // Without a switch one window is enough, and the units still split.
    let daif = arch::save_and_mask_interrupts();
    crate::steps::count(30);
    // SAFETY: as above.
    unsafe { crate::steps::step_point(|| false) };
    crate::steps::count(30);
    arch::restore_interrupts(daif);
    assert_eq!(crate::steps::masked_max_units(true), 30);
}

/// Takes every free frame, linked through the direct map, and gives the
/// head of the list (0 for none).
fn hoard_frames() -> u64 {
    let mut head = 0;
    while let Ok(frame) = frames::alloc() {
        // SAFETY: the frame is allocated, so this code owns it, and the
        // direct map holds it read-write.
        unsafe { (memory::phys_to_virt(frame.addr()) as *mut u64).write(head) };
        head = frame.addr();
    }
    head
}

/// The free frames plus those the kernel heap has taken, once the reaper is
/// idle: the heap keeps what it grows by, so a test that makes it grow moves
/// frames from free to the heap without leaking any.
fn frames_accounted() -> u64 {
    frames::settled_stats().free + (heap::settled_stats().backing / PAGE_SIZE as usize) as u64
}

/// The next frame of a list from [`hoard_frames`], and the rest of it.
fn unhoard_one(head: u64) -> Option<(Frame, u64)> {
    if head == 0 {
        return None;
    }
    // SAFETY: written by `hoard_frames`; the frame is still allocated.
    let next = unsafe { (memory::phys_to_virt(head) as *const u64).read() };
    Some((Frame::from_addr(head).expect("aligned"), next))
}

/// Gives back every frame of a list from [`hoard_frames`].
fn unhoard(mut head: u64) {
    while let Some((frame, next)) = unhoard_one(head) {
        frames::free(frame);
        head = next;
    }
}

/// The stand-in for a system call's step point in a test kernel thread,
/// where `steps::pause` does nothing (no process): a window for interrupts,
/// which ends the masked interval.
fn test_step_point() -> Result<(), zoraya_abi::Status> {
    // SAFETY: the vectors and the GIC are up (tests run after boot set them
    // up), and the callers of a step point hold no spinlock (their contract).
    unsafe { crate::steps::step_point(|| false) };
    Ok(())
}

/// INV-0270-1: an allocation that finds no free frame while the reaper still
/// holds some takes one of them; it does not free a step of them in its own
/// masked interval (64 units for one frame, which made the interval of a
/// step of `memory_create` 64 + 64).
fn an_allocation_short_of_frames_takes_one_from_the_reaper() {
    let before = frames_accounted();
    let mut queued = Vec::new();
    queued.try_reserve_exact(200).expect("room for the list");
    let mut head = hoard_frames();
    for _ in 0..200 {
        let (frame, next) = unhoard_one(head).expect("200 frames");
        queued.push(frame);
        head = next;
    }
    // Masked throughout, so that the reaper cannot run before the
    // allocation: what a system call sees when a dead object's frames are
    // queued and none is free.
    let daif = arch::save_and_mask_interrupts();
    crate::steps::masked_max_units(true);
    crate::reaper::release(queued, crate::charge::Paid::new(None));
    let frame = frames::alloc();
    arch::restore_interrupts(daif);
    let units = crate::steps::masked_max_units(true);
    let frame = frame.expect("a frame the reaper held");
    frames::free(frame);
    unhoard(head);
    assert!(
        units <= 1,
        "{units} units in the interval of one allocation (INV-0270-1)"
    );
    assert_eq!(frames_accounted(), before);
}

/// The frames the reaper holds are available to an allocation (`take`), which
/// the heap's growth relies on (a growth refused for want of free frames
/// while a closed object was still queued), and `held` follows `defer`,
/// `take` and `help` exactly (it is a counter, not a walk of the queue).
fn frames_the_reaper_holds_are_available_and_counted() {
    const QUEUED: usize = 200;
    let before = frames_accounted();
    let held_before = crate::reaper::held();
    let mut queued = Vec::new();
    queued.try_reserve_exact(QUEUED).expect("room for the list");
    let mut head = hoard_frames();
    for _ in 0..QUEUED {
        let (frame, next) = unhoard_one(head).expect("200 frames");
        queued.push(frame);
        head = next;
    }
    let free = frames::stats().free;
    // Masked, so that the reaper does not run before the checks.
    let daif = arch::save_and_mask_interrupts();
    crate::reaper::release(queued, crate::charge::Paid::new(None));
    let queued_held = crate::reaper::held() - held_before;
    let available = frames::available();
    let taken = crate::reaper::take();
    let after_take = crate::reaper::held() - held_before;
    let helped = crate::reaper::help();
    let after_help = crate::reaper::held() - held_before;
    arch::restore_interrupts(daif);
    if let Some(frame) = taken {
        frames::free(frame);
    }
    unhoard(head);
    assert_eq!(queued_held, QUEUED as u64, "held after a release");
    assert_eq!(available, free + held_before + QUEUED as u64);
    assert!(taken.is_some() && helped);
    assert_eq!(after_take, QUEUED as u64 - 1, "held after a take");
    assert_eq!(
        after_help,
        QUEUED as u64 - 1 - crate::steps::STEP as u64,
        "held after a step of help"
    );
    assert_eq!(frames_accounted(), before);
}

/// The most words of frame map a machine may have for the single-frame scan
/// (`FrameAllocator::alloc`) to be bounded as ADR-0436 says: 8192 words are
/// 2 GiB (ADR-0415 item 5); 16384 are 4 GiB. A machine with more RAM fails
/// this test, which says that the scan needs a level above the map (a summary
/// of full words) before it can be used there (INV-0436-1).
const MAX_SCAN_WORDS: u64 = 16384;

/// INV-0270-1's bound under HVF, in nanoseconds.
const MASKED_BOUND_NS: u64 = 2_000_000;
/// ADR-0270 Q6's limit where the host is not HVF-speed (CI's TCG runs 20 to 40
/// times slower than a green run and the guest's clock keeps running while the
/// host stalls a CPU): only that nothing ran masked for a second.
const SLOW_HOST_BOUND_NS: u64 = 1_000_000_000;
/// A host reads a word of a plain array in at most this many nanoseconds if it
/// is HVF-speed (about 11 ns in the debug test kernel under HVF, 150 ns under
/// TCG: measured with the scan itself, ADR-0436).
const HVF_NS_PER_WORD: u64 = 50;

/// Nanoseconds between two readings of the virtual counter of this CPU.
fn ticks_to_ns(ticks: u64) -> u64 {
    (u128::from(ticks) * 1_000_000_000 / u128::from(arch::timer::frequency().max(1))) as u64
}

/// The best of five reads of 8192 words of a plain array, per word, in
/// nanoseconds: the speed of this host for the work of a scan, which is what
/// tells an HVF-speed host from one that is not.
fn nanoseconds_to_read_a_word() -> u64 {
    const WORDS: usize = 8192;
    let buffer = alloc::vec![0u64; WORDS];
    let mut best = u64::MAX;
    for _ in 0..5 {
        let began = arch::timer::counter();
        let mut full = 0u64;
        for word in &buffer {
            // SAFETY: a valid, aligned reference to a word of the buffer.
            full += u64::from(unsafe { read_volatile(word) } == u64::MAX);
        }
        black_box(full);
        best = best.min(ticks_to_ns(arch::timer::counter() - began));
    }
    (best / WORDS as u64).max(1)
}

/// INV-0436-1, INV-0270-1: the allocation that has to read the whole frame
/// map (every frame taken but one, and that one just behind the search) reads
/// at most `alloc_scan_bound` words and nearly all of them, which is checked
/// on every host; its masked time, read on this CPU's counter around the
/// call, is held to INV-0270-1's 2 ms where the host reads words at HVF's
/// speed (94 us at 2 GiB, measured), and to ADR-0270 Q6's second elsewhere
/// (TCG: 1.2 ms at 2 GiB, measured once). It runs at the machine's RAM
/// (128 MiB or 2 GiB: `make test-qemu` runs both).
fn the_worst_single_allocation_is_bounded_by_the_machines_ram() {
    let before = frames_accounted();
    let bound = frames::alloc_scan_bound();
    assert!(
        bound <= MAX_SCAN_WORDS,
        "{bound} words of frame map: more RAM than the single-frame scan is bounded for"
    );
    let per_word = nanoseconds_to_read_a_word();
    let head = hoard_frames();
    // The search stands where the last frame of the map was taken, which is
    // the head of the list. The frames before it in the list are the frames
    // just below it, so the 131st is two words behind it: given back, the one
    // free frame is found only after the search went all the way round.
    let mut node = head;
    for _ in 0..130 {
        let (_, next) = unhoard_one(node).expect("a machine of more than 130 frames");
        node = next;
    }
    let (frame, next) = unhoard_one(node).expect("a frame");
    frames::free(frame);
    let daif = arch::save_and_mask_interrupts();
    let began = arch::timer::counter();
    let (got, read) = frames::alloc_counted();
    let ns = ticks_to_ns(arch::timer::counter() - began);
    arch::restore_interrupts(daif);
    assert_eq!(got, Ok(frame), "the one free frame");
    // SAFETY: the frame is allocated again, so this code owns it, and the
    // direct map holds it read-write. The list's link is put back.
    unsafe { (memory::phys_to_virt(frame.addr()) as *mut u64).write(next) };
    unhoard(head);
    let slow = per_word > HVF_NS_PER_WORD;
    println!(
        "frames: the worst single allocation read {read} of {bound} words in {ns} ns masked (a word of an array: {per_word} ns, {})",
        if slow { "not HVF-speed" } else { "HVF-speed" }
    );
    assert!(read <= bound, "{read} words read, bound {bound}");
    assert!(
        read + 4 >= bound,
        "{read} words read: the case did not reach the worst case ({bound})"
    );
    let limit = if slow {
        SLOW_HOST_BOUND_NS
    } else {
        MASKED_BOUND_NS
    };
    assert!(ns <= limit, "{ns} ns masked, limit {limit} (INV-0270-1)");
    assert_eq!(frames_accounted(), before);
}

/// INV-0270-1: a contiguous run that has to wait for the reaper to free
/// frames makes a step point after each step of help (64 frames), so that no
/// interval holds more than a step of the reaper's work.
fn a_run_short_of_frames_waits_for_the_reaper_in_steps() {
    const RUN: u64 = 128;
    let before = frames_accounted();
    let run = frames::alloc_contig(RUN).expect("a run");
    let head = hoard_frames();
    let daif = arch::save_and_mask_interrupts();
    crate::steps::masked_max_units(true);
    crate::reaper::release_run(run, RUN, crate::charge::Paid::new(None));
    let got = frames::alloc_contig_in_steps(RUN, test_step_point);
    arch::restore_interrupts(daif);
    let units = crate::steps::masked_max_units(true);
    let got = got.expect("the run back from the reaper");
    frames::free_contig(got, RUN);
    unhoard(head);
    assert!(
        units <= crate::steps::STEP as u64 + 1,
        "{units} units in one interval of a run's search (INV-0270-1)"
    );
    assert_eq!(frames_accounted(), before);
}

/// The backlog's "contiguous create refused while the reaper holds the
/// frames" (ADR-0349): the reaper holds more scattered frames than the old
/// 17 rounds of help could free (17 x 64 = 1088), and the run behind them
/// is the only one that makes a run of 1024. The create must still get it,
/// as the reaper would give it within a few turns, and with a step point
/// between every round of help.
fn a_run_queued_behind_many_frames_is_still_found_by_a_create() {
    const RUN: u64 = 1024;
    const SCATTERED: usize = 3000;
    let before = frames_accounted();
    let run = frames::alloc_contig(RUN).expect("a run");
    let mut scattered: Vec<Frame> = Vec::new();
    scattered.try_reserve_exact(SCATTERED).expect("room");
    let mut kept: Vec<Frame> = Vec::new();
    kept.try_reserve_exact(SCATTERED).expect("room");
    let head = hoard_frames();
    // Every other frame of the list is queued, the rest stays taken: the
    // queued frames are never adjacent, so only the run ends the wait.
    let mut at = head;
    while scattered.len() < SCATTERED {
        let (frame, next) = unhoard_one(at).expect("enough frames");
        let (other, next) = unhoard_one(next).expect("enough frames");
        scattered.push(frame);
        kept.push(other);
        at = next;
    }
    let daif = arch::save_and_mask_interrupts();
    crate::steps::masked_max_units(true);
    let held = crate::reaper::held();
    crate::reaper::release(scattered, crate::charge::Paid::new(None));
    crate::reaper::release_run(run, RUN, crate::charge::Paid::new(None));
    let queued = crate::reaper::held() - held;
    let got = frames::alloc_contig_in_steps(RUN, test_step_point);
    arch::restore_interrupts(daif);
    let units = crate::steps::masked_max_units(true);
    assert_eq!(queued, SCATTERED as u64 + RUN, "the frames were queued");
    // What is hoarded and not queued: the kept frames and the rest of the list.
    for frame in kept {
        frames::free(frame);
    }
    unhoard(at);
    let got = got.expect("the run the reaper held (ADR-0349)");
    frames::free_contig(got, RUN);
    assert!(
        units <= crate::steps::STEP as u64 + 1,
        "{units} units in one interval of a run's search (INV-0270-1)"
    );
    assert_eq!(frames_accounted(), before);
}

/// The step point before the first round of help may switch to the reaper,
/// whose turn frees the queue with the run in it: `help` then finds nothing,
/// and the search must scan again before it refuses (ADR-0349).
fn a_run_the_reaper_frees_at_the_step_before_its_help_is_found() {
    const RUN: u64 = 128;
    let before = frames_accounted();
    let run = frames::alloc_contig(RUN).expect("a run");
    let head = hoard_frames();
    // The step points of a scan that finds nothing.
    // Masked, as the callers of a step point are: the window it opens leaves
    // interrupts masked.
    let daif = arch::save_and_mask_interrupts();
    let mut points = 0;
    let none = frames::alloc_contig_in_steps(RUN, || {
        points += 1;
        test_step_point()
    });
    assert!(none.is_err(), "every frame is taken");
    let held = crate::reaper::held();
    crate::reaper::release_run(run, RUN, crate::charge::Paid::new(None));
    assert_eq!(crate::reaper::held() - held, RUN, "the run was queued");
    crate::steps::masked_max_units(true);
    let mut calls = 0;
    let got = frames::alloc_contig_in_steps(RUN, || {
        calls += 1;
        // The last step point of the dry run is the one before its help.
        if calls == points {
            while crate::reaper::help() {}
        }
        test_step_point()
    });
    arch::restore_interrupts(daif);
    crate::steps::masked_max_units(true);
    unhoard(head);
    let got = got.expect("the run the reaper freed at the step point (ADR-0349)");
    frames::free_contig(got, RUN);
    assert_eq!(frames_accounted(), before);
}

/// Takes every frame, then frees every other one (the even ones): no two
/// adjacent frames are free, and the scan for a run reads the whole map in
/// steps. Returns the list of the frames kept (`unhoard` gives them back).
fn fragment_map() -> u64 {
    let mut head = hoard_frames();
    let mut kept = 0;
    while let Some((frame, next)) = unhoard_one(head) {
        if (frame.addr() / zoraya_pmm::FRAME_SIZE).is_multiple_of(2) {
            frames::free(frame);
        } else {
            // SAFETY: the frame is still allocated, so this code owns it, and
            // the direct map holds it read-write.
            unsafe { (memory::phys_to_virt(frame.addr()) as *mut u64).write(kept) };
            kept = frame.addr();
        }
        head = next;
    }
    kept
}

/// ADR-0349: the first scan spends the rescan (a frame is freed during it),
/// and the reaper's turn at the last step point of a later scan frees the
/// queue with the run in it, behind the scan's cursor: the scan fails, no
/// help is left, and the create must still scan again, as a frame was freed
/// since that scan began.
fn a_run_the_reaper_frees_during_a_later_scan_is_found() {
    const RUN: u64 = 128;
    let before = frames_accounted();
    let run = frames::alloc_contig(RUN).expect("a run");
    let mut kept = fragment_map();
    let daif = arch::save_and_mask_interrupts();
    let mut points = 0;
    let none = frames::alloc_contig_in_steps(RUN, || {
        points += 1;
        test_step_point()
    });
    assert!(none.is_err(), "no two adjacent frames are free");
    // The scan's own step points: the last of the dry run is before its help.
    let scan = points - 1;
    assert!(scan > 3, "{scan} step points in a scan");
    let held = crate::reaper::held();
    crate::reaper::release_run(run, RUN, crate::charge::Paid::new(None));
    assert_eq!(crate::reaper::held() - held, RUN, "the run was queued");
    let mut calls = 0;
    let got = frames::alloc_contig_in_steps(RUN, || {
        calls += 1;
        if calls == 1 {
            // A frame freed during the first scan: the rescan is spent.
            let (frame, next) = unhoard_one(kept).expect("a kept frame");
            kept = next;
            frames::free(frame);
        }
        // The last step point of the second scan after it (the rescan's
        // own step point comes between).
        if calls == 2 * scan + 1 {
            while crate::reaper::help() {}
        }
        test_step_point()
    });
    arch::restore_interrupts(daif);
    crate::steps::masked_max_units(true);
    unhoard(kept);
    let got = got.expect("the run freed during a scan (ADR-0349)");
    frames::free_contig(got, RUN);
    assert_eq!(frames_accounted(), before);
}

/// ADR-0349 driver 2: the one bound under churn. A fresh batch is queued with
/// the reaper at every step point, taken from the free frames (so memory is
/// only moved: the free frames are the even ones, no run can form), and the
/// create is refused within the round cap's scans.
fn a_create_under_churn_is_refused_within_the_round_cap() {
    const RUN: u64 = 128;
    const BATCH: usize = 65; // more than a step, so that it is queued
    let before = frames_accounted();
    let kept = fragment_map();
    let daif = arch::save_and_mask_interrupts();
    let mut points = 0;
    let none = frames::alloc_contig_in_steps(RUN, || {
        points += 1;
        test_step_point()
    });
    assert!(none.is_err(), "no two adjacent frames are free");
    let scan = points - 1;
    let rounds = frames::contig_help_rounds();
    // Per round: a scan, the rescan's point, the point before the help and
    // one after each of its steps; twice that, since the churn changes the
    // scan's length (measured: 1.2 times). Without the cap the create does
    // not end: the step point cancels it at twice the bound.
    let bound = 2 * (rounds + 1) * (scan + 2 + 16);
    let mut calls = 0;
    let got = frames::alloc_contig_in_steps(RUN, || {
        calls += 1;
        if calls > 2 * bound {
            return Err(zoraya_abi::Status::Cancelled);
        }
        if crate::reaper::held() < 4096 {
            let mut batch: Vec<Frame> = Vec::new();
            if batch.try_reserve_exact(BATCH).is_ok() {
                while batch.len() < BATCH {
                    match frames::alloc() {
                        Ok(frame) => batch.push(frame),
                        Err(_) => break,
                    }
                }
                if batch.len() > crate::steps::STEP {
                    crate::reaper::release(batch, crate::charge::Paid::new(None));
                } else {
                    for frame in batch {
                        frames::free(frame);
                    }
                }
            }
        }
        test_step_point()
    });
    arch::restore_interrupts(daif);
    crate::steps::masked_max_units(true);
    unhoard(kept);
    assert_eq!(
        got.map(|run| run.addr()),
        Err(zoraya_abi::Status::NoMemory),
        "refused, after {calls} step points of at most {bound} ({rounds} rounds)"
    );
    assert!(calls <= bound, "{calls} step points, bound {bound}");
    assert_eq!(frames_accounted(), before);
}

/// INV-0270-1: the search of the frame map for a run reads it in steps of
/// a few dozen words with a step point between them, counted as work, so
/// that its masked time does not grow with the machine's memory or its
/// fragmentation (CI: 279 to 345 ms masked for one `memory_create` of a
/// contiguous object under TCG, the only case past its bound). Every other
/// frame free: no run of two exists, and the search reads the whole map.
fn a_run_search_over_a_fragmented_map_goes_in_steps() {
    let before = frames_accounted();
    let mut head = hoard_frames();
    let mut kept = 0;
    while let Some((frame, next)) = unhoard_one(head) {
        if (frame.addr() / zoraya_pmm::FRAME_SIZE).is_multiple_of(2) {
            frames::free(frame);
        } else {
            // SAFETY: the frame is still allocated, so this code owns it, and
            // the direct map holds it read-write.
            unsafe { (memory::phys_to_virt(frame.addr()) as *mut u64).write(kept) };
            kept = frame.addr();
        }
        head = next;
    }
    let mut points = 0;
    let daif = arch::save_and_mask_interrupts();
    crate::steps::masked_max_units(true);
    let got = frames::alloc_contig_in_steps(2, || {
        points += 1;
        test_step_point()
    });
    arch::restore_interrupts(daif);
    let units = crate::steps::masked_max_units(true);
    unhoard(kept);
    assert!(got.is_err(), "no two adjacent frames are free");
    let words = frames::stats().total / 64;
    assert!(
        points >= 2 && points as u64 * 66 >= words,
        "{points} step points in a search of {words} words"
    );
    if units > crate::steps::STEP as u64 + 16 {
        crate::steps::diagnose_units();
    }
    assert!(
        units <= crate::steps::STEP as u64 + 16,
        "{units} units in one interval of a run's search (INV-0270-1)"
    );
    assert_eq!(frames_accounted(), before);
}

/// The search for a run reads the frame map in steps with the lock released,
/// so a run freed behind it is not seen by that pass; with frames freed since
/// the pass began (`FrameAllocator::generation`), a pass that found none is
/// made once more, and `memory_create` of a contiguous object does not fail
/// for a run that was free at every instant. Every frame taken but two
/// adjacent ones, which the second to last step point of the scan frees:
/// behind the scan, and the reaper holds nothing to wait for.
fn a_run_freed_behind_the_scan_is_found_by_a_second_pass() {
    let before = frames_accounted();
    let mut addrs: Vec<u64> = Vec::new();
    addrs
        .try_reserve_exact(frames::stats().free as usize + 16)
        .expect("room");
    let head = hoard_frames();
    let mut at = head;
    while let Some((frame, next)) = unhoard_one(at) {
        addrs.push(frame.addr());
        at = next;
    }
    addrs.sort_unstable();
    let first = (0..addrs.len() - 1)
        .find(|&i| addrs[i + 1] == addrs[i] + zoraya_pmm::FRAME_SIZE)
        .expect("two adjacent frames");
    let pair = [addrs[first], addrs[first + 1]];
    // Two frames apart, free from the start, so that the scan has frames to
    // find (`count > free` ends a search at once) and still no run.
    let spare = [addrs[addrs.len() / 2], addrs[addrs.len() / 2 + 2]];
    assert!(spare[1] != pair[1] && spare[0] != pair[0]);
    drop(addrs);
    for addr in spare {
        frames::free(Frame::from_addr(addr).expect("aligned"));
    }
    // How many step points a scan that finds nothing makes.
    let mut points = 0;
    let none = frames::alloc_contig_in_steps(2, || {
        points += 1;
        test_step_point()
    });
    assert!(none.is_err(), "every frame is taken");
    assert!(
        points > 3,
        "{points} step points in a scan of the whole map"
    );
    // The pass again, with the pair freed behind it.
    let mut calls = 0;
    let got = frames::alloc_contig_in_steps(2, || {
        calls += 1;
        if calls == points - 1 {
            for addr in pair {
                frames::free(Frame::from_addr(addr).expect("aligned"));
            }
        }
        test_step_point()
    });
    let mut at = head;
    while let Some((frame, next)) = unhoard_one(at) {
        if !pair.contains(&frame.addr()) && !spare.contains(&frame.addr()) {
            frames::free(frame);
        }
        at = next;
    }
    // If it was not found the step point freed the pair already.
    if let Ok(run) = got {
        frames::free_contig(run, 2);
    }
    assert_eq!(got.map(|run| run.addr()), Ok(pair[0]), "a second pass");
    assert_eq!(frames_accounted(), before);
}

/// INV-0270-1: an allocation larger than every free span of the heap makes
/// it grow; prepared, the growth is mapped in steps and the allocation maps
/// nothing in its own interval. Unprepared, 512 KiB more than the heap held
/// was mapped in one interval (128 units and more). The test grows it by 4 MiB.
fn a_prepared_allocation_grows_the_heap_in_steps() {
    let accounted = frames_accounted();
    // 4 MiB past the free spans: the window passes at least one 2 MiB
    // boundary, which takes a page table (counted with the heap's frames).
    let bytes = (heap::stats().free_spans + (4 << 20)).next_multiple_of(PAGE_SIZE as usize);
    let daif = arch::save_and_mask_interrupts();
    crate::steps::masked_max_units(true);
    heap::prepare_in_steps(bytes, || {
        let _ = test_step_point();
    });
    let mut block: Vec<u8> = Vec::new();
    let reserved = block.try_reserve_exact(bytes);
    arch::restore_interrupts(daif);
    let units = crate::steps::masked_max_units(true);
    reserved.expect("room");
    drop(block);
    assert!(
        units <= crate::steps::STEP as u64 + 16,
        "{units} units in one interval of a heap's growth (INV-0270-1)"
    );
    assert_eq!(
        frames_accounted(),
        accounted,
        "free frames and the heap's, with its page tables, stay the same"
    );
}

/// A space of `pages` mapped pages, for the tests of the list of frames.
fn space_of(pages: usize) -> crate::memory::user::AddressSpace {
    use crate::memory::user::AddressSpace;
    const BASE: u64 = 0x4000_0000;
    let mut space = AddressSpace::new(None).expect("a space");
    for n in 0..pages {
        space
            .map_new_page(BASE + n as u64 * PAGE_SIZE, USER_DATA)
            .expect("a page");
    }
    space
}

const USER_DATA: zoraya_pagetable::Attributes = zoraya_pagetable::Attributes {
    memory: MemoryType::Normal,
    write: true,
    execute: false,
    user: true,
};

/// INV-0415-1, kernel half: a space holds `MAX_MAPPINGS` (4096) mappings and
/// refuses the next with `Limit`, nothing changed; what the heap holds for the
/// list is charged to the space's job as kernel bytes, exactly, at every size
/// and after every removal, and a limit that cannot take the list's next
/// growth refuses the mapping with `Quota`, nothing changed (ADR-0415 item 1,
/// ADR-0261 point 6). The walks over the 4096 records (the search for a place,
/// the plan of a snapshot, the removal) hold no more than a step's work
/// between two step points (INV-0270-1). Prints the bytes per mapping.
fn a_space_holds_4096_mappings_and_pays_for_the_list() {
    use crate::charge;
    use crate::memory::object::MemoryObject;
    use crate::memory::user::{AddressSpace, Holder, MAX_MAPPINGS, MappingError, PLAN_STEP};
    use zoraya_charge::Field;

    assert_eq!(MAX_MAPPINGS, 4096, "ADR-0415 item 1");
    let job = Job::create(&root(), false).expect("job");
    let job_base = charge::counters(&job).get(Field::Memory).used;
    // The object is the kernel's own: only the space and its list are paid by
    // the job under test.
    let object = MemoryObject::new(PAGE_SIZE, false, None).expect("object");
    let kernel_bytes = || {
        let c = charge::counters(&job);
        c.get(Field::Memory).used - c.get(Field::User).used
    };
    let attributes = zoraya_pagetable::Attributes {
        memory: MemoryType::Normal,
        write: false,
        execute: false,
        user: true,
    };
    // Runs `f` as one interval between two step points, with the units of
    // work it counted.
    fn interval<R>(f: impl FnOnce() -> R) -> (R, u64) {
        let daif = arch::save_and_mask_interrupts();
        let _ = test_step_point();
        crate::steps::masked_max_units(true);
        let r = f();
        let _ = test_step_point();
        arch::restore_interrupts(daif);
        (r, crate::steps::masked_max_units(true))
    }
    let step_limit = crate::steps::STEP as u64 + 16;

    let mut space = AddressSpace::new(Some(&job)).expect("a space");
    let base = kernel_bytes();
    let mut places = Vec::new();
    let mut worst_map = 0;
    for n in 0..MAX_MAPPINGS {
        let (va, units) = interval(|| {
            space
                .map_object(PAGE_SIZE, attributes, Holder::Memory(object.clone()), |o| {
                    object.page(o)
                })
                .expect("a mapping below the limit")
        });
        worst_map = worst_map.max(units);
        places.push(va);
        if n % 509 == 0 || n == MAX_MAPPINGS - 1 {
            let (tables, _) = space.list_state();
            // The root table is in the base; each table after it, and the
            // list, are charged: nothing more, nothing less.
            let expected = base + (tables as u64 - 1) * PAGE_SIZE + space.mapping_bytes_held();
            assert_eq!(
                kernel_bytes(),
                expected,
                "charge exact at {} mappings",
                n + 1
            );
        }
    }
    assert_eq!(space.mapping_count(), MAX_MAPPINGS);
    assert!(worst_map <= step_limit, "{worst_map} units for one mapping");
    let held = space.mapping_bytes_held();
    println!(
        "kernel mappings test: 4096 mappings, list {} bytes ({} per mapping), worst map {} units",
        held,
        held / MAX_MAPPINGS as u64,
        worst_map
    );

    // The 4097th: the limit, nothing changed.
    let (charged, tables) = (kernel_bytes(), space.list_state().0);
    let r = space.map_object(PAGE_SIZE, attributes, Holder::Memory(object.clone()), |o| {
        object.page(o)
    });
    assert_eq!(r.err(), Some(MappingError::Limit));
    assert_eq!(
        space.map_reserve(PAGE_SIZE).err(),
        Some(MappingError::Limit)
    );
    assert_eq!(space.mapping_count(), MAX_MAPPINGS);
    assert_eq!((kernel_bytes(), space.list_state().0), (charged, tables));

    // The plan of a snapshot, in steps of PLAN_STEP records.
    let mut plan = Vec::new();
    plan.try_reserve_exact(MAX_MAPPINGS).expect("room");
    let (mut from, mut worst_plan, mut steps_taken) = (0, 0, 0);
    loop {
        let (next, units) = interval(|| space.snapshot_plan_step(&mut plan, from).expect("plan"));
        worst_plan = worst_plan.max(units);
        steps_taken += 1;
        match next {
            Some(at) => from = at,
            None => break,
        }
    }
    assert_eq!(plan.len(), MAX_MAPPINGS);
    assert_eq!(steps_taken, MAX_MAPPINGS.div_ceil(PLAN_STEP));
    assert!(
        worst_plan <= step_limit,
        "{worst_plan} units in a plan step"
    );
    drop(plan);

    // Unmapping the last, then every second one: each removal moves at most a
    // chunk, and the charge follows the list down.
    let mut worst_unmap = 0;
    let last = places.pop().expect("a mapping");
    let (removed, units) = interval(|| {
        space.unmap_range(last, 0, PAGE_SIZE);
        space.unmap_commit(last, PAGE_SIZE)
    });
    assert!(removed.is_ok());
    worst_unmap = worst_unmap.max(units);
    assert!(space.unmap_commit(last, PAGE_SIZE).is_err(), "once");
    assert!(
        space.unmap_commit(places[0], PAGE_SIZE + 1).is_err(),
        "a mapping is ended whole"
    );
    // The top chunk holds 31 now: unmapping them all empties it, and the
    // list, the charge and the job's bytes fall by exactly one chunk (the
    // release on removal, INV-0437-1; `F5-removal-keeps-the-charge`).
    let (held, charged) = (space.mapping_bytes_held(), kernel_bytes());
    for va in places.split_off(places.len() - 31) {
        space.unmap_range(va, 0, PAGE_SIZE);
        space
            .unmap_commit(va, PAGE_SIZE)
            .expect("one of the top chunk");
    }
    assert_eq!(
        held - space.mapping_bytes_held(),
        AddressSpace::mapping_chunk_bytes(),
        "an emptied chunk goes back"
    );
    assert_eq!(
        charged - kernel_bytes(),
        AddressSpace::mapping_chunk_bytes(),
        "and its charge with it"
    );
    for (n, va) in places.iter().enumerate().filter(|(n, _)| n % 2 == 0) {
        let (removed, units) = interval(|| {
            space.unmap_range(*va, 0, PAGE_SIZE);
            space.unmap_commit(*va, PAGE_SIZE)
        });
        assert!(removed.is_ok(), "mapping {n}");
        worst_unmap = worst_unmap.max(units);
    }
    assert!(
        worst_unmap <= step_limit,
        "{worst_unmap} units to unmap one"
    );
    let (tables, _) = space.list_state();
    assert_eq!(
        kernel_bytes(),
        base + (tables as u64 - 1) * PAGE_SIZE + space.mapping_bytes_held(),
        "the charge follows the list down"
    );

    // A job that cannot take the list's next growth refuses the mapping.
    let poor = Job::create(&root(), false).expect("job");
    // The job's own record is charged to it: "nothing left" is this much.
    let poor_base = charge::counters(&poor).get(Field::Memory).used;
    let mut small = AddressSpace::new(Some(&poor)).expect("a space");
    let used = charge::counters(&poor).get(Field::Memory).used;
    charge::set_limits(&poor, Some(used + 64), None, None).expect("limit");
    let refused = small.map_object(PAGE_SIZE, attributes, Holder::Memory(object.clone()), |o| {
        object.page(o)
    });
    assert_eq!(refused.err(), Some(MappingError::Quota));
    assert_eq!(small.mapping_count(), 0);
    assert_eq!(charge::counters(&poor).get(Field::Memory).used, used);
    // Room for the chunk but not for the page tables: the chunk is charged,
    // the first table refused, and `map_abort` gives the chunk back, with its
    // charge (the path of `memory_map` when a table frame is refused).
    charge::set_limits(
        &poor,
        Some(used + AddressSpace::mapping_chunk_bytes() + 100),
        None,
        None,
    )
    .expect("limit");
    let refused = small.map_object(PAGE_SIZE, attributes, Holder::Memory(object.clone()), |o| {
        object.page(o)
    });
    assert_eq!(refused.err(), Some(MappingError::Quota), "a table frame");
    assert_eq!(small.mapping_count(), 0);
    assert_eq!(small.mapping_bytes_held(), 0, "the chunk went back");
    assert_eq!(charge::counters(&poor).get(Field::Memory).used, used);
    drop(small);
    // The table's frames go back through the reaper.
    yield_until(|| charge::counters(&poor).get(Field::Memory).used == poor_base);

    // Dropping the space gives every record's charge back, and does it in
    // steps (`AddressSpace::drop`).
    // Step points count in this thread as in a process's call.
    crate::steps::set_kernel_steps(true);
    let (_, units) = interval(|| drop(space));
    crate::steps::set_kernel_steps(false);
    assert!(units <= step_limit, "{units} units to drop a space");
    yield_until(|| charge::counters(&job).get(Field::Memory).used == job_base);
}

/// INV-0270-1: the list of frames of a space grows by a copy
/// (`GlobalAlloc::realloc`: allocate, copy, free) that is work in the
/// interval it runs in, counted as `len * 8 / 4096` units, and a step
/// prepared with `AddressSpace::prepare_step` makes it, and the heap's
/// growth for the new list, before its interval. Two equal spaces one step
/// short of the list's doubling: one takes a step of 64 pages as it is, the
/// other after `prepare_step`.
fn a_space_list_growth_is_counted_and_prepared() {
    const BASE: u64 = 0x4000_0000;
    let step_pages = |space: &mut crate::memory::user::AddressSpace| {
        for n in 0..crate::steps::STEP {
            space
                .map_new_page(BASE + (1 << 30) + n as u64 * PAGE_SIZE, USER_DATA)
                .expect("a page");
        }
    };
    // Pages until the list has room for fewer than a step more, at a
    // capacity where the copy is more than a unit.
    let mut pages = 0;
    let mut probe = space_of(0);
    loop {
        let (len, capacity) = probe.list_state();
        if capacity >= 2048 && capacity - len <= 40 {
            break;
        }
        probe
            .map_new_page(BASE + pages as u64 * PAGE_SIZE, USER_DATA)
            .expect("a page");
        pages += 1;
    }
    drop(probe);
    let mut unprepared = space_of(pages);
    let mut prepared = space_of(pages);
    let (len, capacity) = unprepared.list_state();
    assert!(len + crate::steps::STEP > capacity, "the step needs growth");
    assert!(prepared.step_growth() > 0);

    // The step points open and close the intervals (the test thread runs
    // with IRQ masked, which `restore_interrupts` alone would not end).
    let daif = arch::save_and_mask_interrupts();
    let _ = test_step_point();
    crate::steps::masked_max_units(true);
    step_pages(&mut unprepared);
    let _ = test_step_point();
    arch::restore_interrupts(daif);
    let as_it_is = crate::steps::masked_max_units(true);

    prepared.prepare_step();
    let daif = arch::save_and_mask_interrupts();
    let _ = test_step_point();
    crate::steps::masked_max_units(true);
    step_pages(&mut prepared);
    let _ = test_step_point();
    arch::restore_interrupts(daif);
    let ahead = crate::steps::masked_max_units(true);

    // `reserve_step` in the interval, as `memory_map` does under the lock:
    // the copy is counted there too.
    let mut inline = space_of(pages);
    let daif = arch::save_and_mask_interrupts();
    let _ = test_step_point();
    crate::steps::masked_max_units(true);
    inline.reserve_step();
    let _ = test_step_point();
    arch::restore_interrupts(daif);
    let reserve = crate::steps::masked_max_units(true);

    drop((unprepared, prepared, inline));
    crate::reaper::drain();
    let copy = (len * core::mem::size_of::<Frame>() / PAGE_SIZE as usize) as u64;
    assert!(copy >= 3, "the test copies a list of {len} frames");
    assert!(
        ahead <= crate::steps::STEP as u64 + 16,
        "{ahead} units in a prepared step (INV-0270-1)"
    );
    assert!(
        reserve >= copy,
        "{reserve} units in a reserve of the list of {len} frames, the copy is {copy}"
    );
    assert!(
        as_it_is >= ahead + copy,
        "{as_it_is} units in an unprepared step, {ahead} in a prepared one, \
         and the copy of {len} frames is {copy}"
    );
}

/// INV-0270-1: the last part of the search for a run (up to a step of words
/// of the frame map, and the 16 words of a free run of 1024 frames) and the
/// first step of zeroing it are two intervals: a step point between them.
/// With the step points on in this thread (`steps::set_kernel_steps`), a
/// contiguous object of 1024 frames is made and dropped in a masked thread.
fn a_contiguous_object_is_zeroed_apart_from_its_search() {
    use crate::memory::object::MemoryObject;
    let before = frames_accounted();
    crate::steps::set_kernel_steps(true);
    let daif = arch::save_and_mask_interrupts();
    let _ = test_step_point();
    crate::steps::masked_max_units(true);
    let object = MemoryObject::new(1024 * zoraya_pmm::FRAME_SIZE, true, None);
    let _ = test_step_point();
    arch::restore_interrupts(daif);
    crate::steps::set_kernel_steps(false);
    let units = crate::steps::masked_max_units(true);
    if units > crate::steps::STEP as u64 + 16 {
        crate::steps::diagnose_units();
    }
    drop(object.expect("a contiguous object of 4 MiB"));
    assert!(
        units <= crate::steps::STEP as u64 + 16,
        "{units} units in an interval of a contiguous object (INV-0270-1)"
    );
    crate::reaper::drain();
    assert_eq!(frames_accounted(), before);
}

/// `hello`'s ELF file with `extra` bytes more of zeroed memory in its last
/// writable segment.
fn with_bss(file: &[u8], extra: u64) -> Vec<u8> {
    let mut bytes = file.to_vec();
    let word = |b: &[u8], at: usize| u64::from_le_bytes(b[at..at + 8].try_into().expect("8"));
    let phoff = word(&bytes, 32) as usize;
    let phnum = u16::from_le_bytes(bytes[56..58].try_into().expect("2")) as usize;
    let mut last = None;
    for i in 0..phnum {
        let ph = phoff + i * 56;
        let kind = u32::from_le_bytes(bytes[ph..ph + 4].try_into().expect("4"));
        let flags = u32::from_le_bytes(bytes[ph + 4..ph + 8].try_into().expect("4"));
        if kind == 1 && flags & 2 != 0 {
            last = Some(ph);
        }
    }
    let ph = last.expect("a writable segment");
    let memsz = word(&bytes, ph + 40) + extra;
    bytes[ph + 40..ph + 48].copy_from_slice(&memsz.to_le_bytes());
    bytes
}

/// INV-0270-1: `spawn` of a program with a large address space holds no more
/// than a step of work in any interval: the list of frames grows ahead of each
/// step (`prepare_step` in the image, the stack and the startup block), where
/// the doubling was a copy and the heap's growth for the new list in one
/// interval. The list's capacities are not powers of two: the first
/// `prepare_step` reserves `STEP_FRAMES` past the root table, and each growth
/// doubles that, so they are 73 times a power of two. A probe spawn reads the
/// capacity the list has at about 8,192 frames (9,344), and `hello` is then
/// spawned with its bss sized so that the list passes that capacity at each of
/// the last 32 frames it takes, with the heap squeezed to the least free, so
/// that each doubling (a copy of 73 KiB, a new list of 146 KiB) is a growth of
/// the heap. (The stack's and the startup block's `prepare_step` are not
/// reached by this: with `hello`'s 16 stack pages and one page of block, the
/// list never passes its capacity after a step boundary that is there; see
/// ADR-0270.)
///
/// Each spawn leaves the heap larger by what its lists took (about 100
/// frames): the heap keeps the frames it grows by, and the squeeze takes the
/// spans the last spawn's lists left. The test was first written for a list of
/// 16,384 frames, which it never passed (its capacity was 18,688), and its 33
/// spawns of 64 MiB with that growth needed all but about a hundred of the
/// free frames of a 128 MiB guest: it failed with `NoMemory` when the tests
/// before it had left the heap a little larger. So the room is checked before
/// each spawn, and a list that does not pass the probe's capacity fails.
/// Measured through the kernel's own call with the step points on in this
/// thread (`steps::set_kernel_steps`). It runs last: the heap keeps the frames
/// it grew by, scattered, and 4 MiB runs are few in a 128 MiB guest.
fn a_spawn_with_a_big_space_is_bounded() {
    const PROBE: usize = 8192;
    const SWEEP: usize = 32;
    // Frames a spawn takes beside its bss and list: `hello`'s other pages,
    // its stack and block, the tables, and the heap's growth (measured at
    // about 100 frames a spawn; twice that and more).
    const BESIDE: usize = 512;
    let (_, file) = ramdisk::find("hello").expect("hello");
    let before = frames_accounted();
    let spawn_with = |bss: usize| {
        let free = frames::settled_stats().free as usize;
        assert!(
            free > bss + BESIDE,
            "{free} frames free: no room for a spawn of {bss} bss pages"
        );
        // No free span of the heap fits the new list: its growth is the
        // step's, unless it was prepared.
        let mut hog: Vec<alloc::boxed::Box<[u8; 4096]>> = Vec::new();
        hog.try_reserve_exact(heap::stats().free_spans / 4096 + 64)
            .expect("room");
        while heap::stats().free_spans > 32 << 10 {
            hog.push(alloc::boxed::Box::new([0; 4096]));
        }
        let big = with_bss(file, (bss * PAGE_SIZE as usize) as u64);
        crate::steps::set_kernel_steps(true);
        let daif = arch::save_and_mask_interrupts();
        let _ = test_step_point();
        crate::steps::masked_max_units(true);
        let spawned = process::spawn_bytes_suspended("big", &big);
        let _ = test_step_point();
        arch::restore_interrupts(daif);
        crate::steps::set_kernel_steps(false);
        let units = crate::steps::masked_max_units(true);
        if units > crate::steps::STEP as u64 + 16 {
            crate::steps::diagnose_units();
        }
        let process = spawned.unwrap_or_else(|e| {
            panic!(
                "a process with {bss} bss pages: {e:?}, {} frames free, {} held, heap {:?}",
                frames::stats().free,
                crate::reaper::held(),
                heap::stats()
            )
        });
        let list = process.space.lock().as_ref().expect("a space").list_state();
        let _ = process.signal(RequestKind::Terminate, 0);
        let _ = process.resume();
        process.wait();
        drop(process);
        drop(hog);
        crate::reaper::drain();
        (units, list)
    };
    let (_, (len, cap)) = spawn_with(PROBE);
    assert!(len < cap, "a list of {len} frames in a capacity of {cap}");
    // The list's length grows with the bss by one a page, and by a table
    // every 512 pages: a second probe corrects the first's estimate.
    let first = PROBE + cap + 1 - len;
    let (_, (now, _)) = spawn_with(first);
    let first = (first + cap + 1)
        .checked_sub(now)
        .expect("a list near the capacity");
    let mut worst = 0;
    for d in 0..SWEEP {
        let bss = first + d;
        let (units, (now, grown)) = spawn_with(bss);
        println!(
            "a spawn of {bss} bss pages: list of {now} frames in {grown}, {units} units, \
             {} free, {} accounted",
            frames::stats().free,
            frames_accounted()
        );
        assert!(
            now > cap && grown > cap,
            "a list of {now} frames in {grown} did not pass the probe's capacity of {cap}"
        );
        assert!(
            units <= crate::steps::STEP as u64 + 16,
            "{units} units in an interval of a spawn of {bss} pages (INV-0270-1)"
        );
        worst = worst.max(units);
    }
    assert!(worst > 0);
    assert_eq!(frames_accounted(), before);
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

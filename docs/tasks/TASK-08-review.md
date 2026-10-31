# TASK-08 independent review

- **Reviewed:** branch `kernel/stage-2`, commits `main..kernel/stage-2` (4 commits, `bda5757..ea88550`), HEAD `ea885502d1b664c9871043f1dcce83df356b62af`, base `main` at `e5d6f21`.
- **Reviewer:** independent subagent that did not write the code (TASK-08, Process).
- **Date:** 2026-10-31.
- **Environment:** Apple Silicon M4 Mac, macOS (Darwin 25.4.0), HVF available; Rust 1.99.1 (pinned), QEMU 11.1.2. All builds, probes and mutations ran in fresh clones under the scratch directory. The real repository was not modified: `git status` there is clean and HEAD is unchanged.
- **Not available:** a copy of the Arm ARM. Register fields, reset values and sequences below are from my knowledge of DDI 0487, not checked against the text. `rust-src` is not installed, so the behaviour of `Arc::from(Box)` is from my knowledge of `alloc`. The Linux CI workflow was not run. Everything ran under QEMU, so ordering and cache effects of real hardware are reasoned, not observed.

## make test from a fresh clone

```
git clone --branch kernel/stage-2 /Users/svitlana/workspace/os clone && cd clone && make test
```

**Exit status: 0** (wall time 36 s, fresh `target/`).

Summary lines, verbatim (`test result` lines with 0 tests left out):

```
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.01s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.21s
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
adr-check: 22 ADRs checked against INV-0001-1..5
adr-check: warning: 62 invariants of accepted ADRs have no check yet (ADR-0001 rule 6):
adr-check: 0 errors, 62 warnings
[auto-default] PASS: 62 passed, 8 expected failures
[auto-smp4-2g] PASS: 62 passed, 8 expected failures
[auto-el2] PASS: 62 passed, 8 expected failures
[tcg-default] PASS: 62 passed, 8 expected failures
[tcg-smp4-gicv2] PASS: 62 passed, 8 expected failures
[tcg-el2] PASS: 62 passed, 8 expected failures
[tcg-smp4-gicv3] PASS: 62 passed, 8 expected failures
[boot-auto] PASS: 0 passed, 0 expected failures
[boot-tcg] PASS: 0 passed, 0 expected failures
make test: all checks passed
```

The only `warning` lines in the log are adr-check's. A second fresh clone with `CARGO_NET_OFFLINE=true make test` also exited 0. I could not disconnect the network; `Cargo.lock` has no `source =` line, so nothing can come from a registry.

**Flakiness.** `make test-qemu` ran 10 more times from the same clone: 3 idle (12 s each), 4 with 20 spinning processes (28 to 30 s), 3 with 40 spinning processes (51 to 54 s). All 10 exited 0, with all 7 configurations passing. Nothing flaked. This covers HVF and TCG on this Mac only.

**Configurations outside the Makefile,** with the 8 expected panics and no line checks:

| Configuration | Result |
|---|---|
| TCG, EL2, GICv3 | PASS 62 + 8 |
| HVF, EL2, 4 CPUs | PASS 62 + 8 |
| HVF, 8 GiB | PASS 62 + 8 |
| HVF, 32 MiB | FAIL: `running_out_of_asids_is_an_error` reaches 76 live processes, not 255 (see R-22). Later failures cascade from that test's leaked processes. The kernel itself returned `NoMemory` cleanly |

## Findings

| ID | Area | Finding | Severity | Suggested fix |
|---|---|---|---|---|
| R-01 | `Process::wait` | `process.rs:76-82` calls `arch::restore_interrupts(daif)` and then `return record` while the guard `exit` is still alive. The guard's drop runs after the restore and re-applies its own saved `DAIF`, which was the masked value, so **`wait` always returns to its caller with IRQ and FIQ masked**. Ran it: a probe in the test kernel prints `arch::interrupt_mask()` straight after `wait()`, and every one of about 60 calls shows `daif=0x2c0` (I and F set), under HVF and TCG. The boot thread then stays masked until something resets it. This breaks ADR-0030 ("Kernel threads run with IRQ unmasked"), and a kernel thread that polls for a timer effect after `wait` never sees it. No test reads the mask after `wait`, and other threads carry their own mask, so nothing shows it today. Between the restore and the drop, IRQ is also unmasked with the lock held, which is the "lock taken twice" panic if a handler ever takes that lock. | error | Copy the record out in a block so the guard drops first, then restore. Add an assertion on `interrupt_mask()` after `wait` in `user_process_prints_and_exits`. |
| R-02 | `Process::wait` | `Exit.waiter` holds one thread, and `process.rs:84` overwrites it. With two waiters the first is never woken: a lost wake-up. Ran it: two kernel threads wait on one `spin` process; the output is `PROBE waiter 2 got ExitRecord {...}` then `PROBE two waiters done=1`, on HVF and TCG. Worse, the overwrite drops the `Arc` that was the blocked thread's only reference. The thread, with its stack, is then freed while `BLOCKED`, `SCHED.threads` stays too high, and everything its stack owned leaks. So the SAFETY comment at `sched.rs:252-254`, "`prev` by ... whoever will wake it", and the doc of `block_current` do not hold. No caller reaches this in stage 2 outside my probe, but stage 3 process handles will. | error | Keep a list of waiters with fallible growth, or refuse a second waiter with an error, and add a test with two waiters. |
| R-03 | Slice and deadline queue | `sched.rs:243`: `s.slice = timer::arm(end, end_of_slice, 0).ok()`. If the 32-entry queue is full, the slice is silently not armed and that thread runs until it blocks or exits. Ran it: 31 `p_sleepfar` sleepers plus one far kernel deadline, then two CPU-bound processes that never yield: `forever start 4129`, `forever end 4631`, `forever start 4631`, `forever end 5159`, so they ran strictly one after the other, and the boot thread stayed ready for the whole 1 s. That breaks INV-0030-3. By my reasoning user programs alone cannot reach it, because `schedule` cancels the old slice before arming, and a full queue with no slice needs the last slot taken while nothing else is ready. It takes a kernel-armed deadline, which stage 3 will have. `docs/design.md` Known limitations mentions only that `sleep_until` fails. | imprecision | Reserve one queue slot for the slice, or keep the slice outside the queue. At least count the failure and record it. |
| R-04 | Mask state of new threads | `schedule` masks only I and F, so a new thread inherits D and A from whoever switched to it. (a) `kernel_thread_main` (`sched.rs:167`) clears only I and F. First run after a syscall gives `PROBE kthread daif=0x300`: SError is masked on that kernel thread for good, against ADR-0024 ("Unmasked at run time: SError"). (b) The opposite for a new user thread reached from a kernel thread: `__exception_return` runs with `A` unmasked, so an SError between `msr elr_el1` and `eret` (`exception.rs:208-233`) would clobber both, after `SP_EL0` already holds the user value that the EL1 vectors read as the limit. (c) Likewise `SP_EL0` and SP disagree for a few instructions in `__context_switch` with `A` open, while `switch`'s contract says "every exception is masked or cannot reschedule". SError is always fatal, so the worst outcome is a wrong report. | imprecision | `kernel_thread_main`: set `DAIF` to a stated value (`daifclr #7`). Start of `__exception_return`: `msr daifset, #0xf`. Have `schedule` use `save_and_mask_all`, or state the SError case. |
| R-05 | EL0 access to timer and PMU | The kernel never writes `CNTKCTL_EL1` or `PMUSERENR_EL0` (grep finds neither outside a comment), so EL0's access depends on values I recall as UNKNOWN at reset. `boot.rs` says EL1 and EL0 own the PMU counters. Under QEMU: `cntvct`, `cntpct`, `cntv_ctl`, `cntv_cval` from EL0 all trap, EC 0x18, on both accelerators, so a process cannot touch the kernel's timer today, only because the reset value is 0. PMU differs: under TCG, `pmccntr_el0`, `pmcr_el0` and `pmcntenset_el0` trap; **under HVF they do not**: `p_pmccntr` read a live counter and `p_pmcntset` wrote without a fault, with `pmuserenr_el0` reading 0. I infer QEMU's HVF register emulation does not check EL0, so the kernel may not be able to fix that case, but the guest should still state its policy. | imprecision | Set `CNTKCTL_EL1 = 0` and `PMUSERENR_EL0 = 0` in the boot path, and add the probes as `f_*` programs. Record the HVF behaviour in Known limitations. |
| R-06 | Interrupt latency | `syscall.rs:14` allows 64 KiB per `console_write`, and syscalls run with everything masked. Measured for one call: 690 ms and 1,492 ms under HVF, 727 ms under TCG. So one user process can hold off every interrupt and deadline for over a second, repeatedly, against a 10 ms slice. At a real 115200 baud it would be about 5.7 s. ADR-0030 admits "a long kernel path delays everyone" but no bound is stated anywhere. | imprecision | Lower `MAX_WRITE` (for example 4 KiB, returning the short count, which the ABI allows), or record the bound in `design.md`. |
| R-07 | Test: preemption | `spin.rs` and `regs.rs` call `clock_monotonic` in their loops, so they reach `preempt_point` through a syscall. Mutant `m26`, removing `preempt_point` from the IRQ path (`exception.rs:436`), passes all of `make test-qemu`. **So nothing tests that a program which never enters the kernel gets preempted**, which is what TASK-08's "two CPU-bound user processes" and INV-0030-3 are about. The code itself works: my `p_preempt` probe (3,000,000-iteration loops with no syscall) was preempted and shared the CPU. Also `PREEMPTIONS` counts a call that switches nothing, despite its doc comment "switched out", so `preempted >= 2` is weaker than it reads. | imprecision | Make `spin` loop without syscalls for most of its run, for example count iterations and exit by a shared bound, and count only real switches. |
| R-08 | Test: register survival | `regs.rs` checks one x register, d31 and `TPIDR_EL0`. Passing mutants: `m04` x23 not restored on return; `m06` q16 and q17 not restored. No test checks x2 to x30 across a syscall, the upper halves of q, or `FPCR`/`FPSR` at EL0. The code is right: my `p_regsave` (x2 to x30 across calls 1, 2, 3 and an unknown one, 2,000 rounds each) and `p_preempt` (all x, all 128-bit q, NZCV, `FPCR`, `FPSR`, TLS across preemption) passed on HVF and TCG. | imprecision | Adopt those two programs. |
| R-09 | Test: new process state | Mutant `m21`, a new thread's `TPIDR_EL0` set to `0xdead`, passes. Nothing checks registers or TLS at process start, though a stale value there is a leak between processes. The code is right (see "Checked"). | imprecision | Add a program that snapshots registers in `_start`. |
| R-10 | Test: stack limit from EL0 | Mutant `m24`, `__exception_save_el0` no longer loads the limit into `SP_EL0`, passes all of `make test-qemu`. So ADR-0031's Stack limit rule, which settles ADR-0024's open question, has no test: `fault_report_for_thread_stack_overflow` runs on a kernel thread that never came from EL0. The arithmetic is right by reading it. | imprecision | Add an expected-failure test: a test-only syscall path or a user-thread entry that overflows its kernel stack. |
| R-11 | INV-0030-1 status | Marked `checked: make test-qemu`, but its only test is stage 1's check on an empty queue (`testing.rs:834`), and it involves no scheduler. Mutant `m12`, arming a slice for every user thread even with nothing waiting, is exactly a break of the invariant and passes. | imprecision | Test `is_enabled()` while a lone user process runs, or mark it not checked. |
| R-12 | INV-0002-1 status: is the lint honest? | Only partly. (a) `parameters_come_from_the_allow_list` can fail only for `EntryPoint`: every other `Param` is accepted by the match. Its value is that a new kind forces a compile-time decision, which is real but is not a test of the calls. (b) `SYSCALLS` is a hand-written table, unconnected to `syscall.rs::dispatch`. The kernel could read a fourth register or take a raw integer object name, and nothing would notice. (c) The invariant's second half, "a unit test that `RelativePath::new("/x")` fails", does not exist: `RelativePath` appears nowhere in `crates`, `kernel` or `user`. (d) `Param::ExitCode` is not on the allow-list ADR-0002 states (`Handle`, `RelativePath`, `Deadline`, flags, sizes, buffers, `detail: u32`), and the invariant says the only integer payloads are `detail` fields. `process_exit(code)` follows ADR-0004 rule 1, so the two accepted ADRs need reconciling, but the extension is silent. CLAUDE.md requires an ADR for that. | imprecision | Status "partly checked; `RelativePath` not yet applicable (stage 4)", say so in the ADR's open questions (rule 6), and name `ExitCode` in ADR-0032 as a proposed reading of INV-0002-1. |
| R-13 | INV-0031-4, INV-0032-2, INV-0024-2 | INV-0031-4's check also promises "user-range casts"; `check-user-access` greps only `ldtr` and `sttr`, so a plain `read_volatile(addr as *const u8)` in `syscall.rs` passes. INV-0032-2 says "every call", but `fuzz.rs` skips `sleep_until` and `process_exit`. My `p_sleepedge` and `p_sleepfar` found no fault with 0, 1, now and `u64::MAX - 1`. INV-0024-2 says the normal kernel has no path that resumes after a kernel fault, and `usercopy::fixup` is now such a path. ADR-0024's text plans the exception, but the invariant and its `checked` status do not say so. | imprecision | State each limit in the check text, fuzz `sleep_until` with near deadlines, and have the architect amend INV-0024-2. |
| R-14 | ADR-0022 | `sched.rs:98-102` and `process.rs:172` use `Arc::from(Box)`, which allocates a new block and copies, so it cannot fail softly: out of memory there panics, and the box is wasted. `arc` returns `Result`, and its comment "a box first would only move the failure" admits this while the signature claims otherwise. Today only the kernel spawns, so ADR-0022's bounded-call rule may still cover it. Stage 3's `spawn` system call would not be covered. | imprecision | A small own `try_arc`, or an intrusive count, before stage 3. Say in the comment that this is infallible. |
| R-15 | Evidence (ADR-0001 rule 5) | ADR-0031 Option A, the Linux baseline, says Linux uses "ordinary loads", and Option B says Linux used `ldtr` forms "for a period". Fetched upstream today: `arch/arm64/include/asm/uaccess.h` passes `"ldtr"`/`"sttr"` to `__raw_get_mem`/`__raw_put_mem`, and `lib/copy_from_user.S` loads through `user_ldst ... ldtrb`. So Linux does what Option B proposes, now. The option is steelmanned against something Linux no longer is, and the Recommendation's rationale should be restated. ADR-0031 Option C cites `#l4-family-and-sel4` for registers and a mapped IPC buffer, and `#fuchsia--zircon-incl-starnix` for checked user-pointer types; ADR-0032 Option C cites `#l4-family-and-sel4` for capability invocations. None of these is in those sections, by grep and reading, and these three have no `[outside evidence]` tag. ADR-0030 Option B's fixed priorities are not there either, but that one has a tag. Some evidence bullets are untagged reasoning, for example ADR-0030 B "Simple and predictable". The `adr-check` citation check passes because the sections exist. | imprecision | Correct the Linux description, re-tag the three claims as `[outside evidence: ...]` with a source. |
| R-16 | ADR-0033 | The Recommendation says frames are filled, "then pages are mapped". `load_segment` (`process.rs:121-140`) maps first, then fills. Harmless, since the process is not running, but the ADR is not what the code does. | imprecision | Reword the ADR, it is still `proposed`. |
| R-17 | Safe functions with unsafe contracts | `mmu::set_user_table` (`mmu.rs:131`) takes any `u64` into `TTBR0_EL1`; `sync_instruction_cache` (`mmu.rs:185`) runs `dc cvau` on any address, and its SAFETY says "mapped memory", which nothing enforces. Both are safe `pub fn`. TASK-05's R-08 made `unmask_interrupts` unsafe for the same reason. | imprecision | Make them `unsafe fn` with `# Safety` sections. |
| R-18 | SAFETY wording | `sched.rs:42`: `context` touched "for a thread that is not running", but `__context_switch` writes the running thread's context. `sched.rs:252`: does not name `s.idle`, the owner when `prev` is idle, or the leaked count of an `Infinite` sleep, and see R-02. `testing.rs:390`: "boot stack, or the emergency stack" no longer lists a kernel thread's stack, where a panic in a thread runs. Conclusions hold in all three, apart from R-02. | imprecision | Reword. |
| R-19 | Stale and wrong comments | `main.rs:1` "stage 1". `memory/mod.rs:25` "Nothing is mapped here" over the stack window, which now holds every stack. `badptr.rs`, "reached part way through": the range `0x3f_f000` starts unmapped, so the first byte faults, and no test faults part way through a copy. | imprecision | Correct, and start the range in the last mapped page of a segment. |
| R-20 | ABI | `svc #7` is accepted as a system call (`p_svcimm` exited normally). ADR-0032 says `svc #0`, and its driver 4 says unknown things fail. `Syscall` dispatch reads only x8, and the `ESR` immediate is unused. | note | Return `BadSyscall` for a non-zero immediate while no caller depends on it. |
| R-21 | Lock order | `design.md:203` states heap, tables, frames only. ADR-0030 driver 3 says scope is written down. New edges found by reading: `exit` then `SCHED`; `SCHED` then `QUEUE`; `SCHED` then heap (`enqueue_new`), which can reach tables and frames with everything masked; `SLOTS` then heap. No cycle found. | note | Add them. |
| R-22 | Test sizing | `running_out_of_asids_is_an_error` needs more than 32 MiB (76 live processes fit there), so a small `--mem` fails it, where TASK-05's suite passed at 32 MiB. | note | State the minimum, or end the test at the first `NoMemory`, freeing what it holds. |
| R-23 | PAN | PAN is never set where available. A stray kernel pointer into user memory succeeds silently, while `-cpu host` has the feature. ADR-0031's choice of `ldtrb` needs no PAN, and PAN would be an extra net. | note | Consider it when stage 3 adds kernel objects. |
| R-24 | `fixup` scope | `usercopy::fixup` checks only the PC, so a fault on the kernel `strb` destination would also read as `BadAddress`. Not reachable: the destination is a live `Vec`. | note | Compare `FAR` with the user range if the routine grows. |
| R-25 | ELF | `PF_R` is ignored: a segment with no read flag is mapped readable. Harmless at stage 2. | note | Refuse it, or say so in ADR-0033. |
| R-26 | `EPD0` caching | TASK-05 found `EPD0` treated as possibly cached in a TLB. `set_user_table` changes it with no invalidation. It should be safe: `TTBR0 = 0` selects ASID 0, which holds no entries. I cannot confirm the architectural wording. | note | Check when the Arm ARM is at hand. |
| R-27 | Fault coverage | INV-0034-1 says "a fault program for each class"; there are four classes. My probes also hit EC 0x01, 0x18, 0x1d, 0x22 and TCG's EC 0x00 for SVE, all ending in `Fault` correctly. `class_name` has no name for 0x1d (printed only for kernel faults). | note | Add `wfi` and an unaligned PC to `f_*`. |

## Checked and found correct

**make test and repository rules**
- `make test` passes from a fresh clone, also with `CARGO_NET_OFFLINE=true`, and nothing flaked (above).
- `Cargo.lock` has no `source =` lines. `dependencies.md` adds no crate.
- No change on the branch to `docs/research/`, `tools/run-qemu.sh` or `tools/posix-surface/`. In `design.md` the removed lines are only as-built text; the Vision section and the roadmap table are unchanged.
- `allow(` appears nowhere in `kernel`, `crates`, `user` or `tools/mkramdisk`.
- ADRs were committed first (`bda5757`), status `proposed`, "Architect's decision" empty. All four commits use the configured identity.

**EL0 boundary: no way found to crash, hang or corrupt the kernel, reach its memory, or read another process**
- About 40 adversarial programs ran under HVF and TCG; the kernel survived all of them and every test after them passed.
- Ends in `Fault`: read, write and execute of the kernel image and direct map (execute gives EC 0x20, so UXN holds); write to own text; execute own rodata; misaligned PC; `msr daifset`; `dc zva`, `dc cvau`, `dc civac`, `dc ivac`, `ic ivau`; `ctr_el0`; `wfi`; `at s1e1r`; `hvc`, `smc`, `eret`; `sp_el0`, `mdscr_el1`, `dbgdtr_el0`; write of `tpidrro_el0` and `cntfrq_el0`.
- `wfe` traps under TCG. Under HVF it returned without a fault, which I read as an event already pending; it cannot hang the kernel, since the timer interrupt ends it.
- `SCTLR_EL1 = 0x30d0_180d`: `UCI`, `UCT`, `DZE`, `nTWI`, `nTWE`, `UMA`, `SA0` clear. So that is why caches, `dc zva` and `wfi` trap, and a process that wants to read `ctr_el0` is killed, which is acceptable. `CPACR_EL1` enables FP only; SVE and SME trap to a `Fault`.
- SP set to a kernel address or to an odd value, then `svc`: the kernel uses `SP_EL1` only, and the process continued normally.
- The vector, return path and `SP_EL0` swap are right: limit = SP + 2 frames - stack size = bottom + frame, equal to `KernelStack::limit`, and the immediates encode. `SP_EL1` is at the stack top at every EL0 entry. `tst x2, #0xf` selects EL0t. `SPSR` comes only from hardware or the all-zero first frame.
- **No leak on return:** every register comes from the frame, so x2 to x30 hold the user's values (`p_regsave`, 2,000 rounds of 4 calls), and all q, `FPCR`, `FPSR`, NZCV and TLS survive preemption (`p_preempt`).
- **New process state:** after a process that dirtied its stack, `TPIDR_EL0`, `FPCR`, d0, d8 and d31, a new process's snapshot at `_start` showed x0 to x30, all q, `TPIDR_EL0`, `FPCR`, `FPSR`, NZCV and `TPIDRRO_EL0` all zero, and every stack word below SP zero, 4 rounds on each accelerator.
- User frames come from `alloc_zeroed`, tables too. `m01`, not zeroed, is caught by `isolate`. Kernel stack frames are not zeroed, but they are never mapped to EL0, and the first frame is written whole.
- `UserSlice`: `checked_add`, `end <= 2^47`; `ldtrb` for every byte; fault resumes at the error exit; `len` bounded before allocation; `try_reserve_exact`; `resize` cannot then allocate. `x0` to `x3` and `x30` are right at the exit. Deadlines of 0, 1, now and `u64::MAX - 1` are safe, since `ticks_at` uses `u128` and saturates. A user cannot make `sleep_until` panic.
- All of `f_*` and `badptr` behave as asserted.

**Scheduler (`sched.rs`, `context.rs`)**
- `Context` offsets match the assembly, and the `const` asserts cover them. Callee-saved set, `FPCR`, `FPSR`, TLS and limit are saved and restored. Only d8 to d15 are needed: `switch` is an `extern "C"` call.
- Sleep: interrupts stay masked from `current()` to `block_current`, so a wake cannot find a thread not yet `BLOCKED`, even for a past deadline. The raw count keeps a sleeper alive and `wake_sleeper` consumes it once. A sleeping thread cannot exit, so no timer callback outlives its thread.
- No `Arc` is held across `schedule`: every caller's local is dropped first. `process::exit_current` and `fault_current` drop theirs before `sched::exit_current`. The `if` condition temporary in `exception_dispatch` ends before the block.
- `reap` runs only on another thread, after `prev` has switched away, and `dead` and `ready` keep reserved capacity, so `schedule` does not allocate. `m20` is caught.
- A thread's `daif` is a local on its own stack, so it is restored on the right thread, R-04 being about new threads only. Exception-path entries have all of `DAIF` set for the whole switch.
- Idle masks around its check; `wfi` wakes on a masked interrupt. Idle is never queued. No lock guard spans a switch. `TimerId`s are not reused, so a stale `slice` id cancels nothing. Round robin logic holds on each path: lone thread, wake while alone, kernel thread next.
- `unsafe impl Send/Sync for Thread`: sound on one CPU, wording apart (R-18).

**MMU**
- `nG`, `PXN` and `AP[1]` set on every user page; `UXN` follows the segment. `m13` and `m14` are caught by host tests, and `m14` also by `processes_cannot_see_each_other`.
- ASID: 8 bits with `TCR.AS = 0`, 1 to 255, `aside1is` before frames are freed and before the ASID is freed, with TTBR0 off first. `m02`, dropping the invalidation, is caught: a reused ASID really does see stale entries there, and `user_tables_obey_wx` fails. `m19` is caught.
- TTBR0 and its ASID change in one write, so user to user needs no flush. `m17` is caught.
- I-cache: `dc cvau` over the page through the direct map, `dsb ish`, `ic ialluis`, `dsb ish`, `isb`, after the copy. `m16` is caught.
- Kernel stacks and user tables: only invalid to valid when mapping, with `unmap_page` invalidating before the frame is freed, so no break-before-make case arises. A reused slot is a fresh invalid-to-valid mapping.
- W^X: refused by the ELF parser (`m15` is caught), `USER_STACK` is RW and XN, WXN stays set. `p_textw` and `f_exdata` confirm it in hardware terms.
- `Half::Lower` is used with `USER_LOW` and `USER_HIGH` bounds, and a stack guard page is kept by the parser's `high`.

**`unsafe` and `SAFETY`**
- All 53 added lines mentioning `unsafe` or `SAFETY` in the kernel were read. Each stated invariant holds, apart from R-02, R-04, R-17 and R-18. Clippy enforces the comments; `unsafe fn`s have `# Safety`. `zoraya-abi`, `zoraya-elf` and `zoraya-ramdisk` have `forbid(unsafe_code)`.

**ADR conformance**
- ADR-0030 to 0034 Recommendations match the code, apart from R-01, R-04 and R-16. That covers stacks, slice, idle, reaping, layout, ASIDs, stack limit, `ldtrb`, register convention, five calls, statuses, UTF-8, embedded ramdisk, total parsers, `Fault` records with EC in `detail`, the one-line log, SError from EL0 as a kernel fault.
- ADR-0033's measured claim is true: `-initrd` with the ELF kernel gives a device tree with no `linux,initrd` property.
- Accepted ADRs: ADR-0002 handles are `u32`, 0 invalid. ADR-0004 `ExitRecord` and `ExitReason` match rule 1, and rule 5's no-channel case. ADR-0006: `sleep_until` takes a `Deadline`, no relative timeout. ADR-0020 layout: stacks in the window. ADR-0022: no infallible allocation on any system call path. ADR-0023: no periodic tick; the slice is one-shot. ADR-0024: panic path unchanged, and the normal ELF contains no `recover_to` or `__catch` strings.
- Each of ADR-0030 to 0034 has three options, a Linux baseline, a non-Unix precedent, and an empty decision section. The Hubris, DM-067, DM-068, CP-18, CP-20 citations say what is claimed. Exceptions are in R-15.
- Other `invariants.md` statuses match their tests, apart from R-07, R-10, R-11, R-12 and R-13.

**Spec coverage and mutation results**
- Every TASK-08 test exists and runs under all seven configurations. The normal kernel prints `hello from EL0`, `code: 7, reason: Exited` and `stage 2 complete` (`--expect-line` in `test-boot`).
- Parsers: read by hand as total; the host truncation and mutation tests pass. Overlap, page sharing, bad entry and ranges are refused.
- Caught by `make test-qemu`: `m01`, `m02`, `m03` (x9), `m05`, `m07`, `m08`, `m09`, `m11`, `m14`, `m15`, `m16`, `m17`, `m18`, `m19`, `m20`, `m22`, `m23`, `m25`, `m27`, `m30`, `m31`, `m32`.
- Caught by host tests only: `m13` (no PXN on user pages), so the in-QEMU half of INV-0031-1 reads only `UXN`.
- Not caught: `m04`, `m06` (R-08); `m12` (R-11); `m21` (R-09); `m24` (R-10); `m26` (R-07); `m10`, `ldrb` in place of `ldtrb` with the range check kept, which is expected of defence in depth. With both gone, `m09` is caught.
- `m29`, removing the guard offset, was not a useful mutant: it broke slot arithmetic and failed for that reason.

## Resolution

The implementing agent applied these after the review, in the commit that adds this file. The review text above is unchanged.

| ID | Resolution |
|---|---|
| R-01 | Fixed. `wait` releases the lock in a block of its own before restoring the mask. New test `wait_keeps_the_interrupt_mask`. |
| R-02 | Fixed. `Exit` holds a list of waiters, grown fallibly; with no room, a waiter polls by yielding. New test `every_waiter_is_woken`, with three waiters plus the boot thread. |
| R-03 | Fixed. One queue slot is reserved for the slice (`timer::arm_reserved`). New test `full_deadline_queue_keeps_the_slice`. |
| R-04 | Fixed. `schedule` masks everything, a new kernel thread clears A, I and F, and `__exception_return` masks everything first. |
| R-05 | Fixed: `CNTKCTL_EL1` and `PMUSERENR_EL0` are written as 0 in `exception::init`. EL0's PMU access under HVF is recorded in Known limitations; it was not re-probed. |
| R-06 | Fixed: `MAX_WRITE` is 4 KiB, and the bound is in `syscall.rs` and Known limitations. The 60 ms figure is scaled from the review's measurement, not measured. |
| R-07 | Fixed. `spin` makes no system call while it runs, and `PREEMPTIONS` counts only real switches. The mutant result is in the note below. |
| R-08 | Fixed. New `regsave` checks x2 to x30 across system calls. `regs` checks all 128-bit q registers, x19 to x28, FPCR and TLS across preemption. |
| R-09 | Fixed. The runtime's `_start` faults if any x or q register, `TPIDR_EL0` or `FPCR` is not zero, in every program. |
| R-10 | Fixed. New expected-failure test `fault_report_for_stack_overflow_from_el0`, through a system call that exists only in the test kernel. |
| R-11 | Fixed. New test `lone_process_has_no_timer` reads the timer from a lone user process. |
| R-12 | Status corrected: INV-0002-1 is back to `not yet applicable (stage 4)`. ADR-0002's open questions and ADR-0032 state what the lint does not cover, and that `process_exit`'s `code` needs the two ADRs reconciled. That is for the architect. The table is still not connected to the dispatcher. |
| R-13 | INV-0031-4 and INV-0032-2 now state their limits, and `fuzz` calls `sleep_until` with near deadlines. INV-0024-2 is accepted, so its exception is recorded in ADR-0024's open questions for the architect. |
| R-14 | Partly. The function no longer pretends to be fallible, and its comment and Known limitations say stage 3's `spawn` needs a fallible form first. |
| R-15 | Fixed. ADR-0031 makes Option B the Linux baseline, and the citations that the sections do not support are now tagged as outside evidence or removed. The other untagged reasoning named in the finding was not changed. |
| R-16 | Fixed in ADR-0033. |
| R-17 | Fixed: both are `unsafe fn` with `# Safety` sections. |
| R-18 | Fixed. |
| R-19 | Fixed, and `badptr` now has a copy that faults part way through. |
| R-20 | Fixed: a non-zero `svc` immediate returns `BadSyscall`. No test covers it. |
| R-21 | Fixed in `design.md`. |
| R-22 | Recorded in Known limitations. |
| R-23 | Recorded in Known limitations. Not done. |
| R-24 | Not changed: not reachable, as the finding says. Revisit when the routine grows. |
| R-25 | Fixed: an unreadable segment is refused, with a host test, and ADR-0033 says so. |
| R-26 | Open: needs the Arm ARM. |
| R-27 | Fixed: `f_wfi` and `f_align` added. |

# TASK-29 audit: in-QEMU tests of the kernel and of the user programs

Read-only audit, 2026-11-14, tree at `5e9bbabf`. Nothing was built or run. One CI log was fetched
(`gh run view 43437795275 --attempt 1 --log-failed`, saved as `build/audit/run-43437795275-failed.log`).

## How a failure reaches the runner (traced end to end)

- User side: a failing case in a `t_*` program panics (`assert!`, `assert_err!`, `expect`). The panic handler
  (`crates/zoraya-rt/src/lib.rs:899`) prints `user panic: ...` and calls `process_exit(101)`. A program can also
  return a non-zero code from `main`.
- Kernel side: each kernel test spawns the program and asserts the exit record, e.g. `exited_in("t_handles")`
  (`kernel/src/testing.rs:1805-1816`) is `assert_eq!(run_program(name), exited(0))`. A mismatch panics in the test.
- Harness: `on_panic` (`testing.rs:625`) prints `ZORAYA-TEST fail NAME panic ...` and resumes with the next test.
- Runner: `Checker::finish` (`tools/test-runner/src/protocol.rs:222-310`) fails the run for every `fail` line whose
  name is not under `--expect-panic`, for a count mismatch between `begin N`, the result lines and `end pass= fail=`,
  for a missing `--expect-line`, a timeout, no power-off within 5 s of the end line, a non-zero QEMU status, or a
  `ZORAYA-PANIC` line.
- The printed `t_X: all passed` lines are not needed for the verdict (the exit code is); they matter only where the
  Makefile expects them (`t_gui`, `t_net`, `t_netd`).

## Confirmed findings

### K1. `tests/shell/console.script:212-217`: the burst check races the shell, and does not test what it says (the `test-boot` flake)

- Claims: "A burst larger than the ring, during a job, drops bytes and counts them."
- Script: `> spin\r`, `= 8192`, `< bytes dropped]`, then `@ \x03 | spin: interrupted`. The runner sends `spin\r` and
  the 8192 bytes back to back with no wait (`script.rs:556`, `send`), and a `<` step waits until the whole run's
  deadline (`QEMU_TIMEOUT`, 180 s; there is no per-step limit).
- The shell queues the notice for the next prompt (`user/src/bin/zosh.rs:2535-2541`, `jobs.queue`); it is shown only
  before a prompt (`zosh.rs:5013`), above the line being edited (`zosh.rs:5042-5048`), or by `wait`/`fg`.
  `wait_foreground` (`zosh.rs:2551-2625`) never shows it. So since `b99e4ba2` (job control) the notice can appear
  before the Ctrl-C only if the ring overflowed while the shell was still at the prompt, before it read `spin\r`.
- Evidence, run 43437795275 attempt 1: the six passing legs print `/> ^[[J[input: 7941 bytes dropped]` and only
  then `/> spin` (7941 = 8192 - 251: the ring of 256 held `spin\r` and 251 `x`), so the drop happened at the prompt,
  not during a job. The failing leg `boot-tcg-smp4-gicv3` prints `/> spin`, then 320 echoed `x`, no notice, and
  sits until `timed out waiting for "bytes dropped]"` 180 s later: the shell read `spin\r` first, `spin` came to the
  front, the notice was queued for a prompt that the script never asks for.
- The same message is the "known input flake" of `docs/tasks/TASK-26-report.md:307` (`boot-tcg-smp4-gicv2`) and
  `:805` (`boot-auto-smp4-2g`), and `docs/backlog.md:165`. The backlog's explanation ("the shell echoing slowly")
  is not the cause.
- Class: order assumption between the runner's two sends and the shell's read (runner-side race). Not a kernel
  defect. Why the smp 4 legs lose the race more often was not established.
- Smallest fix (script only): put the burst behind a job that announces itself, and interrupt before expecting the
  notice:
  `> fanout 8\r` / `< fanout: 8 children spinning` / `= 8192` / `@ \x03 | fanout: interrupted` / `< bytes dropped]` /
  `< \r\n/> `. The later `x bytes dropped` step stays. To verify on CI: that bytes are always dropped on this path
  (the failing leg echoed 320 of 8192, so they were there), and that several queued notices are acceptable.

### K2. Nothing on the host checks which tests ran, or how many

- `protocol.rs:239-247`: the count is compared only with `begin N`, which the kernel prints from `TESTS.len()`
  (`testing.rs:596`). `tools/test-summary.sh:41` counts configurations, not tests. A test removed from `TESTS`
  together with its function, or a subtest call removed from a `t_*` `main`, disappears silently.
- What limits it: `make clippy` builds the kernel with `--features qemu-test -D warnings` (`Makefile:1317`) and the
  user programs, so a function left out of the list is dead code and fails `make check`; the nine `--expect-panic`
  names must run (`protocol.rs:283-290`); `harness_resumes_after_panic` pins the nine failures (`testing.rs:3059`).
- Fix (runner and Makefile only): `--expect-count N` or a committed list of names compared with the `run` lines.

### K3. `--expect-panic` accepts any panic; three of the nine can panic for the wrong reason

- `protocol.rs:271-273` checks only that the message starts with `panic `.
- `fault_report_for_unexpected_interrupt` (`testing.rs:1177-1181`) ends in `wait_until(|| false)`: with no interrupt
  it panics after 10 s with "no interrupt in time". Saved by the specific line
  `ZORAYA-FAULT interrupt 40 has no handler`.
- `fault_report_for_thread_stack_overflow` (`:2139-2146`) ends in `wait_until_yielding(|| false)` ("no progress"), and
  `fault_report_for_stack_overflow_from_el0` (`:2135`) panics in `run_program` if `koverflow` is missing. The line
  `ZORAYA-FAULT kernel stack overflow from EL1` is shared by all three overflow tests, so it does not tell them
  apart; `every_overflow_reached_the_limit_check` (`:3026`, the counter is 3) does.
- `harness_reports_exception_as_failure` (`:740`) has no line of its own; `ZORAYA-FAULT synchronous from EL1`,
  `SPSR_EL1 0x`, `x30 0x` and `  sp  0x` are printed by every report (`exception.rs:366-397`).
- Specific and sound: `ESR_EL1=0xf2000321` (only `fault_report_for_brk`), `FAR_EL1 0xffffffff7fffe000`,
  `no syndrome: ...`, `process f_kread: fault, EC 0x24`.
- Net: no test passes vacuously today, but only through two other tests. Fix (runner and Makefile):
  `--expect-panic NAME=TEXT`, matching the fail message.

### K4. Device-gated tests that pass having done nothing

- `t_gui` (`user/src/bin/t_gui.rs:816-819`) returns 0 without a GPU. Only `auto-default` and `tcg-default` pass
  `--gui`, and both expect `t_gui: all passed` (`Makefile:239`, `:258`). The other five configurations count
  `gui_from_a_process` as passed with nothing done.
- `t_net` (`t_net.rs:222-225`) and `t_netd` (`t_netd.rs:211-214`) return 0 without a network device. None of the
  seven `QEMU_CONFIGS` passes `--net`, so both tests are empty in all seven. They are real only in `make test-net`
  (`Makefile:318-320`, expected lines and the pcap checks); its TCG leg runs only with `FULL=1` (`Makefile:340`).
- `t_snapshot`'s `devices_refuse_a_snapshot` (`t_snapshot.rs:917-922`) prints "INV-0102-5 not exercised" and returns
  if slot `DEVICE` holds no device. All seven configurations attach the disk, so it runs today, but nothing would
  notice if it stopped. Fix: print a positive line when it ran and add it to `EXPECT`, or panic when the device is
  missing (test code only; check first that no run of the test kernel lacks `--disk`).

### K5. `t_quota`: two fixed sleeps order the parent's limit before the child's map

- `user/src/bin/t_quota.rs:436` (parent: `nap(150)`, read the use, narrow the limit) and `:449` (child: `nap(400)`,
  then `memory_map` must be refused).
- Fails without a defect if the parent runs more than about 250 ms late: the child maps under the old limit and
  exits 1. The `t_cpu` fix commit `8b2fcaa3` records host stalls of 145 and 157 ms on CI's TCG runner, and
  `t_cpu.rs:155` an overrun of 250 ms in 2 s. Not seen failing in the docs I read.
- Class: fixed sleep. Fix (test only): the child waits for a message on a channel that the parent sends after
  `job_set_limits`.

### K6. `console.script:149-152`: `< flood` is satisfied by the echo of the typed command

- `> flood\r`, `< flood`, `> \x03`: the first `flood` in the output is the shell's echo of the line, so the Ctrl-C
  can go out before the program has printed anything, and "a program that floods the console still takes Ctrl-C"
  passes with no flood. In the log read the flood was running. Fix: wait for `flood\r\nflood\r\nflood`.

## Suspected (not seen failing; not verified beyond the lines named)

- **S1. `t_cpu` lower bounds are wall-clock under TCG.** `t_cpu.rs:293`, `:347`, `:407` ("starved": at least 80 %
  of the budget) and `:255`. `upper()` was relaxed off HVF (`:158`), the lower bounds were not. If budgets of
  periods that pass during a host stall are lost (periods are clock-aligned; I did not read the kernel's accounting),
  one 157 ms stall in `a_deep_job_tree`'s 600 ms window loses about 40 ms of a 150 ms budget whose slack is 30 ms.
  Fix as the `MAX_UNITS` pattern: with `factor() > 1` only a gross bound (`used * 2 >= budget`).
- **S2. `t_cpu` lateness bounds scale with a speed calibration, not with stalls.** `:603-607` and `:485`. At
  factor 12 the bound is 300 to 600 ms against stalls of 157 ms seen; at a lower factor it is below them. Fix:
  off HVF a 1 s limit, as `masked_limit()`.
- **S3. Other absolute wall-clock bounds:** `t_thread.rs:1184` (process ends within 400 ms; `:1168` is a 50 ms
  sleep "to let the last reach its wait", which only weakens the case); `t_gui.rs:680-685` (0.5 s a round and ten
  rounds within the compositor's 2 s `FLUSH_DEADLINE_NS`; runs under TCG in `tcg-default`); `t_quota.rs:397`
  (2 s beside a fork bomb of 24); `t_storage.rs:348` (every sync under 1 s); `t_streams.rs:112` (1 s).
- **S4. `port_wait_ignores_idle_subscriptions`** (`testing.rs:2113-2119`, `portbench`): a ratio of two medians
  measured one after the other must be at most 150 %; a host that slows between the phases fails it. Only the
  constants and the ratio line of `portbench.rs` were read. Fix: interleave the two measurements.
- **S5. `timer_interrupts_agree_with_the_clock`** (`testing.rs:961`, `:1001`): the shortfall allowed is 20 %, which
  its own comment calls a 40 ms host pause. **`deadlines_fire_in_order_and_cancel`** (`:1078-1084`): the cancel
  assumes under 20 ms between reading the clock and the call. Fix for the second: cancel right after arming.
- **S6. `t_storage.rs:327-331`**: "no reply within 300 ms" also holds without the fold if a commit takes over
  300 ms on a slow host; vacuous there, sound under HVF.

## Checked and found sound

- Kernel timer tests with lower bounds only (`one_shot_deadline_is_not_early`, `sleep_ends_no_earlier_than_its_deadline`),
  the 10 s `PATIENCE_NS` hang guards, and the unit-based INV-0270 tests (`testing.rs:2250-2839`), which use no clock.
- `job_signal_reaches_every_process_of_its_tree`: cooperative yields on one CPU, deterministic.
- No test is gated on `cpus > 1`: the kernel runs one CPU (`testing.rs:636-640`; the secondaries stay parked,
  `boot.rs:10-16`), so the smp 4 configurations differ only in the device tree, the GIC and memory.
- The lines after the end line (`first process: hello`, `first process ended: ... code: 7`) are still matched
  (`protocol.rs:141-145` runs before the end check).

## Coverage of this audit

- Read in full: `kernel/src/testing.rs`, `tools/test-runner/src/protocol.rs` and `main.rs`, `user/src/bin/t_cpu.rs`,
  the Makefile's `EXPECT`, `test-qemu-*`, `test-net` and `test-boot`, the `t_cpu` fix `8b2fcaa3`.
- Read in part: `script.rs` (step semantics, `send`, `expect`), `console.script` (lines 1-30, 80-100, 140-228),
  `zosh.rs` (the notice path), `t_handles.rs`, `t_quota.rs` (375-500), `t_gui.rs` (665-690, 800-841),
  `t_storage.rs` (300-350), `t_thread.rs` (436-452, 826-850, 1150-1188), `t_suspend.rs` (495-525),
  `t_bound.rs` (78-100), `t_snapshot.rs` (905-925), `exception.rs` (360-400).
- Only grepped (asserts on time, sleeps, early returns): `t_spawn`, `t_signals`, `t_ports`, `t_stress`, `t_leak`,
  `t_console`, `t_memory`, `t_channels`, `t_devices`, `t_loop`, `t_image`, `t_text`, `t_rooms`, `t_net`, `t_netd`,
  `t_streams`, and the rest of `t_snapshot`, `t_suspend`, `t_bound`, `t_thread`, `t_gui`, `t_storage`. Cases in them
  that check a return value but not the effect, or loops that run zero times, were not looked for.
- Not covered: `t_open` (a GUI application, not run by the test kernel); `tests/posix-core/t_threads.c` (the
  `waits` flake at line 837); the other shell scripts; `gui.rs` of the runner; the N6 backlog row
  (`a_spawn_with_a_big_space_is_bounded`, intermittent `NoMemory` in the 128 MiB guest).

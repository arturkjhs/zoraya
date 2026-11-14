# Backlog

Known small problems, to take up when the area is next worked on. Each id is a row of the fifth (V) or sixth (W) review in
[`research/stage-4/review-notes.md`](research/stage-4/review-notes.md), which has the evidence. Where a review was
limited to the lines it changed, what it finds outside them is added here.

| Id | Where | What |
|---|---|---|
| V3 | ADR-0050 Object | "Copied once more" holds only for a structure read as input. An output structure in one `Buffer` needs no extra copy, so the rejection of that alternative overclaims. |
| V4 | ADR-0057 INV-0057-1 | The reason for treating 875 ns as an upper bound on the rate is weak. A generation takes an install and a close, two system calls, which is the better argument. |
| V6 | ADR-0055 driver 4 | `fs/requirements.md` has an anchor for part of the driver. F7's reason for leaving it uncited is not the whole story. |
| V10 | ADR-0050 `channel_create`, ADR-0054 `device_map`, `dma_pin` | Lower-case `out` beside the `Out` kind. Say that these are `Buffer`, or allow `Out` for fixed records. |
| V11 | ADR-0051 INV-0051-5 | `Out` is in the check, not in the rule, while ADR-0050 says the invariant names it. |
| V12 | ADR-0054 Interrupt object | "ADR-0045's lint is enough" is stale: INV-0051-5 now covers the index. |
| V13 | Depends lines | ADR-0050 lacks ADR-0044, and ADR-0051 lacks ADR-0050 and ADR-0054. |
| V14 | ADR-0057 Option B, INV-0057-1 | Two sets of figures, for 1 microsecond and 875 ns. Use one rate. |
| V15 | ADR-0057 Option C, Consequences | 11 bits hold slots up to 2047. `POWER` keeps its value too, and is not named. |
| W3 | ADR-0051 Calls | "An address kind for the last" now reads as `length`. Name `memory_unmap`'s address. |
| W5 | ADR-0041 note, ADR-0054 Tensions | The exception to INV-0041-1 covers frames only; a quarantined pin also keeps heap use. |
| W6 | ADR-0053 What `init` receives | "`init` keeps them" now reads as giving `Power` to a restarted driver. |
| W8 | ADR-0054 Invariants | INV-0054-7 is out of order against `invariants.md`. |
| - | ADR-0054 Consequences | "Quarantined frames stay lost until a new driver runs" no longer covers a live driver that closes a pin. |
| - | ADR-0051 INV-0051-5 | Its list of payloads, from the sixth review's backlog items. |
| - | `zoraya-events` | The signal table keeps its peak size, so one burst of channels stays as heap until reboot. |
| R-23 | ADR-0062 INV-0062-1 | "Only the first process starts with a `Power` handle" is false now that `init` hands one to `zosh`. It should say that only the kernel makes one. |
| R-27 | ADR-0050 No cycles | The rule on moving an end with handles queued holds for `channel_write`. `spawn` can move such an end, which makes no cycle. Say "through a channel". |
| - | `zoraya-fs-service` | A loss seen during a request is reported by what it lost (`DEVICE_WRITE`), not as `DEVICE_LOST`. `init` treats them alike, so nothing depends on it yet. |
| - | `zoraya-protocol`, `zoraya-fs` | Two name validators, kept alike only by their tests (ADR-0013 open question). |
| R-18 | `zoraya-fs-service` `fs_status` | `ReadOnly`, `BadHandle` and `Internal` all become `StorageFailed`: a read-only mount reports every write as a storage failure. |
| R-6 | `zoraya-virtio` `Driver::new` | Capacity is two 32-bit reads with no `ConfigGeneration` loop; config-change interrupts are ignored; `F_RO` and `F_ACCESS_PLATFORM` are not considered. None of it happens with QEMU's device. |
| R-7 | `blkd` `Queues::publish` | The notify store is ordered by `dmb ish`; Linux uses `dmb oshst` before MMIO. Safe under QEMU, where the store traps. |
| R-8 | `zoraya-virtio` `Kind::Flush` | A flush is queued whether or not `F_FLUSH` was negotiated; unreachable, since the engine mounts read-only without flush. |
| R-9 | `zoraya-virtio` `Shared` | A safe trait whose implementation dereferences any offset: safety rests on the driver passing only its own offsets. |
| R-21 | `t_storage` | No test kills a driver with a request on the device (see design.md, Known limitations). |
| R-26 | INV-0015-4 | The device's `UNSUPP` becomes `FAILED` in `blkd` and `IoError` in the adapter: `DevError::Unsupported` is never produced. |
| R-27 | INV-0013-1 | `zoraya-protocol`'s `Name` has a table of cases and no property test against an independent predicate. |
| R-29 | `blkd` flush | Nothing in `make test` sees a flush reach the device from the real driver. |
| R-36 | `fsd` `copy_from`, `copy_to`, `DriverLink::data` | Copies from memory a client or device may change are data races in Rust's model; a volatile copy would match the SAFETY comments. |
| R-37 | `blkd` `Window` | The kernel checks a transport's size only up to `Status`; a read at 0x104 could leave a short mapping. Not on QEMU `virt`. |
| R-38 | `kernel/src/object.rs` `Object::same` | `Image`, `Interrupt` and `Pin` compare unequal even with themselves. Only tokens use it now. |
| R-39 | `zosh` `slots` | Generation 0 only: a reused slot's handle is not shown. Needs a kernel call to list a table. |
| R-40 | `kernel/src/device.rs` | `base + FRAME_SIZE` is not checked for overflow; only a device tree naming the last page reaches it. |
| R-41 | `tools/test-runner` `o` step | Counts occurrences so far, not over the whole run. |
| W7 | ADR-0054 Quarantine | A pin can also end when a queue is torn down: the text reads as a full list of ways. |
| - | ADR-0004 rule 5, INV-0004-6 | Exception channels are not built and no roadmap stage names them: a fault from a POSIX or native process ends it with reason `Fault` (ADR-0034, ADR-0106 item 12). INV-0004-6 stays `not yet applicable (until exception channels)`. A crash reporter, `tcc -run` or a debugger would bring them in, each by ADR. |
| - | `kernel/src/arch/aarch64/usercopy.rs` | Copying between user memory and the kernel is byte by byte: 3.2 us a KiB under HVF. After stage 4, try word-sized copies through the unprivileged instructions, with a measurement. |
| - | `init` (architect, 2026-11-02) | `init` must not block while it restarts one driver: today it waits up to 6 s for the compositor to take the new driver, and watches nothing else meanwhile. **Fix before stage 6's implementation starts.** |
| - | `compd` attach | Attaching a new display driver (`Info`, `Attach`) still waits for it, up to 2 s each (ADR-0093, Decision); flushes no longer do. |
| - | `blkd`, `zoraya-virtio` | `blkd` keeps its own split queue beside the generic one in `queue.rs`. Move it onto the generic one, with its mutation tests, when the blk driver is next worked on. |
| - | `compd` `Platform::call` | The display driver's end is subscribed on two ports while a call waits (the main port and the call port). It works; a single port with a per-call key would be simpler. |
| - | Mutation runs (all of them, from TASK-16) | **Memory guard, keep for every future mutation run.** A cargo-mutants run on `zoraya-stream` with four jobs took 63 GB and stopped the host twice: a mutant that never consumes a queue makes a test grow without bound, and macOS ignores `ulimit -v`. Three guards now: every host-tested crate under `crates/` installs `zoraya_test_alloc::cap_memory!()` (512 MiB of live heap per test process; past it the allocation fails and the process aborts, so the mutant counts as caught); `make mutants` runs at most two mutants at once (`MAX_WORKERS`) and `make mutants-host` passes `-j 2 --timeout 300`; both run under `tools/mutants/watchdog.sh`, which kills any process of the run above `WATCHDOG_MB` (3 GiB) and the largest when the run passes `WATCHDOG_TOTAL_MB` (12 GiB). A new host-tested crate must install the cap; `tools/` crates do not yet. |
| - | Mutation runs, TASK-20 (2026-11-03) | **The host froze at about 60 GB.** The command: `make mutants-host CRATE=zoraya-posix MUTANTS_HOST_ARGS="--exclude crates/zoraya-posix/src/ffi.rs"` (cargo-mutants, two jobs, under `tools/mutants/watchdog.sh`), host tests only: no QEMU and no guest program ran, and its output went to `build/mutants/cargo.log` through `tail -5`. The cause: `crates/zoraya-posix/tests/files.rs` did not install `cap_memory!()`; the library's cap covers its unit-test binary only, not an integration test's. Mutants that make `read_dir` always give a name (`process.rs` 1487 and 1498) turned `while let Some(..) = read_dir(..)` into a loop that pushed names for ever. The watchdog's log shows seven such processes killed at 3.1 to 3.9 GB of resident memory, two at a time. How the whole reached 60 GB is not established by the logs; a guess, not checked: the watchdog reads `ps`'s RSS, which on macOS leaves out compressed memory, and a vector of one repeated name compresses well. The fix: the cap in that file (with it, the same mutant aborts at 512 MiB, run by hand; and it showed that the unmutated tests needed more than the cap too, since each rig's volume was 64 MiB, 16,384 blocks I had taken for sectors, and twenty tests run at once: now 8 MiB and 4 MiB), `make check-test-caps` in `make test` (every library with tests and every top-level `tests/*.rs` caps its heap), and `tools/test-runner`'s bounds on guest output (`test-runner: bound guest output`: 64 MiB a run, 200 + 200 lines on the terminal, 64 KiB lines, bounded buffers and channels, with host tests of an endless output and an endless line). Since then (the architect's decision) the watchdog reads each process's physical footprint through `tools/footprint` (`proc_pid_rusage`, `ri_phys_footprint`, which counts compressed pages), with RSS as the fallback off macOS, and a test holds 2 GiB of touched zeros under `WATCHDOG_MB=1024` and requires the kill. Measured on the M4 with memory to spare, that process's RSS and footprint were the same (2.0 GiB): macOS compressed nothing without pressure, so the guess above stays unconfirmed; the footprint covers it either way. |
| - | TCG, `t_snapshot` (TASK-16) | **Cause found, not a kernel defect.** The first frame-exhaustion sweep of `t_snapshot` (commit `96c5a79`) held all free memory and gave it back at every step, writing a list link into each held frame. On the test kernel's second run of the program in one boot, under TCG, every step took about 3 s instead of 0.1 s, so the sweep took about 1000 s and looked hung (20 of 20 runs with two QEMUs at once; `make test`'s 180 s and a 900 s run). Measured with counters read over the gdb stub: the allocator scans about one bitmap word per call in both runs and no other thread is ready, but the guest does about 35 times fewer allocations a second in the second run; with the list kept in a static array instead of in the frames, the second run is as fast as the first (two runs). The frames then free include thousands that held user code the first run's snapshot children executed; under TCG, memory accesses to those pages stay slow. A QEMU TCG cost. Likely explanation, unverified: QEMU tracks writes to guest pages that hold translated code (its
| N6 | `kernel/src/testing.rs` `a_spawn_with_a_big_space_is_bounded` | Intermittent `NoMemory` (then `first process did not start: OverQuota`) in the 128 MiB test guest: seen in `test-qemu` and `test-net` under `make test-changed`, and in 1 of 3 and 2 of 4 standalone `make test-net` runs, on the tree of `main` (ac8d7de) as well as on `work/n6`; `make test-qemu-one` passed 2 of 2. The test says a 4 MiB run is rare in the guest; the ramdisk is 27 MB of the 128 MiB, and the reaper's timing decides whether the earlier tests' frames are back. Not looked into. |
self-modifying-code detection), and accesses to those pages stay on a slow path; this was not checked against QEMU's
sources or with QEMU's own tracing. The sweep since `e423bb5` holds frames once and gives back one a step, which avoids it. To repeat: `tools/repro/tcg-loop.sh 96c5a79 20 2` (builds that commit in `build/repro-96c5a79`, two QEMUs at most, samples a run that does not end). Other code that writes many frames that once held code under TCG would pay the same. |
| - | `Power`, ADR-0062 (architect, 2026-11-03) | `reboot` is a shell builtin (ADR-0155 Decision): the kernel needs a reset call for a `Power` holder (PSCI `SYSTEM_RESET`), its shape proposed in ADR-0159 (TASK-18 plan step 12). Kernel work; until then `reboot` says it is not available (INV-0155-7). |
| - | Fuzzing | `zoraya-term`'s parser and the GUI decoders have deterministic random tests in `make test-unit`; no coverage-guided target exists for them in `fuzz/` (no nightly toolchain on the host in TASK-14). |
| - | dash (ADR-0142) | The fork group hint and `vforkexec` with `POSIX_SPAWN_TCSETPGRP` first ran in 6d (TASK-21) and needed no change. dash's fallback when `posix_spawn` fails (`vfork` with no group hint) puts the child in the shell's group, so its `setpgid` and `tcsetpgrp` fail; it only runs after a spawn failure, which dash then reports. |
| - | Kernel test `running_out_of_asids_is_an_error`: the "ASID fault" (fix/asid, 2026-11-09) | **Fixed; one leftover.** There was no fault, and no other CPU to hold stale TLB entries: only the boot CPU runs, in the smp4 configurations too (`docs/design.md`, "Single core"). CI's record was `code: 101, reason: Exited`, a user panic: "memory allocation of 5 bytes failed", as in the loaded-M4 failure. The "EC 0x0 at `ELR_EL1` 0x401008" line is `t_image`'s process with the text zeroed on purpose, printed in every run. A running `hello` takes a 1 MiB heap step (`zoraya-heap`'s `GROW_SIZE`), and the test let its 255 run while it spawned them, so how many held a step at once depended on the host's speed; what 128 MiB leaves free holds at most 36. Reproduced every time with 0.5 ms slices (219 of 255 failed). The test now spawns them suspended and runs them one at a time, at a 0.5 ms slice (`sched::set_test_slice`, test kernel only) so that the old form would fail on any host; it measures 15045 frames for the 255 suspended processes, with 9391 free under 128 MiB, and asserts room for two heap steps; it asserts only once every process has ended, and spawns one more as soon as the first ends, while 254 are held. The quarantine (`KNOWN_ASID`, the kernel's `KNOWN_FAILURE`) is gone. **Leftover:** the runner keeps `--known-failure` with no user, so that a branch that still passes it does not break; remove it once no branch does. |
| - | Images (TASK-21, TASK-22) | An `exec` or `posix_spawn` reads the whole program through the file protocol each time (25 ms under HVF for 1.4 MiB): 61 % of W8's wall time under HVF and 67 % under TCG (TASK-22). **The next task after stage 6e:** an image cache in the broker, on ADR-0212's conditions (read access proved by the caller's connection, key (volume, object id, version), bounded; INV-0212-2 to -4). |
| - | T2 (architect, 2026-11-04) | The order to T2: the image cache first (ADR-0212), then T2's 29 missing names (`make -f ports/Makefile t2-probe`: toybox's T2 applets lack `regcomp regexec regerror getline mkdtemp getgrgid getgrnam getgroups getgrouplist link lchown mknod dirname dprintf getentropy gethostbyname hstrerror __h_errno_location sethostname towlower`, busybox's `awk expr gzip` lack `regcomp regexec regerror regfree rand srand strncasecmp fileno_unlocked fputs_unlocked`), then W4 measured for real (`make measure-w4`). `dlopen` stays out (ADR-0210's decision) until T2's ports show how many programs need it. |
| - | One CPU (architect, 2026-11-04) | The POSIX session's guest has one CPU, so `-j` gives nothing: W8 takes 88 s under HVF at every `-j`, against 13.4, 6.8 and 3.4 s at `-j1`, `-j2`, `-j4` on TASK-02b's four cores. SMP for user processes is not in any plan. |
| - | Stage 6e (TASK-22) | A process ended by a signal sends no statistics (ADR-0211 item 3): W9's interrupted makes are not in its totals. |
| - | Stage 6e (TASK-22) | INV-0109-1's check of tcc's `a.out` from the W1 check on the host needs the file out of the image (ADR-0110's Option D tool, deferred); W1's `./t` running is the check today. |
| - | Stage 6e (TASK-22) | The POSIX session's guest needs 512 MiB: every POSIX process holds a 1 MiB heap step at least (`zoraya-heap`'s `GROW_SIZE` on eagerly committed objects); a smaller first step would let 128 MiB hold W8 at `-j4`. |
| - | `ttyd` (TASK-21 review R13) | A reply that finds the client's channel full is dropped; a client has one request out and a held read, so it does not happen today. |
| - | Broker (ADR-0104, "Trust at stage 6") | A process can name any of its pids' records it is the parent of; the kernel has no query tying a process handle to a job. |
| - | `ports/` (ADR-0143) | The host check of tcc's link step uses a tcc compiled for the host from the same source; the Zoraya tcc's own output is first seen in W1 (6e). |
| - | `ports/` | Programs go into `build/rootfs-posix` with their debug information (`-g` as measured): dash 853 KB on disk for 170 KB loaded, tcc 1.7 MB for 549 KB. Strip them for the image if the volume's size matters; the toolchain has no strip tool without `llvm-tools`. |
| - | `zosh` in a terminal (ADR-0172's decision of 2026-11-09) | **The crash window of `/config/rooms` written in place.** A shell holding a tree entry cannot publish, so `room new` and `settings` truncate and rewrite `/config/rooms` and `/config/settings` where they are, not by writing a new file and replacing the old one. A crash between the truncation and the last write's sync leaves the file empty or cut short: the panel then reads fewer rooms, or none, and `zosh` falls back to the declared settings. Accepted for now by the architect; to close it, give the shell's `/config` entry the right to publish, or have the panel (which holds the root) write the file for it. |
| - | POSIX job control, `tests/posix/scenarios.script` on CI (seen at `522269a` and on 2026-11-09) | **Fixed (2026-11-10): Ctrl-Z came before the job held the terminal.** Traced with prints in the broker and reproduced locally under TCG with Ctrl-Z 250 ms after Enter: the terminal's `Suspend` named group 2, the interactive shell's own (`suspend group=2 fg=Some(2)`), and only then did the job's `tcsetpgrp` reach the broker (`foreground told 3`); dash ignores SIGTSTP, so nothing stopped and the job ran to its end, as CI showed. This is POSIX's behaviour (dash's `forkchild` gives the terminal to the job only in the child, after the fork, which takes over a second through the broker on CI's runner), not a broker bug: the script sent Ctrl-Z too soon. The script now waits for a word the job prints once it holds the terminal and sends Ctrl-Z the moment it shows (three rounds, while `sh` is starting `sleep`); `zoraya-broker`'s `a_suspend_before_the_jobs_tcsetpgrp_is_the_shells` forces the lost interleaving and the stop that follows it. |
| - | The file manager (7h) | **A trash** inside each writable place (for example `~/project/.trash`), so that a delete can be undone without giving files any new right: Delete moves into the trash of the place the file is in (a rename within the place), and the trash is emptied by a delete in it (the architect's request of 2026-11-10). |
| - | The GUI, under TCG (2026-11-09) | **Keys typed right after a focusing click can be lost.** In `tests/gui/windows.script`, after the closed terminal's replacement was focused by a click (its focused ground already drawn), `poweroff` typed at once arrived as `off`, locally and on CI; with 500 ms between the click and the keys every key arrives. Not traced: whether the keys reach the compositor before the click's focus change, or `term` drops them. The script waits; find where they go. |
| - | `zoraya-elf-check` | `USER_LOW` (4 MiB) is a copy of `kernel/src/memory/user.rs`'s; move it into `zoraya-abi`, which both would use, when that crate is next changed (TASK-17 review A11). |
| - | `posix-surface-check` | `hide` and `check` share the tool's ELF reader; the cross-check against `zoraya-ar`'s index (its own reader) is what keeps a reader bug from hiding a name and passing the check alike. |
| - | `libc.a` (ADR-0141) | `getpwnam` and friends call `pthread_setcancelstate` and the `nscd` client's `socket`, `connect` and `sendmsg`. Settled in 6c (proposed ADR-0180 item 9): `pthread_setcancelstate` succeeds and changes nothing, the socket calls fail with `ENOSYS`, and musl's parsers, which read the files first, are not patched. Drop this row if the architect accepts ADR-0180. |
| - | Desktop memory footprint (found by Q1) | The GUI session at scale 2 used about 96% of a 256 MiB guest: the charge is within 0.5 MB of the frames in use (used 230.85 MB, peak 233.12 MB of 241 MB free at boot; with no limit only 10.7 MB stays free). Q1's session memory limit refused the files manager's menu in `gui-files-manage-auto-2`; the GUI guests now have 512 MiB. To look at: the compositor's surface copies for up to 8 clients, the 1 MiB heap steps per process, the 64 KiB kernel stack per thread. Decide a target once the Q tasks measure. |

## Mutation testing: `zoraya-virtio`

`make mutants-host CRATE=zoraya-virtio` (cargo-mutants 27.1.0), 2026-11-02, branch `tests/virtio-mutants`. Before:
168 mutants, 139 caught, 25 missed, 4 unviable. The `mutant-triage` subagent sorted the 25; new tests in
`crates/zoraya-virtio/src/tests.rs` cover the holes. After: 156 caught, 8 missed, 4 unviable.

- **Killed: 17.** Each was a hole in the tests, not a bug in the driver: every new test passes on the unmutated code.
  The holes were a request with exactly 30 pieces (and a literal 31 refused), each request of a batch reaching its own
  sector, a failure of the second request in a batch, an answer left intact by a later `queue`, the high words of a
  queue address above 4 GiB, `FAILED` kept when the device already shows it, and the queue layout: rings aligned as
  2.7 requires, not overlapping, and with the headers within the four pages `QUEUE_MEMORY` promises.
- **Equivalent: 8**, all left missed on purpose.
  - Six in the layout constants (`lib.rs` 93:34, 93:38, 93:42 twice, 94:44 twice). Each changes a size that is then
    rounded up to 4096 and lands on the same page, so `USED_AT` and `HEADERS_AT` keep their values.
  - 265:36 writes the reserved header word at `header - 4` instead of `header + 4`. The queue memory is zeroed
    (`Driver::new`'s contract) and nothing else writes either place, so the word is 0 either way.
  - 275:36 `|` to `^` on the descriptor flags. `DESC_NEXT` (1) shares no bit with the data flags (0 or `DESC_WRITE`,
    2), so the two give the same value.
- **Left: 0** that are not equivalent.

The triage's one miss: it said the 30-piece test kills `MAX_PIECES` = 16, but a test that takes its count from
`MAX_PIECES` changes with it. The test uses a literal 30.
| - | Stage 9, "living in the system" (architect, 2026-11-12) | **curl and wget (N4's W11, W13)** are deferred from stage 8: a C TLS library first (BearSSL or rustls-ffi), then curl and wget and the `+net ... curl -O` check. `zfetch` stands in until then. Not a blocker for stage 8. |

## `sleep 60 &` ends at once in a dock-launched terminal (Q7)

Seen once in `tests/gui/spinners.script` (`[1] done: sleep 60`, a terminal launched from the dock in a room); not reproduced by the reviewer,
and the same line in the console shell's room stays running. `sleep` ignored `sleep_until`'s early return; it now loops until
`clock_monotonic()` passes the deadline (Q7 review fixes). If it still happens in the GUI, look at what wakes the process (a signal or
an event on its terminal) and at the job notice for a background job in a dock room. `spinners.script` uses `spin &`.

## A failed `t_gui` leaves its GUI stack running (Q7)

When `t_gui` panics before `gui.end()`, the compositor and drivers it started keep their frames: every later process test
of the same `make test-qemu` run fails with `NoMemory` (`t_bound`, `t_rooms`, `t_snapshot`, then
`a_spawn_with_a_big_space_is_bounded`), so one failure reads as five. Seen on the candidate run of `work/q7`
(`candidate/work-q7-a`) and reproduced with a `panic!` after `full_redraw`. Read the first `user panic` of the log; the
fix is for the GUI stack to end with the process that started it (a job of its own that `t_gui`'s exit kills).

## Gate blockers from the review of the system calls (TASK-26, branch `work/q7-fixes`)

`room new --untrusted` stays refused. Fixed on `work/q7-fixes`: errors 1 and 2, warnings 3 to 6 (see `docs/tasks/TASK-26-report.md`). Open:

- Warning 7: the heap keeps its high-water mark while the charge is refunded. Measure first (`HEAP_BACKING` and free frames after a burst of queued 64 KiB
  messages, on 512 MiB), then a heap ceiling for the untrusted job, or return free chunks to the frames, or record the bound in ADR-0261.
- Notes 8 to 14 of `docs/tasks/TASK-26-syscall-review.md`: uncharged growth, `Paid` under `EVENTS` holding the last `Arc<Job>`, `console_write` at 60 ms, code from memory
  objects, the refusal only in the clients, contiguous memory, smaller items.
- `Job::drop`'s `retain` and the `children` growth are linear in a parent's children (one interval each); a long job chain's drop runs in whichever call drops the last handle.
- Gate rows that are not truly `checked:` (the strict check lists them), the pending `disk-filler-tmp`, INV-0262-4 for `+net`, the architect's decision on ADR-0299, then the full CI.

## `t_cpu`'s masked-time limit off HVF (TASK-27, branch `work/cpu-flake`)

Three CI failures of `cpu_ceiling_from_a_process` at `t_cpu.rs:181` (`the exit of 40 sleepers: 156708320 ns masked`, `rooms next to
a woken process: 144941856 ns`, both on TCG at factor 12 against 128 ms) were host stalls, not kernel work: the 156.7 ms interval held
55 units, the same as the same job's green runs (1.6 ms on TCG, 87 us on HVF; 120ee31 before 9b the same). Off HVF the time limit is now
`t_bound`'s 1 s and the units (`MAX_UNITS`, ADR-0270) are the check; under HVF it stays 2 ms. Open: one HVF failure (2696 us against
2 ms on the `work/9b` review run, `a job about 30 levels deep`, a loaded host) is under the HVF limit's own rule; if it recurs, look at
the deep job's 64 units (849 us locally, the longest interval of the program).

## Follow-ups from the 9b libposix review (architect, 2026-11-13)

Known limits of the accepted ADR-0314 and ADR-0315, each to be taken up later:

- Service-reply waits inside the library lock: `stop_self` and the tty `TCSADRAIN` reply still hold the library lock while they wait. Move them to Pending waits (ADR-0315 item 1, the `call()` audit; ADR-0302 item 9).
- The cancel-in-wait leak: a thread cancelled inside a long wait leaks the frames' heap blocks, bounded at about 32 KiB per cancelled thread, usually under 300 bytes. Restructure with the CANCELLED marker so the cancel acts at the top of the call (ADR-0315, the deviation from ADR-0305 item 4).
- The `check-cancel-points` script is not written (ADR-0305 item 4; ADR-0315).
- In-QEMU cases for `waitpid` and `accept` in a thread: `waitpid` has a case in `t_posix.c` now; `accept` has none, the session has no loopback server (INV-0302-2, ADR-0302).
- The HVF `t_cpu` flake: 2696 us against 2 ms on a loaded host, the 30-deep job, 64 units (the `cpu_ceiling_from_a_process` test; the off-HVF limit was fixed on `work/cpu-flake`, the HVF rule is unchanged).


## Mutation lists that no longer apply (part 9b, branch `work/texec-fix`)

Part 9b's review committed a mutant's `replace` by mistake (b447edd: `exec` ended the old image after `state()`
returned, so the library lock was released before the end, and a sibling's `write` could follow the new image's
first; `texec` caught it on CI's TCG leg). `make check-mutant-lists` (`zoraya-mutants --check`, in `make check`)
now fails when a mutant's `find` is gone, which a committed `replace` causes. It skips four lists
(`MUTANT_STALE` in the `Makefile`): 15 of their mutants (in `compositor-settings`, `fs-limits`, `fs-service` and
`kernel-objects`) do not apply, code that moved since the lists were written. Update those lists, then empty
`MUTANT_STALE`.

## Follow-ups from the architect's decisions on part 9 (2026-11-13)

- A program started by `dash` or `make` (for example `curl`) has no `getrandom` connection, so OpenSSL cannot seed (ADR-0310; the shell gives the connection only to programs it launches itself).
- Record locks shared across processes (ADR-0319 Option B): the lock table lives in `fsd`, keyed by the process's connection. Reason: with the in-process table two processes on one SQLite file do not see each other's locks and can corrupt it. Needs a protocol change and a review. Decided 2026-11-14: it stays in the backlog until stage 10 and is not scheduled before.
- The wheel in the native applications beyond the terminal (9e, proposed ADR-0322): `zoraya-app` drops `Wheel` (ordinal 46), so the file manager's list, the editor and the settings window do not scroll with it; `zoraya-ui`'s `ScrollView` and the list need a wheel event of their own. NetSurf takes no horizontal turn (libnsfb has no button for it).
- The `make test-boot` leg `boot-tcg-smp4-gicv3` timed out once on CI (run 43437795275, 936db09: `timed out waiting for "bytes dropped]"` after the `spin` command, the shell echoing slowly under TCG with four CPUs); the re-run of the job passed. A timing flake of the flood check; make the wait independent of wall-clock speed if it recurs. **Cause found (TASK-29, F5, `docs/research/test-audit/report.md` section 3): not slow echo but the script's order.** The shell reports the count at its next prompt; the step passed only when the burst reached the shell before it had read `spin`, and timed out when the job was already in front. `tests/shell/console.script` now starts the job first and expects the count after it ends (branch `work/test-audit`).
- `tools/test-runner/tests/flood.rs`: two of its four tests missed their 60 s bound (69.7 s) when `cargo test -p zoraya-test-runner` ran beside other work on an M1 (TASK-29's review, R13); all four pass alone. A wall-clock bound on the host; make it scale or drop it if it recurs.
- **Ctrl-C does not end a continued POSIX job once it runs a later child (found by TASK-29, candidate run 43575482537, job `posix-scenarios-tcg`).** In the POSIX session: `sh -c 'printf "in\146g\n"; sleep 5; echo first; sleep 1; echo second; sleep 30'`, Ctrl-Z at `infg`, `fg`, wait for the job's own `first` and `second`, then Ctrl-C. Seen: `^C` is echoed, the job runs on for the 30 s of `sleep 30` and `echo after=$?` prints `after=0`. Expected: the job ends at once with 130, as it does when Ctrl-C comes half a second after `fg` (during the continued `sleep 5`), which is what `tests/posix/scenarios.script` checks and has always checked: its `< first` and `< second` matched the line `fg` echoes. One run, under TCG; not looked into (ttyd's foreground handle, the broker's group and the job signal after `job_resume` are where to look). No test covers it: the corrected step was put back to the early Ctrl-C so that CI stays green, and says so.


## CI downloads: a flaky upstream server makes CI red (2026-11-13, `main` at `7adf938`) - done (9f-ci, 2026-11-14)

`make fetch-ports` downloaded every source of `ports/sources.lock` when CI's ports cache missed, and the cache key was the hash of the
lock, so any change of the lock missed and every archive came from the Internet (`main`'s run 43511821485: `ftp.gnu.org` timed out for
`make-4.4.1.tar.gz`). Done, with no new ADR (the cache is a CI detail under ADR-0109, whose SHA-256 check is unchanged): the build job
restores `build/ports/src` with `actions/cache/restore` (key `ports-src-` plus the lock's hash, restore key `ports-src-`, so a changed
lock starts from the newest older entry and downloads only the new archives) and saves it with `actions/cache/save` after the fetch even
if a later step fails. `tools/fetch-ports.sh` verifies every archive against the lock and downloads again one that fails, so a corrupted
entry is never trusted (`ports/test-fetch.sh`). `tools/check-fetch-cache.sh` (run by `make check-fetch-ports`, so by `make check`)
fails if a job fetches without those steps. Left: a first run on an empty cache still reaches the upstream servers, and a GitHub release as
a mirror in `sources.lock` is not built; a fetch outside `build` (the nightly's, if one is added) must follow the same steps.

## Memory of the run machines (2026-11-14)

The `Makefile` comment above `RUN_NET_CMD` gave the per-launch leak of 8.5 MiB (`curl`, `wget`) as the reason for 512 MiB. That leak
is fixed (`a9f0cfe`; `apps-launches` runs 16 launches on 128 MiB), so the reason was wrong. The real one is the working set: the
programs a person runs there are large (`python3` 15.5 MB, NetSurf 12.5 MB, `curl` and `wget` 6.7 MiB each, loaded whole), each
Python thread stack is committed in full and `os.fork` copies the whole writable space (11 children on 512 MiB with 8 MiB stacks, 16
with 1 MiB), the GUI surfaces are 1024 x 768 per client, and the charge reserve is 1/16 of memory (`docs/design.md`: the 256 MiB GUI
test refused the files manager's menu). No limit smaller than 512 MiB has been measured for these machines, so 512 MiB stays and
nothing is known to go wrong with less; measuring the smallest size that runs `make run-gui` with a Python program and NetSurf is not
done.

## Stage 9 parts: what the reports left (TASK-27, 2026-11-13)

From the "Not done" and limits lists of `docs/tasks/TASK-27-report.md` that no row above holds yet:

- **Threads (9b).** INV-0306-3 (a fork child has one thread and no parent thread handle) and INV-0306-5 (a failed `snapshot_process`
  parks no thread) still say `not yet applicable (stage 9)` in `docs/adr/invariants.md` though stage 9 is done: no in-QEMU test of
  either exists (a `job_get_usage` of the child's job; a refused charge, then the siblings' counter): write them or record why not
  (ADR-0001 rule 6). INV-0308-3 is checked by `make posix-calls` since 9f-ci (`posix-stats --no-thread-calls`, already in the Makefile). The mapping count of a CPython-sized process with 128 thread stacks is not measured (ADR-0300's open question; 9c measured
  4000 threads one at a time, not 128 at once). The SHA-256 / AES-GCM / `RAND_bytes` half of ADR-0309 item 4
  (`tests/posix/ossl_threads.c`) is not written.
- **TLS programs (9a).** No `tls.roots` setting reaches C programs (they read `/system/tls/roots` only). 9a's own code (`random.rs`,
  the `ZPBX` parse, `posix_child`'s close paths) has no hand-written mutation list in `tools/mutants/`.
- **`work/tls-options`.** ADR-0321's refused lists are read by hand from curl's `tool_getparam.h` and wget's `init.c`: a check that
  diffs them against a new release's tables is not built. Neither program's `https` to `http` redirect rule is tested (the test
  servers send no redirect).
- **NetSurf (9d).** Downloads through the chooser (a second `ZPBX` slot for a chooser channel); a shell that `init` restarts gets no
  window channel until the panel restarts; a page's links to other hosts need `+net HOST` words on the line; timed waits are a 2 ms
  poll loop; no JavaScript, SVG, cookies, proxy, clipboard or cursor shape; titles Latin only. INV-0317-1's shell side is checked
  only by the marker script (the refusal is unreachable while an untrusted room cannot be entered). `docs/design.md` has the section on the window of a POSIX
  program and NetSurf (9f-ci).
- **CPython (9c).** `make test-python-long` (`test_queue`, `test_threading`) runs only by hand: too long for CI's 45-minute jobs under
  TCG; `test_threading` ends "env changed" because `os.fork` runs out of memory (no copy-on-write). No `socketpair`, `AF_UNIX`,
  server sockets, `posix_spawn`, shared modules or `ctypes`. The archive's Sigstore bundle is not checked. The cold-cache time of CI's
  `build` job with NetSurf and CPython is not measured (the job has 60 minutes).
- **Compositor (9e).** The session's 20 clients are not carved per room; a client with no window keeps its slot (ADR-0323's open
  questions). The scale-2 and TCG GUI runs of 9e's scripts ran only in CI.
- **NetSurf on the real web (9f final step, `docs/tasks/TASK-27-report.md`, "9f: a usable browser").** Wikipedia (ru, uk) loads over
  TLS with the production roots and its Cyrillic text is right, but the layout is flattened and an article's prose below the lead was
  not seen (cause not diagnosed: CSS support or a layout bug); SVG and WebP images are not drawn (empty frames); CJK and other scripts
  are boxes (no font); no JavaScript, so no search box. `make hand-netsurf-wikipedia` and `-2` rerun the screens by hand (real Internet,
  no CI).

## Follow-ups from the architect's decisions on stage 10's design (2026-11-14)

ADR-0331 to ADR-0342 are accepted (TASK-28; the plan is `docs/research/stage-10/plan.md`). Each item below is a limit an accepted ADR
states, to be taken up later:

- Atomic save for `vi`: nvi2 saves a file in place, so a failure in the middle of a write can leave it cut short (ADR-0335: `vi FILE`
  holds one name of the file's directory and cannot write a second file and rename it). Needs a way to save by rename under a
  one-name grant.
- A stored git token in git's own directory: git stores no credential, so a person types the token at every push (ADR-0339; the
  directory is ADR-0341's, which git's children hold to read only). Decide where the token lives and how hooks and other children
  are kept from reading it.
- Pseudo-terminals when `tmux` or an ssh server is wanted: stage 10 has none (ADR-0338: when one is needed it is a pair of handles
  from the broker, with no name; the pty functions stay on ADR-0342's guarded list until then).

## Follow-ups from the architect's decisions of 2026-11-14 (after stage 9f)

- CPython's `random` seeding from the time and the pid in an `os.fork` child (the review of the `getrandom` fix, finding 6), and a
  nested `python3` that fails (one `python3` started by another), have one cause: a child has no random service connection.
  TASK-31's part 10h closes both (ADR-0339 item 7: a child gets the random connection). The test belongs there: `os.urandom` works
  in an `os.fork` child, and a `python3` started by `python3` creates a default SSL context.
- The Ctrl-C defect on a continued job (the late Ctrl-C after `fg`, recorded by TASK-29's `scenarios.script` step V6) goes to
  TASK-31's part 10c.
- `work/test-fixes`, a small part after `stage-cxx`, beside TASK-31's first parts: the open items of the test and CI audit
  (`docs/research/test-audit/report.md`): the remaining vacuous-test findings; the `test-crash` check and the `t_quota` handshake;
  the `libc.a` relink that rebuilds every port in the build job; the stale "not yet
  applicable" invariant rows and `adr-check` accepting any text after `checked:`; checks for INV-0172-4 and INV-0230-1.
  Done (first item): `tools/test-lanes.txt` gives `test-apps`, `test-netsurf`, `test-posix-suites` and `test-posix-threadtest3` lanes,
  `test-python` and `test-python-regr` print `SKIPPED` unless `PAR_LONG=1`, a step with no lane fails the run, `Makefile`,
  `tools/run-qemu.sh` and `tools/test-runner/` are in the map (`make check-test-lanes`). The new lanes were not run end to end here
  (no ports cache in the worktree); see the part's report.
- A URL operand's name rule is public-only for curl and wget too, not only for NetSurf (ADR-0328, extended by the architect on
  2026-11-14): done in `work/9g-url` when it merges; until then curl and wget keep ADR-0223's name rule.

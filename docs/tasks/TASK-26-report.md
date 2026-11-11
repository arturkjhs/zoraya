# TASK-26 report: stage 8, quotas and the network

Running report, updated after each part. Written by the Sonnet main session; reviews by `opus-review`.

## Q1: frame and kernel budgets on jobs (branch `work/q1`)

### Built

- `crates/zoraya-charge`: the charge walk (`charge`, `refund`, `set_limit`, `effective_limit`) behind a `Tree` trait,
  with a model test over random trees (INV-0261-1, -2).
- `zoraya-abi`: `Status::OverQuota`, `QuotaField`, `Rights::LIMIT`, `job_signals::OVER_QUOTA`, `JobLimits`, `JobUsage`,
  `job_set_limits` (52 is `job_get_usage`, 51 `job_set_limits`), `UsageFlags::CLEAR`.
- `zoraya-channel` messages and `zoraya-events` subscriptions carry a payer that drops with them (the writer pays,
  ADR-0261 point 7; the holder pays for a subscription).
- Kernel `charge.rs`: `Paid` (RAII), `arc`, `prepaid_arc`, `prepaid_box`; charged: address spaces (user frames and page
  tables), memory objects, processes (count, record, handle table, thread and kernel stack), channels, queued messages,
  streams, ports, events, images, subscriptions, job records. A refusal asserts `OVER_QUOTA` on the refusing job and
  puts the field in the value register.
- `init`: the kernel reserve on `init`'s job; the session job (three quarters of `init`'s memory, 200 processes) for
  `ttyd`, the broker and the shell.
- `make check-charged-alloc` (INV-0261-5's lint), `make check-untrusted-gate` with `tools/untrusted-gate.txt`; the
  refusal of `room new --untrusted` is marked `untrusted-gate: refusal` in `zosh` and the panel and stays.
- Tests: `t_quota` (in-QEMU), host tests of the charge crate and the channel payer.

### Decisions where the ADRs are silent

- **Every job counts `used`, not only limited ones** (ADR-0261 point 4 keeps a counter only where a limit is at or
  below). It keeps `used` right when a limit is set on a job that already holds something, at the cost of an add per
  ancestor (at most 32).
- **A process is charged for its handle table at 1024 slots** while it lives (the charge goes back in `exit_current`, so
  a zombie keeps only its record); a port is charged for 64 packets and 64 more each time its ready list outgrows what
  it has paid (kept until the port ends); a channel for its queues' slots (1 KiB). Over-charging while live, never
  under. Measured: after this the GUI test's init job ends at 230.8 of 233.6 MB (was 231.2), with the charge within 0.5
  MB of the frames really used, so over-charging is not the cause of the thin margin.
- **`held`** is the whole of a job's memory once it is killed and empty. A per-holder figure needs identities the
  kernel does not keep (ADR-0261 open question).
- **`failure value`**: the refusing field is in x1; the stubs expose only the status.
- **Kernel reserve stays 1/16** (ADR-0261 point 10 is `[assumption]`): at 1/16 and at 1/32 the 256 MiB GUI test
  (`files-manage:auto:2`) refused the files manager's menu. See 'Reserve' below for the architect's decision.
- Not charged: devices and pins (made by driver processes, ADR-0261 point 6), kernel threads, the transient copies of
  `spawn`'s buffer (bounded and freed in the call), the vectors of a job's member lists (covered by the process's fixed
  `LIST_ENTRIES`).
- `OverQuota` maps to `ENOMEM`/`EAGAIN` in `libposix` until Q2 gives the table by field.
- The hint text of the refusal in `zosh` still names ADR-0056 (rejected); Q6 changes it with the keeper.

### Open for Q2 and later

INV-0261-2's in-QEMU half around a refused `spawn` and `snapshot_process`; INV-0261-6 with an object held from outside
past the kill; INV-0261-7's in-QEMU half; the fork bomb half of INV-0261-4; `snapshot_process` is refused after the
copy, not before.

### Review

`docs/tasks/TASK-26-review-q1.md` (by `opus-review`).

### Model split

Sonnet main session: implementation, tests, report. Haiku `test-runner`: all QEMU and `make test-changed` runs.
Opus: the Q1 review.

### Handoff after Q1's candidate run (for a fresh session)

- **State:** branch `work/q1` (pushed at `2548c16`; one local docs commit after it: `docs/design.md`'s Quotas
  section). Candidate tag `candidate/work-q1`, full CI run `42974811107` (watched by a `ci-watch` subagent; if its
  result is not in this report, read it with `gh run view 42974811107 --json status,conclusion`). Q1 was merged into `main` at `0a99422` with the 1/64
  reserve, before the decision above; this correction (reserve 1/16, GUI guests 512 MiB) is a second merge.
- **Next:** if the run is green: push the docs commit, merge `origin/main` into the branch, merge into `main` with
  `--no-ff` from a detached HEAD at `origin/main`, push `HEAD:main`, `ci-watch` on `main`'s run, then tag `stage-8-q1`
  (`refs/tags/stage-8-q1`) and delete the branch. If red: `log-triage` on the failing job's log, fix forward.
  Then Q2 (`work/q2` from `main`).
- **Working rules from the architect (usage check, after the first hour):** keep the main session's context small (grep
  first, read line ranges, repository questions to the Explore or a Haiku subagent); a fresh session after each merged
  part and at large step boundaries; `opus-review` gets the diff and the names of the ADRs, not the exploring; Opus only
  for the kernel, the charge and scheduler code, services' accounts, trust boundaries and `unsafe`, and a Sonnet
  subagent reviews docs, tests and plain service code; report the cost per part by model.
- **Cost per model, Q1:** not measured by the session (no counter is available to it); the architect's usage check says
  the main session ran at about 290k tokens of context per request. Recorded as the reason for the rules above.

### Reserve: 1/16, GUI guests at 512 MiB (the architect's decision)

The GUI session at scale 2 really uses about 96% of 256 MiB (the charge is within 0.5 MB of the frames in use: used
230.85 MB, peak 233.12 MB of 241 MB free at boot; with no limit only 10.7 MB stays free). 1/16 and 1/32 both refused
`files-manage:auto:2`'s files manager menu. The charge is accurate, so the margin cannot be won by charging less. The
architect decided: the reserve stays at ADR-0261 point 10's 1/16 until a Q task measures what the kernel needs (it is
an assumption to be measured, not tuned to make a test pass); the GUI test guests and `make run-gui` get 512 MiB; the two
smaller charges stay (the handle table's charge goes back at exit; ports pay for packets in steps). The footprint is
the backlog row 'Desktop memory footprint'. An earlier commit of this branch lowered the reserve to 1/64; it is undone.
The tags `candidate/work-q1` and `candidate/work-q1-b` are published and stay.

## Q2: processes, the fork bomb and bounded kernel work (branch `work/q2`)

### Built

- **errno by field** (ADR-0263 item 4, INV-0263-3). `zoraya_rt::last_refusal()` returns the `QuotaField` that the kernel
  put in the value register with the last `OverQuota` (the runtime keeps it in one word; the stubs are unchanged).
  `zoraya_posix::errno::from_refusal`/`from_status_seen` map memory to `ENOMEM` and processes to `EAGAIN` by call kind
  (`Call::Process` is new); `fork`, `posix_spawn` and the memory calls use it. Host test `quota_refusals_map_by_field_and_call`.
- **The `processes` field** was already charged first in `spawn` and `snapshot_process` (Q1). Tests: `t_quota` (a
  refused `spawn` leaves no event source, frame or heap byte: review Q1 error 1; the fork bomb; a foreign object mapped
  at the limit is `OverQuota` with no effect: review Q1 error 2) and `t_snapshot` (a snapshot refused at the process
  limit and at the memory limit changes no counter, frame, heap byte, event source or handle).
- **Steps** (`kernel/src/steps.rs`): `STEP` = 64; `between()` (unmasks, preempts if the slice ended, fails with
  `Cancelled` if the process was asked to end) and `pause()` (the same, never fails: for frees and unmaps). Sites:
  `memory_create` (`MemoryObject::make`, also `new_for_copy`), `memory_map` (reserve, map in steps under the space lock
  one step at a time, commit), `memory_unmap` (steps then commit), `snapshot_process` (`snapshot_plan`, `SnapshotBuild`,
  `snapshot_step` with a resumable page-table walk `PageTable::for_each_mapping_from`, `finish` in steps), `spawn`'s
  segment copy and stack, and `channel::close`. `exit_current` closes the handles before it clears the user table, since
  a step may yield.
- **The `reaper`** kernel thread (`kernel/src/reaper.rs`): the frames of a memory object over 64 pages and of an address
  space over 64 frames are queued with their `Paid`, so the charge stays until they are freed; the reaper frees 1024
  frames a turn in steps of 64 under the lock and yields. `frames::alloc` frees a step of the queue before it reports
  no memory. The queue is allocated at start and never grows (a drop must not make the heap take frames).
- **The masked-time counter** (test kernel): hooks in `save_and_mask_*`, `restore_interrupts`, `unmask_*`, the exception
  entry and exit; the idle thread's `wfi` is not counted; `MASKED_MAX_NS` test call; `sync::held()` and an assertion
  that no spinlock is held at a step point (INV-0270-2).
- **Tests**: `t_bound` (new; INV-0270-1 and -2: each site against its one-step case, `job_kill` during a loop of large
  `memory_create` and of large `snapshot_process`), `t_quota`, `t_snapshot`, `tests/posix/scenarios.script` (the
  fork bomb under the session job ends in `EAGAIN`), a `pagetable` host test of the resumable walk.
  `tools/untrusted-gate.txt`: `fork-bomb` and `masked-time` are `test-qemu`.

### Decisions where the ADRs are silent

- The reaper frees 1024 frames a turn, not 64: it runs with interrupts on, so the bound is untouched, and 64 made it
  slower than the test that starts 255 processes (it ran out of frames). Recorded in ADR-0270's open questions.
- The reaper only frees frames and the charge; `schedule()`'s `reap()` still drops a dead thread and its remainder
  (kernel stack, record): bounded, small, and it cannot be moved without a wake from inside `schedule()`. Open item.
- `snapshot_process` copies a mapped object that another process maps in steps: a concurrent writer is seen part way.
  Pre-charging the caller's own pages (review Q1, note 14) is not done; a refusal still leaves no effect.
- `drain()` and the `settled_stats` functions exist only in the test kernel; tests that compare counts wait for the
  reaper, as INV-0270-2 says ("once the reaper is idle").
- The fork bomb is `t_quota` (native: two copies each, the shape of `:|:&`), not a shell script: the posix session job
  holds the shell and the broker, so there is no command "outside" the bomb's job until rooms exist (Q6a). The posix
  half is the existing `t_posix forkbomb`, which now checks the `EAGAIN` text.
- Not done: `dma_pin` is not stepped (drivers only); the program per row of INV-0263-3 in a limited room (needs Q6a).

### Invariants delta

INV-0263-3 checked (table by host test; `fork` row by `t_posix forkbomb`; the per-row program in a room is open).
INV-0270-1 and INV-0270-2 checked by `t_bound`. INV-0261-2 and INV-0261-4 gained their `spawn`, `snapshot_process` and
fork-bomb halves (the rows say what stays open). `make check-untrusted-gate` still passes: stage 1 has no `pending`
line for Q2 now.

### Measurements (HVF, default guest, test kernel, `t_bound`; 24 MiB objects)

| Site | Before (one call) | After: one step | After: whole |
|---|---|---|---|
| `memory_create` | 1707 us | 19 us | 19 us |
| `memory_map` | 1333 us | 15 us | 19 us |
| `memory_unmap` | 786 us | 8 us | 10 us |
| close of the object | 962 us | 10 us | 11 us (the reaper frees them) |
| `snapshot_process` | 0.4 ms for 2.2 MiB (design.md) | 157 us (the fixed work) | 91 us (24 MiB) |
| close of a 10,000 chain | about 11 ms for 20,000 (design.md) | 78 us (64 messages) | 91 us |
| exit of a process holding 24 MiB / `job_kill` of one | not measured before | | 91 us / 31 us |

The fork bomb (`t_quota`): a limit of 24 processes was reached in 5 to 6 ms with 23 or 24 refusals, and a command started
outside the limited job ended in under 1 ms. The "before" for `memory_create` to `close` was measured in this session
on the commit that had the counter but no steps; the others come from `docs/design.md`'s earlier figures (the first
`t_bound` run failed before them).

### Review fixes (after `docs/tasks/TASK-26-review-q2.md`)

Done: the teardown of many small objects is counted across the whole exit (`steps::before`: the exit's handle loop, its
mappings, `channel::close`), the reaper's queue doubles instead of freeing whole, the reaper takes contiguous runs and
yields after each emptied item; `memory_create(CONTIGUOUS)` is capped at 4 MiB (users: `fsd` 2 MiB, drivers' queues,
`t_devices`, `t_loop`), zeroed and freed in steps, and its search makes at most 17 rounds; `t_bound` asserts the INV-0270-1
bound of 2 ms scaled by a calibration (1 under HVF), the units of work per masked interval on every host, an exit and a
`job_kill` of 400 objects of 256 KiB and of 256 mappings with 1024 handles, a contiguous case and its refusal, and that
a kill ended a call between steps; the warnings 4 to 11 and the notes 18 and the CI timing failure (below).

The CI failure of the first candidate run (`memory_create: 2756496 ns masked, limit 2000000`, 14 to 17 ms elsewhere) was
a time bound on hosts that are not HVF. The 2 ms bound is `[assumption]` under HVF. It is now checked as ADR-0270 open
questions say: the work between two step points is counted (at most 64 + 16 units, on any host) and the time is scaled
by a calibration printed in the test's output. I could not reproduce the TCG figures locally (TCG runs are skipped);
the CI run of this head tells whether the factor is enough. The test that still fails there would be a measure of the
scan below.

Measured under HVF after the fixes (`t_bound`, 128 MiB guest, calibration factor 1): exit of a process holding 24 MiB
97 us masked, `job_kill` 14 us; exit of 400 objects of 256 KiB 102 us, kill 28 us; exit of 256 mappings and 1024
handles 104 us, kill 60 us (at most 65 units of work in any interval); a contiguous 4 MiB create 382 us, of which about
250 us is the frame-map scan; a kill landed between steps in the create, snapshot and contiguous loops. The teardown
assertion is the 2 ms bound (it was 4 ms), and the figures are well inside it.

Not fixed, and why:
- The scan of the frame map for a contiguous run is one masked pass that costs about 250 us on the 128 MiB guest and
  grows with the machine's memory, not with the request (ADR-0270 open question). A resumable scan was not built.
- The right for `CONTIGUOUS` (ADR-0051) is not checked; the cap bounds the job without it.
- Drops that free on the spot when the heap cannot grow the reaper's queue (exhaustion) stay masked.
- Review notes 12 (the charge outlives frames that `help()` frees), 13 (`SnapshotBuild` assumes the mappings did not
  change), 14 (`walk_from` at the top of the address space and with block leaves), 15 (`LAST_REFUSED` and steps), 16
  (the `fork` row's POSIX script cannot fail on the field), 17 (stack headroom for an IRQ in a call): open, as recorded
  in the review; none changes behaviour today.
- Warning 11's second half (a refusal in `copy_page` part way through a snapshot) has no test; the row says so.
- Warning 10: the deviations from ADR-0270 point 4 stay open-question notes, stated plainly in the ADR; no ADR was
  proposed because the architect reserves the numbers.
- The `boot-tcg-smp4-gicv2` "timed out waiting for bytes dropped" of the candidate run is the known input flake.

### Tests run

`make check` before every commit. After the review fixes, `make test-changed` ran four times; the last, on the head of
the branch, passed all nine steps (the three before failed `test-qemu` on `t_bound`'s new cases, each fixed in its own
commit). Earlier: `make test-changed` (every step, because the Makefile and `zoraya-rt` changed; HVF; TCG
runs skipped locally) was run ten times while the code was built; the last run on the head of the branch passed all nine
steps (`test-boot`, `test-qemu`, `test-libposix`, `test-files`, `test-crash`, `test-posix`, `test-posix-gui`,
`test-tour-gui`, `test-gui`). One earlier run failed `boot-auto-smp4-2g` (the shell's "bytes dropped" line, a timing
test) and passed unchanged on the next; it is probably a flake and the nightly run will say. `FULL=1` (TCG, crash rounds)
was not run: CI does it.

### Handoff

- State: branch `work/q2` from `main` at `9daf721`, local only. Run `make test-changed` (every step: the Makefile and
  `zoraya-rt` changed). A `candidate/work-q2` run is the architect's.
- Review: the independent reviewer should read `kernel/src/steps.rs`, `reaper.rs`, `memory/object.rs`, `memory/user.rs`
  (`SnapshotBuild`, the `map_*`/`unmap_*` parts, `Drop`), `syscall.rs` (`memory_map`, `memory_unmap`,
  `snapshot_process`), `process.rs` (`exit_current`, `create`), `channel.rs` (`close`), `memory/frames.rs` (`alloc`),
  `sched.rs` (idle), `sync.rs`, `crates/zoraya-pagetable` (`for_each_mapping_from`), `crates/zoraya-posix/src/errno.rs`,
  `crates/zoraya-rt/src/lib.rs`, and the tests `t_bound`, `t_quota`, `t_snapshot`.
- Risks to look at: a step point reached with a spinlock held in the normal kernel (only the test kernel asserts it);
  `exit_current`'s new order; `frames::alloc`'s retry under the heap's lock (it only frees frames, drops nothing);
  the stale `LAST_REFUSAL` and `rt` word on SMP (Q1 note 10).

### Handoff rule: the nightly run (infrastructure part, 2026-11-11)

CI no longer runs every job on a merge candidate: a candidate runs the steps its change touches and a smoke set, and
the full set (all of `tools/ci-tests.txt`) runs nightly on `main` (02:17 UTC), on demand and on a `candidate-full/NAME`
tag. A failure that only the full set shows is therefore found by the nightly run. **A red nightly run is the first
item of the next session**, before any new work. Where to look: `gh run list --workflow 'make test' --event schedule
--limit 5`, then `gh run view RUNID --json jobs` for the failing jobs and `log-triage` on the log of the first. Read
it at the start of every session; do not infer it.

### CI: test-gui split (2026-11-11)

The single `test-gui` job took about 1257 s on CI (run 42987295524: 20 runs, 1194 s of runs plus the build). It is now
four jobs, one `make test-gui GUI_RUNS="..."` line each in `tools/ci-tests.txt`, balanced by the per-run times of that
run (job log; the VM boot is inside each run's time). The map, `tools/ci-plan.sh` and `tools/plan-lib.sh` are unchanged:
a plan step `test-gui` selects every line whose target is `test-gui`, so a change that selects it runs all four groups,
and the smoke set's `terminal:auto:1` run is added only when no group is selected. `make test-gui`, `make test`,
`test-par` and `test-changed` still run every run of `GUI_RUNS_ALL` in one go.

| Group | Runs | Estimated runs time |
|---|---|---|
| 1 | terminal:auto:2 terminal:tcg:2 windows:auto:2 | 312 s |
| 2 | terminal:auto:1920x1200 files-manage:auto:2 terminal:auto:1600x1000 | 297 s |
| 3 | rooms, files, files-manage at scale 1, auto and tcg | 303 s |
| 4 | terminal, windows, layouts, settings at scale 1, auto and tcg | 282 s |

Add the job's own setup (artifact download, `tools-storage`, image build): the guess is 5 to 6 minutes a job.
`make check-ci-jobs` now fails unless the runs of all `test-gui` lines equal `GUI_RUNS_ALL` as a multiset (a run in no
line, in two lines, or not in `GUI_RUNS_ALL`, or a line without `GUI_RUNS`). Checked by breaking copies of the list.

To verify on the candidate run (a change touching the compositor's map entry): four `test-gui` jobs, each green and
at or under about 6 minutes; the wall time of the run against the previous 27 minutes for such a change; the four jobs'
`[gui-...] PASS` lines together name all 20 runs.

## Status after Q2 (orchestrator, 2026-11-11)

- **Merged and tagged:** Q1 (`stage-8-q1`, main `8af2588`), the CI parts (`work/ci`, `work/ci-gui`, merged without a stage
  tag), the files manager's release fix and the files-manage script guard (`work/gui-flake`), and Q2 (`stage-8-q2`, main
  `0e6164a`, full CI green on main after one re-run of `w9-term` tcg, which hit its 10-minute job timeout once: attempt 2
  took 3 min 39 s against 7 to 8 min earlier, so a runner stall).
- **Flakes found and fixed on the way:** `files-manage:auto:2` step 24 was a real app bug (a release re-applied the click
  with the modifiers read at release time); its step 113 `Exit` timeout was a script race (keys typed before the focus
  click took effect on a slow guest). Frame timings across runs vary 2x between runners; no Q2 slowdown is shown.
- **Candidate CI** now runs what the change touches: a kernel change about 10-16 min, a compositor change about 15 min
  (was about 27), the full set about 13-16 min (49 jobs, `test-gui` split in four). Not measured: a docs-only candidate.
- **Open for the architect:** the kernel reserve (1/16 kept; the desktop uses ~96% of 256 MiB, GUI guests now 512 MiB;
  backlog row 'Desktop memory footprint'); ADR-0270 point 4 deviations (the `reaper` frees 1024 frames a turn, `reap()` in
  `schedule()` not moved; the time bound is an assumption under HVF, checked on CI by a calibration factor and a unit
  count); `init` holds `LIMIT` on its own job; the panel's launches are outside the session job until Q6a.
- **Cost by model:** no counter is available to the session; the work was done by Sonnet subagents (Q1 fixes, CI parts,
  Q2 build and fixes, the flake hunt), Opus for the Q1 and Q2 reviews and the Exit diagnosis, Haiku (`ci-watch`,
  `log-triage`, `test-runner`) for CI and test runs.
- **Next:** N1 (the driver answers ARP) on `work/n1` from main, then N2, Q5, Q3 (fsd), Q4, N3, N4 with Q3's netd account,
  N5, Q6a, Q6b, N6, Q7. Gate: closed (`make check-untrusted-gate`; fork-bomb and masked-time rows now run in `test-qemu`).

## N1: the driver answers ARP (branch `work/n1`)

### Built

- `crates/zoraya-virtio-net`: the virtio-net driver's logic over `zoraya-virtio`'s traits (`#![forbid(unsafe_code)]`):
  `VERSION_1` plus `MAC` and `STATUS` only, one receive and one transmit queue of 128 descriptors, two descriptors per
  buffer, the MAC from the configuration space, slot states, every device value checked (table below). Its module
  `serve` is the client-facing half (messages in and out over a `Host` trait; what a full channel refused is kept as
  bounded state and written again). 34 host tests against the fake device and a fake channel, with a 200-seed random
  mix of requests and lies.
- `crates/zoraya-proto-nic`: the frame protocol (`Info`, `Attach`, `Transmit`, `Release`; pushed `Received`, `Sent`,
  `LinkChanged`, and `Rejected` with a reason for a refused transmit) and `arp`, a bounds-checked ARP parser/builder/answerer; 12 host tests (truncations, wrong hardware or
  protocol type or sizes, operations, multicast sender, total decoding of every message).
- `user/src/bin/ethd.rs` (the process), `t_net.rs` (plays `netd` against the real device), `init` starts `ethd` when a
  virtio-net device is alone on its page (`gui::scan` gained `net`), the kernel prints `ID_AA64ISAR0_EL1`
  (`kernel/src/main.rs`), a kernel test `network_driver_answers_arp`.
- `tools/run-qemu.sh --net`, `--net-pcap FILE`, `--net-capture-all` (`--net` with `--gui` refused); `make test-net`
  (a step of `make test`, `tools/ci-tests.txt`, a lane of `tools/test-par.sh`) and `make run-net`.
- `fuzz/fuzz_targets/net_rx.rs` (ARP parser, a hostile device, and the client half: arbitrary messages to `serve::Server`
  over a channel that is sometimes full). Not run under libFuzzer: no nightly or `cargo-fuzz` on this host. Its body was
  compiled and driven with 20000 random inputs in a scratch crate outside the repo (no panic).
- Docs: `docs/design.md` "Network driver" (what exists, measurements, the provider check), ADR-0221's open-question note
  (rule 6).

### Decisions where the ADRs are silent

- Proposed ADR-0280 (slot layout; recommendation: accept Option B, header in the driver's queue memory, so a slot is a
  bare frame and the driver never reads frame bytes). Inside it: `Received.at` (wake time), `Rejected` (with a reason)
  reports refused transmits and `Sent` only finished ones, nothing pushed is lost on a full channel (bounded state, not a
  queue), one contiguous object, any undecodable message ends the driver.
- The plan says the runner sends the ARP who-has through SLIRP. SLIRP asks the guest for its MAC only when the host
  starts a connection to it and does not know it yet. So `t_net` sends the who-has for the gateway itself, and the reply
  arrives through the receive path. For the acceptance (the guest answers an ARP request for `10.0.2.15`), `make
  test-net` runs QEMU with `--net-forward 47015` (a `hostfwd` to the guest) while `tools/net-poke.sh` connects to that
  port every quarter second; SLIRP then asks who has `10.0.2.15`, `t_net` answers by `arp::answer` through a transmit
  slot, and the step greps the pcap for both the gateway's reply and the guest's. Try 1 showed IP frames only: SLIRP
  had learned the guest's MAC from `t_net`'s own who-has, which had sender `10.0.2.15`. Try 2, with sender `10.0.2.30`,
  works (HVF: 1 request answered).
- No new crate dependency: `docs/dependencies.md` unchanged.
- The provider check is recorded in `docs/design.md`, not in `docs/research/network/survey.md` (CLAUDE.md forbids edits to
  `docs/research/`).

### Invariants delta

INV-0221-2: the driver half is checked (`cargo test -p zoraya-virtio-net -p zoraya-proto-nic`); the row stays `not yet
applicable (stage 8)` for `netd`'s half (note in ADR-0221's open questions). New INV-0280-1 (proposed ADR): `checked:
make check-ethd-maps` (a source check, part of `make check` and CI; the kernel cannot list a process's mappings, so
there is no check in QEMU). INV-0230-1 and -3 are partly covered (`restrict=on` under `--test`; the step skips without
user-mode networking, listed by `make test`'s summary, and fails on CI) but their checks (printed command line assertion, fake `qemu` runner test) are not written: still `not yet
applicable (stage 8)`.

### Where a device-controlled number enters, and where it is checked (for the reviewer)

| Number | Entry | Check |
|---|---|---|
| used index | `Queue::take`, `zoraya-virtio/src/queue.rs` | `new > outstanding` is `Error::Device` |
| used entry id | same | `>= size` or not offered is `Device`; marks it not offered, so a repeat is `Device` |
| used length, receive | `Net::poll` | below 12 or above 12 + 1514 is `Device`; 12 to 25 is a runt, dropped and reposted |
| used head, both queues | `Net::poll` | odd, `slot >= 64` or slot not in the matching state is `Device` |
| used length, transmit | `Net::poll` | ignored (not trusted, not used) |
| feature bits | `queue::begin` | agreed = device & wanted; `MAC` missing is `Unusable` |
| queue size | `Queue::set_up` | `QUEUE_NUM_MAX < 128` is `Error::Queue` |
| MAC, link | `Net::new`, `read_link` | zero or multicast MAC is `Unusable`; link is one bit |
| client slot, length | `Net::release`, `transmit` | slot range, state, length 14..=1514 before any index; refused is counted |
| client attach | `Server::attach`, `Net::attach` | one pinned run of at least 196,608 bytes, `checked_add` on the address |
| client message | `Server::handle` (`serve.rs`) | `Header::decode` and `decode` are total; sequences bounded at 64; unknown ordinal ends the driver |
| frame bytes | none in `ethd` | the driver reads no slot; `t_net` parses with `arp::parse` (checked slices) |

Buffers under DMA: the queue memory and the client's object are pinned (`dma_pin`) before the device sees an address, and
are unpinned only after `Net::reset()` returned true (the device is stopped, `Ethd::stop`). The client's handle is closed
after the pin; the pin keeps the frames. The slot buffers are posted once per slot and re-posted only after the client's
`release` (or at once if the client could not be told), tracked per slot (`Posted`/`Lent`, `Free`/`Queued`), so the
device never holds a slot the client believes it owns, except the client's own transmit slots, which it must not
rewrite until `Sent` (the client's frames are its own data). If `ethd` is killed with buffers in flight, the pins'
last handles close without `dma_unpin`: the frames stay quarantined (ADR-0054, INV-0054-3 and -7). The kernel's
`dma_release_quarantine` resets the device and releases them (`release_quarantine` in `kernel/src/device.rs`, ADR-0070);
the next `ethd` calls it first, before `device_map`, as `blkd` and `inputd` do (not `Net::new`, whose `begin` also
resets, but later). `t_net` now closes the channel and waits for `ethd` to end by itself (exit code 0: reset and both
unpinned), then calls `dma_release_quarantine` and expects nothing to release. Not tested: a kill with frames in flight
(open item for N2, with INV-0221-3).

### Tests run

- `cargo test -p zoraya-proto-nic -p zoraya-virtio-net`: 12 and 34 passed (after the review fixes).
- `make check`: all 14 steps pass (fmt, clippy, adr-check, the `check-*` including `check-ethd-maps`, `check-ci-jobs`).
- `make test-net` (HVF), after the review fixes: PASS, 104 passed and 9 expected failures of the test kernel; `t_net`:
  the gateway's ARP reply, 1 request for `10.0.2.15` answered, `ethd` ended by itself (reset, unpinned, exit 0),
  `dma_release_quarantine` found nothing; `zoraya-fsck` clean; the pcap holds both replies. `FULL=1 make test-net`
  (HVF then TCG): both PASS, same lines.
- `make test-changed` after the review fixes (HVF, local; the Makefile changed, so every step but `test-net`): all pass
  (`test-boot`, `test-qemu`, `test-libposix`, `test-files`, `test-crash`, `test-posix`, `test-posix-gui`, `test-tour-gui`,
  `test-gui`; 404 s). The `t_bound` failure of `auto-smp4-2g` that the first run showed (also on `main`) did not occur in
  this run: it is a flake of Q2's reaper, not N1's, and is not fixed here.
- Not run: the fuzz target under libFuzzer; `make test-boot` with `--net` (the normal kernel starts `ethd` under `make run-net`, which no
  step boots).

### Measurements (`t_net`, 20 rounds, a single run each)

| | HVF | TCG |
|---|---|---|
| ARP round trip (guest transmit to the reply read), median / min / max | 42 / 40 / 258 us | 384 / 372 / 1309 us |
| the driver's wake to the message read in the client, median / max | 7 to 8 / 11 us | 190 / 794 us |
| `ID_AA64ISAR0_EL1` | `0x0221100110212120` (RNDR 0) | `0x0000000000011120` (RNDR 0) |

SLIRP's ARP behaviour under `restrict=on`: it answers the gateway's who-has for `10.0.2.2` with MAC `52:55:0a:00:02:02`
at once; it sent no request of its own.

### The provider check (ADR-0228)

ring works: `rustls 0.23.46` (`default-features = false`, `ring`, `tls12`) with `ring 0.17.14` builds for
`aarch64-unknown-none` with `clang` and musl's headers from `build/sysroot` as `-isystem`, ring's feature
`less-safe-getrandom-custom-or-rdrand`, and `getrandom 0.2` (`custom`) with a registered function. `cargo build
--release` produced ring's C objects and rlibs. Recommendation: keep ring for native programs as ADR-0228 says; N5
decides the randomness source and runs it on the target. No dependency row was added (nothing links ring yet).

### Handoff

- State: branch `work/n1` from `main` at `9a74448`, local only.
- By hand: `make run-net` (interactive, with Internet through SLIRP; the console prints `ethd: virtio-net 52:54:00:12:34:56`
  and `ID_AA64ISAR0_EL1 ...` at boot); `make test-net` (add `FULL=1` for TCG); `target/test-logs/test-net.pcap` opens in
  Wireshark.
- Review: read `crates/zoraya-virtio-net/src/lib.rs` and `tests.rs`, `crates/zoraya-proto-nic/src/lib.rs`,
  `crates/zoraya-virtio/src/queue.rs` (`Queue::take`, `begin`), `user/src/bin/ethd.rs`, `user/src/bin/t_net.rs`,
  `tools/run-qemu.sh`, `docs/adr/0280-the-frame-slot-layout.md`, the `Makefile` `test-net` recipe, `fuzz/fuzz_targets/net_rx.rs`.
- Open for N2: `netd` must create the object `CONTIGUOUS`; supervise `ethd` (`init` does not); the
  kill-with-frames-in-flight test; INV-0230-1/-3 checks; `fuzz/Cargo.lock` is not updated (the fuzz workspace was not
  built; cargo regenerates it on the first run).

## N2: `netd` answers ping and DHCP (branch `work/n2`, on `work/integ`)

### Built

- `crates/zoraya-abi/src/net.rs`: `Scope` (reach and listen rules; names, wildcards, addresses, prefixes, `any`; port
  ranges; TCP and UDP) with a total decoder, `encode`, the matcher and `within` (the subset relation `narrow` needs); 11 host
  tests (`net_tests.rs`: round trip, every proper prefix refused, mutations, junk, mixed case, names that look like addresses,
  a 4000-pair subset property). The start-up record gains the `net` slot (ADR-0220 item 7).
- `crates/zoraya-proto-net` (`status`, `ping`, `narrow`, `net status`'s text; 4 tests) and `crates/zoraya-net` (39 tests,
  heap-capped): smoltcp over copied frames (`link`), the stack with DHCP, the echo run and `status` (`stack`), the slot
  bookkeeping against the driver (`nic`), scope and rights per connection (`connection`), the requests (`service`), the `net.*`
  settings (`config`). Tests: a scripted DHCP server and echo responder (lease, renewal changing `status` with no write, silence
  and retry, static, unconfigured), 6000 hostile frames, a lying link, a flood in bursts of 64, a hostile driver (bad slots and
  lengths, a slot offered twice, `Sent` for slots not in flight, 20,000 random messages), every request kind and refusal, 20,000
  random requests, the settings.
- `zoraya-describe`: setting types `address` and `prefix` (ADR-0229), key `needs net-status` (ADR-0224 item 2).
- `user/src/bin/netd.rs`, `net.rs`, `t_netd.rs`; `user/src/netclient.rs` (calls on a connection; `start_network`, shared by `init` and
  `t_netd`); `init` starts `ethd` and `netd` after the storage stack, makes the shell's root network connection (slot 9) with `narrow`
  and restarts both at most three times per boot; `zosh` gives a `needs net-status` command a `status`-only connection in the `net` slot.
  `ethd` and `netd` print what their start-up table holds (INV-0221-1).
- `tests/shell/net.script` (the normal kernel with `--net`, note 16 of the N1 review); `conventions.script` and `docs/tour.md`
  (the `help` listing) updated for the `net` command. `make test-net` runs `t_net`, `t_netd` and the normal-boot script
  (TCG too with `FULL=1`), and checks the pcap for the ARP replies, a DHCP exchange and an echo request and its reply.
- `fuzz/fuzz_targets/net_stack.rs` (scope decoder, frames and DHCP replies into the stack, driver messages into `Nic`,
  requests into `Service`, settings). Not run under libFuzzer (no nightly). Its body was compiled in a scratch crate outside the
  repository and driven with 200,000 random and near-valid inputs, release and debug: no panic. `fuzz/Cargo.lock` not updated.
- Docs: `docs/design.md` "Network service", `docs/dependencies.md`, ADR-0221's open questions, proposed ADR-0281.

### Decisions where the ADRs are silent (proposed ADR-0281; recommendations)

1. **smoltcp as a registry crate, pinned `=0.14.0`.** Accept for now; vendor when the TLS provider brings the next registry
   crates or the CI cache is a cost. A deviation from CLAUDE.md's "make test must not need network once the toolchain is
   installed" (the first build runs `cargo fetch`): **read this first.**
2. **The scope's bytes** and a conservative subset rule (a child inside one parent rule; IPv4; canonical prefixes). Accept.
3. **A diagnostic `ping` request** behind `reach` and a rule covering the address; smoltcp's own echo reply off. Accept.
4. **Setting types `address` and `prefix`.** Accept.
5. **`init` restarts `ethd` and `netd` together (three times); a running shell is not told.** Accept for N2; N3 builds ADR-0221 item 6's channel.
6. **`netd` reads `/config` once**, no reload. Accept; a `reload` request is small.
7. **Under `restrict=on` the lease has no router and no DNS server** (a SLIRP fact); ADR-0230 stays.

### Invariants delta

`checked`: INV-0220-3 and -4 (`cargo test -p zoraya-abi`), INV-0221-1 (`make test-net`; a report each process makes about its
own start-up table, since no call lists another process's handles), INV-0221-2 (both halves), INV-0229-1 (`cargo test -p
zoraya-describe -p zoraya-net`), INV-0229-3 (`cargo test -p zoraya-net`; `Stack` takes settings as bytes and `netd` closes its
read-only `/config` connection after reading). New INV-0281-1 (checked). Partly covered, rows unchanged: INV-0221-3 (native half in
`t_netd`; the `libposix` half needs sockets), INV-0221-4 (code in `init`; the test needs a fetch between kills, N3), INV-0230-1 and
-3 (N1's note stands). adr-check total 504.

### Where a device- or network-controlled number enters, and where it is checked

| Number | Entry | Check |
|---|---|---|
| frame bytes | `Nic::recv`, `LinkDevice::receive` | copied to a private buffer before smoltcp parses; length at most `MAX_FRAME` and the buffer, else dropped and counted |
| received slot, length | `Nic::arrive` | slot below 64, state `Device`, length 14..=1514; bad length gives the slot back; a slot offered twice or out of range is counted; `Frames::read_slot` (`netd.rs`) re-checks slot below 128 and length at most 1536 before indexing |
| `Sent` / `Rejected` slot | `Nic::finish` | slot in 64..128 and in flight, else a fault; `STILL_QUEUED` does not free it |
| driver message | `Nic::handle` | total decoders; an unknown ordinal or an undecodable message closes the driver (`netd` exits, `init` restarts) |
| MAC, link | `netd.rs` main, `Stack::new` | status OK and length 6; unicast and non-zero before the interface exists |
| DHCP reply | smoltcp `dhcpv4`, `Stack::dhcp_events` | smoltcp: valid mask, unicast `your_ip`; here: unicast address, prefix at most 30, unicast router; lease time via `DhcpRepr::parse` (an `Option`) |
| lease left | `Stack::status` | `i64`, clamped to `0..u32::MAX - 1` |
| echo reply | `Stack::ping_step` | `new_checked`, `Repr::parse` with checksums, identifier, source equal to target, sequence below those sent, first reply per sequence; round trip clamped to `u32` |
| request | `Service::handle` | `Header::decode`, `decode::<M>` with the handle count; unknown ordinal gets `ErrorReply`; malformed closes that connection |
| scope, rights | `Connection::decide`, `Scope::decode` | total decoder (counts, lengths, UTF-8, names, ports, protocols, trailing bytes); unknown right bits refused; child within parent |
| echo count, timeout | `Below<17>`, `Below<30001>`; `Stack::ping` | decoder refuses larger; ping refuses 0 and over 64 |
| settings file | `config::read` | `zoraya_settings::read` (total), `parse_address`, `parse_prefix`, `valid_name`; static needs unicast address, prefix at most 30, on-link gateway |
| `Narrow` handle | `netd.rs` `serve` | exactly one, of type channel, else the connection is closed |

### Tests run

- `cargo test -p zoraya-abi -p zoraya-describe -p zoraya-proto-net -p zoraya-net`: 66, 23, 4, 39 passed.
- `make check`: all 14 steps pass.
- `make test-changed` (HVF, every step: `user/` and the Makefile changed): test-boot, test-qemu, test-libposix, test-files, test-crash,
  test-posix-gui, test-gui, test-net passed; test-posix and test-tour-gui failed once because the tour's `help` listing lacked
  `net` (fixed in `docs/tour.md`), then passed alone (`make test-tour-gui`: 914 steps; `make test-posix`).
- `FULL=1 make test-net` (HVF then TCG): PASS, `t_net`, `t_netd` and the boot script under both.
- Not run: the fuzz target under libFuzzer. `make test-changed` needed `make fetch-ports` in this worktree.

### Measurements (`t_netd`, single runs)

| | HVF | TCG |
|---|---|---|
| lease after `netd` starts | 110 ms | 114 ms |
| `ping 10.0.2.2` x4, round trip min / avg / max | 199 / 399 / 538 us | 3492 / 4321 / 4853 us |
| restart after `netd` killed, to a lease | 124 ms | 180 ms |
| restart after `ethd` killed (quarantine released), to a lease | 126 ms | 183 ms |
| `netd` killed with a ping in flight: client closed and `ethd` exited | 302 ms | 324 ms |

The kill rows include a 300 ms wait before the kill. SLIRP's lease: 24 h; no router or DNS under `restrict=on` (the tests).

### Handoff

- State: branch `work/n2` from `work/integ` (`7a39a57`), local only, not merged.
- By hand: `make run-net` (console shows `ethd:`, `netd:` and the lease; at the prompt `net status`; try `settings net.gateway 10.0.2`
  which is refused); `make test-net` (`FULL=1` for TCG); `target/test-logs/test-net.pcap` opens in Wireshark.
- For the reviewer: `crates/zoraya-abi/src/net.rs`, `crates/zoraya-net/src/{nic,stack,link,service,connection,config}.rs`,
  `user/src/bin/netd.rs` (the unsafe in `Frames`), `init.rs`, `zosh.rs` (`table`), `netclient.rs`,
  `crates/zoraya-describe/src/lib.rs`, `docs/adr/0281-*.md`, the `Makefile` `test-net` recipe, `fuzz/fuzz_targets/net_stack.rs`.
- Open: (1) registry versus vendoring; (2) the services channel: a running shell keeps a dead connection, shells the panel starts and the
  POSIX session have no `NET` slot; (3) the `libposix` half of INV-0221-3, INV-0221-4's test, INV-0230-1 and -3; (4) with `restrict=on`
  there is no gateway or DNS in the lease, so N3's resolver needs `net.dns` or the runner's forward; (5) no `reload`, no accounts
  (Q3's netd account); (6) the fuzz targets never ran under libFuzzer, `fuzz/Cargo.lock` is not updated; (7) the burst bound of 64 is
  untuned; (8) a CI runner without a crates.io cache downloads the registry crates in each job (the workflow was not touched), and the
  CI `test-net` job must build the normal kernel too now.

## Q4: the CPU ceiling (branch `work/q4`, on `work/n2`)

### Built

- **`crates/zoraya-cpu`** (new, `no_std`, host tests on a simulated clock): `CpuAcct` per job, `charge` (the run walks up
  every ancestor; a run that crosses a period boundary counts only its later part to the budget), `decide` (unlimited,
  run with a slice of `min(10 ms, budget left)` at least 50 us, or park), `set_limit` (1..=1000, not above the parent's
  effective ceiling) and `RunQueue` (ready list plus one parked list; `pick` parks, `release` returns 64 at a time).
  12 tests, among them INV-0264-1 and -4 over 2 s on the model and INV-0264-2.
- **Kernel** `cpu.rs` (job tree adapter, `JobRef`, the period with a test setter, `usage`), `job.rs` (`cpu` leaf lock;
  `set_limits` takes `cpu`, all or nothing with the others), `sched.rs` (charge at each switch, `pick`, slice armed also for a
  lone limited thread, `period_end` deadline, `arm_release`), `timer.rs` (two reserved slots), `syscall.rs` (usage
  record of 88 or 120 bytes; test call `SET_CPU_PERIOD` 0x7018).
- **ABI**: `JobUsage.cpu: CpuUsage {limit, used_ns, period_used, parked}`, `USAGE_SIZE_CPU` = 120; the decoder reads an
  88-byte record with the CPU words zero. `zoraya_rt::job_get_usage` asks for 120.
- **Tests**: `t_cpu` (new, run by the test kernel as `cpu_ceiling_from_a_process`): INV-0264-1, -4, -2 and the wake-up
  latency at 20, 10 and 100 ms; `t_quota` covers `cpu` in `job_set_limits` (range, parent, all or nothing).

### Decisions where the ADRs are silent (proposed ADR-0285; recommendations)

One parked list, not one per job; parking only in `pick`; a 50 us slice floor; at most 16 parks per pick; `cpu` 0 refused;
every ancestor charged; IRQ time charged to the interrupted thread; a killed job's parked threads end at the next
boundary; a 120-byte usage record; two reserved deadline slots; the period settable only in the test kernel. Numbers
0285..0289 were taken for Q4, 0285 used.

### Invariants delta

INV-0264-1, -2, -4 checked (`cargo test -p zoraya-cpu`, `make test-qemu`). INV-0264-3 stays "not yet applicable" until Q6a:
the GUI test needs `room new --cpu` and untrusted rooms (refused); its kernel half is in `t_cpu`. New INV-0285-1. The gate
file gets `cpu-ceiling test-qemu Q4`; `cpu-spinners` stays `pending`, owner Q6a. Open-question notes in ADR-0264 and ADR-0270.

### Measurements (HVF, default guest, test kernel, single runs)

See `docs/design.md`, "Quotas", Q4. 8 spinners at 25% for 2 s: 502 ms of 500 ms; three rooms at 25% under 50%: 1006 ms of
1000 ms; longest masked interval over the tests 314..1051 us (limit 2 ms), at most 69 units. Wake-up lateness (median / p99 /
max): 10 ms period 0.75 / 5.6 / 5.7 ms; 20 ms 0.68 / 9.3 / 9.6 ms; 100 ms 0.68 / 49.7 / 50.1 ms. TCG not run locally.

### Scheduler touch points (for the reviewer)

Lock order `SCHED` then the timer queue then a job's `cpu` lock (leaf). `schedule()`: charge walk (<= 32 nodes) and `pick`
(<= 16 parks, each a walk) with all exceptions masked; `period_end` (timer interrupt, SCHED held): <= 64 moves then an
immediate re-arm; `arm_release` under SCHED; `JobRef` holds a raw pointer with one SAFETY comment (the thread, and so its
process and the job chain, is held for the walk). `job.set_limits` reads and writes the `cpu` lock without SCHED: one CPU, no
kernel preemption (SMP would need SCHED there). `timer::arm` leaves 2 slots instead of 1.

### Not done

- The GUI test of INV-0264-3 (Q6a dependency, above): not written, not run.
- TCG and the `FULL=1` runs: CI's.
- No measurement of the per-switch walk on `portbench`/`chanbench`.

### Handoff

Branch `work/q4` on `work/n2`, unmerged. Review: `crates/zoraya-cpu/src/lib.rs`, `kernel/src/sched.rs`, `kernel/src/cpu.rs`,
`kernel/src/job.rs` (`set_limits`), `kernel/src/timer.rs`, `user/src/bin/t_cpu.rs`, ADR-0285. Commands: `cargo test -p zoraya-cpu`;
`make test-qemu-one` (prints the `t_cpu:` lines); `make test-changed`.

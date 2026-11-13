# TASK-26 report: stage 8, quotas and the network

Running report, updated after each part. Written by the Sonnet main session; reviews by `opus-review`.

## Status (orchestrator, 2026-11-13, main at `7de36fc`)

Stage 8 is built and merged: every part below is on `main` with its tag. **The untrusted gate stays closed** (`room new --untrusted` is
refused; `make check-untrusted-gate` passes with the gate closed and lists what is open when a marker is removed). There is no `stage-8`
tag yet: it waits for the architect and for the full runs.

| Part | Merge on main | Tag |
|---|---|---|
| Q1 | `8af2588` (`0a99422` first merge) | `stage-8-q1` |
| Q2 | `0e6164a` | `stage-8-q2` |
| N1 | `cad9f5a` (with `work/t-bound-fix`, `740e6a2`) | `stage-8-n1` |
| N2 | `4d4812c` | `stage-8-n2` |
| Q5 | `3995d03` (carried by the Q3 merge) | `stage-8-q5` |
| Q3 | `3995d03` | `stage-8-q3` |
| Q4 | `3f034e1` (`67e203d` plus the t_cpu masked-time fix) | `stage-8-q4` |
| N3 | `da771a0` | `stage-8-n3` |
| N4 | `a322e23` | `stage-8-n4` |
| N5 | `1be11a1` | `stage-8-n5` |
| Q6a | `1e9b1aa` | `stage-8-q6a` |
| Q6b | `e9c5368` | `stage-8-q6b` |
| N6 (partly done, see its section) | `cd58996` | `stage-8-n6` |
| Q7 | `4250211` | `stage-8-q7` |
| Review of the system calls and `work/q7-fixes` | `7de36fc` (no part tag) | - |

Follow-ups merged without a stage tag:

- `ci-timeout` (`d4b9477`): a 300 s deadline for the TCG and smp4 QEMU steps.
- `system-ro` (`0cbef2b`): `/system` read-only at run time, the architect's decision.
- `tbound-fix` (`d78bdea`): `t_bound`'s time bound off HVF.
- `units-fix` (`745669c`): bounded masked work in the frame allocator, the reaper and heap growth.
- `work/q7-fixes` (merged as `7de36fc`): the fixes of the system-call review and of its four review rounds, then the red candidate run's fixes
  (`0cc3863`, `29351b3`).

### The review of the system call surface

`docs/tasks/TASK-26-syscall-review.md` (opus-review, head `a3866f3`, ADR-0268 point 6): verdict, the gate stays closed. Two errors (INV-0270-1
did not hold for the job tree walks, nor for subscriptions), five warnings, seven notes. Both errors are fixed on `work/q7-fixes`
(walks in steps; subscriptions bounded to 1024 per port and per source, proposed ADR-0299), as are warnings 3 to 6; four review rounds of the
fixes were answered (section "Review of the system call surface after stage 8"). Open: warning 7 and notes 8 to 14, below.

### The gate

- Closed: `room new --untrusted` is refused in the shell and the panel; the `+net` refusal for untrusted rooms is marked (`net-refusal`,
  `net-refusal-call`), dead code until the gate opens. `make check-untrusted-gate` reads `tools/untrusted-gate.txt` and
  `docs/adr/invariants.md`, counts the markers and passes a row only if its status begins `checked:` and names nothing open.
- Rows of `tools/untrusted-gate.txt` with a step: charged-allocation, memory-hog, fork-bomb, masked-time, cpu-ceiling, cpu-spinners, disk-filler,
  service-floods, keeper, keeper-restart, keeper-reconnect, prompt-rebind, many-rooms, hostile-programs, fuzzing (stage 1); net-account-flood,
  net-fuzz (stage 2). **Pending:** `disk-filler-tmp` (an untrusted room's `/tmp`, owner Q8).
- Blockers still open (Q7 and review sections, `docs/backlog.md` "Gate blockers from the review of the system calls"):
  - Warning 7: the heap never returns frames while the charge is refunded; to measure, then a heap ceiling for the untrusted job, or return of
    free chunks, or the bound in ADR-0261's open questions.
  - Notes 8 to 14, not fixed: uncharged growth and the reaper's queue (8); `Paid` dropped under `EVENTS` holding the last `Arc<Job>` (9);
    `console_write` keeps IRQ masked about 60 ms at 4 KiB (10); code can run from a memory object (11); the refusal lives only in the clients
    (12); contiguous memory is open to every process (13); smaller items (14).
  - Left over from the fixes: `Job::drop`'s `retain` and the growth of `children` are linear in a parent's children, in one interval each.
  - Invariants not `checked:` that the strict check lists: INV-0172-3, INV-0260-1, INV-0261-2, -6, -7, INV-0262-1, -2, -3, INV-0263-3,
    INV-0264-3 (the architect's bound), INV-0265-1, INV-0265-3 (the `/tmp` half), INV-0265-4, INV-0268-3, INV-0270-1 (names known breaches);
    stage 2 also needs INV-0262-4 (a `netd` connection's rings charged to the client's job).
  - Not built: an untrusted room's `/tmp` and `tmpd`, `+net`/`+listen` words in rooms (N6 "Not done"), the untrusted line reader.
  - The architect's go-ahead, then a full run (`candidate-full/`) of the commit that removes the refusal, then `main`'s.

### Items for the architect

- **Proposed ADRs awaiting a decision:** ADR-0295 (register access by call for shared pages), ADR-0296 (the services channel and the restart
  test), ADR-0297 (the `+net` word in the shell), ADR-0298 (how the gate's tests run), ADR-0299 (subscriptions bounded to 1024; a change of
  ADR-0006's behaviour). ADRs 0287 to 0294 were accepted as recommended on 2026-11-12; the status of the earlier stage-8 proposals (0281, 0285, 0286) is in `docs/adr/index.md`.
- **INV-0264-3's GUI bound** (ADR-0298 item 4): +450 ms over the same run's no-spinner baseline, not 50 ms. The invariant is `not yet applicable`
  until the architect accepts it or asks for a lower bound; the kernel half is checked by `t_cpu`.
- **ADR-0006 INV-0006-3 against ADR-0299:** the check measures 10,000 idle subscriptions, which the bound of 1024 refuses. `portbench` now
  measures with 1 and 1024 and fails on a refused call; whether the check's number becomes 1024 is the architect's change (recorded in ADR-0006's
  open questions).
- **Toolchain pin and rustup:** fuzzing uses `nightly-2026-11-12` (script, workflow, ADR-0298 item 3). Installing it ran
  `rustup toolchain install`, which also updated rustup itself (1.29.2 to 1.29.3) on the host.
- **ADR-0270 open items:** the address-space frame list doubling still runs inline; the `memory_map` list copy under the lock costs N/512 units
  (a flat list); the 2 ms bound is an `[assumption]` under HVF, checked on CI by a calibration factor; ADR-0299's product bound (16 MiB moved)
  and its 1.5 ms extrapolation are not measured at the full size.
- **Decided (2026-11-12): N4's curl and wget are deferred** to the next stage ("living in the system": BearSSL or rustls-ffi as the C TLS
  library, then curl and wget). `zfetch` stands in. Not a blocker for stage 8 (`docs/backlog.md`).
- **Decided (2026-11-12):** `/system` is read-only at run time for everyone (ADR-0289's "Architect's decision"); `+w /system` is refused with a
  hint; a system-update path is a later ADR. The Q6a paths in QEMU (keeper restart, reconnect, prompt rebind) became gate rows, now built and
  passing in `test-keeper` (Q7).
- **Open from earlier parts:** the kernel reserve (1/16 kept, GUI guests 512 MiB); the 'Not done' lists of Q6b (rooms overview, `Raise`
  button, `top --session`, disk and connection use, refusal notices), N5 (BearSSL and curl, `getrandom`, `rngd` restart, `tls.roots`) and N6
  (rooms' `+net`, pills and hover card, `REPORT`, `+listen`).
- **CI infrastructure:** `QEMU_SLOW_TIMEOUT` is 300 s; the CI image is on ghcr (section 'CI image'); timing tests flake on loaded hosts. The red
  candidate run 43311917098 (`FORCE_SWITCH` under TCG; `portbench`) was fixed in `0cc3863` and `29351b3`.

### Known limitations and backlog

Recorded in `docs/design.md` (the network on the desktop and when it fails; two known limitations) and `docs/backlog.md`: `sleep 60 &` ending at once
in a dock-launched terminal (not reproduced); a failed `t_gui` leaves its GUI stack running, so one failure reads as five; the fuzz targets
reach few states of the keeper and the compositor (no structure-aware generator), and 600 s a target has not run in CI; the heap does not give
frames back (warning 7); INV-0262-4 and the `status` counters are not built; two checks of the review fixes are by reasoning only (the
front-to-back walk mutant, and the order of `write` and `prepare`); the `Vec::remove` mutant survives as equivalent. ADR-0270's own text
still says '16,384 frames' and 'power of two' for the mutant survivors; the true figures are in the units-fix section (Q6b), from `kernel/src/testing.rs`.

### What was and was not run

Each part's section lists its own runs. For the fixes: `make check`, `make test-qemu-one` (HVF and, for the red run's fix, TCG),
`cargo test -p zoraya-events`, the mutations named in the rounds. **Not run by the agents:** `make test-changed` on the fixes, the TCG runs
of most steps, `make test` and `make test-par`, `test-posix*`, `test-tour-gui`, `test-crash`, fuzzing at 600 s a target.
Candidate tags exist for every part (`candidate/work-*`); the newest on the fixes is `candidate/work-q7-fixes-f` at `29351b3`, whose result this report does not record.

- **Full run `candidate-full/stage-8` on `7de36fc`: success** (run 43327428941, 56 of 56 jobs; the plan job ran with `full=1`). `main`'s own push run on `7de36fc` (43323966568) also passed, 56 of 56. The candidate run of `work/q7-fixes` at `29351b3` (43320805132) passed too.
- `main`'s run on the commit that carries this line: see the `stage-8` tag, which is placed only after it is green.

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
## CI image

Why: on 2026-11-11 Ubuntu's mirrors were slow or unreachable for hours (a runner's apt fetched 1.4 MB in 8 minutes and
the `build` job was cancelled in its apt step). Now the jobs of `.github/workflows/test.yml` run in an image with
everything installed.

- **Contents** (`.github/ci/Dockerfile`, `ubuntu:24.04` by tag: no digest was readable offline): build-essential,
  clang, lld, llvm, zstd, `qemu-system-arm`, git, curl, patch, perl, python3, ruby, xz/bzip2/unzip/zip, gawk, and the
  Rust toolchain of `rust-toolchain.toml` (channel, components, targets) in `/usr/local/{rustup,cargo}`, on `PATH`.
  Jobs run as root; `safe.directory` is set system-wide.
- **Tag:** `tools/ci-image-tag.sh` = first 16 hex digits of SHA-256 of `.github/ci/Dockerfile`,
  `tools/ci-apt-install.sh` and `rust-toolchain.toml`. Image: `ghcr.io/<owner>/<repo>/ci:<tag>` (lowercase) and `:latest`.
  Both workflows call the script.
- **Rebuild:** `gh workflow run ci-image.yml --ref BRANCH` (always builds); it also runs on a push to `main` that
  changes those files, and on `candidate/**` and `candidate-full/**` tags (skipped when the tag exists).
- **Fallback chain:** test.yml's job `image` logs in to ghcr.io with the job token and runs `docker manifest inspect`;
  `ref` is empty on any failure. With a ref, `build`, `checks` and `tests` use `container:` (no apt, no rustup
  install). Without one, `container` is `null` (the bare runner) and the guarded steps install as before, through
  `tools/ci-apt-install.sh`: skip when clang and zstd exist; 3 attempts on the runner's mirror; then
  `mirrors.edge.kernel.org` and `mirror.math.princeton.edu` (noble, -updates, -security, x86_64 only); then the
  cached .debs without the network. The Dockerfile builds with the same script.
- **To verify on the first run** (not testable offline): (1) `ci-image.yml` builds and pushes (the push needs the
  repository's Actions workflow permissions to allow packages; the package is created private and linked to the
  repo); (2) `image` finds it (`image: ghcr.io/...` in its log); (3) `container: null` really is accepted when `ref`
  is empty (run test.yml once before the image exists, e.g. on this branch); (4) with the image, the three jobs
  start (the credentials are the job token, which needs `packages: read`; a fork's pull request has none and falls
  back); (5) in the container: `git ls-files`, `make`, `qemu-system-aarch64`, rust-cache, `actions/cache` (zstd)
  work; `check-host-paths` and `make check-ci-jobs` pass; (6) the ports build needs no tool the image lacks (the
  runner image has more packages than the container).
- **Build job time:** before, 3 min 10 s (green main run 42996307525, 2026-11-11, from the runner's own clang and
  zstd). After: to be read from the first run with the image (`gh run view RUN --json jobs`).
## Q5: storage limits (branch `work/q5`, on `work/integ`)

**Built.** `zoraya-fs` format version 2: a limit record `{ limit, used }` per directory (`K_LIMIT`) and a link from every
file and link to the directory it is charged to (`K_FPARENT`). `used` is kept by create, mkdir, symlink, write, `set_size`,
truncate, unlink, rmdir, rename (replace, no-replace, exchange), `create_unnamed` and `publish`; checked against every
limited ancestor; `OverQuota` rolls the request back. A directory crossing a boundary is `CrossesLimit`. `Volume::set_limit`
and `limit_of`; `fsck` recomputes every `used` and `repair` rewrites it. File protocol: statuses `OVER_QUOTA` (16),
`CROSSES_LIMIT` (17) (`Code` widened to `Below<18>`; the old bound dropped any reply with them), requests `SetLimit` (38,
needs `WRITE`) and `GetLimit` (39); `zoraya-fsclient` `set_limit` and `limit`; `libposix` `EDQUOT` and `EXDEV`;
`zoraya-command` words. Test-only setter: `zoraya-mkfs --limit PATH=SIZE`; the POSIX images carry `/limited` at 64K
(`POSIX_LIMITED`). The ports have no `dd`, so `t_posix diskfill DIR OUT` does what it does (scenarios.script).

**Decisions** (ADR-0290, proposed; the numbers 0290 to 0299 are Q5's, N2 may take 0281): a parent link for files; unnamed files
charged where made and `publish` moves the charge; a file unlinked while open keeps its charge until freed, and when its
directory is removed it is charged to the parent; link = 1 block + target in blocks; a directory is one block; a limit
below `used` stops growth only; a new limit scans the subtree in one request (stalls the volume for a very large tree).
Recommendation: accept ADR-0290 as written.

**Invariant delta.** INV-0265-1, -2: checked (`make test-unit`, `crates/zoraya-fs/tests/limits.rs`; the protocol half in
`zoraya-fsclient`). INV-0265-3 (a room's directory) stays for Q6a; the `dd` half runs now in `make test-posix`
(scenarios). New INV-0290-1 (checked by `fsck` on every crash image). Counts 503 -> 504.

**Tests run (this session).** `cargo test` of zoraya-fs (all, including `limits.rs`: 803 refusals and 35 `CrossesLimit` in the
random run; 2162 crash images each `fsck`-clean with the counters equal to an API recomputation), zoraya-fsclient,
zoraya-fs-service, zoraya-posix, zoraya-proto-file, zoraya-mkfs: pass. `make check`: pass. `make test-posix` with
`POSIX_SCRIPTS=tests/posix/scenarios.script`: PASS (81 steps; `diskfill wrote 15 blocks before Quota exceeded`, a write
outside works, fsck clean). `tools/test-par.sh test-files test-crash test-qemu test-libposix` (HVF, one lane): all ok. Not run:
the full `make test-posix`, test-gui, test-boot, test-tour-gui, test-net, mutation runs (the full CI does).

**Measurement.** A growing one-block write to a file in `a/b/c`, release build, in-memory device, best of five over 4000
writes: 6931 ns with no limit, 7041 ns with a limit on `a` (+111 ns, 1.6%), 7023 ns with the limit on the parent. Run it:
`cargo test --release -p zoraya-fs --test limits -- --ignored --nocapture`. Not measured under QEMU.

**Open items.** Nothing sets limits for a person yet (Q6a); `stat`/`files` do not show the record (the protocol has
`GetLimit`); the format change makes version 1 volumes unreadable (rebuild with `zoraya-mkfs`); `tools/test-map.txt` lines
added for `tools/zoraya-mkfs/` and for the proto, fs and fs-service crates (test-posix); `Makefile` and the testkit are not
mapped so `make test-changed` runs every step.

**Handoff / by hand.** `zoraya-mkfs --size 64M --limit room=64K --from DIR img`; `make test-posix POSIX_SCRIPTS=tests/posix/scenarios.script`.
Review: `crates/zoraya-fs/src/volume.rs` (the "Storage limits" block, `unname`, `delete_object`, rename, `resize`, `write`),
`check.rs`, `format.rs`, `tests/limits.rs`, `crates/zoraya-proto-file/src/lib.rs`, `crates/zoraya-fs-service/src/lib.rs`,
`tests/posix/t_posix.c` (`diskfill`), docs/adr/0290. Model use: Sonnet only; no Opus review of the engine was run.

## Q3 (fsd): accounts in the file service (branch `work/q3`, on `work/q5`)

**Built.** `zoraya-fs-service` keeps accounts (ADR-0262 item 2). Each connection has an account; `open`, `clone`, `narrow` and
`share` join the caller's; the root connection's is the session's. Each account has a limit and a count for open handles
(`MAX_OPEN` 896 in all: connections, tokens, buffers, held-reply handles), mapped buffers (`MAX_BUFFERS` 240) and held replies
(`MAX_HELD` = 896). A new request `CloneAccount { rights, budget }` (40; `zoraya-fsclient::clone_account`) carves an account from
the caller's: `budget` open, `budget/8` buffers, `budget/32` held (each at least 1). From the session's account a budget is 1..=32
and at most four such accounts live (the untrusted column: 128 open, 16 buffers, 4 held; session keeps 768, 224, 892). At the
open bound: `TOO_MANY_OPEN`; at the buffer bound: `LIMIT_REACHED`; a reply that would be a second held one in an account at
its bound closes its connection. The last connection of an account frees it and returns its limits to its parent
(sub-accounts are re-parented; if the session account ends first its carve is dropped and a new session account takes what
the live ones leave). `Sync` on an untrusted account's connection (any carved account) does not commit: it waits for the next
commit (timed or another client's `sync`), the connection is not read meanwhile (interest `Closed`), a failed volume answers it
`STORAGE_FAILED`, nothing to commit answers at once. `fsd.rs` needed no change (the account code is in the library). Q5's
`SetLimit`/`GetLimit` and statuses untouched.

**Decisions** (ADR-0291, proposed; all reversible): `CloneAccount` as a new request, `Clone` unchanged; the account's
buffers and held bounds derive from one budget (explicit numbers were option C); FIFO stream ends (only in `fsd --memory`)
are service-level, counted in the global bound only; the held bound closes the connection (a made reply has no room for a
status); only `Sync` is folded, not `DURABLE` writes (open question: same stall, needs a deferred reply with a count);
"untrusted" = any carved account. **For the architect, from Q5's review:** if `SetLimit` must not be allowed to a plain
`WRITE` holder, the natural place to say who may set limits is the account (a connection of an untrusted account is
`account.untrusted`); I did not change it.

**Invariant delta.** INV-0262-1, -2, -3: `checked: make test-unit` for `fsd` (the other services say so in the row; the ADR
text is per service). INV-0265-5 stays `not yet applicable (until Q3)` in the table: its check is the in-QEMU loop (Q6a/Q7);
the host half is `an_untrusted_sync_waits_for_the_next_commit` and `a_sync_loop_in_an_untrusted_account_adds_no_commit`.
New INV-0291-1 (the sums as an equality). Counts 504 -> 505. `make adr-check`: 0 errors.

**Tests added** (`crates/zoraya-fs-service/tests/service.rs`, fsclient, proto): an account at its open, buffer and held bound
is refused while a second uses its full bound and the session is not refused; the column (four accounts, no fifth, budget 0
and over 32, 768/224 left); free with the last connection, in either order, with a sub-account of a dead parent; a random run
(1500 steps of carve/open/attach/detach/close) checking after each step that the limits sum to 896/240/896, the column is
within its bounds, counts do not pass limits and equal `open_count()`, and that all state is gone at the end; the folded
`sync` (waits, no commit of its own, answered by a session `sync` or the timer, five rounds, a close while waiting);
the wire (`CloneAccount` round trip, ordinals); `clone_account` through the client.

**Measurements** (`worst_case::<u32>()`, from `the_service_size_follows_from_the_bounds`; run it with
`cargo test -p zoraya-fs-service --test service the_service_size -- --nocapture`): fixed 192,512 B (input, output, data
buffers); 766 B per open handle (connection, record, two names); 65,540 B per held reply; 72 B per account.
Untrusted account (32 open, 1 held): 90,124 B. Session: 59,050,040 B. Whole service: 59,603,048 B. The 56 MiB is 896 held
replies of 64 KiB: the session's bound is the number of connections it can have (unchanged by Q3); it would be 4 MiB at 64
held. Proposed, not done (architect): lower the session's `MAX_HELD`. Handle size on the target is 8 B; the numbers move by
under 1%.

**Tests run (this session).** `cargo test` of zoraya-fs-service (65 service tests, fifo, time), zoraya-fsclient (10),
zoraya-proto-file (7): pass. `make check` (fmt, clippy, adr-check and every check-*): pass. `make test-changed` (everything changed since origin/main, so all steps; HVF; `make fetch-ports` was needed in this worktree first): 54 host test binaries ok, then test-boot, test-qemu, test-libposix, test-files, test-crash, test-posix, test-posix-gui, test-tour-gui, test-gui, test-net all ok (511 s). It found a Q5 break, fixed here: `zoraya-posix`'s `every_file_status_maps_once_and_only_ok_to_zero` still stopped at status 16; it now covers 0..18.

**Open items.** No launcher calls `CloneAccount` yet (Q6a: one per untrusted room; the broker/tmpd of a shell in the shell's
room job are Q6a too). `netd`'s part waits for N4/N6. `fsd` does not serve one request per account per pass (ADR-0262 open
question, noted there). `top` does not read account counts yet (`Service::account_totals` is the host form).
INV-0262-1..3 for `netd`, the compositor and the broker are still unchecked.

**Handoff / by hand.** `cargo test -p zoraya-fs-service --test service accounts budgets random_accounts untrusted`;
`cargo test -p zoraya-fsclient clone_account`. Review (reviewer gets the diff and ADR names): `crates/zoraya-fs-service/src/lib.rs`
(`Account`, `carve`, `free_account`, `charge`/`release`, `room`, `send`, `flush_held`, `close_connection`, `fold_sync`,
`answer_syncs`), `tests/service.rs` (end), `crates/zoraya-proto-file/src/lib.rs` (`CloneAccount`), ADR-0262, ADR-0265 item 9,
ADR-0291. Model use: Sonnet only; no Opus review of the trust-boundary code was run (the orchestrator's review of Q3 is due).

**Q5 review fixes** (`docs/tasks/TASK-26-review-q5.md`, dispositions at its end; proposed ADR-0292 for who may set a limit). The "Tests run" line above was wrong for `zoraya-posix` (finding 1); after the fixes `cargo test` of zoraya-fs, -fs-service, -fsclient, -mkfs, -fs-testkit, -proto-file, -posix and -command passes, `make check` passes, and `make test-posix` with `scenarios.script` passes again (81 steps). Changed since: the orphan rule (a directory with a charged open file under a limit cannot be removed), `SCAN_BOUND` 50,000, checked sums, `set_limit` not a freeing request unless it removes a limit, and the model/crash tests keep handles open and change limits.

**Q3 (fsd) review fixes** (`docs/tasks/TASK-26-review-q3.md`, dispositions at its end; after merging `work/q5` at 26812b3 with its
ADR-0292). Fixed: finding 1 (an account with no connection whose sub-accounts live is kept with its leftover limits until the
last of them ends; the untrusted column is whole-subtree; the session's account is kept the same way, which also makes
INV-0291-1 exact after it ends: finding 9), finding 2 (`answer_syncs` runs at the end of `after_engine`), finding 3
(`NoMemory` replies are held and retried, only `Full` counts against the bound), finding 4 (durable writes of untrusted
accounts fold like `sync`, with their count), finding 5 (per-account refusal counter, `account_refusals`), finding 6 (the model
adds Clone, Share, Narrow, tokens, buffers, held replies by full queue and by no memory, and the session's own close, and
sums the real column), finding 7 (a service with FIFOs refuses `CloneAccount`; `carve` saturates), finding 8 (`total()` counts
an account per handle). Open: the launcher cannot read the refusal count yet (Q6a: a request or the supervisor channel),
notes 10 and 11 (sync bookkeeping allocations; any wake of a syncing connection closes it), note 12 (who may set limits, with
ADR-0292).

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

### Tests run (Q4)

`make check` before each commit. `make test-changed` (every step, HVF): host tests (51 binaries), test-qemu, test-libposix,
test-files, test-crash, test-posix, test-posix-gui, test-tour-gui, test-gui and test-net passed; `test-boot` failed once at
`boot-auto-smp4-2g` ("timed out waiting for bytes dropped]", the known input flake) and passed on `make test-boot` alone. The
first `test-changed` attempt stopped at the build because this worktree had no ports archives; they were copied from another
worktree's cache (no download).

### After the review (`docs/tasks/TASK-26-review-q4.md`)

Fixed W1 to W6 and notes 7 to 11 (dispositions in the review file). A spent job's threads never run past the cap of parks per
pick (idle calls `schedule` again); `JobRef::of` is `unsafe`; the time allowance is scaled only in its EL0 part and capped at
2x the ceiling; the ceiling is checked by counts next to an uncapped spinner as well as by `used_ns`; ADR-0285 names its two
deviations from ADR-0264; new `t_cpu` cases: 40 spinners, a job about 30 levels deep, a kill of a parked room.
New finding for the architect: no sharing among rooms under one parent (one of three used 0 ms in 2 s).
Open: the site of the largest masked interval; Q6a's check of `LIMIT` inside rooms.

### The units bound on CI (second fix)

Run 43085222352 (`candidate/work-q4-b`) failed `t_cpu`'s units bound in five jobs: 110 units (the 30-level job, on
`auto-smp4-2g` and `tcg-smp4-gicv2`; rooms next to a woken process, `tcg-smp4-gicv3`), 116 (a room of 40 at 25%,
`tcg-el2`), 94 (`net-tcg`), against 80, every unit from `frames::free`. The values are two exits' tails (2 x 55, 2 x 58,
55 + 39; one tail measures 55 to 71). Cause: `steps::pause` called the preemption point after its window, and a thread
switched away there resumed inside the masked interval of the threads that ran meanwhile; switched away at the flush
before its tail, it then ran the tail (which cannot yield) in the interval of the exit that switched to it. Only an
exit as long as the slice meets that timing, hence slow hosts only. Fix: `steps::step_point` opens a second window
when the scheduler ran (`sched::preempt_point` says so). The kernel test
`a_switch_at_a_step_point_starts_a_new_interval` forces the timing with a stand-in switch; mutant
`K-step-point-no-window-after-switch` (`tools/mutants/kernel-objects.toml`). `frames::free` and `free_contig` count at
the caller's line, so `steps::diagnose_units` names the freeing path. The units bound stays 64 + 16 (an exit's tail:
the space's frames, at most 64 inline, and one 16-frame kernel stack).

## N3: a native program fetches a page over HTTP (branch `work/n3`, on `work/q4`)

### Built

- `crates/zoraya-dns` (new, `no_std`, 16 tests): the A query and a total, bounded decoder of the answer (backward compression
  pointers only, at most 8 jumps, names of at most 253 bytes in a fixed buffer, 32 records read, 8 addresses kept, a message of
  at most 1,232 bytes, our identifier and our question repeated, an address only from a record owned by the name or by a CNAME
  chain from it). `crates/zoraya-http` (new, `no_std`, 17 tests): `parse_url`, the request, and an incremental response
  decoder (any split of the input gives the same result; 8 KiB head, 64 fields, 256-byte lines, 64 MiB body; strict framing).
  `fuzz/fuzz_targets/net_dns_http.rs`: compiled and driven with 400,000 random inputs in a scratch crate (not run under libFuzzer;
  `fuzz/Cargo.lock` not updated).
- `zoraya-proto-net`: `connect` (ordinal 3, three handles: the connection channel's server end, the write end of the stream the
  client reads, the read end of the stream it writes), `connecting`, and on the connection channel `connected`, `failed`,
  `reset` (ADR-0222 item 3 with its note of 2026-11-10: the client's stream ends, so INV-0262-4 holds for a netd connection).
- `zoraya-net`: `tcp` (the socket table, smoltcp's closures, release with the reset going out first), `dns` (the resolver:
  `net.hosts`, cache, retries), `conn` (`Conns::turn`: the single loop's order, the copy, half close, reset, refusal, timeout, the
  local-address rule), `Request::Connect` in `Connection::decide`, `Action::Connect` in `Service`. 59 tests (was 39): the new
  ones run connections over a scripted peer (a TCP server and a DNS server at fixed addresses on the far end of the frame
  queues) with rings from 100 bytes to 64 KiB and chunks of 7 to 1,000 bytes.
- `netd`: `StreamClient` (the three handles; only non-blocking calls; a stream end is subscribed only while the connection waits
  for it) and `Conns` in the loop. `user/src/netconn.rs`: `connect` for clients. `user/src/bin/fetch.rs`:
  `fetch [-o FILE] [--sum] URL`. `zoraya-describe`: operand kind `url`. `zosh`: `net_for` gives a base-system command with a
  `url` operand a connection with one reach rule (the URL's host and port, TCP). `docs/tour.md` and the generated scripts list `fetch`.
- `tools/test-httpd` (std only, 127.0.0.1, bounded, exits after its lifetime), `tools/run-qemu.sh --net-open`, `make test-net`'s
  last step `test-net-fetch` with `tests/shell/net-fetch.script` (page byte for byte; 1,000,000 / 300,000 chunked by name /
  70,000 until close / 4,000,000 bytes by checksum; a file written and read back; 404; redirect; a cut connection; a refused
  port; bad URLs; `-o` keeping an existing file on 404, refusal and a cut page; a URL from a variable or a substitution refused),
  and `tests/shell/net-dns.script` (SLIRP's DNS answering `localhost`, refused as local, and `nothing.invalid`) in a second boot, skipped
  on CI with a visible `[net-dns] SKIP` unless `NET_DNS=1`. The server takes `--port 0`.
- Docs: `docs/design.md` "TCP, names and `fetch`", proposed ADR-0286, `docs/adr/invariants.md`.

### Decisions where the ADRs are silent (proposed ADR-0286; recommendations)

1. **`connect`'s shape** as above; reasons `Unresolvable`, `Refused`, `Unreachable`, `TimedOut`, `Local`, `OverLimit`. Accept.
2. **Copy through smoltcp's closures, 32 KiB socket buffers, 8 sockets and 16 connections, subscriptions only while waited
   for.** Accept; the bounds are global until Q3's per-scope account.
3. **Resolver details** (A only, attempts at 0/1/2/4 s, give up at 8 s, 64-entry cache, no TCP fallback, guessable ids and
   ports until ADR-0227). Accept.
4. **A literal `url` operand designates (ADR-0224 item 2); a value is refused.** After the review (`TASK-26-review-n3.md`) the
   designation is `zoraya_shell::netgrant::net_for`, host-tested, and a `url` operand from a variable, a substitution or a
   glob is refused before the line runs. Not built: the `+net`/`+listen` words (which would let a value designate), `grants`
   output for the scope, the REPORT message and the hint (N3b). ADR-0224's open questions carry the note.
5. **`restrict=off` for the fetch step only** (`--net-open`), against a loopback server. Accept.

### Invariants delta

`checked`: INV-0222-1, -2 (`cargo test -p zoraya-net`; "connected before any stream end" is the note's new shape: the client holds
the stream ends from the start and `connected` says the connection opened), INV-0222-3 (`cargo test -p zoraya-proto-net`),
INV-0223-2 (the scope tests of `connect`), INV-0223-4 (`cargo test -p zoraya-dns`). New INV-0286-1, -2 (checked). Partly covered,
rows unchanged: INV-0223-3 (the refusal is tested; "reported" needs the REPORT message), INV-0223-1 and INV-0220-2 (the decision is
tested; the frame-recorder test is not written), INV-0221-4 (the test between kills still needs a fetch loop: a candidate for N6),
INV-0224-1 (the `url` source exists; the `+net` words do not). adr-check total 507.

### Where a network-controlled number enters, and where it is checked

| Number | Entry | Check |
|---|---|---|
| DNS answer bytes | `Stack::dns_poll`, `zoraya_dns::parse_response` | copied from smoltcp's UDP socket into a 1,233-byte buffer; source must be a configured server on port 53; the identifier must be a query in flight; then the decoder: length 12 to 1,232, QR and opcode, one question equal to ours, rcode, at most 32 records, each length against the message, pointers backward and at most 8, names at most 253, an A record of 4 bytes, an owner equal to the name or a CNAME target |
| DNS TTL | `parse_response` | the smallest of the addresses' TTLs, at most 3,600 s, then multiplied as an `i64` of microseconds |
| resolved addresses | `Conn::next_candidate` | the local-address rule against `status` (own address, gateway, DNS servers) and the scope's address rules; `tcp_open` refuses non-unicast |
| TCP payload | smoltcp's `recv` closure | never parsed here; copied to the ring (`Wr::Wrote(n)` capped at `data.len()`) |
| stream read count | `StreamClient::read_outbound` | `n.min(buf.len())`, 0 only as end of file; the closure result is capped again by `tcp_send` |
| HTTP response | `zoraya_http::Parser` in `fetch` | 8 KiB head, 64 fields, strict framing, 64 MiB body; the cap holds for every framing and every split (INV-0286-2) |
| URL | `zoraya_http::parse_url` in `fetch` and in `zosh` | scheme, host (name or dotted quad, a name never ends in a number), port 1 to 65,535, no user information, printable path, at most 2,048 bytes |
| `connect` request | `Service::handle` | decoder: three handles, port below 65,536, name at most 253 bytes; then UTF-8 and `valid_name`, the right `reach`, the scope for name or address and port, handle types in `netd` |

### Tests run

- `cargo test -p zoraya-dns -p zoraya-http -p zoraya-proto-net -p zoraya-net -p zoraya-describe -p zoraya-shell`: 16, 17, 5, 59, 23, 103 passed.
- `make check` before each commit; `adr-check` clean (0 errors).
- `PAR_QEMU=1 make test-changed` (every step, because `user/Cargo.toml` and `tools/run-qemu.sh` are not in the map; HVF, one guest at a time):
  host tests (56 binaries), test-boot, test-qemu, test-libposix, test-files, test-crash, test-posix, test-posix-gui, test-tour-gui,
  test-gui and test-net all passed, 949 s. The worktree had no ports archives; they were copied from another worktree's cache
  (no download) and `make fetch-ports` verified them.
- `make test-net-fetch` on its own after the last change to `fetch`: PASS, 57 steps. Not run: `FULL=1` (TCG for the fetch step) and the
  fuzz targets under libFuzzer. The TCG fetch step is written (`FULL=1 make test-net` runs it with a 400 s timeout) and was exercised
  only through the benchmark script, which ran the same fetches under TCG.

### Measurements

See `docs/design.md`, "TCP, names and `fetch`": HVF 16 MiB in 232 ms (213 ms without `fetch`'s hashing, 79 MB/s), TCG 1,463 ms
(1,423 ms, 11.8 MB/s); a one-byte page, connect to end of file, 27.7 ms (HVF) and 7.8 ms (TCG). Medians of 5 and 10 runs, single
boots. The copies' share is an estimate (9 to 16 percent of the HVF time from the stream rates of stage 6), not a measurement.

### Handoff

- State: branch `work/n3` on `work/q4` on `work/n2` on `work/integ`, local only, not merged.
- By hand: start `target/debug/zoraya-test-httpd --port 8080` on the host, then `make run-net` and at the prompt
  `fetch http://10.0.2.2:8080/`; `fetch --sum http://10.0.2.2:8080/pattern/16777216` for the numbers;
  `fetch http://localhost:8080/` (refused as local); `net status`. `make test-net` (`FULL=1` for TCG);
  `make test-net-fetch` alone; the server's log is `target/test-logs/test-httpd.log`.
- For the reviewer: `crates/zoraya-net/src/{conn,tcp,dns,service,connection}.rs` and `conn_tests.rs`; `crates/zoraya-dns/src/lib.rs`;
  `crates/zoraya-http/src/lib.rs`; `user/src/bin/netd.rs` (`StreamClient`, the `Action::Connect` arm, `turn`);
  `user/src/netconn.rs`; `user/src/bin/fetch.rs`; `user/src/bin/zosh.rs` (`net_for`); `tools/test-httpd/src/main.rs`;
  `tools/run-qemu.sh` (`--net-open`); the `test-net-fetch` recipe; `docs/adr/0286-*.md`.
- **Merging with the fixes on `work/n2`** (the N2 review's open items: `poll` returning `Some(0)`, `ErrorReply` answering an
  `ErrorReply`, the ICMP socket recreate): not duplicated. My edits to files those fixes may touch are small: `stack.rs` (three
  fields made `pub(crate)` and one field `extra` added, plus its initialiser), `netd.rs` (the `turn` body and the match on keys),
  `service.rs` (one ordinal arm and one `Action` variant). Expect a textual conflict in `Stack`'s struct literal and in `turn`.
  `Conns::turn` takes the delay `stack.poll` returns and combines it with the resolver's and the connections' deadlines; if the
  `Some(0)` fix changes what `poll` returns when frames wait, `turn` follows it with no change.
- Open: (1) the `+net` words, `grants`, the REPORT message and the hint (N3b); (2) the `Reset` reasons (smoltcp does not say a timeout
  from a reset); (3) per-scope bounds and accounts for the 8 sockets, the 16 connections and netd's buffers (Q3); (4) the HVF latency
  of 27.7 ms against TCG's 7.8 ms: the guest timer tick and smoltcp's ACK delay are the suspects, no cause found; (5) `datagram`,
  `listen`, `resolve`, `peer`/`local`/`share`, AAAA, TCP DNS, negative caching; (6) the frame slot is copied byte by byte with volatile
  reads (`Frames::read_slot`), a cost on every received byte; (7) the fuzz targets never ran under libFuzzer and `fuzz/Cargo.lock` is not
  updated; (8) INV-0221-4's test between two kills needs a fetch in a loop; (9) `libposix` sockets are N4; (10) the test needs SLIRP's
  DNS forwarder for `localhost` and `nothing.invalid`, which works without the Internet on this host (QEMU 11.1.2); a CI image whose
  host has no resolver that knows `localhost` would fail those two steps, and they are the first to suspect if CI differs.

### Review fixes (after `docs/tasks/TASK-26-review-n3.md`; its "Dispositions" section has each finding)

Errors 1 to 3 fixed (a reset after the peer's FIN and a 30 s limit for half-closed connections; `net_for` in `zoraya-shell` with a
refusal of values; `fetch` fails on a reset whatever the framing), warnings 4 to 8 fixed (ADR-0224 note, DNS steps split and skipped
on CI, `-o` opens at the first byte, `--port 0`, host tests). Tests: `cargo test -p zoraya-net` 61, `-p zoraya-shell` 110 (7 new).
Open: notes 11, 12, 14.

## N4: a POSIX program fetches a page, with Q3's `netd` account (branch `work/n4`, on `main` at da771a0)

### Built

- **`netd` accounts and `resolve`** (commits f9c74cc, 7dce152, earlier in the branch): the account of ADR-0262 item 2 on `narrow` with a
  budget (`NarrowAccount`, ordinal 7), connections, pending names and cache entries bounded per account, `connect` refused with
  `OVER_QUOTA`; `resolve` (ordinal 4, ADR-0222 item 2, ADR-0223): one lookup per request, charged as a pending resolution, local
  answers left out.
- **`libposix` sockets** (`crates/zoraya-posix/src/{socket,net}.rs`, `ffi/sockets.rs`): `Desc::Socket` in the descriptor table, with its
  arms in `read`, `write`, `lseek`, `fstat` (`S_IFSOCK`), `fsync`, `fcntl` and `release`; `socket`, `connect`, `send`, `sendto`, `recv`,
  `recvfrom`, `shutdown`, `setsockopt`, `getsockopt`, `getsockname`, `getpeername`, `getaddrinfo`, `freeaddrinfo`, `gai_strerror`,
  `htons`, `ntohs`, `ntohl`, `inet_pton`, `inet_ntop`, `inet_aton`, `inet_addr`, `inet_ntoa`; `select` over sockets. The names are in
  ADR-0287's `posix-surface-added` block; `socket` and `getaddrinfo` are off the check's deny list (ADR-0225 item 1).
- **The grant**: the connection to `netd` reaches a POSIX program in the second bootstrap message (`ZPBN`: `SELF` and a channel;
  `ZPBS` without one). `zosh` makes it from a literal `url` operand of a POSIX program the image describes
  (`user/describe/posix/zfetch.cmd`, `Ready::Posix::net`, `posix_child::Launch::net`), as for `fetch`.
- **`zfetch`** (`tests/posix-net/zfetch.c`): `fetch` in C; `make test-net-posix` (a step of `make test-net`) builds it with `zcc`,
  puts it in `/bin` of a disk and fetches the test server's pages by the shell, by address and by name.

### Decisions where the ADRs are silent (proposed ADR-0287; recommendations)

1. **The connection in the second bootstrap message, not a fixed slot and not a record field.** A fixed slot 1006 was built first: the
   first run of a program *without* a grant answered `connect` with `EIO`, because the kernel puts a handle that arrives in a message
   in the slot after the last one `spawn` filled, so the first directory entry was in slot 1006 and the library took a file
   service's channel for `netd`. Found by the test of a program started without the grant (the reason that test is in the script).
   Differs from the plan's "`NET` in `ZPB1`" (no record version moves).
2. **A child has no network; a socket does not cross `spawn` (`EBADF`) or `fork` (`ENOTSUP`).** ADR-0225 item 9's three-handle
   inheritance is not built; refusing is the closed choice.
3. **`inet_pton` and `getaddrinfo` read an address strictly** (the N3 review asked); `inet_aton`/`inet_addr` keep the historical forms.
4. **The workload is `zfetch`, not curl.** curl and wget (W11, W13) need `poll`, `getnameinfo`, `gethostbyname`, TLS; the plan's done-when
   ("`+net test.zoraya curl -O ...`") is not met as written: `+net` words are not built (N3b) and curl is not a port yet.
5. **`socket` and `getaddrinfo` leave the check's deny list** (`tools/posix-surface-check`, ADR-0225 item 1 accepted). ADR-0005's text
   is unchanged: its superseding note is the architect's (ADR-0225 says so).
6. **`getaddrinfo` returns one malloc block per address** (`addrinfo`, `sockaddr_in` and the canonical name together).

### Invariants delta

- INV-0225-2 and INV-0225-3: `checked: cargo test -p zoraya-posix --test net` (the counting fake `netd`).
- INV-0287-1, -2, -3: new, all `checked`. Total 512.
- Still `not yet applicable (stage 8)`: INV-0225-1 (the one report per target: the hint is not built; the `EACCES` half is tested by the
  script's run without the grant), -4 (`poll`; only `select` is built), -5 (`getnameinfo` is not built).

### Where a program-controlled or network-controlled number enters, and where it is checked

| Input | Where | Check |
|---|---|---|
| `sockaddr` pointer and length from the program | `connect`, `ffi/sockets.rs` | length at most 128, copied, then `parse_sockaddr_in` (length, family, port 0 refused) |
| `getaddrinfo` strings and hints | `ffi/sockets.rs`, `socket::plan` | `c_str` bounds (PATH_MAX); `valid_name`; flags, family, type, protocol checked; unknown flags `EAI_BADFLAGS` |
| `setsockopt` value | `ffi/sockets.rs` | at least four bytes, an `int`; the timeouts are not read |
| `Resolved`, `Connecting`, `Failed`, `Reset` | `net.rs` | decoded by the protocol crate (total); a status or reason outside the table is `EIO` |
| a page (in `zfetch`) | `zfetch.c` | head 8,192 bytes, header lines scanned inside it, chunk-size line 63 bytes and at most 2^40, lengths by `strtoull`; a malformed chunk is an error |

### Tests run

- `cargo test -p zoraya-abi -p zoraya-posix`: 66, 48, 49 (files), 13 (net) passed. `make adr-check`: 0 errors.
- `make check`: all 14 checks passed.
- `make test-net-posix` (alone, HVF, four runs while the test was fixed; the last): PASS, 45 steps, then `zoraya-fsck` clean.
  `libc.a` against the tier list: 315 exports against 318 names, passed.
- `make test-changed` (HVF, parallel lanes, 485 s): host tests 47 binaries 81 s; build, test-boot, test-qemu, test-libposix, test-files,
  test-crash, test-posix, test-posix-gui, test-tour-gui, test-gui, test-net all ok (`test-net` includes `test-net-posix`). Locally TCG
  runs and the tour's second run are skipped.
- Not run: `FULL=1` (TCG for the posix step; it is written, 400 s), `make test` and `make test-par` (the architect's and CI's).

### Measurements

See `docs/design.md`, "POSIX sockets": HVF 16 MiB in 226 ms for `zfetch` against 236 ms for `fetch`; TCG 1,305 against 1,380 ms; a
one-byte page 23.1 against 28.1 ms (HVF). Medians of 5 and 10 runs, single boots, one guest.

### Not done

curl and wget (W11, W13) and the plan's `+net ... curl -O` check (deferred by the architect, 2026-11-12, to the next stage, "living in
the system": BearSSL or rustls-ffi as the C TLS library, then curl and wget; backlog row; not a blocker for stage 8); `bind`, `listen`, `accept`, datagrams, `socketpair`, `poll`,
`sendmsg`/`recvmsg`, `getnameinfo`, `gethostbyname`; inheritance through `spawn` and `fork`; the REPORT message and hint of
ADR-0224 rule 5 (INV-0225-1); `SO_RCVTIMEO`; `getsockname` (no `local` request). `FULL=1` was not run. No fuzz target for the new
parsers: `parse_sockaddr_in`, `inet_pton4`, `inet_aton` and `plan` are total by construction and tested on boundary inputs.

### Handoff

- State: branch `work/n4`, pushed without a tag, not merged. The brief said the worktree held uncommitted libposix code with an
  ffi layer, a C `zfetch`, tests, wiring and measurements; it held only `socket.rs`, `net.rs`, the `errno` rows and the
  `Desc::Socket` variant, none of which compiled. Everything else here was written in this session.
- For the reviewer (trust boundary, untrusted input): `crates/zoraya-posix/src/{socket,net}.rs`; `src/ffi/sockets.rs` (every `unsafe`
  block and `build_addrinfo`/`freeaddrinfo` against musl's `struct addrinfo`); the `Desc::Socket` arms in `process.rs`; `jobs.rs`
  (`fork_moves_into`); `ffi.rs` (`bootstrap`: the `ZPBN` message); `user/src/posix_child.rs` (`Launch::net`, every failure path closes
  it); `user/src/bin/zosh.rs` (`posix_net`, `Ready::Posix::net`); `tests/posix-net/zfetch.c`; `tools/posix-surface-check/src/surface.rs`
  (the deny list); `docs/adr/0287-*.md`.
- Review questions: (1) answered by the review (finding 1): a `fork` child does not hold the connection (a channel end is not
  duplicable); `after_fork_in_child` clears `net` and the names, and its first `connect` is `EACCES` (ADR-0287 item 3).
  (2) `select` on a connecting socket waits on the control channel for both directions; a `Reset` on a connected socket's channel
  wakes a read wait, and the read then waits again. (3) `getaddrinfo`'s one-block-per-address layout and `freeaddrinfo` (a program
  that frees the `ai_addr` or `ai_canonname` separately would corrupt the heap; musl's own does the same with one block).

### Review fixes (after `docs/tasks/TASK-26-review-n4.md`)

- 1 (error): `after_fork_in_child` sets `net = None` and clears the remembered names; host test
  `a_fork_child_has_no_connection_to_netd` (the child's `connect` and `getaddrinfo` are `EACCES`, nothing reaches `netd`). Question 1
  above and ADR-0287 item 3 corrected: a fork child holds no connection.
- 2: `Service::fresh_token` skips a token a pending lookup holds; test `a_wrapped_lookup_token_never_takes_a_pending_ones` (it fails
  without the check).
- 3: `zosh`'s `posix_net` now uses `narrow_account` with a budget of 2 (`POSIX_NET_BUDGET`), so each POSIX program counts against its
  own account. Not done: a script line that goes past the budget. The test program `zfetch` holds one connection at a time; going
  past needs a second C program with its own description in the image, left for a later step. The bound itself is host-tested
  (`zoraya-net`) and the call runs in `make test-net-posix`.
- 5, 9: stated in ADR-0287 (items 3 and 7). 6: `posix_child::launch` closes `me` and `net` when the `ZPBS`/`ZPBN` write fails.
  7: `recvfrom` checks `addr`/`addrlen` before taking data. 8: `getsockopt` stores the bytes copied in `*len`. 10: `zfetch.c`
  requires `strlen(head) >= 12` before `atoi(head + 9)`.
- Fuzz: `fuzz/fuzz_targets/net_stack.rs` target 4 now also drives a connection narrowed with an account and `resolve_done` with
  fuzzed tokens. `cargo check` of the fuzz crate passes; `cargo fuzz` is not installed here, so it was not run.
- 4 (N4 done-when not met: no curl, wget or `+net`; no TCG run) stays the architect's call, as the review says.
- 11 (for Q7's gate tests): a literal `url` operand designates the network with no check of the room's trust, for native and POSIX
  programs alike, and trust is by path: any binary at `/bin/zfetch` gets the designated network (ADR-0153 rule 10's posture; the gate
  refuses untrusted rooms for now). Q7's gate tests must cover it before the gate is lifted.

## N5: HTTPS with a real clock and real randomness (branch `work/n5`, on `main` at a322e23)

### Built

- **Step 0 (disk).** 44 GB free at the start (the task's threshold is 25 GB): nothing removed.
- **The clock** (ADR-0226; commit c1c2993): `zoraya_dtb::platform::rtc`, the PL031 as a `Device` (one range, `intid` 0) at slot 5,
  `FIRST_DEVICE_SLOT` 7; `init::stack::read_rtc` and `fsd --time-floor`; `time_base(latest, monotonic, floor)`; `ZNS1` with a 32-byte
  header and `realtime_base`; `zoraya_user::namespace::realtime()`; the `date` command; the runner's `D SECONDS` step compares a UTC
  line with the host's clock when it reads it.
- **The random service** (ADR-0227; abf7d69): the seed in a memory object at slot 6; crate `zoraya-rng` (ChaCha20 with fast key erasure,
  protocol, service logic); `rngd`; the shell's connection at slot 10 and a `random` slot in `ZNS1` for each native program;
  `zoraya_user::random`; the `random` command; the runner's `U N PREFIX` step (N words after a prefix, all different).
- **TLS** (ADR-0228; a92ae82, 3b22dde): `zoraya-tls`, `fetch https://`, `zoraya-http` with `tls`, the roots in the shell's table
  (`/system/tls/roots`, 108 Mozilla roots as CDLA-Permissive-2.0 data), the vendored ring, `tools/ring-cc`, `tools/ring-include`, the
  TLS terminators of `tools/test-httpd`, the test PKI `tests/net/tls`, `docs/dependencies.md`, `docs/design.md`.

### The provider check, corrected

N1 said ring builds for `aarch64-unknown-none`. It built the objects; the **link** failed (`ring_core_*_bn_mul4x_mont` and the other
assembly routines) because ring's `build.rs` assembles its aarch64 assembly only for the operating systems it lists, and the host's
`ar` makes an empty archive of ELF objects. Fixed by a vendored ring with one line changed (`tools/vendor-ring.sh`), `tools/ring-cc` and
`zoraya-ar` (`.cargo/config.toml`). BearSSL was not needed. Cost: 8 MB in `vendor/`; `check-host-paths` leaves `vendor/` alone
(its authors' addresses). Proposed ADR-0289 records it.

### Decisions where the ADRs are silent (proposed ADR-0288 and ADR-0289; recommendations)

The numbers are the next free of the task's 0280 to 0299 (the task reserved a range, not numbers). ADR-0288: the RTC device shape, the
slot layout, `ZNS1`'s growth, the seed object, the generator's construction (ours: for the reviewer), the service, what is not built.
ADR-0289: ring's vendoring, the TLS crate, `fetch https`, the roots file, the test servers on ports (not `guestfwd` 443).

### Invariants delta

INV-0226-1, -2, INV-0227-2, -3 and INV-0228-1 are `checked`. INV-0226-3 (a POSIX `time()` against `realtime()`) is not tested in QEMU
(both come from the same base by construction); INV-0227-1 holds for native programs only (no forked pair); INV-0228-2 was checked by hand
(`strings` finds no `rustls`/`ring` in `netd` or `zosh`, 43 hits in `fetch`); INV-0228-3 is not checked. New: INV-0288-1 to -3, INV-0289-1 to -3.

### Where a network-controlled number or byte enters, and where it is checked

| Input | Where | Check |
|---|---|---|
| TLS records, handshake, certificates | `zoraya-tls` (rustls, webpki) | rustls' parsers; the incoming buffer is bounded at 49,152 bytes; errors map to `Error`, nothing is skipped |
| the server's name | `Connection::connect` | `ServerName::try_from` of the address's host, the host that was granted |
| the date | `Config::new`'s function | none: no date is no handshake |
| the roots file | `pem_certificates` | total on any bytes (host test); unusable blocks are skipped; no usable root is `NoRoots` |
| the random service's reply | `random::fill` | the length must equal the request or the call fails |
| the RTC value | `read_rtc` | 0 is "none" and no floor; a u32 of seconds |
| `/chosen/rng-seed` | kernel `init_seed` | cut at 4,092 bytes; `rngd` checks the length again |

### Tests run

- `cargo test`: `-p zoraya-dtb` 14, `-p zoraya-abi` 66, `-p zoraya-fs-service --test time` 3, `-p zoraya-files` 34, `-p zoraya-rng` 10,
  `-p zoraya-tls` 8, `-p zoraya-http` 17 and `-p zoraya-shell` (110) passed; `-p zoraya-test-runner` passed (unit tests of the new steps).
- `make check`: all 14 passed. `make test-files` (with `clock.script`): passed, `date` twice 3 s apart within 2 s of the host, three `random`
  lines differ. `make test-net-fetch`: passed, 108 steps, `fsck` clean, "the refused TLS servers read no request byte".
- `make test-changed` (HVF, parallel lanes, 608 s; locally TCG, the tour's second run and crash rounds past 2 skipped): host tests 69 binaries, build, test-qemu, test-libposix, test-posix, test-posix-gui, test-tour-gui, test-gui and test-net ok. The first two runs failed on things this part changed and the scripts had not followed: the tour's `help` list (date, random), the conventions script, `rngd: ready` matching the jobs script's `d: ready`, the shell's `slots` line (the random connection at slot 10) and the grants script's table (one more channel). Fixed; `make test-boot` and `make test-files` were then run alone and passed. The third run's test-boot and test-files steps are those two.

### Measurements (HVF, one run)

`fetch --sum https://test.zoraya:PORT/pattern/1000000`: TLS handshake 51.1 ms; 1,000,000 bytes in 25.1 ms; the same page over HTTP 38.8 ms
(same boot). `fetch` is 1.06 MB, `netd` 0.30 MB. TCG not run for the HTTPS lines.

### Not done

BearSSL and the curl port (W12's curl half, and the plan's `+net test.zoraya curl -O` check); `https` for the POSIX `zfetch`; the
virtio-rng reseed; `getrandom` and `/dev/urandom` in `libposix`; random connections for the panel's, the broker's and a `fork` child's
programs; a restart of `rngd`; `netd`'s own seeding; the `grants` line for the random connection; the `tls.roots` setting; the GUI shells'
roots entry; the HTTPS lines under TCG; INV-0289-2 and -3 as automatic checks. No fuzz target for `zoraya-rng` (20,000 random messages
in a host test) or `zoraya-tls`. No mutation run.

### Handoff

- State: branch `work/n5` from `main` at a322e23, pushed without a tag. A candidate tag needs the `opus-review` first.
- For the reviewer (TLS, a trust boundary; the generator is hand-written; a kernel and `init` change): `crates/zoraya-tls/src/lib.rs`
  (the pump loop, the `getrandom` source, the time provider, error mapping) and `tests/`; `crates/zoraya-rng/src/lib.rs` (ChaCha20,
  `mix`, `fill`, `Service::handle`); `user/src/bin/fetch.rs`; `user/src/bin/rngd.rs`; `kernel/src/device.rs` (`init_rtc`, `init_seed`,
  the `intid` 0 filter) and `kernel/src/process.rs` (slots 5 and 6); `user/src/stack.rs` (`read_rtc`) and `user/src/bin/init.rs`;
  `user/src/bin/zosh.rs` (the roots entry in `ready`, the random share in the table); `crates/zoraya-abi/src/native.rs`;
  `tools/ring-cc`, `tools/vendor-ring.sh`, `.cargo/config.toml`, the one-line diff of `vendor/ring/build.rs`;
  `rootfs/system/tls/roots` (its filters); ADR-0288 and ADR-0289.
- Review questions: (1) is the generator acceptable or should it be a library DRBG; (2) the seed also stays in the kernel's device tree
  copy; (3) `fetch` treats a connection that ends without `close_notify` as an error unless the page was complete, and the roots entry
  is checked by the terminal's `not_given` only in `open_entry`; (4) any hostile-byte path in `pump` (the 49,152-byte bound, `discard`
  accounting); (5) the vendored ring: is one line the right size of change.

### Review fixes (N5, `docs/tasks/TASK-26-review-n5.md`)

1. `rngd` refuses a seed under 32 bytes or all zero ("no entropy source", it does not serve; `Generator::from_seed`); `seed_random` returns a `Result` and refuses fewer than 32 bytes (tests in `zoraya-rng` and `zoraya-tls/tests/unseeded.rs`).
2. The realtime base's lowest bit says the real-time clock gave the date; `fetch` gives TLS no clock without it, so the handshake fails with `NoClock` ("no date") (`zoraya-abi` test; `no_date_means_no_handshake`).
3. Only connections of depth 0 (`init`) and 1 (the shell) may `Share`, 64 live children each, freed when a child closes (`only_the_first_two_levels_share_and_children_are_counted`).
4. `vendor/ring.sha256` is checked by `make check` (`tools/check-vendor-ring.sh`, through `check-host-paths`); that check's exclusion is `vendor/ring/` only.
5. The kernel hands the seed object on once (`take()`); ADR-0288 item 4 says so; `rngd` overwrites its seed buffer.
6. The `net-fetch` check requires `ended after 0 request bytes` for both refused servers.
7. `Connection::connect` takes 32 seed bytes and mixes them in; INV-0289-1 says the generator-backed `getrandom` runs only in `make test-net-fetch`.
8. The test roots carry `nameConstraints=permitted;DNS:.zoraya` (PKI regenerated); `tests/net/tls/README.md` says the keys are public.
9. Limitation recorded (not fixed): a console shell, or a granted `fetch URL -o /system/tls/roots`, can replace `/system/tls/roots`. INV-0227-2's text says the check covers `mix` only. `tools/test-map.txt` has lines for `zoraya-tls`, `zoraya-rng`, `vendor/`, `rngd`; `device.rs` uses `checked_add`.
Not done: the differential property test of `pem.rs` against rustls's PEM parser.

## Q6a: the rooms keeper and the room line (branch `work/q6a`, on `main` at 1be11a1)

### Built

- **Step 0 (disk).** 42 GB free at the start (the task's threshold is 25 GB): nothing removed; the ports archives (15 MB) were copied from the
  orchestrator's `build/ports/src` into this worktree, read only, so that `make test-changed` has them.
- **The room's line** (`crates/zoraya-settings/src/rooms.rs`; ADR-0266 points 1, 3, 5, 8): `Limits` (the six words), `parse_size` and
  `format_size` (K, M, G, 1024-based), `Machine` and `Ceilings` (the session's and the untrusted job's), `fill_defaults` (point 3's table,
  tested on both guest sizes), `Limits::set` (`none` for a trusted room only, `--tmp` for untrusted only), `check_within`
  (`--mem 512M: above the untrusted rooms' 178M`), `replace` (one line rewritten, every other byte kept), and the quota hint's pure
  part (`refused_field`, `quota_hint`). 9 new host tests.
- **The protocol** (`crates/zoraya-proto-rooms`): `Launch`, `Leave`, `Set`, `Usage`, `Report`, `Stop`, `Open` and their replies
  (`Launched`, `Done`, `Denied`, `UsageList`, `Opened`), the unasked `Refused`, `Keep`, `Forget` and `Attach` between `init` and the
  keeper; `ChannelKind` (`Manage(Desktop|Console)`, `Room(R)`, `Usage`); a 192-byte `UsageRecord` that holds a room's use, its six limit
  words and four service reports. 6 host tests (every message round-trips and decodes totally).
- **The decisions** (`crates/zoraya-rooms`, 20 host tests over a model of the kernel's jobs): the three channel kinds and what each may
  send (INV-0269-2), one job per room, launch jobs without `LIMIT`, the untrusted job (ADR-0266 point 4), at most four untrusted rooms,
  one `Manage` holder at a time with a lease, idle rooms let go after 5 s, `Set` with the ceilings checked also for a room that does not
  run, `Stop`, `Report`, `home` as a trusted room without limits under the session job, a restarted keeper (`resume`) and the notices of
  `Refused`.
- **The process** (`user/src/bin/rooms.rs`): the `Env` over the kernel (jobs, limits, usage, `OVER_QUOTA` subscriptions), `/config/rooms`
  read through a connection, channels served from `init`'s `Attach`, `Keep` and `Forget` to `init`.
- **`init`** (`user/src/bin/init.rs`, `user/src/gui.rs`): starts the keeper with the volume (session job, start-up channel, `/config`),
  attaches a `Manage` channel for the panel (its slot 8) and for the console shell (slot 11), keeps the jobs the keeper sends and restarts it
  up to three times with them, and answers a launcher's request on its reconnect channel (panel slot 9, shell slot 12) with a new channel.
- **The launchers.** The panel (`user/src/bin/panel.rs`) asks the keeper for a launch job before anything else of a launch, spawns the
  programs under it (`spawn_program(parent, ...)`), starts a terminal's broker in it (`beside::start_broker_in`) and gives the shell of a
  room a `Room(R)` channel (`Give::Keeper`, slot 11, minted by `Open`); it sends `Leave` after the spawn. `zosh` asks for a launch job when it
  enters a room and runs every line under it; `room --leave`, `room remove` and entering another room give it back.
- **The shell** (`user/src/bin/zosh.rs`, `user/describe/builtins/room.cmd`): `room new NAME [--mem ...] GRANT...`, `room set NAME WORD VALUE...`
  (writes the line, asks the keeper, puts the old line back if refused; `--disk` on the `+w` directories), `room remove NAME --kill`,
  `room` lists the limits after the trust, `grants` ends with the room's `limits ...  (room NAME)` from the keeper's record, and one
  hint per line naming the field and the room.
- **Tests.** `t_rooms` (QEMU, `make test-qemu`; the real keeper under a session job the test makes, the test standing in for `init`);
  `tests/shell/rooms.script` (QEMU, `make test-files`; the real `init`, the console shell and `memhog`, a new test program that maps
  memory until its job refuses).
- **Documents.** ADR-0293 (proposed), `docs/design.md` ("Rooms with limits and the room keeper"), the invariants table, the gate list
  (`keeper` and `many-rooms` are `test-qemu` now), `tools/test-map.txt`.

### Decisions where the ADRs are silent (proposed ADR-0293; recommendations)

1. `Open` (request 7, `Manage` only) mints a `Room(R)` or `Usage` channel, so the panel can give each shell in a room its own. Recommend: accept.
2. `Leave` and a lease for an untrusted room: busy while another `Manage` channel holds a launch job it has not given back or the room has
   a process. The holder's closed channel gives the lease up. Recommend: accept.
3. `init` keeps duplicates (`Keep`/`Forget`) and gives launchers a reconnect channel; the ADR's "services channel" did not exist.
4. `home` is a room the keeper serves (launch jobs under the session job, no limits): the panel's `home` launches were outside the session
   job until now (the orchestrator's open item). The console shell in `home` still launches in its own job, which `init` made under the session.
5. The keeper checks every ceiling (at `Launch` and `Set`, also for a room that does not run); the shell checks syntax only. `room set`
   writes first and rolls back on refusal, so the two cannot disagree.
6. The hint reads the room's refusal counters from `Usage` at the end of each line (synchronous, no race with the notice); `Refused`
   notices are sent for Q6b's pills. At most one hint per line.
7. `--disk` is set by the shell on the room's `+w` directories through `SetLimit` (ADR-0292 option A). Option B (a `LIMIT` right) is for Q7.
8. Idle rooms are let go after 5 s (`IDLE_MS`), which is what lets a fifth untrusted room run after one has ended.

### Where a program-controlled number enters, and where it is checked (for the reviewer)

- A room name arrives on a channel: `Text<24>` on the wire, `RoomName::new` for a `Room(R)` kind, a lookup in `/config/rooms` for a name
  that is not running (`is_room_name` is the file reader's rule). A `Room(R)` channel is compared with R before any message is acted on.
- A `Launch` from an untrusted room's own shell creates a job whose record is charged to the room (`Job::charge_record` charges the job
  and its ancestors), so a flood stops at the room's memory limit (OverQuota becomes `Denied(KERNEL)`); the reply is a queued message on a
  channel the keeper writes (a channel that is full or closed is dropped by the keeper).
- Limits come from the line (`parse_size` bounds the digits and `checked_mul`; counts 1 to 1,000,000; `--cpu` 1 to 100%), are checked
  against the parent's ceilings by `check_within`, and by the kernel again (`job_set_limits`, INV-0261-3).
- `Report` counts are stored as given (they are the launcher's own account); `Usage` copies them out unchanged.

### Invariants delta

INV-0261-8, INV-0263-4, INV-0266-1, INV-0266-2, INV-0269-2: checked (`make test-unit`, `make test-qemu`, `make test-files` as the rows say).
INV-0269-1 and -3: checked at the keeper's channels in `t_rooms` (the test stands in for `init`; the GUI form of -1 and `init`'s own
restart path are not run). INV-0266-3 stays open for the compositor's hover card (Q6b); INV-0265-3 and -4 and INV-0268-3 stay open (the
room's `/tmp` and an untrusted room's `tmpd` and broker are not built). Counts unchanged (the table has 518 rows).

### net-port-seed (`work/net-port-seed`): the first ephemeral port of a restarted `netd`

- **Incident** (main CI run 43330650492, `net-fetch-tcg`): `Extra.next_port` started at 0 and nothing seeded it, so `ephemeral_port()` gave
  `PORT_FIRST` (49152) to the first socket of every `netd`. After the test's deliberate `net crash`, the first fetch of the restarted `netd`
  reused the four-tuple 10.0.2.15:49152 -> 10.0.2.2:PORT of the killed `netd`'s connection, which SLIRP still held; its SYNs were swallowed, the
  fetch waited for retransmits (1 to 3 s even in green runs) and sometimes hit `CONNECT_US` (10 s): "the host did not answer", step timeout.
- **Fix**: `Stack::new` seeds the first port with `PORT_FIRST + seed % 16384` (RFC 6056 randomized selection, `Extra::seed_ports`);
  `ephemeral_port` still skips ports in use and wraps 65535 -> 49152. Host tests: different seeds give different first ports, the same seed the
  same port, seeds 0, 16383 (port 65535) and `u64::MAX` stay in 49152..=65535, and the wrap from 65535 goes to 49152.
- **Seed source**: `netd` passes `clock_monotonic() ^ MAC` (user/src/bin/netd.rs), not a constant; it differs between starts. The entropy
  service of ADR-0227 is not wired into `netd` yet, so the seed is still not random (not a secret; unchanged). No doc states a fixed first port.
- Tests: `cargo test -p zoraya-net`, `make check`.

### Tests run (this session)

- `cargo test -p zoraya-settings` (27), `-p zoraya-proto-rooms` (6), `-p zoraya-rooms` (20), `-p zoraya-panel` (14): pass.
- `make test-qemu-one` (HVF, the test kernel, all tests including `t_rooms`): pass; `t_rooms` prints each invariant's line.
- `tests/shell/rooms.script` through the runner against the real kernel and `init`: PASS, 48 steps, `zoraya-fsck` clean.
- `make check`: pass (clippy and fmt included, `adr-check` 0 errors).
- `make test-changed` (every step: `init` and the Makefile changed; HVF, TCG and crash rounds past 2 skipped locally): test-qemu, test-libposix, test-posix, test-posix-gui, test-tour-gui, test-gui, test-net ok; test-files failed once on `conventions.script` (the `room --help` text, fixed in the same branch), then `make test-files test-crash` passed (rooms, conventions, grants and the rest).

### Measurements (HVF, one run, `t_rooms`)

- The keeper's round trip per `Launch` (`home`, average of 50, request, launch job and reply): 24 us.
- Four untrusted rooms at `--mem 8M --procs 12` (a fork bomb and a hog each): each stopped at 8183 of 8192 KiB with 5 processes (the memory
  limit came first; the process limit is `t_quota`'s test), refused 6 to 7 times; a `home` program then started 16 processes and mapped 16 MiB
  in the session job (64 MiB, of which the untrusted job holds 32).
- `memhog` in a room at `--mem 64M` on the 128 MiB console guest: refused after 63,872 KiB; the session's ceiling there is 71,100K, so
  `room set agent-1 --mem 90G` is refused with `above the session's 71100K`.

### Not done

- **The GUI tests**: INV-0269-1's GUI form (a terminal from the dock and the console's shell in one job) and INV-0264-3's GUI half
  (`cpu-spinners`), which need a dock room with limits; the `cpu-spinners` and `disk-filler` gate rows stay `pending`.
- **An untrusted room's `/tmp`** (`--tmp`, a `tmpd` in the room's job per shell, ADR-0265 point 7) and the console shell's own broker and
  `tmpd` in an untrusted room's job (INV-0268-3). `+w` directories only and no `+net` in untrusted lines (ADR-0268 point 7): the line reader
  has no network words, and `room new --untrusted` is refused whole by the gate before any line is built.
- **The quota hint through `Refused` notices** and the rooms overview's form and hover card (Q6b).
- **A run of `init`'s keeper restart and of the reconnect channel in QEMU** (the logic is tested through `t_rooms`' stand-in for `init`),
  and of `zosh`'s prompt rebind. Decided by the architect (2026-11-12): these are Q7's gate tests, the `pending` rows `keeper-restart`,
  `keeper-reconnect` and `prompt-rebind` of `tools/untrusted-gate.txt`; the gate stays closed until Q7 builds them.
- **A POSIX session** (`/etc/zoraya/session`) gets no keeper channel.
- **Mutation testing** of `zoraya-rooms` (not run: the budget went to the integration; the crate is small and `mutants-host` is the
  next step before the merge).

### Handoff

- State: branch `work/q6a` from `main` at 1be11a1, pushed; no candidate tag (the review comes first).
- For the reviewer (a trust boundary and `init`): `crates/zoraya-rooms/src/lib.rs` (`handle`, `launch`, `set`, `collect_idle`,
  `refusals`), `crates/zoraya-proto-rooms/src/lib.rs`, `user/src/bin/rooms.rs` (`Sys`, `serve`, `read_init`, the startup loop),
  `user/src/bin/init.rs` (`start_rooms`, `RoomKeeper`, `answer_reconnect`, the arms for `KEY_ROOMS*`), `user/src/roomsclient.rs`
  (`rebind`, `once`), `user/src/bin/panel.rs` (`launch`), `user/src/bin/zosh.rs` (`join_room_job`, `leave_room_job`, `room_set`,
  `quota_hint`), `crates/zoraya-settings/src/rooms.rs` (`Limits`, `replace`), `user/src/bin/t_rooms.rs`, ADR-0293.
- Review questions: (1) a `Room(R)` channel can call `Launch` without end: is the charge to the room (job record) enough, or should the
  keeper count launch jobs per channel? (2) The lease: is "busy while leased or has processes" right when a launcher dies holding a launch
  job? (3) `Keep` sends a duplicate of the untrusted job with `LIMIT` to `init`; `init` holds it for ever: is that more authority than ADR-0269 point 5 intends? (4) The panel falls
  back to its own job when the keeper does not answer (a trusted room only): acceptable, or should it refuse? (5) `reconnect`: `init` hands a
  `Manage` channel to whoever writes on a launcher's reconnect channel, which only the launcher holds.

### Review fixes (Q6a, the `opus-review` findings)

1. **Leases per channel** (`Running.leases`): `Leave` and `channel_closed(chan, kind)` remove only their own; a room is idle only
   with no lease. Test `a_lease_is_per_channel_and_one_launcher_leaving_keeps_the_others` failed before the change.
2. **Holder by kind**: the BUSY test compares `Launcher` kinds, never channel slots (`a_slot_reused_by_the_other_launcher...`).
   `Keep` carries the last holder's kind (`launcher`), re-sent when it changes; `init` keeps it; a resumed room with processes and
   no known holder answers BUSY. `t_rooms` checks the restarted keeper refuses the desktop on a console-held room (not run yet).
3. **A resumed room is leased** by its old holder's kind until `Leave`, a launch of that kind, or a `Manage` channel of that kind closing
   (round 1 added a 30 s `ORPHAN_MS`; round 3 removed it, see below).
4. **`Keep` before the launch job**: `serve` calls `tell_init` first and closes the channel (no job sent) if a write to `init` failed.
5. **`Keep` has `room_untrusted`** for every room job; `resume` takes the trust from it, not from the line.
6. **`Set` checks the ceilings before looking whether the room runs.** `room set` of a room with no line is now `no such room`.
7. **No launch job, no limited room**: `Limits::needs_job` (`--mem --cpu --procs --conns --tmp`; `--disk` is the volume's and
   applies without a job) makes `join_room_job` and the panel's `launch` refuse when the keeper is absent or gone.
8. **`home`** is refused `Stop` and `Set` by the keeper; `room remove NAME --kill` resolves the line (and refuses `home`) first.
9. **Gate row `keeper` is `pending` again** (its text says what ran: the keeper's channels and a keeper restarted by a stand-in for
   `init`). Not done: a QEMU step that kills the keeper and checks that `init` restarts it and a launcher reconnects.
10. **64 channel places, 4 reserved for `Manage`** (`MANAGE_RESERVED`); a refused attach prints a line.
11. Notes: `resume` starts `seen_mem`/`seen_procs` at the job's counters (no spurious `Refused`); `room set` of an untrusted line takes the
    defaults from the limits of the shell's own job and, with none known, leaves the missing words for the keeper (no made-up
    1 TiB machine); the rollback re-reads the table and puts back only the changed line (a window remains: no file lock);
    `start_rooms` closes both channel ends if `config_connection` fails; `Sys::close` and `get` both use `checked_sub`.
12. Property test `random_bytes_on_every_channel_kind...`: 4000 random or mutated messages per channel kind.

ADR-0293 also records that `init` holding the `LIMIT` duplicate of each job is acceptable (no authority beyond the `LIMIT` it
already holds on the session job; it goes only to a restarted keeper).

### Review rounds 2 and 3 (Q6a)

Round 2 (`b786bc5`): `room remove` always stops; the orphan lease kept while a channel of its kind was attached; `room set` fills
words from the keeper's `Usage`; `Open` and init failures answer a denial.

Round 3 (opus re-review; each test written first and seen to fail on `b786bc5`'s code):

1. ERROR, fixed. The orphan lapsed on time while the console's `zosh` (which rebinds only when a call fails) was idle, so the empty
   room was collected under its launch job. The 30 s limit is removed: the lease ends on a claiming launch, `Leave`, the closing of a
   `Manage` channel of its kind, or `Stop`. `zosh` also calls `Keeper::pump` before each prompt and so binds to a restarted keeper
   before its next command. Tests `a_resumed_rooms_lease_ends_only_when_its_kind_of_channel_closes` and
   `a_resumed_room_keeps_its_holders_kind...`.
2. Fixed. The test of the previous fix did not fail without it; the `manage` list it tested is gone, and the new test uses 0
   processes, a long wait and the closing of the channel (the mutants that drop the `channel_closed` clearing fail it).
3. Fixed by the same change (no `manage` pin remains); the width of the rule (any `Manage` channel of the kind closing ends it;
   a kind that never returns keeps an empty room until `Stop`) is stated in ADR-0293's open questions.
4. Fixed. `Keeper::undo_last_launch` (called by `rooms` when `init` did not take the launch's events) removes the channel's lease,
   restores holder, orphan and launch time, and collects a room the launch made. Test `a_launch_that_init_did_not_take_is_undone`.
5. Fixed. The logic moved to `zoraya_rooms::complete_from_running` (used by `room set`); test
   `room_set_takes_the_missing_words_from_the_running_rooms_usage_or_refuses`.

Round 4 (opus re-review of `0411eea`; tests written first, seen to fail on it; ADR-0293 updated):

1. Fixed. `Undo` carries the channel's earlier lease and `undo_last_launch` puts it back (a second launch of `zosh` into a room it
   holds, undone, no longer deletes the first lease). Test `undoing_a_second_launch_of_a_channel_restores_its_earlier_lease`.
2. Fixed. `pump` and `call` rebind only on `PeerClosed` (`zoraya_rooms::keeper_is_gone`, host-tested); a `BufferTooSmall` or a 5 s
   timeout against a slow live keeper keeps the channel. Plus the lease move of 3: a reconnect loses no lease. The client code
   itself is not exercised in QEMU.
3. Fixed. `rooms` attaches one `Manage` channel per kind: a second one replaces the old (`Keeper::replace_channel` moves its
   leases). Stated as an invariant in ADR-0293. Test `a_replacement_manage_channel_takes_over_the_leases_of_the_one_it_replaces`.
4. Fixed. Test `undoing_a_launch_into_a_resumed_room_restores_its_orphan_lease` (it passed on the old code, as the finding says; the
   hand mutant `R-undo-drops-the-orphan` in `tools/mutants/rooms.toml` is what proves it).
5. Fixed. `undo_last_launch` pushes a `Created` with the restored holder when the holder changed. Test
   `undoing_a_launch_that_changed_the_holder_tells_init_the_old_holder`.
6. Recorded as a limitation in ADR-0293's open questions: up to 5 s per prompt while `init` does not answer and the keeper is down.

Not run: the `zosh` prompt rebind and the `rooms` undo path are not exercised in QEMU (the `rooms` binary's `!told` branch needs
a failing write to `init`).

### `/system` read-only (branch `work/system-ro`, on `main` at 1be11a1; the architect's decision on review finding 9)

The architect decided: "/system is read-only at run time for everyone, the console shell included; fsd serves it read-only and no grant can give write to it (+w /system is refused with a hint). It changes only when the image is built (later: an explicit system-update path, its own ADR)." Recorded as the `### Decision` of ADR-0289 (proposed; status untouched) and as the decided note of ADR-0228's open questions (accepted; its "Architect's decision" section untouched).

- **Built.** `Service::seal(name)` in `zoraya-fs-service`: each record knows whether it is outside, the root, or sealed (from the path it was opened by); `has` never gives `WRITE`, `CREATE`, `REMOVE`, `RENAME` or `PUBLISH` on a sealed record, so no clone, narrowing, token, `set_size`, `set_attributes` or limit can write, and `open` with a create, truncate or write right under the seal is refused before the engine runs. On the root, the sealed entry cannot be removed, renamed (either side, exchange included), published over, made or given a limit. `fsd` seals `system` on a disk volume (not on `--memory`). The shell refuses `+w` at or under `/system` (`system_is_read_only`, in `grants` too) with the hint "the system image changes only when it is built; use +r to let a command read it". Writers of `/system` at run time: none (`init` and `fsd` do not touch it; `zoraya-mkfs --from rootfs` writes the image on the host, before the volume is served; the test Makefile targets copy the test root into the host directory before `zoraya-mkfs`).
- **Tests.** `cargo test -p zoraya-fs-service --test service a_sealed_entry_is_read_only_for_every_connection` (fails without the `seal` call); `cargo test -p zoraya-shell` (`a_write_grant_never_reaches_the_system_directory`); `cargo test -p zoraya-tls --test handshake a_name_outside_the_roots_constraints_fails_before_any_byte_is_sent` (new leaf `tests/net/tls/outside.pem`, `NameConstraintViolation`, no application byte); QEMU: `tests/shell/system-ro.script` in `make test-files` (run alone here, 68 steps, then `zoraya-fsck` clean): the console shell's redirections, `cp`, `rm`, `mv`, `mkdir`, a refused `+w /system...`, and a `+w /` grant do not change `/system/tls/roots` (size and first line as before), and `/home` is still writable.
- **Invariants.** INV-0289-4 added (checked), INV-0289-1 names the name-constraint test. ADR-0289's limitation (review finding 9) is closed.
- **Decisions of mine, small.** The seal is by name on the root of a disk volume, in the service (not in the engine or the ABI); `+w /` stays allowed because the volume refuses what it covers under `/system`. No wire change.
- **Handoff.** State: branch `work/system-ro`, pushed without a candidate tag. For the reviewer (a security boundary, `opus-review`): `crates/zoraya-fs-service/src/lib.rs` (`Seal`, `has`, `seal_after`, `is_sealed_name`, `open_as`, `clone_connection`, `rename`, `publish`), `user/src/bin/fsd.rs`, `crates/zoraya-shell/src/eval.rs`, the three tests and `tests/shell/system-ro.script`. Question: the seal is by the name of the root's entry; a volume that is not the root volume and has a `system` entry gets it too (only `fsd` on a disk does; the only disk is the root volume).

## Q6b: what the person sees (branch `work/q6b`, on `main` at 1e9b1aa)

### Built

- **Step 0 (disk).** 42 GB free (threshold 25 GB): nothing removed. The cleanup of merged worktrees was denied by the
  permission classifier and was not needed; the worktree `zoraya-q6b` was made beside the others. The ports archives (15 MB) were
  copied from the orchestrator's `build/ports/src`, read only, as in Q6a.
- **One reading** (`zoraya-settings`' `usage` module; 26 host tests): `Reading` (the keeper's record in plain numbers),
  `top_table`, `room_words` (`--mem 64M (58M)`), `Reading::card_lines`, `limits_line` (the text of `grants`), `pills` (at
  `NEAR_PERCENT` = 90), the notice's lines (`(refused 3 mem)`, `raise: room set agent-1 --mem 128M`), `to_wire`/`from_wire` (total),
  `RoomRow` for the settings window, `Throttle` and `watch` (at most one reading a second). `zoraya-rooms`: `reading_of` and
  `Tracker` (the CPU share since the last reading, 200 ms at least).
- **`top`** (builtin of `zosh`, ADR-0294 point 1): `top`, `top ROOM`, `top --watch`; `room` shows the use after each limit of a room
  that runs. A room's shell asks the keeper for its own room by name (a `Room(R)` channel is refused the empty name).
- **The panel** reads `Usage` on its once-a-second tick and sends `SetUsage` (new request 44, client manager only) when the text
  changed; answers `RoomsQuery` and `SetRoomLimit` from the settings window (new, ADR-0294 point 8).
- **The compositor** (`usage.rs`): the accounts of untrusted rooms (3 surfaces, 2 display areas; 12 and 8 together; 4 accounts;
  `OVER_QUOTA` = 8 in `CreateToplevel`, `CreatePopup`, `SetBuffer`); `SetUsage` all or none; the pill in the title bar at 90%
  (before the grants); the hover card outside the window (above or below it) of a room with a limit.
- **The settings window**: a `Rooms` section (scrolled page) with a field per limit word, the use beside it, the `room set` line
  of what is being edited and Apply, through the panel; the window holds no new right.
- **Tests and documents.** `tests/gui/limits.script` (`make test-gui`, in `tools/ci-tests.txt`), `tests/shell/rooms.script` (`top`,
  `room`), `memhog --hold`, the two tour scripts list `top`; ADR-0294 (proposed), `docs/design.md` ("What the person sees of limits"),
  the invariants table (INV-0262-1/-3 compositor, INV-0266-3, INV-0267-1/-2 checked; INV-0294-1 to -3).

### Decisions where the ADRs are silent (proposed ADR-0294; recommendations)

1. `top` is a builtin, not a program with a keeper slot (the shell holds the channel; no new designation rule). Accept, or send back.
2. The settings window edits limits through the panel (`RoomsQuery`, `SetRoomLimit`), not over a `Manage` channel of its own: no
   application gains launch authority. A settings window of an untrusted room is refused. Accept, or send back.
3. The notice is text on the card and in `top ROOM`, not a `Raise` button; the rooms overview is not built (it is not built in stage
   7 either). The card appears only for a room that has a limit, so a room without one loses no text under it.
4. `OVER_QUOTA` is a new `status` of the compositor protocol (`Status` is `Below<9>`).
5. The compositor's untrusted accounts are keyed by the record's room; one room's applications share one account.

### Where a number or text from outside enters, and where it is checked (for the reviewer)

- `SetUsage` text: only on the client manager's channel (`clients && none` in the ordinal match); `usage::from_wire` is total
  (every line a reading with a room name, at most 33, names distinct, flags 0 to 3); the compositor draws numbers it formats
  itself, never the panel's words.
- `SetRoomLimit` / `RoomsQuery`: only from `app == "settings"` launchers; the room must be in the table (`with_limit`), the word and
  value go through `Limits::set`; the panel refuses when the window's room (as launched) is untrusted; the keeper checks the
  ceilings and the old line goes back if it refuses.
- Window requests of an untrusted client are bounded before any mapping (`within_account` before `map_buffer`).

### Invariants delta

INV-0262-1 and -3 (compositor), INV-0266-3, INV-0267-1, INV-0267-2: checked (rows say how). INV-0294-1, -2 checked, -3 checked after the review
(the panel's handler is `zoraya_panel::set_room_limit`, host-tested). Table 522 rows. `make adr-check`: 0 errors.

### Tests run (this session)

- `cargo test -p zoraya-settings` (42), `-p zoraya-rooms` (43), `-p zoraya-compositor` (176, 7 new), `-p zoraya-settings-window` (9),
  `-p zoraya-proto-chooser`, `-p zoraya-proto-gui`: pass.
- `make check`: pass. `make test-changed`: host tests, test-boot, test-qemu, test-libposix, test-files (with `rooms.script`'s new
  `top` steps), test-crash, test-posix-gui, test-tour-gui, test-gui (with the new `limits` run), test-net ok; test-posix failed once
  on the tour's `help` list (it lacked `top`; fixed in the tour scripts), then `make test-posix test-tour-gui` passed and
  `make test-gui GUI_RUNS=limits:auto:1` passed again (37 steps) after the last change. TCG runs and rounds past 2 were skipped.

### Measurements (HVF, one run, `tests/gui/limits.script`)

- The panel's once-a-second reading, average of the first 5 ticks: `Usage` from the keeper and the text 866 us (tour-gui with no
  room: 53 us), the send of `SetUsage` 1,227 us. At one tick a second that is about 0.2% of one CPU.
- A room at its memory limit: `top` showed `65473K of 64M`-class rows, the pill `mem 99%` was in the title bar within the 2.5 s
  pause, and the card listed `(refused 1 mem)` and `raise: room set project --mem 128M` (shots `build/shots/limits-pill-*`, `limits-card-*`).

### Screens

Shots of the run: `limits-calm` (`top`, `grants` in room `project --mem 64M --procs 16`), `limits-pill` (title bar `zosh -- ~/project  [mem 99%] [+w ~/project] ...`),
`limits-card` (the card: `project`, the grants, `mem 65473K of 64M`, `procs 3 of 16`, `limits --mem 64M --procs 16`, `(refused 1 mem)`,
`raise: room set project --mem 128M`). Terminal transcript:

```
project ~/project> top
ROOM     MEM           CPU  PROCS    DISK  CONNS
project  5450K of 64M  -    2 of 16  -     -
session  5450K         -    2        -     -
project ~/project> grants echo
limits --mem 64M --procs 16  (room project)
```

### Not done

- The rooms overview (System+R) and the `+` chip's form with the limit fields; `Raise` as a button; a pill on the badge and the dock chip.
- `top --session` and the job tree of `top ROOM` (no kernel call lists a job's children; the keeper has no session numbers).
- Disk and connection use in `top` and the card (no launcher forwards the services' counts: `Report` has no sender yet).
- The compositor's refusal count reaching the keeper (`Compositor::accounts` has it); the notice on a refused window request.
- The settings window's Rooms section and `SetRoomLimit` in QEMU (no GUI script clicks it); the handler's logic is host-tested since the review.
- A card that waits for the pointer to rest (it appears at once while the pointer is on a title bar of such a room).
- Mutation testing of the new pure code: run after the review, `tools/mutants/limits-q6b.toml` and `limits-q6b-panel.toml` (see below).
- The gate is untouched: `room new --untrusted` is still refused (`make check-untrusted-gate` passes); no code from outside the
  system image runs as untrusted.

### Handoff

- State: branch `work/q6b` from `main` at 1e9b1aa, pushed, no candidate tag (the review comes first: the compositor and the panel
  are a trust boundary).
- For the reviewer: `crates/zoraya-compositor/src/usage.rs` (`within_account`, `set_usage`, `card_*`), its hooks in `lib.rs`
  (`create`, `create_popup`, `set_buffer`, the ordinal 44 arm, `remove_window`), `draw_rooms.rs` (`win_views`, `card`),
  `user/src/bin/panel.rs` (`tell_usage`, `set_room_limit`, `write_rooms`, `launcher_requests`), `crates/zoraya-settings/src/usage.rs`,
  `rooms.rs` (`with_limit`), `crates/zoraya-rooms/src/lib.rs` (`reading_of`, `Tracker`), `crates/zoraya-settings-window/src/lib.rs`,
  `user/src/bin/settings.rs`, `user/src/bin/zosh.rs` (`top_builtin`, `readings`), `tests/gui/limits.script`, ADR-0294.
- Review questions: (1) is `SetRoomLimit` from a trusted room's settings window acceptable (it can raise another room's limit, as
  `room set` can from a trusted shell)? (2) the compositor groups accounts by the record's room name: can a trusted client
  influence its record (it cannot: `AddRoomClient` is manager-only)? (3) the panel blocks up to 5 s on a keeper that does not
  answer, once a second: acceptable? (4) the card covers the top right of a terminal while the pointer is on its title bar (changed by the review: it is now outside the window).

### Review of Q6b (Opus, head ae426dd) and its dispositions

Fixed (each with a test written first; `cargo test -p zoraya-compositor` 182, `-p zoraya-panel` 19, `-p zoraya-settings`):

1. **Warning, the card could be forged.** The card sat in the window body. It now lies above the window, else below it, else (a window
   as large as the work area) starts on the title bar, which no client draws (`usage.rs`, `card_geometry`). Tests:
   `a_client_buffer_cannot_cover_or_imitate_the_card` (the card rectangle is disjoint from the body, and its pixels do not depend on
   the buffer's colour) and `a_card_with_no_room_beside_its_window_covers_the_frame_first`. ADR-0294 point 4, `docs/design.md` and the
   comments of `tests/gui/limits.script` follow; the script's assertions did not name the card's place.
2. **Warning, the tail of the account test asserted nothing.** Now: Close, then the next request is served. New tests: a popup counts
   as a surface (`OVER_QUOTA` from `CreatePopup`), `SetBuffer` refusals on pixels, a fifth account refused with surfaces to spare, a
   client's death gives its surfaces back. The `all_surfaces <= 12` and `all_pixels <= 8 areas` terms can never bind (4 accounts x 3
   surfaces, x 2 areas): said in `usage.rs` and ADR-0294 point 7; their mutants are equivalent. Hand mutants for the three
   `within_account` calls and the account terms: `tools/mutants/limits-q6b.toml`.
3. **Warning, the untrusted-origin refusal had no test.** The handler moved to `zoraya_panel::set_room_limit` (file and keeper as
   closures); tests for the refusal (nothing read, written or asked), the write and ask, the rollback and a missing keeper;
   mutants in `tools/mutants/limits-q6b-panel.toml`. INV-0294-3 is `checked: make test-unit`.
4. **Note, the pending `SetBuffer` buffer.** `Window::pixels` counts it; a newer one replaces it in the count (test
   `a_new_buffer_counts_in_the_account_until_it_replaces_the_old`).
5. **Note, the card after a restack.** `rehover` runs after a raise, a new window, a removed window and a dropped client (test
   `the_card_follows_a_restack_under_a_still_pointer`).
6. **Note, a wide room name.** Line 0 is measured in the title face it is drawn in.
7. **Note, `refusals` never reset.** Pruned when the room's last client goes (`prune_refusals`; test
   `the_refusals_of_a_room_go_with_its_last_client`). It never has more entries than there are clients.
8. **Note, the size caps.** `MAX_USAGE_TEXT` = 33 x 258 and `MAX_ROOMS_REPLY` = 33 x 467, the worst case derived in
   `zoraya_settings::usage` (`MAX_READING_LINE`, `MAX_ROW_LINE`; test with 33 readings and rows at their longest); a compile-time
   check in `zoraya-panel` ties the protocol constants to it. The panel's send buffer and the settings window's reply buffer were
   raised to fit. `to_wire` therefore cannot cut a room off.
9. **Note, Apply's calls.** Partly fixed: the settings window stops sending after the first answer that says the panel or the keeper
   does not answer (`user/src/bin/settings.rs`, not host-tested). Recorded as a limitation in ADR-0294: no transaction check, one
   call per word.

Recorded, not fixed (ADR-0294, Consequences, "Limitations"): the unlocked read-modify-write of `/config/rooms` (10); `cpu_share` is of
one CPU while `cpu_permille` is of the machine, right only while user threads run on one CPU (11). ADR-0294 point 8 now says outright
that the launch room of the settings window matters only when untrusted, because ADR-0269 gives the settings app `Manage`.

Mutation run (`make mutants`, the two lists, 22 mutants): all caught after the review's tests; three tests were added for survivors
of the first run (SetBuffer refusal, the pending buffer, the fifth account, the rehover after a close). The two totals' mutants are
not listed (equivalent).
**Second review of Q6b (Opus, head after the first fixes).** Fixed, each with a test written first:

1. **Warning, the card's damage.** `Compositor::card_sync` (run before each frame is composed) damages the rectangle the card was
   last drawn at and the one it has now; so a drag, a tiling, a resize and a shrinking reading leave no stale pixels. Tests compare
   the screen with a forced full redraw (`the_card_of_a_dragged_window_is_damaged_where_it_was_and_where_it_is`, the tiled and
   shrinking variants). The first test run found the halo one pixel short of the material's blurred edge; `card_rect` adds it.
   The tiling test passes also without `card_sync` (the card then goes through `rehover`, now called by `tile`).
2. **Warning, a hidden window.** `set_hidden` calls `rehover`; `card_geometry` is None for a hidden window (`a_hidden_window_has_no_card`).
3. **Note, the left limit.** The card's x is clamped to the screen (`the_card_stays_on_the_screen_at_the_left_limit`).
4. **Note, the no-free-side case** is recorded in ADR-0294 point 4.
5. **Note, the compile-time check** moved to `user/src/bin/panel.rs` beside the `MAX_ROOMS_TEXT` assert.
6. **Note, the keeper test** now has a concurrent edit (`a_refusal_of_the_keeper_keeps_an_edit_made_while_it_was_asked`); the old
   test is renamed `a_refusal_of_the_keeper_puts_the_old_line_back`.

### Units bound: the reaper's help, the heap's growth and the run search in steps (branch `work/units-fix`, from `work/tbound-fix`)

`t_bound`'s units assertion (`units > 80` between step points) failed twice locally on `auto-el2` inside a loaded
`make test-changed`, with no log; three CI runs failed its contiguous case alone on time (279 to 345 ms masked).

- **Proved first.** Test-kernel tests that force each path, run on the code before the change: an allocation with no
  free frame while a dead object's frames are queued (65 units in its interval), a 128-frame run that waits for the
  reaper (129 units), an allocation 512 KiB past every free span of the heap (267 units). The run search over a
  fragmented map (every other frame free) was one masked pass, bit by bit, with no units counted.
- **Built.** `reaper::take` (an allocation takes one queued frame instead of freeing a step); `alloc_contig_step` in
  `zoraya-pmm` (a resumable, word-wise search; 48 words a step, counted) and step points around each help round and
  before the first zeroing step of a contiguous object; `Backing::LEAST_GROWTH` and `Heap::fits` in `zoraya-heap`
  (the kernel heap grows by 64 KiB at least; user heaps keep 1 MiB); `heap::prepare` (a large growth mapped ahead in
  steps) at `memory_create`'s list and before each step of `spawn`, a snapshot's copy and `finish`, and `memory_map`.
  `t_bound`'s time bound was first put back to 2 ms scaled by the calibration; the review below undid that (1 s off
  HVF again). Details and what is not covered: ADR-0270's last two Q6 notes.
- **Tests.** Test kernel: `an_allocation_short_of_frames_takes_one_from_the_reaper`,
  `a_run_short_of_frames_waits_for_the_reaper_in_steps`, `a_run_search_over_a_fragmented_map_goes_in_steps`,
  `a_prepared_allocation_grows_the_heap_in_steps`; host: `zoraya-pmm`'s
  `a_run_search_in_steps_is_a_first_fit_within_its_budget`, `zoraya-heap`'s `fits` and least-growth tests. `t_image`,
  `t_quota` and the test kernel's `*_from_a_process` compare free frames plus the heap's (the heap grows more often).
  Mutants added to `tools/mutants/kernel-objects.toml` (not run here). A growth of the heap counts the reaper's
  frames as available (`frames::available`): with 64 KiB growths, free frames alone refused one in `t_bound`'s create
  loop (`NoMemory`) on one run.
- **Run.** `make check`; `cargo test -p zoraya-heap -p zoraya-pmm`; `make test-qemu-one`, `make
  test-qemu-auto-smp4-2g`, `make test-qemu-tcg-el2` on the final code, one at a time: each 111 passed, 9 expected
  failures, `t_bound: all passed` (contiguous 4 MiB case 17 us masked under HVF, 76 us under TCG at factor 9).

**Opus review fixes of the units bound** (branch `work/units-fix`, candidate `candidate/work-units-fix-b`):

- **1, 8 fixed (counted, split, spawn measured).** The list of frames grows by `realloc` (allocate, copy, free); the copy
  is counted as `len * 8 / 4096` units, `prepare_step` gives it a step point after it, and `spawn` prepares the list
  before the startup block. The new test `a_spawn_with_a_big_space_is_bounded` (hello with up to 64 MiB more bss,
  through step points in a kernel thread) found a second bug: `spawn` counted its steps per segment, so the last pages
  of one segment and the first of the next made 80 units in one interval; it counts them across segments now.
  `a_space_list_growth_is_counted_and_prepared` shows the copy and the heap growth counted in an unprepared step and
  absent in a prepared one. Not done: building the list outside the lock for `memory_map` (the copy under the lock is
  inherent to a flat list); recorded in ADR-0270 with the other inline paths.
- **2 fixed.** `t_bound`'s bound is 2 ms at factor 1 and a 1 s sanity limit off HVF (the form of `68a22cb`);
  INV-0270-1's status says so.
- **3 partly fixed.** 17 mutants in `tools/mutants/kernel-objects.toml`: 9 killed (the image's `prepare_step`, the
  list's copy counted in `reserve_step` and in `adopt`, `prepare_step`'s reserve and its step point, `available`, the
  reaper's counter, the first zeroing step, the rescan) and 8 survivors with the reason in each note: a snapshot's two
  `prepare_step`s and `memory_map`'s two need a user process whose frame list passes a capacity (the list's capacities are 73 times a power of two, not powers of two: 9,344 at about 8,192 frames, 18,688 at about 16,384; the `t_bound` spawn test passes the first, and a snapshot or `memory_map` case would need a crafted image),
  the stack's and the startup block's are unreachable with `hello`, the segments' count needs a longer segment, and the
  kernel's tables come from the boot pool until 128 MiB of window.
- **4 fixed.** `reaper::held` is a counter; `free_some` and `take` start at a cursor past the emptied items.
- **5 fixed.** `FrameAllocator::generation`; a scan that found no run and saw a free rescans once (kernel test and a
  host test).
- **6 fixed.** The scan's step points are `steps::between`; `memory_create` of a contiguous object ends `Cancelled`
  during its scan.
- **7 fixed.** Kernel page-table frames are in `heap::stats().backing`.
- **Run.** `make check`; `cargo test -p zoraya-pmm`; `make test-qemu-one` (114 passed after the first three new tests,
  116 with all), `make test-qemu-auto-smp4-2g` and `make test-qemu-tcg-el2` (116 passed, 9 expected failures each,
  `t_bound: all passed` on tcg-el2). Two kernel tests that spawn or run the scheduler have to come after the timer
  tests: before them they left a slice deadline queued (`timer_is_off_with_no_deadlines`).

## N6: failure, rooms and the desktop (branch `work/n6`, on `main` at e9c5368), partly done

Done: the register-access kernel call and `--gui --net` (ADR-0221 item 8), the restart reaching the console shell (items 5 and 6),
the `+net` word in the shell, `make bench-net`. Not done: the rooms' `+net` words, untrusted rooms with a network, the pills, the
hover card, the panel's hint and network, the `REPORT` message. They are listed under "Not done".

### Built

- **Register access by kernel call** (ADR-0221 item 8 as the task decided; proposed ADR-0295). Three calls in
  `crates/zoraya-abi/src/lib.rs` (53 to 55): `device_transport(page, index)` (needs `MAP`) returns the `Device` of one transport,
  made at boot in `kernel/src/device.rs` (`Device::make_transports`; one per range, sharing the page's kernel mapping, with its own
  pins, quarantine, reset and interrupt, no mapping); `device_read(device, offset)` (`READ`) and `device_write(device, offset,
  value)` (`WRITE`) on a transport device only. `Device::register_offset` is the one place an offset is checked: a multiple of 4,
  the word inside the transport's block (the device tree's size, at most 0x200); a value above `u32::MAX` is refused. `DeviceObject`
  gained `READ` and `WRITE` in its rights (the page devices have them and the calls refuse them). `device_map` on a transport
  device is `AccessDenied` (`begin_mapping`). No allocation in the calls (the Arcs exist from boot), so no charge (ADR-0261).
- **The drivers** open their device through `user/src/mmio.rs` (`Window::open`: a mapping, or calls when the kernel answers
  `AccessDenied`); `ethd`, `gpud`, `inputd` and `blkd` lost their own `Window` types and no logic changed. `init`'s scan (`user/src/gui.rs`)
  keeps `Page::One` as before and, for `Page::Several`, splits each transport (`device_transport`), reads it with `device_read`, and gives
  it to its driver with range 0. `tools/run-qemu.sh` accepts `--net --gui`. `tools/check-ethd-maps.sh` follows (`Window::open(device,` once).
- **The restart** (ADR-0221 items 5, 6; proposed ADR-0296). `init` already restarted `ethd`/`netd` (N2); now the console shell has a services
  channel at slot 13: `NetworkRestarted` (ordinal 32, one handle, the new root) and `NetworkDown` (33) in `zoraya-proto-net`;
  `init` makes the new root with `narrow` from its control connection (`root_connection`) and sends it; `zosh`'s `network_available`
  reads the channel before a line that needs the network and refuses the line while the root is closed. The test-only `Crash` (40):
  `netd` honours it only if it has an event at slot 8, which `init` gives only if the volume holds `/etc/zoraya/net-crash`; `net crash`
  sends it.
- **`+net` in the shell** (ADR-0224 item 1; proposed ADR-0297): `zoraya_shell::netgrant::{parse_word, net_for, describe}`; the parser, `eval`
  (`Exec::nets`) and `zosh` (`grants` prints the `net:` lines; a value `url` operand designates inside a word).
- **`make bench-net`** (ADR-0230 item 8; `tests/shell/net-bench.script`).

### Decisions where the ADRs are silent (proposed ADR-0295, ADR-0296, ADR-0297; recommendations)

1. **A transport device per transport, calls only on it, `device_transport` needs `MAP` on the page** (ADR-0295). Accept. Open: a process that maps
   the page can still reach every transport; only `init` maps a shared page (once, to read the identifiers). The kernel does not make
   the mapping and the calls exclude each other.
2. **`Size` for the register offset and `Opaque` for the value** in the ABI (no new parameter kind); the allow-list test names the calls.
3. **The console shell only gets the services channel.** The panel and the broker hold no root connection today (a shell started by the panel has
   none), so there is nothing to tell them. Accept for now; they come with the rooms' words.
4. **`Crash` honoured by a flag that is an event at slot 8**, given by `init` when `/etc/zoraya/net-crash` exists (a described program may
   not read argv, INV-0155-2). Accept.
5. **`+net` grammar** as ADR-0297 (any, name, `*.suffix`, address, `ADDRESS/BITS`, `:PORT`; both protocols; any described program).
6. **A line that needs the network is refused while the root is closed** rather than run without a connection (the ADR-0221 example).

### Where a number or text from outside enters, and where it is checked (for the reviewer)

| Input | Entry | Check |
|---|---|---|
| register offset and value | `device_read`, `device_write` in `kernel/src/syscall.rs` | `Device::register_offset`: transport devices only, multiple of 4, `offset + 4 <= range.size <= 0x200` with `checked_add`; the value `u32::try_from`; the rights `READ`/`WRITE` first |
| transport index | `device_transport` | `MAP` on the page, `get(index)` on the 8-slot array, a transport device refuses |
| `+net` argument | `netgrant::parse_word` | total over any text: port 1 to 65535, `valid_name`, dotted address, prefix bits 0 to 32 with no bits past it, at most `MAX_RULES` rules |
| `NetworkRestarted` / `NetworkDown` | `netclient::poll_services` | decoded with the exact handle count; the handle must be a channel; others are closed |
| `Crash` | `netd` before `Service::handle` | only with the event at slot 8 and no handle with the message |

### Invariants delta

`checked`: INV-0221-4 (`make test-net-fetch`: three `net crash` with a fetch after each, the fourth leaves the network down and the next line
is refused), INV-0295-1 (`t_devices`), INV-0295-2 (`tests/gui/net.script`), INV-0296-1 (`tests/shell/net.script`), INV-0297-1 (`cargo test -p
zoraya-shell netgrant`). Not changed (still `not yet applicable (stage 8)`): INV-0221-3 (partial: `t_netd` shows `PEER_CLOSED` for a native client, no
`ECONNRESET` test), INV-0224-1 to -5 (notes in ADR-0224), INV-0230-1 to -4 (as before; `make test-net` runs). adr-check total 526.

### Tests run (this session)

- `cargo test -p zoraya-abi -p zoraya-proto-net -p zoraya-net -p zoraya-shell`: 67, 5, 83, 121 passed (8 new `netgrant` tests).
- `make check` before each commit: all 14 checks pass; adr-check 0 errors, total 526.
- `make test-qemu-one`: 116 kernel tests and `t_devices` (the new `transport_devices`) pass (twice, standalone).
- `make test-gui GUI_RUNS="net:auto:1"`: PASS, 12 steps (`--gui --net`; `net status` read from the screen).
- `make test-net-fetch`: PASS, 156 steps (the +net lines, three `net crash` with a fetch after each, the fourth leaves the network down, the next line is
  refused), `net-dns` 14 steps, the refused TLS servers read no request byte. `make bench-net` runs (HVF).
- `PAR_QEMU=1 make test-changed` (every step, as `init.rs` and the `Makefile` are not in the map; HVF, one guest at a time; the worktree had no
  ports archives, copied from another worktree's cache and verified with `make fetch-ports`): 45 host test binaries, test-boot, test-libposix,
  test-files, test-crash, test-posix, test-posix-gui, test-tour-gui, test-gui (315 s) passed; **test-qemu and test-net failed** on
  `a_spawn_with_a_big_space_is_bounded` (`NoMemory`), an intermittent failure that is not N6's: the same message came from `make test-net` on the
  tree of `main` (ac8d7de, worktree `zoraya-q6b`) in 1 of 3 runs, and on `work/n6` in 2 of 4 standalone runs (the others passed; `test-qemu-one`
  passed 2 of 2). Entered in `docs/backlog.md`. I did not re-run all of `test-changed`.
- Not run: `FULL=1`, TCG, fuzz targets, mutation tests.

### Measurements (HVF, one run)

Register read by call (`t_devices`, 2,000 reads, three boots): 1,393 to 1,419 ns. `make bench-net` (5 runs): 16 MiB 216, 217, 231, 235, 243 ms
(median 231 ms, 72 MB/s); one byte 14.4, 19.8, 27.2, 27.3, 27.3 ms (median 27.2 ms). The restart's duration is not measured (see `docs/design.md`).

### Not done

- The rooms' `+net` and `+listen` words (`room new NAME ... +net HOST`, resolved against the creator's scope, INV-0224-3), the panel giving a
  window's shell a network connection narrowed to its room, untrusted rooms with network words (the gate's stage 2 rows `net-account-flood` and
  `net-fuzz` stay `pending`), the pills and hover card for a network rule (INV-0224-5), the `REPORT` message and the hint (INV-0224-4), `+listen`,
  the roots entry that goes with a network rule, editor highlighting of `+net`.
- A `libposix` client's `ECONNRESET` after a kill (INV-0221-3), the panel's and the broker's services channels.
- Counters in `status` beyond frames: the per-account bounds and refusals exist (N4), `status` does not print them yet.
- TCG for the new steps (GUI net, bench) and the fuzz targets under libFuzzer. No mutation run (the kernel part is small and bounded; the
  checks are in `t_devices`).
- `docs/design.md` has the section "The network on the desktop, and when it fails" and two Known limitations; the roadmap row is unchanged.

### Handoff

- State: branch `work/n6` from `main` at e9c5368, pushed, no candidate tag (`opus-review` first: a kernel call and a driver boundary).
- For the reviewer (kernel, `unsafe`-adjacent, the driver boundary): `kernel/src/device.rs` (`make_transports`, `transport`,
  `read_register`, `write_register`, `register_offset`, `begin_mapping`), `kernel/src/syscall.rs` (`device_transport`, `device_read`,
  `device_write`), `crates/zoraya-abi/src/lib.rs` (calls 53 to 55, `DeviceObject` rights), `user/src/mmio.rs`, `user/src/gui.rs` (`scan`,
  `read_transport`, `classify`), `user/src/bin/t_devices.rs` (`transport_devices`), `user/src/bin/init.rs` (`root_connection`, `net_crash`,
  the arms for `KEY_NETD | KEY_ETHD`, `start_shell`), `user/src/netclient.rs` (`poll_services`, `send_restarted`, `send_down`,
  `crash`, `start_network_with`), `user/src/bin/netd.rs` (the `crash_on_request` check), `user/src/bin/zosh.rs` (`network_available`,
  `grants`), `crates/zoraya-shell/src/netgrant.rs`, ADR-0295 to ADR-0297.
- Review questions: (1) is it right that a page device handle has `READ`/`WRITE` rights that the calls refuse, or should the
  rights be per type; (2) `device_transport` returns a handle with `MAP`: acceptable since `device_map` refuses a transport device
  (`begin_mapping`), or should the rights exclude `MAP`; (3) the mapping of a shared page and the register calls do not exclude
  each other (only `init` maps it): is that enough; (4) the `Crash` hook in a production `netd`: acceptable behind the event handle,
  or should it be a build feature; (5) the shell refuses a line while the root is closed instead of running it without a
  network: the right default?

### Review of N6 (Opus, head 9c2ff92) and its dispositions

Fixed (tests written first where a test can show it):

1. **Warning, a transport and its page did not see each other** (kernel). The quarantine check of a transport device read only its own
   `mapped`, always false, so a driver given a whole single-device page could map it, pin through a transport, end the pin and release the
   transport's quarantine (device reset, frames freed) with the mapping still live; and two processes could drive one transport. Now
   (`kernel/src/device.rs`, `kernel/src/memory/user.rs`, `kernel/src/syscall.rs`): a transport device knows its page (`Weak`, made with
   `Arc::new_cyclic` at boot); the first `device_write` makes the process the transport's *holder* (a `DeviceClaim` in its address space,
   ended by `handle_close` of its last handle to the transport with `WRITE`, or by its exit); a second holder, a holder while the page is
   mapped, the page's mapping or release while a transport has a holder or a live pin, and a transport's release while its page is mapped
   are all `BadState`. Lock order: the page's state, then a transport's. A transport handle has no `MAP` (`init` gives drivers their
   rights less `MAP` for one, `stack::driver_rights`). Test: `t_devices` `a_transport_and_its_page_keep_holders_apart` (the review's
   scenario failed before the fix: `dma_release_quarantine(t) succeeded`; then a second writer in a child, the hold's end with the last
   handle and with the child's exit, live pins refusing the page). New INV-0295-3, `checked: make test-qemu`. Reads take no hold (they
   restart nothing), so `init` identifies a transport without holding it; a hold whose handle was moved away lasted until the process
   ended (corrected by the re-review's finding 1 below).
2. **Warning, `Crash` was a persistent denial of the network.** The Crash path is built only with the `zoraya-user` feature `net-test`,
   which only `make test-net-fetch`'s image has (`build/zoraya-kernel-net-test.elf`, ramdisk `build/ramdisk-net-test.bin`, target
   directories `target/user-net-test` and `target/net-test`; `make ci-build` builds it and `make clippy` lints the feature). In that image
   `netd` honours `Crash` only on `init`'s control connection; `net crash` makes the shell ask `init` on its services channel (after the
   program's own connection is made), `init` sends it, and the `net` program's own `Crash` is answered as an unknown ordinal. `make
   test-net` now fails if the image's `netd` holds `netd: ended on request` (checked: the production `netd` has 0 occurrences, the test
   image's 1). INV-0296-1 reworded. `make test-net-fetch` still does three restarts and the fourth stays down (156 steps, `init: the shell
   asked the network service to end` four times).
3. **Warning, `shell_services` was never closed:** closed next to `shell_reconnect` when the shell ends (`init.rs`). No test (`init`'s
   table is not observable from a test today).
4. **Warning, the gate's network rows:** `net-account-flood` and `net-fuzz` are re-owned to **Q7** in `tools/untrusted-gate.txt`
   (still `pending`); N6 did not build them.
5. **Note, overlapping transport blocks:** `device::init` leaves out a transport whose block would overlap the one before it in its
   page (`follows`), with a message; kernel test `overlapping_transports_are_left_out`. INV-0295-1 says so.
6. **Note, byte access by call:** `device_read` and `device_write` take a width (1, 2 or 4, aligned; narrower than 4 only in the
   configuration from 0x100, since QEMU answers a narrow read of a control register with 0, as `t_devices` showed on the first try).
   ABI change (calls 54 and 55 gain `width: Size`), `zoraya-rt` `device_read_width` and `device_write_width`, `Window::read8`/`write8`
   by call make one byte access. Tests in `transport_devices`. ADR-0295 item 2.
7. **Note, no root:** `zoraya_shell::netgrant::available` (host test
   `a_launcher_with_no_root_refuses_a_line_that_needs_the_network`); `zosh` refuses a line that needs the network when it holds the
   services channel and no root. ADR-0296 item 3.
8. **Note, `+net $X`:** recorded in ADR-0297 item 1 (as ADR-0153 rule 3 for `+r $X`; `X=any` gives full reach, which `grants` shows).
9. **Note, a failed `send_restarted`:** it closes the root it could not move (`netclient.rs`). No test (as 3).

The answers to the handoff's review questions: (2) and (3) are finding 1's fix, (4) is finding 2's, (5) stands with finding 7.

### Tests run (review fixes, this session)

- `cargo test -p zoraya-abi` 67, `-p zoraya-shell` 122, `-p zoraya-proto-net` 5, `-p zoraya-virtio` 38: passed.
- `make check` before each commit: passed; adr-check 0 errors, total 527.
- `make test-qemu-one`: before the fix, `devices_from_a_process` failed on the review's scenario; after it, 118 kernel tests passed
  (`devices_from_a_process`, `overlapping_transports_are_left_out`) with the 9 expected panics. Two of the four runs failed on
  `a_spawn_with_a_big_space_is_bounded` (`NoMemory`, then `first process did not start: OverQuota`), the intermittent failure in
  `docs/backlog.md`, which another agent is looking at; one of those also had the first form of the width test, which read a control
  register a byte at a time and got QEMU's 0.
- `make test-net-fetch` (the new test image): PASS, 156 steps, and `net-dns` 14 steps.
- `make test-gui GUI_RUNS="net:auto:1"`: PASS, 12 steps.
- `PAR_QEMU=1 make test-changed` (every step, HVF, one guest at a time): the checks, 45 host test binaries, test-boot, test-libposix,
  test-files, test-crash, test-posix, test-posix-gui, test-tour-gui and test-gui passed; **test-qemu and test-net failed only on
  `a_spawn_with_a_big_space_is_bounded`** (`NoMemory`; 117 passed, the 9 expected panics), the intermittent failure above. test-net's
  new INV-0296-1 check of the image's `netd` passed before the boot.
- State: branch `work/n6` pushed, tag `candidate/work-n6` on its head for CI's candidate run.

### Re-review of N6 (Opus, head 4cc31c3) and its dispositions

1. **Warning, a `Move` did not end a transport's hold** (fixed). Only `handle_close` ended it, so a driver that moved its only
   transport handle (at `spawn`, over a channel, at `snapshot_process`) kept the hold until it exited: the receiver's first write,
   and `init`'s `device_map` and `dma_release_quarantine` on the page, got `BadState`. One helper, `Calls::end_hold_if_unwritable`
   (`kernel/src/syscall.rs`), now runs after every removal: `handle_close`, the moves of `spawn` and `snapshot_process`, and
   `channel_write` before the message is queued. Test first: `t_devices` `the_hold_ends_with_a_move` (failed before the fix,
   "the child the handle moved to writes", 1 for 0): a held transport moved to a child at `spawn` and its write succeeds; moved
   over a channel, the page maps and is released while the message is queued, and the reader's child writes. Mutants
   `K-transport-hold-kept-on-spawn-move` and `K-transport-hold-kept-on-channel-move` in `tools/mutants/kernel-objects.toml`.
   ADR-0295 item 3a, INV-0295-3 and `docs/design.md` say so; the limitation is gone from Known limitations.
2. **Note, pins are one-way** (recorded). `dma_pin` through a transport is allowed while the page is mapped or another process
   holds the transport (a pin starts nothing; `t_devices` builds the first review's scenario so); a live pin refuses the mapping
   and both releases. ADR-0295 item 3a, INV-0295-3 and `docs/design.md`.
3. **Note, `init`'s overlap `continue` was untested** (fixed). The placing of a transport in its page is `device::place`, which
   `init` calls; the kernel test `overlapping_transports_are_left_out` runs it on a built list through `blocks_kept` (the
   overlapping block left out and the next kept, nine blocks of which eight kept). Mutant `K-transport-overlap-kept`.
4. **Note, the page's lock across a reset's spin** (recorded): ADR-0295 Consequences and `docs/design.md` (bounded by
   `RESET_NANOS`, 100 ms; other transports of the page wait behind it). ADR-0270 is accepted and not edited.
5. **Note, two doc comments** (fixed): `AddressSpace::pop_mapping` has its own again and `hold` only its own;
   `device_transport` says its handle has the device rights but `MAP`.
6. **Note, INV-0296-1's check was a canary** (fixed). `NET_CRASH_MARKS` in the `Makefile` lists a string of each `Crash` path
   (`netd`'s end, `init`'s request and its look for `/etc/zoraya/net-crash`); `make test-net` fails if the image's `netd` or `init`
   holds one, and `make test-net-fetch` fails if its own image's lacks one (a check that cannot go blind). The shell's `net crash`
   path has no string and only asks `init`. ADR-0296 and the invariants table say so.

## Q7: the gate's tests, both stages (branch `work/q7`, on `main` at cd58996)

Sonnet main session. No `opus-review` yet (the branch has no candidate tag; the review comes first). **The gate stays closed:**
`room new --untrusted` is still refused in `zosh` and the panel, `make check-untrusted-gate` passes with the refusal in place, no
code from outside the system image ran as untrusted, and nothing here opens anything.

### Built

- **Fuzz targets** (`fuzz/fuzz_targets/`, `wire.rs` makes messages with valid headers around fuzzed bodies): `spawn_request`
  (`SpawnRequest::decode`, `StartupBlock::decode`, a valid request with bytes changed), `keeper_messages` (the room keeper's
  `handle` on the three channel kinds, a model of the kernel's jobs), `broker_tty` (the POSIX broker on a model kernel, the terminal
  server with keys, output and resizes), `compositor_protocol` (a client's messages and input frames on a fake platform),
  `file_protocol` (`fsd`'s `Service` over a fake transport on a small volume). The network's are the existing `net_rx`, `net_stack`,
  `net_dns_http`. `tools/run-fuzz.sh` builds them with the `nightly` toolchain and LLVM's coverage flags (no cargo-fuzz), runs
  libFuzzer for `FUZZ_SECONDS` each, in parallel, output in `target/test-logs/fuzz-TARGET.log`; `make test-fuzz`, `make test-fuzz-net`.
- **`make test-keeper`** (`tests/shell/keeper.script`, the net-test image): `room crash-keeper` (test image only, ADR-0298 item 2) makes
  `init` kill the keeper under a running room; the script checks `init`'s restart (the room keeps its job, its process and its
  limit: `top`, `grants`, a hog refused at the kept limit), the reconnect (`room set` over the channel `init` gives, applied to the same
  job) and the prompt's rebind, twice in a row. HVF and TCG.
- **`tests/shell/disk-filler.script`** and **`tests/shell/hostile.script`** in `make test-files`; new test program `diskfill` (fills a
  directory until the limit refuses; `--syncs N` and `--loop S` time syncs).
- **Service floods:** `t_storage` (`fsd`: an account of 20 open handles; `clone_account` as the launcher does), `t_gui` (the compositor:
  untrusted-room clients played by the test, `Gui::new_room_client`), `t_netd` (`netd`: `narrow_account` of 3 connections).
- **`tests/gui/spinners.script`** in `make test-gui`; the runner's `latency N MS` step fails if the median key is more than MS over one
  screendump read.
- **`make check-quota-tests`** (INV-0260-1): reads ADR-0260's hard rows, requires each to name a gate row with a step
  (`tools/quota-tests.txt`); it reports the one gap (an untrusted room's `/tmp`) and fails only when the refusal is gone.
- `check-untrusted-gate.sh` now joins the Makefile's `TEST_STEPS` lines (a step at a line's end was not found); ADR-0298 (proposed);
  `docs/adr/invariants.md`: INV-0298-1, -2, and the statuses below.

### Gate rows

| Row | Step | Result (this session) |
|---|---|---|
| keeper-restart, keeper-reconnect, prompt-rebind | `test-keeper` | pass, 55 steps, HVF and TCG (`FULL=1` TCG run once) |
| keeper | `test-qemu` | `t_rooms` (unchanged); the QEMU paths are the three rows above; the GUI form of INV-0269-1 is in `spinners.script` |
| cpu-spinners | `test-gui` | `spinners:auto:1` pass, 67 steps. Two rooms at `--cpu 25%` with two spinners each (jobs `running`): median key 168 ms to a screendump (floor 3.9 ms); the same script's terminal without spinners (`terminal:auto:1`): 188 ms. The bound is relative: at most 450 ms over the median of the same run's no-spinner baseline (`latency N +MS`) |
| disk-filler | `test-files` | pass, 60 steps: the filler stops at 224 KiB of `--disk 256K` (`OverQuota`), home and another room write, 100 write-and-syncs in home next to a sync loop (138193 syncs in 40 s): slowest 0 ms (loop 2 ms) |
| service-floods | `test-qemu` | `fsd`: the room stopped at 19 of 20, another account opened 18, home 100 (pass); compositor: 3 surfaces admitted, 1 refused (status 8), another room and home admitted (`t_gui` pass); `netd` in `net-account-flood` |
| hostile-programs | `test-files` | pass, 48 steps: `stubborn` ended by the second Ctrl-C, `badptr`, `fuzz` (random registers into every call) exit 0 in a room with limits, a hog refused, the shell and home answer, the room removed with `--kill` |
| fuzzing | `test-fuzz` | 180 s each, no finding: `spawn_request` 199M runs, `keeper_messages` 3.0M, `broker_tty` 12.9M, `compositor_protocol` 56k, `file_protocol` 490k (cov 122 / 529 / 1487 / 1605 / 2100) |
| net-fuzz (stage 2) | `test-fuzz-net` | 180 s each, no finding: `net_rx` 8.9M, `net_stack` 16.4M, `net_dns_http` 337M |
| net-account-flood (stage 2) | `test-net` | `t_netd`: an account of 3 connections opened 12 to an address that answers no ARP: 3 admitted, 9 refused `OVER_QUOTA` at once, the root connection still connects. (`make test-net` as a whole fails here at `test-net-posix`: the ports' sources are not in this worktree) |

### Decisions where the ADRs are silent (proposed ADR-0298; recommendations)

1. The tests play the room's client against the production service (account carved as the launcher does) and use trusted rooms with limits for the job's limits; no switch lifts the refusal. Recommend accepting.
2. A test hook on the net-test image: `room crash-keeper` -> `init` kills the keeper; two marks in `NET_CRASH_MARKS` keep it out of the production image.
3. Fuzzing through `tools/run-fuzz.sh` on nightly, 600 s a target in two 30-minute CI jobs, a fetch step outside `make test` (`cargo +nightly fetch --locked` in `fuzz/`).
4. INV-0264-3's GUI half is bounded at 450 ms over the same run's no-spinner baseline, not 50 ms (the runner polls the screen every 150 ms); the 50 ms stays in `t_cpu`. **Architect: accept or ask for a lower bound.** INV-0264-3 was marked `checked` on that basis and is reverted to `not yet applicable` until accepted (Review fixes below); `spinners.script` is a starvation smoke test.

### Invariants delta

INV-0264-3 `not yet applicable` (kernel half checked, GUI half waits for the architect, see 4 and Review fixes), INV-0265-5 `checked: make test-qemu` (`t_storage`) and `make test-files`, INV-0265-3's directory half checked (the `/tmp` half is not
built, status stays `not yet applicable`), INV-0260-1 has its check (`make check-quota-tests`), INV-0298-1 and -2 added. Stage 1
invariants of the gate's list still not `checked`: **INV-0172-3** (stage 7: the compositor's confirm card for a drop or paste into an
untrusted window; no QEMU form), **INV-0260-1** (one resource with no test), **INV-0265-3** (`/tmp` half), **INV-0265-4** (an untrusted
room's `/tmp` and its `tmpd`), and, outside the checked pattern, **INV-0268-3** (broker and `tmpd` in an untrusted room's job); stage 2: **INV-0262-4**
(a `netd` connection's stream rings charged to the client's job: not tested).

### Where a number or text from outside enters (for the reviewer)

`wire::messages` (fuzz) feeds ordinals and bodies; `room crash-keeper` is the only new message on a reconnect channel and is compiled out
outside `net-test`; `diskfill`'s operand and options come from its description; the runner's `latency N MS` reads a number from a script.

### Tests run (this session)

`make check` (all pass); `cargo test -p zoraya-test-runner`; `make test-fuzz` and `test-fuzz-net` at 15 and 180 s; `make test-keeper` (`FULL=1`, HVF and TCG);
`make test-files` (every script, with `disk-filler` and `hostile`: pass);
`make test-qemu-auto-default` (`t_gui`, `t_storage`, `t_quota` etc. pass), `make test-qemu-one`; `make test-net` up to `t_netd: all passed`;
`make test-gui GUI_RUNS="spinners:auto:1"` and `terminal:auto:1`. **Not run:** `make test-changed` (an unmapped path, `Makefile` and `init.rs`,
makes it run every step, and its build step needs the ports' sources, which are not in this worktree: `make fetch-ports` downloads);
the TCG runs of everything but `test-keeper`; the whole of `test-posix*`, `test-tour-gui`, `test-crash`; the candidate and the `candidate-full/` run.

### Not done

- **The gate's open items, for the architect:** the six invariants above; an untrusted room's `/tmp` (`--tmp`, a `tmpd` in the room's job), the broker and `tmpd` in
  an untrusted room's job, the untrusted line reader, `+net` in rooms (ADR-0268 point 7; the stage 2 rows run the services' own accounts, not an
  untrusted `+net` line); the architect's go-ahead; the `opus-review` report of the system-call surface (point 6).
- The fuzz targets reach few states of the keeper (valid names are rare in random bytes) and of the compositor; no structure-aware generator. 600 s a target has not run in CI.
- `sleep 60 &` in a terminal launched from the dock ended at once in `spinners.script` (`[1] done: sleep 60`), while the same line in the console shell's room, and
  `spin &` in the GUI, stay running. Not reproduced by the reviewer; `sleep` now loops until the clock passes its deadline (see "Review fixes"); backlog entry.
- INV-0262-4 (rings charged to the job) and the `status` counters; the TCG runs of `t_netd` flood under `FULL=1`.

### Handoff

- State: branch `work/q7` from `main` at cd58996, pushed, no candidate tag. Commits: fuzz targets and steps; keeper test; `t_netd` flood, `diskfill`, scripts;
  compositor flood, GUI script, ADR-0298; gate file and invariants; quota check.
- For `opus-review`: `user/src/bin/init.rs` (`answer_reconnect`, the `net-test` branch), `user/src/roomsclient.rs` (`crash`), `user/src/bin/zosh.rs`
  (`room_builtin`'s `crash-keeper`), `user/src/netconn.rs` (`start` and `wait`, split from `connect`), `user/src/bin/t_netd.rs` (`account_flood`),
  `user/src/bin/t_storage.rs` (`account_flood`), `user/src/bin/t_gui.rs` (`surface_flood`), `user/src/gui.rs` (`new_room_client`), `fuzz/fuzz_targets/*.rs` (the new ones),
  `tools/run-fuzz.sh`, `tools/check-quota-tests.sh`, `tools/check-untrusted-gate.sh`, `tests/shell/{keeper,disk-filler,hostile}.script`, `tests/gui/spinners.script`, ADR-0298.
- Review questions: (1) is the test hook's reach (a reconnect channel message) acceptable on a net-test image only; (2) are tests that play an untrusted client
  enough for INV-0262-1 in the compositor and `netd`, or must a room's real line exist first; (3) is +450 ms over a baseline an acceptable GUI bound for INV-0264-3; (4) should the full CI
  run the fuzzers 600 s each; (5) the candidate bug above.

### Review fixes (opus-review of Q7, a later session)

- **INV-0265-5** was `checked` on a script whose rooms are trusted (the fold of ADR-0265 item 9 needs an untrusted account, made only by
  `carve`/`clone_account`). New `t_storage::untrusted_sync_folds`: a carved account writes and sends a `sync` it does not wait for; no
  reply within 300 ms while nothing commits; 100 write-and-sync rounds of the session's account each under 1 s (88 ms in all, slowest 3 ms);
  the first commit answers the room's `sync`. **Mutation:** `may_fold` made to return false: `t_storage` panics "the room's sync was
  answered before any commit: it is not folded" (reverted). The status is `checked` on `make test-qemu` (t_storage) plus the script as a smoke test.
  `disk-filler.script` now also fails on `diskfill: sync N: status` and expects the looper's result.
- **INV-0264-3** is back to `not yet applicable (until the architect accepts ADR-0298 item 4's bound ...)`, kernel half by `t_cpu`.
  `spinners.script` stays as a starvation smoke test; ADR-0298 item 4, the gate file and this report say so.
- **prompt-rebind** is tested: the net-test `init` prints "the shell reconnected to the room keeper (a test)" when it answers a reconnect, and
  `keeper.script` expects it right after the restart, before any keeper call. **Mutation:** the rebind in `Keeper::pump` removed:
  `make test-keeper` fails (timed out waiting for that line); reverted. The line is a fourth mark of `NET_CRASH_MARKS`; the mark loops of
  `test-net` and `test-keeper` now fail when the production program is missing, and `test-keeper` builds `$(KERNEL)` first.
- **keeper.script:** `top` is matched with `1 of 16` (`sleep 300` survived); after `room remove agent-1 --kill` the notice `sleep 300: killed` is expected (INV-0269-3).
- **Fuzz:** `FUZZ_JOBS` limits runs (the oldest is waited for; the earlier code waited for a `PID:target` and did nothing); default 2 targets,
  `-rss_limit_mb=1024` (was 3 x 2048); `make test-fuzz` and `test-fuzz-net` skip unless `CI` or `FUZZ_FORCE=1`; the toolchain is pinned to
  `nightly-2026-11-12` (script, workflow, ADR-0298 item 3). Installing it ran `rustup toolchain install`, which also updated rustup itself (1.29.2 to 1.29.3).
  `keeper_messages`: the model's `set_limits` and `create_job` panic when the keeper targets a launch job (the old end-of-input assertion could not fail).
  `compositor_protocol` also sends `AddRoomClient`; its header now says that state carries over between inputs, so a saved artifact is not a per-input
  reproduction. `file_protocol` now runs the volume's check (`zoraya_fs::check`) after a final `sync`.
- **Gate file:** `disk-filler-tmp` (the `/tmp` half, `pending`, owner Q8) split from `disk-filler`; `check-untrusted-gate` requires it; the gate stays closed.
- **Other:** `t_gui` asserts `OVER_QUOTA` (8); `t_netd` asserts the control channel's message has ordinal 17; `tools/test-map.txt` maps `init.rs`,
  `zosh.rs` and `roomsclient.rs` to `test-keeper`; `sleep` loops until the clock passes the deadline.

## Review of the system call surface after stage 8 (opus-review, head `a3866f3`) and its fixes (branch `work/q7-fixes`)

The review is `docs/tasks/TASK-26-syscall-review.md` (ADR-0268 point 6). **Verdict: the gate stays closed.** Two errors (INV-0270-1 did not hold for
the job tree walks and for subscriptions), five warnings and seven notes; 40-odd calls checked and found sound otherwise.

### Fixed on `work/q7-fixes` (one commit each)

1. **Error 1, job tree walks.** `Job::walk` goes in steps of `steps::STEP` visits with `TREE` held inside a step only and `steps::pause` between steps
   (a path of at most 32 frames; dead child slots stay while a walk runs so no index moves; processes are visited from the end of their list).
   `t_bound::job_tree_sites` builds 700 jobs in chains of 12 under one job and measures `job_signal`, `job_suspend`, `job_resume` and `job_kill`
   against 8 jobs (units on every host: at most 80). ADR-0270 has a note that lists the sites (open-question note, no Decision change).
2. **Error 2, subscriptions.** Not stepped (`set` runs in handlers): bounded. 1024 per port and per source, `Error::TooMany`, `LimitReached` for a
   program. **Proposed ADR-0299** (a behaviour change of ADR-0006), INV-0299-1; host tests `a_port_holds_a_bounded_number_of_subscriptions` and
   `a_source_has_a_bounded_number_of_subscriptions`; `t_bound::subscription_sites` at the limit (one run, auto-default: last subscribe 11 us,
   signal of a source 64 us, close of the port 313 us masked). The frees of 1024 `Paid` are timed, not counted in units.
3. **Warning 3, `+net`.** `zosh` refuses a line that would get a network connection while its room is untrusted (`untrusted-gate: net-refusal`; dead code
   while no untrusted room can be entered). `check-untrusted-gate.sh` fails when no such marker exists unless stage 2 is ready and INV-0262-4 is `checked`,
   gate open or closed.
4. **Warning 6, the check.** Markers are counted (the shell's two, the panel's one); a row passes only if its status begins `checked:` and says nothing
   is open or not yet checked; INV-0268-3 is in the list; INV-0262-4 is left to stage 2. **Mutation (reverted):** removing only the marker of
   `room new --untrusted` in `zosh.rs` made `make check-untrusted-gate` fail (15 rows listed); with the gate closed it passes as before.
5. **Warning 4, `stream_create`.** The table's lock is released while the stream is built (`reserve_many(2)`, build, lock, insert).
6. **Warning 5, `device_transport`.** The transport gets the page handle's rights that a device handle has, without `MAP`; `t_devices` case
   `a_transport_has_no_right_its_page_lacks`.

### Review of the fixes (opus-review of `work/q7-fixes` at `7af044c`) and its dispositions

Eight findings, fixed in order, one commit each (on top of `7af044c`):

1. **Error, dead child slots leaked.** `WALKS` is gone. A dropped job leaves its parent's list by `swap_remove`, as a process does. A walk counts down from the end,
   so none is skipped; one may be visited twice (`Job::walk`'s comment, ADR-0270 note).
2. **Error, suspend and resume interleaved (INV-0105-5).** `job_suspend`, `job_resume` and `job_kill` take turns (`Job::Exclusive`, waiting at `steps::pause`, no
   lock held). A caller that was stopped while it waited parks before taking its turn (`process::park_if_stopped`), else its `job_resume` would run right after the
   `suspend` returned. Chosen over an ancestor check under `TREE` because that does not settle two calls on one top job. Test: `t_suspend`
   `a_resume_from_inside_cannot_undo_a_suspend_walk` (a resumer inside 700 jobs; a stress case, since the walks interleave only when a slice ends during one; the
   mutation without the turns was **not** run: it is probabilistic on one CPU).
3. **Error, `remove_matching`.** Each entry is found by binary search on its key and removed, not by a pass over every port's list. Worst case (1024 ports of
   1024 entries, the source's entry first): 1.05 ms on the host, release build (the pass over every list was 1.94 ms there, about 10 ms in the kernel by the
   review's estimate). ADR-0299 states the product bound (16 MiB moved). Host test `freeing_a_full_source_on_full_ports_is_bounded`; `t_bound` `wide-source`:
   32 ports of 1024, `handle_close` of the job 47 us masked (auto-default, factor 4), so about 1.5 ms scaled to 1024 ports, linear in the ports.
4. **Error, `weight`.** A port weighs `1 + subscriptions/16`, and any object with a source `1 + subscribers/16`; `end_subscriptions` and `close` count those units, so
   the 80-unit check sees them on every host. `t_bound` `full-ports`: 16 ports of 1024, each with a job of 1024 subscribers, half closed port first: 779 us masked for the whole
   run. **Smaller than the review's 64:** the test machine has 128 MiB and the heap does not give frames back (warning 7), so 64 ports (65k subscriptions) and 128 ports of
   `wide-source` made later tests fail for want of memory (run and seen). By reasoning, weight 1 would put over 1000 units in one interval and fail the 80-unit check.
5. **Warning, shared per-source bound.** The broker polls (100 ms) a job it could not subscribe to instead of ignoring the error. Not prevented: a per-holder bound
   is bypassed by duplicating the handle, and a reserve needs the kernel to tell the broker from other holders. ADR-0299 records it. Not tested (no test of the
   broker fills a source; the code is small).
6. **Warning, `+net` marker.** `untrusted-gate: net-refusal-call` at each of the two calls in `zosh.rs`, at least 2 required by `check-untrusted-gate.sh`. **Mutation
   (reverted):** deleting the POSIX call and its marker failed the script ("1 of 2 markers").
7. **Warning, tree test.** `t_suspend` `a_large_tree_is_suspended_whole_while_jobs_come_and_go` (put in `t_suspend`, which has the counting children, not `t_bound`):
   700 jobs, 16 counting processes spread through them, a second process making and closing jobs under the top job and a middle job; every job `SUSPENDED` and the 16
   stopped after each of 15 suspends, all running after each resume.
8. **Notes.** `Job::create` takes the parent's `suspended` under `TREE` and makes the new job match (both ways); `Job::walk`'s comment says a second visit can happen.

Run: `make check` before each commit; `cargo test -p zoraya-events`; `make test-qemu-one` three times (the first run failed for the memory above; the second and the last, after the `t_suspend` tree test, passed with
`t_bound` and `t_suspend`). Not run: `make test-changed`, TCG, CI, the broker's polling.

### Review round 2 (opus-review of `work/q7-fixes` at `b94798f`) and its dispositions

1. **Error, moved handles not paced.** `channel_write` collects the moved entries and ends their subscriptions after the table's lock is dropped, each after
   `steps::before(weight)` (error paths too); spawn and snapshot moves call `before(weight)` per handle. `t_bound` `moved-jobs` (16 full jobs in one write).
2. **Error, unfair `Exclusive`.** A ticket queue (FIFO) for `suspend` and `resume`; `job_kill` takes no turn. Not per room (a root-level walk must exclude room
   walks). ADR-0270 note corrected ("bounded by the holder's walk" was only true with one walk; now by the walks queued before).
3. **Warning, vacuous test.** Test-kernel call `FORCE_SWITCH` (0x701a, `qemu-test` only) makes every `steps::pause` ask for a switch; `t_suspend` sets it for
   `a_resume_from_inside_cannot_undo_a_suspend_walk` and the tree test. **Mutation (reverted, run):** `Exclusive::take` returning at once (no turns) made `t_suspend` fail at
   round 0 ("SUSPENDED on a job below", `t_suspend.rs:168`); the unmutated run passes. So the test is no longer vacuous.
4. **Warning, `swap_remove`.** A `dropper` child drops the last handle of one of the first four children of the top job (visited last) while the walk is between
   steps (forced switch); it yields while it waits (spinning cost a slice per forced switch and the first version gave up after 20 s). **Mutation (reverted, run):** `Vec::remove` instead of
   `swap_remove` in `Job::drop` **survived**: it is an equivalent mutant for the walk (the shift makes at most a second visit, never a skip). A skip needs a front-to-back walk, which was not built as
   a mutant; the test would fail it (every one of 700 jobs must be `SUSPENDED`), but that is by reasoning, not run.
5. **Warning, broker poll.** `next_poll` is fixed; the poll runs at the top of the loop whenever `clock_monotonic() >= next_poll`, whatever woke the loop. A subscribe error
   (`BadHandle`) lands in `polled` and is reported as terminated (`object_get_signals` fails). Not run: no test of the broker covers it.

Run: `make check` (all 15 pass); `make test-qemu-one` (last one passes: `t_bound` with `moved-jobs`, `t_suspend` all passed; 9 expected failures as before). Not run: `cargo test -p zoraya-events` (not touched), `make test-changed`, CI.
The first commit of the round holds `FORCE_SWITCH` in `syscall.rs`, whose `steps` side is in the third commit, so only the head builds.
6. **Notes.** ADR-0299 states: the time bound unchecked at factor above 1, the 1.5 ms extrapolated, the gate marker a proxy. `channel::close_one` counts a dropped port's units.

### Review round 3 (opus-review of `work/q7-fixes` at `131e5b5`) and its dispositions

1. **Error, `write` no longer adjacent to `prepare`.** `channel_write` now queues the message right after `prepare` and ends the moved entries' subscriptions (paced, no
   lock held) after `pair.write`; the old ids are dead (`next_id`), so the order does not matter. Spawn and snapshot moves have no `prepare`/`write` pair (they write no
   channel), so they were not affected. Test: `t_bound` `peer_closing_in_a_write` (`FORCE_SWITCH` on, a child closes the peer end at the writer's first pause; the write is
   accepted or refused `PeerClosed`; if accepted the carried end's peer must see `PEER_CLOSED`; the heap returns). In the run all 6 rounds were accepted. **Not run:** the test against the
   old order (no mutant of it), so its power is by reasoning. **Seen:** a spawn and exit leave about 150 bytes of live heap per round (a control run with nothing moved shows it too), so the heap
   bound allows 1 KiB per round; not investigated (record for the architect).
2. **Warning, spinning turn waiters.** A waiter calls `steps::pause` then `sched::yield_now`, so N waiters cost switches, not a slice each. The bound (quota(threads) x quota(jobs) x
   slowdown) is in ADR-0270's notes. **Mutation (run, reverted):** `Exclusive::take` returning at once made `t_suspend` fail (`t_suspend.rs:188`, "SUSPENDED on a job below"). A first
   try of the guard below reset `FORCE_SWITCH` at the start of `main`, which the children run too, so the mutant survived twice; the reset is now in the test process only.
3. **Notes.** The handle's own `steps::count(1)` is counted in the moved paths (channel write, spawn, snapshot). The `failed` path of `channel_write` is commented as defensive and
   unreachable. `t_suspend` sets `FORCE_SWITCH` through a guard (`Forced`, off on drop) and resets it before the first case and at the start of each forced case (a panic aborts, so the
   reset at the start is what protects the next case).

Run: `make check`; `make test-qemu-one` (passes with the final head). Not run: `make test-changed`, CI, `cargo test`.

### Review round 4 (final findings) and its dispositions

1. **Error, last references dropped together.** `channel_write` now consumes `moved_out`, dropping each object after its own `before`/`count`/`end_subscriptions`. The spawn and
   snapshot move paths already drop each entry at the end of its iteration. Test: `t_bound` `moved_memory_is_freed_a_step_at_a_time` (64 memory objects of 63 pages moved to a
   reader that closes them, forced switch). **Mutation (run, reverted):** the old loop (`for taken in &moved_out`) freed 4033 units in one interval and the test failed (limit 80); the fix gives 64.
2. **Warning, heap residue.** Not a kernel leak. A loop of spawn and exit in a room kept open leaves the live heap exactly constant (8 rounds measured; closing the room frees its 544-byte record).
   The 144 bytes a round were the write test's own page mapping, never unmapped. The test unmaps it and now asserts exact equality after round 0. Nothing for `docs/backlog.md`.
3. **Notes.** `moved_out.try_reserve_exact` is above `prepare`; ADR-0270 now says "finds no other thread ready".

Run: `make check`; `make test-qemu-one` (passes with the final head). Not run: CI, `make test-changed`.

### Gate blockers (what remains before the refusal can be removed; also in `docs/backlog.md`)

- **Warning 7:** the heap never returns frames while the charge is refunded (a room's burst of kernel objects moves its ceiling's worth of frames into the heap for
  good). To measure (`HEAP_BACKING`, free frames, on 512 MiB), then a heap ceiling for the untrusted job, or return of free chunks, or the bound in ADR-0261's open questions.
- **Notes 8 to 14**, not fixed: uncharged growth on call paths and the reaper's queue (8); `Paid` dropped under `EVENTS` must never hold the last `Arc<Job>` (9);
  `console_write` keeps IRQ masked about 60 ms at 4 KiB (10); any process can run code from a memory object (11); the refusal lives only in the clients (12);
  contiguous memory is open to every process (13); smaller items: the RTC's rights, calls 53 to 55 under a proposed ADR, `held` counts the job's own record (14).
- **Left over from the fixes:** `Job::drop`'s `retain` and the growth of `children` are still linear in a parent's children, in one interval each; the drop of a long job chain
  runs in whichever call drops the last handle (the walks no longer interleave: ADR-0270 note).
- **Rows the strict check now lists as not `checked:`** (run the check with a marker removed to see them): INV-0172-3, INV-0260-1, INV-0261-2, INV-0261-6, INV-0261-7,
  INV-0262-1, -2 and -3, INV-0263-3, INV-0264-3 (the architect's bound), INV-0265-1, INV-0265-3 (the `/tmp` half), INV-0265-4, INV-0268-3, INV-0270-1 (its status names known breaches).
  INV-0262-4 is needed for `+net` (stage 2).
- The pending row `disk-filler-tmp` (an untrusted room's `/tmp` and its `tmpd`); the architect's go-ahead; a full CI run (`candidate-full/`) of the commit that removes the refusal, then `main`'s.
- ADR-0299 (proposed) needs the architect's decision.

### Red candidate run 43311917098 (`candidate/work-q7-fixes-e`, head 96da472) and its fixes

- **Every test-qemu job and test-net timed out at 300 s** (stopping between test 110 and 117 of 127, wherever the budget ran out; the 9 failures
  are the expected panics). No hang: under TCG on the development host `job_suspend_from_a_process` took 240 s (it used to take a few), of
  which the two forced cases of round 2 took about 235 s (`a_resume_from_inside_cannot_undo_a_suspend_walk` 2 s a round for 40 rounds,
  `a_large_tree_is_suspended_whole_while_jobs_come_and_go` 9 s a round for 15). `FORCE_SWITCH` makes each step point of a 700-job walk
  switch, and every process spinning at EL0 then kept the CPU for a whole 10 ms slice before the walk went on. Fix: while `FORCE_SWITCH` is
  on, the test kernel gives user threads 1 ms slices (`test_calls::FORCED_SLICE_NS`). `t_suspend` then took 39 s under TCG and the whole
  tcg-default run 141 s (PASS); the mutation of `Exclusive::take` returning at once still fails `t_suspend` (HVF, auto-default).
- **`portbench` panicked (`subscribe: LimitReached`) and the test passed**: it subscribed 10,000 times to one port and one job, which
  ADR-0299's bound of 1024 refuses; its panic exit 101 is under `PORT_RATIO_LIMIT` (150). It now measures with 1024 subscriptions and
  exits with `i32::MAX` on any refused call. INV-0006-3's 10,000 is recorded in ADR-0006's open questions for the architect.

### Tests run (this session)

`make check`; `make test-qemu-one` (the whole test kernel on auto-default, which includes `t_bound` and `t_devices`) after steps 1 to 2 and after steps 5 and 6; `cargo test -p zoraya-events`;
the mutation above. Not run: `make test-changed`/`test-keeper` (the shell's refusal is dead code until the gate opens), `make test-qemu` on TCG, CI.

## Hand-run: `make run-net` on the architect's machine (branch `work/hand-run`, on `main` at 120ee31)

**What the architect saw.** `make run-net` on a MacBook Air: `net status` right (lease, gateway, DNS), but `fetch http://google.com`
and `fetch https://google.com` both "zosh: /system/tls/roots: the file service is gone", and the prompt `/>` instead of `~>`.

**Cause.** Not the read-only `/system` (ADR-0289 item 8): the disk. `make run`, `run-net` and `run-gui` attached `build/disk.img`, which
the Makefile made once (`$(DISK): | $(KERNEL)`, order-only) and never again. A disk made before Q5 (format 2, ADR-0265; f7c95e1,
2026-11-11) is format 1, which `fsd` does not mount. Reproduced with this host's own kept `build/disk.img` (from 2026-11-02), copied, under
the exact `make run-net` command line (`tools/run-qemu.sh --net --disk DISK build/zoraya-kernel.elf`, interactive mode, through the
runner): `fsd: mount: Corrupt`, `init: storage failed`, `zosh: there is no volume: the file commands will fail`, the prompt `/>`
(the shell has no home), and after three restarts `init: no more restarts: the volume stays down`; `zoraya-fsck` on that image: "root
records: no valid root record". Every command with a network grant then fails before it runs, because the shell adds
`/system/tls/roots` to its table (ADR-0228 item 3) and opening it on a volume that is gone is `FILE_GONE` ("the file service is gone"),
for `http://` too. With a fresh disk from the same build the same command line gives `~>` and `fetch https://example.com/` prints the
page. A disk made between Q5 and N5 (2026-11-11 to -12) would mount but have no `/system/tls/roots` at all, and since `/system` is
read-only at run time for everyone, no disk ever gets a new root store: the same staleness, quieter.

**Fix.** `tools/run-disk.sh IMAGE`, the recipe of `$(DISK)` (run on every `make run`, `run-net`, `run-gui`): it keeps the disk while
`IMAGE.key` matches what the disk was made for (the format version from `crates/zoraya-fs/src/format.rs`, the size, and a checksum of
`rootfs/system`); otherwise it moves the disk to `IMAGE.old` (one generation), says so, and makes a new one from `rootfs/`. `/system` stays
read-only for everyone; nothing writes into an existing image. Cost: a change of format or of `rootfs/system` takes a person's files on
that disk with it into `build/disk.img.old` (readable by an older build, or `zoraya-fsck`); carrying them over needs the system-update
path that ADR-0289 leaves to a later ADR. The command lines of the three targets are one definition each (`RUN_CMD`, `RUN_NET_CMD`,
`RUN_GUI_CMD` in the Makefile), which the test calls.

**The step that closes the gap: `make test-hand-run`** (in `TEST_STEPS`, `tools/ci-tests.txt`, `tools/plan-lib.sh`, a lane of
`tools/test-par.sh`, the smoke set of `tools/ci-plan.sh`, the 30-minute jobs of the workflow, and `tools/test-map.txt` for the paths that
make the image). It boots `build/zoraya-kernel.elf` (not a test image) with a disk made by `tools/run-disk.sh` at
`build/hand-run-disk.img` (so a person's disk stays), first replacing a planted stale disk without a key and then checking that a second
call keeps the new one byte for byte; then:
- `RUN_NET_CMD` (interactive mode, no `restrict`), `tests/shell/hand-run-net.script`: the prompt `~>`; `net status` with lease, gateway
  10.0.2.2 and DNS 10.0.2.3; `fetch http://10.0.2.2:PORT/` the test page from `tools/test-httpd`; `fetch https://10.0.2.2:TLSPORT/`
  "certificate is not signed by a trusted root"; forbidden throughout: "fsd: mount", "storage failed", "there is no volume", "the file
  service is gone".
- `RUN_CMD`, `tests/shell/hand-run.script`: the prompt `~>` and a command.
- `RUN_GUI_CMD` with `--display-opts none --qmp SOCKET`, `tests/shell/hand-run-gui.script` (the runner's GUI script): the desktop
  starts and its terminal shows `~>`.

**Decisions (for the architect).**
- *HTTPS against the production roots.* The image must be exact, so no test roots overlay: the production store
  (`rootfs/system/tls/roots`) accepts no certificate a test server on the host can present. The step checks that `fetch https://`
  gets its roots, runs the handshake and refuses the test PKI's certificate as unknown issuer, which also shows the roots were read
  (without them the message is the file error). The full HTTPS path with a trusted root is `make test-net-fetch`'s (test image, the test
  PKI's root as `/system/tls/roots`), unchanged. No name is fetched: the image has no `net.hosts`, and SLIRP's DNS forwarder needs the
  host's resolver (as `net-dns` is skipped on CI).
- *What differs from a person's run.* Only the disk's path, stdin (the runner's pipe instead of a terminal; `-serial mon:stdio` is
  kept), and for `run-gui` no window and a QMP socket (the runner reads the screen and types through it; it does not change the
  machine). `GUI_SCALE` is the Makefile's: 2 on macOS, 1 on CI's Linux. In interactive mode the guest can reach the Internet through
  SLIRP; the scripts contact only 10.0.2.2.
- *Seen, not fixed.* `fsd` reports a format-1 volume as `mount: Corrupt`, and `zoraya-fsck` as "no valid root record", not the format
  message `format.rs` has ("format version is not 2: rebuild the volume with zoraya-mkfs"); a hint there would have named the cause.
  While storage restarts, the console shell greets three times and drops the lines typed in between.

**By-hand commands in the reports, `docs/design.md` and the README** (there is no README file) and what covers them:

| Command (where) | Covered by |
|---|---|
| `make run` (design.md "Using the shell", 3a's criterion) | `test-hand-run` (`RUN_CMD`) |
| `make run-net`, with `net status`, `fetch` (this report N1, N2, N3; design.md "Network") | `test-hand-run` (`RUN_NET_CMD`); `settings net.gateway ...` by hand (N2): `test-net`'s `net-settings` on a test command line, backlog for the exact one |
| `zoraya-test-httpd --port 8080` then `make run-net` and `fetch` (N3) | `test-hand-run` (the same server, a port of its choosing) |
| `make run-gui` (TASK-24, design.md "Desktop") | `test-hand-run` (`RUN_GUI_CMD`): boot, desktop, prompt. Backlog: the cocoa window's checks (Retina scale 2 on a real screen, the window resized and the display following it, a click landing where the pointer points, Command reaching the guest; TASK-24 "To check by hand") need a window and a person |
| `make debug` (Makefile, design.md) | backlog: no step starts it and attaches a debugger |
| `make posix-image` and `tools/run-qemu.sh --mem 512M --disk build/posix-disk.img ...` (design.md "Running a workload by hand", TASK-22) | the image is built by `ci-build` and booted by `test-posix` in test mode; backlog: the interactive command line itself |
| `make bench-net` (N6, design.md) | backlog: runs only by hand, asserts only that each fetch ends |
| `make measure-w4`, `make test-posix-long` (design.md, TASK-22) | backlog: not in `make test` by decision (design.md "run only where someone runs them") |
| `zoraya-mkfs --size 64M --limit room=64K ...`, `make test-posix POSIX_SCRIPTS=tests/posix/scenarios.script` (Q5) | `test-posix` (scenarios.script is a CI job) and `test-unit` (mkfs) |
| `cargo test -p zoraya-fs-service --test service ...` (Q3) | `test-unit` |

**Run (this session, on the development host, HVF).** The reproduction above (the old disk and a fresh one, each under the exact
`make run-net` line through the runner, with a timeout); `make test-hand-run` twice (the second after adding the `run` and GUI boots):
the stale disk replaced and the new one kept, `hand-run-net` PASS (25 steps), `hand-run` PASS (12), `hand-run-gui` PASS (9), each disk
clean under `zoraya-fsck`; `tests/shell/hand-run.script` against the old disk fails ("forbidden output: fsd: mount"); `make check`.
Not run: TCG, CI, `make test-changed`. Left for a follow-up session on this branch: a `ping` command, the shipping check of the
Mozilla roots, `run-gui`'s network default.

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

### Tests run

`make check` before every commit. `make test-changed` (every step, because the Makefile and `zoraya-rt` changed; HVF; TCG
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

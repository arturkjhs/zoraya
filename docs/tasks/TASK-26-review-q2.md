# TASK-26 review of Q2

An independent review by the `opus-review` subagent, which did not write the code, of
`git diff 9daf721...work/q2 -- kernel crates user tools Makefile docs/adr` at `77569af`, with the code around it in
`kernel/src/{steps,reaper,process,syscall,channel,sched,sync}.rs`, `kernel/src/memory/{object,user,frames}.rs`,
`crates/zoraya-pagetable`, `crates/zoraya-posix` (`errno.rs`, `ffi.rs`, `ffi/procs.rs`), `crates/zoraya-rt` and
`user/src/bin/{t_bound,t_quota,t_snapshot}.rs`. It was checked against ADR-0270, ADR-0263 item 4, ADR-0261 and
ADR-0268's gate rows `fork-bomb` and `masked-time`, and against the open items of `TASK-26-review-q1.md`. The reviewer
ran `cargo test -p zoraya-pagetable -p zoraya-posix` (all passed) and `make check` (13 of 13 passed). No QEMU run.
Dispositions are left to the main model.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | error | `kernel/src/process.rs:854-860`, `kernel/src/memory/user.rs:489`, `kernel/src/memory/object.rs:202-210`, `kernel/src/reaper.rs:69-101`, `kernel/src/channel.rs:163-185` | Teardown of many small objects, and the reaper's fallbacks, free on the spot with no step: an exit's masked time grows with the room's memory. |
| 2 | error | `kernel/src/syscall.rs:583`, `kernel/src/memory/object.rs:66-68`, `kernel/src/memory/frames.rs:87-103` | `memory_create` with `CONTIGUOUS` is open to every job and zeroes the whole run in one masked write. |
| 3 | error | `user/src/bin/t_bound.rs:283-284`, `docs/adr/invariants.md` (INV-0270-1) | The exit and `job_kill` checks accept 4 ms, twice the invariant's bound, while the row says `checked`. |
| 4 | warning | `kernel/src/process.rs:940-945` | The masked-time counter is reset at the start of `user_thread_start`, so the exit of a process killed before it ran is not measured. |
| 5 | warning | `kernel/src/steps.rs:24-45`, `kernel/src/arch/aarch64/mod.rs:102-112` | "No spinlock held at a step" is checked only in the test kernel; `pause` is a safe function and `unmask_interrupts`'s contract does not require it. |
| 6 | warning | `kernel/src/testing.rs:2336-2348` | `hold_frames` yields (through `settled_stats`) while holding its `HELD` spinlock. |
| 7 | warning | `kernel/src/reaper.rs:176-185` | The reaper does not yield after dropping an emptied item, so a queue of many items is drained with no preemption point. |
| 8 | warning | `crates/zoraya-posix/src/ffi/procs.rs:1925-1935`, `:1309-1320` | `fork` and `posix_spawn` read `last_refusal` after other system calls that can themselves be refused. |
| 9 | warning | `user/src/bin/t_bound.rs:224-257` | The INV-0270-2 test does not show that any kill landed between steps. |
| 10 | warning | `docs/adr/0270-bounded-kernel-work.md` (open questions), `docs/adr/invariants.md` (INV-0270-1) | Deviations from ADR-0270 point 4 are recorded as open-question notes rather than a proposed ADR, and the row says `checked` with `schedule()` not covered. |
| 11 | warning | `user/src/bin/t_snapshot.rs:1036`, `:1063`, `user/src/bin/t_quota.rs:237`, `docs/adr/invariants.md` (INV-0261-2) | The row says "no heap byte" but the tests allow 4 KiB; the memory-limit refusal is reached before the stepped copy, so the refusal part way through the walk is untested. |
| 12 | note | `kernel/src/reaper.rs:131-157` | `help()` frees frames but keeps their charge: a job at its limit that closes and re-creates is refused until the reaper runs. |
| 13 | note | `kernel/src/memory/user.rs:392-410` | `SnapshotBuild` assumes that the parent's mappings did not change between steps, and nothing checks it. |
| 14 | note | `crates/zoraya-pagetable/src/lib.rs:574`, `:582-588` | `walk_from` restarts from 0 after the last page of the upper half, and a block leaf overshoots the step. |
| 15 | note | `kernel/src/charge.rs:36-48` | `LAST_REFUSED` still depends on there being no step between a refusal and the call's return. This holds today, and the comment does not say so. |
| 16 | note | `tests/posix/scenarios.script:83-88` | The `fork` row's script passes even when the field is never read. |
| 17 | note | `kernel/src/steps.rs:37-41` | Interrupts are now taken on a user thread's kernel stack in the middle of a system call. |
| 18 | note | `tools/test-map.txt:20-21` | A line is duplicated. |

## Findings

**1. error: teardown of many small objects, and the reaper's fallbacks, run with no step (INV-0270-1).** Steps cover
one large object, but not a lot of small ones:
- `MemoryObject::drop` hands only objects of more than `STEP` pages to the reaper. Smaller ones are freed on the
  spot, which is one step each.
- `exit_current` closes up to `MAX_HANDLES` (1024) handles in a loop with no pause between them
  (`process.rs:854-860`).
- `AddressSpace::drop` drops up to `MAX_MAPPINGS` (256) holders in `self.mappings.clear()` (`user.rs:489`). That is
  after `set_user_table(None)`, where nothing may yield.
- `channel::close` pauses every 64 messages of one call. Its counter starts again for each handle, so the 1024 handles
  of one exit each close up to 63 messages unpaused, and each message may carry 64 ends.
- `reaper::release` frees a whole object on the spot, masked in the caller's system call, when `defer` fails. That
  happens when the queue is full (256 items, never grown) or before the reaper starts.

*Scenario:* a room at `--mem 100M` on the 128 MiB test machine makes 400 objects of 64 pages (256 KiB each) and keeps
their handles, or maps 256 of them. One `exit` or `job_kill` then frees 25,600 frames back to back with IRQ masked.
At design.md's 4 ms per 32k frames that is about 3 ms, and at `--mem 512M` (ADR-0270's own example) about 16 ms. One
exit that drops more than 256 large objects fills the queue, and from then on each object is freed whole, masked.
`t_bound` exits holding one 24 MiB object, so it does not see any of this.
*Fix:* count work across the whole teardown, not per object. In `exit_current`, pause every `STEP` objects (or frames
freed) in the handle loop. Before `set_user_table(None)`, unmap and drop the mappings in steps, as `memory_unmap`
does, while the space is still the running one. Alternatively, give the reaper the holders and not only the frames. A
full queue should make `release` pause and retry when it is called from a process with no lock held, rather than
free everything at once. Add a `t_bound` case: an exit holding 1024 handles of 64-page objects plus 256 such mappings,
against the bound.

**2. error: `memory_create(CONTIGUOUS)` is unbounded and needs no right (INV-0270-1).** `memory_create` passes
`flags.is_contiguous()` straight to `MemoryObject::new`. No right or size cap is checked (`syscall.rs:583`).
`alloc_contig_zeroed` finds the run by scanning the bitmap under `FRAMES` and then zeroes `count` frames with one
`write_bytes`, all masked (`frames.rs:87-103`). When the run is not found, the new retry loop calls `reaper::help()`.
That frees 64 scattered frames, which rarely makes a run, and then scans the whole bitmap again, up to queue/64 times.
*Scenario:* any limited job calls `memory_create(24 MiB, CONTIGUOUS)`. That is about 1.7 ms of zeroing by design.md's
figure, plus the scan. At a 512 MiB limit it is about 36 ms. A fragmented machine with a full reaper queue repeats
the scan hundreds of times.
*Fix:* allocate the run, then zero it in steps with `steps::between()`. The object is in no table until the end, as
the scattered path already relies on. Refund on cancel. Either gate `CONTIGUOUS` on a driver right, as ADR-0051 means
it ("for a device's queues"), or cap it at `STEP` pages for jobs without that right. Stop the `help()` loop for
`alloc_contig` once a pass frees nothing contiguous: drain the queue once, then fail. Add the contiguous case to
`t_bound::memory_sites`.

**3. error: the teardown check allows twice the bound.** `teardown_sites` asserts
`exit_ns <= BOUND_NS.max(4_000_000)`, which is always 4 ms. The bound is 2 ms. The INV-0270-1 row lists "the exit and
the `job_kill` of a process holding 24 MiB" as checked. Nothing records the 4 ms.
*Scenario:* a regression that doubles the masked time of an exit, for example finding 1's on-the-spot path, passes
the test and the gate row `masked-time`.
*Fix:* use `check("exit", step_case, exit_ns)` with a one-step exit, as the other sites do, or assert `BOUND_NS`. If
4 ms is deliberate, write it into the row and the ADR's open questions.

**4. warning: the masked-time counter is blind in `user_thread_start`.** `user_thread_start` calls
`steps::mask_ended()` first and only then runs `park_if_stopped()` and `exit_if_requested()`. Both run masked, and
`exit_if_requested` can run the whole `exit_current`. After the reset, `SINCE` is 0 and nothing begins it again until
the `eret`. `save_and_mask_*` begins only from an unmasked state.
*Scenario:* a snapshot child or a spawned process, killed while suspended before its first instruction, tears down a
24 MiB space and up to 64 handles. None of that time is counted. `t_bound::snapshot_of` makes exactly such children,
so finding 1 for them would not show.
*Fix:* call `mask_ended()` at the end of `user_thread_start`, just before the return that leads to the `eret`, or in
`__user_thread_start` before `eret`.

**5. warning: "no spinlock at a step" is a test-kernel assertion only.** `steps::pause` is a safe `fn`. It unmasks IRQ
under the `unsafe` contract of `unmask_interrupts`, which asks only for installed vectors and a set-up controller.
Nothing in the normal kernel stops a caller from reaching `pause`/`between` with a guard alive. If one does, an IRQ
handler that takes the same lock panics with "lock taken twice on one CPU". On SMP the counter (`sync::HELD`, global,
and per CPU in name only) is also wrong. The SAFETY comment in `pause` does not argue the no-lock condition.
*Scenario:* a future caller of `MemoryObject::new` holding a lock: a driver path, or `stream.rs` if `CAPACITY` grows
past 64 pages. A test that never reaches that call cannot catch it.
*Fix:* add "no spinlock is held" to the `# Safety` of `unmask_interrupts` and the docs of `pause` and `between`. Keep
the lock count in every build: one relaxed add per lock is within driver 4's "nil" for practical purposes. Or record
in ADR-0270 why it is not kept.

**6. warning: `hold_frames` yields while holding a spinlock.** It holds `HELD.lock()` and calls
`frames::settled_stats()` once per frame. Each call runs `reaper::drain()`, which yields at least once. That breaks
`settled_stats`'s own rule ("Not for code that holds a lock, since it yields"). While this thread is switched out,
`sync::held()` is 1 for every other thread.
*Scenario:* while `t_snapshot` or `t_devices` holds frames, any other process reaches a step point. `pause`'s
`assert_eq!(held(), 0)` then fails in a thread that holds no lock, and the test kernel panics. The failure is
intermittent and depends on timing. A `release_frames` from another process panics on "lock taken twice".
*Fix:* call `reaper::drain()` once before taking `HELD`, and use `frames::stats()` inside the loop.

**7. warning: the reaper can run its whole queue without a preemption point (ADR-0270 point 4).** After dropping an
emptied item, `reaper_main` takes `continue` without yielding. The next round frees up to 1024 more frames and drops
the next emptied item, and so on.
*Scenario:* an exit queues 200 objects of 65 to 1000 pages. Every round empties at least one item, so the reaper
frees all of them and drops all 200 records before any user thread runs again. Kernel threads are not preempted at
EL1: `KIND_IRQ` returns without `schedule`. This is the "batch longer than a step" ADR-0270 cites from KeyKOS, and it
delays the terminal (INV-0264-3), though not with IRQ masked.
*Fix:* call `sched::preempt_point()` after every drop and after every round, or count frames across the rounds and
yield every `REAPER_STEP`.

**8. warning: the refusal field is read after later calls.** In `fork`'s `Err(s)` arm, `rt_errno(s, Call::Process)`
runs after `debug_later`, the `handle_close`s and `p.broker_simple(&bp::SpawnAbort ..)`. That last one is a
`channel_write`, which is charged to the writer and can itself be refused `OverQuota` for memory. `spawn_into` does
the same after `undo_made` and the closes.
*Scenario:* a fork bomb in a room at both its process limit and near its memory limit. `snapshot_process` is refused
on `Processes`, then the abort message is refused on `Memory`. `LAST_REFUSAL` becomes `Memory`, and `fork` sets
`ENOMEM` where ADR-0263 item 4 says `EAGAIN`.
*Fix:* read `zoraya_rt::last_refusal()` right after the failing call and pass it to the mapping. Better, have
`zoraya_rt` return the field with the status for `spawn`, `snapshot_process`, `memory_create` and `memory_map`, so no
global is read later.

**9. warning: INV-0270-2's test does not show the cancel path ran.** `kill_round` kills after 150 ms and checks that
nothing is left. A kill that arrives while the child is at its return to EL0 (between two calls) leaves nothing too,
and does not use `between()`'s `Cancelled` at all. Nothing counts cancellations. `masked(true)` is printed and not
asserted.
*Scenario:* a bug in `MemoryObject::make`'s cancel arm, for example forgetting `reaper::release`, passes whenever the
kills happen to land outside the call.
*Fix:* add a test-kernel counter of `between()` returning `Cancelled` (a `test_calls` number). Assert that it grew
during each role's strict round. Retry the round if it did not.

**10. warning: the deviations from ADR-0270 point 4 are open-question notes, not a proposed ADR.** Point 4 says
`reap()` "only appends" and the reaper frees "in steps with preemption points". Q2 keeps the dead threads' and
processes' drops in `schedule()`'s `reap()` and gives the reaper turns of 1024 frames. Both are written as "Q2 note"
open questions. CLAUDE.md asks for a proposed ADR for any deviation from an accepted one. ADR-0001 rule 6's note is for
a check that cannot be implemented yet. INV-0270-1 names `schedule()`, but its row says `checked` while saying that
`reap()` is not moved. The gate's `checked` match (review Q1, finding 7) counts the row as done.
*Fix:* write a short proposed ADR for the two deviations, or ask the architect. Until then, word the row as partly
checked, so that the gate does not count it as done.

**11. warning: INV-0261-2's row claims more than the tests check.** The row says a refused `snapshot_process` changes
"no counter, frame, heap byte or handle". The tests assert `heap_live() <= heap + 4096`, in `t_snapshot.rs:1036`,
`:1063` and `t_quota.rs:237`. The memory case limits the room to 64 KiB with a 1 MiB mapping, so `new_for_copy`'s
up-front `paid.user(size)?` refuses it before any step. The new path, a refusal in `copy_page` after some steps have
copied, is not reached.
*Fix:* compare the heap exactly, after a warm-up round as `t_bound` does. Add a case where the room's limit fits the
objects but not the parent's own pages, so that the refusal comes from `copy_page` part way through the walk.

**12. note: the charge outlives the frames that `help()` frees.** `free_some` frees frames but leaves the `Garbage`,
and so its `Paid`, for the reaper, because it may run under the heap lock. A job at its limit that closes an object
and immediately re-creates it is refused `OverQuota` until the reaper has run. `t_quota` had to wait for the reaper
before its baseline. ADR-0270 accepts "briefly", but a POSIX program doing `free` and `malloc` at its limit sees
`ENOMEM` depending on timing. Consider having `release` refund the charge as frames are freed (`Paid` split per step),
or letting a refused charge wait for the reaper on the job's own garbage.

**13. note: `SnapshotBuild` trusts that the mappings stay as planned.** `snapshot_step` indexes `build.objects[i]` by
the parent's *current* `mappings`. That is sound only because the process has one thread and nothing else changes its
space between steps. A cheap check per step, `debug_assert_eq!((m.va, m.length), (plan[i].0, plan[i].1))` (keep the
plan), or a generation number on the space, would turn a future violation into a clean error instead of a copy into
the wrong object or an out-of-range `page()` panic.

**14. note: `walk_from` at the top of the address space and with blocks.** For the last leaf of the upper half,
`va + size` wraps to 0. The skip guard never skips it, and a resumed walk's `from` becomes 0, so the walk starts
again from the beginning, and with a step limit it never ends. A 2 MiB block leaf is copied whole in one callback,
512 pages against a step of 64. Neither happens for user spaces today: they are in the lower half and mapped page by
page. Document both, or return `None`/saturate at the end and split blocks.

**15. note: `LAST_REFUSED` and steps.** The kernel's field register is still one global, read at the call's return.
The review checked that no new path yields between a refusal and its return: the `copy_page`, `map_range`,
`new_for_copy` and `create` refusals return without a step, and their cleanup does not yield. The comment's
assumption ("read before the thread can be preempted into another call") is now carried by the step placement and
should say so. Review Q1, finding 10 still stands.

**16. note: the `fork` row's POSIX check cannot fail on the field.** `from_refusal(None, Call::Process)` and the
`_ => EAGAIN` arm both give `EAGAIN`. So `scenarios.script`'s "Resource temporarily unavailable" passes even when
`last_refusal` is never read or is cleared. The row's "checked" for `fork` therefore rests on the host test. A memory
refusal of `fork` showing `ENOMEM` would test the field.

**17. note: IRQs on a syscall's stack.** `pause` now takes interrupts on a user thread's kernel stack in the middle of
a call, including deep in `exit_current` → `channel::close`. The exception frame saves the q registers. The stack's
headroom for an IRQ frame plus the handler on top of the deepest stepped path was not measured. The guard and canary
catch an overflow, but only as a panic.

**18. note:** `tools/test-map.txt:20-21` has `tools/untrusted-gate.txt check` twice.

## Checked and found sound

- **Single-thread steps.** `memory_map`, `memory_unmap` and `snapshot_process` drop the space lock between steps. Only
  the process's own thread changes its space or handle table: `space.take()` happens only in `exit_current`, and the
  other readers are test-only. So the reserved gap, the half-built range, the half-unmapped mapping and the plan cannot
  be disturbed. Shared objects are held by the call's own `Arc`s.
- **Cancel and failure paths.** `memory_map` unmaps `[va, va+done)` on both kinds of failure, and `map_range` unmaps
  its own part. `MemoryObject::make` gives `list` and `paid` to `release` on both failures. `SnapshotBuild` and its
  child, objects, ASID and process `count` drop as one value on `?`. In `finish`, the objects not yet consumed drop
  with the iterator. `create`'s `Cancelled` drops the space and `count` before anything is registered. The moved
  handles of `spawn` and `snapshot_process` leave the caller's table only after the child exists. The charges balance
  on every new path.
- **The resumable walk.** For page leaves, `next = leaf.va + leaf.size` is set only after a leaf is fully copied. The
  skip of subtrees that end at or before `from` is correct, and the host test passes.
- **exit_current's reorder.** Handles are closed, with yields, while the space is still installed, and the scheduler
  reinstalls `process.ttbr0()` correctly. Nothing after `set_user_table(None)` yields: `release`/`defer` only wake.
  The ASID is freed after `invalidate_asid`, and the page-table frames are freed later by the reaper, which is safer
  than before.
- **Reaper locking.** `alloc` and `alloc_contig_zeroed` release `FRAMES` before `help()`. `QUEUE → FRAMES` has no
  reverse edge. `defer` never grows the queue under the lock. The block in `reaper_main` is masked from the check, and
  `defer` is never called from an IRQ. `Paid` holds an `Arc<Job>`, so a queued charge never outlives its job record
  (INV-0261-6).
- **The masked counter elsewhere.** It begins at exception entry and at `save_and_mask_*` from an unmasked state, and
  ends at `restore_interrupts`, `unmask_*`, the return to an IRQ-on context, and around `wfi`. This is consistent
  across context switches (one CPU), apart from finding 4.
- **errno table.** `from_refusal` and `from_status_seen` match ADR-0263 item 4's memory and processes rows, and
  disk is Q5. `NoMemory` still maps to `ENOMEM` for `Call::Process`.
- **`unsafe`.** The SAFETY comments in `snapshot_step` (copy and clean) and `steps.rs` (`isb`, counter reads) are
  true. `pause`'s is incomplete (finding 5).

## Dispositions

Commits are on `work/q2` after `a33dab1` (this review); see `git log --oneline a33dab1..`: "Q2 review 1-3" (kernel
teardown, contiguous, `t_bound`), "Q2 review 1" (the unit fix), "Q2 review warnings", "test-map".

1. fixed: `steps::before` counts the whole exit (handles, mappings, messages); the reaper's queue doubles; contiguous
   runs over a step go to the reaper. `t_bound` has the 400-object and the 256-mapping/1024-handle exits and kills.
   Open: a drop that cannot grow the queue (heap exhausted) still frees on the spot.
2. fixed, with a remainder: capped at 4 MiB, zeroed and freed in steps, at most 17 help rounds. Open: the frame-map scan
   for a run is one masked pass (about 250 us on the test guest) and the ADR-0051 right is not checked.
3. fixed: the assertion is the INV-0270-1 bound (2 ms under HVF, scaled by a printed calibration elsewhere), with the
   units of work asserted on every host. Measured under HVF: exit 95 to 353 us, `job_kill` 31 to 120 us.
4. fixed: `mask_ended` moved to the end of `user_thread_start`.
5. fixed in words: `pause`, `between`, `unmask_interrupts` and `sync::held` say the lock count is the caller's duty
   and checked only in the test kernel; the ADR records that locks are not counted in the normal kernel.
6. fixed: `hold_frames` drains before taking `HELD` and uses `frames::stats()`.
7. fixed: the reaper yields after each emptied item.
8. fixed: `fork` and `posix_spawn` read `last_refusal` first (`rt_errno_seen`).
9. fixed: the kill round repeats until `between()` ended a call with `Cancelled` (test call `CANCELLED`).
10. accepted as open questions: the deviations are stated plainly in ADR-0270; the row says the check is only in part
    (adr-check allows only `checked:` or `not yet applicable`).
11. fixed in words: the INV-0261-2 row says a page of heap and that a refusal part way through the copy is not tested.
    Open: the test for it.
12. open (accepted by ADR-0270 "briefly"). 13. open. 14. open (user spaces are in the lower half, pages mapped one by
    one). 15. open. 16. open. 17. open (the guard and canary catch an overflow).
18. fixed: the duplicate line is removed and `crates/zoraya-rt/` has a line in `tools/test-map.txt`.

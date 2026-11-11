# TASK-26 review of Q1

An independent review by the `opus-review` subagent, which did not write the code, of
`git diff origin/main...work/q1 -- kernel crates tools Makefile user/src/bin/t_quota.rs` at `91c963e` (with
`user/src/bin/init.rs` and `user/src/gui.rs` read for the session job). It was checked against ADR-0260, ADR-0261,
ADR-0263, ADR-0268 and the plan's row Q1. It covers the charged paths, refunds, lock order, the failure value register,
the ADR's rights and limits, `unsafe`, and the two check scripts. The reviewer ran
`cargo test -p zoraya-charge -p zoraya-channel -p zoraya-events -p zoraya-heap -p zoraya-abi` (all passed),
`tools/check-charged-alloc.sh` (ok) and `tools/check-untrusted-gate.sh` (ok, 9 stage-1 tests pending). No QEMU run.
Dispositions are left to the main model.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | error | `kernel/src/process.rs:570-572` (`assemble`) | A refused kernel charge leaks the process's event source. |
| 2 | error | `kernel/src/syscall.rs:621-625`, `:806-810` | `memory_map` and `device_map` return `NoMemory` when a page-table charge is refused. |
| 3 | warning | `kernel/src/process.rs:680`, `user/src/beside.rs:115` | `init` gets `LIMIT` on its own job, which carries the kernel reserve, and `JobObject::RIGHTS` now hands `LIMIT` wherever it is used. |
| 4 | warning | `user/src/gui.rs:384`, `user/src/bin/panel.rs:106`, `user/src/bin/init.rs:84-104` | The panel's launches are not under the session job, and `session()` silently falls back to `init`'s job. |
| 5 | warning | `kernel/src/port.rs:16`, `kernel/src/job.rs:342-357` | A `Paid` dropped under `EVENTS` may hold the last `Arc<Job>`, and `Job::drop` takes `EVENTS` again. |
| 6 | warning | `tools/check-charged-alloc.sh` | The lint does not enforce INV-0261-5 beyond `uncharged_*` and `Arc/Box::new(`. |
| 7 | warning | `tools/check-untrusted-gate.sh:63-68`, `docs/adr/invariants.md` | The gate counts a row as `checked` when it contains the word, including rows whose text says half is open. |
| 8 | warning | `kernel/src/process.rs:560-564`, `:847` | Every process is charged a full 1024-slot handle table, and a zombie keeps that charge after the table is freed. |
| 9 | note | `kernel/src/memory/user.rs:67`, `:250`, `kernel/src/job.rs:118`, `kernel/src/process.rs:187`, `kernel/src/syscall.rs:103`, `kernel/src/device.rs:315-319` | Some allocations on system call paths are uncharged. |
| 10 | note | `kernel/src/charge.rs:36-48` | `LAST_REFUSED` is sound only under assumptions the code does not check. |
| 11 | note | `kernel/src/memory/user.rs:202-205`, `:404` | `pay_user` sets `refused` and the `Quota` path does not take it back. |
| 12 | note | `kernel/src/job.rs:163-169` | `held` includes the job's own record. |
| 13 | note | `kernel/src/process.rs:768-772` | The comment says 1/16; the constant is 1/32. |
| 14 | note | `kernel/src/memory/user.rs` `snapshot`/`copy_page` | `snapshot_process` is not refused before copying. |
| 15 | note | `crates/zoraya-charge/src/tests.rs:102` | The model test has no kills. |

## Findings

**1. error: a refused kernel charge for a new process leaks its event source (INV-0261-2).** `assemble` calls
`port::new_source(bits)?` and only then `charge::arc(count, extra, |paid| Process { .. source .. })`. `charge::arc`
charges first and returns `Err(OverQuota)` without calling `build`. The closure is then dropped, and so are the values
it captured (`space`, `owned`, `handles`). `source` is a `SourceId`, which is `Copy`, so nothing frees it:
`Process::drop` is the only thing that does, and no `Process` exists yet. The old comment "its `Drop` freed the source"
is true only when the allocation fails, not when the charge is refused.
*Scenario:* a job near its memory limit, say a hog holding all but 40 KiB. The process count passes and the image
frames fit, but the kernel charge for the record and the table (about 57 KiB) is refused. Each such `spawn` or
`snapshot_process` leaves one live source in `EVENTS`. A loop of refused spawns grows `EVENTS.sources` without bound,
on the kernel heap, uncharged, until the kernel reserve is gone. `failed_allocation_frees_each_object_once` would show
it as `EVENTS.live()` drifting, but it covers only `FAIL_NEXT_ARC`, not a refusal.
*Fix:* charge before `new_source`, so that `charge::arc`'s `build` creates nothing that can fail. Or call
`paid.kernel(arc_cost + extra)?` on `count` before `new_source` and then use `prepaid_arc`. Add a `testing.rs` case: a
job limited just above a process's frames, a refused `spawn`, then `EVENTS.lock().live()` back where it was.

**2. error: page-table refusals at `memory_map` and `device_map` return `NoMemory` (ADR-0263 point 1).**
`map_object` returns `MappingError::Quota` when `alloc_table`'s charge is refused. Both handlers map every error
except `Limit` to `Status::NoMemory`. Meanwhile `refused()` has already counted the refusal and asserted `OVER_QUOTA`.
So the caller sees "the machine is out" with x1 = 0, while the keeper sees a refusal on the job. Driver 2 of ADR-0263
is exactly this distinction.
*Scenario:* a limited job maps a large memory object received from another job, which is paid by its creator
(ADR-0261 point 8). The mapper pays only page tables, and the first table over the limit returns `NoMemory`.
*Fix:* add `MappingError::Quota => Status::OverQuota` in both `map_err`s, and a `t_quota` case that maps a foreign
object at the limit.

**3. warning: `LIMIT` on the reserve job, and `LIMIT` handed on implicitly.** `kernel_handles` gives `init` its own job
with `JobObject::RIGHTS`, which now includes `LIMIT`. That job's parent is the root, which has no limit, so
`job_set_limits(JOB, {memory: None, ..})` from `init` succeeds and removes the kernel reserve (ADR-0261 point 10 and
driver 3: no limit is raised from inside). The same widening reaches every existing `dup(.., JobObject::RIGHTS)`, such
as `beside.rs:115`, which hands a POSIX child its own job: each such holder can now set and lift limits on its own job.
That gains nothing while its job is unlimited, but it is a silent widening of grants (CP-11, ADR-0040).
*Fix:* give `init` `JobObject::RIGHTS.without(Rights::LIMIT)` in `kernel_handles`, and pass `without(LIMIT)` in
`beside.rs` and every other place a process is handed its own job. Add a `t_quota` assertion that `job_set_limits(JOB, ..)`
from `init`'s children is `AccessDenied`.

**4. warning: the session ceiling does not cover every launch (ADR-0261 point 10, the plan's Q1).** `start_panel`
(`gui.rs:384`) makes the panel's job under `init`'s `JOB`. The panel makes every launch's job under its own `JOB`
(`panel.rs:106`). So terminals, files and editors started from the dock are outside the session's memory and process
ceilings, and only `init`'s reserve bounds them. Also, `session()` returns `JOB` if `job_create` or `job_set_limits`
fails, and retries on every call. A failure therefore starts that shell or broker with no session ceiling, and nothing
says so.
*Fix:* start the panel's job under `session()`, or give the panel a session handle for its launches. Have `session()`
print a line when it falls back, and record the decision.

**5. warning: `Job::drop` under `EVENTS` would panic with "lock taken twice".** Subscriptions now own a `Paid`, and
`zoraya-events` drops them inside `EVENTS.lock()` (`free_port`, `free_source`, `remove_owner`, a failed
`subscribe_paid`). `Paid` holds an `Arc<Job>`. If that is the last reference, `Job::drop` runs under `EVENTS` and
calls `port::free_source`, which takes `EVENTS` again. `SpinLock::lock` asserts on that. The reviewer found no
reachable path today: a subscription is bounded by the subscriber's handle to the source, which `exit_current` closes
while the process still holds its job. That bound is an unstated invariant, though, and Q2's reaper and Q3's service
accounts are the kind of change that breaks it.
*Fix:* keep the `Paid`s out of the lock. Have `remove_*` and `free_port` return the removed subscriptions' payers in a
pile that the kernel drops after `EVENTS` is released, as `channel::close` does with messages. Or document the
invariant at `EVENTS` and assert it.

**6. warning: `check-charged-alloc` is narrower than INV-0261-5 claims.** The lint does not see:
- `charge::prepaid_arc` and `prepaid_box`, which take any `T` and are an uncharged form with no marker. Nine call sites
  use them, and nothing checks that the value holds a charged `Paid`.
- `frames::alloc*`, the main resource. A new frame allocation on a call path is invisible to the lint.
- `try_reserve` and `Vec` or `VecDeque` growth (finding 9).
- `Arc::<T>::new(`, `.map(Arc::new)`, `Box::new_uninit(`, `Box::pin(`.
- Files outside its fixed list: `device.rs`, `memory/stack.rs`, `timer.rs`, `console.rs`.

The in-QEMU half covers memory objects, ports, events, channels and streams. It does not cover `spawn`,
`snapshot_process`, `channel_write`, `image_create`, `job_create`, `port_subscribe`, `memory_map` or `dma_pin`.
*Fix:* bound `prepaid_*` by a trait that only types holding a `Paid` implement (`trait Prepaid { fn paid(&self) -> &Paid; }`),
or require the marker for them too. Add `frames::alloc` and `try_reserve` to the marker rule for the system call
files, and widen the regexps. Until the delta test covers every allocating call, keep the status of INV-0261-5 open,
not `checked`.

**7. warning: the gate accepts invariants that are half checked (INV-0268-1).** `invariants_ok` takes any row
containing `checked:`. INV-0261-2, -4, -5, -6 and -7 read `checked: ...; the in-QEMU half is open` or
`...comes with Q2`. At Q7, a commit that removes the refusal while those halves are still open passes
`make check-untrusted-gate`. ADR-0268 point 2 requires every invariant to be checked.
*Fix:* record the open half as `partly checked:` or `open:`, so the status no longer begins with `checked:`. Make the
script require the status cell to begin with `checked:` and to contain no `open`. Also note that INV-0268-2's check
compares steps, not tests: `memory-hog → test-qemu` holds whether or not `t_quota` is in `test-qemu`'s list.

**8. warning: a process is charged a 1024-slot handle table for life.** `assemble` charges
`MAX_HANDLES × (size_of::<Entry>() + 16)`, about 57 KiB, per process, beside the 64 KiB stack. `exit_current` frees the
table (`:847`), but the charge stays with the zombie until its last handle closes. A job's `kernel` therefore runs far
above the heap's delta, and INV-0261-5's "matches the heap deltas" cannot hold for `spawn`. The report's 256 MiB GUI
refusal, with the session at 214.8 of 215.7 MiB, is consistent with that over-charge.
*Scenario:* `make -j8` in a room, with a parent that is slow to wait. Each finished child keeps 57 KiB of the room's
limit charged for a table that no longer exists.
*Fix:* charge the table as it grows, in `HandleTable::reserve*` against the process's `Paid`, or at least
`release_kernel(table)` in `exit_current` after the table is dropped.

**9. note: uncharged growth on call paths.** All of these are bounded, and INV-0260-2 allows a per-object bound, but
none is in the delta test:
- `UserMemory.frames` (`user.rs:67`): 8 to 16 B for every user frame and table of a space.
- `AddressSpace.mappings` (`user.rs:250`): up to 256 × about 48 B per process.
- `Job.children` (`job.rs:118`).
- `Exit.waiters` (`process.rs:187`).
- `UserSlice::read` transients (`syscall.rs:103`): up to `MAX_SPAWN_SIZE` and the like, freed before return except a
  message body, which is charged.
- `Pin` and its list (`device.rs:315-319`): `dma_pin` is driver only, and ADR-0261 point 6 exempts devices.

Record the bound for each in ADR-0260's table, or charge the first two to the space's `Paid`.

**10. note: `LAST_REFUSED`.** It is correct on one CPU because the kernel is not preempted (`sched.rs:3`), no
interrupt handler charges, and no handler blocks between a refusal and its return. With the reaper thread (Q2) or SMP,
a refusal elsewhere could overwrite it before `dispatch` reads it. Prefer carrying the field in the error: a
`Status`-with-field type through `Calls`, or a per-thread slot.

**11. note: a stale refusal flag.** `map_new_page` (`self.memory.pay_user().map_err(|_| MapFailed::Quota)`) and
`copy_page` (`pay_user()?`) leave `UserMemory.refused` set. Both callers drop the space today, but a later live-space
caller would report its next `NoMemory` as `OverQuota`. Use `failure()` there too, or let `pay_user` not set the flag.

**12. note: `held` includes the job's own record.** When `closed && live == 0`, `held` is `memory.used`, which counts
the job's record charged to itself. So a dead job held only by a handle shows a non-zero "held by others". Subtract the
record.

**13. note: the comment in `spawn_first` says 1/16.** `KERNEL_RESERVE_DIVISOR` is 32, which departs from ADR-0261 point
10's `[assumption]`. The report proposes the change with numbers, as the architect's decision asks. Fix the comment, and
list the number in the report's architect's queue.

**14. note: `snapshot_process` is refused during the copy, not before it.** ADR-0261 point 6 says the call "is refused
before copying if it does not fit". Mapped objects are charged up front, but the process's own pages are charged page
by page in `copy_page`. A refusal after a partial copy leaves no effect, so INV-0261-2 holds, but it wastes work under
the masked-time bound (Q2's ADR-0270). Pre-charge `count of own pages × 4 KiB` before the walk.

**15. note: the model test has no kills.** INV-0261-1's check names "random trees, charges, refunds, limit changes
and kills". `random_trees_keep_every_counter_equal_to_its_recomputation` has no node removal with live charges. Add a
kill that refunds a subtree's holdings in random order.

## Checked and found sound

- **The charge crate.** The all-or-nothing walk; a refusal counted only on the nearest node it would pass; peaks; the
  overflow refusal; `set_limit` against the parent's effective limit, with the two-field rollback in
  `charge::set_limits`; `User` as a part of `Memory`.
- **`Paid`.** It is charged before the allocation in every converted constructor (port, pair, stream, memory object,
  thread, event, image, job record, subscription, queued message). It is refunded once, by drop, on every failure
  after the charge. The process slot is released at `exit_current` and not again at drop. `charge::arc` drops the
  value, and so its `Paid`, when the allocation fails.
- **Who pays.** The writer pays for queued messages (`message_cost` with the vectors' capacities), and the reader pays
  nothing. A spawn and a snapshot are paid by the target job. Page tables are paid by the space's job. Device objects,
  interrupts, kernel threads and boot objects are uncharged and marked.
- **Lock order.** `WALK` is a leaf, and `refused()` signals after releasing it. `EVENTS → WALK` (a `Paid` dropped
  under `EVENTS`) has no reverse edge. No charge is made with `EVENTS` held.
- **ABI.** `OverQuota = 16`, `LIMIT = 1 << 14` with `KNOWN` widened, `OVER_QUOTA` waitable on jobs, `UsageFlags`
  refusing unknown bits, total decoders, `job_set_limits` needing `LIMIT` and `job_get_usage` needing `WAIT`, and
  `CLEAR` applied only after the record is written.
- **`OVER_QUOTA`.** It is asserted only on the refusing job (`r.at`).
- **`unsafe`.** The one new block, in the heap test, has a true SAFETY comment. `zoraya-charge` forbids `unsafe`.
- **`check-untrusted-gate`.** It does fail when a marker is removed while stage-1 tests are pending, apart from
  finding 7.

## Dispositions (Sonnet main session)

- **Error 1** (leaked event source on a refused spawn): fixed in `assemble`: the process is charged before
  `new_source`. A test of `EVENTS.live()` around a refused spawn is open (Q2 writes the refused-`spawn` half of
  INV-0261-2).
- **Error 2** (`memory_map`/`device_map` turned `Quota` into `NoMemory`): fixed, both return `OverQuota`. The `t_quota`
  case that maps a foreign object at the limit is open for Q2.
- **Warning 3** (`init` holds `LIMIT` on its own job): accepted for stage 8. `init` is trusted; untrusted rooms hold no
  handle on it, and the keeper (Q6a) hands out jobs without `LIMIT` (INV-0269-2). Recorded for Q7's review.
- **Warning 4** (panel launches outside the session job; `session()` falls back to `init`'s job): open. The panel's
  launches move under the session in Q6a with the keeper; the fallback should fail loudly then.
- **Warning 5** (`Paid` dropped under `EVENTS`): an unstated invariant (a subscription's holder process outlives it).
  Stays on Q7's list.
- **Warnings 6, 7** (lint reach; `checked:` rows with open halves): accepted for Q1. The lint's reach grows with each
  part; Q7 re-reads the rows whose halves are open before the gate can open (stage 1 stays `pending` until then).
- **Warning 8** (zombies keep the table charge): accepted; it is the cause of the thin margin in the 256 MiB GUI test
  and goes to the architect (report, Q1 decisions).
- **Notes:** the stale 1/16 comment is fixed; the rest (single-CPU `LAST_REFUSED`, `held` counting the job's own record,
  snapshot charging page by page, the model test without kills) are recorded in the report.

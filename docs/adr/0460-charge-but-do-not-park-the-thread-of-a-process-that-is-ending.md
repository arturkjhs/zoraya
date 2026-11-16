# ADR-0460: Charge but do not park the thread of a process that is ending

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0264, ADR-0270, ADR-0285, ADR-0300
- **Evidence:** CP-07, quotas/survey.md#cpu, quotas/survey.md#linux-cgroups-v2

## Context

ADR-0285 item 7 says a killed job's parked thread ends at the next period boundary, at most one period after the kill.
TASK-31's `work/smp-tcg` found that this does not hold when the process's teardown needs more than one period's budget
(`docs/tasks/TASK-31-smp-tcg.md` section 4.3, with the kernel test run of section 5 that keeps its logs; the ADR-0285
open question has the note, ADR-0001 rule 6).

**The mechanism** (read in `kernel/src/process.rs`, `kernel/src/sched.rs` and `crates/zoraya-cpu/src/lib.rs`):

1. A process that ends has its threads leave through `process::leave_current`. The last thread to leave is the owner and
   runs `teardown` on its own stack: it closes the handles, unmaps and frees the space, in steps that yield at
   `steps::step_point` (ADR-0270) so that no masked interval is long.
2. Each yield enters `sched::schedule`, which charges the running thread's time to its process's job (`JobRef::of(process.job())`)
   at every switch (ADR-0264 point 5), and then calls `RunQueue::pick`.
3. `pick` takes the first ready thread whose job has budget; every thread ahead of it whose job has none is parked until
   the period ends (`Decision::Park`, at most `MAX_PARKS_PER_PICK` per call). `job_of` gives only the thread's process's
   job; nothing there says the process is ending. So the teardown thread of a killed room whose budget is spent is parked
   like any other thread of a spent job, and it resumes at the next period's release (INV-0285-1).
4. A teardown has many step points. Each park costs one period.

**Measured** (TASK-31 section 4.3, under TCG, `t_cpu`'s `a_killed_room_with_parked_threads_dies`: a room at 10% with 20
parked spinners and a period of 100 ms; the parks of the room after the kill, counted with temporary code): 0 parks, one
period; 1 to 9 parks, two periods; 12 parks, four; 55 parks, nine. In 74 rounds the room took 18 to 1 246 ms, median
147 ms, and the test failed twice. A killed room at a low ceiling keeps its frames, and is charged, for as many periods as
its teardown has parked turns. **Assumed:** one CPU (ADR-0264's revisit trigger for SMP).

**The architect decided, on 2026-11-16, Option A below.** The ADR itself stays `proposed`: the architect accepts it after
the implementation and its test exist. Option A is the architect's decision of 2026-11-16.

## Decision drivers

1. ADR-0285 item 7 is a promise to anyone who kills a room: the room is gone, and its memory is back, within one period
   (CP-07; the ceilings of `quotas/survey.md#cpu` bound use, they do not delay an exit).
2. The teardown's own work is bounded per step by ADR-0270; its total is bounded by what the room owns. An overrun by one
   teardown is finite and known; a delay of nine periods is not what a ceiling is for.
3. `RunQueue::pick`'s rule that no thread of a spent job runs keeps the overrun per period to one floor slice however many
   threads the job has (its doc comment, ADR-0264 point 5). Any change must say how that stays true.
4. The CPU charged to a job must stay true (the usage record of ADR-0285 item 8): an exemption from charging would hide
   the teardown's cost from the job's parent.

## Considered options

### Option A: charge, but do not park, the thread of an ending process (the architect's decision)

- **Description:** a thread that runs the teardown of a process that is ending is charged to its job at every switch as
  now, and `pick` does not park it for a spent budget: it runs on, with the floor slice at least, until the teardown
  finishes. Which threads qualify is exactly one kind: the thread that began the process's end (the owner of
  `teardown`) and the siblings that are leaving (`leave_current`), while the process's `exit_requested` is set. Nothing
  else changes: every other thread of the job is parked as before.
- **Evidence for:** the room dies in the time its teardown needs, one period at most after the kill if the teardown fits
  its budget, and in the teardown's own time if it does not; the charge stays true (driver 4); the change is in
  one decision of `pick` (a flag of the thread, read by `job_of` or `decide`) and the host test of `zoraya-cpu` can check
  it.
- **Evidence against:** the job overruns its ceiling by one teardown (finite, ADR-0270 bounds the steps; the number of
  steps is bounded by what the room owns). `pick`'s rule that no thread of a spent job runs gets one named exception
  (below).
- **Cost:** *(estimate)* a flag on the thread and a branch in `pick`; a host test in `zoraya-cpu`; a QEMU kernel test.
- **Precedent:** none found among the systems of `quotas/survey.md#cpu`; Linux's cgroup v2 `cpu.max` throttles the tasks
  of a group (`quotas/survey.md#linux-cgroups-v2`), and what it does to an exiting task is `[assumption]` here, not checked.

### Option B: exempt the ending thread from parking and from the charge

- **Description:** as A, and the teardown's time is charged to no one (or to the parent).
- **Evidence for:** the simplest accounting for the dying room's budget: it is not made to pay for its death.
- **Evidence against:** the usage record would miss CPU that was used, and a parent's ceiling would not see it
  (driver 4); a room could make its teardown long on purpose (many handles) and the time would be free.
- **Cost:** *(estimate)* as A, and a second path in `schedule`'s charge.
- **Precedent:** none found.

### Option C: leave the scheduler and lower the guarantee

- **Description:** ADR-0285 item 7 says "within as many periods as the teardown has parked turns", and nothing changes in
  the kernel. The test's bound becomes a function of the parks.
- **Evidence for:** no change to the scheduler; the rule that no thread of a spent job ever runs stays without
  exception.
- **Evidence against:** a person who kills a room cannot say when its memory returns: 1 246 ms was measured for a room
  at 10%; at 1% it would be ten times longer; the promise of item 7 is gone (driver 1).
- **Cost:** *(estimate)* none in code; a weaker promise.
- **Precedent:** the present behaviour; Linux's `cpu.max` baseline as far as `[assumption]` above holds.

## Recommendation

**The architect's decision is Option A (2026-11-16).** The agent's analysis agrees: it keeps ADR-0285 item 7 true, keeps
the charge true, and puts one named exception into `pick`. This is a proposal for the architect to accept after the
implementation.

**How A keeps `pick`'s rule.** `pick` says no thread of a spent job runs, so that the overrun per period is one floor
slice. With A, a thread of a spent job runs only if it belongs to a process that is ending, and then only to run the
rest of its current call (to the call's next cancellation point, `steps::between` or a wait's `exit_requested` check, or
to its end for a call that only pauses), `leave_current` and the process's teardown. That work is finite (ADR-0270
bounds each step, and the steps follow what the process owns), is charged to the job and its parents (to `used_ns`, the
usage record, and to the period it runs in), and runs once per process (the process cannot end twice, and an ended
process has no thread). The charge is not paid back: the period it lands in is already over the budget, and the next
period starts with the full budget (`CpuAcct::used_in` reads zero for any period but the one counted), so the overrun is
lasting and not an overdraft (open question 2). A process cannot make itself "ending" without ending: `exit_requested` is set by `exit`, a fatal signal or a
kill, and the thread does not return to user mode after it. So a limited job cannot use the exception to run user code
past its ceiling.

## Consequences

- **Positive.** A killed room dies at once, whatever its ceiling; ADR-0285 item 7 holds again; the test of
  `a_killed_room_with_parked_threads_dies` no longer needs a bound of many periods.
- **Negative.** A job overruns its ceiling, and every ceiling above it (the exception overrides the `Park { at }` of any
  ancestor), by the way out of each of its processes that ends: the rest of the current calls of its threads,
  `leave_current` and the teardown. The overrun is charged to `used_ns` but not paid back in a later period (no
  overdraft, open question 2). The bound: one way out per process that ends, each bounded by ADR-0270's steps over what
  the process owns. The loop that uses it: a room at 10% spends each period's budget building a child process in the
  room that holds much (handles, mappings, a large space within its frame quota) and kills it; the child's teardown runs
  past the budget, unparked, and the next period starts full. Its steady use is its ceiling plus one teardown per
  cycle. Under TCG, 40 spinner processes' ways out ran 58 ms against a budget of 100 us (the kernel test).
- **What runs unparked is per process, not per thread.** The guard is `Process::is_ending`, a flag of the process, so
  every thread of an ending process is taken unparked, including one in the middle of a call; no user code runs (below).
- **One CPU.** "No user code after `ending`" holds because a thread reaches EL0 only through `exit_if_requested` after
  its last switch, with interrupts masked to the `eret`. The kernel starts one CPU in every configuration today. With
  SMP, a sibling at EL0 on another CPU when `ending` is set runs there to its next exception (open question 4).
- **The host test models the way out as turns of the slice `pick` gives** (`zoraya-cpu`,
  `a_killed_room_dies_in_the_period_after_the_kill_and_pays_for_it`), and not as the kernel's real step points.
- **Follow-up work.** The flag and the branch in `kernel/src/sched.rs`, `crates/zoraya-cpu` and `kernel/src/process.rs`
  (built on `work/park-ending`: `Process::is_ending`, set only by `Process::end_from_now` with the exit record, and
  read by a second closure, `ending`, of `RunQueue::pick`, not by `job_of` or `decide`);
  a host test in `zoraya-cpu`; the QEMU kernel test of INV-0460-1, **to be built by another agent** (the part that
  implements this ADR); ADR-0285's open question gets a note that this ADR answers it, and ADR-0285 item 7 is restated
  when this ADR is accepted.

## Invariants

- **INV-0460-1.** A room killed with its budget spent and its threads parked has no process left before the end of the
  period after the kill's, whatever number of steps its teardown has, while its ways out fit in one period. *Check:*
  the room has no process left before the end of the period after the kill's, checked by the test kernel's
  `a_killed_room_ends_in_the_period_after_the_kill` in `kernel/src/testing.rs` (one thousandth of a 250 ms period, 40
  parked spinners, a bound in periods).
- **INV-0460-2.** The ending process's teardown time is charged to its job: the job's usage after the kill is at least the
  time the teardown ran. *Check:* the test kernel's `a_killed_room_ends_in_the_period_after_the_kill` reads
  `cpu::usage` of the room and of its parent before the kill and after the room is empty, and asserts that the room is
  charged at least the time its ending threads ran, which `sched::ending_watch` counts at each switch apart from the
  charge.
- **INV-0460-3.** `RunQueue::pick` parks every thread of a spent job except a thread of an ending process; a thread of a
  process that is not ending is parked as before. *Check:* `cargo test -p zoraya-cpu`.

## Open questions

- Whether the floor slice limits a teardown thread's turn, or the thread runs until its next step point: the second is
  simpler, the first keeps the masked-time figures of ADR-0270 as measured. The implementer measures `t_bound` and says.
  *The floor slice was built* (agent's note under ADR-0001 rule 6, `work/park-ending`, 2026-11-16): `pick` returns
  `MIN_SLICE_NS` for an ending thread of a spent job. The kernel is not preempted, so the slice decides only at which
  step point the thread yields, not how long an interval is masked: each interval is still one step (ADR-0270), and the
  other jobs get their turns between an ending thread's turns of 50 us. Figures of one HVF run of
  `make test-qemu-auto-default` with the change: `t_bound`'s exits 82 to 572 us masked at 64 to 65 units, `job_kill` of
  one 86 to 252 us (these rooms have no ceiling, so they do not take the exception); the killed room at one thousandth,
  whose 40 ways out all take it, 235 us longest masked from the kill to the room's end (45 us in another run). Under TCG
  `-smp 4` the same case read 14.5 ms, a figure not compared with a run without the change and not separated from TCG's
  own stalls (which the counter counts); it stays to be explained. The other option was not built, so it was not
  measured.
- *For the architect:* whether the overrun is paid back. Two options: (a) an overdraft carried into the next period
  (`period_used - budget` counted against the next period's budget, a change to ADR-0264's accounting), so that the
  loop of the Consequences gets its ceiling over time; or (b) accept the overrun as it is built: charged to `used_ns`,
  never paid back, at most one way out per process that ends. ADR-0285 item 2's floor already lets a job overrun by one
  slice. Built: (b), the least committing.
- The teardown's moving to the reaper (charged to no one) is a different decision, Option B's family, and is not
  chosen.
- With more than one CPU running: a thread of an ending process at EL0 on another CPU runs user code to its next
  exception, possibly past the ceiling if its job spent the budget meanwhile. Whether the kill should then interrupt
  the other CPUs (an IPI) is for the multi-CPU stage.

## Revisit triggers

- A teardown measured longer than one period at the ceiling the parent set.
- SMP with per-CPU budgets (ADR-0264): an ending process's thread on one CPU while the job's budget is spent on another.
- A process that ends by a path that does not set `exit_requested`.

## Architect's decision

(Left empty for the architect. The architect decided Option A on 2026-11-16; the status is set to accepted by the
architect after the implementation.)

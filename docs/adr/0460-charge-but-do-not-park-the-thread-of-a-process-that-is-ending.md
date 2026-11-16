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

### Decision on open question 2

Accepted 2026-11-16. Decided by the architect; recorded by an agent at the architect's request.

The overdraft, not acceptance (the architect, 2026-11-16): the overrun of an ending process is charged as debt that the
room and its parents pay back in the following periods, so the ceiling holds on average and the start-and-kill loop
gains nothing; a killed room still ends at once (the ending thread is still not parked). This records the decision on
open question 2 only; the ADR's status is the architect's.

**As built** (`work/park-ending`, `crates/zoraya-cpu`):

1. **What is debt.** When `schedule` charges a run of a thread whose process is ending (`charge(.., ending)`), each
   node with a ceiling, the job and every ancestor by the same rule, adds to its `debt` the part of the in-period run
   past its own budget: `after - max(before, budget)` where `before` and `after` are its `period_used` around the run.
   A run inside the budget makes no debt; a node without a ceiling keeps none (removing a ceiling clears it).
2. **How it is paid.** `settle` brings a node to a later period: each period after the one counted pays up to one
   budget of the debt. The periods in between, in which nothing was counted, pay a whole budget each (the job could
   have used it); the new period begins with `period_used = min(debt, budget)` and that much less debt. So a job whose
   debt is at least its budget is parked for the whole period, and its debt falls by one budget a period. `decide` and
   `job_get_usage` read the settled figure (`period_used` includes the debt paid in the period); `used_ns` is the time
   run, unchanged.
3. **The cap.** A node's debt is at most `DEBT_CAP_PERIODS` (100) budgets (`debt_cap`): at most 100 periods of
   parking, 2 s at the 20 ms period. Beyond it the overrun is forgiven.
4. **A killed room still ends at once.** The ending thread is still taken by `pick` (floor slice) and charged; the debt
   parks only threads that are not ending.
5. **A dead room.** When a job's record is dropped, its debt as of then goes to the nearest ancestor with a ceiling
   (`pass_debt`, from `Job::drop`), up to that node's cap. An ancestor that also went past its own budget in the same
   runs then holds that part twice (its own debt and the passed one): this errs on the side of the ceiling.

**How A keeps `pick`'s rule.** `pick` says no thread of a spent job runs, so that the overrun per period is one floor
slice. With A, a thread of a spent job runs only if it belongs to a process that is ending, and then only to run the
rest of its current call (to the call's next cancellation point, `steps::between` or a wait's `exit_requested` check, or
to its end for a call that only pauses), `leave_current` and the process's teardown. That work is finite (ADR-0270
bounds each step, and the steps follow what the process owns), is charged to the job and its parents (to `used_ns`, the
usage record, and to the period it runs in), and runs once per process (the process cannot end twice, and an ended
process has no thread). What it runs past a budget is debt, paid back from the next periods' budgets (the decision on
open question 2, above), so over time the job keeps its ceiling. A process cannot make itself "ending" without ending: `exit_requested` is set by `exit`, a fatal signal or a
kill, and the thread does not return to user mode after it. So a limited job cannot use the exception to run user code
past its ceiling.

## Consequences

- **Positive.** A killed room dies at once, whatever its ceiling; ADR-0285 item 7 holds again; the test of
  `a_killed_room_with_parked_threads_dies` no longer needs a bound of many periods.
- **Negative, in one period.** A job, and every ceiling above it (the exception overrides the `Park { at }` of any
  ancestor), can run past its budget in a period by the ways out of its processes that end: the rest of the current
  calls of its threads, `leave_current` and the teardown, each bounded by ADR-0270's steps over what the process owns.
  Under TCG, 40 spinner processes' ways out ran 58 ms against a budget of 100 us (the kernel test).
- **On average, the ceiling holds.** That overrun is debt (the decision on open question 2). Over any N consecutive
  periods a node with budget B is charged at most `N * (B + F) + D0 - D1 + X`, where F is the floor overrun of a
  period (ADR-0285 item 2), D0 and D1 the debt at the start and the end, and X what the cap forgave. For the
  start-and-kill loop (a room builds a child that holds much, the child ends, again): every child needs the room's
  budget to reach its end, and a room in debt has none, so the loop waits while it pays; over N periods it gets at
  most `N * B + N * F` plus one way out (the last, whose debt is still owed) plus one budget (a period's debt is paid
  only from the next), unless one way out is longer than the cap of 100 budgets, whose excess is forgiven once per
  such child, that is at most one excess per 100 periods. The test kernel's `a_start_and_end_loop_keeps_its_ceiling`
  checks over 11 periods that the room's charge beyond its ending threads' own time (measured in the same boot, since
  a way out's length depends on the machine) stays within the ceiling's allowance, and that no second way out begins
  while the debt stands.
- **A busy machine stretches a way out.** An ending thread of a spent job runs in turns of the floor slice (50 us)
  between other ready threads' turns of up to 10 ms, so a long way out next to busy jobs takes many rounds; a killed
  room's death is "at once" only in the room's own terms (seen in the host model: with an uncapped spinner beside the
  room, the start-and-end loop's 6 ms ways out ran so slowly that the room used 101 ms in 50 periods with or without the
  debt, its bare ceiling).
- **The charge of a run is by the state at the switch.** A thread that ends its own process in a turn has the whole
  turn charged as ending, its user part before the exit included: the part past the budget is debt, which is stricter,
  not looser.
- **What runs unparked is per process, not per thread.** The guard is `Process::is_ending`, a flag of the process, so
  every thread of an ending process is taken unparked, including one in the middle of a call; no user code runs (below).
- **One CPU.** "No user code after `ending`" holds because a thread reaches EL0 only through `exit_if_requested` after
  its last switch, with interrupts masked to the `eret`. The kernel starts one CPU in every configuration today. With
  SMP, a sibling at EL0 on another CPU when `ending` is set runs there to its next exception (open question 4).
- **The host test models the way out as turns of the slice `pick` gives** (`zoraya-cpu`,
  `a_killed_room_dies_in_the_period_after_the_kill_and_pays_for_it`, `a_start_and_end_loop_keeps_its_ceiling_with_the_debt`),
  and not as the kernel's real step points.
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
  `a_killed_room_ends_in_the_period_after_the_kill` in `kernel/src/testing.rs` (one thousandth of a 100 ms period, 16
  parked spinners, a bound in periods).
- **INV-0460-2.** The ending process's teardown time is charged to its job: the job's usage after the kill is at least the
  time the teardown ran. *Check:* the test kernel's `a_killed_room_ends_in_the_period_after_the_kill` reads
  `cpu::usage` of the room and of its parent before the kill and after the room is empty, and asserts that the room is
  charged at least the time its ending threads ran, which `sched::ending_watch` counts at each switch apart from the
  charge.
- **INV-0460-3.** `RunQueue::pick` parks every thread of a spent job except a thread of an ending process; a thread of a
  process that is not ending is parked as before. *Check:* `cargo test -p zoraya-cpu`.
- **INV-0460-4.** What an ending process's threads run past a budget is debt that the job and each ancestor with a
  ceiling pay back from the next periods' budgets, up to a cap of 100 budgets, and a dead room's debt goes to its
  nearest ancestor with a ceiling: over N periods a room whose processes end one after the other is charged at most N
  budgets, a floor slice a period and one budget beyond what its ending threads ran, and while the debt stands no new
  way out begins. *Check:* `cargo test -p zoraya-cpu` (carry, cap, parent chain, dead room, the loop with and without
  the debt) and the test kernel's `a_start_and_end_loop_keeps_its_ceiling` (over 11 periods, the room's charge and the
  clock against that bound with the ending threads' time measured in the same boot, and no way out begun in the 11 periods after the first).

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
  `-smp 4` the same case read 14.5 and 13.6 ms inside full runs. *Compared* (2026-11-16, the case as trimmed: 16
  spinners at one thousandth of 100 ms; the test kernel running that case alone, 5 boots each under
  `make test-qemu-tcg-smp4-gicv2`, one QEMU at a time, alternating): with the change, the longest masked interval from
  the kill to the room's end was 2.002 to 2.113 ms, median 2.034 ms, the room gone 1 period after the kill's in 94 to
  95 ms; on `origin/main` (661a39b6) with only the case added, 3.952 to 4.209 ms, median 4.055 ms, the room gone 17
  periods after the kill's in 1 687 to 1 691 ms. So the change does not lengthen the masked interval; the window is
  shorter and the worst interval in it about half. The figures inside full runs include what other tests left
  running. The other option was not built, so it was not measured.
- Whether the overrun is paid back: *decided 2026-11-16, the overdraft* (see "Decision on open question 2" under
  Recommendation); built on `work/park-ending`.
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

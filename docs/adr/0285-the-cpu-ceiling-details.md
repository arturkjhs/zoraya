# ADR-0285: What Q4 decided where ADR-0264 is silent: one parked list, a slice floor, the unit and its range, a 120-byte usage record, and two reserved deadlines

- **Status:** proposed
- **Date:** 2026-11-11
- **Depends on:** ADR-0030, ADR-0261, ADR-0264, ADR-0270
- **Evidence:** CP-07, quotas/survey.md#cpu, quotas/survey.md#linux-cgroups-v2

## Context

Q4 of TASK-26 builds ADR-0264 option D (CP-07; the survey of what other systems do is `quotas/survey.md#cpu`). The ADR fixes the unit, the 20 ms period, charging at each switch, the slice
for the smaller of 10 ms and the budget left, parked lists with one global deadline, nesting, and the usage fields. The
code needed a few more choices. Most are small additions; **two depart from the text of accepted ADR-0264** (point 3, interrupt time; point 5, where a woken thread waits) and are marked DEVIATION below and in "Tensions with accepted ADRs". They are listed so that a reviewer of the
scheduler finds them in one place (CLAUDE.md, "Decisions"). **Measured** (`docs/design.md`, "Quotas", Q4): 8 spinners at
25% used 502 ms of 500 ms in 2 s; three rooms at 25% under 50% used 1006 ms of 1000 ms; the longest masked interval over
those tests 1051 us at most; an uncapped process's wake-up lateness next to two capped rooms at most 5.7, 9.6 and 50.1 ms
at periods of 10, 20 and 100 ms. **Assumed:** one CPU (SMP is a revisit trigger of ADR-0264).

## Decision drivers

1. The scheduler's lock order and masked time are what INV-0270-1 bounds; a change keeps both small and says where it
   touches them.
2. Nothing a limited room does may strand a thread or the deadline queue (ADR-0023: 288 entries, two kept for the slice and the period's deadline).
3. A program written for Q1's 88-byte `job_get_usage` keeps working.

## Considered options

### Option A: the choices below (the recommendation)

Per-job parked lists, as the ADR's wording has them, are the alternative to point 1; a hard floor of zero for the slice
is the alternative to point 2; a range check only on the upper end is the alternative to point 3.

### Option B: the literal reading of the ADR

A parked list per job, found through the job; a slice exactly the budget left; `cpu` of 0 meaning "no time"; a new
usage record size that older callers must meet. It costs a list (and its reserved capacity) in every job, a timer
interrupt per microsecond of budget left, a job that never runs, and a break of every caller of `job_get_usage`.

### Option C: Linux's way, a quota enforced by a hierarchical fair scheduler

Rejected by ADR-0264 option A (`quotas/survey.md#linux-cgroups-v2`).

## Recommendation

**Recommendation (not a decision): Option A.**

1. **One parked list for all jobs.** Every parked thread returns at the same boundary, so a list per job gains nothing
   and costs a list in every job record. A thread is parked only when it reaches the front of the run queue with its job
   over budget (`RunQueue::pick`, `zoraya-cpu`); a thread woken or preempted just joins the run queue and is parked
   before it can run. **DEVIATION** from ADR-0264 point 5 ("a thread woken while its job is parked joins the parked
   list"): the result is the same, no user instruction of a spent job runs, but the wake costs a walk in the next
   `schedule`. At most 16 threads are parked by one pick (INV-0270-1: the walks are bounded); at that cap the next
   thread goes back to the front, idle runs and calls `schedule` again, so the rest are parked in further masked
   intervals and a spent job with 200 threads still runs one floor at most per period (review W1; host test with 200
   threads and `t_cpu` with 40). `cpu.parked` counts parks on the job whose budget was spent.
2. **A slice floor of 50 us.** The slice is the smaller of 10 ms and the budget left, but at least 50 us, so a job with a
   few microseconds left does not arm a timer for each. A job over budget by this floor is the overrun of ADR-0264 point
   4 (plus the time to the next return to EL0). A ceiling whose budget is below 50 us (under 0.25% of 20 ms) therefore
   runs at about 0.25%.
3. **The unit's range.** `cpu` is 1 to 1000 thousandths; 0 and above 1000 are `InvalidArgument`, as is a value above
   the parent's effective ceiling. 1000 is a ceiling that never parks but is counted and walks like any other. `None`
   removes the job's own ceiling. `job_set_limits` sets all three limits or none (INV-0261-3 extended): the CPU one
   first, put back if the memory or process limit is refused.
4. **Charging walks every ancestor, limited or not**, as Q1 does for memory: `used_ns` is the job's since creation, so
   `cpu.used_ns` is right for a job that gets a ceiling later. The cost is one lock and an add per ancestor per switch.
   **DEVIATION** from ADR-0264 point 3 ("interrupt handlers charge no one"): the time a thread runs includes the
   interrupts that hit it, since separating them needs the counter read in every handler. The effect is a ceiling a
   little stricter than the sentence, never looser.
5. **The boundary.** A run that crosses a period boundary counts only its part after the boundary to the new period's
   budget; the part before is in `used_ns`. A period's `period_used` reads as zero in the next period without work.
6. **Release in batches of 64.** The deadline at the boundary moves 64 parked threads and, if more remain, arms itself
   for now: the interrupt returns and unmasks between batches (ADR-0264 point 5's "steps of ADR-0270's size").
7. **A killed job's parked thread** ends at the next boundary (at most one period), since a parked thread is ready, not
   blocked, and `job_kill` wakes only blocked threads.
8. **The usage record is 120 bytes** (`USAGE_SIZE_CPU`): the 88 bytes of Q1 and four u64: `cpu.limit` (thousandths or
   `NONE`), `used_ns`, `period_used`, `parked`. The kernel writes as much as the buffer holds from 88 up; the decoder
   reads missing CPU words as zero and no limit.
9. **Two reserved slots in the deadline queue.** `arm` leaves two free for the scheduler (the slice and the period's
   deadline). If arming the period's deadline fails, the parked threads are returned at once; a thread is never
   stranded.
10. **The period is a constant** (`zoraya_cpu::PERIOD_NS`); the test kernel can set another (`SET_CPU_PERIOD`, restored
    between tests) for the measurement.

## Consequences
- **Positive.** The scheduler change is `sched.rs` (about 80 lines) and `cpu.rs`; the logic is a crate with host tests on
  a simulated clock; nothing in an unlimited job's path changes but a walk of its ancestors per switch.
- **Negative.** A switch walks the job tree (at most 32 nodes, two lock operations each) with every exception masked; a
  floor of 50 us of overrun per period at most.
- **Tensions with accepted ADRs.** ADR-0264 point 3 (interrupt time is charged to the interrupted thread) and point 5
  (a woken thread of a spent job is parked at the next pick, not at the wake; one parked list, not one per job).
- **Follow-up.** The GUI half of INV-0264-3 waits for rooms with `--cpu` (Q6a); weights and a burst allowance stay with
  ADR-0264's triggers.

## Invariants
- **INV-0285-1.** A thread leaves the parked list only through `RunQueue::release`, and every park has a deadline
  armed or a bounded batch of the parked threads was returned at once. *Check:* `cargo test -p zoraya-cpu` (the model scheduler never
  strands a thread) and `make test-qemu` (`t_cpu`: a parked spinner moves again next period).

## Open questions
- *For the architect:* accept the two deviations above, or ask for interrupt time to be subtracted and for the wake to
  park at once?
- Rooms under one parent are not promised any share: with three rooms at 25% under 50%, one room used 0 ms in 2 s
  while the other two used the parent's whole budget (`t_cpu`, measured). ADR-0264 has ceilings, not shares; weights
  are its revisit trigger. Is that acceptable for untrusted rooms?
- Whether the floor of 50 us should be a function of the period (it is 0.5% of 10 ms).
- Whether the per-switch walk shows in `portbench` or `chanbench` (not measured here).
- Lock order, for the reviewer: `SCHED`, then the timer queue's lock, then a job's `cpu` lock; the last is a leaf. The
  walks run in `schedule` with every exception masked and in `period_end` in the timer interrupt.

## Revisit triggers
- SMP: per-CPU budgets from one pool (ADR-0264).
- A measurement that shows the per-switch walk in a benchmark.

## Architect's decision

(Left empty for the architect.)

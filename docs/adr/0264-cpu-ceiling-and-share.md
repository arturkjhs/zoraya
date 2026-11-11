# ADR-0264: CPU: a ceiling per job enforced by the scheduler in stage 8, shares between jobs later

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0023, ADR-0030, ADR-0172, ADR-0261, ADR-0262, ADR-0263, ADR-0266, ADR-0267, ADR-0269, ADR-0270
- **Evidence:** CP-07, quotas/survey.md#cpu, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#windows-job-objects, quotas/survey.md#sel4-untyped-memory-and-scheduling-contexts, quotas/survey.md#fuchsia-jobs-and-memory-attribution, quotas/survey.md#zoraya-today

## Context
The scheduler is one run queue, round robin, on one CPU, with 10 ms slices armed only when another thread is ready
and no preemption inside the kernel (ADR-0030; `quotas/survey.md#zoraya-today`). Every thread is equal, so a room
that starts 64 spinning processes gets 64 of every 65 slices, and a terminal's keystroke waits up to 64 slices,
0.64 s, before its thread runs. Nothing charges CPU to a job. The task asks what "this agent may use at most a quarter
of the machine" means on one CPU, and what a share between jobs and a ceiling per room need.

## Decision drivers
1. A person's sentence, "at most a quarter", must be what the system does, also when the CPU is otherwise idle: the
   ceiling is the promise a person checks (`quotas/survey.md#cpu`).
2. An interactive terminal next to a spinner must stay responsive: its keystroke runs within a bounded time whatever
   the room does (TASK-25's test).
3. The scheduler is small and has no preemption in the kernel; a change must keep it so (ADR-0030).
4. A ceiling bounds the room's whole tree, at any depth (CP-07, ADR-0261's nesting).
5. SMP is later; a percentage must keep its meaning there.

## Considered options

### Option A: cgroups' `cpu.weight` and `cpu.max` (Linux baseline)
- **Description:** Hierarchical weights among siblings, plus a quota per period on any group, throttled until the
  next period when spent (`quotas/survey.md#linux-cgroups-v2`, `quotas/survey.md#cpu`).
- **Evidence for:** Both tools, on one tree; widely used; the period and quota are exactly "a quarter".
- **Evidence against:** Two mechanisms at once on the hot path; a hierarchical fair scheduler is a rewrite of
  ADR-0030's queue. Throttling makes a bursty task wait a period even on an idle CPU, which Linux answered with a
  burst allowance (`quotas/survey.md#cpu`).
- **Cost:** *(estimate)* 800 lines: per-job run queues with virtual time, plus bandwidth.
- **Precedent:** Linux.

### Option B: Budgets as capabilities (seL4 MCS scheduling contexts)
- **Description:** Every thread runs on a scheduling context with a budget per period, configured through a per-CPU
  control capability (`quotas/survey.md#sel4-untyped-memory-and-scheduling-contexts`).
- **Evidence for:** Exact and capability-shaped; a ceiling is a property of an object one can hold.
- **Evidence against:** Every thread needs a configured context; a thread that blocks often forfeits budget with the
  default replenishments; it puts CPU time on threads, not on the job a person named.
- **Cost:** *(estimate)* A new object type and a sporadic-server replenishment; a rewrite of thread creation.
- **Precedent:** seL4 MCS.

### Option C: Weights only (Zircon's fair scheduler, Windows' weight)
- **Description:** Each job gets a weight; CPU is divided in proportion among ready jobs
  (`quotas/survey.md#fuchsia-jobs-and-memory-attribution`, `quotas/survey.md#windows-job-objects`).
- **Evidence for:** Work-conserving: an idle CPU is never withheld; the terminal gets its share among spinners.
- **Evidence against:** Driver 1: a weight is not "at most a quarter"; a lone agent takes the whole CPU.
- **Cost:** *(estimate)* 400 lines: a job-level round robin with weights.
- **Precedent:** Zircon, Windows' `WEIGHT_BASED`.

### Option D: A ceiling per job as a budget per 20 ms period, in the existing queue; shares later
- **Description:** A job may have `cpu: Some(per-mille)`. The scheduler charges each user thread's run time to its
  job's limited ancestors (ADR-0261's walk), and when a limited job's budget for the current period is spent, its
  threads are moved off the run queue to a parked list until the period ends. Periods have global boundaries. All
  untrusted rooms sit under one job with its own ceiling (ADR-0261 point 10). The round robin is otherwise as it is.
- **Evidence for:** Drivers 1, 3, 4: Windows' hard cap and `cpu.max` behave the same way
  (`quotas/survey.md#windows-job-objects`, `quotas/survey.md#linux-cgroups-v2`); one more check at the place a slice is
  armed; the job tree already exists. Driver 2, by arithmetic: with every capped job together at most `C` of the
  period `P`, a thread of an uncapped job that wakes waits at most `2 × C × P` (capped threads running to the end of
  one period and from the start of the next, the worst a hostile room can arrange) plus one slice and ADR-0270's step.
  With the untrusted rooms' job at 50% and a 20 ms period that is 20 ms + 10 ms + a step, about 32 ms.
- **Evidence against:** Not work-conserving: a capped room waits while the CPU idles, which is what the person asked
  for but costs throughput; a burst longer than the budget waits a period (`quotas/survey.md#cpu`). The bound holds
  only for the capped part: many uncapped spinners in a trusted room still share equally with the terminal.
- **Cost:** *(estimate)* 200 lines in `sched.rs` and the charge crate; one global period deadline.
- **Precedent:** Windows' `JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP`; `cpu.max`.

## Recommendation
**Recommendation (not a decision): Option D in stage 8**, with Option C's weights between jobs as the next step when a
trusted workload is measured to starve the terminal.

1. **Unit.** `cpu` is a share of the whole machine in per-mille, written as a percentage (`--cpu 25%`). On one CPU,
   25% is 5 ms of CPU time in every 20 ms period. With `n` CPUs (later) it is `n × 5 ms` per period across them, as
   Windows' rate is of the whole machine (`quotas/survey.md#windows-job-objects`): "a quarter of the machine" keeps its
   meaning when the machine grows.
2. **Period.** 20 ms `[assumption]`, with boundaries at multiples of 20 ms of the monotonic clock, for every job at
   once. Shorter than `cpu.max`'s default of 100 ms (`quotas/survey.md#linux-cgroups-v2`) because the latency bound
   of driver 2 grows with it. Unspent budget does not carry over (no burst in stage 8).
3. **Charging.** At every switch away from a user thread, its run time since it was switched in is charged to its
   job's limited ancestors' `cpu.period_used`. Time a thread spends in the kernel on its own call is its own.
   Interrupt handlers charge no one.
4. **The slice.** When the scheduler switches to a thread of a limited job, it arms the slice for the smaller of
   10 ms and the remaining budget of its most constrained ancestor, also when no other thread is ready (a change to
   ADR-0030, which arms a slice only then), so a job overruns its budget by at most the time to the next return to EL0
   plus ADR-0270's step.
5. **Parking.** A thread whose limited ancestor has spent its budget is not put back on the run queue; it goes to that
   job's parked list. One global deadline at the next period boundary, armed only while some job has parked threads,
   advances the global period number and returns the parked threads to the end of the run queue. A job's `period_used` carries the period number it was counted in and reads as zero when that number is old, so the callback does no work per job; the threads it moves are at most the session's 200 processes, in steps of ADR-0270's size, with interrupts unmasked between them, so INV-0270-1 covers it. So parking takes one
   entry of the 288-entry deadline queue (`quotas/survey.md#zoraya-today`), not one per job. A thread woken while its
   job is parked joins the parked list. Parking is invisible to the program: no status, no signal.
6. **Inheritance and nesting.** As memory: a child's ceiling may not exceed its parent's; siblings may sum past it
   (ADR-0261 points 1 and 2). The untrusted rooms' job has a ceiling of 50% `[assumption]` (ADR-0266), so all
   untrusted rooms together never pass it, and an untrusted room's `--cpu` may be at most that.
7. **Kernel threads and services are never capped.** Services (`fsd`, `netd`, the compositor) serve accounts in turn
   (ADR-0262 point 4); their time is not charged to clients in stage 8.
8. **Observability.** `job_get_usage` gains `cpu.limit`, `cpu.used_ns` (total since creation), `cpu.period_used` and
   `cpu.parked` (times parked). `top` shows the share of the last second (ADR-0267).
9. **Later.** Weights per job (`--weight N`), scheduling the job tree in proportion, when a measurement shows the
   terminal's keystrokes delayed past the bound of INV-0264-3 by trusted work; a burst allowance if a capped room's
   latency matters; lending a client's ceiling to a service, as lottery tickets are lent
   (`quotas/survey.md#cpu`), if a client is measured to make a service spend more than its ceiling.

**On one CPU, "at most a quarter"** means: in every 20 ms period the room's threads run at most 5 ms in total, plus the
overrun of point 4, whether or not anything else wants the CPU; between its budget and the next period the room is
parked. A trusted room may be given `--cpu 100%`, which is no ceiling; an untrusted room cannot pass its parent's 50%.

**Tensions with accepted ADRs.** *ADR-0030:* a slice is armed for a thread of a limited job even when it runs alone,
and parked threads are a second list beside the run queue. *ADR-0023:* one more user of the deadline queue (one
entry). *ADR-0172:* the untrusted rooms' job is a level ADR-0172's jobs do not have (ADR-0269 creates it).

**Examples.**

```
zosh ~> room new agent-1 --untrusted --cpu 25% +w ~/sandbox/agent-1
zosh ~> top
ROOM      MEM            CPU         PROCS
home      41M            4%          12
agent-1   12M of 89M     25% of 25%  8 of 32   (parked 48 times in the last second)
zosh ~> room set agent-1 --cpu 80%
room: agent-1: --cpu 80%: above the untrusted rooms' 50%
```

On the desktop the room's pill shows `cpu 25% of 25%` while the room is parked in most periods (ADR-0263's 90% rule);
show-commands writes `room set agent-1 --cpu 50%` (the untrusted rooms' most) under it.

### Effect of the vision
An agent cannot take the machine's CPU, nor can all agents together take more than half, and a person reads the
ceiling as the sentence they wrote. For the general-purpose base, trusted rooms are uncapped, and the round robin is
unchanged. SMP keeps the unit; per-CPU budgets drawn from one pool per job (as CFS's bandwidth slices,
`quotas/survey.md#cpu`) are the change then.

## Consequences
- **Positive.** A small change to the scheduler; the ceiling a person types is the one enforced; the terminal's
  latency next to a capped room has a bound by arithmetic.
- **Negative.** A capped room is slower even on an idle machine; bursts wait; uncapped trusted spinners still share
  equally with the terminal.
- **Follow-up.** Q4 in `quotas/plan.md`; the latency test below; weights when measured.

## Invariants
- **INV-0264-1.** In every period, the threads under a job with a CPU ceiling run at most its budget plus one slice's
  overrun. *Check:* in-QEMU test: 8 spinners in a room at 25% for 2 s; `job_get_usage`'s `cpu.used_ns` at most
  2 s × 25% + 100 × the overrun bound (one per period), and at least 80% of the budget (the room is not starved
  either).
- **INV-0264-2.** A parked thread does not run until its period ends. *Check:* host test of the scheduler's parking
  logic in the charge crate with a simulated clock; in-QEMU test that a spinner's progress counter does not advance
  while its job is parked.
- **INV-0264-3.** With two untrusted rooms at their default ceilings, each running 16 processes that spin and 16 that sleep until 1 ms before a period boundary and then spin (32, their `--procs`), a keystroke to a terminal in `home` is echoed within 50 ms
  `[assumption]` under HVF. *Check:* GUI test in `tests/gui/` with the runner's key latency measurement, as the
  TASK-24 terminal tests read it.
- **INV-0264-4.** All untrusted rooms together run at most the untrusted rooms' job's ceiling per period. *Check:*
  in-QEMU test: three untrusted rooms at 25% each, 8 spinners in each, for 2 s; their sum at most 50% plus the
  overrun bound.

## Open questions
- The period: 20 ms against 10 ms and 100 ms; measure INV-0264-3's latency and the parking overhead with each.
- Whether a room's ceiling should cover the time services spend on it (lending), once measured.
- *Q4 note (agent, ADR-0001 rule 6):* the period measured with `t_cpu` (HVF, one run each; an uncapped process next to
  two rooms at 25% each under 50%, each with 16 spinners and 16 that burst at the boundary): the longest wake-up
  lateness was 5.7 ms at 10 ms, 9.6 ms at 20 ms and 50.1 ms at 100 ms; the median is 0.7 ms at all three. The 20 ms
  of the ADR holds INV-0264-3's 50 ms with room, 100 ms only just; the parking overhead (a park per spinner per
  period) did not show in the used CPU: 8 spinners at 25% used 502 ms of 500. The GUI half of INV-0264-3 waits for Q6a.
  The details Q4 chose where this ADR is silent are in proposed ADR-0285.

## Revisit triggers
- SMP.
- A trusted workload that delays keystrokes past INV-0264-3's bound: add weights.
- A workload whose bursts a ceiling makes visibly slow: add a burst allowance.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.

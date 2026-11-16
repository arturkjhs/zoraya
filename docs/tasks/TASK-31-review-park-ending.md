# TASK-31: review of `work/park-ending`, ADR-0460 (charge but do not park the thread of an ending process)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff $(git merge-base origin/main origin/work/park-ending) origin/work/park-ending` at `a085862d`:
`crates/zoraya-cpu/src/lib.rs` and `tests.rs`, `kernel/src/process.rs`, `kernel/src/sched.rs`,
`kernel/src/testing.rs`, `user/src/bin/t_cpu.rs`, the rule-6 note of ADR-0285 and the Status column of INV-0460-1 to
-3. Read against ADR-0460 (proposed; the architect chose Option A on 2026-11-16), ADR-0285, ADR-0264 point 5,
ADR-0270, ADR-0300 item 6 and ADR-0004 rule 2, and the code around the diff (every way to EL0, every writer of the exit
record), not the commit messages.

## What was run

In a throwaway detached worktree of `a085862d` (removed afterwards):

| Command | Result |
|---|---|
| `cargo test -p zoraya-cpu` | 15 pass |
| The exception reverted (`Decision::Park { .. } if false && ending(&thread)`), `cargo test -p zoraya-cpu` | 2 fail: `an_ending_thread_of_a_spent_job_runs_and_the_others_park` (no thread picked) and `a_killed_room_dies_in_the_period_after_the_kill_and_pays_for_it` (`left: Some(22)`, `right: Some(3)`) |
| The same revert, `make test-qemu-auto-default` (HVF), once | `a_killed_room_ends_in_the_period_after_the_kill` fails: "a killed room ended 34 periods after the kill's period (3390 ms), 593 parks after the kill, 5108958 ns charged after it" (its `charged > 0` and parent asserts passed; the period bound failed). `t_cpu`'s `a_killed_room_with_parked_threads_dies` **passed** ("a parked room died in 76 ms") |
| `make check` (on this review's branch) | passes |

## Findings

Counts: 1 must-fix (error), 4 should-fix (warning), 4 notes.

### 1. error (must-fix): ADR-0460 says the overrun is repaid; the scheduler never repays it

- **Where:** `docs/adr/0460-charge-but-do-not-park-the-thread-of-a-process-that-is-ending.md:97` (Recommendation, "How A
  keeps `pick`'s rule": "charged (so the next period's budget of the job and of its parents is reduced by what it
  used ...)"); against `crates/zoraya-cpu/src/lib.rs:74-80` (`used_in` is 0 for any period but the one counted) and
  `decide` (`lib.rs:195-215`, the budget left is `budget - used_in(period)`).
- **What is wrong:** there is no overdraft. The teardown's time is added to `used_ns` (the usage record) and to the
  current period's `period_used`, which is already over the budget; at the next boundary the job starts with a full
  budget. The ADR's own open question 2 ("whether the job's `used` figure may go below zero ... an overdraft carried
  into the next period") shows this was undecided, yet the argument the architect is asked to accept states it as
  fact. Only the usage record is true; the budget of later periods is untouched.
- **Scenario:** a room at 10% (10 ms of 100 ms) runs a loop: in each period it spends its budget making a child process
  in the room hold resources whose teardown is long (handles, mappings, a large space within its frame quota), and
  kills it. The child's teardown runs in the next period unparked, past the budget, and the period after starts full
  again. The room's steady use is its ceiling plus one teardown per cycle, never paid back; the parent's ceiling (whose
  `Park { at }` the exception also overrides, `lib.rs:330`) is overrun the same way.
- **Fix:** before the architect accepts ADR-0460, make the text true: either (a) implement the overdraft (carry
  `period_used - budget` into the next period's `used_in`, a change to ADR-0264's accounting that needs its own
  decision, open question 2), or (b) change the sentence to what is built: the teardown is charged to `used_ns` and to
  its period, the overrun past the ceiling is not repaid, and a job that starts and kills processes in a loop overruns
  by one teardown per process; turn open question 2 into a stated consequence. (b) is a text change in a proposed ADR.

### 2. warning (should-fix): INV-0460-1's text and Check name a test that does not exist and a bound nothing checks

- **Where:** `docs/adr/0460-*.md:117-120` and `docs/adr/invariants.md:854`.
- **What is wrong:** the Check says "a QEMU kernel test in `kernel/src/testing.rs` (the case
  `a_killed_room_with_parked_threads_dies` ...)". That name is `t_cpu`'s user test (`user/src/bin/t_cpu.rs:564`); the
  kernel test is `a_killed_room_ends_in_the_period_after_the_kill` (`kernel/src/testing.rs:1474`). The statement says
  "ceiling at 10% and 20 parked spinners ... within one period (plus the floor slice) of the kill"; the kernel test uses
  one thousandth and 40 spinners and checks `died <= killed + 1` (gone before the end of the period after the kill's,
  up to two periods after the kill), and `t_cpu` checks `(PERIOD + 50 ms) * factor()`. "Plus the floor slice" cannot
  hold under Option A: parked threads stay parked to the boundary (`request_exit`'s `sched::wake` does nothing to a
  READY thread, `kernel/src/sched.rs:641-647`), and the teardown then takes its own time (Option A's "in the teardown's
  own time if it does not fit"), far more than 50 us. `t_cpu`'s test passed with the fix reverted (76 ms, table above),
  so it does not check this invariant.
- **Fix:** in both places write the statement as built and the check that exists, e.g.: *"A room killed with its
  budget spent and its threads parked has no process left before the end of the period after the kill's, whatever
  number of steps its teardown has, while its teardowns fit in one period. Check: the test kernel's
  `a_killed_room_ends_in_the_period_after_the_kill` in `kernel/src/testing.rs` (one thousandth, 40 parked spinners, a
  bound in periods), under `make test-qemu`."* Drop `t_cpu`'s test from the Status cell (it stays a regression test of
  ADR-0285 item 7).

### 3. warning (should-fix): INV-0460-2's check passes without what it claims

- **Where:** `kernel/src/testing.rs:1529-1533`; `docs/adr/invariants.md:855`.
- **What is wrong:** the invariant is "the job's usage after the kill is at least the time the teardown ran". The test
  asserts `charged > 0` and `parent_charged >= charged`; the second is always true (the room is the parent's only
  child and `charge` walks up), the first holds for any charge at all. A kernel that charged only part of the way out
  (`leave_current` but not the owner's `teardown`, or a clamp of the charge at the budget) passes. The reverted run
  passed both asserts too. The Check also says `job_get_usage`; the test reads `cpu::usage` in the kernel.
- **Fix:** measure the time independently: a `qemu-test` counter in `sched::schedule` that adds `ran` at every
  switch-out of a thread whose process `is_ending()` in the room's job, reset before the kill; assert `counter > 0` and
  `charged >= counter`. Correct the Check text to that and to `cpu::usage`.

### 4. warning (should-fix): ADR-0460's open question 1 is left unanswered

- **Where:** `docs/adr/0460-*.md:129-130` ("The implementer measures `t_bound` and says").
- **What is wrong:** the implementation chose the floor slice (`crates/zoraya-cpu/src/lib.rs:333`, `MIN_SLICE_NS`),
  the option that keeps ADR-0270's masked-time figures, but neither the ADR (rule-6 note), the commit messages nor a
  report record the choice or a `t_bound` / `kernel_work_is_bounded` figure with the change.
- **Fix:** a rule-6 note under open question 1: the floor slice, and why (the kernel is not preempted, so the slice
  only decides at which step point the thread yields; masked intervals unchanged), with the `kernel_work_is_bounded`
  lines of one run with the change.

### 5. warning (should-fix): the kernel test's period bound is not slow-host proof, as its comment says

- **Where:** `kernel/src/testing.rs:1469-1471` ("so a slow host does not move it") and `:1534-1538`.
- **What is wrong:** `died <= killed + 1` leaves the teardowns only the period after the kill's when the kill lands late
  in period P: the 40 threads come back at the start of P+1, and all 40 ways out plus the test thread's 1 ms nap must
  fit in its 100 ms. The overrun measured under TCG is 58 ms; on a loaded CI runner under TCG (`tcg-default`) or a
  slow host, 40 teardowns can pass 100 ms, and the test fails with the scheduler correct.
- **Fix:** kill at a known phase (nap to just after a boundary, so the teardowns have the rest of P and all of P+1), or
  bound by `killed + 2`, which still fails the reverted kernel by far (34 periods); say in the comment that the bound
  assumes the teardowns fit in a period.

### 6. note: the host test models the way out as one turn

- **Where:** `crates/zoraya-cpu/src/tests.rs:532-600` (the `ending(&th)` branch charges `TEARDOWN` at once and drops the
  thread).
- **What:** ADR-0285 item 7's failure was a teardown with many turns, each re-picked from a spent job. The simulated
  teardown never yields, so "an ending thread is taken again and again from a spent job" is covered only by the first
  host test's single pick. It still fails without the exception (22 against 3). Modelling the way out as k turns of
  the slice `pick` returns, pushed back between turns, would test the real mechanism and the floor slice of each turn.

### 7. note: what runs unparked is more than "the teardown"

- **Where:** ADR-0460 Option A ("Which threads qualify is exactly one kind ...") and the doc comments at
  `kernel/src/process.rs:159-165`, `crates/zoraya-cpu/src/lib.rs:295-302`.
- **What:** the flag is per process, so every thread of it is taken unparked, including one in the middle of a call:
  it runs the rest of the call to its next cancellation point (`steps::between`, a wait's `exit_requested` check)
  before `return_to_user` sends it out; a call that only uses `steps::pause` runs to its end. Bounded by the call's
  limits and no user code runs, so not a bypass; the ADR's "only to run that process's teardown" should say "the rest
  of its current call, `leave_current` and `teardown`".

### 8. note: a process flag through a second closure, not a thread flag read by `job_of` or `decide`

- **Where:** `kernel/src/sched.rs:546-557`; ADR-0460 Option A, "Evidence for".
- **What:** equivalent and simpler (one atomic load under `SCHED`, no lock-order change). The ADR's Follow-up could
  name `Process::is_ending` and `pick`'s `ending` argument so that the text matches.

### 9. note: one CPU is load-bearing for "no user code after `ending`"

- **Where:** `kernel/src/process.rs:159-165`.
- **What:** the argument is that a thread reaches EL0 only through `exit_if_requested` after its last switch. With SMP
  a sibling can be at EL0 on another CPU when `ending` is set and runs there to its next exception. The ADR lists SMP
  as a revisit trigger; the field's comment should say "on one CPU".

## Checked and found sound

- **Who sets `ending`.** The exit record (`Requests::exit`) has one writer, `Process::end_from_now`
  (`kernel/src/process.rs:336`): `request_exit` (from `Job::kill`, `end_every_process`, `signal` with a `Terminate`
  disposition, `set_disposition` to `Terminate` with a request queued, `exit_current`, which `process_exit` and
  `fault_current` reach) and `leave_current`'s last-thread exit. No syscall or handle operation writes it except by
  ending the process; nothing clears it (`exit: None` only in `assemble`). A `Deliver` disposition queues a request and
  never touches the record (`Process::signal`), so a caught signal does not mark a process ending, and a fault always
  ends the process (no user handler of faults, ADR-0034). `ending` is therefore set only when the process truly ends,
  and it is irreversible.
- **No user code after `ending`.** EL0 is entered only through `__exception_return` from `exception_dispatch`
  (`kernel/src/arch/aarch64/exception.rs:470-485`: after a syscall and after an IRQ from EL0, both through
  `process::return_to_user`; a synchronous exception from EL0 that is not `svc` ends the process) and through
  `__user_thread_start` (`context.rs:82`, `user_thread_start`, also the first run of a fork child's or a new thread's
  context). Both run `exit_if_requested` last, after `preempt_point` and `park_if_stopped` (the only switches on that
  way), with interrupts masked to the `eret` (`msr daifset, #0xf` at `__exception_return`), on one CPU. A sibling that
  was spinning in EL0 when `process_exit` ran is in the ready list inside `preempt_point` (timer IRQ from EL0); when it
  is picked by the exception it resumes there and leaves at `exit_if_requested`. A sibling in the middle of a syscall
  resumes in the kernel: blocking waits return `Cancelled` on `exit_requested` (`thread.rs:338`, `:369`, `port.rs:156`),
  calls that build in steps stop at `steps::between` (`steps.rs:184`), and `thread_create` into an ending process
  is refused (`thread.rs:192`). So what an ending process's threads run unparked is kernel work: the rest of a call up
  to its next cancellation point, `leave_current` and `teardown`. User code past the ceiling through this exception: none
  on one CPU.
- **Lock order.** `is_ending` is a lock-free load, read under `SCHED`; the record's lock is taken before `SCHED`
  (design.md lock order), so reading `exit_requested` there would have inverted it. Release after the record, Acquire
  in the scheduler: correct, and on one CPU in any case.
- **Ordinary jobs.** `ending` is evaluated only in a `Park` arm; for a thread whose process is not ending the arms that
  follow are the old ones unchanged (parks counted, the cap of 16, `push_front` and `None`). `Unlimited` and `Run` are
  untouched. An ending thread behind 16 parks is put back at the front and taken by the next call. Kernel threads
  return before (`job_of` is `None`).
- **Fairness.** An ending thread taken by the exception is pushed to the back of the ready list at its next switch, so
  other jobs keep their round-robin turns; it never jumps the queue. How many such threads exist is bounded by the
  room's thread and process quotas, and each one's work by ADR-0270's steps.
- **unsafe.** No new `unsafe`; the one `JobRef::of` block moved into the new call with its `// SAFETY:` comment intact.
- **`spawn_in`** is `qemu-test` only.
- **Totals.** `docs/adr/invariants.md` keeps 850 rows and Total 850; only three Status cells changed.

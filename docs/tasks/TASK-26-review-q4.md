# TASK-26 review of Q4

An independent review by the `opus-review` subagent, which did not write the code, of
`git diff work/n2...work/q4` (merge base `8d0bfcf`), with the code around it in `kernel/src/{sched,timer,steps,sync,job,process,charge,syscall}.rs`. It was
checked against ADR-0264 (INV-0264-1 to -4), proposed ADR-0285, ADR-0030, ADR-0261 (limits, nesting), ADR-0263
item 2, ADR-0270 and row Q4 of `docs/research/quotas/plan.md`. The reviewer ran
`cargo test -p zoraya-cpu -p zoraya-abi -p zoraya-charge` (all passed: 66, 6 and 12 tests),
`cargo clippy -p zoraya-cpu -p zoraya-abi --all-targets -- -D warnings` and
`cargo clippy -p zoraya-kernel --target aarch64-unknown-none --features qemu-test -- -D warnings` (both clean), on an
export of `work/q4`. No QEMU run, so the measured numbers in ADR-0285 and the report were not reproduced. The main
model decides what to do with each finding.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | warning | `crates/zoraya-cpu/src/lib.rs:315-325`, `docs/adr/0285-the-cpu-ceiling-details.md` point 1-2, INV-0264-1 | When a spent job has more than 16 ready threads, one of them runs for each 16 parked, so the overrun per period grows with the number of threads, not "one floor". |
| 2 | warning | `kernel/src/cpu.rs:45-60` | `JobRef::of` is a safe function, and the `SAFETY` argument rests on what its caller does. Safe code can make a dangling `JobRef`. |
| 3 | warning | `user/src/bin/t_cpu.rs:45`, `:145-147`, `:186`, `:231`, `docs/adr/invariants.md` (INV-0264-1, -4) | The whole overrun allowance is scaled by the calibration factor (up to 64). On a slow host (TCG in CI) the ceiling checks then accept up to several times the budget. |
| 4 | warning | `user/src/bin/t_cpu.rs:173-187`, `:220-231` | INV-0264-1 and -4 are checked only with `cpu.used_ns`, which is the counter the scheduler itself enforces with. An under-charge would pass. |
| 5 | warning | `docs/adr/0285-the-cpu-ceiling-details.md:12`, points 1 and 4 | ADR-0285 says its choices are "none against the ADR", but two of them depart from the text of accepted ADR-0264 (points 3 and 5). |
| 6 | warning | `user/src/bin/t_cpu.rs` (whole), `crates/zoraya-cpu/src/tests.rs` | Nothing tests the paths a review of the scheduler needs: more than 16 parks in QEMU, a job tree 32 deep, a killed job's parked threads, and the fallback when arming fails. |
| 7 | note | `kernel/src/sched.rs:427-430` | The `SAFETY` comment at the switch does not name the parked list among the places that keep `prev` alive. |
| 8 | note | `kernel/src/sched.rs:227-232` | `thread_paid` still charges two queue places per thread, but a thread now has three lists it can be in. |
| 9 | note | `kernel/src/sched.rs:323-331`, `:344-347` | The "release everything at once" fallbacks are unbounded masked loops. They cannot be reached at present, and nothing asserts that. |
| 10 | note | `kernel/src/timer.rs:15-18`, `:88`, ADR-0285 driver 2 | Stale text: "the scheduler's one time slice", and "288 entries, one for the slice". |
| 11 | note | `kernel/src/syscall.rs:1287-1290`, `kernel/src/cpu.rs:37-39` | `SET_CPU_PERIOD` takes any value. A period below 1 µs makes low ceilings' budgets zero. Changing the period mid-run reuses period numbers. |
| 12 | note | ADR-0270 Q4 note, ADR-0285 "Measured" | The 1051 µs masked maximum is half of INV-0270-1's 2 ms, and the site that produced it is not named. |
| 13 | note | `kernel/src/cpu.rs:91-93`, `kernel/src/job.rs:149-157` | `cpu` of `None` in `job_set_limits` now removes an existing CPU ceiling. Where this reaches a room is not checked here. |
| 14 | note | `kernel/src/sched.rs:353`, `:365-375` | `reap()` runs before `now` is read, so freeing other threads' stacks is charged to the thread that calls `schedule`. Interrupt time is charged too (ADR-0285 point 4). |

## Findings

**1. warning: the overrun of a job with more than 16 ready threads grows with their number (INV-0264-1).**
`RunQueue::pick` parks at most 16 threads and then runs the next thread whose job is over budget for `MIN_SLICE_NS`
(`lib.rs:320-325`). That thread is pushed back to the end of the queue after its run, and then parked only if fewer
than 16 parks precede it at that pick. So a wave of N over-budget threads gives about N/17 floor runs. Each blocked
thread can also join the ready list once per period through `wake`, which ADR-0264 point 5 says should go to the
parked list but here goes to `ready`.
*Scenario:* the untrusted rooms' job at 50% has the session's 200 processes ready when its budget runs out (spinners,
or sleepers whose deadlines fall together). About 12 floor runs follow in that period, each 50 us plus the time to the
next return to EL0, so roughly 600 us or more against the "one floor" of ADR-0285 point 2. t_cpu allows 250 us per
period and uses 8 spinners per room, so this never reaches 16 parks. The host test `a_pick_parks_at_most_sixteen`
asserts this behaviour but never measures it against the per-period bound.
*Fix:* once the cap is reached, do not run user code of a spent job. Put the thread back at the front, run the idle
thread (or the first unlimited or kernel thread) with `need_resched` set, and let the next `schedule` park the next
16 in another masked interval. Otherwise, restate the bound in ADR-0285 and INV-0264-1's test as
`ceil(N / 16) x (floor + return to EL0)` and test it with more than 16 threads, both on the host model with a
per-period check and in t_cpu.

**2. warning: `JobRef` is unsound as an abstraction.** `JobRef(NonNull<Job>)` is `pub` and `Copy`, and
`JobRef::of(&Job)` is safe. The `SAFETY` comment on `as_ref` (`cpu.rs:56-59`) rests on "the caller keeps the job
alive for the walk". That is a duty of a safe function's caller, which Rust does not allow. `JobTree(PhantomData)`
is also public, so any code in the kernel crate can write
`let r = JobRef::of(&job); drop(arc); JobTree(PhantomData).get(r)`, which is a use-after-free in safe code.
*Present paths, checked:* none is unsound. In `schedule` the charge walk runs while `current` (an
`Arc<Thread>`, and so `Arc<Process>` and `Arc<Job>`) is a local. In `pick` the thread is an owned local from
`pop_front` until after `note_parked`. `set_limit` and `usage` take `&Job`. Parents are `Arc`s held by children
(`job.rs`, `parent: Option<Arc<Job>>`), and `Process::job` is an immutable `Arc<Job>` (`process.rs:127`). No `JobRef`
is stored beyond a call.
*Fix:* make `of` an `unsafe fn` with a `# Safety` section (the job outlives every use of the `JobRef` and of the
ids derived from it), and make `JobTree`'s constructor private to `cpu.rs`, with `charge` and `pick` wrappers taking
`&Arc<Thread>`.

**3. warning: on a slow host the ceiling checks are close to vacuous.** `upper()` allows
`budget + periods x 250 us x factor`, and `factor` is clamped to 1..=64 from a memory-fill calibration.
*Scenario:* under TCG with factor 20, a room at 25% may use 5 ms + 5 ms per 20 ms period, twice its ceiling, and still
pass. At factor 59 or more, the allowance over 2 s (0.5 s + 102 x 14.75 ms) is more than the 2 s elapsed. The
invariants table says `checked: make test-qemu`, and CI runs that under TCG.
The 50 us floor and the period are wall time and do not scale; only the time to the next return to EL0 does.
*Fix:* scale only the return-to-EL0 part, and bound it by what the run measured. The test reads the masked-time counter
anyway, so `upper = budget + periods x (MIN_SLICE + masked_max)`. Alternatively, cap the factor the ceiling check uses
(for example 4) and say in the row that the bound holds under HVF only.

**4. warning: INV-0264-1 and INV-0264-4 are checked with the scheduler's own counter.** `used_ns` is written by the same
`charge` call that `decide` reads. A bug that under-charges keeps enforcement and test in agreement. Examples are a
path that switches without going through the charge, `switched_in` not reset after `adopt_boot_thread`, or a split
that drops time. The INV-0264-2 test does use an independent witness (a counter in a shared page).
*Fix:* add one independent measurement. An uncapped spinner counts in a shared page next to the room, and its rate
is compared with its rate alone over the same interval: it should get at least 75% (or 50% for INV-0264-4), less the
overrun. That needs no kernel counter.

**5. warning: ADR-0285 does not call its deviations deviations.** ADR-0264 point 3 says "Interrupt handlers charge no
one", and ADR-0285 point 4 charges them to the interrupted thread. ADR-0264 point 5 says "A thread woken while its job
is parked joins the parked list" (per job), and ADR-0285 point 1 puts it on the ready list to be parked when picked,
which is what makes finding 1 possible. Both changes are in a proposed ADR, so the process is kept. But
`0285:12` says "none against the ADR", and the architect could accept it without seeing that two points of an
accepted ADR change. *Fix:* reword the Context, and list the two points under "Tensions with accepted ADRs".

**6. warning: the hostile paths are untested.** No test covers these:
- More than 16 parks in one pick in QEMU (see finding 1).
- A job tree 32 deep with a ceiling at the leaf, for the masked time of a charge walk plus 17 decide walks of 32
  nodes each. That is the INV-0270-1 worst case of `schedule`; t_cpu's trees are 2 or 3 deep.
- `job_kill` of a room whose threads are parked. ADR-0285 point 7 claims they end at the next boundary; t_cpu's
  `end()` kills running rooms, which may or may not be parked at that moment.
- The fallback when arming the release deadline fails. INV-0285-1 claims it, but the host model releases at every
  boundary unconditionally and never models arming.
*Fix:* add the depth-32 and more-than-16 cases to t_cpu with `check_masked`, add a kill-while-parked case (park with a
100 ms period, then kill at +40 ms and wait for `processes.used == 0` within one period), and either model arm failure
in `simulate` or say in INV-0285-1 that the fallback is unreachable (finding 9).

**7. note.** `sched.rs:427-430` lists "the ready queue, the dead list, `s.idle`, ..." as what keeps `prev` alive.
`prev` can now be in `RunQueue::parked` too: it was pushed to `ready`, and `pick` parked it at once when it was alone.
The argument still holds, but the comment should name the list.

**8. note.** `thread_paid` (`sched.rs:229`) charges `2 * size_of::<usize>()` for "a place in each of the scheduler's
two queues". `count_new` now reserves room for every thread in `ready`, `parked` and `dead`, so the charge is 8 bytes
short per thread (ADR-0261 point 5). *Fix:* charge 3 words.

**9. note.** The two `while s.ready.release() > 0 {}` loops (`sched.rs:330`, `:346`) move every parked thread in one
masked interval. They cannot be reached as things stand: `arm` keeps the total at 286 or fewer, and the scheduler has
at most one slice and one release deadline queued, since `release_armed` guards the second and the fired entry is
popped before `period_end` runs. So `arm_reserved` from these two sites always finds a slot. If they were reached, the
loop in `arm_release` would also livelock with idle: park, arm fails, release all, idle reschedules, park again.
*Fix:* replace them with a `debug_assert!` and a comment on why there is a slot, or bound them.

**10. note.** The `arm_reserved` doc (`timer.rs:88`) still says "for the scheduler's one time slice", and ADR-0285
driver 2 says "288 entries, one for the slice". Both should say two: the slice and the period's release.

**11. note (test kernel only).** `SET_CPU_PERIOD` accepts any `a0`. A period under 1000 ns gives `budget_ns(1, p) = 0`,
and below the timer's resolution `period_end` re-arms itself into a storm. After a change of period, `CpuAcct.period`
numbers from the old length are compared with numbers of the new one. That is harmless at the values t_cpu uses
(10, 20 and 100 ms). *Fix:* refuse values under 1 ms.

**12. note.** The masked maximum over the Q4 tests, 1051 us under HVF, is half of INV-0270-1's bound, and neither
ADR-0270's note nor the report says which site produced it. If it is the boundary storm (32 sleepers' `wake_sleeper`
and `period_end` in one `on_interrupt` loop), that predates Q4. If it is `schedule`, it is the headroom that finding
6's depth-32 case would use up. *Fix:* record the site (the test kernel's counter can attribute it) in the ADR-0270
note.

**13. note (outside the diff).** `job_set_limits` with `cpu: None` now removes a job's CPU ceiling, as `None` does for
memory. A caller that sets only memory, the way t_quota's `limits()` helper does, silently drops the CPU ceiling. For
rooms (Q6a), check that no process inside a room holds `LIMIT` on its own room's job, and that `room set --mem`
re-sends the CPU ceiling.

**14. note.** `reap()` (`sched.rs:353`) runs before `now` is read, so the thread entering `schedule` pays for freeing
other threads' stacks, and a capped room pays for an uncapped one's exits. It is small after Q2. Charged interrupt
time is recorded in ADR-0285 point 4.

## Checked and found sound

- **Lock order and masking.** `SCHED`, then `timer::QUEUE` (`arm_reserved` and `cancel` in `schedule`), then a job's
  `cpu` lock (`JobTree::get`/`set`). Nothing is taken under `cpu`: `Job::usage` takes `state` and then `cpu`, which
  agrees with `cpu` being a leaf. `on_interrupt` calls the callbacks with `QUEUE` released (`timer.rs:110-112`), so
  `period_end` taking `SCHED` and then `QUEUE` does not invert the order. `SpinLock::lock` masks IRQ and FIQ
  (`sync.rs:49`), so neither the timer interrupt nor `period_end` can arrive while `SCHED` or any `cpu` lock is held.
  `period_end` and `end_of_slice` touch no `cpu` lock anyway. `job_set_limits` takes each `cpu` lock alone and masked,
  and the kernel is not preempted between those takes, so on one CPU there is neither a "lock taken twice" panic nor a
  lost update. On SMP, the get/modify/set of `charge` against `set_limit` would race (an ADR-0264 revisit trigger).
- **Bounded masked work.** A charge walk is at most 32 nodes (`MAX_DEPTH`), and a pick is at most 17 decide walks of
  at most 32 nodes each. A release moves at most 64 threads, and if more remain it re-arms for `now`; `on_interrupt`'s
  single `now` snapshot defers that to the next interrupt.
- **Budget arithmetic.** `budget_ns` is computed in `u128`, and every add and subtract saturates. The boundary split
  (`since = max(period_start(now), switched_in)`) charges only the post-boundary part to the new period, and the
  pre-boundary part of such a run is at most the slice, which is at most the budget left. A run therefore never puts
  a period over budget by more than the floor plus the time to EL0. A ceiling of 1000 never parks, since
  `used <= now - period_start < period`. Lowering a ceiling below the current use parks at the next pick. Nesting takes
  the minimum of the ancestors' ceilings, and `AboveParent` matches the memory rule of `zoraya-charge`.
- **Liveness.** A parked thread is `READY` and owned by `RunQueue::parked`. `wake` acts only on `BLOCKED`, so a thread
  is never queued twice. Every park is followed in the same masked section by `arm_release`. `release_armed` is cleared
  only once nothing is parked. A thread released early, by a late or stale deadline, is decided again at `pick`, so
  INV-0264-2 holds by construction. Parking happens only inside `schedule`, which is reached with no spinlock held (the
  test kernel asserts this at step points). Kernel threads, idle and the reaper have `process == None` and are never
  limited. A lone limited thread gets its slice (`sched.rs:410-411`). The idle loop reschedules after a release
  through the timer's interrupt. The reserved-slot accounting holds 286 normal and 2 reserved entries.
- **The usage record.** The kernel writes `min(len, 120)` bytes after checking for at least 88. The decoder is total
  and reads missing CPU words as zero and no limit; a partial word (a buffer of 89 to 119 bytes) is ignored. The
  88-byte decode test is present.
- **SMP configurations.** The kernel brings up one CPU, and nothing in the diff depends on the QEMU `-smp` value.

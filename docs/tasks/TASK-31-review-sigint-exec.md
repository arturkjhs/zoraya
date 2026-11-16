# TASK-31 review: work/sigint-exec (a Ctrl-C lost across `exec` and at the entry of a wait)

Independent review by the `opus-review` subagent of `git diff origin/main...origin/work/sigint-exec` at eddfd477
(2026-11-15). The branch starts from c5fea4e2. It was read against CLAUDE.md, ADR-0004 rule 3, ADR-0106, ADR-0315,
the proposed ADR-0392 on the branch, and the backlog entry that `origin/main` (4640c56e, `work/tcg-slow`) added for this
defect. Line numbers are those of eddfd477.

What the reviewer ran:

- `cargo test -p zoraya-posix --test threads` on the head: 20 passed.
- Hand reverts in a scratch worktree, one at a time, each run against `--test threads`: the `interrupt_wait` check in
  `wait_for` (`jobs.rs:177`) made `a_handler_run_at_a_calls_entry_*` fail. `exec_carry` without its filter, without
  the `ignored` test, and without `signals.shared` each made `an_exec_hands_on_*` fail.
- The two C-boundary hunks reverted together (`exec()` with `carried = 0`, and `waiting` always going to
  `restartable`), against the whole `cargo test -p zoraya-posix`: exit 0, every suite green (finding 4).
- `make check` on the head: exit 0.
- `git merge-tree` of the head and `origin/main`: clean.
- `git ls-tree` of every `origin/` branch for `docs/adr/039[0-5]*`.
- No QEMU run. `zosh.script` is not a short script, and its Ctrl-C steps depend on timing.

Severities: **must-fix** (wrong before merge), **should-fix** (fix in this part or record it), **note**.

## Findings

1. **must-fix** `crates/zoraya-posix/src/jobs.rs:316`, called from `crates/zoraya-posix/src/ffi/procs.rs:2203`: the
   change breaks accepted INV-0106-2 and does not say so. `exec_carry` calls `self.sys.take_request()` (a
   `process_take_request`) on every `execve`, including in a process with no handler installed. INV-0106-2 allows
   that call only in `kill`, `sigaction`, `sigprocmask`, `sigsuspend`, `pause`, `raise` and `abort`. ADR-0392 says
   the opposite twice: driver 2 says "no cost for a process without handlers (INV-0106-2)", and Option A's evidence
   says "a handler-less process pays nothing". Only its Cost line has "one `process_take_request` per `exec`". The
   invariant's check cannot see the call, because `exec_carry` does not increment `stats.takes` (`next_delivery` does,
   at `jobs.rs:264`). So `ZORAYA_POSIX_STATS=1 t_posix files` would still print `takes=0` for a handler-less `execve`
   workload. *Scenario:* `make`, or any handler-less shell, running thousands of `execve`s pays one wasted system
   call each, and the INV-0106-2 row still reads as checked. In a handler-less process the kernel queue is empty
   when `exec` runs, for three reasons: its dispositions are `Terminate` or `Ignore` (ADR-0106 item 2 for blocked
   ones); `sigwait` drains on its way out (`drain_requests`); and moving a disposition back to `Terminate` with a
   request queued ends the process at once (ADR-0004 rule 2, `kernel/src/process.rs` `set_disposition`). *Fix:* take
   the queue only when `self.signals.handlers_installed() || self.sigwait_active()`, the condition that `poll` and
   `interrupt_sources` use, and keep the pending sets unconditional (they are library memory). Increment
   `stats.takes` for each take. Add a host assertion that `exec_carry` on a handler-less process makes no take: the
   fake `Sys` already counts requests.

2. **should-fix** `docs/adr/0392-signals-across-exec-and-at-the-entry-of-a-wait.md:106` (Follow-up) and
   `docs/design.md` (stage 6d paragraph): the gap "old image *ended* during its own spawn" is real, but the claim that
   it "needs the kernel's spawn to refuse, or end, a process made by a caller whose exit is requested … which is a
   kernel decision" is not accurate. The library can close the gap itself. Before `spawn_into`, `exec` sets the old
   image's kernel disposition to `Deliver` for each default-terminating, non-ignored signal. A job signal during the
   spawn then waits in the old image's queue instead of ending it. `exec_carry` takes it to the new image. If the
   spawn fails, `exec` puts the dispositions back. `set_disposition(Terminate)` with a request queued ends the process
   at once (ADR-0004 rule 2), which is the POSIX outcome of a signal during a failed `execve`. *Scenario as recorded:*
   `exec /bin/sleep 20` from a program that does not catch `SIGINT`, with Ctrl-C during the spawn. The walk ends the
   old image, the new one is made after the walk, and it runs its 20 s. *Fix:* add this as an option in ADR-0392,
   with its cost (about 25 `process_set_disposition` calls each way per `exec`, and the take of finding 1 then needed
   in every process). Either adopt it or say why the kernel route is preferred. Correct design.md's sentence to match.

3. **should-fix** ADR-0392 (Context, Follow-up, Open questions): the `fork` window is not recorded. The backlog
   entry that this branch fixes (`origin/main` `docs/backlog.md:307-318`) located the loss "during or just after
   `exec`/`fork`". ADR-0392 handles `exec` only, and nowhere says that `fork` has the same hole. A fork child is a
   process made during a job signal's walk (ADR-0004 rule 3), so by the ADR's own reasoning it can miss the request.
   *Scenario (not run; it follows from the same rule):* `/bin/sh -c 'echo go; /bin/sleep 20; echo x'`. The sleep is
   not the last command, so dash forks for it. If the Ctrl-C lands during the fork, the parent dash (which catches
   `SIGINT`) notes it and waits for the child. The child runs its full 20 s, and only then does the job end with 130.
   Linux closes this by restarting `fork` when a group signal comes during it. `zosh.script` has no case with a fork.
   *Fix:* record the fork window in ADR-0392's follow-up and in design.md next to the `Terminate` gap, as part of the
   kernel question. Optionally, add a `zosh.script` step with a forked sleep, marked as expected to be flaky until the
   kernel side exists.

4. **should-fix** `crates/zoraya-posix/src/ffi/procs.rs:121-137`, `:677-681` and `:2203-2210`: the C-boundary half of
   the fix has no host test, and `nanosleep`'s new branch has no test at all. The reviewer replaced
   `p.exec_carry(facts.ignored)` with `0u64` and made `waiting` always take `restartable` (`if true || …`). Every
   suite of `cargo test -p zoraya-posix` still passed. The two host tests exercise `Posix::exec_carry` and the
   `interrupt_wait` field directly, with the flag set by hand, so they would pass if `exec()` never called the carry,
   or if `waiting` never set the flag. The only cover is `zosh.script`'s `go-a`, `go-r` and `go-c` steps. Those catch
   the defect only when the Ctrl-C lands inside the window (exec's load, or between two of dash's library calls).
   That probably happens on most runs, but it is not forced. INV-0392-2 does not state the `nanosleep` rule (any
   handler, `SA_RESTART` or not), and no check covers it. *Fix:* (a) add the two hand mutations to a list in
   `tools/mutants/` (CLAUDE.md, "A mutation found by hand…"), so that the next run repeats them against the QEMU
   steps. (b) Add a deterministic in-QEMU `t_posix` case. A child sends `SIGUSR1` to the parent, whose handler has no
   `SA_RESTART`, and then sets a flag in a `MAP_SHARED` page (F3 has merged). The parent spins on the flag without a
   library call, then `read`s an empty pipe and must get `EINTR`. With `SA_RESTART`, the same read must return the
   byte a second child writes later. A `nanosleep(1 s)` after the same spin must be `EINTR` with about 1 s left.
   (c) Add the `nanosleep` rule to INV-0392-2, or give it an invariant of its own.

5. **should-fix** `docs/backlog.md` after the merge: the branch was cut before `work/tcg-slow` merged. `origin/main`
   now carries the entry "A Ctrl-C typed just after a job's first output is not acted on … Not fixed"
   (`docs/backlog.md:307-318` on `origin/main`). The merge is textually clean, so that entry would stay on `main` and
   say "not fixed" for the defect this branch fixes. ADR-0392's Context cites "`docs/backlog.md`, part 10c's
   leftovers", which does not exist on the branch. The entry also asks for "a check … with a limit of a few seconds,
   once the cause is fixed". The branch's `x ended-a`/`x ended-r`/`x ended-c` lines with the 10 s sleep are that
   check, and the entry should say so. *Fix:* merge `origin/main`. Rewrite the entry as fixed by ADR-0392, keeping
   what remains open (findings 2 and 3). Also look at `docs/backlog.md:174` (TASK-29's step V6, a Ctrl-C after `fg`
   lost while `sh -c '…; echo second; sleep 30'` goes on). dash execs `sleep 30`, the last command of `-c`, right
   after `second`, which is the same `exec` window. Either restore V6's late Ctrl-C in `scenarios.script` and see it
   pass, or note in the entry that it may be the same cause.

6. **note** `crates/zoraya-posix/src/jobs.rs:315` (`self.signals.pending | self.signals.shared`): a signal that was
   pending because it was blocked ends the new image at once. POSIX keeps it pending, with the carried mask, until
   the new image unblocks it. *Scenario:* a program blocks `SIGTERM` with a handler installed, `SIGTERM` arrives, and
   the program `execve`s a program that never unblocks it. On Linux the program runs. Here it ends with `SIGTERM` as
   soon as it starts. Before this change it ran (the signal was lost). This is consistent with ADR-0106 item 2 and the
   exception in INV-0106-4: the same signal sent after the `exec` would also end it. ADR-0392 mentions it under
   "Evidence against". INV-0392-1 should say "including blocked ones", so that the behaviour is part of the
   invariant and not only of the discussion.

7. **note** `crates/zoraya-posix/src/ffi/procs.rs:2188-2203`: between `spawn_into` returning and `exec_carry` taking
   the queue, a job signal reaches both images (the new one is already in the job). The new image can then receive
   it twice: once directly and once carried. The kernel queue does not coalesce (`zoraya-events`
   `RequestQueue::push`); only the carry's bitset coalesces the old side. The new image runs a handler twice only if
   it installed one inside that window. ADR-0392 records this as "the duplicate". It is accepted here, but the
   revisit trigger should name the double handler run, not only "runs that handler instead of dying".

8. **note** `crates/zoraya-posix/src/ffi/procs.rs:677`: `nanosleep` is `EINTR` with all of its time left whenever
   the entry poll ran any handler, `SA_RESTART` included, even for `nanosleep({0, 0})`. Linux never interrupts a
   sleep for a handler that ran before the sleep began. This is POSIX-permissible (the program cannot tell the
   timings apart), and `rem` is written. A program with a periodic `SA_RESTART` handler (make's `SIGCHLD`) will see
   more `EINTR`s from `usleep` loops. Record it in ADR-0392's consequences.

9. **note** numbering and tables: ADR-0392 is free. No `origin/` branch has `docs/adr/0392*`. `docs/adr/039[0-5]*`
   exists only as `0390` on `work/10d`, `work/10g` and `work/ports-early`, and `0391` on `work/10d`. However,
   `work/10d` (`0390-surface-for-curses-less-and-vi.md`) and `work/10g` (`0390-what-part-10g-decided.md`) both use
   0390. That collision is for the architect, outside this branch. `docs/tasks/TASK-31-report.md:22` lists the
   numbers handed out and does not include 0392. Whoever reserved it should add it there. The invariants table has
   both rows, and Total 820 → 822 matches (`make check` passes). `tools/test-map.txt` has no line for
   `docs/adr/0392-*`, while ADR-0389 has one (line 215). An ADR-only change then runs every step. A line like
   0389's (`check test-libposix test-posix-threads test-posix`) would narrow it.

## Checked and found sound

- **The flag cannot go stale.** `waiting` sets `interrupt_wait` and clears it inside one `state` run, under the
  library lock. `wait_for` takes it before any deferral (`jobs.rs:177`), so `Err(PENDING)` never escapes a flagged
  run and no other thread sees the flag. A first run has no `resume`. A handler that `longjmp`s out of the entry poll
  leaves before the flag is set. So a `waitpid`/`wait4` is `EINTR` only when the entry poll of that same call ran a
  handler without `SA_RESTART`.
- **No new `EINTR` for handler-less or `SA_RESTART` programs.** `poll` returns `ran: false` without a handler, and
  `waiting` then goes to the unchanged `restartable`. When every handler that ran had `SA_RESTART`, it does the same.
  Every `EINTR` that `waiting` gives is one that the same wait could already give. A read with data ready returns the
  data (host test), and a `write_stream` that did part of its work returns its count.
- **Coverage of entry-poll calls.** Every call with an entry poll and an interruptible wait now goes through
  `waiting`, except `connect` and `getrandom`, both recorded. `getaddrinfo`'s lookup waits without interruption.
  `pselect`/`select`/`poll` were already `EINTR` when the entry delivery ran a handler. `readv`/`writev` go through
  `read_fd`/`write_fd`.
- **No pid reuse, no wider sender.** The carry signals the process handle `spawn_into` returned, not a pid. It sends
  only what was queued to this pid's old image (by senders the broker and kernel already allowed for this pid and
  job) or what the image's own pending sets held. The old image holds a handle with `Rights::SIGNAL`
  (`ProcessObject::RIGHTS`) to its new image and could signal it directly anyway. No group, job or room rule of
  ADR-0331 to ADR-0342 is bypassed.
- **Dispositions across `exec`.** Handlers reset to default and `SIG_IGN` is kept. The filter `default_terminates &&
  !ignored` drops what the new image would ignore (`SIGCHLD`, `SIGWINCH`, `SIGURG`, `SIGCONT`, and `SIG_IGN` ones).
  Stop signals are not carried (recorded). A fork child carries nothing of its parent:
  `after_fork_in_child` clears `pending` (`jobs.rs:1248`) and `shared` (`threads.rs:160`).
- **The commit.** The broker's `ExecCommit` checks only the handle types (`zoraya-broker/src/lib.rs:1120`). A carried
  `SIGINT` that ends the new image before the commit is therefore reported as the job's `SIGINT`, not as 127.
- **Each signal is carried at most once**, through the bitset. A failed spawn carries nothing: the carry comes after
  `Ok(h)`.
- **No `unsafe` was added.**
- **Gaps (3).** "Terminate during own spawn" and "handler installed at once" are real and recorded (but see finding
  2). The `connect`/`getrandom` open question is accurate. `fork` is missing (finding 3).
- **`zosh.script`.** The `x ended-*` lines fail under the old behaviour: the job's `ended-a=0` comes before the
  prompt after 10 s, and `go-r`'s read never ends, so the run hits its limit. The echoed command line cannot match,
  because of the `end""ed` quoting.

## Check of the changed lines

The `opus-review` subagent read `git diff eddfd477..origin/work/sigint-exec -- crates tests tools user kernel` at
c9c56942 (the fix pass), with the code around it, on 2026-11-15. Line numbers are those of c9c56942. It ran
`cargo test -p zoraya-posix` in a throwaway worktree (exit 0, 279 tests in 10 suites, including the three `an_exec_*`
tests) and `make check` on this branch. It made no QEMU run. The `zosh.rs` and `zosh.script` hunks come from the merge
of `origin/main` (`work/tcg-slow`), not from this fix pass.

### Findings

10. **must-fix** `docs/adr/0392-signals-across-exec-and-at-the-entry-of-a-wait.md:1` and `:84`, `docs/adr/0106-signals-in-libposix.md`
    (Open questions), and `docs/architect-queue.md` (section A): the hold breaks two parts of accepted ADR-0106, and
    the branch states this in only one sentence of a Cost bullet. The title still says "where ADR-0106 is silent".
    (a) INV-0106-2: a handler-less `execve` now always makes at least one `process_take_request`, because `held != 0`
    (`jobs.rs:359`). The ADR says so at `:84`. The row in `docs/adr/invariants.md` and INV-0304-5, which cites the
    list, are unchanged. (b) Item 2's mapping from a POSIX action to a kernel disposition: during the spawn, every
    `SIG_DFL` signal whose default terminates is `Deliver`. The ADR does not name this deviation. It is the same kind
    of deviation as ADR-0315 item 10, which has a rule-6 note in ADR-0106's open questions. This one has none. (c)
    ADR-0392 is not in `docs/architect-queue.md`, so the architect has no entry for accepting either deviation.
    *Scenario:* the branch merges, and on `main` ADR-0106 and the invariants table still say that a handler-less
    process takes no request outside seven functions, while every `execve` takes one. The architect learns this only
    from a Cost bullet in a proposed ADR. *Fix (documents only):* give ADR-0392 a "Tensions with accepted ADRs"
    paragraph that names INV-0106-2, ADR-0106 item 2 and INV-0304-5, and says that the architect accepts the
    deviations or asks for a superseding ADR. Drop "where ADR-0106 is silent" from the title, the index and the
    invariants heading, or make it say that ADR-0392 also amends ADR-0106. Add a rule-6 note to ADR-0106's open
    questions like the one for ADR-0315. Add an ADR-0392 row to section A of `docs/architect-queue.md`.

11. **should-fix** `docs/adr/invariants.md:641` (INV-0392-1, Status `checked`) and `tools/mutants/sigint-exec.toml:17`:
    the clause "a signal sent while the new image is made is held for that" has no check. `S-exec-holds-nothing` is a
    known survivor. The host test `an_exec_holds_*` tests `Posix::exec_hold` on its own, and no test sees that
    `exec()` calls it. The same is true of the failure path: deleting `p.exec_release(held);` (`procs.rs:2197`) is
    not in the mutation list, and it would survive every test too. *Scenario for the second:* a spawn fails after the
    load (`E2BIG`, `EAGAIN`, an inherit failure) in a program with no handler, and `execve` returns -1. `SIGINT`,
    `SIGTERM` and about 20 other signals stay `Deliver` with no handler, and `poll` never looks
    (`handlers_installed` is false). From then on, every Ctrl-C and `kill` to the process is silently ignored. *Fix:*
    in the row's Status, say that the hold is not checked (ADR-0392 open question). Add `S-exec-never-releases`
    (`find = 'p.exec_release(held);'`, `replace = ''`) to `sigint-exec.toml` as a known survivor. A test that is
    cheap to write: a `t_posix` case whose `execve` fails at the spawn, which then sends itself a `SIGTERM` with
    `kill` and must end with it.

12. **note** `docs/adr/0392-…md:81-83` (Cost): the cost is an estimate, not a measurement, and two of its figures are
    wrong. The count is 22, not "about 25": `default_terminates` covers 1-8, 10-16, 24-27 and 29-31. On a failed
    spawn the calls happen twice. The per-call figure cites INV-0006-3, which is the latency of `port_wait`, not of
    `process_set_disposition`. Nothing measured `exec` with and without the hold, although `stats.exec_ns` exists
    (`procs.rs:2223`) and `ZORAYA_POSIX_STATS=1` would show it. Against the measured job cost, the estimate is small:
    70 `/bin/sleep` jobs took 29 s under TCG (`zosh.script:262`), about 400 ms each, so 22 calls at 0.3 ms total
    are under 0.1 %. The shell's 70-job loop no longer `exec`s at all (`( : )`, from `work/tcg-slow`), so no
    exec-heavy loop in the test set exercises the hold's cost. *Fix:* correct the count and the cited figure, and
    either measure `exec_ns` once with and without the hold (HVF, `ZORAYA_POSIX_STATS=1`), or keep `[estimate]` and
    leave the revisit trigger as it is.

13. **note** ADR-0392 Follow-up (`:144`): the gap recorded for `fork` has a second case, `posix_spawn`. Its child
    goes into the parent's group job (`bp::group::INHERIT`), so it is also a process made during a job signal's walk
    (ADR-0004 rule 3). Nothing hands it a signal it missed, and it is not recorded. `make` and other spawn users
    reach it. *Fix:* say "`fork` and `posix_spawn`" in the follow-up and in `docs/design.md`.

14. **note** numbering: 0392 is still free on every `origin/` branch. The index, the invariants rows and the
    per-ADR count (2, Total 822 for 822 rows; `make check` passes) agree. `docs/tasks/TASK-31-report.md:22` still
    does not list 0392 among the numbers handed out (finding 9). The `test-map.txt` line was added.

### Checked and found sound

- **The new image's dispositions.** The new image's dispositions do not depend on the hold. `spawn_into` writes them
  into the spawn record (`start_dispositions`, `procs.rs:1760`): `Ignore` for the carried `SIG_IGN` set and for
  signals whose default ignores, `Terminate` for every other signal. So a held signal is back at its default, an
  ignored one stays ignored, and handlers start as `SIG_DFL`. A held signal that was carried ends the new image with
  that signal, which is the POSIX outcome of a signal pending across `execve` with its default action.
- **No stale value is put back, and no other thread interferes.** `exec` runs the hold, the spawn, the release or the
  carry, and the exit inside one `state` closure, which holds the library lock until `process_exit`. The broker
  calls inside it use `call` (`process.rs:523`), which waits on the connection only. Nothing in the window takes a
  request or runs `deliver`. Another thread cannot run `sigaction`, `sigwait_begin` or a poll until the process has
  ended. `exec_release` does not restore a saved value: it recomputes `effective_disposition`. A thread already in
  `sigwait`, outside the lock, has its signals at `Deliver` through `effective_disposition`, and the hold skips them.
  A woken waiter cannot take the queue without the lock.
- **Killed between the hold and the carry.** Every POSIX signal the broker sends is a `job_signal`
  (`zoraya-broker/src/lib.rs:1327-1405`). Before the spawn, a signal reaches only the old image, which queues it.
  After the spawn, it reaches both images. `job_kill` is not a request and is not held. Its own rule-3 window for a
  new image made during the kill's walk existed before this change and is unchanged.
- **No silent loss (check 3).** Whenever the hold set any disposition, `held != 0` and the carry takes the queue
  (`jobs.rs:359`). With `held == 0`, no handler and no `sigwait`, no disposition is `Deliver`, so the queue is empty.
  A `set_disposition` that fails leaves its signal as `Terminate`, which is the behaviour from before. A request that
  reaches the old image after the carry has a copy in the new image, which is in the job.
- **The `ignored` test in `exec_hold`** is redundant: `ignored_for` is the old image's `SIG_IGN` set, whose kernel
  disposition is `Ignore`, not `Terminate`. It is harmless.
- **The host tests** assert what they claim: `stats.takes` is counted per take, there is no take without a handler
  or a hold, and a held signal's disposition is set to `Deliver` and then put back.

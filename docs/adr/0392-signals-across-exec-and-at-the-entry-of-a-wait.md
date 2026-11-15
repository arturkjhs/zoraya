# ADR-0392: Signals across `exec` and at the entry of a call that waits, where ADR-0106 is silent

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0004, ADR-0104, ADR-0106, ADR-0190, ADR-0315, ADR-0331
- **Evidence:** CP-10

## Context

In `sh` inside `zosh`, a Ctrl-C typed right after a job's first line was lost (`docs/backlog.md`, the leftovers of
part 10c, "A Ctrl-C typed just after a job's first output"): `/bin/sh -c 'echo go; exec /bin/sleep 20'` ran its 20 s
under HVF, and under TCG `/bin/sh -c 'echo go; read x'` never ended. `zosh` had the job's `tcsetpgrp`, sent `SIGINT`
to the job's group, and the broker's `job_signal(Interrupt)` returned `Ok`. Two library paths lost the signal; both
are places where ADR-0106 says nothing.

**Measured (traces in the kernel's `Process::signal` and `take_request` and in `libposix`'s delivery, since removed).**

1. **`exec`.** dash catches `SIGINT` in `sh -c` (`setsignal`: `iflag || minusc`), so the kernel's disposition is
   `Deliver`. `execve` in `libposix` (ADR-0104 item 6) spawns the new image as a process of its own in the pid's job,
   hands the broker over and ends the old image with `process_exit(0)`. A `job_signal` made while the old image was
   loading the new one was queued on the old image only: ADR-0004 rule 3 lets a process made during the walk be made
   after it, and the new image starts with its handlers back to `SIG_DFL`. The old image did not look at its queue
   again, so the request ended with it. With a probe that printed what the old image's queue held after the spawn,
   every such Ctrl-C showed `SIGINT` there (`0x2`), and `zosh.script`'s step printed `ended-a=0` after the full sleep
   before the change, the prompt 46 ms after the Ctrl-C with it.
2. **The entry of `read`.** dash reads its input under `INTOFF` (`preadbuffer`) and acts on a `SIGINT` its handler
   noted only when the `read` returns. A request that came while dash ran between two library calls was taken by the
   poll at `read`'s entry (ADR-0106 item 5): the handler ran (`onsig`, which only notes it under `INTOFF`) and the read
   then waited for input that never came. The trace showed `Deliver`, the request taken and the handler run, then
   nothing. On Unix the signal is acted on where it arrives: before `INTOFF` the handler ends the shell, during the
   read the read is `EINTR`. Wait-point delivery (CP-10, ADR-0106 Option C) moved the handler to a point Unix never
   runs it at, the start of a call that then blocks.

**Not measured, from the same rule (the review of the first version).** A program *without* a handler for the signal
that `exec`s: the job signal's walk ends the old image (`Terminate`) and misses the new one, made after the walk, which
runs on. And `fork`: a child made during a job signal's walk can miss it the same way. `/bin/sh -c 'echo go;
/bin/sleep 20; echo x'` forks for the sleep (it is not the last command); a Ctrl-C during that fork is noted by the
parent dash, which then waits for a child that runs its 20 s.

## Decision drivers

1. A signal a process was sent is acted on or stays pending; POSIX keeps a pending signal across `execve` and the
   kernel's guarantee for job signals (ADR-0004 rule 3) does not reach a process made during the walk.
2. CP-10 and ADR-0106: no asynchronous interruption; the fix must stay inside the library's wait points and entry
   poll, with no new kernel call; a process without handlers keeps paying nothing on its ordinary calls (INV-0106-2).
3. The `EINTR` paths of the library's waits exist and are used: a wait point already returns `EINTR` when a request
   comes during it (ADR-0106 item 6, ADR-0315), so a call that may wait already handles it.
4. dash, the shell every POSIX job of `zosh` runs under, must be interruptible at once (ADR-0331).

## Considered options

### Option A: hold and carry the old image's signals across the spawn, and let an entry delivery interrupt the call's wait

- **Description:**
  1. **Hold.** `execve`, once the program is loaded and just before the spawn of the new image, sets every signal
     whose kernel disposition is `Terminate`, whose default ends a process and which the new image does not ignore to
     `Deliver` (`Posix::exec_hold`). A job signal during the spawn then waits in the old image's queue instead of
     ending it. If the spawn fails, each held disposition is put back (`exec_release`); a request queued meanwhile then
     ends the process at once (ADR-0004 rule 2), the POSIX outcome of a signal during a failed `execve`.
  2. **Carry.** Right after the spawn, `execve` takes the old image's request queue (only when something can be in
     it: a handler, a `sigwait`, or a hold; each take counted in `stats.takes`) and its pending sets, blocked ones
     included (`Posix::exec_carry`), keeps the signals whose default ends a process and that the new image does not
     ignore (`SIG_IGN` is carried, ADR-0190 item 18), and sends each to the new image with `process_signal`. From
     the spawn on, a job signal reaches the new image itself.
  3. **Entry.** A call that may wait (`read`, `write`, `pread`, `pwrite`, `recv`, `send`, `recvmsg`, `sendmsg`,
     `waitpid`, `wait4`) whose entry poll ran a handler without `SA_RESTART` makes its first wait point `EINTR`
     instead of a wait (`waiting`, `Posix::interrupt_wait`); with `SA_RESTART` handlers only, it waits as before; a
     call that does not reach a wait returns its result. `nanosleep` whose entry poll ran any handler, `SA_RESTART`
     or not, is `EINTR` with all of its time left (POSIX never restarts it).
- **Evidence for:** Drivers 1 to 4; the change is in the library; every `EINTR` it gives is one the wait could already
  give; it closes the handler-less `exec` case too, which no kernel change is needed for.
- **Evidence against:** The new image gets a carried request a little after it starts: one that installs a handler
  at once may run that handler instead of dying of it, and a signal sent between the spawn and the carry reaches the
  new image twice (directly and carried; the kernel queue does not coalesce), so a handler installed in that window
  runs twice. A carried signal that the old image blocked ends the new image at once (POSIX keeps it pending until
  the new image unblocks it), as INV-0106-4 already has it for a blocked default-action signal sent after the
  `exec`. A read with data ready at the moment of the entry delivery still returns the data (the flag acts only at a
  wait). `nanosleep` is `EINTR` where Linux would sleep: a handler that ran before the sleep began, `SA_RESTART`
  included, even for a zero sleep; a program with a periodic handler (make's `SIGCHLD`) sees more `EINTR`s from
  `usleep` loops.
- **Cost:** About 120 lines in `libposix` and host tests. Per `exec` that reaches its spawn: one
  `process_set_disposition` per held signal (about 25 for a handler-less process, about 25 µs under HVF and 0.3 ms
  under TCG at INV-0006-3's per-call figure `[estimate]`), and at least one `process_take_request`, in every process.
  This extends INV-0106-2's list of the functions that may call `process_take_request` in a handler-less process by
  `execve` (and so `execv`, `execvp`, `execl`), as ADR-0315 item 10 extended it by `sigwait`.
- **Precedent:** Linux keeps a pending signal across `execve` (the same task) and makes an interruptible sleep that
  a signal preceded return `-EINTR` `[outside evidence]`.

### Option A': as A without the hold (item 1)

- **Description:** Carry and entry only; leave the handler-less `exec` window to a kernel change: the kernel's spawn
  refuses, or ends, a process made by a caller whose exit is requested (Linux's `fatal_signal_pending` check in
  `fork` `[outside evidence]`).
- **Evidence for:** No cost for a handler-less `exec`; INV-0106-2 unchanged.
- **Evidence against:** Needs a kernel decision on ADR-0004 rule 3 before the gap closes; until then a Ctrl-C during
  the `exec` of a program without a handler is lost.
- **Cost:** About 40 lines less than A in the library; a kernel change later.
- **Precedent:** Linux's `fork`.

### Option B: spawn the new image suspended, carry, then resume it (Linux's atomic `execve`)

- **Description:** As A (2), with the new image made suspended so that nothing of it runs before the carry, then
  resumed.
- **Evidence for:** No duplicate: the new image has only its start dispositions while the carry runs.
- **Evidence against:** Without the hold, if the old image is ended by a terminating request during its spawn
  (ADR-0004 rule 3's "created after"), it never resumes the new image, which then stays suspended in the job for good.
- **Cost:** One more call per `exec`; a new hang to reason about.
- **Precedent:** `posix_spawn` with `POSIX_SPAWN_TCSETPGRP` already spawns suspended (ADR-0104 item 5).

### Option C: a kernel `exec` that keeps the process and its queue

- **Description:** A kernel call replaces the image of a process in place, keeping its handles, dispositions and
  request queue, as Linux does.
- **Evidence for:** Exact POSIX semantics for pending signals and the pid.
- **Evidence against:** A new kernel facility against ADR-0002 rule 3 and ADR-0104 item 6, for a hole the library
  can close.
- **Cost:** A kernel call, an ADR of its own.
- **Precedent:** Linux `execve` `[outside evidence]`.

### Option D: for the entry, deliver the handler and fail the call with `EINTR` whatever it would do

- **Description:** A call whose entry poll ran a handler without `SA_RESTART` returns `EINTR` at once.
- **Evidence for:** Simplest.
- **Evidence against:** A read of a regular file or of a pipe with data would fail where Linux never fails it;
  programs do not retry reads of regular files.
- **Cost:** The same lines as A (3).
- **Precedent:** none.

## Recommendation

**Recommendation (not a decision): Option A.** It closes the three holes of `exec` and of a call's entry inside the
library, gives no `EINTR` that a wait could not already give, and does not wait for a kernel decision. The cost of the
hold is a few dozen calls per `exec`, small against loading a program; if it is measured to matter, Option A' is the
fallback, with the kernel route.

## Consequences

- **Positive.** A Ctrl-C reaches a job during its `exec` (with or without a handler in the old image) and at the entry
  of its read: `zosh.script`'s Ctrl-C steps fail when the job runs on (`ended-` lines; a sleep of 10 s and the run's
  limit as the bounds), and `t_posix entryintr` forces the entry case.
- **Negative.** The duplicate and the blocked-signal rule of Option A's "evidence against"; `nanosleep`'s extra
  `EINTR`s; a stop signal is not carried (the broker stops by its own record, ADR-0106 item 9); a request to the old
  image after the carry is the new image's already.
- **Follow-up (kernel).** `fork`: a child made during a job signal's walk misses it (ADR-0004 rule 3), and the library
  cannot hand it on, since the child must not inherit the parent's pending signals. Closing it needs the kernel
  (Linux restarts `fork` when a group signal comes during it `[outside evidence]`), a question for ADR-0004's owner.
  `zosh.script` has no step with a forked command for this reason.

## Invariants

- **INV-0392-1.** `execve` hands on to the new image every signal its old image was sent and did not act on (the
  kernel's queue, and the pending sets with blocked signals included) whose default action ends a process and which
  the new image does not ignore; a signal sent while the new image is made is held for that, not acted on by the old
  image; the queue is taken only by a process with a handler, a `sigwait` or a hold. *Check:* `cargo test -p
  zoraya-posix --test threads` (`an_exec_*`) and `make test-posix` (`zosh.script`, the `go-a` and `go-c` steps).
- **INV-0392-2.** A call that may wait, whose entry poll ran a handler without `SA_RESTART`, is `EINTR` at its first
  wait point instead of waiting (with `SA_RESTART` handlers only it waits), a call that does not reach a wait returns
  its result, and `nanosleep` whose entry poll ran any handler is `EINTR` with its time left. *Check:* `cargo test -p
  zoraya-posix --test threads` (`a_handler_run_at_a_calls_entry_*`) and `make test-posix` (`t_posix entryintr`, and
  `zosh.script`'s `go-r` step).

## Open questions

- Whether `connect` and `getrandom`, which wait but are not in the list, should join it; neither is on a measured
  path with a handler.
- The kernel half for `fork` (the follow-up above).
- No test forces the handler-less `exec` window (the hold): it needs a signal to land during the spawn system call
  (`tools/mutants/sigint-exec.toml`, `S-exec-holds-nothing`, is a known survivor).

## Revisit triggers

- A program that installs a handler at once after `exec` and runs it for a signal sent before the `exec`, or runs it
  twice for one signal sent between the spawn and the carry.
- A measured cost of the hold on a workload that `exec`s often (Option A').
- Option D of ADR-0106 (a status word) or an asynchronous upcall, either of which removes the entry poll.

## Architect's decision


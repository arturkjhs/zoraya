# ADR-0392: Signals across `exec` and at the entry of a call that waits, where ADR-0106 is silent

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0004, ADR-0104, ADR-0106, ADR-0190, ADR-0315, ADR-0331
- **Evidence:** CP-10

## Context

In `sh` inside `zosh`, a Ctrl-C typed right after a job's first line was lost (`docs/backlog.md`, part 10c's leftovers):
`/bin/sh -c 'echo go; exec /bin/sleep 20'` ran its 20 s under HVF, and under TCG `/bin/sh -c 'echo go; read x'` never
ended. `zosh` had the job's `tcsetpgrp`, sent `SIGINT` to the job's group, and the broker's `job_signal(Interrupt)`
returned `Ok`. Two library paths lost the signal; both are places where ADR-0106 says nothing.

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

## Decision drivers

1. A signal a process was sent is acted on or stays pending; POSIX keeps a pending signal across `execve` and the
   kernel's guarantee for job signals (ADR-0004 rule 3) does not reach a process made during the walk.
2. CP-10 and ADR-0106: no asynchronous interruption; the fix must stay inside the library's wait points and entry
   poll, with no new kernel call and no cost for a process without handlers (INV-0106-2).
3. The `EINTR` paths of the library's waits exist and are used: a wait point already returns `EINTR` when a request
   comes during it (ADR-0106 item 6, ADR-0315), so a call that may wait already handles it.
4. dash, the shell every POSIX job of `zosh` runs under, must be interruptible at once (ADR-0331).

## Considered options

### Option A: carry the old image's signals to the new one, and let an entry delivery interrupt the call's wait

- **Description:** (1) `execve`, right after the spawn of the new image, takes the old image's whole request queue
  and its pending sets (`Posix::exec_carry`), keeps the signals whose default ends a process and that the new image
  does not ignore (`SIG_IGN` is carried, ADR-0190 item 18), and sends each to the new image with `process_signal`.
  From the spawn on, a job signal reaches the new image itself. (2) A call that may wait (`read`, `write`, `pread`,
  `pwrite`, `recv`, `send`, `recvmsg`, `sendmsg`, `waitpid`, `wait4`) whose entry poll ran a handler without
  `SA_RESTART` makes its first wait point `EINTR` instead of a wait (`waiting`, `Posix::interrupt_wait`); a call that
  does not wait returns its result as before. `nanosleep` whose entry poll ran any handler is `EINTR` with the
  whole time left (POSIX never restarts it).
- **Evidence for:** Drivers 1 to 4; the change is in the library; a handler-less process pays nothing (the entry poll
  is unchanged); every `EINTR` it gives is one the wait could already give.
- **Evidence against:** The new image gets a carried request a little after it starts: one that installs a handler
  at once may run that handler instead of dying of it. A carried signal that the old image blocked ends the new
  image at once, as INV-0106-4 already has it for a blocked default-action signal. A read with data ready at the
  moment of the entry delivery still returns the data (the flag acts only at a wait).
- **Cost:** About 60 lines in `libposix`, one `process_take_request` per `exec` (and one `process_signal` per carried
  signal), host tests.
- **Precedent:** Linux keeps a pending signal across `execve` (the same task) and makes an interruptible sleep that
  a signal preceded return `-EINTR` `[outside evidence]`.

### Option B: spawn the new image suspended, carry, then resume it (Linux's atomic `execve`)

- **Description:** As A (1), with the new image made suspended so that nothing of it runs before the carry, then
  resumed.
- **Evidence for:** No duplicate: the new image has only its start dispositions while the carry runs.
- **Evidence against:** If the old image is ended by a terminating request during its spawn (ADR-0004 rule 3's
  "created after"), it never resumes the new image, which then stays suspended in the job for good; today it runs.
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
- **Cost:** The same lines as A (2).
- **Precedent:** none.

## Recommendation

**Recommendation (not a decision): Option A.** It closes both holes inside the library, gives no `EINTR` that a wait
could not already give, and keeps the case Option B would hang running as it does today.

## Consequences

- **Positive.** A Ctrl-C reaches a job during its `exec` and at the entry of its read: `zosh.script`'s Ctrl-C steps
  now fail when the job runs on (`ended-` lines; a sleep of 10 s and the run's limit as the bounds).
- **Negative.** The duplicate of Option A's "evidence against"; a carried stop signal is not carried (the broker
  stops by its own record, ADR-0106 item 9); a request to the old image after the carry is the new image's already.
- **Follow-up.** The case of ADR-0004 rule 3 where the old image is *ended* (disposition `Terminate`) while the new one
  is being made: the new image runs on, unsignalled. It needs the kernel's spawn to refuse, or end, a process made by
  a caller whose exit is requested (Linux's `fatal_signal_pending` check in `fork`), which is a kernel decision.

## Invariants

- **INV-0392-1.** `execve` hands on to the new image every signal its old image was sent and did not act on (the
  kernel's queue and the pending sets) whose default action ends a process and which the new image does not ignore,
  and leaves the old image's queue empty. *Check:* `cargo test -p zoraya-posix --test threads` (`an_exec_hands_on_*`)
  and `make test-posix` (`zosh.script`, the `go-a` and `go-c` steps).
- **INV-0392-2.** A call that may wait, whose entry poll ran a handler without `SA_RESTART`, is `EINTR` at its first
  wait point instead of waiting, and a call that does not reach a wait returns its result. *Check:* `cargo test -p
  zoraya-posix --test threads` (`a_handler_run_at_a_calls_entry_*`) and `make test-posix` (`zosh.script`, the `go-r`
  step).

## Open questions

- Whether `connect` and `getrandom`, which wait but are not in the list, should join it; neither is on a measured
  path with a handler.
- The kernel half of the follow-up above (who decides: ADR-0004's owner).

## Revisit triggers

- A program that installs a handler at once after `exec` and runs it for a signal sent before the `exec`.
- Option D of ADR-0106 (a status word) or an asynchronous upcall, either of which removes the entry poll.

## Architect's decision


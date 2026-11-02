# ADR-0106: Signals in `libposix`

- **Status:** accepted
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0004, ADR-0005, ADR-0006, ADR-0034, ADR-0046, ADR-0101, ADR-0103, ADR-0104, ADR-0105
- **Evidence:** CP-10, X-5, DM-017, DM-027, DM-059, DM-130, stage-6/sources.md#other-programs-and-inputs, stage-6/sources.md#posix-semantics, task-02/callsite-analysis.md#feature-2-signals, task-02/conflict-impact.md#x-3-no-asynchronous-interruption-cp-10-vs-posix-signal-semantics, task-02/syscall-surface.csv, task-02b/callsite-analysis.md#4-cleanup-on-interruption, task-02b/conflict-impact.md#x-3-no-asynchronous-interruption-cp-10-vs-posix-signal-semantics, task-02b/event-timelines.md#w10-background-jobs-wait-concurrent-pipelines-run-w10-parallel-r1, task-02b/parallel-surface.md#interrupted-system-calls

## Context

ADR-0004 decided the native side: request kinds with kernel-held dispositions, `job_signal`, exit records with
`reason`, `detail` and, after ADR-0046, `request_kind`; and it sketched the personality mapping: handlers run at wait
points, `SIGCHLD` is a port subscription only while a handler is installed, `kill` goes through the broker, death by
signal is read from the exit record. ADR-0006 mapped `pselect`, `sigsuspend` and `wait4` onto the port. ADR-0046 left
two tables to this stage: the `Fault` exception class to signal, and the signal for `reason = Limit`. This ADR fixes
`libposix`'s signal implementation: what a wait point is, how a handler gets to run, `sigprocmask`, `EINTR`,
re-raising on oneself, and the `WTERMSIG` tables.

**What is measured.**

- In uninterrupted builds the only signal is `SIGCHLD`, used to end a wait early; under `-j` it ends 55 to 60 of 72 to
  100 `pselect6` calls per W8 build, each followed by `rt_sigreturn` returning `EINTR`
  (`task-02b/conflict-impact.md#x-3-no-asynchronous-interruption-cp-10-vs-posix-signal-semantics`).
- Interrupted system calls appear only where the program asked: `pselect6` with its mask and `sigsuspend`; plus
  blocking `wait4` calls that make's never-returning handler abandons; `SA_RESTART` restarted one `writev` in 4 of 9
  SIGTERM runs (`task-02b/parallel-surface.md#interrupted-system-calls`). No program saw `EINTR` as an error to retry.
- Make's `fatal_error_signal` does its whole shutdown inside the handler: reset, unlink the FIFO, forward `SIGTERM`
  to children, delete targets, a blocking `reap_children`, re-raise
  (`task-02b/callsite-analysis.md#4-cleanup-on-interruption`). dash's `waitproc` blocks all signals and `sigsuspend`s
  until `SIGCHLD` or a trapped signal; `onint` re-raises `SIGINT`; `waitforjob` raises `SIGINT` on the shell when the
  job died of it ([D1], `stage-6/sources.md#other-programs-and-inputs`).
- Handlers installed in T0 to T2: dash `SIGCHLD`, `SIGINT`, and in T2 `SIGHUP`, `SIGTERM`, `SIGPIPE` from `trap`;
  make `SIGCHLD` and the fatal set `HUP INT QUIT PIPE TERM USR1 XCPU XFSZ`; toybox `USR1`, `USR2`
  (`task-02/conflict-impact.md#x-3-no-asynchronous-interruption-cp-10-vs-posix-signal-semantics`). tcc installs none
  in the measured paths (`task-02/callsite-analysis.md#feature-2-signals`: fault capture only under `-run`).
- The only process killed by a signal in the core runs is W5's foreground `sleep` by Ctrl-C; `yes` dies of `SIGPIPE`
  in W10 (`task-02b/event-timelines.md#w10-background-jobs-wait-concurrent-pipelines-run-w10-parallel-r1`).
- Linux signal numbers and default actions on ARM ([P12], `stage-6/sources.md#posix-semantics`): `SIGHUP` 1, `SIGINT`
  2, `SIGQUIT` 3, `SIGILL` 4, `SIGTRAP` 5, `SIGABRT` 6, `SIGBUS` 7, `SIGFPE` 8, `SIGKILL` 9, `SIGUSR1` 10, `SIGSEGV` 11,
  `SIGUSR2` 12, `SIGPIPE` 13, `SIGALRM` 14, `SIGTERM` 15, `SIGCHLD` 17 (ignore), `SIGCONT` 18, `SIGSTOP` 19, `SIGTSTP`
  20, `SIGTTIN` 21, `SIGTTOU` 22 (stop), `SIGURG` 23 (ignore), `SIGXCPU` 24, `SIGXFSZ` 25, `SIGWINCH` 28 (ignore);
  `SIGKILL` and `SIGSTOP` cannot be caught, blocked or ignored; musl's headers use these numbers.
- A `Fault` exit record carries the exception class in `detail` (ADR-0034, INV-0034-1); the kernel reports a fault as
  `[fault: data abort, class 0x24]` (`docs/design.md`, "Using the shell").

**What is assumed.** The reaction latency of a compute-bound program with a handler is unmeasured (ADR-0004's open
question); the cost of a poll per library entry is arithmetic from INV-0006-3's 875 ns.

**Forces from TASK-01.** Signals as a notification mechanism and async-signal safety are the mistake (DM-017), `EINTR`
is its cost (DM-027), NT delivers console control on a fresh thread (DM-017), seL4 uses notifications (DM-017),
`signalfd` showed what it takes to make signals waitable (DM-130); CP-10 keeps asynchronous interruption out of the
native ABI and ADR-0004 chose wait-point delivery with Option D (a typed upcall) as its first revisit trigger.

## Decision drivers

1. **Every measured delivery is at a wait** (what is measured), so wait-point delivery covers the data; the open risk
   is a handler-installing program in a compute loop (ADR-0004's open question).
2. **Make's handler must run as written**: a blocking `wait4` and `unlink` inside it, then a re-raise that the parent
   sees as death by that signal (`task-02b/callsite-analysis.md#4-cleanup-on-interruption`).
3. **`EINTR` only where asked** (INV-0006-7, DM-027): `pselect`, `sigsuspend`, an abandoned `wait4`; `SA_RESTART`
   resumes.
4. **The hot path pays nothing**: tcc, which installs no handler, must see no signal cost on its 5,294 header opens
   (ADR-0002 driver 6).
5. **Default actions without the target's cooperation** (ADR-0004 driver 5): the kernel's disposition ends a
   handler-less process; stops and continues go through the broker (ADR-0105).
6. **One table from exit record to `WTERMSIG`**, with the two entries ADR-0046 left open.

## Considered options

### Option A: Asynchronous handlers (Linux signals, ADR-0004 Option D; NT's fresh-thread console handler as the non-Unix form)

- **Description:** A kernel upcall interrupts the thread wherever it is and runs the handler on its stack; or, as NT
  does for console control events, the kernel starts a fresh thread in the process to run it (DM-017).
- **Evidence for:** Exact fidelity, including compute loops and `tcc -run`'s fault capture; NT's form needs no
  async-signal-safety rules.
- **Evidence against:** Rejected by ADR-0004 for CP-10 and DM-017; no native facility exists; it would be a second
  personality-only facility (ADR-0002 rule 3(b)).
- **Cost:** The hardest kernel primitive in ADR-0004's words; NT's form is a thread injection ADR-0004 Option C
  weighed and rejected.
- **Precedent:** Unix; Starnix emulates it on Zircon and calls it a hard part; NT consoles (DM-017).

### Option B: Wait points only (ADR-0004 Option B read literally)

- **Description:** Requests are taken only inside `pselect`, `sigsuspend`, `wait*`, `nanosleep`, `pause` and blocking
  reads and writes; nowhere else.
- **Evidence for:** Zero cost outside waits; every measured delivery is covered (driver 1).
- **Evidence against:** A shell loop of builtins (`while :; do :; done`) never reaches a wait and cannot be
  interrupted by Ctrl-C; make between waits (parsing, directory scans) reacts late. dash runs its pending traps at
  the start and end of every command node (`dotrap()` in `evaltree`, [D3], `stage-6/sources.md#other-programs-and-inputs`)
  from state its handlers set; with no handler ever running between waits that state stays clear.
- **Cost:** Lowest; a visible fidelity gap in the shell.
- **Precedent:** ADR-0004's own fidelity note.

### Option C: Wait points plus a poll at library entry when a handler is installed

- **Description:** As B, and in addition every `libposix` function that makes a native call first polls for a pending
  request, by one `process_take_request`, *if* the process has a handler installed for any request-mapped signal.
  Processes without handlers, tcc among them, never poll.
- **Evidence for:** Drivers 1 to 4: dash and make poll on every library call they make, so a builtin loop that calls
  anything (and `:` calls nothing, but `echo`, `test -f`, `[`, assignments with command substitution all do) reacts
  within one call; tcc pays nothing. The poll is one system call of about 1 µs under HVF and 13 µs under TCG
  (INV-0006-3's `port_wait` figures as a proxy `[estimate]`): W4's roughly 190,000 system calls from shells and make
  `[estimate from the per-workload counts in task-02/syscall-surface.csv]` cost about 0.2 s under HVF and 2.5 s
  under TCG in polls.
- **Evidence against:** A pure compute loop with no library call still waits; the poll doubles the system calls of
  every shell and make operation; under TCG the cost is visible.
- **Cost:** *(estimate)* 50 lines in the library's entry path; the measured overhead above.
- **Precedent:** Cooperative delivery at safe points is how musl's own cancellation and many runtimes' signal handling
  work (DM-059's deferred cancellation at declared points); seL4 delivers asynchronous events as notifications a
  thread polls or waits on (DM-017), which is the non-Unix form of the same rule.

### Option D: A kernel-written status word in a process-readable page

- **Description:** The kernel maps a read-only page into every process with a word it updates when `REQUEST_PENDING`
  changes; the library reads the word at entry, no system call.
- **Evidence for:** Option C's coverage at the cost of a load, so the poll can run on every entry, handler or not.
- **Evidence against:** A new kernel facility and a new fixed mapping for every process, personality or not; a shared
  page the kernel writes is a channel to reason about (X-5's shape without the hostile side); nothing measured needs it
  yet.
- **Cost:** *(estimate)* A page per process, a mapping rule, an ADR of its own.
- **Precedent:** Linux's vDSO data page `[outside evidence]`.

### Considered also: who stops a process

Stops (`SIGSTOP`, `SIGTSTP`, `SIGTTIN`, `SIGTTOU`) and `SIGCONT` are not requests in the kernel: ADR-0105 has the
terminal and the broker call `job_suspend` and `job_resume`. This section is part of the option analysis, not a
separate decision: the alternative, a `Deliver` disposition taken at a wait point that then asks the broker to stop,
would let a compute-bound job ignore Ctrl-Z, and Linux stops are not deliverable to the target anyway.

## Recommendation

**Recommendation (not a decision): Option C**, with Option D as the recorded upgrade if the poll's cost is measured to
matter.

**Dispositions.**

1. **Mapping.** `SIGINT`, `SIGTERM`, `SIGHUP`, `SIGQUIT` are the kinds `Interrupt`, `Terminate`, `Hangup`, `Quit`;
   every other signal number `n` in 1 to 31 is `Custom(n)`. `sigaction(SIG_DFL)` sets the kernel disposition
   `Terminate` for signals whose default action terminates (1 to 16 except 9, and 24 to 27, 29 to 31) and `Ignore`
   for those whose default is to ignore (17, 23, 28) or to stop or continue (18 to 22: the broker acts on the
   *target's* POSIX disposition, which `sigaction` for 18, 20, 21 and 22 registers with it, ADR-0104 item 8a; item
   9). `SIG_IGN` sets `Ignore`; a handler sets `Deliver`. `SIGKILL` and `SIGSTOP` refuse `sigaction` with `EINVAL`.
   At startup `libposix` sets every slot from this table except those the bootstrap record marks as ignored on entry
   (ADR-0107), which stay `Ignore` and which `sigaction` reports as `SIG_IGN` (make's and pdpmake's inherited ignores).
2. **Blocking.** `sigprocmask` and the mask argument of `pselect` and `sigsuspend` are library state. A request taken
   while its signal is blocked goes into the library's pending set (one bit per signal; standard signals do not
   queue) and is delivered when unblocked. The kernel's disposition does not change on block: a blocked signal whose
   POSIX disposition is `SIG_DFL` and whose default action terminates still ends the process at once, a stated
   deviation. The alternative, flipping the kernel disposition to `Deliver` for the duration, would cost a
   `process_set_disposition` per terminating signal per block and unblock: dash blocks every signal around each
   `sigsuspend` in `waitproc` and around `tcsetpgrp` ([D1]), so about 80 system calls per wait, times the 3,751 child
   waits of the core runs, seconds under TCG. No measured program blocks a default-action signal and receives it
   while blocked: make's blocked `SIGCHLD` has a handler, and dash's momentary blocks are around waits it leaves at
   once. ADR-0004's mapping says "the kernel queue holds it"; here the library's set does (tension below).

**Delivery.**

3. **Wait points** are every `port_wait` the library makes: `pselect`, `select`, `poll`, `sigsuspend`, `pause`,
   `wait*`, `nanosleep`, `sleep`, and the retries of blocking `read` and `write` on streams and terminal connections
   (ADR-0101, ADR-0105). At each, the process's own handle (slot `SELF`, ADR-0107 item 5: given by the spawner over
   the bootstrap channel, and by `snapshot_process` itself for a `fork` child, ADR-0102 item 3) is subscribed for
   `REQUEST_PENDING` (Level) when any request-mapped signal has a handler and is unblocked. `pselect` on a `File` or
   `Dir` fd reports it ready at once: regular files are `NotWaitable` (ADR-0006 rule 1) and a file connection's
   `READABLE` means a reply, not data; only stream ends and terminal connections are subscribed. `SIGCHLD`'s sources are the children's *process job* handles
   (ADR-0104), subscribed for `TERMINATED` only while a `SIGCHLD` handler is installed and unblocked (INV-0004-8), and
   for `SUSPENDED` too under `SA_NOCLDSTOP` clear.
4. **Taking a request.** On `REQUEST_PENDING` the library calls `process_take_request` until `NotFound`, and for each
   `(kind, detail)`: the signal number is `detail` if it is a valid number for that kind, else the kind's own
   (ADR-0046's rule); blocked → pending set; `Deliver`d with a handler → run it (item 6); otherwise the default action
   (item 7). A `TERMINATED` packet from a child's job with a `SIGCHLD` handler runs that handler.
5. **The poll.** Every exported function that makes a native call begins with `process_take_request` when
   `handlers_installed` is set (any request-mapped signal with a handler); a request found is handled as item 4 before
   the function's own work. `kill`, `sigaction`, `sigprocmask`, `sigsuspend`, `pause`, `raise` and `abort` poll
   always. Pure functions (`strlen`, `printf` to a buffer) never do.
6. **Running a handler.** On the calling thread, at the wait point or the entry, with the signal added to the mask
   for its duration (`SA_NODEFER` clears that), `siginfo_t` filled with `si_signo`, `si_code` (`SI_USER` for a
   request from `kill`, `CLD_EXITED` or `CLD_KILLED` for `SIGCHLD`), and the sender's pid when the broker can
   supply it. A handler may call anything: a nested `wait4` is a nested `port_wait`, as make's `reap_children`
   needs (driver 2). After the handler: `EINTR` from `pselect`, `select`, `poll`, `sigsuspend`, `pause` and
   `nanosleep` (the latter with the remaining time); a blocking `read`, `write` or `wait*` returns `EINTR` without
   `SA_RESTART` and resumes with it (INV-0006-7, [P12]'s rule). A handler that never returns (make's) leaves the
   original wait abandoned, which is what the timelines show.
7. **Default actions.** `Terminate` kinds never reach the library: the kernel ended the process (ADR-0004 rule 2).
   For a request the library holds (blocked then unblocked, or taken with `SIG_DFL` after a disposition change race):
   terminate by `process_exit_with_reason(Terminated, n)`; ignore by dropping it; stop and continue by asking the
   broker (ADR-0105 item 8).
8. **On oneself.** `raise(n)` and `kill(getpid(), n)` never go to the kernel or the broker: a handler runs at once,
   `SIG_IGN` drops it, `SIG_DFL` is `process_exit_with_reason(Terminated, n)` (make's re-raise), or for stop signals
   the broker's `stop_self`. `abort()` is `SIGABRT` this way. `SIGPIPE`: on `PeerClosed` from `stream_write`
   (ADR-0101), `SIG_DFL` → `process_exit_with_reason(Terminated, 13)`; otherwise `EPIPE`, after a handler if one is
   installed (W10's `yes`).
9. **`kill(pid, n)`** for another process: the broker (ADR-0104 item 9), with `sig` 9 as `job_kill`; 18 to 22 by the
   target's disposition the broker holds (`job_suspend` or `job_resume`, `Custom(n)` to a handler, nothing when
   ignored); and every other number as `job_signal(kind(n), n)`. The stop signals' POSIX dispositions reach the
   broker from `sigaction` (`set_stop_disposition`, ADR-0104 item 8a), the one `sigaction` that costs a round trip.
10. **Timers.** `nanosleep`, `sleep` and `pselect`'s timeout are the `Deadline` of the `port_wait`; no timer object is
    needed at stage 6 (`alarm` and `setitimer` are excluded, ADR-0005). `DeadlineExceeded` ends the wait normally.

**Exit records to `wait` status.**

11. **`WIFSIGNALED`** iff `reason ∈ {Terminated, Killed, Fault, Limit}` (INV-0004-2). **`WTERMSIG`**: `Terminated`:
    `detail` if 1 ≤ `detail` ≤ 31, else by `request_kind` (`Interrupt` 2, `Terminate` 15, `Hangup` 1, `Quit` 3,
    `Custom(n)` n), else 15 (ADR-0046's rule). `Killed`: 9. `Limit`: 9 `[assumption, ADR-0046's candidate: the
    nearest Linux analogue is the OOM killer's SIGKILL]`. `Fault`: by the exception class in `detail`
    (ADR-0034): instruction abort (0x20, 0x21) and data abort (0x24, 0x25) → 11 `SIGSEGV`; PC alignment (0x22) and
    SP alignment (0x26) → 7 `SIGBUS`; unknown reason (0x00), trapped system instruction (0x18) and illegal execution
    state (0x0E) → 4 `SIGILL`; trapped floating point (0x2C) → 8 `SIGFPE`; `BRK` (0x3C) → 5 `SIGTRAP`; any other
    class → 11. The class alone cannot tell an alignment fault from a translation fault inside a data abort, so both
    are `SIGSEGV` (a documented difference from Linux, which gives `SIGBUS` for alignment). **`WEXITSTATUS`**: `code`
    when `reason = Exited`. **`WIFSTOPPED`**: the job's `SUSPENDED` (ADR-0105), `WSTOPSIG` 20 or 21 as the broker
    recorded. **`WCOREDUMP`**: never.
12. **No fault capture.** No exception channel is bound to a POSIX job at stage 6 (ADR-0005 excludes `tcc -run` and
    fault handlers): a faulting process ends with reason `Fault` and item 11 reports it. INV-0004-6 stays unchecked
    until some stage binds one.

**Errors.** `process_signal`'s `LimitReached` (a full request queue, ADR-0046) → `EAGAIN` from `kill`;
`QuotaExceeded`, when ADR-0056 exists, → `ENOMEM` wherever the call allocated.

**Tensions with accepted ADRs.**

- *ADR-0004*'s mapping has a blocked signal held in the kernel queue; item 2 takes it into the library's pending set
  and does not flip `Terminate` dispositions, with the stated deviation.
- *ADR-0004*: "a delivered request is taken at a wait point or at `libposix` entry" is made precise by item 5; its
  mapping of `SIGCHLD` to "child process handles" becomes process job handles (ADR-0104). The stop signals as
  `Custom(n)` with kernel default `Ignore` extend ADR-0004's startup table, which named only terminate and ignore
  defaults.
- *ADR-0046*: the two open tables are filled (item 11), with `Limit → SIGKILL` as it suggested.
- *ADR-0006*'s "timers are objects": none is built for `nanosleep`; the port's deadline serves. Its personality text
  says "a timer object subscribed to the port"; this is a simplification, not a contradiction.
- *ADR-0002 INV-0002-3*: the poll is a kernel call, not broker IPC; the invariant holds.

### Effect of the vision

For target 2 nothing here crosses a trust boundary: the poll, the pending set and the tables are inside the sandboxed
process; what the kernel enforces (`Terminate` dispositions, `job_kill`) is unchanged. For target 3 a desktop program
with a handler and a compute loop would want Option D or ADR-0004's Option D; the ABI does not change for it. Threads:
`sigprocmask` becomes per thread and the pending set per process, as POSIX says; the poll stays per call. Networking:
`SIGPIPE` on sockets follows item 8.

### Deferred to a later stage

Option D's status page; exception channels and fault capture; `SIGALRM` and timers as objects; `siginfo` fields beyond
the three named.

## Consequences

- **Positive.** Every measured scenario of ADR-0006's walkthrough holds, make's handler runs unpatched, dash's
  per-command `dotrap` sees a pending trap after any command that makes a library call, and tcc pays nothing.
- **Negative.** Shells and make make twice the system calls while a handler is installed. A pure compute loop still
  reacts only at its next library call. Alignment faults report `SIGSEGV`.
- **Follow-up.** The disposition and `WTERMSIG` tables in `zoraya-posix` with host tests; the poll's measured cost on
  W4 (ADR-0110); the `ignored-on-entry` bits in the bootstrap record (ADR-0107).

## Invariants

- **INV-0106-1.** `WIFSIGNALED` is derived from `reason` only, and `WTERMSIG` from item 11's tables: a child that
  calls `process_exit_with_reason(Terminated, 2)` reports `SIGINT`, one that faults on an unmapped address reports
  `SIGSEGV`, one killed by `job_kill` reports `SIGKILL`, and one that calls `exit(130)` reports none. *Check:* an
  in-QEMU `libposix` test with those four children (INV-0004-2's test extended).
- **INV-0106-2.** A process with no handler installed makes no `process_take_request` call outside `kill`,
  `sigaction`, `sigprocmask`, `sigsuspend`, `pause`, `raise` and `abort`. *Check:* a host test of `zoraya-posix`
  under a counting system-call shim, running a file workload with and without a handler.
- **INV-0106-3.** `pselect`, `sigsuspend`, `pause` and `nanosleep` return `EINTR` only after a handler ran; a blocking
  `read` interrupted by a handler with `SA_RESTART` resumes and completes. *Check:* `libposix` unit tests with each
  disposition (INV-0006-7's test) and an in-QEMU test of the `SA_RESTART` read.
- **INV-0106-4.** A blocked signal whose POSIX disposition is a handler or `SIG_IGN` is never lost and never acted on
  while blocked: a request taken while blocked is delivered on unblock, as the handler, or dropped if ignored. A
  blocked signal whose POSIX disposition is `SIG_DFL` and whose default action terminates is outside this rule: it
  ends the process at once, as item 2 states. Stop, continue and default-ignore signals never reach the library as
  requests under `SIG_DFL` (items 1 and 7) and are outside it too. *Check:* in-QEMU test blocking `SIGTERM` with a
  handler, receiving a `kill`, unblocking, and observing the handler run exactly once; the same with `SIG_IGN`,
  observing nothing; and with `SIG_DFL`, observing the exit record `Terminated` with `detail` 15 while the signal is
  still blocked.
- **INV-0106-5.** `kill(getpid(), n)` and `raise(n)` with `SIG_DFL` end the process with `ExitRecord { reason:
  Terminated, detail: n }` with no broker message. *Check:* in-QEMU test under the counting broker.

## Open questions

- The poll's cost on W1 to W4 (ADR-0110), and the threshold at which Option D is built.
- Reaction latency in a compute loop: a synthetic test (ADR-0004's open question), for the record.
- Whether `SI_USER` with a sender pid is worth a `detail` convention from the broker (`detail` is 32 bits: a pid fits,
  but then `WTERMSIG`'s "valid signal number" test must change). Not for stage 6.

## Revisit triggers

- A measured program in scope needing a handler to run inside a compute loop: Option D, or ADR-0004's revisit.
- The poll measured above a few percent of W4's run time: Option D.
- A program needing `SIGBUS` for alignment: `detail` would need the fault status, a change to ADR-0034.
- A program of the T1 or T2 workloads blocks a signal whose default action terminates and receives it while blocked:
  item 2's deviation then costs a wrong termination, and the disposition flip it rejected is built for that signal.

## Architect's decision

Accepted as recommended, 2026-11-02, with the architect's six changes already in the text of ADR-0100 to ADR-0110: the 10 % threshold in ADR-0102, `KILL` for `job_suspend` and `job_resume`, `make test-posix-long`, `make fetch-ports`, `aarch64-unknown-zoraya` with the T2 cost of `config.sub`, INV-0004-6 until exception channels, and INV-0106-4. Decided by the architect; recorded by an agent at the architect's request.

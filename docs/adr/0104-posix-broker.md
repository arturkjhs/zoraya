# ADR-0104: The POSIX broker

- **Status:** accepted
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0003, ADR-0004, ADR-0006, ADR-0040, ADR-0041, ADR-0046, ADR-0050, ADR-0052, ADR-0053, ADR-0072, ADR-0101, ADR-0102, ADR-0103
- **Evidence:** CP-01, CP-06, CP-07, CP-15, DM-003, DM-005, DM-070, DM-100, alternatives-postmortem.md#exokernel-mit-aegis--xok, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#minix-3, stage-6/sources.md#other-programs-and-inputs, stage-6/sources.md#posix-semantics, task-02/callsite-analysis.md#feature-4-tty-and-job-control, task-02/callsite-analysis.md#m4-validation-patches, task-02/conflict-impact.md#x-3-no-asynchronous-interruption-cp-10-vs-posix-signal-semantics, task-02/method.md#tiers-runs-and-sets, task-02/posix-surface.md#linux-syscall-surface-m2, task-02b/event-timelines.md#w9a-ctrl-c-run-w9-int-d15-r1, task-02b/parallel-surface.md#new-syscalls-and-argument-patterns

## Context

ADR-0002 puts pids, process groups, sessions and the controlling terminal in a per-session broker, on the control path
only. ADR-0003 rule 5 makes `exec` a `spawn` plus an exit "with the broker moving the pid". ADR-0004 makes process
groups sub-jobs, has the broker create every process it spawns in a sub-job of its own, refuses a `setpgid` that would
move a process out of a job it shares, and routes `kill` through the broker. ADR-0053 leaves the broker to be "started
as services are". This ADR decides what the broker holds, its protocol, how a pid survives `exec`, how `waitpid`
finds the right process, and what the broker may not know.

**What is measured.**

- Control-path calls are rare: 15 `TIOCSPGRP`, 1 `TIOCGPGRP`, `setpgid` 4 and `getpgid` 1 in W9, `kill` 7 to 17 per
  interrupted build (`task-02/posix-surface.md#linux-syscall-surface-m2`,
  `task-02b/parallel-surface.md#new-syscalls-and-argument-patterns`), and `kill(getpid(), SIGINT)` as each make's
  re-raise (`task-02b/event-timelines.md#w9a-ctrl-c-run-w9-int-d15-r1`). No T1 program but the shell uses process groups
  (`task-02/callsite-analysis.md#feature-4-tty-and-job-control`).
- Waits are frequent: `wait4` 4,030 blocking and 3,307 `WNOHANG` in the core runs, about 226 per W8 `-j4` build
  (`task-02/posix-surface.md#linux-syscall-surface-m2`, `task-02b/parallel-surface.md#new-syscalls-and-argument-patterns`);
  3,751 `SIGCHLD` deliveries (`task-02/conflict-impact.md#x-3-no-asynchronous-interruption-cp-10-vs-posix-signal-semantics`).
- `execve` succeeded 3,002 times; `/bin/sh` is run 143 times by make for recipes, and the shell `exec`s external
  commands through `vfork` plus `execve` on the non-interactive path, and keeps `vfork` on the interactive one because
  `tcsetpgrp` in the child has no `posix_spawn` attribute (`task-02/callsite-analysis.md#m4-validation-patches`).
- dash's job control: `setjobctl` loops on `tcgetpgrp`, calls `setpgid(0, pgrp)` and `xtcsetpgrp`; `forkchild` and
  `forkparent` both call `setpgid` with the job's first pid, which "can fail because we are doing it in the child
  also"; a pipeline's stages share one group ([D1], `stage-6/sources.md#other-programs-and-inputs`).
- The exit status is an interface: every make re-raises its signal so that its parent sees "killed by SIGINT", and
  dash abandons the line on that (`task-02b/event-timelines.md#w9a-ctrl-c-run-w9-int-d15-r1`).
- POSIX: a pid is unchanged across `exec` ([P7]); `waitpid` takes a pid, -1, 0 or a negative group ([P6]); `kill`
  takes the same forms ([P13]); `setpgid` fails after a child's `exec` and for a group in another session ([P5]);
  `setsid` makes a session with no controlling terminal ([P14]) (`stage-6/sources.md#posix-semantics`).

**What is assumed.** Round trips to the broker are unmeasured. The pid counter's rate is arithmetic.

**Forces from TASK-01.** A pid is a reusable integer name whose reuse races needed `pidfd` (DM-003); processes should
be objects named by handles with the exit status on the object (CP-06); zombies and reparenting are DM-005; process
groups and sessions are a two-level hierarchy for one terminal (DM-070); containment should be inescapable (CP-07,
DM-100); ExOS left the process map in unprotected shared memory and Starnix keeps pid allocation in its runner
(`alternatives-postmortem.md#exokernel-mit-aegis--xok`, `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`);
MINIX 3's process manager is a server that could not be restarted (`alternatives-postmortem.md#minix-3`).

## Decision drivers

1. **Shared POSIX process state lives in one protected place** (ADR-0002 driver 4, CP-15).
2. **`exec` keeps the pid and the parent's wait keeps working** (ADR-0003 rule 5, [P7]): what a parent waits on must
   survive the switch from the old process object to the new.
3. **A pid never names the wrong process** (DM-003): the broker holds the process handle (CP-06) and never reuses a
   pid while anything in the session could name it.
4. **Process groups are jobs** (ADR-0004), so `kill(-pgid)` and the terminal's interrupt are one `job_signal`.
5. **The broker is off the data path** (INV-0002-3) and off the hot wait path as far as possible: 3,751 child exits
   and 7,000 waits must not each cost a round trip if a cheaper correct form exists.
6. **The broker learns of everything it must know from the processes it serves**, by protocol, since the kernel has
   no pid table and no query for "which job holds this process".

## Considered options

### Option A: Pids, groups and sessions in the kernel (Linux)

- **Description:** The kernel assigns pids, keeps the parent link, process groups and sessions, implements `kill`,
  `waitpid` and `setpgid`, and reparents orphans to pid 1.
- **Evidence for:** Exactly POSIX; `exec` keeps the pid trivially because the task is the same object; no round trips.
- **Evidence against:** ADR-0002 rule 3 forbids a kernel pid table (CP-06); it is DM-003 and DM-070 in the trusted
  base; `kill` by integer is ambient authority (CP-01).
- **Cost:** Permanent kernel complexity; rejected by an accepted ADR.
- **Precedent:** Unix.

### Option B: Pid table in shared memory among the session's processes (ExOS)

- **Description:** Every POSIX process maps one table; `fork` and `exit` update it; `kill` looks up a handle there.
- **Evidence for:** No server, no round trips.
- **Evidence against:** The table is writable by every process in it, so one bug or one hostile process corrupts
  everyone's view (`alternatives-postmortem.md#exokernel-mit-aegis--xok`, CP-15); handles cannot live in shared memory
  anyway (ADR-0050), so `kill` would still need a holder of the process handle.
- **Cost:** *(estimate)* Small code, no isolation.
- **Precedent:** ExOS (research; the shared-state fix was never built).

### Option C: A per-session broker, a job per process, pids tied to jobs (MINIX PM shape with job objects)

- **Description:** One broker per session, an ordinary program started by `init`'s session path. Every POSIX process
  runs in a *process job* of its own, inside its *group job*, inside the *session job*. A pid names a process job. The
  broker holds, per pid: the process job handle, the current process handle, the parent pid, the group. `exec` spawns
  the new image into the caller's own process job and tells the broker; the parent waits on the process job's
  `TERMINATED`, which ADR-0046 asserts only when no live process is left in it, so the switch is invisible; the exit
  record comes from the broker. `kill(pid)` is `job_signal` on the process job, `kill(-pgid)` on the group job.
- **Evidence for:** Drivers 1 to 6. The exec problem is solved by an existing kernel semantic (job `TERMINATED`,
  ADR-0046) rather than by a shim process or a handle swap in the parent. A process job holds exactly one live process
  except during the `exec` handover, so `job_signal` on it is `kill(pid)`. Groups as jobs are already decided.
  Waits stay on the port (ADR-0006), with one `reap` round trip per child exit, not per wait: 3,751 against 7,000.
- **Evidence against:** Three job levels per POSIX process: a `job_create` per spawn and per fork, and the depth of
  32 (ADR-0046) bounds nesting of sessions, which does not arise at stage 6. Two round trips per spawn (`prepare`,
  `commit`) and one per reap. The broker trusts what a process tells it about handles (driver 6); at stage 6 every
  process is trusted (ADR-0056 not built), and the limitation is stated. The broker is a session-wide component whose
  death ends the session's POSIX state, as MINIX 3's PM could not be restarted either
  (`alternatives-postmortem.md#minix-3`).
- **Cost:** *(estimate)* 1,500 lines for the broker and its protocol crate; a `job_create` per POSIX process.
- **Precedent:** MINIX 3 PM (a server owning the process table), Starnix's runner (pid table in user space),
  QNX's `procnto` `[outside evidence: QNX documentation, not fetched]`; jobs as the containment unit are Zircon's
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option D: A full personality server on the wait and signal paths (Starnix)

- **Description:** As C, but `waitpid`, `sigsuspend`, `pselect` and `kill` all go through the broker, which holds every
  process handle and does the waiting for its clients.
- **Evidence for:** One place implements every POSIX process semantic; clients hold no process handles at all.
- **Evidence against:** Every one of the 7,000 waits and 3,751 exits becomes a round trip, on the path ADR-0006 built
  the port for; the Starnix RFC's own performance warning (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
  ADR-0006's `pselect` mapping has the library wait on its own port.
- **Cost:** *(estimate)* The broker becomes a bottleneck and a larger trusted component (ADR-0002's revisit trigger).
- **Precedent:** Starnix (performance work ongoing).

## Recommendation

**Recommendation (not a decision): Option C.**

**Structure.**

1. **Jobs.** `init` creates the broker's job with the `posix` flag (ADR-0046). In it the broker creates two child
   jobs: the temporary directory service's job (ADR-0107) and the *session job*, which holds only POSIX processes, so
   that a signal to the whole session (`kill(-1)`, the terminal's hangup) never reaches the broker or `tmpd`, whose
   dispositions are the default `Terminate`. In the session job the broker creates one *group job* per process
   group; in a group job, one *process job* per POSIX process. All inherit the bit. A pid names a process job for its
   whole life.
2. **The pid table.** Per pid: the process job handle with every right a job can carry (`DUPLICATE | TRANSFER |
   WAIT | SIGNAL | CREATE_PROCESS | KILL`), from which the broker makes the narrower duplicates below; the current
   process handle (`WAIT | SIGNAL`); parent pid; group pid; the POSIX disposition of the stop and continue signals
   (item 8a); state (running, stopped, zombie, reaped). Pids are a 31-bit counter starting at 2,
   never reused within a session: at 1,000 spawns a second a session exhausts it in about 25 days `[arithmetic]`, and
   the broker then refuses `spawn_prepare` with `EAGAIN`. Pid 1 is the broker itself as `getppid` of the shell and
   as the reparenting target. The session id is the shell's pid.
3. **Identity.** `getuid`, `geteuid`, `getgid`, `getegid` return fixed values from the bootstrap record (ADR-0107),
   1000 `[assumption]`, never 0 (ADR-0002 rule 6); `getlogin` a fixed name. No broker call.

**Protocol** (`zoraya-proto-posix`, declared with ADR-0052; one connection per POSIX process, created by the broker
for each child before it exists and moved to the child at slot `BROKER`):

4. `spawn_prepare(group: Inherit | New | Join(pid)) → {pid, pgid, parent_job (moved), child_job (moved),
   child_broker_end (moved)}`. The broker creates the process job in the caller's group job, a new group job, or
   `pid`'s group job (which must be in the session, else `EPERM`), and a channel whose server end it keeps. It
   returns two duplicates of the process job: `parent_job` with `WAIT | DUPLICATE | TRANSFER`, which the parent keeps
   in its child list for `waitpid` (item 11), and `child_job` with `CREATE_PROCESS | TRANSFER`, which the parent uses
   as the `job` of `spawn` or `snapshot_process` and moves into the child at slot `PROCESS_JOB` (ADR-0107 item 5) for
   the child's own `exec` (item 6). The caller moves `child_broker_end` into the child at slot `BROKER`. `libposix`'s
   `posix_spawn` and `fork` call this. The group hint comes from the caller: `posix_spawn` with
   `POSIX_SPAWN_SETPGROUP` passes `New` for `pgroup` 0 or `Join(pgroup)`; `fork` passes what the Zoraya-only
   function `zoraya_fork_group(Inherit | New | Join(pid))` set before the call, else `Inherit`. The dash patch of
   ADR-0109 calls it in `forkshell` with the group dash will `setpgid` to (dash's `forkchild` and `forkparent` then
   call `setpgid`, which item 8 confirms as a no-op).
5. `spawn_commit(pid, process (moved, WAIT | SIGNAL))` after a successful `spawn` or `snapshot_process`; the broker
   subscribes nothing, it keeps the handle for the record. `spawn_abort(pid)` after a failure frees the pid, its job
   and the channel. For an interactive foreground job (ADR-0003 rule 2's `vforkexec` path, ADR-0109 item 9) the
   parent spawns with `START_SUSPENDED`, calls `tcsetpgrp` (item 10) for the child's group, commits, and then
   `process_resume`s the child, so the child cannot read the terminal before its group is foreground; `posix_spawn`
   does this when the Zoraya-only attribute flag `POSIX_SPAWN_TCSETPGRP` is set with `SETPGROUP`.
6. `exec_commit(process (moved))` from the process that `exec`s: the broker replaces its current handle. The caller
   has already spawned the new image into its own process job (it holds that job's handle from startup, ADR-0107),
   with the attribute block and the bootstrap record's `ignored` set built from its *POSIX* dispositions (a signal
   whose POSIX disposition is `SIG_IGN` stays `Ignore` in both; every other request-mapped signal is `Terminate` in
   the block, as `exec` resets caught signals, [P7]; the stop and continue signals, whose kernel disposition is
   `Ignore` whatever POSIX says, are reported by their POSIX disposition, ADR-0106 item 1) and the fds of ADR-0103
   item 7, and exits right after the reply.
7. `reap(pid) → ExitRecord`, allowed to the parent only, after the process job's `TERMINATED`: the broker reads the
   record from the current process handle, marks the pid reaped, closes its handles and the group job if empty. A
   parent that exits with unreaped children: the broker reparents them to pid 1 and reaps them itself when they end,
   discarding the records (DM-005's `init` role).
8. `setpgid(pid, pgid)`: `pid` must be the caller or a child of the caller that has not called `exec_commit`
   (`EACCES` after, [P5]); `pgid` 0 means `pid`. If the process's group pid already equals `pgid`, the call succeeds
   and changes nothing (this is how dash's `setjobctl`, `forkchild` and `forkparent` calls all succeed after a `New`
   or `Join` prepare); anything else would move a process between jobs and is `EPERM` (CP-07; ADR-0004's rule).
   `getpgid`, `getppid`, `getsid` are lookups. `setsid` is not in the T1 surface and is not offered.
8a. `set_stop_disposition(sig, Default | Ignore | Handler)` for `SIGTSTP`, `SIGTTIN`, `SIGTTOU` and `SIGCONT`:
   `libposix` sends it from `sigaction` on those four, so that the broker, which executes stops and continues (item
   9), knows what the *target* wants. `SIGSTOP`'s is always `Default`. A new pid's record starts at `spawn_prepare`
   as the caller's record for `fork` (dispositions are kept across `fork`, as `snapshot_process` keeps the kernel's,
   ADR-0004 rule 6) and as `Default` for `posix_spawn`, with `Ignore` where the Zoraya spawn attribute block of item
   6 says so; `exec_commit` resets `Handler` to `Default` and keeps `Ignore`, as `exec` does ([P7]).
9. `kill(pid | 0 | -1 | -pgid, sig, detail)`: `job_signal` on the process job, the caller's group job, or the named
   group job, with the kind and `detail` of ADR-0004 and ADR-0106; for `-1`, `job_signal` on every process job of the
   session but the caller's, which is Linux's behaviour ([P13]: POSIX requires the caller to be included, Linux
   leaves it out, and the measured programs ran on Linux); `sig` 0 is an
   existence check; `ESRCH` for an
   unknown or reaped pid, `EPERM` for a pid outside the session. `SIGKILL` is `job_kill`. For the stop and continue
   signals the broker acts by the target's recorded disposition (item 8a): `SIGSTOP`, and `SIGTSTP`/`SIGTTIN`/
   `SIGTTOU` with `Default`, are `job_suspend` of the target's job (the process job, or the group job for `-pgid`),
   recorded as stopped so that the parent's `waitpid(WUNTRACED)` reports it; with `Handler` they are
   `job_signal(Custom(n), n)` and no stop; with `Ignore` nothing. `SIGCONT` is `job_resume` always, plus
   `job_signal(Custom(18), 18)` when the target has a handler, as POSIX runs the handler after continuing. For a
   group (`-pgid`, or the terminal's `suspend(20)` on the foreground group, ADR-0105 item 3) the broker goes process
   job by process job, each by its own record, since the records are per pid; a `job_suspend` of the whole group job
   happens only when every member's record is `Default`. `kill(getpid(), sig)` and
   `raise` never reach the broker (ADR-0106), except for a stop of oneself, which `libposix` asks for as
   `stop_self(sig)`.
10. `tcsetpgrp(pgid)` and `tcgetpgrp()`: `pgid` must be a group of the session (`EPERM` otherwise, [P10]); any
    process of the session may call it, which is Linux's behaviour for a caller that has `SIGTTOU` blocked or ignored
    ([P10]) and what dash's `forkchild` (in the child, for its own new group) and `waitforjob` (in the shell, after a
    foreground job) do ([D1], `stage-6/sources.md#other-programs-and-inputs`); the `SIGTTOU` Linux would send a
    background caller that has it unblocked is not emulated (ADR-0105 item 8). The broker records the foreground
    group and sends the terminal service a duplicate of that group job's handle with `SIGNAL | KILL` (`KILL` is the
    right `job_suspend` and `job_resume` need, by the architect's decision; ADR-0105 item 5); `tcgetpgrp` is a
    lookup. `ENOTTY` when the session has no terminal.
11. `terminated(pid)` notifications: none. The parent learns of a child's end from the process job's `TERMINATED`
    signal on its own port (ADR-0106 subscribes process jobs where ADR-0004 and ADR-0006 say "child process handles");
    `waitpid(-1)` is a `port_wait` over the parent's children's job handles; `WNOHANG` is `object_get_signals` on each;
    `WUNTRACED` adds the job's `SUSPENDED` (ADR-0105).

**The session.**

12. `init` starts the broker as it starts a service (ADR-0053), in a `posix` job, with: a root directory connection
    with every right (ADR-0055 `clone`), the terminal's control channel, a terminal connection and the output stream's
    write end (ADR-0105), its own job handle and the ramdisk directory for `fsd --memory` (ADR-0107). The broker
    creates the session job and `tmpd`'s job (item 1), starts the temporary directory service, builds the shell's
    namespace table and bootstrap record (ADR-0107), gets the shell's image through the root connection (ADR-0108),
    prepares pid 2 in a new group, spawns `/bin/sh` with the environment `PATH=/bin:/usr/bin`, `HOME=/home`,
    `TERM=dumb`, `SHELL=/bin/sh` and whatever `/etc/zoraya/session` adds (ADR-0110 item 1; the measurement ran under
    `env -i` with those four, `task-02/method.md#tiers-runs-and-sets`), commits, and gives the terminal the shell's
    group job. It then serves its port until the shell's process job is `TERMINATED` and no other pid is live, and
    exits; `init` treats that as it treats `zosh` ending today. For `make test` the session file names the shell's
    command (ADR-0110).
13. **What the broker never knows:** file names, fds, cwd, environment, `umask`, signal handlers, the contents of any
    pipe or file. It holds no filesystem connection but the one it clones for the shell.

**Trust at stage 6.** The broker checks every handle's type with `handle_type` (ADR-0072) and refuses a pid outside its
table, but it cannot verify that a `spawn_commit` handle is the process in that pid's job, or that an `exec_commit`
handle is in the caller's job: the kernel has no such query. A process that lies breaks only its own pid's record.
Every process is trusted at stage 6 (ADR-0056 is not built); target 2 needs either a kernel query (`process_job` or
an `object_same` over jobs) or spawning by the broker itself (Option D's shape for creation only).

**Tensions with accepted ADRs.**

- *ADR-0003 rule 3*: a `fork` child is in a job of its own, not "the same job" (ADR-0102 lists it).
- *ADR-0004*'s personality mapping says `kill(pid)` is `process_signal` on the process handle and that `libposix`
  subscribes "child process handles"; here they are `job_signal` on the process job and subscriptions to process job
  handles, for the `exec` reason above. During an `exec` handover a `kill(pid)` reaches the exiting old process too,
  which is harmless. Accepting this ADR replaces those two phrases.
- *ADR-0004*'s "the broker creates every process it spawns directly ... in a sub-job of its own" becomes every POSIX
  process, spawned by anyone, in a job of its own.
- *ADR-0046*: `spawn`'s `group` field is unused by `libposix` (the `job` field is the process job); the depth bound of
  32 is reached at about 28 nested sessions, which nothing creates.
- *ADR-0003 rule 2*: the interactive `vforkexec` path keeps rule 2's shape (spawn suspended, set the group and the
  terminal from the parent, resume) through `posix_spawn`'s attributes (item 5), not through a child-side `tcsetpgrp`.
- *ADR-0004 rule 8*'s "hangup to the session job" reaches only POSIX processes, because the session job holds nothing
  else (item 1).
- *[P10]*: `tcsetpgrp` from any process of the session succeeds (item 10); no `SIGTTOU`.
- *ADR-0053*: one more service `init` starts, by the same path.
- *ADR-0006*'s personality text has a blocking `wait4` as `object_wait_one` on the child's process handle; here it is
  a wait on the child's process job handle followed by one `reap` round trip (item 11), for the `exec` reason.

### Effect of the vision

For target 2 the broker is the right seam: a sandbox's POSIX processes are a session job the launcher owns, and the
trust limitation above becomes a requirement for a kernel query or broker-side spawning, both small. For target 3,
several sessions are several brokers, which is the per-session choice of ADR-0002. Threads: a process job with a
multi-threaded process changes nothing here. Networking: no effect.

### Deferred to a later stage

`setsid` and a session that outlives its terminal (DM-070's "ritual"); `kill` across sessions; a kernel query that
ties a process handle to its job; restart of a dead broker (MINIX 3's unsolved case).

*Note, 2026-11-03 (the architect's decision on ADR-0153, recorded by an agent):* the broker gains a mode with no first shell, in which `init` gives the native shell a client connection with no pid and a connection to the temporary service's root; and `spawn_prepare` gains a fourth group form, `Group(job)`, a `posix` job from the caller's tree in which the broker creates the process job, so POSIX elements of a `zosh` command line stay in `zosh`'s job tree. `zosh` reaps its pids as their parent (ADR-0153 rule 10).

## Consequences

- **Positive.** `exec` keeps its pid with no shim process and no handle swap in the parent. One `job_signal` per
  `kill`. Waits stay on the port. The broker is small and never on the data path.
- **Negative.** Three jobs per POSIX process; a `job_create` and two round trips per spawn, one per reap. A dead
  broker ends the session. Trust in what processes report, until a kernel query exists.
- **Follow-up.** `zoraya-proto-posix`; the broker program; `init`'s session path; a measurement of the round trips
  on W1 to W3.

## Invariants

- **INV-0104-1.** Within a session no pid is assigned twice, and after `reap` or an unknown pid every call on it
  returns `ESRCH`. *Check:* a host test of the broker's table through its internal interface over a long run of
  prepares, commits and reaps, and an in-QEMU test reusing a reaped pid.
- **INV-0104-2.** A parent's `waitpid` on a child that `exec`s returns once, after the new image exits, with the new
  image's exit record. *Check:* an in-QEMU `libposix` test: a child that `exec`s a program exiting 7 is waited on
  once and reports 7, never the old process's exit.
- **INV-0104-3.** `kill(pid, sig)` reaches exactly the processes of `pid`'s process job, and `kill(-pgid, sig)` exactly
  those of the group job. *Check:* in-QEMU test with two groups of two processes, each counting requests it took.
- **INV-0104-4.** `setpgid` never moves a process between jobs: it succeeds only when the process's group is already
  the one named, and changes nothing. *Check:* host test of the broker's rule table, and an in-QEMU test of the
  pipeline case (`Join` then `setpgid`) and the refused case (`EPERM`).
- **INV-0104-5.** The broker handles no message on the `open`, `read`, `write`, `close`, `stat`, `lseek`, `mmap` or
  `pipe` paths.
  *Check:* INV-0002-3's counting broker.

## Open questions

- Round trips per spawn and per reap on Zoraya: measure on W2 and W8 (ADR-0110).
- Whether `reap` should be replaced by the broker pushing the new process handle to the parent at `exec_commit`,
  saving the round trip at the cost of an unsolicited message the parent must drain.
- The kernel query for process-to-job membership, when target 2 is chosen.

## Revisit triggers

- Measured broker round trips dominating W8 at `-j4`: push instead of `reap`, or move creation into the broker.
- A program in scope calling `setsid` or needing a second session.
- ADR-0056 accepted: the broker's trust assumptions become checks.

## Architect's decision

Accepted as recommended, 2026-11-02, with the architect's six changes already in the text of ADR-0100 to ADR-0110: the 10 % threshold in ADR-0102, `KILL` for `job_suspend` and `job_resume`, `make test-posix-long`, `make fetch-ports`, `aarch64-unknown-zoraya` with the T2 cost of `config.sub`, INV-0004-6 until exception channels, and INV-0106-4. Decided by the architect; recorded by an agent at the architect's request.

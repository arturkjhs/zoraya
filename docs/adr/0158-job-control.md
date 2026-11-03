# ADR-0158: Job control in the native shell

- **Status:** proposed
- **Date:** 2026-11-03
- **Depends on:** ADR-0004, ADR-0006, ADR-0046, ADR-0096, ADR-0104, ADR-0105, ADR-0151, ADR-0152, ADR-0153
- **Evidence:** DM-017, DM-070, DM-100, CP-07, CP-10, X-12, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno, shell/survey.md#bash, shell/survey.md#fish, shell/survey.md#plan-9-rc

## Context

`zosh` already runs every command in a job of its own (a sub-job, ADR-0046), waits for a job's `TERMINATED`, runs a
command in the background with `&`, and has `jobs`, `wait [%n]` and `kill [-int|-term|-hup|-quit] %n`. Ctrl-C sends
`Interrupt` to the foreground job's tree with `job_signal`, and the third Ctrl-C kills it; Ctrl-\ sends `Quit`
(`docs/design.md`, "Using the shell"). Stage 6a builds `job_suspend` and `job_resume` with the `KILL` right
(ADR-0105 item 5, ADR-0004 rule 3), so Ctrl-Z becomes possible. POSIX programs started through the broker have their
stop and continue dispositions held by the broker (ADR-0104 items 8a and 9).

**What is known.** POSIX job control is a two-level hierarchy of sessions and groups tied to one terminal, with
`SIGTTIN`, `SIGTTOU`, orphaned-group rules and hang-up ritual (DM-070). bash's interface is `%n` jobspecs, Ctrl-Z,
`fg`, `bg` and a `[1] 25647` line (`shell/survey.md#bash`); fish keeps the same commands (`shell/survey.md#fish`).
Plan 9 replaced job control with windows (`shell/survey.md#plan-9-rc`, `alternatives-postmortem.md#plan-9--inferno`).
A job object gives an inescapable tree that process groups cannot (DM-100, CP-07, X-12), and the native ABI has no
asynchronous signals (CP-10, DM-017).

## Decision drivers

1. Ctrl-C, Ctrl-Z and `fg`/`bg` as people know them, shown simply (`shell/survey.md#bash`).
2. A job is a job object: everything it starts is inside it and stops, continues or dies with it (CP-07, DM-100).
3. No terminal arbitration natively: no `SIGTTIN`/`SIGTTOU`, no sessions (DM-070).
4. Never lose a person's work by surprise: leaving the shell with running jobs warns first.

## Considered options

### Option A: POSIX job control (baseline)
- **Description:** Process groups, a controlling terminal, foreground group, `SIGTSTP`, `SIGTTIN`, `SIGTTOU`,
  jobspecs, `fg`, `bg`, `disown`.
- **Evidence for:** Everyone knows the commands (`shell/survey.md#bash`).
- **Evidence against:** The machinery is DM-070's; background reads stop a job by a signal the person never sees.
- **Cost:** *(estimate)* Large, most of it in the broker and `ttyd` (ADR-0104, ADR-0105), already specified there
  for dash.
- **Precedent:** Unix shells.

### Option B: Jobs as job objects, the shell owns the terminal
- **Description:** Each command line is a job object; the shell gives the terminal's input only to the foreground
  job and an empty input to background jobs; Ctrl-C is `job_signal(Interrupt)`, Ctrl-Z is `job_suspend`, `fg` and
  `bg` are `job_resume` plus the shell's choice of where input goes.
- **Evidence for:** Drivers 1 to 3; nothing new in the kernel beyond stage 6a; the shell already relays the terminal
  (ADR-0152 rule 1).
- **Evidence against:** A background job that wants input gets end of file instead of stopping, which differs from
  bash; POSIX programs that call `tcsetpgrp` see no terminal (ADR-0153's stated limitation).
- **Cost:** *(estimate)* A few hundred lines over today's `zosh`.
- **Precedent:** Zircon's jobs as the containment unit (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`);
  bash's and fish's commands on top.

### Option C: No job control; windows instead (Plan 9)
- **Description:** Run long things in another terminal window; no `&`, no Ctrl-Z.
- **Evidence for:** The simplest model (`shell/survey.md#plan-9-rc`).
- **Evidence against:** The serial console has one screen; `zosh` already has `&` and `jobs`, and taking them away
  surprises people.
- **Cost:** None.
- **Precedent:** Plan 9's rio.

## Recommendation

Recommended: **Option B**. It is a recommendation, not a decision.

1. **A job** is one command line's pipeline, run in a new sub-job of the shell's job (ADR-0046) holding every element
   (ADR-0152), POSIX elements included: their process jobs are made inside it by the broker (ADR-0153 rule 10). Its
   number is the smallest free one from 1. The shell keeps the job handle with `KILL | SIGNAL | WAIT`. A line whose
   first element is a builtin or function runs in the shell and cannot be a background job: `&` on it is refused
   (`zosh: for: a builtin or function cannot run in the background`).
2. **Foreground and background.** A line ending in `&` runs in the background and prints `[N] LINE`. A background job
   gets an `in` stream whose write end is already closed (end of file), and its `out` and `err` still reach the
   terminal (ADR-0154 rule 7 keeps them off the line being edited). Exactly one job, or none, is in the foreground
   and gets the terminal's input.
3. **Keys while a foreground job runs.** The shell reads every key and turns these into job calls itself. Ctrl-C:
   `job_signal(Interrupt)` on the job tree, shown as `^C`; if the job is still running a second later, the shell
   prints `(Ctrl-C again to kill it)`, and the next Ctrl-C is `job_kill`. Ctrl-\: `job_signal(Quit)`. Ctrl-Z: the job
   stops (below) and the prompt returns with `[N] stopped: LINE`. Other keys are the job's input. (Today's `zosh`
   kills at the third Ctrl-C; the second is enough once the shell has said so.)
4. **Stopping.** For a native job Ctrl-Z is `job_suspend` of its tree. For a POSIX element the shell asks the broker,
   which applies each process's stop disposition (ADR-0104 item 9); a job some of whose processes ignore or handle
   the stop keeps running, and the shell says `[N] did not stop: LINE`.
5. **Commands.** `jobs` lists `[N]  running|stopped  LINE` with its grants and designations as `grants` words them
   (ADR-0153 rule 7); `fg [%N]` prints the job's line, resumes it (`job_resume`) if stopped and gives it the
   terminal; `bg [%N]` resumes it in the background and prints `[N] LINE &`; `wait [%N]` waits for one or all
   background jobs; `kill [--interrupt|--terminate|--hangup|--quit|--kill] %N` sends the request (`--terminate` is the
   default; `--kill` is `job_kill`). `%N` defaults to the newest job. There is no `disown`: a job lives in the shell's
   job tree and ends with the shell (CP-07). Errors are the builtin's: `fg: no job to continue`.
6. **Notices.** A job's end is reported exactly once, as `[N] done: LINE`, or `[N] LINE: WHAT` with ADR-0151's words
   for a failure, and always at one of these points, never in the middle of a line being edited:
   - `kill` waits up to 2 s for the job to end and reports it before returning; if the job is still running it says
     `[N] still running; kill --kill %N ends it`;
   - `wait` reports each job as it ends;
   - otherwise, the next time the prompt is drawn after the shell has read the job's `TERMINATED`.
7. **Leaving.** `exit` with jobs running or stopped prints `2 jobs are still running; exit again to end them` and
   does nothing; a second `exit` in a row ends them (`job_kill`) and the shell.

**Examples.**

```
~> make -C big &
[1] make -C big
~> jobs
[1]  running   make -C big   (~/big: change anything in)
~> spin
^Z
[2] stopped: spin
~> bg
[2] spin &
~> jobs
[1]  running   make -C big   (~/big: change anything in)
[2]  running   spin
~> kill %2
[2] spin: terminated
~> sleep 60
^C
sleep: interrupted (Ctrl-C)
~> stubborn
^C
(Ctrl-C again to kill it)
^C
stubborn: killed
~> exit
1 job is still running; exit again to end it
~> wait
[1] done: make -C big
~> fg
fg: no job to continue
```

In this example `make`'s description makes `-C DIR` a `tree` operand, so `jobs` shows that `make` may change anything
in `~/big`, and nothing more of `~`.

### Effect of the vision

For target 2 each agent command is a job the person can stop, continue or kill as a whole, and nothing it starts can
leave the job tree (CP-07). Threads: `job_suspend` stops every thread (ADR-0105 INV-0105-5).

**Tensions with accepted ADRs.**
- *ADR-0004 rule 8* has the terminal service hold the foreground job and call `job_signal` and `job_suspend` for the
  interrupt and suspend characters, and *ADR-0096* expects the broker to interpret Ctrl-C at stage 6. For its own
  sessions `zosh` is that terminal service: it reads the keys and makes the calls (rule 3), as it does today. The
  terminal still names no process.
- *ADR-0105*: background POSIX jobs started by `zosh` never meet `NotForeground`, because they hold no terminal
  connection (ADR-0153); `SIGTTIN` is not reached from `zosh`. Nothing in ADR-0105 changes for a POSIX session run
  by `init` alone.
- *ADR-0104 items 1 and 4*: POSIX jobs live in `zosh`'s tree through `spawn_prepare(Group(job))`, declared in
  ADR-0153.

## Consequences
- **Positive.** Familiar keys and commands; whole-tree stop and kill; no terminal arbitration.
- **Negative.** A background job that reads input gets end of file instead of stopping; no `disown`; Ctrl-Z needs
  stage 6a's `job_suspend`.
- **Follow-up.** Ctrl-Z, `fg`, `bg` and the notices in `zosh`; the broker request for POSIX jobs.

## Invariants
- **INV-0158-1.** Ctrl-C, Ctrl-\ and Ctrl-Z reach only the foreground job's tree, and every process in it. *Check:*
  an in-QEMU test with a foreground job of two processes and a background job of two, each counting the requests it
  takes, and a stopped-state check after Ctrl-Z.
- **INV-0158-2.** A background job never receives a byte of terminal input. *Check:* an in-QEMU test with a
  background reader that reports end of file at once while keys are typed at the prompt.
- **INV-0158-3.** `exit` with live jobs ends nothing the first time and every job the second. *Check:* a scripted
  serial session (`tests/shell/`).
- **INV-0158-4.** Every job's end is reported exactly once, by `kill`, by `wait` or before a prompt, and never while
  a line is being edited. *Check:* a host test of the job table and notice queue with fake job ends arriving at
  every point (during `kill`, `wait`, editing, between prompts).

## Open questions
- Whether a background job that reads should stop (bash) rather than see end of file; nothing measured.
- Whether `fg` should take a job by name as well as by number (`%make`).

## Revisit triggers
- An in-scope POSIX program needs `SIGTTIN` or a tty from `zosh` (ADR-0153's trigger).
- People losing background output in the middle of their typing: hold background output until the line is entered.

## Architect's decision

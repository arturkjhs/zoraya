# ADR-0158: Job control in the native shell

- **Status:** proposed
- **Date:** 2026-11-03
- **Depends on:** ADR-0004, ADR-0006, ADR-0046, ADR-0104, ADR-0105, ADR-0151, ADR-0152, ADR-0153
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
   (ADR-0152). Its number is the smallest free one from 1. The shell keeps the job handle with `KILL | SIGNAL | WAIT`.
2. **Foreground and background.** A line ending in `&` runs in the background and prints `[N] LINE`. A background job
   gets an `in` stream whose write end is already closed (end of file), and its `out` and `err` still reach the
   terminal. Exactly one job, or none, is in the foreground and gets the terminal's input.
3. **Keys while a foreground job runs.** Ctrl-C: `job_signal(Interrupt)` on the job tree; a second Ctrl-C again, with
   the line `(Ctrl-C again to kill it)`; a third `job_kill`. Ctrl-\: `job_signal(Quit)`. Ctrl-Z: the job stops
   (below) and the prompt returns with `[N] stopped: LINE`. Other keys are the job's input.
4. **Stopping.** For a native job Ctrl-Z is `job_suspend` of its tree. For a POSIX job the shell asks the broker,
   which applies each process's stop disposition (ADR-0104 item 9); a POSIX job whose processes ignore or handle
   the stop keeps running, and the shell says `[N] did not stop: LINE`.
5. **Commands.** `jobs` lists `[N]  running|stopped  LINE` with grants (ADR-0153); `fg [%N]` resumes the job
   (`job_resume`) if stopped and gives it the terminal; `bg [%N]` resumes it in the background; `wait [%N]` waits for
   one or all background jobs; `kill [--interrupt|--terminate|--hangup|--quit|--kill] %N` sends the request
   (`--terminate` is the default; `--kill` is `job_kill`). `%N` defaults to the newest job. There is no `disown`: a
   job lives in the shell's job tree and ends with the shell (CP-07).
6. **Notices.** When a background job ends the shell prints, before the next prompt and never in the middle of a
   line being edited, `[N] done: LINE`, or `[N] LINE: WHAT` with ADR-0151's words for a failure.
7. **Leaving.** `exit` with jobs running or stopped prints `2 jobs are still running; exit again to end them` and
   does nothing; a second `exit` in a row ends them (`job_kill`) and the shell.

**Examples.**

```
~> make -C big &
[1] make -C big
~> jobs
[1]  running   make -C big   (+w ~/big)
~> spin
^Z
[2] stopped: spin
~> bg
[2] spin &
~> jobs
[1]  running   make -C big   (+w ~/big)
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
^C
stubborn: killed
~> exit
1 job is still running; exit again to end it
~>
[1] done: make -C big
~> fg
zosh: no job to continue
```

In this example `make`'s description says `cwd: write` and `-C DIR` is a `tree` operand, so the job's grant is shown
as `+w ~/big`.

### Effect of the vision

For target 2 each agent command is a job the person can stop, continue or kill as a whole, and nothing it starts can
leave the job tree (CP-07). Threads: `job_suspend` stops every thread (ADR-0105 INV-0105-5).

**Tensions with accepted ADRs.** *ADR-0105*: background POSIX jobs started by `zosh` never meet `NotForeground`,
because they hold no terminal connection (ADR-0153); `SIGTTIN` is not reached from `zosh`. Nothing in ADR-0105
changes for a POSIX session run by `init` alone.

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

## Open questions
- Whether a background job that reads should stop (bash) rather than see end of file; nothing measured.
- Whether `fg` should take a job by name as well as by number (`%make`).

## Revisit triggers
- An in-scope POSIX program needs `SIGTTIN` or a tty from `zosh` (ADR-0153's trigger).
- People losing background output in the middle of their typing: hold background output until the line is entered.

## Architect's decision

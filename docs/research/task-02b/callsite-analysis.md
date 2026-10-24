# TASK-02b: call-site analysis for parallel builds (M7)

This covers every compiled call site in GNU make, pdpmake and dash that implements one of four features:

1. the jobserver (client and server sides);
2. readiness waiting and child reaping under `-j`;
3. signal handlers and signal masks used during parallel builds;
4. cleanup on interruption.

All sites are in [`callsites.csv`](callsites.csv). The generated tables, with one row per site and its
event-model sentence, are in `tools/posix-surface/results/par/callsite-tables.md`.

**Method.** It is the same as TASK-02 M3 ([`../task-02/method.md`](../task-02/method.md) → M3):

- **Compiled code only.** A pattern match counts only if its line is in the DWARF line table of the measured binary,
  inside an executable section (`analysis/callsites_par.py`, which shares `analysis/dwarfsites.py` with M3).
- **Features by enclosing function.**
  - Jobserver: every call inside a `jobserver_*` function, plus every call of a `jobserver_*` function from
    elsewhere.
  - Wait/reap: readiness waits and the wait family, wherever they appear.
  - Signals: dispositions and masks.
  - Cleanup: calls inside the interruption handlers (`fatal_error_signal`, `delete_target`, `make_handler`,
    `onint`, …).
- **One review per line.** A line can carry two features; the review is per line.

**Estimates.** Classes, efforts and the event-model sentences are estimates from reading the code; they are not
measurements. The coordinator wrote the reviews (`review/m7-coordinator.tsv`). Where a site was already in M3, its
class matches M3's.

**The event model the mapping refers to.** It is the one named in the task: process exit, pipe readiness and user
interrupt are all delivered as events on handles that a process waits on. Each mapping sentence says what the site
would become in such a model. The model is a lens for describing the code, not a proposal.

## Overview

138 distinct lines (143 site–feature pairs):

| Program | direct | restructure | emulate | n/a | direct lines | restructure lines |
|---|---|---|---|---|---|---|
| GNU make | 55 | 20 | 11 | 3 | 43 | 45 |
| pdpmake | 2 | 3 | 2 | 0 | 13 | 20 |
| dash | 0 | 5 | 37 | 0 | 0 | 10 |

By feature (site–feature pairs; a line in two features counts in both): see `callsite-tables.md`. In short:

- **GNU make's jobserver:** 35 direct, 4 restructure, 10 emulate. The emulate sites are all the *protocol*, not the
  mechanism.
- **GNU make's signals and masks:** 10 of 17 are restructure. They all exist to make SIGCHLD and the fatal signals
  safe to take asynchronously.
- **dash:** almost entirely emulate. The shell language exposes signals through `trap`, `kill` and job control.

pdpmake has no parallel mode: `-j` is accepted and documented as "not implemented". W2-j, W3-j and W8 confirm it
never runs more than one recipe at a time (`parallel-surface.md`). Its sites are the serial ones from TASK-02.

## 1. Jobserver (GNU make)

**What the code does** (`src/posixos.c`, `src/main.c`, `src/job.c`):

- **Server.** The top make with `-jN` creates the jobserver in `jobserver_setup` (`main.c:2217`), filled with N−1
  one-byte tokens (`posixos.c:212`). There are two styles:
  - FIFO, the default in 4.4.1: `mkfifo /tmp/GMfifo<pid>`, then two `open`s (`posixos.c:164–180`).
  - Pipe, with `--jobserver-style=pipe`: `pipe()` (`posixos.c:195`).
- **Auth string.** The server publishes `--jobserver-auth=fifo:PATH` or `--jobserver-auth=R,W` in `MAKEFLAGS`
  (`main.c:2220`).
  - In pipe style, the fds are inherited only by recursive `$(MAKE)` recipes. `jobserver_pre_child` and
    `jobserver_post_child` toggle close-on-exec around each spawn (`job.c:1474/1479`, `posixos.c:415–427`).
  - Every other recipe gets `--jobserver-auth=-2,-2` appended (`variable.c:1075`), so a sub-make there knows its
    fds are not real.
- **Client.** A sub-make parses the auth string (`main.c:1844`) and opens the FIFO or adopts the fds
  (`posixos.c:237/245`).
- **Acquiring a token.** Each make has one implicit token. For more jobs it calls `jobserver_acquire`
  (`job.c:1882`): a `pselect` on the token fd with an empty signal mask (`posixos.c:484`).
  - SIGCHLD is blocked everywhere else, so a child exit interrupts this wait and nothing else. The wait thus means
    "token *or* child exit".
  - It is followed by a non-blocking `read`, which can fail with `EAGAIN` when another make took the token first
    (`posixos.c:506`).
- **Releasing.** `free_child` writes a token back when a child is reaped (`job.c:1142`). At exit, the top make
  reads all tokens back (`main.c:3790`) and unlinks the FIFO.

**Classes (estimate):**

| Group | Class | Why |
|---|---|---|
| create / read / write tokens; inherit / no-inherit toggling; close | direct | a counting-semaphore handle passed explicitly to children covers it; 1–3 lines each, 10 for setup |
| `pselect` + empty mask + blocked SIGCHLD (`posixos.c:465/484`, `main.c:2002–2015`, `job.c:1882`) | restructure, about 10 lines | the construction exists only because "child exited" is a signal and "token available" is an fd. In the event model it is one wait over two handles |
| auth string, FIFO name in `/tmp`, `-2,-2`, opening the FIFO by name (`main.c:1844/2220`, `variable.c:1075`, `posixos.c:164/175/180/195/237/245/354`) | emulate | `MAKEFLAGS --jobserver-auth` is a protocol other programs read: other make versions, ninja, cargo and the Linux kernel's build. The fd numbers and the FIFO path must stay meaningful in the POSIX personality, even if make itself could use a passed handle |

**Measured behaviour that bears on this** (`parallel-surface.md`, `results/par/parallel-tables.md`):

- **Token traffic is tiny.** W8 `-j4` used about 3–4 token reads per build for 64 jobs.
- **Every make spends its free token first.** Leaf makes then mostly sit in `pselect6`, and their wait ends by
  SIGCHLD from their own child (`ERESTARTNOHAND`) far more often than by a token.
- **Practical result.** The jobserver's job is to *cap* concurrency across processes, and the dominant wake-up is
  child exit. Both styles behave the same; only the setup differs (`mknodat` + `openat` of the FIFO vs `pipe2`).

## 2. Readiness waiting and child reaping

**GNU make.**

- `reap_children` is called from about ten places (`job.c`, `remake.c`, `function.c`, `main.c`).
  - Non-blocking reaps use `waitpid(-1, WNOHANG)`, a macro the regex does not match on its own line.
  - Blocking reaps use `wait()` (`job.c:762`).
- The callers are all `direct`: each is a point where make drains, or blocks for, child-exit events.
- Under `-j`, `wait4` with `WNOHANG` appears in the traces, and `pselect6` becomes the main blocking call.
- The serial runs had neither `pselect6` nor `rt_sigreturn` (`syscall-delta.csv`).

**pdpmake.** It runs every recipe line with `system()` (`make.c:103`, direct): spawn + wait. During the wait
`system()` ignores SIGINT and SIGQUIT in make itself.

**dash** (`jobs.c:1209–1223`, emulate):

- `waitproc` uses `wait3` (with `WUNTRACED` under job control). It blocks all signals, then `sigsuspend`s until
  SIGCHLD or a trapped signal arrives.
- That is the `wait` builtin's mechanism in W10: `rt_sigsuspend` woken by SIGCHLD, `ERESTARTNOHAND`, handler return,
  then `wait4`.
- POSIX requires `wait` to return early when a trapped signal arrives, so this stays emulate.

## 3. Signal handlers and masks during parallel builds

**GNU make.**

- **Handlers.** `fatal_error_signal` is installed for HUP, QUIT, PIPE, INT, TERM, XCPU and XFSZ through the macro
  `FATAL_SIG` (`main.c:1270–1295`). A signal ignored on entry stays ignored. These 7 lines were missed by the
  TASK-02 M3 patterns; M7 matches `FATAL_SIG` (`research-log.md`). `child_handler` is installed for SIGCHLD
  (`main.c:2002`).
  - `child_handler` only increments a counter and calls `jobserver_signal`, which is a no-op with `pselect`
    (`job.c:615`).
- **Masks.**
  - SIGCHLD is blocked for the whole run (`main.c:2015`) and unblocked only inside `pselect`.
  - `block_sigs`/`unblock_sigs` (`job.c:475–491`, and callers at `job.c:1037/1076/1096/1465/1663`) block the fatal
    signals while the children list is edited. The handler walks that list.
  - All of this is `restructure`, at no extra effort beyond the handler restructure (`commands.c:530`, 30 lines). It
    disappears when the interrupt is an event taken at the wait rather than an asynchronous handler.
- **SIGUSR1** toggles debug output at run time (`main.c:2025`): restructure, a control message. It was never sent.

**pdpmake.** `make_handler` for HUP and TERM: reset, remove the target, re-raise (restructure, 20 lines, as M3). The
inherited-disposition query (`main.c:446`) is `emulate`: "ignored on entry" must be inherited.

**dash.**

- **The interactive shell** catches SIGINT, SIGQUIT and SIGTERM (`trap.c:397–399`) while it owns the terminal.
- **Children** restore those dispositions (`jobs.c:914–916`).
- **Background jobs of a non-job-control shell** ignore SIGINT and SIGQUIT (`jobs.c:903/904`). W10 shows it: each
  `cmd &` child sets `SIGINT:SIG_IGN` and `SIGQUIT:SIG_IGN` before exec. POSIX requires the inherited ignore.
- **`onint`** (`error.c:108–111`) re-raises SIGINT so that a non-interactive shell dies by it.
- **`waitforjob`** (`jobs.c:1060`) makes the interactive shell interrupt itself when its foreground job died of
  SIGINT. This is the `tkill(SIGINT)` at the end of the W9 Ctrl-C timeline.
- **The heredoc writer child** (`redir.c:378–384`) ignores signals. It exists only for heredocs larger than a pipe
  when memfd is unavailable (restructure, 10 lines: dash already uses `memfd_create`, seen in W4).

## 4. Cleanup on interruption

**GNU make's `fatal_error_signal`** (`commands.c:524–601`) does all of its cleanup inside the signal handler:

1. resets the disposition (`:530`);
2. closes the jobserver and unlinks the FIFO (`:534`);
3. on SIGTERM only, forwards SIGTERM to every child (`:544`);
4. deletes the targets of running children (`:568`, `delete_target` → `unlink`, `:650`);
5. **waits for all children** (`reap_children`, `:573`), a blocking wait inside a signal handler;
6. removes intermediates (`:582`);
7. re-raises the signal on itself (`:600`).

The W9 timelines show this sequence in every make process, in every repetition (`event-timelines.md`).

Classes (estimate):

| Site | Class | Note |
|---|---|---|
| the handler as a whole (`:530`) | restructure, 30 lines | runs from the main wait on an interrupt event |
| `kill(child, SIGTERM)` (`:544`) | direct | terminate request on a child or group handle |
| target deletion (`:568`, `:650`, `:669/673`), `remove_intermediates` | direct | plain file operations |
| `reap_children` inside the handler (`:573/578`) | restructure | wait for exit events from the main loop |
| re-raise (`:600`) | **emulate** | the parent (outer make, shell) must see "terminated by SIGINT/SIGTERM", not a failure code. dash aborts the rest of a command line only on death by SIGINT |

pdpmake's equivalent is `make_handler` + `remove_target` (`main.c:432–434`, `make.c:13`). dash has no cleanup of
its own: it relies on the terminal driver signalling the whole foreground job.

## What the mapping exercise shows (estimate)

- **For make, the event model mostly *removes* code.**
  - The 20 GNU make restructure sites total about 45 lines. Almost all exist to make asynchronous signals safe:
    blocked SIGCHLD, the empty mask for `pselect`, the fatal-set masking around the children list, a handler that
    does a whole shutdown.
  - In a model where child exit, token availability and user interrupt are events on handles, make waits on all
    three at one point, and none of that machinery is needed.
- **The irreducible parts are interfaces to other programs, not mechanisms.** Two things stay `emulate`:
  1. the `--jobserver-auth` string in `MAKEFLAGS`: fd numbers or a FIFO path that other tools read;
  2. "terminated by signal N" as an exit status that parents act on.
- **The shell is different.** dash's 37 emulate sites are the POSIX shell language itself: `trap`, `kill`, `wait`
  returning on trapped signals, background jobs ignoring SIGINT, job control. TASK-02 found the same.

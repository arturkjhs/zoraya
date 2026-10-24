# TASK-02: impact of the measurements on the TASK-01 conflicts

For each conflict from [`candidate-principles.md`](../candidate-principles.md) → "Conflicts between principles" that
the data touches, this document records three things: what the measurements show, which options they rule in or out,
and what stays unknown. **It makes no decisions.**

Every figure below comes from a pipeline output, and the source is named each time:

- `tools/posix-surface/results/*.md`;
- the CSVs and summaries in this directory: [`posix-surface.md`](posix-surface.md) and
  [`callsite-analysis.md`](callsite-analysis.md).

Each figure was checked against the independent review (`research-log.md` → "Independent review"). Classes and
efforts from M3 are estimates. Dynamic figures cover only W1–W6.

Scope of the evidence:

- The programs are dash, toybox, busybox (5 applets), tcc, GNU make, pdpmake, oksh, mrsh, clang and lld.
- The workloads are W1–W6 on one Alpine aarch64 VM.
- make always ran serially (no `-j`), and the build never ran `tcc -run`.

"Rules in" means the data does not contradict an option. "Rules out" means the data contradicts the premise an
option depends on, for these programs.

## X-1: capability-only access (CP-01, CP-02) vs ease of POSIX porting (CP-27)

### What the data shows

- **Absolute paths opened** in core runs (`results/path-prefixes.md`):

  | Path | Opens | What it is |
  |---|---|---|
  | the workload's own tree | 7,826 | configure and make pass absolute `srcdir`/prefix paths; tcc's own include directory is under the prefix |
  | `/usr/include` | 5,294 | system headers, tcc |
  | `/usr/lib` | 559 | crt files and libc, tcc's linker |
  | `/dev/null` | 156 | |
  | `/lib/*` | 28 | T3 shared libraries |
  | `/tmp/*` | 9 | |
  | `/etc/passwd` | 2 | |
  | `/etc/group` | 1 | |
  | `/dev/tty` | 1 | dash job control, T0 |

  Relative opens outnumber absolute ones: 19,007 vs 13,921 (`syscall-surface.csv`).
- **Paths come from data, not code.** Most absolute paths are built at run time from configure results, makefile
  variables, `-I`/`-L` options and tcc's built-in search paths. They are not literals at a call site. M3 found only a
  handful of path literals:
  - `/bin/sh`;
  - `/dev/null`, `/dev/tty`, `/dev/urandom`;
  - `/tmp`;
  - `/etc/passwd`, `/etc/group`, `/etc/profile`;
  - `/proc/self/exe`.
- **Executions name absolute paths.**
  - 2,922 of 3,002 successful `execve` calls used an absolute path (`syscall-surface.csv`). Shells and musl's
    `execvp` resolve `$PATH` in user space, then exec the absolute result.
  - Hard-coded `/bin/sh`: GNU make 143 (7 + 136), pdpmake 469 (W4p), shebangs 92 (`summary-tables.md`).
  - `config.guess` probes `/bin/uname`, `/bin/arch` and seven paths that do not exist.
- **cwd is part of the programs' meaning.**
  - GNU make: `-C`, `CURDIR` and `$(realpath)`, 6 emulate sites.
  - dash: `cd` and `pwd`.
  - toybox: `xchdir` and `xgetcwd`, restructure.
  - `chdir` was called with an absolute path 32 times and a relative path once (`syscall-surface.csv`).
- **A per-process view of the global namespace was enough for every workload.** Every measured run executed in a
  mount namespace where `/bin` was replaced per measured set (`method.md` → "Namespace"). All core workloads passed
  their checks there, and none executed a binary outside the set (`summary-tables.md`). This is the
  measurement setup, not a test of capability mode. But it shows that these programs tolerate a namespace whose
  global paths are bound by the launcher.
- **Identity databases.** `getpwnam`, `getpwuid` and `getgrgid` are used for `~user` expansion (dash, GNU make) and
  for printing owners (toybox `ls -l`, `id`, `tar`). `/etc/passwd` and `/etc/group` were opened 3 times in total.

### Rules in / rules out (for these programs)

- **Out: "the toolchain works with handles granted at startup only."** These programs open paths named in their
  input, after startup: makefiles, `#include` lines, configure scripts, `$PATH`. tcc alone opened 5,294 header paths
  under `/usr/include`, all derived from `#include` and its search list. A capability-only process model without a
  name-resolution layer cannot run them unmodified. That is consistent with GR-008 (Capsicum tcpdump).
- **In: a POSIX personality that resolves global names inside a per-process namespace constructed by its launcher**
  (Plan 9 style, GR-004). The namespace could be backed by directory handles the launcher holds.
  - The measured working set is small and enumerable: the workload tree, `/usr/include`, `/usr/lib`, a `/bin` with
    `sh`, `/dev/null`, `/dev/tty` for interactive use, `/tmp`, and two `/etc` files.
  - The measurement harness itself worked this way for `/bin`.
- **In: removing literal global paths from the native ABI while keeping them in the personality.** Most path literals
  are `direct` in M3 (`/dev/null` → a null handle, `/etc/passwd` → a lookup API). The exceptions are emulate:
  - `/dev/tty` (the controlling terminal);
  - `/tmp` in `mktemp`, whose job is to print a path;
  - `/bin/sh` in shebangs and in dash's ENOEXEC fallback.

### Unknown

- Whether a cwd can be a directory handle without breaking `realpath`, `getcwd` and make's `CURDIR`. These need a
  printable absolute path back from the handle (emulate sites in GNU make and dash).
- How much of the 7,826 opens of the workload's own tree would survive a launcher that grants only the tree. No run
  tried it.
- Whether the capability benefit survives inside the POSIX personality, once it is given the namespace above. That is
  the KeyKOS question (PM): the data shows what the namespace must contain, not whether it is still a useful
  boundary.

## X-2: no `fork` (CP-05) vs hosting the C toolchain

### What the data shows

- **Only shells fork.** Apart from 3 forks by a script in hello's build, every fork-style clone comes from a shell
  (`results/fork-children.md`; `summary-tables.md` → "Process creation by program").
  - GNU make creates every child vfork-style through `posix_spawn`: 281 in core runs. M3 classes all 8 of its
    process sites `direct`.
  - toybox (xargs) and clang use vfork-style creation only.
  - tcc creates no processes.
- **Every vfork-style child execs:** 2,041 from dash, 281 from GNU make, 2 from toybox and 1 from clang in core runs
  (`results/fork-children.md`). This is exactly the `posix_spawn` shape.
- **The shell's forks split** (core runs, `results/fork-children.md`):
  - **536 fork+exec.** Pipeline elements and background commands that run an external program.
  - **1,169 fork-without-exec.** The child keeps running shell code: subshells, command substitution, the heredoc
    writer, pipeline elements that are builtins. 695 of them start processes of their own.
- **M3, dash.** `forkshell` (fork) is `emulate`, and `vforkexec` is `restructure`, about 60 lines (estimate).
  - The comparison shells have the same structure. oksh and mrsh fork every subshell, pipeline element, `&` list
    and command substitution: all emulate.
  - toybox's `xfork`/`xpopen(argv == NULL)` re-run the applet in a child. They are restructure, but no workload
    reached them except through `XVFORK` + exec.
- **M4.** dash's non-interactive `vforkexec` was replaced by `posix_spawn` in 52 lines, and pdpmake's `system()` in
  34 lines. Outputs were identical in both cases. The interactive and job-control path was left on `vfork`, because
  handing the terminal to the child's process group (`tcsetpgrp`) has no `posix_spawn` attribute
  (`callsite-analysis.md` → M4).
- **musl's `posix_spawn` is itself a `CLONE_VM|CLONE_VFORK` clone.** Swapping `vfork` for `posix_spawn` left the
  `clone` count unchanged (W4: 13,392 → 13,392) and added pipes and signal resets (`m4-compare.md`).

### Rules in / rules out

- **Out: "the C toolchain needs `fork`."** For the compiler, make, linker and core utilities, the data shows no
  fork at all, and every child they create execs. GR-005's caveat that the toolchain "mostly uses fork+exec" is
  confirmed in its strong form for these programs: there is no fork, only spawn-shaped vfork and `posix_spawn`.
- **Out: "a spawn primitive is enough for the shell as written."** 1,169 of dash's 1,705 forks never exec.
  POSIX shell semantics put subshells and command substitution in a separate process with a copy of the shell
  state.
- **In: spawn-only native creation, with fork confined to the shell.** Three variants fit the data:
  1. the POSIX personality emulates `fork` for shell processes only;
  2. the shell is restructured so that subshells run in-process, or re-exec the shell with serialised state;
  3. a different shell.

  The data cannot rank these. It shows that variant 1's emulation would serve a single program.
- **In: `posix_spawn` with a process-group attribute as the native-shaped interface for make and non-interactive
  shells.** M4 shows it works for the non-interactive case. Interactive job control additionally needs
  "give this process group the terminal" from the parent (see X-3 and CP-25).

### Unknown

- The cost of fork emulation for dash. Of the 1,169 no-exec forks, W4 accounts for 1,167: 764 in configure and
  403 in make (`results/fork-children.tsv`). The memory footprint per fork (X-6) was not measured.
- Whether a fork-free shell can run autoconf `configure` scripts unchanged. No such shell was measured.
- Parallel make (`-j`) was not run. GNU make still used `posix_spawn` in every observed path, but the jobserver
  path (`pselect`, pipes shared with sub-makes) was not exercised.

## X-3: no asynchronous interruption (CP-10) vs POSIX signal semantics

### What the data shows

- **Signals actually delivered** (`results/signals-delivered.md`, core runs):

  | Signal | Count | Where |
  |---|---|---|
  | `SIGCHLD` (child exited) | 3,751 | every workload that spawns processes |
  | `SIGCHLD` (child killed) | 1 | W5 |
  | `SIGINT` from the terminal (Ctrl-C) | 1 | W5 |
  | `SIGINT` raised by dash itself | 1 | W5 |

  - No other signal was delivered in any core run.
  - In T1–T3 runs, `SIGCHLD` is the only signal at all.
  - The only process killed by a signal is the foreground `sleep` hit by Ctrl-C in W5.
  - The comparison shells show the same pattern.
- **Interrupted system calls** occur only in W5:
  - one `nanosleep` (the killed `sleep`);
  - the shells' `rt_sigsuspend`, woken by `SIGCHLD`/`SIGINT`.

  No `EINTR` or restart was seen in any T1–T3 run (`results/signals-delivered.md`).
- **Handlers installed** (`summary-tables.md` → "Signal handlers installed"; M3):
  - **dash:** `SIGCHLD` and `SIGINT` always. In W4 also `SIGHUP`, `SIGTERM` and `SIGPIPE`, from configure's
    `trap` lines.
  - **GNU make:** `SIGCHLD` plus a fatal-signal cleanup set (`HUP INT QUIT TERM PIPE USR1 XCPU XFSZ`). The cleanup
    deletes partial targets and re-raises the signal.
  - **pdpmake:** `HUP` and `TERM` cleanup.
  - **toybox:** `USR1` and `USR2`.
  - **clang and lld:** 15 signals, for crash reporting.

  Most of the `rt_sigaction` traffic is musl resetting dispositions in children.
- **What the signal sites need** (M3, `callsite-analysis.md` → feature 2; class estimates):
  - **Child-exit notification** (`SIGCHLD`, `sigsuspend`, `pselect` with SIGCHLD unblocked).
    - GNU make: restructure. It only needs a wakeup.
    - dash: emulate.
  - **Death-by-signal propagation.** dash resets and re-raises `SIGINT` so that its parent sees "terminated by
    `SIGINT`": emulate. That status is part of the shell's interface.
  - **Termination cleanup** in GNU make, pdpmake and toybox: restructure, about 15–30 lines each (estimate). It
    needs a "termination requested" event.
  - **Job control** (`SIGTSTP`, `SIGTTIN`, `SIGTTOU`, `SIGCONT`, `SIGHUP` to jobs; `killpg`) in dash, oksh and
    mrsh: emulate. W5 exercised only Ctrl-C.
  - **The `kill` builtin**, where the user names the signal: emulate.
  - **Fault capture** in tcc `-run` and clang/lld crash handlers: restructure (tcc). Not exercised by T1–T2
    workloads.

### Rules in / rules out

- **Out: "the toolchain relies on asynchronous signal delivery in normal operation."** In T1–T3 the only signal
  delivered is `SIGCHLD`, and no system call was interrupted. Every `SIGCHLD` use found in non-shell programs is a
  wakeup for a wait (GNU make). That could be a wait result on a process handle (CP-06, CP-08).
- **In: native ABI without async interruption, and a POSIX personality that emulates a small, enumerable signal
  set:**
  - `CHLD`, `INT`, `TERM`, `HUP`, `PIPE`, `QUIT`;
  - for interactive shells, `TSTP`, `TTIN`, `TTOU` and `CONT`;
  - handler/ignore/default dispositions, masks, `sigsuspend`;
  - "terminated by signal N" exit status;
  - "ignored on entry" inheritance, which shells must honour for `trap`.

  The data puts the emulation's hard part in the interactive shell (Ctrl-C, job control), not in the build.
- **Unconstrained: synchronous fault delivery** (CP-10 cost). Only tcc `-run` and the clang/lld crash handlers use
  it, and no workload triggered a fault.

### Unknown

- **Fidelity under parallel make and real interrupts.** The data has one Ctrl-C, and make never ran with `-j`. A
  user interrupting a parallel build (signal forwarding, cleanup of several targets, `EINTR` in waits) was not
  measured.
- **Whether `SIGPIPE`-by-default matters.** No process was killed by `SIGPIPE`, but the workloads did not include
  `cmd | head`-style early-closing pipelines at volume.
- **What toybox's `USR1`/`USR2` handlers are for.** They are installed in T2 runs and never delivered. Not
  investigated.

## Other conflicts the data touches

### X-4: completion-native async (CP-08) vs readiness for POSIX

- **No core run made a single readiness call:** no `poll`, `select`, `ppoll` or `pselect6` in the T0–T3 core traces
  (`syscall-surface.csv`). `ppoll` appears only in busybox sh's line editor (W5 comparison, 222 calls).
- GNU make imports `pselect` for the jobserver, and it is M3's only DM-023 site (restructure, about 60 lines). It was
  not called, because make ran serially.
- **Shown:** serial builds need no readiness interface. **Not shown:** what `make -j` or an interactive line editor
  would need.

### X-6: strict commit (CP-14) vs fork emulation

- If fork is emulated (X-2), the data shows it would be for the shell only: 1,169 no-exec forks of dash in core runs.
  No compiler or make forks.
- The commit cost of a dash fork was not measured.

### X-15: explicit configuration (CP-21) vs ergonomics and the POSIX layer

- `environ` is imported by every measured program (`libc-surface.csv`).
- `getenv` sites are almost all `direct` in M3 (DM-085: 33 of 34 sites). `putenv`/`setenv` are used by dash, GNU
  make, pdpmake, mrsh and toybox.
- pdpmake's M4 patch passes an explicit `envp`, and it worked unchanged. That shows the plumbing cost on one program:
  part of the 34 lines.
- `setlocale` is called by dash, GNU make and toybox. `newlocale`/`uselocale` are called by toybox applets
  (`libc-surface.csv`). All 9 DM-097 sites are `direct`.
- **Rules in:** an explicit environment and locale at the native level, with `environ` and `setlocale` emulated in
  the personality.

### CP-25: terminal rights and job control

- Terminal control appears only in T0, the interactive shell: `tcsetpgrp`, `tcgetpgrp` and `setpgid`, plus 15
  `TIOCSPGRP` ioctls.
- No T1/T2 program uses process groups or `tcsetpgrp` (`callsite-analysis.md` → feature 4).
- The only tty request made outside job control is the window-size query: 552 `TIOCGWINSZ` calls.

### CP-03 / X-13: no superuser, no privilege change on exec

- The only privilege changes in T0–T2 code are the set-uid drops at startup of the full toybox and busybox binaries
  (`setuid`, `setgid`, `setresuid`, `setresgid`; 12 M3 sites, all `direct`, removable by a config switch).
- busybox's drop ran in every busybox applet call as a no-op. No workload needed a privilege change.
- Root checks (`geteuid() == 0`) select behaviour in toybox and dash's prompt. They are `direct`.
- **Shown:** these programs do not depend on set-uid or a superuser. **Not shown:** anything about recovery paths
  (X-13 proper).

### X-8: strict names vs POSIX byte names

- Not measured. No workload exercised non-UTF-8 or control-character names.

## Open questions the data frames but cannot answer

1. Does a launcher-built namespace keep enough of the capability benefit to justify CP-01 for POSIX processes (X-1)?
2. Which of the three shell options for X-2 has the lowest total cost: fork emulation for one program, a
   restructured dash, or another shell? Each needs its own measurement.
3. Parallel make (`-j`): jobserver pipes, `pselect`, signal forwarding on interrupt. These were unexercised and are
   the main gap in X-2, X-3 and X-4.
4. Memory cost of shell forks under strict commit (X-6).
5. tcc + autoconf friction that is not POSIX. A program called `ld`, `ar`, and 22 configure checks differ from gcc
   (`results/configure-checks.md`). These matter for T2 regardless of the OS design.

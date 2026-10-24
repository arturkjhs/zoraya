# TASK-02: call-site analysis (M3) and validation patches (M4)

Every compiled call site of the conflict-critical features in the T0–T2 programs. For comparison, the alternative
shells (oksh, mrsh) and pdpmake are included as well. All 663 sites are in [`callsites.csv`](callsites.csv), and the
generated tables in `tools/posix-surface/results/callsite-tables.md`. The extraction and review process is described
in [`method.md`](method.md) → M3.

**Classes and effort are estimates.** They come from reading the code. Each program had one reviewer, and the
coordinator handled the 36 late sites: 23 after the extractor fix and 13 after the independent review (see below). They are not measurements. Effort is in estimated changed lines; for
`emulate` it is 0 by definition. M4 at the end calibrates two of them.

Scope notes:

- A site counts only if its line is in the DWARF line table of the measured binary, at an address in an executable
  section. Unconfigured code and code removed by `--gc-sections` are excluded.
- For toybox and busybox the binary is the T2 minimal build. Only the applets the workloads ran are covered.
- The workloads ran the *full* toybox and busybox binaries. Their shared startup code (toybox `main.c`, busybox
  `libbb/appletlib.c`) runs on every applet call but is compiled out of the minimal builds. For those two files, the
  full binaries' line tables are used too. The independent review found this gap; it added 13 sites.
- busybox in T0–T2 is a fallback for five applets (`tr`, `diff`, `awk`, `expr`, `gzip`). Its call sites cover those
  applets only.

## Overview

| Program | Kind | direct | restructure | emulate | n/a | direct lines | restructure lines |
|---|---|---|---|---|---|---|---|
| dash | core | 26 | 1 | 33 | 1 | 54 | 60 |
| toybox | core | 87 | 12 | 46 | 1 | 167 | 330 |
| busybox | core | 26 | 0 | 0 | 5 | 49 | 0 |
| tcc | core | 12 | 9 | 0 | 1 | 27 | 48 |
| gmake | core | 70 | 8 | 16 | 1 | 127 | 90 |
| oksh | comparison | 76 | 42 | 72 | 17 | 96 | 421 |
| mrsh | comparison | 38 | 2 | 32 | 1 | 92 | 81 |
| pdpmake | comparison | 21 | 4 | 3 | 0 | 61 | 20 |

By feature, all programs (core programs in parentheses):

| Feature | direct | restructure | emulate | n/a |
|---|---|---|---|---|
| 1 process | 51 (40) | 8 (6) | 13 (3) | 1 (0) |
| 2 signals | 27 (16) | 66 (20) | 41 (15) | 2 (2) |
| 3 global path/cwd | 34 (26) | 2 (2) | 24 (14) | 5 (5) |
| 4 tty/job control | 21 (13) | 0 (0) | 47 (6) | 5 (0) |
| 5 identity/permissions | 43 (25) | 0 (0) | 62 (46) | 0 (0) |
| 6 (TASK-01 design mistakes, all IDs) | 180 (101) | 2 (2) | 15 (11) | 14 (2) |

The rule-based first pass was changed by review on 202 of 663 sites. Sites whose class was `emulate` concentrate
in two places:

- the **shells**: process creation, job control and signal semantics;
- **toybox's file-metadata applets**: `chmod`, `chown`, `cp -p`, `install`, `id`, `ls -l`, which by definition
  operate on POSIX modes, owners and the passwd/group databases.

## Feature 1: process creation, exec, wait, pipes

Core programs: 40 direct, 6 restructure, 3 emulate.

- **make never forks.** GNU make already uses `posix_spawn` (2 sites, direct as-is), `execvp` in the child path
  (2), `pipe` (3) and `wait` (1). All are direct. pdpmake runs every recipe line through `system()`. That is direct,
  and M4 replaced it (below).
- **tcc** has no process creation in the paths compiled for these workloads.
- **dash**, which the T0–T2 workloads use, has three sites that need the compat layer, plus one restructure:
  - `forkshell` (`src/jobs.c:982`, `fork`), **emulate**. It forks for subshells, pipelines, background jobs, command
    substitution and the heredoc writer. The child continues running shell code in a copy of the shell's memory.
    That is the fork-without-exec case.
  - `waitproc` (`src/jobs.c:1209`, `wait3` with `WUNTRACED`), **emulate**. It reaps any child, including stopped
    ones, for the job table.
  - `tryexec` (`src/exec.c:163`, `execve`), **emulate**. It is reached from the forked child and from the `exec`
    builtin. Its ENOEXEC fallback runs `/bin/sh`.
  - `vforkexec` (`src/jobs.c:1002`), **restructure, about 60 lines**. The rule said direct. The reviewer doubted it
    and picked the harder class because of job-control setup in the child. M4 tested this.
- **toybox**: 6 restructure, rest direct. Its child creation goes through `xpopen_setup`, `xfork` and `XVFORK`.
  - Some uses re-run the same applet in the child with `argv == NULL`. For example, `cpio -p` and `tar` fork a copy
    of themselves as the other end of a pipe. Under vfork, toybox re-execs `/proc/self/exe` for this.
  - These need restructuring into an explicit helper mode, estimated at 40–50 lines each.
  - The only toybox process creation that the workloads actually executed is xargs' `XVFORK` (direct, followed by
    exec).
- **The comparison shells behave the same way.** oksh and mrsh fork for every subshell, pipeline element, `&` list
  and command substitution: all emulate. mrsh's external-command fork is restructure (about 80 lines), like dash's.
- **Pipes between processes** are plain `pipe` + `dup2` at every site. All are direct; the pipe is not the problem.
  What creates the other end is.

## Feature 2: signals

Core programs: 16 direct, 20 restructure, 15 emulate. Signals named at the site are in
`callsite-tables.md` → "Signal sites by signal name". Most sites name no signal: they are mask operations and
generic wrappers.

Why programs use signals, by purpose:

| Purpose | Where | Class |
|---|---|---|
| Child exit (`SIGCHLD`) | dash: SIGCHLD always caught, `sigsuspend` in `waitproc`. GNU make: SIGCHLD handler wakes the jobserver's `pselect`. oksh: handler reaps children | dash emulate; gmake restructure; oksh restructure (`j_waitj` sigsuspend estimated at 300 lines) |
| Interrupt (`SIGINT`) and death-by-signal reporting | dash `onint` resets and re-raises SIGINT; `waitforjob` re-raises when the foreground job died of SIGINT. oksh `quitenv` re-raises to its pgrp | emulate. The parent must observe "terminated by signal N" |
| Fatal-signal cleanup | GNU make `fatal_error_signal` (delete partial targets, forward SIGTERM, re-raise). pdpmake `make_handler` (HUP/TERM). toybox `exit_signal` (atexit list on any fatal signal) | restructure, about 15–30 lines each. Needs some "on termination request" hook |
| Fault capture | tcc `-run` catches SIGFPE/SIGILL/SIGSEGV/SIGBUS/SIGABRT to backtrace JIT code | restructure. Not exercised by W1–W4; tcc compiles to files there |
| Job control (`SIGTSTP`, `SIGTTIN`, `SIGCONT`, `SIGHUP` to jobs) | dash `setjobctl`/`restartjob`; oksh and mrsh equivalents | emulate |
| `kill` builtin | dash, oksh: user-chosen signal to pid or pgrp | emulate. The user names the signal |
| Timeout | oksh `TMOUT` with `alarm` | restructure |
| Masking around critical sections | dash `sigblockall`/`sigclearmask`; oksh blocks SIGCHLD around job-table edits (about 30 sites) | dash emulate; oksh restructure, 2 lines each |
| Inherited dispositions | dash `setsignal` and oksh/mrsh/pdpmake query the inherited disposition to honour "ignored on entry" | emulate in shells (POSIX requires it for `trap`); restructure in pdpmake |

Dynamic evidence (M2, `posix-surface.md`): in T0–T2 core runs, handlers are actually installed for `SIGCHLD`,
`SIGINT`, `SIGHUP`, `SIGTERM` and `SIGPIPE` (dash, make), the make fatal set, and `SIGUSR1`/`SIGUSR2` (toybox). Most
`rt_sigaction` traffic is musl resetting dispositions in spawned children.

## Feature 3: global paths, `$PATH`, cwd

Core programs: 26 direct, 2 restructure, 14 emulate, 5 n/a.

- **`/bin/sh`.** GNU make and pdpmake hard-code it for recipes. Shebangs and dash's ENOEXEC fallback use it too.
  - The source sites are the `_PATH_BSHELL`/`default_shell` constants, plus `tryexec` (see feature 1).
  - Dynamically, 7 + 136 executions by GNU make and 469 by pdpmake (`posix-surface.md`).
  - Replacing the constant is direct. Shebang lines in third-party scripts are data, not code, so a compat layer
    has to resolve `#!/bin/sh` itself.
- **`/dev/null`.** toybox (3 sites) and busybox (2) use it, all direct: open a null handle.
- **`/dev/tty`.** toybox (2) uses it, emulate: it means "the controlling terminal".
- **`/dev/urandom`**, toybox, direct.
- **`/dev/stdin`, `/dev/stdout`, `/dev/stderr`**, busybox, n/a. These are names compared as strings, never opened.
- **Temp files.**
  - `mkstemp` and `tmpfile` are direct in GNU make, busybox and toybox, because the caller only needs a private
    file.
  - toybox's `mktemp` applet has the emulate cases: `mkstemp`, `mkdtemp` and a default of `/tmp`. Creating and
    printing a temp path is that applet's whole job.
  - Dynamically, `openat` with a `/tmp` prefix happened 9 times.
- **`/etc/passwd`, `/etc/group`**, toybox (`id` and friends), direct: read through a lookup API instead.
  `getpwnam`/`getgrgid` are counted under feature 5.
- **`/etc/profile`**, dash, direct. **`/etc/busybox.conf`**, busybox set-uid policy file: direct, compiled out with
  set-uid support.
- **cwd dependence.**
  - GNU make's `chdir` for `-C`, `getcwd` for `CURDIR`, and `realpath`: emulate (6 sites).
  - dash's `cd`/`pwd` (`chdir`, `getcwd`): emulate, since a per-process cwd is part of the language.
  - tcc: `getcwd` for the compilation directory in debug info and coverage output, and `realpath` to compare
    files for `#pragma once`. All direct.
  - toybox: `fchdir` is direct; its `xchdir`/`xgetcwd` wrappers are restructure (10–40 lines).
- **`$PATH` search.**
  - The shells walk `$PATH` themselves. dash's `tryexec` is emulate, as above.
  - toybox has one direct `PATH` lookup (tar looking for an external decompressor) and one emulate lookup:
    `env` must search `$PATH` for the command it runs.

## Feature 4: TTY and job control

Core programs: 13 direct, 6 emulate. All 6 emulate sites are in dash; oksh and mrsh add 41 more.

- **dash job control** is entirely emulate: `tcgetpgrp`/`getpgrp`/`setpgid` in `setjobctl`, `setpgid` in
  `forkchild`/`forkparent`, and `tcsetpgrp` in `xtcsetpgrp`. These are the process-group and foreground-terminal
  semantics themselves. W5 exercises them: background job, `jobs`, Ctrl-C of a foreground job.
- **Line editing.** oksh saves and sets raw mode with `tcgetattr`/`tcsetattr`; mrsh saves and restores modes per
  job.
- **direct** sites are `isatty`, plus dash's `tcgetattr` probe for input buffering. In dash, `isatty` serves
  `test -t`, `read -p` and deciding interactive mode. toybox and oksh query the window size (`TIOCGWINSZ`, 552 calls
  dynamically). They need a handle-type or size query, not tty semantics.
- **Non-shells.** No T1/T2 program (make, tcc, toybox build applets) uses process groups or `tcsetpgrp`. Job control
  is purely a T0 interactive-shell feature.

## Feature 5: identity and permissions

Core programs: 25 direct, 46 emulate.

- **toybox** accounts for most of it. The emulate sites are `chmod`/`fchmod`/`fchmodat`, `fchown`/`lchown`/`fchownat`,
  `getpwnam`/`getpwuid`/`getgrgid`/`getgrnam`, and `getgroups`, in `chmod`, `cp -p`, `install`, `ls -l`, `id` and
  `tar`.
  - These applets exist to manipulate or print POSIX owners and modes. They are emulate by definition, not because
    of how they are written.
  - The direct sites are root checks (`geteuid() == 0`) and `access` probes.
- **Set-uid support at startup** (toybox `toy_init`, busybox `parse_config_file`/`check_suid`), 12 sites:
  - `getuid`, `geteuid`, `getgid`, `setuid`, `setgid`, `setresuid`, `setresgid`, plus `getgrgid`/`getpwnam` for
    `/etc/busybox.conf` policy entries.
  - All direct, 1 line each (estimate). The config switch that the minimal builds already use removes them.
  - They only matter for binaries installed set-uid, which CP-03 rules out natively.
  - busybox's `setgid`/`setuid` drop was executed in the workloads, as a no-op (dynamic evidence).
- **GNU make**:
  - `umask` (4 sites, direct);
  - `getpwnam` and `getlogin` for `~` / `~user` expansion in makefiles and globs (6 sites, emulate);
  - `eaccess`, an `X_OK` probe on `$PATH` candidates when a recipe runs without a shell (2, emulate).
- **dash**: `umask` builtin (3, emulate), `faccessat` for `test -r/-w/-x` (emulate), `getpwnam` for `~user`
  (emulate), `geteuid`/`getegid` for `test -O/-G` (emulate) and the root prompt (direct).
- **Dynamic evidence.** `umask` was called 4,336 times (`syscall-surface.csv`): dash's umask builtin, run by
  configure scripts. `fchmodat` was called 4 times (toybox `chmod`/`install`). In W4, `configure` and
  `make install`-style steps exercise modes.

## Feature 6: other TASK-01 design mistakes in the data

| DM | What | Sites (core) | Class |
|---|---|---|---|
| DM-060 | non-reentrant / static-buffer APIs: `strerror` 86, `localtime` 11, `readdir` 5, `strtok` 4, `getlogin` 3, `ctime` 2, `getpwent`, `ttyname` | 113 (49) | almost all direct (109); swap to the reentrant variant |
| DM-085 | environment as process-global mutable state (`getenv`/`setenv`/`putenv`) | 34 (20) | direct except one toybox site. pdpmake's `system` patch also gives an explicit envp |
| DM-065 | wall-clock time used where monotonic is meant | 26 (14) | direct (15); 11 n/a in oksh |
| DM-039 | symlink-following path semantics, `lstat`/`readlink` races | 20 (16) | toybox `symlink`/`symlinkat`/`lchown`: emulate (8); the rest direct |
| DM-097 | global locale | 9 (9) | direct |
| DM-038 | `rename` atomicity assumptions | 3 (3) | direct |
| DM-053 | untyped `ioctl` | 3 (2) | direct: `TIOCGWINSZ` window size (toybox, oksh), `BLKGETSIZE64` (toybox) |
| DM-036 | `fcntl` record locks | 2 (2) | GNU make `--output-sync` lock, direct; never called dynamically (no lock command in strace) |
| DM-023 | `select`/`poll` readiness on fd sets | 1 (1) | GNU make jobserver `pselect` with SIGCHLD atomically unblocked: restructure, about 60 lines |

## M4: validation patches

The two patches are in `tools/posix-surface/patches/m4/`. Both were built and rerun on W1, W2 and W4 in both
measurement modes (`remote/m4.sh`). The comparison is generated in `tools/posix-surface/results/m4/m4-compare.md`.

| Patch | M3 estimate | Actual patch | Behaviour |
|---|---|---|---|
| **pdpmake** `make.c:103` `docmds`: `system()` → `posix_spawn` of `/bin/sh -c` with an explicit envp + `waitpid` | direct, 12 lines | 34 lines added, 1 removed | identical outputs, exit statuses and `config.h` |
| **dash** `src/jobs.c:1002` `vforkexec`: `vfork`+`execve` → `posix_spawn` | rule: direct; review: restructure, about 60 lines | 52 lines added. Covers only the non-interactive, no-job-control case. Interactive shells keep `vfork`, because `tcsetpgrp` in the child has no `posix_spawn` equivalent. Any spawn failure falls back to the old path, to keep diagnostics and 126/127 | identical outputs, exit statuses, `config.h` and `config.mak` |

What the calibration shows (estimate):

- **The pdpmake patch was about 3× the estimate.** The extra lines were not about the call itself. They were waiting
  and status decoding (which `system()` did implicitly), building `argv` for `sh -c`, and passing the environment
  explicitly. A `direct` estimate counts the call swap. The surrounding contract (status, environment, signals
  ignored during the wait) roughly triples it.
- **The dash patch was about the estimated size**, but only by narrowing scope. A complete replacement would also
  need the job-control child setup: process group, `tcsetpgrp`, signal resets, and `/dev/null` stdin for background
  jobs. `posix_spawn` attributes cover `SETPGROUP` and `SETSIGDEF` but not handing over the terminal. The reviewer's
  choice of restructure was the right call; the rule's `direct` was not.
- **Moving to `posix_spawn` does not shrink the kernel surface under musl** (`m4-compare.md`, strace, all
  processes):
  - The `clone` count is unchanged: W4 13,392 → 13,392. musl's `posix_spawn` is itself a
    `CLONE_VM|CLONE_VFORK` clone.
  - `pipe2` goes up: W4 +6,789, because musl uses a pipe to report exec failure.
  - `rt_sigaction` goes up: W4 +57,966, because musl's child resets every signal.

  The gain is at the API level: the program no longer runs code in a child sharing its memory. Whether that matters
  depends on how the compat layer implements `posix_spawn`, not on musl.
- **The remaining `fork` is the shell's.** After both patches, W4 still has dash calling `fork` in 363 process
  images, the same as before. Those are subshells, pipelines and command substitution (`forkshell`, emulate).

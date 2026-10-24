# TASK-02: POSIX surface of a minimal C toolchain

Summary of M1 (libc API surface) and M2 (Linux syscall surface) for
[TASK-02](../TASK-02-posix-surface-measurement.md). Every number here is copied from a file the pipeline writes
(`tools/posix-surface/run.sh`): `tools/posix-surface/results/summary-tables.md` unless another file is named. Per-symbol
and per-syscall rows are in [`libc-surface.csv`](libc-surface.csv) and [`syscall-surface.csv`](syscall-surface.csv).
How each number is produced, and its limits, is in [`method.md`](method.md). Call sites (M3) and patches (M4) are in
[`callsite-analysis.md`](callsite-analysis.md).

## Reading the two bounds

- **Upper bound (static).** The libc functions that the program's own objects import. It includes dead code, error
  paths and unused features, so it overestimates.
- **Lower bound (dynamic).** The libc functions that an `LD_PRELOAD` interposer saw called from program code (not from
  inside musl) during W1–W6. It covers only exercised paths, so it underestimates.
- **Full vs. minimal multi-call builds.** toybox and busybox are multi-call binaries. Their defconfig builds import
  every applet's needs (toybox 327, busybox 393). Per tier, a minimal build containing only the applets that the
  tier's workloads ran gives a tighter upper bound. That bound is still tied to these workloads.

Conclusions below say which bound they rely on.

## Tiers as measured

Tier membership is taken from the data. A tier is made of the programs observed in its core runs, cumulative from
T0 to T2.

| Tier | Workloads | Programs observed |
|---|---|---|
| T0 | W5 (interactive dash session) | dash, toybox (cat, ls, sleep, touch), busybox tr |
| T1 | + W1 tcc self-build, W2 Lua, W3 SQLite | + GNU make, tcc, more toybox applets, busybox diff |
| T2 | + W4 GNU hello `./configure && make` | + 25 more toybox applets, busybox awk/expr/gzip |
| T3 | W6 alone | clang, ld.lld |

All core workloads pass their output checks (`W*-check`, exit 0). pdpmake can build Lua (W2) and hello (W4p) but not
tcc (W1), because tcc's Makefile needs GNU make. The smallest T1 measured is therefore built on GNU make, with
pdpmake as a comparison.

## libc symbols per tier (M1)

| Tier | Upper, full binaries | Upper, minimal multi-call builds | Lower (dynamic) | Gap (min upper − lower) |
|---|---|---|---|---|
| T0 | 485 | 166 | 84 | 82 |
| T1 | 518 | 250 | 134 | 116 |
| T2 | 518 | 293 | 189 | 104 |
| T3 (alone) | 333 | 333 | 89 | 244 |

musl 1.2.5 exports 1,613 functions (and 30 data symbols, `results/musl-exports.tsv`). For scale, the smallest surface that runs T1
is **134 functions observed, at most 250 imported** (minimal builds). Measured against these workloads, that is
roughly 8–16 % of musl's exported functions.

Every dynamic hit also appears in the same program's static imports (consistency check, `summary-tables.md`). A few
symbols were observed but are not in the minimal imports: busybox's setuid handling, `getopt_long` and `getuid`, and
toybox's root check. They come from shared startup code that the full binaries contain and the minimal builds drop.
They are listed in `summary-tables.md` and counted in the lower bound.

### Per category (min upper / lower)

| Category | T0 | T1 | T2 | T3 |
|---|---|---|---|---|
| stdio | 22 / 10 | 43 / 24 | 50 / 42 | 36 / 7 |
| string | 45 / 18 | 53 / 29 | 63 / 39 | 64 / 17 |
| process | 32 / 19 | 51 / 30 | 58 / 39 | 43 / 13 |
| signal | 10 / 7 | 12 / 9 | 12 / 9 | 9 / 5 |
| file | 20 / 9 | 40 / 15 | 49 / 25 | 48 / 10 |
| dir | 6 / 5 | 7 / 5 | 10 / 9 | 8 / 4 |
| time | 8 / 1 | 11 / 2 | 12 / 3 | 10 / 0 |
| locale | 4 / 2 | 4 / 2 | 4 / 4 | 9 / 0 |
| tty | 5 / 5 | 6 / 5 | 6 / 5 | 2 / 1 |
| memory | 4 / 3 | 6 / 4 | 6 / 5 | 14 / 7 |
| threads | 0 / 0 | 3 / 3 | 3 / 3 | 33 / 19 |
| network | 1 / 0 | 1 / 0 | 4 / 0 | 12 / 0 |
| other | 9 / 5 | 13 / 6 | 16 / 6 | 45 / 6 |

Categories come from a header scan plus a hand-written override table (`analysis/categories.py`), so the split
between categories is partly a choice. The totals are not.

### Jumps between tiers

- **T0 → T1:** +84 minimal upper, +50 lower. These are mostly stdio, string, file and process functions for the
  compiler and make. Three thread functions appear at T1: tcc calls `sem_init`, `sem_post` and `sem_wait` for its internal compile lock (`libc-surface.csv`). No thread
  is ever created by T0–T2: strace shows zero thread clones outside T3.
- **T1 → T2:** +43 minimal upper, **+55 lower**. The lower bound grows more than the upper because `configure`
  exercises paths that the T1 programs already import but W1–W3 never ran. The new functions are listed in
  `summary-tables.md`. They are mainly:
  - more stdio (`fprintf`, `getline`, `getdelim`, the `*_unlocked` family);
  - temp files and directories (`mkstemp`, `mkdtemp`, `mkdir*`, `rmdir`, `symlink`);
  - regex (`regcomp`, `regexec`, `regfree`, from toybox grep/sed);
  - identity lookups (`getpwuid`, `getgrgid`, `getegid`);
  - locale objects (`newlocale`, `uselocale`);
  - process functions (`execvp`, `waitpid`, `dup`);
  - `memfd_create` and `syscall`, from toybox.
- **T3 vs T1:** clang+lld alone imports 333 (more than T1's 250), but only 89 were observed (fewer than T1's 134).
  Of T3's observed functions, 45 are not in T1's lower bound: threads 19, process 8, string 6, file 4, memory 3,
  other 3, signal 2. **Threads are what separates T3 from T1–T2.** ld.lld creates 4 threads in W6; nothing in
  T0–T2 creates any.

### Comparison programs (lower bound, same workload)

| Comparison | Programs | Dynamic libc functions |
|---|---|---|
| W5 shell session | dash / oksh / mrsh / busybox sh | 54 / 66 / 58 / 73 |
| make process, W2 Lua | GNU make / pdpmake | 61 / 30 |
| make process, W4 hello | GNU make / pdpmake | 66 / 31 |

pdpmake uses about half the libc surface of GNU make for the same builds. The cost is W1, which it cannot run.
dash has the smallest interactive-shell surface of the four.

## Linux syscall surface (M2)

This is what musl and the programs need from the kernel. It is not a proposal for our ABI.

| Tier | Distinct syscalls in this tier's runs | Cumulative |
|---|---|---|
| T0 | 39 | 39 |
| T1 | 43 | 49 |
| T2 | 55 | 59 |
| T3 (alone) | 35 | — |

69 distinct syscalls appear across all runs, including comparison runs (`syscall-surface.csv`).

Argument patterns that define semantics (core and comparison runs, `syscall-surface.csv` → `notable_args`):

- **`clone`.** 2,328 vfork-style (`CLONE_VM|CLONE_VFORK|SIGCHLD`), 1,710 fork-style (`SIGCHLD` only), and 4 threads,
  all from ld.lld. The fork-style clones come from the shells (dash: 1,690 in T2 core runs). vfork-style clones come
  from dash's own `vfork` for simple commands, toybox xargs (`XVFORK`), and musl's `posix_spawn`/`system`, which GNU
  make, clang and pdpmake use (`libc-surface.csv`, rows `vfork`, `posix_spawn`, `system`).
- **`execve`.** 3,002 succeeded and 7 failed. 2,922 used an absolute path, because musl's `execvp` and the shells'
  own `$PATH` walk resolve the name in user space. Only 87 used a relative path.
- **`openat`.** 19,005 relative paths and 13,921 absolute ones, with prefixes `/usr` 5,886, `/dev` 159, `/lib` 28,
  `/etc` 11 and `/tmp` 9. Flags are overwhelmingly `O_RDONLY`, then `O_WRONLY|O_CREAT|O_TRUNC` (1,317) and
  `O_APPEND` (393). `O_EXCL` appears 3 times (temp files).
- **`fcntl`.** `F_SETFD` (5,360) and `F_DUPFD_CLOEXEC` (3,894) dominate, that is, close-on-exec bookkeeping around
  redirections and spawns. `F_GETFL`/`F_SETFL` are rare (39 and 25). No record locks were used.
- **`ioctl`.** Only tty requests: `TIOCGWINSZ` (552), `TCGETS` (12), `TIOCSPGRP` (15) and `TIOCGPGRP` (1).
- **`rt_sigaction`.** Overwhelmingly resets to `SIG_DFL`, mostly in spawned children before exec: musl's
  `posix_spawn` resets every signal. Handlers installed per program are listed in `summary-tables.md`. T0–T2 install
  handlers for `SIGCHLD`, `SIGINT`, `SIGHUP`, `SIGTERM` and `SIGPIPE` (dash, make), plus `SIGQUIT`, `SIGUSR1`,
  `SIGXCPU` and `SIGXFSZ` (GNU make, for its fatal-signal cleanup), and `SIGUSR1`/`SIGUSR2` (toybox). clang and lld
  install handlers for 15 signals: crash reporting.
- **`wait4`.** Options: `0` (4,030), `WNOHANG` (3,307), and `WSTOPPED` (16, alone or with `WNOHANG`). `WSTOPPED`
  comes only from interactive job control.
- **`mmap`.** Anonymous private memory dominates. `MAP_SHARED` appears twice (clang).

### Absolute paths chosen by programs

These executions name an absolute path rather than going through `$PATH` (strace, `summary-tables.md`):

- GNU make and pdpmake run every recipe with `/bin/sh`: 7 times in GNU make's W1–W3 runs, 136 in W4, and 469 in
  pdpmake's W4p.
- In W4, scripts run `/bin/sh` 92 times through `#!/bin/sh`.
- `config.guess` probes `/bin/uname`, `/bin/arch` and seven paths that do not exist (`/usr/convex/getsysinfo`
  and others).
- clang runs `/usr/bin/ld.lld`.

No core run executed anything outside the measured sets.

## Key observations

1. **The T1 surface is small and bounded.** Self-hosting C without autoconf needs 134 to 250 libc functions under
   these workloads. The interval is between the dynamic and the minimal-static bound.
2. **autoconf is a bigger step in exercised surface than in imported surface.** T1→T2 adds 55 observed functions but
   only 43 imports. This rests on both bounds.
3. **T0–T2 are single-threaded, and T3 is not.** Thread functions and thread clones appear only in T3 (strace and
   dynamic data). The 3 thread functions observed at T1 are tcc's semaphores. They synchronise nothing, because no second thread exists.
4. **fork is a shell matter.** All 1,710 fork-style clones come from shells (dash in core runs, others in
   comparison runs). make, tcc, toybox and busybox create processes vfork-style or through `posix_spawn`. The shell
   forks for subshells, pipelines and command substitution (M3).
5. **Signals in T0–T2 are mostly resets and cleanup.** Handlers exist for child exit (`SIGCHLD`), interrupt
   (`SIGINT`) and fatal-signal cleanup (make). The bulk of `rt_sigaction` traffic is musl resetting dispositions in
   spawned children.
6. **Global paths are few but load-bearing.** `/bin/sh` is hard-coded in both makes and in shebangs. `/dev/null`,
   `/dev/tty`, `/tmp`, `/etc/passwd` and `/etc/group` show up in toybox and the shells. Details and replaceability are
   in `callsite-analysis.md`.
7. **The tcc + autoconf combination has friction that is not about POSIX.** hello's `configure` needs a program
   called `ld` and an `ar` (`research-log.md`). Of the checks that both a tcc and a gcc configure run performed, 22
   gave a different result (`results/configure-checks.md`):
   - Most are C11/C23 language features tcc lacks or reports differently: `static_assert`, `bool`, `max_align_t`,
     `stdckdint.h`, a C99-conforming `stdint.h`, `alloca` as a builtin, and GNU C.
   - The rest are tool choices, plus one platform fact: with tcc, `config.guess` reports
     `aarch64-unknown-linux-gnu` on musl.

   None of these blocked the build.

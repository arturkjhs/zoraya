# M3 review notes: dash, mrsh, tcc (reviewer: shells1)

Every class is an estimate. Counts are rows in `m3-shells1.tsv`.

## dash (61 sites: 26 direct, 1 restructure, 33 emulate, 1 n/a)

- The hard classes come from three clusters. (1) `forkshell` (jobs.c:982) is the one fork behind subshells, pipelines, `&`, command substitution and the heredoc writer. (2) Job control: setjobctl, forkchild/forkparent, xtcsetpgrp and `fg`/`bg`/`kill`, i.e. pgrp, tcsetpgrp, killpg and SIGTTIN/SIGCONT. (3) The trap/signal machinery: setsignal, sigblockall/sigclearmask, and `waitproc` using wait3(WUNTRACED) plus sigsuspend on SIGCHLD or a trapped signal.
- If the compat layer provides fork, process groups with a controlling tty, and POSIX signal delivery, it covers about 30 of the 33 emulate rows. cd/getcwd, umask, `~user` (getpwnam) and `test -O/-G/-r` need the cwd, umask, user database and permission model.
- The only spawn-shaped site is `vforkexec` (jobs.c:1002). I marked it restructure (~60 lines), not direct: the forkchild side effects (signal reset/ignore, setpgid, tcsetpgrp, `/dev/null` stdin for background jobs) and the ENOEXEC→/bin/sh fallback all have to move into spawn attributes or into the parent. Without job control it gets close to direct.
- On this build, the heredoc-writer fork (redir.c:378–384) is a fallback that runs only if memfd_create fails (USE_MEMFD_CREATE=1). It is direct and close to dead code.
- Most rows are strerror (DM-060) and isatty. They are trivial direct changes and are not the problem.
- Rule corrections: input.c:138 is a tcgetattr *query* (direct, not emulate). cd.c:283 getcwd is a string-literal false positive.

## mrsh (73 sites: 38 direct, 2 restructure, 32 emulate, 1 n/a)

- Same pattern as dash, spread over more call sites. There are separate forks for subshell (task.c:18), `&` (task.c:342), pipeline element (pipeline.c:62) and `$(...)` (word.c:97), plus job.c job control with termios save/restore per job and trap.c sigaction. The same fork + pgrp/tty + signal emulation covers them all.
- mrsh does more in the fork child of an external command than dash does (simple_command.c:55): pgrp and foreground, setenv of exported variables, opening redirections, then execv. Restructure is ~80 lines, because redirections and env would have to move into the parent for a spawn.
- Easy wins: `command` builtin (command.c:73, a plain fork+execv with no child setup), the heredoc writer fork (redir.c:71), `/dev/null` stdin (task.c:29/357), the setuid check (entry.c:152), access() pre-checks.
- Rule corrections: job.c:244 `waitpid` is a string-literal false positive. wait.c:87 waits on own children by pid with no WUNTRACED, so it is direct, but it needs a pid→handle mapping.

## tcc (22 sites: 12 direct, 9 restructure, 0 emulate, 1 n/a)

- No processes, job control or async signals. The only restructure is the `-run` fault handler (tccrun.c:1354–1381): SIGSEGV/FPE/ILL/BUS/ABRT with ucontext, used to print a backtrace of JIT code. These are synchronous faults, not X-2-style async signals. It needs a native exception-handler primitive, and becomes emulate if there is none.
- Global-path items are local: realpath for `#pragma once` (compare file identity instead), and getcwd used only to build names for -g and -ftest-coverage. The rule said emulate for the getenv(C_INCLUDE_PATH...) rows; they are direct. The include/library search directories are a global-path issue, but no site in this list covers them.

## M4 validation-patch candidates

1. **mrsh builtin/command.c:73+78**: replace fork+execv in `command` with posix_spawn plus a wait (~10 lines). Test: rerun a workload that uses `command ls` / `command -p`.
2. **mrsh shell/redir.c:42+71**: replace the heredoc writer fork with a temp-file or memory-file handle (~15 lines). Test: a heredoc larger than PIPE_BUF (4096 bytes).
3. **mrsh shell/task/task.c:29/357** (backup): replace `open("/dev/null")` with a null handle or a pre-opened fd (~2 lines each). Test: `( cat )` and `cat &` in a non-interactive script must see EOF on stdin.

Calibration with more substance: dash jobs.c:1002 vforkexec→posix_spawn (estimated 60 lines). It is restructure, so it tests whether that estimate is too pessimistic.

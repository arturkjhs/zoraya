# M3 review notes: oksh (207 sites, --enable-small)

Counts: direct 76 (about 100 lines in total), restructure 42 (about 420 lines), emulate 72, n/a 17.

- **Emulate is driven by job control and fork.** `exchild` fork (jobs.c:444), execve (exec.c:382/719), and roughly 45 sites of pgrp/tcsetpgrp/termios/killpg in jobs.c, tty.c and edit.c. One compat-layer feature, "POSIX sessions + process groups + controlling tty + termios", covers almost all emulate rows. The remaining emulate rows are builtin semantics: `cd`/`pwd`/getcwd/readlink (cwd), `umask`, `kill`, `trap`/setsig, `test -h/-r/-G`, glob lstat, `~user` getpwnam/getpwent, and $PATH search `access`.
- **Restructure is one change.** 38 of the 42 rows are the SIGCHLD-handler job loop. Every job-table function wraps itself in `sigprocmask(SIG_BLOCK, &sm_sigchld)` because `j_sigchld` reaps with `waitpid(-1, WNOHANG|WUNTRACED)` in handler context. The core is the `sigsuspend` wait in `j_waitj` (jobs.c:1003). Turning it into an event loop over child handles plus trap events is about 300 lines. Doubt: as long as job control is emulated, the layer has to deliver SIGCHLD anyway, so this could reasonably be `emulate`. The other 4 rows are TMOUT `alarm` (lex.c:1087, trap.c:90/96), which becomes a read deadline (about 40 lines).
- **The rule pass over-counted in several places:**
  - All 10 `sigsetjmp(...,0)`/`siglongjmp` sites are plain setjmp-style exception unwinding. They save no mask and are never reached from a handler, so they are `direct`, with one macro.
  - The 6 CLOCK_MONOTONIC and 6 `time()` sites are correct clock use (n/a under DM-065).
  - `readdir` is n/a: its state is per DIR stream, not hidden global state.
  - 5 matches are inside warningf/bi_errorf format strings (n/a).
- **Rule results I made easier:** privilege drop (misc.c:297-301) and /etc/suid_profile only run when the shell starts setuid. Without setuid in the native model they are dead code (direct). `getlogin` and `ttyname` only feed the prompt (direct).
- **DM-060:** 33 `strerror`, 6 `localtime` and the getlogin/ttyname rows are trivial `_r` swaps. The shell is single-threaded, so today they cause no real hazard.

## M4 candidates
1. **io.c:456/464 + var.c:1005: here-doc temp files in /tmp go through a temp-dir capability** (about 5 lines). Test it by rerunning any workload that uses here-documents, then check strace for no `/tmp` openat outside the granted dir.
2. **jobs.c:510: background-job stdin `/dev/null` becomes a null handle** (2 lines). Test with `sh -c 'cat & wait'` in the non-interactive workload: it must still see EOF.
3. Optional: **main.c:368 / tty.c:43,45 / c_test.c:272: replace `isatty` with a handle-type query** (1 line each). Test with `test -t 0` and interactive detection under the workload's pty and non-pty runs.

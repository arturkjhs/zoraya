# M3 review notes: busybox (T2 build), GNU make, pdpmake

All classes and line counts are estimates. Counts are rows in `m3-bbmake.tsv`.

## Extractor issue (affects every program built with --gc-sections)

`callsites.py` keeps a line if it appears anywhere in `objdump --dwarf=decodedline`. In `busybox_unstripped`
the linker drops unused functions but leaves their line entries, relocated to addresses below `.text`
(0x2d20). 49 busybox rows are only in such dropped functions: `xfork`, `safe_waitpid`, `xpipe`, all of
`get_terminal_width_height`, the termios helpers, `xsetuid` and the other set-id wrappers, `xchroot`,
`chdir_or_warn`, the ioctl wrappers, `bb_signals`, `wait_for_any_sig` and others. I marked them `n/a`, with the
class they would have if linked in the note. Rows that are compiled out by a constant `if` (`setlocale` behind
`ENABLE_LOCALE_SUPPORT=0`) or that only match inside a string (gmake main.c:2016) are also `n/a`. gmake and pdpmake
have no dead-address rows. Suggested fix: drop line entries whose address is below the start of `.text`. The
toybox-T2 rows likely need the same check.

## busybox (117 rows: direct 30, emulate 33, n/a 54)

- `CONFIG_SH_IS_ASH=y` (the default) links ash in as the `sh` applet, although no T2 workload runs it. All 33
  emulate rows are ash, or libbb code that only ash reaches: fork for subshells and pipelines, waitpid(-1) plus
  sigsuspend, trap dispositions, cd, umask, getgroups for the PATH executability test, and termios for
  `read -s`. Building with `CONFIG_SH_IS_NONE` would remove them all. The rule's shell check (`"ash" in fn`)
  missed ash functions such as `forkshell`, so it classed them as non-shell.
- ash's `vforkexec` (simple foreground command) is spawn-shaped. It is direct (about 40 lines) because job
  control is configured out. The heredoc writer child (`write2pipe`) is a fork that a temp file can replace.
- The applets measured in T2 are clean. awk's `|` pipes use popen/pclose, which musl already implements with
  posix_spawn. diff uses `/dev/null` and `/tmp/difXXXXXX`. gzip only calls isatty. awk's `/dev/stdin` and
  `/dev/stdout` names are table keys and are never opened.
- One compat feature, signal emulation for the shell, covers all the ash signal rows.

## GNU make (95 rows: direct 70, restructure 8, emulate 16, n/a 1)

- It already uses posix_spawn. `USE_POSIX_SPAWN=1` in config.h, so `child_execute_job` (job.c:2312) calls
  posix_spawn with dup2 file actions, and the vfork branch (job.c:2340) is compiled out. That matches the strace
  data, which shows only vfork-like clones. Recipes and `$(shell)` (function.c:1919) both use this path.
- No fork path is left in the binary. The only remaining exec is in place: `exec_command`'s `execvp` re-execs
  make after it remakes makefiles (main.c:2817). It can become spawn, wait, then exit, which the `__EMX__` branch
  (main.c:2789) already does.
- The hard part is the SIGCHLD design. SIGCHLD stays blocked everywhere and is only unblocked inside `pselect`
  on the jobserver pipe (posixos.c:484). Its handler counts dead children, and `reap_children` calls
  `wait()`. Replacing this means one wait over the jobserver pipe and the child handles, about 60 lines. The
  fatal-signal handler, which cleans up and re-raises, is the other restructure (about 30 lines).
- The emulate rows are the cwd model (`-C`, `CURDIR`, chdir back), `$(realpath)`, tilde expansion through
  passwd, and the PATH search with `eaccess`. Per-process cwd plus a passwd view would cover all of them except
  realpath.
- The pattern scan missed sites behind macros: `FATAL_SIG` (bsd_signal), `WAIT_NOHANG` (waitpid), `TTYNAME`
  (ttyname), and `LOADAVG` (`/proc/loadavg`).

## pdpmake (28 rows: direct 21, restructure 4, emulate 3)

- Very simple. `system()` runs each recipe line, `popen` handles `!=` and `$(shell)`, and both are
  posix_spawn-based in musl. It never forks or waits itself.
- Most setenv rows pass MAKEFLAGS, macros, CURDIR or pragmas to sub-makes through `environ`. Once `system()`
  is replaced by a spawn with an explicit envp, each of these becomes one entry in that array.
- The only restructure is the HUP/TERM cleanup handler (main.c:432-448). The emulate rows are cwd-related:
  `-C`, `CURDIR`, and `realpath(argv[0])`.

## M4 candidates

1. **pdpmake make.c:103, `system(cmd)` to spawn `/bin/sh -c` with an explicit envp, then wait on the child**
   (about 12 lines). Every recipe takes this path, so any make workload tests it. It also prepares the envp that
   the setenv sites need.
2. **gmake main.c:2817 / job.c:2606, re-exec to spawn, wait, exit(status)** (about 10 lines). Test it with a
   workload whose makefile includes a generated file, which forces a restart and checks `MAKE_RESTARTS`.
3. **busybox editors/diff.c:733, `/tmp/difXXXXXX` to an anonymous temp file from a temp-dir handle**
   (about 3 lines). Test with `diff` reading a pipe (`cmd | diff - file`).
   Alternative: awk.c:3131/3353 popen to an explicit spawn helper (about 15 lines), tested by awk `"cmd" | getline`.

# M3 review: toybox (T2 minimal build, 171 sites)

Counts: direct 65, emulate 46, restructure 8, n/a 52.

- **The filter lets dead code through (52 n/a).** The build uses `--gc-sections`, but the discarded functions keep their DWARF line-table entries. As a result, 27 lib helpers with no T2 caller (reset_env, names_to_pid, read_password, update_password, set_terminal, xvdaemon, xsetuser, xpidfile, xtzset, xgetmountlist, ...) show up as sites. I checked each one with `nm generated/unstripped/toybox`. Each note gives the class the site would get if it were linked. `callsites.py` should also check that the enclosing function is in the symbol table. Busybox probably has the same problem.
- **Hard classes come from three sources.**
  1. `xfork` (portability.c:25) is the only fork. It does three things: fork+exec of an external command, fork+run a builtin in-process via `toy_exec`, and fork-self with `argv==0` (tar.c:1141, cpio -p). This is restructure, about 50 lines. Upstream's nommu path (`!CFG_TOYBOX_FORK`: vfork + re-exec of `/proc/self/exe`) can serve as the template. The workloads never reach it dynamically.
  2. sigatexit (lib.c:899, portability.c:456, xwrap.c:997) installs handlers on all fatal signals. Its only linked user is the sed -i temp-file cleanup.
  3. Process cwd: tar -C uses `xchdir` and then works relative to it, and `xabspath`/`getcwd` drive tar's check against extracting outside the target directory.
- **Most emulate sites are the applets' purpose.** They cover owner and mode restoration (cp -p, install, cpio, tar), `id`, user-db lookups, the umask, symlink creation, access()-based permission prompts (rm, find -readable), env's $PATH search, and mktemp/`/dev/tty`. One compat-layer feature set would cover about 35 of the 46: POSIX uid/gid/mode/umask on files plus a passwd/group database. Symlinks add 7 more. A wrong guess here is cheap.
- Direct sites are mostly isatty, `/dev/null`, locale setup, TZ juggling in xparsedate, and root checks via `geteuid`.
- Missed site: `XVFORK()` at xargs.c:225 is a macro for `vfork()`, so the pattern never matched it. It is the only process creation the workloads actually exercise (W4-configure: vfork, execvp, waitpid).

## M4 candidates
1. **xargs spawn**: change xargs.c:225-230 (XVFORK + xexec) and the `waitpid(-1)` at :109 to posix_spawnp plus waiting on the stored pids. About 15 lines. Rerun W4-configure.
2. **env**: change env.c:62-63 from `execve` to spawn + wait + exit with the child's status. About 10 lines. Rerun W4-make.
3. **main.c:285**: delete the NSIG `signal(i, SIG_DFL)` loop that runs on every applet start. 1 line. Any workload tests it; check that the strace shows no rt_sigaction calls.

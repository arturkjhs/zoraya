# TASK-02 research log

Log for [TASK-02](../TASK-02-posix-surface-measurement.md): what was tried, what failed and how it was handled, and
which questions remain open. Dates are 2026-10-20 to 2026-10-21. Numbers quoted in the other documents come from
`tools/posix-surface/results/` (machine-written by the pipeline); this log only describes process.

## Environment setup

- **Prerequisite VM missing.** No VM answered SSH on `localhost:2222`. The UTM "Linux" VM turned out to be an empty,
  never-installed disk with no ISO attached. The task says to stop in that case, so I stopped and reported. The
  human then asked me to set up the VM from the command line.
- **Lima instead of UTM.** `limactl start --name=alpine-posix template:alpine` created an Alpine 3.23.3 aarch64 VM:
  musl 1.2.5, Linux 6.18.7, 4 CPUs, 4 GiB of RAM, 20 GiB of disk, no host mounts, key-based SSH. Lima uses Apple's
  Virtualization.framework, not QEMU/HVF. The guest kernel and userspace are what the measurements depend on, and
  they are the same, so this deviation does not affect results. It is recorded in `method.md`.
- **Git identity.** At first, neither name nor email was configured. I stopped and asked, and the human set
  `user.email` in the repo. Git derives the author name from the macOS account, so commits on this branch are
  `Svitlana <nzamb7@gmail.com>`. No `-c user.*` override was used.

## Build problems and how they were handled

Fixes that affect only the build are recorded in `tools/posix-surface/remote/build.sh` and `patches/build/`. None of
them changes the libc usage being measured, except where noted.

| Program | Problem | Fix |
|---|---|---|
| all | `cp -R` reset timestamps, so automake tried to regenerate files | copy with `cp -Rp` |
| all | Alpine's fortify-headers wrap string functions; this broke toybox and could alias imports | `-U_FORTIFY_SOURCE` |
| oksh 7.9 | builds without curses fail because `x_redraw` uses curses symbols outside the `NO_CURSES` guard | `--enable-small`, which also drops extended history; oksh is only a comparison shell |
| toybox | build scripts need bash | bash is installed as a build-time host tool only |
| GNU make 4.4.1 | its bundled gnulib declares `getenv()` in pre-C23 style, which fails under GCC 15's C23 default | `-std=gnu17` |
| tcc mob | configure does not detect musl | `--config-musl` |
| tcc mob | on aarch64 Linux, tcc predefines `wchar_t` as `int`, but the psABI and musl use `unsigned int`; any file including both headers fails | `patches/build/tcc-aarch64-wchar.patch`, one hunk, also applied to the W1 self-build tree. This is an upstream portability bug |
| busybox | defconfig's `tc` applet fails against musl headers | disabled; busybox is a fallback and comparison program |
| pdpmake | the build produces a binary named `make` | installed as `pdpmake` |

## Measurement problems found and fixed

1. **The interposer broke clang.** clang aborted with `std::bad_alloc`. Cause: musl 1.2.5 disables its memalign
   family when `malloc` is interposed but `aligned_alloc` is not (`DISABLE_ALIGNED_ALLOC` in
   `src/malloc/mallocng/glue.h`; the decision is made in `ldso/dynlink.c`). Fix: `aligned_alloc` is always interposed
   as a pass-through. The self-test now covers `posix_memalign`.
2. **musl calls through the interposed malloc.** musl routes its own internal `malloc` calls through an interposed
   `malloc`. Logging them would have counted calls made inside libc as program calls. Fix: the trampoline passes its
   return address, and hits from inside musl's mapping are not logged. A symbol switches to the fast path only after a
   call from outside libc has been logged, so an early internal call cannot hide a later direct one.
3. **Absolute `/bin/sh` escaped the measured set.** GNU make and pdpmake run recipes with `/bin/sh` by absolute path.
   Scripts with a `#!/bin/sh` shebang, and `config.guess` (`/bin/uname`, `/bin/arch`), do the same. In the first full
   run, 1,164 shell executions in W4 ran Alpine's busybox ash, not dash. Fix: every workload runs in an unprivileged
   user and mount namespace in which `/bin` is a per-set directory. Names the measured set provides point at measured
   binaries; every other name falls through to the original `/bin`. The workload then drops back to the real uid in a
   nested user namespace (`remote/ns_exec.sh`), so identity-dependent behaviour is unchanged. Absolute-path executions
   are still counted, as a separate table, because they are an M3 finding (feature 3).
4. **busybox's `unshare -r` makes the process uid 0.** That would change identity behaviour. Fix: util-linux
   `unshare`, outer `--map-root-user` only for the bind mounts, then an inner `--map-user/--map-group` to the real ids.
5. **W5 harness bugs.**
   - `python3` was not on the restricted `PATH`, so the harness now uses an absolute path.
   - The harness itself must not be preloaded or traced. Preload and strace are applied to the shell only.
   - mrsh has no `-i` option; it is interactive on a tty.
   - mrsh rejects the prompt `PSURF$ ` ("invalid parameter name"), so the prompt is now `PSURF> ` for every shell.
6. **strace attribution for vfork children.** A vfork child's syscalls are logged before the parent's `clone`
   returns the child pid, so streaming attribution assigned them to `?`. Fix: two passes over the trace. The first
   collects exec events and parent links, the second attributes each line.
7. **Wrong caller for absolute-path execs.** The first version attributed an `execve` to the image it started. It now
   uses the image that issued the call. For example, `/bin/sh` is run by gmake, not by dash.
8. **Multi-call binaries inflate the static bound.** Full toybox defconfig (327 imports) and full busybox (393) count
   every applet. Fix: `minbuild.sh` builds toybox and busybox per tier with only the applets that tier's core runs
   executed. Both bounds are reported.
   - The full binaries' shared startup code calls a few symbols the minimal builds do not import: busybox's setuid
     handling and `getopt_long`, and toybox's `getuid` root checks. Those are reported as a gap, and the minimal bound
     is defined as minimal imports plus observed calls.
   - busybox's Kconfig "sh is" choice always selects ash, even under `allnoconfig`. The minimal builds therefore
     linked ash until `SH_IS_NONE` was forced.
9. **The M3 extractor let dead code through.** The M3 reviewers found this. `--gc-sections` removes unused
   functions, but their DWARF line rows remain, at addresses outside `.text`. Rows are now kept only when their address
   lies in an executable section. All 100 rows this removed had already been marked `n/a` by the reviewers.
10. **The M3 extractor missed macro wrappers.** The M3 reviewers also found this. toybox's `XVFORK()` and busybox's
    `xfork`/`spawn_and_wait`/`safe_waitpid` style wrappers were not matched. The xargs `XVFORK` is the only process
    creation that toybox actually executes in the workloads. The wrappers were added to the patterns, wrapper
    definitions are skipped, and the 23 new rows were classified by the coordinator (`review/m3-coordinator.tsv`).

## Workload findings that are results, not failures

They are recorded in the outputs; listed here so they are not mistaken for harness problems.
- pdpmake cannot run tcc's Makefile (W1), which needs GNU make.
- hello's `configure` fails without a program called `ld` (libtool's `LT_PATH_LD`) even though tcc links by itself.
  With `LD=tcc` it then fails at `ar`: toybox has none and busybox defconfig has none. With `AR="tcc -ar"`, W4
  passes. These are the probes `W4-configure-noLD` and `W4-make-noAR`.
- toybox defconfig lacks `tr`, `diff`, `awk`, `expr` and `gzip`, which the workloads used. They came from busybox.
- After Ctrl-C in W5, mrsh reports `$?` = 129 where dash, oksh and busybox ash report 130. mrsh's status for a
  signal-terminated job differs; not investigated further.

## Analyses added after M4 (2026-10-24)

- `analysis/configure_checks.py`. The task asks which configure checks fail with tcc, and no script answered that.
  This one diffs hello's tcc `config.log` against a gcc baseline. 22 checks differ.
- `analysis/fork_children.py`, `analysis/signals_delivered.py` and `analysis/path_prefixes.py`. These are inputs that
  `conflict-impact.md` needed for X-2, X-3 and X-1. The first version of `fork_children.py` missed every pid that
  strace pads with two spaces. The totals did not reconcile with `syscall-surface.csv`, which exposed the bug; they
  now match (1,708 + 2 in `W3-check` = 1,710).
- `analyze.py` sorted a category tally by count only, so ties came out in hash order and the summary changed between
  runs. Ties are now sorted by name.

## Independent review (step 5)

A separate subagent reviewed `method.md` against the scripts. It had not written the scripts or the M3 reviews. It
also spot-checked 20 random rows of each CSV against raw VM data with its own scripts (seed
`random.Random(20261024)`). Full report: `tools/posix-surface/review/step5-review.md`.

| CSV | Rows checked | Mismatches |
|---|---|---|
| `libc-surface.csv` | 20 | 1: `system` labelled `unused-applets`, though pdpmake calls it (E2) |
| `syscall-surface.csv` | 20 | 0: counts, argument patterns and program attribution all matched |
| `callsites.csv` | 20 | 2: only the `signals` tag was wrong; api, line, function and class were right |

Findings and what was done:

- **E1, M3 missed executed code.** The full toybox/busybox binaries run set-uid startup code on every applet call
  (`toybox/main.c` `toy_init`; busybox `parse_config_file`/`check_suid`). The minimal builds compile it out, so M3,
  which filtered through their line tables, never saw it.
  - **Fixed:** `callsites.py` also uses the full binaries' line tables for those two files.
  - 13 sites were added and classified by the coordinator: all `direct`, removable by the config switch the minimal
    builds already use.
  - M3 is now 663 sites.
- **E2, `tier` labels.** `sym_tier()` labelled symbols that comparison programs call as `unused-applets`.
  - **Fixed:** dynamic evidence now takes precedence. The labels are now 166 `unused-applets` and 19 `comparison`.
  - The `tier` column is now defined in `method.md`. Per-tier counts were not affected.
- **I1, stack-protector symbols counted as API.** **Fixed:** they are excluded with the crt references. Every static
  upper bound drops by 2: T0 164, T1 248, T2 291, T3 331. Lower bounds are unchanged.
- **I2, vDSO wording.** `clock_gettime` does reach the kernel sometimes. The known-limits row was corrected.
- **Minor.**
  - The `signals` regex now includes `SEGV`, `BUS`, `ILL`, `FPE`, `ABRT`, `TRAP` and `SYS`.
  - Basename false positives are documented as possible; the review found none.
  - The review merge key, the `n/a` class, the raw data size (about 490 MB) and `SHELL` are now documented.
- **Notes left as they are:**
  - No script asserts that exit statuses match with and without the interposer. The review confirmed they match for
    all 25 runs.
  - W1 and W4 run several attempts in the same tree, so they depend on run order.
  - `dlsym`'s empty dynamic column means "never interposed", not "never called".
  - Per-tier syscall counts include syscalls from programs the workloads built.

All published numbers were regenerated after the fixes. `posix-surface.md`, `callsite-analysis.md` and
`conflict-impact.md` quote the regenerated values.

## Open questions

See the end of `conflict-impact.md` for questions the data frames but cannot answer, and `method.md` → "Known
limits" for measurement limits.

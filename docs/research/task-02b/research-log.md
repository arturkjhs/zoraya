# TASK-02b research log

Log for [TASK-02b](../TASK-02b-parallel-build-measurement.md). It records what was tried, what failed and how it was
handled, what varied between runs, and the independent review. The work ran on 2026-10-24.

Numbers in the other documents come from the pipeline outputs in `tools/posix-surface/results/par/`. This log only
describes process.

## Environment

- **VM.** The same Lima VM as TASK-02 (`alpine-posix`: Alpine 3.23.3, aarch64, musl 1.2.5, Linux 6.18.7, 4 vCPUs).
  It was reachable over SSH and was not reconfigured.
- **Git.** Branch `research/task-02b`, created from `research/task-02`. Author identity: the repository's
  configured one.
- **Versions.** Every program keeps its TASK-02 pin (`tools/posix-surface/versions.env`). No new program was
  added. The new workloads are committed under `tools/posix-surface/workloads/`.

## How the pipeline was extended

- **New stage `parallel`** (`remote/parallel.sh`), between `m4` and `analyze` in `run.sh`. It works the way the M4
  stage does:
  - its own raw directory (`raw-par/`) and results directory (`results/par/`);
  - the TASK-02 sets, namespace and `run_wl`.

  TASK-02 raw data and results are only read, never written.
- **Changes to shared TASK-02 code**, all behaviour-preserving:
  - `remote/workloads.sh`: the strace options are a variable whose default is the old literal, and `*W9-*` runs use
    the existing pty-harness path that W5 uses.
  - The DWARF helpers moved from `analysis/callsites.py` into `analysis/dwarfsites.py` so M7 can reuse them.
    `callsites.py` imports them.
  - The full rerun checked that TASK-02 outputs are unchanged (see "Reproduction").
- **New analyses:**
  - `analysis/parallel.py`: M5 and M6.
  - `analysis/callsites_par.py`: M7 extraction, in the VM.
  - `analysis/callsite_par_report.py`: the M7 merge with `review/m7-*.tsv`, local, like M3.
- **Trace options.** Parallel runs use `strace -f -q -ttt -v`:
  - `-q` keeps the exit lines;
  - `-ttt` gives the timeline clock;
  - `-v` prints the environment of each `execve`, which is where `MAKEFLAGS --jobserver-auth` is visible.

## Workload decisions

- **W3-j.** TASK-02's W3 was a dash script, so there was nothing for `make -j` to parallelise. W3-j is the same
  three tcc commands as a Makefile (`workloads/w3/Makefile`). Its serial baseline is the same Makefile with `-j1`,
  not the TASK-02 script.
- **Job counts.** -j2 and -j4. The VM has 4 cores, so -jN is -j4 and was not run twice (`results/par/params.tsv`).
- **pdpmake `-j`.** It is accepted and documented in its source as "not implemented" (`main.c:8`). It was run with
  -j2 and -j4 and stays serial: max one recipe at a time.
- **W8 sizing.** tcc is too fast for real compiles to keep job slots busy. The whole SQLite amalgamation compiles in
  about 45 ms in this VM, and a 3 MB generated file in about 50 ms.
  - Each leaf target therefore runs a small table generator (`workloads/w8/mktab.c`, about 0.4 s of computation),
    which a leaf compiles first with tcc.
  - The generator writes its target progressively, so an interrupted target is a real partial file.
  - W8 takes about 13 s serially and about 3.5 s at -j4, which leaves room to interrupt it early, mid-way or late.
- **W9 controlling process.** The shell is dash `-i`, session leader, with the pty as its controlling terminal.
  - In TASK-02's W5, strace was the session leader. For W9, strace runs as a detached grandchild in its own session
    (`strace -DDD`), so the shell is the harness's direct child and the controlling process, and Ctrl-C cannot
    reach strace.
  - The harness waits for the detached strace to exit before it returns.
- **W9 delays.** 0.3, 1.5 and 3.0 s after the command line: early, middle, late. Each case was repeated 3 times per
  delay, in both modes.

## Problems found and fixed

1. **The harness took the pty echo for the result.** It waited for `status=` after sending `make -j4; echo
   status=$?`, and the terminal echo of the typed line already contains that text.
   - Also, after Ctrl-C an interactive dash abandons the rest of the line, so the `echo` never ran.
   - Fix: send `make -j4` alone, wait for the prompt, then ask for `$?` with a separate command.
2. **Read-only source trees.** `src/` is read-only after fetching, and `cp -Rp` keeps that. Copying the W3-j
   Makefile into a fresh tree failed, and a later `rm -rf` of the half-built tree failed too. Fix: make the copy
   writable before adding files and before removing an old tree.
3. **Make levels by `-C` only.** Lua recurses through `cd src && $(MAKE)`, so its sub-make looked like a top-level
   make. Fix: the level is now the number of distinct make images among the ancestors.
4. **Timeline order.** A call that blocked is logged twice (`<unfinished ...>` and `<... resumed>`). The timeline
   first placed it at its start time, so a `wait4` that reaped a child appeared *before* that child died. Fix:
   events are placed at the time the call returned.
5. **strace pads short lines** (`read(3, "", 0)  = 0`, two spaces before `=`). The return-value parser expected
   exactly `) = `, so some results came out as `?`.
   - The unfinished/resumed join also left a stray space, which hid the options of blocking `wait4` calls.
   - Fix: whitespace-tolerant parsing. Syscall counts were not affected; only argument patterns were.
6. **Toybox start-up noise in timelines.** Every toybox applet resets about 61 signal dispositions at start-up. In
   the W10 timeline, a process's run of more than 3 resets is collapsed into one line.
7. **`FATAL_SIG` macro sites missed.** GNU make installs `fatal_error_signal` through a macro
   (`main.c:1270–1295`), which neither the TASK-02 M3 patterns nor the first M7 patterns matched.
   - M7 now matches `FATAL_SIG`: 7 sites, classified with the handler.
   - M3 is left unchanged, so that TASK-02 outputs reproduce exactly. The gap is recorded here: TASK-02 M3 is missing
     these 7 restructure lines, but its class totals for GNU make signals do not change in kind.

## Reproduction

A full `tools/posix-surface/run.sh` (all TASK-02 stages plus `parallel`, about 20 minutes on a VM that already had
the builds) reproduced the outputs as follows.

- **TASK-02 outputs.** All unchanged except three timing-dependent syscall counts in `syscall-surface.csv`:
  - ld.lld `futex` in W6-link: 185 → 182;
  - `read` in the W4 configure runs: up to 20 in 15,000;
  - `readv` in W4-configure: up to 5 in 7,700.

  These are the same kinds of variation TASK-02 already documented (`../task-02/method.md` → Known limits,
  "Reruns"). No libc symbol, class, category or published summary number changed. The committed TASK-02 files keep
  their earlier values.
- **TASK-02b outputs.** Regenerated. The values that legitimately differ between runs are pids, timestamps and
  timing-dependent counts.

## Run-to-run variation (TASK-02b)

Measured by repeating every non-interrupt workload twice and every W9 case 9 times (`results/par/syscall-reps.tsv`,
`parallel-tables.md`, stability tables in `timelines.md`).

- **Identical between repetitions:**
  - exit statuses and output checks;
  - concurrency reached (max recipes at once);
  - handler and mask set-up (`rt_sigaction`, `rt_sigprocmask` counts);
  - jobserver set-up;
  - every W9 event kind except the two below.
- **Varying with timing:**
  - jobserver token reads in W8 `-j4`: 17–26 per build;
  - `pselect6` calls: 72–100 per W8 `-j4` build;
  - `wait4` counts (within a few percent);
  - W8 build duration (±0.1 s);
  - in W9, which recipes had already finished (depends on the delay);
  - in W9 SIGTERM: whether a nested SIGCHLD from a just-killed recipe interrupted the handler's "Deleting file"
    `writev` (4/9 runs);
  - in W9 SIGTERM: whether a leaf make started a new recipe before the forwarded SIGTERM reached it, giving 3 deletions
    instead of 4 (1/9: `W9-term-d3.0-r2`);
  - in W9, how far the build had got under strace vs without it: 24–25 vs 28 tables at the 3.0 s delay. strace slows
    the build.

## Measurement limits specific to TASK-02b

- **strace changes timing.** The W9 sequences were checked against untraced runs (interposer mode) for outcome only:
  status, deletions, leftovers. The event order inside the untraced runs is not observable.
- **Order across processes** in a timeline is the order strace observed. Within a process it is exact.
- **Pre-exec attribution.** A forked child's calls before its `execve` run the parent's image. They are attributed
  to it as `shell(child before exec)`: for example, dash's job-control `setpgid` and `TIOCSPGRP` in the child that
  becomes make. The signal-delivery and stability tables attribute a receiver by its final image, so a signal
  delivered to a child before its exec would be credited to the program it was about to become.
- **The token count is a heuristic**: 1-byte `+` reads and writes that return 1. GNU make's token character is `+`
  by default. The count is not restricted to the jobserver fd, so other 1-byte `+` I/O would be counted too.
- **Parallel only up to -j4.** The VM has 4 vCPUs. Higher `-j` was not run.
- **W8 is synthetic.** Its recipes are a generated table program, not compiles, so that slots stay busy (see
  "Workload decisions").

## Independent review (step 5)

A separate subagent reviewed the work; it had not written the scripts. It checked:

- the W9a, W9b and W10 timelines against the raw traces and harness logs, plus 4 further W9 runs for the stability
  claims;
- 20 random rows of `syscall-delta.csv`, recounted from raw traces (seed `random.Random(20261024)`);
- the jobserver figures on 4 runs, and the new-libc table against both interposer data sets and `readelf`;
- 10 random M7 sites against the source.

Full report: `tools/posix-surface/review/step5-review-02b.md`.

**Verified with no discrepancy:**

- all 20 sampled `syscall-delta.csv` rows, and every entry of the "new syscalls" table;
- token reads and writes, `pselect6` counts and interrupted counts, the 9 `EAGAIN` reads, `mknodat` and the auth
  strings;
- the libc table (the six new functions are already imported, per `readelf`);
- the 10 M7 sites (line, function, description; no misclassification);
- in the timelines: senders and receivers, `si_code`, who deletes what, who re-raises, the exit statuses, and the
  shell's `TIOCSPGRP`/`tkill`;
- W10 in full.

**Errors found, all fixed in the documents:**

1. **`wait4` "restarted" was wrong.** The top and level-1 makes' blocking `wait4` returns `ERESTARTSYS` and is never
   resumed. `fatal_error_signal` issues its own `wait4` calls and never returns: none of the 54 interrupted pids
   calls `rt_sigreturn`. Fixed in `event-timelines.md`, `parallel-surface.md` and `conflict-impact.md`. This changes
   an emulation requirement in X-3: no `SA_RESTART` restart of `wait4` was observed.
2. **The cause of the interrupted `writev` was wrong.** It is the handler's own "Deleting file" message, interrupted
   by SIGCHLD from the recipe the same make had just killed. The handler, entered from `pselect6`, runs with that
   call's empty mask. It is not a `writev` in progress when SIGTERM arrived. This explains why it happens with
   SIGTERM (4/9) and never with Ctrl-C.
3. **"4 deletions in 36/36" and "one timing-dependent event kind" were false.** `W9-term-d3.0-r2` deleted 3. A leaf
   make started a new recipe 9.7 ms after the SIGTERM, 0.2 ms before the forwarded SIGTERM reached it, and killed
   that recipe before it created its target. "No partial target left, no stray process" still holds in 36/36.
   - New for X-3: forwarding SIGTERM down the tree leaves a window in which lower makes keep starting jobs.

**Imprecisions, fixed:**

- The W9b times for the level-1 and top makes' reaps, and the time of leaf a/1's SIGCHLD in W9a.
- FIFO open flags: only the top make opens the read side `O_NONBLOCK`; sub-makes set it with `fcntl`.
- The "bad stdin" `pipe2`: at most one per make, only in makes that run jobs concurrently.
- The extra `setpgid`/`getpgid`/`readlinkat`/`TIOCSPGRP`/`ttyname` in W9 come from the interactive-shell context,
  not from the interruption.
- `notable_args` counts are sums over repetitions; this is now documented.
- The `-j2` token explanation is reworded: implicit slots allow only two leaf makes at a time.

## Open questions

See the end of `conflict-impact.md`.

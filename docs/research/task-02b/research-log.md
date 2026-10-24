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

## Run-to-run variation

See "Reproduction" and "Independent review" below.

## Independent review (step 5)

To be appended after the review runs.

## Open questions

See the end of `conflict-impact.md`.

# ADR-0110: Acceptance harness for stage 6

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0001, ADR-0003, ADR-0005, ADR-0006, ADR-0053, ADR-0080, ADR-0100, ADR-0102, ADR-0104, ADR-0105, ADR-0106, ADR-0107, ADR-0109
- **Evidence:** alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#hubris, task-02/conflict-impact.md#x-2-no-fork-cp-05-vs-hosting-the-c-toolchain, task-02/method.md#known-limits, task-02/method.md#tiers-runs-and-sets, task-02b/parallel-surface.md#interruption-what-happens-to-the-build-w9, task-02b/parallel-surface.md#setup-in-one-paragraph

## Context

ADR-0005 defines T1's acceptance: the workloads W1, W2, W3, W5, W2-j, W3-j, W8, W9 and W10 run inside Zoraya with
their output checks passing (INV-0005-4), every function of the tier list exercised or marked `static-only`
(INV-0005-3), and the surface diffed against the exports (INV-0005-1, INV-0005-2). It also gates T2 on measuring
`snapshot_process` on W4 (ADR-0003's open question). The roadmap's criterion for stage 6 is `tcc hello.c && ./a.out`
inside Zoraya. `make test` runs scripted serial sessions through `tools/test-runner` today (`docs/design.md`,
"Tests"). This ADR decides how the workloads get into an image, how they run and are checked under `make test`, and
how the W4 measurement is taken.

**What is measured.**

- The workloads and their checks (`task-02/method.md#tiers-runs-and-sets`): W1 `sh ./configure --cc=tcc ...; make` in
  tcc's tree, checked by the stage-2 `./tcc -v`, a compile and a run; W2 Lua's `make posix` with GNU make, checked by
  `lua -e 'print(...)'` printing `lua ok 42`; W3 tcc on SQLite's two files, checked by a query printing 42; W5 the
  interactive session on a pseudo-terminal with `$?` after Ctrl-C; W6 is T3 and out. TASK-02b added W2-j and W3-j at
  `-j2` and `-j4` in both jobserver styles, W8 (a synthetic recursive build, about 13 s serial and 3.5 s at `-j4`
  on the measurement VM), W9 (W8 interrupted by Ctrl-C or `SIGTERM` after 0.3, 1.5 or 3.0 s) and W10 (a dash script
  of background jobs, `wait` and pipelines) (`task-02b/parallel-surface.md#setup-in-one-paragraph`). All 220 runs
  exited as expected: builds 0, Ctrl-C 130, SIGTERM 143 (`task-02b/parallel-surface.md#setup-in-one-paragraph`).
- The W5 script's steps and its `PSURF> ` prompt, and the W9 harness's actions (write `^C` to the pty after the delay,
  or `kill(SIGTERM)` the top make found as the shell's child) are in `tools/posix-surface/remote/w5_session.py` and
  `w9_session.py`; W8's makefiles and `mktab.c` and W10's script are in `tools/posix-surface/workloads/`.
- W9's outcomes: status 130 or 143, four partial targets deleted (three in one run), none left behind, no process
  left 0.5 s after make returned (`task-02b/parallel-surface.md#interruption-what-happens-to-the-build-w9`).
- The existing runner plays keys on the serial line and checks output: `<` waits for text, `>` sends keys, `!` forbids
  text, `~ MS TEXT` waits MS milliseconds failing if TEXT arrives, `o` requires exactly one occurrence, and a run
  passes only if QEMU exits 0 after the kernel's power-off (`tools/test-runner/src/script.rs`). `make test-boot`,
  `make test-files` and `make test-crash` use it with `--keys` and `--disk`; a failed step gives its log's path
  (`docs/design.md`, "Tests").
- Measured so far on this kernel: boot to prompt 0.105 s under HVF and 0.211 s under TCG; `port_wait` 875 ns and
  about 13 µs; the storage table of `docs/design.md`. Nothing about the toolchain's speed on Zoraya.
- 1,167 of dash's 1,169 no-exec forks are in W4 (`task-02/conflict-impact.md#x-2-no-fork-cp-05-vs-hosting-the-c-toolchain`),
  so W4 is the only measurement of `snapshot_process` at volume.

**What is assumed.** Run times inside Zoraya under TCG are unknown; W3's SQLite compile and W1's self-build are the
long ones. CI's 30-minute budget (`.github/workflows/test.yml`) may not hold them all.

**Forces from TASK-01.** A test that is not run proves nothing (CLAUDE.md's work discipline, ADR-0001 rule 6); the
measurement harness of TASK-02 relied on `strace` and `LD_PRELOAD`, neither of which exists on Zoraya
(`task-02/method.md#known-limits`), so observation inside the guest must come from `libposix` itself.

## Decision drivers

1. **Every acceptance workload runs inside Zoraya under `make test`**, checked by its own `W*-check` (INV-0005-4), with
   the result visible on the serial line and the run ending in a power-off (the runner's pass condition).
2. **Interactive scenarios need keys at the right moments** (W5, W9's Ctrl-C), which the runner already does.
3. **The guest can be observed only through what it prints**: coverage of the tier list (INV-0005-3), fork counts and
   times, round trips, must be collected by `libposix` and the broker and printed at the session's end.
4. **The T2 decision needs a number** (ADR-0003, ADR-0005): time and memory per `snapshot_process` on W4's 764 forks,
   in a form the architect can compare with a threshold.
5. **`make test` stays runnable from a clean checkout** without network (ADR-0109's fetch step) and within a budget
   the architect sets.
6. **Reuse the measurement's inputs**: the same pinned sources, makefiles, scripts and checks, so that a pass here
   means what a pass meant on Linux.

## Considered options

### Option A: Run the TASK-02 harness as it is, inside the guest (the Linux measurement setup transplanted: the baseline)

- **Description:** Port Python, `strace` and the interposer to Zoraya and run `run.sh`.
- **Evidence for:** Identical harness, identical numbers.
- **Evidence against:** Python, `strace` and `LD_PRELOAD` are T3 and beyond: threads, `ptrace`, dynamic linking,
  all excluded (ADR-0005); the harness would be larger than the system under test.
- **Cost:** Not achievable at stage 6.
- **Precedent:** The TASK-02 and TASK-02b runs on Alpine Linux.

### Option B: Drive the guest from the host over the serial line with the existing runner, checks as shell scripts

- **Description:** One disk image holds the ports (ADR-0109), the workloads' sources and a shell script per workload
  that runs it and prints `Wn-OK` or `Wn-FAIL`, plus `Wn-STATUS $?` and the partial-target count. `init` starts the
  POSIX session on `ttyd` over the console (ADR-0105) with dash at a known prompt; a runner script per workload types
  the command, waits for the markers, sends Ctrl-C where W9 needs it, types `exit`, and expects the power-off. The
  broker prints the session's statistics before it exits.
- **Evidence for:** Drivers 1 to 3 and 5 with tools that exist: the runner, `zoraya-mkfs`, `init`'s session path, the
  serial console. W5's and W9's actions are keys with delays, which the runner has (`>`, `~`). The checks are the
  measured ones, as shell commands (driver 6).
- **Evidence against:** Serial output is the only channel, so a check must print something distinctive; a hung guest
  is a timeout, diagnosed from the log. Interactive timing (W9's delays) is wall-clock on the host against a guest
  whose speed under TCG differs from the measurement VM, so the point of interruption differs; the measurement itself
  varied the delay for that reason.
- **Cost:** *(estimate)* One runner script per workload (about 15 lines each), the shell scripts, a `make test-posix`
  target, the statistics printout.
- **Precedent:** `make test-boot` and `make test-files` (this repository); Hubris drives its tests from the host
  through a debug probe and reads results back (`alternatives-postmortem.md#hubris`), the same host-driven shape.

### Option C: A guest-side native test program under the `ZORAYA-TEST` protocol

- **Description:** The test kernel's suite gains a test that starts the broker and the workloads itself and checks
  their output through pipes, reporting pass or fail as one `ZORAYA-TEST` case.
- **Evidence for:** No host timing; checks are code, not text matching; it fits the in-QEMU suite.
- **Evidence against:** The test kernel's processes are started by the kernel, not by `init`, so the session path of
  ADR-0104 would need a second launcher; W5 and W9 need keystrokes with a terminal in the loop, which a native test
  would have to fake through `ttyd`'s backend; a failing workload is one failed case with no transcript.
- **Cost:** *(estimate)* A native launcher duplicating `init`'s, and a fake terminal.
- **Precedent:** `t_storage` (this repository), for a smaller system; Fuchsia runs its tests as components under a
  test manager inside the system (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`; the test manager is
  `[outside evidence: fuchsia.dev test runner framework, not fetched]`).

### Option D: Host-side checking of the disk image after the run

- **Description:** After each run, a host tool reads the image (the engine runs on the host) and checks the files the
  workload produced: `lua`, `sqlite3`, `t4.h` absent, the statistics file.
- **Evidence for:** Rich checks without serial parsing; `zoraya-fsck` already runs after every disk test.
- **Evidence against:** It cannot see exit statuses or prompts, so W5 and W9 still need Option B; a file-extracting
  tool does not exist yet.
- **Cost:** *(estimate)* An `--extract` option on `zoraya-fsck` or a new tool, 150 lines.
- **Precedent:** `make test-crash`'s `crashcheck` (this repository), which runs in the guest.

## Recommendation

**Recommendation (not a decision): Option B, with Option D as an addition when a check needs a file.**

1. **The image.** `make posix-image` builds `build/posix-disk.img` with `zoraya-mkfs` from `build/rootfs-posix/`
   (ADR-0109) plus `tests/posix/rootfs/`: `/etc/passwd` and `/etc/group` with the fixed identity (ADR-0002 rule 6),
   `/etc/zoraya/session` naming the session command and environment (`sh -i` with `PS1='PSURF> '`, `ENV=/dev/null`,
   `PATH=/bin:/usr/bin`, `HOME=/home`, `TERM=dumb`, `SHELL=/bin/sh`, as the measurement's `env -i` did,
   `task-02/method.md#tiers-runs-and-sets`; ADR-0104 item 12), a `/home` directory, `/src/tcc` (the pinned tcc tree), `/src/lua`, `/src/sqlite` (the amalgamation), `/src/w8`,
   `/src/w10` (from `tools/posix-surface/workloads/`, unchanged), and `/tests/wN.sh` scripts. `init` reads
   `/etc/zoraya/session` when the volume has it and starts `ttyd`, the broker and that command instead of `zosh`
   (ADR-0104 item 12, ADR-0053); without the file it starts `zosh` as today, so every existing test is unchanged.
2. **The scripts.** `/tests/w1.sh`: copy `/src/tcc` to `/work/w1`, run the measured `configure` and `make` with the
   Zoraya target (ADR-0109 item 8), then the W1 check, printing `W1-OK` or `W1-FAIL rc`. `/tests/w2.sh`: Lua's
   `make posix` with `CC=tcc AR="tcc -ar rcu" RANLIB=true`, serial, then `-j2` and `-j4` in both jobserver styles
   (W2-j), each followed by the check. `/tests/w3.sh`: the two tcc compiles and the link, then the query (W3), then
   the W3 Makefile at `-j2` and `-j4` (W3-j). `/tests/w8.sh`: W8 serial and `-j2`, `-j4` in both styles, with its
   output check and the recipe count. `/tests/w10.sh`: `w10.sh parallel` and `serial`, diffing their outputs with
   busybox `diff` and printing the pipeline status line. Every script ends with `Wn-STATUS $?`. The host runner
   script `tests/posix/wN.script` types `sh /tests/wN.sh` at the prompt, expects the markers, forbids `-FAIL`,
   `panicked` and `ZORAYA-FAULT`, types `exit`, and expects the broker's statistics and the power-off.
3. **W5.** `tests/posix/w5.script` is the measured session's steps verbatim: `cd "$HOME"`, the pipe through `tr`, the
   redirection, `touch` and the glob, the export and `sh -c`, `sleep 2 &`, `jobs`, `sleep 30` then `\x03` after a
   `~ 500` quiet step, `echo status=$?` expecting `status=130`, `cd /tmp; pwd; cd -`, `wait; echo waited`, `exit`;
   each step waits for `PSURF> `. `uname` is added as the first step (INV-0109-2).
4. **W9.** `tests/posix/w9-int.script`: at the prompt, `cd /work/w8 && make -j4 >/dev/null`, then `~ 1500` (a
   quiet wait), then `\x03`, then wait for the prompt. An interactive dash whose foreground job died of `SIGINT`
   raises `SIGINT` on itself and abandons the rest of the typed line ([D1]; the measurement's harness asked for the
   status on a separate line for that reason, `tools/posix-surface/remote/w9_session.py`), so the next steps are
   separate lines at the prompt: `echo W9-STATUS $?`, then `n=0; for f in t*.h; do [ -e "$f" ] && n=$((n+1)); done;
   echo W9-PARTIAL $n` (dash only, no applet), then expect
   `W9-STATUS 130` and `W9-PARTIAL 0` and, after half a second, `W9-LEFT 0` from a `ps`-less check: the script's
   next line counts the session's pids through the broker's `pids` builtin added to dash as a test aid (or a
   `/tests/pids` program over the broker protocol). `tests/posix/w9-term.script`: the same build in the background
   with its pid in a file, `sleep 1.5; kill -TERM $(cat make.pid)` as a second command, expecting `W9-STATUS 143`.
   The delays are the measurement's 1.5 s; the point reached differs with the guest's speed, as it did between the
   measurement's delays, and the checked outcomes are the ones that did not vary.
5. **The roadmap criterion.** `tests/posix/hello.script`: `printf 'int main(){puts("hello from tcc");return 0;}'
   > hello.c && tcc hello.c && ./a.out`, expecting the line.
6. **`make test-posix`** runs items 2 to 5 under `auto` and `tcg`, `zoraya-fsck` after each, as `test-files` does,
   and is a step of `make test` after `test-files`; `QEMU_TIMEOUT` per script from a table, set from the first
   measured durations with a margin. If CI's budget does not hold W1 and W3 under TCG, the architect chooses between a
   longer budget and a `make test-posix-long` outside the default steps; this ADR does not pre-empt it.
7. **Observation.** A `stats` feature of `zoraya-posix`, on in the test image: each process counts `snapshot_process`
   calls and their wall-clock time (`clock_monotonic` around the call), `exec`s and their time, broker round trips,
   `share` and `seek` requests, resolver calls and time, request polls (ADR-0106), the largest `spawn` buffer it
   built, the peak fd count, and a bitmap of the tier-list functions it called first (INV-0005-3), and sends them to
   the broker in its last message before exit. The broker also counts the peak of live pids and, from `tmpd`'s
   `status`, the peak space of `/tmp`. At the session's end the broker prints one line per counter, `POSIX-STATS
   name=value`, and the call bitmap as hex, `POSIX-CALLS <hex>`. The runner's log keeps them; `tools/posix-stats`
   (host) turns a log into a table for the task report.
8. **INV-0005-3.** `tools/posix-stats --calls` decodes `POSIX-CALLS` from every T1 run's log, unions them, and diffs
   against `docs/adr/posix-surface-T1.txt`: every function not called must be marked `static-only` in the list's
   second column, else the step fails.
9. **INV-0005-1 and INV-0005-2.** ADR-0100's export check runs in the same step.
10. **The W4 measurement** (`make measure-w4`, not part of `make test`): the image also holds GNU hello 2.12.3's
    source and the T2 applets the ports build when asked; the script runs `./configure CC=tcc LD=tcc AR="tcc -ar"`
    and `make`, with the statistics of item 7. The report gives: forks, total and mean `snapshot_process` time,
    `exec`s and time, peak live pids, peak `/tmp`, wall-clock of configure and of make, under HVF and TCG, beside the
    Linux figures TASK-02 has for the same run (`task-02/method.md#tiers-runs-and-sets`). A second number comes from
    the test kernel: one `snapshot_process` of a process with a known mapped size, with frame counts before and
    after (INV-0102-3's counters), giving bytes copied per fork, which with the fork count gives the memory charged.
    The threshold is the architect's (ADR-0003's open question).
11. **Other measurements the design asked for**, from the same statistics: path-resolution cost on W1 (ADR-0002),
    broker round trips per spawn and reap (ADR-0104), `share` and `seek` counts (ADR-0103), the poll's share of
    system calls (ADR-0106), the largest `spawn` buffer against 64 KiB (ADR-0042), the peak process count against 255
    (ADR-0031), and wake-ups per second at `-j4` on W8 (ADR-0006), with `-j8` as a variant when the host has the
    cores. Each goes into the stage 6 task report and `docs/design.md`'s measured table.
12. **The W5 extension** (Ctrl-Z, `fg`, `bg`; ADR-0004's open question): `tests/posix/w5-jobs.script`, run by
    `make measure-w4`'s target too, not by `make test`, until the architect promotes it.

**Tensions with accepted ADRs.**

- *ADR-0005 INV-0005-3*'s check speaks of "a compile-time call-log feature in `libposix`"; item 7's `stats` feature
  is that, with the broker as the aggregator and the serial line as the channel. *INV-0005-4* says "CI runs the
  TASK-02/02b workload scripts against the prototype"; the scripts are rewritten as dash scripts and runner scripts
  (items 2 to 4) with the measured commands and checks, since the Python harness cannot run on Zoraya.
- *ADR-0053*: `init` gains a second session shape, chosen by a file on the volume; the kernel still starts one
  process (INV-0053-1).
- *ADR-0080*: a restart of the storage stack ends the POSIX session with the shell, as it ends `zosh`; a test that
  sees one is a failed test.

### Effect of the vision

No change for any target: this is a test harness. For target 2 the same image and scripts would run a sandboxed
session, with the broker's statistics as the first cost model of a sandbox. Threads and networking: no effect.

### Deferred to a later stage

Option D's extraction tool; mutation lists for the new crates (`tools/mutants/posix.toml`, by CLAUDE.md's rule once
the code exists); T2's acceptance (W4 as a test rather than a measurement); a timing-independent W9 (a build that
blocks on a FIFO until the harness releases it).

## Consequences

- **Positive.** The acceptance is the measured checks, run by the tools the repository has, with every number the
  design asked for coming from one statistics line.
- **Negative.** `make test` grows by several minutes, more under TCG; W9's interruption point depends on timing; a
  guest hang is a timeout to diagnose from a log.
- **Follow-up.** `tests/posix/`, `make posix-image`, `make test-posix`, `make measure-w4`, `tools/posix-stats`,
  `init`'s session file, the `stats` feature, the timeout table.

## Invariants

- **INV-0110-1.** `make test` fails unless every T1 acceptance workload (W1, W2, W3, W5, W2-j, W3-j, W8, W9, W10) and
  the roadmap criterion run inside Zoraya to their `OK` marker, `W9-STATUS 130` and `143`, `W9-PARTIAL 0`, and a
  power-off. *Check:* `make test-posix`, a step of `make test`, with one runner script per workload.
- **INV-0110-2.** The union of `POSIX-CALLS` over the T1 runs covers every function of the tier list not marked
  `static-only`. *Check:* `tools/posix-stats --calls` in `make test-posix`.
- **INV-0110-3.** A W4 report produced by `make measure-w4` carries every counter of item 10 (fork count, total and
  mean `snapshot_process` time, `exec` count and time, peak live pids, peak `/tmp` use) and names the accelerator; a
  report missing one is not a report. *Check:* `tools/posix-stats` fails on a missing counter.
- **INV-0110-4.** A `-FAIL`, `panicked` or `ZORAYA-FAULT` line in any POSIX run fails the step. *Check:* the `!`
  lines of every runner script.

## Open questions

- Run times under TCG for W1 and W3, and so CI's budget: the first run decides item 6's split.
- Whether W9 under TCG reaches a state at 1.5 s where any recipe runs (the measurement's 0.3 s runs had 0 of 32
  tables done); a delay table per accelerator may be needed.
- The `pids` test aid of item 4: a dash builtin patch or a separate program.

## Revisit triggers

- CI exceeding its budget: `make test-posix-long`.
- T2 accepted: W4 moves from measurement to acceptance.
- A workload whose check cannot be a printed line: Option D's tool.

## Architect's decision


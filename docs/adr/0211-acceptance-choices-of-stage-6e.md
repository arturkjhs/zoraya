# ADR-0211: Choices of stage 6e that ADR-0110 leaves open

- **Status:** proposed
- **Date:** 2026-11-04
- **Depends on:** ADR-0005, ADR-0100, ADR-0103, ADR-0104, ADR-0106, ADR-0110, ADR-0180, ADR-0190
- **Evidence:** CP-27, task-02/method.md#m1-libc-api-surface, task-02/method.md#tiers-runs-and-sets

## Context

ADR-0110 decides the acceptance harness: the image, the scripts, two targets, a `stats` feature of `zoraya-posix`
whose counters and call bitmap go to the broker, the broker's `POSIX-STATS` and `POSIX-CALLS` lines, and
`tools/posix-stats`. Building it (TASK-22) needed choices the ADR does not make: how a call is recorded in a library
that is static and has no interposer, when a process reports, where the peak of `/tmp` comes from, what the 6c
launcher does with a report, and how the measured commands meet a system with only T1's applets. TASK-02's
interposer logged "the first call per symbol per process image whose return address lies outside musl's mapping"
(`task-02/method.md#m1-libc-api-surface`); the workloads ran in a fresh copy of their tree, made by the harness, under
`env -i` (`task-02/method.md#tiers-runs-and-sets`).

## Decision drivers

1. **A pass here must mean what a pass meant on Linux** (ADR-0110 driver 6): the same calls counted, the same
   commands, the same checks.
2. **Observation must not change what it observes** beyond a bounded cost, and must not need a feature ADR-0005
   excludes (no dynamic loader, no `ptrace`).
3. **No change to an accepted protocol's meaning** that a client relies on.
4. **The least code that the measurement needs** (CP-27).

## Considered options

### Option A: wrappers at archive construction, reports by round trip (as built)

- **Description:** Items 1 to 8 below.
- **Evidence for:** Drivers 1 to 4: the wrappers see what TASK-02's interposer saw (calls from outside the
  library), cost two instructions and a store a call, and need nothing at run time; a round trip cannot lose a
  report to the broker's order of events.
- **Evidence against:** A process ended by a signal never reports (its counters and calls are lost); a round trip
  at every exit and `exec` adds to what is measured.
- **Cost:** *(estimate)* 200 lines of tool and Rust, one protocol message, one count in an existing reply.

### Option B: count in the Rust half only (the Linux `strace` view)

- **Description:** Count the system calls and the Rust half's functions; leave musl's functions uncounted.
- **Evidence against:** INV-0005-3 is about libc functions, most of which are musl's C; driver 1 fails.

### Option C: a separate call-log library linked before `libc.a` (an `LD_PRELOAD` shape)

- **Description:** Every program links `-lcalllog` first; its wrappers call `dlsym`-like real names.
- **Evidence against:** Needs every build (tcc's, `zcc`'s, every Makefile) to add the library: the measured
  commands change; a static link has no `RTLD_NEXT`.

### Option D: reports without a reply

- **Description:** As A, but the report is a message with no answer.
- **Evidence against:** The broker may reap the pid and drop its connection before reading the report, so reports
  are lost by timing; a round trip removes the race (driver 1).

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The call log** is made when `libc.a` is: `posix-surface-check calllog` renames each exported function's
   definition, and every reference to it inside the library's members, to `__zoraya_real_NAME`, and adds a member
   `zw-NAME.o` per function whose `NAME` sets byte *i* of `__zoraya_calls` and branches to the real definition, with
   only x16 and x17 used (AAPCS64's veneer registers), so variadic functions, `setjmp` and `vfork` behave as without
   it. A program's calls reach the wrapper; the library's own calls do not, as TASK-02's interposer saw only calls
   from outside musl. Data names are not wrapped (as TASK-02's `not-interposed.txt`). One member per wrapper, so that
   a static link pulls in only what it calls. The bitmap's order is `usr/lib/zoraya-calls.txt`, written with the
   archive.
2. **`stats` is on in the image** the stage 6 tests boot, which is the only POSIX image: `ports/Makefile` builds
   `zoraya-posix` with the feature and the call log by default (`POSIX_STATS=0` builds neither). The feature also
   counts system calls in `zoraya-rt` (`count`).
3. **When a process reports:** a `ReportStats` request (broker ordinal 15) with its counters, in the order
   `zoraya_proto_posix::stats::NAMES` gives, and its bitmap packed to bits, before it exits (`_exit`, `exit`'s end)
   and before an `exec` hands over (while the broker still answers its connection); then it counts from zero, and a
   `fork` child starts from zero, its copy of the parent's counts being the parent's. A process ended by a signal
   (W9's interrupted makes) does not report; its calls and counts are missing from the totals. The broker sums the
   counters, keeps the largest of those named `max_`, joins the bitmaps, and counts the reports as `images`.
4. **The peak of `/tmp`:** the file service keeps the most blocks in use since its mount, sampled after every
   request, and answers it in bytes as `status`'s `Done.count`, which was 0 and which no client reads; the broker
   asks `tmpd` once at the session's end. A peak between two requests' samples is not seen; the engine allocates
   only inside a request, so none is missed.
5. **The peak of live pids** is the broker's count after each `spawn_prepare`.
6. **The 6c launcher answers reports:** `posixrun` serves its child's broker channel while the child runs, answers
   a `ReportStats` and does not count it; every other message is counted as before (INV-0103-3), so `broker
   messages 0` still means that the file paths sent the broker nothing.
7. **Exec and spawn time:** `exec_ns` runs from `execve`'s entry to the new image's start (loading the program,
   `image_create`, `spawn`), before the report and the commit; `spawn_ns` is a whole successful `posix_spawn`
   (prepare, load, spawn, commit). Their sum over a workload's wall time is the share of loading programs.
8. **Wake-ups** are `waits`, the kernel waits at wait points (ADR-0106); with `takes`, the request polls, and
   `syscalls`, the share of the poll is `takes / syscalls`.
9. **The guest has 512 MiB in the POSIX session** (`POSIX_MEM`): W8 at `-j4` ran out of 128 MiB (`tcc: memory
   full`), and four rounds of W8 at `-j2` in one boot showed no growth. Every POSIX process holds at least a 1 MiB
   step of heap (`zoraya-heap`'s `GROW_SIZE`, eagerly committed objects), its copied segments and 64 KiB of stack;
   `tmpd` holds 16 MiB.
10. **The trees are copied on the host**: `tests/posix/trees.sh` makes a fresh copy of the pinned source in
    `/work/ID` for every build, as TASK-02's harness did (`fresh_run`, `tree`); T1's applets have no `cp`, and the
    copy is the harness's work, not the workload's (`task-02/method.md#tiers-runs-and-sets`).
11. **pdpmake's set** is `/sets/t1p`, a directory whose `make` is `/bin/pdpmake`, first on `PATH` for W2's and W4's
    pdpmake runs, as TASK-02's set `t1p`; the commands stay `make ...`.
12. **Checks written with dash alone** where TASK-02b's used an applet T1 does not have: W8's `tail -n 1 | grep -q
    "0 };"` is a `while read` loop and a `case`; W9's partial targets are the tables that exist and do not end that
    way. W8 also counts the recipes it ran (72). W9's "no process left" is `t_posix live N`: `kill(pid, 0)` for
    every pid from 3 to N but itself and its parent, which the broker answers from its table (`ESRCH` once reaped;
    pids are never given twice); ADR-0110's open question of a `pids` aid.
13. **Wall time is measured on the host**: the runner's `% NAME` step prints `RUNNER-TIME NAME SECONDS` since the
    last mark, as TASK-02's harness timed outside the measured processes; nothing is added inside the guest.
14. **The call log's check** reads the runner's full logs, `target/test-logs/posix-*.log` (ADR-0110 item 8 names
    `build/logs/`, where `make test` keeps each step's terminal output, which the runner cuts to 400 lines). Its
    `static-only` marks are computed, not stored: a tier-list function is `static-only` unless TASK-02's T0 and T1
    `core` runs saw a measured program call it (`dynamic-programs.tsv`, 134 names, TASK-02's lower bound) or
    TASK-02b added it (six). The tier list's file stays the CSV's alone, as `posix-surface-check list --check`
    requires.
15. **W4 records how far it gets**: T2's applets and rows are not built (TASK-22's scope), so `configure` meets
    T1's tools; `/tests/w4.sh` prints `W4-STOP` rather than `-FAIL` at the step that fails, and `make -f
    ports/Makefile t2-probe` links T2's applets against `libc.a` and lists the names they lack.

**Tensions with accepted ADRs.**

- *ADR-0110 item 7* says "a bitmap of the tier-list functions it called first": the bitmap covers every exported
  function, the tier list's and the added blocks' (ADR-0142, ADR-0144, ADR-0210); `tools/posix-stats --calls` checks
  the tier list's and reports the others.
- *ADR-0103 item 6* and ADR-0010's `status`: its count now carries the peak; its status is unchanged.
- *INV-0103-3*'s check in `posixrun` excludes the report, as item 6 says.
- *ADR-0110 item 8*: the logs' place and the computed marks (item 14).
- *ADR-0110 item 4*: W9's partial count is of incomplete tables in the leaves, where W8's tables are (the ADR's
  `for f in t*.h` names the top directory, which has none).

## Consequences

- **Positive.** Every number ADR-0110 item 11 asks comes from one session's lines; INV-0005-3 is checked against
  the same notion of a call TASK-02 used.
- **Negative.** Processes ended by signals are not counted; every exit and `exec` pays one broker round trip; the
  archive has 290 more members.
- **Follow-up.** `tools/posix-stats`; the W-scripts; `docs/design.md`'s measured table.

## Invariants

- **INV-0211-1.** Every exported function of `libc.a` is reached through its call-log wrapper, and `libc.a`'s own
  members reference only the real definitions. *Check:* `posix-surface-check`'s host tests
  (`call_log_wraps_exactly_the_listed_defined_functions`, `call_log_assembly_names_its_byte_and_target`), and
  `make check-ports`' export check on the built archive (the wrappers are the exports; a broken copy without one
  fails).
- **INV-0211-2.** A session's `POSIX-STATS` lines name every counter of `stats::NAMES`, `peak_pids` and
  `peak_tmp_bytes`, and the broker refuses a malformed report. *Check:* `zoraya-broker`'s host tests
  (`reports_are_summed_maxed_and_joined`, `a_report_is_taken_and_the_peak_of_live_pids_kept`),
  `zoraya-fs-service`'s `status_counts_the_peak_of_the_space_in_use`, and `tools/posix-stats`, which fails on a
  missing counter (INV-0110-3).

## Open questions

- Counting what a process ended by a signal did (a report from the broker's reap of its job would need the counts
  outside the process).

## Revisit triggers

- A workload whose counts depend on processes ended by signals.
- A client that reads `status`'s count.

## Architect's decision

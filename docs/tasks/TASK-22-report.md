# TASK-22 report: stage 6e, acceptance and measurement of the POSIX personality

Branch `stage-6e`, not merged or pushed. Inside Zoraya, tcc builds itself, Lua and SQLite with GNU make, pdpmake and
dash. The measured session (W5), the recursive build (W8), its interruption (W9) and the background jobs (W10) run
with TASK-02's and TASK-02b's commands and checks. All runs are on the M4, under QEMU 11.1.2.

**From a clean clone** of the branch, with the ports' cache seeded from this worktree (`make fetch-ports` checked
every hash and downloaded nothing):

- `make test`: 17 of 17 steps passed in 1,066 s; `test-posix` took 376 s for its 15 boots.
- `make test-posix-long`: passed in 849 s. That covers W1, W2, W2-j, W3 and W3-j under HVF and TCG, W8 under TCG,
  and the call log's union.
- `make measure-w4`: passed (W4 and the W5 extension under HVF and TCG; `build/w4-report.md`). It was run in the
  worktree, not the clean clone.

CI has not run the branch, since nothing is pushed.

## The measurements beside Linux

The table, with every figure of ADR-0110 item 11, is in `docs/design.md`, under "Stage 6 acceptance: the toolchain
inside Zoraya". TASK-02 recorded wall times only for W8 (under `strace`). Linux's figures are its counts of programs
loaded and its durations:

| Workload | HVF / TCG | Linux | Programs loaded here (Linux) | Share loading, HVF / TCG |
|---|---|---|---|---|
| W1 | 3.5 / 35 s | – | 59 (64) | 44 / 32 % |
| W2 | 7.9 / 74 s | – | 128 (127) | 65 / 52 % |
| W2-j | 13 / 134 s | 0.3–0.5 s a build | 181 | 61 / 48 % |
| W3 | 1.1 / 11 s | – | 7 (6) | 24 / 17 % |
| W3-j | 4.3 / 45 s | 0.2 s a build | 25 | 23 / 15 % |
| W5 | 2.2 / 3.8 s | – | 8 (8) | 12 / 52 % |
| W8 | 89 / 488 s | 13.4 / 6.8 / 3.4 s | 571 (555) | 61 / 66 % |
| W9 Ctrl-C; SIGTERM | 4.5 / 30 s; 4.8 / 97 s | – | – | – |
| W10 | 4.4 / 25 s | – | 24 | 36 / 32 % |
| `tcc hello.c && ./a.out` | 0.4 / 4.6 s | – | 2 | 11 / 8 % |

Across the workloads:

- **Forks:** the mean `snapshot_process` of a dash fork is 0.44–1.7 ms under HVF and 7.2–14 ms under TCG.
- **Broker:** every child takes three broker round trips.
- **Path resolution:** 0.46–0.76 µs a path under HVF (W1: 1,892 paths, 0.46 µs) and 33–119 µs under TCG.
- **Request polls:** 0.1–0.5 % of system calls in the builds, 5–21 % in the interactive sessions.
- **Wake-ups:** W8 at `-j4` wakes 5 times a second under HVF.
- **Limits:** the largest `spawn` buffer is 1,120 of 65,536 bytes; at most 13 pids are live at once (of 255) and 10
  fds open in one process; `/tmp` reaches at most 52 KiB.
- **One CPU:** the guest has one CPU, so `-j` gives nothing. On the backlog by the architect's word.
- **Loading programs** is the largest cost: two thirds of W8. The image cache of ADR-0212 is the next task.

## The call log's union against TASK-02's surface

Over the nine T1 runs under both accelerators (20 logs), 161 exported functions were called.

- **Called here, never measured by TASK-02 (18):** `ferror_unlocked fprintf getegid getopt iscntrl memchr putc puts
  setenv sscanf strcasecmp strsignal strspn strtoll strtoull time vfprintf waitpid`. TASK-02's interposer did not
  count the products; the union here does (ADR-0213 item 4). The tools' own runs also differ by their builds.
- **Measured, never called (3):** `vfork`, `getopt_long` and `writev`. Each is exempted by name in ADR-0211 item 16:
  - `vfork`: the ported dash uses `posix_spawn`.
  - `getopt_long`: the ported busybox is a minimal build.
  - `writev`: TASK-02b reached it through toybox's `tail` and `grep`, which T1 lacks.
- **Static-only, never called (81):** functions TASK-02 saw imported but never called, as expected.
- **Not logged (6):** the data names `environ optarg optind stderr stdin stdout`.
- **Called outside the tier list (8):** `abs getdelim getpwuid isblank posix_spawnattr_setpgroup
  posix_spawnattr_setsigdefault system zoraya_fork_group`, from ADR-0142's, ADR-0144's and ADR-0210's blocks.

## W4 in brief, and T2

GNU hello's `configure` stops in its first lines, under both makes and both accelerators: `sed: not found`, `expr:
not found`. T2's applets are not built, and they cannot be yet: linked against this `libc.a`, T2's toybox lacks 20
names and busybox's `awk expr gzip` lack 9 (regex, group lookups, `getline`, `mkdtemp`, `link`, `mknod` and others;
`build/w4-report.md`).

Before it stops, `configure` forks dash 120 times:

- `snapshot_process` takes 0.55 ms mean under HVF and 5.7 ms under TCG; the whole fork in the parent takes 0.68 ms
  and 10.7 ms.
- At those means, the 1,167 forks W4 made on Linux would cost about 0.8 s under HVF and 13 s under TCG.
- W4's 2,881 program loads, at W8's means, would cost about 270 s and 1,700 s.

**Recommendation on T2:** the fork is not what T2 waits for: eager copying holds at W4's volume, and no
copy-on-write is needed for it. Before T2's acceptance, three things are needed:

1. The image cache (ADR-0212).
2. T2's rows: the regex, group and directory names above.
3. Then W4 measured for real.

## Fixes to 6c and 6d code

Each fix has a test that fails without it:

1. **`libc.a`'s exports:** the 48 names W2's and W3's programs and pdpmake need (ADR-0210). The ten new functions
   are `chmod`, `rmdir`, `symlink`, `utimes`, `fsync`, `gethostname`, `getrusage`, `mmap`, `pclose` (musl's, through
   a shim route for `__sys_wait4`) and `system` (musl's). Each is tested by `t_posix products`, which also calls the
   38 names that were already there, hidden; `fsync` also has a host test.
2. **`fsd`'s `status` count:** it now carries the peak use, for the peak of `/tmp` (ADR-0211; host test).
3. **`posixrun`:** it answers a statistics report without counting it (`test-libposix`).
4. **The stats feature:** the call log, `ReportStats` (ordinal 15) and `zoraya-rt`'s system-call count (ADR-0211;
   host tests).
5. **`getrusage(RUSAGE_THREAD)`**, from the review (R15).

No 6c or 6d defect was found by the builds themselves: W1, W2 and W3 ran once the names linked.

## Invariant delta

**Checked now:**

- INV-0005-3: the union, with ADR-0211 item 16's three exemptions.
- INV-0005-4.
- INV-0110-1 to INV-0110-4.
- INV-0109-2's `uname`.
- INV-0210-1.
- INV-0211-1 and INV-0211-2.
- INV-0213-2 (ADR-0213 is proposed).

**Not yet applicable:**

- INV-0212-2 to INV-0212-4: until the image cache.
- INV-0212-1 and INV-0213-1: until the architect decides ADR-0213, which rewords INV-0212-1 for CI's two jobs; CI has
  not run the branch.

The table's total is 399 invariants.

## ADRs

- Accepted by the architect on 2026-11-04 and recorded at the architect's request: ADR-0210, ADR-0211 and ADR-0212.
- Proposed: ADR-0213, the review's corrections and statements: W9 interrupted on progress, INV-0212-1 reworded, the
  harness's remaining differences from the measurement.

## Open questions

- CI's speed against the M4: the posix job's 30 minutes rest on the M4's figures (`make test-posix` takes 376 s
  here, with W8 under HVF; on CI it runs under TCG twice).
- SQLite's `dlopen`: ADR-0210 Option A (the flag, as built) or B (stubs).
- A process ended by a signal reports nothing (ADR-0211 item 3).
- INV-0109-1's host check of tcc's `a.out` needs a tool to extract files from the image.
- The POSIX guest needs 512 MiB because of `zoraya-heap`'s 1 MiB first step (backlog).

## Time per step

These are approximate, from the commits' times and the runs' own durations.

| Step | Time |
|---|---|
| Reading, experiments, writing code and docs | about 3.5 hours |
| Builds (ports from clean: about 2 min; `libc.a` rebuilds: seconds) | under 15 min |
| Host tests (`test-unit`: 199 s each full run) | about 15 min |
| QEMU runs, of which the clean `make test` 1,066 s, `make test-posix-long` 849 s and `measure-w4` 42 s | about 2 hours |
| The review (14 min) and the check of the changes (2 min) | 16 min |
| Mutants (four rounds of `make mutants-host`) and triage | about 35 min |

## Disk

The worktree's `build/` is 987 MB and `target/` 1.5 GB (the ports' cache, the trees, the images and the mutants'
copies). The clean clone in the scratchpad took 2.7 GB and is removed with this report, as is `build/exp`, about
270 MB of experiment logs.

## Running W1 to W3 by hand

```sh
make fetch-ports          # once; the only step with network
make posix-image
target/debug/zoraya-mkfs --size 256M --from build/posix/root build/posix-disk.img
tools/run-qemu.sh --mem 512M --disk build/posix-disk.img build/zoraya-kernel.elf
```

At the `PSURF>` prompt:

```sh
sh /tests/w1.sh           # W1: tcc builds itself; W1-OK
sh /tests/w2.sh serial    # W2: Lua with pdpmake, then GNU make; W2-OK w2-p, W2-OK w2-g
sh /tests/w3.sh serial    # W3: SQLite; W3-OK w3
exit
```

`sh /tests/w2.sh parallel` and `sh /tests/w3.sh parallel` are W2-j and W3-j. Each `/work` tree is good for one build;
make a fresh image to run one again.

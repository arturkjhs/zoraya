# ADR-0212: Loading programs, and what of stage 6's acceptance fits CI

- **Status:** proposed
- **Date:** 2026-11-04
- **Depends on:** ADR-0042, ADR-0051, ADR-0102, ADR-0104, ADR-0108, ADR-0110, ADR-0211
- **Evidence:** task-02b/parallel-surface.md#concurrency-actually-achieved

## Context

ADR-0110 item 6 puts W5, W8, W9, W10 and the roadmap's criterion into `make test`, under `auto` and `tcg`; on CI
(Linux) both are TCG. CI's job has `timeout-minutes: 30`; `make test` took about 18 minutes there at the last merge
(run 41797194695: `test-posix` 228 s, `test-libposix` 50 s, `test-posix-gui` 75 s). TASK-22 measured the fast
workloads on the M4 under QEMU 11.1.2, one run each, with the statistics of ADR-0211 (wall time from the runner's
marks, loading time from `exec_ns` and `spawn_ns`):

| | HVF | TCG |
|---|---|---|
| W8, five builds (`-j1`, `-j2` and `-j4` in both styles) | 88 s | 496 s |
| W8: share of wall time loading programs (175 `exec`, 396 `posix_spawn`) | 61 % | 67 % |
| W8: mean `exec`; mean `posix_spawn` | 99 ms; 91 ms | 423 ms; 654 ms |
| W9 Ctrl-C; W9 SIGTERM; W10; `tcc hello.c`; W5 (whole boot, TCG) | | 59 s; 13 s; 33 s; 14 s; 12 s |
| W10: share loading | | 33 % |

The guest has one CPU, so W8 takes the same time at every `-j` (TASK-02b's four-core VM: 13.4, 6.8 and 3.4 s,
`task-02b/parallel-surface.md#concurrency-actually-achieved`). Under TCG on the M4, W8 alone is about 500 s per
accelerator; on CI, with both accelerators TCG and a runner slower than the M4 `[assumption: not measured on CI]`,
`make test-posix` would add about 20 minutes to 18, past the 30-minute limit. Two thirds of W8's time is the cost
TASK-21 recorded: every `exec` and `posix_spawn` reads the whole program through the file protocol into a fresh
memory object, `image_create` checks it, and `spawn` copies its segments; nothing is kept between spawns of one
program (`docs/backlog.md`, "Images"; `docs/research/stage-6/plan.md` puts an image cache out of stage 6 with a
revisit trigger). TASK-22's instruction: report with the numbers and a proposed ADR; do not build a cache without a
decision.

## Decision drivers

1. **`make test` must fit CI's budget** (ADR-0110 driver 5) with the acceptance in it (INV-0110-1).
2. **The measured workloads stay as measured** (ADR-0110 driver 6): W8 is five builds.
3. **No design without a decision** (CLAUDE.md): a cache is an architectural choice (ADR-0108's object model).
4. **The cost is real**: a program run 571 times in W8 is read 571 times; a cache helps every POSIX user, not only
   the tests.

## Considered options

### Option A: an image cache in the broker (what Linux's page cache gives)

- **Description:** The broker keeps the image (`image_create`'s handle) of each program by the volume's object id
  and modification time; `exec` and `posix_spawn` ask it before reading the file. The image is immutable once
  created (ADR-0108), so a cached one is shared safely; a changed file has a new time and a new entry.
- **Evidence for:** Removes the read and the check for every repeat: in W8, every load but the first of `tcc`,
  `sh`, `make` and `mktab` (four programs); an estimated two thirds of 67 % `[estimate: spawn's segment copy
  remains]`. Linux keeps file pages cached, which is why TASK-02's numbers have no such cost.
- **Evidence against:** A new broker request and a cache's eviction rule; memory held for images (tcc's file is
  1.7 MB with its debug information, 549 KB loaded).
- **Cost:** *(estimate)* 200 lines: one broker request, `libposix`'s use of it, a bound.

### Option B: `image_create` from a file connection

- **Description:** The kernel takes a file connection and the service hands it the file's blocks as a memory
  object (a mapping of the service's cache), so no copy goes through the protocol.
- **Evidence for:** Removes the read's copies for every load, first ones too.
- **Evidence against:** The kernel does not speak the file protocol; the service would export cache frames to a
  client, a new sharing rule in ADR-0051's terms; larger than A.
- **Cost:** *(estimate)* a kernel call, a file-protocol request, frame sharing rules.

### Option C: keep loading as it is; `make test` runs W8 under one accelerator

- **Description:** `make test-posix` runs W5, W9, W10 and the criterion under `auto` and `tcg`, and W8 under `auto`
  only (on CI: TCG once, about 500 s on the M4); the second accelerator's W8 moves to `make test-posix-long`.
- **Evidence for:** No new design; INV-0110-1's W8 still runs in `make test`, once.
- **Evidence against:** About 12 minutes added on CI by the M4's figures, near the limit with the existing 18;
  W8 under HVF is not in CI at all (CI has no HVF).
- **Cost:** a Makefile line.

### Option D: CI runs `make test-posix` as its own job

- **Description:** A second CI job (its own 30 minutes) runs `make test-posix`; `make test` runs the rest.
- **Evidence for:** Every workload under both accelerators, unchanged.
- **Evidence against:** CI minutes double for that part; `make test` on a developer's machine still runs it all
  (ADR-0110: a step of `make test`).

## Recommendation

**Recommendation (not a decision): Option A**, with Option C until it is built. A is the cost every POSIX program pays
and the only one that brings W8 under TCG near its compute; C keeps `make test` inside CI's limit now.

## Consequences

- **Positive (A).** Loading programs stops dominating builds; W8 under TCG to an estimated 200 s.
- **Negative (A).** A cache to bound and invalidate; memory held by images.
- **Follow-up.** The cache's ADR items; then W8 back under both accelerators in `make test`.

## Invariants

- **INV-0212-1.** `make test` stays within CI's `timeout-minutes` with `make test-posix` in it. *Check:* CI itself
  (a run past the limit fails).

## Open questions

- CI's runner speed against the M4 for these workloads (the first CI run of the branch measures it).

## Revisit triggers

- CI's limit or runner changes; a cache built (W8 under both accelerators again).

## Architect's decision

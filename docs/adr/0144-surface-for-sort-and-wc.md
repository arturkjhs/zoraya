# ADR-0144: The surface toybox's `sort` and `wc` need

- **Status:** proposed
- **Date:** 2026-11-03
- **Depends on:** ADR-0005, ADR-0100, ADR-0109, ADR-0141, ADR-0142
- **Evidence:** CP-27, task-02/libc-surface.csv

## Context

ADR-0109 item 6 builds toybox with the T1 applets of `applets-by-tier.tsv` plus `sort`, `wc` and `yes`, which W10
runs, and lists that addition as a tension with ADR-0005. It did not add their imports to the tier list. TASK-17
built that toybox: with the tier list of ADR-0100 item 7 its link fails on two names, both hidden in `libc.a`
(ADR-0141):

- `atoll`, called by `sort`'s `compare_keys` (`toys/posix/sort.c`, numeric keys);
- `getdelim`, called by toybox's line reader `do_lines` (`lib/lib.c`), which `sort` and `wc` use.

Both are rows of `task-02/libc-surface.csv` at tier T2 (`atoll` imported by busybox and toybox, `getdelim` by
toybox's T2 build and called by its `grep` and `sed` in W4). Both are retained musl C (`src/stdlib/atoll.c`,
`src/stdio/getdelim.c`): they need nothing of the Rust half. The T1 list does not have them because the T1 toybox of
TASK-02's minimal builds had neither `sort` nor `wc`.

## Decision drivers

1. **The surface changes only by ADR, with a workload behind each row** (CP-27, INV-0005-1): W10 runs `sort` and
   `wc -l` (ADR-0109 item 6).
2. **No new work in the Rust half**: both functions are pure musl.
3. **Patch third-party programs as little as possible** (ADR-0109 item 9).

## Considered options

### Option A: Add the two rows (recommended)

- **Description:** This ADR adds `atoll` and `getdelim` to the names `libc.a` may export, through the block below,
  which `posix-surface-check` reads beside the generated tier list (the mechanism of ADR-0142).
- **Evidence for:** Drivers 1 to 3: W10 needs the applets, the applets need the names, the names are measured T2 rows
  and cost nothing.
- **Evidence against:** The T1 surface grows by two rows taken from T2.
- **Cost:** None in code; two lines.
- **Precedent:** ADR-0005 added `writev` the same way, on TASK-02b's evidence.

### Option B: Patch toybox to avoid them

- **Description:** `sort` calls `strtoll` (a tier row) instead of `atoll`; `do_lines` reads lines without
  `getdelim`.
- **Evidence for:** The tier list stays the CSV's T0 and T1 rows.
- **Evidence against:** `do_lines` is toybox's common line reader; rewriting it is a behavioural change in every
  applet that uses it, carried across toybox versions (driver 3).
- **Cost:** A patch of toybox's core library.
- **Precedent:** None in the files.

### Option C: Leave `sort` and `wc` out of T1 (baseline: the TSV as measured)

- **Description:** Build toybox with the TSV's nine T1 applets only.
- **Evidence for:** Matches `applets-by-tier.tsv` exactly.
- **Evidence against:** W10 then cannot run (ADR-0110 item 2), which reverses ADR-0109 item 6.
- **Cost:** An acceptance workload lost.
- **Precedent:** TASK-02's minimal builds.

## Recommendation

**Recommendation (not a decision): Option A.** Until the architect decides, `make ports` builds and checks with these
names admitted, and `posix-surface-check` prints that it admits them from a proposed ADR.

```posix-surface-added
atoll
getdelim
```

## Consequences

- **Positive.** toybox links with the applets W10 needs; nothing changes in the Rust half.
- **Negative.** The exported surface is the CSV's T1 list plus two names here and three in ADR-0142.
- **Follow-up.** INV-0005-3's call log (6e) covers the two names like any other.

## Invariants

- **INV-0144-1.** `libc.a` exports `atoll` and `getdelim` only through this ADR's block. *Check:*
  `posix-surface-check check`, which reads the block, run by `make check-ports`.

## Open questions

- Whether other applets the architect may add to T1 (ADR-0005's open question) bring more T2 rows; each would come
  with its own ADR in this form.

## Revisit triggers

- A change of toybox's version or of its T1 applets.

## Architect's decision


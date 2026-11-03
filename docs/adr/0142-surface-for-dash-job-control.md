# ADR-0142: The surface dash's job-control patches need

- **Status:** accepted
- **Date:** 2026-11-02
- **Depends on:** ADR-0003, ADR-0005, ADR-0100, ADR-0104, ADR-0109, ADR-0141
- **Evidence:** CP-27, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, task-02/libc-surface.csv

## Context

ADR-0109 item 9 lists two dash patches beyond TASK-02's M4 patch: the fork group hint of ADR-0104 item 4 in
`forkshell` (dash tells the broker, before `fork`, which process group the child will join), and `vforkexec` turned
into `posix_spawn` with `POSIX_SPAWN_SETPGROUP` and the Zoraya-only `POSIX_SPAWN_TCSETPGRP` for the interactive
foreground path, so that the parent hands the terminal over before the child runs (ADR-0104 item 5). ADR-0104 names
the Zoraya-only function `zoraya_fork_group(Inherit | New | Join(pid))` and the flag; it does not say where they are
declared or what their values are.

TASK-17 found that these patches cannot link against the archive ADR-0141 builds:

- `posix_spawnattr_setpgroup` is not a row of the tier list (`task-02/libc-surface.csv`: T1 has `posix_spawnattr_init`,
  `_destroy`, `_setflags` and `_setsigmask`), so `libc.a` hides it; a `posix_spawn` with `SETPGROUP` cannot set its
  group without it.
- dash's `forkchild` resets `SIGINT`, `SIGQUIT`, `SIGTERM`, `SIGTSTP` and `SIGTTOU` in a job-control child, which an
  interactive shell ignores; across `posix_spawn` that is `POSIX_SPAWN_SETSIGDEF` with
  `posix_spawnattr_setsigdefault`, also not a row.
- `zoraya_fork_group` is no row, and ADR-0141's checker counts it as an export outside the list.

INV-0005-1 allows an export outside the CSV's rows "in an accepted superseding ADR". ADR-0104 is accepted and needs
these names, but does not add them as rows, so the export check cannot admit them without a decision. TASK-17
therefore applies only the M4 patch (non-interactive simple commands through `posix_spawn` with no attributes, as
measured), which needs nothing new, and leaves the two job-control patches to 6d, where the broker exists and they can
be tested.

## Decision drivers

1. **The surface changes only by ADR** (CP-27, INV-0005-1).
2. **ADR-0104's interactive foreground path needs the parent to set the group and the terminal before the child runs**
   (ADR-0003's follow-up), which in POSIX terms is `posix_spawn` with attributes.
3. **A Zoraya-only interface must be declared somewhere a program can include, without changing musl's standard
   headers' meaning.**

## Considered options

### Option A: Keep `vfork` for interactive shells (baseline, as measured on Linux)

- **Description:** Apply only the M4 patch; interactive and job-control shells keep `vfork` and set the group and
  terminal in the child, as dash does on Linux; `vfork` is `fork` (ADR-0100 item 6).
- **Evidence for:** No new surface at all.
- **Evidence against:** Each interactive command is a `snapshot_process` (ADR-0102), and the child sets the terminal's
  foreground group itself, the race ADR-0104 item 5 removes; the fork group hint (item 4) still needs
  `zoraya_fork_group` for the child to land in the right group.
- **Cost:** None now; the race and the copies stay.
- **Precedent:** dash on Linux.

### Option B: Add the three names to the export check by this ADR (recommended)

- **Description:** This ADR adds `posix_spawnattr_setpgroup`, `posix_spawnattr_setsigdefault` and `zoraya_fork_group`
  to the names `libc.a` may export, as ADR-0005 added `writev`, on ADR-0104's need rather than a measurement. The
  sysroot gains `<zoraya/posix.h>`, Zoraya's own header (not musl's), declaring
  `int zoraya_fork_group(pid_t pgid)` (`pgid` < 0 inherit, 0 a new group led by the child, > 0 join that group, the
  convention of `setpgid`'s second argument; returns 0 or -1 with `errno`) and
  `#define POSIX_SPAWN_TCSETPGRP 0x8000` (no flag of musl or glibc uses that bit). `posix-surface-check` reads the
  added names from a list in this ADR's file, so that the generated tier list stays the CSV's.
- **Evidence for:** Drivers 1 to 3; the patches of ADR-0109 item 9 can then be written and linked in 6d.
- **Evidence against:** Two POSIX functions without a measurement behind them (they replace calls dash makes in a
  `vfork` child, which the interposer could not see), and a non-standard function in every POSIX process.
- **Cost:** *(estimate)* A header of 20 lines, a list of three names, three Rust functions in 6d.
- **Precedent:** Fuchsia's libc adds its own `fdio` functions beside POSIX in separate headers
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option C: Put the hint and the flag into existing calls

- **Description:** No new names: `setpgid` before `fork` records the hint, and `tcsetpgrp` called with a pid that is
  not yet a group is remembered for the next `posix_spawn`.
- **Evidence for:** No surface change.
- **Evidence against:** Gives standard calls hidden, order-dependent meanings that no other program expects; a
  failure would surface as job-control bugs, not link errors.
- **Cost:** *(estimate)* Less code, more state in `libposix`.
- **Precedent:** None in the files.

## Recommendation

### Decision

Option B, with two changes by the architect: the two dash patches are written in 6b (TASK-17) and first run in 6d,
and `POSIX_SPAWN_TCSETPGRP` is `0x4000`, not `0x8000`. `posix_spawnattr_setflags` takes a `short`, and musl's checks
its argument with `if (flags & ~all_flags) return EINVAL`, where `all_flags` is `unsigned`: `0x8000` is the sign
bit, so the `short` -32768 becomes `0xffff8000` on its way to `unsigned`, and bits 16 to 31 fail the mask whatever
`all_flags` holds. `0x4000` is positive as a `short`, passes a mask that includes it, and is stored in the `int`
`__flags` unchanged; no flag of musl (up to `0x80`) or glibc (up to `0x100`) uses it. The Rust half's
`posix_spawnattr_setflags` takes musl's mask with `POSIX_SPAWN_TCSETPGRP` added.

The recommendation as written:

**Recommendation (not a decision): Option B**, written and tested in 6d with the broker. Until then (TASK-17), dash
carries only the M4 patch, and its interactive path uses `vfork` as on Linux. The names this ADR adds, in the form
`posix-surface-check` reads:

```posix-surface-added
posix_spawnattr_setpgroup
posix_spawnattr_setsigdefault
zoraya_fork_group
```

## Consequences

- **Positive.** ADR-0104's race-free foreground handover becomes buildable; the surface change is explicit.
- **Negative.** The tier list is no longer the CSV alone: the check reads the CSV's list plus this ADR's names.
- **Follow-up.** `<zoraya/posix.h>` in the sysroot; the two dash patches; `posix-surface-check` reading the added names.

## Invariants

- **INV-0142-1.** `libc.a` exports `posix_spawnattr_setpgroup`, `posix_spawnattr_setsigdefault` and
  `zoraya_fork_group` only if this ADR is accepted, and no other name outside the generated tier list. *Check:*
  `posix-surface-check check`, with the added names read from this ADR.

## Open questions

- Whether `setpgid` should also be able to report the hint when called before `fork`, for programs other than dash.

## Revisit triggers

- Another program in scope needs job control through `posix_spawn`.

## Architect's decision

Accepted, 2026-11-03, with one change: the two job-control patches of dash are written in 6b (TASK-17), not left to 6d; they are tested with the broker in 6d. The block of added names was put into the text before this record. Decided by the architect; recorded by an agent at the architect's request. Changed by the architect on 2026-11-03: `POSIX_SPAWN_TCSETPGRP` is `0x4000`, the highest bit below the sign bit of the `short` flags, because `0x8000` does not pass musl's flag mask without sign extension (see "### Decision"). Decided by the architect; recorded by an agent at the architect's request.

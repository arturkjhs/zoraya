# ADR-0192: `snapshot_process` creates in the caller's POSIX tree

- **Status:** proposed
- **Date:** 2026-11-04
- **Depends on:** ADR-0002, ADR-0003, ADR-0102, ADR-0104, ADR-0130
- **Evidence:** CP-07

## Context

ADR-0102 item 1 says that `snapshot_process` creates the child in `job`, "which must carry `CREATE_PROCESS` and be
in the caller's job tree". ADR-0130 item 4 (accepted) made that a check that `job` lies within the caller's own job,
and `AccessDenied` otherwise; `t_snapshot` checks it with a job outside the caller's.

ADR-0104 (accepted) gives every POSIX process a process job of its own, inside the job of its process group, and the
broker creates a `fork` child's process job beside its parent's: in the parent's group job (`Inherit`), in a new group
job (`New`), or in another group's job (`Join`). None of these is within the parent's own process job. Stage 6d's
first `fork` (dash's pipeline) was refused with `AccessDenied`: the two accepted ADRs cannot both hold as built.

What is measured: nothing; the refusal was seen on the first run of `make test-posix`.

## Decision drivers

1. ADR-0104's structure: groups as jobs, `kill(-pgid)` as one `job_signal` on a group job (INV-0104-3), `setpgid`
   never moving a process between jobs (CP-07, INV-0104-4). A child must be created in its group's job.
2. `snapshot_process` stays a personality-only call that cannot reach outside a POSIX tree (INV-0003-2, ADR-0002
   rule 3).
3. The least change to the kernel and to the accepted ADRs.

## Considered options

### Option A: The child in the caller's own job, as built

- **Description:** Keep ADR-0130 item 4; the broker creates a `fork` child's process job inside the parent's.
- **Evidence for:** No kernel change.
- **Evidence against:** A child in a new group would be inside the parent's job, so `kill(-pgid)` of the parent's
  group reaches it and `job_signal` of the child's group job cannot be the whole group; the terminal's Ctrl-C would
  reach the shell with its job (driver 1). The job tree would grow one level per fork generation (32 at most).
- **Cost:** none in the kernel; ADR-0104 rewritten.
- **Precedent:** none.

### Option B: No tree check: `CREATE_PROCESS` is the authority (what `spawn` does)

- **Description:** As `spawn`: a job handle with `CREATE_PROCESS` is enough. Linux has no such check either:
  `fork` always creates in the caller's own session and group, and what a process can reach is what it holds.
- **Evidence for:** One rule for both calls; capabilities as everywhere else.
- **Evidence against:** A POSIX process holding a `CREATE_PROCESS` handle to a job outside its tree (a native
  program's) could put a copy of itself there; at stage 6 nothing hands one out, but the gate of driver 2 would rest
  on what handles are given, not on the kernel.
- **Cost:** one line.
- **Precedent:** `spawn` (ADR-0046).

### Option C: The tree under the caller's outermost `posix` ancestor

- **Description:** The job must lie within the root of the run of `posix` jobs that ends at the caller's job: the
  broker's job for a session (ADR-0104 item 1). A job outside that tree is `AccessDenied`, as before.
- **Evidence for:** Driver 1: every job the broker creates for the session is in it. Driver 2: no `posix` process
  can snapshot outside the `posix` tree it runs in, whatever handles it holds; `t_snapshot`'s job outside the tree is
  still refused. Driver 3: one function in `kernel/src/job.rs` and one line in `snapshot_process`.
- **Evidence against:** Two POSIX sessions under one `posix` ancestor would share a tree; ADR-0104 puts each broker
  in a job of its own made by `init` from a non-`posix` job, so they do not. Target 2's sandboxes need the same
  shape (a non-`posix` launcher job above each).
- **Cost:** about ten lines and a test.
- **Precedent:** none; the `posix` bit is Zoraya's.

## Recommendation

**Recommendation (not a decision): Option C.** "The caller's job tree" of ADR-0102 item 1 is the tree under the
outermost job of the run of `posix` jobs that ends at the caller's job (`Job::posix_root`). This supersedes ADR-0130
item 4's reading; its status, `AccessDenied`, is unchanged.

## Consequences

- `fork` works with ADR-0104's jobs: the criterion of stage 6d runs on it.
- A launcher that wants two POSIX trees kept apart starts each under a job without the bit, as `init` does.
- `t_snapshot`'s "outside our tree" case still holds; a sibling case is covered by every `fork` of
  `make test-posix`.

## Invariants

- **INV-0192-1.** `snapshot_process` succeeds into a `posix` job beside the caller's within the caller's `posix`
  tree, and refuses a job outside that tree with `AccessDenied`. *Check:* `t_snapshot`'s
  `the_job_must_be_ours_and_open` for the refusal, and every `fork` of `make test-posix` (dash's pipelines and
  subshells) for the sibling.

## Open questions

- Whether target 2 wants Option B's rule with a narrower handle policy instead.

## Revisit triggers

- A design where two sessions share a `posix` ancestor.

## Architect's decision

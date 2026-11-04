# ADR-0192: `snapshot_process` creates where its job handle allows, in `posix` jobs

- **Status:** accepted
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

### Decision

**Option B, with two conditions** (the architect's decision, 2026-11-04). `snapshot_process` follows `spawn`'s rule:
the target job handle's `CREATE_PROCESS` right is the authority, as ADR-0046 makes it for `spawn`. Two further
conditions: the caller is in a job with the `posix` bit (already true: the call is personality-only, INV-0002-2), and
the target job has the bit; a target without it is `AccessDenied`. There is no tree-membership check. This replaces
ADR-0130 item 4's check; its status, `AccessDenied`, is unchanged for a missing right or a job without the bit. A
process in one session cannot snapshot into another session's jobs because it holds no handle to them: a handle
value it has not been given is `BadHandle`. `Job::posix_root` is gone from the kernel.

The original recommendation follows.

**Recommendation (not a decision): Option C.** "The caller's job tree" of ADR-0102 item 1 is the tree under the
outermost job of the run of `posix` jobs that ends at the caller's job (`Job::posix_root`). This supersedes ADR-0130
item 4's reading; its status, `AccessDenied`, is unchanged.

## Consequences

- `fork` works with ADR-0104's jobs: the criterion of stage 6d runs on it.
- As decided, what keeps POSIX trees apart is the handles each holds, as for `spawn`: the broker hands a process only
  the jobs of its own session (`spawn_prepare`'s `child_job`), and nothing hands a `posix` process a `CREATE_PROCESS`
  handle to a native program's job, which the second condition refuses anyway.
- `t_snapshot` checks the rule's four cases: a job beside the caller's is accepted, a handle without
  `CREATE_PROCESS` and a job without the bit are `AccessDenied`, an unheld handle value is `BadHandle`.

## Invariants

- **INV-0192-1.** `snapshot_process` succeeds into any `posix` job whose handle carries `CREATE_PROCESS`, inside the
  caller's own job or not; it refuses a handle without `CREATE_PROCESS` and a job without the `posix` bit with
  `AccessDenied`, and a handle value the caller does not hold with `BadHandle`. *Check:* `t_snapshot`'s
  `the_job_must_be_ours_and_open` (a sibling `posix` job, a narrowed handle, a job without the bit, unheld values),
  and every `fork` of `make test-posix` into the `child_job` of `spawn_prepare` (dash's pipelines and subshells).

## Open questions

- None for stage 6. Target 2's sandboxes rest on which job handles a process is given.

## Revisit triggers

- A component that hands a `posix` process a `CREATE_PROCESS` handle to a job of another session.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request. Changed from the
recommendation: Option B with two conditions instead of Option C (the `### Decision` above), with the tests of
INV-0192-1.

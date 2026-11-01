# ADR-0046: Job and request details the accepted ADRs leave open

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0003, ADR-0004, ADR-0006, ADR-0041
- **Evidence:** CP-14, DM-100, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context
Building ADR-0003 and ADR-0004 needs answers they do not give. What the process-group field of `spawn` means, given that ADR-0004 makes groups sub-jobs. What happens when a request queue is full, since ADR-0004 says "queues" and CP-14 wants a bound. What `TERMINATED` means for a job that never had a process. Whether a killed job accepts new processes. This ADR covers one question, what a job and its requests do at their limits, because the answers depend on each other.

## Decision drivers
1. `job_signal` and `job_kill` must leave nothing behind (DM-100, INV-0004-3).
2. Delivery must not need memory, or a job-wide request could half-apply (CP-14).
3. Stay inside ADR-0003's and ADR-0004's words.

## Considered options

### Option A: Linux process groups and cgroups
- **Description:** Groups are ids; a pending standard signal coalesces into one bit; `cgroup.kill` iterates and rechecks.
- **Evidence for:** Coalescing never fails or allocates.
- **Evidence against:** DM-100's races, and coalescing loses `detail`, which ADR-0004 needs.
- **Cost:** Low.
- **Precedent:** Linux.

### Option B: Bounded queue with an error; killed jobs closed; group as sub-job
- **Description:** A fixed ring per process; the group field selects a sub-job; a killed job refuses new processes.
- **Evidence for:** No allocation on delivery (driver 2). The sender learns of a loss (CP-14). A closed job has no window for a late child (driver 1). Zircon's jobs refuse new children once killed `[outside evidence: not checked]`, and its job model is cited at `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`.
- **Evidence against:** A flood is reported, not absorbed.
- **Cost:** *(estimate)* 30 lines.
- **Precedent:** Zircon jobs.

### Option C: Unbounded queue from the heap
- **Description:** Each request allocates.
- **Evidence for:** Nothing is lost, as ADR-0004 literally says.
- **Evidence against:** A job-wide request can fail halfway, and a sender can grow kernel memory.
- **Cost:** A partial-failure rule for `job_signal`.
- **Precedent:** None better.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Group.** `spawn`'s group field, if not 0, is a job handle with `CREATE_PROCESS` in the tree of the `job` field. The process goes in that sub-job. Otherwise `InvalidArgument`.
- **Request queue.** 64 entries per process `[assumption]`, in the process, with no allocation. If full, `process_signal` returns `LimitReached`. `job_signal` delivers to everything it can and then returns `LimitReached` if any was full.
- **Job `TERMINATED`.** Asserted exactly while the job's tree has no live process, a new job included, and cleared by `spawn`. A waiter should spawn first.
- **`job_kill`.** Ends every process in the tree with reason `Killed`, and closes the tree: `spawn` and `job_create` then give `BadState`.
- **Depth.** A job tree is at most 32 deep `[assumption]`, so walking it has a bound; deeper gives `LimitReached`.
- **Exit record of a request.** ADR-0004 rule 1 has `detail` carry the kind and the signal number. The top byte is the kind plus one, and the low 24 bits the sender's value, so `process_signal` and `job_signal` refuse a `detail` above 2^24 - 1 with `InvalidArgument`. `process_exit_with_reason` leaves the top byte 0. `WTERMSIG` is the low bits if nonzero, else ADR-0004's map from the kind.
- **Atomicity.** One lock over the job tree covers process creation, `job_signal`, `job_kill` and leaving a job. That is what INV-0004-3 asks for, on one CPU or many.
- **`exit_with_reason`.** Takes only `Terminated`, as ADR-0004 rule 1 defines; any other reason is `InvalidArgument`.
- **`posix` bit.** A flag of `job_create`, inherited by child jobs, and otherwise unused until `snapshot_process`.
- **Not built at stage 3.** `job_suspend`, `job_resume`, `object_wait_one`, `object_get_signals`, timers and event objects. TASK-09 does not list them, and no test needs them. Each stays specified by its ADR.

## Consequences
- **Positive.** `job_signal` cannot half-fail for lack of memory. A kill is final.
- **Negative.** A flooded process loses requests, and only the sender knows. A shell must spawn before waiting on a job.
- **Follow-up.** The ADR-0006 items above, when something needs them.

## Invariants
- **INV-0046-1.** A full request queue gives `LimitReached` and never allocates. *Check:* the queue is a fixed array, which the type states, and an in-QEMU test fills one and sees `LimitReached`.
- **INV-0046-2.** After `job_kill` returns, no process can be created in that tree. *Check:* in-QEMU test.
- **INV-0046-3.** A job tree deeper than 32 is refused. *Check:* in-QEMU test.

## Open questions
- Whether 64 entries suits a real signal storm: nothing measured.

## Revisit triggers
- A workload that loses requests.
- A shell's need for job-level `SUSPENDED` at stage 6, which brings `job_suspend` in.

## Architect's decision

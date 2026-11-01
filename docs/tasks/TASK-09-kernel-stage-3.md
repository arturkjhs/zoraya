# TASK-09: Kernel stage 3 — handles, rights, jobs, processes, ports, native spawn

## Precondition

`main` must contain stage 2, the filesystem engine and the TASK-07 review fixes (the former branch `fs/review-fixes`), and ADR-0035 and ADR-0036 must be accepted. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/adr/invariants.md`, `docs/tasks/TASK-08-kernel-stage-2.md` and its review.

Read **in full**: ADR-0003 (process creation), ADR-0004 (notification without signals), ADR-0006 (event mechanism), ADR-0030 to ADR-0036. Read the Recommendation of ADR-0002.

This is roadmap stage 3:

| Stage | Content | Done when |
|---|---|---|
| 3 | Handles, ports and process objects per ADR-0003/0004/0006; native `spawn` | a process spawns a child and waits on its exit via a port |

Stage 3 is where the accepted architecture first becomes code. The accepted ADRs are the specification. Where they are silent, write a proposed ADR; never fill a gap silently.

## Step 1: Proposed ADRs for what the accepted ADRs leave open

**ADR numbers 0040 to 0049 are reserved for this task.** Template-exact, status `proposed`, at least three options each, a Linux baseline and a non-Unix precedent (Zircon, seL4, NT, KeyKOS and others from `docs/research/alternatives-postmortem.md`). Short: these are engineering decisions.

At least:
1. **Handle table and rights.** Handle representation (index plus generation, or other), what prevents a reused slot from being confused with a closed handle, the rights bit set per object type (at least `duplicate`, `transfer`, `wait`, `signal`, `create_process`, and what ADR-0003/0004/0006 need), rights reduction on duplicate and transfer, per-process handle limits and what happens when they are reached, and handle closing semantics.
2. **Kernel object lifetime.** Reference counting or another scheme; what keeps an object alive (handles, kernel references, port subscriptions); when a process object becomes a zombie and when it is freed; how a job's lifetime relates to its processes.
3. **Passing structures to system calls.** ADR-0032 open question: `spawn` has too many parameters for registers. Layout, versioning, validation, and how the typed user-memory access of ADR-0031 reads them.
4. **Where `spawn` gets its image.** At stage 3 there is no filesystem service. Decide how a parent obtains an image handle for a program in the embedded ramdisk (ADR-0033) without introducing a global name namespace that contradicts ADR-0002 (for example a ramdisk-directory handle given to the first process, with lookup by relative name).
5. **Dispatcher and ABI table.** ADR-0032 open question: build the dispatcher from the `zoraya-abi` signature table, or a test that proves they match. Choose and say how the invariant lints then run over the source of truth.

Also re-number the system calls if needed (ADR-0032 allows it: nothing outside the repository depends on the numbers).

Commit the ADRs before the code that depends on them. Implement the recommended options; the architect accepts or changes them at review, and merging depends on that. Keep decision-dependent code behind clear module boundaries.

## Step 2: What to build

Following the accepted ADRs exactly:

- **Handles:** per-process handle table with rights; `handle_close`, `handle_duplicate` (with rights reduction), and transfer as part of `spawn`.
- **Jobs:** job objects and the job tree; every process is in a job (ADR-0003); the `posix` policy bit exists as a field but `snapshot_process` is **not** implemented (stage 6).
- **Processes:** `spawn` per ADR-0003 (job, image, argv and environment vectors, explicit handle list with slots, rights and move or duplicate, process-group handle, `start_suspended`, inherited-attribute block); `resume`; exit records per ADR-0004 (`code`, `reason`, `detail`); a process handle's signals per ADR-0006.
- **Request signals (ADR-0004):** `Interrupt`, `Terminate`, `Hangup`, `Quit`, `Custom(n)` with kernel-held dispositions (`Terminate`, `Ignore`, `Deliver`), `process_signal`, `job_signal` (atomic over the job tree), `job_kill`. `Deliver` makes a request pending as an observable signal on the process; no code runs asynchronously.
- **Ports (ADR-0006):** `port_create`, `port_subscribe` (level and edge, keyed by handle), `port_unsubscribe`, `port_queue` (user packets), `port_wait` with a `Deadline`; closing a handle removes its subscriptions; closing the port ends waits.
- **User-side library:** a minimal `no_std` runtime crate for Zoraya programs (system call wrappers generated from or checked against the `zoraya-abi` table), so test programs in the ramdisk are written in Rust against typed wrappers, not raw `svc`.
- **Not in scope:** exception channels (ADR-0034 stays in force: a fault ends the process), byte streams and IPC channels (designed in TASK-10), `snapshot_process`, SMP.

If an ADR invariant turns out to be impossible or contradictory once implemented, stop that part, write a proposed superseding ADR, and continue with the rest.

## Step 3: Tests

The roadmap criterion as an in-QEMU test: a process spawns a child from the ramdisk with an explicit handle list, waits on the child's exit via a port, and checks the child's `ExitRecord`.

**Every invariant of ADR-0003, ADR-0004 and ADR-0006 that stage 3 makes checkable gets its check now**, and its status in `invariants.md` becomes `checked: <command>`. This includes at least: INV-0003-1 (no implicit inheritance), INV-0003-3, INV-0003-4, INV-0003-5 (`start_suspended` via the program counter), INV-0003-6 (requested slots are observed slots), INV-0004-1, INV-0004-3 (`job_signal` reaches processes created concurrently, with the stated iteration count), INV-0004-4, INV-0004-5, INV-0006-1, INV-0006-2, INV-0006-4, INV-0006-5, INV-0006-6, INV-0006-8, and INV-0006-3 as the benchmark the ADR defines (report the measured ratio under HVF and TCG). For any invariant that still cannot be checked, record why in the ADR's open questions (ADR-0001 rule 6).

Also:
- rights: every call refuses a handle lacking the right it needs; duplicate never increases rights; transfer by move leaves the parent without the handle;
- lifetime: no object is freed while referenced; no leak after a spawn/exit/close loop (measure kernel heap and frame usage before and after many iterations);
- a stress test: many processes spawning and exiting, signals and port waits interleaved, under the scheduler's preemption;
- hostile inputs: invalid handles, wrong object types, bad user pointers, malformed spawn structures, handle-table exhaustion — each returns its documented `Status` and never panics the kernel.

`make test` passes from a clean clone under every configuration it already runs; CI stays green.

## Documentation

- `docs/design.md`: update the "as built" sections and the repository map. Do not change the Vision section. Mark stage 2 as done and stage 3 as current in the roadmap table: the architect instructs you to do this here.
- `docs/adr/index.md`, `invariants.md`, `docs/dependencies.md` as needed.

## Process

1. Read the context.
2. Step 1 ADRs. Commit.
3. Handles and object lifetime. Commit.
4. Jobs, processes, `spawn`, `resume`, exit records. Commit.
5. Request signals and dispositions. Commit.
6. Ports. Commit.
7. User runtime crate and test programs. Commit.
8. Invariant checks, tests, documentation. Commit.
9. Independent review by a subagent that did not write the code: `make test` from a fresh clone; every implemented invariant against its check, using deliberate code mutations to confirm each check fails when the property is broken (as the TASK-07 review did); handle and object lifetime for use-after-free and leaks; all `unsafe` and `SAFETY` comments; consistency of the code with each accepted ADR. Findings and dispositions in `docs/tasks/TASK-09-review.md`; fix them.
10. Final report: what was built, the `make test` summary, the invariant table delta, the proposed ADRs with one-line recommendations, open questions for the architect.

## Git

Branch `kernel/stage-3` from the latest `main`. Follow the Git section of `CLAUDE.md`: configured identity only, commit after each step, **never merge, never push**.

## Definition of done

- Proposed ADRs 0040+ committed before dependent code.
- Handles with rights, jobs, processes with `spawn`/`resume`/exit records, request signals, and ports work as the accepted ADRs specify.
- The roadmap criterion passes as an in-QEMU test.
- The listed invariants are checked, with mutation evidence from the review.
- Independent review done and addressed.

## Out of scope

- IPC channels and byte streams, the driver model, the filesystem service (TASK-10 designs them; stage 4 builds them).
- `snapshot_process` and anything POSIX (stage 6). Exception channels. SMP.
- Changing accepted ADRs except by proposing superseding ones.

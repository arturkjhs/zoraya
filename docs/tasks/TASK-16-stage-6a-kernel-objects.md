# TASK-16: Stage 6a — kernel objects for the POSIX personality

## Where and how this runs

Locally on the architect's Mac, **in parallel with TASK-17 (6b) in a separate git worktree**. Work only in the worktree this session was started in. Do not touch the other worktree, its branch, or `main`.

## Precondition

`main` is at or after tag `stage-6-design`, with ADR-0100 to ADR-0110 accepted. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/research/stage-6/plan.md` (row 6a) and `docs/research/stage-6/inputs.md`.

Read **in full**: ADR-0003, ADR-0004, ADR-0006, ADR-0031, ADR-0040, ADR-0041, ADR-0042, ADR-0046, ADR-0050, ADR-0051, ADR-0057, ADR-0101, ADR-0102, ADR-0105 (the `job_suspend`/`job_resume` part), ADR-0108, ADR-0109 (the startup block part).

This task builds the kernel side of stage 6, row 6a of `plan.md`. No `libposix`, no musl, no ported program: those are 6b to 6e.

ADR numbers **0130 to 0139** are reserved for this task. Where an accepted ADR is silent or turns out to be impossible as written, write a proposed ADR (or a proposed superseding one); never fill a gap silently.

## What to build

Exactly as the accepted ADRs specify:

- **Byte streams (ADR-0101):** `stream_create`, `stream_read`, `stream_write`, the two end types, never-blocking transfers, 4096-byte atomic writes, `PEER_CLOSED` on both ends from handle counts, signals for ports (ADR-0006). The logic in a host-tested `no_std` crate `zoraya-stream`.
- **`snapshot_process` (ADR-0102):** eager copy at the call, the flat buffer with the child's job, the self slot and the move list, generations preserved, `Duplicable` marks in `OBJECT_TYPES`, all-or-nothing against a full child table, the `posix` job bit gate (ADR-0003, INV-0003-2).
- **`image_create` (ADR-0108):** an image from a memory object the process filled.
- **`job_suspend` / `job_resume`** with the `KILL` right (ADR-0105).
- **Startup block:** the program header table (ADR-0109).
- **`object_get_signals`** for non-blocking checks (`WNOHANG`).
- `zoraya-abi`, `zoraya-rt` and the `SYSCALLS` table updated through the existing single declaration (ADR-0044).

## Tests

- The `plan.md` criterion: `t_streams`, `t_snapshot` and `t_image` pass under every machine configuration, and **a native test program snapshots itself and both halves run**, each observing its own copy of memory and the handles the move list gave it.
- Every INV-0101, INV-0102 and INV-0108 check, plus the INV-0003, INV-0040/0057 and INV-0046 checks these objects touch; statuses in `invariants.md`.
- The two risks `plan.md` names: (1) the eager copy runs with interrupts masked — measure the longest masked interval for a 1 MiB and an 8 MiB process under HVF and TCG, and state in `design.md` whether it needs to be broken up; (2) the move list's all-or-nothing behaviour against a full child table, including every partial-failure point.
- Hostile inputs: malformed snapshot buffers, move lists naming non-duplicable or foreign handles, streams written after close, huge counts. None panics the kernel.
- Leaks: loops of snapshot-and-exit, stream create-and-close with data in flight, `image_create` and drop; kernel heap and frame counts return to their start.
- Measurements under HVF and TCG: snapshot copy time of 1 MiB and 8 MiB and the frames it charges; stream transfer rate; `job_suspend` latency for a job of 16 processes.

`make test` passes from a clean clone in every configuration.

## Process

Follow `CLAUDE.md`: code and the independent review on the main model; test runs through the scripts; mutant triage through the `mutant-triage` subagent with the script as the final judge. Independent review by a subagent that did not write the code (mutation testing of the new crates and kernel paths, the copy and page-table sequences against the Arm ARM, object lifetime, `unsafe`); findings and dispositions in `docs/tasks/TASK-16-review.md`; then one check of the changed lines; if it finds errors, fix and repeat that check at most once more, then report.

## Shared files

`TASK-17` runs at the same time. Keep edits to shared files small and local, so the second merge is easy: `Cargo.toml` workspace members (add lines, do not reorder), `Makefile` (add targets, do not restructure), `docs/design.md` (your own sections only), `docs/adr/index.md` and `invariants.md` (your rows only), `docs/dependencies.md`, `docs/backlog.md`. Do not edit anything under `ports/` or `tools/zcc*`.

## Git

Branch `stage-6a` from the latest `main`, in its own worktree. Configured git identity only. Commit after each step. **Never merge, never push.**

## Definition of done

- Streams, `snapshot_process`, `image_create`, `job_suspend`/`job_resume`, the startup block change and `object_get_signals` work as the accepted ADRs specify.
- The criterion test and all listed checks and measurements exist; `make test` passes from a clean clone.
- Review and the check of its fixes done and addressed. Final report with measurements, invariant delta, proposed ADRs, open questions.

## Out of scope

`libposix`, musl, ports, the broker, the terminal, `/tmp` (6b to 6e); copy-on-write; exception channels; SMP.

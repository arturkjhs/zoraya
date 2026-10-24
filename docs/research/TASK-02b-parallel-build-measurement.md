# TASK-02b: Measure parallel builds, the jobserver and build interruption

## Context

Read first:
- `docs/research/task-02/conflict-impact.md` — especially the open question about parallel `make -j`
- `docs/research/task-02/method.md` and `docs/research/task-02/research-log.md`
- `docs/research/candidate-principles.md` — conflicts X-3 (signals) and X-4 (readiness / event mechanism)

TASK-02 measured the POSIX surface of a minimal C toolchain, but every build ran serially. The jobserver, readiness calls (`pselect`, `ppoll`, `epoll`), signal masks and interruption of a running parallel build were never exercised. Our host has many cores, so parallel builds are the normal case, not an edge case.

This task closes that gap. It measures only. It does **not** make design decisions, write OS code, or implement a compatibility layer.

## Environment

- Use the **same VM that TASK-02 used** (see `docs/research/task-02/method.md`). Do not create, recreate or reconfigure any VM. If the VM is not reachable, **stop and report**.
- **Extend** the existing pipeline in `tools/posix-surface/`. Do not rewrite it. After this task, the single entry-point script must still reproduce all TASK-02 outputs unchanged, plus the new ones.

## Workloads

Pin every program version; reuse the TASK-02 pins where possible.

- **W2-j / W3-j:** the TASK-02 Lua and SQLite builds with `make -j2`, `-j4` and `-jN` where N = number of VM cores.
  - GNU make: run with both `--jobserver-style=pipe` and `--jobserver-style=fifo` if the pinned version supports the option; record if it does not.
  - `pdpmake`: run with `-j` if supported; record if it is not.
- **W8 (recursive make):** a small synthetic project with at least two levels of recursive `$(MAKE)` and enough independent targets to keep all job slots busy, so the jobserver is actually exercised across processes. Commit it under `tools/posix-surface/workloads/`.
- **W9 (interrupting a parallel build):** start W8 or W3-j under a pseudo-terminal, with the shell as the controlling process, and interrupt it mid-build as a human would with Ctrl-C (SIGINT to the foreground process group). Separately: send SIGTERM to the top-level make only. Repeat each case several times at different points of the build.
- **W10 (shell-level parallelism):** a `dash` script that starts several background jobs (`cmd &`), waits for them with `wait`, and uses a pipeline whose stages run concurrently.

## What to measure

### M5. Syscalls and argument patterns under parallelism

Compare with the serial TASK-02 runs and report the delta:
- readiness and waiting: `pselect6`, `ppoll`, `epoll_*`, `wait4` / `waitid` (incl. `WNOHANG`), blocking `read` on the jobserver
- signal handling: `rt_sigaction` (which signals, handler or default), `rt_sigprocmask`, `rt_sigsuspend`, signals actually delivered and to which process
- jobserver mechanics: pipe or FIFO creation, inherited file descriptors, token reads and writes per job
- interruption: `EINTR` occurrences and which syscalls they hit, process-group operations, how children are terminated, which files make deletes as incomplete targets

### M6. Event sequences

For W9 and W10, reconstruct the ordered sequence of events across all processes (who sends what signal to whom, who waits on what, what each process does on receipt). Present each scenario as a short timeline. This is the input for designing a replacement event mechanism, so precision matters more than volume.

### M7. Call-site analysis for the new features

Same method and replaceability classes as TASK-02 M3 (`direct` / `restructure` / `emulate`), for every call site in make (both implementations) and dash that implements:
1. the jobserver (client and server sides)
2. readiness waiting and child reaping under `-j`
3. signal handlers and signal-mask manipulation used during parallel builds
4. cleanup on interruption (deleting incomplete targets, killing children)

For each call site, add one sentence on how it would map onto an event model where process exit, pipe readiness and user interrupt are all delivered as events on handles. Mark this as an estimate.

## Measurement honesty

Same rules as TASK-02:
- every number comes from a script in `tools/posix-surface/` and can be rerun;
- report run-to-run variation, and repeat timing-dependent scenarios (W9) enough times to show which behaviors are stable and which depend on timing;
- record failed or unreliable methods in the log with the fallback used.

## Output files

In `docs/research/task-02b/`:

1. `parallel-surface.md` — summary: what parallelism adds to the TASK-02 surface (new libc symbols, new syscalls, new argument patterns), per make implementation and jobserver style.
2. `syscall-delta.csv` — `syscall, workload, serial_count, parallel_count, notable_args`.
3. `event-timelines.md` — M6 timelines for W9 (both cases) and W10.
4. `callsite-analysis.md` — M7 results.
5. `conflict-impact.md` — update for X-3 and X-4 (and any other conflict the data touches): what the new data rules in or out, what remains unknown. **No decisions.** Also add a one-line pointer in `docs/research/task-02/conflict-impact.md` that the parallel-build question is answered here.
6. `research-log.md` — what was tried, failed, varied between runs; independent review findings.

## Process

1. Read the files listed in Context.
2. Verify VM access. Stop if unavailable.
3. Extend the pipeline; check that it still reproduces TASK-02 outputs exactly (apart from documented run-to-run variation).
4. Run W2-j, W3-j, W8, then W10, then W9.
5. Independent review by a subagent that did not write the scripts: check the timelines against raw traces and spot-check 20 rows of `syscall-delta.csv`. Record findings in the log and fix errors.
6. Write `conflict-impact.md` last, from verified data only.

Work on the branch `research/task-02b`, branched from `research/task-02`. Commit after each step. Do not merge or push.

## Git rule

Use the repository's configured git identity. If none is configured, **stop and ask**. Never pass `-c user.name` / `-c user.email` or invent an identity.

## Definition of done

- W2-j, W3-j, W8, W9, W10 run and traced; W9 repeated enough times to separate stable from timing-dependent behavior.
- M5, M6, M7 complete.
- The entry-point script reproduces TASK-02 and TASK-02b outputs.
- Independent review done and recorded.
- `conflict-impact.md` covers X-3 and X-4.

## Out of scope

- Writing OS or kernel code.
- Designing the event mechanism or making any decision.
- Creating or reconfiguring virtual machines.

# TASK-15: Design for stage 6 — POSIX personality and the self-hosting C toolchain (proposed ADRs)

## Where this runs

A **cloud session**, in parallel with stage 5 work on the architect's Mac. Needs the repository and web access. No kernel code.

## Context

Read first: `CLAUDE.md` (it binds you), `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`.

Read **in full**: ADR-0002, ADR-0003, ADR-0004, ADR-0005, ADR-0006, ADR-0031, ADR-0033, ADR-0040 to ADR-0046, ADR-0050 to ADR-0055, ADR-0057, ADR-0060, ADR-0080. **Every "Deferred to stage 6" section and every open question that names stage 6** is an input to this task: list them all first in `docs/research/stage-6/inputs.md` with their source, and make sure each is answered by an ADR below or explicitly left open with a reason.

Read the measured data: `docs/research/task-02/` and `docs/research/task-02b/` (surfaces, call sites, fork children, signals, event timelines), `docs/research/fs/requirements.md`, and the Fuchsia/Starnix, Redox, QNX and MINIX 3 sections of `alternatives-postmortem.md`.

Roadmap stage 6:

| Stage | Content | Done when |
|---|---|---|
| 6 | `libposix` T1 per ADR-0005, port of tcc, make, dash | `tcc hello.c && ./a.out` inside Zoraya |

ADR-0005 already defines the real acceptance: the T1 workloads W1 (tcc self-build), W2 and W3 (Lua and SQLite builds), W5 (interactive shell session), W2-j, W3-j and W8 to W10 (parallel builds, interrupts, background jobs), run **inside Zoraya**. This design must make that reachable.

This task writes **proposed** ADRs. The architect decides; implementation follows in separate tasks.

## Decisions to write (numbers 0100 to 0129 reserved)

Template-exact, `proposed`, "Architect's decision" empty, at least three options each with a Linux baseline and a non-Unix precedent, steelmanned, with "Effect of the vision". **One decision per ADR** — merge only if two decisions cannot be made apart, and say why. Use the measured numbers; label estimates.

1. **How `libposix` is built.** Write a C-ABI libc in Rust; port musl and replace its system-call layer with calls into native Zoraya primitives (as Fuchsia did with its libc); adopt Redox's relibc; or another route. Cover: the 248 T1 symbols (minus set-uid, plus `writev`) from `task-02/libc-surface.csv`, licensing, how much code each route needs, how ADR-0005's export invariants are checked.
2. **Byte streams and pipes.** The kernel object deferred by ADR-0050: partial reads and writes, `PeerClosed` (ADR-0004, INV-0004-7), readiness signals for ADR-0006, capacity, `PIPE_BUF` atomicity, and how `pipe()`, the jobserver pipe and shell pipelines map onto it.
3. **`snapshot_process`.** The copy-on-write process snapshot of ADR-0003 rule 3: page-table and frame mechanics, cost estimate per fork from the measured dash fork counts (1,169 no-exec forks in the core runs), what happens to memory-object mappings (ADR-0051's deferred question), to handles that cannot be duplicated (ports, channel endpoints), and to threads (there is one per POSIX process at T1).
4. **File descriptors and open file descriptions.** The fd table in `libposix`; how `dup`, `dup2`, `fork`, `spawn` and inherited fds share one open file description with one position (DM-023, the requirement ADR-0055 left for this stage); how this uses or extends the filesystem protocol; reconnection after `snapshot_process` (the handoff object was sketched in TASK-10 and not introduced: decide it here, or choose an alternative such as an explicit move list).
5. **The POSIX broker.** Pids, process groups as sub-jobs (ADR-0046), sessions, the controlling terminal, `kill`, `waitpid` across `exec`; how `exec` keeps the pid (ADR-0003); what the broker may and may not know.
6. **Terminal and job control.** How the stage 5 terminal and the kernel console become controlling terminals for dash: `TIOCSPGRP` and friends, foreground job, Ctrl-C and Ctrl-Z delivery through `job_signal`, `SIGTTIN`/`SIGTTOU` or a documented subset — using the W5 and W9 event timelines from TASK-02/02b.
7. **Signals in `libposix`.** Delivery at wait points per ADR-0004, the SIGCHLD subscription (ADR-0004/0006), `pselect` and `EINTR` (INV-0006-7), re-raising a signal on oneself (make's cleanup), the `WTERMSIG` table for `Fault` and `Limit` (ADR-0046 open question).
8. **Namespace table and the temporary directory service.** The format of ADR-0002's namespace table in `zoraya-abi`, who builds it, `/`, `/tmp`, `/dev/null`, `/dev/tty`, `$PATH`; the per-session temporary directory service with FIFO nodes and the three FIFO open behaviours GNU make relies on (INV-0006-9).
9. **Running programs from the filesystem.** Image handles from files (deferred `plan.md` step 11 of stage 4), `execve` and shebang scripts, `/bin/sh`.
10. **Building the toolchain.** Target triple, sysroot, how tcc, GNU make, dash and the T1 toybox applets are cross-compiled on the host for Zoraya (static ELF, ADR-0033), how tcc running inside Zoraya produces Zoraya executables (its linker, `crt0`, the startup block of ADR-0042), and how `tcc` itself is built inside Zoraya for W1.
11. **Acceptance harness.** How W1–W5, W2-j, W3-j and W8–W10 run inside Zoraya under `make test` (images with sources, expected outputs, the `W*-check` scripts of ADR-0005), and how the `snapshot_process` cost on W4 is measured for the T2 decision ADR-0005 asks for.

Also write `docs/research/stage-6/plan.md`: a non-binding order of implementation in **separate tasks of comparable size** (for example 6a kernel objects, 6b `libposix` core and broker, 6c toolchain port and acceptance), with what depends on which ADR and the risk of each.

## Research rules

Primary sources preferred (musl, relibc, Fuchsia fdio and Starnix docs, POSIX specification text, papers on fork cost); every URL fetched before citing; never invent citations; paraphrase. New sources in `docs/research/stage-6/sources.md` with check dates.

## Review

Independent review by a subagent that did not write the ADRs: citations support claims; options are not strawmanned; every item in `inputs.md` is answered or explicitly open; consistency with every accepted ADR (list tensions explicitly — earlier reviews found far more tensions than the author declared); invariants testable. Findings and dispositions in `docs/research/stage-6/review-notes.md`; fix them. Then **one** check of the fixes, by another subagent, on the changed lines only. If it finds errors, fix them and repeat that check at most once more; then report, whatever the result.

## Git

- Branch `adr/stage-6-design` from the latest `main`.
- Configured identity only. If none, stop and ask.
- Commit after each ADR. **You may push this one branch and nothing else.** Never merge, never push other branches, never force-push.
- Touch only `docs/adr/0100-*` to `docs/adr/0129-*` and `docs/research/stage-6/`. List the `index.md` and `invariants.md` rows to add in your final report.

## Definition of done

- `inputs.md` lists every deferred item; each is answered or explicitly left open.
- Proposed ADRs for decisions 1–11, with "Effect of the vision" and invariants.
- `plan.md` and `sources.md` exist.
- Review and the check of fixes done and addressed; branch pushed.
- Final report: each ADR with its recommendation in one line, tensions with accepted ADRs, questions for the architect.

## Out of scope

Code; T2 (autoconf) beyond the measurement plan; threads, sockets, `dlopen` (excluded by ADR-0005); ADR-0056 quotas.

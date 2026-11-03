# TASK-20: Stage 6c — the `zoraya-posix` core (files, descriptors, memory)

## Where and how this runs

Locally on the architect's Mac, **in its own git worktree** (not the main checkout). One build-and-test agent at a time on this machine: the Mac has crashed twice from memory exhaustion. Follow the memory limits now in `CLAUDE.md` and `docs/backlog.md` (heap caps in host tests, at most 2 mutant workers with timeouts, `tools/mutants/watchdog.sh`, `CARGO_BUILD_JOBS=4`).

## Precondition

`main` contains stage 6a and stage 6b (TASK-16, TASK-17) and CI is green on it. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/research/stage-6/plan.md` (row 6c) and `inputs.md`, `docs/tasks/TASK-16-review.md`, `docs/tasks/TASK-17-review.md`.

Read **in full**: ADR-0002, ADR-0005, ADR-0010, ADR-0013, ADR-0014, ADR-0051, ADR-0055, ADR-0100, ADR-0101, ADR-0103, ADR-0107, ADR-0130, ADR-0140 to ADR-0144, ADR-0153 (rule 11 and INV-0153-7: telling a path outside the grants from a missing file).

This task builds row 6c of `plan.md`: the Rust half of `libposix` for files, descriptors and memory, replacing the ENOSYS stubs of ADR-0140 for those functions. **Process functions (`fork`, `posix_spawn`, `wait*`, signals, `kill`, terminal) stay stubs**: they are 6d.

ADR numbers **0180 to 0189** are reserved for this task. Where an accepted ADR is silent or wrong once implemented, write a proposed ADR or a proposed superseding one; never fill a gap silently.

## What to build

As ADR-0100, ADR-0103 and ADR-0107 specify:

- **Startup** from the bootstrap record (ADR-0107): the namespace table, the standard descriptors, `environ`, `argv`.
- **The resolver:** longest-prefix lexical resolution over the namespace table, `..` as ADR-0107 and INV-0002-4 (as amended by the shell decisions) say, relative paths from the current directory.
- **Descriptors and open file descriptions (ADR-0103):** the fd table, `dup`, `dup2`, `fcntl` for the measured commands, the position kept in the service's open description.
- **The file protocol additions** to `zoraya-proto-file` and `zoraya-fs-service`: `share`, `seek`, `current`, `set_flags`, `open_with_mode` (ADR-0103, ADR-0014 rule 6), with host tests of the protocol before any guest code, and `narrow` if ADR-0153's accepted change to ADR-0055 needs it here.
- **File functions** of the T1 tier list: `open`/`openat` family, `read`, `write`, `readv`/`writev`, `close`, `lseek`, the `stat` family synthesised from ADR-0014's metadata, `rename`, `unlink`, `mkdir`, `rmdir`, `opendir`/`readdir`, `access`, `getcwd`/`chdir`, `umask` and mode handling as ADR-0014 defines, `fsync`/`sync` with ADR-0010's semantics.
- **`pipe`** over 6a's byte streams (ADR-0101).
- **Memory:** `malloc` and friends over memory objects (ADR-0051), private `mmap`/`munmap` as the tier list needs.
- **`errno`**: one table mapping native statuses and file-protocol statuses to `errno` values, documented.
- **Refused paths (INV-0153-7):** the resolver tells "outside what this process was given" from "does not exist" and records it for the shell's `REPORT` channel as ADR-0153 rule 11 says, while the C program still sees the `errno` the ADR prescribes.
- **The export checker** and `posix-surface-T1.txt` as `plan.md` names them; `posix-surface-check` keeps passing with real functions in place of stubs.

## Tests

- **The `plan.md` criterion:** a static C test program built with 6b's `zcc` runs in QEMU from a native launcher and creates, writes, reads, renames, stats and removes files through `libposix` against `fsd`, including files with Ukrainian and Czech names, directories, `dup`/`dup2` sharing one position, a `pipe` between two descriptors of one process, and `malloc` under load.
- Host tests for the resolver, the fd table, the `errno` mapping and the protocol additions, with property tests for the resolver (no path escapes a namespace entry; `..` behaviour as specified).
- Every invariant `plan.md` lists for 6c (INV-0100-1 to -4, INV-0103-1, -3, -5, INV-0107-1 to -3) plus INV-0153-7's `libposix` half; statuses in `invariants.md`.
- The two risks `plan.md` names, each with a recorded outcome: the musl shim header's completeness (every internal system call musl's retained files make is routed or proven unreachable), and `share` semantics against the service's single-threaded loop (two descriptions, interleaved requests, no lost position updates).
- Hostile inputs at the C boundary: invalid descriptors, `NULL` and unmapped pointers where POSIX allows `EFAULT`, absurd sizes, paths with control characters and leading `/` outside the namespace. Nothing crashes the service or the kernel.
- Measurements under HVF and TCG: the `share` and `seek` round trip, the resolver's cost per `open`, read and write throughput through `libposix` for 4 KiB and 1 MiB.

`make test` passes from a clean clone in every configuration, and CI is green after your branch is merged.

## Process

Follow `CLAUDE.md`: code and the independent review on the main model; test runs through the scripts; mutant triage through the `mutant-triage` subagent with the script as the final judge. Independent review by a subagent that did not write the code, with mutation testing of the new Rust code under the memory limits; findings and dispositions in `docs/tasks/TASK-20-review.md`; then one check of the changed lines; if it finds errors, fix and repeat that check at most once more, then report.

## Git

Branch `stage-6c` from the latest `main`, in its own worktree. Configured git identity only. Commit after each step. **Never merge, never push.** The stage tag is set only after CI is green on `main`, by the merge step the architect requests.

## Definition of done

- The file, descriptor, pipe and memory functions of the T1 tier list are real; process functions remain stubs.
- The criterion test, the listed checks and measurements exist; `make test` passes from a clean clone.
- Review and the check of its fixes done and addressed. Final report with measurements, invariant delta, proposed ADRs, open questions.

## Out of scope

`fork`, `posix_spawn`, `exec`, `wait*`, signals, `kill`, the broker, the terminal and job control, `/tmp` and FIFOs (6d); acceptance workloads (6e); T2; threads.

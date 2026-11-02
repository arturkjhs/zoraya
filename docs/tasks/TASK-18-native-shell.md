# TASK-18: The native shell — design and implementation

## Why

`zosh` today is a demonstration shell (TASK-11). It becomes **the shell people use on Zoraya**. The architect's direction:

- **Not POSIX, not a Linux-style shell.** dash is ported in stage 6 only for compatibility: `make` recipes, `configure` scripts, `#!/bin/sh` scripts (ADR-0005, ADR-0109). People do not work in dash. If the interactive shell copied POSIX `sh`, Zoraya's face would be Linux with another kernel.
- **Simple and intuitive in the FreeBSD sense:** one coherent design for the shell and the commands around it, the principle of least astonishment (POLA), documentation that answers questions where they arise.
- **Pleasant and good-looking in the macOS sense:** discoverable, forgiving, clear feedback, nothing cryptic.
- **Native to Zoraya's model:** a program reaches only what it is given (ADR-0002, ADR-0003). The shell is where a person sees and decides what a program gets.

## Where and how this runs

Part 1 (research and design) needs only the repository and web access: it can run in a cloud session or locally. Part 2 (implementation) needs QEMU and runs locally on the architect's Mac. **There is a mandatory stop between them** for the architect's decisions.

## Context

Read first: `CLAUDE.md`, `docs/design.md` (Vision, "Using the shell", "Using the GUI"), `docs/adr/index.md`, `docs/backlog.md`.

Read **in full**: ADR-0002, ADR-0003, ADR-0004, ADR-0006, ADR-0013, ADR-0046, ADR-0053, ADR-0055, ADR-0060 to ADR-0063, ADR-0096, ADR-0101, ADR-0102, ADR-0104, ADR-0105, ADR-0107, ADR-0108. Read the current `zosh` source and its tests. Read the research catalog entries in categories 11 (terminals, sessions, consoles) and 14 (configuration, observability and system management) of `docs/research/design-mistakes.md` and `held-up-well.md`.

ADR numbers **0150 to 0159** are reserved for this task.

## Part 1: research and design (no code)

### Research

Write `docs/research/shell/survey.md`. For each shell below: what it is good at for a person typing commands, what confuses people, and what the evidence is (documentation, design papers or talks by the authors, well-known user complaints with sources). Primary sources preferred; every URL fetched; never invent a citation; paraphrase.

At least: POSIX `sh` and FreeBSD's `/bin/sh` (how FreeBSD makes it usable), bash and zsh (as the baseline of what people know), Plan 9 `rc`, fish (a deliberately non-POSIX friendly shell), Nushell and PowerShell (structured data in pipelines), and Oils/YSH (a cleaned-up shell language). Add FreeBSD's base-system conventions beyond the shell: man pages, `rc.conf`/`sysrc`, `service`, consistent utility flags, and FreeBSD's stated POLA principle.

Also summarise the classic shell pitfalls with sources: word splitting and quoting, glob surprises, error handling (`set -e`), exit statuses, text-only pipelines.

### Decisions (proposed ADRs, one decision per ADR)

Template-exact, `proposed`, at least three options each, with POSIX `sh` as the baseline and at least one non-POSIX precedent. Each ADR includes **examples of what a person types and sees**. At least:

1. **Language core.** Words, quoting, variables, lists, no implicit word splitting (or a justified alternative), globbing rules, comments. What a person who knows bash will trip over, and why that is acceptable.
2. **Control flow, functions and errors.** `if`, loops, functions, how a failed command is noticed and handled, how exit records (ADR-0004: exit, terminated by request, killed, fault) appear in the language and on screen.
3. **Pipelines and data.** Byte streams (ADR-0101) only, or structured values between native commands as well (Nushell/PowerShell style), and how either works with POSIX programs that only speak bytes.
4. **What a program gets.** The central Zoraya decision. By default, what handles does a command receive: the terminal, the current directory, read-only or writable, nothing else? How a person grants more (for example a directory, a file, the network later) with a short, readable syntax; how the shell shows what was granted; what POLA means here. Running POSIX programs (tcc, make, dash scripts) through the broker and the namespace table (ADR-0104, ADR-0107) must work, and what they see must also be explicit.
5. **Interactive experience.** Line editing, history and history search, Tab completion (commands, paths through the file protocol, options from each command's own description), suggestions as you type (fish-style) or not, syntax highlighting or not, how errors are shown (what went wrong, where, and what to try), Unicode input and UTF-8 file names (ADR-0013), working identically on the serial console and the GUI terminal (extend ADR-0096's sequence subset only if needed, by a superseding note).
6. **Commands and their conventions.** The style guide every native command follows: naming, one option syntax, `--help` output format, error message format, exit codes, quiet by default (a successful command prints only its result), colour use. Which commands are builtins and which are programs.
7. **Help and documentation.** `help` and a manual viewer: one source format, where pages live, how a command's description feeds `--help`, completion and the manual at once, so they cannot disagree.
8. **Configuration.** The shell's startup file and the system configuration in the spirit of FreeBSD's `rc.conf`: one readable file of simple settings, edited by a command as well as by hand. Where the file lives given that Zoraya has no users or uids natively (ADR-0002).
9. **Job control.** Background jobs, `jobs`, foreground, Ctrl-C, Ctrl-Z (ADR-0105's `job_suspend` from 6a), shown simply.

### The language tour

Write `docs/research/shell/tour.md`: a short guided tour as a new user would read it — 20 to 40 realistic examples covering daily work (files, finding things, running a program with a granted directory, a pipeline, a small script, a loop, handling an error, a background job, getting help), each with its exact screen output. This is what the architect judges the design by.

Also write `docs/research/shell/plan.md`: implementation order for Part 2, what depends on which ADR, and what depends on stage 6 tasks (byte streams from 6a, running POSIX programs from 6c/6d).

### Review of Part 1

Independent review by a subagent that did not write the design: consistency with accepted ADRs (list tensions explicitly), whether the tour matches the ADRs exactly, whether any rule would surprise a user (POLA), whether the capability syntax is both safe and short. Findings in `docs/research/shell/review-notes.md`; fix them; one check of the changed lines.

### STOP

After Part 1, **stop and report** to the architect: the ADRs with one-line recommendations, the tour, the open questions. Do not start Part 2 until the architect has decided the ADRs. The architect may change the language after reading the tour.

## Part 2: implementation (after the architect's decisions)

Implement the accepted ADRs, in the order of `plan.md`:

- The language: parser and evaluator in a host-tested `no_std` crate (for example `zoraya-shell`), with fuzzing of the parser on arbitrary input.
- The interactive layer: line editor, history, completion, suggestions and highlighting as decided, on both the serial console and the GUI terminal.
- The capability grants exactly as decided, with the shell showing what each command received.
- The native commands' conventions: a shared crate for option parsing, `--help`, error messages and exit codes; existing commands (`ls`, `cat`, `write`, `mkdir`, `rm`, `mv`, `stat`, `sync`, `jobs`, …) brought to the style guide.
- `help` and the manual viewer, with pages for every builtin and command.
- Configuration file and its command.
- Parts that depend on stage 6 (running POSIX programs, pipelines through byte streams, Ctrl-Z) are built when those tasks are on `main`; until then they are listed as pending in `design.md`.

Tests: every invariant of the new ADRs; the **whole tour as an automated scripted session** on the serial console and in the GUI (the existing screendump-to-text test), so the documentation cannot drift from the behaviour; hostile input to the parser; grants that must be refused; completion and history; `make test` passes from a clean clone.

Review: independent review with mutation testing of the shell crate; findings in `docs/tasks/TASK-18-review.md`; one check of the changed lines; at most one more round, then report.

## Documentation

`docs/design.md`: rewrite "Using the shell" from the tour. A section "Conventions for commands" pointing to the style guide ADR. `index.md`, `invariants.md`, `backlog.md`.

## Git

- Part 1: branch `shell/design` from the latest `main`. In a cloud session you may push this branch only; locally, never push.
- Part 2: branch `shell/impl` from `main` after Part 1 is merged. Never merge, never push.
- Configured git identity only. Commit after each step. Touch only the shell, its crates, the native commands, their tests and docs, and your reserved ADR numbers; do not edit kernel code or stage 6 work in progress.

## Definition of done

- Part 1: survey, nine (or justified fewer/more) proposed ADRs with examples, the tour, the plan, the review; stopped and reported.
- Part 2: the accepted design implemented; the tour runs as a test on both console and GUI; `make test` passes; review done and addressed.

## Out of scope

POSIX compatibility in `zosh` (that is dash's job); the GUI design system and window management (a separate stage 7 task); networking.

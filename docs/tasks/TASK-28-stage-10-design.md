# TASK-28: Design for stage 10 — living in the system

## Where and how this runs

On the architect's **MacBook Air M1**, in its own clone, **documents only**: no `make`, `cargo` or QEMU; reading the code to match the design to what exists is expected. The Mac mini runs the stage 9 dispatcher (TASK-27) at the same time; this task never touches its branches. Web access is needed. Main model: **Opus** (a design task); fetching and summarising sources, checking links may go to Haiku subagents. The architect is travelling: **do not ask; decide, record, continue.** Keep the machine awake (`caffeinate -dimsu` in a tmux window).

## Why

After stage 9 Zoraya runs real programs (curl, Python, a browser), but a person still cannot *live* in it: there is no full-screen terminal program, no editor beyond `edit`, a partial toybox, no `less`, no `git`, no way to log into another machine. Stage 10 makes a working day inside Zoraya possible: open a terminal, edit files in `vi`, page with `less`, use the usual tools, commit with `git`, reach another machine with `ssh`. And it must stay Zoraya: a program gets what the line says, and the person sees it.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/tasks/TASK-27-report.md` (its status block), `docs/research/stage-6/` (the POSIX tiers, the ports' method, `applets-by-tier.tsv`).

Read **in full**: ADR-0005, ADR-0100 to ADR-0110, ADR-0140 to ADR-0144 (libposix, ports, `zcc`), ADR-0106 (signals at wait points), ADR-0153 to ADR-0158 (grants, descriptions, `narrow`), ADR-0167 (the chooser), ADR-0172 (rooms), ADR-0090 to ADR-0097 and the terminal's and `ttyd`'s code (what the terminal emulates today, termios, job control), ADR-0220 to ADR-0230 and ADR-0310/0311 (network grants for POSIX programs), ADR-0300 to ADR-0319 (threads, record locks).

ADR numbers **0330 to 0349** are reserved for this task.

## Part 1: research

`docs/research/stage-10/survey.md`, evidence rules as in TASK-01 (primary sources, every URL fetched, never invent a citation, paraphrase):

- **Terminals:** what full-screen programs actually use: the xterm control sequences `vi`, `less`, `vim`, `htop`, `tmux`, Python's REPL and `ncurses` emit (cursor addressing, scroll regions, alternate screen, colours: 16 / 256 / direct, bracketed paste, mouse reporting, title setting, `DECSET`s), the window size (`TIOCGWINSZ`, `SIGWINCH`), UTF-8 and character width (`wcwidth`, East Asian wide). Sources: xterm's ctlseqs, ECMA-48, terminfo(5), what foot, kitty, alacritty and st implement and why they left things out.
- **Curses:** ncurses vs NetBSD curses vs a minimal one; terminfo compiled in vs a database file; size and licences.
- **Editors and pagers:** nvi (BSD, small), vim (tiny/small builds), a Kilo-class editor, `less`, `mg`. What each needs from libc and the terminal.
- **Userland:** toybox's full T2 set vs busybox vs BSD tools; the gaps measured in `applets-by-tier.tsv`; an `awk` (one-true-awk); `tar`, `gzip`/`xz`, `diff`/`patch`, `find`, `grep`, `sed`.
- **Pseudo-terminals:** what needs a pty (`tmux`/`screen`, `script`, an ssh server, `expect`, Python's `pty`), how Fuchsia, Redox, Plan 9 and seL4-based systems do it without Unix's `/dev/ptmx`.
- **git and ssh:** git's needs (zlib, curl, `fork`/`exec`, file locking, `mmap`), an ssh client (OpenSSH vs dropbear): keys, `known_hosts`, the agent, `ProxyCommand`.
- **Capability systems with interactive tools:** how a program that opens files the person names *while it runs* (`:e file` in vi, `git checkout`, `less` with `:n`) gets them on capability systems: Capsicum's `casper`, Plash, the powerbox pattern, Genode's file system sessions, Fuchsia's directory handles, the Mac's sandbox and security-scoped bookmarks.

## Part 2: decisions (proposed ADRs, one decision per ADR)

Template-exact, `proposed`, "Architect's decision" empty, at least three options each with a Linux/FreeBSD baseline and a non-Unix precedent, steelmanned, "Effect of the vision", **examples of what a person types and sees** in `zosh` and on the desktop, and invariants a later implementation can check, with how. At least:

1. **The terminal's emulation:** which control sequences, which `TERM` name and terminfo entry, colours, alternate screen, mouse, bracketed paste, the window size and `SIGWINCH` through `ttyd`; the conformance test (for example vttest's subset, or recorded sessions of `vi`, `less` and `ncurses` test programs replayed against the terminal).
2. **Curses and terminfo:** which library, how the terminfo entry ships, what `TERM` a program sees.
3. **Editor and pager:** which `vi`, which `less`, built how; what `edit` remains for.
4. **The userland set:** toybox's full set or another; which tools come from where; the order.
5. **Files an interactive program opens while it runs:** the central question. The line `vi ~/notes.txt` grants that file; `:e ~/other.txt` inside `vi` names a file the shell never saw. Options at least: grant directories on the line (`+w ~/proj vi`); a trusted prompt the terminal draws (the program asks, `ttyd`/the terminal shows the chooser in its own frame, the person confirms); a room for the session (`room new work +w ~/proj`); refuse and say so. What the person sees in each case, as text (rule 3 of the design).
6. **Pseudo-terminals:** whether stage 10 needs them, and how a pty pair is a capability (no global `/dev/ptmx`).
7. **git:** what the description grants (the repository directory, `.git`, `~/.gitconfig`), the network for `git clone https://...`, credentials; what `grants git clone URL dir` shows.
8. **ssh:** client only or also a server; keys under which grant; `known_hosts`; how `+net HOST:22` and the description fit; what the person sees on a host-key change.

Also `docs/research/stage-10/plan.md`: implementation parts of comparable size (for example 10a the terminal and terminfo, 10b curses and `less`, 10c `vi`, 10d the userland, 10e the runtime file question, 10f git, 10g ssh), each with a visible result, its "done when" test, its dependencies, and which can run in parallel on one 16 GB machine.

## Review

Independent review by a subagent on Opus that did not write the ADRs: citations fetched and accurate; options not strawmanned; consistency with accepted ADRs, listing every tension (ADR-0005, ADR-0153, ADR-0167, ADR-0172, ADR-0228 and ADR-0310/0311 above all); whether the defaults are safe and the syntax short. Findings and dispositions in `docs/research/stage-10/review-notes.md`; fix; one check of the changed lines; at most one more round.

## Git

Branch `adr/stage-10` from the latest `main`, in its own clone. Configured identity only. Commit after each part and push this one branch only; never merge, never force-push, never touch `work/*` or `main`. Touch only `docs/adr/0330-*` to `docs/adr/0349-*` and `docs/research/stage-10/`. List the `index.md`, `invariants.md` and roadmap rows in `docs/research/stage-10/rows-to-add.md`.

On a usage-limit warning: commit, push, write where you stopped in `docs/research/stage-10/status.md`, stop.

## Definition of done

Survey; proposed ADRs for decisions 1 to 8 (merged or split with reasons); `plan.md`; review and the check of fixes; branch pushed. Final report: each ADR's recommendation in one line; the answer to decision 5 in one paragraph with what the person types and sees; the tensions; the questions for the architect.

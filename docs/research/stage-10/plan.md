# Stage 10 implementation outline

Non-binding, part of TASK-28. It orders the work; the ADRs decide. Numbers refer to proposed ADR-0331 to ADR-0342 in
`docs/adr/`. Nothing here starts before the architect accepts the ADRs a part depends on. The parts are cut so that
each ends in something the architect can watch happen, and to roughly comparable size (a few thousand lines of
code, patches and tests each, by estimate; nothing was built to size them, and the review judged the first cut
uneven, so the terminal work is three parts here). Every part ends with `make check` and `make test-changed`
passing locally and its candidate run green.

**Dependencies outside this plan.**

- *Stage 9 as it lands* (TASK-27, on another machine): the ports' build, OpenSSL and curl's library (ADR-0310,
  ADR-0312) for 10g and 10h; CPython (9c) is not needed by any part, but its REPL is a ready test of 10c and 10d
  once both exist.
- *ADR-0212's image cache*: the architect's order of 2026-11-04 puts it before the T2 names. 10e assumes it is
  done or does it first; without it every part works, slower.
- *Decided and not built in the network* (`survey.md`, "The network as built"): the refusal report and hint for
  hosts (ADR-0224 item 5). 10f builds it if stage 8's owners have not.
- *Not decided before this task:* a network connection and the random service for a child of a POSIX program
  (ADR-0339 item 7 proposes it; 10h builds it).
- *Three new description keys* (`data`, `reach`, `repository`), two operand kinds (`rest`, `create-tree`), the
  `GRANTS` slot and `narrow` with a list of names all touch `zoraya-describe`, the shell's planner or the file
  protocol. They are in three parts (10f, 10g, 10h); whichever merges second rebases on the first.

**What the plan does not contain.** A pseudo-terminal (ADR-0338 builds none), an ssh server, `scp` and `sftp`
beyond a follow-up line in 10g, git over ssh, a key agent, credential storage, fonts for wide characters,
full-screen programs on the serial console, the grant line for untrusted rooms.

| Part | Work | ADRs | Depends on | Visible result | Done when | Risk |
|---|---|---|---|---|---|---|
| 10a | **The terminal's parser and grid.** `zoraya-term`: the sequences of ADR-0332 items 2 and 3, sub-parameters, the alternate grid, 256 and direct colours, the larger cell, attributes, tab stops, the two character sets, LF as line feed (with the shell adding the carriage return to its own and native output, ADR-0331 item 8), scrollback as a setting, `Restore`. The width table generated from the pinned musl's data, used by the grid and by the shell's editor. The pinned ncurses archive, a host `tic`, the four entry files and their manifest (ADR-0333). The conformance tool with its exceptions file, and the recorded sessions (recorded on the host). | 0332 (items 1 to 6, 9 to 11), 0333 (item 2), 0331 (item 8) | none | `cat` of a recorded `vi` session in a terminal window shows the editor's screen in colour, and the prompt afterwards is clean | INV-0332-1, -2, -3, -4, -6, INV-0331-7 and INV-0333-1 are checked; `tests/gui/terminal.script` reads a replayed full-screen recording by glyph; the fuzz test of INV-0096-1 passes on the new parser | the size of the parser rewrite; the first grids need a person's comparison with xterm |
| 10b | **Keys, the mouse, selection and paste.** The key table (function keys, Alt, modifiers, application cursor keys) in `term` and `zoraya-keymap`, checked against the entry; mouse reports (1000, 1002, 1006), focus events; selection with invisible cells copied as spaces, `Copy` and `Paste` as a clipboard client (ADR-0170); the `Paste` message and the shell's handling of it at the prompt. | 0332 (items 7, 8) | 10a | text selected with the mouse in a terminal is pasted at the prompt with System+V and waits for Enter; F-keys and Ctrl+arrows show their sequences in a test program | INV-0332-5 (the prompt's half) and INV-0332-7 are checked; the key half of INV-0332-1 passes | key handling across four layouts |
| 10c | **A POSIX program has a terminal from `zosh`.** `zoraya-tty`: `VMIN`/`VTIME`, flush, drain, line tags and the refusal of reads and mode changes from another line. `zosh` hosts the discipline: terminal connections for the line's programs, the line in front, `Size` to `SIGWINCH`, `Restore`, the modes kept across a stop, the attributes at a line's end and at `fg`, the paste path to a program with its confirmation. The broker's control channel to `zosh` for a POSIX shell's `tcsetpgrp`. `libposix`: the readiness request, `open("/dev/tty")`, `ttyname`, and the terminal names of ADR-0342 items 4 and 5 with a committed C test program behind them; the shim's two names retired; the deny list. `grants`' `keys` line. `TERM` for programs with a terminal; `console.term`. The measurement of ADR-0331 item 14. The same discipline features in the session's `ttyd`. | 0331, 0342 (items 3 to 5, 12), 0333 (items 1, 4, 5), 0338 | 10a (for `Restore`); 10b (for the paste path) | `sh` typed in a desktop terminal gives a `$` prompt with Ctrl-C and Ctrl-Z working; a small raw-mode C program draws a box that follows the window as it is resized; a background program cannot read what is typed | INV-0331-1 to -6 and -8, INV-0332-5 (the program's half), INV-0333-2 and -3, INV-0338-1 and -2, INV-0342-2 and -3 are checked; the measured numbers are in the task report; `dash.cmd` no longer says it has no terminal | the shell's event loop under raw-mode traffic; the broker change; the relay's cost (the measurement and the security argument decide whether ADR-0331's Option C is taken instead) |
| 10d | **curses, `less` and `vi`.** The ncurses port (two static archives, headers in the sysroot, no home lookup, letter-named directories). `less` under the Less License. nvi2 with a makefile of the ports, its DB 1 library, and its patches (`:script`, `flock`, recovery, start-up files). Their link probes and `posix-surface-added` blocks, which take over the terminal names from 10c's test program. `vi.cmd`, `ex.cmd`, `view.cmd`, `less.cmd` with `cwd none`, their `help`; the hints for `less` and `more`. A curses test program for `KEY_RESIZE`. If `vi a.txt b.txt` is to work, `narrow` with a list of names (else it lands in 10f). | 0334, 0335, 0342 | 10a, 10b, 10c | `vi notes.txt` in a terminal window: edit, `:wq`, `cat notes.txt` shows the change; `tcc -v &\| less` pages | INV-0334-1 to -3 and INV-0335-1 to -4 are checked; a GUI script edits and saves a file through `vi` with only its one-name grant and is refused a sibling | nvi2's DB 1 dependency and patch set; DB 1's fall-back when a file cannot be mapped |
| 10e | **The userland.** The image cache if it is not there. The probe of toybox's list and its block of libc names (`getline`, `mkdtemp`, the user and group lookups, `link` as `EPERM`, the wide-character names, the locale test); `ports/toybox-applets.txt` and the build from it; the multi-call binary out of `/bin`; the in-place patch for `sed -i` and `patch`; a description and a `help` page for each name in `/bin`, with the lint of INV-0336-2; the review of `tr`, `expr` and `diff` and the T1 workloads run again on them; one-true-awk with its grammar generated on the host; busybox out of the image and `sources.lock`. W4 run for its measurement. | 0336, 0342 (items 1, 2, 7 to 10) | none (parallel with 10a to 10d) | `grep -n TODO *.c`, `sed -i`, `tar -c -z -f` and `awk` work from `zosh`, and `grants` shows what each gets; `configure` of GNU hello gets past `sed` | INV-0336-1 to -6, INV-0342-1, -4, -5 are checked; W4's result is in the task report | about sixty descriptions, each a small security statement; the pending commands' review may reject one |
| 10f | **Giving a file to a running program.** Reports per path and access, read during the run, filtered and quoted (also in the existing hint); the network report and hint (ADR-0224 item 5) if not built; `Status` and `Attention` in `zoraya-proto-term`; the status row in `term`; System+G in the compositor; the grant line in the shell's editor, with its guard, its literal-only parsing and its element choice; the `GRANTS` slot in both records, read by `libposix` and `zoraya-rt` on a miss; `narrow` with a list of names; `jobs` showing run-time grants. | 0337 | 10c; 10d for the test with `vi` | in `vi`, `:e ~/other.txt` shows the refusal on the status line; System+G, `+r ~/other.txt`, Enter, `:e!` shows the file; a hand-made hostile report shows only `a path that cannot be shown` | INV-0337-1 to -7 are checked; a GUI script plays the scenario of the ADR's "What the person sees" | the first mechanism of its kind: the input path and the report filter must be reviewed as a security boundary (the `opus-review` agent) |
| 10g | **A program's own directory, and ssh.** The `data` key with its `read` form, `/data/ROOM` in the shell's table (from the panel and `init`), the planner, `HOME` for such programs, `grants`' two names, the read-only mark for children in `libposix`, no designation into `/data`, the storage limit on `/data/ROOM`, the name list and its lint. The `reach` key and the `rest` operand kind. OpenSSH's client and `ssh-keygen` on the image's OpenSSL, the configuration reader and the askpass variables removed, the user-name patch, their probe and block. The runner's ssh server on the host with throwaway keys. `ssh.cmd`, `ssh-keygen.cmd`, `help`. As a follow-up in the same part if time allows: `scp` and `sftp` with an operand kind for `HOST:PATH`. | 0341, 0340, 0342 (item 11) | 10c; 10f for the host hint | `ssh-keygen`, then `ssh -l NAME HOST` to the runner's server: the fingerprint question, a remote shell, `vi` on the far side drawn in the window; a second server key is refused | INV-0341-1 to -7 and INV-0340-1 to -4 are checked; `make test-net` gains an ssh script | OpenSSH's portability layer against `libposix`; a host ssh server in the tests and on CI's runner |
| 10h | **git.** A child's network connection and random service (ADR-0339 item 7). The `repository` key, the planner's search and `GIT_CEILING_DIRECTORIES`; the `create-tree` kind. git with its switches, the library of the pinned curl, the system `gitconfig`, the configuration-path, certificate and environment patches, its probe and block. `git.cmd` and `help`. The runner serving a test repository over https (for push, the host's git behind the test server). | 0339, 0342 | 10d (`vi`, `less`), 10g (the `data` key) | in `~/proj`: `git init`, `git add`, `git commit` (the message typed in `vi`), `git log` in `less`; `git clone https://test.zoraya/...` and, with `+net`, `git push` | INV-0339-1 to -5 are checked; `make test-apps` gains a git script against the runner's servers | the largest port; smart HTTP needs a real git on the host side of the tests; git's future Rust requirement |
| 10i | **Living in it.** `docs/tour.md` gains the working day (a terminal, `vi`, `less`, the tools, a grant given while running, git, ssh) with its exact screens, played by the tour's scripts; `docs/design.md`'s terminal, shell and ports sections made true; `vttest` and `esctest` by hand as a checklist with its results in the task report; the stage's full run. | all | 10a to 10h | the tour's new chapter, played in the window | the tour scripts pass in the window (and on the serial console for the chapters that are not full-screen); the checklist's results are recorded; `candidate-full/` is green | the tour fixes exact screen text, so late wording changes cost a script run |

## Order and what can run in parallel

Two lanes fit one 16 GB machine under `CLAUDE.md`'s limits (at most two worktrees with builds, at most three QEMU
guests and one host-test job):

- **Lane 1, the terminal:** 10a, then 10b, then 10c, then 10d, then 10f.
- **Lane 2, programs:** 10e, then 10g (once 10c is merged), then 10h (once 10d and 10g are merged).

10a and 10e share nothing: the width table is generated from musl's files and changes nothing in `libc.a`. 10f and
10g both change `zoraya-describe` and the shell's planner (see above) and should not be in review at the same time.
10i runs alone at the end.

The ports' builds are the memory-heavy steps (ncurses, OpenSSH, git; OpenSSL is stage 9's): one port build at a
time, and never beside a mutation run. A mutation run (`make mutants-host`) belongs to 10a (`zoraya-term`), 10c
(`zoraya-tty`) and 10f (`zoraya-shell`), under the watchdog, when the other lane is idle.

## What each part hands the architect

- 10a: two screenshots, a replayed recording here and in xterm, for the first comparison ADR-0332 item 10 asks for.
- 10c: the relay measurement, with a recommendation on ADR-0331's open choice between the shell and a `ttyd` per
  window.
- 10d: the measured sizes of a static `vi` and `less`, for ADR-0334's open question.
- 10e: W4's numbers (ADR-0005's condition for T2), and the list of options each description lets through.
- 10f: a hand-run of the grant line in a first terminal, in a room's own terminal and in a shell that entered a
  room; the review of the input path and the report filter.
- 10g, 10h: hand-runs against a real host (a login; a clone and a push), which the tests' servers cannot replace.
- 10i: the checklist's results and the tour.

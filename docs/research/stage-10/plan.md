# Stage 10 implementation outline

Non-binding, part of TASK-28. It orders the work; the ADRs decide. Numbers refer to proposed ADR-0331 to ADR-0342 in
`docs/adr/`. Nothing here starts before the architect accepts the ADRs a part depends on. The parts are cut to
comparable size (each a few thousand lines of code, patches and tests, by estimate; nothing was built to size
them) and so that each ends in something the architect can watch happen. Every part ends with `make check` and
`make test-changed` passing locally and its candidate run green.

**Dependencies outside this plan.**

- *Stage 9 as it lands* (TASK-27, on another machine): the ports' build, OpenSSL and curl's library (ADR-0310,
  ADR-0312) for 10f and 10g; CPython (9c) is not needed by any part, but its REPL is a ready test of 10b and 10c
  once both exist.
- *ADR-0212's image cache*: the architect's order of 2026-11-04 puts it before the T2 names. 10d assumes it is
  done or does it first; without it every part works, slower.
- *Decided and not built in the network* (`survey.md`, "The network as built"): the refusal report and hint for
  hosts (ADR-0224 item 5), a child's inheritance of its parent's network connection and of the random service
  (ADR-0225 item 9; ADR-0310's open question). 10e builds the first, 10g the second; if stage 8's owners build
  them earlier, those steps fall away.
- *Three new description keys* (`data`, `repository`, `reach`) and the `GRANTS` slot all touch `zoraya-describe`
  and the shell's planner. They are in three parts (10e, 10f, 10g); whichever merges second rebases on the first.

**What the plan does not contain.** A pseudo-terminal (ADR-0338 builds none), an ssh server, `scp` and `sftp`
beyond a follow-up line in 10f, git over ssh, a key agent, fonts for wide characters.

| Part | Work | ADRs | Depends on | Visible result | Done when | Risk |
|---|---|---|---|---|---|---|
| 10a | **The terminal becomes an xterm-256color.** The parser of `zoraya-term` (the sequences of ADR-0332 items 2 and 3, sub-parameters, the alternate grid, 256 and direct colours, the larger cell, attributes, tab stops, the two character sets); LF as line feed with `zosh` writing CR LF; the width crate from pinned Unicode data, used by the grid; the key table (function keys, Alt, modifiers, application cursor keys) in `term` and `zoraya-keymap`; mouse reports, selection, `Copy` and `Paste` in `term`; the `Reply`, `Paste` and `Restore` messages of `zoraya-proto-term` with the shell dropping every reply for now; scrollback as a setting. The pinned ncurses archive, a host `tic`, the four entry files and their manifest (ADR-0333). The conformance tool and the recorded sessions (recorded on the host). | 0332, 0333 | none | `cat` of a recorded `vi` session in a terminal window shows the editor's screen in colour; text selected with the mouse is pasted at the prompt and waits for Enter | INV-0332-1, -2, -4, -6, -7 and INV-0333-1 are checked; `tests/gui/terminal.script` reads a replayed full-screen recording by glyph; the fuzz test of INV-0096-1 passes on the new parser | the size of the parser rewrite; the first grids need a person's comparison with xterm |
| 10b | **A POSIX program has a terminal from `zosh`.** `zoraya-tty`: `VMIN`/`VTIME`, flush and drain. `zosh` hosts the discipline: terminal connections for the line's programs, the job in front, `Size` to `SIGWINCH`, `Restore` and the attributes at a job's end and at `fg`, replies passed only to a raw-mode job in front. `libposix`: the readiness request, `open("/dev/tty")`, `ttyname`, and the first block of ADR-0342 (`tcsetattr`, `cfmakeraw`, `tcflush`, `tcdrain`, `ctermid`, `poll`, `ppoll`, `select`, `tcgetwinsize`), with the shim's two names retired. `TERM` for programs with a terminal; `console.term`. The measurement of ADR-0331 item 12. The same features in the session's `ttyd`. | 0331, 0342 (items 3 to 5), 0333 (items 1, 5) | 10a for `Restore` and replies (the rest is independent) | `sh` typed in a desktop terminal gives a `$` prompt with Ctrl-C and Ctrl-Z working; a small raw-mode C program draws a box that follows the window as it is resized | INV-0331-1 to -6, INV-0332-3, INV-0333-2 and -3, INV-0342-2 are checked; the measured numbers are in the task report; `dash.cmd` no longer says it has no terminal | the shell's event loop under raw-mode traffic; the relay's cost (the measurement decides whether ADR-0331's Option C is reopened) |
| 10c | **curses, `less` and `vi`.** The ncurses port (two static archives, headers in the sysroot); `less` under the Less License; nvi2 with its DB 1 library, recovery and start-up files compiled out; first of all, the check that nvi saves in place (ADR-0335's assumption). Their link probes and `posix-surface-added` blocks. `vi.cmd`, `ex.cmd`, `view.cmd`, `less.cmd`, their `help`; the hints for `less` and `more`. A curses test program for `KEY_RESIZE`. | 0334, 0335, 0342 | 10a, 10b | `vi notes.txt` in a terminal window: edit, `:wq`, `cat notes.txt` shows the change; `tcc -v &\| less` pages | INV-0334-1 to -3 and INV-0335-1 to -4 are checked; a GUI script and a serial script each edit and save a file through `vi` with only its one-name grant | nvi2's DB 1 dependency; if nvi saves by rename, ADR-0335 is reopened before the rest of the part |
| 10d | **The userland.** The image cache if it is not there. The probe of toybox's list and its block of libc names (`getline`, `mkdtemp`, the user and group lookups, `link` as `EPERM`, the wide-character names with the width table, the locale test); `ports/toybox-applets.txt` and the build from it; a description and a `help` page for each command, with the lint of INV-0336-2; the review of `tr`, `expr` and `diff` and the workloads `w5` and `w10` on them; one-true-awk with its grammar generated on the host; busybox out of the image and `sources.lock`. W4 run for its measurement. | 0336, 0342 (items 7 to 10) | none (parallel with 10a to 10c) | `grep -n TODO *.c`, `sed -i`, `tar -c -z -f` and `awk` work from `zosh`, and `grants` shows what each gets; `configure` of GNU hello gets past `sed` | INV-0336-1 to -5, INV-0342-1, -3, -4, -5, INV-0332-5 are checked; W4's result is in the task report | about sixty descriptions, each a small security statement; the pending commands' review may reject one |
| 10e | **Giving a file to a running program.** The `GRANTS` slot in both records, read by `libposix` and `zoraya-rt` on a miss; reports read during the run, with the write-inside-a-read-entry case; the network report and hint (ADR-0224 item 5) if not built; `Status` and `Attention` in `zoraya-proto-term`; the status row in `term`; System+G in the compositor; `shell.attention` on the serial console; the grant line in the shell's editor with the suggestion; `jobs` showing run-time grants; the compositor's card for an untrusted room. | 0337 | 10b; 10c for the test with `vi` | in `vi`, `:e ~/other.txt` shows the refusal on the status line; System+G, Right, Enter, `:e!` shows the file; the same with Ctrl-] on the serial console | INV-0337-1 to -7 are checked; a GUI script plays the scenario of the ADR's "What the person sees" | the first mechanism of its kind: the input path must be reviewed as a security boundary (the `opus-review` agent) |
| 10f | **A program's own directory, and ssh.** The `data` key, `/data/ROOM` in the shell's table (from the panel and `init`), the planner, `grants`' two names, the name list and its lint. The `reach` key. OpenSSH's client and `ssh-keygen` on the image's OpenSSL, the configuration reader disabled, the user-name patch, their probe and block (`getnameinfo`, the user lookups). The runner's ssh server on the host with throwaway keys. `ssh.cmd`, `ssh-keygen.cmd`, `help`. As a follow-up in the same part if time allows: `scp` and `sftp` with an operand kind for `HOST:PATH`. | 0341, 0340, 0342 (item 11) | 10b; 10e for the host hint | `ssh-keygen`, then `ssh -l NAME HOST` to the runner's server: the fingerprint question, a remote shell, `vi` on the far side drawn in the window; a second server key is refused | INV-0341-1 to -5 and INV-0340-1 to -4 are checked; `make test-net` gains an ssh script | OpenSSH's portability layer against `libposix`; a host ssh server in the tests and on CI's runner |
| 10g | **git.** A child's inheritance of the network connection and the random service (ADR-0225 item 9). The `repository` key and the planner's search. git with its switches, the library of the pinned curl, the system `gitconfig`, the certificate patches, its probe and block. `git.cmd` and `help`. The runner serving a test repository over https (for push, the host's git behind the test server). | 0339, 0342 | 10c (`vi`, `less`), 10f (the `data` key) | in `~/proj`: `git init`, `git add`, `git commit` (the message typed in `vi`), `git log` in `less`; `git clone https://test.zoraya/...` and, with `+net`, `git push` | INV-0339-1 to -5 are checked; `make test-apps` gains a git script against the runner's servers | the largest port; smart HTTP needs a real git on the host side of the tests; git's future Rust requirement |
| 10h | **Living in it.** `docs/tour.md` gains the working day (a terminal, `vi`, `less`, the tools, a grant given while running, git, ssh) with its exact screens, played by the tour's scripts; `docs/design.md`'s terminal, shell and ports sections made true; `vttest` and `esctest` by hand as a checklist with its results in the task report; the stage's full run. | all | 10a to 10g | the tour's new chapter, played in the window | the tour scripts pass on the serial console and in the window; the checklist's results are recorded; `candidate-full/` is green | the tour fixes exact screen text, so late wording changes cost a script run |

## Order and what can run in parallel

Two lanes fit one 16 GB machine under `CLAUDE.md`'s limits (at most two worktrees with builds, at most three QEMU
guests and one host-test job):

- **Lane 1, the terminal:** 10a, then 10b, then 10c, then 10e.
- **Lane 2, programs:** 10d, then 10f (once 10b is merged), then 10g (once 10c and 10f are merged).

10a and 10d share nothing but the width crate, which 10a creates and 10d links into `libposix`; whichever is
second takes it from `main`. 10e and 10f both change `zoraya-describe` and the shell's planner (see above) and
should not be in review at the same time. 10h runs alone at the end.

The ports' builds are the memory-heavy steps (OpenSSL, git): one port build at a time, and never beside a
mutation run. A mutation run (`make mutants-host`) belongs to 10a (`zoraya-term`), 10b (`zoraya-tty`) and 10e
(`zoraya-shell`), under the watchdog, when the other lane is idle.

## What each part hands the architect

- 10a: two screenshots, a replayed recording here and in xterm, for the first comparison ADR-0332 item 10 asks for.
- 10b: the relay measurement, with a recommendation on ADR-0331's revisit trigger.
- 10c: the answer to "does nvi save in place" before anything else.
- 10d: W4's numbers (ADR-0005's condition for T2), and the list of options each description lets through.
- 10e: a hand-run of the grant line in a trusted and an untrusted room; the review of the input path.
- 10f, 10g: hand-runs against a real host (a clone and a push; a login), which the tests' servers cannot replace.
- 10h: the checklist's results and the tour.

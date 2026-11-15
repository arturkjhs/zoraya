# The architect's queue

One place for what waits for the architect: proposed ADRs, decisions an agent took to continue (which the architect may
overrule), hand checks with their exact commands, and open questions. Oldest first inside each section. The agent that
finishes a part (or a task's dispatcher) **updates this file in the same push as the part's report**: add what the part
adds, remove what the architect has decided (the decision itself is recorded in the ADR or `docs/backlog.md`).

**Both dispatchers** (TASK-31 on the Mac mini, TASK-34 on the MacBook Air) keep this file. TASK-34's dispatcher: add your
items here too, under the same headings, with the part's name in front.

Last updated: 2026-11-15 (the architect's decisions of the day recorded; after stage 10a, 10b, 10e, `poll`, `py-hook`,
`test-fixes-2`, `ci-expr`; 10c reviewed, its fix pass on `work/10c`).

## A. Proposed ADRs, with the agent's one-line recommendation

| ADR | What | Recommendation | From |
|---|---|---|---|
| ADR-0392 | libposix: a signal sent during `execve` follows the new image (`exec_carry`); a handler that ran at a call's entry ends its first wait with `EINTR`; `exec_hold` marks default-terminating signals `Deliver` during the spawn | **Tension with accepted ADR-0106** (INV-0106-2: a take in every handler-less `execve`; item 2's mapping during the spawn). Recommend Option A (library route); the fallback A' is a kernel change. Yours to accept or to choose A' | sigint-exec |
| ADR-0390, ADR-0391 | 10d: surface for curses, less and vi; the layout, descriptions and refusals of 10d | Option A both. ADR-0391 item 8 (two files of one directory) is now a refusal and an open question: should `narrow` take a list? | 10d |
| ADR-0410 | The first modern browser engine: WPE WebKit without a JIT (Ladybird if a Rust `std` target for POSIX programs is accepted) | Yours; the choice turns on the Rust policy | TASK-33 |
| ADR-0386 | 10c: the shell's host of the discipline (`Restore` and `Reinstate` as messages, 2004 read from the output and believed only out of canonical mode, the held paste's question in the title, nobody reads with no line in front) | Option A | 10c |
| ADR-0387 | 10c: `zosh`'s lines on the discipline (which fds are the terminal, the whole line reads until `tcsetpgrp`, a native line's `Restore` before the next prompt) | Option A. Item 4 (no kill offer for a line with the terminal) **decided** 2026-11-15: accepted with the System+K escape, built in 10f (recorded in the ADR's open questions) | 10c |
| ADR-0388 | 10c: the broker's notices to `zosh` (`ForegroundAsked` names the line by its root; `Background` makes a `tcsetpgrp` of a line not in front `EPERM` at once, as decided 2026-11-15; a signal to a group reaches no group made inside it) | Option A as amended | 10c |
| ADR-0389 | 10c: the terminal calls of `libposix` (the readiness request, `tcsetattr`'s actions, `tcflush`, `tcdrain`, `/dev/tty`, `ttyname`, `TIOCSTI` `ENOTTY`, `TIOCSWINSZ` `EPERM`) | Option A | 10c |

## B. Decisions taken to continue (the architect may overrule)

- **10e:** the ADR-0212 image cache was not built (a broker, `posix_child` and `libposix` change with its own security
  argument); the userland does not depend on it. The independent review of `tr`, `expr` and `diff` was done by an
  `opus-review` (decided 2026-11-15 that it counts).
- **10a:** the shell writes a fixed reset before a prompt that follows a command, until 10c sends `Restore` (done on
  `work/10c`: the reset is gone).
- **10c, keep the shell (ADR-0331 Option B) for the relay:** one HVF run at 1024x768 (`t_tty bench`): a pipe between two
  programs 350 MB/s; 1 MiB to the window through `zosh` and `term` 457 ms (2.3 MB/s); a full 80x24 repaint 0.57 ms; a
  byte then `tcdrain` mean 133 us, worst 351 us. The split of the time between `zosh` and `term` (ADR-0331's "more than half"
  condition) was not measured: a counter of `pump_relay`'s time is the follow-up.
- **10c, decisions where the ADRs are silent** (ADR-0386 to 0389): `Restore` and `Reinstate` as two messages; mode 2004 read
  from the program's output, believed only out of canonical mode; the held paste's question in the window's title until
  ADR-0337's status line; newlines of a paste sent as carriage returns; a paste past 16 KiB cut; with no line in front
  nobody reads; a change of the front line drops unread input; at most 64 terminal connections per line; a `Clone` from a
  line not in front is still answered (refusing it would fail `fork` in a background line, `docs/backlog.md`); the signal
  characters and `SIGWINCH` go to the group a `tcsetpgrp` chose, through the broker.
- **10b:** a paste is marked in the shell's key stream by bytes 0xF8 to 0xFF, which no key can make; a paste while a program
  runs is dropped with a one-line notice; copy and paste drop zero-width and bidi characters and cells whose colours differ
  by 8 or less per channel.
- **`work/poll`:** a terminal polled for input spins until 10c; a program polling a terminal and a socket starves the socket.
- **`work/test-fixes-3` (in review):** CI option A (`--cap-add=SYS_ADMIN --cap-add=NET_ADMIN` on the network test jobs'
  container only); fall back to recording the gap if the container refuses it.
- **`work/py-hook`:** the hook builds a second CPython, in the test job (4 minutes in CI).
- **Test infrastructure:** the semaphore, barrier and latch groups of libc++'s tests run only in the nightly (TCG);
  INV-0042-1 stays `not yet applicable`. INV-0172-4's narrowing of `room new` inside a room is built and checked in 10c.

## C. Hand checks (exact commands)

1. **10a, xterm comparison** (ADR-0332 item 10): in an 80x24 xterm run `TERM=xterm-256color cat crates/zoraya-term/recordings/vi-edit.rec`
   and compare with the same recording in a Zoraya terminal (screenshots: `docs/tasks/TASK-31-part-10a-shots/`).
2. **10a, `vttest`:** needs a pin in `ports/sources.lock` (not pinned, so not recorded).
   `python3 -c 'import sqlite3, ssl, asyncio'` and `ping google.com` in the guest.
4. **Later parts:** 10f's grant line (first terminal, a room's terminal, a shell that entered a room), 10g's `ssh -l NAME HOST`
   to a real host, 10h's `git clone` and `git push`: commands will be listed with the part.

## D. Open questions

- **git's licence in a static program (ADR-0347, ports-early):** git is GPL-2.0-only and `git-remote-http` links libcurl with Apache-2.0 OpenSSL statically; GPL-2.0-only and Apache-2.0 are not combinable, so `git-remote-http` cannot be shipped as built (GNU wget is GPL-3.0-or-later, which is compatible). Options: git over a GPL-compatible TLS library, `git-remote-http` as a program not linked to OpenSSL (a helper process), an OpenSSL exception, or no HTTPS transport. Decide before 10h wires git into the image.
- **DB 1 for nvi (ADR-0390):** 1.85 under the 4-clause UC licence; a maintained copy is an open question.

- **Volume programs:** ADR-0396, 0397 and 0399 are accepted; implementation is part 10j after 10g.
- **A killed room's threads are parked while they die (ADR-0285 item 7, `work/smp-tcg`):** the dying thread runs its
  teardown and is charged and parked like the room's other threads, so a killed room at a low ceiling takes as many
  periods to die as its teardown needs (measured: 55 parks after the kill, nine periods; up to 12 periods). Options:
  charge but do not park an ending process's thread, or move the teardown to the reaper. A proposed ADR needs a number:
  none of 0380..0399 outside 0390..0395 is free on the remote branches. ADR-0285's open questions have the note.


## E. Decided 2026-11-15 and scheduled (removed from the lists above; see `docs/backlog.md`)

ADR-0280, 0281, 0285, 0286, 0384, 0385, 0398 accepted; ADR-0383 Option C (`kbs=^?`, built in 10d); the kill offer's escape
System+K (10f); native `fetch` follows ADR-0328; ADR-0224 item 5's `+net` hint in curl's and wget's refusal; a refused
`tcsetpgrp` returns `EPERM` (10c); INV-0324-2, INV-0326-1 and ADR-0366's count reworded; the confined network steps run on
the runner itself in a job of their own; `vttest` pinned and built with 10d.

## For the TASK-34 dispatcher (MacBook Air)

The CI policy of ADR-0348 (the architect, 2026-11-15) applies to this dispatcher as it does to the Mac mini's: a push to
`work/*` runs `make check` only (no full `workflow_dispatch` runs, except to chase a flake); a candidate runs the steps
`tools/ci-plan.sh` selects (the full set only for `kernel/`, libposix or `tools/test-runner/`; a `Makefile` change maps to the
steps whose recipes or variables it changes); do not wait for `main`'s run after a merge, the next part merges as soon as its
own candidate is green; `main`'s full set runs every 4 hours if `main` changed, and nightly; a red scheduled run
(`gh run list --workflow 'make test' --event schedule`; a skipping 4-hourly run fails if the last full run was red) is fixed first, before the next merge, and its summary lists the merges
since the last green one; a stage tag needs a green scheduled or nightly run that includes the tagged commit (`tools/ci-last-full.rb --green-for SHA`).

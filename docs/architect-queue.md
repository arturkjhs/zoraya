# The architect's queue

One place for what waits for the architect: proposed ADRs, decisions an agent took to continue (which the architect may
overrule), hand checks with their exact commands, and open questions. Oldest first inside each section. The agent that
finishes a part (or a task's dispatcher) **updates this file in the same push as the part's report**: add what the part
adds, remove what the architect has decided (the decision itself is recorded in the ADR or `docs/backlog.md`).

**Both dispatchers** (TASK-31 on the Mac mini, TASK-34 on the MacBook Air) keep this file. TASK-34's dispatcher: add your
items here too, under the same headings, with the part's name in front.

Last updated: 2026-11-15 (the architect's decisions of the day recorded; after stage 10a, 10b, 10e, `poll`, `py-hook`, `test-fixes-2`, `ci-expr`; 10c in review).

## A. Proposed ADRs, with the agent's one-line recommendation

| ADR | What | Recommendation | From |
|---|---|---|---|
| ADR-0410 | The first modern browser engine: WPE WebKit without a JIT (Ladybird if a Rust `std` target for POSIX programs is accepted) | Yours; the choice turns on the Rust policy | TASK-33 |
| ADR-0386, 0387, 0388, 0389 | 10c: the tty discipline in `zosh`, zosh's lines, the broker's notices, the terminal names | On branch `work/10c`, not yet merged; recommendations in `docs/tasks/TASK-31-part-10c.md` | 10c |

## B. Decisions taken to continue (the architect may overrule)

- **10e:** the ADR-0212 image cache was not built (a broker, `posix_child` and `libposix` change with its own security
  argument); the userland does not depend on it. The independent review of `tr`, `expr` and `diff` was done by an
  `opus-review` (decided 2026-11-15 that it counts).
- **10a:** the shell writes a fixed reset before a prompt that follows a command, until 10c sends `Restore` (done on
  `work/10c`).
- **10b:** a paste is marked in the shell's key stream by bytes 0xF8 to 0xFF, which no key can make; a paste while a program
  runs is dropped with a one-line notice; copy and paste drop zero-width and bidi characters and cells whose colours differ
  by 8 or less per channel.
- **`work/poll`:** a terminal polled for input spins until 10c; a program polling a terminal and a socket starves the socket.
- **`work/test-fixes-3` (in review):** CI option A (`--cap-add=SYS_ADMIN --cap-add=NET_ADMIN` on the network test jobs'
  container only); fall back to recording the gap if the container refuses it.
- **`work/py-hook`:** the hook builds a second CPython, in the test job (4 minutes in CI).
- **Test infrastructure:** the semaphore, barrier and latch groups of libc++'s tests run only in the nightly (TCG);
  INV-0042-1 stays `not yet applicable`; INV-0172-4 (a room's shell cannot create a room with more than the room holds)
  waits for its check (10c or the next part touching `zosh`'s rooms).

## C. Hand checks (exact commands)

1. **10a, xterm comparison** (ADR-0332 item 10): in an 80x24 xterm run `TERM=xterm-256color cat crates/zoraya-term/recordings/vi-edit.rec`
   and compare with the same recording in a Zoraya terminal (screenshots: `docs/tasks/TASK-31-part-10a-shots/`).
2. **10a, `vttest`:** needs a pin in `ports/sources.lock` (not pinned, so not recorded).
   `python3 -c 'import sqlite3, ssl, asyncio'` and `ping google.com` in the guest.
4. **Later parts:** 10f's grant line (first terminal, a room's terminal, a shell that entered a room), 10g's `ssh -l NAME HOST`
   to a real host, 10h's `git clone` and `git push`: commands will be listed with the part.

## D. Open questions


- **Volume programs:** ADR-0396, 0397 and 0399 are accepted; implementation is part 10j after 10g.


## E. Decided 2026-11-15 and scheduled (removed from the lists above; see `docs/backlog.md`)

ADR-0280, 0281, 0285, 0286, 0384, 0385, 0398 accepted; ADR-0383 Option C (`kbs=^?`, built in 10d); the kill offer's escape
System+K (10f); native `fetch` follows ADR-0328; ADR-0224 item 5's `+net` hint in curl's and wget's refusal; a refused
`tcsetpgrp` returns `EPERM` (10c); INV-0324-2, INV-0326-1 and ADR-0366's count reworded; the confined network steps run on
the runner itself in a job of their own; `vttest` pinned and built with 10d.

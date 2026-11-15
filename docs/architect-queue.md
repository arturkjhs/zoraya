# The architect's queue

One place for what waits for the architect: proposed ADRs, decisions an agent took to continue (which the architect may
overrule), hand checks with their exact commands, and open questions. Oldest first inside each section. The agent that
finishes a part (or a task's dispatcher) **updates this file in the same push as the part's report**: add what the part
adds, remove what the architect has decided (the decision itself is recorded in the ADR or `docs/backlog.md`).

**Both dispatchers** (TASK-31 on the Mac mini, TASK-34 on the MacBook Air) keep this file. TASK-34's dispatcher: add your
items here too, under the same headings, with the part's name in front.

Last updated: 2026-11-15 (after stage 10a, 10b, 10e, `poll`, `py-hook`, `test-fixes-2`, `ci-expr`; 10c reviewed, its fix pass
on `work/10c`).

## A. Proposed ADRs, with the agent's one-line recommendation

| ADR | What | Recommendation | From |
|---|---|---|---|
| ADR-0280 | The frame slot layout and the driver's side of the frame protocol (N1) | Accept: a slot is a bare Ethernet frame; the virtio header lives in the driver's queue memory | stage 8 (TASK-26) |
| ADR-0281 | What N2 decided where the network ADRs are silent (smoltcp 0.14.0 from crates.io, the scope's encoding, a diagnostic echo request, first supervision) | Accept; vendoring is the alternative to the registry crate | stage 8 |
| ADR-0285 | What Q4 decided where ADR-0264 is silent (one parked list, a slice floor of 50 us, `cpu` 1 to 1000, a 120-byte usage record) | Accept | stage 8 |
| ADR-0286 | What N3 decided where the network ADRs are silent (`connect` with the client's streams, the copy loop, the resolver, a `url` operand that designates, a test server) | Accept | stage 8 |
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
3. **Not done by anyone yet** (from the stage 9 handoff): `netsurf https://example.org/` from a desktop terminal;
   `python3 -c 'import sqlite3, ssl, asyncio'` and `ping google.com` in the guest.
4. **Later parts:** 10f's grant line (first terminal, a room's terminal, a shell that entered a room), 10g's `ssh -l NAME HOST`
   to a real host, 10h's `git clone` and `git push`: commands will be listed with the part.

## D. Open questions

- **CI and the confined network steps:** `test-net-lookup`, `test-net-dns` and `test-hand-run`'s network leg run only locally (CI cannot confine
  them: no `unshare -n` in the job container, even with extra capabilities; `--verify` may read the host's `/sys/class/net`, where
  `/proc/net/dev` would follow the process). A CI runner with unprivileged user namespaces would let them run (`work/test-fixes-3`).

- **INV-0324-2's wording** (a public-only name rule is within a web rule; proposed text in the TASK-27 report, "9g-url, for the architect").
- **INV-0326-1's wording** (its check covers only the grant that comes with `window`).
- **ADR-0366** and its index row say 51 added names; the list holds 52.
- **Native `fetch`** keeps the old name rule (a rebinding can reach the LAN): should it follow ADR-0328 as curl and wget do?
- **curl and wget refusal text** shows no `+net` hint (ADR-0224 item 5's hint is not built).
- **ADR-0230:** what `--net-open` may reach (decided 2026-11-15: the runner's own servers only; implemented in `work/test-fixes-3`).
- **Volume programs:** ADR-0396, 0397 and 0399 are accepted; implementation is part 10j after 10g.

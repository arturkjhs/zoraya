# The architect's queue

One place for what waits for the architect: proposed ADRs, decisions an agent took to continue (which the architect may
overrule), hand checks with their exact commands, and open questions. Oldest first inside each section. The agent that
finishes a part (or a task's dispatcher) **updates this file in the same push as the part's report**: add what the part
adds, remove what the architect has decided (the decision itself is recorded in the ADR or `docs/backlog.md`).

**Both dispatchers** (TASK-31 on the Mac mini, TASK-34 on the MacBook Air) keep this file. TASK-34's dispatcher: add your
items here too, under the same headings, with the part's name in front.

Last updated: 2026-11-16 (the architect's decisions of 2026-11-16 recorded; before that 2026-11-15, the architect's decisions of the day recorded; after stage 10a, 10b, 10e, `poll`, `py-hook`,
`test-fixes-2`, `ci-expr`; 10c reviewed, its fix pass on `work/10c`).

## A. Proposed ADRs, with the agent's one-line recommendation

| ADR | What | Recommendation | From |
|---|---|---|---|
| ADR-0460 | Charge but do not park the thread of a process that is ending (a killed room dies at once). Option A is the architect's decision of 2026-11-16; the ADR is for you to accept after the implementation | Option A (decided); a QEMU kernel test is to be built by another agent | `work/smp-tcg`, TASK-31 |
| ADR-0461 | git's licence: `git-remote-http` links a libcurl built with mbedTLS 3.6 LTS (its GPL-2.0-or-later option), for git only. Decided by the architect 2026-11-16; it must be accepted before 10h wires git in | Option A (decided) | ports-early, TASK-31 |
| ADR-0410 | The first modern browser engine: WPE WebKit without a JIT (Ladybird if a Rust `std` target for POSIX programs is accepted) | Yours; the choice turns on the Rust policy | TASK-33 |
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


- **Volume programs:** ADR-0396, 0397 and 0399 are accepted; implementation is part 10j after 10g.



## E. Decided 2026-11-16 (removed from the lists above)

- ADR-0392 Option A accepted (a signal sent during `execve` follows the new image; a handler run at a call's entry ends its first wait with `EINTR`); the amendments of ADR-0106 accepted (INV-0106-2 gains `execve`, item 2's mapping holds during the spawn as amended).
- ADR-0386, ADR-0387, ADR-0390, ADR-0391 Option A accepted. ADR-0391 item 8: `narrow` takes a list of names (ADR-0337 item 8), to be built (10f built `narrow` with a list at run time; 10j or 10f for the rest).
- DB 1.85's licence accepted for nvi (ADR-0390, ADR-0347): the University of California rescinded the advertising clause of the 4-clause BSD licence in 1999, so it reads as 3-clause BSD.
- git's licence (ADR-0347): `git-remote-http` links a libcurl built with mbedTLS 3.6 LTS, its GPL-2.0-or-later option, for git only (ADR-0461, proposed; before 10h).
- A killed room's threads: charged but not parked while they end (Option A of ADR-0460, accepted 2026-11-16 with the overdraft as built; ADR-0285's note resolved).
- ADR-0460 open question 2: the overdraft, not acceptance. The overrun of an ending process is debt that the room and its parents pay back in the following periods (cap 100 budgets; a dead room's debt goes to its nearest ancestor with a ceiling); built on `work/park-ending`, INV-0460-4.

- `curses-flake`: the GUI runner's per-step limit stays 120 s with the factor 4 under TCG (480 s); extended to the serial-line scripts (`limit SECONDS STEP`, explicit limits for the steps that wait long).
- `curses-flake`: zosh's prompt clears the rest of its row (Option A: `\r` and `ESC [ K` before the prompt); built in `editor.start`.

## E. Decided 2026-11-15 and scheduled (removed from the lists above; see `docs/backlog.md`)

ADR-0280, 0281, 0285, 0286, 0384, 0385, 0398 accepted; ADR-0383 Option C (`kbs=^?`, built in 10d); the kill offer's escape
System+K (10f); native `fetch` follows ADR-0328; ADR-0224 item 5's `+net` hint in curl's and wget's refusal; a refused
`tcsetpgrp` returns `EPERM` (10c); INV-0324-2, INV-0326-1 and ADR-0366's count reworded; the confined network steps run on
the runner itself in a job of their own; `vttest` pinned and built with 10d.

## Condition of the untrusted-room gate (ADR-0268), from F6 (ADR-0438, 2026-11-16)

**An image must be charged to the room it is started in.** Since F6 `image_create` copies a program's pages into an object charged to the
job that calls it. The shell (`user/src/posix_child.rs` `load`, called from `user/src/bin/zosh.rs` `ready_from_volume`) and the broker
make the image for a program they start, so a room's memory limit no longer covers the text of the programs run in it (before F6 a spawn
copied the pages into the child's job). A room with `--mem 64M` can run a program of any size and pays nothing for its text; the shell's
job pays, one image per start, while any process of the room runs from it. ADR-0262's rule (what is made for a client is paid by the
client) says the room pays.

*What F6 found (estimate; not built).* The fix is `image_create(memory, length, job)`: the kernel charges `job` (`process::image_from`
already takes the payer) after checking `CREATE_PROCESS` on it, the right it already needs to put a process there, so no program can charge
a job it could not spawn into and no new right is needed. The shell passes `self.room_job.unwrap_or(JOB)`; the broker the job of the
child it loads for; `posixrun` and `libposix`'s `load_fd` their own job (slot 2 / `PROCESS_JOB`, which has `CREATE_PROCESS`). Touches: the
syscall table and signature test in `zoraya-abi` (`lib.rs` line 1238, `tests.rs` lines 37-44 and 431), `zoraya-rt`, `zoraya-user`, the
kernel handler, `posix_child.rs`, `zosh.rs` (two calls), `broker.rs`, `posixrun.rs`, `crates/zoraya-posix/src/ffi/procs.rs`, and the 14
calls of `t_image`. Judged not small: a kernel ABI change with six callers in the shell, the broker and the library, and the steps that load
programs through the shell (`test-posix`, `test-apps`, the Python lanes) are more than a part session runs. *Tests the gate needs:* a room at
its memory limit cannot start a program through the shell (a script step: `room new r --mem 2M`, then a program whose image is larger is refused
with the room's `OverQuota`, the room's `top` row unchanged), and a kernel test that `image_create` with a job the caller cannot spawn into
is `AccessDenied`.

## For the TASK-34 dispatcher (MacBook Air)

The CI policy of ADR-0348 (the architect, 2026-11-15) applies to this dispatcher as it does to the Mac mini's: a push to
`work/*` runs `make check` only (no full `workflow_dispatch` runs, except to chase a flake); a candidate runs the steps
`tools/ci-plan.sh` selects (the full set only for `kernel/`, libposix or `tools/test-runner/`; a `Makefile` change maps to the
steps whose recipes or variables it changes); do not wait for `main`'s run after a merge, the next part merges as soon as its
own candidate is green; `main`'s full set runs every 4 hours if `main` changed, and nightly; a red scheduled run
(`gh run list --workflow 'make test' --event schedule`; a skipping 4-hourly run fails if the last full run was red) is fixed first, before the next merge, and its summary lists the merges
since the last green one; a stage tag needs a green scheduled or nightly run that includes the tagged commit (`tools/ci-last-full.rb --green-for SHA`).

**2026-11-15, for the TASK-34 dispatcher:** the scheduled full run 43749759496 (head `9b78d380`) is green, 78 of 78 jobs,
including the TCG `w9-term` job and all five `test-qemu-*` jobs. `main`'s red scheduled run (`w9-term`, run 43699859694) was
fixed first (`work/w9-fix`, merged as `0693daf`; the cause was the test's own poll loop starving `make -j4` under TCG).
That run includes F3's merge (`db938461`) and F2's, so `stage-f3` can be tagged and `stage-f2`'s tag confirmed
(`tools/ci-last-full.rb --green-for SHA` names that run for each). The Mac mini tagged `stage-10c` and `stage-10d` on it.

**2026-11-16, from plan part F7 (ADR-0439 and ADR-0440, proposed):** (1) The Unix half of F7 is built for the line's `/tmp` only; a socket file in a
program's own directory needs a kind for "own directory" in the bootstrap record and a transient node in `fsd`: say whether it is wanted
before tmux (ADR-0439 item 1, open question). (2) TCP `bind`/`listen`/`accept` is not built, so `python3 -m http.server` does not serve to `curl`: decide
whether it becomes its own plan part with a network step (ADR-0440: the listen rule the scope already has, a `listen` request, a loopback
interface in `netd` after a smoltcp spike), and the word a person types for a listen rule (ADR-0225 item 5).

**2026-11-16, F7 review:** a socket name authenticates nothing: any program of the line that can write `/tmp` can unlink or rename a live
server's name and bind its own. Should `unlink` and `rename` of a node with a listener be restricted to the connection that made it
(ADR-0439)?

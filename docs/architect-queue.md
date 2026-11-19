# The architect's queue

One place for what waits for the architect: proposed ADRs, decisions an agent took to continue (which the architect may
overrule), hand checks with their exact commands, and open questions. Oldest first inside each section. The agent that
finishes a part (or a task's dispatcher) **updates this file in the same push as the part's report**: add what the part
adds, remove what the architect has decided (the decision itself is recorded in the ADR or `docs/backlog.md`).

**Both dispatchers** (TASK-31 on the Mac mini, TASK-34 on the MacBook Air) keep this file. TASK-34's dispatcher: add your
items here too, under the same headings, with the part's name in front.

Last updated: 2026-11-18 (the architect's decisions of 2026-11-18 recorded; before that those of 2026-11-16; before that 2026-11-15, the architect's decisions of the day recorded; after stage 10a, 10b, 10e, `poll`, `py-hook`,
`test-fixes-2`, `ci-expr`; 10c reviewed, its fix pass on `work/10c`).

## A. Proposed ADRs, with the agent's one-line recommendation

| ADR | What | Recommendation | From |
|---|---|---|---|
| ADR-0460 | Charge but do not park the thread of a process that is ending (a killed room dies at once). Option A is the architect's decision of 2026-11-16; the ADR is for you to accept after the implementation | Option A (decided); a QEMU kernel test is to be built by another agent | `work/smp-tcg`, TASK-31 |
| ADR-0394 | 10j: what the installed-program part decided where ADR-0396, 0397 and 0399 are silent (the record's date, the name's form, the copy, a typed word, what a hand-edited record may do, `--kill`), and what it did not build: the Programs page of `settings` (INV-0399-4) and INV-0397-6's grant-line script | Option A now; the page as Option C, a part of its own (the panel writes the record through `zoraya_settings::apps`) | 10j |
| ADR-0464 | What 10h decided: forms of `git` (`sub`), the clone directory (`derive`), only the transport helper has a connection, the ceiling, ancestors of an entry not reported, a child keeps the write of a `+w` entry | Option A (as built); NOT MERGED: see `docs/tasks/TASK-31-review-10h.md` | `work/10h`, TASK-31 |
| ADR-0395 | 10f: the status row, System+G and System+K, the grant line and **the delivery of a grant to a running POSIX program** (`GRANTS` slot 1006, `ZGR1`, `NarrowNames` ordinal 44), the network refusal report; a `+net` word and a native program are not delivered | Option A as built | 10f |
| ADR-0410 | The first modern browser engine: WPE WebKit without a JIT (Ladybird if a Rust `std` target for POSIX programs is accepted) | Yours; the choice turns on the Rust policy | TASK-33 |

## B. Decisions taken to continue (the architect may overrule)

- **10f (2026-11-19):** `grantable()` does not refuse `/data` (ADR-0341 item 3 does not say `Grants` must, and the abi tests use a `/data/x` entry); a program's own directory is protected by `Held::fixed`. `tests/gui/spinners.script` reads the memhog refusal without its amount (it varies 8960 to 9088 KiB). Open: `cpu_ceiling_from_a_process` failed once under a parallel `test-changed` and passed alone.
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

- **10g, ADR-0288/0435:** a launcher's connection to the random service (`ShareLauncher`, only `init`'s may ask) so that
  the shells the panel starts are shells, not programs, for `rngd`.
- **10g, ADR-0340 INV-0340-4:** its half "`libc.a` exports no `listen` or `accept`" is false since the Unix sockets
  (ADR-0430, ADR-0439); the row says "checked: ... (partly ...)"; the new text of the invariant is yours.
- **10g, not built:** the `/data/ROOM` storage limit (ADR-0341 item 6), `scp` and `sftp`; a file held open for writing in an
  own directory stays writable in a child that inherits it (ADR-0393 item 13, before 10h's git hooks).

- **10j, `+w /` is refused on every line** (ADR-0396 item 2, built as written): `tests/shell/system-ro.script` used it for "a wide grant does not reach /system" and uses `+w /home` now; `docs/tour.md` only sets a variable to that text.
- **10j, the name of an installed program** is at most 24 bytes (the description's limit), not 32; `grant`, `revoke`, `remove`, a help topic, a name of `data-names.txt`, a program of `shell.path` and a stray `/apps/NAME` are refused (ADR-0394 item 2).
- **10j, the hash covers the program then the description**, each behind an 8-byte length; the date is not in it.

## C. Hand checks (exact commands)

1. **10a, xterm comparison** (ADR-0332 item 10): in an 80x24 xterm run `TERM=xterm-256color cat crates/zoraya-term/recordings/vi-edit.rec`
   and compare with the same recording in a Zoraya terminal (screenshots: `docs/tasks/TASK-31-part-10a-shots/`).
2. **10a, `vttest`:** needs a pin in `ports/sources.lock` (not pinned, so not recorded).
   `python3 -c 'import sqlite3, ssl, asyncio'` and `ping google.com` in the guest.
4. **10f's row, grant line and kill offer:** the commands are in `docs/tasks/TASK-31-part-10f.md`, "Hand checks", for a first terminal, a room's own terminal and a shell that entered a room.
5. **Later parts:** 10g's `ssh -l NAME HOST`
   to a real host, 10h's `git clone` and `git push`: commands will be listed with the part.

- **10h, open decisions (ADR-0464):** the ancestor rule applies to every POSIX program (INV-0153-7 narrowed); hooks of git have no network (narrower than ADR-0339 item 5); a proposed text for ADR-0461 about the git test root.
- **10h (git), not merged:** hand-check commands are in `docs/tasks/TASK-31-part-10h.md` (a clone and a push against a real host). Open review findings there.

## D. Open questions


- **Volume programs:** ADR-0396, 0397 and 0399 are accepted and built by part 10j (ADR-0394). Open: the Programs page of `settings`, the hint line after a job given something while it ran, INV-0397-6's script; and: `tools/ci-tests.txt` has a new line for `tests/posix/zosh-install.script` (candidate run for you to tag; a head is in the part report).



## E. Decided 2026-11-19 (removed from the lists above)

- ADR-0465 accepted with its stated limits (the saved `+sock` word is a narrow exception to ADR-0397 item 1). Its review error is closed by a `BIND` right (part `work/bind-right`, queued before `work/rust-std`); the shared 256 nodes and 64 listeners become per-room bounds in F11.

## E. Decided 2026-11-18 (removed from the lists above)

- ADR-0461 accepted, Option A: `git-remote-http` links a libcurl built with mbedTLS 3.6 LTS (its GPL-2.0-or-later option), for git only; part 10h may wire git in.
- ADR-0462 accepted, Option A (a room's terminals get the random service and a network connection bounded by the room's words).
- ADR-0393 accepted, Option A (what 10g decided where ADR-0340 and ADR-0341 are silent).
- ADR-0388 accepted, Option A as amended; ADR-0389 accepted, Option A (the 10c broker notices and terminal calls).

## E. Decided 2026-11-16 (removed from the lists above)

- ADR-0462's rule for a room's network (recorded in the ADR): a room's `+net` words bound what a line may be given; a line gets a connection only when its own words or its description ask for it and the room allows it; a line with no network request gets none and no carved account; at the carved-account limit the line is refused with a hint. Every line getting the room's connection is refused.
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
System+K (10f); native `fetch` follows ADR-0328; ADR-0224 item 5's `+net` hint in curl's and wget's refusal (built in 10f for POSIX programs: connect and resolve); a refused
`tcsetpgrp` returns `EPERM` (10c); INV-0324-2, INV-0326-1 and ADR-0366's count reworded; the confined network steps run on
the runner itself in a job of their own; `vttest` pinned and built with 10d.

## Decided 2026-11-16 (TASK-34, plan parts F2 to F8; removed from the lists above)

- ADR-0431, 0432, 0434, 0436, 0437, 0439 and 0440 accepted as built. F8: `top`'s session row plus the keeper's ceiling refusal are the plan's `top --session`.
- ADR-0441 accepted with a change: each room has its own loopback (a connect to `127.0.0.1:PORT` reaches only listeners of the connecting program's
  room; reaching another room is a later, explicit grant). Built as **part F9**, with a test that a program in room B cannot reach a listener in room A
  with `+net 127.0.0.1:PORT`. A gate condition (ADR-0268).
  **F9 built** (branch `work/f9`, ADR-0442 proposed): the loopback is per room and the loopback condition of ADR-0268's gate is met, resting on: only the trusted console shell and init's control connection hold `ROOM`; the untrusted-room launcher (ADR-0269, not built) must get a connection with its room fixed by init and no `ROOM`; a room's own desktop shell builds its room from any typed name (`zosh.rs` enter_room), harmless today (no network slot) but it must never hold `ROOM`. INV-0442-4 is checked on the POSIX path only. **Decisions for the architect (ADR-0442 open questions):** (a) per-room bounds, or accept the one-bit channel of the machine-wide listener/account bounds; (b) a removed room that still runs shares its loopback and the keeper's job with a new room of the same name: `room new` refuses a name that still runs, or a generation number. Head: see `git log work/f9`.
- F6: the room charging of a program image is built now as **part F10** (`image_create` takes the job to charge; the shell passes the room's job; a test
  that a room at its memory limit cannot start a new program through the shell). A gate condition too (ADR-0262: what is made for a client is paid by the client).
  **F10 built** (branch `work/f10`, ADR-0443 proposed; head: see `git log work/f10`): `image_create` takes the job to charge (needs `CREATE_PROCESS` on it), the shell names the room's, and a room at its limit cannot start a program whose image does not fit (`room-image.script`). This closes this one condition of the gate only: others remain (ADR-0269's untrusted launcher is not built).
- Backlog (next stage): listeners reachable from the NIC (ADR-0440 Option B, a second smoltcp interface) and the rooms' `+listen` words and pills.
- The TCG kernel suites near 420 s: split into two jobs; the limit is not raised.
- The contiguous-create race with the reaper (ADR-0270; `t_bound`'s flake source): backlog, high priority.
- Decided 2026-11-17: ADR-0442 accepted with two changes (bounds per room, so one room cannot use up another's; a room's loopback keyed by a room id that is never reused, not by its name), built as **part F11**; ADR-0443 accepted (its remainders are known limits in the report); socket files in a program's own directory are allowed (ADR-0412; to be built, `docs/backlog.md`); unlinking a listening node follows the ordinary rules (write on its directory); the local `kernel_work_is_bounded` failures on the MacBook Air (the reaper race and load timing) do not block merges.

## For the TASK-34 dispatcher (MacBook Air)

The CI policy of ADR-0463 (the architect, 2026-11-16; it replaces the candidate rules of ADR-0348) applies to this dispatcher as
it does to the Mac mini's: a part merges after `make check`, `make test-changed` under HVF and its independent Opus review pass
locally, with no candidate run in CI, so F7b may merge on its local tests and review; a candidate run is needed only if F7b changes
`.github/workflows`, `tools/ci-plan.sh` (or what it reads) or `tools/test-runner/`, or for a failure that shows only under TCG. A push to
`work/*` runs `make check` only. `main`'s full set runs every 2 hours if `main` changed, and nightly. A red scheduled run
(`gh run list --workflow 'make test' --event schedule`) is fixed first, before any merge; if it is not fixed within an hour, revert the
merge that broke it and fix it on its branch (the red run of 2026-11-16, libc++ `mutex_try_to_lock`, is the first case). A stage tag
needs a green scheduled, nightly or `candidate-full/` run that includes the tagged commit (`tools/ci-last-full.rb --green-for SHA`).

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

**2026-11-16, for the TASK-34 dispatcher (and the architect):** main's red scheduled run 43778039826 (head `e7c34c69`, job
`make test-cxx-libcxx`: `forbidden output: posixrun: ended`) was one libc++ test aborting on its own timing assertion:
`thread.lock.shared.cons/mutex_try_to_lock.pass.cpp` (`Assertion failed: d < Tolerance`, 200 ms; five threads wait on a
mutex that main holds for 250 ms). No kernel panic, fault or exit record is in the log. The same job passed in 11 of 11 later
runs on commits that contain `e7c34c69` (park-ending, kwb-time, f5, f7, curses-flake candidates and scheduled run
43819664106), and in 12 local runs (6 HVF, 6 TCG, 1303 to 1357 steps each); the test alone 40 of 40 under TCG, with the
largest measured `d` 4 to 7 ms. Nothing between `9b78d380` and `e7c34c69` touches the paths the measured interval uses. So
no Zoraya change explains it; the likely cause is one stall of the QEMU process on the runner of more than about 195 ms,
but that is not proven (the CI log has no per-test times). F4 (spawn) and F5 (4096 mappings) are not implicated. The
scheduled run 43819664106 (head `4e86f859`) is green: F5, F8, F6 and F7a (and `stage-f3`) can be tagged on it
(`tools/ci-last-full.rb --green-for SHA`). Open for the architect: exclude this test and its `unique_lock` twin, and other
wall-clock tests, from the TCG libc++ run (`tests/cxx/libcxx-tcg-excluded.txt`, HVF only, as for LWG3343), or keep them and
re-run a failed job once; the new `RUNNER-STEP` lines in the serial runner will show per-step times in the next full run.


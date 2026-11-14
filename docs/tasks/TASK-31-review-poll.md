# TASK-31 review: part work/poll (`poll` and `ppoll` in libposix)

Independent review by the `opus-review` subagent of `git diff origin/main...work/poll` (2026-11-14), read against
`docs/tasks/TASK-31-part-poll.md`, ADR-0398 (proposed, on the branch), ADR-0315, ADR-0342 item 5 and driver 2,
ADR-0414 item 1 and INV-0414-1, ADR-0331 item 5 and row 10c of `docs/research/stage-10/plan.md`. The review began at
d471550. While it ran, the branch moved to 2fc1523 (a merge of `origin/main` and the fix of finding 2). The poll code,
the tests and ADR-0398 are the same in both (`git diff d471550 2fc1523 -- crates tests/posix-core docs/adr/0398*` is
empty), so the line numbers below are those of 2fc1523.

What the reviewer ran:

- `make check` on the branch: it fails at `check-host-paths` (finding 1). With `KEEP_GOING=1`, 22 of the 23 checks pass.
- In a scratch worktree of 2fc1523: `cargo test -p zoraya-posix --test poll --test net` on the unchanged code (18 and 16
  tests pass), then ten hand breaks of `src/poll.rs`, each run against the same two suites.
- One break of the C boundary, run under `make test-libposix` with `build/ports` cloned from the implementer's worktree.
- Two probe tests (below).

Severities: **must-fix** (wrong before merge), **should-fix** (fix in this part or record it), **note**.

## Findings

1. **must-fix** `docs/tasks/TASK-31-part-poll.md:26`: the part report contains an absolute host path (the implementer's
   worktree, under the home directory). Because of it, `make check` fails at `check-host-paths` on d471550 and on 2fc1523:
   `host paths or email addresses in tracked files: docs/tasks/TASK-31-part-poll.md:26`. The report's table says
   "`make check` 23 of 23 passed". That run came before d471550 added the report, so it does not show the head passing.
   On a push to `main` the check job fails. *Fix:* name the worktree in words ("the part's worktree") or drop the
   path, then run `make check` again on the head.

2. **must-fix, fixed at 2fc1523** `.github/workflows/test.yml:252` at d471550: the `hashFiles(...)` list of the
   ports-build cache key had no comma between `'docs/adr/0366-*.md'` and `'docs/adr/0398-*.md'`. GitHub checks the syntax
   of expressions when it loads a workflow, so the whole workflow failed to load. No job of any run started on that
   branch, and after a merge none would start on `main` either. Commit 2fc1523 adds the comma, and the line now matches
   line 230's list. What is left: nothing local checks that the workflow parses. `make check` passed with the broken
   line, and `actionlint` is not installed on the host. *Fix:* record the gap in `docs/backlog.md`, or add a parse check
   of `.github/workflows/*.yml` expressions to `check-ci-jobs`.

3. **should-fix** `crates/zoraya-posix/src/poll.rs:200` (the write probe's mask `WRITABLE | PEER_CLOSED`): no test catches
   the removal of `PEER_CLOSED` from this mask (break B4 below: every test passes). On the real kernel this bit is the
   only thing that ends such a wait. `zoraya-stream`'s `write_signals` (`crates/zoraya-stream/src/lib.rs:226-236`) clears
   `WRITABLE` when the reader is gone and sets only `PEER_CLOSED`. *Scenario:* a producer fills a pipe and calls
   `poll({wr, POLLOUT}, 1, -1)`, then the consumer exits (`yes | head -1` written with `poll`). If the bit is dropped by
   a later edit, for example when F1 merges this look with `select_once`, the producer waits for ever and no test goes
   red. The existing `a_closed_reader_is_an_error_on_the_write_end` closes the reader before the look, so it never
   waits. *Fix:* add the test the reviewer ran in the scratch copy. It passes on 2fc1523 and fails with B4 applied
   (`poll.rs:378` panics):
   ```rust
   let (rd, wr) = r.p.pipe(o::NONBLOCK).unwrap();
   assert_eq!(r.write(wr, &vec![1u8; STREAM_CAPACITY]), Ok(STREAM_CAPACITY));
   let end = read_end(&r, rd);
   r.p.sys.meanwhile = Some(Box::new(move |net: &mut Net| net.close(end)));
   let mut f = [pf(wr, POLLOUT)];
   assert_eq!(poll(&mut r, &mut f, None), Ok(1));
   assert_eq!(f[0].revents, POLLOUT | POLLERR);
   ```

4. **should-fix** `crates/zoraya-posix/src/poll.rs:146` (`sources.extend(probes.iter()...)`): no test has a descriptor with
   two probes that waits and is then woken by its second probe. Replacing the line with `probes.iter().take(1)` (break
   B6) leaves every test green. *Scenario:* a connected socket with a full send stream and nothing to read, polled for
   `POLLIN | POLLOUT` (curl's and GLib's usual request while a request body is being sent). The send stream drains,
   nothing arrives, and with only the first probe in the wait the call never returns. *Fix:* a `net.rs` test. A
   connected socket's outbound stream is filled until `EAGAIN`. Then `poll(POLLIN | POLLOUT, None)` runs with a
   `meanwhile` that drains the stream, and the test asserts `POLLOUT`.

5. **should-fix** `crates/zoraya-posix/src/poll.rs:158` with `crates/zoraya-posix/src/jobs.rs:169-172`: when `poll` has no
   source and no timeout, it fails with `-1`/`EINVAL` instead of blocking. This covers `poll(NULL, 0, -1)`, an array of
   negative fds, and entries with `events` 0 on files, all with timeout `-1`, in a process with no handler installed and
   no child it watches. The reviewer's probe on the fake kernel printed `poll_once(&mut [], None) -> Err(22)` and the
   same for `[{fd: -1}]`. POSIX has the call block until a signal. *Scenario:* an event loop whose only descriptors are
   disabled for now (`fd = -1`, a common idiom) and whose timeout is infinite. Each turn returns `EINVAL` at once, so the
   loop spins on a whole core, or it treats the error as fatal and exits. ADR-0398 item 4 says only that
   "`poll(NULL, 0, ms)` is a sleep" and does not mention this. `pselect` behaves the same way, so the gap is old, but
   ADR-0398 is where it should be recorded. *Fix:* in `poll_once`, when `sources` is empty and `deadline` is `None`,
   wait with the deadline at the clock's end (`Some(u64::MAX)`). A signal that terminates still ends the process, and an
   installed handler is already a source. Add a host test. Or else record the behaviour in ADR-0398 item 4 and in
   `design.md`.

6. **should-fix** ADR-0398 "Context" ("It decides nothing the two accepted ADRs decided") and item 2: the terminal arm
   (`poll.rs:235`, "ready at once") goes against what the accepted ADRs decided:
   - ADR-0342 item 5 says that on a terminal the wait goes by the readiness request.
   - INV-0414-1 says `POLLIN` only when a read would not block.
   - ADR-0342 driver 2 says "a name means one thing, decided before the first port that leans on it, so that two parts
     do not give `poll` two behaviours".

   This part gives `poll` its first terminal behaviour, and 10c will change it. A proposed ADR is the right place to
   say so, but the Context should call it a temporary deviation from ADR-0342 item 5 that the architect accepts or
   refuses, not a choice the ADRs left open.

   On the danger the task asks about, the reviewer finds it acceptable as a limitation but not harmless:
   - **CPU.** A program that polls a terminal set to `O_NONBLOCK` (Python's asyncio on stdin, curses with `nodelay`)
     spins on its own core. No CPU quota exists (ADR-0263 counts memory, processes and threads). The spin gives no new
     authority, because a program can already write `for (;;);`, and `pselect` spins the same way today. It is not a
     denial of service beyond what any program can already do.
   - **Starvation.** This effect is not in the ADR. A program that polls a terminal and a socket together (an `nc`-like
     relay, OpenSSH's client in 10g) is told the terminal is readable. It then blocks in `read(0)`, and the socket is not
     served until a key is typed.

   *Fix:* reword the Context and item 2, add the starvation effect to Consequences and to the part's limits, and state
   in the 10c handover that `Desc::Stream { tty: true }` is a deviation to remove, not a decision.

7. **should-fix** ADR-0398 "Consequences", `docs/design.md`, `tools/test-map.txt:202`: exporting `poll` and `ppoll` changes
   what the ports' `configure` scripts find, and none of the affected steps was run:
   - `ports/Makefile` puts ADR-0398 in `ADDED_ADRS`, and both cache keys hash it, so every port is rebuilt against a
     `libc.a` that defines `poll`.
   - CPython 3.15's link test for `poll` now succeeds. `select.poll` appears, `selectors.DefaultSelector` becomes
     `PollSelector`, and `subprocess.communicate` and asyncio (`test_asyncio.test_tasks`, `test_futures` and
     `test_locks` in `tests/python/regr-modules.txt`) now go through this code instead of `select`.
   - Other ports (toybox, curl, git later) may take their `poll` paths too.

   The part report ran `test-libposix`, `test-posix-threads` and `test-posix` only. ADR-0398 does not mention this
   change of behaviour for existing ports.

   The map line for `docs/adr/0398-*` is `check test-libposix test-posix-threads`, but its block is an input of every
   port's `libc.a` (compare the lines for 0366 and 0343). *Fix:*
   - Say in ADR-0398 Consequences and in `design.md` that ports configured before this part now get `poll`, CPython's
     `PollSelector` among them.
   - Widen the map line to at least `test-posix test-apps test-python`.
   - Do not merge until a candidate run that includes `test-python` and `test-apps` is green. The map sends
     `crates/zoraya-posix` to every step, so a candidate run of this branch does include them.

8. **note** `poll.rs:173-184` with `socket.rs:575`, connected sockets: when the peer's end closes (a FIN), the call
   reports `POLLIN | POLLHUP`. Linux reports `POLLIN | POLLRDHUP` and gives `POLLHUP` only once both directions are shut.
   Python's `PollSelector` turns `POLLHUP` into both read and write readiness, and GLib turns it into `G_IO_HUP`. A
   program that half-closes and goes on writing is unlikely among today's ports. ADR-0398's Option C discusses only
   pipes. *Fix:* one sentence in item 1 or in the revisit triggers.

9. **note** ADR-0398 item 1 against the code: three sentences do not match exactly.
   - "with `POLLERR` for a channel that closes" (connecting socket): a request for `POLLIN` alone gets `POLLHUP` from the
     read probe (`closed: POLLHUP`).
   - "`POLLHUP` and `POLLERR` are reported whether or not asked for": that holds only when a probe exists for the end
     that closed. A pipe's write end polled for `POLLIN` alone, after its reader has gone, gets `POLLIN` (no end, ready
     at once) and no `POLLERR`. A read end polled for `POLLOUT` alone never reports `POLLHUP`.
   - A refused connect (`Failed` on the control channel) reports `POLLOUT` without `POLLERR`. Linux gives
     `POLLOUT | POLLERR | POLLHUP`. Programs read `SO_ERROR` next, so this is harmless.

   *Fix:* say it as built.

10. **note** `ffi.rs` `wait_unlocked` and `port_wait_any`, the deferred path: suppose a sibling thread closes a polled
    descriptor between the look (taken under the lock) and `port_subscribe`. The subscribe fails and `poll` returns `-1`
    with `from_status`'s errno. It does not report `POLLNVAL` on the next look, and if the handle number is reused in
    between, the wait is on the wrong object. `pselect` and a blocking `read` behave the same way, and Linux leaves this
    case unspecified. *Fix (later):* in `poll_once`, map a failed wait on one of its own sources to "look again".

11. **note** INV-0342-1, second half: no `posix-surface-check imports` line checks that `t_poll` imports the two names.
    ADR-0343 has such a line, INV-0343-1. The export check also admits names from a proposed ADR, printing a note
    (`tools/posix-surface-check/src/main.rs:35-52`, the precedent of ADR-0343). Until the architect accepts ADR-0398,
    `poll` and `ppoll` are in `libc.a` on a proposed basis only. Both points are for the architect to know, not defects.

12. **note** 10c and ADR-0414:
    - **10c.** ADR-0331 item 5's readiness request is a message (`PollReadable`), not a handle signal. 10c's terminal
      arm of `probes` will therefore have to send a request. The ADR-0315 boundary runs `poll_once` again from its start
      after every deferred wait (`state()`: "it must have changed nothing it cannot repeat before a wait"), so that
      request must be idempotent or sent once per call, outside the look. Nothing in this part conflicts with that, but
      ADR-0398's "10c replaces that branch" understates the change.
    - **ADR-0414.** Item 1 is followed. Item 2 (`eventfd`, the port recorded before the lock is released) will change
      `wait_unlocked`, not `poll_once`, so the split chosen here leaves room for it.

## Hand breaks

The ten breaks of `crates/zoraya-posix/src/poll.rs` were made in a scratch worktree of 2fc1523. Each was run with
`cargo test -p zoraya-posix --test poll --test net`.

| Break | Result |
|---|---|
| B1 `rev \|= pr.closed` removed (no `POLLHUP`/`POLLERR`) | caught: two `net.rs` tests (`:655`, `:671`) |
| B2 `f.revents = 0` removed (stale `revents`) | caught: two `poll.rs` tests |
| B3 the negative-fd skip removed (`-1` becomes `POLLNVAL`) | caught: three `poll.rs` tests |
| B4 `PEER_CLOSED` dropped from the write probe's mask | **survives** (finding 3; the test given there catches it) |
| B5 `PEER_CLOSED` dropped from the pipe read probe's mask | caught only by the literal mask `5` in the deferred test; on the real kernel `read_signals` sets `READABLE` with `PEER_CLOSED`, so this break is equivalent there |
| B6 only the first probe of a descriptor joins the wait | **survives** (finding 4) |
| B7 the terminal waits like a pipe (`if !*tty` removed) | caught: `a_terminal_is_ready_at_once_...` |
| B8 `nfds > NFDS_MAX` becomes `>=` | caught: `the_arguments_of_a_hostile_caller_are_refused` |
| B9 milliseconds scaled by 1,000 instead of 1,000,000 | caught: the same test |
| B10 the deadline check before the wait removed | survives; equivalent (`wait_for` with a past deadline ends at once as `DeadlineExceeded`, so only a wait call is saved) |

One break of the C boundary was run under QEMU. In `poll_wait` (`ffi/procs.rs:945`), `restore(old)` was removed from the
`EINTR` path, which leaves `ppoll`'s mask in force after a handler runs. `make test-libposix` caught it:
`t_poll: line 391: sigismember(&cur, SIGUSR1) == 1 (errno 4) FAIL` in the `mask` case. The other ways to break the
boundary are covered by these C checks, read but not broken:
- `revents` not written back: every `rev()` call.
- `nfds` not checked: `hostile`.
- A lost wake-up: the 600 rounds of ping-pong in `threads`, both directions, `poll` and `ppoll`.

The deferred path (`state()`, `wait_unlocked`) has no host test of its own in this part. The C `threads` case is its
only check.

## Checked and found sound

- **`unsafe` in `ffi/procs.rs:882-985`:**
  - `array_len` caps `nfds` at 512 before any read or allocation, and a 64-bit count is not truncated. The largest
    span read is 4 KiB, so `fds.add(i)` for `i < n` cannot overflow.
  - Reads and writes are unaligned-safe.
  - Only `revents` is written, at `offset_of!`.
  - A refused call writes nothing (checked in C).
  - `ppoll`'s `timeout.read()` is an aligned read of a C `timespec`.
  - A non-NULL array that is not mapped faults the process. That is acceptable: the library runs in the caller's own
    address space, there is no kernel copy that could return `EFAULT`, no boundary of trust is crossed, and `pselect`
    and musl's own functions fault the same way.
  - The `// SAFETY:` comments are as thin as their neighbours' ("C's contract") but state the right invariant.
- **Results by kind, against POSIX and Linux:**
  - Pipe at end of file: `POLLIN | POLLHUP` (this matches INV-0414-1 and `select`, not Linux; ADR-0398 says so).
  - Write end whose reader closed: `POLLOUT | POLLERR`.
  - A full pipe is not `POLLOUT`; `WRITABLE` means `ATOMIC` bytes free.
  - `POLLNVAL` counts and does not wait.
  - A negative fd is skipped and its `revents` cleared.
  - The return value counts entries, so duplicates count separately, as on Linux.
  - `POLLPRI` is never reported.
  - Bits nobody defined are ignored.
  - `PollFd` has musl's layout and constants.
- **Timeouts:**
  - `poll`: a negative timeout is no timeout, 0 looks once, and `i32::MAX` milliseconds fit without saturating.
  - `ppoll`: the `timespec` is checked as `pselect` checks it, saturates at the clock's end, and is not written back.
  - The deadline is absolute, so another look after a spurious wake-up does not extend it.
- **Signals:**
  - `ppoll`'s mask is swapped in before the first `deliver()`, so a pending signal that it unblocks runs its handler
    under the temporary mask and gives `EINTR`. The caller's mask is restored on all three exits.
  - `SA_RESTART` does not restart `poll`, which is correct (Linux behaves the same).
  - The cancellation test comes before the array is read.
- **Threads (ADR-0315):**
  - The look happens under the library lock, and the wait subscribes level-triggered on a port of its own. A signal
    raised between the look and the wait is delivered as a packet at once, so no wake-up is lost.
  - The resume after a wait is used once, and a look that finds a descriptor ready drops it (`end_run`).
  - A wake-up for a reason that has gone (`(None, Some(0))`) is one more look, not a spin, and each turn makes a new
    blocking wait.
  - INV-0342-2 holds by construction: `probes` and `select_once` take the same end and mask for every kind, and the C
    `select` case compares them.
- **Names:**
  - musl's `src/select` is not among `RETAINED_DIRS` (`ports/musl/musl.mk:12-14`), so `poll` has no duplicate
    definition.
  - `export_name = "poll"` avoids the clash with the file's signal helper `poll()`.
  - The `posix-surface-added` block is read through `ADDED_ADRS`, and the `make test-libposix` run above passed the
    export check before reaching the C cases.

## Recommendation

**Do not merge yet.**
1. Fix finding 1, which is one line and makes `make check` fail.
2. Add the two tests of findings 3 and 4.
3. Fix or record finding 5.
4. Correct ADR-0398's Context and Consequences (findings 6 and 7), and widen the map line.
5. Merge after a candidate run of the head that includes `test-python` and `test-apps` is green.

The design, the `unsafe` code and the threaded wait are sound. Findings 8 to 12 can go in as notes.

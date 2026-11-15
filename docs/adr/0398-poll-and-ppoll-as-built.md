# ADR-0398: `poll` and `ppoll` as built: what each descriptor kind reports, the limits of a call, and the two names

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0106, ADR-0315, ADR-0342, ADR-0414
- **Evidence:** CP-08, CP-10, browser-engine/survey.md#the-posix-library, browser-engine/survey.md#ladybird

## Context

ADR-0342 item 5 admitted `poll`, `ppoll` and `select` as "the same wait as `pselect`", and ADR-0414 (accepted) decided `poll` and
`ppoll` over every descriptor kind, with `eventfd` as a separate part. The backlog's second outside request ("`poll` and `ppoll` in
`libposix` over the deferred-wait protocol (ADR-0315), with tests") builds the two functions only. This ADR records what the code
does where ADR-0342 and ADR-0414 leave a choice open, so that none is silent, and carries the `posix-surface-added` block for the two
names (ADR-0342 item 1: a part that builds a function before a port imports it names its committed test program; the first port's
block takes the name over). It decides nothing the two accepted ADRs decided, with one exception: item 2's terminal is an **interim deviation** from ADR-0342 item 5 (a terminal waits by the readiness request), its driver 2 and INV-0414-1, which the architect accepts or refuses; stage 10 part 10c removes it.

What this part does not build, and what plan part F1 and stage 10 part 10c still do, is listed under "Not in this part".

## Decision drivers

1. CP-08 (one wait model, the port under every readiness call) and ADR-0414 INV-0414-1: `POLLIN` exactly when a non-blocking read would not fail with `EAGAIN`; `POLLOUT` likewise.
2. ADR-0342 INV-0342-2: the same readiness as `pselect` for the same descriptor; `pselect` is the behaviour programs of stage 9
   already depend on.
3. ADR-0315: a method asks for a wait through `wait_for` and is run again, so the readiness look is a method of the library state
   (host-testable) and the C boundary owns the timeout, the signal mask and the copy of the caller's array.
4. CP-10: no signal interrupts a running thread; a wait ends with a handler run at the boundary, `EINTR`.

## Considered options

### Option A: One look in `Posix` (`poll_once`), the boundary of `pselect` copied around it
- **Description:** `crates/zoraya-posix/src/poll.rs` holds `Posix::poll_once(&mut [PollFd], deadline)`: it fills `revents`
  from each descriptor's handle signals, and if nothing is ready asks `wait_for` with every source (so `PENDING` in a process with
  threads, ADR-0315 item 1). `ffi/procs.rs` has `poll`, `ppoll` and the loop `pselect` has (mask in force for the wait, handler ran is
  `EINTR`, an interrupt with none is another look).
- **Evidence for:** Drivers 1 to 4; the readiness logic is tested on the host against the fake kernel (`tests/poll.rs`, `tests/net.rs`).
- **Evidence against:** `pselect`'s own look (`select_once`, in the boundary) stays a second implementation until part F1 makes
  `select` the same code.
- **Cost:** *(estimate)* 600 lines with tests.
- **Precedent:** `pselect` here; musl's `poll` is a system call, which this library has no kernel for; Ladybird's `ppoll` loop is what the engine will call (browser-engine/survey.md#ladybird, browser-engine/survey.md#the-posix-library: Zoraya has `pselect` only).

### Option B: `poll` as a wrapper that builds `fd_set`s and calls `pselect`
- **Description:** Translate to `pselect`'s sets and back.
- **Evidence for:** No second look.
- **Evidence against:** `pselect` cannot say `POLLHUP`, `POLLERR`, `POLLNVAL` or "bad fd without failing the call" (it returns `EBADF`),
  and is limited to 1024 descriptors by `FD_SETSIZE`; INV-0414-1 asks for the per-fd answer.
- **Cost:** *(estimate)* 150 lines; wrong semantics.
- **Precedent:** none that is correct.

### Option C: Linux's `poll` exactly (a hangup without `POLLIN`, `POLLERR` alone on a write end)
- **Description:** Report what Linux reports for a pipe whose writer closed (`POLLHUP`, no `POLLIN` once drained).
- **Evidence against:** INV-0414-1 and `select`: a read at end of file does not block, so `POLLIN` is reported with `POLLHUP`.
- **Cost:** *(estimate)* the same.
- **Precedent:** Linux; programs that test `POLLIN` first and then read 0 work either way.

## Recommendation

**Recommendation (not a decision): Option A.** What it does:

1. **Readiness by kind.** A pipe or FIFO end and a connected socket are read from the handle's signals (`READABLE`, `WRITABLE`,
   `PEER_CLOSED`). Read side: readable, or the peer closed, is the requested `POLLIN`/`POLLRDNORM`; a closed peer also adds `POLLHUP` (on a socket after a FIN too, where Linux gives `POLLIN | POLLRDHUP` and `POLLHUP` only when both directions are shut: a program that half-closes and goes on writing would see `POLLHUP` early)
   (end of file reads as 0, so it is "ready", INV-0414-1). Write side: writable is `POLLOUT`/`POLLWRNORM`; a closed reader is `POLLERR` and
   the requested write bits (a write fails with `EPIPE` at once). A connecting socket's control channel is the source for both
   directions, as `select` has it, with `POLLERR` for a channel that closes on the write request (a read-only request for a closing channel gets `POLLHUP` from its read probe); a refused connect reports `POLLOUT` without `POLLERR` (programs read `SO_ERROR` next). `POLLHUP` and `POLLERR` are reported whether or not asked
   for, but only from an end the call looks at (a write end polled for `POLLIN` alone, or a read end for `POLLOUT` alone, has no probe and is ready at once with that bit only); with `events` 0 the hang-up of the read end (else the write end) is watched. `POLLPRI` and the `*BAND` bits are never ready.
   Only the bits asked for are reported (`POLLRDNORM` is a request of its own).
2. **Ready at once.** Files, directories, `/dev/null`, a socket with no connection, a **terminal** (an interim deviation, see Context: until the readiness request of
   ADR-0331 item 5, stage 10 part 10c, exists this is `select`'s behaviour, so INV-0342-2 holds by sameness, not by truth, and
   INV-0414-1 does not hold for it) and a direction a descriptor has no end for (a read of a write end: `EBADF` at once, which does not block). A request for
   `POLLOUT` on a read end therefore reports `POLLOUT`: `select` does the same.
3. **`POLLNVAL`** for an open-less descriptor, which counts in the return value and is not an error of the call and not a wait; a
   negative `fd` is skipped with `revents` 0 (POSIX).
4. **Limits.** `nfds` above `OPEN_MAX` (512, the table's size) is `EINVAL` (Linux: above `RLIMIT_NOFILE`), checked before the array is
   read (no allocation for a hostile count; `(nfds_t)1 << 32` is not truncated); `nfds` above 0 with a NULL array is `EFAULT`; a
   non-NULL pointer that is not mapped faults the process, as `pselect`'s sets do. `poll(NULL, 0, ms)` is a sleep, and with no source at all and no timeout (`poll(NULL, 0, -1)`, only negative fds) the call blocks until a signal (it waits with the clock's end for a deadline) instead of failing as `pselect` does. A descriptor that a sibling thread closes between the look and the wait makes the wait fail with `EBADF`; the call looks again and reports `POLLNVAL` (not covered by a host test: the fake kernel cannot make that interleaving).
5. **Timeouts.** `poll`: milliseconds, negative for none, 0 looks once, saturating at the clock's end. `ppoll`: a `struct timespec`
   with the checks `pselect` has (`EINVAL` for a negative field or `tv_nsec` out of range), NULL for none; the timeout is **not written
   back** (as `pselect`). A wait that ends by its deadline returns 0 with every `revents` 0.
6. **Signals.** `ppoll`'s mask is `pselect`'s: swapped in for the wait with `SIG_SETMASK`, a deliverable signal runs its handler and the
   call fails with `EINTR`, and the caller's mask is restored on every exit. `poll` takes the caller's mask. Both are cancellation points,
   tested before the array is read (ADR-0315 item 5).
7. **Names.** The block below adds `poll` and `ppoll`; `t_poll` (`tests/posix-core/t_poll.c`) is the committed test program that
   imports them; the first port that calls them (CPython's `selectors`, GLib, git, OpenSSH, ADR-0342) takes the lines over.

```posix-surface-added
poll t_poll
ppoll t_poll
```

### Not in this part

`select` is not exported by `libc.a` (only `ports/apps/zshim.c` has one, over `pselect`): its retirement, the `select` name and a
host test that all three report the same readiness are plan part F1. `eventfd`, `INV-0414-2` and `INV-0414-3`'s deny-list additions are
F1. A terminal's true readiness is part 10c. Unix sockets and listeners (ADR-0412) and memory descriptors have no descriptor kind yet;
they join `Posix::probes` when their kind exists. The measurement of ADR-0414 item 4 (a `poll` over 64 descriptors in a threaded
process) is not made here.

## Consequences

- **Positive.** A program that calls `poll` or `ppoll` links and waits like `pselect`, in a process with threads too; the readiness logic
  is host-tested for pipes, files, `/dev/null` and sockets.
- **Negative.** A terminal reports ready at once: a program that polls a terminal set non-blocking (asyncio on stdin, curses `nodelay`) spins on its own core (no CPU quota exists; a program can spin anyway, and `pselect` does the same), and a program that polls a terminal and a socket together (an `nc`-like relay, OpenSSH's client in 10g) is told the terminal is readable, blocks in `read(0)`, and does not serve the socket until a key is typed. Linking `poll` changes what the ports' `configure` finds: ports built before this part now get it (CPython's `select.poll` and `PollSelector`, `subprocess` and asyncio over it; toybox, curl, git later), so the candidate run must include `test-python`, `test-python-regr` and `test-apps`. Two looks (this and `pselect`'s) until F1; a terminal reports ready at once, so a program that polls the terminal for
  input busy-loops until part 10c; `poll` is linear per call, as ADR-0414 says.
- **Follow-up.** F1 (`select`, `eventfd`, the deny list), part 10c (terminal), F2 (Unix sockets).

## Invariants

- **INV-0398-1.** `poll_once` reports, for a pipe end, a file, a directory, `/dev/null` and a socket (no connection, connecting,
  connected, at end of file), the `revents` of "Readiness by kind" above. *Check:* `cargo test -p zoraya-posix --test poll --test net`.
- **INV-0398-2.** A call's arguments are validated as above: `nfds` above `OPEN_MAX` is `EINVAL` before any read, a NULL array with
  entries is `EFAULT`, a bad `timespec` is `EINVAL`, and no timeout overflows into the past. *Check:* `cargo test -p zoraya-posix --test poll`
  (`the_arguments_of_a_hostile_caller_are_refused`) and the C case `hostile`.
- **INV-0398-3.** From C, `poll` and `ppoll` behave as above for pipes, files, `/dev/null`, an unconnected socket, `POLLNVAL`, timeouts,
  `ppoll`'s mask and hostile arguments; from a thread of a threaded process a sibling's write wakes the wait, a signal is `EINTR`, and 600
  ping-pong rounds lose no wake-up. *Check:* `make test-libposix` and `make test-posix-threads` (`tests/posix-core/t_poll.c`).

## Open questions

- Whether `POLLOUT` on a read end should stay `select`'s "ready at once" or become "never" (Linux) when `select` and `poll` are one
  implementation (part F1): programs rarely ask.
- Whether `poll`'s `PEER_CLOSED` on a connecting socket's control channel should be `POLLHUP` too; the fake `netd` sends `Failed`
  and closes the data streams, which already reports `POLLHUP` with the data side.

## Revisit triggers

- A port that depends on `POLLHUP` without `POLLIN` at end of file (none known).
- Part 10c: the terminal's readiness is a request (`PollReadable`), a message, not a handle signal; `poll_once` is run again from its start after every deferred wait, so that request must be idempotent or sent once per call outside the look. This is more than "replacing a branch".
- Part F1 unifies `select` and `poll`: this ADR's item 2 is then restated there.

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

Option A.

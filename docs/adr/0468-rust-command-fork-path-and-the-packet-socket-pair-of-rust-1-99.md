# ADR-0468: `std::process::Command`'s fork path needs a packet-socket pair across `fork`, which libposix refuses: spawn by `posix_spawn` only, for now

- **Status:** proposed
- **Date:** 2026-11-20
- **Depends on:** ADR-0102, ADR-0190, ADR-0412, ADR-0450, ADR-0451, ADR-0453
- **Evidence:** CP-27, rust-std/survey.md#the-probes, rust-std/survey.md#what-std-calls-through-syscall, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

ADR-0453 says `std::process::Command` spawns by `posix_spawnp`, "or by `fork` and `exec` in the cases above" (a `uid`, `gid`,
`groups`, `chroot` or `pre_exec` setting, or a `PATH` set for a bare program name), and that the broker covers the fork path by
giving the executed child its random connection (item 3, INV-0453-1: "one through the fork path (a bare name with `PATH` set)").
TASK-35's probes ran `Command` only by `posix_spawn` (rust-std/survey.md#the-probes); the fork path was read in the source
(rust-std/survey.md#what-std-calls-through-syscall) and never run.

**Measured by part `work/rust-std` (2026-11-20, HVF, `make test-rust-std`).** Rust 1.99.1's `std` (`library/std/src/sys/process/unix/unix.rs`,
`Command::spawn`) makes its exec-error channel, on Linux, with `socketpair(AF_UNIX, SOCK_SEQPACKET)` instead of a pipe, before it forks: the
child sends the errno of a failed `exec` on one end (`output`) and the parent reads the other (`input`); a read of 0 bytes means the `exec`
happened. libposix's `fork` refuses a process that holds such a socket: a packet socket is one kernel channel end, which cannot be duplicated
(ADR-0190, ADR-0412 item 6, `jobs.rs` `fork_moves_into`), so `fork` fails with `ENOTSUP` (95, "Not supported") and so does `Command::spawn`.
The program holds exactly its three standard descriptors before the call (printed with `fcntl`); the socket is std's. So every `Command` that
takes the fork path fails with `ErrorKind::Unsupported`: a bare program name with `PATH` set, `CommandExt::uid` and `gid` (their `setuid` and
`setgid` fail in the Rust-only archive in any case, ADR-0451 item 5), and any `pre_exec`. `posix_spawn`, which most `Command`s take, works
(measured: status, output, a piped stdin, `kill`, a missing program, `current_dir`, a child that hashes).

A decision is needed because INV-0453-1's second clause cannot be met as written, and because a port that sets `PATH` for its children
(`cargo`-like tools, build scripts) would take this path.

## Decision drivers

1. **No pretence** (CP-27): a call that cannot be answered fails by name, and the surface is a list.
2. **A channel end cannot be duplicated** (ADR-0190): `fork` cannot give both processes a packet socket, as POSIX asks.
3. **What `std` needs is small:** after the fork the parent uses one end and the child the other; the socket is never used by both.
4. **Least committing:** no change to the kernel's handle rules, to ADR-0412 or to `fork`'s semantics for C programs without a decision.

## Considered options

### Option A: `fork` keeps refusing a process that holds a packet socket (as built)

- **Description:** `Command`'s fork path fails with `Unsupported`; a Rust program starts children by `posix_spawn` (no `PATH` setting for a
  bare name, no `uid`, `gid`, `groups`, `chroot`, `pre_exec`); `std`'s other uses of packet sockets are unaffected. A program that sets one
  of those reads `Unsupported` and no child is made.
- **Evidence for:** driver 4; measured (`rstest process`); a `Command` by `posix_spawn` covers every probe check.
- **Evidence against:** INV-0453-1's fork-path clause is not met; a port that sets `PATH` or `pre_exec` must be changed (a path with a `/`
  takes `posix_spawn`).
- **Cost:** none; a test that records the limit.
- **Precedent:** Zoraya today (ADR-0412 item 6).

### Option B: `fork` gives the child one end of each in-process pair

- **Description:** a packet-socket pair made by `socketpair` and not yet distributed has both ends in the process; `fork` moves one end of
  each pair to the child (the second one made, so that `sv[0]` stays with the parent and `sv[1]` goes to the child, which is `std`'s and the
  usual convention) and leaves the other with the parent; the child's copy of the first end and the parent's of the second are closed
  descriptors.
- **Evidence for:** drivers 3 and 4's second half: it is what `std` does with the pair; C programs that follow the convention work too.
- **Evidence against:** a program that uses the other convention (the child uses `sv[0]`) gets a dead descriptor and fails at its first
  `send`; the rule is a convention, not POSIX's duplication.
- **Cost:** *(estimate)* 60 lines in `jobs.rs` with tests; an ADR to change ADR-0412 item 6.
- **Precedent:** none found.

### Option C: duplicable channel ends (revisit ADR-0190)

- **Description:** the kernel gives a channel end a duplicate that shares the queue, so `fork` can copy a packet socket as POSIX does.
- **Evidence for:** `fork` becomes faithful for packet sockets, pipes of other kinds and later services.
- **Evidence against:** a change of the kernel's handle model, which ADR-0190 decided against; two readers of one queue need a rule.
- **Cost:** *(estimate)* weeks, in the kernel and the channel semantics.
- **Precedent:** Fuchsia's channel handles are not duplicable either (a channel's end moves); its Starnix runner translates Linux's `fork` over
  its own processes (alternatives-postmortem.md#fuchsia--zircon-incl-starnix).

### Option D: the Rust-only archive answers `socketpair` for `std`

- **Description:** `libzoraya-rust.a` defines `socketpair` for `SOCK_SEQPACKET` as a pipe pair of byte streams (a pipe is duplicable by
  `fork`), so `std`'s error channel works on a stream; every other `socketpair` call goes to `libc.a`.
- **Evidence for:** driver 3; `std` reads 0 or 8 bytes, whole, and a pipe of 8 bytes is atomic.
- **Evidence against:** the archive may then define a name `libc.a` exports (ADR-0451 item 5 lists only names `libc.a` keeps out) and a Rust
  program's `UnixDatagram` or `SOCK_SEQPACKET` socket would be a pipe, not a socket; the link's symbol resolution makes the archive's
  `socketpair` lose to `libc.a`'s anyway (an archive member is only pulled for an undefined name).
- **Cost:** *(estimate)* does not work without a `--wrap` link flag, which is Option C of ADR-0451 and was rejected.
- **Precedent:** the probe's `--wrap` shim.

## Recommendation

**Recommendation (not a decision): Option A for now, Option C as the follow-up.** The fork path is the less common one, `posix_spawn` covers
what the probes and the test cover, and the refusal is by name (`Unsupported`), not a hang or a wrong answer. Option B is a convention that
breaks a program that uses the other one; Option D does not link.

1. `make test-rust-std` records the limit: `Command::new("name").env("PATH", ...)` fails with `ErrorKind::Unsupported`, no child runs, and
   a spawn by absolute path works.
2. INV-0453-1's fork-path clause is checked instead by a C program of `make test-libposix` (`t_syscall forkexec`): `fork`, then `execv` of a
   program that calls `getrandom`; the executed image has a random connection from the broker (ADR-0453 item 3).
3. INV-0453-2 stands: `uid` and `gid` fail and the child never runs (the error is `Unsupported` now, `ENOSYS` once the fork path works).

### Effect of the vision

For target 2, an agent's Rust program cannot start a child through a path that changes its identity or runs code between `fork` and `exec`
(`pre_exec`); that is the intended confinement, reached by a refusal rather than by a failing `setuid`. Target 3: ported Rust tools that start
helpers by `posix_spawn` work; those that set `PATH` need a change until Option C. Networking, threads: none.

### Examples

```
rstest: fork-path-unsupported ok Unsupported ("Not supported")
```

## Consequences

- **Positive.** The limit is stated, measured and tested; nothing changes for C programs; `Command` by `posix_spawn` is complete.
- **Negative.** INV-0453-1's fork-path clause is met by a C test, not by a Rust `Command`; a bare name with `PATH` set fails.
- **Follow-up.** Option C when channel ends become duplicable; `docs/design.md` names the limit.

## Invariants

- **INV-0468-1.** A `Command` that takes `std`'s fork path fails with `ErrorKind::Unsupported` and starts no child, until `fork` of a process that
  holds a packet-socket pair is defined. *Check:* `make test-rust-std` (`fork-path-unsupported`: a bare name with `PATH` set; `identity-uid` and
  `identity-gid`: the child never runs).

## Open questions

- Whether another `std` call makes a packet socket before a `fork` (Rust 1.99.1's `Command` is the only one read; `UnixDatagram` and
  `UnixStream::pair` are the program's own).
- Whether Option B's convention is worth taking for C programs (`socketpair` then `fork`) on its own: measured by the first port that needs it.

## Revisit triggers

- Channel ends become duplicable (ADR-0190 revisited).
- A port needs the fork path of `Command` and cannot be changed to `posix_spawn`.
- A Rust release changes `Command`'s error channel.

## Architect's decision

(Left empty for the human.)

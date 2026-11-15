# ADR-0453: `std::fs`, `std::net` and `std::process::Command` under Zoraya's grants: a Rust program is a described POSIX program, and the broker gives every POSIX child randomness

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0002, ADR-0013, ADR-0102, ADR-0103, ADR-0104, ADR-0107, ADR-0153, ADR-0227, ADR-0287, ADR-0288, ADR-0310, ADR-0311, ADR-0396, ADR-0397, ADR-0411, ADR-0450, ADR-0451
- **Evidence:** CP-05, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, rust-std/survey.md#the-probes, rust-std/survey.md#what-std-calls-through-syscall, rust-std/survey.md#what-std-links

## Context

A Rust `std` program of ADR-0450 is a POSIX program to the system: libposix's descriptor table, namespace table and
broker. The probes ran it under two existing descriptions (rust-std/survey.md#the-probes): `cxxtest`'s, which takes `+r`
and `+w` grants, and `zfetch`'s, which grants the connection to the URL on the line. Measured:

- **Files.** Every `std::fs` operation worked in the program's own `/tmp` and in a `+w` directory. A directory the line
  did not grant gave `ENOENT` for each operation and the shell's hint with the `+r`/`+w` it would take. `hard_link` is
  `EPERM` (ADR-0013); `metadata().created()` is unknown (no birth time); `canonicalize` inside a granted directory whose
  parent is not granted is `ENOENT` (it reads every ancestor, as `realpath` does); `current_dir` is `/home`,
  `current_exe` `/bin/cxxtest`.
- **Network.** `TcpStream` reached the URL's host and port, by address and by name; another port was `EACCES`; through
  `posixrun`, with no grant, `EACCES`.
- **Processes.** `Command` ran the program itself through `posix_spawn`: status, output with environment, a piped
  `stdin`, `kill` (signal 9), a missing program (`NotFound`) all worked once ADR-0451's `FIONBIO` was there;
  `current_dir` needs `posix_spawn_file_actions_addchdir` (ADR-0451 item 3). A child that made a `HashMap` panicked
  with "failed to generate random data ... NotFound" and exit status 101: a child gets no random connection
  (INV-0411-4: "no network, random or window connection"), `getrandom` is `ENOSYS`, and `std` falls back to
  `/dev/urandom`, which Zoraya does not have (rust-std/survey.md#what-std-calls-through-syscall).

So `HashMap`, which most Rust programs use, fails in every Rust program started by another program, unless that
parent uses ADR-0411's grant mask, which `std::process::Command` cannot set. C children are affected the same way when
they call `getrandom`.

The accepted ADRs disagree on this. ADR-0227 item 3 says randomness "designates nothing, so there is nothing to show",
gives one connection to every program "by every launcher (`zosh`, the panel, the broker for POSIX processes, `init`
for services)", and a `share`d one "for a `fork` child ... and for `spawn`". ADR-0310 item 3 and INV-0310-1 ("a `fork`
child has no connection") and INV-0411-4 ("no network, random or window connection") give a POSIX child none. And
ADR-0288 item 6 lets only `init`'s and the shell's connections `Share`, at most 64 live children each, so that a program
cannot use up the service's 256 connections (`crates/zoraya-rng/src/lib.rs:233-238`, `SHARE_DEPTH = 2`): a program's
connection cannot make one for its child, and the broker, which starts every POSIX child (ADR-0104), holds none today.

`std::process::Command` uses `posix_spawnp` unless `uid`, `gid`, `groups`, `chroot` or a `pre_exec` closure is set, or
the environment sets `PATH` for a bare program name; then it forks and executes (`process/unix/unix.rs:467-474`; on musl
`setsid` too, `unix.rs:757-762`). Both paths reach the broker: `posix_spawn` directly, `exec` by moving the pid
(ADR-0104). `CommandExt`'s `uid`, `gid`, `groups`, `chroot` and `setsid` reach names that fail (ADR-0451 item 5);
`process_group` works (`POSIX_SPAWN_SETPGROUP`) (rust-std/survey.md#what-std-links).

## Decision drivers

1. **A process gets only what it is given** (CP-05, INV-0002-5); no Rust-specific authority.
2. **One description model** for every POSIX program (ADR-0153, ADR-0311, ADR-0396, ADR-0397).
3. **Randomness designates nothing** (ADR-0227 item 3), and "stated, never silent" (ADR-0227 item 1): no weak
   fallback, and no grant word for it.
4. **Measured behaviour** (rust-std/survey.md#the-probes): the grants already hold for Rust; the gap is the child's
   randomness and `current_dir`.

## Considered options

### Option A: The Linux way: the program sees the person's files and network

- **Description:** A Rust program gets the whole namespace and network, as on Linux, because crates expect it.
- **Evidence for:** Fewest surprises for ported tools.
- **Evidence against:** Ambient authority (driver 1); different from every other POSIX program (driver 2).
- **Cost:** A second launcher path.
- **Precedent:** Linux; Starnix gives Linux programs a Linux filesystem view inside a container
  (alternatives-postmortem.md#fuchsia--zircon-incl-starnix).

### Option B: A described POSIX program; the broker gives every POSIX child a random connection

- **Description:** Items 1 to 6 below.
- **Evidence for:** Drivers 1 to 4; measured for files and network; a child's `HashMap` then works as its parent's; it
  is what ADR-0227 item 3 already says (the broker as a launcher).
- **Evidence against:** Changes, for randomness only, INV-0411-4, ADR-0310 item 3 and INV-0310-1's `fork` clause, and
  ADR-0288 item 6's rule of who may `Share`; a session's broker becomes a third connection that may make 64 others,
  so the service's 256 connections are shared among more holders.
- **Cost:** *(estimate)* A connection for the broker from the shell that starts it, a `Share` per spawn and per `exec`
  in the broker, a depth rule in `zoraya-rng` that names launcher connections; tests in `zoraya-rng`, the broker and
  `make test-rust-std`.
- **Precedent:** the shell, which does this for each program it starts (ADR-0288 item 7).

### Option C: A described POSIX program; children stay without randomness

- **Description:** As B without item 3; a parent that wants its children to have randomness uses ADR-0411's
  `POSIX_SPAWN_GRANT_RANDOM`.
- **Evidence for:** No change to an accepted ADR.
- **Evidence against:** `std::process::Command` cannot set that attribute, so every Rust child that hashes panics
  (measured); a Rust port would need a patched `std`; ADR-0227 item 3 stays unimplemented for POSIX children.
- **Cost:** None.
- **Precedent:** today.

### Option D: A capability API for Rust (in the style of Fuchsia's namespaces or WASI)

- **Description:** Rust programs open files only through handles passed in, with a crate like `cap-std`.
- **Evidence for:** Authority explicit in the program's types.
- **Evidence against:** Not `std`: ported crates call `std::fs`; the POSIX namespace table already gives the same
  confinement from outside.
- **Cost:** *(estimate)* A crate and a port of each program.
- **Precedent:** Fuchsia components' namespaces; WASI.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **A Rust program is a POSIX program.** It is installed with a description (`user/describe/posix/NAME.cmd`, or a
   description on the volume, ADR-0396) and run through the broker; what it reaches is its line's grants exactly as for
   C (ADR-0311). Nothing about it says "Rust" to the shell.
2. **`std::fs`** is libposix's namespace table: a path outside every entry is `ENOENT` and reported once; hard links
   `EPERM`; `created()` unknown; `canonicalize` needs every ancestor readable; permissions are advisory (ADR-0002).
3. **The broker is the launcher of ADR-0227 item 3 for POSIX children.** The shell gives each session's broker a
   random connection that may `Share` (a launcher connection, like its own; this supersedes in part ADR-0288 item 6,
   which then reads "`init`'s, the shell's and a broker's"; a program's connection still may not). The broker makes a fresh connection for every
   POSIX process it starts by `posix_spawn` and for every `exec` (so `Command`'s fork path is covered when the child
   executes), and passes it in the bootstrap record. A `fork` child before `exec` still has none (ADR-0102 item 3:
   a channel end is not duplicated). For randomness only, this supersedes in part INV-0411-4 ("no ... random ...
   connection"), ADR-0310 item 3 and INV-0310-1's clause "a `fork` child has no connection" (an executed child now has
   one); network and window stay ungranted unless ADR-0411's mask passes them. When the broker's 64 live connections are
   in use, or the service's 256, the child starts without one and `getrandom` is `ENOSYS`, stated in the child's report
   as today.
4. **`std::net`** has the stream client of ADR-0287 under the line's network grant; `TcpListener` and `UdpSocket` fail
   (`bind` is not built for `AF_INET`, ADR-0287 item 9); a child has no network unless passed (ADR-0411).
   `set_read_timeout` and `set_write_timeout` succeed and never fire (ADR-0287 item 4: `SO_RCVTIMEO` and `SO_SNDTIMEO`
   are accepted and not honoured), a known limit until that ADR's follow-up.
5. **`Command`** spawns through `posix_spawnp` (ADR-0411) with `posix_spawn_file_actions_addchdir` for `current_dir`
   (ADR-0451 item 3), or by `fork` and `exec` in the cases above (a `fork` in a `posix` job is `snapshot_process`,
   ADR-0102); `CommandExt::uid`, `gid`, `groups`, `chroot` and `setsid` fail with `ENOSYS` from the Rust archive
   (ADR-0451 item 5), so the child is never started: a Rust program never changes identity.
6. **`/proc/self/exe`** is the program's image as for every POSIX program; `current_exe` reads it.

### Effect of the vision

For target 2, an agent's Rust tool is exactly as confined as its C equivalent: its description and its line. Randomness
for children adds no authority (it designates nothing). Target 3: ported Rust tools that spawn helpers work.
Networking: unchanged; a child has none unless passed. Threads: none.

### Examples

```
$ +r /work/src rg main /work/src
$ rg main /work/secret
hint: rg tried to read /work/secret, which it was not given
  to allow it: +r /work/secret rg main /work/secret
```

## Consequences

- **Positive.** Rust programs need no new grant kind; `Command` children that hash work; C children get `getrandom` too.
- **Negative.** Every POSIX start costs one request to the random service; a session with more than 64 live POSIX
  processes leaves the later ones without randomness; the service's 256 connections are shared by more holders.
- **Follow-up.** The broker's connection and `Share`s, `zoraya-rng`'s rule for launcher connections; ADR-0411's,
  ADR-0310's and ADR-0288's texts gain a note from the part that builds it (ADR-0001 rule 6 note), after the
  architect's decision.

## Invariants

- **INV-0453-1.** Every POSIX process the broker starts or executes has a random connection of its own, while the
  broker has fewer than 64 live ones, and no network or window connection unless its parent passed it; a program's
  connection can never `Share`. *Check:* `make test-rust-std` runs a `Command` child through `posix_spawnp` and one
  through the fork path (a bare name with `PATH` set) that each make a `HashMap` (exit 0), and one that connects
  (`EACCES`); `zoraya-rng`'s host tests that a program's connection's `Share` is refused and a launcher's is not.
- **INV-0453-2.** A Rust program cannot change its identity: `CommandExt::uid`, `gid`, `groups` and `chroot` fail.
  *Check:* `make test-rust-std` asserts each returns an error and the child never runs.

## Open questions

- Whether 64 live connections per broker and 256 in the service are enough for a session that runs a parallel build
  (cargo-like tools, `make -j`); measured when such a tool is ported. If not, the service's table grows, which is
  ADR-0288's to change.

## Revisit triggers

- The random service gains per-process quotas that make a default connection costly.
- A Rust port needs `TcpListener` (ADR-0287's `bind`).

## Architect's decision

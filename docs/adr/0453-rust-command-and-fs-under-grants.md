# ADR-0453: `std::fs`, `std::net` and `std::process::Command` under Zoraya's grants: a Rust program is a described POSIX program, and every spawned POSIX child gets randomness

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0002, ADR-0013, ADR-0103, ADR-0107, ADR-0153, ADR-0227, ADR-0287, ADR-0310, ADR-0311, ADR-0396, ADR-0397, ADR-0411, ADR-0450, ADR-0451
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
parent uses ADR-0411's grant mask. C children are affected the same way when they call `getrandom`. ADR-0227 item 3
says randomness "designates nothing, so it needs no grant word"; ADR-0310 item 3 gives the shell's programs a random
connection and none to their children; the random service lets one connection make 64 others.

`CommandExt`'s `uid`, `gid`, `groups`, `chroot` and `process_group` with `setsid` reach names that fail
(ADR-0451 item 4) (rust-std/survey.md#what-std-links).

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

### Option B: A described POSIX program; every spawned POSIX child gets a random connection

- **Description:** Items 1 to 6 below.
- **Evidence for:** Drivers 1 to 4; measured for files and network; a child's `HashMap` then works as its parent's.
- **Evidence against:** Changes INV-0411-4 and ADR-0310 item 3 for randomness only; a parent that spawns many children
  meets the random service's 64 connections per connection.
- **Cost:** *(estimate)* A connection made in `posix_spawn` (libposix already asks the service for the shell's
  programs); one test.
- **Precedent:** ADR-0310's shell, which does this for each program it starts.

### Option C: A described POSIX program; children stay without randomness

- **Description:** As B without item 3; a parent that wants its children to have randomness uses ADR-0411's
  `POSIX_SPAWN_GRANT_RANDOM`.
- **Evidence for:** No change to an accepted ADR.
- **Evidence against:** `std::process::Command` cannot set that attribute, so every Rust child that hashes panics
  (measured); a Rust port would need a patched `std`.
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
3. **Every POSIX child started by `posix_spawn`** (and so by `std::process::Command`) gets a random connection made from
   its parent's, when the parent has one; a `fork` child still has none (ADR-0102). This changes INV-0411-4 and
   ADR-0310 item 3 for randomness only: network and window stay ungranted unless ADR-0411's mask passes them. When the
   service refuses a 65th connection, the child starts without one and `getrandom` is `ENOSYS`, as today.
4. **`std::net`** has the stream client of ADR-0287 under the line's network grant; `TcpListener` and `UdpSocket` fail
   (`bind` is not built for `AF_INET`, ADR-0287 item 9); a child has no network unless passed (ADR-0411).
5. **`Command`** spawns through `posix_spawnp` (ADR-0411) with `posix_spawn_file_actions_addchdir` for `current_dir`
   (ADR-0451 item 3); `CommandExt::uid`, `gid`, `groups`, `chroot` and `setsid` fail with `ENOSYS` from the Rust
   archive (ADR-0451 item 4): a Rust program never changes identity.
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
- **Negative.** Every `posix_spawn` costs one request to the random service; a program that spawns more than 64 live
  children leaves the later ones without randomness.
- **Follow-up.** The spawn change in `zoraya-posix`; ADR-0411's and ADR-0310's texts gain a note from the part that
  builds it (ADR-0001 rule 6 note), after the architect's decision.

## Invariants

- **INV-0453-1.** A child started by `posix_spawn` from a parent with a random connection has one of its own, and no
  network or window connection unless the parent passed it. *Check:* `make test-rust-std` runs a `Command` child that
  makes a `HashMap` (exit 0) and one that connects (`EACCES`); a `zoraya-posix` host test counts the child's bootstrap
  connections.
- **INV-0453-2.** A Rust program cannot change its identity: `CommandExt::uid`, `gid`, `groups` and `chroot` fail.
  *Check:* `make test-rust-std` asserts each returns an error and the child never runs.

## Open questions

- Whether the random service's 64 derived connections are enough for a build tool that spawns in parallel (cargo-like
  tools); measured when such a tool is ported.

## Revisit triggers

- The random service gains per-process quotas that make a default connection costly.
- A Rust port needs `TcpListener` (ADR-0287's `bind`).

## Architect's decision

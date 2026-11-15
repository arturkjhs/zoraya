# ADR-0450: Rust `std` for POSIX programs is the stable toolchain's prebuilt `aarch64-unknown-linux-musl` std over `libc.a`

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0002, ADR-0005, ADR-0100, ADR-0109, ADR-0141, ADR-0143, ADR-0410, ADR-0418
- **Evidence:** CP-13, CP-27, DM-057, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#redox, rust-std/survey.md#building-on-stable, rust-std/survey.md#crates-that-bypass-libc, rust-std/survey.md#how-other-systems-did-it, rust-std/survey.md#ladybirds-rust, rust-std/survey.md#route-a-and-inv-0109-2, rust-std/survey.md#route-a-the-prebuilt-musl-std, rust-std/survey.md#route-b-a-zoraya-target, rust-std/survey.md#route-c-no-std, rust-std/survey.md#the-probes, rust-std/survey.md#zoraya-today

## Context

Rust programs on Zoraya are native and `no_std`; ADR-0418 item 4 (accepted) says "No Rust `std` ... for POSIX programs
now; Rust `std` returns with Ladybird (ADR-0410's revisit) or a Rust POSIX tool worth porting, as its own ADR". This is
that ADR. ADR-0410's choice between WPE WebKit and Ladybird waits on it (the architect's note of 2026-11-14), and so
would any Rust command-line tool or window program built outside this repository. `docs/design.md` (Platform) allows
only the stable toolchain, 1.99.1, and no nightly features; INV-0109-2 forbids compiling a port with `__linux__`.

TASK-35 measured the route the architect named (rust-std/survey.md#the-probes): the `aarch64-unknown-linux-musl` target,
which `rustup` ships with a prebuilt `std` (tier 2 with host tools, musl 1.2.5, the same musl as `libc.a`), built with
stable flags only and linked by `zcc` against `libc.a` (rust-std/survey.md#building-on-stable). A probe program ran
inside Zoraya through `posixrun` and through the broker under existing descriptions: files, processes, TCP, panics with
unwinding and threads work; `HashMap`, every blocking wait, `fs::copy`, `Command::output` and `connect_timeout` fail
for two causes in libposix (`syscall()` returns `int`, and `ioctl(FIONBIO)` is unanswered), and with those two
emulated every check passes except `Command::current_dir`. It needs 22 libc names `libc.a` does not export and a rename
of six colliding `__rustc` symbols (rust-std/survey.md#route-a-the-prebuilt-musl-std). ADR-0451 to ADR-0454 decide those
details; this ADR decides the route.

Measured: the above, on HVF, one run each. Not measured: Route B (it needs nightly to build at all), TCG, a real
crate graph (only Ladybird's `Cargo.lock` was read, rust-std/survey.md#ladybirds-rust).

## Decision drivers

1. **Stable only** (`docs/design.md`, Platform). Cargo's build-std is nightly-only and "still in very early stages of
   development" (rust-std/survey.md#route-b-a-zoraya-target).
2. **No pretence of Linux for C** (INV-0109-2, ADR-0143), and its intent: no port takes Linux branches that Zoraya does
   not answer (rust-std/survey.md#route-a-and-inv-0109-2).
3. **A fixed, written surface** (CP-27): whatever Rust programs reach must be a list, changed only by decision.
4. **Where the ABI lives** (DM-057): Zoraya's contract is `libc.a`, not raw system-call numbers; a route must not make
   Linux's numbers a contract beyond a small, listed table.
5. **What it costs to keep** (CP-13): a target of Zoraya's own needs maintainers and a toolchain to carry; small systems
   without that stay tier 3 (rust-std/survey.md#how-other-systems-did-it).
6. **The programs that motivate it**: Ladybird's Rust (one million lines, `std` crates) and Rust tools built
   elsewhere (rust-std/survey.md#route-c-no-std).

## Considered options

### Option A: Run Linux binaries: answer Linux's system-call ABI (the Linux baseline)

- **Description:** Accept any static Linux AArch64 binary, built by any toolchain, by answering Linux's raw `svc`
  numbers, in the kernel or in a user-space translator.
- **Evidence for:** Fuchsia did this for Linux programs with Starnix, a user-space runner that translates Linux calls
  into its own protocols (alternatives-postmortem.md#fuchsia--zircon-incl-starnix); it would run Rust and everything
  else unmodified.
- **Evidence against:** The whole Linux ABI becomes the contract (drivers 3 and 4); Starnix still works on futex and
  signal semantics years in (same section); ADR-0002 keeps POSIX in a library, not the kernel.
- **Cost:** *(estimate)* A second personality the size of a kernel's system-call layer.
- **Precedent:** Fuchsia's Starnix, FreeBSD's Linuxulator, WSL 1 (rust-std/survey.md#how-other-systems-did-it).

### Option B: The prebuilt `aarch64-unknown-linux-musl` std, linked against `libc.a` (Route A)

- **Description:** Rust programs are built for `aarch64-unknown-linux-musl` with the pinned stable toolchain's
  prebuilt `std`, linked by `zcc` against `libc.a`; libposix answers what that `std` calls, by name and through a fixed
  `syscall()` table (ADR-0451); a build wrapper sets the flags (ADR-0452).
- **Evidence for:** Measured working with two libposix fixes (rust-std/survey.md#the-probes); stable flags only
  (rust-std/survey.md#building-on-stable); constants and layouts are musl's, which Zoraya's headers are; the surface
  stays gated by `libc.a`'s exports, so a crate that calls `epoll_create` fails at link
  (rust-std/survey.md#crates-that-bypass-libc); one prebuilt `std` per toolchain step, maintained by Rust's project.
- **Evidence against:** `cfg(target_os = "linux")` is true in `std` and every crate, so Linux branches are taken; a
  crate that issues Linux `svc` itself (`rustix`'s raw backend) bypasses the gate and needs a check
  (rust-std/survey.md#crates-that-bypass-libc); `std`'s Linux code changes between Rust releases, so each toolchain step
  must rerun the Rust test step; no other system was found that did this.
- **Cost:** *(estimate)* The libposix additions (ADR-0451, a few hundred lines with tests), a wrapper and a Rust-only
  archive (ADR-0452), a test step; a week of one part.
- **Precedent:** none found (rust-std/survey.md#how-other-systems-did-it); the nearest is Zoraya's own C++, built for
  the system's musl ABI by a wrapper (ADR-0360).

### Option C: A Zoraya target with build-std (Route B on nightly)

- **Description:** `aarch64-unknown-zoraya` with `std`'s `sys` for Zoraya, built by `-Z build-std`.
- **Evidence for:** No Linux branches; Zoraya's own name in every `cfg`.
- **Evidence against:** Needs a nightly cargo and rustc (driver 1); every crate with `target_os` lists needs a port
  (rust-std/survey.md#route-b-a-zoraya-target).
- **Cost:** *(estimate)* 2,000 to 4,000 lines in `std` and `libc`, two to four weeks, then a nightly in CI.
- **Precedent:** Hermit and Redox on AArch64 (tier 3) (rust-std/survey.md#how-other-systems-did-it).

### Option D: A Zoraya target upstreamed, or a toolchain built from source (Route B on stable)

- **Description:** As C, but the `std` comes from a Rust toolchain Zoraya builds from source with the target added,
  or from Rust's own CI once the target is tier 2.
- **Evidence for:** Redox and Fuchsia have tier 2 targets with `std` shipped by `rustup`
  (alternatives-postmortem.md#redox, rust-std/survey.md#how-other-systems-did-it).
- **Evidence against:** Tier 2 needs at least two maintainers and Rust's CI (driver 5); until then a compiler fork
  rebuilt at every step; tier 3 still needs build-std.
- **Cost:** *(estimate)* Option C's port plus an hour's toolchain build per step and a rebase, or months to tier 2.
- **Precedent:** Fuchsia (by its vendor), Redox, illumos.

### Option E: No `std` (Route C)

- **Description:** Rust stays `no_std` and native; no Rust POSIX programs.
- **Evidence for:** No change; native programs already work this way.
- **Evidence against:** Ladybird's Rust and ordinary Rust tools need `std` (rust-std/survey.md#route-c-no-std).
- **Cost:** None now; it closes ADR-0410's Option C.
- **Precedent:** Zoraya today (rust-std/survey.md#zoraya-today).

## Recommendation

**Recommendation (not a decision): Option B.** It is the only option that is measured working, stays on the stable
toolchain and keeps the surface a written list. Its Linux branches are the price; the list and the checks below make
them visible and bounded.

1. **Target.** A Rust POSIX program is built for `aarch64-unknown-linux-musl` with the prebuilt `std` of the toolchain
   pinned in `rust-toolchain.toml`, whose `targets` gains `aarch64-unknown-linux-musl`. It is a POSIX program in every
   other respect: a static `ET_EXEC` linked by `zcc` against `libc.a`, run through the broker under a description
   (ADR-0453).
2. **Stable only.** No `-Z` option, no `RUSTC_BOOTSTRAP`, no nightly toolchain builds a Rust POSIX program; the flags are
   ADR-0452's.
3. **INV-0109-2 is unchanged for C** (a Rust program's C parts go through `zcc`; `uname -s` prints `Zoraya`). **For
   Rust code** the rule's intent is carried by a mechanical one: a Rust program reaches only `libc.a`'s exports, the
   Rust-only archive of ADR-0451 and the `syscall()` table of ADR-0451, and contains no `svc` instruction outside
   `zoraya-rt` (INV-0450-2).
4. **ADR-0418 item 4 is superseded** by this ADR (its other items stand): Rust `std` exists for POSIX programs.
5. **Scope.** This is for ports and for programs built outside the repository. The repository's own programs stay
   native and `no_std`, and `docs/design.md`'s Platform rule for the workspace is unchanged.
6. **Each toolchain step** (a new `rust-toolchain.toml`) reruns the Rust test step of ADR-0452 and rereads `std`'s
   `syscall!` uses and `ioctl`s against ADR-0451's table before it merges.
7. **Revisit to Option D** when build-std is stable, or when Zoraya can name two maintainers for a target.

### Effect of the vision

For target 2 (untrusted code), an agent's Rust program is a POSIX program with exactly its description's grants; the
`svc` check means a program cannot step around `libc.a` into Linux numbers. Target 3 (a general desktop): Rust tools
and Ladybird's Rust become possible. Networking: `std::net`'s client, under the same grants as C (ADR-0453). Threads:
`std::thread` over libposix's pthreads and futex (ADR-0451).

### Examples

```
$ tools/zcargo build --release            # on the host: target/aarch64-unknown-linux-musl/release/rg
$ +r /work/src rg main /work/src          # in Zoraya, under rg's description
```

## Consequences

- **Positive.** Rust `std` on the stable toolchain without a compiler change; ADR-0410's Option C loses its policy
  blocker; libposix's `syscall()` defect is fixed for C programs too.
- **Negative.** Rust programs take Linux's branches; Zoraya answers a small part of Linux's system-call numbering; each
  toolchain step can change what `std` calls; 152 MB more toolchain in CI and on each machine.
- **Follow-up.** ADR-0451 to ADR-0454; a part that builds them with the Rust test step; `docs/design.md` gains a section
  when built.

## Invariants

- **INV-0450-1.** No build file of a Rust POSIX program uses a nightly feature: no `-Z` option, no `+nightly`, no
  `RUSTC_BOOTSTRAP`, and `rust-toolchain.toml`'s channel is a stable version. *Check:* `make check-rust-stable`, a script
  that greps `tools/zcargo`, `ports/` and the `Makefile` for `RUSTC_BOOTSTRAP`, `-Z` passed to cargo or rustc and
  `nightly`, and reads the channel; `tools/zcargo` itself refuses `-Z` arguments and a set `RUSTC_BOOTSTRAP`.
- **INV-0450-2.** A Rust POSIX program contains no `svc` instruction outside `zoraya-rt`'s system-call functions.
  *Check:* `zoraya-elf-check --no-foreign-svc`, run on every program `tools/zcargo` links, disassembles the executable
  sections and fails on an `svc` whose enclosing symbol is not one of `zoraya-rt`'s; a test program that issues a raw
  `svc` is refused.

## Open questions

- Whether `std`'s Linux code paths drift between toolchain steps faster than item 6 can follow; measured at the first
  step after this is built.
- Whether the `cc` crate builds C parts through `zcc` without adding a `--target` that `zcc` refuses (ADR-0452).

## Revisit triggers

- Cargo's build-std becomes stable: Option D without a fork.
- A Rust release changes `std` to issue `svc` itself on Linux, or to need a call ADR-0451 cannot answer.
- A program worth porting needs a Linux-only interface beyond the table.

## Architect's decision

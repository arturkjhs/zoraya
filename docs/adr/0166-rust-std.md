# ADR-0166: A Rust `std` for Zoraya

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0002, ADR-0003, ADR-0005, ADR-0030, ADR-0100, ADR-0107, ADR-0109
- **Evidence:** CP-13, CP-27, CP-28, DM-007, GR-005, X-1, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#redox, desktop/survey.md#what-a-target-needs, desktop/survey.md#what-redox-and-fuchsia-did, desktop/survey.md#hermit-and-xous-std-without-a-libc, desktop/survey.md#the-cost

## Context
Every Zoraya program is `no_std` over `zoraya-rt` and the protocol crates. A Rust `std` would let ordinary Rust programs and crates run on Zoraya (`std::fs`, `std::thread`, `std::net`, and every crate that assumes them), and it is what a `std` toolkit (ADR-0165 Option A) needs. The task asks whether to port it in stage 7, and if so whether over `libposix` (a Unix-like target, cheaper, the native world then depending on the POSIX personality) or natively over `zoraya-rt`.

What a port is (`desktop/survey.md#what-a-target-needs`): `std`'s platform code is split by concern (`fs`, `net`, `process`, `thread`, `sync`, `time`, `stdio`, `pipe`, `env`, `args`, `alloc`, ...), each selecting a platform with `cfg_select!` and falling back to `unsupported`; under them a `pal` with sixteen platforms. A new target is a specification in `rustc_target`, an entry in bootstrap, usually a patched `libc` crate, and a build of `rustc` and `std` with `./x`; the tier policy admits a tier 3 target with a maintainer and documentation and promises nothing about it. Redox and Fuchsia are the over-a-libc shape: their `std` reuses the `unix` pal over relibc or the SDK's sysroot (`desktop/survey.md#what-redox-and-fuchsia-did`). Hermit and Xous are the native shape: a pal that calls the kernel directly, with Hermit's own comment that every port starts as "stubs that immediately return errors" (`desktop/survey.md#hermit-and-xous-std-without-a-libc`). Both shapes need a modified `rust-lang/rust` tree: the pal lives inside `std`'s sources, so a port is either upstream (tier 3) or a fork of the compiler's library, built with `./x` (`desktop/survey.md#what-a-target-needs`) or with Cargo's `build-std`, which is unstable `[outside evidence: Cargo's `-Z build-std` is a nightly-only feature, not in the survey]`. Zoraya's toolchain is stable 1.99.1 with no nightly features (`docs/design.md`, CP-13's stable boundary); ADR-0109 defined the triple `aarch64-unknown-zoraya` for C and named "a Rust target ... with `std`" as deferred.

What Zoraya has for the concerns: files through a protocol, processes, time, allocation, stdio over channels and streams; what it lacks: threads (ADR-0030 one thread per process; ADR-0005 keeps `pthread` out of T1), synchronisation primitives that block, and networking (`desktop/survey.md#the-cost`). Measured: nothing. Assumed: the line counts of other pals are of the order of a few thousand lines each, from their directory listings, not read in full.

## Decision drivers
1. The stable toolchain and no nightly features (CP-13, `docs/design.md`): a port that needs a forked `rustc` or nightly `build-std` changes how the whole repository is built.
2. The native world stays free of POSIX (ADR-0002, X-1): a `std` over `libposix` makes every "native" Rust program a POSIX process with a namespace table, a broker and `environ`.
3. Scope held fixed (CP-27): stage 7 needs a desktop, and nothing in ADR-0160 to ADR-0169 needs `std`.
4. Threads do not exist, and `std` assumes them at start-up (`thread_local`, `std::sync`) (`desktop/survey.md#the-cost`).
5. Do not preclude it: everything written now should be the pal's foundation later.

## Considered options

### Option A: `std` over `libposix`, a Unix-like target (Redox and Fuchsia; the Linux baseline is `std` over glibc)
- **Description:** `aarch64-unknown-zoraya` with `target_family = "unix"`, `std`'s `unix` pal over ADR-0100's `libc.a`, built with a fork of `rust-lang/rust` carrying the target and a patched `libc` crate (`desktop/survey.md#what-redox-and-fuchsia-did`).
- **Evidence for:** The cheapest port: the `unix` pal exists and the libc is being built in stage 6; Redox reports "std is fully supported" this way and Fuchsia is tier 2 this way; every crate that works on Linux's `std` mostly works.
- **Evidence against:** Driver 2: a `std` program is then a POSIX process (pids, the broker, the namespace table, `environ`), which is the KeyKOS lesson of legacy guests getting none of the capability benefit (X-1); the libc has no threads in T1 (ADR-0005), so `std::thread` and the locks `std` needs at start-up are stubs or `pthread` must be added to the surface by an ADR; driver 1: a forked compiler tree and a bootstrap of `std` on every host, or a nightly; DM-007 returns if crates expect dynamic loading.
- **Cost:** *(estimate)* A target specification, a `libc` crate patch, the fork and its build (hours per host), `pthread` in `libposix`, and the maintenance of the fork across toolchain versions.
- **Precedent:** Redox (ten years, `alternatives-postmortem.md#redox`), Fuchsia (an SDK with its own clang and sysroot, `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option B: `std` natively over `zoraya-rt` (Hermit, Xous)
- **Description:** A `zoraya` pal inside `std`: `fs` over the file protocol with the launcher's directory handles as the root, `process` over `spawn`, `time` over the clock, `stdio` over the terminal connection, `thread` and `sync` once the kernel has threads, `net` on `unsupported`; built as Option A is, with the same fork (`desktop/survey.md#hermit-and-xous-std-without-a-libc`).
- **Evidence for:** Driver 2: a `std` program is a native program with handles; Hermit and Xous show the shape works with no libc; `std::fs::File::open("…")` needs a namespace, which ADR-0107's table gives natively without the broker.
- **Evidence against:** Drivers 1 and 4 as for A, plus the pal written from scratch against a `std` whose internal interfaces change between versions; paths in `std` are strings, so a namespace table is needed anyway, which is ADR-0002's deliberate X-1 trade-off applied to Rust programs.
- **Cost:** *(estimate)* The fork and its build as in A, plus 3,000 to 6,000 lines of pal `[assumption from directory listings]`, plus kernel threads first.
- **Precedent:** Hermit (tier 3, "experimental"), Xous (tier 3).

### Option C: No `std` in stage 7; everything native stays `no_std`, and the pal's foundations are built as ordinary crates
- **Description:** The desktop, its toolkit and its applications are `no_std` crates on `zoraya-rt`, `alloc` and the protocol crates, as every Zoraya program is. What a future pal would need is built as reusable crates where stage 7 needs it anyway: a file API over the protocol (`zoraya-fsclient`), a clock, stdio over a terminal connection, and ADR-0107's namespace resolver as a library. The decision between A and B is taken when a program needs `std`.
- **Evidence for:** Drivers 1 to 5. Nothing in ADR-0160 to ADR-0169 needs `std`; CP-27 holds scope; the Redox warning is that a libc-shaped userland took ten years and still is not self-hosting (`alternatives-postmortem.md#redox`); Fuchsia shows that even with its resources the libc route is an SDK with its own toolchain (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence against:** No third-party Rust crate that needs `std` runs on Zoraya; a port of an existing Rust application is not possible in stage 7; every `no_std` program re-implements the small conveniences `std` gives.
- **Cost:** *(estimate)* None beyond what the desktop builds anyway.
- **Precedent:** Zoraya stages 0 to 6; Hubris and Theseus never offer `std` to tasks `[outside evidence: their designs; alternatives-postmortem.md names them without this detail]`.

### Option D: A `zoraya-std` façade of Zoraya's own
- **Description:** A crate that offers `std`-like modules (`fs`, `io`, `process`, `time`) with Zoraya's shapes (handles, no paths from strings outside the table), for Zoraya's own programs, without being rustc's `std`.
- **Evidence for:** Conveniences without a compiler fork; the API can follow Zoraya's model (GR-005's explicit spawn, handles instead of paths).
- **Evidence against:** No third-party crate uses it, so it unlocks nothing that Option C's crates do not; a second `std`-like API is a design surface to keep stable (CP-13).
- **Cost:** *(estimate)* A thousand lines of API design and re-exports.
- **Precedent:** Fuchsia's `fdio` and `fuchsia-async` crates beside `std` (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, `[outside evidence]` for the crate names).

## Recommendation
Recommended: **Option C** now, with **Option B** as the path when a program needs `std`, and Option A reserved for running ported POSIX-shaped Rust software inside the personality if that is ever wanted. It is a recommendation, not a decision.

- **Stage 7** builds `zoraya-ui`, `zoraya-app`, `zoraya-text`, `zoraya-tokens` and the applications as `no_std` crates (ADR-0165). The file, directory and chooser clients the applications need are `zoraya-fsclient`, a `no_std` crate over `zoraya-proto-file`, written once and shared with `zosh` where TASK-18 allows.
- **Foundations kept pal-shaped.** `zoraya-rt` gains what every pal needs and the desktop needs anyway: a `Clock` (monotonic and, once a wall clock exists, real time), `Stdio` over a terminal connection, `Env` from the startup block, `Process` over `spawn` and exit records. ADR-0107's resolver is a `no_std` library usable by a pal.
- **The trigger** for B is concrete: a program the architect wants that needs `std` (a port of a Rust application, an agent runtime), or kernel threads (ADR-0030's revisit) appearing for another reason. B's open questions then are the fork's build on every host against driver 1, which the architect decides with the numbers of the first attempt.
- **What A would cost the vision** is recorded so that it is not chosen by default later: every Rust program would be a POSIX process (ADR-0002 rule 2), and the native desktop would depend on stage 6's toolchain and its `pthread`.

**Tensions with accepted ADRs.** None: ADR-0109 named the Rust target as deferred, and this ADR keeps it so.

### Effect of the vision
Target 2 (untrusted code): an agent runtime in Rust that needs `std` is the likeliest trigger for B, and B keeps such a runtime native: handles, no ambient namespace unless given a table. Target 3: ports of Rust desktop software favour A or B with a shaper and a toolkit (ADR-0165 Option A). Networking: `std::net` needs CP-28's socket object first. Threads: the first concern either shape needs; ADR-0030's revisit.

### Deferred to stage 6
Nothing here changes stage 6. `libposix` stays a C library with its Rust half; no Rust program links it in stage 7.

## Consequences
- **Positive.** No compiler fork, no nightly, no POSIX in the native desktop; the foundations of a pal exist as tested crates.
- **Negative.** No third-party `std` crate runs; every program is `no_std`.
- **Follow-up.** `zoraya-fsclient`; `zoraya-rt`'s `Clock`, `Stdio`, `Env`, `Process` modules.

## Invariants
- **INV-0166-1.** Every crate in the dependency graph of `user/` (the programs in the image) is `#![no_std]`, and the repository builds with the pinned stable toolchain and no `-Z` flag. *Check:* a host test that runs `cargo metadata` for `user/`, reads each dependency's `src/lib.rs` for the attribute (host-only crates such as `zoraya-fs-testkit` are not in that graph), and `rust-toolchain.toml` for a stable channel; `make test` itself.
- **INV-0166-2.** No native program depends on `libposix`, the broker or the namespace table: the desktop's programs hold only native handles. *Check:* a host test over `user/` and the desktop's `Cargo.toml` dependency graph asserting no dependency on `zoraya-posix`.

## Open questions
- The size of a `zoraya` pal: only known from directory listings; the first attempt measures it.
- Whether upstreaming a tier 3 target is worth its policy cost once B is built.

## Revisit triggers
- A program the architect wants that needs `std`.
- Kernel threads.
- A nightly or forked toolchain accepted for another reason.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.

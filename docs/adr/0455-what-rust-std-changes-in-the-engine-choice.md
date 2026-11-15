# ADR-0455: What a Rust `std` changes in ADR-0410: Ladybird becomes the recommendation once Rust `std` passes its test step and Ladybird ships its alpha

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0410, ADR-0411, ADR-0418, ADR-0421, ADR-0450, ADR-0451, ADR-0452, ADR-0453, ADR-0454
- **Evidence:** CP-13, CP-16, alternatives-postmortem.md#genode, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#ladybird, browser-engine/survey.md#servo, browser-engine/survey.md#the-engines-compared, rust-std/survey.md#ladybirds-rust, rust-std/survey.md#sources, rust-std/survey.md#the-probes

## Context

ADR-0410 (proposed, kept open by the architect on 2026-11-14) recommends WPE WebKit, with Ladybird "if the architect
accepts a Rust `std` target for POSIX programs". Its decision driver 2 is the Rust policy; its revisit trigger reads:
"The architect accepts a Rust `std` target for POSIX programs: Option C becomes the recommendation (it is smaller and
reuses curl and OpenSSL), if Ladybird has shipped its alpha by then." The architect asked this task to say what changes.

What TASK-35 found:

- **Driver 2's policy half is answered** if ADR-0450 is accepted: a Rust `std` on the stable toolchain, without a
  compiler change, measured working once two libposix defects are fixed (rust-std/survey.md#the-probes).
- **Ladybird's crates fit Route A**: of its 433 locked packages, the one that bypasses libc (`rustix` over
  `linux-raw-sys`) is reached only through `cbindgen`, a build-time tool; `tokio`, `mio`, `nix` and `libloading` only
  through its WebRTC crates; a crate `libwasm_jit` exists and must stay unbuilt (rust-std/survey.md#ladybirds-rust).
  Ladybird pins Rust 1.98.0; Zoraya's toolchain is 1.99.1.
- **Ladybird has not shipped its alpha**: its home page, fetched 2026-11-15, says "Our first Alpha for macOS and Linux
  is coming later this year" (rust-std/survey.md#sources).
- **Unchanged by this task**: Ladybird needs clang 19 or GCC 14, so CI's clang 18 moves first (ADR-0418 item 3's
  trigger); it is pre-alpha with 333 merged pull requests a month (ADR-0410 driver 3); its code is 0.83 million lines of
  C++ and 1.02 million of Rust against WPE's 4.39 million of C++ plus 2.69 million bundled
  (browser-engine/survey.md#the-engines-compared, browser-engine/survey.md#ladybird).

This ADR changes nothing in ADR-0410's text: its status and decision are the architect's. It recommends what the
architect's decision on ADR-0410 could say.

## Decision drivers

1. **ADR-0410's own trigger**: Rust `std` accepted, and Ladybird's alpha shipped.
2. **Measured, not assumed** (CP-13): Rust `std` counts when its test step passes in CI, not when an ADR is accepted.
3. **No code written at run time** (CP-16): both engines build without a JIT; Ladybird's `libwasm_jit` must stay out.
4. **A port that does not decay**: a pre-alpha tree is a continuous rebase (ADR-0410 driver 3); the ports that keep up
   are the ones that follow an upstream's releases (browser-engine/survey.md#how-other-systems-got-a-browser).
5. **The foundations come first either way** (ADR-0410's recommendation): F1 to F8 serve both engines.

## Considered options

### Option A: Keep WPE WebKit (ADR-0410 as recommended)

- **Description:** Rust `std` exists, but the engine stays WPE.
- **Evidence for:** A stable release line; no compiler step; WPE's dependencies are C and C++ only.
- **Evidence against:** About 3.8 times the code (7.08 million lines against 1.85 million; 5.3 times the C++); about 28 dependency ports, several LGPL (ADR-0421); GLib's `dlopen`-based
  pieces (browser-engine/survey.md#the-engines-compared).
- **Cost:** ADR-0410's estimate.
- **Precedent:** Haiku's WebKit, Sony's.

### Option B: Ladybird now

- **Description:** Move ADR-0410 to Option C as soon as ADR-0450 is accepted.
- **Evidence for:** Smaller; reuses curl and OpenSSL; Route A answers its Rust (rust-std/survey.md#ladybirds-rust).
- **Evidence against:** No alpha yet (driver 1); Rust `std` unbuilt (driver 2); clang 19 not in CI.
- **Cost:** ADR-0410's Option C estimate plus the Rust part.
- **Precedent:** Haiku's Ladybird port (basic pages in 2023).

### Option C: Ladybird when three gates are passed, decided after the foundations

- **Description:** Items 1 to 4 below.
- **Evidence for:** Drivers 1, 2 and 5; nothing is lost by waiting, because the foundations come first.
- **Evidence against:** A decision that waits on an outside project's release.
- **Cost:** None now.
- **Precedent:** Genode, a non-Unix framework, ports Chromium and Qt and moves with its own four releases a year
  (alternatives-postmortem.md#genode).

### Option D: Servo, now that Rust `std` is possible

- **Description:** Port the all-Rust engine.
- **Evidence for:** Rust throughout.
- **Evidence against:** Needs an OpenGL context, which Zoraya does not have, and SpiderMonkey with no documented JIT-off
  switch (browser-engine/survey.md#servo).
- **Cost:** *(estimate)* A software GL or renderer change, plus SpiderMonkey.
- **Precedent:** none found.

## Recommendation

**Recommendation (not a decision): Option C.** Rust `std` removes the policy reason for WPE; it does not remove the
other two (a compiler step and a pre-alpha engine), and ADR-0410's own trigger asks for the alpha.

0. **Measure now, before the gates.** A link-only build of Ladybird's Rust crate graph at the pinned commit with
   `tools/zcargo` (no run, no C++), in the foundation lane once ADR-0451's names exist: it tests the inferences of
   rust-std/survey.md#ladybirds-rust (`cbindgen` only on the host, WebRTC separable, `getrandom`'s newer versions over
   libc or the table) on 433 crates instead of one probe, and its undefined names are the list a Ladybird port would
   add.
1. **The gates.** ADR-0410's Option C becomes the recommendation when all three hold: (a) ADR-0450 to ADR-0454 are
   accepted and built (after ADR-0411's `posix_spawnp`, or with it, ADR-0451 item 4), with `make test-rust-std` green in
   CI and item 0's link build clean; (b) Ladybird has published an alpha release; (c) CI's
   compiler has moved to clang 19 or newer by ADR-0418 item 3's part. Until then WPE stays the default, as ADR-0410
   and the architect's note say.
2. **When they hold**, before the plan's E parts start: Ladybird is pinned at its alpha's tag, built with
   `ENABLE_JS_JIT` off and without `libwasm_jit` (CP-16), without WebRTC (its `tokio`, `mio`, `nix` and `libloading`),
   with its Rust through `tools/zcargo` (ADR-0452) via `ports/cmake/zoraya.cmake`; its `cbindgen` runs on the host.
3. **The Rust toolchain** is not pinned down to Ladybird's 1.98.0; Zoraya's newer pinned stable is used, and a
   build failure from the difference is that part's to fix.
4. **If the alpha does not come** by the end of the foundation parts F1 to F8, the architect decides between waiting
   and WPE; WPE needs nothing from this task.

### Effect of the vision

For target 2, the engine's content process holds nothing either way (ADR-0411). Target 3: the smaller engine, sooner,
if the gates pass. Networking: Ladybird's RequestServer over curl and OpenSSL, already ported. Threads: none new.

### Examples

```
gate (a)  make test-rust-std          green on main
gate (b)  ladybird.org                an alpha release published
gate (c)  .github/ci/Dockerfile       clang 19 or newer, every port's tests passing
```

## Consequences

- **Positive.** The engine choice rests on three facts anyone can check; no work is lost while they come.
- **Negative.** The choice stays open until an outside release; the plan's E parts cannot start before it.
- **Follow-up.** The architect's decision on ADR-0410; if Ladybird, a revised plan for the E parts.

## Invariants

- **INV-0455-1.** No engine build compiles a JIT or `libwasm_jit`. *Check:* the engine's configuration check of
  INV-0410-1, extended to fail when `libwasm_jit` appears in the engine's cargo build plan.

## Open questions

- Whether Ladybird builds without WebRTC by an option (its CMake was not read).
- Whether Ladybird's Rust builds with Rust 1.99.1 unchanged.

## Revisit triggers

- Ladybird publishes its alpha.
- WPE drops interpreter-only builds or requires a GPU (ADR-0410's trigger).

## Architect's decision

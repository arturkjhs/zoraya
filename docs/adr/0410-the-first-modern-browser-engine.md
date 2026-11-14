# ADR-0410: The first modern browser engine is WPE WebKit, built without a JIT

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0005, ADR-0051, ADR-0100, ADR-0108, ADR-0109, ADR-0317, ADR-0318, ADR-0324, ADR-0342, ADR-0360, ADR-0364, ADR-0367
- **Evidence:** CP-16, CP-27, DM-032, X-1, alternatives-postmortem.md#genode, alternatives-postmortem.md#haiku--beos, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#ladybird, browser-engine/survey.md#servo, browser-engine/survey.md#the-engines-compared, browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach, browser-engine/survey.md#wpe-webkit-in-detail, browser-engine/survey.md#zoraya-today

## Context

NetSurf (ADR-0318) reads simple sites but has no JavaScript and a 2000s layout engine; on real Wikipedia its layout is
poor (TASK-27 9f). A browser for today's web is one of four engines: Blink, Gecko, WebKit or Ladybird. C++ builds since
TASK-30 (ADR-0360 to ADR-0367). This ADR chooses which engine Zoraya ports first. ADR-0411 to ADR-0421 design what it
needs; [the plan](../research/browser-engine/plan.md) orders the work.

Measured (browser-engine/survey.md#the-engines-compared): WPE WebKit 2.54.2 is 4.39 million lines of first-party C++
plus 2.69 million bundled (ANGLE, Skia); Ladybird at `8ab2c0bb9c43` is 0.83 million lines of C++ and 1.02 million of
Rust. Read from their sources (browser-engine/survey.md#wpe-webkit-in-detail, browser-engine/survey.md#ladybird):
WPE needs C++23 and still supports clang 18; Ladybird needs clang 19 or GCC 14 and Rust 1.98 for every build. WPE has a
build-time switch to an interpreter-only JavaScriptCore; Ladybird added a JIT in October 2026 and no switch was found.
Zoraya never maps memory executable at run time (ADR-0051 INV-0051-2, ADR-0108 item 6; browser-engine/survey.md#zoraya-today).

Assumed, not measured: build times on a 16 GB M4, guest memory per tab, and that WPE runs with no GPU and no EGL through
its SHM path (not confirmed by a fetched page). These are the first engine part's measurements.

## Decision drivers

1. **No code written at run time.** W^X holds for every user mapping (CP-16, DM-032); a JIT needs a new right and a
   superseding ADR of ADR-0051. An engine with a supported interpreter-only mode keeps the rule.
2. **The toolchain Zoraya has.** CI's clang 18 and libc++ 20 (ADR-0360); no Rust `std` for POSIX programs
   (browser-engine/survey.md#zoraya-today). An engine that needs neither a newer clang nor Rust `std` starts sooner.
3. **A port that upstream carries.** Haiku's WebKit fell behind once upstream dropped it, and Genode's Chromium is
   "too old" each year it is not refreshed (alternatives-postmortem.md#haiku--beos,
   alternatives-postmortem.md#genode, browser-engine/survey.md#how-other-systems-got-a-browser). An engine with a
   documented interface for new platforms and stable releases limits what Zoraya carries.
4. **What the web needs.** JavaScript, modern CSS, TLS, web fonts; a shipping engine's compatibility.
5. **The POSIX surface stays small** (CP-27, X-1): the engine's demands on `libc.a` should be ones other ports need
   too (Unix sockets, shared memory, `poll`), not Linux-only interfaces.
6. **Fits on one 16 GB machine** beside another task (browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach).

## Considered options

### Option A: Chromium or Firefox, as Linux distributions ship (the Linux baseline)

- **Description:** Port Blink (Chromium's content layer, as Fuchsia's WebEngine and Genode's Qt WebEngine did) or
  Gecko.
- **Evidence for:** The most compatible engines; Chromium has Ozone for new window systems and Mojo's platform channel
  for new IPC primitives (browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach). Genode reached a working
  Falkon on Chromium in 2020 (alternatives-postmortem.md#genode).
- **Evidence against:** Chromium is built only with its own pinned clang and libc++ and recommends more than 16 GB of RAM
  and 100 GB of disk to build; Firefox needs Rust `std` for the target (driver 2, driver 6). Genode's port took a FreeBSD
  base, ran single-process without a sandbox at first, and keeps falling behind
  (browser-engine/survey.md#how-other-systems-got-a-browser). Fuchsia's port was done by the engine's vendor and ended
  as a smart-display service.
- **Cost:** *(estimate)* Tens of thousands of lines of platform code; a second toolchain; months of one developer before
  a page shows.
- **Precedent:** Genode (Chromium through Qt WebEngine), Fuchsia (Google's own port).

### Option B: WPE WebKit with its WPEPlatform API, JavaScriptCore without the JIT

- **Description:** Port WPE WebKit's stable line (2.54.x now). Write a WPEPlatform backend for Zoraya's window channel
  (ADR-0419), a process launcher over `posix_spawn` (ADR-0411), and build with `ENABLE_JIT=OFF` (the assembly LLInt on
  AArch64; the C loop as fallback), the GPU process, WebGL, media and the Bubblewrap sandbox off.
- **Evidence for:** WPEPlatform is the documented, stable interface for a new platform since 2.54, with mandatory
  `connect`, `create_view` and `render_buffer` and everything else optional (browser-engine/survey.md#wpe-webkit-in-detail);
  JavaScriptCore's interpreter-only build is a CMake option (driver 1); clang 18 is still supported and the standard is
  C++23 (driver 2); releases every six months from upstream (driver 3); WebKit is Safari's engine (driver 4). Its IPC is
  `socketpair`, `SCM_RIGHTS` and memfd, its loop GLib's `poll` (driver 5). Haiku and the PlayStation ported WebKit to
  systems that are not Linux (browser-engine/survey.md#how-other-systems-got-a-browser).
- **Evidence against:** The largest code base of the four that fit (4.39 million lines first party), so the slowest
  build and the most memory; it brings GLib, GIO and libsoup (LGPL, ADR-0421) and their POSIX use; the interpreter-only
  build's speed is unmeasured (one 2018 figure, unverified, puts the C loop at half the assembly LLInt); clang 18 support
  carries a FIXME to drop it (ADR-0418); a CPU-only end-to-end run on 2.54 is not confirmed by a source.
- **Cost:** *(estimate)* 3,000 to 6,000 lines of Zoraya code (platform backend, launcher, the browser's chrome),
  20 dependency ports, patches to the build; clean build 1 to 2 hours on the M4.
- **Precedent:** Haiku's HaikuWebKit and its WebKit2 work; Sony's PlayStation port; WPE on Android.

### Option C: Ladybird

- **Description:** Port Ladybird with a Zoraya front end in place of Qt (like its Android one) and LibCore's Unix
  paths.
- **Evidence for:** Half the size (0.83 million lines of C++, 1.02 million of Rust); written to be portable and once
  ran on SerenityOS; its processes already split network (RequestServer) from content (WebContent) and its IPC is a
  `SOCK_STREAM` socket pair with `SCM_RIGHTS` and memfd, its loop `poll` (browser-engine/survey.md#ladybird); BSD-2
  licence; no GLib; a front end implements three pure virtual functions plus hooks.
- **Evidence against:** Pre-alpha, alpha "later this year", 333 merged pull requests a month (an engine that moves under
  a port); Rust is mandatory and its JavaScript engine is now mostly Rust, so Zoraya needs Rust `std` for POSIX programs
  first (ADR-0418); clang 19 or newer, so CI's clang and libc++ change first; a JIT since October 2026 and no switch
  found to build without it (driver 1); 49 vcpkg dependencies including FFmpeg, Vulkan and ANGLE; no Linux-free front
  end on `master` to copy.
- **Cost:** *(estimate)* Rust `std` for Zoraya (weeks), clang 19 in CI, 2,000 to 5,000 lines of front end, 30
  dependency ports; then a continuous rebase.
- **Precedent:** SerenityOS (where it was written), Haiku (basic pages in 2023 after GCC 13).

### Option D: Servo

- **Description:** Port Servo 0.1 with an embedder of Zoraya's own.
- **Evidence for:** Rust, close to Zoraya's own language; an embedding API with an LTS line
  (browser-engine/survey.md#servo).
- **Evidence against:** Needs Rust `std`, SpiderMonkey (C++, with no documented JIT-off switch) and an OpenGL context:
  Zoraya has no GL; monthly releases break the API.
- **Cost:** *(estimate)* Rust `std`, a software GL or a renderer change, SpiderMonkey's build.
- **Precedent:** none on a small system that was found (a Redox recipe, unverified).

### Option E: No modern engine; NetSurf stays the browser

- **Description:** Keep ADR-0318's NetSurf and improve its layout.
- **Evidence for:** Built, small, tested.
- **Evidence against:** No JavaScript; the sites a person uses do not work.
- **Cost:** None now.
- **Precedent:** Redox's NetSurf (unverified).

## Recommendation

**Recommendation (not a decision): Option B, WPE WebKit.**

1. **The engine.** WPE WebKit's stable line, pinned by tarball and SHA-256 in `ports/sources.lock` as other ports are
   (ADR-0109); 2.54.x now; a move to the next stable line is a part of its own (ADR-0418 item 3).
2. **Build configuration.** `PORT=WPE`, `ENABLE_JIT=OFF` with the assembly LLInt (`ENABLE_C_LOOP=OFF`); if the assembly
   LLInt does not build or pass, `ENABLE_C_LOOP=ON`. Off: `ENABLE_WEBASSEMBLY` (it conflicts with the C loop and is not
   needed first), `ENABLE_GPU_PROCESS`, `ENABLE_WEBGL` (no ANGLE), `ENABLE_VIDEO` and `ENABLE_WEB_AUDIO` (no
   GStreamer), `ENABLE_BUBBLEWRAP_SANDBOX`, `ENABLE_WPE_PLATFORM_WAYLAND`, `ENABLE_WPE_PLATFORM_DRM`,
   `ENABLE_WPE_LEGACY_API`, `ENABLE_INTROSPECTION`, `ENABLE_DOCUMENTATION`, `ENABLE_SPELLCHECK`, `ENABLE_JOURNALD_LOG`,
   `USE_LIBBACKTRACE`, `USE_SYSPROF_CAPTURE`; `USE_SYSTEM_MALLOC=ON` (ADR-0415 item 4). On: `ENABLE_WPE_PLATFORM_HEADLESS`
   (the test platform), the Zoraya platform of ADR-0419, Skia on the CPU.
3. **No executable memory, ever, from this engine.** Nothing in the engine asks for `PROT_EXEC`; `libposix` keeps
   refusing it (ADR-0413 item 6). A JIT is a separate, later decision with its own ADR (revisit triggers).
4. **The program.** One described POSIX program, `browser` (UI process), and its two helpers (`WPEWebProcess`,
   `WPENetworkProcess`) in the apps volume beside NetSurf, which stays (it is small, and it is the fallback).
5. **Ladybird stays the alternative.** The plan's foundations (ADR-0412 to ADR-0415) serve Ladybird as well; if the
   revisit triggers fire, a superseding ADR can switch without redoing them.

### Effect of the vision

For target 2, an agent's browser is a WebKit whose content process holds nothing (ADR-0411): no file of the person's,
no network, no window, so a page that compromises it reaches only what the IPC lets it ask. Target 3: the sites a person
uses work, which NetSurf cannot give. Networking: through the network process alone, under the `web` grant
(ADR-0324). Threads: WebKit is heavily threaded; ADR-0315's waits and ADR-0300's 128 threads a process carry it.

### Examples

```
$ browser https://en.wikipedia.org/wiki/Kyiv
```

A window titled `Browser` opens in the room's frame, with the pills `web`, `/system/fonts` and `~browser` (its own
directory, ADR-0411); the article renders with its infobox, the search box takes typed text, and a link to another site
opens in the same window. `top home` shows `browser` with `WPEWebProcess` and `WPENetworkProcess` under it.

```
$ grants browser
net: web  reach public hosts, ports 80 and 443; not this machine or its network (description; network process only)
```

## Consequences

- **Positive.** JavaScript and current CSS without changing the W^X rule; a documented platform interface; an upstream
  that releases twice a year.
- **Negative.** The biggest build of the options that fit; about twenty dependency ports, three of them LGPL
  (ADR-0421); an interpreter-only JavaScript engine, slower than any desktop browser's; WebAssembly off at first.
- **Follow-up.** ADR-0411 to ADR-0421; the plan's parts; measurements of build time, binary size, memory per tab and
  page speed on the first engine part.

## Invariants

- **INV-0410-1.** The engine is built with `ENABLE_JIT=OFF`, and no program of the engine's build contains code that
  calls `mmap` or `mprotect` with `PROT_EXEC`. *Check:* a `make check-engine-config` script that reads the CMake cache of
  the engine's build and fails unless `ENABLE_JIT:BOOL=OFF`; INV-0413-5's test.
- **INV-0410-2.** The engine's version is the one pinned in `ports/sources.lock`, and its build options are those of a
  single file in `ports/` (not set elsewhere). *Check:* `make check-fetch-ports` (the lock) and the configuration script
  above comparing the cache with the file.

## Open questions

- Whether the assembly LLInt with the JIT off is tested upstream on AArch64; if not, whether the C loop is. Resolved by
  building both on the host first (Linux AArch64 in CI) before Zoraya.
- The speed of an interpreter-only JavaScriptCore on real pages under HVF; measured by the first engine part
  (Speedometer and a page-load list).
- Whether WPE 2.54 renders end to end with no EGL and no GPU through `WPEBufferSHM`: the headless backend suggests so;
  the first engine part confirms it on the host.

## Revisit triggers

- Ladybird ships its alpha, offers a build without its JIT, and Rust `std` for Zoraya exists (ADR-0418): reconsider
  Option C.
- WPE drops interpreter-only builds or requires a GPU.
- A JIT is wanted for speed: a new ADR on executable mappings (ADR-0051's follow-up), not a silent change.

## Architect's decision

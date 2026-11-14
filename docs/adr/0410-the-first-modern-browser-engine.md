# ADR-0410: The first modern browser engine is WPE WebKit, built without a JIT

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0005, ADR-0051, ADR-0100, ADR-0108, ADR-0109, ADR-0317, ADR-0318, ADR-0324, ADR-0342, ADR-0360, ADR-0364, ADR-0367
- **Evidence:** CP-16, CP-27, DM-032, X-1, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#ladybird, browser-engine/survey.md#servo, browser-engine/survey.md#the-engines-compared, browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach, browser-engine/survey.md#wpe-webkit-in-detail, browser-engine/survey.md#zoraya-today

## Context

NetSurf (ADR-0318) reads simple sites but has no JavaScript and a 2000s layout engine; on real Wikipedia its layout is
poor (TASK-27 9f). A browser for today's web is one of four engines: Blink, Gecko, WebKit or Ladybird. C++ builds since
TASK-30 (ADR-0360 to ADR-0367). This ADR chooses which engine Zoraya ports first. ADR-0411 to ADR-0421 design what it
needs; [the plan](../research/browser-engine/plan.md) orders the work. The foundations of the plan (Unix sockets,
shared memory, `poll`, `posix_spawn`, mappings) serve either of the two candidates.

Measured (browser-engine/survey.md#the-engines-compared): WPE WebKit 2.54.2 is 4.39 million lines of first-party C++
plus 2.69 million bundled (ANGLE, Skia); Ladybird at `8ab2c0bb9c43` is 0.83 million lines of C++ and 1.02 million of
Rust. Read from their sources (browser-engine/survey.md#wpe-webkit-in-detail, browser-engine/survey.md#ladybird):
WPE needs C++23 and still supports clang 18 (with libstdc++; with libc++ it is untested); Ladybird needs clang 19 or
GCC 14 and Rust 1.98 for every build. Both build without a JIT: WPE by `ENABLE_JIT=OFF`, Ladybird by `ENABLE_JS_JIT`,
which is off by default on any system but Linux, macOS and Windows. Zoraya never maps memory executable at run time
(ADR-0051 INV-0051-2, ADR-0108 decision driver 6; browser-engine/survey.md#zoraya-today), and its Rust toolchain uses
no nightly features (`docs/design.md`, Platform).

Neither engine knows Zoraya: both have Linux branches the Zoraya compiler will not take (`zcc` defines `__zoraya__` and
refuses `__linux__`, INV-0109-2), so each needs its operating-system layer ported (WebKit's WTF and `OS(LINUX)`
branches; Ladybird's LibCore, which its `Documentation/Porting.md` describes for new POSIX systems).

Assumed, not measured: build times on a 16 GB M4, guest memory per tab, and that WPE runs with no GPU and no EGL through
its SHM path (not confirmed by a fetched page). These are the first engine parts' measurements.

## Decision drivers

1. **No code written at run time** (CP-16, DM-032). Both candidates satisfy it; it rules out none of them.
2. **The toolchain Zoraya has.** CI's clang 18 and libc++ 20 (ADR-0360); Rust only `no_std` and only stable features,
   so no Rust `std` for POSIX programs. A Rust `std` for a target of Zoraya's own needs either Rust's unstable
   `build-std` or a patched compiler `[outside evidence: Rust's build-std is a nightly feature]`, or a pretence of
   being Linux that INV-0109-2 forbids for C. This is a policy decision, not only work.
3. **A stable engine to port.** A port carried downstream decays when the engine moves under it: Haiku's WebKit was
   removed upstream and its WebKit2 work bit-rotted between two GSoC projects; Genode's Chromium is "too old" each year
   it is not refreshed (browser-engine/survey.md#how-other-systems-got-a-browser). A release line that changes twice a
   year is easier to carry than a pre-alpha tree with 333 merged pull requests a month.
4. **Size and portability.** Less code to build and to port means a smaller machine and faster iterations; Ladybird
   was rendering basic pages on Haiku in September 2023 after GCC 13 made it build, while Haiku's WebKit2 took years.
5. **What the web needs.** JavaScript, modern CSS, TLS, web fonts; WebKit ships in Safari; Ladybird's WPT subtest count
   passed Safari Technology Preview's in October 2026.
6. **The POSIX surface and the licences** (CP-27, X-1): the engine's demands on `libc.a` should be ones other ports
   need too; WPE brings GLib, GIO and libsoup (LGPL, `dlopen`-based module loading), Ladybird mostly permissive code
   and curl and OpenSSL, which Zoraya already has.
7. **Fits on one 16 GB machine** (browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach).

## Considered options

### Option A: Chromium or Firefox, as Linux distributions ship (the Linux baseline)

- **Description:** Port Blink (Chromium's content layer, as Fuchsia's WebEngine and Genode's Qt WebEngine did) or
  Gecko.
- **Evidence for:** The most compatible engines; Chromium has Ozone for new window systems and Mojo's platform channel
  for new IPC primitives (browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach). Genode reached a working
  Falkon on Chromium in 2020 (browser-engine/survey.md#how-other-systems-got-a-browser).
- **Evidence against:** Chromium is built only with its own pinned clang and libc++ and recommends more than 16 GB of RAM
  and 100 GB of disk to build; Firefox needs Rust `std` for the target (drivers 2 and 7). Genode's port took a FreeBSD
  base, ran single-process without a sandbox at first, and keeps falling behind. Fuchsia's port was done by the engine's
  vendor and ended as a smart-display service.
- **Cost:** *(estimate)* Tens of thousands of lines of platform code; a second toolchain; months of one developer before
  a page shows.
- **Precedent:** Genode (Chromium through Qt WebEngine), Fuchsia (Google's own port).

### Option B: WPE WebKit with its WPEPlatform API, JavaScriptCore without the JIT

- **Description:** Port WPE WebKit's stable line (2.54.x now). Port WTF's and WebKit's operating-system layer to
  Zoraya (its generic Unix paths where they exist), write a WPEPlatform backend for Zoraya's window channel
  (ADR-0419) and a process launcher over `posix_spawn` (ADR-0411), and build with the JIT, the GPU process, WebGL, media
  and the Bubblewrap sandbox off.
- **Evidence for:** WPEPlatform is the documented, stable interface for a new platform since 2.54, with mandatory
  `connect`, `create_view` and `render_buffer` and everything else optional (browser-engine/survey.md#wpe-webkit-in-detail);
  JavaScriptCore's interpreter-only build is a CMake option; clang 18 is still supported (driver 2); stable releases
  every six months (driver 3); Safari's engine (driver 5). Its IPC is `socketpair`, `SCM_RIGHTS` and shared memory,
  its loop GLib's `poll`. The PlayStation ported WebKit to a system that is not Linux, without a JIT
  (browser-engine/survey.md#how-other-systems-got-a-browser).
- **Evidence against:** The largest code base of the options that fit (4.39 million lines first party plus Skia), so
  the slowest build and the most memory (drivers 4 and 7); an OS layer with no Zoraya port and many `OS(LINUX)`
  branches; GLib, GIO, libsoup, glib-networking, libgcrypt, libgpg-error and libtasn1 to port, LGPL (ADR-0421), with
  GModule and libepoxy built on `dlopen` (item 3 below); clang 18 is supported with libstdc++, and Zoraya's libc++ 20
  with clang 18 is untested; the interpreter's speed is unmeasured; a CPU-only end-to-end run on 2.54 is not confirmed
  by a source. Haiku's WebKit is the cautionary precedent of driver 3: removed upstream for lack of maintainers, its
  WebKit2 work needed 20 to 25 GB to build and still lacked keyboard input and copy and paste after two GSoC projects
  (browser-engine/survey.md#how-other-systems-got-a-browser).
- **Cost:** *(estimate)* 5,000 to 9,000 lines of Zoraya code and patches (OS layer, platform backend, launcher, the
  browser's chrome), about 28 dependency ports (the required set with GLib's and libsoup's own), a clean build of 1 to
  2 hours on the M4.
- **Precedent:** Haiku's HaikuWebKit and its WebKit2 work; Sony's PlayStation port; WPE on Android.

### Option C: Ladybird

- **Description:** Port Ladybird with a Zoraya front end in place of Qt (its Android one, 1,319 lines, and its
  `--headless` mode are the models), LibCore's operating-system layer per its porting document, its JavaScript JIT off
  (the default off Linux, macOS and Windows).
- **Evidence for:** Less than half the code (0.83 million lines of C++, 1.02 million of Rust; driver 4); written to be
  portable: its porting document names Alpine, FreeBSD, OpenBSD and Haiku as working POSIX systems, and the Haiku port
  rendered basic pages in 2023 (browser-engine/survey.md#ladybird, browser-engine/survey.md#how-other-systems-got-a-browser);
  its processes already split network (RequestServer) from content (WebContent); its IPC is a `SOCK_STREAM` socket pair
  with `SCM_RIGHTS` and anonymous shared memory, its loop `poll`; its HTTP is curl and OpenSSL, both ported here
  already (ADR-0310, ADR-0312); BSD-2 and mostly permissive dependencies (FFmpeg is LGPL, so ADR-0421 applies unless
  media is left out, which was not checked); a JavaScript engine in Rust `[assumption: Rust's safety holds for it]`;
  its WPT subtest count passed Safari Technology Preview's (driver 5).
- **Evidence against:** Rust is mandatory and LibJS and much of LibWeb are now Rust, so Zoraya needs Rust `std` for POSIX
  programs first, which needs a nightly feature or a patched compiler (driver 2); clang 19 or GCC 14, so CI's clang and
  libc++ change first; pre-alpha, alpha "later this year", 333 merged pull requests a month, so the port is a
  continuous rebase (driver 3); `vcpkg.json` has 49 entries (about 38 distinct names), among them FFmpeg, Vulkan and
  ANGLE, of which a CPU-only build without media needs fewer (not counted).
- **Cost:** *(estimate)* A Rust `std` target for Zoraya (weeks, after a policy decision), clang 19 in CI (days),
  2,000 to 5,000 lines of front end and OS layer, about 25 dependency ports; then a rebase at every upstream month.
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

**Recommendation (not a decision): Option B, WPE WebKit; Option C, Ladybird, if the architect accepts a Rust `std`
target for POSIX programs.** The choice between B and C is close, and it turns on driver 2 and driver 3, not on the
JIT, the process model or the IPC, where the two are alike. Ladybird is smaller, more portable and already uses the
network ports Zoraya has; WPE needs no change of the Rust policy and no compiler change before work starts, and it is a
stable release line rather than a pre-alpha tree. The foundations come first either way (the plan's lane K), so the
architect can decide C at any point before the plan's E parts without losing work.

1. **The engine.** WPE WebKit's stable line, pinned by tarball and SHA-256 in `ports/sources.lock` as other ports are
   (ADR-0109); 2.54.x now; a move to the next stable line is a part of its own (ADR-0418 item 3).
2. **Build configuration.** `PORT=WPE`, `ENABLE_JIT=OFF` with the assembly LLInt (`ENABLE_C_LOOP=OFF`); if the assembly
   LLInt does not build or pass, `ENABLE_C_LOOP=ON`. Off: `ENABLE_WEBASSEMBLY` (it conflicts with the C loop and is not
   needed first), `ENABLE_GPU_PROCESS`, `ENABLE_WEBGL` (no ANGLE), `ENABLE_VIDEO` and `ENABLE_WEB_AUDIO` (no
   GStreamer), `ENABLE_BUBBLEWRAP_SANDBOX`, `ENABLE_WPE_PLATFORM_WAYLAND`, `ENABLE_WPE_PLATFORM_DRM`,
   `ENABLE_WPE_LEGACY_API`, `ENABLE_INTROSPECTION`, `ENABLE_DOCUMENTATION`, `ENABLE_SPELLCHECK`, `ENABLE_JOURNALD_LOG`,
   `USE_LIBBACKTRACE`, `USE_SYSPROF_CAPTURE`; `USE_SYSTEM_MALLOC=ON` (ADR-0415 item 4). On: `ENABLE_WPE_PLATFORM_HEADLESS`
   (the test platform), the Zoraya platform of ADR-0419, Skia on the CPU.
3. **No loader.** GLib's GModule is built without a loader (`g_module_supported()` false); libepoxy, which WPE requires
   and which resolves GL by `dlopen`, is linked with a shim local to the engine's programs whose `dlopen` and `dlsym`
   always fail and whose `dlerror` gives a message (as NetSurf's `nsshim.c` supplies names `libc.a` lacks). libepoxy
   calls `abort()` when its `dlopen` or `dlsym` fails on the way to a GL or EGL function, so the shim is safe only if no
   epoxy entry point is ever called, which a CPU-only build should give but is unverified (WebCore may initialise EGL at
   start); the shim counts its calls and E2's test asserts zero. `libc.a` keeps no `dlopen` (ADR-0342 item 12
   unchanged); the shim is a stated stretch of that item's intent for one port, and if the architect refuses it or the
   count is not zero, WPE is patched to make epoxy optional when no GL is built.
4. **No executable memory, ever, from this engine.** Nothing in the engine asks for `PROT_EXEC`; `libposix` starts
   refusing it (today `mmap` ignores `prot` and returns a read-write mapping; ADR-0413 item 6). A JIT is a separate,
   later decision with its own ADR (revisit triggers).
5. **The program.** One described POSIX program, `browser`, built as one executable that is also the Web and network
   processes (ADR-0415 item 2), in the apps volume beside NetSurf, which stays (it is small, and it is the fallback).
6. **Ladybird stays the alternative.** The plan's foundations (ADR-0411 to ADR-0415) serve Ladybird as well; ADR-0416
   and ADR-0417 apply to it unchanged; ADR-0419's window and ADR-0420's HTTP stack would be redone for it.

### Effect of the vision

For target 2, an agent's browser is a WebKit whose content process holds nothing (ADR-0411): no file of the person's,
no network, no window, so a page that compromises it reaches only what the IPC lets it ask. Target 3: the sites a person
uses work, which NetSurf cannot give. Networking: through the network process alone, under the `web` grant
(ADR-0324). Threads: WebKit is heavily threaded; ADR-0315's waits and ADR-0300's 128 threads a process carry it.

### Examples

```
$ browser https://en.wikipedia.org/wiki/Kyiv
```

A window titled `Browser` opens in the room's frame, with the pills `web`, `/system/fonts`, `/system/tls` and
`~browser` (its own directory, ADR-0411); the article renders with its infobox, the search box takes typed text, and a
link to another site opens in the same window. `top home` shows `browser` with its Web and network processes under it.

## Consequences

- **Positive.** JavaScript and current CSS without changing the W^X rule or the Rust policy; a documented platform
  interface; an upstream that releases twice a year.
- **Negative.** The biggest build of the options that fit; about 28 dependency ports, several LGPL (ADR-0421); an OS
  layer to port and carry; a `dlopen` shim for one port; an interpreter-only JavaScript engine, slower than any desktop
  browser's; WebAssembly off at first.
- **Follow-up.** ADR-0411 to ADR-0421; the plan's parts; measurements of build time, binary size, memory per tab and
  page speed on the first engine parts.

## Invariants

- **INV-0410-1.** The engine is built with `ENABLE_JIT=OFF`, and every `PROT_EXEC` request any engine process makes is
  refused, with zero such requests recorded in the engine's test step. *Check:* a `make check-engine-config` script that
  reads the CMake cache of the engine's build and fails unless `ENABLE_JIT:BOOL=OFF`; INV-0413-5's test; the engine's
  test step counts `libposix`'s refused `PROT_EXEC` requests (a debug counter) and asserts zero.
- **INV-0410-2.** The engine's version is the one pinned in `ports/sources.lock`, and its build options are those of a
  single file in `ports/` (not set elsewhere). *Check:* `make check-fetch-ports` (the lock) and the configuration script
  above comparing the cache with the file.

## Open questions

- Whether the assembly LLInt with the JIT off works on AArch64 with Zoraya's toolchain; if not, whether the C loop
  does. Resolved by the plan's E1 (JavaScriptCore's `jsc` shell in the guest).
- Whether clang 18 with libc++ 20 builds WebKit (upstream tests clang 18 with libstdc++); E1 again.
- The speed of an interpreter-only JavaScriptCore on real pages under HVF; measured by E6.
- Whether WPE 2.54 renders end to end with no EGL and no GPU through `WPEBufferSHM`: the headless backend suggests so;
  E2 confirms it.
- Whether Ladybird's new WebAssembly engine has a JIT switch of its own (not read); matters only for Option C.

## Revisit triggers

- The architect accepts a Rust `std` target for POSIX programs: Option C becomes the recommendation (it is smaller and
  reuses curl and OpenSSL), if Ladybird has shipped its alpha by then.
- WPE drops interpreter-only builds or requires a GPU.
- A JIT is wanted for speed: a new ADR on executable mappings (ADR-0051's follow-up), not a silent change.

## Architect's decision

# Browser engine survey: a modern engine on Zoraya

The research behind proposed ADR-0410 to ADR-0420 (TASK-33): which of the four engines of today's web Zoraya can
port, what each needs from the system, and what other small or non-Unix systems had to build to get one. It makes no
decisions.

**How it was made.** One session on 2026-11-14. Five research subagents (Haiku) fetched and summarised primary sources
under the evidence rules of TASK-01: every URL fetched in the session and the claim checked against the fetched text,
no citation from memory, paraphrase with short quotations only. A measuring subagent downloaded the WPE WebKit 2.54.2
release tarball, a shallow clone of Ladybird at commit `8ab2c0bb9c43` and the release tarballs of ICU4C, FreeType,
HarfBuzz and fontconfig, counted their lines and grepped them; nothing downloaded was built or run, and the trees were
deleted afterwards. Three digest subagents read Zoraya's ADRs and code for [Zoraya today](#zoraya-today). The main
session (which wrote the ADRs) set the questions, read every note, checked the claims an ADR rests on against the
notes' quoted sources and wrote this file. A sentence marked `(inference)` is the researcher's, not the source's; one
marked `[unverified]` rests on no fetched page and no ADR relies on it alone; `(estimate)` marks a number nobody
measured. A count marked **measured** was produced by a command in this session (the method is in
[Measurements](#measurements)).

**What could not be read** is listed in [Pages that could not be fetched](#pages-that-could-not-be-fetched). The gaps
that matter: Open Hub refused every request, so there is no current line count of Chromium or Firefox; no Redox page
could be fetched; the Chromium and Firefox sandbox pages were 404; Ladybird's IPC generator, its `ShareableBitmap` code
and its JIT's build file were not read.

**What was not done.** Nothing was built. No clean build time, build disk or per-tab memory below is measured on a
16 GB M4: TASK-33 runs documents only. Every such number is an `(estimate)`, and [the plan](plan.md) makes measuring
them the "done when" of its first engine part.

## Zoraya today

Read from the repository at `0c3ada97` (2026-11-14) by three digest subagents; the facts an ADR relies on were checked
by the main session in the files named. Every ADR cited here is `accepted` unless marked.

### Processes, handles and channels

- A process gets its handles at `spawn` from its creator, at the slots the creator names (ADR-0003, INV-0003-6); 1024
  handles per process (`crates/zoraya-abi/src/lib.rs:89`). Rights only decrease on duplication (ADR-0040).
- A channel message holds at most 64 KiB and 64 handles; each direction queues at most 64 messages and 256 KiB
  (`crates/zoraya-channel/src/lib.rs:22-28`). The writer pays for queued messages (ADR-0261). An endpoint with handles
  queued in it cannot be moved (`BadState`, ADR-0050), so no cycle of endpoints can form.
- One wait primitive: the port, which waits on many objects with level or edge subscriptions (ADR-0006). A futex
  private to a process (ADR-0301): no `PTHREAD_PROCESS_SHARED`.
- Threads: 128 per process, 1000 in the system (ADR-0300, `kernel/src/timer.rs:18`); live processes at most 255 (the
  ASIDs), a session ceiling of 200 (ADR-0266).
- `spawn` takes at most 64 handles (`crates/zoraya-abi/src/spawn.rs:36`). The kernel copies an image's segments into
  fresh frames at every spawn (ADR-0108, ADR-0033): two processes of one static program share no frames.

### Memory

- A memory object is fixed in size and all its frames are committed when it is made (ADR-0051); two processes may map
  one object; a READ-only duplicate gives a read-only view. No right maps anything executable (INV-0051-2), no
  executable mapping comes from user space (ADR-0108's decision driver 6), and W^X holds for the kernel and for every loaded segment
  (ADR-0020, ADR-0033). ADR-0051 names executable mappings for language runtimes as "a later ADR".
- A memory object's frames are paid by its creator's job for its whole life, however many processes map it; mapping
  charges only page tables (ADR-0261 item 8).
- 256 mappings per process: the kernel's `MAX_MAPPINGS` (`kernel/src/memory/user.rs:33`) and libposix's
  `MAPPING_SLOTS` (`crates/zoraya-posix/src/ffi.rs:778`). Each private `mmap`, each thread stack and each step of the
  heap is one mapping, and each keeps its memory object's handle (`crates/zoraya-posix/src/ffi.rs:571-586`,
  `:829-831`) in a table of 1024 handles (`crates/zoraya-abi/src/lib.rs:89`). The heap grows by `GROW_SIZE` = 1 MiB
  (`crates/zoraya-heap/src/lib.rs:25`), one memory object and one mapping per step. ADR-0320 measured CPython's deep
  recursion ending with `MemoryError` at about 33,000 frames (each frame chunk and each `obmalloc` arena of 1 MiB,
  mapped by `mmap`, is one mapping), and derived from the count of slots, "not measured", that about 250 MiB of small
  objects ends the same way; it left the limit as it is because the kernel's number is the same.
- The default guest has 128 MiB and one CPU (`tools/run-qemu.sh:95-96`), leaving about 86 MiB of session memory
  (ADR-0266). The frame allocator was designed for 128 MiB to 2 GiB (ADR-0021).

### The POSIX library

- `mmap` is private and anonymous only, and ignores `prot` (every mapping is read-write); `MAP_SHARED` and file
  mappings return `ENODEV` (ADR-0103 item 12); `munmap` takes only a whole mapping; `mprotect` is `ENOSYS`; `madvise`
  and `shm_open` are absent; `memfd_create` returns an unmappable file of `tmpd` (`crates/zoraya-posix/src/ffi.rs`, the
  digest's section 5).
- Sockets are outgoing TCP over `netd` only (ADR-0287). ADR-0225 decided `socketpair(AF_UNIX, SOCK_STREAM)` as two
  byte streams crossed, `bind`/`listen`/`accept` over `netd`, and `poll`/`select` over one port wait, and refused
  `AF_UNIX` with a path (`EAFNOSUPPORT`). ADR-0342 item 11 admits `sendmsg`/`recvmsg` "without ancillary data". None of
  `socketpair`, `bind`, `listen`, `accept`, `sendmsg`, `recvmsg` or `SCM_RIGHTS` is built.
- Waiting: `pselect` is built (streams and sockets wait; files and terminals report ready at once). ADR-0342 item 5
  admits `poll`, `ppoll` and `select` as the same wait; not built. `epoll_*` is on the deny list; there is no
  `eventfd`, `timerfd` or `kqueue`.
- ADR-0315's deferred wait: once a process has a second thread, a long wait releases the library lock and waits on a
  port created for that wait, then re-runs the call (one `port_create` per long wait).
- Processes: `posix_spawn` with `adddup2` and `addclose` only, no PATH search, no `posix_spawnp`; `fork` copies every
  page (no copy-on-write; ADR-0102). A POSIX child that a POSIX program spawns gets no network connection, no random
  service and no window (ADR-0287 item 3, ADR-0310 item 3); a descriptor is passed only by inheritance at spawn, fork or
  exec.
- `dlopen` is denied (ADR-0100 item 8, ADR-0342 item 12). Faults end the process: no `SIGSEGV` handler, no
  `sigaltstack`. Signals run at wait points and library calls only (ADR-0106, CP-10).
- New names enter `libc.a` by a link probe of a port and a `posix-surface-added` block of an accepted ADR (ADR-0342
  items 1 and 10), read by `posix-surface-check` from the ADRs listed in `ports/Makefile`.

### The window, text and the network

- A POSIX program gets a window from a `window TITLE` line of its description, through the panel (ADR-0317): one
  window per process, a shared memory object of 32-bit pixels, damage and commit with one frame in flight, key, text,
  pointer, wheel (ADR-0322), focus, resize and close events, integer scale 1 or 2 (ADR-0164). No cursor shape, no
  clipboard, no popups from the C interface, no chooser, no second window.
- The clipboard is held by the compositor and moves only by gesture: System+C asks the focused client for an `Offer`,
  System+V delivers `Paste`; nothing reads it back (ADR-0170). The chooser gives a program a file the person picks and
  never its path (ADR-0167, ADR-0245). Neither is offered to POSIX programs (ADR-0317's follow-ups).
- Text: `zoraya-text` rasterises TrueType outlines without hinting or shaping (ADR-0162); NetSurf draws through it
  (ADR-0326, which rejected FreeType and fontconfig for NetSurf). Four OFL font files are in the image, Latin and
  Cyrillic only (ADR-0327).
- NetSurf (TASK-27 9d to 9g) is one process: curl and OpenSSL run inside it and reach `netd` through libposix under the
  `web` grant (public hosts, TCP ports 80 and 443, ADR-0324, ADR-0328). About 1,300 lines of C platform layer and nine
  patches; no JavaScript, no SVG.
- `top ROOM` shows a room's job tree (ADR-0267); a window's pills show its grants (ADR-0172); `grants` prints them.
- C++: libc++ 20.1.8 static with clang 18 in CI (ADR-0360, held at 20 because libc++ 21 needs clang 19), C++23 by
  default (ADR-0364), CMake through `ports/cmake/zoraya.cmake` (ADR-0367). TASK-30's report lists what a browser
  engine still needs: shared `mmap`, `madvise`, statically linked ICU data, more than 256 mappings, JIT-free modes,
  `AF_UNIX` and server sockets, `posix_spawn`.
- Licences: the "permissive only" rule of `CLAUDE.md` is for crates of the kernel and `no_std` crates. Ports are listed
  in `docs/dependencies.md`; GPL programs and LGPL TinyCC are in the image; no rule addresses an LGPL library linked
  statically into a program.

## The engines

### WPE WebKit

WebKit's port for embedded Linux, maintained by Igalia; the details are in
[WPE WebKit in detail](#wpe-webkit-in-detail), after the measurements, because many of them were read from the
measured tarball.

### Ladybird

Sources [L1] to [L55]; measured facts are from [Measurements](#measurements).

**State.** An independent engine, not a fork of Chromium, Gecko or WebKit; pre-release, with a first alpha for macOS
and Linux "coming later this year" and no date [L1][L2]. The October 2026 update reports a LibJS JIT on Linux and
macOS for x86-64 and AArch64, a new WebAssembly engine with its own interpreter and JIT, style, layout and paint
recording off the main thread, and Web Platform Tests subtest counts of 2,184,722 (Chrome 2,244,397, Firefox
2,189,315, Safari Technology Preview 2,172,990) [L2]. These are subtest counts, not a pass rate. The JIT is a build option, `ENABLE_JS_JIT`, on by default for x86-64 and
AArch64 on Linux (not Android) and Apple systems and x86-64 on Windows, and off elsewhere, where LibJS interprets
[L56]. The April 2026 issue
reports 333 merged pull requests from 35 contributors in a month [L3]. Windows builds and tests the JS engine; Android
and iOS come later [L1]. The porting document names Alpine, FreeBSD, OpenBSD and Haiku as POSIX systems known to work,
and says LibIPC needs Unix domain sockets [L45].

**Languages.** C++23 and, since 2026, Rust. Kling's post of 23 February 2026 announces Rust adoption with AI-assisted
translation, LibJS's front end first, and says the engine continues to be developed in C++ [L5]; the April issue says
Rust "is now mandatory" and the `ENABLE_RUST` option is gone [L3]. Swift was dropped (issue #933 closed 17 February
2026) [L4][L6]. **Measured** at `8ab2c0bb9c43`: 828,173 lines of C++ (with 31 `.mm` files) and 1,018,566 lines of Rust
outside tests, in 33 `Cargo.toml` files with 433 packages in `Cargo.lock`; LibWeb is 375,246 lines of C++ and 542,925
of Rust, LibJS 14,537 of C++ and 354,506 of Rust (with tests) [M]. So the JavaScript engine is now almost all Rust,
which is further than the February post's scope. `rust-toolchain.toml` pins Rust 1.98.0 [L11].

**Toolchain.** C++23; `Meta/Utils/find_compiler.py` requires clang 19 or GCC 14 (Xcode 16.3 on macOS); CI uses gcc-16
and clang-22; CMake 3.25 in the file, 3.30 in the documentation; Ninja, nasm, Python 3; Qt 6.10 or newer for the
desktop front end [L7][L39][L40][M].

**Processes** (from `Documentation/ProcessArchitecture.md` [L9]):

| Process | Role | Network | Files | Sandboxed |
|---|---|---|---|---|
| UI | the application; the only process talking to the window system | no | the person's | no |
| Hub | launches and connects the others; cookies, storage, permissions; which process hosts a document | no | profile | no |
| Compositor | rasterises what renderers record, presents frames, scrolls, animates images | no | no | yes |
| WebContent | LibWeb and LibJS; one per site, a spare kept ready | only through RequestServer | only through Hub | yes |
| RequestServer | HTTP and HTTPS, DNS, cookies from Hub; one per session and top-level site | yes | its site's cache | yes |
| ImageDecoder | decodes images into shared bitmaps | no | no | yes ("strongly") |
| MediaServer, VideoDecoderServer, AudioServer, WebRTCClient | media, on demand | (not stated) | (not stated) | (not stated) |

Web workers are threads in a WebContent process [L9]. Every process but UI and Hub sandboxes itself early, unless Hub
was started with `--disable-sandbox` [L9]. On Linux the sandbox is `PR_SET_NO_NEW_PRIVS`, Landlock (or a file broker
where Landlock is missing) and a seccomp allow-list for renderers; macOS uses Seatbelt; Windows has none yet
[L19][L20]. No `pledge` or `unveil` remains in the tree (**measured** by grep) [M].

**Spawning.** `WebView::Process` creates the IPC transport first and passes it to the child (a socket pair whose end
the child finds through `SOCKET_TAKEOVER` on Linux; Mach ports on macOS; a named pipe and a handle broker in Hub on
Windows) [L9]. `Core::Process::spawn` uses `clone(CLONE_VM|CLONE_VFORK)` on Linux, because `posix_spawn` cannot ask
for a parent-death signal or close undeclared descriptors, and `posix_spawn` on other systems [L15]. A spare
WebContent is pre-launched (`Libraries/LibWebView/Application.cpp:1727`) [M].

**IPC.** `IPC::TransportSocket` is a `socketpair(AF_LOCAL, SOCK_STREAM | SOCK_CLOEXEC)`, not `SOCK_SEQPACKET`; file
descriptors travel by `SCM_RIGHTS` in `Libraries/LibCore/Socket.cpp:390-453`, at most `MAX_TRANSFER_FDS` per message,
extra ones in separate `Attachments` frames; payloads above an inline limit go through an anonymous shared region,
read back with `pread`; the reader keeps at most 128 MiB unprocessed [L16][M]. Messages are described in 66 `.ipc`
files (3,304 lines) and generated by `Meta/Generators/generate_ipc_definitions.py` [L46][M]. The Rust crates have a
twin transport (`Libraries/LibIPC/Rust/src/socket.rs`) [M].

**Shared memory.** `Core::System::anon_create` is `memfd_create` on Linux and FreeBSD, `shm_open(SHM_ANON)` where
defined, and a random-named `shm_open` unlinked at once elsewhere; then `ftruncate`, and on Linux the seals
`F_SEAL_SHRINK|F_SEAL_GROW`; `AnonymousBuffer` maps it `MAP_SHARED` read-write on every platform [L13][L14]. Linux uses
`MADV_POPULATE_READ` with `MADV_WILLNEED` as fallback [L13].

**Event loop.** `EventLoopImplementationUnix::wait_for_file_descriptors` calls `ppoll` on Linux and `poll` elsewhere;
the self-wake is a `pipe2(O_CLOEXEC|O_NONBLOCK)`; no `epoll` or `kqueue` in LibCore (kqueue only in the macOS-only
ProcessReaper and a Compositor `.mm` file) (`Libraries/LibCore/EventLoopImplementationUnix.cpp:162,264,270`)
[L12][M].

**Dependencies.** `vcpkg.json` lists 49 dependencies [L8][M], among them ANGLE, brotli, cpptrace, curl (with HTTP/2,
HTTP/3, OpenSSL, WebSockets, zstd), dbus (Linux, FreeBSD), fast-float, FFmpeg, fontconfig, HarfBuzz, ICU 78.3,
libavif, libdwarf, libjpeg-turbo, libpng, libproxy, libpsl, libedit, libtommath, libwebp, libxml2, llvm-libc, mimalloc,
OpenSSL 3.6.5, pdf.js, Qt (non-Linux), SDL3, simdjson, simdutf, Skia 148, SQLite, TIFF, Vulkan and its headers and
allocator, woff2, wuffs, zlib and zmij. Licences checked: Skia BSD-3-Clause, HarfBuzz "Old MIT", ICU Unicode-3.0,
FFmpeg LGPL-2.1 or later, curl's MIT-style licence, Ladybird BSD-2-Clause [L41][L50]-[L55]; the others were not
checked. Where Skia is consumed on `master` was not found in the build files read [L30]-[L34] (an open discrepancy of
the research; the Compositor's Rust and C++ hold 30,859 lines [M]).

**Front ends.** `UI/CMakeLists.txt` builds exactly one of two front ends: Android, or Qt for every other platform (the
macOS shims are `.mm` files inside `UI/Qt`) [L42][L23]. **Measured**: `UI/Qt` is 64,515 lines, `UI/Android` 1,319 [M].
The April issue names GTK4 and AppKit front ends that are not on `master` [L3][L23]: unresolved. A headless mode
(`--headless`) exists for tests (`ProcessArchitecture.md`, read by the review) [L9]. What a front end
implements is `WebView::ViewImplementation`: three pure virtual functions (`viewport_size`, `to_content_position`,
`to_widget_position`) and optional hooks for the backing store, the clipboard (`clipboard_item`,
`read_clipboard_item`, `insert_clipboard_item`), screen rectangles, the theme, context menus, zoom and new tabs;
dialogs are callbacks such as `on_request_alert`; downloads are LibWebView's `FileDownloader` [L44][L25].

**Fonts and text.** `SystemFontMatcherFontconfig.cpp` on Linux and BSD, CoreText on macOS, DirectWrite on Windows, and a
`SystemFontMatcherNone.cpp`; shaping by HarfBuzz in `Libraries/LibGfx/TextLayout.cpp`, linked unconditionally; WOFF2
too [L26]-[L30].

**On SerenityOS, before the split.** SerenityOS's own document described Browser, one WebContent per tab (LibWeb and
LibJS; input from Browser; painting into shared bitmaps; the outside world only through RequestServer), RequestServer,
and ImageDecoder (a fresh, strongly sandboxed process per image), all but Browser running as an unprivileged user and
"aggressively sandboxed" with `pledge()` and `unveil()`; a WebContent was spawned by SystemServer for each connection
to `/tmp/session/%sid/portal/webcontent`; the document says the code does not yet fully match it [O23].

### Servo

Servo 0.1.0, its first crates.io release and an LTS line, was published on 2026-04-13; monthly releases follow and
break the API [B6][B4]. It is Rust (MPL-2.0) and builds with rustup, a pinned `rust-toolchain.toml` and `./mach
bootstrap`; targets are 64-bit Linux, macOS, Windows, Android and OpenHarmony, with FreeBSD builds added in 2026
[B3][B5][B7]. Its JavaScript engine is SpiderMonkey through the `mozjs` crates (Mozilla's ESR 153 branch), a prebuilt
archive by default or a source build that needs Python 3, clang and LLVM; no switch to disable the JIT is documented
[B9][B10]. A multiprocess mode with OS-sandboxed content processes is "the expected configuration for serious use";
without it script runs in the embedder [B10]. Rendering is WebRender on an OpenGL context through `surfman` and
`mozangle`; no CPU rasteriser is described [B8][B10]. A port therefore needs Rust `std`, a C++ SpiderMonkey build and
an OpenGL context (inference from [B8]-[B10]).

### Why Blink and Gecko are out of reach

| | Chromium (Blink) | Firefox (Gecko) |
|---|---|---|
| Build machine (stated) | 8 GB RAM minimum, more than 16 GB strongly recommended; about 20 GB RAM plus swap for a debug build in tmpfs; 100 GB free disk; a shared git cache of about 30 GB [B1] | 4 GB RAM minimum, 8 GB recommended; 30 GB free disk [B2] |
| Toolchain | its own clang, rebuilt from upstream about every two weeks, "the only supported compiler"; libc++ the only supported standard library; C++23 [B1][B11][B20] | clang and Rust; Rust 1.95.0 for Firefox 161 [B16] |
| Size | 4.8 million executable lines in 2013 (mean of 23 releases, tests removed) [B19]; no current count fetched | 2.9 million executable lines in 2013 [B19]; no current count fetched |
| Platform layer | `base/`, Ozone (a `PlatformWindow`, a `SurfaceFactoryOzone` with EGL or Skia, screen, input, overlay objects) [B12], Mojo's platform channel [B13], the sandbox | widget, IPDL over a transport not described in the fetched page [B17], the sandbox |

A Blink port must be built with Chromium's own pinned clang and libc++ [B11], which Zoraya's toolchain (clang 18,
libc++ 20) is not; a Gecko port needs Rust `std` for the target and a SpiderMonkey with or without its JIT. Neither
build fits beside a 16 GB machine's other work: Chromium's own guide recommends more than 16 GB [B1]. The precedents
below are the evidence that a small team does not port either: Fuchsia's Chromium port was Google's own and is now a
WebView-like service for smart displays only [O13][O14], and Genode reached Chromium through Qt WebEngine, a few
Chromium versions behind (below). `(inference)` from [B1][B11][B19] and the precedents.

Mojo is the precedent for an engine IPC written over native channels: `PlatformHandle` holds a Windows `HANDLE`, a
Fuchsia `zx_handle_t`, a Mach send right or a POSIX descriptor, and `PlatformChannel` is the per-OS pipe given to a
child [B13]. Chromium's older macOS IPC over Mach ports was abandoned because Mach ports could not be waited on in the
same `kqueue` as sockets, and a bridging thread cancelled their speed [B18].

## How other systems got a browser

Sources [O2] to [O23].

| System | Engine | When | Who | JIT | Several processes | Still missing |
|---|---|---|---|---|---|---|
| SerenityOS | LibWeb (June 2019) and LibJS (March 2020), written for it | browsed on SerenityOS from about 2020; Ladybird left on 3 June 2024 and dropped SerenityOS as a target [O2][O4] | one lead and volunteers `(estimate)` | not in the sources | yes, by its own document [O23] | the engine left; the OS no longer gets it [O2] |
| Haiku | HaikuWebKit (WebKitLegacy, one process) for WebPositive [O7][O8] | shipped before 2019 | a few maintainers [O9][O10] | not in the sources | WebKit2 renders "simple websites" (December 2025), not the default [O9][O10] | WebKit2 by default, scrolling, rendering classes that need EGL; upstream removed the port for lack of a build bot and maintainers [O7][O8] |
| Haiku (Ladybird) | Ladybird | basic pages in September 2023, after GCC 13 [O11] | one developer `(estimate)` | | yes | `AF_UNIX` datagram gaps found [O11] |
| Fuchsia | Chromium as WebEngine (`fuchsia.web`) [O13] | desktop work from 2021, ended 2023 [O14] (secondary) | Google | not in the sources | not in the sources | no general browser; smart displays only [O13][O14] |
| Genode | Chromium through Qt WebEngine (Falkon) | 20.08 (2020) [O17][O21] | Genode Labs `(estimate)` | yes, after a cache-maintenance fix [O17] | not in 20.08 [O17] | multi-process and sandboxing not in use (20.08); Chromium 83 then 112, "too old" by 26.05 [O16][O19] |
| PlayStation | WebKit, downstream of Safari | before 2019 | 5 committers, about 1400 patches in 2019 [O22] | no (a CFI build) [O22] | not stated | upstreaming |
| Redox | NetSurf; a Servo recipe | `[unverified]`: no Redox page could be fetched | | | | a modern engine `[unverified]` |

What they had to build:

- **Haiku**'s first port used Haiku's own API for the user interface and networking and POSIX for threads, and was
  removed from upstream WebKit for lack of a build bot and maintainers [O7]. Its WebKit2 work (GSoC 2024) moved WebKit's IPC from Haiku's native messaging to UNIX sockets and its shared
  memory to POSIX shared memory, which the author says lowers maintenance; the run loop is still Haiku's `BLooper` and
  drawing uses `BBitmap`; accelerated compositing is off because it crashed on EGL; the report also says the build needed
20 to 25 GB, that linking took about 30 minutes with `ld`, that the 2019 project's work had bit-rotted, and that after
it keyboard input, copy and paste and resizing still did not work (read by the review, `review-notes.md`) [O9]. Upstream WebKit would take the
  port back with two or three people eligible for commit access and a build bot [O8].
- **Genode** 20.08 based its Qt WebEngine port on FreeBSD's, which "resolved most libc problems", implemented shared
  memory with `mmap`, needed cache maintenance for JIT code, and did not yet use multimedia, certificate verification
  (libnss), multiple processes or sandboxing; Falkon's memory footprint "needs further optimization" [O17]. 22.08 found
  interactive performance "extremely poor" where OpenGL fell back to software, and flagged that Genode does not
  over-provision memory as Linux does as a possible "show stopper" [O15]. 25.02 moved to Qt 6.6.2 (Chromium 112)
  because Chromium 83 was too old for many sites [O16]; 26.05 moved to Qt 6.8.3 because that engine "became too old"
  [O19].
- **Fuchsia**'s components start with no capabilities and receive each one from their creator through routing [O12];
  WebEngine is built on Chromium's content layer and implements `fuchsia.web` [O13].
- **SerenityOS** wrote both engines and the browser, then lost them as a target when the browser needed Linux and
  macOS [O2][O4].

Lessons for Zoraya `(inference)`: (1) every port that reached users kept an engine's own multi-process design or gave
it up for a single process; none rewrote the engine's IPC on a native primitive except Fuchsia, by the engine's vendor;
(2) the ports that keep up are the ones upstream carries (Haiku lost its upstream place; Genode follows Qt's
releases); (3) memory without overcommit is named by Genode as the risk, which is Zoraya's eager commit too.

## Libraries and interfaces

Sources [P1] to [P56].

**ICU.** ICU4C 79.1 is current [P4]. `--with-data-packaging` has five modes: `library` (the default), `static` (one
large static library), `auto`, `files` and `archive` (one `icudt*.dat` file); with `files` and `archive` the
program links ICU's stub data library, with `library` and `static` the data library itself [P3]. The Data Build Tool filters data by locale, by feature (collation, currencies, time zones, units,
conversion, break iteration) and by resource, through `ICU_DATA_FILTER_FILE` [P1]. Its page gives the full data of
ICU 64 as about 27 MiB (11 MiB gzipped), the largest parts conversion mappings 4.9 MiB, break dictionaries 2.8 MiB,
collation 2.8 MiB, time zones 2.7 MiB, currencies 2.5 MiB, locales 2.4 MiB [P1]. **Measured**: `icudt79l.dat` in the
79.1 source is 33,558,160 bytes; ICU's C and C++ are 630,850 lines outside tests [M]. Licence: Unicode-3.0, with old
versions under the ICU licence; two build-time helpers are GPL with an Autoconf exception and are not linked [P6].
ICU can also be given its data at run time by a pointer: the same page says ICU uses the data an application provided
with `udata_setCommonData()` [P3].

**FreeType and HarfBuzz.** FreeType 2.15.0 (2026-10-12), under the FreeType License or GPLv2 [P7][P8]; HarfBuzz
15.1.0 (2026-11-04), "Old MIT" [P11][P12]; HarfBuzz has its own OpenType font functions (`hb-ot`) beside the FreeType
integration [P10]. **Measured**: FreeType 257,493 lines, HarfBuzz 238,050 outside tests [M].

**Fontconfig.** A library that reads XML configuration from `/etc/fonts/fonts.conf` and `$XDG_CONFIG_HOME`, scans
`<dir>` directories and caches under `$XDG_CACHE_HOME`; `FONTCONFIG_FILE`, `FONTCONFIG_PATH` and `FONTCONFIG_SYSROOT`
override them; MIT-style; needs expat; FreeType optional [P13][P14]. **Measured**: 2.18.3 is 31,886 lines outside
tests [M]. Skia ships font managers that need no fontconfig: one scanning a directory (`SkFontMgr_New_Custom_Directory`),
one over fonts in memory (`_Custom_Embedded`, `_Custom_Data`) and an empty one [P16]-[P20].

**Waiting.** POSIX Issue 8 has `poll` and `ppoll` (added in Issue 8) [P21]; `epoll` is Linux-only [P32]; `kqueue` is
BSD [P33]. GLib's main context waits with `g_poll`, which is `poll()` where available and `select()` otherwise; no
`epoll` or `ppoll` [P27][P28]; its wakeup includes `<sys/eventfd.h>` under `HAVE_EVENTFD` [P27]. libcurl's multi
interface reports sockets and a timer to the application's loop, or waits itself in `curl_multi_poll` [P30][P31].
Non-Unix precedents: Fuchsia's `zx_object_wait_many` (at most 64 handles) and ports fed by `zx_object_wait_async`
[P38][P39]; Windows' `WaitForMultipleObjects` (at most `MAXIMUM_WAIT_OBJECTS`) and completion ports [P49][P50].

**Unix sockets and descriptor passing.** Linux has stream, datagram and sequenced-packet Unix sockets, an abstract
namespace, and `SCM_RIGHTS` for up to 253 descriptors a message [P34]; FreeBSD has the three types and `SCM_RIGHTS`
[P35]. POSIX `socketpair` takes `SOCK_STREAM`, `SOCK_DGRAM` or `SOCK_SEQPACKET` [P22]. Windows' `AF_UNIX` (2017) is
stream only, with no `socketpair` and no `SCM_RIGHTS` [P36]. Fuchsia moves handles through channels with their rights
[P37]. Capsicum makes descriptors capabilities; in capability mode `shm_open` creates only anonymous objects [P53].

**Shared memory.** `memfd_create` with `MFD_ALLOW_SEALING`; seals `F_SEAL_SEAL`, `F_SEAL_SHRINK`, `F_SEAL_GROW`,
`F_SEAL_WRITE` and `F_SEAL_FUTURE_WRITE` [P42]; FreeBSD's `shm_open(SHM_ANON)` and `memfd_create` (13.0) [P44][P45];
POSIX `shm_open` (named) [P24]; `MAP_SHARED` [P47]. Fuchsia's VMOs are created with rights (map, read, write,
duplicate, transfer, resize if resizable) and mapped with `zx_vmar_map` [P40][P41]; Windows sections are named or
passed by handle [P48]. Linux's memory cgroup charges a shared area to the cgroup that instantiated it, and when
cgroups share it which one pays is "nondeterministic" [P51].

**posix_spawn.** POSIX's file actions run in order in the child; Issue 8 adds `addchdir` and `addfchdir` [P23]. musl
implements it with `clone(CLONE_VM|CLONE_VFORK)`, the file actions in the child and an error pipe [P25]; its chdir is
`posix_spawn_file_actions_addchdir_np` [P26].

**Mappings.** Linux's `vm.max_map_count` defaults to 65530 [P52]. V8's pointer compression reserves one 4 GB range per
isolate group [P54]; JavaScriptCore's Gigacage is described as a 4 GB region for array buffer storage [P56]. No source
fetched says how many mappings a browser's renderer uses.

## Measurements

Made on 2026-11-14 by downloading and counting (physical lines: one per `\n`, blank lines and comments included;
directories named `Tests`, `tests`, `LayoutTests`, `JSTests`, `ManualTests` and `PerformanceTests` excluded). The
counting script is `count.py ROOT EXTS EXCLUDED` (a 20-line Python file that sums newlines per extension). Nothing was
built or run.

| Tree | Version | Download | C and C++ lines outside tests | Other |
|---|---|---|---|---|
| WPE WebKit | 2.54.2 (`wpewebkit-2.54.2.tar.xz`, 49,565,492 bytes; 535 MiB unpacked, 38,757 files) | wpewebkit.org | 7,084,027 in `Source/`, of which 2,694,494 third party (ANGLE 1,508,819, Skia 1,117,965); 4,389,533 first party | JS 301,204 (mostly the Web Inspector); `.messages.in` 12,178 and `.serialization.in` 25,364 (generated IPC); Rust 40,034 (inside ANGLE and Skia) |
| WPE's five libraries | 2.54.2 | | JavaScriptCore 1,093,323; WebCore 1,941,599; WebKit 578,162; WTF 583,354; bmalloc 143,132 | `Source/WebKit/WPEPlatform` 29,512 |
| Ladybird | `8ab2c0bb9c4376f50bbcb7ea6f1b7717b089ff6e` (2026-11-14), about 392 MB of source | `git clone --depth 1` | 828,173 (with 31 `.mm`); none vendored | Rust 1,018,566; IDL 13,991; `.ipc` 3,304; `Tests/` 546,656 lines more |
| ICU4C | 79.1 (28,162,455 bytes) | GitHub release | 630,850 | `icudt79l.dat` 33,558,160 bytes |
| FreeType | 2.15.0 (2,716,928 bytes) | savannah | 257,493 | |
| HarfBuzz | 15.1.0 (20,515,628 bytes) | GitHub release | 238,050 | |
| fontconfig | 2.18.3 (1,172,016 bytes) | freedesktop GitLab | 31,886 | |

**Greps** (WPE: `Source/WebKit`, `Source/WTF`, `Source/WebCore/platform`; Ladybird: `Libraries/LibCore`, `LibIPC`,
`LibGfx`, `Services`, `UI`; files containing the word):

| Interface | WPE files | Ladybird files |
|---|---|---|
| `socketpair` | 1 (`Platform/IPC/unix/IPCUtilitiesUnix.cpp`) | 12 |
| `SCM_RIGHTS` | 1 (`Platform/IPC/unix/ConnectionUnix.cpp`) | 2 |
| `SOCK_SEQPACKET` | 4 | 0 |
| `memfd_create` | 3 | 4 |
| `shm_open` | 3 | 3 |
| `MAP_SHARED` | 5 | 5 |
| `epoll_` | 1 (a seccomp table) | 2 (Windows loop, a Linux sandbox list) |
| `poll(` | 6 | 8 |
| `eventfd` | 1 (`IPCSemaphoreUnix.cpp`) | 0 |
| `posix_spawn` | 2 | 4 |
| `fork(` | 2 | 2 |
| `g_subprocess` | 4 | n/a |
| `bwrap`, `bubblewrap` | 8 | n/a |
| `seccomp` | 2 | (LibSandbox) |
| `dlopen` | 4 (PlayStation and Darwin only) | 1 (a Linux sandbox) |
| `pledge`, `unveil` | n/a | 0 |

**WPE's IPC, as read.** `IPCUtilitiesUnix.cpp:40` makes `socketpair(AF_UNIX, socketType | SOCK_CLOEXEC, ...)`;
`ConnectionUnix.cpp:53-56` uses `SOCK_SEQPACKET` except on Darwin; `recvmsg` and `SCM_RIGHTS` at lines 245, 263, 424 and
453; WPE's own connection is `Platform/IPC/glib/ConnectionGLib.cpp` over `GSocket` (`g_socket_receive_message` :254,
`g_socket_send_message` :467, `createPlatformConnection(SOCK_SEQPACKET)` :516); `ProcessLauncherGLib.cpp:112` makes
the pair for each child. Shared memory is `Source/WebCore/platform/unix/SharedMemoryUnix.cpp` (`memfd_create` :87,
`shm_open` :102 and :111, `mmap(MAP_SHARED)` :135 and :152). `IPCSemaphoreUnix.cpp` uses `eventfd`. The run loop is
`Source/WTF/wtf/glib/RunLoopGLib.cpp` over `GMainContext` and `GSource` (:138-143). The launcher is
`Source/WebKit/UIProcess/Launcher/glib/ProcessLauncherGLib.cpp` with `BubblewrapLauncher.cpp`, `FlatpakLauncher.cpp`,
`XDGDBusProxy.cpp` and a seccomp table `Syscalls.h`. Messages are described in 240 `.messages.in` and 331
`.serialization.in` files.

**WPE's build requirements, as read.** `cmake_minimum_required(VERSION 3.20)`; `CMAKE_CXX_STANDARD 23`
(`Source/cmake/OptionsCommon.cmake:1-2`); GCC below 12.2 is an error (`WebKitCommon.cmake:127-129`), and
`WebKitCompilerFlags.cmake:244-253` says "Clang 18 does still need to be supported for the GTK and WPE ports".
`Source/cmake/WebKitFeatures.cmake` (lines about 75-110): on arm64 and x86-64 with 4 KiB pages `ENABLE_JIT`, the FTL
and not the C loop by default; with 64 KiB page blocks the JIT is off and the C loop on, except on ARM64. Required
packages (`find_package(... REQUIRED)` in `OptionsWPE.cmake`): GLib 2.70 (GIO Unix, threads, modules), HarfBuzz 2.7.4
(with ICU), ICU 70.1, libjpeg, Epoxy 1.5.4, libgcrypt 1.7, libsoup 3.0, libtasn1, xkbcommon 0.4, libxml2 2.9.13,
libpng, SQLite, WebP, zlib, FreeType 2.9; libxslt if XSLT; libwpe 1.13.3 if the legacy API (on by default); libinput
1.19 and udev for the DRM platform and Wayland 1.20 for the Wayland platform (both on by default); GStreamer 1.18.4 for
media; libseccomp, bwrap and xdg-dbus-proxy only with the Bubblewrap sandbox. Bundled: Skia (graphics; Cairo is off),
ANGLE (WebGL only), pdf.js, xdgmime. `ENABLE_GPU_PROCESS` defaults on and depends on GBM.

## WPE WebKit in detail

Sources [W1] to [W75] and [M].

**Releases.** 2.54.2 (23 October 2026) is the current stable release; 2.54.0 came out on 16 September 2026; majors
come every six months, in March and September, with point releases between them; development snapshots are odd minors
with a `.9x` patch number [W1][W2][W6]. LibWebRTC is planned as the default for 2.56 (around March 2027) [W6].

**Licence.** The FAQ says WPE is "a mix of LGPLv2 and BSD licenses, the same as the WebKit project"; libwpe and
WPEBackend-fdo are BSD-2-Clause [W6]. A sampled WTF header is BSD-2-Clause [W45].

**The WPEPlatform API.** A GObject library in `Source/WebKit/WPEPlatform/` that succeeds libwpe and WPEBackend-fdo;
used only by the UI process; the Web process renders into offscreen buffers shared with the UI process, which hands
them to a view [W9]. It was a preview in 2.48 and 2.50, and in 2.54.0 is "now considered stable and enabled by
default"; libwpe "may be deprecated in the future" [W2][W4][W5]. The development guide says it is always included from
2.56 [W12]. **Measured**: 29,512 lines in 2.54.2 [M]. Its types [W13]-[W21][W25]:

| Type | What a platform provides |
|---|---|
| `WPEDisplay` | the connection: vfuncs `connect` and `create_view` (mandatory), `create_toplevel` (strongly recommended), and optionally the EGL display, keymap, clipboard, preferred buffer formats, screens, DRM device, explicit sync, input method context, gamepad manager [W10][W13] |
| `WPEView` | `render_buffer`, which must report each buffer rendered (`wpe_view_buffer_rendered`) once shown and released (`wpe_view_buffer_released`) when no longer needed, or WebKit stops producing frames; cursor by name or bytes; pointer lock; opaque regions [W10][W17] |
| `WPEToplevel` | the window: title, fullscreen, maximise, minimise, resize; headless tracks only size and fullscreen [W11] |
| `WPEBuffer` | `WPEBufferDMABuf` (planes of descriptors), `WPEBufferSHM` (a `GBytes` of `ARGB8888`, the only format), `WPEBufferAndroid`; `wpe_buffer_import_to_pixels` and `_to_egl_image` [W14]-[W16] |
| `WPEEvent` | pointer down, up, move, enter, leave; scroll (with a precise flag); key down and up with keyval and hardware keycode; touch [W18] |
| `WPEKeymap` | keycode and modifiers to keyval; no key-repeat API [W19] |
| `WPEScreen` | position, logical and physical size, scale, refresh rate, sync observer [W20] |
| `WPEClipboard` | content get and set, synchronous `read_bytes` and `read_text`, a change count, formats as strings [W21] |

A platform registers at the GIO extension point `WPE_DISPLAY_EXTENSION_POINT_NAME` with a priority (Wayland 0, DRM and
headless -100); `WPE_PLATFORM` forces one by name; a backend can be linked in or loaded as a module [W10]. The built-in
backends: Wayland (DMA-BUF through linux-dmabuf, else an EGL import; SHM copied into a `wl_shm` pool, `ARGB8888` only;
a frame callback reports "rendered"), DRM (DMA-BUF only, imported with GBM, atomic KMS; SHM is "unsupported buffer"),
headless (any buffer, pixels never read, frames paced at 60 Hz) [W22]-[W24]. The architecture page calls SHM "the
software fallback" [W7]. A 2026 bring-up on a Raspberry Pi 3B with the Wayland platform needed two compile patches and a
minimal browser of the engineer's own [W28].

**Rendering.** Cairo was removed in 2.54.0; Skia replaces TextureMapper for the Web process compositor [W2]. In 2.48
Skia's GPU rendering was "not yet the WPE default" and `WEBKIT_SKIA_ENABLE_CPU_RENDERING=0` enabled it [W4]; 2.50 moved
threaded rendering onto `SkPictureRecorder` and added an experimental hybrid mode [W5]. libepoxy is required; no EGL
or GLES `find_package` is in `OptionsWPE.cmake` [W29]. Whether 2.54 runs end to end with no GPU and no EGL was not
confirmed by any fetched page `[unverified]`: the headless backend and the SHM path suggest it (inference from
[W10][W22][W24]).

**JavaScriptCore without the JIT.** `ENABLE_JIT` defaults on for ARM64, x86-64 and RISC-V 64 and off elsewhere;
`ENABLE_C_LOOP` defaults on where the JIT is off except on ARM64 with 64 KiB pages; the two conflict, and the C loop
also conflicts with WebAssembly and the sampling profiler; the DFG depends on the JIT [W30][M]. The offline assembler
picks `ARM64` on AArch64, and `C_LOOP` whenever the JIT is off and the C loop on [W31]. So an AArch64 build with
`ENABLE_JIT=OFF` and the C loop off uses the assembly LLInt, an interpreter compiled into the binary that writes no code
at run time (inference from [W30][W31]). Whether upstream's bots test either mode was not found; the only speed figure
(the assembly LLInt about twice the C loop, 2018) is a search snippet `[unverified]` [W30][W31]. The PlayStation port
ran without the JIT [O22].

**Processes.** The GLib launcher knows `Web`, `Network` and `GPU` (the last only with `ENABLE_GPU_PROCESS`) [W33].
`ENABLE_GPU_PROCESS` defaults on in the WPE port and depends on GBM [M]; in 2.48 the GPU process was built but off at
run time and handled only WebGL [W4]. Launching is `GSubprocess` with inherited descriptors, whose comment says the aim
is GLib's `posix_spawn()` path rather than `fork()`; the child's end of the socket pair is given with
`g_subprocess_launcher_take_fd` and its number goes on the child's command line [W33]. With the Bubblewrap sandbox
(default on Linux) the Web process runs in fresh namespaces (UTS, PID, network, IPC), with `/tmp` a tmpfs, `/etc`
read-only, system directories read-only, a filtered session bus through xdg-dbus-proxy, and a seccomp deny list
(`ptrace`, `unshare`, mounts, `TIOCSTI` and more); the network process is started without the sandbox; the Web
process is given the display socket, audio, fonts, GStreamer paths and GPU device nodes [W34].

**IPC.** `ConnectionUnix.cpp`: `SOCK_SEQPACKET` except on Darwin; at most 254 descriptors a message by one
`SCM_RIGHTS` message; an inline body up to 4096 bytes, larger ones through shared memory whose descriptor is the last
attachment; `recvmsg`/`sendmsg` with `MSG_NOSIGNAL`; on `EAGAIN` a send blocks in `poll(POLLOUT)` [W35]. The pair is
made by `socketpair(AF_UNIX, ...)` [W36]; a descriptor is encoded as an attachment [W38]. Messages are generated from
`.messages.in` and `.serialization.in` files [M]. `IPCSemaphoreUnix.cpp` uses `eventfd` [M].

**Shared memory.** In 2.54.2, `Source/WebCore/platform/unix/SharedMemoryUnix.cpp` calls `memfd_create`, falls back to
`shm_open`, and maps with `MAP_SHARED` [M] (the research subagent looked for it on `main` at the old path and did not
find it [W40][W41]; the measured tarball settles it). The launcher also seals a memfd for its Flatpak metadata
[W34].

**Event loop.** `RunLoopGLib.cpp` iterates a `GMainContext` by hand (prepare, query, the context's poll function,
check, dispatch); a wake-up sets a `GSource`'s ready time; repeating timers use a `timerfd` where `HAVE(TIMERFD)`
[W37]. GLib's poll function is `poll()` [P28].

**Dependencies.** Read from `OptionsWPE.cmake` of 2.54.2 [M] and of `main` [W29] (minimum versions from the
tarball):

| Library | Minimum | Licence | Role |
|---|---|---|---|
| GLib (GIO Unix, threads, modules) | 2.70 | LGPL-2.1 or later [W49] | objects, main loop, GIO |
| libsoup 3 | 3.0 | LGPL-2 [W48] | HTTP in the network process |
| HarfBuzz (with ICU) | 2.7.4 | Old MIT [W51] | shaping |
| ICU | 70.1 | Unicode-3.0 [W50] | text, Intl |
| FreeType | 2.9 | FTL [W67] | font files |
| libepoxy | 1.5.4 | MIT [W57] | GL loader |
| libxkbcommon | 0.4 | MIT [W58] | keymaps |
| libxml2 | 2.9.13 | MIT [W54] | XML |
| libgcrypt (required in 2.54.2; replaced by OpenSSL on `main`), with libgpg-error | 1.7 | LGPL-2.1 [W61] | Web Crypto |
| OpenSSL (on `main`) | 3.0 | Apache-2.0 [W68] | crypto |
| libtasn1 (required in 2.54.2) | any | `[unverified]` | ASN.1 |
| libjpeg-turbo, libpng, libwebp | any | IJG/BSD mix, PNG v2, BSD-3 [W55][W70][W56] | images |
| SQLite | any | public domain [W53] | storage |
| zlib | any | zlib [W59] | |
| WOFF2, AVIF, JPEG-XL, LCMS2, hyphen | 1.0.2, 0.9, 0.7, any, any | MIT, BSD-2, BSD-3, MIT, MPL/GPL/LGPL [W64]-[W66][W69][W71] | web fonts, images, colour, hyphenation |
| Wayland, libinput, udev, libdrm, GBM | 1.20, 1.19 | `[unverified]` | the built-in Wayland and DRM platforms only |
| GStreamer | 1.18.4 | LGPL [W73] | media, optional |
| libseccomp, bubblewrap, xdg-dbus-proxy | | LGPL-2.1 [W60][W62][W63] | the sandbox, optional |
| Skia, ANGLE, pdf.js, xdgmime | bundled | BSD-3 [W52] (Skia) | graphics, WebGL |

**Toolchain.** CMake 3.20; C++23 (`CMAKE_CXX_STANDARD 23` in 2.54.2's `OptionsCommon.cmake`); GCC 12.2 or newer in
2.54.2's `WebKitCommon.cmake`, 13.1 on `main`; clang has no minimum there, and `WebKitCompilerFlags.cmake` keeps
workarounds because "Clang 18 does still need to be supported for the GTK and WPE ports", with a FIXME to drop
them [M][W43][W44]. Ruby 2.5, Perl 5.10 and gperf 3.0.1 are build tools [W44]. No fetched file mentions libc++ for the
WPE port; the `stdc++fs` fallback is libstdc++'s [W43]. WebKit is built with libc++ on Apple's platforms
(inference: Apple's toolchain uses libc++).

**Ports.** WPE-Android (Igalia, a prototype in 2024) added process swap on navigation, `ASharedMemory` behind WebKit's
`SharedMemory`, and drove GLib's main loop from Android's looper through a `MessagePump` [W74][W75]; in 2.50 WPEPlatform
built on Android but did not render yet [W5]. What a port supplies, from those and the code: a launcher, the socket and
its event source under the shared Unix connection, the run loop, a buffer type and a view that reports rendered and
released, a keymap and input [W9][W10][W33][W35][W75].

## The engines compared

The numbers a decision needs, measured where the table says **measured**, else from the sources above or an
`(estimate)`.

| | WPE WebKit 2.54.2 | Ladybird (2026-11-14) | Servo 0.1 | Chromium, Firefox |
|---|---|---|---|---|
| Code to port (outside tests) | **measured** 4.39 M lines of first-party C++ plus 2.69 M bundled (ANGLE, Skia); only the platform layer is written here | **measured** 0.83 M C++ plus 1.02 M Rust | about 1 M lines of Rust `(estimate)`, plus SpiderMonkey | more than 10 M lines each `(estimate)`; 4.8 M and 2.9 M executable lines in 2013 [B19] |
| Code to write here `(estimate)` | a WPEPlatform backend (the built-in ones are 3 to 10 thousand lines each: **measured** total 29.5 k for the library with three backends), a launcher (hundreds of lines), the browser's own window and chrome (a few thousand) | a front end in place of Qt (the Qt one is **measured** 64.5 k lines; a minimal one a few thousand), the transport and sandbox hooks | an embedder with a GL context | Ozone or widget, `base` or NSPR, sandbox: tens of thousands |
| External libraries | about 28 for a CPU-only build with the Wayland, DRM, media and sandbox options off: the required set of the table above with GLib's own (libffi, PCRE2), libsoup's (libpsl, nghttp2, brotli), glib-networking, libgpg-error, plus bundled Skia (counted by the main session) | 49 entries in `vcpkg.json` (**measured**), about 38 distinct names once platform duplicates and Windows-only entries are removed (the review's count, `review-notes.md`); fewer for a CPU-only build without media, not counted; curl, OpenSSL, zlib, libpng and SQLite are already ported here | hundreds of crates `(estimate)` | hundreds `(estimate)` |
| Languages and compiler | C++23; clang 18 still supported; GCC 12.2 (13.1 on `main`) | C++23 with clang 19 or GCC 14; Rust 1.98 for every build | Rust `std` | Chromium's own clang; Rust for Firefox |
| JIT-free mode | the assembly LLInt or the C loop, selectable at build time [W30][W31] | `ENABLE_JS_JIT`, off by default on every system but Linux, macOS and Windows: "Elsewhere, LibJS runs JavaScript in its interpreter alone" [L56]; the WebAssembly engine's JIT has no option of its own in that file (not read further) | not documented [B9] | V8 jitless and SpiderMonkey's interpreter exist `[unverified]` |
| Process model | UI, Web, Network (GPU optional) | UI, Hub, Compositor, WebContent, RequestServer, ImageDecoder, media helpers | one or several | many |
| IPC | `socketpair` `SOCK_SEQPACKET`, `SCM_RIGHTS`, memfd | `socketpair` `SOCK_STREAM`, `SCM_RIGHTS`, memfd | ipc-channel `(estimate)` | Mojo, IPDL |
| Event loop | GLib `poll` | `ppoll` or `poll` | tokio and others `(estimate)` | own |
| Upstream state | stable releases every six months | pre-alpha, alpha "later this year" | 0.1, monthly breaking releases | stable |
| Clean build on a 16 GB M4 | 1 to 2 hours at 6 jobs, 10 to 20 GB of build tree `(estimate)` | 30 to 60 minutes plus the Rust crates, about 10 GB `(estimate)` | tens of minutes with prebuilt SpiderMonkey `(estimate)` | hours; Chromium recommends more than 16 GB RAM [B1] |
| Guest memory, one simple tab | 300 to 600 MiB with eager commit and per-process copies of the program `(estimate)` | 400 to 800 MiB (more processes) `(estimate)` | not estimated | not estimated |

The memory estimates are the main session's: three processes of WPE (UI, Web, Network) each loading its own copy of a
static program of the order of 50 to 100 MB of text and data (Zoraya copies every segment at spawn, ADR-0108), plus a
Web process heap of 100 to 200 MiB for a simple page with eager commit; Ladybird runs at least six processes. They are
to be measured by the plan's first engine part.

## Pages that could not be fetched

- Open Hub (`openhub.net/p/chromium`, `/p/firefox`, `/p/servo`): HTTP 403. No current line count of Chromium,
  Firefox or Servo.
- `chromium.googlesource.com/.../docs/linux/sandboxing.md`, `docs/cxx.md`: 404. Firefox's sandbox page: 404. The
  Fuchsia WebEngine concept page: 404. Servo's build page: 404.
- `redox-os.org` (403), the Redox forum (DNS failure), the Redox GitLab (403, also the cookbook's NetSurf recipe fetched
  by the main session). Every Redox fact is `[unverified]`.
- Ladybird: `Documentation/ProcessModel.md` (404; `ProcessArchitecture.md` was read), the recursive tree (too large),
  `ladybird.org/posts/` (404), `LibJS/JIT/CMakeLists.txt` (404). Not read: `LocalSocket` beyond the lines counted,
  `ShareableBitmap.cpp`, the IPC generator, `Seccomp.cpp`.
- ICU's `icu4c/source/README.html` (404); `fcntl(2)`'s seal sub-pages; Android's `fonts.xml` documentation (404);
  fontconfig's README and FreeType's CHANGES (a bot check).
- The Haiku WebKit README, WebKit bug 290172, the Ladybird port to Haiku after 2023, Sculpt OS 26.10's notes: search
  snippets only, not used.

## Sources

Tags carry the letter of their section. Every entry was fetched on 2026-11-14.

### Sources: WPE WebKit (W)

- [W1] WPE WebKit release index, wpewebkit.org, date of index as extracted (includes 2.54.2 dated 23 Oct 2026), https://wpewebkit.org/release/ , primary. Supports: release list and dates, stable and development labels, release counts (from the page as summarised by the fetch tool), cadence (majors in March and September)
- [W2] WPE WebKit 2.54.0 release notes, wpewebkit.org, 16 Sep 2026, https://wpewebkit.org/release/wpewebkit-2.54.0.html , primary. Supports: 2.54.0 date, WPEPlatform stable and default, libwpe may be deprecated, Cairo option removed, Skia replaces TextureMapper for the WebProcess compositor, Enchant spellcheck
- [W3] WPE WebKit 2.52.0 blog post, wpewebkit.org, 18 Mar 2026, https://wpewebkit.org/blog/2026-03-18-wpewebkit-2.52.html , primary. Supports: WPEPlatform new APIs (create_toplevel, disconnected signal, buffers-changed), ScreenSyncObserver change, Android WPEPlatform, libwpe sunset from 2.54, Cog status, ENABLE_WPE_LEGACY_API, WebRTC network and librice, Flatpak SDK removal
- [W4] WPE WebKit 2.48 blog post, wpewebkit.org, 11 Apr 2025, https://wpewebkit.org/blog/2025-04-11-wpewebkit-2.48.html , primary. Supports: Skia CPU vs GPU rendering and WEBKIT_SKIA_ENABLE_CPU_RENDERING, GPU process built but off at runtime, UseGPUProcessForWebGL, WPEPlatform preview and ENABLE_WPE_PLATFORM, ICU 70.1, GStreamer 1.24.9 for WebM
- [W5] WPE WebKit 2.50 blog post, wpewebkit.org, 27 Nov 2025, https://wpewebkit.org/blog/2025-11-27-wpewebkit-2.50.html , primary. Supports: Skia API threaded rendering with SkPictureRecorder, hybrid GPU/CPU workers, GCC 12.2 claim, WPEPlatform buffers (Android AHardwareBuffer), WPEScreenSyncObserver introduced, WPEGamepad, Android build notes, fence move to WPEBuffer, WPEPlatform preview to default in 2.52
- [W6] WPE FAQ, wpewebkit.org, https://wpewebkit.org/about/faq.html , primary. Supports: WPEPlatform default and recommended, legacy libwpe and fdo status, Wayland/DRM/headless need no libwpe, fontconfig and GStreamer runtime needs, Linux-only and Android in progress, licences (LGPLv2 and BSD, libwpe BSD-2, Cog MIT), release every 6 months, LibWebRTC plan for 2.56
- [W7] WPE architecture page, wpewebkit.org, https://wpewebkit.org/about/architecture.html , primary. Supports: buffers as WPEBuffer (DMA-BUF or Android preferred, SHM fallback), platform types (display, view, toplevel), legacy EGL model
- [W8] WPE home page, wpewebkit.org, https://wpewebkit.org/ , primary. Supports: WPE is an embedded-focused engine, no toolkit dependency, Igalia maintains it, use cases
- [W9] WPEPlatform overview (docs/overview.md), WebKit repository, Source/WebKit/WPEPlatform/docs/overview.md, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/docs/overview.md , primary. Supports: WPEPlatform classes, UI-process-only use, built-in backends, the subclassing contract, buffer types named, status of libwpe, things with no WPEPlatform equivalent
- [W10] WPEPlatform platform tutorial (docs/tutorial-platform.md), https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/docs/tutorial-platform.md , primary. Supports: platform selection and registration, priority values, mandatory and optional vfuncs, render_buffer rendered and released contract, sizing and mapping rules, error domain
- [W11] WPEPlatform migration from libwpe (docs/migrating-from-libwpe.md), https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/docs/migrating-from-libwpe.md , primary. Supports: concept mapping, removed APIs, toplevel support per platform, WebKitWebView backend property ignored
- [W12] WPEPlatform compiling guide (docs/compiling.md), https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/docs/compiling.md , primary. Supports: WPEPlatform always included since 2.56, in 2.54 can be disabled, ENABLE_WPE_1_1_API is the only non-WPEPlatform build, GBM needed for DRM backend (summary extraction)
- [W13] WPEDisplay.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEDisplay.h , primary. Supports: WPEDisplay public API and vfuncs
- [W14] WPEBuffer.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEBuffer.h , primary. Supports: WPEBuffer API, fences, import functions, no format accessor
- [W15] WPEBufferDMABuf.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEBufferDMABuf.h , primary. Supports: DMA-BUF constructor, plane getters, format and modifier
- [W16] WPEBufferSHM.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEBufferSHM.h , primary. Supports: SHM constructor with GBytes, ARGB8888 only
- [W17] WPEView.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEView.h , primary. Supports: WPEView API and vfuncs, no EGL functions
- [W18] WPEEvent.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEEvent.h , primary. Supports: event types and accessors
- [W19] WPEKeymap.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEKeymap.h , primary. Supports: keymap API, no key-repeat API
- [W20] WPEScreen.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEScreen.h , primary. Supports: screen API
- [W21] WPEClipboard.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/WPEClipboard.h , primary. Supports: clipboard API, synchronous reads
- [W22] WPEViewWayland.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/wayland/WPEViewWayland.cpp , primary. Supports: Wayland DMA-BUF and SHM paths, EGL fallback, commit and release and frame callbacks, pointer lock
- [W23] WPEViewDRM.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/drm/WPEViewDRM.cpp , primary. Supports: DRM accepts DMA-BUF only, GBM import, atomic KMS, page-flip reporting, no EGL
- [W24] WPEViewHeadless.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/WPEPlatform/wpe/headless/WPEViewHeadless.cpp , primary. Supports: headless accepts any buffer, no pixel reads, 60 Hz pacing, overwrite behaviour
- [W25] WPEPlatform/wpe directory listing, GitHub API, https://api.github.com/repos/WebKit/WebKit/contents/Source/WebKit/WPEPlatform/wpe , primary. Supports: list of headers and subdirectories
- [W26] WPEPlatform wayland, drm and headless directory listings, GitHub API, https://api.github.com/repos/WebKit/WebKit/contents/Source/WebKit/WPEPlatform/wpe/wayland , https://api.github.com/repos/WebKit/WebKit/contents/Source/WebKit/WPEPlatform/wpe/drm , https://api.github.com/repos/WebKit/WebKit/contents/Source/WebKit/WPEPlatform/wpe/headless , primary. Supports: which backend files exist, absence of keymap and clipboard in drm and headless
- [W27] WPEPlatform top-level directory listing, GitHub API, https://api.github.com/repos/WebKit/WebKit/contents/Source/WebKit/WPEPlatform , primary. Supports: WPEPlatform directory layout (docs, scripts, wpe)
- [W28] Igalia blog, "Untangling the Web" / Raspberry Pi WPE Platform API trial, 08 Sep 2026, https://blogs.igalia.com/plampe/trying-wpe-platform-api-on-raspberry-pi/ , primary (engineer's blog). Supports: 2026 bring-up on Raspberry Pi 3B with the Wayland platform, patches, options, GPU configuration, no benchmarks
- [W29] Source/cmake/OptionsWPE.cmake, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/cmake/OptionsWPE.cmake , primary. Supports: WPE build options and defaults, find_package minimum versions, USE_SKIA ON, USE_CAIRO FALSE, USE_LIBEPOXY, USE_LIBDRM and USE_GBM, ENABLE_BUBBLEWRAP_SANDBOX default on Linux, ENABLE_GPU_PROCESS default ON in the port
- [W30] Source/cmake/WebKitFeatures.cmake, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/cmake/WebKitFeatures.cmake , primary. Supports: ENABLE_JIT and ENABLE_C_LOOP defaults by processor and their conflicts, USE_SKIA default OFF, ENABLE_GPU_PROCESS default OFF in the shared file, ENABLE_WEBGL ON, ENABLE_BUBBLEWRAP_SANDBOX default OFF in the shared file
- [W31] Source/JavaScriptCore/CMakeLists.txt, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/JavaScriptCore/CMakeLists.txt , primary. Supports: offlineasm backend selection by architecture and the C_LOOP override
- [W32] Source/cmake/OptionsCommon.cmake, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/cmake/OptionsCommon.cmake , primary. Supports: no JIT, C loop or Skia definitions in this file (fetch used only to confirm absence; no further claim)
- [W33] Source/WebKit/UIProcess/Launcher/glib/ProcessLauncherGLib.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/UIProcess/Launcher/glib/ProcessLauncherGLib.cpp , primary. Supports: GSubprocess spawn, process types, bubblewrap and flatpak paths, client fd via take_fd and argv, close-on-exec for xdg-dbus-proxy
- [W34] Source/WebKit/UIProcess/Launcher/glib/BubblewrapLauncher.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/UIProcess/Launcher/glib/BubblewrapLauncher.cpp , primary. Supports: bwrap namespaces and mounts, Web process access, xdg-dbus-proxy, seccomp deny list, memfd for flatpak-info and BPF, environment overrides; WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS not present
- [W35] Source/WebKit/Platform/IPC/unix/ConnectionUnix.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/Platform/IPC/unix/ConnectionUnix.cpp , primary. Supports: SOCK_SEQPACKET, SCM_RIGHTS limits, 4096-byte inline limit, shared-memory fallback for large bodies, sendmsg/recvmsg, poll on EAGAIN, PlayStation select thread
- [W36] Source/WebKit/Platform/IPC/unix/IPCUtilitiesUnix.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/Platform/IPC/unix/IPCUtilitiesUnix.cpp , primary. Supports: socketpair(AF_UNIX), SOCK_CLOEXEC handling, RELEASE_ASSERT on syscalls
- [W37] Source/WTF/wtf/glib/RunLoopGLib.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WTF/wtf/glib/RunLoopGLib.cpp , primary. Supports: manual GMainContext iteration with poll, wakeUp via ready time, timerfd, libdispatch eventfd
- [W38] Source/WebKit/Platform/IPC/unix/ArgumentCodersUnix.cpp, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/Platform/IPC/unix/ArgumentCodersUnix.cpp , primary. Supports: fd serialization as attachments
- [W39] Source/WebKit/Platform/IPC/unix directory listing, https://api.github.com/repos/WebKit/WebKit/contents/Source/WebKit/Platform/IPC/unix , primary. Supports: IPC unix file set
- [W40] Source/WTF/wtf/unix directory listing, https://api.github.com/repos/WebKit/WebKit/contents/Source/WTF/wtf/unix , primary. Supports: no shared-memory file in WTF unix
- [W41] Source/WebKit/Platform/unix and Source/WebKit/Shared/unix directory listings, https://api.github.com/repos/WebKit/WebKit/contents/Source/WebKit/Platform/unix and https://api.github.com/repos/WebKit/WebKit/contents/Source/WebKit/Shared/unix , primary. Supports: absence of SharedMemoryUnix on main in those folders
- [W42] Root CMakeLists.txt, https://raw.githubusercontent.com/WebKit/WebKit/main/CMakeLists.txt , primary. Supports: cmake_minimum_required 3.20, languages C and CXX
- [W43] Source/cmake/WebKitCompilerFlags.cmake, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/cmake/WebKitCompilerFlags.cmake , primary. Supports: C++17 and C++23 checks, coroutine flags, GCC 14.0.1 and Clang 18 workarounds
- [W44] Source/cmake/WebKitCommon.cmake, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/cmake/WebKitCommon.cmake , primary. Supports: GCC 13.1 minimum, Ruby, Perl, gperf minimums
- [W45] Source/WTF/wtf/Assertions.h, https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WTF/wtf/Assertions.h , primary. Supports: BSD-2-Clause header on a WebKit source file, Apple copyright
- [W46] WebKit ReadMe.md, https://raw.githubusercontent.com/WebKit/WebKit/main/ReadMe.md , primary. Supports: no licence statement in this file (negative finding)
- [W47] WebKit repository root listing, https://api.github.com/repos/WebKit/WebKit/contents/ , primary. Supports: no COPYING or LICENSE file at repository root
- [W48] libsoup COPYING (GNOME GitHub mirror), https://raw.githubusercontent.com/GNOME/libsoup/master/COPYING , primary. Supports: libsoup LGPL-2 (Library General Public License v2)
- [W49] GLib COPYING, https://gitlab.gnome.org/GNOME/glib/-/raw/main/COPYING , primary. Supports: GLib LGPL-2.1 or later
- [W50] ICU license, https://raw.githubusercontent.com/unicode-org/icu/main/icu4c/LICENSE , primary. Supports: ICU Unicode License V3 / Unicode-3.0 and third-party notices
- [W51] HarfBuzz COPYING, https://raw.githubusercontent.com/harfbuzz/harfbuzz/main/COPYING , primary. Supports: HarfBuzz "Old MIT", with subdirectory licences
- [W52] Skia LICENSE, https://raw.githubusercontent.com/google/skia/main/LICENSE , primary. Supports: Skia BSD-3-Clause
- [W53] SQLite copyright page, https://sqlite.org/copyright.html , primary. Supports: SQLite public domain
- [W54] libxml2 Copyright file, https://raw.githubusercontent.com/GNOME/libxml2/master/Copyright , primary. Supports: libxml2 MIT
- [W55] libjpeg-turbo LICENSE.md, https://raw.githubusercontent.com/libjpeg-turbo/libjpeg-turbo/main/LICENSE.md , primary. Supports: IJG, modified BSD, zlib, PNG-ref v2 and BSD-2 components
- [W56] libwebp COPYING, https://raw.githubusercontent.com/webmproject/libwebp/main/COPYING , primary. Supports: libwebp BSD-3-Clause
- [W57] libepoxy COPYING, https://raw.githubusercontent.com/anholt/libepoxy/master/COPYING , primary. Supports: libepoxy MIT
- [W58] libxkbcommon LICENSE, https://raw.githubusercontent.com/xkbcommon/libxkbcommon/master/LICENSE , primary. Supports: libxkbcommon MIT and other permissive notices
- [W59] zlib LICENSE, https://raw.githubusercontent.com/madler/zlib/develop/LICENSE , primary. Supports: zlib licence
- [W60] bubblewrap COPYING, https://raw.githubusercontent.com/containers/bubblewrap/main/COPYING , primary. Supports: bubblewrap LGPL-2.1
- [W61] libgcrypt COPYING.LIB, https://raw.githubusercontent.com/gpg/libgcrypt/master/COPYING.LIB , primary. Supports: libgcrypt LGPL-2.1
- [W62] xdg-dbus-proxy COPYING, https://raw.githubusercontent.com/flatpak/xdg-dbus-proxy/main/COPYING , primary. Supports: xdg-dbus-proxy LGPL-2.1
- [W63] libseccomp LICENSE, https://raw.githubusercontent.com/seccomp/libseccomp/main/LICENSE , primary. Supports: libseccomp LGPL-2.1
- [W64] libavif LICENSE, https://raw.githubusercontent.com/AOMediaCodec/libavif/main/LICENSE , primary. Supports: libavif BSD-2-Clause and other notices
- [W65] libjxl LICENSE, https://raw.githubusercontent.com/libjxl/libjxl/main/LICENSE , primary. Supports: libjxl BSD-3-Clause and other notices
- [W66] woff2 LICENSE, https://raw.githubusercontent.com/google/woff2/master/LICENSE , primary. Supports: woff2 MIT
- [W67] FreeType licence (FTL.TXT), https://raw.githubusercontent.com/freetype/freetype/master/docs/FTL.TXT , primary. Supports: FreeType License
- [W68] OpenSSL LICENSE.txt, https://raw.githubusercontent.com/openssl/openssl/master/LICENSE.txt , primary. Supports: OpenSSL Apache-2.0
- [W69] Little CMS LICENSE, https://raw.githubusercontent.com/mm2/Little-CMS/master/LICENSE , primary. Supports: lcms2 MIT
- [W70] libpng LICENSE, https://raw.githubusercontent.com/pnggroup/libpng/master/LICENSE , primary. Supports: PNG Reference Library License v2
- [W71] hyphen COPYING, https://raw.githubusercontent.com/hunspell/hyphen/master/COPYING , primary. Supports: hyphen tri-licence and a separate pattern-file notice
- [W73] GStreamer licensing FAQ, https://gstreamer.freedesktop.org/documentation/frequently-asked-questions/licensing.html , primary. Supports: GStreamer core LGPL, plugin rules
- [W74] Igalia (Jani), "Bringing WebKit back to Android", 20 Mar 2024, https://blogs.igalia.com/jani/bringing-webkit-back-to-android/ , primary (engineer's blog). Supports: WPE-Android prototype status, PSON, ASharedMemory, GLib main loop on Android looper, Cerbero, arm64 and x86_64 only, 32-bit dropped
- [W75] Igalia (Jani), "Bringing WebKit back to Android: Internals", 17 Oct 2024, https://blogs.igalia.com/jani/bringing-webkit-back-to-android-internals/ , primary (engineer's blog). Supports: libwpe-based WPE-Android, backend plugin, JNI, SurfaceControl, GLib FDs in Android looper via MessagePump, WPE 2.46 base, Skia backend enabled

### Sources: Ladybird (L)

- [L1] Ladybird home page, https://ladybird.org/, primary. Independent engine; alpha for macOS and Linux later this
  year; Windows and mobile status.
- [L2] Ladybird Monthly Update: October 2026, https://ladybird.org/newsletter/2026-10-31/, primary. JIT on Linux and
  macOS (x86-64, AArch64); Wasm engine; threading; WPT subtest counts; Rust in LibJS's runtime and style.
- [L3] This Month in Ladybird, April 2026, https://ladybird.org/newsletter/2026-04-30/, primary. 333 pull requests, 35
  contributors; "Rust is now mandatory"; GTK4 front end named.
- [L4] This Month in Ladybird, February 2026, https://ladybird.org/newsletter/2026-02-28/, primary. Swift removed; the
  Rust LibJS front end on by default.
- [L5] Andreas Kling, "Ladybird adopts Rust, with help from AI", 2026-02-23, https://ladybird.org/posts/adopting-rust/,
  primary. Scope, method, "continue developing the engine in C++".
- [L6] Issue #933 "Swift 6.0 Blockers", closed 2026-02-17, https://github.com/LadybirdBrowser/ladybird/issues/933,
  primary.
- [L7] `Documentation/BuildInstructionsLadybird.md`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Documentation/BuildInstructionsLadybird.md,
  primary. C++23, Qt 6.10, CMake 3.30, CI compilers, platforms.
- [L8] `vcpkg.json`, https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/vcpkg.json, primary.
- [L9] `Documentation/ProcessArchitecture.md`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Documentation/ProcessArchitecture.md, primary.
  Processes, spawning per OS, sandbox coverage.
- [L10] `Cargo.toml` (workspace), https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Cargo.toml, primary.
- [L11] `rust-toolchain.toml`, https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/rust-toolchain.toml,
  primary. Rust 1.98.0.
- [L12] `Libraries/LibCore/EventLoopImplementationUnix.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibCore/EventLoopImplementationUnix.cpp,
  primary. `ppoll` on Linux, `poll` elsewhere.
- [L13] `Libraries/LibCore/AnonymousBuffer.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibCore/AnonymousBuffer.cpp, primary.
- [L14] `Libraries/LibCore/System.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibCore/System.cpp, primary.
  `anon_create`.
- [L15] `Libraries/LibCore/Process.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibCore/Process.cpp, primary. `clone` on
  Linux, `posix_spawn` elsewhere, and why.
- [L16] `Libraries/LibIPC/TransportSocket.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibIPC/TransportSocket.cpp, primary.
- [L19] `Services/RendererSandboxLinux.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Services/RendererSandboxLinux.cpp, primary.
- [L20] `Libraries/LibSandbox/Sandbox.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibSandbox/Sandbox.cpp, primary.
- [L23] Repository root and `UI/` listings, https://api.github.com/repos/LadybirdBrowser/ladybird/contents/UI, primary.
- [L25] `Libraries/LibWebView` listing, https://api.github.com/repos/LadybirdBrowser/ladybird/contents/Libraries/LibWebView,
  primary.
- [L26] `Libraries/LibGfx/Font` listing,
  https://api.github.com/repos/LadybirdBrowser/ladybird/contents/Libraries/LibGfx/Font, primary.
- [L27] `SystemFontMatcherFontconfig.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibGfx/Font/SystemFontMatcherFontconfig.cpp,
  primary.
- [L28] `SystemFontMatcherCoreText.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibGfx/Font/SystemFontMatcherCoreText.cpp,
  primary.
- [L29] `Libraries/LibGfx/TextLayout.cpp`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibGfx/TextLayout.cpp, primary. HarfBuzz.
- [L30] `Libraries/LibGfx/CMakeLists.txt`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibGfx/CMakeLists.txt, primary.
- [L31] `Libraries/LibGfx/Rust/Cargo.toml`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibGfx/Rust/Cargo.toml, primary.
- [L32] `Libraries/LibCompositing/CMakeLists.txt`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibCompositing/CMakeLists.txt, primary.
- [L33] `Libraries/LibCompositing/Rust/Cargo.toml`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibCompositing/Rust/Cargo.toml, primary.
- [L34] `Libraries/LibWeb/CMakeLists.txt`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibWeb/CMakeLists.txt, primary.
- [L39] `Meta/Utils/find_compiler.py`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Meta/Utils/find_compiler.py, primary. Clang 19, GCC
  14.
- [L40] Root `CMakeLists.txt`, https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/CMakeLists.txt, primary.
- [L41] Root `LICENSE`, https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/LICENSE, primary. BSD-2-Clause.
- [L42] `UI/CMakeLists.txt`, https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/UI/CMakeLists.txt,
  primary.
- [L44] `Libraries/LibWebView/ViewImplementation.h`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Libraries/LibWebView/ViewImplementation.h, primary.
- [L45] `Documentation/Porting.md`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/master/Documentation/Porting.md, primary.
- [L46] `Meta/Generators` listing, https://api.github.com/repos/LadybirdBrowser/ladybird/contents/Meta/Generators,
  primary.
- [L50] Skia `LICENSE`, https://raw.githubusercontent.com/google/skia/main/LICENSE, primary. BSD-3-Clause.
- [L51] HarfBuzz `COPYING`, https://raw.githubusercontent.com/harfbuzz/harfbuzz/main/COPYING, primary.
- [L52] ICU `LICENSE`, https://raw.githubusercontent.com/unicode-org/icu/main/LICENSE, primary.
- [L53] FFmpeg `LICENSE.md`, https://raw.githubusercontent.com/FFmpeg/FFmpeg/master/LICENSE.md, primary. LGPL-2.1 or
  later by default.
- [L54] OpenSSL `LICENSE.txt`, https://raw.githubusercontent.com/openssl/openssl/master/LICENSE.txt, primary.
- [L55] curl `COPYING`, https://raw.githubusercontent.com/curl/curl/master/COPYING, primary.
- [L56] `Meta/CMake/cmake_options.cmake` at the measured commit,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/8ab2c0bb9c4376f50bbcb7ea6f1b7717b089ff6e/Meta/CMake/cmake_options.cmake,
  primary (fetched by the main session after the review). `ENABLE_JS_JIT` and its default.

### Sources: Servo, Blink and Gecko (B)

- [B1] Chromium, Linux Build Instructions,
  https://chromium.googlesource.com/chromium/src/+/main/docs/linux/build_instructions.md, primary. RAM, disk, swap,
  libc++ and clang only.
- [B2] Mozilla, Linux Build Instructions (Firefox), https://firefox-source-docs.mozilla.org/setup/linux_build.html,
  primary. 4 GB / 8 GB, 30 GB.
- [B3] Servo Book, https://book.servo.org/, primary.
- [B4] Servo blog index, https://servo.org/blog/, primary.
- [B5] servo/servo README, https://github.com/servo/servo, primary.
- [B6] "Servo is now available on crates.io", 2026-04-13,
  https://servo.org/blog/2026/04/13/servo-0.1.0-release/, primary.
- [B7] "March in Servo", 2026-04-30, https://servo.org/blog/2026/04/30/march-in-servo, primary.
- [B8] servo 0.1.0 on docs.rs, https://docs.rs/crate/servo/0.1.0, primary.
- [B9] servo/mozjs, https://github.com/servo/mozjs, primary.
- [B10] Servo Book, Architecture, https://book.servo.org/design-documentation/architecture.html, primary.
- [B11] Chromium, Clang, https://chromium.googlesource.com/chromium/src/+/main/docs/clang.md, primary.
- [B12] Chromium, Ozone Overview, https://chromium.googlesource.com/chromium/src/+/main/docs/ozone_overview.md,
  primary.
- [B13] Chromium, Mojo platform layer README,
  https://chromium.googlesource.com/chromium/src/+/main/mojo/public/cpp/platform/README.md, primary.
- [B16] Mozilla, Rust Update Policy,
  https://firefox-source-docs.mozilla.org/writing-rust-code/update-policy.html, primary.
- [B17] Mozilla, IPC (Gecko), https://firefox-source-docs.mozilla.org/ipc/index.html, primary.
- [B18] Chromium, Mach based OS X Interprocess Communication (obsolete),
  https://www.chromium.org/developers/design-documents/os-x-interprocess-communication/, primary.
- [B19] A. Almossawi, Measuring the size of Chromium and Firefox releases, 2013,
  https://almossawi.com/firefox/prose-part-two, secondary.
- [B20] Chromium, C++ Style Guide, https://chromium.googlesource.com/chromium/src/+/main/styleguide/c++/c++.md,
  primary. C++23.

### Sources: other systems (O)

- [O2] A. Kling, "Ladybird is forking off from SerenityOS", 2024-06-03, https://ladybird.org/posts/fork/, primary.
- [O4] A. Kling, "Ladybird: A new cross-platform browser project", 2022-09-12,
  https://awesomekling.substack.com/p/ladybird-a-new-cross-platform-browser-project, primary. LibWeb June 2019, LibJS
  March 2020.
- [O7] WebKit Trac, HaikuWebkit (last modified 2013-10-10), https://trac.webkit.org/wiki/HaikuWebkit, primary.
- [O8] Haiku forum, "WebKit status?", August 2019, https://discuss.haiku-os.org/t/webkit-status/8822, primary (forum).
- [O9] Zardshard, [GSoC 2024] Porting WebKit2 Final Report, 2024-08-16,
  https://haiku-os.org/blog/zardshard/2024-08-16_gsoc_2024_porting_webkit2_final_report, primary.
- [O10] Haiku forum, "WebKit2 status", June 2025 to February 2026, https://discuss.haiku-os.org/t/webkit2-status/16901,
  primary (forum).
- [O11] Haiku forum, "My progress at porting Ladybird", August 2023,
  https://discuss.haiku-os.org/t/my-progress-at-porting-ladybird-the-serenityos-browser/13886, primary (forum).
- [O12] Fuchsia, Component framework introduction, https://fuchsia.dev/fuchsia-src/concepts/components/v2/introduction,
  primary.
- [O13] Chromium, `fuchsia_web/README.md`, https://chromium.googlesource.com/chromium/src/+/HEAD/fuchsia_web/README.md,
  primary.
- [O14] Slashdot (citing 9to5Google), "Google ends Chrome browser effort on Fuchsia", 2024-01-15,
  https://tech.slashdot.org/story/24/01/15/2350230, secondary.
- [O15] Genode 22.08 release notes, https://genode.org/documentation/release-notes/22.08, primary.
- [O16] Genode 25.02 release notes, https://genode.org/documentation/release-notes/25.02, primary.
- [O17] Genode 20.08 release notes, https://genode.org/documentation/release-notes/20.08, primary.
- [O19] Genode 26.05 release notes, https://genode.org/documentation/release-notes/26.05, primary.
- [O21] N. Feske, Genode 20.08 announcement, https://lists.genode.org/pipermail/users/2020-August/007224.html, primary.
- [O22] WebKit Trac, PlayStation WebKit Port Update (2019), https://trac.webkit.org/wiki/PlayStationWebKitPortUpdate2019,
  primary.
- [O23] SerenityOS, `Documentation/Browser/ProcessArchitecture.md`,
  https://raw.githubusercontent.com/SerenityOS/serenity/master/Documentation/Browser/ProcessArchitecture.md, primary
  (fetched by the main session).

### Sources: libraries and interfaces (P)

- [P1] ICU Data Build Tool, https://unicode-org.github.io/icu/userguide/icu_data/buildtool.html, primary.
- [P3] ICU Data, https://unicode-org.github.io/icu/userguide/icu_data/, primary. Packaging modes.
- [P4] ICU releases, https://github.com/unicode-org/icu/releases/latest, primary. 79.1.
- [P6] ICU `LICENSE`, https://raw.githubusercontent.com/unicode-org/icu/main/LICENSE, primary.
- [P7] FreeType License, https://freetype.org/license.html, primary.
- [P8] FreeType home page, https://freetype.org/index.html, primary. 2.15.0.
- [P10] HarfBuzz documentation, https://harfbuzz.github.io/, primary.
- [P11] HarfBuzz `COPYING`, https://raw.githubusercontent.com/harfbuzz/harfbuzz/main/COPYING, primary.
- [P12] HarfBuzz `NEWS`, https://github.com/harfbuzz/harfbuzz/blob/main/NEWS, primary.
- [P13] Fontconfig developer documentation, https://fontconfig.pages.freedesktop.org/fontconfig/fontconfig-devel/,
  primary.
- [P14] Fontconfig user documentation, https://fontconfig.pages.freedesktop.org/fontconfig/fontconfig-user.html, primary.
- [P16] Skia `SkFontMgr_custom.cpp`,
  https://skia.googlesource.com/skia/+/refs/heads/main/src/ports/SkFontMgr_custom.cpp?format=TEXT, primary.
- [P17] Skia `src/ports` listing, https://skia.googlesource.com/skia/+/refs/heads/main/src/ports/?format=TEXT, primary.
- [P18] Skia `SkFontMgr_custom_directory.cpp`,
  https://skia.googlesource.com/skia/+/refs/heads/main/src/ports/SkFontMgr_custom_directory.cpp?format=TEXT, primary.
- [P19] Skia `SkFontMgr_custom_embedded.cpp`,
  https://skia.googlesource.com/skia/+/refs/heads/main/src/ports/SkFontMgr_custom_embedded.cpp?format=TEXT, primary.
- [P20] Skia `SkFontMgr_custom_empty.cpp`,
  https://skia.googlesource.com/skia/+/refs/heads/main/src/ports/SkFontMgr_custom_empty.cpp?format=TEXT, primary.
- [P21] POSIX.1-2024, `poll`, https://pubs.opengroup.org/onlinepubs/9799919799/functions/poll.html, primary.
- [P22] POSIX.1-2024, `socketpair`, https://pubs.opengroup.org/onlinepubs/9799919799/functions/socketpair.html, primary.
- [P23] POSIX.1-2024, `posix_spawn`, https://pubs.opengroup.org/onlinepubs/9799919799/functions/posix_spawn.html,
  primary.
- [P24] POSIX.1-2024, `shm_open`, https://pubs.opengroup.org/onlinepubs/9799919799/functions/shm_open.html, primary.
- [P25] musl, `src/process/posix_spawn.c`, https://git.musl-libc.org/cgit/musl/tree/src/process/posix_spawn.c, primary.
- [P26] musl, `posix_spawn_file_actions_addchdir.c`,
  https://git.musl-libc.org/cgit/musl/tree/src/process/posix_spawn_file_actions_addchdir.c, primary.
- [P27] GLib `gmain.c`, https://gitlab.gnome.org/GNOME/glib/-/raw/main/glib/gmain.c, primary.
- [P28] GLib `gpoll.c`, https://gitlab.gnome.org/GNOME/glib/-/raw/main/glib/gpoll.c, primary.
- [P29] GLib, The Main Event Loop, https://docs.gtk.org/glib/main-loop.html, primary.
- [P30] curl, `curl_multi_socket_action`, https://curl.se/libcurl/c/curl_multi_socket_action.html, primary.
- [P31] curl, `curl_multi_poll`, https://curl.se/libcurl/c/curl_multi_poll.html, primary.
- [P32] Linux man-pages, epoll(7), https://man7.org/linux/man-pages/man7/epoll.7.html, primary.
- [P33] FreeBSD, kqueue(2), https://man.freebsd.org/cgi/man.cgi?query=kqueue&sektion=2, primary.
- [P34] Linux man-pages, unix(7), https://man7.org/linux/man-pages/man7/unix.7.html, primary.
- [P35] FreeBSD, unix(4), https://man.freebsd.org/cgi/man.cgi?query=unix&sektion=4, primary.
- [P36] Microsoft, "AF_UNIX comes to Windows", https://devblogs.microsoft.com/commandline/af_unix-comes-to-windows/,
  primary (vendor).
- [P37] Fuchsia, Handles, https://fuchsia.dev/fuchsia-src/concepts/kernel/handles, primary.
- [P38] Fuchsia, `zx_object_wait_many`, https://fuchsia.dev/reference/syscalls/object_wait_many, primary.
- [P39] Fuchsia, `zx_port_create`, https://fuchsia.dev/reference/syscalls/port_create, primary.
- [P40] Fuchsia, `zx_vmo_create`, https://fuchsia.dev/reference/syscalls/vmo_create, primary.
- [P41] Fuchsia, `zx_vmar_map`, https://fuchsia.dev/reference/syscalls/vmar_map, primary.
- [P42] Linux man-pages, memfd_create(2), https://man7.org/linux/man-pages/man2/memfd_create.2.html, primary.
- [P44] FreeBSD 14.0, shm_open(2),
  https://man.freebsd.org/cgi/man.cgi?query=shm_open&apropos=0&sektion=0&manpath=FreeBSD+14.0-RELEASE&arch=default&format=html,
  primary.
- [P45] FreeBSD 14.0, memfd_create(2),
  https://man.freebsd.org/cgi/man.cgi?query=memfd_create&manpath=FreeBSD+14.0-RELEASE&format=html, primary.
- [P47] Linux man-pages, mmap(2), https://man7.org/linux/man-pages/man2/mmap.2.html, primary.
- [P48] Microsoft, Creating Named Shared Memory,
  https://learn.microsoft.com/en-us/windows/win32/memory/creating-named-shared-memory, primary.
- [P49] Microsoft, WaitForMultipleObjects,
  https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitformultipleobjects, primary.
- [P50] Microsoft, I/O Completion Ports, https://learn.microsoft.com/en-us/windows/win32/fileio/i-o-completion-ports,
  primary.
- [P51] Linux kernel, Control Group v2 (memory ownership), https://docs.kernel.org/admin-guide/cgroup-v2.html, primary.
- [P52] Linux kernel, sysctl vm (`max_map_count`), https://www.kernel.org/doc/html/latest/admin-guide/sysctl/vm.html,
  primary.
- [P53] FreeBSD, capsicum(4), https://man.freebsd.org/cgi/man.cgi?query=capsicum&sektion=4, primary.
- [P54] V8, Pointer compression in V8, https://v8.dev/blog/pointer-compression, primary.
- [P56] Project Zero, JITSploitation II, 2020-09, https://projectzero.google/2020/09/jitsploitation-two.html, primary.

### Measurements (M)

- [M] The measuring subagent's downloads and counts of 2026-11-14 described in [Measurements](#measurements): the WPE
  WebKit 2.54.2 tarball from https://wpewebkit.org/releases/wpewebkit-2.54.2.tar.xz, Ladybird at commit
  `8ab2c0bb9c4376f50bbcb7ea6f1b7717b089ff6e` from https://github.com/LadybirdBrowser/ladybird, ICU4C 79.1, FreeType
  2.15.0, HarfBuzz 15.1.0 and fontconfig 2.18.3 from their release pages.

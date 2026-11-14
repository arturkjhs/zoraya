# Plan: big programs and a modern browser engine

The implementation parts that follow from proposed ADR-0410 to ADR-0421 (TASK-33), if the architect accepts them as
recommended. Foundations first, each useful on its own; then the engine's libraries; then the engine. Sizes are
estimates of new or changed lines (code and tests, not counting ported sources or patches' context). Every part ends
with its own green candidate run, as `CLAUDE.md` asks; the "visible result" is what the person types and sees.

## How the parts fit on one 16 GB machine

- **Lane K (kernel and `libposix`).** F1 to F8 change `crates/zoraya-posix`, `crates/zoraya-abi` and the kernel; they
  conflict with each other in the same files, so they run one after another in one worktree. Kernel and `unsafe` code
  is reviewed by `opus-review` (`CLAUDE.md`).
- **Lane P (ports).** D1 to D6 build third-party libraries with the CMake toolchain file on the host and run their own
  small test programs in the guest; they touch `ports/` only, so they run beside lane K in a second worktree. A port's
  in-guest test that needs an F part waits for it.
- **The engine (E1 to E6)** needs both lanes. Its build is the heavy one (1 to 2 hours, 10 to 20 GB of build tree and
  about 2 GB of RAM per compile job, `(estimate)`, ADR-0418 item 5): while it builds with `-j6`, no other compile job
  runs on the machine, and at most two worktrees with builds exist (`CLAUDE.md`'s disk rule).

## Foundations (lane K)

| Part | What | ADR | Size `(estimate)` | Visible result | Done when | Needs |
|---|---|---|---|---|---|---|
| F1 | `poll`, `ppoll`, `select` exported over the one port wait; `eventfd` within a process | ADR-0342 item 5, ADR-0414 | 700 | `python3 -c "import selectors; print(selectors.DefaultSelector())"` prints a `PollSelector`; curl and wget lose `zshim.c`'s `select` | INV-0342-2, INV-0414-1 to -3 checked; `zshim.c`'s `select` removed | none |
| F2 | `AF_UNIX`: `socketpair`, `sendmsg`/`recvmsg` with `SCM_RIGHTS` as handle transfer | ADR-0412 items 1 to 4, 6, 7 | 1,000 | `python3 -c "import asyncio; print(asyncio.run(asyncio.sleep(0, 'ok')))"` prints `ok` (asyncio's self-pipe is a `socketpair`) | INV-0412-1, -2; CPython's `test_socket` Unix-pair cases and `test_asyncio` base-event cases in `tests/python/` pass | F1 |
| F3 | Memory descriptors: `memfd_create`, `shm_open(SHM_ANON)`, `MAP_SHARED`, seals | ADR-0413 | 500 | a C test passes a 4 MiB memfd to a child, which reads what the parent wrote; `top` charges it to the parent | INV-0413-1 to -5 | F2 |
| F4 | `posix_spawn` grant list, `posix_spawnp`, `addopen` | ADR-0411 item 1 | 400 | a C test spawns itself with an empty table and `POSIX_SPAWN_GRANT_NET` and prints what it has; `python3 -c "import subprocess; subprocess.run(['true'])"` uses `posix_spawn` | INV-0411-1 | none |
| F5 | 4096 mappings per process (kernel structure, `MAPPING_SLOTS` from `zoraya-abi`) | ADR-0415 item 1 | 450 (kernel) | in a 2 GiB guest, `python3 -c "x=[bytearray(100) for _ in range(5_000_000)]"` finishes (ended at 256 mappings before) | INV-0415-1; ADR-0320's mapping measurement repeated | F8 |
| F6 | Read-only segments shared between processes of one image | ADR-0415 item 2 | 300 (kernel) | two `python3` processes: `top` shows the image's text once, not per process | INV-0415-2 | none |
| F7 | Socket files in a program's own directory or its line's `/tmp`; TCP `bind`/`listen`/`accept` over `netd` | ADR-0412 item 5, ADR-0225 item 5 | 900 (`libposix`, `fsd`, `tmpd`) | `python3 -m http.server` serves a file to `curl` in the same room (how a person grants a listener is ADR-0225 item 5's and not decided here); a tmux port can follow | INV-0412-3 | F2 |
| F8 | A 2 GiB guest: the frame allocator tested at 2 GiB, `--mem 2G` in the browser's steps | ADR-0415 item 5 | 100 | `make run-browser` boots a 2 GiB guest; `top --session` shows its memory | the frame allocator's tests at 2 GiB under HVF and TCG | none |

F2, F3 and F7 also unblock CPython's `multiprocessing` with `spawn` (partly: its named shared memory stays refused,
ADR-0413 item 1) and a later tmux port.

## The engine's libraries (lane P)

| Part | What | ADR | Size `(estimate)` | Visible result | Done when | Needs |
|---|---|---|---|---|---|---|
| D1 | Host tools in the CI image and on the Mac: CMake 3.20+, Meson, Ninja, Ruby, gperf, unifdef | ADR-0418 item 2 | 50 | `make ci-build` in the new image | the CI image builds; `check-ci-jobs` green | none |
| D2 | ICU 79 static with all data | ADR-0417 items 1, 2 | 300 (port, test) | a C++ test prints `субота, 14 листопада 2026 р.` from `icu::DateFormat` | the test and ICU's `intltest` subset for `uk`, `ru`, `cs`, `en` pass in the guest | D1 |
| D3 | FreeType, HarfBuzz, fontconfig with the fixed configuration | ADR-0416 | 400 (ports, patch, tests) | a test shapes `Київ` with Inter and prints the glyph ids and advances; `fc-match serif` prints `Inter` | INV-0416-1; HarfBuzz's shaping tests for Latin and Cyrillic pass | D1 |
| D4 | GLib and GIO (with libffi, PCRE2), static, `HAVE_EVENTFD` on | ADR-0414, ADR-0421 | 500 (port, patches) | GLib's `mainloop`, `gsocket`-Unix and `gsubprocess` tests pass in the guest | a named subset of GLib's tests in `tests/glib/` | F1 to F4, D1 |
| D5 | libsoup 3, glib-networking (OpenSSL only, static, registered at start), libpsl, nghttp2 | ADR-0420 | 600 | `soupget https://example.org/` (a 60-line test program) prints the page under the `web` grant; an expired certificate is refused | INV-0420-1 against `tools/test-httpd` | D4 |
| D6 | The rest of WPE's required set: libjpeg-turbo, libwebp, libxml2, libxslt, woff2 and brotli, lcms2, hyphen, libtasn1, libepoxy, libxkbcommon | ADR-0410 item 2 | 600 (ports and their tests) | each library's own test program passes in the guest | the ports' tests in `make test-ports-browser` | D1 |

D2, D3 and D6 are independent of each other and of lane K. D4 waits for F1 to F4 to pass its tests (it builds
before).

## The engine

| Part | What | ADR | Size `(estimate)` | Visible result | Done when | Needs |
|---|---|---|---|---|---|---|
| E1 | WPE 2.54 built for Linux AArch64 in CI with Zoraya's options (JIT off, headless, CPU Skia, system malloc), to settle the LLInt-or-C-loop question and CPU-only rendering before Zoraya | ADR-0410 items 2, 3 | 200 (CI job, options file) | a CI job's artefact: a PNG of a test page rendered by the headless backend | the PNG matches a reference; the options file is the one E2 uses; the build's time and peak memory are recorded | D1 |
| E2 | WPE for Zoraya, headless: the launcher with grants, the three programs, a test program that loads a local page with JavaScript and writes its pixels | ADR-0410, ADR-0411 items 2 to 4, ADR-0415 item 4 | 1,500 (launcher, options, test, patches) | in the guest, a page's script computes and the test prints the pixel checksum the host build gave | INV-0410-1, -2, INV-0411-2, -3, INV-0417-1; the record of build time, program sizes, mappings and memory per process | F1 to F6, F8, D2 to D6 |
| E3 | The Zoraya WPEPlatform backend and the browser's chrome | ADR-0419 items 1 to 4, 7 | 3,000 | `browser https://...` opens a window with a toolbar; typing a URL loads it; the wheel scrolls; scale 2 is sharp | INV-0419-3; GUI scripts at scale 1 and 2 | E2 |
| E4 | The network process under the `web` grant, cookies in its own directory | ADR-0411 items 2, 5, ADR-0420 | 600 | the TASK-27 test server's pages load with JavaScript; `top home` shows three processes; `grants browser` prints the network line | INV-0420-1, -2; the NetSurf refusal tests repeated for the browser | E3, D5 |
| E5 | Clipboard and chooser for POSIX windows; downloads and uploads | ADR-0419 items 5, 6 | 900 (`zoraya-surface`, `libc.a`, panel slot, browser) | copy from a page and paste in the editor; a download asks the chooser and shows a pill | INV-0419-1, -2 | E3 |
| E6 | ICU's filter; speed and memory measured on real pages | ADR-0417 item 3 | 300 | `top` shows the browser's memory with the filtered data; a page-load list's times are recorded | INV-0417-2; the ADRs' open questions answered with numbers | E4 |

E5 can run beside E4 (different files: the panel and `zoraya-surface` against the network process). E6 last.

## Order in one line

F1 → F2 → F3 → F4, with F8 → F5 and F6 when the kernel lane is free; D1, then D2, D3, D6 beside lane K; D4 after F4; D5;
E1 as soon as D1 lands (it needs nothing of Zoraya); E2 when lane K and D2 to D6 are done; E3; E4 and E5; E6.

## Measurements the parts must record

The survey's estimates are replaced by numbers in these parts: E1 (build time and peak memory on Linux AArch64 CI), E2
(build time, disk and peak memory on the 16 GB M4 with `-j6`; text and data of each program; mappings in use; memory of
each process with one simple page; idle loop iteration time, ADR-0414 item 4), E3 (frame copy time at 2560×1600,
ADR-0419), E6 (page-load times, Speedometer 2 if it runs, the filtered ICU size).

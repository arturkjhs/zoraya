# Review notes: TASK-33 survey, ADR-0410 to ADR-0421, plan

Independent review (ADR-0001 rule 7, TASK-33 "Review") of `adr/browser-engine` at `2365e17a` (commits `1feeb3e7`,
`2d9c2917`, `2365e17a`). The reviewer did not write these documents. Sources were fetched in this session with `curl`
(WebKit `main`, the WPE 2.54.2 tarball, Ladybird at `8ab2c0bb9c4376f50bbcb7ea6f1b7717b089ff6e`, GLib `main`, ICU,
Genode, Haiku); the repository was read at the files named. Nothing was built; no `make`, `cargo` or QEMU was run.
`./target/debug/zoraya-adr-check docs` was run.

## Summary

**5 high, 11 medium, 12 low.**

The survey's citations mostly hold: of about 40 sources fetched, the claims checked match the text, with the
exceptions below. The template is followed: every ADR has the exact headings, three or more options with a Linux
baseline and a non-Unix precedent, "Effect of the vision", examples, and checkable invariants. Every ADR's Evidence
line matches the IDs its body cites (checked by script). The checker reports only the 12 expected `INV-0001-5` errors.

The recommendation of WPE (ADR-0410) rests partly on a false fact. Ladybird has a build switch for its JIT
(`ENABLE_JS_JIT`), and it is **off by default on any system other than Linux, macOS and Windows**: on Zoraya, LibJS
would run in its interpreter without configuration (H1). Three other facts that cut against WPE are not weighed
(H5, M2, M4). ADR-0415's sharing of read-only segments does not give what ADR-0415 and ADR-0417 claim for WPE's three
different programs (H2). ADR-0415's 4096 mappings cannot be reached while each mapping holds a handle in a 1024-slot
table (H3). ADR-0411 changes three accepted ADRs without saying so (H4). The engine choice should be argued again once
H1, H5, M2 and M4 are corrected. After that WPE may still be the better choice, but the argument as written does not
show it.

## Findings

### High

1. **(high) ADR-0410 Option C and the revisit triggers; survey "The engines compared" row "JIT-free mode" (line 524).**
   *What:* The documents say Ladybird has "a JIT since October 2026 and no switch found to build without it
   (driver 1)", and they set "offers a build without its JIT" as a revisit trigger. Both are false at the measured
   commit. *Evidence:* `Meta/CMake/cmake_options.cmake:12-21` at `8ab2c0bb` says: "The optimizing JIT for JavaScript
   generates code for x86-64 and AArch64 on Linux and macOS, and for x86-64 on Windows. Elsewhere, LibJS runs
   JavaScript in its interpreter alone." It defines `option(ENABLE_JS_JIT ... ${js_jit_default})`, and the default is
   OFF for anything other than (Linux and not Android) or Apple on x86-64/AArch64, or Windows on x86-64.
   `Libraries/LibJS/CMakeLists.txt:14-17` adds the crate feature `jit` only `if (ENABLE_JS_JIT)`. The survey looked
   for `LibJS/JIT/CMakeLists.txt` (404) and then reported the absence as a finding. *Effect:* Driver 1 (W^X) no longer
   tells the two engines apart. The first half of the Ladybird revisit trigger has already fired. *Fix:* Correct the
   survey row, ADR-0410 Context line 19, Option C evidence against, and the revisit trigger. Check whether the new
   WebAssembly engine's JIT has its own switch (not read). Re-argue the recommendation without driver 1 as a
   difference.

2. **(high) ADR-0415 item 2, its Consequences ("Helpers of one program share their code"), the `top` example; ADR-0417
   Context lines 19-20 and Option B; plan F6.** *What:* Sharing read-only segments of one image object does not share
   anything between `browser`, `WPEWebProcess` and `WPENetworkProcess`. They are three different static executables
   (ADR-0410 item 4), so they are three images. Even one program spawned twice shares nothing today, because
   `libposix`'s `execve` and `posix_spawn` read the file into a fresh memory object and call `image_create` per spawn
   (ADR-0108 Recommendation item 2). Images are reused only through a cache: ADR-0212's broker cache, which is
   accepted, not built ("until the image cache task"), and not cited by ADR-0415. ADR-0212's decision also relies on
   the per-spawn copy ("every child still costs its segments' copy and frames"), and ADR-0415 changes that without
   listing ADR-0212. A cached image shared across rooms would hold frames charged to the broker's job, not to the
   rooms that use them: an ADR-0261 item 8 accounting question. *Effect:* ADR-0417 says ICU's 33.6 MB is "shared
   between the processes" by ADR-0415 item 2, so it chooses Option B and keeps Option C only as a fallback. With three
   separate images, the data is held three times. F6's visible result ("two `python3` processes: `top` shows the
   image's text once") does not happen without a cache. *Fix:* Either build WPE as one multi-call executable (dispatch
   on `argv[0]` or a flag; the launcher spawns the same path) and say how one image object reaches every spawn (the
   ADR-0212 cache, or the UI process keeping an image handle and spawning from it through a Zoraya extension), or drop
   the sharing claim. List ADR-0212 and the accounting of cached images among the tensions. In ADR-0417, make Option C
   the recommendation unless the multi-call build is decided.

3. **(high) ADR-0415 item 1 and INV-0415-1, the Python example and plan F5.** *What:* 4096 mappings per process cannot
   be reached by a POSIX program, because every mapping also holds a handle and the handle table has 1024 slots
   (`crates/zoraya-abi/src/lib.rs:89`). The heap's backing keeps each step's memory object handle for the process's
   life (`crates/zoraya-posix/src/ffi.rs:571-586`: "the handle and mapping stay for the process's life"). `mmap`
   records `object.raw()` in its slot (`ffi.rs:829-831`). Thread stacks and two handles per thread (ADR-0300) take
   from the same 1024. The example `[bytearray(100) for _ in range(5_000_000)]` needs about 800 obmalloc arenas of
   1 MiB (ADR-0320: one mapping each), and with them about 800 handles. It ends at the handle table, not at "the
   room's or the machine's memory". *Fix:* Make the library close the object handle once mapped, recording the
   address for `munmap`. ADR-0108 item 1 says frames live while a mapping does, but confirm that for ADR-0051 objects
   and say so. Otherwise raise `MAX_HANDLES` in the same ADR. Change INV-0415-1's check to go through `malloc` and
   `mmap` in a POSIX program, not only an in-kernel mapping loop.

4. **(high) ADR-0411 item 2 and Context: unlisted changes to accepted ADRs at a security boundary.** *What:* (a)
   `WPENetworkProcess` gets `~/.local/share/browser/network` **read-write**. ADR-0341 item 7 (accepted) says
   "Children get it to read, never to write ... The child never holds a handle that can write, whatever its own
   runtime does". (b) The grant mask gives a POSIX child the network and random connections. ADR-0287 item 3 ("A
   child, spawned or forked, gets no network") and ADR-0310 item 3 (no random from other launchers) say otherwise.
   ADR-0411 cites both ADRs as today's facts and never says it changes them. *Fix:* Add a paragraph like ADR-0412's
   ("It changes ADR-0341 item 7, ADR-0287 item 3, ADR-0310 item 3") with the new rule: who may narrow and widen, and
   that write on an own-directory subtree is passed only through the grant list. Add an invariant that without the
   new attributes children still get read-only own directories and no network.

5. **(high) ADR-0410 Option B cost, ADR-0413 Context, ADR-0414 Context, the plan: WebKit's Linux code paths are not
   the ones Zoraya will take.** *What:* `tools/zcc` defines `__zoraya__` and refuses `-D__linux__` (`tools/zcc:13,42`,
   INV-0109-2). The paths the ADRs describe are guarded by `OS(LINUX)` or Linux headers:
   - 2.54.2 `IPCSemaphoreUnix.cpp:33-43` uses `eventfd` only `#if OS(LINUX)`. On `main` the Linux path is now a
     memfd and `SYS_futex`, a futex shared between processes, which ADR-0301 does not have.
   - `SharedMemoryUnix.cpp:83-87` calls `syscall(__NR_memfd_create, ...)` under `HAVE(LINUX_MEMFD_H)`. That is the raw
     `syscall()`, which the shim routes for eleven calls (ADR-0141).
   - Without it, the code uses `shm_open(SHM_ANON)` if `sys/mman.h` defines `SHM_ANON`
     (`OptionsCommon.cmake:336`), else a named `shm_open` and `shm_unlink` (`:104-116`), which ADR-0413 item 1
     refuses (`EACCES`, `ENOENT`).

   WTF has no OS for Zoraya (`PlatformOS.h`). Every `OS(LINUX)` branch in WTF and WebKit (memory footprint, threads,
   `/proc`, semaphores) has to be given a Zoraya path or the generic Unix one. The size of that work is in no ADR, no
   cost line and no plan part. Ladybird's `Documentation/Porting.md` (fetched) describes this case, porting to a new
   POSIX system, and names Haiku, FreeBSD and OpenBSD as working. *Fix:* In ADR-0413, require `SHM_ANON` in the
   sysroot's `sys/mman.h` and state which WebKit branch is taken. In ADR-0414, restate what WebKit actually calls with
   `OS(LINUX)` false. In ADR-0410, add the WTF/WebKit OS layer to Option B's cost and evidence against. In the plan,
   give it a part of its own before E2.

### Medium

1. **(medium) ADR-0418 Option B, ADR-0410 drivers 2 and Option B: "clang 18 is still supported".** *What:* The
   support is for clang 18 *with libstdc++*. The workaround the survey quotes exists because "libstdc++'s
   `<expected>` is guarded behind `__cpp_concepts >= 202002`", and it adds `-D__cpp_concepts=202002` to every C++
   compile (2.54.2 `WebKitCompilerFlags.cmake:244-253`, the same on `main`). Zoraya's combination is clang 18 with
   libc++ 20, which no fetched file shows WebKit testing on Linux (the survey says so itself). A global redefinition
   of `__cpp_concepts` under libc++ is untested. *Fix:* Call it unverified in ADR-0418's evidence. Make E1 build with
   clang 18 and libc++ 20.1.8, not the distribution's libstdc++. Add the libc++ question to the open questions.

2. **(medium) ADR-0410, ADR-0419 and the plan's D4 and D6: libepoxy and GModule need `dlopen`.**
   *What:* `OptionsWPE.cmake` (2.54.2 line 16, `main` line 18) has `find_package(Epoxy 1.5.4 REQUIRED)` and GLib with
   `COMPONENTS ... Module`. libepoxy resolves GL and EGL by `dlopen` and `dlsym`, and GModule wraps `dlopen`.
   `dlopen` is on ADR-0342 item 12's deny list. No ADR mentions either, and plan D6 lists libepoxy as an ordinary
   port. *Fix:* Decide this in an ADR: patch WPE to build without epoxy when Skia is CPU-only, or give libepoxy a
   static provider that reports no GL. Say that GModule is built with no loader. If any of these needs a `dlopen`
   symbol, that changes ADR-0342 item 12 and must say so.

3. **(medium) ADR-0421 item 3, survey table line 488, plan D6: libgcrypt and libtasn1 are required by the pinned
   version.** *What:* WPE 2.54.2's `OptionsWPE.cmake:17,19` has `find_package(LibGcrypt 1.7.0 REQUIRED)` and
   `find_package(Tasn1 REQUIRED)`. OpenSSL replaces libgcrypt only on `main`. ADR-0421 lists libgcrypt "only if the
   pinned WPE needs it", and the survey labels it "if not OpenSSL". Plan D6 leaves out libgcrypt and its LGPL
   dependency libgpg-error. *Fix:* List libgcrypt and libgpg-error (LGPL-2.1) and libtasn1 as required for 2.54.x.
   Add them to D6. Note in ADR-0420 that 2.54.x brings a second crypto library beside OpenSSL (driver 3).

4. **(medium) ADR-0410 Options B and C: the comparison is not like for like.** *What:*
   - **Dependency counts.** Ladybird's "49 dependencies" counts `vcpkg.json` entries, which include platform
     duplicates (skia ×4, qtbase ×3, angle ×2, harfbuzz ×2) and Windows-only `dirent`, `mman` and `pthread`. That is
     about 38 distinct names, fewer for a platform without Qt, Vulkan or ANGLE. WPE's "about 20" is counted with
     options off and leaves out the bundled Skia and ANGLE.
   - **Network stack.** Ladybird's network process uses curl and OpenSSL, which are already ported and tested here
     (ADR-0310, ADR-0312). WPE needs GLib, GIO, libsoup and glib-networking: four new ports, all LGPL (ADR-0421),
     plus ADR-0420's open question about module registration. Option C's evidence says "no GLib" but does not count
     what it saves.
   - **Headless mode.** Ladybird has `--headless` (`ProcessArchitecture.md:47`), so "no Linux-free front end to copy"
     overstates; the Android front end is 1,319 lines.
   - **The Haiku precedent cuts against B.** It is cited for B, but the GSoC 2024 report (fetched) says that after two
     GSoC projects (2019, 2024) WebKit2 on Haiku had no keyboard, no copy and paste, and crashes on resize and
     history. It needed 20 to 25 GB, linking took 30 minutes with `ld`, and the earlier work "bit-rotted". Upstream
     WebKit removed Haiku's first port. A Zoraya launcher, IPC deviations and an OS layer would be carried downstream
     the same way, so driver 3 ("a port that upstream carries") does not favour WPE as written.

   *Fix:* Recount both engines on one rule (required libraries for a CPU-only, single-front-end build, bundled ones
   included). Credit Ladybird with the existing curl and OpenSSL ports. Weigh the Haiku facts in Option B's cost and
   in driver 3.

5. **(medium) ADR-0412 items 2 and 6: a byte stream over a 64-message queue.** *What:* Each `send` on `SOCK_STREAM`
   becomes at least one channel message, and a direction holds 64 messages (`crates/zoraya-channel/src/lib.rs:26`).
   So a writer blocks or gets `EAGAIN` after 64 small writes, whatever their size: 64 bytes in the worst case. Linux
   buffers about 200 KB. tmux's imsg and asyncio's self-pipe write small records. Item 6 also makes `fork` with an open
   Unix socket `ENOTSUP`. Every asyncio program holds a `socketpair` (its self-pipe), so `os.fork` and fork-based
   `multiprocessing` fail in any asyncio program. ADR-0225 item 10 decided crossed byte streams, which are duplicable
   (ADR-0101) and survive `fork`. ADR-0412 says it changes item 10 but not that it loses these properties. *Fix:* List
   both consequences. Consider keeping ADR-0225's crossed byte streams for `SOCK_STREAM` data, with a channel beside
   them only for ancillary data. Otherwise measure the 64-message limit against tmux and asyncio before accepting.

6. **(medium) ADR-0412 item 5: socket objects in a file service.** *What:* "The file service makes a socket object
   there (`S_IFSOCK`) holding a listener channel" in a program's own directory. That directory lives on a persistent
   volume served by `fsd` (ADR-0341). A directory entry holding a live channel is a new object kind for the filesystem
   protocol and its metadata (ADR-0013, ADR-0014, ADR-0055). The ADR does not say what a remount or reboot leaves
   (a stale socket entry, as on Unix?), or how `ls` and `stat` show it. None of these ADRs is listed as changed.
   *Fix:* Say whether the object is in memory only (never on disk), what `readdir` shows, and list the protocol
   change.

7. **(medium) ADR-0413 items 1 to 3; ADR-0410 item 3.** *What:* Today `memfd_create` returns a `tmpd` file that grows
   by `write` (`crates/zoraya-posix/src/ffi.rs:2315`, `process.rs:2094`). Under ADR-0413 it is a descriptor of size 0
   until `ftruncate`, and its size is fixed once mapped or sent. The ADR does not say what `write` does on a size-0
   descriptor or past its end. Programs that use `memfd_create` as an anonymous growing file would regress. ADR-0413
   also changes ADR-0103 item 12 (`MAP_SHARED` is `ENODEV`) without saying so. ADR-0410 item 3 says `libposix`
   "keeps refusing" `PROT_EXEC`, but today `map()` ignores `prot` altogether (`ffi.rs:815-831`), so `PROT_EXEC`
   succeeds with a read-write mapping; `EACCES` would be new behaviour. *Fix:* Define `write` and `pwrite` past the
   size (`EFBIG` or growth while unmapped and unsent). List ADR-0103 item 12. Correct ADR-0410 item 3.

8. **(medium) ADR-0411 item 5: `grants` would print a claim nothing trusted enforces.** *What:* The `helper NAME
   GRANT` key is "printed but not enforced". Only the browser's own launcher, which is untrusted program code, keeps
   the network out of the Web process. A compromised UI process can pass `POSIX_SPAWN_GRANT_NET` to `WPEWebProcess`,
   and `grants browser` will still say "(network process only)". The pills and `grants` are trusted output
   (INV-0172-1, INV-0172-4). *Fix:* Either do not print the annotation, or enforce it: the shell records which helper
   name may receive which grant, and `libposix`'s spawn refuses anything else. That is part of Option C, and it
   should be costed as a variant.

9. **(medium) ADR-0420: environment and checkability.** *What:* GIO and glib-networking read `GIO_USE_TLS`,
   `GIO_EXTRA_MODULES` and `GIO_MODULE_DIR`, and OpenSSL reads `SSL_CERT_FILE` and `SSL_CERT_DIR`. INV-0228-1 and
   ADR-0310 item 6 say nothing from the environment decides what a program trusts. ADR-0420 handles only `SOUP_DEBUG`.
   INV-0420-2's check ("reads `netd`'s connection list and asserts every connection's process") has nothing to read:
   `netd` keeps accounts and connections, not the processes that own them (`crates/zoraya-net/src/`). The UI process
   also holds the `web` grant (ADR-0411 item 2), so "only `WPENetworkProcess` opens TCP connections" depends on
   WebKit's UI code, not on a boundary. *Fix:* Add the environment variables to item 2's patch, and an invariant
   with ADR-0310's style of test. Either give the UI process no network (pass a share to the helper at start and drop
   its own), or rewrite INV-0420-2 as an in-guest test that the UI process's own `connect` is refused.

10. **(medium) Plan: E1 and the sizes of the parts.** *What:* E1 is "WPE 2.54 built for Linux AArch64 in CI". The
    workflows run only on `ubuntu-24.04` (x86-64), with job limits of 60 minutes at most
    (`.github/workflows/*.yml`), while the build is estimated at 1 to 2 hours on the M4. On Ubuntu, clang 18 builds
    against libstdc++, so E1 would not settle M1. The parts are also not comparable in size: E2 ("1,500 lines")
    contains the first build and run of 4.4 M lines on a new OS, the WTF/WebKit OS layer (H5), GLib under ADR-0315
    waits and the launcher, while F8 is 100 lines. *Fix:* Name the runner (an ARM64 runner or a cross build) and its
    time limit, or move E1 to the M4. Split E2 into an OS-layer part, a "JavaScriptCore runs a script in the guest"
    part, and the headless engine.

11. **(medium) ADR-0417 Option C, survey line 295: `udata_setCommonData` is in a fetched page.** *What:* The survey
    and ADR-0417 call "the API for supplying data by pointer" `[unverified]: not in a fetched page`. The ICU Data page
    the survey cites as [P3] (fetched in this review) says: "If the application has called the function
    `udata_setCommonData()`, use the data that was provided." Option C is weakened by a claim that is false, and with
    H2 it is the only option that keeps one copy in memory. *Fix:* Cite [P3]. Re-weigh Option C against Option B.

### Low

1. **(low) Survey "Zoraya today" line 58; ADR-0415 Context.** Both say "ADR-0320 measured" CPython ending at about
   250 MiB. ADR-0320 (line 319) says that figure is "from the count of slots; not measured". The measured fact is
   recursion ending at about 33,000 frames. Also, the 250 MiB is from obmalloc arenas mapped by `mmap`, not from the
   heap's `GROW_SIZE` steps, which the survey's sentence gives as the cause.

2. **(low) ADR-0415, the tensions it lists.** The 256 limit is ADR-0051's "Limit" item (`[assumption]`), not "the
   number in ADR-0300's table limit". If item 3 (reservations) is built, it also changes ADR-0100 item 6
   (`mprotect` is `ENOSYS`, "no permission changes on mappings") and adds a reservation object to ADR-0051.
   Neither is listed.

3. **(low) Postmortem anchors cited for claims they do not contain.** `alternatives-postmortem.md#haiku--beos` says
   nothing about WebKit (ADR-0410 driver 3, ADR-0412 Option B). `#genode` mentions only "Chromium and Qt ports", not
   Falkon (ADR-0410 Option A) or shared memory with `mmap` (ADR-0413 Context). The survey supports these claims;
   drop the postmortem anchors from them. "ADR-0108 item 6" (survey, ADR-0410 Context) is ADR-0108's decision
   driver 6, not a recommendation item.

4. **(low) Option analyses with unsourced claims (ADR-0001 rule 5).** These have no source and no `[outside evidence]`
   or `[assumption]` mark:
   - ADR-0411 Option D: "WebKit2 has no supported single-process mode in WPE".
   - ADR-0420 Option C: "WPE does not build with curl upstream" and "Windows ports" use curl.
   - ADR-0412 Option C: WebKit's "Darwin backend over Mach ports".
   - ADR-0414 Option C: "GLib's Windows path waits with `WaitForMultipleObjects`" (the survey cites only Windows'
     own page).

   Tag or source each.

5. **(low) INV-0418-1.** "Compiled by the one clang of the CI image (or the Mac's host clang)": the Mac's compiler is
   Apple clang 21 (ADR-0418 Context). The invariant permits two compilers, which contradicts driver 1, and a check
   against "the image's clang version" fails for Mac builds. Pick one statement.

6. **(low) INV-0410-1.** The second half ("no program of the engine's build contains code that calls `mmap` or
   `mprotect` with `PROT_EXEC`") is not what the named checks test: INV-0413-5 tests only `libposix`. Restate it as
   "every `PROT_EXEC` request is refused, and the engine's test step records zero such refusals", or add a check that
   scans for these calls.

7. **(low) INV-0414-3.** The deny list today is the prefix `epoll_` only
   (`tools/posix-surface-check/src/surface.rs:39-40`). The check needs `kqueue`, `timerfd_` and `signalfd` added;
   say so in the check.

8. **(low) ADR-0414 item 2.** The `eventfd` wake-up is free of races only if a waiting thread is entered in the
   thread table under the library lock before ADR-0315 releases it for the wait. State that ordering; INV-0414-2
   tests it but does not define it. There is no revisit trigger for WebKit's move of `IPC::Semaphore` to a futex in
   shared memory (`main`, `OS(LINUX)`; see H5).

9. **(low) ADR-0421 driver 3** cites CP-27 ("keep the POSIX compatibility layer's scope fixed and small") for "the
   POSIX layer carries what the chosen engine needs", which is the opposite of what CP-27 says.

10. **(low) ADR-0419 item 6.** An untrusted room's programs get no chooser channel (INV-0172-2), so the browser
    there has no downloads and no uploads. Say so in "Not offered" or in the examples.

11. **(low) Survey line 290.** "Every mode still links the stub data library" overstates [P3]: the stub is needed
    with `files` and `archive`; with `library` or `static`, the data library itself is linked.

12. **(low) Small inconsistencies.**
    - ADR-0410's example pills (`web`, `/system/fonts`, `~browser`) leave out `/system/tls`, which ADR-0411 item 5
      lists.
    - `rows-to-add.md` says it was generated "after the review's fixes" before any review.
    - ADR-0410 Option C says "no Linux-free front end on `master`", although an Android front end and `--headless`
      exist (M4).

## Checked and found sound

- **Fetched sources that match the claims as written:**
  - WebKit `main`:
    - `WebKitFeatures.cmake:94-131,243-381`: JIT and C-loop defaults, conflicts, `PRIVATE` options settable as
      advanced cache entries.
    - `JavaScriptCore/CMakeLists.txt:350-363`: offlineasm backend `ARM64`, `C_LOOP` override.
    - `PlatformEnable.h:930`: the regex JIT follows `ENABLE(JIT)`.
    - `ConnectionUnix.cpp:53-66,245,469-478`: `SOCK_SEQPACKET` except Darwin, 4096-byte inline, 254 attachments,
      `MSG_NOSIGNAL`, `poll(POLLOUT)`.
    - The WPEPlatform tutorial: two mandatory vfuncs, rendered and released, extension-point priorities, linked-in
      registration.
    - `WPEBufferSHM.h`: `ARGB8888` only.
    - `WPEClipboard.h`: synchronous reads.
    - `compiling.md`: always built from 2.56.
  - WPE 2.54.2 (tarball, SHA-256 `f3284e19...4f56`, 49,565,492 bytes as the survey says): the clang 18 FIXME, GCC
    12.2 (13.1 on `main`), `find_package` list, `HAVE_SHM_ANON` and `HAVE_TIMERFD` checks.
  - Ladybird at `8ab2c0bb`:
    - `rust-toolchain.toml` 1.98.0.
    - `find_compiler.py`: clang 19, GCC 14.
    - `TransportSocket.cpp:30,79,629-648`: `SOCK_STREAM` pair, 128 MiB, `anon_create`, `pread`.
    - `EventLoopImplementationUnix.cpp:162,264,270`: `pipe2`, `ppoll`, `poll`.
    - `ProcessArchitecture.md`: Hub, Compositor, sandboxing.
    - `Porting.md`: Unix sockets; Alpine, FreeBSD, OpenBSD, Haiku.
    - `vcpkg.json`: 49 entries.
  - GLib `gpoll.c` (`poll()` under `HAVE_POLL`, no `epoll` or `ppoll`), `gmain.c:66-67` (`eventfd` header),
    `gwakeup.c` (`eventfd`, else pipes).
  - ICU Data (five packaging modes) and Data Build Tool (27 MiB for 64, the part sizes).
  - Genode 20.08 (FreeBSD base, `mmap` shared memory, JIT cache maintenance, no multi-process or sandbox, footprint)
    and 22.08 ("show stopper", "extremely poor").
  - The Haiku GSoC 2024 report (Unix sockets, POSIX shared memory, `BLooper`, EGL crashes).
- **Repository facts:**
  - `MAX_HANDLES` (`zoraya-abi/src/lib.rs:89`), the channel limits (`zoraya-channel/src/lib.rs:22-28`),
    `MAX_SPAWN_HANDLES` (`spawn.rs:36`), `MAX_THREADS_SYSTEM` (`timer.rs:18`), `MAX_MAPPINGS` (`user.rs:33`),
    `MAPPING_SLOTS` (`ffi.rs:778`), `GROW_SIZE` (`zoraya-heap/src/lib.rs:25`) and `run-qemu.sh:95-96`, each at the
    line cited.
  - `mmap` returns `ENODEV` for shared or file mappings; `mprotect` is `ENOSYS`.
  - ADR-0225 items 8 to 10, ADR-0342 items 5, 11 and 12, ADR-0287 item 3, ADR-0310 item 3, ADR-0261 item 8,
    ADR-0051 rights and INV-0051-2, ADR-0108's copy at spawn and its reason, ADR-0170's 500 ms, ADR-0317's follow-ups
    and ADR-0326 driver 2: all as the documents state them.
- **Tensions that are listed correctly:** ADR-0412 (ADR-0225 item 10, ADR-0342 item 11), ADR-0415 (ADR-0108),
  ADR-0416 (ADR-0326 driver 2), ADR-0418 (ADR-0360 by its trigger).
- **Format:** template headings, three or more options with a Linux baseline and a non-Unix precedent in each ADR,
  "Effect of the vision" and examples in each, Evidence lines equal to the body's IDs, and the checker's output (12
  expected `INV-0001-5` errors, nothing else for these files).

## Dispositions (the main session, after the review)

Every finding was accepted; none was rejected. What changed:

- **H1** fixed. Ladybird's `ENABLE_JS_JIT` was fetched at the measured commit by the main session ([L56] in the
  survey) and is in the survey's Ladybird section and comparison table. ADR-0410 was re-argued: driver 1 no longer
  separates the engines; the recommendation stays WPE but says the choice is close and turns on the Rust `std` policy
  (a nightly feature or a patched compiler, against `docs/design.md`'s "no nightly features") and on release
  stability; Option C becomes the recommendation if the architect accepts a Rust `std` target (revisit trigger).
- **H2** fixed. ADR-0410 item 5: one multi-call `browser` executable; ADR-0415 item 2: `libposix` reuses the image of
  its own executable for its spawns, others share only through ADR-0212's cache (listed, its accounting left to it);
  ADR-0417: the data is in memory twice, said so, with `udata_setCommonData` (Option C) if the second copy matters;
  plan F6's visible result changed.
- **H3** fixed. ADR-0415 item 1: `libposix` closes a private mapping's object handle once mapped (a mapping holds its
  object, ADR-0051); INV-0415-1 now goes through `malloc` and `mmap` in a POSIX program; the Python example says
  where it ended.
- **H4** fixed. ADR-0411 lists its changes to ADR-0287 item 3, ADR-0310 item 3 and ADR-0341 item 7 and adds
  INV-0411-4 (no change without the new attributes).
- **H5** fixed. ADR-0410 Context and Option B's cost and evidence name the OS layer; ADR-0413 item 1 defines `SHM_ANON`
  in the sysroot and says which WebKit branch is taken; ADR-0414's Context says what `OS(LINUX)` false means; plan part
  E0 ports the OS layer.
- **M1** fixed in ADR-0410 and ADR-0418 (Option B's evidence against, open questions); plan E1 builds JavaScriptCore
  with Zoraya's clang and libc++.
- **M2** fixed: ADR-0410 item 3 (GModule without a loader; a failing `dlopen` shim local to the engine for libepoxy,
  stated as a stretch of ADR-0342 item 12's intent, with the alternative if refused); plan D4 and D6.
- **M3** fixed: survey table, ADR-0420 item 3, ADR-0421 item 3, plan D6.
- **M4** fixed: ADR-0410 Options B and C recounted (about 28 against about 38 distinct names), curl and OpenSSL
  credited to Ladybird, its Android front end and `--headless` named, the Haiku facts weighed against B in driver 3
  and Option B; the survey's comparison row and Haiku paragraph updated (the Haiku figures cite the review's reading
  of [O9]).
- **M5** fixed: ADR-0412 keeps ADR-0225's crossed byte streams for `SOCK_STREAM` data with an ancillary channel tagged
  by stream offset; `fork` keeps the byte streams; the 64-message limit now applies to sequenced packets only, and is
  stated; INV-0412-2 extended.
- **M6** fixed: ADR-0412 item 5 (a transient object in the service's memory, never on disk; what `readdir` and `stat`
  show; ADR-0055's protocol listed).
- **M7** fixed: ADR-0413 items 2 and 6 and the changes it lists (ADR-0103 item 12, today's `memfd_create`); ADR-0410
  item 4 says `PROT_EXEC` becomes refused.
- **M8** fixed: ADR-0411 item 5: `grants` claims nothing about which process holds a grant; the `helper` key is gone.
- **M9** fixed: ADR-0420 item 2 (the environment variables), INV-0420-2 rewritten as refused `connect`s in the UI and
  Web processes (the network is moved to the network process, ADR-0411 item 3), INV-0420-3 added.
- **M10** fixed: plan E0 and E1 on the M4 (no Linux CI build), E2 split from them.
- **M11** fixed: survey ICU paragraph cites [P3] for `udata_setCommonData`; ADR-0417 Option C re-weighed.
- **L1** fixed in the survey and ADR-0415 Context. **L2** fixed (ADR-0415 lists ADR-0051's limit and, if built,
  ADR-0100 item 6). **L3** fixed (postmortem anchors removed where they did not support the claim; "decision driver
  6"). **L4** fixed (tagged `[assumption]` or `[outside evidence]`, or sourced). **L5**, **L6**, **L7** fixed in the
  invariants. **L8** fixed (ADR-0414 item 2's ordering; a revisit trigger for a shared-memory semaphore). **L9** fixed
  (CP-27 removed from ADR-0421). **L10** fixed (ADR-0419 item 7). **L11** fixed (survey). **L12** fixed (pills,
  `rows-to-add.md`'s wording, Option C's front end).
- `status.md` was written by the main session before the review; it is the task's status file.

## Check of the changed lines

The reviewer checked commit `48d78a8c` against `2365e17a`, changed lines only. The checker still reports only the 12
expected `INV-0001-5` errors, and every ADR's Evidence line still matches its body (checked by script).

**Resolved:**
- **H1:** the survey, ADR-0410 Context, drivers and Option C are corrected, with [L56].
- **H3:** ADR-0415 item 1 closes the handles, and INV-0415-1 now tests through `malloc` and `mmap`. ADR-0051's
  "each handle, each mapping ... holds the object" (line 53) supports closing the handle.
- **H4:** ADR-0411 lists ADR-0287 item 3, ADR-0310 item 3 and ADR-0341 item 7, and adds INV-0411-4.
- **H5:** ADR-0413 item 1 requires `SHM_ANON`, ADR-0414 restates what WebKit takes without `OS(LINUX)`, ADR-0410
  adds the cost, and the plan adds E0.
- **Medium findings:**
  - M3, M8 and M9 are resolved.
  - M7: the remaining gap is point 6 below.
  - M10: the remaining gap is point 2 below.
  - M11 is resolved.
- **Low findings:** L1 to L12 are resolved as described in the dispositions.

**Problems in the fixes:**

1. **(medium) ADR-0414 item 2: unlisted tension with ADR-0315.** *Problem:* The new ordering has a waiting thread
   "record the port it will wait on ... while it holds the library lock, before ADR-0315's boundary releases the
   lock". ADR-0315 (accepted, Recommendation, lines 57-59) does the opposite: on `PENDING` the boundary "releases the
   lock, makes the wait on a port of its own, created for the wait ... no per-thread port to track". The fix
   therefore changes ADR-0315's order: the port is created under the lock and kept in the thread table. ADR-0414 does
   not say so. *Fix:* Add "It changes ADR-0315's deferred wait: the port is created, and recorded with its
   descriptors in the thread table, before the lock is released" to the Recommendation's opening paragraph. Either
   extend INV-0315-1's test or add INV-0414-2's ordering to it.

2. **(medium) Plan E1, ADR-0418 Context and E1: the M4 does not have Zoraya's CI compiler.** *Problem:* E1 builds
   `jsc` "on the M4 with Zoraya's clang and libc++" and claims to answer "the clang 18 with libc++ question". ADR-0418
   Context says the development Mac has Apple clang 21. INV-0418-1 now checks only CI-built images. A build with
   Apple clang 21 does not exercise the clang-18-only workaround (`VERSION_LESS 19`) and does not settle M1. *Fix:*
   Name the compiler E1 uses: LLVM 18 installed on the Mac beside Apple clang, or CI's container image run on the M4.
   Make "the compiler string is clang 18" part of E1's "done when".

3. **(medium) ADR-0410 item 3: the failing-`dlopen` shim and libepoxy's aborts.** *Problem:* libepoxy calls
   `abort()` when `dlopen` or `dlsym` fails on its `exit_on_fail` paths (`src/dispatch_common.c:315-317, 342-344,
   688-692`, fetched from libepoxy `master`). Every GL or EGL entry point resolved through epoxy's dispatch takes
   those paths, so the shim works only if no engine code ever calls an epoxy-dispatched function. With Skia on the
   CPU that is plausible but unverified: WebCore's `PlatformDisplay` may initialise EGL at start. The shim must also
   supply `dlerror`, which epoxy prints. *Fix:* Say that the shim is safe only if no epoxy entry point is called. Make
   E2's "done when" include a run with the shim's `dlopen` counting calls (zero expected). If the count is not zero,
   fall back to the WPE patch now named as the alternative.

4. **(medium) ADR-0412 item 2: the stream offsets go wrong on shared streams, and the order is not given.**
   *Problem 1, shared streams:* Descriptors are tagged with "the stream offset of [the] first byte". Item 6 now
   duplicates a stream socket's byte streams to forked and spawned children, so two processes can write to (or read
   from) one stream. Each library counts only its own bytes, so a parent's tag is wrong as soon as the child writes
   or reads, and descriptors are delivered with the wrong byte or never matched. The asyncio self-pipe case the fix
   was made for is exactly a stream that parent and child both hold. *Problem 2, order:* The ADR does not say whether
   the ancillary message is queued before or after the bytes. Writing the bytes first (as worded: "writes its bytes
   ... and then a message") lets a reader take the tagged byte before the tag has arrived. *Fix:* Queue the ancillary
   message *before* the bytes, and have `recvmsg` read the ancillary channel after reading bytes, holding back bytes
   past a tag in the library. Make `SCM_RIGHTS` on a stream whose byte-stream ends are held by more than one process
   (after `fork`, or after a spawn that duplicated them) `EOPNOTSUPP` in both processes, not only in the child. Add
   the shared case to INV-0412-2's property test.

5. **(low) ADR-0415 item 2(a) and ADR-0417: the second copy is not needed.** *Problem:* `libposix` "makes the image of
   `/proc/self/exe` once", and the UI process "keeps its own", so ICU's data is held twice (ADR-0417: 67 MB unfiltered).
   ADR-0107 (Recommendation, lines 162-169, 183) already gives every process its own image handle as the namespace
   entry `/proc/self/exe` → `Image`. Spawning the helpers from that handle shares the UI process's frames as well. The
   UI image's frames are then paid by the image's creator, the shell's side (ADR-0261 item 8), not the browser's room;
   say so. *Fix:* Spawn helpers from the bootstrap `Image` entry, drop "the UI process keeps its own", correct
   ADR-0417's "twice" and its 67 MB and 24 MB figures, and add one sentence on who pays for the image's shared frames.

6. **(low) ADR-0413 item 2: "never mapped or sent" leaves out inheritance.** *Problem:* A descriptor duplicated into
   a `fork` child or listed in a spawn's file actions is held by two processes as surely as one sent with
   `SCM_RIGHTS`, and replace-and-copy growth in one breaks the other. *Fix:* "never mapped, sent, inherited or forked".

7. **(low) ADR-0413 item 1: `SHM_ANON` in musl's header.** *Problem:* ADR-0100 (Recommendation item 1) makes musl's
   headers the sysroot's. Adding `SHM_ANON` to `sys/mman.h` is a change to that convention, and it is not listed.
   *Fix:* List it, or put the definition in a Zoraya header that `sys/mman.h` includes. Give its value: FreeBSD's is
   the pointer `(char *)1`, which `shm_open` must compare as a pointer.

8. **(low) ADR-0410 drivers 4 and 5 and Option C: new unsourced wording.** *Problem:* "Ladybird was rendering basic
   pages on Haiku within days in 2023": the survey ([O11]) says "basic pages in September 2023, after GCC 13",
   nothing about days. "A memory-safe JavaScript engine" has no source. "No LGPL ADR needed but for FFmpeg" means
   Option C also needs ADR-0421 unless media is off; say which. *Fix:* Drop "within days" or source it. Tag
   "memory-safe" `[assumption]` or drop it. State that a CPU-only build without media needs no LGPL library, if that
   is so.

9. **(low) ADR-0412 Recommendation: wrong item number.** *Problem:* It "narrows ADR-0225 item 1's refusal of paths",
   but the refusal (`AF_UNIX` with a path is `EAFNOSUPPORT`) is in ADR-0225 item 10. *Fix:* Change it to item 10.

10. **(low) Plan E0's "done when".** *Problem:* `TestWTF` lives in `Tools/TestWebKitAPI`. Whether the WPE release
    tarball ships `Tools/` was not checked by the documents or by this review, and the tarball was deleted. *Fix:*
    Confirm it is in 2.54.2. Otherwise name a test that is (for example `jsc`'s own tests for E1, and for E0 a small
    program over WTF built in `tests/wpe/`).

**No new problems found** in:
- ADR-0411's `POSIX_SPAWN_MOVE_NET`. It is consistent with INV-0420-2. The browser ending when the network process
  dies is stated.
- The new INV-0420-2 and INV-0420-3.
- ADR-0420's environment patches, and libgcrypt as a second crypto library.
- INV-0418-1.
- ADR-0419 item 7.
- The rewritten ADR-0412 item 5 (socket objects transient and in memory, ADR-0055 listed).
- ADR-0413 item 6 and its libffi open question.
- Plan F5, F6, D4 and D6, which match the ADRs.

### Dispositions of the check (the main session)

All ten accepted and fixed: (1) ADR-0414's Recommendation states its change of ADR-0315 item 1, and INV-0414-2's
check adds a host test of the boundary's order; (2) plan E1 and ADR-0418 name LLVM 18 beside Apple clang and make the
clang 18 compiler string part of E1's "done when"; (3) ADR-0410 item 3 states libepoxy's `abort()`, the shim's
`dlerror` and call count, and E2's "done when" asserts zero calls, with the WPE patch as the fallback; (4) ADR-0412
item 2 queues the ancillary message before the bytes, holds back bytes past a tag, and refuses `SCM_RIGHTS` in every
process once a stream is shared; INV-0412-2's test gains the shared case; (5) ADR-0415 item 2(a) spawns from the
bootstrap's `Image` entry (ADR-0107), so the UI process and its helpers share one copy, paid by the image's creator;
ADR-0417 corrected to one copy; (6) ADR-0413 "never mapped, sent, inherited or forked"; (7) `SHM_ANON` in a Zoraya
header included by `sys/mman.h`, FreeBSD's pointer value, the ADR-0100 change stated; (8) ADR-0410's unsourced wording
removed or tagged, FFmpeg's licence stated; (9) ADR-0225 item 10; (10) the measured tarball holds `Tools/`
(`TestWebKitAPI`), said in plan E0. Not reviewed again: TASK-33 asks for one check of the changed lines.

# TASK-33: Design for big C++ programs and a modern browser engine

## Where and how this runs

On the architect's **MacBook Air M1**, in its own clone (`~/workspace/zoraya-engine`), **documents only** (no `make`, no QEMU; reading the code is expected), beside TASK-32 on the same machine and TASK-31 on the Mac mini. Main model: **Opus**; Haiku subagents for fetching and summarising sources. Web access is needed. **Do not ask; decide, record, continue.**

## Why

NetSurf after 9f is a usable reader for simple sites, but it has no JavaScript and a 2000s layout engine. A browser for today's web is one of four engines (Blink, Gecko, WebKit, Ladybird). C++ now builds (TASK-30, `stage-cxx`), and its report lists what a browser engine still needs: shared memory between processes, `AF_UNIX` and server sockets, descriptor passing, `poll`, `posix_spawn`, more than 256 mappings per process, ICU with its data linked in, FreeType and HarfBuzz, possibly a newer clang in CI. This task decides which engine, and designs the foundations it needs so that the port is a sequence of buildable parts.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/tasks/TASK-30-report.md` (status block: limits and "what the next port will need"), `docs/tasks/TASK-27-report.md` (9d and 9f: NetSurf's platform layer, the window channel, the `web` grant), ADR-0100 to ADR-0110, ADR-0300 to ADR-0315, ADR-0317, ADR-0320, ADR-0324 to ADR-0328, ADR-0338 (pseudo-terminals as handles), ADR-0360 to ADR-0367.

ADR numbers **0410 to 0429** are reserved for this task.

## Part 1: research

`docs/research/browser-engine/survey.md`, evidence rules as in TASK-01 (primary sources, every URL fetched, never invent a citation, paraphrase):

- **WPE WebKit** (current release): the WPEPlatform API, software rendering through Skia, JavaScriptCore without JIT (LLInt, the C loop), the process model (UI, WebProcess, NetworkProcess, GPU process), the IPC (Unix sockets, descriptor passing, shared memory), every dependency with its licence and size, what a new platform must implement.
- **Ladybird** (current state): LibWeb, LibJS and its Rust parts, the process model (WebContent, RequestServer, ImageDecoder, WebWorker), its IPC, the dependencies through vcpkg (Skia, HarfBuzz, ICU, curl, ...), its Swift and Rust requirements, its own sandboxing (pledge/unveil heritage), how a new front end is written (the AppKit and Qt ones as examples).
- **Servo** briefly, and why Blink and Gecko are out of reach (state it with numbers: source size, build time and memory, platform layers).
- **How other small or non-Unix systems got a modern browser:** SerenityOS (Ladybird's origin), Haiku (WebKit/WebPositive), Fuchsia (Chromium and WebEngine), Redox, Genode (Falkon/QtWebEngine): what each had to build, how long it took, what is still missing.

For each engine: measured where possible, estimated and marked where not: lines of code to port or write here, number of dependencies, a clean build's time and disk on a 16 GB M4, the guest memory one tab needs.

## Part 2: decisions (proposed ADRs, one decision per ADR)

Template-exact, `proposed`, at least three options each with a Linux/FreeBSD baseline and a non-Unix precedent, "Effect of the vision", examples of what a person types and sees, invariants a later implementation can check. At least:

1. **The engine:** WPE WebKit, Ladybird, or another, for Zoraya's first modern browser.
2. **Processes and capabilities:** how the engine's processes map onto Zoraya: which handles each process gets (the content process: none of the person's files, no network; the network process: the `web` grant; the GPU or image process: nothing), how they are started (`posix_spawn` over the kernel's spawn, or the engine's own launcher written for Zoraya), and what a person sees (`top`, the pills).
3. **IPC:** `AF_UNIX` stream and datagram sockets and `socketpair` in `libposix` over Zoraya channels, with descriptor passing (`SCM_RIGHTS`) mapped to handle transfer; or the engine's IPC rewritten over channels. Which, and the rules (no socket files in a shared namespace, or socket files under a program's own directory).
4. **Shared memory:** `memfd_create`/`shm_open` and `mmap` of a shared object between processes, over Zoraya's memory objects; sealing; quota charging (ADR-0260's who pays).
5. **`poll` and event loops:** `poll`/`ppoll` (and whether `epoll` or `kqueue` is needed by the engine or its libraries) over the deferred-wait protocol of ADR-0315.
6. **Memory:** the mapping limit (`MAPPING_SLOTS`) and the heap's 1 MiB steps; what a guest needs (2 GB?) and what the defaults become.
7. **Fonts and text:** FreeType and HarfBuzz as ports beside `zoraya-text`, or `zoraya-text` behind the engine's font interface; fontconfig or a fixed font list.
8. **ICU:** data linked in statically (`--with-data-packaging=static`), which locales, the size.
9. **Toolchain:** whether the engine needs a newer clang than CI's 18 (and so libc++ newer than 20), Rust `std` for Zoraya (Ladybird's LibJS), Swift; the cost of each.
10. **The window:** the engine's platform layer over the window channel (ADR-0317), input, scale, the clipboard by gesture, downloads through the chooser.

## Part 3: plan

`docs/research/browser-engine/plan.md`: implementation parts of comparable size, foundations first (each useful on its own, for example `AF_UNIX` and `socketpair` also unblock CPython's `asyncio` self-pipe, `http.server` and `tmux`), then the engine's dependencies, then the engine, each with a visible result, its "done when", its dependencies, and which can run in parallel on one 16 GB machine.

## Review

An independent `opus-review` subagent that did not write the ADRs: citations fetched and accurate, options not strawmanned, every tension with accepted ADRs listed. Findings in `docs/research/browser-engine/review-notes.md`; fix; one check of the changed lines.

## Git

Branch `adr/browser-engine` from the latest `main`, in its own clone. Configured identity only. Push this one branch only; never merge, never push `main`. Touch only `docs/adr/0410-*` to `docs/adr/0429-*` and `docs/research/browser-engine/`; rows for the index and invariants in `docs/research/browser-engine/rows-to-add.md`. On a usage-limit warning: commit, push, write `status.md`, stop.

## Definition of done

Survey; proposed ADRs; plan; review; branch pushed. Final report: the engine recommended and why in one paragraph; each ADR's recommendation in one line; the foundations' parts in order with their size; the questions for the architect.

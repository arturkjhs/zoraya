# TASK-27: Stage 9 — applications: TLS for C, threads, Python, a browser

## Where, who and how

Locally on the architect's Mac mini (M4, 16 GB), as a **second dispatcher** beside TASK-26 (stage 8), which is finishing N6 and Q7. The architect is travelling: **do not ask; decide, record, continue.**

**Models** (as in TASK-26): this main session is a thin dispatcher on **Sonnet**; each part, or each large step of a part, goes to a fresh `general-purpose` subagent with model `sonnet` in its own worktree; `opus-review` (Opus, or Fable if offered) reviews every part and debugs what two attempts did not fix; `ci-watch`, `log-triage`, `test-runner`, `mutant-triage` (Haiku) wait on CI and read logs. Keep only summaries in this session; write the handoff after each part.

**Sharing the machine with TASK-26:**
- **Until the tag `stage-8` exists**, run only steps that build nothing: 9b's design (reading, ADRs, plan). Check with `git fetch --tags && git tag -l stage-8`.
- **After `stage-8`**, at most two parts with builds at once, as in TASK-26.
- Disk: before any build, check free space; below 25 GB, remove worktrees of merged parts (theirs or yours) that are not locked; remove a part's worktree as soon as it is merged; never `git worktree remove -f -f`.
- Memory limits from `CLAUDE.md`: `CARGO_BUILD_JOBS=3` per part, one QEMU test at a time per part, mutation runs only when nothing else builds.
- Merges: always merge `origin/main` into the branch just before merging it, recompute the invariants Total from the rows; if the push to `main` is rejected because TASK-26 merged first, repeat.

**Testing:** `make check` and `make test-changed` locally; the full candidate CI before every merge; `ci-watch` on CI.

## Why

Zoraya now has a desktop, a POSIX layer, TCP/IP with HTTPS and quotas. Stage 9 makes it run real applications: a TLS library for C programs (curl, wget and everything after), POSIX threads (most software needs them), CPython, and a graphical web browser.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/tasks/TASK-26-report.md` (its status block and "Not done" lists), `docs/research/stage-6/` (the POSIX tiers and the ports' method), ADR-0100 to ADR-0110 and ADR-0140 to ADR-0144 (libposix, ports, `zcc`), ADR-0106 (signals at wait points), ADR-0052 (protocols), ADR-0160 to ADR-0172 (the desktop), ADR-0220 to ADR-0230 and ADR-0287 to ADR-0289 (network, sockets, TLS), ADR-0260 to ADR-0270 (quotas).

ADR numbers **0300 to 0329** are reserved for this task.

## The parts

**9a — TLS for C programs, zlib, curl, wget.** Port OpenSSL 3 (static, `no-shared`, `no-dso`, `no-threads` until 9b, `no-engine`; pinned in `ports/sources.lock` with SHA-256), zlib, libcurl and `curl`, and `wget`, through `zcc`. `getrandom` in `libposix` over `rngd` (N5's "Not done"); OpenSSL's entropy from it. Roots from `/system/tls/roots`. Descriptions for `curl` and `wget` so `zosh` grants them `+net` by URL operand (ADR-0224) and the output file. Done when: `curl -O https://...` and `wget` fetch from the runner's TLS test server inside Zoraya, refuse an expired and a wrong-name certificate, and `grants curl -O https://x/y` shows exactly the network and file grants (closes N4's W11/W13).

**9b — POSIX threads.** Design first (ADRs): `pthread_*` in `libposix` over the kernel's native threads; mutexes, condition variables and `pthread_once` over a futex-like wait (reuse the kernel's primitive or add one); thread-local storage with musl; the signal rules with threads under ADR-0106 (which thread takes a signal at which wait point); `fork` in a threaded process (only the caller survives, as POSIX says); quotas (threads under ADR-0260's processes figure or a new field); `exec` from a threaded process. Then the implementation and tests: a pthread conformance subset (for example from musl's libc-test or the Open POSIX Test Suite), SQLite's threaded tests, a stress test, and OpenSSL rebuilt with threads. Done when: those pass under HVF and TCG, and ADR-0100's "no threads" is superseded by an accepted ADR.

**9c — CPython.** The current stable CPython (pinned), cross-built with `zcc`, static (`Modules/Setup.local`, no `dlopen`), the standard library installed under `/usr/lib/python3.X`, with `_ssl` (OpenSSL), `_sqlite3`, `_socket`, `select`, `_asyncio`, `zlib`, `_hashlib`. A description for `python3` (a script operand is read; `+r`/`+w`/`+net` as for any POSIX program). Done when: a chosen subset of CPython's own test suite passes (record which and why), an `asyncio` HTTPS client fetches from the test server, and `python3 -c 'import sqlite3, ssl, asyncio'` works in `zosh`.

**9d — NetSurf.** NetSurf's framebuffer front end with a `libnsfb` surface for Zoraya (a window from the compositor, its buffer, keyboard and pointer events); libcurl with OpenSSL from 9a, libpng, libjpeg, libnsgif or what the release needs; fonts through NetSurf's own or through `zoraya-text` behind a small C interface (decide, record). A description for `netsurf` (`+net` from the URL operand; downloads into a chosen directory through the chooser). Done when: it opens the runner's test pages (HTML, CSS, images, a form, HTTPS) inside a Zoraya window, screens in the report; by hand, a few real sites the architect names.

**Also:** a generic way to add programs built outside this repository to the system image (for example `make image EXTRA_ROOT=<dir>`: a directory copied into the volume, with its programs' descriptions), documented in `docs/design.md`, with a test using a small sample program.

## Order

- **Now (before `stage-8`):** 9b's design.
- **After `stage-8`:** 9a; then 9b's implementation and 9d in parallel; then 9c; the image addition alongside whichever part is light.

## Working without the architect

As in TASK-26: where accepted ADRs are silent, choose the simplest, least surprising, reversible and safest option, write it as a proposed ADR (0300 to 0329), and continue; merge each part after its candidate CI is green, its review found no open error, and none of its proposed ADRs changes an accepted decision; tag `stage-9a` … `stage-9d`; a part that cannot be merged waits on its branch with the reason in the report, and the next part goes on. Stop completely only for: anything outside this repository's worktrees, force-pushes or moved tags, memory or disk exhaustion despite the limits, or untrusted code (the gate stays closed). On a usage-limit warning: commit, push, update the report and handoff, stop.

## Report

`docs/tasks/TASK-27-report.md` with a "Status for the architect" block at the top, updated and pushed after each part: what is merged and tagged, what is in progress, the proposed ADRs with one-line recommendations, the manual checks the architect must do (9d's real sites) with exact commands, the measurements, and the time and model per part.

## Git

Configured identity only. Branches `work/9a` … (not named like tags); tags `stage-9a` … and `stage-9` when all four are merged. Commit and push after each step.

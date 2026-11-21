# Zoraya 0.1: the final report

The architect closed the experiment at version 0.1 on 2026-11-21. This page says what Zoraya is, what exists, how to run
it, what is missing and how the work was done. It was written by the dispatcher agent of TASK-31 from the repository
at the commit named in "The tag" below; every number was counted from that tree, from the ADR index or from CI.

## What Zoraya is

Zoraya is an experiment in building an operating system from scratch with AI agents, where every design decision is
grounded in measured evidence and recorded as an ADR (`docs/design.md`, "Vision"). Its core idea: a process reaches only
what it was explicitly given. There is no ambient authority, no global namespace, no `fork` and no asynchronous signal
in the native world; POSIX lives in an isolated personality on top.

- **Capabilities.** A process holds handles (rights-carrying references to ports, memory objects, files, network
  scopes) and nothing else (ADR-0002 to ADR-0006). Files come as connections to a file service with rights, narrowed
  by whoever hands them on. The one right outside `ALL` is `BIND` (ADR-0466), which only a program's own directory and
  `/tmp` carry.
- **Grants from the command line.** A shell line says what the program gets: `+w PATH` (write), `+net HOST:PORT`,
  `+sock PATH`, and so on. The shell holds the user's authority and passes a narrower copy to one program. A program
  started without a grant gets none, and `grants` shows what each running program holds (ADR-0337 and the tour in
  `docs/tour.md`).
- **The compositor's pills.** In the desktop, the compositor draws, outside any client's reach, a row of pills that
  state a window's grants. No program output reaches the status row or the grant line, and the system key combination
  that gives a running program a file reaches no client (stage 10f).
- **Rooms.** A room is a set of windows with their own grants, quotas and loopback network. A room is trusted or
  untrusted; an untrusted room is bounded in memory, processes, CPU, disk and sockets by quotas
  (ADR-0260 to ADR-0270, ADR-0442). A room keeper supervises it and `top` shows it.
- **Quotas.** Budgets on jobs, accounts in the file service, the broker, the network service and the compositor,
  directory limits, a CPU ceiling and bounded kernel work. The gate of the quota stage is a room in which a fork bomb,
  a memory hog, 64 spinners and `dd` each stop at the room's limits while a home terminal stays responsive
  (`make test-quotas`, ADR-0268).
- **POSIX as a personality.** `libposix` is a per-process library for the data path (files, pipes, memory, time,
  `posix_spawn`, readiness waits), a per-session broker keeps pids, process groups and sessions, and a temporary
  directory service gives `/tmp` and FIFOs. musl is the C library; the kernel has no POSIX system calls
  (ADR-0002, ADR-0005). That is how CPython, git, OpenSSH and the rest run unmodified apart from patches under
  `ports/patches/`.

## What exists, stage by stage

First commit 2026-10-20; the dates are those of the tags (`git tag -l 'stage-*'`).

| Stage | What it gave | Tag, date |
|---|---|---|
| 0 and 1 | Boot, UART, panic handler, device tree, test harness; exception vectors, GIC, timer, MMU with the kernel in the upper half, physical frame allocator, kernel heap | before `stage-2` |
| 2 and 3 | Threads, scheduler, EL0, native system calls, ELF loader, ramdisk; handles, ports and process objects, native `spawn` and wait | `stage-2` and `stage-3`, 2026-11-01 |
| 3a | `zosh`, the native console shell, over the serial line | `stage-3a`, 2026-11-01 |
| 4 | virtio-blk, the file service, `mkfs` on the host; files survive reboot | `stage-4`, 2026-11-02 |
| 5 | virtio-gpu, virtio-input, the compositor, the terminal as a GUI client | `stage-5`, 2026-11-02 |
| 6 | Kernel objects as built (6a), the toolchain (6b), `libposix` core (6c), processes and terminal (6d), acceptance: `tcc hello.c && ./a.out` inside Zoraya (6e) | `stage-6`, 2026-11-04 |
| 7 | Design system, text rendering, window management, keyboard layouts, scaling, toolkit, file chooser, panel, file manager, editor, settings, clipboard, rooms (7a to 7h) | `stage-7`, 2026-11-10 |
| 8 | Networking (virtio-net, `netd` on smoltcp, `+net` grants, names, sockets, real clock, random service, TLS), quotas (q1 to q7), and the gates (n1 to n6) | `stage-8`, 2026-11-13 |
| 9 | Applications: ported programs, `stage-9a` to `stage-9f`; then C++ (`stage-cxx`, 2026-11-14) | `stage-9`, 2026-11-13 |
| F | Foundations ahead of stage 10: `stage-f2` to `stage-f8` (2026-11-15 and 2026-11-16) | `stage-f8`, 2026-11-16 |
| 10 | Living in the system: the terminal as `xterm-256color` (10a, 10b), a terminal for POSIX programs from `zosh` (10c), ncurses, `less`, `vi` (10d), toybox's userland (10e), giving a file to a running program (10f), a program's own directory and ssh (10g), git over https (10h), installed programs (10j); 10i, the tour and checklist, was not done | `stage-10a` to `stage-10h` and `stage-10j`, 2026-11-14 to 2026-11-19 |
| After 10 | Own-directory sockets and the `BIND` right; the host draws the pointer and frames are paced (`work/gui-smooth`); Rust `std` for POSIX programs (`work/rust-std`, ADR-0450 to ADR-0455); input frames carry the driver's time (ADR-0469) | on `main`, untagged |

Stage 10's acceptance, as `docs/design.md` states it: in a desktop terminal a person edits a file in `vi`, pages a log
in `less`, searches with `grep`, gives `vi` a second file with System+G while it runs, commits with `git`, pushes with
`+net` and logs into the runner's ssh server, with `grants` showing each program's files and hosts. Part 10i, which
would have written the tour for it, did not run (see "The work left").

## The numbers

Counted on the tag's tree (`git ls-files`, `docs/adr/index.md`, CI):

- **Commits on `main`:** 1919 at the time of counting, from 2026-10-20.
- **Rust:** 284,787 lines in the repository outside `vendor/` and `ports/` (`git ls-files '*.rs'`). Of these, 200,418 lines are in `crates/`, 18,531 in `kernel/` and 39,154 in `user/`. The workspace has 73 crates under `crates/`, plus `kernel` and `user`, and a separate `fuzz/` project.
- **ADRs:** 275 files; 262 accepted, 10 proposed, 1 rejected, 1 superseded.
- **Invariants:** 907 in `docs/adr/invariants.md`, each with a check (`make adr-check` verifies the table against the ADRs).
- **CI:** one workflow (`.github/workflows/test.yml`) with 68 steps in `tools/ci-tests.txt` (the QEMU kernel tests are sharded and run under HVF and TCG, in three machine configurations), 126 jobs in a full run, plus 30 `check-*` jobs in `make check`.
- **Documents:** about 252,000 lines under `docs/` (ADRs, research, task reports, reviews).
- **Reviews:** 83 review records in `docs/tasks/`.
- **Ported programs**, each built from sources pinned in `ports/sources.lock` and run inside Zoraya; those with a test step in CI's full run are marked with the step:
  CPython (`test-python`, `test-python-ext`, `test-python-regr`, `test-python-mem`, `test-python-serve`), LLVM's libc++ and its test suite (`test-cxx-libcxx`), GoogleTest (`test-cxx-gtest`), SQLite with its `threadtest3` (`test-posix-threadtest3`), OpenSSL and curl (`test-net`, `test-apps`), libc-test (musl's), git over https and OpenSSH (the tests of parts 10g and 10h), nvi (`vi`), less and ncurses with vttest (parts 10d and 10c's terminal tests), NetSurf (`test-netsurf`), toybox (`test-posix-suites`), and the toolchain itself: tcc, dash, make, pdpmake, awk, bison, lua, wget, zlib, libpng, libjpeg, mbedtls.
  Whether each step passes is stated in "The tag": a step counts only if it was green in the full run on the tagged commit.

## How to build and run it on a Mac with Apple Silicon

The development host was an M4 Mac mini. Zoraya runs on QEMU's `virt` machine under HVF (the `host` CPU), or under TCG
elsewhere. Real hardware is out of scope.

1. Install the Rust toolchain (`rust-toolchain.toml` pins 1.99.1 and the `aarch64-unknown-none` target), QEMU 11 (Homebrew), Python 3 and a C compiler for the host tools.
2. `make ports` fetches and builds the pinned ports (`ports/sources.lock`; the sources are cached under `build/ports/src`, nothing is installed outside `build/`). `make apps` builds the applications that go on the disk. `make build` builds the kernel and the ramdisk.
3. `make run` boots with the serial console: `Type help to begin, or help tour.` and the prompt `~> `. Ctrl-A X leaves QEMU.
4. `make run-gui` boots the desktop with a GPU, keyboard, tablet and the network (`NET=0` for none), at scale 2 on a Retina screen (`GUI_SCALE=1` or `2` chooses). The window can be resized and the desktop follows it.
5. `make check` runs the fast checks (format, clippy, ADR and invariant checks, every `check-*`). `make test-changed` runs the host tests of what changed and the QEMU steps that cover it, under HVF. `make test-par` and `make test` run everything.

What to try first: in the desktop, press System+Enter (Command+Enter) for a terminal; type `help tour` and follow
`docs/tour.md`. Open Files, give a file to a running program with System+G, run `grants` to see what each program holds,
and create a room with `room new`. In a POSIX terminal try `vi`, `less`, `python3`, `git clone https://...` with a
`+net` grant, and `fetch` or `ping` for the network. Without a grant a program is refused and the shell prints the hint.

## Known limits

- **QEMU only.** No real hardware.
- **One CPU in normal runs.** SMP runs in a few kernel test configurations (`-smp 4`), not in the desktop.
- **No audio or video.**
- **No modern web engine.** NetSurf is the browser; the plan for a modern engine is in `docs/research/browser-engine`.
- **No dynamic loading.** Programs are linked statically.
- **Rust `Command::spawn`'s fork paths** fail with `Unsupported` (ADR-0468, proposed): a program that sets `uid`, `gid`
  or `pre_exec`, or finds a program by name through `PATH`, cannot spawn.
- **The two shared bounds** of the file service (256 socket nodes, 64 listeners) are per volume, not per room.
- **Input order** is the order of the drivers' times (ADR-0469), not the hardware's.

## The work left

- **F11 and the other gate conditions** of the untrusted-room stage: per-room bounds for the file service's nodes and listeners, and the gate of ADR-0268 on the whole.
- **The untrusted-room launcher** that starts an untrusted room from outside the repository.
- **The security review** (TASK-32), which has not run.
- **The browser-engine plan** (`docs/research/browser-engine`).
- **SMP** in normal runs.
- **Part 10i**: the tour for stage 10, `docs/design.md`'s last pass and the checklist.
- **Unfinished branches**, pushed and not merged (see the end of this report): `work/host-ports` (host ports given by the OS, so parallel `make test-changed` runs do not collide) and `work/cpu-ceiling-robust` (a `cpu_ceiling_from_a_process` that holds under host load).
- **The generic requests** in `docs/backlog.md` (R20 to R28, an emoji font, installing from a desktop terminal, and the others).
- **The open architect queue** in `docs/architect-queue.md`: the ten proposed ADRs and their recommendations.

## How the work was done

- **The architect** decided every ADR (agents write them as `proposed`; `docs/adr/` is binding once accepted), set the
  stage plan, gave the permissions that the rules name (a merge into `main`, a stage tag) and closed the project.
- **Dispatchers on Sonnet** each ran a task: they started a fresh Sonnet agent per part in its own worktree, kept summaries,
  merged parts with a fixed merge form and waited on CI. **Implementers** (Sonnet) wrote the parts. **Independent Opus
  reviews** read every part's diff, ran mutations by hand, and debugged what two attempts did not fix.
  `docs/tasks/` holds 83 review and report records.
- **The CI policy changed as the work showed its costs** (ADR-0348, amended by ADR-0463): parts merge after `make check`,
  `make test-changed` under HVF and an independent review; candidate runs only for changes to the workflow, the CI plan
  scripts and the test runner; the full set runs on a schedule; a stage tag needs a green scheduled, nightly or
  `candidate-full/` run that includes the commit.

What happened, as facts:

- **The reviews caught real errors, not style.** The review of own-directory sockets found that the place rule of `bind`
  was enforced only by `libposix`, so a program without it could plant a socket node anywhere; the `BIND` right
  (ADR-0466) closed it. The review of `work/gui-smooth` found that the cursor image request went through the
  compositor's blocking call and could swallow a queued `Resized`, losing a resize and leaving a stray reply; the fix made
  it asynchronous. The first review of `work/rust-std` found that the broker's random connections were not bounded by the
  asker, so a process could drain the connection limits; the credit per spawn replaced it, and a second review found that
  the first fix still minted credit for free on a fork prepare. Mutation checks by hand showed which new tests were
  vacuous (two tests of the pointer fix survived their mutations until they were extended).
- **A merge that is green locally can break the full run.** `work/gui-smooth` merged after its local tests and review
  and turned `main` red in two files-app GUI jobs. Three causes, all from reading a device's messages together: a drag's
  moves were told once at the end of the round, held buttons went to the window that never got the press, and a
  device's whole backlog was drained before the next, so a Shift-click lost its Shift when the compositor was behind.
  Only the third needed a protocol change (ADR-0469, a time in each input frame); the full run needed three attempts.
- **A step that no local run reached.** The kernel test list in one guest under smp4 with 2 GiB passed locally under HVF and
  timed out at 420 s on CI's TCG (`kernel_work_is_bounded` did not finish). It was split into two shards.
- **Waiting sessions stalled.** A dispatcher idled for hours on an agent that had stopped, and a merge went in without a
  look at the red scheduled run before it. The rules that followed are in `CLAUDE.md`: a dispatcher never waits without a
  30-minute wake-up that then checks every agent's branch, log and processes; a red scheduled run is fixed first.
- **Permission refusals shaped the rules.** A subagent's push to `main` was refused by the permission classifier, which
  led to the rule that only the dispatcher merges. A broad `pkill` was refused because another session built on the same
  machine; a push of `main` was refused when `make check` had exited 2 in the pushing worktree (a stale sysroot there),
  although it had passed in another worktree; an edit of a proposed ADR's text was refused once. Each was dealt with by
  satisfying the rule instead of working around it.
- **CI load decided the pace.** A candidate run waited about 80 minutes in the queue and ran about an hour; the full run
  has 126 jobs; local `make test-changed` runs collided on host ports and under shared load (`cpu_ceiling_from_a_process`,
  `kernel_work_is_bounded`), which is why those two have a rule of their own and why `work/host-ports` and
  `work/cpu-ceiling-robust` exist.
- **Stale local state looked like failures.** A worktree's `build/sysroot` that predated a header change failed
  `check-libposix-widths`; a stale `fuzz/Cargo.lock` failed two fuzz jobs of a candidate; missing pinned source tarballs
  in a fresh worktree failed builds before any test ran.

## The tag

The tag `zoraya-0.1` is an annotated tag on the commit that contains this file, put there by the dispatcher with the
architect's explicit permission (his instruction of 2026-11-21). The evidence for it is a `candidate-full/zoraya-0.1` run
on `eb5afd38` (run 44878482107, 91 jobs, all green); the tagged commit is `eb5afd38` plus documents only (this page and
the README; their push run, 44883324079, was green). CLAUDE.md counts a green `candidate-full/` run for a stage tag
(ADR-0348), but `tools/ci-last-full.rb --green-for` reads only scheduled runs, and GitHub's cron started no scheduled
run in the six hours after `eb5afd38` (the last one, 44860388634 on 2026-11-21 at 03:17 UTC, was red in the files-manage GUI job on the Shift-click order that ADR-0469 fixed), so the tool exits 1 for this commit. That is stated here so that nobody reads the tag
as a scheduled-run tag. The stage tags `stage-10h` (`f043ca57`) and `stage-10j` (`fe1feacd`) were pushed earlier on a
green scheduled run (44716595367, on `4b7eabfd`).

What was green and what was not, at the end:

- A `candidate-full/zoraya-0.1` run on `eb5afd38` (run 44878482107) ended green in all 91 jobs, with the sweep-wake limit
  of `t_thread` widened to 800 ms after one failure of 429 ms under TCG (run 44875851405, a manual run).
- The steps skipped locally by policy (the four python steps, TCG rounds) run only in CI; where this page says a program's
  suite passes, it means CI's green full run, not a local run.
- Part 10i, the work/host-ports branch and the work/cpu-ceiling-robust branch are not on `main`.

Unfinished branches, pushed to `origin`, not merged:

| Branch | Head | State |
|---|---|---|
| `work/host-ports` | `06ac628e` | host ports chosen by the OS (`--net-forward auto:FILE`) so that parallel runs do not collide; three commits, tests not finished; no review |
| `work/cpu-ceiling-robust` | `4abac058` | `cpu_ceiling_from_a_process` made robust to host load; a first version with numbers in `docs/tasks/TASK-31-report.md`; open items listed there; no review |

Proposed ADRs for the architect (the ten with status `proposed` in `docs/adr/index.md`; recommendations in
`docs/architect-queue.md`): ADR-0345, 0346, 0347, 0394, 0395, 0410, 0464, 0467, 0468 and 0469. `docs/tasks/TASK-31-report.md` holds the handoffs of
2026-11-19 and 2026-11-20 with the queue as it stood.

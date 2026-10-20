# TASK-02: Measure the POSIX surface required by a minimal C toolchain

## Context

Read first:
- `docs/research/candidate-principles.md`, especially conflicts X-1, X-2, X-3
- `docs/research/design-mistakes.md`, `docs/research/held-up-well.md`
- `docs/research/research-log.md`, especially open questions

TASK-01 showed that "clean" principles (capability-only access, no `fork`, no signals) conflict with hosting real software, and that every alternative OS that wanted real software ended up building a POSIX compatibility layer.

Our plan is a native ABI of our own design plus POSIX as a sandboxed compatibility personality in userspace. Before any ADR, we need numbers: **exactly which POSIX functionality a minimal self-hosting C toolchain actually uses, where, and how hard each use is to avoid.** This turns the X-1..X-3 debates into measurable questions.

This task measures. It does **not** make design decisions, write OS code, or implement a compatibility layer.

## Target software, by tier

Pin every program to an exact version or commit and record it.

| Tier | Purpose | Programs |
|---|---|---|
| T0 | Minimal interactive system | a shell (`dash`; also `oksh` or `mrsh` for comparison), core utilities needed by builds (`toybox`; compare with `busybox` where cheap) |
| T1 | Self-hosting C without autoconf | TinyCC (`tcc`, current mob branch, incl. its built-in assembler and ELF linker), a make (`pdpmake` and GNU `make`, compared), T0 |
| T2 | Building typical third-party C software | T1 + running real `./configure` scripts (autoconf-generated) |
| T3 | Scale reference only | `clang` + `lld` compiling and linking one file. Measure surface size only, no call-site analysis |

## Workloads

Each measurement must run a real workload, not just `--version`:

- **W1:** `tcc` compiles itself (self-build).
- **W2:** build Lua with a plain Makefile using `tcc` + make + shell.
- **W3:** build SQLite (amalgamation) with `tcc`.
- **W4 (T2):** run `./configure && make` for one small autoconf project of your choice (e.g. GNU `hello` or similar). Record which configure checks fail with `tcc`.
- **W5 (T0):** a scripted interactive shell session: pipes, redirections, globbing, background job, `Ctrl-C` of a foreground job, `cd`, environment variables.
- **W6 (T3):** `clang -c` of one C file and `lld` link of the result.

## Environment

### Prerequisite (human does this before the agent starts)

An Alpine Linux aarch64 VM in UTM (QEMU, HVF acceleration), with SSH enabled via key and a port forward from the Mac (e.g. `localhost:2222`). Alpine is chosen because it uses musl, so static analysis against a single small libc is straightforward.

### Agent setup

- Verify SSH access first. If it does not work, **stop and report** — do not try to create or reconfigure VMs.
- Install what you need with `apk` (e.g. `build-base`, `strace`, `gdb`, `git`, `linux-headers`, `musl-dev`, `clang`, `lld`).
- All measurement scripts live in the repo under `tools/posix-surface/` and run from the Mac over SSH, so the whole task is reproducible with one command.

## What to measure

### M1. libc API surface (primary result)

This is the most important output: it is what our compatibility layer must implement.

- **Upper bound (static):** compile each program's own object files (not libc's) and collect undefined symbols resolved by musl (`nm -u` on objects, or linker trace/map). Only count the API boundary: functions the program itself calls, not musl calling itself.
- **Lower bound (dynamic):** functions actually called during workloads W1–W6. Use an `LD_PRELOAD` interposer generated from the static symbol list, or another method you can justify. Handle variadic functions (`printf`, `open`, `fcntl`, `ioctl`) correctly.
- For each symbol: header/category (stdio, string, process, signal, file, dir, time, locale, tty, memory, threads, network, other), which programs use it, static/dynamic evidence, tier.
- Report counts per tier and per category, and the difference between upper and lower bound.

### M2. Linux syscall surface (secondary result)

What musl needs underneath. This is not our ABI, but it shows the kernel-side semantics the compat layer must provide.

- `strace -f` over each workload. Collect syscall names and counts per program.
- For syscalls where arguments define semantics, record the argument patterns actually used: `clone` flags (fork vs. thread), `openat` flags and whether paths are absolute/relative/dirfd-based, `fcntl` commands, `ioctl` request codes, `rt_sigaction` signal numbers and handlers, `mmap` flags, `wait4` usage.

### M3. Call-site analysis for the conflict-critical features

For every call site in the tier T0–T2 programs that uses one of these features, record: file, function, line, what it does, and a **replaceability class**:

- `direct` — can be replaced by a non-problematic primitive with a small local change (e.g. `fork`+`exec` → `posix_spawn`), estimated lines changed
- `restructure` — requires changing program structure, estimated effort
- `emulate` — realistically needs the compat layer to emulate the POSIX semantics

Features:
1. `fork`, `vfork`, `exec*`, `posix_spawn`, `wait*`, pipes between processes
2. Signals: which ones, handler vs. default, why (child exit, interrupt, timeout, job control)
3. Global path access: absolute paths, `$PATH` search, `/tmp`, `/dev/*`, `/proc/*`, `/etc/*`, current working directory dependence
4. TTY and job control: `termios`, process groups, sessions, `isatty`, `tcsetpgrp`
5. Identity and permissions: uid/gid, `chmod`, `umask`, file modes
6. Anything else that TASK-01 entries flag as a design mistake and that shows up in the data (cite the DM ID)

### M4. Validation patches (small, optional but recommended)

For 2–3 `direct` call sites (at least one in the shell and one in make), actually write the replacement patch, rebuild, and rerun the workload. This calibrates whether the effort estimates in M3 are realistic. Keep patches in `tools/posix-surface/patches/`, never modify the measured upstream trees in place.

## Measurement honesty

- Static analysis overestimates (dead code, error paths). Dynamic tracing underestimates (only exercised paths). Always report both and say which number a conclusion relies on.
- Every number in the outputs must come from a script in `tools/posix-surface/` that can be rerun. No numbers from memory or from other people's write-ups.
- If a measurement fails or a method is unreliable on aarch64 musl, record it in the log and use a fallback; do not silently drop it.
- Where you interpret data (e.g. replaceability), mark it as an estimate.

## Output files

In `docs/research/task-02/`:

1. `posix-surface.md` — summary: counts per tier and category (upper/lower bound), the smallest surface that supports T1, the jump from T1 to T2 and to T3, key observations.
2. `libc-surface.csv` — one row per libc symbol: `symbol, category, programs, static, dynamic, tier`.
3. `syscall-surface.csv` — one row per syscall: `syscall, programs, call_count_per_workload, notable_args`.
4. `callsite-analysis.md` — M3 results grouped by feature, with replaceability classes and effort estimates; M4 patch results and how they compared to estimates.
5. `conflict-impact.md` — for each conflict X-1, X-2, X-3 (and any other conflict the data touches): what the measurements show, which options the data rules in or out, what remains unknown. **No decisions.**
6. `method.md` — environment, pinned versions/commits, exact commands, known limits of each method.
7. `research-log.md` — what was tried, what failed, open questions.

## Process

1. Read the TASK-01 files listed in Context.
2. Verify the environment. Stop if SSH to the VM is not available.
3. Write the measurement scripts and run them on T0 first; check that results look sane before scaling to T1–T3.
4. Run M1–M2 for all tiers, then M3 for T0–T2, then M4.
5. Have a separate subagent that did not write the scripts review `method.md` and spot-check 20 random rows of each CSV against raw data. Record findings in the log.
6. Write `conflict-impact.md` last, from verified data only.

Work on the branch `research/task-02`. Commit after each step with a clear message. Do not merge or push.

## Git rule

Use the repository's configured git identity. If no identity is configured, **stop and ask** the human. Never pass `-c user.name` / `-c user.email` or invent an identity.

## Definition of done

- M1 and M2 completed for all tiers T0–T3 with both static and dynamic results where applicable.
- M3 completed for every call site of the listed features in T0–T2 programs.
- At least 2 validation patches from M4, or a recorded reason why not.
- All numbers reproducible via `tools/posix-surface/` from a single entry-point script.
- Independent review done and recorded.
- `conflict-impact.md` covers X-1, X-2 and X-3.

## Out of scope

- Writing OS or kernel code.
- Implementing the compatibility layer.
- Making design decisions or writing ADRs.
- Porting any program to our OS.

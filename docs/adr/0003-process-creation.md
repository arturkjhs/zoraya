# ADR-0003: Process creation

- **Status:** proposed
- **Date:** 2026-10-25
- **Depends on:** ADR-0002
- **Evidence:** CP-02, CP-05, CP-06, CP-07, CP-14, CP-21, CP-25, X-2, X-6, X-12, X-15, DM-001, DM-002, DM-003, DM-005, DM-006, DM-008, DM-030, DM-085, DM-100, GR-003, GR-005, task-02/posix-surface.md#linux-syscall-surface-m2, task-02/posix-surface.md#key-observations, task-02/callsite-analysis.md#feature-1-process-creation-exec-wait-pipes, task-02/callsite-analysis.md#m4-validation-patches, task-02/conflict-impact.md#x-2-no-fork-cp-05-vs-hosting-the-c-toolchain, task-02/conflict-impact.md#x-6-strict-commit-cp-14-vs-fork-emulation, task-02b/conflict-impact.md#x-2-no-fork, task-02b/event-timelines.md#w10-background-jobs-wait-concurrent-pipelines-run-w10-parallel-r1, task-02b/callsite-analysis.md#3-signal-handlers-and-masks-during-parallel-builds, alternatives-postmortem.md#exokernel-mit-aegis--xok, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

ADR-0002 places process construction in the native ABI and allows the personality one reserved kernel facility for `fork`. This ADR decides the native primitive and the shape of that facility, using the TASK-02 call-site data.

**What is measured.**

- Process creation by program, core runs (`task-02/conflict-impact.md#x-2-no-fork-cp-05-vs-hosting-the-c-toolchain`; `tools/posix-surface/results/fork-children.md`):

  | Parent | fork-style clones | of which exec | of which no-exec | no-exec that spawn | vfork-style clones (all exec) |
  |---|---|---|---|---|---|
  | dash | 1,705 | 536 | 1,169 | 695 | 2,041 |
  | GNU make | 0 | — | — | — | 281 |
  | toybox (xargs) | 0 | — | — | — | 2 |
  | clang | 0 | — | — | — | 1 |
  | tcc | 0 | — | — | — | 0 |
  | a build script (`vc-list-files`) | 3 | 0 | 3 | 0 | 0 |

  Every vfork-style child execs. GNU make creates every child through `posix_spawn` (M3: 2 sites, `direct`), which under musl is itself a `CLONE_VM|CLONE_VFORK` clone (`task-02/callsite-analysis.md#m4-validation-patches`). tcc creates no processes. Parallel make changes nothing: clone counts were identical serial vs `-j` (W8: 78 and 78, `task-02b/conflict-impact.md#x-2-no-fork`).
- dash's no-exec forks are subshells, command substitution, the heredoc writer and pipeline elements that are builtins (`task-02/callsite-analysis.md#feature-1-process-creation-exec-wait-pipes`, `forkshell` at `src/jobs.c:982`, class `emulate`). Of the 1,169, W4 (autoconf `configure` + make) accounts for 1,167: 764 in configure and 403 in make (`task-02/conflict-impact.md#x-2-…`, Unknown). T1 without autoconf has 2.
- Shell background jobs and pipelines in W10 are plain forks followed by exec, plus one subshell fork that itself vforks (`task-02b/event-timelines.md#w10-…`). Background children set `SIGINT`/`SIGQUIT` to ignore *before* exec (`task-02b/callsite-analysis.md#3-signal-handlers-and-masks-during-parallel-builds`), which is state a spawn call must be able to express.
- M4 calibration: dash's non-interactive `vforkexec` was replaced by `posix_spawn` in 52 lines with identical outputs; the interactive path was left on `vfork` because handing the terminal to the child's process group (`tcsetpgrp` in the child) has no `posix_spawn` attribute. pdpmake's `system()` was replaced in 34 lines, about 3× the `direct` estimate, because waiting, status decoding, argv building and explicit `envp` are part of the contract (`task-02/callsite-analysis.md#m4-validation-patches`).
- Fd handling around spawns is the bulk of `fcntl` traffic: `F_SETFD` 5,360 and `F_DUPFD_CLOEXEC` 3,894 calls (`task-02/posix-surface.md#linux-syscall-surface-m2`), that is, close-on-exec bookkeeping, which is DM-002's retrofit at work. musl's `posix_spawn` child resets every signal disposition and uses a pipe to report exec failure (`task-02/callsite-analysis.md#m4-validation-patches`).
- toybox's `xfork`/`xpopen(argv == NULL)` re-run the applet in a child (`cpio -p`, `tar`), class `restructure`, 40–50 lines each; no workload reached them (`task-02/callsite-analysis.md#feature-1-…`).

**What is assumed.** The memory cost of a dash fork under strict commit was not measured (X-6, `task-02/conflict-impact.md#x-6-strict-commit-cp-14-vs-fork-emulation`). The cost of a fork-free shell is unmeasured; no such shell was run (`task-02/conflict-impact.md#x-2-…`, Unknown).

**Forces from TASK-01.** `fork` is the root of DM-001, DM-002 and DM-004 and it encourages overcommit (DM-030). Explicit creation held up (GR-005), with the caveats that `posix_spawn` cannot express terminal attributes or namespace entry and reports child-side failures poorly, and with Drepper's dissent that fork-without-exec is legitimate (DM-001, GR-005). Cygwin's fork emulation over CreateProcess is "neither fast nor reliable" (GR-005). Xok's fork took about 6 ms against under 1 ms on OpenBSD because it could not share page tables (`alternatives-postmortem.md#exokernel-mit-aegis--xok`). Zircon has no fork or exec; a launcher builds the process from handles (GR-005, `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). Every process should be created inside a job (CP-07, DM-100).

## Decision drivers

1. **The toolchain does not need `fork`; the shell does.** Zero forks in make, tcc, toybox, clang; 1,169 no-exec forks in dash, almost all in autoconf runs (`task-02/conflict-impact.md#x-2-…`, "Out: the C toolchain needs fork"). This bounds X-2 to one program class.
2. **Every non-shell child execs**, which is the `posix_spawn` shape (`task-02/conflict-impact.md#x-2-…`, "Every vfork-style child execs"). A spawn-with-handle-list primitive replaces these directly (M3: GNU make's 8 process sites all `direct`).
3. **Spawn must express what `posix_spawn` cannot**: process group membership and terminal hand-over for interactive job control (M4, `task-02/callsite-analysis.md#m4-validation-patches`), and inherited "ignored" dispositions for background jobs (`task-02b/callsite-analysis.md#3-…`). GR-005 lists exactly these gaps.
4. **Explicit handle lists remove DM-002**, and the data shows how much code exists only to work around it (9,254 close-on-exec `fcntl` calls).
5. **A child gets nothing by default** (CP-05, CP-01), inside a job (CP-07), with an explicit environment vector (CP-21, X-15) and an argv vector, not a string (DM-008).
6. **Strict commit makes fork expensive in a new way** (CP-14, X-6): fork without overcommit needs commit for a full copy of the parent (DM-030). The cost is unmeasured for dash.
7. **Processes are handles; exit status lives on the object** (CP-06, DM-003, DM-005): the spawn primitive returns a process handle, and the parent relationship is not special.

## Considered options

### Option A: `fork` + `exec` as the native primitive (baseline, "do what POSIX does")

- **Description:** The kernel duplicates the calling process; the child customises itself and calls `exec`. Shells, make and everything else work unchanged.
- **Evidence for:** No emulation anywhere; dash's 1,169 no-exec forks and the heredoc writer work as written (`task-02/callsite-analysis.md#feature-1-…`). Drepper's position: fork without exec is useful for multi-process designs that want a shared layout, and spawning workers loses ASLR-layout sharing and page de-duplication (DM-001, GR-005). Fork is "27 lines" of mechanism and needs no parameter per piece of state (DM-001).
- **Evidence against:** Not thread-safe, inherits everything by default, cost grows with parent size, encourages overcommit, 25 POSIX special cases for inheritance, and it constrains kernel design by forcing all process state into one copyable place (DM-001). Fd inheritance by default needed a per-call flag retrofit (DM-002). It contradicts CP-05 and pushes against CP-14 (X-6). The measured programs other than the shell never use it (driver 1).
- **Cost:** *(estimate)* Lowest compatibility cost. Kernel pays CoW machinery and copyable process state for every process. Under strict commit every fork of a large process must either reserve a full copy or overcommit (DM-030).
- **Precedent:** Unix, Linux, BSDs. Works; it is the source of the entries cited.

### Option B: Native spawn with explicit handle list; `fork` emulated for `posix` jobs by a kernel address-space snapshot

- **Description:** The native primitive builds a process from parts: a job handle, an executable image handle, an address-space description, an explicit list of (slot, handle, rights) to transfer, an argv vector, an environment vector, and start attributes (process group, initial suspended state). It returns a process handle (CP-06). For `posix` jobs only (ADR-0002 rule 3), a second call snapshots the calling process's address space copy-on-write into a new process in the same job and starts a thread with the caller's register state: this is `fork`. Precedents: NT `CreateProcess` with `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` (GR-003, GR-005), `posix_spawn` (GR-005), Zircon's launcher built on process/VMAR/VMO primitives (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence for:** Matches the data: 2,325 vfork-style children in core runs all exec and map to spawn (driver 2); the one program that forks gets a gated facility. `posix_spawn` costs about 0.5 ms regardless of parent size (GR-005). NT's attribute list extended for 20 years without breaking callers (GR-005). Fuchsia shows a launcher built from low-level primitives works at scale (GR-005). The gating keeps CP-05 for native code and makes the X-2 cost one visible facility.
- **Evidence against:** Fork emulation is the hard, slow part in every port: Cygwin (GR-005), Xok's 6 ms fork (`alternatives-postmortem.md#exokernel-mit-aegis--xok`). The snapshot must copy or share the parent's handle table, which with move-only handles (CP-02) means duplicating every handle; what a duplicated channel endpoint means for the child is a design question Zircon never had to answer (DM-022, `[unverified]` on cycles). Under strict commit (CP-14) each dash fork commits dash's writable pages; 764 forks per configure. The spawn API needs a parameter for every piece of state (DM-001, GR-005 cost).
- **Cost:** *(estimate)* Kernel: process-from-parts calls (the same calls the system library uses anyway) plus one snapshot call with CoW mapping and handle-table duplication, a few hundred to a few thousand lines. Library: `posix_spawn` as a direct wrapper; `fork` as the snapshot plus `libposix` state fix-up (pid from the broker, fd table already copied). Performance: spawn constant-time; fork proportional to dash's mapped pages, unmeasured. Compatibility: everything measured in T0–T2.
- **Precedent:** NT (CreateProcess; fork-free since 1993, Cygwin emulation slow), Fuchsia (no fork; Starnix emulates it for Linux binaries, `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`), KeyKOS (POSIX guests on a fork-free kernel with mixed performance, `alternatives-postmortem.md#keykos--eros--capros-and-coyotos`).

### Option C: Native spawn only; no fork facility; a shell that runs subshells without fork

- **Description:** As Option B for spawn, but the kernel never snapshots an address space. The shell is restructured so that subshells, command substitution and builtin pipeline elements run either in-process (with saved and restored shell state) or by re-executing the shell binary with its state serialised into the child. Non-shell programs are unaffected (driver 1).
- **Evidence for:** Keeps CP-05 absolute and CP-14 simple: no CoW facility, no commit for copies, no handle-table duplication semantics to define. The data confines the change to one program (`task-02/conflict-impact.md#x-2-…`, "In: spawn-only native creation, with fork confined to the shell", variants 2 and 3). The heredoc writer is already avoidable: dash uses `memfd_create` in W4 (`task-02b/callsite-analysis.md#3-…`, restructure 10 lines). dash's non-interactive external commands already moved to `posix_spawn` in 52 lines (M4). Windows ports of shells and PowerShell run without fork `[outside evidence: common knowledge; not in the research files]`.
- **Evidence against:** No fork-free POSIX shell was measured, and whether one can run autoconf `configure` unchanged is unknown (`task-02/conflict-impact.md#x-2-…`, Unknown). A subshell is semantically a copy of the whole shell state: variables, functions, traps, cwd, fd table, options, and the ability to exit without affecting the parent. In-process emulation must snapshot and restore all of it; re-exec must serialise it. `[assumption]` This is thousands of lines in dash or a new shell, and every divergence shows up as a configure script that behaves differently. 764 subshell-like forks per configure means the fidelity bar is high. mrsh and oksh have the same structure (`task-02/callsite-analysis.md#feature-1-…`), so switching shells does not help.
- **Cost:** *(estimate)* Kernel: least. Shell: a major restructure or a new shell, cost unmeasured and probably large; T2 (ADR-0005) depends on it. Risk: configure-script incompatibilities discovered late. Performance: subshells become cheaper than fork if done in-process.
- **Precedent:** No POSIX shell in the research files runs without fork. The Plan 9 shell `rc` uses `rfork` (`alternatives-postmortem.md#plan-9--inferno` mentions fork/exec was kept). Windows: no fork, no POSIX shell (Cygwin and MSYS emulate fork instead; GR-005).

### Option D: Two-level creation: empty process plus cross-process operations (Baumann et al. / ExOS-style), with fork composed from them

- **Description:** The native ABI exposes only low-level, cross-process operations: create an empty process in a job; map a memory object into it; install a handle into its table; set a thread's initial registers; start it. "Spawn" is a system-library function composed from these, and "fork" is the same composition with copy-on-write clones of the parent's memory objects and a copy of the thread state. There is no monolithic snapshot syscall. DM-001 lists this as an alternative ("cross-process syscalls that operate on an empty child before it starts (ExOS-style)"); Zircon's process builder works this way (GR-005).
- **Evidence for:** One mechanism serves spawn, fork, debuggers, checkpoint and Zygote-style preforking, which DM-001 and GR-005 say otherwise need a separate snapshot API. The kernel has no notion of "fork" at all, so there is nothing to gate: the authority to clone a memory object copy-on-write and to write another process's thread state is what needs a right (CP-02), and native code may legitimately hold it (a debugger). It avoids an open-ended spawn parameter set (DM-001 cost; NT's attribute lists, GR-005) because the library composes whatever it needs.
- **Evidence against:** Many small cross-process operations are a larger ABI surface than one spawn call, and each is a security-relevant primitive: writing another process's memory and registers must be capability-gated carefully (CP-02, DM-160). A fork built from N calls is slower than one syscall unless batched; Xok's slow fork came from being unable to share page tables, which is the same class of problem (`alternatives-postmortem.md#exokernel-mit-aegis--xok`). Fuchsia still ended up with a launcher *service* and job policy restricting direct construction (GR-005, DM-100), which is a sign that the raw primitives are not the interface most code should use. Atomicity: a half-built process must be unobservable to the job's other members until started.
- **Cost:** *(estimate)* Kernel: the primitives are the same ones Option B's spawn is built from internally, so similar size, but exposed and therefore versioned (CP-13). Library: a process builder. Fork: library-composed, possibly several syscalls per fork; 764 forks per configure make this measurable. Security review: each primitive.
- **Precedent:** Zircon (process_create, vmar_map, process_write_memory, thread_start; plus a launcher service), ExOS (research), Baumann et al.'s proposal (DM-001, GR-005).

## Recommendation

**Recommendation (not a decision): Option B as the ABI, implemented internally as Option D's primitives.** Concretely:

1. **The native primitive is `spawn`**, a single ABI call taking: job handle; image handle; argv vector; environment vector; an explicit handle list of (slot number, handle, rights to transfer, move or duplicate); start attributes: process group handle (CP-07, CP-25), `start_suspended`, and a small inherited-attribute block (ADR-0004 defines its contents; it carries "interrupts ignored" for background jobs). It returns a process handle. Nothing not listed is inherited (CP-05, DM-002). The kernel implements it from internal create/map/install/start steps but does not expose those as ABI in the first release; exposing them later for debuggers is a separate ADR (Option D stays available).
2. **`start_suspended` plus a `resume` call** replace the `vfork`-time child setup that `posix_spawn` cannot express (driver 3): the parent spawns suspended, performs group and terminal setup through the broker and the terminal handle, then resumes. This is what dash's interactive `vforkexec` needs (M4) and what Windows does with `CREATE_SUSPENDED` `[outside evidence: Win32 CreateProcess flag]`.
3. **Fork for `posix` jobs** is one kernel call, `snapshot_process`, gated by the job's `posix` policy bit (ADR-0002 rule 3(a)): it creates a new process in the same job whose address space is a copy-on-write clone of the caller's, whose handle table holds duplicates of every duplicable handle in the caller's table at the same slots (non-duplicable handles are absent in the child, and `libposix` must not depend on them), and whose single thread starts with the caller's register state and a distinguishable return value. It is exactly what dash's `forkshell` needs and nothing more. The `libposix` `fork()` wrapper then registers the child with the broker for a pid.
4. **Which call sites go where** (from M3/M4, `task-02/callsite-analysis.md#feature-1-…`): GNU make's `posix_spawn`, `execvp`-in-child, `pipe`, `wait` sites (8, `direct`) → `spawn`; pdpmake `system()` → `spawn` + wait (M4, 34 lines); dash `vforkexec` → `spawn` with `start_suspended` for the interactive path (restructure, about 60 lines); toybox xargs `XVFORK` → `spawn`; toybox `xfork`/`xpopen(argv==NULL)` → either `snapshot_process` (they are `posix` jobs) or the 40–50-line helper-mode restructure; dash `forkshell` (`emulate`) → `snapshot_process`; dash heredoc writer → `memfd` path (10 lines), so no process.
5. **`exec`** is `spawn` into a new process followed by exit of the old one, with the broker transferring the pid to the new process. `libposix` implements it this way; there is no in-place image replacement in the native ABI (Zircon has none either, GR-005). *Cost noted:* `exec` after `fork` therefore creates two processes; dash's 536 fork+exec children per run pay this. The non-interactive path already avoids it via `posix_spawn` (M4).

Why B over C: C's cost is unmeasured and lands on the T2 scope at its most fragile point (configure scripts); B's cost is one gated facility whose semantics are known (DM-001) and whose performance can be measured on W4 before committing to T2. C remains the right answer if that measurement fails; see revisit triggers. Why B over D as *ABI*: D exposes more security-relevant surface before there is a user for it (CP-13, X-10: an exposed primitive is permanent); B keeps the option by building on the same internals. Why not A: drivers 1, 4, 5 and 6.

**Principles this recommendation strains.** CP-05 ("there is no fork") is violated for `posix` jobs by rule 3, deliberately, as the X-2 tradeoff. CP-14 (strict commit) interacts with rule 3: each snapshot commits the parent's writable pages; the open question below measures it.

## Consequences

- **Positive.** Native code never sees fork, exec or implicit inheritance. Make, tcc, toybox and clang map onto `spawn` with `direct` changes. The one program that needs fork gets it, at a cost that is confined and measurable. `start_suspended` closes the `posix_spawn` gaps that M4 found.
- **Negative.** Two process-creation paths in the kernel (`spawn` and `snapshot_process`). Handle-table duplication semantics for snapshot must be defined per handle type. Fork emulation performance under strict commit is a known unknown that gates T2. `exec` costs a second process.
- **Follow-up work.** Define the inherited-attribute block (ADR-0004). Define per-type duplicability for snapshot in the `abi` crate. Measure snapshot cost on W4 (764 configure forks). Patch dash's interactive `vforkexec` to `spawn` + `start_suspended` as an M4-style calibration.

## Invariants

- **INV-0003-1.** `spawn` has no implicit inheritance: a child created with an empty handle list holds zero handles. *Check:* a kernel test that spawns with an empty list and enumerates the child's handle table.
- **INV-0003-2.** `snapshot_process` fails with a permission error in any job without the `posix` policy bit. *Check:* kernel test; the `abi` attribute of INV-0002-2 marks it.
- **INV-0003-3.** `spawn` takes argv and environment as vectors of byte strings, never as a single string. *Check:* the `abi` types `ArgVec` and `EnvVec`; a lint that no `spawn` parameter is a single string.
- **INV-0003-4.** Every process is created inside a job; `spawn` and `snapshot_process` require a job handle with the `create_process` right. *Check:* `abi` signature plus a kernel test that a process handle without a job cannot be obtained.
- **INV-0003-5.** A process created `start_suspended` executes no user instruction before `resume`. *Check:* a test that spawns suspended an image whose first instruction writes a shared memory object, waits, and asserts the write did not occur.
- **INV-0003-6.** The handle slots requested in a `spawn` handle list are the slots the child observes (needed for `MAKEFLAGS` fd numbers, ADR-0002 rule 7). *Check:* a test spawning with handles at slots 3 and 4 and reading them back in the child.
- **INV-0003-7.** `snapshot_process` never transfers a handle type marked non-duplicable in the `abi` crate; such slots are empty in the child. *Check:* a test per non-duplicable type.

## Open questions

- Memory and time cost of `snapshot_process` for dash under strict commit (X-6): measure on W4's 764 configure forks on the prototype. Threshold for "too slow" to be set by the architect.
- Semantics of a duplicated channel endpoint in the child after snapshot (both processes hold the same end): define per type; no evidence in the files (DM-022 lists the Zircon cycle question as `[unverified]`).
- Whether toybox's `xfork` applets (`cpio -p`, `tar`) are in scope at all (ADR-0005); if not, their restructure is not needed.
- Does any T1/T2 program depend on `exec` keeping the pid, apart from the shell's `exec` builtin? The broker's pid transfer covers the shell; nothing else was observed.

## Revisit triggers

- Measured `snapshot_process` cost makes W4 configure more than a small multiple slower than Linux `[assumption: multiple to be set by the architect]`: reopen Option C (fork-free shell) or a cheaper snapshot (shared page tables, as Xok lacked).
- A debugger, checkpoint or Zygote-style need appears natively: reopen Option D as exposed ABI.
- A T2 program outside the shell is found to fork without exec: rule 3's gating assumption (one program class) no longer holds.
- `exec`-as-new-process is measured as a problem for shells with many external commands: consider in-place image replacement.

## Architect's decision


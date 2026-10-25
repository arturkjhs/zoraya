# ADR-0005: Scope of POSIX compatibility

- **Status:** accepted
- **Date:** 2026-10-25
- **Depends on:** ADR-0002, ADR-0003, ADR-0004
- **Evidence:** CP-03, CP-13, CP-15, CP-16, CP-27, CP-28, X-1, X-2, X-3, X-6, X-10, DM-003, DM-019, DM-036, DM-059, GR-019, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#keykos--eros--capros-and-coyotos, alternatives-postmortem.md#l4-family-and-sel4, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#qnx, alternatives-postmortem.md#redox, alternatives-postmortem.md#sprite, task-02/callsite-analysis.md#feature-2-signals, task-02/callsite-analysis.md#feature-3-global-paths-path-cwd, task-02/callsite-analysis.md#feature-5-identity-and-permissions, task-02/callsite-analysis.md#feature-6-other-task-01-design-mistakes-in-the-data, task-02/conflict-impact.md#x-2-no-fork-cp-05-vs-hosting-the-c-toolchain, task-02/conflict-impact.md#x-4-completion-native-async-cp-08-vs-readiness-for-posix, task-02/libc-surface.csv, task-02/method.md#known-limits, task-02/method.md#pinned-inputs, task-02/method.md#tiers-runs-and-sets, task-02/posix-surface.md#comparison-programs-lower-bound-same-workload, task-02/posix-surface.md#jumps-between-tiers, task-02/posix-surface.md#key-observations, task-02/posix-surface.md#libc-symbols-per-tier-m1, task-02/posix-surface.md#linux-syscall-surface-m2, task-02/posix-surface.md#tiers-as-measured, task-02/research-log.md#workload-findings-that-are-results-not-failures, task-02/syscall-surface.csv, task-02b/parallel-surface.md#concurrency-actually-achieved, task-02b/parallel-surface.md#interrupted-system-calls, task-02b/parallel-surface.md#new-libc-functions-interposer-lower-bound

## Context

CP-27 says the POSIX layer exists to host a C compiler and its toolchain, that its target surface is written down, and that it changes only by decision. TASK-02 measured that surface by tier. This ADR decides which tier is in scope, writes the surface down by reference to the measured data, and lists what is explicitly out of scope.

**What is measured.**

- Tiers as measured (`task-02/posix-surface.md#tiers-as-measured`): T0 is dash plus toybox applets (W5 interactive session); T1 adds GNU make, tcc and busybox `diff` (W1 tcc self-build, W2 Lua, W3 SQLite); T2 adds further toybox applets and busybox `awk`/`expr`/`gzip` (W4 GNU hello `./configure && make`); T3 is clang + lld alone (W6). The normative applet list per tier is the generated `tools/posix-surface/results/applets-by-tier.tsv` (9 toybox applets at T1, 31 at T2); the prose summary's "25 more" differs from the TSV's 22 and the TSV is authoritative. pdpmake can build Lua and hello but not tcc, whose Makefile needs GNU make, so the smallest T1 is built on GNU make.
- libc surface per tier, minimal multi-call builds (`task-02/posix-surface.md#libc-symbols-per-tier-m1`):

  | Tier | Upper bound (imports) | Lower bound (observed) | Of musl's 1,613 exports |
  |---|---|---|---|
  | T0 | 164 | 84 | |
  | T1 | 248 | 134 | 8–15 % |
  | T2 | 291 | 189 | 12–18 % |
  | T3 alone | 331 | 89 | |

  Per `task-02/libc-surface.csv`, 164 symbols are first needed at T0, 84 at T1, 43 at T2; 161 at T3 only; 166 are imported only by applets no workload ran; 19 only by comparison programs.
- The T1→T2 jump adds 43 imports but 55 observed functions, because `configure` exercises paths the T1 programs already import. The new functions are mainly more stdio (`fprintf`, `getline`, `getdelim`, `*_unlocked`), temp files and directories (`mkstemp`, `mkdtemp`, `mkdir*`, `rmdir`, `symlink`), regex (`regcomp`/`regexec`/`regfree` from toybox grep and sed, busybox awk and expr), identity lookups (`getpwuid`, `getgrgid`, `getegid`), locale objects (`newlocale`, `uselocale`), process functions (`execvp`, `waitpid`, `dup`), `memfd_create` (dash) and `syscall` (toybox) (`task-02/posix-surface.md#jumps-between-tiers`; that file attributes `memfd_create` to toybox, but `task-02/libc-surface.csv` records it as imported by dash, clang and lld and called by dash in W4, and TASK-02b M7 agrees; the CSV is authoritative).
- Syscalls: T1 cumulative 49, T2 cumulative 59 distinct Linux syscalls (`task-02/posix-surface.md#linux-syscall-surface-m2`).
- **Fork volume is a T2 property.** Of dash's 1,169 no-exec forks in core runs, W4 accounts for 1,167: 764 in `configure` and 403 in make (`task-02/conflict-impact.md#x-2-…`, Unknown). T0 and T1 together (W5 and W1–W3) have 2 (`tools/posix-surface/results/fork-children.tsv`: one in W1-configure, one in W5). So ADR-0003's `snapshot_process` and its strict-commit cost (X-6) are almost entirely a T2 cost.
- **tcc + autoconf friction that is not about POSIX** (`task-02/posix-surface.md#key-observations`, item 7; `tools/posix-surface/results/configure-checks.md`): hello's `configure` needs a program called `ld` (libtool's `LT_PATH_LD`) and an `ar`; toybox has none and busybox defconfig has none; with `LD=tcc AR="tcc -ar"` W4 passes (`task-02/research-log.md#workload-findings-that-are-results-not-failures`). Of 251 checks both a tcc and a gcc configure ran, 22 differ: C11/C23 features tcc lacks or reports differently (`static_assert`, `bool`, `max_align_t`, `stdckdint.h`, a C99-conforming `stdint.h`, `alloca` as a builtin, GNU C, `alignas` as macros), tool choices (`install`, `mkdir -p`, `sed`, `grep`, `ar`, `ranlib`, `cpio` for ustar), and one platform fact: `config.guess` reports `aarch64-unknown-linux-gnu` under tcc. None blocked the build.
- **Threads separate T3 from T1–T2.** No thread is created in T0–T2; ld.lld creates 4 in W6. tcc calls `sem_init`/`sem_post`/`sem_wait` for an internal lock that synchronises nothing because no second thread exists (`task-02/posix-surface.md#jumps-between-tiers`, `#key-observations` item 3).
- **Never used in T0–T2** (`task-02/libc-surface.csv`, `tier` column; `task-02/posix-surface.md#linux-syscall-surface-m2`; `task-02/callsite-analysis.md#feature-6-…`): `ptrace` is imported by no measured program at all; System V IPC (`msgget`, `semget`, `shmget`) and `flock`, `setitimer`, `setresuid` are `unused-applets` (imported only by busybox applets no workload ran); `select` is `unused-applets`; `poll`, `socket`, `getaddrinfo`, `shm_open`, `sigaltstack`, `mmap` (as a libc call from program code), `alarm`, `pthread_*` are T3-only; `fcntl` record locks are imported by GNU make for `--output-sync` and never called; `dlopen` is imported by make (`load`) and tcc (`-run`) and never called (`task-02/method.md#known-limits`); `MAP_SHARED` appears twice, in clang; `tcsetattr` is `comparison` (oksh, mrsh line editing). `memfd_create` is imported by dash at T0 and called in T2 (W4).
- **Set-uid support** is 12 `direct` sites at startup of the full toybox/busybox binaries, removable by the config switch the minimal builds already use (`task-02/callsite-analysis.md#feature-5-identity-and-permissions`).
- **Parallel builds add no imports**: all six newly observed functions were already in the T1 static bound (`task-02b/parallel-surface.md#new-libc-functions-interposer-lower-bound`). pdpmake has no real `-j` (`task-02b/parallel-surface.md#concurrency-actually-achieved`), so parallel builds require GNU make.
- **Comparison programs** (`task-02/posix-surface.md#comparison-programs-lower-bound-same-workload`): pdpmake uses about half of GNU make's dynamic surface (30–31 vs 61–66 functions) but cannot build tcc; dash has the smallest interactive-shell surface of the four shells (54 vs 58–73).

**What is assumed.** The bounds are tied to W1–W6 on one Alpine aarch64 VM; static bounds overestimate and dynamic bounds underestimate (`task-02/method.md#known-limits`). Other autoconf projects may exercise more than hello's `configure`. No measurement covers programs beyond the listed set.

**Forces from TASK-01.** Every alternative OS that wanted real software built a compatibility layer, and pressure to widen it recurs with every useful program that does not port (CP-27). Plan 9 spent an estimated 90–95 % of its effort on external standards (`alternatives-postmortem.md#plan-9--inferno`); Sprite's small team lost to moving compatibility targets (`alternatives-postmortem.md#sprite`). Fuchsia's source-level "POSIX Lite" deliberately omitted global-state calls such as `kill`, was not enough for important software, and forced the Starnix binary-compatibility project (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). L4 systems run a whole Linux VM instead of a personality (`alternatives-postmortem.md#l4-family-and-sel4`). KeyKOS legacy guests ran but got no capability benefit (`alternatives-postmortem.md#keykos--eros--capros-and-coyotos`). Redox explicitly does not aim at full POSIX and is still not self-hosting after ten years (`alternatives-postmortem.md#redox`).

## Decision drivers

1. **Scope is a decision, not a drift** (CP-27; Sprite, Plan 9). The surface must be enumerable from data.
2. **T1 is small and bounded**: 134–248 functions, 49 syscalls (`task-02/posix-surface.md#key-observations`, item 1).
3. **T2 is where autoconf lives, and autoconf is how third-party C software is built.** T2 adds 43 imports and 55 observed functions, mostly ordinary file and string work (`task-02/posix-surface.md#jumps-between-tiers`). The T2 programs are the same binaries plus more applets.
4. **T2 is also where fork volume and emulation cost live**: 1,167 of 1,169 no-exec forks (`task-02/conflict-impact.md#x-2-…`). Accepting T2 commits to ADR-0003's `snapshot_process` performing on configure scripts.
5. **The tcc-specific friction is about C standards and tool names, not the OS** (`tools/posix-surface/results/configure-checks.md`); it is paid regardless of scope, and it is solved by `LD=tcc AR="tcc -ar"` and by wrapper names in the namespace table (ADR-0002 rule 5).
6. **Threads, networking, `ptrace`, System V IPC, record locks, `dlopen` and shared mappings are absent from T0–T2** (what is measured). Excluding them is supported by data, not by preference. DM-019 and DM-036 class SysV IPC and POSIX record locks as mistakes anyway; DM-003 and DM-059 would make `ptrace` and `pthread_cancel` expensive to emulate faithfully.
7. **Exclusions must survive pressure** (CP-27): each needs a written reason and a revisit trigger, so that re-opening is a decision too.

## Considered options

### Option A: Full Linux ABI compatibility (baseline, "do what Linux does", Starnix-shaped)

- **Description:** Aim to run unmodified Linux binaries: the full syscall surface, threads, signals with handlers, `ptrace`, sockets, SysV IPC, `/proc`. The personality grows toward Starnix (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence for:** Fuchsia's experience says source-level subsets are not enough once important software cannot be recompiled (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). A full ABI removes the recurring scope debate by answering "yes" once. Redox's incremental path has taken ten years without reaching self-hosting (`alternatives-postmortem.md#redox`), which argues for either going all in or staying very small.
- **Evidence against:** The data shows 8–18 % of musl is needed for T1–T2 (driver 2, 3); the rest is cost without a measured user. Starnix's hard parts (signals, futexes, inotify) are still being worked on after five years with Google's resources (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). CP-27 and Sprite: a small team chasing the Linux ABI loses. Full fidelity would force ADR-0004 Option D (asynchronous upcalls) and more kernel facilities under ADR-0002 rule 3.
- **Cost:** *(estimate)* An order of magnitude more personality code than T2; several new kernel facilities; permanent compatibility obligation (CP-13, X-10). Compatibility: highest.
- **Precedent:** Starnix (active, unverified performance), WSL1 (cited by the Starnix RFC as a performance warning), MINIX 3 and Redox (POSIX-targeting, stalled or slow).

### Option B: T1 only (self-hosting C without autoconf)

- **Description:** The personality's written surface is the T1 set: the 248 imported / 134 observed functions, the 49 syscalls, dash, GNU make (serial and `-j`), tcc, and the toybox/busybox applets observed in W1–W3. autoconf `configure` is out of scope; third-party software is built with hand-written makefiles or pre-generated headers.
- **Evidence for:** Smallest surface with a measured user (driver 2). Almost no fork emulation: 2 no-exec forks in T1 runs (driver 4), so ADR-0003's `snapshot_process` is nearly idle and X-6 is moot. No regex, no `getpw*`/`getgr*` lookups, no locale objects, no `mkdtemp` (`task-02/posix-surface.md#jumps-between-tiers`). It matches CP-27's wording ("host a C compiler and its toolchain") most literally. Lua and SQLite, two real third-party projects, built at T1 (`task-02/posix-surface.md#tiers-as-measured`).
- **Evidence against:** Most third-party C software ships `configure`; without T2 "hosting existing software" means porting each project's build by hand, which is the moving target Sprite lost to (`alternatives-postmortem.md#sprite`). The T1→T2 delta is modest and ordinary (driver 3): 43 imports, mostly stdio and file functions. The pressure CP-27 warns about would start on day one.
- **Cost:** *(estimate)* Lowest personality cost. Hidden cost: per-project build porting, unbounded.
- **Precedent:** Fuchsia's POSIX Lite is the nearest: a deliberately partial, source-level surface that proved insufficient (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option C: T1 + T2 (autoconf `configure` support), enumerated and staged

- **Description:** The written surface is the T2 set: 291 imported / 189 observed functions, 59 syscalls, the T2 program list, with T1 as the first acceptance gate and T2 as the second. Each tier's surface is the corresponding rows of `task-02/libc-surface.csv` and `syscall-surface.csv`. T3 and everything in the exclusion list below are out.
- **Evidence for:** Drivers 3 and 5: T2 is the step that makes third-party C software buildable as shipped, and its delta is ordinary library work. The friction that remains is tcc's C standard coverage and tool names, which the OS cannot fix and which did not block the build. Parallel builds add nothing to the surface (what is measured). The data on fork volume (driver 4) makes the cost of T2 visible and measurable in advance: `snapshot_process` on 764 configure forks.
- **Evidence against:** T2 is 18 % of musl and 59 syscalls, so "fixed and small" is strained (CP-27). The fork-emulation performance risk (X-6) is concentrated here and unmeasured. hello is one small autoconf project; larger ones may exercise more (what is assumed). toybox's file-metadata applets (`chmod`, `chown`, `cp -p`, `install`, `id`, `ls -l`) are `emulate` by definition and bring POSIX modes and owners into scope (`task-02/callsite-analysis.md#feature-5-…`), which ADR-0002 rule 6 handles as advisory metadata.
- **Cost:** *(estimate)* T1 cost plus 43 functions, regex (available as a library), passwd/group file parsing, locale objects, FIFO nodes (ADR-0006), and the fork path exercised at volume. Compatibility: autoconf projects of hello's complexity.
- **Precedent:** QNX hosts conventional software through its POSIX library (`alternatives-postmortem.md#qnx`); Redox builds autotools projects through relibc `[assumption: inferred from its ported-software list; not stated in the research files]`.

### Option D: No personality; run the toolchain in a Linux VM (L4/seL4 precedent)

- **Description:** Do not host POSIX natively. Run a Linux guest under a hypervisor component and build software there; the native system consumes build outputs. "Virtualised Linux became the de facto OS middleware" for L4 systems (`alternatives-postmortem.md#l4-family-and-sel4`); KeyKOS ran UNIX as a guest (`alternatives-postmortem.md#keykos--eros--capros-and-coyotos`).
- **Evidence for:** Zero personality code; full compatibility at once; the X-1/X-2/X-3 conflicts disappear because the capability system never meets POSIX. seL4's "incremental cyber-retrofit" pattern is deployed in practice and the postmortem rates L4's VM-based niche a success (`alternatives-postmortem.md#l4-family-and-sel4`). A VM is the strongest isolation boundary the research endorses: hardware address spaces held up well and a VM is a stronger form of the same thing (GR-019, CP-15).
- **Evidence against:** A VM needs a hypervisor, a virtual device set and a guest kernel: on QEMU `virt` that is nested virtualisation or a full emulator in user space, neither measured nor designed. The toolchain would not be self-hosting *on our system*, which is the stated purpose (CP-27). The guest gets no capability benefit at all, the KeyKOS outcome (`alternatives-postmortem.md#keykos--eros--capros-and-coyotos`). The native system still needs *some* way to run C-compiled programs, so the W^X and loader questions (CP-16) remain.
- **Cost:** *(estimate)* Hypervisor and virtio backends instead of a personality; unknown relative size. Compatibility: total, inside the guest only.
- **Precedent:** L4/seL4 (successful in embedded and defence), KeyKOS guests, Redox's own proposal to delegate hardware support to a Linux host (`alternatives-postmortem.md#redox`).

## Recommendation

**Recommendation (not a decision): Option C, with the surface written down as follows and the exclusions below.**

**In scope.**

- **Tier T1 (acceptance gate 1):** dash 0.5.13.5, GNU make 4.4.1 serial and `-j` with both jobserver styles, TinyCC (the pinned mob commit) compiling to files, the toybox 0.8.15 applets and busybox 1.38.0 `tr`/`diff` observed in W1–W3 and W5 (`task-02/method.md#pinned-inputs`, `tools/posix-surface/results/applets-by-tier.tsv`). Surface: the `T0` and `T1` rows of `task-02/libc-surface.csv` (248 symbols) minus the set-uid rows (`setuid`, `setgid`; see the exclusion table), plus `writev`, and the T1-cumulative rows of `task-02/syscall-surface.csv` (49 syscalls). Of the six functions TASK-02b observed (`kill`, `mkfifo`, `pselect`, `stpncpy`, `ttyname`, `writev`), five are in the 248; `writev` is `tier = T3` in the CSV because only the full toybox binary imports it, and it is added here as a T1 row on TASK-02b's evidence (toybox calls it under W8/W9, `task-02b/parallel-surface.md#new-libc-functions-interposer-lower-bound`). Workloads W1, W2, W3, W5, W2-j, W3-j, W8, W9, W10 are the acceptance tests.
- **Tier T2 (acceptance gate 2):** T1 plus the 25 toybox applets and busybox `awk`/`expr`/`gzip` observed in W4, with `LD=tcc AR="tcc -ar"` provided as wrapper names in the namespace table. Surface: add the `T2` rows (43 symbols, total 291) and the T2-cumulative syscalls (59). Workload W4 is the acceptance test. T2 is accepted only after `snapshot_process` is measured on W4 (ADR-0003 open question).
- The surface changes only by a superseding ADR that adds rows with a measured workload (CP-27).

**Out of scope**, each with the data that supports excluding it:

| Excluded | Data | Note |
|---|---|---|
| T3 (clang, lld) and threads | thread functions and thread clones appear only in T3; T0–T2 create no thread (`task-02/posix-surface.md#key-observations`, item 3) | tcc's `sem_*` (3 functions) stay in scope as single-threaded no-ops |
| `ptrace` | imported by no measured program (`task-02/libc-surface.csv`) | DM-003 and the broker's pid model make it costly anyway |
| System V IPC (`msgget`, `semget`, `shmget`) | `unused-applets` (`task-02/libc-surface.csv`) | DM-019 classes it a mistake |
| Sockets and name resolution (`socket`, `getaddrinfo`) | T3-only (`task-02/libc-surface.csv`) | CP-28 covers the native design later |
| `poll`, `select`, `epoll`, `ppoll` as general multiplexers | `poll` T3-only, `select` `unused-applets`, `ppoll` only in busybox sh's line editor (`task-02/conflict-impact.md#x-4-…`) | `pselect` on one fd stays in scope (ADR-0006) |
| Asynchronous signal handlers outside wait points | all measured deliveries were at waits (`task-02b/parallel-surface.md#interrupted-system-calls`) | ADR-0004 revisit trigger |
| Fault capture (`SIGSEGV` etc. handlers), `sigaltstack`, `tcc -run` | T3 crash handlers only; `-run` not exercised (`task-02/callsite-analysis.md#feature-2-signals`) | ADR-0004 rule 5 ends the process with reason `Fault` |
| `fcntl` record locks, `flock` | imported by make for `--output-sync`, never called; `flock` `unused-applets` (`task-02/callsite-analysis.md#feature-6-…`) | DM-036 classes POSIX locks a mistake |
| `dlopen`/`dlsym` | imported by make and tcc, never called (`task-02/method.md#known-limits`) | needed for `make load` and `tcc -run` only |
| `MAP_SHARED` file mappings | twice, in clang (`task-02/posix-surface.md#linux-syscall-surface-m2`) | anonymous private `mmap` is in scope |
| Set-uid, `setuid`/`setgid`/`setresuid`/`setresgid` | 12 `direct` sites, no-op drops, removable by config (`task-02/callsite-analysis.md#feature-5-…`); `setuid`/`setgid` are `T0` rows in the CSV only because the full busybox binary runs them at startup | CP-03; not exported by `libposix`; the minimal builds compile the call sites out, so these rows are struck from the tier list (ADR-0002 rule 6) |
| Job control beyond Ctrl-C and background jobs (Ctrl-Z, `SIGTTIN`/`SIGTTOU`, `SIGWINCH`) | not exercised by W5 (`task-02/method.md#tiers-runs-and-sets`) | specified to POSIX in ADR-0004 but not an acceptance test until measured |
| Line editing (`tcsetattr`, raw mode) | `comparison` only (oksh, mrsh) (`task-02/libc-surface.csv`) | dash has no line editor |
| `setitimer`, `alarm` | `unused-applets` / T3 (`task-02/libc-surface.csv`); oksh `TMOUT` is comparison-only | timers exist natively (ADR-0006) |

**Why C.** Option B's saving is 43 functions and the fork path, and its cost is every third-party build ported by hand, which is the unbounded, team-killing cost CP-27 describes. Option A buys fidelity the data does not ask for at the Starnix price. Option D abandons self-hosting on the native system. C keeps the surface enumerable, stages the one real risk (fork emulation at configure volume) behind a measurement, and excludes everything the data shows unused.

**Principle this recommendation strains.** CP-27 ("fixed and small"): 291 functions and 59 syscalls is small relative to musl (18 %) but not tiny; the mitigation is that the surface is a list of CSV rows, each with a workload that justifies it, and that growth requires an ADR.

## Consequences

- **Positive.** The surface is a file, not an argument. Autoconf projects of hello's shape build as shipped. Exclusions have written reasons and triggers, so re-opening is a decision.
- **Negative.** T2 commits the project to fork emulation performing on configure scripts (ADR-0003). Larger autoconf projects may need rows beyond hello's; each addition is an ADR. toybox's mode/owner applets bring advisory POSIX metadata into the filesystem service.
- **Follow-up work.** A generated `docs/adr/posix-surface-T1.txt` and `-T2.txt` from the CSVs as the normative lists (CI can diff them against what `libposix` exports). The W4 fork-cost measurement. A second autoconf project of larger size as a T2 robustness check (unmeasured today).

## Invariants

- **INV-0005-1.** Every symbol `libposix` exports is in the accepted tier list (the CSV rows of the accepted tier, minus the struck set-uid rows, plus `writev`) or in an accepted superseding ADR. *Check:* a CI script that generates the tier list from the CSV with those adjustments and diffs `libposix`'s export table against it; extra symbols fail the build.
- **INV-0005-2.** `libposix` exports no symbol from the exclusion table (`ptrace`, `msgget`, `semget`, `shmget`, `socket`, `getaddrinfo`, `pthread_create`, `dlopen`, `flock`, `setuid`, `setgid`, `setresuid`, `setresgid`, `sigaltstack`, `setitimer`, `alarm`, `epoll_*`) unless an accepted ADR adds it. *Check:* the same CI script with a deny list; the deny list and the tier list are disjoint by construction (the generator strikes the set-uid rows).
- **INV-0005-3.** Every function in the tier list is exercised by at least one acceptance workload (W1–W5, W2-j, W3-j, W8–W10 for T1; plus W4 for T2) on the prototype, or is marked `static-only` with the TASK-02 upper-bound justification. *Check:* a compile-time call-log feature in `libposix` (no dynamic loader or `LD_PRELOAD` needed) that records the first call of each exported function per process, aggregated over the acceptance runs and diffed against the tier list.
- **INV-0005-4.** The acceptance workloads' output checks (`W*-check`) pass on the prototype with exit status 0, and W9 ends with 130/143 and no partial targets. *Check:* CI runs the TASK-02/02b workload scripts against the prototype.

## Open questions

- A larger autoconf project (several thousand configure checks, libtool-built libraries) was not measured; it may add rows. Measure one before T2 is accepted.
- Whether `tcc -ar` and the `ld` wrapper suffice for autoconf projects that call `ld` directly: hello's probes (`W4-configure-noLD`, `W4-make-noAR`) are the only data.
- toybox `mktemp` prints a `/tmp` path (`emulate`, `task-02/callsite-analysis.md#feature-3-…`): confirm the per-session `/tmp` (ADR-0002) satisfies configure's uses.
- Which applets of toybox are the accepted set: `applets-by-tier.tsv` lists them; the architect may wish to add common ones (for example `find`, `xargs` beyond what W4 ran) without data.

## Revisit triggers

- An in-scope workload fails for want of an excluded feature: that row's reason is re-examined by ADR.
- `snapshot_process` is measured too slow on W4 (ADR-0003 trigger): T2 is re-staged or Option B becomes the accepted scope.
- The project wants to host a program that needs threads (for example a modern linker or compiler): T3 and the thread model become a new ADR.
- Pressure to add sockets for fetching sources: CP-28's native design first, then a personality mapping by ADR.

## Architect's decision

Accepted as recommended, 2026-10-25. Recorded by an agent from the architect's decision in docs/adr/index.md (commit 8c4ee71).

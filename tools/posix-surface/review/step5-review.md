# TASK-02 step 5: independent review of method.md and CSV spot-checks

Reviewer: separate subagent. I did not write the measurement scripts or the M3 reviews.
Date: 2026-10-24. Branch `research/task-02` at 8586bc3. I did not modify any repo or VM file except this one.

## Scope & seed

- Read: `docs/research/TASK-02-posix-surface-measurement.md`, `docs/research/task-02/method.md`, every script in
  `tools/posix-surface/` (`run.sh`, `remote/*.sh`, `remote/w5_session.py`, `analysis/*.py`, `interposer/gen.py`,
  `interposer/psurf_rt.c`), `results/summary-tables.md`, `review/m3-*.tsv`.
- Raw data: VM `lima-alpine-posix`, `~/psurf/raw/{dynamic,strace}`, `~/psurf/prefix/{bin,min}`, `~/psurf/build`,
  `~/psurf/src`. I confirmed that the VM's `results/*.csv`, `results/callsites.tsv`, `tools/analysis/analyze.py`,
  `tools/analysis/callsites.py` and `tools/interposer/psurf_rt.c` have the same md5 as the repo copies, and that
  `docs/research/task-02/{libc,syscall}-surface.csv` are identical to `tools/posix-surface/results/`.
- Row selection: Python `random.Random(20261024).sample(range(n), 20)`, applied separately to the data rows (header
  excluded) of each CSV. Row numbers below are 1-based data-row indices (data row 1 = file line 2).
  - `libc-surface.csv` (639 rows): 20, 64, 77, 89, 164, 179, 181, 197, 316, 347, 356, 409, 485, 515, 516, 525, 539, 573, 603, 628
  - `syscall-surface.csv` (68 rows): 3, 4, 6, 10, 22, 23, 25, 26, 33, 34, 36, 40, 42, 48, 50, 54, 56, 58, 66, 68
  - `callsites.csv` (650 rows): 20, 64, 77, 89, 164, 179, 181, 197, 316, 347, 356, 409, 485, 515, 516, 525, 539, 573, 603, 628
- Verification was independent of the pipeline's derived tables, using raw data only:
  - libc: a fresh script piped over ssh that runs `nm -D --undefined-only` on every file in `~/psurf/prefix/bin/*`,
    `~/psurf/prefix/min/*`, and clang/ld.lld plus their `ldd` closure. It then greps
    `~/psurf/raw/dynamic/*.tsv` for the symbol and maps `exe` and `argv[0]` to a program name.
  - syscalls: a fresh per-file count of `^<pid> <name>(` lines in `~/psurf/raw/strace/*.txt`. Argument patterns were
    recounted over core runs, with `wait4` recounted after joining unfinished/resumed lines. The program attribution
    was redone with my own exec/clone-parent walk.
  - callsites: `sed -n` around the line in `~/psurf/build/<tree>` (and `cmp` against `~/psurf/src`), with the
    enclosing function taken from `ctags -x` sorted by line.

## method.md findings

Severity: **error** = method.md claims something the scripts don't do, or a flaw that makes a published number or
completeness claim wrong; **imprecision** = wording or number not quite right, small effect; **note** = undocumented
limit or fragility, no number affected today.

### Errors

E1. **M3 misses executed call sites in toybox/busybox shared startup code (undocumented).** `analysis/callsites.py`
filters candidates by the DWARF line table of the *T2 minimal* builds (`toybox-T2`, `busybox-T2`, from
`allnoconfig`). The workloads ran the *full defconfig* binaries. Their startup code executes on every applet
invocation, but the minimal builds do not compile it (`CFG_TOYBOX_SUID`, busybox `FEATURE_SUID` are off).
`summary-tables.md` shows these calls dynamically (`busybox:tr/diff/awk/expr: setuid, setgid, getuid, getgid,
getopt_long`; `toybox:*: getuid`). Sources: `~/psurf/src/toybox/main.c:243` (`getuid()`, `geteuid()`, then
`setuid(uid)` at :247) and `~/psurf/src/busybox/libbb/appletlib.c:532` (`ruid = getuid()`) plus `check_suid()`
(setgid/setuid privilege drop). None of these sites is in `callsites.csv`; the only toybox identity rows are in
`cp.c`, `cpio.c`, `id.c` and `tar.c`. method.md documents this gap for the M1 minimal bound (lines 109–110) but not
for M3. Effect: feature 5 (identity/permissions, including the privilege-change `setuid`/`setgid` that X-1-type
arguments care about) is undercounted by roughly 5–8 executed sites. The DoD claim "M3 completed for every call
site … in T0–T2 programs" does not hold for this code. Fix: also scan these files with the full binaries' line
tables, or document the gap and add the sites by hand.

E2. **`tier` column of `libc-surface.csv` mislabels comparison-only symbols as `unused-applets`.** In
`analyze.py: sym_tier()`, a symbol in no tier's minimal bound or lower bound is labelled `unused-applets` whenever it
is in any full busybox/toybox import table. Only otherwise is it labelled `comparison`. A symbol that comparison
programs actually call, and that full busybox also happens to import, is therefore labelled as if nothing called it.
In the CSV, 8 symbols have dynamic evidence and are still labelled `unused-applets`: `clearerr`, `glob`, `globfree`,
`ppoll`, `putc_unlocked`, `sigsetjmp`, `system` (pdpmake in W2-make-p, W4p-*), and `tcsetattr` (busybox sh, mrsh
and oksh in W5). The label counts are therefore wrong: `unused-applets` should be 168, not 176, and `comparison`
should be 17, not 9. `tcsetattr` matters: the TTY discussion must not read it as "unused". method.md also never
defines the `tier` column or its values (`T0..T3`: first tier whose minimal upper bound ∪ lower bound contains the
symbol; `unused-applets`; `comparison`). Per-tier counts are not affected.

### Imprecisions

I1. **Compiler-inserted symbols counted as program API.** Alpine's gcc enables `-fstack-protector-strong` by
default, and `build.sh` does not turn it off, so every measured binary imports `__stack_chk_fail` (function) and
`__stack_chk_guard` (data). `static.py` treats them as libc API (they are not in `CRT`), and they get tier T0. They
inflate every tier's static upper bound by 2: T0 minimal 166 → 164, T1 250 → 248, T2 293 → 291; the full and T3
bounds are inflated too. The lower bound is unaffected. method.md says "only the API boundary: functions the program
itself calls" and lists only crt exclusions.

I2. **vDSO limit is overstated.** method.md (Known limits, strace) says `clock_gettime` "never enter[s] the kernel".
The traces contain 64 `clock_gettime` syscalls (62 `CLOCK_REALTIME`, 2 `CLOCK_MONOTONIC`), from make, lua, toybox
find/mktemp and oksh; `syscall-surface.csv` row 3 lists them. Some calls do reach the kernel, probably through the
vDSO's syscall fallback in this VM. The conclusion "time syscalls are under-represented" still holds, but the counts
are partial, not zero.

I3. **M3 basename matching can also produce false positives.** The Known-limits row says basename matching gives
"false negatives possible". Matching on basename can also *admit* a line from an uncompiled file that shares a
basename with a compiled one. Colliding basenames exist among the files in `callsites.csv`: mrsh
`shell/word.c`/`shell/task/word.c`/`parser/word.c`, `shell/trap.c`/`builtin/trap.c`; toybox
`lib/tty.c`/`toys/posix/tty.c`, `lib/env.c`/`toys/posix/env.c`; gmake `src/misc.c`/`src/w32/subproc/misc.c`. I
checked that every colliding file that has rows is actually compiled (an object file exists or the applet is
built), so no false positive was found. The wording should still be fixed.

I4. **Review merge key loses the feature.** `callsite_report.py` keys reviews by (program, file, line, api) without
the feature. Seven sites appear under two features: getlogin, lchown and ttyname under identity/tty and under
DM-039/DM-060. Both rows get the same review, and where a review file has two entries for the key, the later one
wins. For example, oksh `lex.c:1298 ttyname` and `main.c:469 getlogin` have effort 3 and 1 in `m3-oksh.tsv`, and
both rows end up with 1. The effort-sum effect is a few lines. Also, 147 of the 790 review keys match no extracted
site (stale entries from before the extractor fix); this is harmless but undocumented.

I5. **Class `n/a` is not defined in method.md.** 27 rows in `callsites.csv` have `class=n/a` (false regex matches
that reviewers marked, defined only in `review/M3-BRIEF.md`). method.md lists only direct/restructure/emulate.

I6. **The `signals` column of `callsites.csv` is a same-line regex, and its signal list is incomplete.**
`callsites.py: SIGNAME` omits SEGV, BUS, ILL, FPE, ABRT, TRAP and SYS, so tcc `tccrun.c:1377–1381` (sigaction for
SIGFPE/ILL/SEGV/BUS/ABRT) gets an empty `signals` value. A name inside a condition is also picked up:
`toybox lib/portability.c:456` is tagged `KILL` because the line says `!= SIGKILL`, which is the one signal that
does *not* get the handler. Overall, 105 of 136 feature-2 sites have no signal named. This weakens the "Signal sites
by signal name" table in `callsite-tables.md`, and the task asks "which ones". method.md does not describe how the
column is derived.

I7. **"Raw traces, about 226 MB".** `du` in the VM gives `~/psurf/raw` 240 MB and `~/psurf/raw-m4` 252 MB, about
490 MB in total.

I8. **Clean environment detail.** `SHELL` is always `$SETS/t1g/bin/sh` (`workloads.sh: run_wl`), also for t1p and
t3 runs, not `<set>/sh`. It is the same dash binary, so there is no effect.

### Notes

N1. **Exit-status validation is not scripted.** `dynamic-status.tsv` and `strace-status.tsv` are identical (I
compared them: all 25 runs match), but nothing asserts this. `analyze.py` only prints both lists.

N2. **Workloads that share a tree depend on order.** W1 runs `W1-make-pdpmake` and then `W1-make` (GNU make) in the
same tree. W4 runs `configure-noLD`, `configure-noAR` and `make-noAR` before `configure` and `make`. Today this is
harmless: pdpmake fails at once (`make: (Makefile:153): invalid macro assignment`, exit 2, 2 brk calls), and
`W4-make.log` shows a full recompile with 162 tcc lines. A future change that made pdpmake partly succeed would
silently shrink the core W1 measurement. method.md's W1 row does not mention the pdpmake attempt.

N3. **`bin-overrides.tsv` lists both t1p and t1g, and the Mapper keeps the last entry.** `/bin/make` therefore
always maps to gmake, even in t1p runs. No run execs `/bin/make` by absolute path, so there is no effect. The M4
sets (m4g, m4p) are not recorded, but they differ only in dash/pdpmake, which resolve to the same program names.

N4. **The `dlsym` dynamic column is empty by construction**, because it is never interposed. method.md's
Known-limits row cites "empty dynamic column" as evidence that no workload calls `dlopen`/`dlsym`. That holds for
`dlopen`, which is interposed, but not for `dlsym`.

N5. **Syscall per-tier distinct counts include products.** `tier_sc` in `analyze.py` counts every syscall in a core
run's trace, including built products (stage-2 tcc, lua, sqlite3, hello, conftest) and the T3 `W6-check` ./hello.
The M1 dynamic side excludes `built:`. The `programs` column shows `built`, so it is visible, but method.md M2 does
not say it.

N6. Checked and found correct, with no finding:
- trampolines save x0–x8 and q0–q7 and tail-branch, so stack args and variadics are untouched;
- the logger uses raw syscalls plus `dlsym` only, and `interposer.sh` enforces this with `nm`;
- the self-test covers the listed functions;
- caller filtering uses the `ld-musl` mapping range;
- the musl `aligned_alloc` pass-through is justified in `gen.py`;
- the static import-table method equals the object-file method for dash (`static-crosscheck.txt`: 101 = 101);
- the "dynamic ⊆ static" consistency check passes;
- vfork-before-return attribution in the strace parser is handled by the two-pass design;
- `src/` is read-only.

## Spot-check tables

### libc-surface.csv

For every row, the static column was recomputed from `nm -D --undefined-only` on all binaries and min builds, and
the dynamic column from `~/psurf/raw/dynamic/*.tsv`. Core-run fids collapse to `W<n>` as in `analyze.py`. Category
was checked against musl's declaring header and the override table; tier against the algorithm in
`analyze.py: sym_tier`.

| Row | Symbol | Field(s) checked | Raw evidence | Verdict |
|---|---|---|---|---|
| 20 | alphasort | static, dynamic, cat dir, tier | nm: only `busybox` full; no dynamic hits; dirent.h | OK |
| 64 | daemon | static, dynamic, cat, tier | nm: `busybox` only; no hits; unistd.h → override process | OK |
| 77 | endmntent | static, cat, tier T3 | nm: busybox, toybox (full), clang+libs, ld.lld+libs; not in any min build → first tier is T3; mntent.h → file | OK |
| 89 | execlp | static, dynamic, tier | nm: busybox only; no hits | OK |
| 164 | getentropy | static, tier T2, category | nm: toybox, toybox-T2, clang+libs, ld.lld+libs; no hits; tier T2 correct | OK (category `file` follows the rule because musl declares it in unistd.h, but `other`/random would be more natural) |
| 179 | getline | static, dynamic, tier T2 | nm matches; hits: mrsh in W5-mrsh; toybox:cpio in W4-configure, -noAR, -noLD, W4p-configure (`execve(".../t1g/bin/cpio", ["cpio","-o","-H","ustar"...])` in `strace/W4-configure.txt`) | OK |
| 181 | getlogin | static, dynamic, tier T1 | nm: gmake, oksh; hit: oksh W5-oksh | OK |
| 197 | getpwnam | static, dynamic, tier T0 | nm: dash, gmake, mrsh, oksh, toybox, toybox-T2; no hits | OK |
| 316 | nanosleep | static, dynamic, tier T0 | nm incl. toybox-T0/T1/T2; hits toybox:sleep in W4-configure (×3 variants), W4p-configure, W5-dash, W5-busybox, W5-mrsh, W5-oksh | OK |
| 347 | posix_memalign | static, dynamic, tier T3 | nm: clang+libs, ld.lld+libs; hits clang W6-compile/W6-link, ld.lld W6-link | OK |
| 356 | posix_spawnattr_setsigmask | static, dynamic, tier T1 | nm: gmake; hits gmake W1-make, W2-make-g, W4-configure, W4-make, -noAR, -noLD, W4-make-noAR | OK |
| 409 | raise | static, dynamic, tier T0 | nm: busybox, dash, clang+libs, ld.lld+libs; hits busybox W5-busybox, dash W5-dash | OK |
| 485 | shmdt | static, dynamic, tier | nm: busybox only; sys/shm.h → memory | OK |
| 515 | statvfs | static, tier T3 | nm: busybox, toybox (full), clang+libs, ld.lld+libs | OK |
| 516 | stderr | static (data), tier T0 | nm list identical (busybox-T2, toybox-T0..T2, gmake, mrsh, pdpmake, tcc, ...); data, so no dynamic | OK |
| 525 | strchrnul | static, dynamic | nm: busybox, dash; dash hits in 20 runs, W1/W2/W3/W4 core + 11 non-core fids exactly as listed | OK |
| 539 | strncmp | static, dynamic (10 programs) | every program/run pair matches; `built:` hits (W1-check ./tcc, W3-check sqlite3) correctly excluded | OK |
| 573 | system | static, dynamic, **tier** | nm: busybox, pdpmake; hits pdpmake W2-make-p, W4p-configure, W4p-make. Tier `unused-applets` although pdpmake (comparison) calls it | **MISMATCH** (tier label; see E2) |
| 603 | unlinkat | static, dynamic, tier T1 | nm incl. toybox-T1/T2; hits toybox:rm W1-configure, W4*, W4p* | OK |
| 628 | wcslen | static, tier T3 | nm: clang+libs, ld.lld+libs only; no hits | OK |

### syscall-surface.csv

Counts: my per-file `grep '^<pid> <name>('` count equals `call_count_per_workload` **exactly, for every workload,
in all 20 rows**. Programs: checked against my own exec/parent attribution (argv[0] of the attributed image).
Patterns: recounted over core runs.

| Row | Syscall | Field(s) checked | Raw evidence | Verdict |
|---|---|---|---|---|
| 3 | clock_gettime | counts (13 runs), programs | counts equal; attribution make 47, lua 4 (built), find 4, mktemp 7, oksh 2 → built;gmake;oksh;pdpmake;toybox | OK (see I2) |
| 4 | clone | counts (19 runs), kind/flags | counts equal; core kinds vfork 2328, fork 1710, thread 4 | OK |
| 6 | copy_file_range | counts, programs | counts equal; argv0 sed 264, install 16, cp 8 → toybox | OK |
| 10 | exit_group | counts (25 runs) | equal | OK |
| 22 | getgid | counts, `called=getgid(103)` | equal; core = 1 (W1-configure) + 101 (W4-configure) + 1 (W5-dash) = 103; argv0 tr/expr/diff/id/awk/mrsh/oksh/busybox | OK |
| 23 | getpgid | counts, pattern | equal; 1 per W5 shell; core 1 | OK |
| 25 | getppid | counts (20 runs) | equal | OK |
| 26 | getrusage | counts, programs | W5-oksh:16, argv0 oksh | OK |
| 33 | mkdirat | counts, path patterns | equal; core rel 15, abs 1, /tmp 1; argv0 mkdir, cpio, mktemp → toybox | OK |
| 34 | mmap | counts (25 runs), prot/flags top 8 | equal; 50472 / 3002 / 32 / 32 / 11 / 7 / 4 / 2 identical | OK |
| 36 | mremap | counts, programs | equal; all 34 by tcc | OK |
| 40 | openat | counts (21 runs), flags/path/prefix | equal; O_RDONLY\|O_LARGEFILE 30338, rel 19005, abs 13921, /usr 5886, O_WRONLY\|O_CREAT\|O_TRUNC 1317, … identical | OK |
| 42 | ppoll | counts, programs | W5-busybox:222, busybox | OK |
| 48 | renameat | counts, programs | equal; mv 311 (toybox), clang 1, ld.lld 1 | OK |
| 50 | rt_sigprocmask | counts (25 runs), how | equal; SETMASK 9861, BLOCK 6703, UNBLOCK 723 | OK |
| 54 | set_tid_address | counts (25 runs) | equal | OK |
| 56 | setpgid | counts, programs, `called(18)` | equal; dash 18, oksh 18, busybox 18, mrsh 27 | OK |
| 58 | sigaltstack | counts, programs | W6-compile 2, W6-link 4; clang 4, ld.lld 2 | OK |
| 66 | wait4 | counts (19 runs), opts | equal; after joining unfinished/resumed: 0 → 4030, WNOHANG 3307, WNOHANG\|WSTOPPED 9, WSTOPPED 7 (a naive single-line parse gives 690/3293, so the join matters and `analyze.py` does it right) | OK |
| 68 | writev | counts (24 runs) | equal | OK |

### callsites.csv

All 20 files are byte-identical between `~/psurf/build/<tree>` and `~/psurf/src/<prog>`.

| Row | Site | Field(s) checked | Raw evidence (source line / ctags) | Verdict |
|---|---|---|---|---|
| 20 | dash src/jobs.c:220 tcgetpgrp | api, function, class | `if ((pgrp = tcgetpgrp(fd)) < 0)` in `setjobctl` (@202); emulate plausible | OK |
| 64 | oksh c_ksh.c:123 strerror | api, function, class | `bi_errorf(..., strerror(errno))` in `c_cd` (@17); direct | OK |
| 77 | oksh c_sh.c:302 isatty | api, function, class | `if (isatty(fd))` in `c_read` (@250); direct. "what" says `read -p prompt?`; the ksh syntax is `read var?prompt` | OK (wording nit) |
| 89 | oksh edit.c:436 getpwnam | api, function, class | `getpwnam(tilde + 1)` in `x_tilde_expand` (@418); emulate | OK |
| 164 | oksh jobs.c:480 tcsetpgrp | api, function, class | `tcsetpgrp(tty_fd, j->pgrp)` in `exchild` (@382); emulate | OK |
| 179 | oksh jobs.c:680 killpg | api, function, class, signals | `(void) killpg(j->pgrp, SIGCONT)` in `j_kill` (@658); signals=CONT; emulate | OK |
| 181 | oksh jobs.c:682 strerror | api, function, class | `bi_errorf("%s: %s", cp, strerror(errno))` in `j_kill` | OK |
| 197 | oksh jobs.c:824 sigprocmask | api, function, class | `sigprocmask(SIG_BLOCK, &sm_sigchld, &omask)` in `j_njobs` (@818); restructure (SIGCHLD loop) plausible | OK |
| 316 | mrsh shell/redir.c:42 pipe | api, function, class | `if (pipe(fds) != 0)` in `create_here_document_fd` (@40); direct 2 plausible | OK |
| 347 | toybox lib/lib.c:795 fchown | api, function, class | `ignored = fchown(fd, statbuf.st_uid, statbuf.st_gid);` in `copy_tempfile` (@778); direct 1 | OK |
| 356 | toybox lib/portability.c:456 xsignal | api, function, class, **signals** | `if (signames[i].num != SIGKILL) xsignal(signames[i].num, handler);` in `xsignal_all_killers`; restructure OK. `signals=KILL` names the one signal that is excluded | **MISMATCH** (signals field; I6) |
| 409 | toybox toys/posix/cp.c:320 fchown | api, function, class | `else rc = fchown(fdout, try->st.st_uid, try->st.st_gid);` in `cp_node` (@154); emulate | OK |
| 485 | busybox archival/bbunzip.c:116 isatty | api, function, class | `... && isatty(STDIN_FILENO)` in `bbunpack` (@40); direct | OK |
| 515 | tcc tccelf.c:1741 getcwd | api, function, class | `getcwd (wd, sizeof(wd));` in `tcc_tcov_add_file` (@1726); direct 3 plausible | OK |
| 516 | tcc tccelf.c:2831 strerror | api, function, class | `tcc_error_noabort("could not write '%s: %s'", ..., strerror(errno))` in `tcc_write_elf_file` (@2817) | OK |
| 525 | tcc tccrun.c:1379 sigaction | api, function, class, **signals** | `sigaction(SIGSEGV, &sigact, NULL);` in `set_exception_handler` (@1366); restructure OK; `signals` empty because SEGV is not in `SIGNAME` | **MISMATCH** (signals field; I6) |
| 539 | gmake src/commands.c:600 kill | api, function, class | `if (kill (make_pid (), sig) < 0)` in `fatal_error_signal` (@484); direct 3 plausible | OK |
| 573 | gmake src/job.c:2674 strerror | api, function, class | `OSS (error, NILF, "%s: %s", new_argv[0], strerror (errno));` in `exec_command` (@2513) | OK |
| 603 | gmake src/output.c:522 strerror | api, function, class | `const char *err = strerror (errno);` in `pfatal_with_name` (@520) | OK |
| 628 | pdpmake main.c:108 strerror | api, function, class | `error("can't chdir to %s: %s", optarg, strerror(errno));` in `process_options` (@97) | OK |

No replaceability class in the sample is a clear misclassification.

## Summary counts

| CSV | Rows checked | OK | MISMATCH | UNVERIFIABLE |
|---|---|---|---|---|
| libc-surface.csv | 20 | 19 | 1 (row 573 tier label; systemic, 8 symbols) | 0 |
| syscall-surface.csv | 20 | 20 | 0 | 0 |
| callsites.csv | 20 | 18 | 2 (rows 356, 525: `signals` field only; api/line/function/class all correct) | 0 |

method.md: 2 errors (E1 M3 startup-code gap; E2 tier labelling and the undefined `tier` column), 8 imprecisions
(I1–I8), 6 notes (N1–N6).

What would change a headline number:
- I1: every tier's static upper bound drops by 2 if the stack-protector symbols are excluded
  (T0 166 → 164, T1 250 → 248, T2 293 → 291).
- E2: label counts `unused-applets` 176 → 168, `comparison` 9 → 17. Per-tier counts are unchanged.
- E1: M3 feature-5 site counts for toybox and busybox go up by about 5–8 executed sites (getuid, geteuid, setuid,
  getgid, setgid in startup code).
- No lower-bound (dynamic) count, syscall count or syscall argument pattern was found wrong.

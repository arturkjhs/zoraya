# TASK-02b step 5: independent review

Reviewer: a subagent that did not write the TASK-02b scripts. Date: 2026-10-24. Nothing in the repo or the VM was
modified. The only file written is this one. Scratch scripts were run in the VM through `ssh ... python3 -` from
stdin, with nothing written to disk there.

## 1. Scope and seed

- **Read:**
  - `docs/research/TASK-02b-parallel-build-measurement.md`;
  - `docs/research/task-02b/{parallel-surface,event-timelines,callsite-analysis,research-log}.md` (also
    `conflict-impact.md`, where it repeats the claims checked here);
  - `tools/posix-surface/analysis/parallel.py`;
  - `tools/posix-surface/results/par/{timelines,parallel-tables}.md`.
- **Raw data:** read in the VM (`ssh -F ~/.lima/alpine-posix/ssh.config lima-alpine-posix`):
  - `~/psurf/raw-par/strace/*.txt|*.log`;
  - `~/psurf/raw/strace/W2-make-{g,p}.txt`;
  - `~/psurf/raw{,-par}/dynamic/*.tsv`;
  - `~/psurf/src/{gmake,dash}/src`.
- **RNG:** Python `random.Random(20261024)`, used for both samples.
  - **syscall-delta.csv:** `rng.sample(range(868), 20)` over the data rows. The selected file line numbers (header =
    line 1) are **21, 65, 78, 90, 182, 198, 317, 348, 517, 526, 540, 574, 604, 629, 666, 668, 672, 745, 762, 788**.
  - **callsites.csv:** a new `Random(20261024)`, `rng.sample(range(150), 10)`. The selected file lines are
    **6, 17, 21, 24, 47, 51, 80, 88, 136, 145**.
- **Independent counting:**
  - **Syscall counts.** A regex counts `^pid [ts] name(` lines; `<... resumed>` lines are not counted. Files per
    label follow `parallel.py` COMP (lines 286–308), with `*-check` excluded.
  - **Timelines.** Read directly from the raw lines with `awk` on the `-ttt` clock, relative to the `HARNESS t=` line
    of the `.log`.
  - **Stability.** A separate per-process event extractor was run on 4 other W9 runs.

## 2. Timeline findings (task 1)

### W9a Ctrl-C, `W9-int-d1.5-r1` (t0 = 1792873598.440737, `write VINTR`)

Verified against the raw trace:

- **SIGINT from the tty.** `SI_KERNEL` SIGINT goes to all 11 processes of the job: 7 makes and 4 `mktab 4`. The
  receipts span +0.045 to +0.121 ms. dash receives none.
- **Leaf makes** (26835–26838):
  - `pselect6(4, [3], NULL, NULL, NULL, {sigmask=[]})` returns `ERESTARTNOHAND`;
  - then `rt_sigaction(SIGINT, SIG_DFL)`, `close(3)`, `close(4)`;
  - then `newfstatat t4.h`, "Deleting file", `unlinkat("t4.h")`;
  - then `wait4` → `WIFSIGNALED SIGINT`;
  - then `getpid`, `kill(self, SIGINT)` (`SI_USER`), killed by SIGINT.
- **Top make** 26832 unlinks `/tmp/GMfifo26832` (+0.49 ms).
- **Exit order.** Leaves die at +0.9–1.0 ms. Level-1 makes reap (+0.9/+1.1), re-raise (+1.4) and die (+1.5). The
  top make reaps (+1.5/+1.7), re-raises (+2.0) and dies (+2.1).
- **dash:**
  - `wait4(..., WSTOPPED)` reaps make@top (`WIFSIGNALED SIGINT`) at +2.12 ms, then SIGCHLD `CLD_KILLED`;
  - `ioctl(10, TIOCSPGRP, [26828])` at +2.34;
  - `tkill(26828, SIGINT)` at +2.51, received as `SI_TKILL` at +2.61;
  - `MAKE_STATUS=130`.
- **kill calls:** 7 `kill(self, SIGINT)` plus 1 `tkill`. Make sends nothing to its children on SIGINT.
  `commands.c:539–545` forwards only on `sig == SIGTERM`.
- The "State when the interrupt arrives" section is correct: leaves in `pselect6` with an empty mask, top and
  level-1 in blocking `wait4(-1, ..., 0)`, 12/32 tables.

### W9b SIGTERM, `W9-term-d1.5-r1` (t0 = 1792873621.065848, `kill SIGTERM pid=1708`)

Verified:

- **Top make:** `wait4` `ERESTARTSYS` at +0.15 ms, SIGTERM `SI_USER` from the harness pid 1701 at +0.17, FIFO
  unlink at +0.28.
- **Sends:** `kill(1710=b)` at +0.33, `kill(1709=a)` at +2.17.
- **Level-1 makes:** each forwards SIGTERM to its 2 leaves at +2.2–2.3.
- **Leaves:**
  - `pselect6` `ERESTARTNOHAND`;
  - `kill(mktab)` at +2.38–2.74;
  - `unlinkat t4.h`, reap, `kill(self)`, death.
- **Totals:** 17 `kill` = 2 + 4 + 4 + 7.
- **dash:**
  - reaps at +4.39 ms;
  - writes `Terminated\n` to stderr (not mentioned in the doc; harmless);
  - `TIOCSPGRP` at +4.66;
  - no `tkill`;
  - `MAKE_STATUS=143`.

### W10, `W10-parallel-r1`

Verified:

- 3 forks, and `SIGINT`/`SIGQUIT` → `SIG_IGN` in each background child before exec;
- 2 `rt_sigsuspend([])` calls, each ending in `ERESTARTNOHAND`;
- two pipelines with `pipe2` ×2 and ×1;
- `yes`: `writev` → `EPIPE`, then `SIGPIPE {SI_USER, si_pid=22044 (yes itself)}`, killed by SIGPIPE;
- dash reaps it with `WIFSIGNALED SIGPIPE`; output `pipeline status: 0`;
- exit 0.

Times match the generated table within 0.1 ms.

### Errors and imprecisions found

**T-E1 (error): `wait4` in top and level-1 makes is never restarted.**

- The claim "blocked `wait4` returned `ERESTARTSYS`" is **correct**.
- The claim that it was then "restarted (`SA_RESTART`)" is **wrong**. In all 18 W9 strace runs, each of the 3
  pids with an interrupted `wait4` makes **no `rt_sigreturn` afterwards** and ends `killed by SIG*`. Check:
  `awk` over `W9-*.txt`, recording a pid on `wait4 resumed>.*ERESTARTSYS` and then looking for a later
  `rt_sigreturn` from it. Result: 3 pids per run, 0 later `rt_sigreturn`, 3 killed.
- What happens instead: the handler (`fatal_error_signal`) issues **new** `wait4(-1, ..., 0)` and
  `wait4(..., WNOHANG)` calls (`reap_children`, `commands.c:573`). It never returns, because it re-raises at
  `:600`.
- **Wrong in:**
  - `event-timelines.md` line 55 ("restarted after the handler started reaping") and line 144 (stability row "`wait4`
    restarted");
  - `parallel-surface.md` line 138 ("restarted because of `SA_RESTART`");
  - `conflict-impact.md` line 61 ("as restarts under `SA_RESTART` (`wait4`, …)") and line 99 ("the latter
    restarted, `SA_RESTART`").
- **Effect on conclusions.** The emulation requirement in `conflict-impact.md` should read: a blocking `wait4` is
  interrupted, and the handler never returns to it. A restart of `wait4` under `SA_RESTART` is not exercised. The
  only real `SA_RESTART` restart observed is `writev` (see T-E2).

**T-E2 (error): the wrong signal is blamed for the `writev` interruption.**

- The claim: "In 4 of 9 runs a leaf make was inside `writev` when SIGTERM arrived" (`event-timelines.md` line 96;
  same in `parallel-surface.md` line 138, `conflict-impact.md` lines 58–59, `research-log.md` line 121).
- What all 4 occurrences show: `W9-term-d0.3-r2` (pid 32567), `W9-term-d0.3-r3` (909), `W9-term-d1.5-r1` (1713),
  `W9-term-d3.0-r2` (5017).
  - The writev is the **"Deleting file" message inside the SIGTERM handler**.
  - It is interrupted by **SIGCHLD `CLD_KILLED` from the `mktab` that the same make had killed about 0.07 ms
    earlier**.
  - The SIGCHLD handler then returns and the `writev` is re-issued. Example from `W9-term-d1.5-r1`: `1713 kill(1734,
    SIGTERM)`, then `writev(2, "…Deleting file 't4.h'…") = ? ERESTARTSYS`, then `--- SIGCHLD {CLD_KILLED,
    si_pid=1734}`, then `rt_sigreturn`, then `writev … = 34`.
- SIGCHLD can arrive there because the handler runs with `pselect6`'s empty mask: it was entered from `pselect6`.
- This also explains the 0/9 for Ctrl-C. There, the tty kills the recipes at the same moment as the make, so their
  SIGCHLD arrives before the handler writes anything.
- The representative run used in the doc's W9b table, `W9-term-d1.5-r1`, is one of the 4. Its table does not show
  the interruption.

**T-E3 (error): "4 partial targets deleted in 36/36 runs" and "one timing-dependent event kind" are false.**

- `W9-term-d3.0-r2` (strace) has **3** "Deleting file" messages. `results/par/parallel-tables.md` itself shows `3`
  in its W9-outcomes row.
- What happened in that run:
  1. Leaf make pid 5015 reaped `mktab 7`.
  2. It **started a new recipe 9.7 ms after the harness SIGTERM**: `clone` → 5054 at .779515, execve `/bin/sh -c
     './mktab 8 > t8.h'`.
  3. The forwarded SIGTERM reached 5015 only at .779769.
  4. 5015 killed the `sh` before it had opened `t8.h`. `newfstatat t8.h` returned ENOENT, so there was nothing to
     delete.
  5. The log shows `[../../leaf.mk:57: t8.h] Terminated` without a "Deleting" line.
- The generated stability table lists `recipe-sh receives SIGTERM` and `recipe-sh killed by SIGTERM` as 1/9 (at
  3.0 s), but `event-timelines.md` and `research-log.md` ("every W9 event kind except the two below") omit them.
- **Wrong in:**
  - `event-timelines.md`, stability table: "every make deletes the partial target… all 9" for SIGTERM, and "4
    'Deleting file' messages" in the 36-run sentence;
  - `parallel-surface.md`: "4 per run";
  - `conflict-impact.md` line 56: "4 partial targets deleted".
- **Effect on conclusions:**
  - "No partial target left behind" and "no process left" still hold in 36/36.
  - It is new and relevant for X-3: with SIGTERM to the top make only, propagation takes milliseconds, and in that
    window lower makes keep **starting new jobs**. This race is absent for Ctrl-C, where the tty signals the whole
    group at once.

**T-I1 (imprecision): W9b row times.**

- The row "+3.6 … +3.9 make@a and make@b reap their leaves, re-raise, die" is too late for the reaps. The reaps are
  at +3.1, +3.2, +3.3 and +3.5. Re-raise and death are at +3.6 to +3.9.
- The row "+4.2 … +4.4 make@top reaps both" has the same problem. The top make reaps at +3.8 and +4.0, re-raises at
  +4.2 and dies at +4.4.

**T-I2 (imprecision): W9a row "+0.1 … +0.4 … each leaf make receives SIGCHLD".** For leaf a/1 the SIGCHLD comes at
+0.5 ms, inside the handler, after its `unlinkat`.

### Stability check on other runs

`W9-int-d0.3-r2`, `W9-int-d3.0-r3`, `W9-term-d0.3-r1` and `W9-term-d3.0-r1` were checked with an independent
per-process extractor. Every "all 9" behaviour in the doc's table holds in all four:

- the tty sends SIGINT to every recipe and every make (int);
- SIGTERM is forwarded top → level 1 → leaves → recipes (term);
- each of the 4 leaves unlinks `tN.h` (t1.h at 0.3 s, t7.h at 3.0 s);
- all 7 makes re-raise and die by the signal;
- `pselect6` `ERESTARTNOHAND` in the 4 leaves;
- `wait4` `ERESTARTSYS` in the 3 upper makes;
- the shell does `TIOCSPGRP`, plus `tkill(SIGINT)` in the int case only;
- `mktab` had already exited normally at 3.0 s and not at 0.3 s.

The exceptions are T-E2 and T-E3 above.

## 3. syscall-delta.csv spot-check (task 2)

All 20 rows reproduce exactly from the raw traces. The files follow COMP, and serial means and parallel means
match to one decimal.

| line | syscall | workload | serial files → counts | parallel files → counts | CSV (s / p) | match | notable_args |
|---|---|---|---|---|---|---|---|
| 21 | mmap | W2 gmake -j2 pipe | W2-make-g → 2003 | W2j-g-pipe-j2-r1,r2 → 2021, 2021 | 2003 / 2021 | yes | — |
| 65 | openat | W2 gmake -j2 fifo | 2003→1910 | 1913, 1913 | 1910 / 1913 | yes | plausible; see J-I1 |
| 78 | writev | W2 gmake -j2 fifo | 296 | 296, 296 | 296 / 296 | yes | — |
| 90 | getppid | W2 pdpmake -j2 | W2-make-p → 40 | 40, 40 | 40 / 40 | yes | — |
| 182 | wait4 | W2 gmake -j4 fifo | 40 | 134, 135 | 40 / 134.5 | yes | WNOHANG 253 + opts=0 16 = 269 = 134 + 135, ok |
| 198 | lseek | W2 pdpmake -j4 | 808 | 808, 808 | 808 / 808 | yes | — |
| 317 | getdents64 | W3 gmake -j4 pipe | W3j-g-j1-r1,r2 → 4, 4 | 2, 2 | 4 / 2 | yes | — |
| 348 | getdents64 | W3 gmake -j4 fifo | 4, 4 | 2, 2 | 4 / 2 | yes | — |
| 517 | newfstatat | W8 gmake -j4 pipe | W8j-g-j1 → 423, 423 | 339, 339 | 423 / 339 | yes | — |
| 526 | set_tid_address | W8 gmake -j4 pipe | 111, 111 | 111, 111 | 111 / 111 | yes | — |
| 540 | fcntl | W8 gmake -j4 fifo | 256, 256 | 284, 282 | 256 / 283 | yes | F_SETFD 311 etc. are totals over the 2 runs (see S-N1) |
| 574 | exit_group | W8 pdpmake -j4 | W8j-p-j1 → 79, 79 | 79, 79 | 79 / 79 | yes | — |
| 604 | geteuid | W10 dash | W10-serial → 5, 5 | 7, 7 | 5 / 7 | yes | — |
| 629 | clock_gettime | W9 Ctrl-C 0.3 s | W8j-g-fifo-j4-r1,r2 → 7, 7 | W9-int-d0.3-r1..3 → 7, 7, 7 | 7 / 7 | yes | — |
| 666 | write | W9 Ctrl-C 0.3 s | 25, 17 | 9, 9, 9 | 21 / 9 | yes | token(9) = 3 × 3, ok |
| 668 | brk | W9 Ctrl-C 1.5 s | 229, 229 | 95, 95, 95 | 229 / 95 | yes | — |
| 672 | close | W9 Ctrl-C 1.5 s | 535, 534 | 217, 217, 217 | 534.5 / 217 | yes | — |
| 745 | tkill | W9 Ctrl-C 3.0 s | 0, 0 | 1, 1, 1 | 0 / 1 | yes | SIGINT->self(3), ok |
| 762 | getdents64 | W9 SIGTERM 0.3 s | 30, 30 | 30, 30, 30 | 30 / 30 | yes | — |
| 788 | write | W9 SIGTERM 0.3 s | 25, 17 | 9, 9, 9 | 21 / 9 | yes | token(9), ok |

Also checked against the CSV, all correct:

- every entry of the "New syscalls" table in `parallel-surface.md`;
- `rt_sigaction` 1698/1698 and `rt_sigprocmask` 592/592 (W8 `-j4`);
- `fcntl` 256 → 344 (pipe) / 283 (fifo);
- `wait4` 40 → 134.5–146 (W2) and 78 → 225.5/232 (W8).

**S-N1 (note).** In `notable_args`, the per-pattern counts are **sums over all parallel repetitions**, while
`serial_count` and `parallel_count` are **means**. For example, `write,W2 gmake -j2 pipe` has 35 against
`token(70)`. Neither the CSV nor `parallel-surface.md` says so.

## 4. Jobserver and libc checks (task 3)

Recounted in `W8j-g-fifo-j4-r1`, `W8j-g-fifo-j4-r2`, `W2j-g-pipe-j4-r1` and `W8j-g-pipe-j4-r1`. Each line below
gives my count, then the value in `parallel-tables.md` / `parallel-surface.md`.

- **Token reads** (`read(fd, "+", 1) = 1`, plain and `<... read resumed>, "+", 1) = 1`):
  - W8 fifo -j4: 25 / 17, against [25, 17], ok;
  - W8 pipe -j4 r1: within [26, 18], ok;
  - W2 pipe -j4: 38, ok.
- **Token writes:** 25 / 17, 38 and 26, ok.
- **`pselect6` calls (interrupted):**
  - W8 fifo: 85 (55) and 72 (57), ok;
  - W2 pipe -j4: 56 (21), ok;
  - W8 pipe: 100 (60), ok.
- **Argument shape:** every `pselect6` is `pselect6(4, [3], NULL, NULL, NULL, {sigmask=[], sigsetsize=8})`, ok.
- **EAGAIN token reads in W8 fifo -j4:** 8 + 1 = 9, ok ("9 in two runs").
- **mknodat:** exactly one `mknodat("/tmp/GMfifo<pid>", S_IFIFO|0600)` per fifo run, none in pipe runs, ok.
- **Auth strings:**
  - fifo: `fifo:/tmp/GMfifoN` ×110 per W8 run (220 over 2), ok;
  - pipe: `3,4` ×110 and `-2,-2` ×104 per W8 run, matching "`3,4 then -2,-2` ×208; `3,4` ×12", ok;
  - W2 pipe: `3,4` ×42 and `-2,-2` ×38 per run, matching 76 + 8 over 2, ok.

Issues found:

- **J-I1 (imprecision): the FIFO open flags differ between makes.** `parallel-surface.md` says "openat read side
  `O_RDONLY|O_NONBLOCK`, write side `O_WRONLY`, in every make".
  - Raw `W8j-g-fifo-j4-r1`: `GMfifo` opened `O_RDONLY|O_NONBLOCK` ×1 (top make only), `O_RDONLY` ×6 (the sub-makes)
    and `O_WRONLY` ×7.
  - The sub-makes make the fd non-blocking afterwards with `fcntl(F_SETFL, O_RDONLY|O_NONBLOCK)` (14 over 2 runs = 7
    per run).
  - W2 is the same: 1 `O_NONBLOCK` open and 2 plain opens per run.
- **J-I2 (imprecision): "bad stdin" pipes are not one per make.** The doc says "one extra `pipe2` per make". The
  extra flags-0 `pipe2` calls are: W2 fifo +1 per run (3 makes), W3 +1 (1 make), W8 `-j4` fifo +5.5 (7 makes), W8
  `-j2` fifo +1. So it is at most one per make, created lazily, and only in makes that start a job while another
  runs.
- **J-I3 (imprecision): the W9 extras come from the interactive shell.** "W9 vs a complete W8 `-j4`: … `setpgid` 4,
  `getpgid` 1, `readlinkat` 2" and "What this adds" #5 ("Interruption adds … `setpgid`/`TIOCSPGRP`") attribute
  these to interruption. In `W9-int-d1.5-r1`:
  - `getpgid`/`setpgid` are dash job control at start-up, in the pre-exec child, and at exit;
  - `readlinkat("/proc/self/fd/1")` and `readlinkat("/proc/self/fd/2")` are make's `ttyname`, because stdout is a
    pty;
  - `TIOCSPGRP` happens for every foreground job.

  These are consequences of running under an interactive shell on a pty (the baseline W8 is not), not of the
  interrupt. The same applies to `ttyname` in the "new libc" table.
- **J-N1 (note): the W8 `-j2` explanation is loose.** It says "four leaf makes each hold an implicit slot", but the
  measured concurrency at `-j2` is 2. The token count (2) is right.

**libc table.** I recomputed per-program sets, keyed by the binary basename in `raw{,-par}/dynamic/*.tsv`. The
result is identical to the doc:

- gmake: getpid, kill, mkfifo, open, pselect, signal, sscanf, strsignal, ttyname, waitpid, write;
- dash: signal, stpncpy, strsignal;
- pdpmake: chdir;
- toybox: writev;
- new to TASK-02 altogether: kill, mkfifo, pselect, stpncpy, ttyname, writev (TASK-02 total: 267 distinct
  functions).

If `*-check` runs are included on both sides, dash adds `lstat` and toybox adds `ispunct`, but nothing new
overall. The "already in the static imports" claim is confirmed with `readelf --dyn-syms`:

- gmake imports kill, mkfifo, pselect and ttyname;
- dash imports stpncpy;
- toybox imports writev.

## 5. M7 spot-check (task 4)

Each sampled site was checked against `~/psurf/src/{gmake,dash}/src`, including the enclosing function found by
scanning back for a definition.

| csv line | site | source at line | function ok | what ok | class |
|---|---|---|---|---|---|
| 6 | gmake commands.c:568 delete_child_targets | `delete_child_targets (c);` | fatal_error_signal, ok | ok | direct, ok |
| 17 | gmake job.c:477 sigprocmask | `sigprocmask (SIG_BLOCK, &fatal_signal_set, 0)` | block_sigs, ok | ok | restructure, ok |
| 21 | gmake job.c:491 sigprocmask | `sigprocmask (SIG_SETMASK, &empty, 0)` | unblock_all_sigs, ok | ok | direct, ok |
| 24 | gmake job.c:1037 unblock_sigs | `unblock_sigs ();` (children-chain comment) | reap_children, ok | ok | restructure, ok |
| 47 | gmake main.c:1278 FATAL_SIG | `FATAL_SIG (SIGINT);` (macro at 1263–1267: keep SIG_IGN if ignored) | main, ok | ok | restructure, ok |
| 51 | gmake main.c:1307 bsd_signal | `(void) bsd_signal (SIGCHLD, SIG_DFL);` | main, ok | ok | direct, ok |
| 80 | gmake posixos.c:297 fd_noinherit | `fd_noinherit (job_fds[1]);` | jobserver_parse_auth, ok | ok | direct, ok |
| 88 | gmake posixos.c:396 read | `EINTRLOOP (r, read (job_fds[0], &intake, 1));` | jobserver_acquire_all, ok | ok | direct, ok |
| 136 | dash redir.c:380 signal | `signal(SIGHUP, SIG_IGN);` | openhere, ok | ok | restructure, ok |
| 145 | dash trap.c:289 sigaction | `sigaction(signo, &act, 0);` | setsignal, ok | ok | emulate, ok (consistent with M3) |

There are no misclassifications in the sample. I also checked the line references `commands.c:534` (jobserver_clear),
`:544` (`kill(c->pid, SIGTERM)` inside `if (sig == SIGTERM)`), `:573` (`reap_children (1, 0)`) and `:600`
(`kill (make_pid (), sig)`). They are correct.

## 6. Summary

| id | severity | finding | where | changes a conclusion? |
|---|---|---|---|---|
| T-E1 | **error** | Interrupted `wait4` in top and level-1 makes is never restarted: 0 `rt_sigreturn` in 54 of 54 cases over 18 runs. The handler issues new `wait4` calls and dies by re-raise. | event-timelines L55, L144; parallel-surface L138; conflict-impact L61, L99 | Yes, the emulation requirement in conflict-impact ("`wait4` … restarted, `SA_RESTART`"). Restarting `wait4` is not exercised. |
| T-E2 | **error** | The `writev` `ERESTARTSYS` in 4/9 SIGTERM runs is caused by SIGCHLD from the child the make had just killed, during the "Deleting file" message inside the SIGTERM handler. It is not "when SIGTERM arrived". | event-timelines L96–97; parallel-surface L138; conflict-impact L58–61; research-log L121 | It refines the cause: a nested SIGCHLD inside the fatal handler, possible because the handler inherits `pselect`'s empty mask. |
| T-E3 | **error** | `W9-term-d3.0-r2` has 3 deletions, not 4. A leaf make started a new recipe 9.7 ms after the SIGTERM, before the forwarded signal reached it, and killed the recipe's `sh` before the target existed. The generated stability table's `recipe-sh … 1/9` rows are omitted from the docs. | event-timelines stability table and 36-run sentence; parallel-surface "4 per run"; conflict-impact L56; research-log "except the two below" | Partly. "No leftovers" still holds. New for X-3: SIGTERM propagation leaves a window in which sub-makes keep spawning jobs. |
| T-I1 | imprecision | W9b row times for the level-1 and top reaps are 0.4–0.5 ms too late. | event-timelines W9b table | no |
| T-I2 | imprecision | W9a: leaf a/1's SIGCHLD comes at +0.5 ms, not within +0.1…+0.4. | event-timelines W9a table | no |
| J-I1 | imprecision | FIFO read side: only the top make opens it `O_NONBLOCK`; sub-makes open `O_RDONLY` and then use `fcntl F_SETFL`. | parallel-surface, Jobserver bullet | no |
| J-I2 | imprecision | "Bad stdin" adds at most one `pipe2` per make, only in makes that run jobs concurrently, not one per make. | parallel-surface | no |
| J-I3 | imprecision | W9's `setpgid`/`getpgid`/`readlinkat`/`TIOCSPGRP`, and `ttyname`, come from the interactive shell and the pty, not from interruption. | parallel-surface "New syscalls" W9 row, "What this adds" #5 | No, but #5 overstates what interruption itself adds. |
| S-N1 | note | `notable_args` counts are sums over repetitions; the count columns are means. Undocumented. | syscall-delta.csv | no |
| J-N1 | note | The "implicit slot" explanation for W8 `-j2` does not match the measured concurrency of 2. | parallel-surface token table | no |

These all reproduce exactly: the 20 sampled syscall-delta rows, the jobserver numbers, the libc table, the 10 M7
sites, W10, and the W9a/W9b sequences apart from the items above.

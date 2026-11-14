# Handoff to the next session (2026-11-14)

Written at the end of the session that ran TASK-26 stage 8's close, TASK-27 stage 9 (9a to 9f, 9g-url), TASK-29 (the
test and CI audit), TASK-30 (C++) and `work/test-fixes`. Everything below was read from the repository, GitHub and
the session's reports on 2026-11-14; a number with no source here is in the report named next to it.

## 1. Main and its CI

- `main` is at the commit that holds this file (see `git log -1 origin/main`). The last commit before the handoff
  commits is `d0335c8` (the `test-map` fix).
- **The last full green run on `main` is `cb93cc4`** (run 43591537102, the merge of `work/cxx`; 69 jobs, about
  67 minutes). `stage-cxx` is on it. Before it: `7b02798` (the `getrandom` merge, run 43580446459, green; `stage-9f`).
- The pushes after `cb93cc4` are `9917bc0` (merge of `work/test-fixes`), `05bc71e` (backlog, a lane for the three C++
  steps) and `d0335c8`. `9917bc0` and `05bc71e` were pushed while `make check` failed (`check-test-lanes`: three lines of
  `tools/test-map.txt` did not name the C++ steps); `d0335c8` fixes it. Their runs are superseded. **The run on the head
  of `main` is the first item of the next session**: read `gh run list --branch main`.
- `main`'s runs cancel each other: a push cancels the run in progress (the concurrency group). Do not push twice in
  a row while a run you need is going.
- CI is slow: a candidate or main run waits in the queue 10 to 70 minutes before its first job (free-plan
  concurrency, shared with the other machine's candidates). A full run is 66 to 70 jobs and 30 to 67 minutes
  after it starts (the audit's change gave 44 to 31 minutes at its best, see `docs/research/test-audit/report.md`).
- Known flakes and how they stand:
  - `test-python` `SSLError 0xa080024`: not a flake; `getrandom` returned EIO in a threaded process. Fixed in
    `work/py-roots` (merged `7b02798`).
  - `wheel:auto:2` (and the scale 1 run): races between the tablet's click and the keyboard under TCG; fixed twice
    in `tests/gui/wheel.script` (a wait on the terminal's focused title bar before typing).
  - `thread_thread_condition_notify_all_at_thread_exit_lwg3343`: was a real bug (thread-exit order), fixed
    (`__cxa_thread_atexit_impl`); under TCG the upstream test trips its own counter, so it is left out under TCG only
    (`tests/cxx/libcxx-tcg-excluded.txt`, evidence in `docs/tasks/TASK-30-report.md`).
  - `thread.semaphore/lost_wakeup` takes the runner's whole limit under CI's TCG: libc++'s atomic wait polls here,
    it does not use the futex (backlog follow-up).
  - `cpu_ceiling_from_a_process` (`t_cpu.rs:181`) failed once under `make test-changed`'s parallel load (backlog).
  - CI downloads from upstream servers can fail; the ports archive cache (9f-ci) makes a hit not need the Internet.
- Local `make test-changed` now fails or prints `SKIPPED: STEP: REASON` for a step it selects and does not run
  (`tools/test-lanes.txt`, `make check-test-lanes`). `test-python` and `test-python-regr` print SKIPPED unless
  `PAR_LONG=1`.
- The gate is closed: `room new --untrusted` is refused (`tools/untrusted-gate.txt`, `tools/check-untrusted-gate.sh`)
  and nothing from outside the image has run as untrusted.

## 2. Tags

`stage-7a` to `7h`, `stage-8` (and `stage-8-q1` to `q7`, `n1` to `n6`), `stage-9`, `stage-9a` to `9d`, `stage-9f`,
`stage-cxx`. There is no `stage-9e` (9e was merged inside stage 9). `candidate/*` has 145 tags and
`candidate-full/stage-8`; they are build records and can be deleted by the architect.

## 3. Branches, candidates and agents

Merged into `main` (their branches can be deleted): `work/9a`, `9b-libc`, `9b-locks`, `9c`, `9d`, `9e`, `9f-ci`,
`9f-text`, `9f-wheel`, `9f-net`, `9g-url`, `wheel-exit`, `py-roots`, `tls-options`, `cpu-flake`, `texec-fix`,
`ci-docs`, `cxx`, `test-audit`, `test-fixes`, `adr/stage-10`, `docs/9f-final`, `docs/stage9-final`,
`docs/stage9-status`.

Not merged, on purpose:
- `work/ci-arm`: the arm64-runner experiment. Result in the TASK-27 report: no KVM, 1.4 to 1.7 times slower, nothing
  moves. The architect said not to merge it.
- `docs/review-9f-net`, `docs/review-9f-tw`, `docs/review-9g-url`: the independent reviews (one file each under
  `docs/tasks/`), merged into `main` with this handoff.

Agents: none is needed. Every agent of the session has finished; the last `ci-watch` is stopped with this handoff.
Candidate tags last used: `candidate/work-test-fixes-a` (green, merged), `work-cxx-i` (green, merged),
`work-9g-url-b` (green push run on the same head, merged), `work-test-audit-c` (green, merged).

Worktrees still on the disk (remove what is not needed; each holds `target/` and `build/`):
`../zoraya-rev`, `../zoraya-rev2`, `../zoraya-rev3`, `../zoraya-mrg`, `../zoraya-rv-*` (detached review trees), and, under
the `os` clone, `.claude/worktrees/agent-*` of stage 8 (some locked). Disk was 90 GB free at the last look; the
rule is 25 GB before a build.

## 4. What waits for the architect

Decisions and records:
- **Proposed ADRs of stage 8:** ADR-0280, ADR-0281, ADR-0285, ADR-0286 (all other ADRs through ADR-0367 are accepted).
- **INV-0324-2's wording:** it says "no rule but a web rule is within a web rule", which ADR-0328 makes false (a
  public-only name rule is within a web rule). The proposed text is in the TASK-27 report, section "9g-url", "For the
  architect". The invariants row stays `checked` and is true only of the proposed text.
- **INV-0326-1's wording:** its check covers only the grant that comes with `window` (a rule-6 note in ADR-0326 says
  so); the rewording is the architect's.
- **ADR-0366 and its index row** say 51 added names; the list now holds 52 (`design.md` and the report say 52).
- **Native `fetch`** keeps the old name rule (a rebinding can reach the LAN); should it follow ADR-0328 as curl and
  wget now do?
- **curl and wget refusal text:** a refused name shows `curl: (6) Could not resolve: NAME:PORT` and wget's
  `failed: Permission denied`; neither hints at `+net` (ADR-0224 item 5's hint is not built).
- **TASK-31** (stage 10's implementation, `docs/tasks/TASK-31-stage-10.md`, from `docs/research/stage-10/plan.md`):
  the architect's file appeared in the worktree at the end of the session and is committed with this handoff. It
  starts after `stage-cxx` (now tagged) and `work/test-fixes` (merged); it was **not started**. It reserves ADR numbers
  0343 to 0349 and 0380 to 0399 (free ADR numbers otherwise: 0351 to 0359, 0368 to 0379). Notes already in the
  backlog: part 10c takes the Ctrl-C defect on a continued job; part 10h closes
  CPython's `random` seeding in `os.fork` children and the nested `python3` failure (ADR-0339 item 7), with a test that
  `os.urandom` works in an `os.fork` child.
- **Licences:** confirmed (wget GPL-3.0-or-later as a separate program, SQLite public domain, OFL-1.1 for unmodified
  font files, ADR-0327); `CLAUDE.md` says so.

Manual checks nobody has done: `netsurf https://example.org/` from a desktop terminal, `python3 -c 'import sqlite3, ssl,
asyncio'` and `ping google.com` in the guest.

Known weak spots (each in the TASK-27 report or `docs/backlog.md`): NetSurf renders a Wikipedia article's prose below
the lead as white space (cause not diagnosed: CSS support or a layout bug; `netsurf -v` on an article would show it),
SVG and WebP images are empty frames, CJK is boxes, no JavaScript; record locks are in the process only (across
processes stays in the backlog until stage 10); curl's, wget's and NetSurf's own non-TLS `rand` use was not read;
CPython's `random` can still seed from the time and pid in an `os.fork` child; libc++'s atomic wait polls instead of
using the futex; the compositor's wheel for native programs other than the four of 9f-wheel is not done; a restarted
shell gets no window channel until the panel restarts; the run machines keep 512 MiB and no smaller size has been
measured.

Two automated security warnings ("instruction poisoning") came with two subagents' notices. Their tool calls and diffs
were read; nothing outside the task was found (the one ADR status change was the architect's own decision, relayed).

## 5. What remains of the TASK-29 audit

Source: `docs/research/test-audit/report.md` (findings V1 to V45, F, T, C, D) and `docs/backlog.md` ("work/test-fixes").
The audit's own commits fixed: D1 to D3 (ADR-0350: a run and its `tcg` twin run once where `auto` is TCG), T2, T3, V41,
V1, F5, V22, V6, V7, V8, V14, V20, V21, V34, V29 and the review's R1 to R13. `work/test-fixes` (first item) made a
listed step that is not run fail or print `SKIPPED`. Open:

1. **The remaining vacuous-test findings:** every V number of the report that is not in the fixed list above: V2
   (no host-side check of which kernel tests ran), V3 (`--expect-panic NAME` accepts any panic), V5, V9 to V13, V15 to
   V19, V23 to V28, V30 to V33, V35, V36 (`tests/posix/smoke.script`, `make test-posix-long` and
   `make posix-cancel-points` are in no CI list while INV-0110-1, INV-0110-2 and INV-0005-3 say `checked`), V40,
   V42 to V45. Make each fail when the behaviour is absent and show it by hand (apply the break, see the failure,
   revert, say so in the commit).
2. **V4, the `test-crash` check:** it passes when no round saw a `crash: durable` line (`tools/test-runner/src/crash.rs`).
   A floor needs the time to a round's first sync, or two local rounds can fail it.
3. **The `t_quota` handshake:** replace the fixed-sleep ordering at `user/src/bin/t_quota.rs:436` and `449` with a
   test-only handshake.
4. **The `libc.a` relink in the build job (T1):** in 21 of 21 builds `libc.a` was relinked because it depends on
   `libzoraya_posix.a` (`ports/Makefile:174`), which cargo builds anew each job, so curl, wget and NetSurf are relinked
   and Python rebuilt every time; the ports cache key hit in 6 of 21. The fix is in the report's T1 (a stamp on
   the content, or restoring the library with the cache).
5. **The stale `not yet applicable` rows (C1)** and **`adr-check`'s `checked:` rule (V40):** 55 rows of
   `docs/adr/invariants.md` say `not yet applicable (stage 6/7/8/9)` for code that exists (INV-0172-3 and -4,
   INV-0220-1 and -2, INV-0224-1 to -5, INV-0228-2 and -3, INV-0230-1 to -4, among others); `adr-check`
   (`tools/adr-check/src/main.rs:373`) accepts any non-empty text after `checked:` (INV-0042-1's row reads "by type and
   review; no command fails if it is broken"). Make `checked:` name a command that exists in the Makefile.
6. **Checks for INV-0172-4 and INV-0230-1 (C2, C3):** INV-0172-4 (a room holds only what its creator held) has no
   test (the refusal is at `user/src/bin/zosh.rs:3646-3658`); INV-0230-1 (tests reach no outside network) is unchecked
   (seven recipe lines pass `--net-open`, `tools/run-qemu.sh:286-287`; assert the command line).
7. **Also from the audit and the merge agents:** the plan rarely narrows a candidate (T4: `Makefile` maps to every
   step, now explicit), a minute and a half of overhead per test job (T5), `tools/test-map.txt`'s `tests/gui/` line
   triggers `test-gui` for the hand-run Wikipedia scripts.

## 6. Process notes that cost time

- A command chain that commits or pushes must stop at the first failure (`CLAUDE.md`). Two pushes to `main` went out
  while `make check` failed because the chain printed the failure and went on.
- Agents decline to set an ADR status when the instruction reaches them relayed through the orchestrator. The architect's
  decisions were pasted by the architect into the orchestrator session; the orchestrator recorded them itself (or
  re-instructed the agent with the exact words). Do the same.
- Several `ci-watch` agents started overlapping poll loops; give them one loop, a 6-minute poll and a limit.
- Run a candidate on a tag `candidate/NAME` on the branch head; a push run on `work/*` is the same workflow but is not
  a candidate by the rules.

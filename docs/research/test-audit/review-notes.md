# TASK-29 part 2: independent review

Reviewed: `git diff origin/main...work/test-audit` at 83363480, commits c33b7bda (D1-D3, proposed ADR-0350),
f3497def (T3, T2, V41), 60a7322e (V1), 68dc9161 (F5, V22), f1d618f4 (V6-V8, V14, V20, V21), eabc091a (V34),
84f4c7df (V29). The part 1 documents (c513753a) were read only as the source of the findings.

Run here (no QEMU): `make check-ci-jobs`, `make check-ci-plan`, `make adr-check` (all pass);
`make print-legs` for FULL=0/1 and HOST_ACCEL=hvf/tcg; `tools/ci-plan.sh` with `CI_PLAN_PATHS` set to
`tests/gui/x`, `fuzz/x`, `unmapped/x`, `tests/shell/console.script`; `make check-ci-jobs CI_TESTS=<list>` on eight
altered lists under `build/`; `tools/check-ci-jobs.sh` with `MAKE` pointed at an altered copy of the Makefile;
`cargo test -p zoraya-test-runner` (see R13).

## Findings

### R1 (warning): INV-0350-2's check does not see a run dropped on a TCG host unless it is half of a pair

`tools/check-ci-jobs.sh:291-308`. For the TCG host the check only asks that the `auto` side of each
`QEMU_AUTO_TWINS` pair is gone, that its `tcg` side is there, and that no GUI run appears with its twin. It never asks
that everything else is still there. Shown: a copy of the Makefile whose TCG branch of `QEMU_RUN` also filters out
`auto-smp4-2g`, and whose `GUI_RUNS_ONCE` also drops `files-manage:auto:2`, `wheel:auto:2`, `windows:auto:2`,
`terminal:auto:1600x1000` and `terminal:auto:1920x1200`. `make print-legs FULL=1 HOST_ACCEL=tcg` then lists 4
configurations and 12 GUI runs, and `tools/check-ci-jobs.sh` with `MAKE="make -f <copy>"` passes. CI itself loses
nothing, because `tools/ci-tests.txt` is checked separately. What breaks is `make test FULL=1` on Linux, which is
what INV-0350-2 covers.
**Fix:** for the TCG host, require that `qemu:` equals `QEMU_CONFIGS` minus the `auto` side of each twin, and that
`gui:` equals `GUI_RUNS_ALL` minus each `S:auto:N` whose `S:tcg:N` is listed. Both are exact set comparisons, like the
ones the HVF branch already makes.

### R2 (warning): nothing keeps a twin's expected lines a superset of the dropped run's

`Makefile:259-283`. `test-qemu-tcg-default` now repeats `auto-default`'s three lines by hand (`cpus: 1`,
`memory: 131072 KiB total`, `console: pl011 ...`). `tcg-el2` already had `auto-el2`'s line. Today the sets are
right: I compared all four recipes. But if someone adds an `--expect-line` to `auto-default` or `auto-el2` later, CI
never runs it, and nothing reports that. ADR-0350 and the Makefile comment both say "the twin expects their lines
too", but no check makes that true.
**Fix:** put each `auto` configuration's lines in a variable (`AUTO_DEFAULT_EXPECT`, `AUTO_EL2_EXPECT`) that both
recipes of the pair use. Or have `check-ci-jobs` compare the `--expect-line` sets of `make -n test-qemu-<auto>` and
`make -n test-qemu-<tcg>` for each pair in `QEMU_AUTO_TWINS`.

### R3 (warning): `HOST_ACCEL` follows `ZORAYA_ACCEL`, but the `--accel auto` runs do not

`Makefile:52`, `tools/run-qemu.sh:94,128,167`. `HOST_ACCEL := $(shell tools/run-qemu.sh --print-accel)` passes no
`--accel`, so it reports `${ZORAYA_ACCEL:-auto}` resolved. Every recipe passes `--accel auto` explicitly, which
overrides the environment, so on a Mac those runs still use HVF. A Mac session with `ZORAYA_ACCEL=tcg` exported
therefore gets `AUTO_IS_TCG=1` under `FULL=1`, and runs are lost:
- the second TCG runs of `test-net`, `test-net-fetch`, `test-net-posix` and `test-apps` (`TCG_TOO=0`, while their
  `auto` run is HVF);
- the HVF runs of `auto-default`, `auto-el2`, the twelve paired GUI runs and the `LOCAL_ACCELS` loops.

The task's rule "never weaken a check on HVF" is broken on that path.
**Fix:** `HOST_ACCEL := $(shell tools/run-qemu.sh --accel auto --print-accel 2>/dev/null)`.

### R4 (warning): the dropped `net-tcg` run was not the same run as `net-auto`

`Makefile:365-370`. `run tcg --label net-tcg` booted `NET_DISK` after `net-settings-auto` had changed it. By then the
disk's `/config/settings` held `net.gateway = 10.0.2.2` and `net.dns = 1.1.1.1` (`tests/shell/net-settings.script`,
lines 20-29). `net-auto` boots a fresh disk. So the dropped run was the only one in which `t_net`/`t_netd` ran under
DHCP with a configured gateway and DNS on the disk. The coverage looks accidental, but it is still lost on CI.
Meanwhile ADR-0350's Context says "nothing else tells the two runs apart". (`net-boot-tcg` is the same run as
`net-boot-auto`. The pcap is checked only after `net-auto`, and no host check followed `net-tcg`, so the pcap
checks lose nothing.)
**Fix:** either record this in ADR-0350's Consequences (what is lost) and correct the Context sentence, or keep the
case on purpose with one run of the test kernel on the configured disk under `auto`.

### R5 (warning): F5 is correct, but whether the burst overflows in front depends on speed; the old failure was a script race

`tests/shell/console.script:212-224`, `user/src/bin/zosh.rs:2504-2558, 5029-5069`.
- **The new order is right.** `read_keys` queues `[input: N bytes dropped]\n` with `jobs.queue`. Only
  `take_notices` shows it: in the main loop just before `editor.start(prompt)`, or in `wait`/`fg`. The job's
  `fanout: interrupted (Ctrl-C)` is printed by `run_entry` before that. So `@ ... | fanout: interrupted`, then
  `< bytes dropped]`, then `< /> ` follow the code's order, and the prompt always comes right after the notice.
- **The count is reported once.** The script's `\x03` bytes enter QEMU's stdin pipe after all 8192 `x`. So the
  `read_keys` that reads the Ctrl-C that lands has already received or dropped every `x`, and its
  `console_dropped` covers them all. A repeated Ctrl-C comes after 200 ms into an empty ring, so no later notice
  can follow (`x bytes dropped` holds). `uptime` is sent only after the prompt, so it cannot go to the job.
- **The intent is tested again.** The burst now comes while the job is in front, after its own line. The step
  fails, with a timeout and not a silent pass, if no byte is dropped.
- **The old failure was not a defect.** Since TASK-11 the shell has queued this notice for the next prompt (the
  queue became `jobs.queue` in b99e4ba2, ADR-0158's notices). The old script passed only when the drops happened at
  the prompt, before the job started.
- **Flake risk left.** Nothing forces an overflow while the job is in front. On one CPU the spinning child holds the
  CPU and the 256-byte ring (ADR-0060) almost certainly fills. On the SMP legs `test-boot` runs
  (`boot-auto-smp4-2g`, `boot-tcg-smp4-gicv2`, `boot-tcg-smp4-gicv3`) the shell has a CPU of its own. While a job
  is in front it does less work per key (`type_key` and an echo) than the editor does at the prompt, where the old
  script's drops happened. I cannot bound this without QEMU.
- **Fix:** make the overflow independent of speed. Send a much larger burst (for example `= 65536`), or send it while
  the shell is busy and not reading the console. Then run the flood step about 20 times on the SMP4 TCG leg in one
  candidate before relying on it.
- **Also (pre-existing, same block):** `< up` after `> uptime\r` matches the echoed `uptime`, so it is vacuous. The
  builtin prints `up N.NNN s\n` (`zosh.rs:4220`). Use `< \r\nup `.

### R6 (warning): two of the three steps V1 wakes up still miss part of what they claim

`tools/test-runner/src/script.rs:591-601` (Quiet), `tests/shell/console.script:115-117`, `tests/posix/zosh.script:77`.
- **`~ MS TEXT` sees only bytes that arrive after the step starts.** It records
  `start = since_match().len()` and searches only after that point. Text that arrived after the last match, before
  the step began, is never seen: a prompt in the same read as the matched `tick`, or one before it.
  - At console.script:117, `fanout 1 tick leave` spawns the ticker, prints its line and returns at once
    (`user/src/bin/fanout.rs:34-37`), and the ticker prints `tick` as soon as it runs (`ticker.rs`).
  - With the defect (a prompt when the first process ends), the prompt can come before the first `tick` or in the
    same read. `< tick` then matches past it, and `~ 400 \r\n/>` passes.
  - **Fix:** add `x \r\n/>` right after line 116 `< tick`. It covers the text since the prompt of line 112, which
    holds only the echo and fanout's line. Better still, make Quiet search from the last match
    (`since_match()` whole). Every committed `~` use stays correct under that reading: I checked lines 113 and 120,
    files-write.script:175 and the tour's `~ 1000 zzz`.
- **The forbid at zosh.script:77 depends on exact line-end bytes.** `! tstp: continued\r\ntstp: continued` matches
  only if the two writes arrive with exactly `\r\n` between them (`t_posix.c:1007` writes `\n`; `zoraya-tty` applies
  `ONLCR`). A different line end makes it silent again. **Fix:** after line 96's `< tstp: continued`, add
  `o tstp: continued`. It counts the text and does not depend on line ends.
- **On correct behaviour all three steps pass**, so neither problem turns CI red:
  - console:117: no prompt while the ticker lives;
  - files-write:175: the shell waits for `sleep 60`, so only the echo arrives;
  - zosh:77: the handler prints `continued` once per continue, and fg continues once.
- files-write:175 is weaker than it reads: a prompt that comes more than 300 ms after the send is missed. That is
  acceptable, because the next step (`\x03` then `< sleep: interrupted`) catches the INV-0152-2 defect itself.

### R7 (warning): T3 also cuts fuzzing to 60 s when the change is the fuzz target or its decoder

`tools/ci-plan.sh:108-111`. The `sed` changes `FUZZ_SECONDS=600` to `60` on every candidate, including
`CI_PLAN_PATHS=fuzz/x` (the plan shows `make test-fuzz FUZZ_SECONDS=60 ...`). ADR-0268 point 3 makes "10 minutes
without a finding" the gate for a decoder an untrusted program reaches. ADR-0298 item 3 sets that as 600 s in "the
full CI list". Read literally, that does not cover candidates. But a part that adds or changes a target now merges
after 60 s, and the first 10-minute run happens on `main`, after the merge. The task allows a shorter run on
candidates for work "not about the change". A fuzz target or decoder change is about the change.
**Fix:** keep 600 s when a changed path is under `fuzz/`, `tools/run-fuzz.sh` or a crate a target fuzzes
(`plan-lib.sh` already has the changed paths). Or record the reading in a proposed ADR (0351) or in ADR-0298's open
questions.

### R8 (note): INV-0213-3's status no longer says what the check checks

`docs/adr/invariants.md:412`. The statement is "every script of `test-posix` under every accelerator", with
`checked: make check-ci-jobs`. On this branch the check accepts `tcg` in place of `auto`, so on CI the statement is
true only as ADR-0350 reads it, and ADR-0350 is proposed. The text cannot change (adr-check), but the Status column
is the agents' to keep true (CLAUDE.md).
**Fix:** use `checked: make check-ci-jobs (on CI once under TCG, read with proposed ADR-0350)`, and say in the
report that merging this branch before the architect decides ADR-0350 makes INV-0213-3 hold only under that reading.

### R9 (note): documents now untrue

- `docs/design.md:2311`: "`net:auto:1`, `net:tcg:1` on CI". CI runs `net:tcg:1` only.
- `docs/design.md:2468-2472` (the CI paragraph): add that where `auto` is TCG each pair runs once (ADR-0350).
- `docs/design.md:1036-1046`: the configuration table. On CI `auto-default` and `auto-el2` are not run; their twins
  are.
- `docs/backlog.md:165`: the `bytes dropped]` flake row is now explained and fixed (F5); update or close it.

### R10 (note): ADR-0350 against ADR-0001

The ADR is a fair record, with two exceptions:
- R4 contradicts the claim that nothing tells a pair apart.
- ADR-0001 rule 5 asks for a Linux/POSIX baseline and a non-Unix precedent among the options. A CI-policy ADR has
  neither; say so in a line rather than leaving it out silently.

The rest is in order:
- Status `proposed`, and the Architect's decision section is left empty.
- The number comes from the task's reserved range (0350-0359).
- The index row and the invariants rows are there, and `make adr-check` passes.
- Marking a proposed ADR's invariants `checked:` has precedent (ADR-0316's rows).

INV-0350-1 says "once". The check enforces exactly one for GUI runs and refuses an `auto`+`tcg` pair. It does not
refuse the same POSIX script listed twice under `tcg` (a duplicate line passes), which wastes time but loses no
test. INV-0350-2: see R1.

### R11 (note): V29's assertion is a weak floor

`crates/zoraya-fs/tests/crash.rs:185,197,254`. `t.images > 0` passes a harness that checks one image instead of
thousands. The seeds are fixed, so the counts are deterministic.
**Fix:** assert a floor close to the measured count (for example at least half of what the test prints today), so
that a harness that silently skips most windows fails.

### R12 (note): negative tests of `check-ci-jobs` always report a count mismatch

`tools/check-ci-jobs.sh:140-144`. `tools/ci-plan.sh` always reads `tools/ci-tests.txt` (line 27), so
`make check-ci-jobs CI_TESTS=<other>` with a different number of lines always adds an "all N jobs ... in the full
plan" error. This is pre-existing. It makes "shown failing" claims noisier than they need to be. Each negative I
built also failed for its own reason (below), so this costs nothing today.

### R13 (note): runner tests

`cargo test -p zoraya-test-runner`:
- The library and binary tests passed (49 and 7, including `quiet_and_forbid_take_the_escapes`).
- With the other targets running in parallel at a load average of about 6.6, `tests/flood.rs`'s
  `endless_lines_fail_the_line_mode` and `endless_lines_fail_the_script_mode` failed on their 60 s bound (69.7 s).
- Re-run alone (`--test flood -- --test-threads=1`), all 4 passed.

The flood path is untouched by this branch. The failure is a pre-existing sensitivity to load, worth a backlog row.

## Checked and found sound

- **D1-D3 pairs.** For every dropped pair, the remaining run has the same `run-qemu.sh` options, the same script and
  at least the same expected lines:
  - `auto-default`/`tcg-default` and `auto-el2`/`tcg-el2`;
  - the 11 POSIX script pairs, `w9-term` and `tour`;
  - `test-tour-gui`, `test-posix-gui` and the four `LOCAL_ACCELS` loops;
  - the 12 GUI pairs;
  - `net-boot-tcg`;
  - `net-fetch-tcg`, `net-posix-tcg`, `apps-tcg` and `apps-refusals-tcg`.

  `run-qemu.sh` resolves `auto` before anything else, and nothing after that tests the word `auto`. The GUI runner
  asks `--print-accel`, and per-name POSIX limits do not depend on the accelerator. The exception is R4.
- **Timeouts.** Every remaining run's limit is at least the dropped one's: `AUTO_TIMEOUT` gives 400 s and
  `APPS_TCG_TIMEOUT` 500 s where `auto` is TCG. The job limits in `test.yml:347` still fit: every
  `POSIX_ACCELS="tcg"` line still contains `tcg` (30 min), and every other changed line does less work than before
  under the same limit. Host checks that read a server log run after one run instead of two and still find their
  lines.
- **On a Mac nothing changes.** `print-legs` with HVF gives `auto tcg`, all 7 configurations, all 29 GUI runs and
  `tcg-too: 1` under FULL=1, and the same as `main` under FULL=0. The recipes take the old values when
  `AUTO_IS_TCG=0`.
- **`check-ci-jobs` on altered lists.** It refuses each of these:
  - dropping `tcg-el2`;
  - hello/w5 under `auto` only;
  - `terminal:auto:1` in place of `terminal:tcg:1`;
  - `w9-term` under both;
  - `auto-default` in place of `tcg-default`.

  It accepts `test-tour-gui POSIX_ACCELS="hvf"`, `test-net FULL=0` and `test-posix-suites LOCAL_ACCELS=auto`. That
  weakness (any variable on a non-split line) is pre-existing, and the first would turn CI red rather than pass
  silently. I found no list that leaves out a real run and passes.
- **`ci-plan.sh`.** The plans for `tests/gui/x`, `fuzz/x`, `unmapped/x` (42 jobs, with the candidate fuzz at 60 s)
  and `tests/shell/console.script` (the smoke set) are sensible. The smoke set's `terminal:auto:1` is the full list's
  `terminal:tcg:1` on CI.
- **Other tools.** `tools/test-par.sh`, `tools/test-changed.sh` and `tools/test-map.txt` read neither
  `ci-tests.txt` nor the accelerator, so they are unaffected.
- **`run-qemu.sh` reordering.** `--print-accel` now answers without QEMU, and every other mode still stops at the
  QEMU check. An empty `HOST_ACCEL` falls back to running both legs.
- **V1.** No committed or generated script (tests/**/*.script, `tools/tour` output, `tests/libc-test/build.sh`, the
  Makefile's `sed` templates, which are committed `.script` files) has a `~` or `!` line with a backslash other than
  the three named ones. GUI scripts use `gui.rs`'s parser.
- **V6/V7/V8/V14.**
  - Native programs' lines on the console follow the `\r\n` of Enter's echo or of the previous line, as the
    unchanged `< \r\nfirst line\r\n/> ` and `< \r\ncleared` steps show.
  - POSIX output goes through `ONLCR`, so `< \n...` matches either way.
  - In scenarios.script, dash's `fg` prints the job's command line and then a newline, so `\nfirst` and `\nsecond`
    are the job's lines.
  - The room prompt `agent-1 /t/w> ` does not contain `\r\n/t/w> `.

  Each step passes on correct output and fails without it.
- **V20/V21.** `t_posix`'s only failure line is `t_posix: NAME: FAIL line N: ...` (`t_posix.c:124`), and nothing
  else in those scripts prints `: FAIL line`.
- **V22.** The shell prints `zosh: NAME: no such command` (`zoraya-shell/src/eval.rs:1489`).
- **V34.** `NET_OR_SKIP` keeps the local skip, fails under `CI`, and is used correctly in all nine recipes (ends in
  `;`, and the rest of the line continues).
- **T2/V41.** I could not verify T2 (rust-cache v2.9.3 and the cache service) without the network. `make
  check-mutant-lists` in the checks matrix is consistent with `check-ci-jobs` and the docs-only rule.

## Could not check

QEMU runs: whether the woken steps and the edited expectations pass on CI (I judged them from the code); how often
the flood step fails to overflow (R5); rust-cache v2.9.3 (T2).

## Dispositions (the audit session, 2026-11-14)

| Finding | Disposition |
|---|---|
| R1 | Fixed. `tools/check-ci-jobs.sh` compares, for a host where `auto` is TCG, the whole sets `make print-legs` gives with the full ones minus the `auto` twins, and checks `FULL=0` gives `auto` only on both kinds of host. Shown failing: a `Makefile` that kept only the `tcg` GUI runs where `auto` is TCG ("does not run every run of GUI_RUNS_ALL but the auto twins"). |
| R2 | Fixed. `AUTO_DEFAULT_EXPECT` and `AUTO_EL2_EXPECT` hold the lines once; both targets of each pair use them. |
| R3 | Fixed. `HOST_ACCEL` asks `run-qemu.sh --accel auto --print-accel`. |
| R4 | Fixed by keeping the run: `net-tcg` is made with `FULL=1` on every host again (it boots on the disk `net-settings-auto` left). `net-boot-tcg` has no disk and stays left out. ADR-0350 says so; its figure is 54 minutes, not 58. |
| R5 | Partly. The burst is 65536 bytes, not 8192, and `uptime`'s answer is expected at the start of a line. Not done: twenty repeats on one candidate (CI's budget; each candidate runs the script on seven machines). If the step times out because nothing was dropped while the job was in front, that is a finding about the test's premise, to be read from the log. |
| R6 | Fixed as proposed: `x \r\n/>` after the child's first line in `console.script` (and that line is expected at the start of a line, since `tick` is in the typed command); `o tstp: continued` in `zosh.script`. The `~` step is not changed to search from the last match: that changes the runner's language. |
| R7 | Fixed. A candidate keeps 600 s when the map sends a changed path to a fuzz step; 60 s only when the fuzz jobs are in the plan because an unmapped path widened it. |
| R8 | Fixed: the status cell of INV-0213-3 names proposed ADR-0350. |
| R9 | Fixed: the three places of `docs/design.md`; `docs/backlog.md`'s row on the `test-boot` flake now gives the cause. |
| R10 | Fixed: the ADR says why rule 5's baseline and precedent options have no counterpart; the check refuses a script listed twice under one accelerator. |
| R11 | Fixed: floors at about half the counts of 2026-11-14 (14460, 4404 and 407 images). Shown failing with a floor above the count ("only 4404 crash images were checked"). |
| R12 | Not changed: the count message is the existing check's. |
| R13 | Recorded in `docs/backlog.md`. |

One check of the changed lines: `make check-ci-jobs`, `make adr-check`, `make check-ci-plan`, `make check-host-paths`,
`cargo test -p zoraya-fs --test crash` (5 passed) after the fixes. The script edits of R5 and R6 are, like the
others, first run by the candidate.

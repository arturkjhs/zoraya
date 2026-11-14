# TASK-29: host tests, check-* scripts, mutant lists, fuzz targets that can pass vacuously

Read-only audit. Nothing was built with cargo, no QEMU, no mutation or fuzz run. The only commands run were
grep/python scripts over the tree and the pure-shell checks `make -s check-charged-alloc check-untrusted-gate
check-quota-tests check-user-access check-argv check-kernel-tty check-test-caps check-tokens` (all passed).
"CONFIRMED" means confirmed by reading the code named; no finding was confirmed by breaking the code and
running the test.

## Coverage

- Fully (mechanically, every file): `#[ignore]`, `should_panic`, env/tool/file skips, orphan module files,
  feature-gated test modules; all 2045 `#[test]` functions under `crates/ tools/ fuzz/ kernel/ user/` scanned for
  a missing assertion (38 with none + 13 weak, each read or its helper read); all 251 mutants of all 20 lists
  checked for `find` occurrence, match in a comment / test code / cfg-gated code, test command validity; all 700
  rows of `docs/adr/invariants.md` for status form.
- Read whole: `tools/mutants/src/{main,list}.rs`, `tools/run-fuzz.sh`, `tools/check-charged-alloc.sh`,
  `check-untrusted-gate.sh`, `check-quota-tests.sh`, `check-ci-jobs.sh`, `check-ethd-maps.sh`,
  `check-tls-roots.sh`, `check-vendor-ring.sh`, `check-fetch-cache.sh`, the Makefile recipes of every CHECK_STEP,
  the status logic of `tools/adr-check/src/main.rs`.
- Sampled only: `tools/check-ci-plan.sh`, `tools/test-run-disk.sh`, `ports/test-fetch.sh` (grep for skip paths
  only), `check-python-cache` (recipe read, not analysed), the rest of adr-check (INV-0001-1..3, 5), fuzz target
  bodies (assertions grepped; `wire.rs`, `spawn_request.rs`, `common.rs` read whole). Whether each mutant's
  listed test really kills it cannot be known without running; only reachability by crate/command was checked.

## Confirmed findings

### H1. `make adr-check` accepts any text after `checked:`
- Where: `tools/adr-check/src/main.rs:373-376` (`status_is_known`: `checked: ` + non-empty).
- Claims: CLAUDE.md "`make adr-check` verifies the table against the ADRs"; Status = `checked: <command>`.
- Break that passes: a row `checked: by type and review; no command fails if it is broken` (this is the real
  row INV-0042-1, `docs/adr/invariants.md:123`); a status naming a deleted test or a target outside `make test`.
  The rule and method columns are compared with the ADR text, the status is only shape-checked.
- Present state (script `inv.py`/`inv2.py`): 551 `checked`, 128 `not yet applicable`, 21 `replaced by`; 12
  `checked` rows have no backticked `make`/`cargo` command; INV-0005-3 and INV-0110-2 are `checked: make
  test-posix-long`, which is in neither TEST_STEPS nor the workflow (Makefile:1104 says so), so nothing
  automatic runs their check.
- Fix: require a `make T` or `cargo test` in a `checked:` status, verify T is a Makefile target, and warn when T
  is not reachable from TEST_STEPS.

### H2. `check-mutant-lists` never runs in CI
- Where: Makefile:1454 (CHECK_STEPS has it), 1423-1424 (TEST_STEPS does not), `.github/workflows/test.yml:287-305`
  (checks matrix does not), `tools/check-ci-jobs.sh:52` (compares the workflow with TEST_STEPS only).
  `tools/test-map.txt:189` maps `tools/mutants/` to `check`, which in CI therefore verifies nothing about it.
- Break that passes: a refactor moves or rewords a mutated line; the list is stale, every CI job is green.
- Found by: grep for `check-mutant-lists` in workflow, ci-tests.txt, ci-plan.sh (no hit).
- Fix: add `- make check-mutant-lists` to the matrix; make check-ci-jobs loop over CHECK_STEPS too.

### H3. 15 mutants in four lists no longer apply and are excluded from the check (known: docs/backlog.md:154-158)
- Where: Makefile:1390 `MUTANT_STALE := compositor-settings fs-limits fs-service kernel-objects`.
- My script (`mut.py`, same rule as `list::apply`) reproduces exactly 15, 0 in the other 16 lists:
  compositor-settings `C-restyle-attached`; fs-limits `L-narrowed-set-limit`; fs-service `S-clone-adds` (find
  now occurs twice), `S-sent-handles`, `S-copy-room`; kernel-objects `K-snapshot-any-job`,
  `K-snapshot-moved-stays`, `K-snapshot-object-pages-mixed`, `K-suspend-stops-nothing`,
  `K-stopped-ignores-kill`, `K-resume-wakes-nothing`, `K-resume-wakes-everything`,
  `K-contig-help-without-step-point`, `K-transport-hold-kept-on-spawn-move`,
  `K-transport-hold-kept-on-channel-move`.
- Effect: in a real `make mutants` run a non-applying mutant is an error (exit 1, listed in report.md), not a
  silent skip; but the snapshot, suspend/resume and transport-hold invariants have no live mutant today, and new
  rot in these four lists is invisible to `make check`.
- Fix: repair or delete the 15, then empty MUTANT_STALE.

### H4. A mutant counts as killed without any assertion failing; unviable and timed-out mutants are not reported
- Where: `tools/mutants/src/main.rs:196-211` (outcome), 296-345 (report lists only survivors and not-applied),
  494-507 (exit status counts only Survived and NotApplied).
- How killed: the list's command is run with `sh -c`; any non-zero exit is "caught", unless the output contains
  `error: could not compile` or `error[E` (then "unviable"); past the timeout is "timed out". The baseline runs
  once.
- Breaks that pass:
  (a) a `replace` that stopped compiling after a refactor is "unviable": exit 0, report says "No mutant
      survived", the id appears only in the scrolling stdout line;
  (b) a non-rustc build failure is "caught" with no test run: `tls-options.toml` mutates `.patch` files, so a
      patch that fails to apply or a C compile error counts as a kill;
  (c) a watchdog kill (`tools/mutants/watchdog.sh:14` says so), a flaky QEMU boot, or a mutant that breaks boot
      before the intended test is reached all count as caught; (d) a loop counts as "timed out", exit 0.
- Fix: list unviable and timed-out ids in report.md and fail on unviable; add an optional per-mutant
  `expect = "text"` that the failing output must contain (the test's name).

### H5. Fuzz steps: 5 of 13 targets are never built or run, and nothing notices a target that does no work
- Where: Makefile:1224-1225 (`FUZZ_TARGETS`, `FUZZ_NET_TARGETS`), `tools/run-fuzz.sh:83-92`,
  `.github/workflows/test.yml:356-374`, `tools/test-map.txt:115-116`.
- A crash does fail the step: each target's exit status is collected by `wait` and any non-zero (crash,
  libFuzzer timeout, rss limit, the perl alarm) gives `exit 1` (read, not run).
- (a) `mount_walk`, `raw_image`, `describe`, `shell_parse`, `editor_keys` are in `fuzz/Cargo.toml` and
  `fuzz/README.md` but in no Makefile list; `run-fuzz.sh` builds only the bins it is given, so they can stop
  compiling unnoticed. INV-0012-3/4, INV-0150-3, INV-0154-1, INV-0156-1 have only their deterministic tests.
- (b) Executions and coverage are printed, never checked: a target that returns at once on every input, or one
  whose log has no `Done N runs` line (`no result`), ends "no finding".
- (c) The corpus is `target/fuzz-run/T/corpus`, not committed and not cached by the workflow: every CI run starts
  empty (the Q7 targets carry in-code seeds, which mitigates this). Crash inputs in
  `target/fuzz-run/T/artifacts/` are not uploaded; only `tail -n 150` of the log survives the job.
- (d) Locally both steps exit 0 with `[fuzz] skipped` (Makefile:1227); on a merge candidate they run only when
  `fuzz/` or `run-fuzz.sh` changed, so a decoder change is fuzzed first by the nightly.
- Beyond "no panic" (debug assertions and overflow checks are on): spawn_request asserts counts of handles,
  argv, envp and program-header size; file_protocol ends with a clean fsck; keeper_messages asserts limits on a
  job model; net_dns_http asserts body bound, one header end, address count; net_stack asserts scope re-encode;
  describe, shell_parse, editor_keys assert positions in range; broker_tty, compositor_protocol, net_rx,
  mount_walk, raw_image: no panic only.
- Fix: add the five to a list (or delete them); fail when a target's run count or `cov` is below a floor;
  upload `target/fuzz-run/*/artifacts` on failure; cache or commit a small corpus.

### H6. `check-tokens` (INV-0161-1, INV-0165-3) does not look at the application crates
- Where: Makefile:1411 `GUI_SOURCES` = compositor, term, ui, app, panel, `compd.rs`, `term.rs`, `panel.rs`.
- The invariant says "the applications" too. Not scanned: `crates/zoraya-files`, `zoraya-editor`,
  `zoraya-settings-window`, `zoraya-chooser`, `user/src/bin/{edit,chooser,files,settings}.rs`.
- Break that passes: `0x00ff8800` or `Duration::from_millis(150)` in `crates/zoraya-files/src/view.rs`.
  Today those paths are clean (my grep with the recipe's own filter: 0 hits each).
- Also: a listed path that is renamed makes grep print to stderr and the pipeline (`|| true`) pass.
- Fix: add the paths; `test -e` each one first.

### H7. `check-charged-alloc` (INV-0261-5, INV-0260-2) covers a fixed file list and two constructors
- Where: `tools/check-charged-alloc.sh:27-28`.
- It scans 11 named files for `(Arc|Box)::(new|from)(`. A file renamed or split (`syscall/mod.rs`) makes grep
  fail inside `$(... || true)`: silent pass. A new kernel file reached by a system call is not scanned
  (`reaper.rs`, `steps.rs`, `device.rs` are outside the list today). `Vec`/`String`/collection growth in the
  listed files is outside it (11 such lines in `syscall.rs`, 9 in `memory/user.rs`, by a rough grep) and rests
  on `t_quota` alone, which the status names for memory, ports, events, channels and streams.
- Fix: `[ -r file ] || fail`; scan all of `kernel/src` minus an explicit boot-only list.

### H8. The crash and decoder-fuzz tests never assert that they produced any case
- Where: `crates/zoraya-fs/tests/crash.rs:167-195` and 201-251 (totals only printed with `eprintln!`),
  `crates/zoraya-fs/tests/decoder_fuzz.rs:219-262` (`mounted` only printed).
- Claims: INV-0010-1/2, INV-0015-2 (`cargo test -p zoraya-fs --test crash`), INV-0012-3/4.
- Break that passes: `RecordingDevice` stops recording marks or `crash::windows` returns nothing: 0 crash
  images, every assertion sits in a closure that never runs. In the targeted test the all-or-nothing assertion
  is also skipped when the image has no `target` (`crash.rs:299`). In the fuzz test, a decoder that refuses
  every mutated image gives `0 mounted` and passes.
- Mitigation: `tests/units.rs:536-547` tests the harness itself (`cases > 300`).
- Fix: `assert!(t.images > 1000 && t.windows > 0)`, `assert!(b.mounted > 0)`.

### H9. `close_sends_close_and_frees_the_buffer` asserts nothing
- Where: `crates/zoraya-surface/src/tests.rs:425-428`; `Surface::close` at `src/lib.rs:473-477` consumes the
  surface, so the fake platform cannot be inspected, and the fake has no `Drop` check.
- Break that passes: delete both statements of `close`.
- Fix: let the fake share its record through `Rc<RefCell<..>>` and assert the `Close` message and `freed`.

### H10. `check-fetch-cache` passes when it finds no fetch at all
- Where: `tools/check-fetch-cache.sh` (awk `END`): it matches only the exact line `run: make fetch-ports`.
- Break that passes: the step rewritten as a `run: |` block; zero fetches found, "every fetch ... has its cache
  steps". Fix: fail unless at least one fetch was seen.

## Lower severity or suspected (not confirmed beyond reading)

- S1. The refusal markers of `check-untrusted-gate` / `check-quota-tests` are comment-only lines
  (`user/src/bin/zosh.rs:3141, 3601`, `crates/zoraya-panel/src/lib.rs:705`): deleting the refusing statement
  and keeping the comment passes both scripts. `tests/shell/rooms.script` and `conventions.script` mention the
  refusal text, so a script step may still catch the shell half; the panel half was not traced.
  `check-quota-tests` exits 0 with its one gap while the gate is closed (by design).
- S2. `crates/zoraya-ramdisk/src/lib.rs:137`: tests are `#[cfg(all(test, feature = "std"))]`. They compile only
  through feature unification with `tools/mkramdisk`; `cargo test -p zoraya-ramdisk` alone would run 0 tests
  and pass. `make test-unit` and the narrowed plan include the dependants, so CI runs them.
- S3. `check-test-caps` is a string match anywhere under `src/` (a comment satisfies it) and covers `crates/`
  only (documented). Today every crate has a real, non-comment use.
- S4. `check-argv`, `check-user-access`, `check-kernel-tty` are negative greps with no positive control.
  `check-argv` sees only `user/src/bin/*.rs` (30 of 105 files have `describe!(`); a helper in `user/src/*.rs`
  that reads `startup().argv` for a described program is not seen. `check-user-access` is live today (3 hits in
  `arch/aarch64/usercopy.rs`) but matches lower-case mnemonics in `.rs` files only.
- S5. `check-ethd-maps` reads `lib.rs` and `serve.rs` of zoraya-virtio-net only (a new module, or `src/serve/`,
  is not read) and `ethd.rs` only (a mapping made by a helper in `user/src/` is not seen); `count -le 2` also
  passes at 0.
- S6. `check-ci-jobs` collects every `make ...` list item of the workflow whatever its job, `if:` or matrix
  `exclude:`.
- S7. `tools/mockups/tests/parsers.rs:9-40` claims "every file" but skips a direction folder without
  `tokens.txt` and asserts only `files >= 1`.
- S8. The host tests of zoraya-posix run without the `stats` feature, while `libc.a` is built with it
  (`crates/zoraya-posix/Cargo.toml:15-19`); that code path is linted by clippy and exercised only in QEMU.
- S9. `tools/plan-lib.sh:48` discards `cargo tree` errors (`2>/dev/null`): a failure narrows the host-test
  package list silently.

## Checked and found sound

- `#[ignore]`: 9 tests (zoraya-paint `samples`; zoraya-text `grid_cost`, `sample`; zoraya-fs `limits.rs`
  `measure_the_cost_of_a_growing_request`; zoraya-term `samples`; zoraya-compositor `samples` x2; mockups
  `glow_cost`, `blur_cost`). All are measurements or sample writers, none is cited in invariants.md, CI never
  runs them. The only env-var early returns are inside these. No test skips on a missing tool or file.
- No orphan test module files; no `#[should_panic]` without `expected` (2 uses, both with it).
- Of the 38 assertion-less tests, the rest assert through a helper (`guarded`, `exclusion`, `floods`, `total`,
  `round_trip!`, `exercise`) or are honest "never panics" tests; `translation_is_total_over_every_layout` relies
  on `Key.text: Option<char>`.
- Mutant lists: no `find` matches a comment, a test module or cfg-gated code; every test command names an
  existing crate, test file, filter (at least one test) or make target; 1 of 155 test names in notes is
  abbreviated (`a_room_holds`, limits-q6b), the test exists. `ports/Makefile` lists the patches as
  prerequisites, so the tls-options mutants do rebuild curl and wget.
- `check-vendor-ring`, `check-tls-roots`: real content checks.

Scripts used are in the session scratchpad (`noassert.py`, `orphans.py`, `inv.py`, `inv2.py`, `mut.py` to
`mut5.py`); they are not part of the repository.

# TASK-11 review

- **Reviewer:** independent reviewer subagent (Claude), which did not write this code.
- **Commit reviewed:** `3295663` on `shell/console` (`git diff fad9102..HEAD`).
- **Date:** 2026-11-01

## Summary

The kernel path is sound as far as I could test it: I found no way for a process to panic the kernel through the new calls, and the lock order, level signal and handle use held up. The problems are in the tests. One scripted step fails intermittently, and several invariants marked `checked` are not checked by what is named. Of 68 mutants, 40 were killed and 28 survived; 2 of the 28 are equivalent.

## Findings

| ID | Severity | Location | Evidence | Suggested fix | Disposition |
|---|---|---|---|---|---|
| R-1 | error | `tests/shell/console.script`, burst step | `make test-boot` can fail. Ctrl-C is sent while the ring may still be overflowing, is dropped, and `spin` never ends. Failure: `timed out waiting for "[terminated: Interrupt]"`, no `^C` in the output. Full script under HVF: 1 failure in the first 20 runs, 0 in the next 20. Burst steps alone under HVF: 0 of 60 on an idle host, 3 of 30 with 2 × ncpu spinners on the host. TCG: 0 of 10. The comment "so it comes after" is wrong: serial order puts the key last in the same overflowing stream. | Add a script step that waits for silence, or resend `\x03` until the report appears. Say in ADR-0060 that the interrupt key itself can be lost. | Fixed: the runner has `@ KEYS | TEXT`, which resends until the text appears. ADR-0060 says the key can be lost. |
| R-2 | error | `docs/adr/invariants.md` INV-0061-2; `user/src/bin/t_console.rs` | Marked checked, but K7 (next cookie + 2, half the names skipped) passes the test kernel and the script. The test only asks for more than 10 entries and `hello` once, and `hello` is entry 0. | Compare the listing against the full ramdisk name list, in the test kernel. | Fixed: `t_console` exits with the count of names, and the test kernel compares it with the ramdisk. |
| R-3 | error | INV-0063-1; `tests/shell/console.script` | Marked checked by `make test-boot`, but K12 (kernel also starts `hello`, which gets a Power handle too) passes the script. No step looks for it. The second half of the status, that `process::spawn` is not built, does not stop `spawn_first` being called twice. | Forbid `hello from EL0` until after `run hello`, or print a marker for each process the kernel starts. | Fixed: `x hello from EL0` right after the banner. |
| R-4 | error | INV-0062-1; `crates/zoraya-abi/src/tests.rs` | The named "unit test over `OBJECT_TYPES`" for "no call makes one" does not exist beyond a count of 7 types. K16 (`directory_lookup("power")` makes a Power handle) and K17 (Power can carry WRITE) both survive. | Test the exact rights of `PowerObject`, and state that no call makes one is held by review if no test can show it. | Fixed: a unit test of `PowerObject`'s exact rights. That no call makes one is stated as held by review. |
| R-5 | error | INV-0062-3; `tests/shell/console.script` | P1 (`poweroff` exits the shell, no `power_off` call) passes, because the kernel powers off when the first process ends (ADR-0063), also with status 0. The check cannot tell the two paths apart. | Add `! first process ended` to the script. | Fixed: `! first process ended`. |
| R-6 | error | `tests/shell/console.script`, fanout step; task step 3 | The task asks that no `fanout` child is left, checked with `jobs` and the job's terminated signal. Z2 (Ctrl-C reaches only the leader, and the shell waits for the leader) passes, leaving the children running. `jobs` only reads the shell's own table, so it cannot show this. | After Ctrl-C, have a program or test observe the job's `TERMINATED` independently, for example a child that prints until stopped followed by a forbidden marker. | Fixed: `fanout 4 tick`, then a quiet time with no `tick`. |
| R-7 | imprecision | `tests/shell/console.script`, wait step | `wait` is not tested: Z16 (`wait %n` does not wait) and Z20 (`wait` does nothing) survive, since the `done` line and a prompt appear anyway. `wait` is only run with no jobs. | After `wait %n`, expect the `done` line with no prompt before it, and test bare `wait` with a job running. | Fixed: `x zosh$ ` after each `done` line. |
| R-8 | imprecision | `user/src/bin/zosh.rs`, `spawn_in_job`; ADR-0060 | Nothing tests "only the handles the shell lists" or "the shell lists `WRITE` only". Z6 (children get READ and WAIT) and Z7 (every program gets console, job and directory) survive. | A demonstration program that reports what it holds, run from the script. | Fixed: `handles` reports slots and the console read. |
| R-9 | imprecision | `user/src/bin/zosh.rs:149` (`JobObject::RIGHTS`) | `fanout` and `parent` receive their job with every right, including `KILL`, `SIGNAL`, `DUPLICATE` and `TRANSFER`. By reading, `fanout` uses only `CREATE_PROCESS`. It also gets a duplicable console that it never hands on, and its children get no handles. I did not run a narrowed version. | Grant `CREATE_PROCESS` only, and give each program its own list. | Partly: the job is `CREATE_PROCESS` only. Both programs still get the same list. |
| R-10 | imprecision | INV-0063-2 in `docs/adr/invariants.md` | The status `checked: by review` is neither form CLAUDE.md allows (`checked: <command>` or `not yet applicable (stage N)`). `make adr-check` does not object. The ADR's open question does record why, so the content is honest. | Use an allowed form, or add a test path. | Fixed: status is `not yet applicable (stage 4)`, with the reason in the open question. |
| R-11 | imprecision | ADR-0060, ADR-0062, `crates/zoraya-abi/src/lib.rs` | ADR-0060 says ADR-0040 and ADR-0043 "allow" the rights growth, but ADR-0040's table lists Console's rights and calls exactly, and code now differs from an accepted table while ADR-0060 is proposed. ADR-0006 rule 1 lists terminals as byte streams and rule 6 has a non-blocking read return `WouldBlock`; `console_read` returns 0 and ADR-0060 names neither. ADR-0062 has no tensions note for the new right and type in ADR-0040's lists, or slot 4 in ADR-0043's convention. | State these tensions in the ADRs, for the architect to rule on. | Fixed: tensions stated in ADR-0060 and ADR-0062, for the architect. |
| R-12 | imprecision | `kernel/src/syscall.rs`, `console_read` | A buffer over 256 bytes is `InvalidArgument`, which ADR-0060 does not say, so the ring size leaks into the ABI. K20 shows the boundary is untested. The call also zero-fills the whole buffer, though the doc comment says bytes move to the front. | Clamp to the ring size, or document the limit, and test 256. | Fixed: clamped to the ring's size and documented, tested at 256. |
| R-13 | imprecision | `kernel/src/syscall.rs`, `console_read`; `t_console.rs` | The bad-address ordering is right but has no test: K9 survives. With a temporary test added to `t_console`, the unmodified code passed (see below) and K9's edit failed, so the code is correct and the test is simply absent. | Add those cases to `t_console`. | Fixed: `t_console` tests a bad address. |
| R-14 | imprecision | `tools/test-runner/src/script.rs` | Three properties the runner relies on have no test: R2 (QEMU's status ignored), R5 (final forbidden check removed), R6 (no power-off passes). The header says a forbid applies "from here to the end", while `violation` scans all output. By reading, `write_all` can block past the deadline, and `let _ = label` is a leftover. | Test `wait_exit` and `drive` against a small fake command. | Fixed: host tests against `sh`. The blocking `write_all` stays, as an accepted limit of a test tool. |
| R-15 | imprecision | `crates/zoraya-shell/src/lib.rs`, `Editor::key` | Only two-byte escapes are understood. Run: Delete (`\e[3~`) left `~` in the line (`ab~cd`), and `\eOA` left `A` (`efAgh`). By reading, Ctrl-C straight after a lone Esc is swallowed. Host tests also miss S9, S10, S11 and S12. | Consume a CSI sequence to its final byte, let Ctrl-C through an escape, and add the tests. | Fixed: CSI runs to its final byte, SS3 takes one, Ctrl-C passes through. Tests added. |
| R-16 | imprecision | `tests/shell/console.script`; `user/src/bin/zosh.rs` | Z4 (clean exit prints `[exited 0]`) and Z12 (dropped total not remembered) survive. The script has no check that a clean exit prints nothing. Several `< text` steps also match the shell's own echo of the typed line (`cleared`, `alive`, `remembered`). | Add a forbidden-text step scoped to that command, and expect output the echo cannot produce. | Fixed: scoped `x` steps, and `\r\n` in the expected text. |
| R-17 | imprecision | `docs/design.md` | The Console bullet sits inside the ADR-0004 list of what replaces Unix signals (line 56). The Tests section (line 310) still describes `make test-boot` as printing heap and timer checks and powering off, with no mention of the script. | Move the bullet and update the section. | Fixed. |
| R-18 | note | Known choice: fault report without an address | Judged acceptable. `ExitRecord.detail` holds the class under ADR-0034 and 32 bits cannot hold an address. The kernel's line does give it: `process fault: fault, EC 0x24, ELR_EL1 0x400004, FAR_EL1 0x10`. It does differ from the task's example `[fault: data abort at 0x...]`, which is an example only, and is recorded in design.md. | Leave it. Keep it in the task report so the architect sees it. | Left as it is. In the final report. |
| R-19 | note | Known choice: typing during a foreground job is dropped | Judged acceptable, and Z15 shows the script protects it. No child holds `READ`, so nothing else could use the bytes. Limits: these bytes are not in the dropped count, and by reading a pasted second line is lost. The design note "do not reach the next prompt" holds only for bytes read before the job ends. | Say so in design.md. | Stated in design.md. |
| R-20 | note | `user/src/bin/zosh.rs`, `ended` | The report is the leader's record, while the wait is for the whole job. By reading, a leader that exits 0 and leaves children that are later interrupted or fault prints nothing, and `exit-code` shows a clean exit. No demonstration program does this. | Accept and document it, or report that children outlived the leader. | Stated in design.md. `fanout 1 tick leave` tests the wait. |
| R-21 | note | `user/src/bin/zosh.rs`, `PATIENCE` | The third Ctrl-C calling `job_kill` goes beyond the task and is documented only in design.md. `[killed]` is never reached in a test: Z14 survives, and no demonstration program ignores Interrupt. | Add such a program, or remove the rule. | Tested with `stubborn`, and stated in design.md. |
| R-22 | note | `zosh.rs`, `command` | Run: bare `kill` gives `no such program: kill`, as does `wait a b` with `wait`; `echo hi &` and `ls &` ignore the `&`. | Usage lines for built-ins; refuse `&` on them. | Fixed: usage lines, and `&` refused on built-ins. |
| R-23 | note | `user/src/bin/fanout.rs` | `fanout 300` printed nothing in 260 s under HVF, and `fanout 100` completed. The shell stayed responsive at 300 (`jobs`, `echo`), and Ctrl-C and `kill -int` ended the job. I infer the leader gets slower with each spinning child, but did not measure it. | None needed for the demonstration. | No change. |

## Checked with no finding

- **Locks.** `SpinLock` masks interrupts, and `receive` and `read` take INPUT, then SOURCE, then EVENTS in that order. By reading, nothing takes them the other way round. `Object::signals` takes SOURCE alone, before EVENTS.
- **Allocation in the interrupt path.** By reading, `Events::set` uses reserved room and `sched::wake` uses a queue reserved per thread.
- **Lost wakeups and READABLE.** `port_wait` keeps interrupts masked from `take` to blocking, and `read` clears the signal under the INPUT lock. K1, K2 and K3 are killed.
- **Bad addresses.** With a temporary edit to `t_console`, `console_read` and `directory_entry` gave `BadAddress` for an unmapped address, a code address and a kernel address, and the two queued bytes were still read afterwards. A 257-byte buffer gave `InvalidArgument`, and an empty one gave 0. The edit was reverted.
- **Handle leak.** 1200 programs in a row ran, and the next one still reported correctly. Z8 and Z9 both fail that script, so it does detect a leak. Each `start` failure path closes what it opened, by reading.
- **Exit order.** `exit_current` writes the record before `job.leave`, so `TERMINATED` cannot arrive ahead of the leader's record.
- **Jobs table.** Nine background jobs gave `too many jobs` with no damage. Ctrl-C ended `wait` and `wait %1`, leaving the jobs running. `kill %99` was refused.

## Mutations

Each mutant was a small edit, run through the narrowest check, and reverted with `git checkout -- <file>` by the script, which stopped if `git status --short` was not clean. Script runs used a rebuilt `build/zoraya-kernel.elf` under TCG. A "survived" means only the checks in that row, not all of `make test`.

**Result: 68 mutants, 40 killed, 28 survived.** K15 and K19 are equivalent. K2 stopped at the test kernel, so the script was not run for it.

| Mutant | What it breaks | File | Check run | Result |
|---|---|---|---|---|
| C1 | Ring drops without counting | `crates/zoraya-console/src/lib.rs` | `cargo test -p zoraya-console` | killed: a host test failed |
| C2 | Ring full one byte early | `crates/zoraya-console/src/lib.rs` | `cargo test -p zoraya-console` | killed: a host test failed |
| C3 | Full ring overwrites the oldest byte | `crates/zoraya-console/src/lib.rs` | `cargo test -p zoraya-console` | killed: a host test failed |
| C4 | `take` leaves `count` unchanged | `crates/zoraya-console/src/lib.rs` | `cargo test -p zoraya-console` | killed: a host test failed |
| C5 | `push` writes at `first`, ignoring `count` | `crates/zoraya-console/src/lib.rs` | `cargo test -p zoraya-console` | killed: a host test failed |
| K1 | READABLE never cleared | `kernel/src/console.rs` | test kernel, TCG, one configuration | killed: `console_and_listing_from_a_process` |
| K2 | READABLE never set | `kernel/src/console.rs` | test kernel, TCG, one configuration | killed: `console_and_listing_from_a_process` |
| K3 | READABLE cleared on any read, with bytes left | `kernel/src/console.rs` | test kernel, TCG, one configuration | killed: `console_and_listing_from_a_process` |
| K4 | `console_read` asks WRITE, not READ | `kernel/src/syscall.rs` | test kernel, TCG, one configuration | killed: `console_and_listing_from_a_process` |
| K5 | `console_dropped` asks WRITE, not READ | `kernel/src/syscall.rs` | test kernel, TCG, one configuration | killed: `console_and_listing_from_a_process` |
| K6 | `directory_entry` asks no right | `kernel/src/syscall.rs` | test kernel, TCG, one configuration | killed: `console_and_listing_from_a_process` |
| K7 | Next cookie is cookie + 2, so every second name is skipped | `kernel/src/syscall.rs` | test kernel, TCG, one configuration and runner, console.script, TCG | survived |
| K8 | Last entry gives a non-zero next cookie | `kernel/src/syscall.rs` | test kernel, TCG, one configuration | killed: `console_and_listing_from_a_process` |
| K9 | `console_read` skips the user-memory probe before taking bytes | `kernel/src/syscall.rs` | test kernel, TCG, one configuration and runner, console.script, TCG | survived; a test I added temporarily does kill it (see below) |
| K10 | `kernel_handles` installs POWER in every process | `kernel/src/process.rs` | test kernel, TCG, one configuration | killed: `closing_a_port_ends_a_blocked_wait` (another test failed first) |
| K11 | `power_off` asks no right | `kernel/src/syscall.rs` | test kernel, TCG, one configuration | killed: `console_and_listing_from_a_process` |
| K12 | Normal kernel also starts `hello` with a Power handle | `kernel/src/main.rs` | runner, console.script, TCG | survived |
| K13 | Receive interrupt not enabled | `kernel/src/main.rs` | runner, console.script, TCG | killed: timed out waiting for "a b" |
| K14 | PL011 mask has the timeout interrupt only | `kernel/src/pl011.rs` | runner, console.script, TCG | killed: timed out waiting for "a b" |
| K15 | `drain` reads one byte per interrupt | `kernel/src/console.rs` | runner, console.script, TCG | survived; equivalent under QEMU, which keeps the line up while bytes remain |
| K16 | `directory_lookup("power")` makes a Power handle | `kernel/src/syscall.rs` | test kernel, TCG, one configuration and runner, console.script, TCG | survived |
| K17 | Power object can also carry WRITE | `crates/zoraya-abi/src/lib.rs` | `cargo test -p zoraya-abi` | survived |
| K18 | `DirectoryEntry::decode` ignores reserved bytes | `crates/zoraya-abi/src/lib.rs` | `cargo test -p zoraya-abi` | killed: a host test failed |
| K19 | `directory_entry` drops its size check | `kernel/src/syscall.rs` | test kernel, TCG, one configuration | survived; equivalent: `UserSlice::write` refuses the same case |
| K20 | `console_read` refuses exactly 256 bytes | `kernel/src/syscall.rs` | test kernel, TCG, one configuration and runner, console.script, TCG | survived |
| S1 | History off by one | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| S2 | Ctrl-U does not clear | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| S3 | Up walks past 32 lines | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| S4 | `[exited n]` prints detail | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| S5 | `is_clean` ignores the reason | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| S6 | `[limit]` reported as `[killed]` | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` and runner, console.script, TCG | survived |
| S7 | Class 0x24 not named data abort | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| S8 | Blank lines kept in history | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| S9 | `\n` no longer ends a line | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` and runner, console.script, TCG | survived |
| S10 | Ctrl-C leaves the history position | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` and runner, console.script, TCG | survived |
| S11 | Enter leaves the history position | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` and runner, console.script, TCG | survived |
| S12 | `-quit` sends Hangup | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` and runner, console.script, TCG | survived |
| S13 | Ctrl-C does not clear the line | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| S14 | History wraps at 31 | `crates/zoraya-shell/src/lib.rs` | `cargo test -p zoraya-shell` | killed: a host test failed |
| Z1 | Ctrl-C signals the leader only, not the job | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: timed out waiting for "[terminated: Interrupt]" |
| Z2 | Z1, and the shell waits for the leader instead of the job | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| Z3 | Shell waits for the leader instead of the job | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| Z4 | Clean exit printed as `[exited 0]` | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| Z5 | `exit-code` record never stored | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: timed out waiting for "[exited 0]" |
| Z6 | Children get the console with READ and WAIT | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| Z7 | Every program gets console, job and directory | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| Z8 | Image handle leaked per program | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived; a 1200-program reviewer script kills it |
| Z9 | Job handle leaked per program | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived; a 1200-program reviewer script kills it |
| Z10 | Ctrl-C sends Terminate | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: timed out waiting for "[terminated: Interrupt]" |
| Z11 | Ctrl-\ sends Interrupt | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: timed out waiting for "[terminated: Quit]" |
| Z12 | Dropped total not remembered | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| Z13 | Dropped bytes never reported | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: timed out waiting for "bytes dropped]" |
| Z14 | Third Ctrl-C does not kill | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| Z15 | Keys typed during a job reach the editor | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: timed out waiting for "alive" |
| Z16 | `wait %n` does not wait | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| Z17 | `kill` ignores its flag | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: timed out waiting for "[11] done sleep [terminated: Hangup]" |
| Z18 | Process handle closed before its record is read | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: timed out waiting for "[exited 7]" |
| Z19 | `parent` and `fanout` console lacks DUPLICATE | `user/src/bin/zosh.rs` | runner, console.script, TCG | killed: forbidden output: panicked |
| Z20 | `wait` with no number does nothing | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| R1 | Forbidden text never found | `tools/test-runner/src/script.rs` | `cargo test -p zoraya-test-runner` | killed: a host test failed |
| R2 | QEMU exit status ignored | `tools/test-runner/src/script.rs` | `cargo test -p zoraya-test-runner` and runner, console.script, TCG | survived |
| R3 | A match does not move on | `tools/test-runner/src/script.rs` | `cargo test -p zoraya-test-runner` | killed: a host test failed |
| R4 | ZORAYA-PANIC not always forbidden | `tools/test-runner/src/script.rs` | `cargo test -p zoraya-test-runner` | killed: a host test failed |
| R5 | Final forbidden-text check removed | `tools/test-runner/src/script.rs` | `cargo test -p zoraya-test-runner` and runner, console.script, TCG | survived |
| R6 | No power-off counts as a pass | `tools/test-runner/src/script.rs` | `cargo test -p zoraya-test-runner` and runner, console.script, TCG | survived |
| Q1 | `--keys` ignored, stdin stays /dev/null | `tools/run-qemu.sh` | runner, console.script, TCG | killed: send: Broken pipe (os error 32) |
| P1 | `poweroff` exits the shell instead of calling `power_off` | `user/src/bin/zosh.rs` | runner, console.script, TCG | survived |
| P2 | `power_off` returns instead of shutting down | `kernel/src/syscall.rs` | runner, console.script, TCG | killed: no power-off |

## What was and was not checked

**Run in this session, on the unmodified commit, in this worktree:** `make test-unit`, `make test-qemu` (7 configurations, 88 passed and 9 expected failures each), `make test-boot` (7 configurations, 110 steps each), `make adr-check` (0 errors, 17 warnings), `make fmt-check`, `make clippy`, `make check-user-access`. All passed on those runs; R-1 explains why `make test-boot` will not always pass.

**Not checked:**
- `make test` as one command from a clean clone, and CI on Linux.
- `make run` at a real terminal: Ctrl-C, Ctrl-\ and Ctrl-A X through `mon:stdio` were read, not tried.
- The 255-process limit: `fanout 300` never got there in 260 s, so what happens when `spawn` fails there is unknown.
- SMP (one CPU runs), real hardware. K14 shows that the timeout interrupt alone delivers no input under this QEMU, so the timeout path is not exercised.
- INV-0063-2: the path where the first process ends was reached only by P1, which ended the shell on purpose and powered off cleanly.
- `sleep` with a huge argument: Ctrl-C came too soon to show what the overflow does.
- Rates in R-1 are small samples, on this host only.

## After the fixes

Run by the author, not the reviewer, one configuration each. Rerun and now killed: K7, K12, K17, K20, S6, S9, S12, Z2, Z3, Z4, Z6, Z7, Z8, Z9, Z12, Z14, Z16, Z20, R2, R6, P1. Mutant K20 was rerun in a different form, a read clamped to 4 bytes. Not rerun: K9, K16, S10, S11, R5. K16 has no test (held by review); the others have new tests that were not shown to kill them. K15 and K19 are equivalent. The fixes had no second independent review.

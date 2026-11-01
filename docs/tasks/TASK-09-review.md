# TASK-09 independent review

- **Reviewed:** branch `kernel/stage-3` at `82fc0f13e3d7502ea4afc55bb1ec17446e4d4298` (`main..82fc0f1`, 4 commits).
- **Reviewer:** independent subagent that did not write the code (TASK-09, step 9).
- **Date:** 2026-11-01.
- **Environment:** Apple Silicon Mac, macOS (Darwin 25.4.0), the pinned toolchain, QEMU via Homebrew. All builds, mutations and probes ran in clones under the scratch directory. The only change on this branch is this file.
- **Reviewer's own material, not repository files:** the mutation lists `mut/m1.py` and `mut/m2.py`, the driver `mut/harness.py`, per-mutant logs in `mut/logs/`, and the probes `hostile/user/src/bin/h_{heap,mask,root,deep}.rs`, all under the session scratchpad.

## Result

`make test` passes, and 80 of the 106 mutants die. It does not follow that stage 3 is sound: a process can panic the kernel (R-01), and can hold interrupts masked for seconds (R-02). Five `checked` claims have mutants that survive (R-03 to R-07).

| Severity | Count | Findings |
|---|---|---|
| error | 7 | R-01 to R-07 |
| imprecision | 13 | R-08 to R-20 |
| note | 5 | R-21 to R-25 |

## After the fixes

The author reran the 21 mutants that were findings on the fixed code, plus new ones: `n-doublefree`, `n-stale`, `n-suspended`, `n-kind`. All die, each in the test the finding asked for; `k-tableful`, `k-fifo` and `e-reserve-q` were rewritten for the changed code first. A surviving rerun, `n-quad`, changed a path that bulk removal no longer uses and is not evidence. `make test` from a fresh clone of `kernel/stage-3`: exit 0, 87 passed and 9 expected failures in all 7 configurations. These reruns were by the author, under `auto-default`; no second independent review was done.

## make test from a fresh clone

```
git clone --branch kernel/stage-3 <worktree> fresh && cd fresh && make test
```

**Exit status: 0.** Wall time 2:56 with a fresh `target/`. `make test: all checks passed`. 50 `cargo test` summaries, all `ok`. `adr-check: 32 ADRs checked against INV-0001-1..5`, `0 errors, 18 warnings`.

```
[auto-default]    PASS: 84 passed, 9 expected failures
[auto-smp4-2g]    PASS: 84 passed, 9 expected failures
[auto-el2]        PASS: 84 passed, 9 expected failures
[tcg-default]     PASS: 84 passed, 9 expected failures
[tcg-smp4-gicv2]  PASS: 84 passed, 9 expected failures
[tcg-el2]         PASS: 84 passed, 9 expected failures
[tcg-smp4-gicv3]  PASS: 84 passed, 9 expected failures
[boot-auto]       PASS: 0 passed, 0 expected failures
[boot-tcg]        PASS: 0 passed, 0 expected failures
```

INV-0006-3, as printed by `portbench`:

```
[auto-default]    1 idle: 875 ns,   10000 idle: 875 ns,   ratio 100%
[auto-smp4-2g]    1 idle: 875 ns,   10000 idle: 875 ns,   ratio 100%
[auto-el2]        1 idle: 875 ns,   10000 idle: 834 ns,   ratio 95%
[tcg-default]     1 idle: 13008 ns, 10000 idle: 13008 ns, ratio 100%
[tcg-smp4-gicv2]  1 idle: 14000 ns, 10000 idle: 14000 ns, ratio 100%
[tcg-el2]         1 idle: 13008 ns, 10000 idle: 12992 ns, ratio 99%
[tcg-smp4-gicv3]  1 idle: 12992 ns, 10000 idle: 13008 ns, ratio 100%
```

## Findings

Severity: **error** means a defect, or a check that does not fail when the property it claims is broken. **imprecision** means something is wrong or missing with limited effect. **note** means no action is required for stage 3.

| ID | Severity | Location | Finding and evidence | What to do | Disposition |
|---|---|---|---|---|---|
| R-01 | error | `kernel/src/process.rs:450-456`, `kernel/src/job.rs:58-61`, `kernel/src/port.rs:51-54`, `kernel/src/memory/heap.rs:121-125` | **A process can panic the kernel: when `try_arc` fails, the source or port is freed twice.** On failure `try_arc` drops `value` (its own doc comment on `try_box` says so; `fits.then(\|\| Arc::new(value))` drops the unused closure). Dropping a `Process` or `Job` runs `Drop`, which calls `port::free_source`, and dropping a `Port` calls `free_port`. Each caller then frees the same id again, and `Events::source` or `port` panics on `expect`, with `EVENTS` held. ADR-0022 and ADR-0041 require `NoMemory` here, and ADR-0041's open question relies on it. **Reproduced from user space:** `h_heap` starts hogs that fill the heap through `port_create` and `port_queue`, ends one helper to get frames back, then calls `spawn` and keeps the handles of children that exit. Zombies use up the free `Process` blocks while frames are available, so `spawn` reaches `try_arc` and it fails. Output: `h_heap: spawn of a short child 0: Ok(912)` then `ZORAYA-PANIC at crates/zoraya-events/src/lib.rs:143:44: source is alive`. `try_arc_reports_failure` tests only `can_allocate`, so no test reaches a failing `try_arc` with a real type. | Delete the second free at all three sites: the drop has done it. Add a test-only way to make `try_arc` fail for each of `Process`, `Job` and `Port`, and rerun a heap-filling program as a test. | Fixed: the second free is gone at all three sites; `failed_allocation_frees_each_object_once` and mutant `n-doublefree`. `h_heap` itself was not rerun. |
| R-02 | error | `crates/zoraya-events/src/lib.rs:227-240`, `:312-321`, `:165-171`; `kernel/src/object.rs:60-64`; `kernel/src/process.rs:529-535` | **One system call holds interrupts masked for seconds, and the time is quadratic and in the caller's hands.** System calls run masked (ADR-0030). `remove_owner` and `free_source` call `remove_subscription` once per subscription, and each call does `ready.retain` over the whole ready list when the subscription is queued, plus a `Vec::remove`. ADR-0006 rule 2 sets no cap on subscriptions, so `n` is bounded by memory only. Measured under HVF by `h_mask`, with `Level` subscriptions on one handle of a suspended child, all queued: 1,000 gave 3 ms, 5,000 gave 87 ms, 20,000 gave 1,381 ms, 40,000 gave **5,539 ms for one `handle_close`**. Subscribing them took 26 ms in all, so the attack is cheap. `Edge`, not queued: 8 ms at 40,000. No timer, slice or other process runs during that. A process's exit takes the same path. `make test` does not see this, because INV-0006-3 measures only `port_wait`. | Make removal independent of the ready list's length, for example by leaving a stale entry that `take` drops, as it already does for a deasserted `Level` item, and tag entries so that a reused slot is not confused with it. Add a test that bounds `handle_close` with many queued subscriptions. Decide whether the `Vec::remove` on the sorted list also needs a bound. | Fixed: stale items and one sweep per port. Host test `ending_a_handle_is_not_quadratic`. The reviewer's `h_mask` rerun on the fix: one `handle_close` of 40,000 subscriptions took 4 ms under HVF and 47 ms under TCG, from 5,539 ms under HVF. The run of a mutant restoring a per-item `retain` was stopped before it gave a result, so that this test fails on a quadratic path is not shown. |
| R-03 | error | `user/src/bin/t_handles.rs:115-116`; `kernel/src/syscall.rs:210`, `:345`, `:360`, `:365`, `:375` | **Five rights checks can be removed and no test notices.** TASK-09 asks that every call refuses a handle lacking its right, and INV-0040-2 cites that test. Replacing the right with `Rights::NONE` survives for `port_subscribe`'s port (`WRITE`), `port_unsubscribe` (`WRITE`), `port_queue` (`WRITE`), `port_wait` (`WAIT`) and `spawn`'s group (`CREATE_PROCESS`): mutants `r-subport`, `r-unsub`, `r-queue`, `r-wait`, `r-group`. The comment in `t_handles` says a narrower port is tested in `t_spawn`. It is not: `t_ports::a_port_moves_but_is_not_copied` moves a port with `WAIT` and nobody uses it. The code itself is correct by reading. | Move a port to a child with each single right, and have the child show that the other calls give `AccessDenied`. Add a group handle without `CREATE_PROCESS` to `t_spawn`. | Fixed: `port-wait-only`, `port-write-only`, a weak group. All five mutants die. |
| R-04 | error | `user/src/bin/t_signals.rs:177-195`; `kernel/src/job.rs:19` | **INV-0046-3's check does not check 32.** `depth_has_a_limit` asserts only that some depth below 40 gives `LimitReached`. `MAX_DEPTH` of 33 and 38, which break the invariant, survive (`k-depth33`, `k-depth38`), and so does 4 (`k-depth4`), which breaks what ADR-0046 allows. | Assert the exact count: `count == 31` jobs below the root process's job, given where the test starts. | Fixed: exact count, 30 from a job at depth 1. All three die. |
| R-05 | error | `crates/zoraya-abi/src/tests.rs:75-85` | **INV-0006-6's lint cannot see a new status.** `no_status_means_interrupted` walks `Status::from_raw`, so `Interrupted = 12` added to the enum and not to `from_raw` passes: mutant `a-interrupted` survives. The count of 12 does not help, for the same reason. The invariant's check is exactly this lint. | Make `from_raw` and the enum come from one list, or match exhaustively in the test, so that a new variant must be judged to compile. | Fixed: one list, `Status::ALL`. Dies. |
| R-06 | error | `kernel/src/syscall.rs:252-259`; `user/src/bin/t_ports.rs` | **Ending subscriptions when a handle is moved is untested.** ADR-0040 states it, and it is the moved-handle form of INV-0006-4. `k-movesubs` removes `end_subscriptions` from the move loop and survives. Only a port is ever moved with a subscription near it, and a port has none as a source. | Test: subscribe a process handle, move it with `spawn`, assert the signal, and expect no packet. | Fixed: `moving_a_handle_ends_its_subscriptions`. Dies. |
| R-07 | error | `kernel/src/testing.rs:1540-1565` | **INV-0041-1's test runs only `parent`, though its comment says it covers what `t_stress` does.** A round has one spawn, one port and a normal exit. It has no job, no kill, no suspended child and no wait with a deadline. Two leaks survive: `l-job` leaks every job from `job_create`, and `l-sleepcount` leaks a thread, with its stack and process, each time a wait ends before its deadline. TASK-09 asks for a leak check after a spawn, exit and close loop, and a kill during a sleep is the case most likely to leak. | Run a round with jobs, `job_kill`, `Terminate` on a process blocked with a deadline, a suspended child killed before it starts, and a refused `spawn`. Correct the comment. | Fixed: `t_leak`. Both die. |
| R-08 | imprecision | `kernel/src/syscall.rs:224`, `:193`, `:333`; `crates/zoraya-handle/src/lib.rs:107-112` | **`spawn` with a full table is untested.** `reserve` is what stops a child from starting and its handle being lost. Removing it from `spawn` (`k-noreserve`) or its check (`k-tableful`) survives: `t_handles::a_table_can_be_filled` tries `port_create`, `job_create` and `directory_lookup` only. ADR-0040 says a full table has no other effect, and the code keeps that by reading. | Add `spawn` to that test, and assert that no child was started. | Fixed: `spawn` in the full-table test, and no child started. `k-noreserve` dies; `k-tableful` see below. |
| R-09 | imprecision | `kernel/src/process.rs:474-487`, `kernel/src/main.rs:121` | **The first process can kill the root job for good.** ADR-0043 gives it all rights, and `closed` never clears. `h_root` calls `job_kill(JOB)`. Then: `ZORAYA-PANIC at kernel/src/main.rs:121:47: parent in the ramdisk: JobClosed`. Only the boot sequence's `expect` panics, and only a program the kernel starts can do this. The same `expect` gives `NoMemory` after a program fills the heap. | Give a kernel-started process a sub-job, or a handle without `KILL`, which needs an ADR-0043 change. Or report a failed boot program without a panic. | Fixed: a process the kernel starts has a job of its own (ADR-0043), and boot reports a failure instead of panicking. |
| R-10 | imprecision | `kernel/src/process.rs:118-124`, `:255-258` | **An exit record does not say which request ended the process.** ADR-0004 rule 1: "`detail` carries the request kind and, for the personality, the POSIX signal number". The personality's mapping needs the kind when `detail` is 0 (`Interrupt → SIGINT`, and so on). `terminated(detail)` stores only the caller's value, so `process_signal(p, Hangup, 0)` and `process_signal(p, Interrupt, 0)` give equal records. | Encode the kind in `detail`, or write a proposed ADR that says how the personality gets it. | Fixed: the kind is in the top byte (ADR-0046). The architect should review this encoding. |
| R-11 | imprecision | `kernel/src/process.rs:295-305` | **`process_resume` returns `Ok` for a process that was killed while suspended.** By reading: `suspended` stays true through exit, so the swap succeeds, and the doc comment says `BadState` if it is not suspended. `t_spawn` asserts `BadState` only for one that was resumed first. | Clear `suspended` at exit, or check the record first. | Fixed, tested. `n-suspended` dies. |
| R-12 | imprecision | `crates/zoraya-abi/src/tests.rs:24`, `docs/adr/0045-port-keys-as-opaque-payloads.md` | **The lint allows what ADR-0045 does not list.** ADR-0045's recommendation and INV-0045-1 name `port_subscribe` and `port_queue`. `port_unsubscribe` also takes `Opaque`, and it must. | Correct the ADR's text before the architect decides. | Fixed in ADR-0045 and the row. |
| R-13 | imprecision | `kernel/src/memory/heap.rs:128-138` | **`can_allocate` is a safe `pub fn` with an unsafe precondition.** Its comment says the size is not zero, and its `SAFETY` comment cites "the caller's contract", which a safe function cannot have. A zero size is undefined behaviour under `GlobalAlloc`. `try_arc` never passes one. | Return `true` for size 0, or make it `unsafe fn` with `# Safety`. | Fixed: size 0 needs no block. |
| R-14 | imprecision | `crates/zoraya-rt/src/lib.rs:156-163` | **`process_exit_record` lets the kernel write through a pointer derived from `&raw`.** `bytes(&raw)` takes a shared reference, so the write is not allowed by Rust's aliasing rules, and the `black_box` after it is a repair of what that caused, as its comment shows. It is user code, and it works today. | Take the pointer from `&mut raw`, with a `Buffer` built for writing, as `port_wait` does. Remove the `black_box`. | Fixed. |
| R-15 | imprecision | `crates/zoraya-handle/src/lib.rs:107-112`, `:177-185`; `kernel/src/process.rs:529-535` | **A zombie keeps its handle table's memory.** `Table::pop` empties the table but the `Vec` keeps its capacity, so a zombie holds more than the record that ADR-0041 driver 4, and `docs/design.md`, say. `reserve` adds to this: `try_reserve(1)` at a full 1024-slot vector doubles it to 2048 slots, past the limit. | Replace the table with an empty one at exit. In `reserve`, reserve only when a new slot will be pushed. | Fixed both. |
| R-16 | imprecision | `docs/adr/invariants.md` (INV-0042-1) | **`checked: make test-unit` claims a test that does not exist.** `k-twice` reads the buffer twice and survives. The property holds by reading: `spawn` has one read, and `decode` takes `&[u8]`, which is what the ADR's check says. No command fails if it is broken. | Say in the row that it is checked by construction and review, or add a test that counts reads. | Row now says by type and review. |
| R-17 | imprecision | `crates/zoraya-abi/src/spawn.rs:211-220`, `:224-230` | **A bad single entry in a handle list is untested, and `handles()` would hide it.** `a-entrylast` skips validation of the last outer entry. With one entry nothing validates it, and it survives. The `filter_map` in `handles()` then drops the bad entry silently, which would make the child lack a handle. `the_decoder_is_total` asserts no panic, not a refusal. | Test a one-entry list with a bad slot, rights and mode. Make `handles()` unable to skip, or say why it cannot happen. | Fixed: `a_single_bad_entry_is_refused`. Dies. `handles` keeps `filter_map`: `decode` checks every entry. |
| R-18 | imprecision | `kernel/src/process.rs:99-107`, `:299`, `:541-544`; `kernel/src/syscall.rs:211`; `crates/zoraya-events/src/lib.rs:379-380`; `user/src/bin/t_signals.rs:85-95` | **Behaviour the ADRs state, with no test.** Each survives: the request queue returning last in, first out (`k-fifo`); `resume` leaving `SUSPENDED` asserted (`k-resumeclear`); exit leaving `SUSPENDED` and `REQUEST_PENDING` asserted (`k-exitclear`); a group equal to the job being refused (`k-groupself`, `k-within-noself`); `port_queue` not reserving, which makes it allocate infallibly (`e-reserve-q`, ADR-0022). `requests_queue_in_order_with_a_bound` tests neither order nor bound. | Add tests for these, and rename or fill that function. | Fixed; all die but see below for `k-fifo`. |
| R-19 | imprecision | `user/src/bin/t_stress.rs:4-5`, `:18` | **The stress test does not exercise preemption as it says.** Its comment says "under preemption by the children that spin", and TASK-09 asks for that. No role spins: `yield` makes a system call in a loop, and no count of preemptions is asserted. | Add a spinning role, and assert that `sched::preemptions()` rose. | Fixed: `spin` role, and the kernel test asserts preemptions. |
| R-20 | imprecision | `kernel/src/sched.rs:269-273`, `kernel/src/memory/stack.rs:88-95` | **Two `SAFETY` comments are not accurate.** In `schedule`, the list of what keeps `prev` alive omits `Process::thread`, which is what holds a user thread blocked in `sleep_until(Infinite)` or in a wait. In `first_pc`, a safe `pub fn` relies on "the caller asks about a thread not running". It is test-only. | Correct the first. Make the second `unsafe`, or say that reading a mapped stack is safe either way and the answer may only be stale. | Fixed both comments. |
| R-21 | note | `kernel/src/process.rs:78-116` | `Requests` is logic that needs no hardware, so CLAUDE.md puts it in a `no_std` crate with host tests. Its wrap-around has no test. | Move it when R-18 is addressed. | Fixed: `RequestQueue` in `zoraya-events`, with host tests. |
| R-22 | note | `crates/zoraya-handle/src/lib.rs:96-104`, `kernel/src/port.rs:126-130`, `crates/zoraya-abi/src/lib.rs:504`, `user/src/bin/fuzz.rs:22-24`, `crates/zoraya-rt/src/lib.rs:56-57`, `docs/adr/invariants.md` (INV-0035-1) | Small differences of wording. ADR-0040 says next-fit, and the code uses never-used slots first, then next-fit. ADR-0041 says a port id is freed when the port closes, and the code frees it with the last `Arc`, just after. ADR-0032 says `process_exit` does not return, and a code wider than 32 bits returns `InvalidArgument` (INV-0044-2). INV-0032-2 says every call except `process_exit`, and `fuzz` also skips `process_exit_with_reason` and `job_kill`. A duplicated doc line above `raw_syscall`. INV-0035-1's row says stage 4 as if `RelativePath` were missing, while ADR-0035's note gives the real reason. | Align the texts. | Texts aligned in ADR-0040, ADR-0041, `fuzz`, `zoraya-rt`. ADR-0032 and INV-0032-2 wording left for the architect. |
| R-23 | note | `docs/adr/0041-kernel-object-lifetime.md` | ADR-0041's open question is real: with no charge to a job, `h_heap`'s hogs left `spawn` with `NoMemory`, and by reading, 255 suspended children would hold every ASID. Once R-01 is fixed this is a denial of service, not a crash. | Architect's priority. | For the architect: job memory charging. |
| R-24 | note | `kernel/src/syscall.rs:198-261` | A `spawn` with 61,580 bytes of arguments took 1,210 µs under HVF, interrupts masked. The startup block is written one byte at a time. No ADR sets a bound, and `console_write` already allows about 60 ms. | None for now. | None. |
| R-25 | note | `git log main..kernel/stage-3` | The process lists one commit per step, and steps 3 to 8 are three commits. | None. | None. |

## Mutations

Each mutant was applied to a scratch clone, rebuilt, run, and reverted. `baseline`, a change to a comment, ran first and passed, with 84 tests, to show that the harness can report survival. A kernel mutant ran under the `auto-default` configuration (the runner, HVF). A host mutant ran `cargo test -p`. Each kill below was read from its log, to confirm that the named test failed for the mutated reason. "hang" means the test being run never ended and the runner's 60 s timeout ended the run, which is how a missing wake-up fails.

Of the 26 survivors, 3 are equivalent and `k-posix` cannot be observed before stage 6. Each of those is marked in the table. The other 22 are findings, and the table names them.

| Mutant | Claim | Change | Result | Evidence |
|---|---|---|---|---|
| `baseline` | - | comment only | survived | The control: 84 passed. |
| `r-console` | INV-0040-2 | console_write WRITE | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:36` |
| `r-jobcreate` | INV-0040-2 | job_create CREATE_PROCESS | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:63` |
| `r-spawnjob` | INV-0003-4 | spawn job CREATE_PROCESS | killed | `spawn_keeps_its_rules` at `t_spawn.rs:131` |
| `r-group` | INV-0040-2 | spawn group CREATE_PROCESS | survived | R-03 |
| `r-image` | INV-0040-2 | spawn image READ | killed | `spawn_keeps_its_rules` at `t_spawn.rs:151` |
| `r-move` | INV-0040-2 | spawn move TRANSFER | killed | `spawn_keeps_its_rules` at `t_spawn.rs:80` |
| `r-dup` | INV-0040-2 | spawn duplicate DUPLICATE | killed | `spawn_keeps_its_rules`, `ports_keep_their_rules` at `t_spawn.rs:81` |
| `r-subset` | INV-0040-2 | spawn rights subset check | killed | `spawn_keeps_its_rules` at `t_spawn.rs:69` |
| `r-resume` | INV-0040-2 | process_resume RESUME | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:61` |
| `r-record` | INV-0040-2 | process_exit_record WAIT | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:103` |
| `r-psignal` | INV-0040-2 | process_signal SIGNAL | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:99` |
| `r-jsignal` | INV-0040-2 | job_signal SIGNAL | killed | `handles_and_rights_hold_from_user_space` |
| `r-jkill` | INV-0040-2 | job_kill KILL | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:62` |
| `r-subport` | INV-0040-2 | port_subscribe port WRITE | survived | R-03 |
| `r-subwait` | INV-0006-8 | port_subscribe target WAIT | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:65` |
| `r-unsub` | INV-0040-2 | port_unsubscribe WRITE | survived | R-03 |
| `r-queue` | INV-0040-2 | port_queue WRITE | survived | R-03 |
| `r-wait` | INV-0040-2 | port_wait WAIT | survived | R-03 |
| `r-lookup` | INV-0040-2 | directory_lookup LOOKUP | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:64` |
| `k-startcheck` | INV-0003-5 | no exit check in user_thread_start | killed | `requests_and_jobs_keep_their_rules` at `t_signals.rs:69` |
| `k-movesubs` | INV-0006-4 | moving a handle keeps its subscriptions | survived | R-06 |
| `k-closesubs` | INV-0006-4 | closing a handle keeps its subscriptions | killed | `ports_keep_their_rules` at `t_ports.rs:96` |
| `k-killflag` | INV-0046-2 | job_kill does not set closed | killed | `spawn_keeps_its_rules`, `requests_and_jobs_keep_their_rules` at `t_spawn.rs:116` |
| `k-admit` | INV-0046-2 | admit ignores closed | killed | `spawn_keeps_its_rules`, `requests_and_jobs_keep_their_rules` at `t_spawn.rs:116` |
| `k-jobcreate-closed` | INV-0046-2 | job_create ignores closed | killed | `requests_and_jobs_keep_their_rules` at `t_signals.rs:169` |
| `k-within-true` | ADR-0046 | is_within always true | killed | `spawn_keeps_its_rules` at `t_spawn.rs:184` |
| `k-within-self` | ADR-0046 | is_within: only self (equivalent check) | survived | Equivalent by construction. |
| `k-within-noself` | ADR-0046 | is_within false for the job itself | survived | R-18 |
| `k-empty` | INV-0003-1 | a child gets one handle it was not given | killed | `spawn_keeps_its_rules` at `t_spawn.rs:28` |
| `k-dir` | INV-0043-1 | a child gets the directory at slot 3 | killed | `spawn_keeps_its_rules`, `ports_keep_their_rules` at `t_spawn.rs:28` |
| `k-slots` | INV-0003-6 | handles land one slot up | killed | `spawn_keeps_its_rules` at `t_spawn.rs:37` |
| `k-portclose` | INV-0006-6 | closing a port handle does not close the port | killed | hang in `closing_a_port_ends_a_blocked_wait` |
| `k-exitcheck` | INV-0004-4 | no exit check on return to EL0 | killed | hang in `handles_and_rights_hold_from_user_space` |
| `k-suspend` | INV-0003-5 | start_suspended ignored | killed | `spawn_keeps_its_rules`, `requests_and_jobs_keep_their_rules` at `t_spawn.rs:166` |
| `k-walkchild` | INV-0004-3 | walk skips child jobs | killed | hang in `requests_and_jobs_keep_their_rules` |
| `k-walkproc` | INV-0004-3 | walk skips the first process of a job | killed | hang in `spawn_keeps_its_rules` |
| `k-disp-ignored` | INV-0004-5 | the block is not used | killed | `spawn_keeps_its_rules`, `requests_and_jobs_keep_their_rules` at `t_spawn.rs:278` |
| `k-waitspur` | INV-0006-6 | second empty pass of a wait returns | killed | `handles_and_rights_hold_from_user_space`, `spawn_keeps_its_rules` |
| `k-nowake` | INV-0004-4 | request_exit does not wake | killed | hang in `requests_and_jobs_keep_their_rules` |
| `k-leak-proc` | INV-0041-1 | exit leaks the process | killed | `nothing_is_left_after_spawn_exit_close` |
| `k-keepspace` | INV-0041-2 | address space kept past exit | killed | `user_tables_obey_wx`, `running_out_of_asids_is_an_error` |
| `k-keephandles` | INV-0041-2 | handles kept past exit | killed | `an_ended_process_holds_only_its_record` |
| `k-earlymove` | INV-0042-3 | moves happen before the child exists | killed | `spawn_keeps_its_rules` at `t_spawn.rs:102` |
| `k-queuefull` | INV-0046-1 | full queue drops silently | killed | `requests_and_jobs_keep_their_rules` at `t_signals.rs:126` |
| `k-depth33` | INV-0046-3 | depth limit 33 | survived | R-04 |
| `k-depth38` | INV-0046-3 | depth limit 38 | survived | R-04 |
| `k-disp-inherit` | INV-0004-5 | a child inherits the parent's dispositions | killed | hang in `spawn_keeps_its_rules` |
| `k-depth4` | INV-0046-3 | depth limit 4 | survived | R-04 |
| `k-killed` | ADR-0004 | job_kill records Terminated | killed | `spawn_keeps_its_rules`, `requests_and_jobs_keep_their_rules` at `t_spawn.rs:192` |
| `k-badbuf` | ADR-0006 | port_wait does not probe its buffer | killed | `ports_keep_their_rules` at `t_ports.rs:115` |
| `k-exitwait` | INV-0006-6 | a wait does not see the process ending | killed | hang in `requests_and_jobs_keep_their_rules` |
| `k-pending` | ADR-0004 | Deliver does not assert REQUEST_PENDING | killed | `requests_and_jobs_keep_their_rules` at `t_signals.rs:124` |
| `k-subset-dup` | INV-0040-2 | duplicate can add rights (seen in QEMU) | killed | `handles_and_rights_hold_from_user_space` at `t_handles.rs:37` |
| `k-tableful` | INV-0040-3 | reserve does not refuse a full table (seen in QEMU) | survived | R-08 |
| `k-group` | ADR-0046 | group ignored | killed | hang in `spawn_keeps_its_rules` |
| `k-scan` | INV-0006-3 | take scans every subscription | killed | `port_wait_ignores_idle_subscriptions` (ratio 8362%) |
| `h-gen` | INV-0040-1 | generation not incremented | killed | `a_slot_is_retired_at_its_last_generation`, `a_closed_handle_never_names_anything_again`, `the_last_slot_retires` |
| `k-dispterm` | ADR-0004 | change to Terminate with a request queued does nothing | killed | hang in `disposition_change_ends_at_once` |
| `h-wrap` | INV-0040-1 | generation wraps | survived | Equivalent: a slot at generation 65535 never holds an entry, so it is never removed. |
| `h-gencheck` | INV-0040-1 | get ignores the generation | killed | `a_slot_is_retired_at_its_last_generation`, `a_closed_handle_never_names_anything_again` |
| `h-subset` | INV-0040-2 | duplicate can add rights | killed | `duplication_never_adds_a_right` |
| `h-count` | INV-0040-3 | one slot too many | killed | `a_closed_handle_never_names_anything_again`, `the_last_slot_retires`, `a_full_table_says_so_and_stays_as_it_was` |
| `h-dupright` | INV-0040-2 | duplicate without DUPLICATE | killed | `duplication_never_adds_a_right` |
| `e-level` | INV-0006-5 | Level misses what came first | killed | `level_sees_what_was_asserted_before_it_subscribed` |
| `h-retire` | INV-0040-1 | retired slots are reused | killed | `the_last_slot_retires` |
| `e-owner` | INV-0006-4 | remove_owner removes nothing | killed | `closing_a_handle_ends_its_subscriptions_only`, `many_subscriptions_come_and_go_in_any_order` |
| `e-owner-wrong` | INV-0006-4 | remove_owner removes everyone | killed | `closing_a_handle_ends_its_subscriptions_only`, `many_subscriptions_come_and_go_in_any_order` |
| `e-reserve` | INV-0041-3 | subscribe reserves no ready room | killed | `asserting_a_signal_allocates_nothing` |
| `e-reserve-q` | INV-0041-3 | queue reserves no room | survived | R-18 |
| `e-freesource` | INV-0041-4 | free_source leaves subscriptions | killed | `a_subscription_cannot_outlive_either_side` |
| `e-closeport` | INV-0041-4 | close_port leaves subscriptions | killed | `a_subscription_cannot_outlive_either_side` |
| `e-closewake` | INV-0006-6 | close wakes nobody | killed | `a_waiter_wakes_for_a_packet_and_for_a_close_only` |
| `e-queuewake` | INV-0006-6 | queue wakes nobody | killed | `a_waiter_wakes_for_a_packet_and_for_a_close_only` |
| `e-levelonce` | ADR-0006 | Level item not requeued | killed | `level_yields_on_each_wait_while_asserted` |
| `e-edgecount` | ADR-0006 | Edge coalesces | killed | `edge_gives_one_packet_per_assertion` |
| `a-move` | ADR-0042 | a moved handle may repeat | killed | `a_slot_or_a_moved_handle_twice_is_refused` |
| `a-rest` | ADR-0042 | trailing bytes accepted | killed | `trailing_bytes_flags_and_reserved_fields_are_refused` |
| `a-split` | INV-0042-2 | string bounds not checked | killed | `the_decoder_is_total` |
| `a-entry` | INV-0004-1 | a call that takes an entry point | killed | `only_thread_creation_takes_an_entry_point`, `parameters_come_from_the_allow_list` |
| `a-entrylast` | INV-0042-2 | last entry not validated | survived | R-17 |
| `a-handletrunc` | INV-0044-2 | Handle truncated | killed | `a_value_too_wide_is_refused_not_truncated` |
| `a-wordtrunc` | INV-0044-2 | flags truncated | killed | `a_value_too_wide_is_refused_not_truncated` |
| `a-interrupted` | INV-0006-6 | an Interrupted status | survived | R-05 |
| `a-deadline` | INV-0006-2 | blocking call without a Deadline | killed | `blocking_calls_take_a_deadline_and_no_masks_or_timeouts`, `parameters_come_from_the_allow_list` |
| `a-noblock` | INV-0006-2 | port_wait not marked blocking | killed | `blocking_calls_take_a_deadline_and_no_masks_or_timeouts` |
| `a-opaque` | INV-0045-1 | Opaque on another call | killed | `parameters_come_from_the_allow_list` |
| `k-level` | INV-0006-5 | Level misses what came first (seen in QEMU) | killed | hang in `spawned_child_is_waited_for_through_a_port` |
| `a-abspath` | INV-0045-1 | RelativePath accepts / | killed | `a_relative_path_is_not_absolute` |
| `a-string` | INV-0003-3 | spawn takes one buffer | killed | `spawn_takes_vectors` |
| `a-regs` | INV-0044-1 | dispatch decodes at the wrong register | survived | Equivalent today: no call has a parameter after a two-register one. |
| `a-number` | INV-0044-1 | an extra number reaches a call | killed | `every_number_reaches_its_handler`, `numbers_round_trip_and_unknown_ones_are_refused` |
| `a-waiting` | INV-0006-1 | a type with no bits | killed | `every_object_type_says_how_it_waits` |
| `a-portdup` | INV-0040-2 | a port can be duplicated | killed | `every_object_type_says_how_it_waits` |
| `l-kill` | INV-0041-1 | ending a process from outside leaks its thread and process | killed | `job_signal_reaches_every_process_of_its_tree`, `nothing_is_left_after_spawn_exit_close` |
| `l-job` | INV-0041-1 | every job_create leaks a job | survived | R-07 |
| `l-sleepcount` | INV-0041-1 | a wait woken before its deadline leaks its thread | survived | R-07 |
| `l-port` | INV-0041-1 | every port leaks | killed | `nothing_is_left_after_spawn_exit_close` |
| `k-twice` | INV-0042-1 | spawn reads its buffer twice | survived | R-16 |
| `k-noreserve` | INV-0040-3 | spawn does not reserve a slot for its result | survived | R-08 |
| `k-fifo` | ADR-0004 | requests come out last first | survived | R-18 |
| `k-ancestors` | ADR-0046 | live count stops at the job itself | killed | `spawn_keeps_its_rules` at `t_spawn.rs:212` |
| `k-groupself` | ADR-0046 | group equal to job refused | survived | R-18 |
| `k-resumeclear` | ADR-0006 | resume leaves SUSPENDED asserted | survived | R-18 |
| `k-exitclear` | ADR-0006 | exit leaves SUSPENDED and REQUEST_PENDING asserted | survived | R-18 |
| `k-posix` | ADR-0046 | posix bit not inherited | survived | Not observable before stage 6. |
| `k-exitrecord-order` | ADR-0004 | TERMINATED never asserted | killed | hang in `spawned_child_is_waited_for_through_a_port` |

`INV-0003-2`, `-7`, `INV-0004-2`, `-6`, `-7`, `-8` and `INV-0006-7`, `-9` are `not yet applicable`, so they were not mutated. Removing `TREE` was not tried: on one CPU with interrupts masked it changes nothing, so a surviving mutant would say nothing. The check of INV-0004-3 is correct for what one CPU can do, and is no evidence for SMP.

## Lifetime audit (item 3)

Traced by reading. Only R-01 is a defect here. R-15 is the one memory that outlives what the ADR says.

- **Arcs across functions that do not return.** `dispatch` drops `Calls` before `exit_current`. `exit_if_requested` and `fault_current` drop their `Arc<Thread>` in an inner block. `exit_current` drops its `Arc<Process>` before `sched::exit_current`. `kernel_thread_main` holds none.
- **Double take reachable from user space.** Each `lock` on the system call paths was followed. No lock is held across a call that takes it again. `match EVENTS.lock().take(..)` keeps the guard through its arms, and those arms take no lock. Lock order: `TREE` → job state, `TREE` → `requests` and `thread`, `EVENTS` → `SCHED`, `SCHED` → timer queue. On one CPU with masked interrupts, order matters only for re-entry, and none exists. R-01 panics with `EVENTS` held, but through a double free and not a double take.
- **`Drop` under a lock.** `Process::drop`, `Port::drop` and `Job::drop` take `EVENTS` or a parent's state. Checked at each site where a last count could go: `reap` (no lock held), `Job::create`'s error paths (the guards are declared after `job`, so they go first), `leave` (`member` is dropped after `TREE`), and `wake` dropping its `Arc<Thread>` under `EVENTS`, which is never the last count: the thread is ready or running, or is pushed to the ready queue. A failed `Table::insert` drops the object under that process's `handles` lock, and no `Drop` takes it. `walk` drops a child `Arc` under `TREE`, and it is not the last, as nothing else runs meanwhile.
- **Thread and process cycle.** `process.thread` is cleared at exit, and on the path where `admit` fails. The thread then lives in `SCHED.current`, then `dead`, and `reap` drops it and with it the last `Arc<Process>` that a thread holds.
- **Blocked threads.** A blocked user thread is held by `process.thread` until it runs its own exit, so `schedule`'s drop of `current` for a blocked thread is never the last count, and no stack is freed under itself. Waiter lists hold a second count, which `wait_masked` removes before it returns on every path.
- **`sleep_until(Infinite)`.** A user thread: held by its process, woken by `request_exit`, and it exits on return. A kernel thread: leaks a count on purpose, which only the test kernel can reach.
- **Timer entries after a kill.** `block_until` cancels its entry on return, and drops the count if the callback did not run. A thread cannot reach exit with an entry queued. `wake_sleeper` firing for a thread already ready drops a count that is not the last. `l-sleepcount` shows that this path has no leak test (R-07), not that it leaks.
- **Failed creation.** `spawn_user` failing and `admit` failing both leave no thread and no job membership. `discard` keeps the count of threads right. The child's table holds clones and is dropped without `object::close`, so the parent's port is not closed. Only the `try_arc` path is wrong (R-01).
- **Ports.** `Calls::port` holds the `Arc` during a wait, so the id stays valid after a close. A port has one handle: `decode` refuses a moved handle listed twice, and `DUPLICATE` is absent, so no second handle can close it under the first.
- **Events.** Each index in `ready` names a live subscription: `remove_subscription` purges it and `close_port` clears the list. `queued` matches membership, so `pending -= 1` cannot underflow and `reserve` keeps `set` from allocating.

## `unsafe` and `SAFETY` audit (item 4)

Every `unsafe` added or changed on the branch was read, outside `testing.rs` and the expected-fault test programs.

| Location | Result |
|---|---|
| `process.rs` `load_segment`, `map_startup_block` | Correct. The bounds come from the parser, and frames are zeroed by `alloc_zeroed`. |
| `process.rs` `exit_current`, `sched.rs` `adopt_boot_thread` | Correct: no table is always safe. |
| `sched.rs` `Send`/`Sync` for `Thread` | Correct. `context` is reached only in `schedule`, and `new_*` before it is shared. |
| `sched.rs` `schedule`, switch | The reasoning holds. The comment is incomplete (R-20). |
| `sched.rs` `block_current`, `exit_current`, `wake_sleeper`, `block_until`, `kernel_thread_main` | Correct. The count from `into_raw` is consumed exactly once. |
| `memory/heap.rs` `try_box` | Correct. |
| `memory/heap.rs` `can_allocate` | R-13. |
| `memory/stack.rs` `push_user_frame` | Correct. |
| `memory/stack.rs` `first_pc` | R-20. |
| `memory/user.rs` `peek` | Correct, test-only. |
| `arch/aarch64/usercopy.rs` | Correct. `fixup` needs the PC in the routine and `FAR_EL1` in the user range, so a fault on the kernel's side stays a kernel fault (ADR-0036). `sttrb` checks as EL0. |
| `arch/aarch64/context.rs`, `exception.rs` `write_user_frame` | Correct. `__user_thread_start` runs with `SP` at the frame, every exception masked, and an all-zero frame, so registers, FP state and TLS start at zero. |
| `syscall.rs` `UserSlice` | Correct. `at` adds an offset that the length bounds. |
| `crates/zoraya-rt` `raw_syscall`, `startup_block` | Correct. `process_exit_record` has no `unsafe` of its own, yet is the real problem (R-14). |

## ADRs against the code (item 5)

| ADR | Status | Divergences |
|---|---|---|
| ADR-0003 | accepted | None in what stage 3 builds. |
| ADR-0004 | accepted | R-10. |
| ADR-0006 | accepted | None in behaviour. The subscription "memory charged to the holder's job" does not exist, which the ADR's open question records. R-02 is a consequence of the rule that has no cap. |
| ADR-0030 | accepted | None. |
| ADR-0031 | accepted | None. Registers, stack, startup block and `port_wait`'s scratch are zeroed. ASIDs are invalidated before reuse. |
| ADR-0032 | accepted | R-22. |
| ADR-0033, ADR-0034, ADR-0036 | accepted | None. |
| ADR-0035 | accepted | R-22 (the row's text). |
| ADR-0022 | accepted | R-01. A grep of the system call paths finds no infallible allocation: `Arc::new` appears only behind `try_arc` and at boot, and each `push` follows a `try_reserve` or a reservation in `count_new`, `alloc_slot` or `KernelStack::new`. Exception: R-18's `e-reserve-q` shows that this rests on a line with no test. |
| ADR-0040 | proposed | R-22, R-15. |
| ADR-0041 | proposed | R-01, R-15, R-22. |
| ADR-0042 | proposed | None. |
| ADR-0043 | proposed | None. R-09 is a consequence of its rights rule. |
| ADR-0044 | proposed | None. |
| ADR-0045 | proposed | R-12. |
| ADR-0046 | proposed | None. |

Panics reachable from user input: `expect` and `unwrap` on the system call paths are `sched`'s "a thread is running", `process.rs`'s "a process is running", and the indexing in `zoraya-events`. All hold by construction, apart from R-01. Debug-build overflow: the sums on user values use `checked_add`, `saturating_add` or `u128` in `ticks_at`, and the rest are bounded by earlier checks. `dispositions[kind.raw()]` is bounded by `RequestKind::from_raw`.

Kernel stack: painted stacks, under `h_deep`, gave a high-water mark of **18,896 bytes** of 64 KiB. That run used a tree 32 deep, a `job_signal` over it, a `job_kill`, and dropping that chain of jobs. The arrays in `port_wait` take about 3 KiB of that budget.

## Hostile programs (item 6)

| Question | Answer |
|---|---|
| Crash the kernel? | **Yes:** R-01, shown from a system call. R-09 for a program the kernel starts. Random and malformed arguments did not, and `fuzz` and `hostile_buffers` pass. |
| Leak kernel memory past limits? | Handles (1024), user packets (1024 per port), requests (64), job depth (32) and ASIDs (255) are limited. Subscriptions are not, by ADR-0006 rule 2, and nothing charges a job (R-23). No unbounded leak was found: memory comes back when the owner ends. |
| Wedge the scheduler? | **Yes, for seconds:** R-02, and it grows with the square of the count. |
| Read another process's data? | No. Every user page is zeroed before mapping, the startup block holds only what the parent passed, `port_wait` zeroes the buffer it uses and copies only taken packets, a new thread's frame is zero, and `AddressSpace::drop` invalidates the ASID before freeing frames and the ASID. |
| Keep running after `job_kill`? | No. Every way back to EL0 passes `exit_if_requested`, and `user_thread_start` covers a thread that never ran. Blocked threads are woken, and `port_wait` checks the request before it blocks. `sleep_until` has no such check, and needs none on one CPU: a request cannot arrive between entry and blocking. Check again for SMP. |

## Could not verify

- Anything about SMP. `try_arc` and `TREE` are argued for one CPU only.
- R-02's and R-24's times under TCG: they were measured under HVF only. Beyond 40,000 subscriptions the growth is an extrapolation from the four measurements.
- Mutants ran under `auto-default` only. A mutant that fails only under another configuration would be listed as surviving.
- The kernel stack high-water mark for the idle thread, whose stack is never dropped and so was not scanned.

# TASK-31: review of `work/kwb-time` (`kernel_work_is_bounded` without the spacer mappings)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff 661a39b6...origin/work/kwb-time` at `24583771`: `crates/zoraya-mappings/src/lib.rs` and its
`tests/mappings.rs`, `kernel/src/memory/user.rs`, `kernel/src/syscall.rs`, `user/src/bin/t_bound.rs` and
`docs/backlog.md`. Read against ADR-0270 (INV-0270-1, the Q2 and Q6 notes) and the INV-0270-1 row of
`docs/adr/invariants.md`. The commit message's claims were re-run where it mattered, not taken.

## What was run

In a detached scratch worktree of `24583771` (removed afterwards), one QEMU at a time, through `make` only.

| Run | Change | Result |
|---|---|---|
| `cargo test -p zoraya-mappings` | none | 17 + 1 passed |
| the same, with the fix reverted (`at = e.va + e.length + guard`) | `lib.rs:328` | `find_gap_stays_at_or_above_low` fails (`tests/mappings.rs:176`) |
| a scratch property test (not committed): 20 000 random lists of up to 40 records, `low` at `MAP_LOW`, at a page inside or past the records, unaligned, or within 4 pages of `u64::MAX`; `high` sometimes just past the records; against a brute force (lowest of `low` and each `end + guard >= low` that fits and overlaps nothing) | none | passed |
| B0, `make test-qemu-auto-default` (HVF) | none | PASS (122, 10 expected failures). `t_bound`: fill 8417 ns and 8542 ns (the far-pages child), factor 1; far pages 63 us masked |
| R1, the same | `memory_map` in one interval: `let step = length.max(FRAME_SIZE)` (`kernel/src/syscall.rs:818`) | **`kernel_work_is_bounded` passes**: `memory_map: one step 18 us, large 1317 us masked` (bound 2 ms, units within 80); fill 9541 ns, factor 1. Two other tests failed (`cpu_ceiling_from_a_process`, `snapshots_from_a_process`): finding 1 |
| R2, the same | `memory_create`'s step point only every 65 536 frames (`kernel/src/memory/object.rs:117`) | caught: `6144 units of work between two step points, limit 80` (`t_bound.rs:172`). The same run's fill: **11167 ns, factor 2, time bound 1000 ms, under HVF**: finding 2 |
| `make check` (on this review branch) | | exit 0 |

## Findings

### 1. error (must-fix, before INV-0270-1's row may claim it; pre-existing, not made by this branch): a `memory_map` with no step point at all passes `t_bound` on every host

*Where:* `user/src/bin/t_bound.rs:162-189` (`measure`, `check`) and `memory_sites` (`:211-240`); the kernel's units
(`kernel/src/memory/user.rs:474-497`, `map_range` counts nothing per page table entry).

*What:* a step of `memory_map` writes 64 page-table entries and allocates almost no frames, so the units of the
interval are the few table frames (about 13 for 24 MiB). A regression that drops the stepping keeps the units under 80;
only the time can see it. Under HVF a step costs 15 to 18 us, so the whole 24 MiB (96 steps) is 1.3 ms, under the
2 ms bound; off HVF the limit is 1 s, which 6144 entries never reach under TCG.

*Scenario:* R1. `memory_map` in one interval: `t_bound` prints `large 1317 us masked` and passes. INV-0270-1's Status
says `memory_map ... at 24 MiB ... against one step` is checked; it is not, for this regression.

*Fix:* make the time-only work units: count the page-table entries `map_range` / `unmap_range` write (a unit per 64,
say), so an unstepped 24 MiB map is 96 + tables units and fails on every host; or, host-independent enough with a
re-run, assert `large <= K * step` per site as `inner` already does for the shared snapshot (`t_bound.rs:1256-1260`;
R1's ratio was 73). Until then, record the gap in the ADR-0270 note and the row.

### 2. error (must-fix): the calibration decides "HVF" with a 1.05 to 1.19 margin, and flipped under HVF in one of three runs; the branch adds a second, independent calibration

*Where:* `user/src/bin/t_bound.rs:46-78` (`REFERENCE_FILL_NS = 10_000`, `calibrate`), `:90-95` (`bound_ns`), and new at
`:424` (`far_pages` calls `calibrate()` itself).

*What:* any factor above 1 turns the bound from 2 ms into 1 s (500 times), not into a scaled bound. Measured fills on
this host under HVF: 8417, 8542, 9541 and 11167 ns; the last run (R2) checked every HVF case against 1 s. A slower
host core, a thermal state or one busy neighbour is enough; nothing fails or warns, the line only prints `factor 2`.
The kernel cannot inflate the fill (it is EL0 work), so a regression cannot loosen the bound by itself, but a host
does, silently, and then finding 1's class of regression passes up to 1 s. The far-pages child now calibrates on its
own, so its 2 ms check is lost whenever either of two calibrations misses, not only the parent's.

*Fix:* pass the parent's factor to the far-pages child (its role argument) instead of measuring again. Better, let the
accelerator decide: `tools/run-qemu.sh` knows what `--accel auto` resolved to; give it to the guest (a chosen
property or a boot argument) and use 2 ms whenever it is HVF, failing loudly if the calibration then says otherwise.

### 3. warning (should-fix): `teardown`'s wait is met by the long child's charge, not by what it holds

*Where:* `user/src/bin/t_bound.rs:694-707`; the charge in `kernel/src/memory/object.rs:76` (`paid.user(size)`).

*What:* `memory_create` charges the whole size before it allocates a frame, and the long child is a separate process
that the parent polls every 5 ms for `used >= peak / 16 * 15`. For `hold-long` (one 24 MiB object, then its mapping)
the condition is true as soon as the create's charge is made, so the kill lands during the create's 96 steps or
before the `memory_map`: the case printed as "job_kill of one [process holding 24 MiB]" kills a process whose 24 MiB
is not mapped, often not yet allocated (under TCG, where the create takes hundreds of ms, almost always). For
`crowd-mapped-long` the 768 page objects after the 256 mappings are about 3 MiB of 64 MiB, under the 1/16, so the kill
can come before the handle table fills. For `maps-long` it comes at about 3750 of 4000 mappings. The old `nap(500)`
was too long under HVF, but on an idle host it killed the built process; the new wait does not guarantee it on any
host. Each kill is still a bounded call, so INV-0270-1 is not broken, but the row's case ("the `job_kill` of a process
holding the room's whole memory") is no longer what is measured. Also: the long children end by themselves after
30 s (`hold(30_000)` etc.), so past 30 s the 60 s deadline can only fail, with a message that blames the build.

*Fix:* let the long child say it is built: a channel end in its spawn list, written once after the build, read by the
parent with the 60 s deadline (or a word in a shared memory object). Keep the deadline below the child's 30 s.

### 4. warning (should-fix): the far-pages case is not the same layout: its empty page tables are gone

*Where:* `user/src/bin/t_bound.rs:417-436`; `crates/zoraya-pagetable/src/lib.rs:439` ("Tables left empty stay
allocated").

*What:* the old spacers were unmapped, but their tables stayed: the old child held 22 L2 tables over about 11 264
empty L3 tables (the "45 MB of tables" of the old comment), and the snapshot's walk read about 5.8 million
descriptors in counted steps (`user.rs:727`, `WALK_READS`), the case `for_each_mapping_bounded`'s doc names ("a
walk of a space of mostly empty tables"). The new layout has 22 L2 and 22 L3 tables, about 22 000 reads. The counted
walk of mostly invalid entries is still exercised by `spread_mappings` (2016 L3 tables of 1 valid entry, about one
million reads), but not empty subtrees under valid table descriptors, and the commit message and the backlog entry
say "the same cases" / "the same layouts".

*Fix:* either say it in the backlog entry and the row, or make empty tables cheaply with the floor: map and unmap one
page in each 2 MiB of a gigabyte (512 calls, no spacer's entries) before the 22 pages.

### 5. note: `SET_MAP_FLOOR` takes any value

*Where:* `kernel/src/syscall.rs:1744-1749`, `kernel/src/memory/user.rs:349-357`.

*What:* an unaligned floor makes `find_gap` return an unaligned address, `map_pages` refuses it (`MapError`), and
`memory_map` returns `NoMemory`; a floor at or above `MAP_HIGH` is `NoMemory` too. Nothing panics (checked by
reading `zoraya-pagetable::map_with`), but a test's mistake reads as memory exhaustion. *Fix:* `InvalidArgument`
unless the value is 0 or page-aligned in `[MAP_LOW, MAP_HIGH)`.

### 6. note: the 1 s limit off HVF (the author's open item)

Under 10 hogs, 6 of 10 rounds over 1 s with units in bound are host stalls. Dropping the time check off HVF would
lose little power that the limit has now: R1 shows that what only the time sees (page-table entries, uncounted walks)
is far below 1 s under TCG at the room's limit, so off HVF the limit catches only a gross loop. Recommended: keep the
1 s limit and re-run a site that goes over it (the minimum of three, as `memory_sites` and `best_snapshot` do; a
kernel loop repeats, a stall does not); it loses only intermittent regressions. Then move the time-only work into
units (finding 1), so that off HVF the units carry the claim, as ADR-0270's Q6 note already intends.

## Checked and found sound

- **The test call is test-kernel only.** `test_calls` (`syscall.rs:1609`), `test_call` (`:1697`), its hook in
  `dispatch` (`:1823`), the field `map_floor` (`user.rs:226`), its initialisation (`:303`), `set_map_floor` (`:456`)
  and the floor's use in `find_gap` (`:350-354`) are all `#[cfg(feature = "qemu-test")]`; the normal kernel's
  `find_gap` uses `MAP_LOW`. No change to `zoraya-abi` or to any user library: the number is a private constant of
  `t_bound`, as the other test calls are. No `unsafe` was added. `process()?` / `as_mut()?` fall through to the normal
  table for a thread without a live space, as the other test calls do. The floor is per address space and starts at 0
  in the only constructor, so a snapshot's child does not inherit it.
- **The `find_gap` fix** (`zoraya-mappings/src/lib.rs:328`) is correct for every input tried: records wholly below
  `low` no longer pull the search under it, a record straddling `low` or ending within a guard of it moves the search
  to its end plus the guard, `low + length` overflow still returns `None` (`checked_add`), `low >= high` gives `None`,
  and alignment is the caller's (the result is `low` or a record's end plus the guard). The new host test fails
  without the fix.
- **Normal kernels do not change behaviour.** Every record in an `AddressSpace`'s list comes from `find_gap` with
  `low = MAP_LOW` (`map_object`, `map_reserve` then `map_commit`; a snapshot copies its parent's records), so
  `e.va + e.length + guard > MAP_LOW` and the `max` is the identity there. There is no fixed-address or hint path
  (`MAP_FIXED`, `map_at`) in the kernel; thread stacks and shared objects map through the same two calls.
- **`best_snapshot`** (`t_bound.rs:472-478`) gives the same verdict as the minimum of three for the call's interval
  (it is under the bound if and only if one of the samples is). In a green run the child's end (`:503`) and the
  units (`measure`) are now checked on one sample instead of three: fewer chances for an intermittent overrun to
  show, and fewer for a stall to fail the end. Consistent with ADR-0270's Q2 note.
- **`teardown` cannot hang on its own wait**: the loop fails after 60 s with a message (but see finding 3). The
  `frames_accounted()` after the kill (`reaper::drain`, `kernel/src/reaper.rs:197`) has no deadline of its own, as
  everywhere else in `t_bound`; it puts the reaper's part of the kill inside the measured window, which is stronger
  than the old `nap(100)`.
- **R2**: a regression that moves units (a step point dropped from `memory_create`) is still caught on every host, by
  the units, even with the bound at 1 s.
- **Docs.** The INV-0270-1 row is unchanged, `make adr-check` passes; the backlog entry matches the commit except
  findings 3 and 4.

## Check of the changed lines

A check by the `opus-review` subagent of the fix pass, `git diff 24583771..origin/work/kwb-time -- kernel user crates
tools Makefile` at `29ee64ec` (commits `c28b094e`, `56f9ad2d`, `29ee64ec`; the rest of that range is the merge of
F6 from `main`, not reviewed here). Read only: nothing was built or run but `make check`, so the commit's numbers
("111 units against 80", "factor 2, bound 2 ms") are not re-measured here.

### C1. must-fix: the undo of a cancelled or failed `memory_map` is one interval of up to the whole mapping

*Where:* `kernel/src/syscall.rs:832-835` (`unmap_range(va, 0, done)` after a failed step), `:841-848` (after
`steps::between()` returns `Cancelled`), `:855-860` (`unmap_range(va, 0, length)` after `map_commit` fails).

*What:* each clears every entry mapped so far under one lock, with no step point. Now that entries are units
(`user.rs:233-237`), the test kernel would count it; the release kernel just runs it. The backlog's new note records
it as "not reached by a test", but it is a breach of INV-0270-1 reachable by any limited job, and neither the
ADR-0270 note nor the row's list of known breaches ((a), (b)) names it.

*Scenario:* a process maps an object of its room's whole limit (2 GiB in the 2 GiB guest: 524 288 entries); its
parent (or `job_kill` of its own job from another thread) asks it to end in the middle. `between()` returns
`Cancelled` at about 2 GiB - 256 KiB done, and `unmap_range` clears about 8190 steps' entries in one interval. At
R1's 13.7 us per 64 entries under HVF, about 110 ms masked against 2 ms. `a_kill_between_steps_leaves_nothing` kills
`create-loop`, `snapshot-loop` and `contiguous-loop`, not a `memory_map`, so nothing sees it.

*Fix:* undo in steps: drop the space lock, then `unmap_range` a step at a time with `steps::pause()` between (the
mapping lock `_map` is held across the steps already, as `unmap_exact` does), then `map_abort`. Add a `map-loop`
role to `a_kill_between_steps_leaves_nothing` (24 MiB, so the units of the undo would read 96 + tables). Until then,
record it as item (c) in the ADR-0270 note and in the row.

### C2. should-fix: the INV-0270-1 row's Status is no longer true

*Where:* `docs/adr/invariants.md:509`.

*What:* it says "the masked time, 2 ms when the calibration says HVF and off HVF only a sanity limit of 1 s". Now the
bound follows `MIDR_EL1` (any CPU but a Cortex-A72 gets 2 ms), a run over 1 s under TCG is repeated up to three
times, and page-table entries are units (1 per 64). The far-pages case's 512 empty tables and C1's breach are not
mentioned either. CLAUDE.md: keep the Status column true. *Fix:* rewrite those clauses; `make adr-check` does not
compare the prose.

### C3. should-fix: under HVF a single host stall over 2 ms now fails the run, with no repeat

*Where:* `user/src/bin/t_bound.rs:132-141` (`time_ok`: `assert!(on_tcg(), ...)`).

*What:* before, a slow HVF run moved the bound to 1 s (finding 2, wrong in the other direction). Now HVF is 2 ms
always, and only TCG repeats. A vCPU thread of HVF is descheduled by the host as TCG's is; `make test-changed` runs
up to 3 guests in parallel lanes. One preemption of the vCPU inside a masked interval fails `kernel_work_is_bounded`
with nothing to show it was the host. *Fix:* apply the same repeat under HVF (three runs over 2 ms in a row fail; a
kernel loop repeats, a stall does not), and keep the line that prints each over-limit site so a flake is visible.

### C4. note: the weight of 64 entries per unit catches the loss of every step point, not a larger step

*Where:* `kernel/src/memory/user.rs:233`; `MAX_UNITS = 64 + 16` (`t_bound.rs:45`).

*What:* a legitimate step of `map_range` / `unmap_range` is 64 entries = 1 unit plus its few table frames, so an
interval may hold up to about 80 * 64 = 5120 entries before the units check fails (20 MiB of 4 KiB pages). That is
consistent with the 2 ms bound on the reference host (R1: about 13.7 us per 64 entries, 80 units = about 1.1 ms),
and every real caller is stepped: `memory_map` and `unmap_exact` call these with `n <= STEP` pages
(`syscall.rs:817-820`, `:376-379`), 1 unit each, so a 24 MiB map is 96 intervals of 1 unit plus tables, and an
unstepped one is 96 + tables > 80. But a regression that raises the step to 4096 pages (16 MiB, 64 units) passes
the units on every host and, at about 0.9 ms, the time under HVF too. Weighting closer to the time (1 unit per 4
entries makes a legitimate step 16 units) would catch a step four times too large. Also `count_entries` is not
`#[track_caller]`, so `diagnose_units` attributes every entry unit to `user.rs:237` and not to `memory_map` or the
undo.

In the release kernel the count costs nothing: `steps::count` is `#[inline(always)]` and only `let _ = units` without
`qemu-test` (`kernel/src/steps.rs:58-63`), so the division is dead code and is removed. Nothing in the release build
reads the count; step points (`between`, `pause`, `before`'s `PENDING`) do not depend on it, so ordinary builds
behave as before.

### C5. note: the re-run under TCG lets an intermittent time-only regression through

*Where:* `t_bound.rs:1325-1340` (`outer`).

*What:* a kernel loop that appears in one run of three (a kill landing in a window) passes unless it hits all three:
with p = 1/3 the run fails 1 time in 27. This only concerns work the units do not count (the units are asserted in
every run, before or regardless of the time, and an assertion exits 101, which `outer` does not repeat), and it is
what a 1 s limit could catch at all under TCG; HVF (2 ms, no repeat) still sees it. Acceptable; record it in the row.
Each repeat adds a whole `inner` (14 s idle, about 100 s under 10 hogs) to a step whose deadline is
`QEMU_SLOW_TIMEOUT` = 420 s, with 267 s seen in CI: two repeats on a slow runner can time the step out, which reads as
a hang and not as "over the time limit".

### C6. note: a long teardown child that fails before `built()` costs 25 s and a misleading message

*Where:* `t_bound.rs:788-797`, `:638-642`.

*What:* the loop waits for `READABLE` only. If the child panics before `built()`, its end of the channel closes
(`PEER_CLOSED`), and the parent naps until 25 s, then says "did not finish its build in 25 s". *Fix:* stop on
`PEER_CLOSED` too and report the child's exit record. The rest is sound: the child naps only after `built()`, so its
own 30 s end always comes after the parent sees the message; the kill now lands on a process that holds all it built
(the row's case). `crowd-mapped` closes its last small object first, so the message is not refused for room.

### Checked and sound

- **MIDR** (`kernel/src/syscall.rs:1736-1743`): inside `test_call`, which is `#[cfg(feature = "qemu-test")]`; a read
  of an ID register, `nomem`, the SAFETY comment holds. `tools/run-qemu.sh:190-194` is the only place a `-cpu` is
  chosen: `hvf` gives `host`, `tcg` gives `cortex-a72`, for every Makefile target (`--accel auto|tcg|hvf`, `--smp 4`,
  `--machine-opt gic-version=...`, `virtualization=on`, `--debug`); no `-cpu max` anywhere. The decode in
  `t_bound.rs:67-76` (implementer `0x41`, part `0xd08`) is A72's. An M-series host under HVF reports Apple's
  implementer `0x61`, so it is held to 2 ms; an unknown CPU gets the strict bound, which fails loudly and is the
  right direction. A change of TCG's CPU in `run-qemu.sh` would fail every TCG run at 2 ms, loudly; a comment there
  naming `t_bound` would save the search.
- **SET_MAP_FLOOR** (`syscall.rs:1724-1735`): 0, or page-aligned in `[MAP_LOW, MAP_HIGH)`; otherwise `InvalidArgument`
  before the space is touched. Note 5 is done.
- **Far pages** (`t_bound.rs:504-520`): a page mapped and unmapped in each 2 MiB of a gigabyte leaves 512 empty L3
  tables under 512 valid table descriptors of one L2 (tables left empty stay, `zoraya-pagetable`). The snapshot walks
  the page table, not the list (`user.rs:746`, `for_each_mapping_bounded`), so it reads those 262 144 invalid
  descriptors, about 256 walk budgets of `WALK_READS`, and counts them (`user.rs:804`, 1 unit per 128 reads): a walk
  that lost its budget would show about 2048 units in one interval on every host. Finding 4 is answered, at a 22nd of
  the old size, and the backlog now says so.
- **Units on every run:** `measure` asserts units in every run, `time_ok` returns without panicking under TCG, the
  children that time (`far-pages`, `wide-source`) return `time_verdict()` and their parents fold exit 3 in with
  `child_ended`.
- `make check` on this branch: exit 0.

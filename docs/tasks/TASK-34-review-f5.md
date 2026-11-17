# TASK-34 review: part F5 (4096 mappings per process; `libposix` keeps no handle for a private mapping)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f5` at 5f34a44f, 2026-11-16. I read it
against `docs/tasks/TASK-34-foundations.md`, `docs/research/browser-engine/plan.md` row F5, ADR-0415 item 1, ADR-0270
(INV-0270-1), the proposed ADR-0437 (INV-0437-1..3), INV-0415-1, and the bug classes of `TASK-34-review-f3.md` and
`-f8.md`. Code read around the diff: `crates/zoraya-mappings`; `kernel/src/memory/user.rs` (the list, `map_object`,
`map_reserve`/`map_commit`/`map_abort`, `unmap_commit`, `pop_mapping`, `snapshot_plan_step`, `snapshot_step`,
`SnapshotBuild::new`/`finish`, the `Drop`s); `kernel/src/syscall.rs` (`memory_map`, `unmap_exact`, `snapshot_process`
with `MapGuard` and `Freeze`); `kernel/src/process.rs` (`exit_current`'s mapping loop, `create_snapshot`);
`kernel/src/steps.rs`; `kernel/src/memory/frames.rs` (which allocations count units); `kernel/src/charge.rs`
(`Paid::drop`); `zoraya-pagetable`'s `for_each_mapping_bounded`; `crates/zoraya-posix/src/{maptab.rs,ffi.rs}` (`map`,
`unmap`, `mappings()`, `Objects::grow`, `__unmapself`); `t_bound`, `t_maps.c`, `maps.script`; the kernel test.

Tests the reviewer ran on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("make test: all checks passed").
- `cargo test -p zoraya-mappings -p zoraya-pagetable -p zoraya-posix`: exit 0, 355 tests passed (zoraya-mappings
  10, zoraya-pagetable 22, zoraya-posix 83 + its integration files).
- `make test-qemu-one` (HVF): exit 0, `PASS: 122 passed, 10 expected failures`. Its output includes
  `kernel mappings test: 4096 mappings, list 232448 bytes (56 per mapping), worst map 34 units`, `t_bound: memory_map,
  up to 4032 mappings: ... large 118 us masked`, `snapshot_process, 4032 mappings: ... large 157 us`, `snapshot_process,
  a table for each of 2016 pages: ... large 148 us`, `exit of 4000 mappings of 4000 objects: 406 us masked, 64 units;
  job_kill of one: 848 us masked, 64 units`, and `t_bound: all passed`.
- `make test-libposix` (HVF): exit 0, `libposix-auto` `PASS: 159 steps`, `libposix-maps-auto` `PASS: 17 steps`
  (`t_maps: 3900 + 134 more mappings before the limit`, then limit, heap and arenas passed).
- `make test-posix-threads` (HVF): exit 0, `PASS: 104 steps`.
- Not run: any TCG step, `make test-keeper`, `make test-posix`, the python steps, and any mutation run.
- A throwaway host program, in the scratchpad and not committed, drives `zoraya-mappings` through `find_gap`,
  `reserve_for`, `insert` and `remove_exact` the way the kernel does (finding 5).

Verdict: **fix first.** Finding 1 is the part's acceptance test. Findings 2 to 4 are claims of INV-0437-1/-2 that no test
checks, on the paths this part makes common.

## Findings

### 1. error: the plan's "done when" (the CPython command) and its "ADR-0320's mapping measurement repeated" are run by no step, and the documents say CI runs them

Where: `tests/posix-core/maps.script:3`, `tests/posix-core/t_maps.c:13-23`, `docs/design.md:2399` ("CPython's own
command is shown by CI's python step").

What is wrong: plan row F5's "done when" is `python3 -c "x=[bytearray(100) for _ in range(5_000_000)]"` finishing in a
2 GiB guest. No test in the repository runs it. `grep -rn "bytearray(100)\|5_000_000"` over `tests/`, `ports/`,
`tools/` and the `Makefile` finds only the comment in `t_maps.c`, plus an unrelated line in `tools/test-dns`. The
python steps boot with `PYTHON_QEMU ?= --mem 512M` (`Makefile:726`), so no python step is a 2 GiB guest either. The
plan's other check, "ADR-0320's mapping measurement repeated" (the 4000 threads / 30 forks / recursion-depth figures
of ADR-0320 line 319), is not repeated, and nothing says so. `t_maps arenas` is a C stand-in: it shows what the
command needs from libposix and the kernel, not that CPython's obmalloc runs to the end. CI cannot "show" the command,
because no step contains it. The candidate run will go green without ever running the acceptance test.

Scenario: CPython's obmalloc frees arenas, or its frame chunks map in a pattern `t_maps` does not use (for example
`munmap` of arenas interleaved with new ones, which is where finding 5's fragmentation comes from). Or CPython hits
another limit (the handle table, the job's kernel memory). The part is merged and tagged `stage-f5` with that
failure unseen.

Fix: add a step that runs the plan's command in a 2 GiB guest, as a `tests/python/` script booted at `--mem 2G`, or a
line in `maps.script` if the image has `python3`. Repeat ADR-0320's measurement there: the free count after 4000
threads and 30 forks, and the recursion depth that now ends in `MemoryError` or in the room's memory. Until then,
correct `design.md:2399`, `maps.script:3` and `t_maps.c:23`: the command is not run.

### 2. warning: the kernel test's "the charge follows the list down" never empties a chunk, so the release on removal is not checked by the test INV-0437-1 names

Where: `kernel/src/testing.rs:2897-2929`; `kernel/src/memory/user.rs:363-369` (`release_records`) and `:523-534`
(`unmap_commit`).

What is wrong: the 4096 mappings are added at the top, so the list is 128 full chunks of 32. The test then removes the
last record (chunk 127 keeps 31) and every second one of `places[0..4095]`, 16 of each chunk of 32. No chunk becomes
empty, `bytes_held()` never decreases, and `release_records` releases 0 every time. The assertion
`kernel_bytes() == base + tables + mapping_bytes_held()` therefore holds for any `release_records`, including one that
releases nothing. The final `drop(space)` check passes through `Paid::drop` (`charge.rs:272`), which refunds whatever
is left, not through the release path. INV-0437-1's "after every removal ... a mapping made and unmapped leaves it as
it was" names this test as its check. ADR-0437 rule 2 credits `t_thread` instead, and no mutation run is on record
for this branch (`build/mutants/report.md` holds only the libposix mutant run of ad03d4b5).

Scenario: a later change drops `self.release_records(before)` from `unmap_commit` (the mutant
`F5-removal-keeps-the-charge`). A process that maps and unmaps in a loop leaks one chunk's charge (1816 bytes) each
time a chunk empties, and the job reaches `OverQuota`. The F5 kernel test still passes.

Fix: in the kernel test, unmap the 32 records of one chunk (for example the top 32) and assert that
`mapping_bytes_held()` fell by `CHUNK_BYTES` and the job's kernel bytes with it. Unmap all of them and assert the
charge is `base` plus tables. State in the mutant list which test kills `F5-removal-keeps-the-charge`.

### 3. warning: `map_abort` after the list grew is not tested in the kernel; the only refusal tested is the one before any growth

Where: `kernel/src/testing.rs:2931-2946` (the "poor" job, `Some(used + 64)`); `kernel/src/memory/user.rs:372-376`,
`:409-418`; `kernel/src/syscall.rs:837, 848, 856-859`.

What is wrong: with a limit of `used + 64`, `reserve_record`'s charge of `max_growth()` (1816 bytes) fails first, so
the mapping is refused before the list or the tables change. The path that commit d65e55ba adds ("a refused mapping
gives its room back") is a different one: the chunk is made and charged, then `map_pages` or `alloc_table` is refused,
then `map_abort` → `trim` → `release_records`. No kernel test reaches it, and the mutant lists have no mutant on
`map_abort`. Reading the code, the path is correct (`trim` pops the empty chunk `reserve_for` appended, and
`release_records` gives back exactly that chunk). But INV-0437-1's "refuses the mapping with `OverQuota`, nothing
changed" is checked only for the case that changes nothing anyway.

Scenario: a change that skips `map_abort` in one of `memory_map`'s three error paths leaves an empty chunk charged
each time a map is refused for a table frame. A job close to its limit then loses 1816 bytes per refusal, and no test
notices.

Fix: in the same test, set the poor job's limit to `used + CHUNK_BYTES + 100`, so the chunk is charged and the first
table frame is refused. Assert `Quota`, `mapping_count() == 0`, `mapping_bytes_held() == 0` and the job's bytes equal
to `used`. Add a mutant that removes `self.map_abort();` from `map_object`'s `map_pages` error path.

### 4. warning: the step point F5 adds before `memory_unmap` frees the last holder is untested, and it is the path every libposix `munmap` now takes

Where: `kernel/src/syscall.rs:399-404` (`steps::before(holder.weight()); steps::count(1); drop(holder)`).

What is wrong: before F5, libposix's `munmap` closed the object's handle after `memory_unmap`, so the frames were
freed by the close. Now the handle is closed at `mmap` time (`ffi.rs` `map`, `Objects::grow`, `mappings()`), so the
frames of every private mapping and heap step are freed inside `memory_unmap`, by the drop of its holder. That drop
can free up to a step of frames (64 units) on top of the last `unmap_range` step and `unmap_commit`. The step point
keeps them in separate intervals. No test measures it: `t_bound`'s `memory_unmap of one of 4032` unmaps a one-page
mapping whose handle stays open (not the last holder), and `t_maps` checks no units. The report lists the mutant on
this step point among the four never run. By reading, it survives every listed test.

Scenario: the step point is removed or moved after the drop. A CPython `munmap` of a 1 MiB arena then holds IRQ masked
for its last unmap step plus 64 frames freed, about 130 units against the limit of 80, and nothing fails.

Fix: add a `t_bound` case that maps a 1 MiB (and a 24 MiB) object, closes the handle, and `measure`s the `memory_unmap`.
Run the never-run mutants (`F5-exit-keeps-the-charge`, `F5-worst-growth-kept`, the walk bound in `zoraya-pagetable`
through `make mutants-host CRATE=zoraya-pagetable`, the unmap step) before the merge, as TASK-34 requires for a kernel
part. By reading, `F5-worst-growth-kept` is killed by the kernel test's exact check at one mapping. `F5-exit-keeps-the-
charge` is close to equivalent: the charge is refunded when the reaper drops `Paid`, only later. Record that, instead
of leaving the mutant unrun.

### 5. warning: chunks never merge, so the list's chunk count is bounded only by the record count; the ADR's costs and "moves at most a chunk" hold only for a list filled upward

Where: `crates/zoraya-mappings/src/lib.rs:107-131` (`reserve_for`), `:207-235` (`remove_exact`, `drop_if_empty`);
`docs/adr/0437-the-mapping-list-as-built.md` Option E and rule 2.

What is wrong: a chunk splits when full and is freed only when empty. Records removed from a chunk leave it
under-full for good. The only bound on chunks is "none empty but the last", that is `len + 1`. The throwaway host
program follows the kernel's placement (`find_gap` lowest first, a guard page, 4096 at most). It fills the list, unmaps
all but every second record, and refills with mappings half the size, repeatedly. It reached **485 chunks for 4096
records**: 880 KB charged against the 232,448 bytes the ADR states for 4096. This is a simple pattern; a chosen one
can approach 4097 chunks, about 7.4 MiB of kernel heap per process. The job is charged, so this is no escape from the
quota. But (a) a process's kernel cost is up to 32 times what ADR-0437 and `design.md` state. (b) Each split and each
chunk removal moves the outer `Vec` of up to 4097 headers (98 KB, `chunks.insert`, `chunks.remove`). Its `try_reserve(1)`
reallocates up to about 196 KB under the space lock, with no `heap::prepare` before it. Both are counted as one unit,
which contradicts the module doc and Option E ("an insertion or removal moves at most a chunk (about 4 KiB)").

Scenario: a long-lived interpreter that `mmap`s and `munmap`s arenas of varying sizes (CPython, a JS engine's GC
heap) is charged megabytes of kernel memory for its list. A small room's limit refuses its mappings with `OverQuota`
well before 4096.

Fix: on removal, merge a chunk with its neighbour when the two together fit in one chunk (no allocation is needed: the
records move into the existing chunk's capacity, and the emptied chunk is freed and released). That gives
`chunks <= 2 * len / CHUNK + 2`. Add a host test with this unmap and remap pattern that asserts that bound. Or, if
merging is not wanted, state the worst case in ADR-0437 (charge, outer move, outer reallocation) and count the outer
move as `chunks * 24 / 4096` units, as `count_list_copy` does for the frame list.

### 6. warning: the note on the surviving mutant `F5-finish-ignores-tables` is wrong: the layout that kills it does not need a 1 GiB object

Where: `tools/mutants/mappings-f5.toml` (`F5-finish-ignores-tables`); `kernel/src/memory/user.rs:882`.

What is wrong: the note says three tables per mapping "needs mappings 1 GiB apart, which needs a 1 GiB object".
Two tables per mapping are enough, an L2 table and an L3 table, which mappings 1 GiB apart get. They do not need a
1 GiB object: the kernel lets one object be mapped many times, and `t_memory` maps one object 4096 times. Sixteen
mappings of one 64 MiB object (or 512 of a 2 MiB one) take a gigabyte of address space as a spacer. A page goes after
each spacer, and the spacers are unmapped only once every page is placed, so the lowest-gap search does not refill
them. In `finish`, a mapping then costs one uncounted page placement, two table frames (one unit each,
`frames::alloc`) and two record units. Without the `tables >= STEP / 2` test the step ends at `pages >= 64`: 22
mappings, about 88 units against the limit of 80, which the checker would catch. Seventeen to twenty-two such
mappings need about 34 to 44 MiB of table frames. Unmapped tables stay with the space, so the case belongs in a child
process, whose tables go with it. That is affordable in the 2 GiB guest. This bound is new in F5, and its only check
is the reading.

Fix: add the case (a child of `t_bound` that builds the layout and snapshots itself), or correct the note to say the
hole can be closed and why it was left open. `F5-snapshot-step-ignores-tables` (the process's own image and stack
pages) is plausibly out of reach of a small test. Its note is fair, but say that the bound for a sparse image is then
unverified.

### 7. note: `reserve_record` charges a whole chunk even when the chunk has room, so a job close to its limit is refused a mapping that adds nothing to the list

Where: `kernel/src/memory/user.rs:347-359`; `crates/zoraya-mappings/src/lib.rs` `max_growth`.

`max_growth()` is always `CHUNK_BYTES`. A job with less than 1816 bytes of headroom gets `OverQuota` for a mapping into
a chunk that has room (one that also needs no new table frame). This is not a leak (the charge is given back), but it
is a refusal ADR-0261 point 6 does not need. Fix: `max_growth_for(va)`, which returns 0 when `chunk_for(va)` is not full.

### 8. note: `record` drops the holder it says it gives back, under the space lock, before the caller unmaps

Where: `kernel/src/memory/user.rs:378-387` (doc: "Gives the holder back if there was none"); `:194-198`;
`kernel/src/syscall.rs:856-859`.

`insert`'s `Err(entry)` is discarded by `map_err(|_| ...)`, so the holder is dropped while the range is still in the
page table. The unmap comes after. The path cannot be reached under the reserve contract (I checked every
`reserve_for` case: empty list, room, split, append at the top, a trailing empty chunk). In `memory_map`, the local
`frames` clone keeps the object alive anyway. But for `map_object` with a `Holder::Device` (`syscall.rs:1105`) the
order would be wrong if the path were ever reached. Fix: return the entry, unmap, then drop it, or make the doc say
what the code does.

### 9. note: `AddressSpace::drop` now pauses (`steps::before`), so every place that drops a space that still has mappings must hold no spinlock

Where: `kernel/src/memory/user.rs:749-767`.

I checked the paths that can drop a space with records: `create_snapshot`'s failure (`process.rs:780-787`), the
`?` returns of `snapshot_process` and `SnapshotBuild::finish`, and a failed spawn. None holds a spinlock. `exit_current`
empties the list before the `flush` after which nothing may yield, and `pop_last` cannot return `None` while records
remain. No comment states this argument, and the test kernel's `assert_unlocked` in `pause` is the only guard. Fix: a
sentence at the `Drop` saying which callers may hold the last reference to a space with mappings.

### 10. note: the documents disagree on the chunk

Where: `docs/adr/index.md:243` ("chunks of 64 records"; the ADR and the code have 32); `crates/zoraya-mappings/src/
lib.rs:10` ("about 2.3 KiB" a chunk; measured 1816 bytes, ADR-0437 rule 2); ADR-0437 Option E ("about 4 KiB").

## Checked and sound

- `zoraya-mappings`, by reading and by the host tests:
  - `chunk_for`'s partition is monotone, with the empty chunk only last.
  - `reserve_for` makes nothing visible before both allocations succeed (atomic on `NoRoom`).
  - Every `reserve_for` case leaves `insert` room at the same `va`: empty list, room, split at the half, append at the
    top, a trailing empty chunk.
  - `trim`, `pop_last` (skips the trailing empty chunk), `remove_exact` (exact length), `iter_from` on an empty list
    and past the end.
  - `find_gap` is unchanged from the old kernel code: every record lies in `[MAP_LOW, MAP_HIGH)`, because
    `map_object` and `map_reserve` are its only producers. Its overflow returns `None`.
  - The charge equals `chunks * CHUNK_BYTES` after every change that goes through `reserve_record`,
    `release_records` and `map_abort`. `Paid::drop` refunds the remainder when the reaper drops it. No underflow:
    `saturating_sub`, and `release_kernel` takes only what was charged.
- The plan in steps: `snapshot_process` holds `MapGuard` and `thread::Freeze` for the whole call, and every mapping
  change (`memory_map`, `memory_unmap`, the device map, thread exit's unmap) takes `MapGuard`. The list cannot change
  between plan steps or between `map_reserve` and `map_commit`. A kill between steps ends with `Cancelled`, and
  `memory_map`'s cancel path calls `map_abort`.
- `for_each_mapping_bounded`:
  - Skipped entries are not read and not counted.
  - `resume` is the first entry not read, and `snapshot_step` takes its maximum with `next`.
  - Progress is guaranteed: at most 4 reads to reach `from`, against 1024.
  - The host test checks every leaf exactly once at limits of 6 to `usize::MAX`.
- Exit and kill: `exit_current` pops one mapping per lock hold, with `before(weight)` and `count(1)` (`t_bound`: 64
  units for 4000 mappings of 4000 objects). The `Drop` and `HeldObjects` loops are in steps.
- libposix:
  - `MapTable` (first free slot, exact removal, zero size refused).
  - `mappings()` makes the table under `MEM`, and the table does not list itself, so `munmap` cannot end it. Its
    `// SAFETY:` argument holds.
  - A `fork` child has the copy at the same address, with the static pointing at it. A `posix_spawn` child starts
    with `None`.
  - `__unmapself`: the slot is freed before the kernel unmaps, and a concurrent `mmap` cannot get the same address.
  - Memory descriptors (F3) go through `memory_mmap`/`memory_munmap` before the table and are not affected.
- `invariants.md`: three new rows, Total 845, INV-0415-1's Status. `make adr-check` passed.
- The rest:
  - `test-map.txt` has a line for `crates/zoraya-mappings/`.
  - `cap_memory!()` is in both test files, and the library has `#![forbid(unsafe_code)]`.
  - No host paths in the diff.
  - `t_memory` runs inside the kernel test `memory_objects_from_a_process`, at the full 4096.

## The report

- The CPython command is "for CI to show". No CI step runs it (finding 1).
- Fork at 4096 is covered by `t_bound` only (4032 plus the program's own, about 4096). The plan steps there are the
  same as at 4096, and the units are asserted on every host.
- `make test-posix-threads` passed in this review. `make test-keeper` and `make test-posix` were not run by the
  reviewer either.

## Re-check

Reviewed `git diff 09b47261..d780deff` (seven commits) against findings 1 to 10.

Verdict: **fix first, small.** Findings 1 to 10 are fixed. Finding 11 is a hole in the tests of the merging code that
fixed finding 5, and the commit message calls the mutants that show it equivalent. Its fix is one host test and a
corrected note. Findings 12 to 15 are notes.

### Findings 1 to 10

1. Fixed. `make test-python-mem` boots the python volume at `--mem 2G` (`--expect-line "2048 MiB  RAM"`) and runs the
   plan's command (with a `print` added, then `status`). `tests/python/mappings.py` repeats ADR-0320's measurement. The
   step is in `TEST_STEPS`, `.PHONY`, `ci-tests.txt`, `test-lanes.txt` (lane and `PAR_LONG` opt-in), `plan-lib.sh` and
   the map. The workflow takes its jobs from `TEST_STEPS`, and `make check` (with `check-ci-jobs`, `check-ci-plan`,
   `check-test-lanes`) passes. The assertions pin no number: the free count is compared with the start, within 2.
2. Fixed. The kernel test empties the top chunk (31 records) and asserts that the list and the job's bytes fall by one
   chunk. Merging does not change this: 1 + 32 is not under a chunk, so the chunk empties and is not merged first.
3. Fixed. The poor job gets room for a chunk and not for a table frame: `Quota`, no mapping, no chunk, bytes as before.
   `F5-abort-keeps-the-chunk` is listed.
4. Fixed. `t_bound` has a case for `memory_unmap` of the last holder (64 pages and 24 MiB). The mutant on the step point
   is listed as equivalent under the unit check. I accept this. The frees count a unit each (`frames::free`), so the
   unit check would see the work, and by the note it stays under 80. I did not run the mutant.
5. Fixed, apart from finding 11. Chunks merge on removal and spill to a neighbour before they split. Checked by reading
   below.
6. Fixed. `t_bound far-pages`: 22 one-page mappings 1 GiB apart, built in a child, with 352 unmapped 64 MiB spacer
   mappings. The mutant is in `tools/mutants/mappings-f5-2g.toml`. By reading it is killed: 22 x (2 tables + 2 record
   units) is 88 against 80. I did not run the mutant. The case ran in this re-check (161 us masked). It runs wherever
   more than 40,000 frames are free, a 512 MiB guest included, not only "the 2 GiB guests".
7. Fixed (`growth_for`). It agrees with `reserve_for` in every branch: room, spill up, spill down, split. Both use
   `route`. The host test asserts equality over 30,000 random operations.
8. Fixed. `record` and `map_commit` give back the record or holder. `map_object` drops it after the unmap and
   `map_abort`, and `memory_map` drops it after the space lock is released.
9. Fixed (the comment at `Drop`).
10. Fixed (the index says 32, `lib.rs` says 1816 bytes, Option E gives the figures).

### 11. warning: the forward merge in `rebalance` is not tested; three of the 11 host survivors are this hole, not equivalents

Where: `crates/zoraya-mappings/src/lib.rs:275` (`if c + 1 < self.chunks.len() && ... < CHUNK`);
`build/mutants/cargo/mutants.out/missed.txt`; commit d92275b4 ("11 survivors, all equivalent").

What is wrong: the survivors `275:22 replace < with >` (the forward branch never runs), `275:93 replace < with ==` and
`275:66 replace + with *` (it merges less often) all switch off the merge with the chunk after. No host test needs that
merge. `small_neighbours_merge_and_the_charge_follows` takes records from the end of the last chunk, so its merge is the
backward one. The pattern test removes in address order, so it too merges backwards only. The random test runs
`check` only every 211 rounds, and its inserts heal a broken pair before then. With the forward branch gone, removals
from a chunk whose next neighbour is small leave pairs under 32 records. Then "every two neighbours hold at least
32" fails, and so does the chunk bound that ADR-0437 rule 2 states (468,528 bytes at the worst). The bound rests on
this branch. Only `275:93 < → <=` and `278:80 < → <=` (merging at exactly 32) are equivalent: the merged chunk is full,
fits its capacity, and keeps the pair invariant.

Evidence: a throwaway host test, not committed. It fills 96 one-page records (3 full chunks), removes 31 from the
top (32, 32, 1), then removes 2 from the middle chunk (30 + 1 must merge). It passes on d780deff. With mutant `275:22`
applied, the 14 committed tests of `tests/mappings.rs` pass and this test panics at `lib.rs:345` ("chunks 1 and 2 could
be one").

Fix: add this case (and its mirror for the backward branch) to `tests/mappings.rs`, asserting `check()` and
`chunk_count() == 2`. Correct the survivor list in the mutant notes or the report: the forward-merge three are killed
by the new test, and the rest are equivalent.

### 12. note: `F5-worst-growth-kept` is not equivalent when `reserve_for` fails

Where: `kernel/src/memory/user.rs:360-364`; the note in `tools/mutants/mappings-f5.toml`.

The note says that the difference to give back "is always 0". That is true when the chunk is made. When `try_reserve`
or `new_chunk` fails (`NoRoom`), `grown` is 0 and `worst` (a chunk) must be given back. The mutant keeps 1816 bytes
charged, with nothing allocated, until the space is reaped. No test can reach that path without injecting a heap
failure, so it survives. Its note should say this, instead of "equivalent".

### 13. note: a refused mapping after a split in the middle keeps the new chunk and its charge

Where: `crates/zoraya-mappings/src/lib.rs:164-168`, `trim`; `kernel/src/memory/user.rs:378-382`; ADR-0437 rule 2 ("a
mapping that fails after its room was made gives the room back").

`trim` removes only a trailing empty chunk, which is what the top split makes. A split in the middle (16 and 16, both
neighbours full) stays when the mapping is then refused. The lowest-gap search often places a mapping in the middle once
mappings have been unmapped. The charge is exact (the chunk exists and holds records) and stays within the
`2 * len / 32 + 2` bound, so this is no leak. But the job pays one chunk more for a refusal, and the ADR's sentence holds
only for the top split. Fix: say so in rule 2, or undo the split in `map_abort` (merge the two halves back, which
allocates nothing).

### 14. note: the outer list's slack is up to 12 KiB, not 6

Where: ADR-0437 rule 2 ("at most 24 bytes for each chunk the list ever had at once, 6 KiB at the worst");
`lib.rs:77-81`.

`try_reserve(1)` doubles the capacity. Adding the 257th chunk makes it 512 places, 12,288 bytes. This is all slack once
the list shrinks. That is two places for each chunk the list ever had at once, not one. The amount is immaterial and
uncharged, but the stated figure is wrong.

### 15. note: a change to the mapping list's kernel code runs neither `test-python-mem` nor `test-libposix` on a candidate

Where: `tools/test-map.txt:61, 66-67` (`kernel/`, `crates/zoraya-pagetable/`, `crates/zoraya-mappings/` map to
`test-qemu` and a few others).

This branch runs everything because it changes the `Makefile`. A later change to `crates/zoraya-mappings/` or
`kernel/src/memory/user.rs` is covered on a candidate by `test-qemu` only. The CPython acceptance test and `t_maps`
then run only nightly. This follows the map's existing practice for `kernel/`, so it is a note. Fix: add
`test-libposix test-python-mem` to the `crates/zoraya-mappings/` line.

### Checked and sound

- The chunk list (`lib.rs`), by reading:
  - Address order and no overlap are kept by both spills. Spill down moves a chunk's first record to the end of the
    one before. Spill up moves its last record to the front of the next, and only when `route` kept the record inside
    the chunk, so `va` is below the moved record. `insert`'s `route` finds the same chunk as `reserve_for`'s did,
    after either spill and after either split.
  - The pair invariant (every two non-empty neighbours hold at least 32) is kept:
    - A spill takes one record from a full chunk, and its other neighbour is non-empty.
    - A split leaves 16 + 16 next to full neighbours.
    - A removal merges at most once. After a forward merge no further merge applies, and after a backward one no
      forward one applies.
  - The invariant gives at most `2 * len / 32 + 1` non-empty chunks, plus the trailing one.
  - Merging allocates nothing: the sum is under the capacity of 32 that `new_chunk` reserved exactly.
  - No chunk `Vec` ever grows past 32. A spill pushes into a chunk under 32, and so does a merge.
  - The outer `Vec` allocates only in `reserve_for` (at most 12 KiB, within the fixed growth that `heap::prepare`'s doc allows without a prepare).
- The charge follows the chunks:
  - `growth_for` equals what `reserve_for` adds.
  - A merge and an emptied chunk lower `bytes_held` by exactly one chunk, which `release_records` returns.
  - `map_abort` after a spill changes nothing, and after a top split it trims the chunk.
  - No underflow: `saturating_sub`, `release_kernel` capped.
  - A merge could remove a trailing empty chunk only while a reservation is pending. `MapGuard` forbids removals then.
- Bounded time:
  - A removal moves at most 31 records, plus one merge of at most 31 records and up to 257 headers, about 9.6 KB.
  - An insertion moves at most 31 records twice (spill and insert), or a split: 16 records, an outer insert, and an
    outer reallocation of at most 6 KB.
  - Both are counted as one or two units (a unit is 128 records read, about 7 KB). They are under-counted by at most
    half a unit, which is immaterial against 80.
- ADR-0437: 258 x 1816 = 468,528 and 128 x 1816 = 232,448 agree with the code and with the bound `check` asserts.
- `tests/python/mappings.py`: the free count measures one-page private `mmap`s and closes them all. The ±2 tolerance
  covers a detached thread still leaving. The recursion and the bytearrays are asserted by output and exit status.

### Run in this re-check (d780deff, HVF)

- `make check`: exit 0.
- `cargo test -p zoraya-mappings -p zoraya-pagetable -p zoraya-posix`: exit 0 (`mappings`: 14 passed; `pagetable`: 22
  passed; every `posix` binary passed).
- `make test-qemu-auto-smp4-2g`: PASS, 122 passed, 10 expected failures. `t_bound` printed "22 pages 1 GiB apart: 161
  us masked" and "memory_unmap of the last holder: one step 50 us, large 32 us masked".
- `make test-python-mem`: PASS, 24 steps, about 18 s with the image build. "free mappings at the start 4086", "free
  mappings after 4086", "recursion to 200000 frames", "bytearrays 5000000".
- Not run: any mutant of the kernel lists, `test-python-mem` under TCG (what CI uses). Its TCG time is unmeasured, and
  the candidate run in progress will show it against `PYTHON_MEM_TIMEOUT` 900 s.

# TASK-34 review: part F6 (read-only segments shared between the processes of one image; `libposix` spawns its own image)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f6` at eea2efa9, 2026-11-16. I read it
against `docs/tasks/TASK-34-foundations.md`, `docs/research/browser-engine/plan.md` row F6, ADR-0415 item 2 (INV-0415-2),
ADR-0108, ADR-0270 (INV-0270-1), ADR-0431, ADR-0437, the proposed ADR-0438 (INV-0438-1..4) and the bug classes of
`TASK-34-review-f3.md`, `-f4.md` and `-f5.md`. Code read around the diff: `crates/zoraya-elf` (`from_table`,
`read_segment`, `Kept`); `crates/zoraya-mappings` (`find_gap`); `kernel/src/process.rs` (`image_from`, `keep_program`,
`load_kept`, `load_segment`, `tick`, `create`); `kernel/src/memory/object.rs` (`make`, `copy_from`, `weight`, `Drop`);
`kernel/src/memory/user.rs` (`Record`, `map_kept`, `map_kept_page`, `unmap_range`, `unmap_commit`, `snapshot_plan_step`,
`snapshot_step`, `SnapshotBuild::new`/`finish`, `Drop for AddressSpace`); `kernel/src/syscall.rs` (`UserSlice`,
`image_create`, `spawn`, `memory_map`, `unmap_exact`, `dma_pin`); `kernel/src/charge.rs` (`Paid`); `kernel/src/reaper.rs`;
`crates/zoraya-handle` (`reserve`, `keep_room`, `insert`); `crates/zoraya-posix/src/{process.rs,ffi/procs.rs}`
(`own_image`, `resolve_program`, `load_fd`, `child_record`, `spawn_child`, `zoraya_child_memory`, `MAX_BUFFER`);
`tests/posix-core/t_shared.c`; `user/src/bin/t_image.rs`; the five kernel tests.

Tests the reviewer ran on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("make test: all checks passed").
- `cargo test -p zoraya-elf -p zoraya-mappings -p zoraya-posix`: exit 0, 358 tests passed, 0 failed.
- `make test-qemu-one` (HVF): exit 0, `PASS: 127 passed, 10 expected failures`; the five new tests print
  `ZORAYA-TEST ok`.
- `make test-libposix` (HVF): exit 0, `libposix-auto` `PASS: 164 steps`, `libposix-maps-auto` `PASS: 17 steps`. Its
  output includes `t_shared: image keeps at least 548864 bytes; L's job: 1455040, +2176256 for a spawn of the file, +139264
  +139264 for two of its image`, `children (...): 1315904 1315904 1315904 1315904 bytes`, `t_shared: passed`, and
  `zoraya-elf-check`'s `t_shared: ok, entry 0x4332a0, r-- 0x400000+0x2329c, r-x 0x4332a0+0xc81dc, rw- 0x50b480+0x1a84`.
- `make test-posix-threads` (HVF): exit 0, `PASS: 104 steps`.
- Not run: any TCG step, `make test-posix`, the python steps, any mutation run. Finding 2's "the check passes without
  the image's charge" is arithmetic on the numbers above, not a mutant I ran.

## Findings

### 1. error: the plan's "done when" (`top` shows its text once for the three children) is not shown, and the documents state its result as fact

- **Where:** `docs/research/browser-engine/plan.md:29` (row F6: "a C test that spawns itself three times: `top` shows
  its text once for the three children"); `docs/tasks/TASK-34-report.md` F6 notes ("`top` shows a room's memory, so the
  image's pages appear once in the creator's room"); `docs/adr/0438-shared-read-only-segments-as-built.md` item 3 and
  "Effect of the vision" ("charged to the room that started it (which `top` shows once)"); the note added to ADR-0415.
- **What is wrong:** no step runs `top` around a program that spawned itself. `t_shared` reads the job of its launcher L
  and of each child through `zoraya_child_memory`; it never reads the job that paid for the shared text (the shell's,
  which made T's image: T, L and the three `s` children all run the shell's image). The report's sentence about `top` is
  an inference, not a run, and CLAUDE.md forbids reporting a behaviour without the command that shows it. `TASK-34`
  names the "done when" as the acceptance test; F5's review made the same gap an error (its finding 1).
- **Scenario:** a regression where the kernel copies each spawn's text into frames charged to the image's creator (each
  spawn adds 200 pages to the shell's room) passes `t_shared` (L's job and the children's jobs are unchanged) and every
  check the report cites for "once"; only the kernel test with `hello` would see it, and the C program's room is never
  looked at.
- **Fix:** `top` is a zosh builtin that a script already drives (`tests/shell/browser-guest.script:21`, F8). Add a step
  that runs `t_shared` (or a variant that holds its three children) in a room, captures `top`'s room line before and
  while the children live, and checks that the room grew by the children's own pages and not by three texts; or have
  the architect accept `t_shared`'s job-level proxy as the "done when" and say so in the report, with the `top`
  sentences rewritten as inferences.

### 2. warning: `t_shared`'s "a spawn of the file costs the image's pages" is met by libposix's 1 MiB read buffer, so it does not show that the spawner pays for the image

- **Where:** `tests/posix-core/t_shared.c` (`if (d_path < d_self[0] + kept)` and `kept_at_least()`, which is half the
  span); `crates/zoraya-posix/src/process.rs:93,1240` (`MAX_BUFFER`, made at a process's first file read and kept);
  INV-0438-2's row and `tests/posix-core/libposix.script`'s comment, which cite `t_shared` for "the job of a launcher grows
  by the image's pages after one spawn of its file".
- **What is wrong:** the `p` spawn is L's first file read, so it also creates and keeps the 1 MiB transfer buffer. From
  the run: d_path - d_self = 2,176,256 - 139,264 = 2,036,992 bytes, about 497 pages; the image keeps about 239
  (`r--` 36 pages, `r-x` 0x433000..0x4fc000 201 pages, `rw-` at most 2), and the remaining ~258 are the buffer (256
  pages) and records. Without any image charge, d_path would be about 139,264 + 1,048,576 = 1.19 MB, above the
  threshold d_self + kept = 688,128, so the check would still pass. The kernel test `images_share_their_read_only_pages`
  does catch `F6-creator-does-not-pay`; `t_shared` would not, and it is the C half INV-0438-2 cites.
- **Also:** `after_path` is read soon after `load_fd` closed the file's 610-page object, which goes to the reaper with
  its charge (`MemoryObject::drop`, > `STEP` frames). If the reaper has not freed it when T reads `after_path`, the next
  read is smaller and the test fails with "L's job shrank". Under HVF the window is long and this did not happen; under
  TCG it is a timing dependency the test does not control.
- **Fix:** warm the buffer before `before` (L reads one byte of its own executable at start), wait for the reaper
  (re-read L's usage until two reads agree) before taking `after_path`, and compare d_path - d_self with the image's
  exact kept size computed from the program headers (`getauxval(AT_PHDR)`/`AT_PHNUM`, `Program::kept`'s rule) within a
  few pages, not with half the span.

### 3. warning: INV-0438-4's test does not measure the copy or the instruction-cache loop of `image_create`

- **Where:** `kernel/src/testing.rs` `an_image_of_a_large_program_is_bounded` (with `with_shared_bss`);
  `kernel/src/process.rs` `keep_program` (the copy loop's `steps::between()?` and the `sync_instruction_cache` loop's).
- **What is wrong:** the large segment of the test is a zero tail: its 2048 pages are zeroed by `MemoryObject::new`
  (already stepped before F6); the file bytes copied are `hello`'s few pages; and `with_shared_bss` clears `PF_W` only,
  so the large segment is `R`, not `X`, and the icache loop runs over `hello`'s small text. The new loops F6 adds are not
  in the measured interval at any size above a step. The 200-page `an_images_pages_hold_the_files_bytes_piece_by_piece`
  checks bytes, unmeasured. Neither step point has a mutant in `tools/mutants/images-f6.toml`, so the "5 of 5 caught"
  says nothing about them.
- **Scenario:** deleting `steps::between()?` from the copy loop makes `image_create` of a 100 MB text (python's) copy
  25,600 pages in one interval; every test passes.
- **Fix:** run `measured` over `image_from` of `patterned_elf(2048)` (R X, 2048 file pages: both loops), assert the
  units, and add the two step points as mutants.

### 4. warning: a `fork` child of a process started from an image has no test that it maps the image's frames read-only and charges nobody for them

- **Where:** ADR-0438 item 2 ("A `fork` child maps the same frames as its parent does") and driver 3 ("a `fork` of the
  child take[s] nothing of the creator's room"); `kernel/src/memory/user.rs` `snapshot_plan_step`/`finish` (the
  `Holder::Memory` + `share` path the kept record now takes); the report's limitation note.
- **What is wrong:** ADR-0431's tests cover `SHARED` records made by `memory_map` of an object with a handle; the kept
  record has no handle, carries `execute`, and is the text of every forked POSIX process now. Before F6 a fork child
  copied text into frames of its own; the change in charge (child job no longer pays for text) and in attributes
  (the child maps the image's frames as the parent's record says) is checked by nothing; `test-libposix`'s fork cases
  only show that the child runs.
- **Fix:** a kernel test: spawn from an image, `snapshot_process` it, compare the child's text frames with
  `image.kept.page(..)`, assert no page writable, the creator's usage unchanged and the child's job charged only its
  own pages; end parent and child in either order and check the frames go with the last.

### 5. note: `image_create` reserves a handle slot, then pauses for the copy, then inserts; the comment's "refuses before the pages are made" does not hold

- **Where:** `kernel/src/syscall.rs:1439-1449`.
- **What is wrong:** `reserve()` keeps nothing; while `image_from` pauses at its step points another thread of the
  process can fill the table, and `insert` then refuses with `LimitReached` after the whole copy (and drops the image
  under the handle-table lock, bounded by `weight`). No leak, but the refusal is after the work, and `keep_room` and
  `insert_kept` (`syscall.rs:178-187`) exist for exactly a call that pauses before it inserts.
- **Fix:** `keep_room` before `image_from`, `insert_kept` after, `drop_kept` on the error paths.

### 6. note: test assertions that check less than they say

- `images_share_their_read_only_pages`: `assert!(shared > 0 && shared < kept.pages() + 8, "hello has a text")`; the upper
  bound always holds (shared pages are a subset of kept pages).
- `an_images_pages_are_paid_by_its_creator_alone`: of the four spawns into `snug`, only the first is checked; the
  comment "the later ones may not" accepts any outcome for three of them.
- `records_below_the_window_do_not_move_the_search` (`crates/zoraya-mappings/tests/mappings.rs`): no record straddling
  `low` and none ending within `guard` below it, the two boundary cases of the `at.max(..)` fix. The model was changed
  the same way as the code, so the model test does not check the fix independently either.

### 7. note: the equivalent mutant is in the list with no marker

`tools/mutants/images-f6.toml` `F6-the-image-keeps-the-source` is equivalent (the kept object is always new, so it is
never the source). The next `make mutants` run will report it as a survivor; the note says so, which is the existing
practice, but the report's "1 equivalent not run" means the list has not been run whole.

### 8. note: the note added to ADR-0108 is outside the two kinds of change CLAUDE.md allows an agent on an accepted ADR

The change itself was already decided by accepted ADR-0415 item 2 and is recorded in the proposed ADR-0438 ("A change
from ADR-0108"), which is the right place; the cross-reference note in ADR-0108's open questions is neither a
`### Decision` sub-heading nor a rule-6 note. Earlier parts added similar notes (F8 on ADR-0415). For the architect to
accept or remove.

## Checked and sound

- **Copy layout.** `from_table` refuses page-overlapping and unordered segments (`Error::Overlap`), W+X segments,
  `memsz < filesz`, `vaddr % PAGE != offset % PAGE`, and `offset + filesz > length`; so no page is shared between a
  read-only and a writable segment, and every copy is in range of both objects (`copy_from` checks again). `Kept` lays
  segments out with no gap; a shared segment keeps every page including its zero tail, a writable one the pages with
  file bytes; bytes go at `vaddr % PAGE` in the first kept page; the rest is zero from `MemoryObject::new`. `load_kept`
  maps shared page `start + k` to `kept.page(first + k)` and copies whole kept pages into a writable segment's own
  pages, the tail fresh. An executable segment always exists, so the kept object is never size 0.
- **W^X and no writer.** `map_kept` and `map_kept_page` refuse a writable attribute; the kernel has no protect call;
  `memory_map` picks its own address and needs a handle, and the kept object has none (nor does `dma_pin` reach it);
  every kernel write to user memory goes through `copy_to_user`, checked as EL0; the snapshot copies the record's
  attributes; a thread stack must be `user_writable`. A process can `memory_unmap` its own text record, which harms
  only itself (`unmap_range` frees no leaf frame).
- **Lifetime.** The kept object is held by the image and by each record (spawned and forked); `Paid` holds an
  `Arc<Job>`, so a dead creator's job keeps its charge until the last holder goes, and a large object goes to the
  reaper with its charge. The kernel tests drop handle and processes in both orders and check frames and charge.
- **Quota.** The creator pays the kept object and the table once; the child's job pays its records, tables and own
  pages; a refused `image_create` or spawn holds nothing (tested at the exact limit and one byte short).
- **Spawn bound.** `load_kept` counts a unit per mapped page and two for the record, with a step point every `STEP`
  units; measured by the 2048-page test.
- **`libposix`.** `own_image` matches only a path that resolves to the `Image` entry itself; the duplicate has the
  bootstrap handle's own rights and is closed after the spawn as `load`'s image was; the child's `Image` entry is a
  duplicate of the image it is spawned from, and only if the spawn's table lists the entry; nothing else is added.
  `zoraya_child_memory` reads only a job the parent created for its own child, through the handle it already holds; it
  is in the ADR's surface block, both width tables and `ports/Makefile`'s `ADDED_ADRS`; `make check` passes.
- **`find_gap`.** Records below `low` no longer move the search below it; a record inside the window still pushes it.
- **Documents and wiring.** INV-0415-2 and four new rows, Total 849 (`make adr-check` in `make check`); `t_shared` is in
  `libposix.script` (already in the lanes and lists); the test-map line for ADR-0438; SAFETY comments on the two new
  `unsafe` blocks state sound invariants; no host paths. ADR-0438 records the ADR-0108 semantics change and leaves the
  decision to the architect.

## Verdict

Fix first: finding 1 (or the architect's acceptance of the proxy, with the `top` sentences corrected), then 2 to 4.
The kernel mechanism itself (copy, W^X, lifetime, charge, spawn bound) is sound as far as I could find.

## Re-check

Re-check by the `opus-review` subagent of `git diff 99b344a0..16d69b1b` (the fixes and the merge of `origin/main`; the
merged files are not reviewed), 2026-11-16. Read: `keep_program` and `image_from` (`kernel/src/process.rs`),
`mmu::clean_to_unification` and `invalidate_instruction_cache`, `SnapshotBuild::finish` and the snapshot's clean,
`steps::between`/`count`, the reaper's `REAPER_STEP` and `MemoryObject::drop`, `image_create` with
`Table::keep_room`/`insert_kept`/`drop_kept`, the new and changed kernel tests, `t_shared.c`, `top.script`,
`tools/check-top-image.py`, the Makefile lines, both mutant lists, ADR-0438, the notes on ADR-0108 and ADR-0415, the report.

Tests run on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("make test: all checks passed"; `adr-check` with Total 854).
- `cargo test -p zoraya-elf -p zoraya-mappings -p zoraya-posix`: exit 0, 359 passed, 0 failed.
- `make test-qemu-one` (HVF): exit 0, `PASS: 128 passed, 10 expected failures`;
  `a_fork_child_of_an_image_process_maps_the_images_frames` and `an_image_of_a_large_program_is_bounded` print
  `ZORAYA-TEST ok`.
- `make test-libposix` (HVF): exit 0; `libposix-auto` `PASS: 164 steps` (`t_shared: image keeps 978944 bytes; L's job:
  2776256, +1125504 for a spawn of the file, +139264 +139264 for two of its image`, children 1582144 each, `passed`),
  `libposix-maps-auto` `PASS: 17 steps`, `libposix-top-auto` `PASS: 34 steps`; `top tab` read `1K` (empty room), `4259K`,
  `9302K`, `11946K`, `1407K` (0 procs); `check-top-image: ... 1681K each ... +2644K ... an image keeps 956K`, `a child of
  the image holds 1545K in its own job`, `check-top-image: ok`.
- Not run: any TCG step, any mutation run (no kernel mutant report is in the worktree's `build/mutants/`).

### Status of the findings

1. **Addressed, with the decision left to the architect** (see R3 for what it is). `top.script` reads `top` in a room
   around three self-spawns and one spawn of the file, and the checker compares the readings with each other and with
   the printed image size, pins none. The numbers the report and ADR-0438 quote are the ones this run produced. ADR-0438
   item 3 and the report now say plainly that `top` cannot show the program's own image (the shell makes it, outside
   every room) and that the done-when is met by a job-level proxy. Two weaknesses of the step are in R2.
2. **Fixed.** `t_shared` warms L's buffer (`'r'`) before measuring, waits for L's usage to settle, and compares
   `d_path - d_self` with the exact kept size from the program headers within 16 pages (978944 against 986240 in the run).
   With no image charge `d_path - d_self` is about 0, so the C mutant `F6-creator-does-not-pay-c` fails the lower bound
   (arithmetic on the run; the mutant itself I did not run). The settle loop is bounded by its count (100000 × 20 ms, so
   in practice by the runner's timeout). Two equal readings 20 ms apart is a heuristic: the reaper drops a
   610-frame item's charge at once (`REAPER_STEP` 1024), so an equal pair taken before the reaper ran gives an
   `after_path` that is too large. That fails the upper bound, a flake and never a false pass; on one CPU with T asleep
   the reaper runs in the gap.
3. **Fixed, with one gap left (R1).** `an_image_of_a_large_program_is_bounded` now measures `image_from` of 2048 pages
   of R X file bytes, through both loops, at most `STEP + 16` units. The three step points have mutants.
4. **Fixed.** The new kernel test snapshots a process of an image: the same frames, not writable, attributes as the
   segment's, writable pages copied, the child's job charged what the parent's is, the creator's use unchanged, the
   frames gone with the last holder. It checks only one order of ending (R6).
5. **Fixed.** `keep_room` before `image_from`, `drop_kept` on its only error path (`Cancelled`, `NoMemory`,
   `OverQuota`, `InvalidArgument` all come back through it), `insert_kept` after. No leak of the kept count.
6. **Fixed.** Exact partition assertion, the three later spawns into `snug` must be `OverQuota`, and the boundary cases
   of `find_gap` with written-out answers (the first two cases are the same boundary, both records end at `LOW - PAGE`).
7. **Fixed in the list** (the equivalent mutant is marked). Whether the list was run is not on record (R4).
8. **Not resolved.** The ADR-0108 note is relabelled "Open-question note (ADR-0001 rule 6)". Rule 6 is about an
   invariant whose check cannot be built yet, and this note records a change in semantics, so the label does not fit.
   ADR-0438 item 1 now states the deviation for the architect, and that is the right place. The note stays for the
   architect to accept or remove.

### New findings

#### R1. warning: an executable segment's last cleans and the next segment's first copied piece fall in one interval (up to 2 × STEP units)

- **Where:** `kernel/src/process.rs:704-722` (`keep_program`: the clean loop has a step point only before page
  `n > 0, n % STEP == 0`, and nothing after the loop; the next segment's copy counts its first piece, up to `STEP` pages,
  before its own `between`).
- **What is wrong:** the last chunk of cleans (1 to 64 units) and the next segment's first piece (up to 64) share one
  interval: up to 128 units, against the test's `STEP + 16 = 80`. The test cannot see it, because `patterned_elf` has a
  single segment. Every real program is laid out `r--, r-x, rw-`. For `t_shared` it is 201 text pages (201 mod 64 = 9)
  plus a first piece of 64 of its 65 data pages, 73 units, under the limit by luck. A program with a text of `64k`
  kept pages, or a remainder above 16, and a data segment of 64 or more file pages exceeds it (python's data is several MB).
  The loop is still bounded at any image size: at most `2 × STEP` units per interval, plus the one `ic ialluis`. But
  INV-0270-1's per-interval bound is broken by a factor of up to 2.
- **Fix:** `steps::between()?` after each executable segment's clean loop. Add a case to
  `an_image_of_a_large_program_is_bounded` with an R X segment of 2048 pages followed by an RW segment of at least 64
  file pages, and a mutant that removes the new step point.

#### R2. warning: the `top` step reads the room without waiting for the reaper, and its last check cannot see a leak

- **Where:** `tests/posix-core/t_shared.c` `driver(keep = 1)` (`ask('p')` then `phase(...)`, no `memory_of`);
  `tools/check-top-image.py` (`end > p0 + 16`).
- **What is wrong:** (a) after `'p'` L has closed the file's object (about 610 pages), which goes to the reaper with its
  charge. `top tab` is typed as soon as the phase line appears. If the reaper has not dropped the item by then, P4 is
  about 2440K too high and `file_child - own` fails `<= kk + 64`. HVF passed. Under TCG, which CI runs, this depends on
  timing that the test does not control. The non-keep mode settles exactly for this reason. (b) The last reading was
  1407K with 0 processes, against 1K for the empty room at the start. The checker compares it with P0 (4259K), so a
  leaked image (956K) or a leaked transfer buffer (1024K) passes. The docstring's claim that "the room gets everything
  back" is not checked, and the report quotes 1407K without saying what it is (reaper lag, or something kept).
- **Fix:** in keep mode, T calls `memory_of(l)` before each `phase` (and the hold children are at rest by then). The
  script waits for `[1] done`, then reads `top tab` until it stops changing, or T waits for the room to empty. The
  checker captures the empty-room reading and requires `end` within a few K of it.

#### R3. warning: what the architect has to decide is wider than the display: a room's memory limit no longer covers the text of the programs run in it

- **Where:** `docs/adr/0438-shared-read-only-segments-as-built.md` item 3 and "Effect of the vision"; the report's F6 notes.
- **What is wrong:** before F6 a spawn copied the program's pages into frames charged to the child's job, inside the
  room. Now the image of a program the shell starts is charged to the shell's job, outside every room. A room with
  `--mem 64M` can therefore run a program whose text is far above 64 MB, and the room pays nothing for it. The shell's
  job pays, a separate image for each start (there is no cache), for as long as any process of the room runs from it.
  The ADR states only the effect on `top` ("`top` does not show it at all").
- **Fix (documents):** list for the architect, in ADR-0438 and the report, the three things to decide. (1) Accept the
  job-level proxy as F6's done-when, or ask for a line per image in `top`. (2) Accept that a room's limit does not
  cover shell-made images, or have the shell make each image in the job of the room it starts the program in. (3) The
  ADR-0108 deviation (item 1).

#### R4. note: the report is not updated for the re-run and the new mutants

`docs/tasks/TASK-34-report.md` F6 still quotes the old `t_shared` run (2176256, 1315904). ADR-0438 quotes the new one
(1125504, 1582144), which this re-check reproduced. The mutation line still says "`images-f6.toml` 5 of 5 caught, 1
equivalent not run". The three new kernel mutants and `images-f6-libposix.toml` have no recorded result, and nothing in
the worktree shows a run. Run them (`make mutants` with both lists) and record the outcome, or say they were not run.

#### R5. note: cache maintenance across step points relies on one CPU

`keep_program`, like `SnapshotBuild::finish`, issues `dc cvau` across step points and one `dsb ish; ic ialluis; dsb
ish; isb` at the end. A DSB completes only the maintenance its own PE issued, and `__context_switch` has no DSB. On an
SMP kernel, a thread that migrates between a clean and the final barrier could leave a clean incomplete when the I-cache
is invalidated. The kernel runs on one CPU (`docs/design.md`), so this is sound today. Record it as a limitation for
the SMP work (a `dsb ish` in the switch path, as other kernels do).

#### R6. note: the fork test checks one order of ending

`a_fork_child_of_an_image_process_maps_the_images_frames` says "whichever ends first" but drops the image, then the
parent, then the child. It also runs the snapshot's steps by hand rather than through `snapshot_process`, so it does not
test the charge path of the real call.

### Checked and sound

- **Bound at any size.** The copy steps once per piece of `STEP` pages. The clean steps every `STEP` pages, each page
  one unit. There is a step point before the first piece. So `image_create` of a 100 MB text is at most `2 × STEP`
  units in an interval (R1), plus one `ic ialluis`.
- **Instruction cache.** Only executable segments are cleaned, every kept page including the zero tail (zeroed by
  `MemoryObject::new` through the cacheable direct map). The line size is `CTR_EL0.DminLine`, and the start is aligned.
  The `dsb ish` that completes the cleans comes before `ic ialluis` (all lines, so frames that once held other code are
  covered), and `dsb ish; isb` come after. All of it happens before the image is returned, so before any mapping of its
  frames exists. A pause between the cleans and the invalidate is harmless because nothing maps the frames yet (one CPU,
  R5). Read-only and writable data are not cleaned, and the W^X refusal is unchanged.
- **`image_create`'s handle room.** Sound (finding 5).
- **Invariants.** Total 854 is the count of the rows, and `make adr-check` passes. The test-map line for the checker is
  present.

### Verdict

Fix first: R1, which is small (one step point and one test case), and R2 (a settle before each phase and an
end-of-room check), with R4's report and mutation record. R3 and finding 8 go to the architect with the decision listed.
The fixes for 1 to 7 are otherwise sound.

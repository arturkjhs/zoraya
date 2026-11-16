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

# TASK-34 review: part F3 (memory descriptors: `memfd_create`, `shm_open(SHM_ANON)`, `MAP_SHARED`, seals)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f3` at 9c308b4f, 2026-11-15, read
against `docs/tasks/TASK-34-foundations.md`, `docs/research/browser-engine/plan.md` row F3, ADR-0413 (accepted), the
proposed ADR-0431 and ADR-0432, and the amendment notes on ADR-0100, 0102, 0103 and 0107. Kernel code read around the
diff: `memory_map` and `snapshot_process` (`kernel/src/syscall.rs`), `snapshot_table`, `AddressSpace::snapshot_step`
and `SnapshotBuild` (`kernel/src/memory/user.rs`), `MemoryObject` (`kernel/src/memory/object.rs`), `SpinLock`
(`kernel/src/sync.rs`, masks IRQ and FIQ while held), and ADR-0270's INV-0270-1.

Tests run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("make test: all checks passed").
- `cargo test -p zoraya-posix`: exit 0, 250 tests passed over 9 test binaries (`tests/memfd.rs` among them), none
  failed.
- `make test-qemu-auto-default`: exit 0, `PASS: 119 passed, 9 expected failures`; `t_snapshot: shared mappings stay
  shared across a snapshot, others are copied`, `t_quota: a shared object is charged to its creator alone`.
- `make test-libposix`: exit 0, `PASS: 125 steps`, `t_unix: memfd passed`.
- `make test-posix-threads`: exit 0, `PASS: 104 steps`.

Not run by the reviewer: `make test-posix`, which is the only target that runs `t_memfd` (`tests/posix/invariants.script`):
the in-QEMU halves of INV-0413-2 (the read-only fork child's `SIGSEGV`), -3 and -5 rest on it, and the candidate run
must show it green. No mutation run (`make mutants`, `make mutants-host`) was made by the reviewer.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

1. **error**: a snapshot step no longer has a bound when the process has shared mappings: INV-0270-1 (accepted
   ADR-0270) is broken by arguments a limited job may pass. Where: `kernel/src/memory/user.rs:566`
   (`Some(m) if m.va <= va && build.plan[i].share.is_some() => continue,`) against `:593` (`taken += 1`) and `:596`
   (`taken < STEP`). `snapshot_step` runs with the parent's `space` spin lock held, which masks IRQ and FIQ
   (`kernel/src/sync.rs:49`). A page of a shared mapping is skipped with `continue`, so it never counts toward `taken`,
   and the walk only ends a step after `STEP` pages were *copied*. Each skipped page still costs one leaf visit and
   one `partition_point` over the mappings. Before F3 every visited page was a copied page, so the step was bounded.
   *Scenario:* a POSIX process in a room with a 64 MiB limit makes one 32 MiB object and maps it `SHARED` 255 times
   (`MAX_MAPPINGS` is 256, and each mapping costs only page tables, about 64 KiB per 32 MiB). That is about 2 million
   shared pages. Then it calls `fork`. The first `snapshot_step` walks all 2 million pages in one interval with
   interrupts masked, which is tens of milliseconds against the 2 ms bound. A received object, which another job paid
   for, makes the mapping still cheaper for the forker. `t_bound` has no snapshot with a shared mapping, so
   INV-0270-1's check does not cover the new path.
   *Fix:* count a skipped shared page as a unit of the step (`taken += 1` before `continue`). Better, when the walk
   reaches a shared mapping, set `next = m.va + m.length` and return `false`, so that the next step resumes past the
   mapping through `for_each_mapping_from`, which skips the table entries below `from` at each level. Extend `t_bound`'s
   `snapshot_process` case with a shared mapping repeated up to `MAX_MAPPINGS`, under the masked-time counter, and add
   that mutant to `tools/mutants/shared-mappings-f3.toml`.

2. **warning**: a `SHARED` mapping hands a `fork` child a live alias of an object whose handle the snapshot will not
   give it. This contradicts ADR-0431's own "a process reaches the frames a handle gave it and no more". Where:
   `kernel/src/syscall.rs:763` (`memory_map` asks for `READ`, or `READ|WRITE`, whatever the flags) and `:839`, against
   `snapshot_table` (`syscall.rs:305`, a handle without `DUPLICATE` is not placed in the child).
   *Scenario:* a service gives a POSIX process a memory handle with `READ|WRITE|TRANSFER` but no `DUPLICATE`, so that
   only that process holds it. The process maps it `WRITE|SHARED` and forks. The child, possibly in another job
   (`request.group`), has no handle but keeps a writable mapping of the same frames, also after the parent exits.
   Before F3 the child got a private copy taken at the fork. Every handle `libposix` maps `SHARED` has `DUPLICATE`,
   so nothing in the tree breaks today. The hole is in the ABI the ADR makes.
   *Fix:* make `memory_map` with `SHARED` need `DUPLICATE` as well (or have `snapshot_plan` copy a shared mapping
   whose handle could not be duplicated, which needs the right recorded on the mapping). Say so in ADR-0431 item 4,
   and add the refusal to INV-0431-3 and `t_snapshot`.

3. **warning**: the rights of a sent memory handle are found by a probe that reads any failure as "no `WRITE`". Where:
   `crates/zoraya-posix/src/ffi.rs:64-77` (`rights_of`: `handle_duplicate(h, MemoryObject::RIGHTS)`, and on `Err(_)`
   the rights without `WRITE`).
   *Scenario:* a process whose handle table is full sends a writable, unsealed memfd. The probe's duplicate fails with
   `LimitReached`, not `AccessDenied`. The channel entry is made read-only, while the record says `access` 2 and the
   send reports success. The receiver's `mmap(PROT_WRITE, MAP_SHARED)` then fails with `EACCES`, and its `write`
   copies through a mapping that the kernel refuses. Write access is lost silently. The probe also costs a system call
   and a handle slot on every send of a memory descriptor.
   *Fix:* the library already knows the answer (`Memory::writable`, `memfd.rs`). Pass the rights from
   `prepare_rights` (`unix_io.rs:287`) with the handle instead of probing for them. Failing that, map only
   `AccessDenied` to read-only and propagate any other error as the send's error.

4. **warning**: a received descriptor reports the sender's seals as its own, and nothing checks them. That includes
   `F_SEAL_WRITE`, the seal a receiver tests to trust that the contents can no longer change. Where:
   `crates/zoraya-posix/src/unix_io.rs:851-859` (`seals` taken from `write_off`) and `memfd.rs:170`
   (`Memory::received`).
   *Scenario:* a hostile client builds the record itself, with kind 7, `access` 0 and `write_off` = `F_SEAL_WRITE`, and
   keeps a writable mapping of the object. The receiver (a compositor-like service, or the UI process of ADR-0419)
   sees `fcntl(F_GET_SEALS) & F_SEAL_WRITE` and treats the buffer as immutable, while the client goes on writing it.
   `F_SEAL_SHRINK` and `F_SEAL_GROW` stay true, because the kernel object never changes size. `F_SEAL_WRITE` and, for
   other holders, `F_SEAL_FUTURE_WRITE` say nothing the kernel enforces. ADR-0432 item 1 says seals are per holder
   ("a limit"), but neither it nor `docs/design.md` says that a received seal is the peer's unverified claim. Driver
   1 of ADR-0432 ("a seal that binds is one the kernel enforces") reads as if it did.
   *Fix:* on receipt, keep only what the receiver can rely on: `SHRINK|GROW` (always true) and `FUTURE_WRITE` only
   if its own handle has no `WRITE`. Never report `F_SEAL_WRITE` from the wire. Otherwise, state the trust limit in
   ADR-0432 item 1 and in `docs/design.md`'s limits, and add a host test that a forged `F_SEAL_WRITE` record is either
   masked or documented.

5. **note**: a `fork` child keeps a handle to each of the parent's `MAP_PRIVATE` copy objects and never uses it.
   Where: `memfd.rs:121` (`MemMap::private`) with `memory_after_fork` (`memfd.rs:791`). The snapshot duplicates the
   handle and copies the mapping into a fresh object paid by the child. The parent's copy object, charged to the
   parent's job for its life, then stays alive for as long as the child keeps the record (until its `munmap` or its
   exit), also after the parent unmapped it. The anonymous `mmap` table in `ffi.rs` has the same pattern from before
   F3. *Fix:* in the child, close each `private` handle after the fork and set it to `None`, or record the pattern in
   ADR-0432 item 4.

6. **note**: an unreachable branch. `memfd.rs:695` (`if !m.writable { return Err(EACCES) }` in `memory_mmap`) cannot
   be reached. `writable` is false only after a `READ_ONLY` seal, which `:692` already refused, or for a descriptor
   that arrived with `access` 0, which `:689` refused. A mutant there is equivalent, so do not count it as a test
   hole. Its comment in `ADR-0432` item 1 ("a record that says read-write over a handle that is not ... the kernel
   refuses") is right, but that check is the kernel's, not this branch's.

7. **note**: test texts claim more than the tests do.
   - `crates/zoraya-posix/tests/memfd.rs:958`: "The same descriptor given to the child as another fd by a file
     action". The call then passes no file action (`inherit(&[], 0)`). A `dup2` file action of a close-on-exec memfd
     onto a kept fd goes through `child_fds` and `jobs.rs`'s `_ => Err(EBADF)`, so it fails closed, but nothing tests
     that.
   - `memfd.rs:1041`: "every order of up to five of the steps"; the loop runs every order of four (6,561, as the
     status column says).
   - `user/src/bin/t_snapshot.rs:210`: "the child's mapping of a read-only handle is read-only". The child only reads
     it. The fault on a write is checked by `t_memfd seal` alone, which only `make test-posix` runs.

8. **note**: mutation testing. The six kernel mutants of `tools/mutants/shared-mappings-f3.toml` are relevant and, as
   reported, caught. `F3-shared-pages-still-copied`'s note calls it "an equivalent mutant unless the copy is counted".
   It is not equivalent: without the `continue`, the step copies the parent's page into `objects[i].page(va - m.va)`,
   an offset in the *mapping*, not in the object. For the partial mapping at offset 2 pages that overwrites pages 0
   and 1 of the shared object, which `t_snapshot` sees. No mutant or test covers finding 1. For the host side, the
   report says about 60 mutants were never run. The reviewer did not run them and cannot list which survive. From
   reading, the branches most likely to survive without being holes are finding 6's branch and `Memory::received`'s
   `writable_out` (adding seals needs `O_RDWR`, so it is equivalent for a read-only arrival). The likely real hole is
   any mutant of `rights_of`'s error arm (finding 3), which no host test reaches (the fake kernel has no `rights_of`).

## Checked and found sound

- `snapshot_process` with shared mappings: the plan carries an `Arc` of the object, so the child's mapping keeps it
  alive and nothing is charged for its frames. `MemoryObject` never changes size, so `object.page(base + offset)` stays
  inside it (`memory_map` checked `offset + length <= size`). The mapping lock and `thread::Freeze` keep `plan[i]`
  aligned with `self.mappings[i]` across steps. A cancelled build drops only `Arc` clones and its fresh objects. The
  child's mapping is recorded `SHARED` again, with the same attributes, never executable. `finish` maps the pages in
  steps. No frame is leaked or freed twice: frames belong to the object, and mappings only hold it.
- Quota: `t_quota` shows the maker paying and a 2 MiB room mapping 4 MiB. ADR-0261 item 8 holds by construction.
- Seals as rights in one process: the downgrade closes the writable handle exactly once. `replace_object` after a
  seal makes the new object read-only before installing it. `dup` shares the description. A send, a `fork` and an
  arrival set `writable_out`, so `F_SEAL_WRITE` is `EBUSY` after any of them. A writable mapping made before
  `F_SEAL_FUTURE_WRITE` keeps working, as Linux has it. Every write path (`write`, `pwrite`, `writev` through
  `write`, writable `MAP_SHARED`) checks the seal. A forged record that claims read-write holds a handle the kernel
  refuses to map writable.
- Size and offset overflow: `checked_add` on `offset + length` in `memory_mmap` and in `memory_write`'s end. `round`
  cannot overflow for a positive `i64` length. `munmap` uses `checked_next_multiple_of`. `MAX_SIZE` limits
  `ftruncate`, a write's end and the wire's size. A received size larger than the object only makes the kernel refuse
  a map (an error, not a panic).
- `ftruncate` shrink while mapped is `EBUSY` (the descriptor is pinned) and never replaces a mapped object. `ftruncate`
  to 0 closes the handle once. `munmap` takes whole mappings only and closes a private copy's handle.
- `PROT_EXEC`: refused before any work, on every `mmap` path and in `mprotect`. The kernel never maps a memory object
  executable.
- ABI: `MapFlags` decodes 0 to 3. Old callers pass 0 or 1 and mean the same, and the abi test covers the decoder.
- The `posix-surface-added` block (`shm_open`, `shm_unlink`) is in ADR-0432, and ADR-0432 is in `ADDED_ADRS`. The
  widths file has both. `PYTHON_ABSENT` keeps `_posixshmem` unbuilt.
- `docs/adr/invariants.md`: 809 `INV-` rows and a Total of 809. The ADR-0431 and ADR-0432 rows are added with 3 each.
  No ADR status or Architect's decision section was changed, and the amendment notes are in the open-questions
  sections.
- CLAUDE.md: `make check` covers host paths, test caps and the ADR table and passed. `tools/test-map.txt` already
  maps every changed directory. `memfd.rs` is `forbid(unsafe_code)`. The new `unsafe` blocks in `t_snapshot` and
  `t_quota` carry `SAFETY` comments.

## Verdict

Fix first: finding 1 (a kernel bound of an accepted ADR). Findings 2 to 4 are fixed or recorded in ADR-0431 and
ADR-0432 before the architect decides on them.

## Re-check at 66ad2da7

The `opus-review` subagent read `git diff 9c308b4f..66ad2da7` against the findings above, 2026-11-15. Run by the
reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("make test: all checks passed").
- `cargo test -p zoraya-posix`: exit 0, 252 passed, none failed (the two new tests in `tests/memfd.rs` among them).
- `make test-qemu-one`: exit 0, `PASS: 119 passed, 9 expected failures`; the guest measured factor 2, and
  `t_bound: snapshot_process, shared: one step 283 us, large 199 us masked`; `t_snapshot: shared mappings stay shared
  across a snapshot, others are copied`.
- `make mutants MUTANTS_ARGS="--jobs 1 --only F3-shared-pages-not-counted tools/mutants/shared-mappings-f3.toml"`:
  caught. Under the mutant, the large shared case held IRQ masked for 46.9 ms against 0.77 ms for the small one
  ("shared snapshot grows with the pages"). That is caught by the ratio check, and under HVF by the 2 ms bound as well.

Not run: `make test-posix` (`t_memfd`), the other mutants, and the TCG lanes.

Findings:

1. Fixed. `snapshot_step` counts a skipped shared page toward `taken` (`kernel/src/memory/user.rs:570`). A step ends
   at the next leaf boundary after `STEP` units, whether the pages were copied or skipped, which was the bound before
   F3. `t_bound` maps one object 41 times and checks the bound and the growth ratio. Both ran, and the mutant shows
   the ratio check bites.
2. Fixed. `memory_map` with `SHARED` needs `DUPLICATE` (`kernel/src/syscall.rs:771`). `map_commit` has one caller,
   so no other path records a shared mapping. The child of a snapshot keeps its mapping as shared, and the parent
   needed `DUPLICATE` to make it. Spawn carries no mappings. A handle that has `DUPLICATE` can be duplicated without
   it, but making that copy needs `DUPLICATE` in the first place, so a process that can map `SHARED` could always
   have passed the object on. Closing the duplicable handle after mapping does leave a fork child with frames and no
   handle, but that is the ordinary "a mapping outlives its handle" case, not a widening. Every handle `libposix`
   maps has `MemoryObject::RIGHTS` minus at most `WRITE`, so it has `DUPLICATE`. `t_snapshot` checks both the
   refusal and that a private mapping is still allowed.
3. Fixed. The probe is gone from `ffi.rs`. `prepare_rights` lists the handles whose descriptor is not writable, and
   `RtSys` takes `WRITE` off exactly those. A full handle table no longer matters because no slot is spent. The fake
   kernel asserts the list against its own state in both directions.
4. Fixed. `F_SEAL_WRITE` from the wire is never reported. `SEAL`, `SHRINK` and `GROW` are kept, and `FUTURE_WRITE`
   is set for a read-only arrival. ADR-0432 item 1 and `docs/design.md` state the limit. The one legitimate loss is
   that a receiver no longer sees a genuine sender's `F_SEAL_WRITE`, only `F_SEAL_FUTURE_WRITE`. That is recorded,
   and nothing in the tree reads received seals.
5. to 8. Recorded (5, 6 in ADR-0432's open questions) or corrected (7, 8: the test comments, the
   `F3-shared-pages-still-copied` note).

ADR edits touch only the proposed ADR-0431 (item 4, INV-0431-3) and ADR-0432 (item 1, open questions). No status
line or Architect's decision section changed. `docs/adr/invariants.md` has 809 rows and a Total of 809, and its
INV-0431-3 row matches the ADR.

New findings:

9. **note**: the received record's `access`, not the handle, decides `FUTURE_WRITE` and the read-only list.
   `crates/zoraya-posix/src/unix_io.rs:871` and `:290`, `memfd.rs:184`. The comment "which the kernel enforces" and
   ADR-0432 item 1's "if its own handle is read-only" claim more than the code checks. *Scenario a:* a forged record
   with `access` 0 over a writable handle reports `F_SEAL_FUTURE_WRITE`. The library refuses writes through that fd,
   so the descriptor's own behaviour holds, but the handle could write. *Scenario b:* INV-0432-1's forged case,
   `access` 2 over a read-only handle. Forwarding that descriptor is now refused by `channel_write`
   (`kernel/src/syscall.rs:910`, `AccessDenied`), where the old probe sent it read-only. This fails closed. The fake
   kernel asserts in that case instead of modelling the refusal (`tests/kernel/mod.rs:200`). *Fix:* reword the
   comment and the ADR ("if the record says read-only; the library writes nothing through it"), and say in ADR-0432
   that a descriptor received with an overstated access cannot be forwarded.
10. **note**: `snapshot_of` checks only masked time, not the units counter (`measure`), so on a slow host the bound
    rests on the 10x ratio alone. The margin under the mutant was 60x. Making the snapshot report units would make
    the check independent of host speed (this predates F3).

Verdict: merge. No error or warning is open. Notes 9 and 10 can be handled when the architect decides on ADR-0432.

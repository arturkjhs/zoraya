# TASK-34 report: foundations for big programs (F2 to F8)

## Status for the architect

Updated 2026-11-16.

- Merged and tagged: F2 (`stage-f2`), F3 (`stage-f3`) and F4 (`stage-f4`), the tags placed on the green scheduled full run 43749759496 that includes all three (`tools/ci-last-full.rb --green-for`). **Merged, not yet tagged** (`tools/ci-last-full.rb --green-for` names no green scheduled full run that includes them yet): F8 (d38c0a9c), F5 (c1061eca), F6 (e31fc2ce) and F7a (5a3372c7); I tag `stage-f8`, `stage-f5`, `stage-f6` and `stage-f7` when it does (`stage-f7` only after F7b). F1 (`poll`) was merged before this task.
- In progress: F7b, TCP `bind`/`listen`/`accept` over `netd` (the plan's "done when" `python3 -m http.server` serving a file to `curl` is not met until it is built). F7a (Unix socket files in the line's `/tmp`, ADR-0439) is merged.
- ADRs: ADR-0430 (item 7), ADR-0433 and ADR-0435 were accepted by the architect on 2026-11-15 and recorded; sockets surviving `exec` (`execve` with a Unix socket open is `EBADF`) are in the backlog. ADR-0431 (a `SHARED` bit in `memory_map`, kernel ABI), ADR-0432 (memory descriptors as built) and ADR-0434 (netd `Duplicate` makes a connection of the caller's own rights, bounded to 8 live per family) are **proposed**, each recommended as written; ADR-0431 first, as a kernel ABI change. Numbers 0436 to 0449 are free.
- Machine: MacBook Air M1, 8 GB, so `CARGO_BUILD_JOBS=2`, one part with builds and one QEMU guest at a time.

## Parts

| Part | State | Tag | Time | Model |
|------|-------|-----|------|-------|
| F2 | merged | stage-f2 | about 3 sessions | Sonnet implementation; Opus review (1 error, 2 warnings, all fixed) and re-check; candidate run 43681721034 green (75 jobs) |
| F3 | merged | stage-f3 pending a green full run | about 3 sessions | Sonnet implementation; Opus review (1 error, 3 warnings, all fixed) and re-check; candidate run 43700119735 green after one rerun of the `w9-term` TCG job |
| F4 | merged | stage-f4 pending a green full run | about 4 sessions | Sonnet implementation; Opus review (3 errors, 4 warnings), re-check (2 errors) and re-check 2, all fixed; candidate run 43732969721 green on the 3rd candidate after one rerun of the `test-tour-terminal` TCG job |
| F8 | merged | stage-f8 pending a green full run | about 2 sessions | Sonnet implementation; Opus review (1 error, 5 warnings) and re-check; candidate run 43751085581 green after one fix (the keeper script pinned a session size that varies with boot) and one rerun of the `test-tour-terminal` TCG job |
| F5 | merged | stage-f5 pending a green full run | about 2 sessions | Sonnet implementation; Opus review (1 error: the plan's CPython command was never run; 5 warnings), re-check (1 warning) and a test-only follow-up; candidate run 43770158088 green, 79 of 79 jobs, including the new `test-python-mem` (python at 2 GiB, 5,000,000 `bytearray(100)`, 2 m 51 s under TCG) |
| F6 | merged | stage-f6 pending a green full run | about 3 sessions | Sonnet implementation; Opus review (1 error: the plan's `top` check never ran; 3 warnings), two re-checks and the architect's decisions on ADR-0438 (accepted 2026-11-16); candidate run 43789776708 green on attempt 2 (the first attempt timed out in `test-qemu-tcg-smp4-gicv3` at 420 s on a slow runner) |
| F7 | F7a merged (Unix socket files); F7b (TCP listen) in progress | stage-f7 after F7b | about 4 sessions so far | Sonnet implementation; independent Opus review (1 error: the shim's `ENOSYS` `bind`/`listen`/`accept` stubs hid the new calls from every ported program; warnings) and a re-check; candidate run 43816764155 green after fixes (the python smoke line was slow, not hung: the shell's line editor redrew a 270-character line) |

## F2 notes

- Review: `docs/tasks/TASK-34-review-f2.md`. The error (a shared stream socket sent from a fork parent was lost) and the fork-child handle leak are fixed with host tests.
- CI found one more effect: `usleep` in libposix made wget's configure use `sleep`, which libc lacks. wget is configured with `ac_cv_func_usleep=no` and curl with `--disable-socketpair`, in `ports/Makefile`.
- Known limitations are in ADR-0430: `bind`/`listen`/`accept` came with F7 (Unix socket files only); a packet socket cannot be half-closed; the send-then-peer-closes window can lose a moved socket. Guest console output stalled with `-v` or `-W` in regrtest; not diagnosed.
- I tagged `candidate/work-f2-b` on the same commit as the green run; its duplicate run is not needed.
- Files outside the task's scope: `ports/` (wget, curl, the CPython port), `tools/libposix-widths.txt`, ADR-0225 and ADR-0342 notes.

## F3 notes

- Built: memory descriptors as ADR-0413 specifies (`crates/zoraya-posix/src/memfd.rs`), and in the kernel a `SHARED` flag of `memory_map` that
  `snapshot_process` honours (ADR-0431: a shared mapping is not copied into a `fork` child; without it a fork child's `MAP_SHARED` mapping
  was a private copy). Proposed ADR-0431 and ADR-0432 record what the accepted ADRs leave silent; notes on ADR-0100, 0102, 0103, 0107 and 0413.
- Checked: INV-0413-1 to -5 and INV-0431-1 to -3, INV-0432-1 to -3 (`docs/adr/invariants.md`; Total 809).
- What waits for F4: a memory descriptor does not cross `spawn` or `exec` (`EBADF` without `O_CLOEXEC`), so the plan's "passes a 4 MiB memfd to a
  child" is checked with a `fork` child (`t_memfd fork`) and in one process over a socketpair (`t_unix memfd`), plus two processes over a fake
  kernel; seals are per holder; the in-QEMU "in another process" half of INV-0413-2 needs F4.
- Found by the in-QEMU tests, not the host's: a channel entry for a sealed memory handle must name the rights it has (`rights_of` in `ffi.rs`).
- Ports: CPython's `_posixshmem` stays unbuilt (`shm_open` and `shm_unlink` are in `PYTHON_ABSENT`, as F2's lesson); no other configure
  result changes (grep of `ports/` for `shm_open`, `memfd`, `mmap`, `mprotect`, `PROT_EXEC`: none).
- Review: `docs/tasks/TASK-34-review-f3.md`. The error was an unbounded snapshot step (skipped shared pages were not counted, INV-0270-1); fixed with a `t_bound` case and a mutant.
- CI: `quotas_from_a_process` failed under TCG/EL2/SMP4 because F3's own new `t_quota` case was too tight for slow machines (fixed). The `w9-term` TCG job timed out in two F3 runs and also in `main`'s scheduled run 43699859694 (without F3); its step time moves between 262 s and 589 s against a 600 s runner limit. **For the architect:** raise the limit, shrink the build tree in `w9-term`, or poll less often than every 0.1 s (each poll forks `ls` under one-CPU TCG). I changed no timeout.
- Mutation: host run cut at 30 minutes (about 60 of 388 mutants never run; later re-check: 14 of the 20 misses in new code are equivalent or untestable); 7 kernel hand mutants all caught. The never-run host mutants were not run afterwards (about 35 minutes).
- The merge into `main` had conflicts in `Makefile`, `ports/Makefile` and `invariants.md` (Total 820 recomputed from rows); the combined `t_tty` plus `t_memfd` lines were not run in CI before the merge.

## F4 notes

- Review: `docs/tasks/TASK-34-review-f4.md`. Errors found and fixed: `addopen` gave the child the wrong file (parent fd read as a child fd); a spawn with no attributes moved inherited Unix sockets (now only a socket the file actions list moves, and a stream socket moves whole); netd `Duplicate` had no bound, and the first bound reset when the asker closed (now a family token, 8 live per family); a refused spawn leaked entry handles (one `abandon()` closes them on every refusal path).
- Closes what F2 and F3 left: a Unix socket and a memory descriptor now cross `spawn` (`t_spawn`, `t_memfd` as real two-process tests); INV-0412-1, INV-0413-2 and INV-0432-2 are `checked`.
- CI effects found: `nvi`'s compile log lost a line to interleaved `sh -x` traces (one whole log line per compile now); `posix_spawn_file_actions_addclose` was outside the tier list, so CPython's `posixmodule.c` failed to link (it joins ADR-0433's surface block). The python lanes (`test-python`, `-ext`, `-regr`) pass with the new `subprocess` path.
- The `test-tour-terminal` TCG job (`posix-curses.script`, new on `main`, one earlier TCG pass) hung at step 55 in the first run and passed on rerun; the slow runner (about 2x the frame time of the one earlier pass) and the script's fixed pauses before typing into `vi` are the likely cause, not F4. **Follow-up for stage 10:** wait for `vi`'s screen with a `screen` step after `vi notes.txt` instead of fixed pauses.
- Known limitations (ADR-0433): a child with no cwd reads relative paths from `/`; `GRANT_WINDOW` moves, it does not share; the positive network grants are tested only over a fake netd on the host (no in-QEMU program holds a connection); a connection that holds the `narrow` right can still make connections without limit through `Narrow` (older than F4, program connections lack the right); `execve` with a Unix socket open is `EBADF`.
- Mutation: `make mutants-host CRATE=zoraya-posix --in-diff` 160 mutants, 26 missed in the first run; the holes were closed with tests, the equivalent ones (disjoint-bit `|` to `^`) remain.
- Process note: the F4 implementer's final reports were refused by the harness three times; I read its work from the branch and commits, and I made the ADR-0433/0435 decision transcription myself.

## F8 notes

- Built: `FrameAllocator::alloc_counted` and `alloc_scan_bound` (a single-frame scan reads at most one word per 256 KiB, 8192 at 2 GiB; the wrap-around no longer rereads the start region whole); `crates/zoraya-pmm/tests/two_gib.rs` (counts, a full drain, exact worst case, run search in bounded steps, 128 MiB to 4 GiB in one and two regions above 4 GiB); the test kernel's `the_worst_single_allocation_is_bounded_by_the_machines_ram`; `make run-browser` (`run-gui` at `--mem $(BROWSER_MEM)`, 2G; `make run` unchanged); `tests/shell/browser-guest.script` run by `make test-keeper` at `--mem 2G` (HVF, TCG on the full set). Proposed ADR-0436 (recommended as written; **deviation for the architect:** `top --session` is not built, `top`'s session row is the check, and `room set --mem 1800M` is refused above the session's 1452930K). Nothing in the kernel assumed a RAM size (boot tables map 4 GiB, direct map from blocks, heap on demand); the 2 GiB steps already ran.
- Measured (MacBook Air M1, 4 CPUs, debug test kernel): worst single allocation 512 words / 6.5 us at 128 MiB; 8192 words / 94 us under HVF and 1.2 ms under TCG at 2 GiB (ADR-0270's bound 2 ms). Boot to the shell script's end: 0.26 s HVF and 0.73 to 0.80 s TCG, the same at 128M and 2G. Kernel reserved: 30000 KiB at 128 MiB, 30120 KiB at 2 GiB (frame map 8 KiB against 128 KiB). Free after boot 2066388 KiB; session ceiling 1452930K.
- Run: `cargo test -p zoraya-pmm` (17 + 5 passed); `make test-qemu-one` and `make test-qemu-auto-smp4-2g` (120 passed, 9 expected failures each); `make test-qemu-tcg-smp4-gicv3` (same, before the per-word limit replaced a fixed one; not rerun); `make test-keeper` (HVF, passed, 29 steps); `make check` (passed). Not run: the TCG keeper run (full set only), `make test-changed`.
- Mutation: host `--in-diff` on `zoraya-pmm` 15 mutants: 10 caught, 3 unviable, 1 timeout, 1 missed (the wrap-around's end bound, equivalent while a frame is free); kernel list `tools/mutants/frames-f8.toml` 2 of 2 caught.
- Limitations: a guest above 4 GiB needs a second level of the frame map (the test says so); the 1 us a word limit is generous under TCG (150 ns a word).

## F8 notes

- Review: `docs/tasks/TASK-34-review-f8.md`. Error: the first in-guest limit (1 us per word plus 200 us) allowed 8.4 ms at 2 GiB against the accepted 2 ms of INV-0270-1; now the words read are checked against `alloc_scan_bound` on every host and 2 ms only at HVF speed (1 s otherwise, ADR-0270 Q6).
- Measured (M1 Air, debug test kernel): worst single allocation 8192 words at 2 GiB, 94 to 104 us under HVF and 1.33 ms under TCG; boot 0.26 s (HVF) and 0.73 to 0.80 s (TCG) at 128 MiB and at 2 GiB; frame map 128 KiB at 2 GiB against 8 KiB at 128 MiB.
- **For the architect (ADR-0436, proposed):** `top --session` is not built (ADR-0267 gives `top` services' accounts); the plan's "done when" was read as `top`'s session row plus the keeper's session-ceiling refusal. Accept that, or build `top --session` now. ADR-0436 also records that ADR-0415 says there is no `make run-browser` default of 2 GiB while `BROWSER_MEM ?= 2G`.
- Known limitations: a guest above 4 GiB needs a second level of the frame map; the `memory_map`/`adopt` frame-list copy and the heap's growth (up to 256 allocations per interval) stay unbounded; the HVF time check is a single measurement (a vCPU stall can fail it); `make run-browser` is only the machine (no browser exists before plan E2).

## F5 notes

- Built: `zoraya-mappings`, a chunked sorted list (32 records per chunk, at most 2*len/32+2 chunks, merge and spill on removal), 4096 slots (`MAPPING_SLOTS`), charged to the job by whole chunks (worst case 468,528 bytes per process against 232,448 for a list filled upward); every walk over the list counted or taken in steps (INV-0270-1); `libposix` keeps no handle for a private mapping and `Objects::grow` no longer leaks a handle per MiB.
- Review: `docs/tasks/TASK-34-review-f5.md`. The error was the plan's own acceptance (`python3 -c "x=[bytearray(100) for _ in range(5_000_000)]"` at 2 GiB) not being run anywhere; `make test-python-mem` now runs it, and repeats ADR-0320's mapping measurement (free one-page mappings 4086 before and after 4000 threads and 30 forks, against 248 and 247 of 256; a recursion of 200,000 frames ran, it ended near 33,000 before).
- Also found and fixed: after unmapping 40 mappings of 24 MiB a process left about 1000 empty page tables and the next snapshot read them all in one 5.8 ms masked interval (now 127 us).
- Measured (HVF): at 4032 mappings the longest masked interval is 133 us for `memory_map`, 169 us for `snapshot_process` and 322 us for the exit of a process with 4000 mappings.
- Known limitations: `F5-snapshot-step-ignores-tables` is unkillable at this test size (a sparse image layout is unverified); the three equivalent survivors (reaper refunds the charge anyway; growth estimate exact; the unmap step point); fork at 4096 mappings is covered by `t_bound` and not by a C `fork` test; `find_gap` is linear in the records below the place.

## Curses tour flake count

`test-tour-terminal` (TCG, `posix-curses.script`, step 55) hung once in the F4 candidate run and once in the first F8 candidate run (both passed on rerun, and it passed in the F5 run). Follow-up for stage 10: wait for `vi`'s screen with a `screen` step instead of fixed pauses.

## F6 notes

- Built: `image_create` copies the program's pages once into an object of the creator's job (`Program::kept`, `zoraya-elf`); `spawn` maps each read-only segment as a `SHARED` record over those frames and copies the writable ones; `posix_spawn` and `execve` of `/proc/self/exe` use the bootstrap's `Image` handle; `zoraya_child_memory`; `t_shared` and `top.script` (a room, `top`, `tools/check-top-image.py`) in `make test-libposix`. ADR-0438 (proposed; the architect's answers are relayed in it, not recorded). INV-0415-2 and INV-0438-1 to -4 checked (Total 854).
- `top` in a room, twice around `t_shared keep` (`tests/posix-core/top.script`): `tab 4259K` (launcher alone), `9302K` (+ three children of its own image, 1681K each, no image), `11946K` (+ one child of the file: +2644K, 956K more than a child of the image, the image's pages), `1K` at the end (the empty room's reading); the second run reads the same. **`top` cannot show the program's own image:** the shell makes it, in no room (job-level proxy, accepted by the architect per the dispatcher; a per-image line is in `docs/backlog.md`).
- `t_shared` (HVF): image keeps 978944 bytes (from the program headers); L's job 2776256, +1125504 for a spawn of the file, +139264 +139264 for two of its image; children's own jobs 1582144 each.
- Found by tests: `Mappings::find_gap` let a record below the mmap window pull the search below it (every C program aborted at start); the 2048-page text case found 128 units in one interval (a step point before the first piece), and the text-then-data case another (a step point after the clean loop).
- Review (`docs/tasks/TASK-34-review-f6.md`): findings 1 to 7 and R1, R2, R4 to R6 fixed. Finding 8: the ADR-0108 note removed. R3 and the room-limit charge: **not built**; `image_create` naming the room's job is in `docs/architect-queue.md` as a condition of the untrusted-room gate, with the estimate.
- Run: `cargo test -p zoraya-elf -p zoraya-mappings -p zoraya-posix`, `make test-qemu-one` (128 passed), `make test-libposix` (164, 17 and 66 steps), `make test-posix-threads` (104) earlier on this branch.
- Mutation: host `--in-diff` zoraya-elf 32 of 34 caught (2 unviable), zoraya-mappings 10 of 10; kernel `tools/mutants/images-f6.toml` 10 of 10 non-equivalent caught (the copy, the charge, the piece, three step points in `keep_program` and one in the spawn, a writable record; the equivalent one is marked, not run); `images-f6-libposix.toml` 2 of 2 caught (no image charge: `t_shared` fails; image never freed: `check-top-image` fails, 961K then 1921K at the end).
- Limitations: `image_create` holds the file's object and the copy at once; two images of one file share nothing; programs started from the ramdisk still copy; instruction-cache maintenance across step points is correct on one CPU only (ADR-0438); no TCG run.

## F7 notes

- Built (ADR-0439, proposed): `bind`, `listen`, `accept`, `accept4` and `connect` of a path for `SOCK_STREAM` and `SOCK_SEQPACKET` Unix
  sockets. The node is a file of the `/tmp` service (`fsd --memory`) with `S_IFSOCK` kept in the service; `listen` sends the
  service the listener channel's end; `connect` opens the node with `WRITE` through the connecting program's own table and sends the far end of a
  new pair, which the service forwards unread and the listener validates. `crates/zoraya-posix/src/unix_file.rs`, the socket nodes of
  `crates/zoraya-fs-service` (messages 41 to 43), `fsd`'s transport (three handles in a write; `Outgoing::Refused`).
- **Not built, and the "done when" is not met:** TCP `bind`/`listen`/`accept` (`EOPNOTSUPP`), so `python3 -m http.server` does not serve a
  file to `curl`. Reading the code: the scope already has `ListenRule` (so the listener grant is a rule that exists, ADR-0440 item 1), but `netd` has no
  `listen` request and its stack has no loopback interface, so a connection to `127.0.0.1` leaves through the NIC and does not return.
  Both are new network code with a QEMU network step to prove it, which I judged out of reach on this machine alongside the Unix half; ADR-0440 records the design
  and the first spike (smoltcp's two-interface dispatch). No Python step was added for the same reason.
- Not built: sockets in a program's own directory (no kind in the bootstrap record, and `fsd` has no transient nodes; ADR-0439 item 1, the
  architect's question), datagram `bind`.
- Checked: INV-0412-3 (`unix::tests::bind_is_allowed_only_in_the_tmp_entry`, `tests/socketfile.rs`, `t_unix file`) and INV-0439-1 to -3
  (`docs/adr/invariants.md`; Total 859).
- Run: `cargo test -p zoraya-fs-service -p zoraya-posix -p zoraya-proto-file` (all pass), `make check` (passes), `make test-libposix`
  (exit 0: `t_unix file` through zosh, `t_spawn` and the other cases, `t_shared`, the `top` check, `zoraya-fsck` clean).
- Found by the in-QEMU test, not the host's: a socket made without `SOCK_CLOEXEC` makes `posix_spawn` fail with `EBADF` (ADR-0433 item 6), so `t_unix file`
  makes its sockets close-on-exec, as a program that spawns a second one does. Found by thinking about the kernel's rule for moved handles:
  the service forwards a far end with the type's full rights, and the kernel refuses a handle that has fewer; the first version read that
  refusal as a dead listener, so any connector could have ended a listener. `Outgoing::Refused` separates them (test and mutant).
- Mutation: `tools/mutants/sockets-f7.toml`, 28 hand mutants of the service and the library, all caught after survivors got tests; `make mutants-host --in-diff`: zoraya-posix cut at 20 minutes (114 of 147 run caught, 12 missed: equivalent disjoint-bit `|`/`^`, defence-in-depth checks, and the fork guard, since tested in `t_unix file`), zoraya-fs-service 52 of 60 caught (misses closed with tests).
- Review: `docs/tasks/TASK-34-review-f7.md` (no errors, five warnings): fixed the listener takeover (only the binder listens), a quota refusal killing a live listener (`OverQuota` is not `Closed`), and the wrong `spawn` errno in ADR-0439; the rest in the ADR and backlog.
- Limits (ADR-0439): the 256 nodes of the service are shared by the line; a node stays after its listener goes until `unlink`; the queue is the channel's 64
  messages and a blocking `connect` into a full queue is `EAGAIN`; `readdir` shows a socket file as a plain name; CPython's `test_socket`
  server classes and the asyncio cases that said "bind (plan part F7)" in `tests/python/regr-ignore.txt` were not run (no Python rootfs
  was built here): `test-python-regr` must show whether an `AF_UNIX` server now passes or hangs a case.

## F7b notes

- Built (proposed ADR-0441): TCP `bind`, `listen`, `accept`, `accept4`, `getsockname`, `SO_ACCEPTCONN` and `poll` of a listener; `netd`'s `listen`
  request (5) and `Accepted` (19); `+listen PORT` in the shell; the loopback as `netd` handing the client's stream ends to the listener
  (no second smoltcp interface: read, not measured). Plan part F7's "done when" is met: `make test-python-serve`.
- Not built: listening on the NIC, `localhost`, per-connection `ECONNRESET`, rooms' words for `+listen`. Limits in `docs/design.md` and ADR-0441.

# TASK-34 review: part F10, independent look (an image is charged to the job its creator names; proposed ADR-0443)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f10` at 6c40383c, 2026-11-17, read against
`CLAUDE.md`, the proposed ADR-0443, ADR-0438 (INV-0438-2), ADR-0261 point 9, ADR-0262, ADR-0268, ADR-0172 and the F4, F5 and F6
reviews for their bug classes. The earlier read-only review was not taken as given; every claim below was derived again from the
code. Code read around the diff: `Calls::image_create`, `Calls::job`, `Calls::object`, `spawn`, `job_create` (`kernel/src/syscall.rs`);
`process::image_from`, `keep_program`; `Paid` and `charge::arc` (`kernel/src/charge.rs`); `Job::admit`, `Job::create`, `Job::kill`
(`kernel/src/job.rs`); the shell's `image_job`, `ready_from_volume`, the line's `job_create` and launch path, `enter_room`,
`join_room_job`, `leave_room_job`, the `grants` probe; `posix_child::load`, `launch`; the broker's `load_image`, `main`, `Broker::first`,
`abort_first`; `libposix`'s `load_fd`, `resolve_program`, `spawn_child`, `exec`, `spawn_into`; `posixrun`; `gui.rs`'s panel spawn;
`t_image`.

Run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("all checks passed").
- `cargo test -p zoraya-abi`: exit 0, 82 passed.
- `make test-qemu-one`: exit 0, `auto-default` PASS 131 passed, 10 expected failures; `t_image: all passed`.
- `make test-libposix`: exit 0; `libposix-room-image-auto` PASS 43 steps, `check-room-image: the room's row read [1, 1, 1, 1] K
  around two refusals`; `check-top-image: ok`.
- The shell hand mutant `F10-the-shell-names-its-own-job` (`image_job` returns `JOB`), with
  `make test-libposix POSIX_CORE_SCRIPT=tools/mutants/empty.script POSIX_CORE_MAPS_SCRIPT=tools/mutants/empty.script`, reverted
  after: exit 2, killed. In the 1M room the program started and died, `/bin/t_unix: ended by signal 6`, and step 16 timed out
  waiting for `over quota`. Under the same mutant `check-top-image` passed (room 4259K alone, against 5246K with F10).
- `make test-qemu-auto-smp4-2g` alone, on an idle host (no other QEMU, cargo or rustc running): exit 2, `kernel_work_is_bounded`
  failed (finding 1).

Not run: `make test-changed`, `make mutants`, the kernel hand mutants. Nothing here claims they pass.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

### 1. warning: the failure in `make test-changed` is not the known contiguous-create race

*Where:* `build/logs-par/test-qemu.log:726` (the implementer's run); `kernel/src/testing.rs:2341`; `user/src/bin/t_bound.rs`.

*What:* the report and the hand-off call the one failure the known `kernel_work_is_bounded` race, `contiguous create: NoMemory`.
The log says something else. `t_bound` exited with code 3 after `t_bound: run 3 went over the time limit`. The measurements over the
2 ms masked limit were the exit of a process holding 24 MiB, the exit of 400 objects of 256 KiB and the exit of 256 mappings with 1024
handles (between 2.5 and 5.4 ms). There is no `NoMemory` anywhere in the log. I reproduced it with `make test-qemu-auto-smp4-2g` run
alone on an idle host: exit 2, `run 2` and `run 3` over the limit, the same exits plus `snapshot_process` at 2.66 ms.

*Cause:* not established. F10's kernel change is the `image_create` handler only, and `t_bound` never calls `image_create`, so I
see no causal path from this diff. I did not build `origin/main` to compare, because the disk has 17 GB free, below the 25 GB rule.
The full set on `main` passed in CI on 2026-11-16 (run 43890402197).

*Fix:* correct the report: masked-time limit, exit paths, `auto-smp4-2g` under HVF on this host, reproduced alone. Let the
candidate CI run decide. If CI's `auto-smp4-2g` fails too, it goes in the backlog next to the contiguous-create entry, not under it.

### 2. warning: INV-0443-5 and `room-image.script` say the refusal is "at the image", and the test rests on a margin it does not assert

*Where:* `tests/posix-core/room-image.script:1-6`; `tools/check-room-image.py`; INV-0443-5 (`docs/adr/invariants.md`, ADR-0443);
`docs/tasks/TASK-34-report.md` (F10 notes); `docs/design.md` ("cannot start a program whose image does not fit").

*What:* I computed `t_unix`'s kept size from its program headers by ADR-0438's rule: 155648 + 868352 + 8192 bytes, so about 1008K.
The room holds 1K of 1024K, so the image most likely fits, with its record and table, at about 1012K. The `over quota` then comes from
the child's launch: the image plus the child's own start do not fit. The shell prints `zosh: /bin/t_unix: over quota` for a refusal
at `posix_child::load` (`zosh.rs:1535`) and for one at `posix_child::launch` (`zosh.rs:5146-5152`) alike, so the log cannot tell
the two apart. The test still discriminates today. With the shell naming its own job, the child starts in the 1M room and aborts
(signal 6), and `over quota` never appears (my mutant run above). So F10's image is what turns that run into a refusal. But nothing
asserts the margin that makes this true, namely that the child's start alone fits in 1M.

*Scenario:* `t_unix` or libposix grows, so that the child's own start no longer fits in 1M (a bigger stack, a larger `.bss`). The
launch is then refused with the same `zosh: /bin/t_unix: over quota` whoever pays for the image. The script and the checker pass
with the shell charging itself again, and INV-0443-5 stays "checked".

`top.script` does not cover this either: the room reads 987K higher with F10, but `check-top-image` passed under the mutant too.

*Fix:*
- Wording: "a room whose limit holds the child's start but not the start and the image refuses it". Drop "at the image", or make it
  true with a program whose kept image alone is larger than the room.
- Add the missing positive control as an assertion. Have `t_shared` print its own job's use, as it already prints the child's
  (`zoraya_child_memory`) and the image's kept bytes. Then make `check-top-image.py` assert, with no pinned number, that the room's
  "alone" reading is at least L's own job plus the image's kept bytes. That fails under the shell mutant (4259K against about
  5246K).
- Or add a room whose limit is the roomy run's measured child use plus half the image: it starts the program with the mutant and
  refuses it with F10.

Not a merge blocker given the mutant evidence, but INV-0443-5's Status should say what is checked.

### 3. note: `image_create` charges a killed job; no revival and no leak, but inconsistent with `spawn` and `job_create`

*Where:* `kernel/src/syscall.rs:1452-1465`; ADR-0443 open questions.

*What:* this is judged here, not left open only. A killed job cannot be revived: `Job::admit` and `Job::create` refuse a `closed`
job with `BadState` (`job.rs:330`, `:187`). Nothing leaks: `Paid` holds the `Arc<Job>` it charged and gives the charge back to the
same job and its ancestors when the image's last holder drops it (`charge.rs:273`). What is new is a fresh charge to a dead job and
its ancestors for the image's whole life. ADR-0261 point 9 describes only charges made before the kill ("what is held from outside
stays charged"). The holder already had `CREATE_PROCESS`, so it gains no authority. The shell drops `room_job` on leaving and closes a
failed line's image at once, so the dead room's charge is short-lived on today's paths.

*Fix (suggested):* `BadState` for a closed payer, checked before `keep_room`, like `spawn`. A kill after the check is INV-0443-4's
case, which is already tested. Add one `t_image` line: `job_kill(j)` then `image_create(.., j)` is `BadState`.

### 4. note: "names `JOB` (slot 2), which every spawner holds" is not true of POSIX processes; the libposix mutant is probably covered

*Where:* ADR-0443 Option A and item 3; `crates/zoraya-posix/src/ffi/procs.rs:1550`; `procs.rs:1918` and `posix_child.rs:558`
(a POSIX child's table lists `PROCESS_JOB`, not slot 2).

*What:* a POSIX process holds its own job at `PROCESS_JOB` (1002), and slot 2 is empty. So `unwrap_or(JOB)` is `BadHandle`, and
then `ENOEXEC`, for a process without `PROCESS_JOB`. The report says so; the ADR's sentence does not. Because of the same fact, the
report's "not covered: libposix's `process_job`" is likely wrong. Replacing `p.process_job.unwrap_or(JOB)` with `JOB` would make
every `posix_spawn` of a path fail in `test-libposix`. The broker's `first.job` mutant (to `JOB`) is really unobservable: the
broker's job and the first child's are in the same session job, and no limit lies between them. That is acceptable.

*Fix:* correct the ADR's sentence. Add the libposix mutant to `tools/mutants/images-f10-libposix.toml` and run it.

### 5. note: two claims of INV-0443-1 and -2 are weaker in the test than in the text

*Where:* `user/src/bin/t_image.rs`, `an_image_is_charged_to_the_job_it_names`.

*What:*
- "The number of processes started from the image changes nothing" is read only after `run` has waited for the child's exit. A
  live child is never measured. INV-0438-2's own tests cover a live child, but not against a named job.
- INV-0443-2's `BadHandle` (a non-job handle that carries `CREATE_PROCESS`) is never reached. The non-job case passes a memory
  handle, which gets `AccessDenied` from the rights check first. If no other object type can carry `CREATE_PROCESS`, the branch is
  unreachable and the ADR should say so.

*Fix:*
- Spawn without waiting, read `used(named)`, then kill and wait.
- Name the reachable outcome for a non-job handle in INV-0443-2.

## The gate's honest remainder (every path by which a program starts in a room)

- The shell, a volume program (native or POSIX) in a room with a launch job: the room pays for the image (F10). Each line's job is
  made under `room_job` (`zosh.rs:5029`), and the image is closed after the spawn.
- The shell in a trusted room with no limits (no launch job, `room_job` is `None`): the image is charged to the shell's own job.
  There is no limit to bound. An untrusted room is refused without the keeper (`join_room_job`), and still refused outright
  (`untrusted rooms wait for quotas`).
- A shell the panel launched in a room: its `JOB` is inside the room, so `image_job()` names the room.
- The shell's staging object (`posix_child::load`'s `memory_create`): the shell's job. It is transient and at most `LOAD_LIMIT`.
  Recorded in ADR-0443 item 4 and design.md.
- The `grants` probe (`zosh.rs:4458`): the shell's job, closed in the same expression, and never spawned. Only the shell's user
  triggers it; no program in a room can, so it is not a hole.
- `libposix` `posix_spawn` and `execve` of a path: the caller's own process job, which is inside the room.
  `posix_spawn`/`exec` of `/proc/self/exe`: a duplicate of the bootstrap image, no new charge; the original payer (the room, through
  the shell) keeps paying.
- Ramdisk programs (`directory_lookup`, the panel's launches): no image charge; the child's job pays for the copied pages.
- The broker's session-first process: the first child's job. `posixrun`: its own job (the test harness, outside rooms).
- Not built: ADR-0269's untrusted launcher. It must name the room's job and hold no job above it (ADR-0443 follow-up).

## Checked and found sound

- The kernel handler. The memory `READ` check, then `self.job(job, CREATE_PROCESS)` (`handles.check` gives `AccessDenied` for a
  handle with fewer rights, a duplicate with `WAIT` included), then `keep_room`, then `image_from`, in that order. Nothing is
  allocated or charged before the job check. Every refusal after `keep_room` calls `drop_kept`. A charge refused in
  `MemoryObject::new`, a failing `keep_program` or a refused `charge::arc` drops what was made, and `Paid` gives the charge back.
  A failed `insert_kept` drops the image. The job's `Arc` is cloned under the table lock, so a sibling closing the handle changes
  nothing.
- No double charge and no underflow: one `Paid` (frames) for the kept object and one (record and table) for the `MemoryImage`, both
  on the named job, both refunded to it. The walk adds ancestors once (the test's `own_delta < cost + cost/2`).
- Authority and denial of service: the jobs a program can charge are the jobs it can spawn into. That is the shell's `room_job`
  (from the keeper, closed on leaving or on a change of room), the keeper, the broker's process jobs (`TABLE`), and a process's own
  job (`OWN`). The parent's handle to a child's job (`PARENT`) lacks `CREATE_PROCESS`. A holder that fills a job's quota with images
  could fill it with processes anyway, which is consistent with ADR-0262 and ADR-0003.
- The broker's reorder: `core.first()` comes before the load, and `abort_first` runs on a failed load. The broker returns 1, and
  the dropped `actions` and handles go with its exit. There is no child spawn before the image exists, so there is no race.
- The ABI. Syscall 46 keeps its number. The third argument is a plain handle with no sentinel, and every caller was changed (a
  two-argument call no longer compiles). `zoraya-abi`'s signature test lists it. Old binaries on a volume would pass an arbitrary
  x2 and fail closed (`BadHandle` or `AccessDenied`), never charge a wrong job.
- `t_image`. Its limit is set to exactly record + cost, the first image fits and the second is refused with `refused + 1`. Frames
  and heap are unchanged after each refusal. More refusals are made than the table has slots (the table-room mutant). The image
  outlives a killed payer and a closed payer handle, with pages and charge returned.
- Docs. Invariants: 876 rows, and the per-ADR sum is 876 (`make adr-check` passes). ADR-0443 is `proposed` with an empty
  "Architect's decision", and no other ADR's status or decision changed. The "Amends (if accepted)" line names ADR-0438 item 2,
  INV-0438-2 and ADR-0108 item 1. The architect-queue line claims only this one gate condition. No new `unsafe`. `make check`
  covers host paths and test caps.
- Disk: 17 GB free on the host, below `CLAUDE.md`'s 25 GB threshold. Clean merged worktrees before the next build.

## Verdict

No errors. Merge after warnings 1 and 2 are fixed or recorded (the report's description of the failure; INV-0443-5's wording and the
missing control) and after the candidate CI run is green, which also settles `auto-smp4-2g`.

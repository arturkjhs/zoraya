# ADR-0443: An image's pages are charged to the job its creator names (plan part F10): `image_create` takes the job, the shell names the room's

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0003, ADR-0108, ADR-0172, ADR-0261, ADR-0262, ADR-0268, ADR-0438
- **Amends (if accepted):** ADR-0438 item 2 and INV-0438-2 ("the job that made an image pays" becomes "the job the creator names pays"); ADR-0108 item 1's signature `image_create(memory, length)`.
- **Evidence:** CP-01, GR-008

## Context

ADR-0438 (plan part F6) made an image own its program's pages and said who pays: the job of the process that called
`image_create`, once, for as long as the image or any process of it lives (`Paid`). That rule leaves a hole for rooms (ADR-0172,
ADR-0266). The shell makes the image of every program it starts from the volume, in the shell's own process, so the image is charged
to the shell's job, outside the room the program runs in. A room at its memory limit can start any number of new programs, each a
large image charged to the shell, and `top` shows none of it in the room's row (the F6 report's gate condition; the architect's
decision of 2026-11-16 made it part F10). ADR-0262's rule is that what is made for a client is paid by the client. ADR-0268's
gate needs it: a room's limit must bound what the room can make the machine hold.

## Decision drivers

1. **The client pays** (ADR-0262): the image of a program started in a room is the room's.
2. **No new authority**: a program must not be able to charge a job it could not put a process into (no charge into someone else's
   quota; ADR-0003 item 4, INV-0003-4).
3. **Nothing leaks on a refusal**: a refused call holds no frame, no handle and no charge (INV-0108-1, INV-0438-2).
4. **The least commitment**: one argument to one accepted call; no new call, no new right, no sentinel value.

## Considered options

### Option A: a third argument, a job handle that must hold `CREATE_PROCESS` (built)

`image_create(memory, length, job)`. The kernel checks `CREATE_PROCESS` on `job` (the right a spawner holds on the job it
starts a process in), then makes the image with that job as the payer (`process::image_from` already took the payer). A caller that
means its own job names the handle it spawns into: `JOB` (slot 2) for a native program, `PROCESS_JOB` (slot 1002) for a
POSIX process, which holds nothing at slot 2.
- **Evidence for:** drivers 1 to 4. The check is the one `spawn` and `job_create` make, so the set of jobs a program can charge is the
  set it can spawn into.
- **Evidence against:** every caller changes (14 in `t_image`, the shell, the broker, `posixrun`, `libposix`).
- **Cost:** *(estimate)* small: a signature, one check, the callers.
- **Precedent:** `spawn`'s job handle; Zircon's launcher takes the job a process is made in.

### Option B: an optional job handle, 0 meaning the caller's own job

A sentinel in a slot that can be valid (slot 0 is never valid, ADR-0040, but a default that silently charges the creator is the bug
this part removes). Not built: it keeps the unsafe default available by omission.

### Option C: a new right (`CHARGE`) on the job

A second right with the same holders as `CREATE_PROCESS` in every case that exists. A new right is a larger commitment than the
check it would replace. Not built.

### Option D: charge the room in the shell by a second call (`job_charge`)

A call that moves a charge between jobs: a new call with the transfer's own failure modes (a half-moved charge). Not built.

## Recommendation

**Recommendation (not a decision): Option A, as built.**

1. **The call.** `image_create(memory, length, job: Handle)`. After the memory handle's check (`READ`), the kernel looks up `job` with
   `CREATE_PROCESS` (a handle without `CREATE_PROCESS`, whatever its kind, is `AccessDenied`; only a job carries that right, and a handle cannot gain a right, so the `BadHandle` arm for a non-job handle is defensive and not reachable), and refuses a job that was killed with `BadState`, as `spawn` and `job_create` do, all, before the table room is kept and before
   anything is made. The image's kept pages and record are charged to `job` (the payer of ADR-0438 item 2 is now `job`, not the
   caller's own job); a job at its memory limit refuses `OverQuota` and the call holds nothing.
2. **The lifetime.** As ADR-0438: `Paid` keeps the payer job alive for as long as the image or any process of it lives. A job that
   ends first (`job_kill`, or its last handle closed) keeps what the image holds charged to it as `held` (ADR-0261 point 9) until the
   last holder goes, and then the pages and the charge go back. The creator ending first changes nothing: the image is not the
   creator's.
3. **Callers.** The shell names the room's launch job when it holds one (`room_job`), else its own job (a trusted room without limits has no launch job), so a program started outside
   a room is charged as before. The broker names the job of the child it loads for. `posixrun` names its own job (`JOB`) and `libposix`'s program loader the process's own (`PROCESS_JOB`): a job that charges itself keeps its behaviour. Every other caller names `JOB`.
4. **Not built.** The staging memory object the shell fills before the image exists (`memory_create`, the program's bytes, closed
   after `image_create`) is the shell's own and is charged to the shell's job while it lives; the room's row shows the image only.

### Effect of the vision

A room can no longer make the machine hold images it does not pay for: its memory limit bounds what it can start. A program cannot
charge a job it could not spawn into, so the call adds no way to spend another room's quota (CP-01, GR-008).

## Consequences

- **Positive.** The F6 gate condition is met on the shell's path: a room at its limit is refused when it starts a program, with
  the room's `OverQuota` and the room's row unchanged. The ABI change removes the unsafe default.
- **Negative.** A kernel ABI change to an accepted call; every caller of the call changes. A program that holds a job handle
  without `CREATE_PROCESS` can no longer make images (none exists).
- **Follow-up.** The untrusted-room launcher (ADR-0269, not built) must make its images with the room's job and must not hold
  any job handle above the room's.

## Invariants

- **INV-0443-1.** The pages and the record of an image are charged to the job `image_create` names, once, and to no other job
  that is not its ancestor: a sibling's use and a live process of the image, started in a job of its own, change nothing. *Check:* `t_image`'s
  `an_image_is_charged_to_the_job_it_names` (`make test-qemu-one`) and `cargo test -p zoraya-abi` (the signature).
- **INV-0443-2.** `image_create` of a handle without `CREATE_PROCESS` (a weakened job handle, or any other kind of handle: only a job
  carries the right) is refused `AccessDenied`, of a job that was killed `BadState`, and each leaves no charge, no frame and no heap. *Check:* `t_image`'s
  `an_image_is_charged_to_the_job_it_names` (`make test-qemu-one`).
- **INV-0443-3.** A job at its memory limit refuses `image_create` with `OverQuota`, holds nothing, and a job with room takes the same
  file. *Check:* `t_image`'s `an_image_is_charged_to_the_job_it_names` (`make test-qemu-one`).
- **INV-0443-4.** An image outlives the end of the job that pays for it and the closing of that job's handle, and its last holder
  gives back the pages and the charge. *Check:* `t_image`'s `an_image_is_charged_to_the_job_it_names` (`make test-qemu-one`).
- **INV-0443-5.** The image of a program the shell starts in a room with a launch job is charged to that room's job: a room whose
  limit holds the program's start but not the start and its image refuses it (the shell says `over quota` and the room's row
  counts the refusal and does not change), while the same program starts in a room with room; and a room sized between the program's
  start and its start plus its image (from `top.script`'s own reading) does not run it, which a shell that charged its own job would. *Check:*
  `make test-libposix` (`tests/posix-core/room-image.script` read by `tools/check-room-image.py`, and `tests/posix-core/room-mid.script`
  with the limit from `tools/room-mid-limit.py`).

## Open questions

### Known limitations: what the gate still lacks (the honest remainder)

Programs started in a room whose image is **not** charged to the room:

- The shell in a trusted room without limits has no launch job (`room_job` is `None`), so `image_job` is the shell's own job; there is
  no limit to bound. An untrusted room is refused without the keeper.
- The shell's staging object (the program's bytes before the image exists) and the `grants` probe's image (made and closed in one
  expression) are short-lived and charged to the shell's own job; only the shell's user starts either.
- Ramdisk programs (`directory_lookup`; the panel's launches) have no image charge: the child's job pays for the copied pages.
- The broker's first child and `posixrun` name their own jobs, outside rooms.
- The untrusted-room launcher of ADR-0269 is not built; it must name the room's job and hold no job above it.

A refusal in the test of INV-0443-5 comes at the child's launch as well as at the image: the shell prints `over quota` for both, so the
script alone cannot tell them apart; the room sized by `tools/room-mid-limit.py` is what shows the image is the room's.

### Questions

- Whether the staging object of the shell's loader (item 4) should be charged to the room too: it is transient and bounded by the
  program's size, and it is the shell's own memory.

## Revisit triggers

- The untrusted-room launcher is built (ADR-0269): the job it names for an image.
- A program that must make an image for a job it holds without `CREATE_PROCESS` (a loader service): a narrower right.

## Architect's decision

(Left empty for the architect.)
